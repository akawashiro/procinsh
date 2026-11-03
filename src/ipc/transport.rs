//! Bounded native-endian binary framing over private Unix streams.
//!
//! # Interface
//! - [`configure`] (`pub`): `fn configure(socket: &UnixStream) -> anyhow::Result<()>`.
//! - [`fn@write`] (`pub`): `fn write<T: Wire>(socket: &mut UnixStream, value: &T, limit: usize) -> anyhow::Result<()>`.
//! - [`read`] (`pub`): `fn read<T: Wire>(socket: &mut UnixStream, limit: usize) -> anyhow::Result<T>`.
//! - [`read_timeout`] (`pub`): `fn read_timeout<T: Wire>(socket: &mut UnixStream, limit: usize, timeout: Duration) -> anyhow::Result<T>`.
//!
//! Supporting types: [`UnixStream`], [`Wire`], [`Duration`].
use super::codec::{self, Wire};
use super::schema::TIMEOUT;
use anyhow::{Context, Result, ensure};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

pub fn configure(socket: &UnixStream) -> Result<()> {
    socket.set_read_timeout(Some(TIMEOUT))?;
    socket.set_write_timeout(Some(TIMEOUT))?;
    Ok(())
}

pub fn write<T: Wire>(socket: &mut UnixStream, value: &T, limit: usize) -> Result<()> {
    let bytes = codec::encode(value, limit)?;
    let deadline = Instant::now() + TIMEOUT;
    write_until(socket, &(bytes.len() as u32).to_ne_bytes(), deadline)?;
    write_until(socket, &bytes, deadline)?;
    Ok(())
}

pub fn read<T: Wire>(socket: &mut UnixStream, limit: usize) -> Result<T> {
    read_timeout(socket, limit, TIMEOUT)
}

pub fn read_timeout<T: Wire>(
    socket: &mut UnixStream,
    limit: usize,
    timeout: Duration,
) -> Result<T> {
    let deadline = Instant::now() + timeout;
    let mut header = [0; 4];
    read_until(socket, &mut header, deadline).context("monitor IPC disconnected or timed out")?;
    let size = u32::from_ne_bytes(header) as usize;
    ensure!(
        size > 0 && size <= limit,
        "invalid monitor frame length: {size}"
    );
    let mut bytes = vec![0; size];
    read_until(socket, &mut bytes, deadline).context("truncated monitor IPC frame")?;
    codec::decode(&bytes).context("invalid monitor IPC message")
}

fn remaining(deadline: Instant) -> std::io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "monitor frame timed out"))
}
fn read_until(
    socket: &mut UnixStream,
    mut bytes: &mut [u8],
    deadline: Instant,
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        socket.set_read_timeout(Some(remaining(deadline)?))?;
        match socket.read(bytes) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "monitor disconnected",
                ));
            }
            Ok(n) => bytes = &mut bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn write_until(
    socket: &mut UnixStream,
    mut bytes: &[u8],
    deadline: Instant,
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        socket.set_write_timeout(Some(remaining(deadline)?))?;
        match socket.write(bytes) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "monitor disconnected",
                ));
            }
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[allow(clippy::wildcard_imports)]
    use super::*;
    use crate::{
        ipc::elf::{FileChunk, MappedFileData},
        ipc::schema::{MAX_RESPONSE, MonitorResponse, WireResponse},
    };
    #[test]
    fn typed_elf_reply_transfers_large_files() {
        // This raw file exercises an IPC frame larger than 32 MiB.
        let length = 33 * 1024 * 1024;
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        let writer = std::thread::spawn(move || {
            let reply: WireResponse = Ok(MonitorResponse::Elf(Some(MappedFileData {
                identity: (1, 2, length as u64, 3, 4),
                chunks: vec![FileChunk {
                    offset: 0,
                    bytes: (0..length).map(|index| index as u8).collect(),
                }],
            })));
            write(&mut sender, &reply, MAX_RESPONSE).unwrap();
        });
        let reply: WireResponse = read(&mut receiver, MAX_RESPONSE).unwrap();
        let MonitorResponse::Elf(Some(file)) = reply.unwrap() else {
            panic!("expected typed ELF reply")
        };
        assert_eq!(file.identity, (1, 2, length as u64, 3, 4));
        assert_eq!(file.chunks[0].bytes.len(), length);
        assert!(
            file.chunks[0]
                .bytes
                .iter()
                .enumerate()
                .all(|(index, byte)| *byte == index as u8)
        );
        writer.join().unwrap();
    }
    #[test]
    fn oversize_frame_is_rejected_before_reading_the_body() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        sender
            .write_all(&((MAX_RESPONSE + 1) as u32).to_ne_bytes())
            .unwrap();
        assert!(read::<WireResponse>(&mut receiver, MAX_RESPONSE).is_err());
    }
}
