use super::protocol::TIMEOUT;
use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

pub(super) fn configure(socket: &UnixStream) -> Result<()> {
    socket.set_read_timeout(Some(TIMEOUT))?;
    socket.set_write_timeout(Some(TIMEOUT))?;
    Ok(())
}

/// Limit serialized allocation as well as the transmitted frame size.
struct BoundedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("monitor response exceeds size limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn write<T: Serialize>(socket: &mut UnixStream, value: &T, limit: usize) -> Result<()> {
    let mut buffer = BoundedBuffer {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut buffer, value)?;
    let deadline = Instant::now() + TIMEOUT;
    write_until(socket, &(buffer.bytes.len() as u32).to_be_bytes(), deadline)?;
    write_until(socket, &buffer.bytes, deadline)?;
    Ok(())
}

pub(super) fn read<T: DeserializeOwned>(socket: &mut UnixStream, limit: usize) -> Result<T> {
    read_timeout(socket, limit, TIMEOUT)
}

pub(super) fn read_timeout<T: DeserializeOwned>(
    socket: &mut UnixStream,
    limit: usize,
    timeout: Duration,
) -> Result<T> {
    let deadline = Instant::now() + timeout;
    let mut header = [0; 4];
    read_until(socket, &mut header, deadline).context("monitor IPC disconnected or timed out")?;
    let size = u32::from_be_bytes(header) as usize;
    ensure!(
        size > 0 && size <= limit,
        "invalid monitor frame length: {size}"
    );
    let mut bytes = vec![0; size];
    read_until(socket, &mut bytes, deadline).context("truncated monitor IPC frame")?;
    serde_json::from_slice(&bytes).context("invalid monitor IPC message")
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
