use crate::http_server::socket_types::{SocketProtocol, SocketState, SocketType};
// Visibility is scoped to the consumers of the parent process façade.
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    time::Instant,
};

#[derive(Clone, Debug)]
pub(in crate::http_server) struct SocketInfo {
    pub(in crate::http_server) protocol: SocketProtocol,
    pub(in crate::http_server) state: SocketState,
    pub(in crate::http_server) local: Option<SocketAddr>,
    pub(in crate::http_server) remote: Option<SocketAddr>,
    pub(in crate::http_server) path: Option<String>,
    pub(in crate::http_server) peer_inode: Option<u64>,
}

pub(super) fn read_text(path: &str) -> Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 4 * 1024 * 1024,
        "{path}: 4 MiB limit exceeded"
    );
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn address(text: &str) -> Result<SocketAddr> {
    let (ip, port) = text.split_once(':').context("missing port")?;
    ensure!(
        ip.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid IP encoding"
    );
    let ip = match ip.len() {
        8 => IpAddr::V4(Ipv4Addr::from(u32::from_str_radix(ip, 16)?.to_le_bytes())),
        32 => {
            let mut bytes = [0u8; 16];
            for (index, out) in bytes.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                out.copy_from_slice(
                    &u32::from_str_radix(&ip[index * 8..index * 8 + 8], 16)?.to_le_bytes(),
                );
            }
            let ip = Ipv6Addr::from(bytes);
            ip.to_ipv4_mapped()
                .map(IpAddr::V4)
                .unwrap_or(IpAddr::V6(ip))
        }
        _ => bail!("invalid IP address"),
    };
    Ok(SocketAddr::new(ip, u16::from_str_radix(port, 16)?))
}

pub(super) fn parse_inet(text: &str, protocol: SocketProtocol) -> HashMap<u64, SocketInfo> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<_> = line.split_whitespace().collect();
            let inode: u64 = f.get(9)?.parse().ok()?;
            if inode == 0 {
                return None;
            }
            Some((
                inode,
                SocketInfo {
                    protocol,
                    state: SocketState::inet(u32::from_str_radix(f.get(3)?, 16).ok()?),
                    local: Some(address(f.get(1)?).ok()?),
                    remote: Some(address(f.get(2)?).ok()?),
                    path: None,
                    peer_inode: None,
                },
            ))
        })
        .collect()
}

pub(super) fn parse_unix(text: &str) -> HashMap<u64, SocketInfo> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut rest = line.trim();
            let mut f = Vec::new();
            for _ in 0..7 {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                if end == 0 {
                    return None;
                }
                f.push(&rest[..end]);
                rest = rest[end..].trim_start();
            }
            let protocol = SocketProtocol::Unix {
                socket_type: SocketType::from_code(u32::from_str_radix(f[4], 16).ok()?),
            };
            let state = SocketState::unix_proc(
                u32::from_str_radix(f[5], 16).ok()?,
                u32::from_str_radix(f[3], 16).ok()? & 0x10000 != 0,
            );
            Some((
                f[6].parse().ok()?,
                SocketInfo {
                    protocol,
                    state,
                    local: None,
                    remote: None,
                    path: (!rest.is_empty()).then(|| rest.to_owned()),
                    peer_inode: None,
                },
            ))
        })
        .collect()
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_ne_bytes(
        bytes
            .get(offset..offset + 4)
            .context("short netlink field")?
            .try_into()?,
    ))
}

fn parse_diag(bytes: &[u8]) -> Result<(u64, SocketInfo)> {
    ensure!(
        bytes.len() >= 16 && bytes[0] == libc::AF_UNIX as u8,
        "invalid UNIX diag message"
    );
    let mut info = SocketInfo {
        protocol: SocketProtocol::Unix {
            socket_type: SocketType::from_code(bytes[1] as u32),
        },
        state: SocketState::unix_diag(bytes[2] as u32),
        local: None,
        remote: None,
        path: None,
        peer_inode: None,
    };
    let mut offset = 16;
    while offset < bytes.len() {
        ensure!(offset + 4 <= bytes.len(), "short UNIX diag attribute");
        let length = u16::from_ne_bytes(bytes[offset..offset + 2].try_into()?) as usize;
        let kind = u16::from_ne_bytes(bytes[offset + 2..offset + 4].try_into()?) & 0x3fff;
        ensure!(
            length >= 4 && offset + length <= bytes.len(),
            "invalid UNIX diag attribute length"
        );
        let data = &bytes[offset + 4..offset + length];
        if kind == 2 {
            let inode = u32_at(data, 0)? as u64;
            info.peer_inode = (inode != 0).then_some(inode);
        }
        if kind == 0 && !data.is_empty() {
            info.path = Some(if data[0] == 0 {
                format!(
                    "@{}",
                    String::from_utf8_lossy(&data[1..]).replace('\0', "\\0")
                )
            } else {
                String::from_utf8_lossy(data.strip_suffix(&[0]).unwrap_or(data)).into_owned()
            });
        }
        offset += (length + 3) & !3;
    }
    Ok((u32_at(bytes, 4)? as u64, info))
}

// Query only metadata. No endpoint is opened, consumed, or modified.
pub(super) fn unix_diag(deadline: Instant) -> Result<HashMap<u64, SocketInfo>> {
    let raw = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            libc::NETLINK_SOCK_DIAG,
        )
    };
    ensure!(
        raw >= 0,
        "NETLINK_SOCK_DIAG: {}",
        std::io::Error::last_os_error()
    );
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut kernel: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    kernel.nl_family = libc::AF_NETLINK as u16;
    let mut request = [0u8; 40];
    request[0..4].copy_from_slice(&40u32.to_ne_bytes());
    request[4..6].copy_from_slice(&20u16.to_ne_bytes()); // SOCK_DIAG_BY_FAMILY
    request[6..8].copy_from_slice(&0x301u16.to_ne_bytes()); // REQUEST | DUMP
    request[8..12].copy_from_slice(&1u32.to_ne_bytes());
    request[16] = libc::AF_UNIX as u8;
    request[20..24].copy_from_slice(&u32::MAX.to_ne_bytes());
    request[28..32].copy_from_slice(&5u32.to_ne_bytes()); // SHOW_NAME | SHOW_PEER
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
        "UNIX diag send: {}",
        std::io::Error::last_os_error()
    );
    let mut result = HashMap::new();
    let mut buffer = vec![0u8; 65536];
    let mut total = 0;
    loop {
        ensure!(Instant::now() < deadline, "UNIX diag timed out");
        let mut poll = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, 50) };
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
            "truncated UNIX diag packet"
        );
        ensure!(sender.nl_pid == 0, "UNIX diag sender is not kernel");
        total += count as usize;
        ensure!(total <= 8 * 1024 * 1024, "UNIX diag exceeds 8 MiB limit");
        let bytes = &buffer[..count as usize];
        let mut offset = 0;
        while offset < bytes.len() {
            ensure!(offset + 16 <= bytes.len(), "short netlink header");
            let length = u32_at(bytes, offset)? as usize;
            ensure!(
                length >= 16 && length <= bytes.len() - offset,
                "invalid netlink length"
            );
            let kind = u16::from_ne_bytes(bytes[offset + 4..offset + 6].try_into()?);
            let flags = u16::from_ne_bytes(bytes[offset + 6..offset + 8].try_into()?);
            ensure!(
                u32_at(bytes, offset + 8)? == 1 && flags & 0x10 == 0,
                "UNIX diag dump interrupted"
            );
            let body = &bytes[offset + 16..offset + length];
            match kind {
                3 => {
                    if body.len() >= 4 {
                        ensure!(u32_at(body, 0)? == 0, "UNIX diag dump failed");
                    }
                    return Ok(result);
                }
                2 => {
                    let code = u32_at(body, 0)? as i32;
                    if code != 0 {
                        bail!(
                            "UNIX diag: {}",
                            std::io::Error::from_raw_os_error(code.saturating_neg())
                        );
                    }
                }
                20 => {
                    let (inode, info) = parse_diag(body)?;
                    result.insert(inode, info);
                }
                _ => bail!("unexpected netlink message {kind}"),
            }
            offset += (length + 3) & !3;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv4_ipv6_and_mapped_addresses() {
        assert_eq!(
            address("0100007F:1F90").unwrap().to_string(),
            "127.0.0.1:8080"
        );
        assert_eq!(
            address("00000000000000000000000001000000:0050")
                .unwrap()
                .to_string(),
            "[::1]:80"
        );
        assert_eq!(
            address("0000000000000000FFFF00000100007F:0050")
                .unwrap()
                .to_string(),
            "127.0.0.1:80"
        );
        assert!(address("bad").is_err());
    }

    #[test]
    fn proc_tables_and_diag_preserve_unknown_socket_codes() {
        use crate::http_server::socket_types::AddressFamily;
        let protocol = SocketProtocol::Tcp {
            family: AddressFamily::Ipv4,
        };
        let table = parse_inet(
            "header\n0: 0100007F:1F90 0200007F:0050 FE 0:0 0:0 0 0 0 18446744073709551615\n",
            protocol,
        );
        let info = &table[&u64::MAX];
        assert_eq!(info.protocol, protocol);
        assert_eq!(info.state, SocketState::UnknownInet(254));
        assert_eq!(info.local.unwrap().port(), 8080);
        let unix = parse_unix(
            "header\n000: 2 0 00010000 0001 01 42 /tmp/name with spaces\n000: 2 0 0 00FF FF 43\n",
        );
        assert_eq!(unix[&42].state, SocketState::Listen);
        assert_eq!(unix[&42].path.as_deref(), Some("/tmp/name with spaces"));
        assert_eq!(
            unix[&43].protocol,
            SocketProtocol::Unix {
                socket_type: SocketType::Unknown(255)
            }
        );
        assert_eq!(unix[&43].state, SocketState::UnknownUnix(255));
        let mut bytes = vec![0u8; 16];
        bytes[0] = libc::AF_UNIX as u8;
        bytes[1] = 254;
        bytes[2] = 255;
        let (_, info) = parse_diag(&bytes).unwrap();
        assert_eq!(
            info.protocol,
            SocketProtocol::Unix {
                socket_type: SocketType::Unknown(254)
            }
        );
        assert_eq!(info.state, SocketState::UnknownUnix(255));
    }

    #[test]
    fn diag_peer_and_malformed_attributes() {
        let mut bytes = vec![0u8; 24];
        bytes[0] = 1;
        bytes[1] = 1;
        bytes[2] = 1;
        bytes[4..8].copy_from_slice(&42u32.to_ne_bytes());
        bytes[16..18].copy_from_slice(&8u16.to_ne_bytes());
        bytes[18..20].copy_from_slice(&2u16.to_ne_bytes());
        bytes[20..24].copy_from_slice(&99u32.to_ne_bytes());
        let (inode, info) = parse_diag(&bytes).unwrap();
        assert_eq!(inode, 42);
        assert_eq!(info.peer_inode, Some(99));
        bytes[16] = 0;
        assert!(parse_diag(&bytes).is_err());
        assert!(parse_diag(&[]).is_err());
    }
}
