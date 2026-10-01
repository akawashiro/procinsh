//! Structured socket metadata and descriptor capabilities shared by both views.
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
    Seqpacket,
    Unknown(u32),
}
impl SocketType {
    pub(super) fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Stream,
            2 => Self::Dgram,
            5 => Self::Seqpacket,
            n => Self::Unknown(n),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum SocketProtocol {
    Tcp { family: AddressFamily },
    Udp { family: AddressFamily },
    Unix { socket_type: SocketType },
}
impl SocketProtocol {
    pub(super) fn is_tcp(self) -> bool {
        matches!(self, Self::Tcp { .. })
    }

    pub(super) fn is_inet(self) -> bool {
        !matches!(self, Self::Unix { .. })
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
            Self::Unix { socket_type } => f.write_str(match socket_type {
                SocketType::Stream => "UNIX STREAM",
                SocketType::Dgram => "UNIX DGRAM",
                SocketType::Seqpacket => "UNIX SEQPACKET",
                SocketType::Unknown(_) => "UNIX",
            }),
        }
    }
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
