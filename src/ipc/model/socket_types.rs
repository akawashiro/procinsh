//! Structured socket metadata and descriptor capabilities shared by both views.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`AddressFamily`] | `pub(crate)` | `enum AddressFamily` |
//! | [`SocketType`] | `pub(crate)` | `enum SocketType` |
//! | [`SocketProtocol`] | `pub(crate)` | `enum SocketProtocol` |
//! | [`SocketConnectionStatus`] | `pub(crate)` | `enum SocketConnectionStatus` |
//! | [`SocketState`] | `pub(crate)` | `enum SocketState` |
//! | [`FdKind`] | `pub(crate)` | `enum FdKind` |
//! | [`FdAccess`] | `pub(crate)` | `enum FdAccess` |
//! | [`InetAddress`] | `pub(crate)` | `struct InetAddress` |
//! | [`socket_type_from_code`] | `pub(crate)` | `fn socket_type_from_code(code: u32) -> `[`SocketType`] |
//! | [`socket_protocol_is_tcp`] | `pub(crate)` | `fn socket_protocol_is_tcp(state: `[`SocketProtocol`]`) -> bool` |
//! | [`socket_protocol_is_tcp_or_udp`] | `pub(crate)` | `fn socket_protocol_is_tcp_or_udp(state: `[`SocketProtocol`]`) -> bool` |
//! | [`socket_protocol_from_codes`] | `pub(crate)` | `fn socket_protocol_from_codes(family: u32, socket_type: u32, protocol: u32) -> `[`SocketProtocol`] |
//! | [`inet_socket_state`] | `pub(crate)` | `fn inet_socket_state(code: u32) -> `[`SocketState`] |
//! | [`unix_proc_socket_state`] | `pub(crate)` | `fn unix_proc_socket_state(code: u32, listening: bool) -> `[`SocketState`] |
//! | [`unix_diag_socket_state`] | `pub(crate)` | `fn unix_diag_socket_state(code: u32) -> `[`SocketState`] |
//! | [`fd_access_from_flags`] | `pub(crate)` | `fn fd_access_from_flags(flags: u32) -> `[`FdAccess`] |
//! | [`fd_access_from_mode`] | `pub(crate)` | `fn fd_access_from_mode(mode: `[`Option`]`<u32>) -> `[`FdAccess`] |
//! | [`opposite_fd_access`] | `pub(crate)` | `fn opposite_fd_access(state: `[`FdAccess`]`, other: `[`FdAccess`]`) -> bool` |

use std::{
    fmt,
    net::{IpAddr, SocketAddr},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AddressFamily {
    Ipv4,
    Ipv6,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SocketType {
    Stream,
    Dgram,
    Raw,
    Rdm,
    Seqpacket,
    Dccp,
    Packet,
    Unknown(u32),
}
pub(crate) fn socket_type_from_code(code: u32) -> SocketType {
    match code {
        1 => SocketType::Stream,
        2 => SocketType::Dgram,
        3 => SocketType::Raw,
        4 => SocketType::Rdm,
        5 => SocketType::Seqpacket,
        6 => SocketType::Dccp,
        10 => SocketType::Packet,
        n => SocketType::Unknown(n),
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SocketProtocol {
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
pub(crate) fn socket_protocol_is_tcp(state: SocketProtocol) -> bool {
    matches!(state, SocketProtocol::Tcp { .. })
}

pub(crate) fn socket_protocol_is_tcp_or_udp(state: SocketProtocol) -> bool {
    matches!(
        state,
        SocketProtocol::Tcp { .. } | SocketProtocol::Udp { .. }
    )
}

pub(crate) fn socket_protocol_from_codes(
    family: u32,
    socket_type: u32,
    protocol: u32,
) -> SocketProtocol {
    let inet = match family as i32 {
        libc::AF_INET => Some(AddressFamily::Ipv4),
        libc::AF_INET6 => Some(AddressFamily::Ipv6),
        _ => None,
    };
    match (family as i32, socket_type as i32, protocol as i32) {
        (_, libc::SOCK_STREAM, libc::IPPROTO_TCP) if inet.is_some() => SocketProtocol::Tcp {
            family: inet.unwrap(),
        },
        (_, libc::SOCK_DGRAM, libc::IPPROTO_UDP) if inet.is_some() => SocketProtocol::Udp {
            family: inet.unwrap(),
        },
        (libc::AF_UNIX, _, _) => SocketProtocol::Unix {
            socket_type: socket_type_from_code(socket_type),
        },
        _ => SocketProtocol::Other {
            family,
            socket_type: Some(socket_type_from_code(socket_type)),
            protocol,
        },
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SocketConnectionStatus {
    Listening,
    Unconnected,
    PeerPresent,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SocketState {
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
pub(crate) fn inet_socket_state(code: u32) -> SocketState {
    match code {
        1 => SocketState::Established,
        2 => SocketState::SynSent,
        3 => SocketState::SynRecv,
        4 => SocketState::FinWait1,
        5 => SocketState::FinWait2,
        6 => SocketState::TimeWait,
        7 => SocketState::Close,
        8 => SocketState::CloseWait,
        9 => SocketState::LastAck,
        10 => SocketState::Listen,
        11 => SocketState::Closing,
        12 => SocketState::NewSynRecv,
        n => SocketState::UnknownInet(n),
    }
}

pub(crate) fn unix_proc_socket_state(code: u32, listening: bool) -> SocketState {
    if listening {
        SocketState::Listen
    } else {
        match code {
            1 => SocketState::Unconnected,
            2 => SocketState::Connecting,
            3 => SocketState::Connected,
            4 => SocketState::Disconnecting,
            n => SocketState::UnknownUnix(n),
        }
    }
}

pub(crate) fn unix_diag_socket_state(code: u32) -> SocketState {
    match code {
        1 => SocketState::Connected,
        10 => SocketState::Listen,
        7 => SocketState::Unconnected,
        n => SocketState::UnknownUnix(n),
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FdKind {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FdAccess {
    Read,
    Write,
    ReadWrite,
    Unknown,
}
pub(crate) fn fd_access_from_flags(flags: u32) -> FdAccess {
    if flags == u32::MAX || flags & libc::O_PATH as u32 != 0 {
        FdAccess::Unknown
    } else {
        fd_access_from_mode(Some(flags & libc::O_ACCMODE as u32))
    }
}

pub(crate) fn fd_access_from_mode(mode: Option<u32>) -> FdAccess {
    match mode {
        Some(0) => FdAccess::Read,
        Some(1) => FdAccess::Write,
        Some(2) => FdAccess::ReadWrite,
        _ => FdAccess::Unknown,
    }
}

pub(crate) fn opposite_fd_access(state: FdAccess, other: FdAccess) -> bool {
    matches!(
        (state, other),
        (FdAccess::Read, FdAccess::Write | FdAccess::ReadWrite)
            | (FdAccess::Write, FdAccess::Read | FdAccess::ReadWrite)
            | (
                FdAccess::ReadWrite,
                FdAccess::Read | FdAccess::Write | FdAccess::ReadWrite
            )
    )
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InetAddress {
    pub(crate) ip: IpAddr,
    pub(crate) port: u16,
}
impl From<SocketAddr> for InetAddress {
    fn from(address: SocketAddr) -> Self {
        Self {
            ip: address.ip(),
            port: address.port(),
        }
    }
}

crate::ipc::codec::wire_enum!(AddressFamily{0=>Ipv4,1=>Ipv6});

crate::ipc::codec::wire_enum!(SocketType{0=>Stream,1=>Dgram,2=>Raw,3=>Rdm,4=>Seqpacket,5=>Dccp,6=>Packet,7=>Unknown(value)});

crate::ipc::codec::wire_enum!(SocketProtocol{0=>Tcp{family},1=>Udp{family},2=>Unix{socket_type},3=>Other{family,socket_type,protocol}});

crate::ipc::codec::wire_enum!(SocketConnectionStatus{0=>Listening,1=>Unconnected,2=>PeerPresent,3=>Unknown});

crate::ipc::codec::wire_enum!(SocketState{0=>Established,1=>SynSent,2=>SynRecv,3=>FinWait1,4=>FinWait2,5=>TimeWait,6=>Close,7=>CloseWait,8=>LastAck,9=>Listen,10=>Closing,11=>NewSynRecv,12=>Unconnected,13=>Connecting,14=>Connected,15=>Disconnecting,16=>UnknownInet(value),17=>UnknownUnix(value)});

crate::ipc::codec::wire_enum!(FdKind{0=>Pipe,1=>Socket,2=>Fifo});

crate::ipc::codec::wire_enum!(FdAccess{0=>Read,1=>Write,2=>ReadWrite,3=>Unknown});

crate::ipc::codec::wire_struct!(InetAddress { ip, port });
