//! Bounded netlink dump requests and attribute parsing shared by socket and RDMA
//! diagnostics. Requests only read kernel tables; no endpoint is opened or modified.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`NetlinkDumpRequest`] | `pub(super)` | `struct NetlinkDumpRequest` |
//! | [`open_socket`] | `pub(super)` | `fn open_socket(protocol: `[`i32`]`, name: &`[`str`]`) -> `[`Result`]`<`[`OwnedFd`]`>` |
//! | [`attributes`] | `pub(super)` | `fn attributes(bytes: &[`[`u8`]`]) -> `[`Result`]`<`[`Vec`]`<(`[`u16`]`, &[`[`u8`]`])>>` |
//! | [`u32_at`] | `pub(super)` | `fn u32_at(bytes: &[`[`u8`]`], offset: `[`usize`]`) -> `[`Result`]`<`[`u32`]`>` |

use anyhow::{Context, Result, bail, ensure};
use std::{
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    time::Instant,
};

const HEADER: usize = 16;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLM_F_DUMP_INTR: u16 = 0x10;
// NLM_F_REQUEST | NLM_F_DUMP
const DUMP_FLAGS: u16 = 0x301;
const REPLY_LIMIT: usize = 8 * 1024 * 1024;

pub(super) fn open_socket(protocol: i32, name: &str) -> Result<OwnedFd> {
    let raw = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            protocol,
        )
    };
    ensure!(raw >= 0, "{name}: {}", std::io::Error::last_os_error());
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

pub(super) fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_ne_bytes(
        bytes
            .get(offset..offset + 4)
            .context("short netlink field")?
            .try_into()?,
    ))
}

/// Splits a 4-byte aligned attribute stream into `(type, payload)` pairs.
/// The nested and byte-order flags are removed from the type.
pub(super) fn attributes(bytes: &[u8]) -> Result<Vec<(u16, &[u8])>> {
    let mut result = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        ensure!(offset + 4 <= bytes.len(), "short netlink attribute");
        let length = u16::from_ne_bytes(bytes[offset..offset + 2].try_into()?) as usize;
        let kind = u16::from_ne_bytes(bytes[offset + 2..offset + 4].try_into()?) & 0x3fff;
        ensure!(
            length >= 4 && offset + length <= bytes.len(),
            "invalid netlink attribute length"
        );
        result.push((kind, &bytes[offset + 4..offset + length]));
        offset += (length + 3) & !3;
    }
    Ok(result)
}

/// One dump request. `label` prefixes errors, such as `"UNIX diag timed out"`.
pub(super) struct NetlinkDumpRequest<'a> {
    pub(super) label: &'a str,
    pub(super) message_type: u16,
    /// The only data message type accepted in the reply.
    pub(super) reply_type: u16,
    pub(super) sequence: u32,
    /// Family header and attributes following the netlink header, 4-byte aligned.
    pub(super) payload: &'a [u8],
}

impl NetlinkDumpRequest<'_> {
    /// Sends the request and passes each reply body to `visit` until `NLMSG_DONE`.
    /// Interrupted dumps, replies from other senders or sequences, error messages and
    /// replies above 8 MiB fail. The deadline bounds the whole exchange.
    pub(super) fn run(
        &self,
        fd: &OwnedFd,
        deadline: Instant,
        mut visit: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        let label = self.label;
        ensure!(Instant::now() < deadline, "{label} timed out");
        let mut kernel: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        kernel.nl_family = libc::AF_NETLINK as u16;
        let mut request = vec![0u8; HEADER];
        request[0..4].copy_from_slice(&((HEADER + self.payload.len()) as u32).to_ne_bytes());
        request[4..6].copy_from_slice(&self.message_type.to_ne_bytes());
        request[6..8].copy_from_slice(&DUMP_FLAGS.to_ne_bytes());
        request[8..12].copy_from_slice(&self.sequence.to_ne_bytes());
        request.extend_from_slice(self.payload);
        let sent = unsafe {
            libc::sendto(
                fd.as_raw_fd(),
                request.as_ptr().cast(),
                request.len(),
                0,
                (&kernel as *const libc::sockaddr_nl).cast(),
                std::mem::size_of_val(&kernel) as _,
            )
        };
        ensure!(
            sent == request.len() as isize,
            "{label} send: {}",
            std::io::Error::last_os_error()
        );
        let mut buffer = vec![0u8; 65536];
        let mut total = 0;
        loop {
            ensure!(Instant::now() < deadline, "{label} timed out");
            let mut poll = libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            let timeout = remaining.as_millis().clamp(1, 50) as i32;
            let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
            if ready < 0 {
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                bail!(std::io::Error::last_os_error());
            }
            if ready == 0 {
                continue;
            }
            let mut sender: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
            let mut sender_len = std::mem::size_of_val(&sender) as libc::socklen_t;
            let count = unsafe {
                libc::recvfrom(
                    fd.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    libc::MSG_TRUNC | libc::MSG_DONTWAIT,
                    (&mut sender as *mut libc::sockaddr_nl).cast(),
                    &mut sender_len,
                )
            };
            if count < 0 {
                let error = std::io::Error::last_os_error();
                if matches!(error.raw_os_error(), Some(libc::EINTR) | Some(libc::EAGAIN)) {
                    continue;
                }
                return Err(error.into());
            }
            ensure!(
                count > 0 && count as usize <= buffer.len(),
                "truncated {label} packet"
            );
            ensure!(sender.nl_pid == 0, "{label} sender is not kernel");
            total += count as usize;
            ensure!(total <= REPLY_LIMIT, "{label} exceeds 8 MiB limit");
            let bytes = &buffer[..count as usize];
            let mut offset = 0;
            while offset < bytes.len() {
                ensure!(offset + HEADER <= bytes.len(), "short netlink header");
                let length = u32_at(bytes, offset)? as usize;
                ensure!(
                    length >= HEADER && length <= bytes.len() - offset,
                    "invalid netlink length"
                );
                let kind = u16::from_ne_bytes(bytes[offset + 4..offset + 6].try_into()?);
                let flags = u16::from_ne_bytes(bytes[offset + 6..offset + 8].try_into()?);
                ensure!(
                    u32_at(bytes, offset + 8)? == self.sequence && flags & NLM_F_DUMP_INTR == 0,
                    "{label} dump interrupted"
                );
                let body = &bytes[offset + HEADER..offset + length];
                match kind {
                    NLMSG_DONE => {
                        if body.len() >= 4 {
                            ensure!(u32_at(body, 0)? == 0, "{label} dump failed");
                        }
                        return Ok(());
                    }
                    NLMSG_ERROR => {
                        let code = u32_at(body, 0)? as i32;
                        if code != 0 {
                            bail!(
                                "{label}: {}",
                                std::io::Error::from_raw_os_error(code.saturating_neg())
                            );
                        }
                    }
                    kind if kind == self.reply_type => visit(body)?,
                    _ => bail!("unexpected netlink message {kind}"),
                }
                offset += (length + 3) & !3;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attribute(kind: u16, data: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&((4 + data.len()) as u16).to_ne_bytes());
        bytes.extend_from_slice(&kind.to_ne_bytes());
        bytes.extend_from_slice(data);
        bytes.resize((bytes.len() + 3) & !3, 0);
        bytes
    }

    #[test]
    fn attributes_strip_flags_skip_padding_and_reject_bad_lengths() {
        let mut bytes = attribute(0x8000 | 19, &attribute(20, &[1, 2, 3]));
        bytes.extend(attribute(2, b"mlx\0"));
        let parsed = attributes(&bytes).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, 19);
        assert_eq!(attributes(parsed[0].1).unwrap()[0], (20, &[1u8, 2, 3][..]));
        assert_eq!(parsed[1], (2, &b"mlx\0"[..]));
        bytes[0] = 3;
        assert!(attributes(&bytes).is_err());
        assert!(attributes(&[8, 0, 1]).is_err());
        assert!(attributes(&[]).unwrap().is_empty());
    }
}
