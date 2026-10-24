//! Socket table parsing and UNIX peer diagnostics for process and system scans.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`SocketInfo`] | `pub(in crate::privileged_helper)` | `struct SocketInfo` |
//! | [`read_text`] | `pub(super)` | `fn read_text(path: &`[`str`]`) -> `[`Result`]`<`[`String`]`>` |
//! | [`parse_inet`] | `pub(super)` | `fn parse_inet(text: &`[`str`]`, protocol: `[`SocketProtocol`]`) -> `[`HashMap`]`<`[`u64`]`, `[`SocketInfo`]`>` |
//! | [`parse_unix`] | `pub(super)` | `fn parse_unix(text: &`[`str`]`) -> `[`HashMap`]`<`[`u64`]`, `[`SocketInfo`]`>` |
//! | [`diagnostic_socket`] | `pub(super)` | `fn diagnostic_socket() -> `[`Result`]`<`[`OwnedFd`]`>` |
//! | [`unix_diag`] | `pub(super)` | `fn unix_diag(namespace: &`[`NetworkNamespaceReader`]`, deadline: `[`Instant`]`) -> `[`Result`]`<`[`HashMap`]`<`[`u64`]`, `[`SocketInfo`]`>>` |

use super::socket_namespace::NetworkNamespaceReader;
use crate::privileged_helper::netlink;
use crate::privileged_helper::socket_types::{
    SocketConnectionStatus, SocketProtocol, SocketState, SocketType,
};
// Visibility is scoped to the consumers of the parent process façade.
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    os::fd::OwnedFd,
    time::Instant,
};

#[derive(Clone, Debug)]
/// Socket kind and independently observed connection metadata.
/// A known protocol does not imply a known state, address or peer.
pub(in crate::privileged_helper) struct SocketInfo {
    pub(in crate::privileged_helper) protocol: SocketProtocol,
    pub(in crate::privileged_helper) state: Option<SocketState>,
    pub(in crate::privileged_helper) local: Option<SocketAddr>,
    pub(in crate::privileged_helper) remote: Option<SocketAddr>,
    pub(in crate::privileged_helper) path: Option<String>,
    pub(in crate::privileged_helper) peer_inode: Option<u64>,
}

impl SocketInfo {
    /// Preserve direct observations; table fallbacks only confirm positive facts.
    /// CLOSE, zero addresses, and UNIX state names never prove ENOTCONN.
    pub(in crate::privileged_helper) fn connection_status(
        &self,
        observed: SocketConnectionStatus,
    ) -> SocketConnectionStatus {
        if observed != SocketConnectionStatus::Unknown {
            observed
        } else if self.state == Some(SocketState::Listen) {
            SocketConnectionStatus::Listening
        } else if self.peer_inode.is_some()
            || (self.protocol.is_tcp_or_udp()
                && self
                    .remote
                    .is_some_and(|remote| remote.port() != 0 && !remote.ip().is_unspecified()))
        {
            SocketConnectionStatus::PeerPresent
        } else {
            SocketConnectionStatus::Unknown
        }
    }
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
                    state: Some(SocketState::inet(u32::from_str_radix(f.get(3)?, 16).ok()?)),
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
                    state: Some(state),
                    local: None,
                    remote: None,
                    path: (!rest.is_empty()).then(|| rest.to_owned()),
                    peer_inode: None,
                },
            ))
        })
        .collect()
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
        state: Some(SocketState::unix_diag(bytes[2] as u32)),
        local: None,
        remote: None,
        path: None,
        peer_inode: None,
    };
    for (kind, data) in netlink::attributes(&bytes[16..])? {
        if kind == 2 {
            let inode = netlink::u32_at(data, 0)? as u64;
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
    }
    Ok((netlink::u32_at(bytes, 4)? as u64, info))
}

pub(super) fn diagnostic_socket() -> Result<OwnedFd> {
    netlink::open_socket(libc::NETLINK_SOCK_DIAG, "NETLINK_SOCK_DIAG")
}

// The socket keeps its creation namespace even when queried by another thread.
// Query only metadata. No endpoint is opened, consumed, or modified.
pub(super) fn unix_diag(
    namespace: &NetworkNamespaceReader,
    deadline: Instant,
) -> Result<HashMap<u64, SocketInfo>> {
    let fd = namespace.diagnostic_socket(deadline)?;
    // struct unix_diag_req
    let mut request = [0u8; 24];
    request[0] = libc::AF_UNIX as u8;
    request[4..8].copy_from_slice(&u32::MAX.to_ne_bytes());
    request[12..16].copy_from_slice(&5u32.to_ne_bytes()); // SHOW_NAME | SHOW_PEER
    let mut result = HashMap::new();
    netlink::NetlinkDumpRequest {
        label: "UNIX diag",
        message_type: 20, // SOCK_DIAG_BY_FAMILY
        reply_type: 20,
        sequence: 1,
        payload: &request,
    }
    .run(&fd, deadline, |body| {
        let (inode, info) = parse_diag(body)?;
        result.insert(inode, info);
        Ok(())
    })?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_connection_fallback_requires_positive_table_evidence() {
        use crate::privileged_helper::socket_types::AddressFamily;
        use SocketConnectionStatus::*;
        let mut info = SocketInfo {
            protocol: SocketProtocol::Udp {
                family: AddressFamily::Ipv4,
            },
            state: Some(SocketState::Close),
            local: None,
            remote: Some("0.0.0.0:0".parse().unwrap()),
            path: None,
            peer_inode: None,
        };
        assert_eq!(info.connection_status(Unknown), Unknown);
        info.remote = Some("127.0.0.1:54321".parse().unwrap());
        assert_eq!(info.connection_status(Unknown), PeerPresent);
        assert_eq!(info.connection_status(Unconnected), Unconnected);
        info.state = Some(SocketState::Listen);
        assert_eq!(info.connection_status(Unknown), Listening);
        assert_eq!(info.connection_status(Unconnected), Unconnected);
        info.protocol = SocketProtocol::Unix {
            socket_type: SocketType::Dgram,
        };
        info.state = Some(SocketState::Connected);
        assert_eq!(info.connection_status(Unknown), Unknown);
        info.peer_inode = Some(42);
        assert_eq!(info.connection_status(Unknown), PeerPresent);
    }

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
        use crate::privileged_helper::socket_types::AddressFamily;
        let protocol = SocketProtocol::Tcp {
            family: AddressFamily::Ipv4,
        };
        let table = parse_inet(
            "header\n0: 0100007F:1F90 0200007F:0050 FE 0:0 0:0 0 0 0 18446744073709551615\n",
            protocol,
        );
        let info = &table[&u64::MAX];
        assert_eq!(info.protocol, protocol);
        assert_eq!(info.state, Some(SocketState::UnknownInet(254)));
        assert_eq!(info.local.unwrap().port(), 8080);
        let unix = parse_unix(
            "header\n000: 2 0 00010000 0001 01 42 /tmp/name with spaces\n000: 2 0 0 00FF FF 43\n",
        );
        assert_eq!(unix[&42].state, Some(SocketState::Listen));
        assert_eq!(unix[&42].path.as_deref(), Some("/tmp/name with spaces"));
        assert_eq!(
            unix[&43].protocol,
            SocketProtocol::Unix {
                socket_type: SocketType::Unknown(255)
            }
        );
        assert_eq!(unix[&43].state, Some(SocketState::UnknownUnix(255)));
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
        assert_eq!(info.state, Some(SocketState::UnknownUnix(255)));
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
