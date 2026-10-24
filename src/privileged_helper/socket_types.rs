//! Structured socket metadata and descriptor capabilities shared by both views.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`AddressFamily`] | `pub(super)` | `enum AddressFamily` |
//! | [`SocketType`] | `pub(super)` | `enum SocketType` |
//! | [`SocketProtocol`] | `pub(super)` | `enum SocketProtocol` |
//! | [`SocketState`] | `pub(super)` | `enum SocketState` |
//! | [`SocketConnectionStatus`] | `pub(super)` | `enum SocketConnectionStatus` |
//! | [`FdKind`] | `pub(super)` | `enum FdKind` |
//! | [`FdAccess`] | `pub(super)` | `enum FdAccess` |
//! | [`InetAddress`] | `pub(super)` | `struct InetAddress` |
use serde::Serialize;
use std::{
    fmt,
    net::{IpAddr, SocketAddr},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AddressFamily {
    Ipv4,
    Ipv6,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
pub(super) enum SocketType {
    Stream,
    Dgram,
    Raw,
    Rdm,
    Seqpacket,
    Dccp,
    Packet,
    Unknown(u32),
}
impl SocketType {
    pub(super) fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Stream,
            2 => Self::Dgram,
            3 => Self::Raw,
            4 => Self::Rdm,
            5 => Self::Seqpacket,
            6 => Self::Dccp,
            10 => Self::Packet,
            n => Self::Unknown(n),
        }
    }
}
impl fmt::Display for SocketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stream => "STREAM",
            Self::Dgram => "DGRAM",
            Self::Raw => "RAW",
            Self::Rdm => "RDM",
            Self::Seqpacket => "SEQPACKET",
            Self::Dccp => "DCCP",
            Self::Packet => "PACKET",
            Self::Unknown(code) => return write!(f, "TYPE {code}"),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum SocketProtocol {
    Tcp {
        family: AddressFamily,
    },
    Udp {
        family: AddressFamily,
    },
    Unix {
        socket_type: SocketType,
    },
    /// Linux domain, socket type and protocol numbers outside TCP/UDP/UNIX.
    /// A missing socket type means that only a procfs table supplied metadata.
    Other {
        family: u32,
        socket_type: Option<SocketType>,
        protocol: u32,
    },
}
impl SocketProtocol {
    pub(super) fn is_tcp(self) -> bool {
        matches!(self, Self::Tcp { .. })
    }

    pub(super) fn is_tcp_or_udp(self) -> bool {
        matches!(self, Self::Tcp { .. } | Self::Udp { .. })
    }

    pub(super) fn from_codes(family: u32, socket_type: u32, protocol: u32) -> Self {
        let inet = match family as i32 {
            libc::AF_INET => Some(AddressFamily::Ipv4),
            libc::AF_INET6 => Some(AddressFamily::Ipv6),
            _ => None,
        };
        match (family as i32, socket_type as i32, protocol as i32) {
            (_, libc::SOCK_STREAM, libc::IPPROTO_TCP) if inet.is_some() => Self::Tcp {
                family: inet.unwrap(),
            },
            (_, libc::SOCK_DGRAM, libc::IPPROTO_UDP) if inet.is_some() => Self::Udp {
                family: inet.unwrap(),
            },
            (libc::AF_UNIX, _, _) => Self::Unix {
                socket_type: SocketType::from_code(socket_type),
            },
            _ => Self::Other {
                family,
                socket_type: Some(SocketType::from_code(socket_type)),
                protocol,
            },
        }
    }
}
impl fmt::Display for SocketProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp { family } => f.write_str(if *family == AddressFamily::Ipv6 {
                "TCP6"
            } else {
                "TCP"
            }),
            Self::Udp { family } => f.write_str(if *family == AddressFamily::Ipv6 {
                "UDP6"
            } else {
                "UDP"
            }),
            Self::Unix { socket_type } => write!(f, "UNIX {socket_type}"),
            Self::Other {
                family,
                socket_type,
                protocol,
            } => {
                if *family == libc::AF_NETLINK as u32 {
                    write!(f, "NETLINK ")?;
                    match netlink_name(*protocol) {
                        Some(name) => f.write_str(name),
                        None => write!(f, "PROTOCOL {protocol}"),
                    }?;
                    if let Some(SocketType::Unknown(code)) = socket_type {
                        write!(f, " TYPE {code}")?;
                    }
                    return Ok(());
                }
                if matches!(*family as i32, libc::AF_INET | libc::AF_INET6)
                    && *socket_type == Some(SocketType::Raw)
                {
                    write!(
                        f,
                        "RAW{} ",
                        if *family == libc::AF_INET6 as u32 {
                            "6"
                        } else {
                            ""
                        }
                    )?;
                    return match *protocol {
                        1 => f.write_str("ICMP"),
                        2 => f.write_str("IGMP"),
                        6 => f.write_str("TCP"),
                        17 => f.write_str("UDP"),
                        58 => f.write_str("ICMPV6"),
                        _ => write!(f, "PROTOCOL {protocol}"),
                    };
                }
                match *family as i32 {
                    libc::AF_INET => f.write_str("INET")?,
                    libc::AF_INET6 => f.write_str("INET6")?,
                    libc::AF_PACKET => f.write_str("PACKET")?,
                    libc::AF_VSOCK => f.write_str("VSOCK")?,
                    libc::AF_BLUETOOTH => f.write_str("BLUETOOTH")?,
                    libc::AF_CAN => f.write_str("CAN")?,
                    libc::AF_ALG => f.write_str("ALG")?,
                    libc::AF_XDP => f.write_str("XDP")?,
                    _ => write!(f, "AF {family}")?,
                }
                if let Some(socket_type) = socket_type {
                    write!(f, " {socket_type}")?;
                }
                if !matches!(*family as i32, libc::AF_PACKET | libc::AF_VSOCK) || *protocol != 0 {
                    write!(f, " PROTOCOL {protocol}")?;
                }
                Ok(())
            }
        }
    }
}

fn netlink_name(protocol: u32) -> Option<&'static str> {
    Some(match protocol {
        0 => "ROUTE",
        1 => "UNUSED",
        2 => "USERSOCK",
        3 => "FIREWALL",
        4 => "SOCK_DIAG",
        5 => "NFLOG",
        6 => "XFRM",
        7 => "SELINUX",
        8 => "ISCSI",
        9 => "AUDIT",
        10 => "FIB_LOOKUP",
        11 => "CONNECTOR",
        12 => "NETFILTER",
        13 => "IP6_FW",
        14 => "DNRTMSG",
        15 => "KOBJECT_UEVENT",
        16 => "GENERIC",
        18 => "SCSITRANSPORT",
        19 => "ECRYPTFS",
        20 => "RDMA",
        21 => "CRYPTO",
        22 => "SMC",
        _ => return None,
    })
}
/// Independently observed listener/peer availability, not a transport state.
///
/// A peer includes an unnamed UNIX endpoint or a Netlink default destination;
/// it does not identify a process. Unknown observations must remain visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SocketConnectionStatus {
    Listening,
    Unconnected,
    PeerPresent,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
pub(super) enum SocketState {
    Established,
    SynSent,
    SynRecv,
    FinWait1,
    FinWait2,
    TimeWait,
    Close,
    CloseWait,
    LastAck,
    Listen,
    Closing,
    NewSynRecv,
    Unconnected,
    Connecting,
    Connected,
    Disconnecting,
    UnknownInet(u32),
    UnknownUnix(u32),
}
impl SocketState {
    pub(super) fn inet(code: u32) -> Self {
        match code {
            1 => Self::Established,
            2 => Self::SynSent,
            3 => Self::SynRecv,
            4 => Self::FinWait1,
            5 => Self::FinWait2,
            6 => Self::TimeWait,
            7 => Self::Close,
            8 => Self::CloseWait,
            9 => Self::LastAck,
            10 => Self::Listen,
            11 => Self::Closing,
            12 => Self::NewSynRecv,
            n => Self::UnknownInet(n),
        }
    }

    pub(super) fn unix_proc(code: u32, listening: bool) -> Self {
        if listening {
            Self::Listen
        } else {
            match code {
                1 => Self::Unconnected,
                2 => Self::Connecting,
                3 => Self::Connected,
                4 => Self::Disconnecting,
                n => Self::UnknownUnix(n),
            }
        }
    }

    pub(super) fn unix_diag(code: u32) -> Self {
        match code {
            1 => Self::Connected,
            10 => Self::Listen,
            7 => Self::Unconnected,
            n => Self::UnknownUnix(n),
        }
    }
}
impl fmt::Display for SocketState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Established => "ESTABLISHED",
            Self::SynSent => "SYN_SENT",
            Self::SynRecv => "SYN_RECV",
            Self::FinWait1 => "FIN_WAIT1",
            Self::FinWait2 => "FIN_WAIT2",
            Self::TimeWait => "TIME_WAIT",
            Self::Close => "CLOSE",
            Self::CloseWait => "CLOSE_WAIT",
            Self::LastAck => "LAST_ACK",
            Self::Listen => "LISTEN",
            Self::Closing => "CLOSING",
            Self::NewSynRecv => "NEW_SYN_RECV",
            Self::Unconnected => "UNCONNECTED",
            Self::Connecting => "CONNECTING",
            Self::Connected => "CONNECTED",
            Self::Disconnecting => "DISCONNECTING",
            Self::UnknownInet(n) | Self::UnknownUnix(n) => return write!(f, "{n:02X}"),
        };
        f.write_str(text)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FdKind {
    Pipe,
    Socket,
    Fifo,
}
impl fmt::Display for FdKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pipe => "pipe",
            Self::Socket => "socket",
            Self::Fifo => "fifo",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FdAccess {
    Read,
    Write,
    ReadWrite,
    Unknown,
}
impl FdAccess {
    pub(super) fn from_flags(flags: u32) -> Self {
        if flags == u32::MAX || flags & libc::O_PATH as u32 != 0 {
            Self::Unknown
        } else {
            Self::from_mode(Some(flags & libc::O_ACCMODE as u32))
        }
    }

    pub(super) fn from_mode(mode: Option<u32>) -> Self {
        match mode {
            Some(0) => Self::Read,
            Some(1) => Self::Write,
            Some(2) => Self::ReadWrite,
            _ => Self::Unknown,
        }
    }

    pub(super) fn opposite(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Read, Self::Write | Self::ReadWrite)
                | (Self::Write, Self::Read | Self::ReadWrite)
                | (Self::ReadWrite, Self::Read | Self::Write | Self::ReadWrite)
        )
    }
}
impl fmt::Display for FdAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::ReadWrite => "read/write",
            Self::Unknown => "N/A",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct InetAddress {
    pub(super) ip: IpAddr,
    pub(super) port: u16,
}
impl From<SocketAddr> for InetAddress {
    fn from(address: SocketAddr) -> Self {
        Self {
            ip: address.ip(),
            port: address.port(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_keep_codes_and_match_shared_display_contract() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("test_support/socket-protocols.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let codes = &case["codes"];
            let family = codes[0].as_u64().unwrap() as u32;
            let protocol = codes[2].as_u64().unwrap() as u32;
            let kind = match codes[1].as_u64() {
                Some(socket_type) => {
                    SocketProtocol::from_codes(family, socket_type as u32, protocol)
                }
                None => SocketProtocol::Other {
                    family,
                    socket_type: None,
                    protocol,
                },
            };
            assert_eq!(serde_json::to_value(kind).unwrap(), case["value"]);
            assert_eq!(kind.to_string(), case["label"].as_str().unwrap());
            assert_eq!(
                kind.is_tcp_or_udp(),
                matches!(
                    kind,
                    SocketProtocol::Tcp { .. } | SocketProtocol::Udp { .. }
                )
            );
        }
    }

    #[test]
    fn unknown_codes_and_descriptor_capabilities_are_preserved() {
        assert_eq!(
            serde_json::to_value(SocketType::from_code(999)).unwrap(),
            serde_json::json!({"kind":"unknown","code":999})
        );
        assert_eq!(
            serde_json::to_value(SocketState::inet(99)).unwrap(),
            serde_json::json!({"kind":"unknown_inet","code":99})
        );
        assert_eq!(
            SocketState::unix_proc(99, false),
            SocketState::UnknownUnix(99)
        );
        assert_eq!(FdAccess::from_flags(libc::O_PATH as u32), FdAccess::Unknown);
        assert_eq!(FdAccess::from_flags(u32::MAX), FdAccess::Unknown);
        assert!(!FdAccess::Unknown.opposite(FdAccess::Write));
        assert!(FdAccess::Read.opposite(FdAccess::Write));
    }
}
