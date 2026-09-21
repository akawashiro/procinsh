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
pub struct SocketInfo {
    pub protocol: String,
    pub state: String,
    pub local: Option<SocketAddr>,
    pub remote: Option<SocketAddr>,
    pub path: Option<String>,
    pub peer_inode: Option<u64>,
}

pub fn read_text(path: &str) -> Result<String> {
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
            for (index, out) in bytes.chunks_exact_mut(4).enumerate() {
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

fn tcp_state(state: &str) -> String {
    match state {
        "01" => "ESTABLISHED",
        "02" => "SYN_SENT",
        "03" => "SYN_RECV",
        "04" => "FIN_WAIT1",
        "05" => "FIN_WAIT2",
        "06" => "TIME_WAIT",
        "07" => "CLOSE",
        "08" => "CLOSE_WAIT",
        "09" => "LAST_ACK",
        "0A" => "LISTEN",
        "0B" => "CLOSING",
        "0C" => "NEW_SYN_RECV",
        _ => state,
    }
    .into()
}

pub fn parse_inet(text: &str, protocol: &str) -> HashMap<u64, SocketInfo> {
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
                    protocol: protocol.into(),
                    state: tcp_state(f.get(3)?),
                    local: Some(address(f.get(1)?).ok()?),
                    remote: Some(address(f.get(2)?).ok()?),
                    path: None,
                    peer_inode: None,
                },
            ))
        })
        .collect()
}

pub fn parse_unix(text: &str) -> HashMap<u64, SocketInfo> {
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
            let protocol = match f[4] {
                "0001" => "UNIX STREAM",
                "0002" => "UNIX DGRAM",
                "0005" => "UNIX SEQPACKET",
                _ => "UNIX",
            };
            let state = if u32::from_str_radix(f[3], 16).ok()? & 0x10000 != 0 {
                "LISTEN"
            } else {
                match f[5] {
                    "01" => "UNCONNECTED",
                    "02" => "CONNECTING",
                    "03" => "CONNECTED",
                    "04" => "DISCONNECTING",
                    _ => f[5],
                }
            };
            Some((
                f[6].parse().ok()?,
                SocketInfo {
                    protocol: protocol.into(),
                    state: state.into(),
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
        protocol: match bytes[1] {
            1 => "UNIX STREAM",
            2 => "UNIX DGRAM",
            5 => "UNIX SEQPACKET",
            _ => "UNIX",
        }
        .into(),
        state: match bytes[2] {
            1 => "CONNECTED",
            10 => "LISTEN",
            7 => "UNCONNECTED",
            _ => "UNKNOWN",
        }
        .into(),
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
pub fn unix_diag(deadline: Instant) -> Result<HashMap<u64, SocketInfo>> {
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
