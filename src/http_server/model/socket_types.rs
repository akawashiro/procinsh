//! Structured socket metadata and descriptor capabilities shared by both views.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`AddressFamily`] | `pub` | `enum AddressFamily` |
//! | [`SocketType`] | `pub` | `enum SocketType` |
//! | [`SocketProtocol`] | `pub` | `enum SocketProtocol` |
//! | [`SocketConnectionStatus`] | `pub` | `enum SocketConnectionStatus` |
//! | [`SocketState`] | `pub` | `enum SocketState` |
//! | [`FdKind`] | `pub` | `enum FdKind` |
//! | [`FdAccess`] | `pub` | `enum FdAccess` |
//! | [`InetAddress`] | `pub` | `struct InetAddress` |

#[allow(unused_imports, reason = "Preserve the complete façade interface")]
pub use crate::ipc::model::socket_types::{
    AddressFamily, FdAccess, FdKind, InetAddress, SocketConnectionStatus, SocketProtocol,
    SocketState, SocketType,
};
use std::net::IpAddr;
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::socket_types::AddressFamily")]
#[serde(rename_all = "snake_case")]
enum AddressFamilyJson {
    Ipv4,
    Ipv6,
}
impl serde::Serialize for crate::http_server::model::socket_types::AddressFamily {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AddressFamilyJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::socket_types::AddressFamily {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        AddressFamilyJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::socket_types::SocketType")]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
enum SocketTypeJson {
    Stream,
    Dgram,
    Raw,
    Rdm,
    Seqpacket,
    Dccp,
    Packet,
    Unknown(u32),
}
impl serde::Serialize for crate::http_server::model::socket_types::SocketType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SocketTypeJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::socket_types::SocketType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SocketTypeJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::socket_types::SocketProtocol")]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SocketProtocolJson {
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
impl serde::Serialize for crate::http_server::model::socket_types::SocketProtocol {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SocketProtocolJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::socket_types::SocketProtocol {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SocketProtocolJson::deserialize(d)
    }
}

/// Independently observed listener/peer availability, not a transport state.
///
/// A peer includes an unnamed UNIX endpoint or a Netlink default destination;
/// it does not identify a process. Unknown observations must remain visible.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::socket_types::SocketConnectionStatus")]
#[serde(rename_all = "snake_case")]
enum SocketConnectionStatusJson {
    Listening,
    Unconnected,
    PeerPresent,
    Unknown,
}
impl serde::Serialize for crate::http_server::model::socket_types::SocketConnectionStatus {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SocketConnectionStatusJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de>
    for crate::http_server::model::socket_types::SocketConnectionStatus
{
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SocketConnectionStatusJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::socket_types::SocketState")]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
enum SocketStateJson {
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
impl serde::Serialize for crate::http_server::model::socket_types::SocketState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SocketStateJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::socket_types::SocketState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SocketStateJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::socket_types::FdKind")]
#[serde(rename_all = "snake_case")]
enum FdKindJson {
    Pipe,
    Socket,
    Fifo,
}
impl serde::Serialize for crate::http_server::model::socket_types::FdKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FdKindJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::socket_types::FdKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        FdKindJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::socket_types::FdAccess")]
#[serde(rename_all = "snake_case")]
enum FdAccessJson {
    Read,
    Write,
    ReadWrite,
    Unknown,
}
impl serde::Serialize for crate::http_server::model::socket_types::FdAccess {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FdAccessJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::socket_types::FdAccess {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        FdAccessJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::socket_types::InetAddress")]
struct InetAddressJson {
    pub ip: IpAddr,
    pub port: u16,
}
impl serde::Serialize for crate::http_server::model::socket_types::InetAddress {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        InetAddressJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::socket_types::InetAddress {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        InetAddressJson::deserialize(d)
    }
}

#[cfg(test)]
mod tests {
    #[allow(clippy::wildcard_imports)]
    use super::*;

    #[test]
    fn kinds_keep_codes_and_match_shared_display_contract() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../privileged_helper/test_support/socket-protocols.json"
        ))
        .unwrap();
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
