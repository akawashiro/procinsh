//! Native RDMA codes, addresses, ports and queue pair observations.
//! Unknown kernel codes are preserved.
//!
//! # Interface
//! - [`RdmaQpType`] (`pub`): `enum RdmaQpType`.
//! - [`RdmaQpState`] (`pub`): `enum RdmaQpState`.
//! - [`RdmaCmState`] (`pub`): `enum RdmaCmState`.
//! - [`RdmaPortSpace`] (`pub`): `enum RdmaPortSpace`.
//! - [`IbGid`] (`pub`): `struct IbGid`.
//! - [`RdmaAddress`] (`pub`): `enum RdmaAddress`.
//! - [`RdmaPortInfo`] (`pub`): `struct RdmaPortInfo`.
//! - [`RdmaEndpointOwner`] (`pub`): `enum RdmaEndpointOwner`.
//! - [`RdmaEndpoint`] (`pub`): `struct RdmaEndpoint`.
//! - [`RdmaCmInfo`] (`pub`): `struct RdmaCmInfo`.
//! - [`RdmaRelation`] (`pub`): `struct RdmaRelation`.

use crate::ipc::schema::ProcessId;
use std::{fmt, net::IpAddr};

/// `enum ib_qp_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RdmaQpType {
    Smi,
    Gsi,
    Rc,
    Uc,
    Ud,
    RawIpv6,
    RawEthertype,
    RawPacket,
    XrcIni,
    XrcTgt,
    Driver,
    Unknown(u8),
}
impl RdmaQpType {
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Smi,
            1 => Self::Gsi,
            2 => Self::Rc,
            3 => Self::Uc,
            4 => Self::Ud,
            5 => Self::RawIpv6,
            6 => Self::RawEthertype,
            8 => Self::RawPacket,
            9 => Self::XrcIni,
            10 => Self::XrcTgt,
            0xff => Self::Driver,
            n => Self::Unknown(n),
        }
    }

    /// Types whose QP addresses exactly one remote QP.
    pub fn is_connected(self) -> bool {
        matches!(self, Self::Rc | Self::Uc | Self::XrcIni | Self::XrcTgt)
    }
}
impl fmt::Display for RdmaQpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Smi => "SMI",
            Self::Gsi => "GSI",
            Self::Rc => "RC",
            Self::Uc => "UC",
            Self::Ud => "UD",
            Self::RawIpv6 => "RAW_IPV6",
            Self::RawEthertype => "RAW_ETHERTYPE",
            Self::RawPacket => "RAW_PACKET",
            Self::XrcIni => "XRC_INI",
            Self::XrcTgt => "XRC_TGT",
            Self::Driver => "DRIVER",
            Self::Unknown(code) => return write!(f, "TYPE {code}"),
        })
    }
}

/// `enum ib_qp_state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RdmaQpState {
    Reset,
    Init,
    Rtr,
    Rts,
    Sqd,
    Sqe,
    Err,
    Unknown(u8),
}
impl RdmaQpState {
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Reset,
            1 => Self::Init,
            2 => Self::Rtr,
            3 => Self::Rts,
            4 => Self::Sqd,
            5 => Self::Sqe,
            6 => Self::Err,
            n => Self::Unknown(n),
        }
    }
}
impl fmt::Display for RdmaQpState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Reset => "RESET",
            Self::Init => "INIT",
            Self::Rtr => "RTR",
            Self::Rts => "RTS",
            Self::Sqd => "SQD",
            Self::Sqe => "SQE",
            Self::Err => "ERR",
            Self::Unknown(code) => return write!(f, "STATE {code}"),
        })
    }
}

/// The kernel's `enum rdma_cm_state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RdmaCmState {
    Idle,
    AddrQuery,
    AddrResolved,
    RouteQuery,
    RouteResolved,
    Connect,
    Disconnect,
    AddrBound,
    Listen,
    DeviceRemoval,
    Destroying,
    Unknown(u8),
}
impl RdmaCmState {
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Idle,
            1 => Self::AddrQuery,
            2 => Self::AddrResolved,
            3 => Self::RouteQuery,
            4 => Self::RouteResolved,
            5 => Self::Connect,
            6 => Self::Disconnect,
            7 => Self::AddrBound,
            8 => Self::Listen,
            9 => Self::DeviceRemoval,
            10 => Self::Destroying,
            n => Self::Unknown(n),
        }
    }
}
impl fmt::Display for RdmaCmState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Idle => "IDLE",
            Self::AddrQuery => "ADDR_QUERY",
            Self::AddrResolved => "ADDR_RESOLVED",
            Self::RouteQuery => "ROUTE_QUERY",
            Self::RouteResolved => "ROUTE_RESOLVED",
            Self::Connect => "CONNECT",
            Self::Disconnect => "DISCONNECT",
            Self::AddrBound => "ADDR_BOUND",
            Self::Listen => "LISTEN",
            Self::DeviceRemoval => "DEVICE_REMOVAL",
            Self::Destroying => "DESTROYING",
            Self::Unknown(code) => return write!(f, "STATE {code}"),
        })
    }
}

/// `enum rdma_ucm_port_space`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RdmaPortSpace {
    Ipoib,
    Tcp,
    Udp,
    Ib,
    Unknown(u32),
}
impl RdmaPortSpace {
    pub fn from_code(code: u32) -> Self {
        match code {
            0x0002 => Self::Ipoib,
            0x0106 => Self::Tcp,
            0x0111 => Self::Udp,
            0x013f => Self::Ib,
            n => Self::Unknown(n),
        }
    }
}
impl fmt::Display for RdmaPortSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ipoib => "IPOIB",
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
            Self::Ib => "IB",
            Self::Unknown(code) => return write!(f, "PS 0x{code:04x}"),
        })
    }
}

/// A 128-bit IB global identifier, written like sysfs `gids/N`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IbGid(pub [u8; 16]);
impl IbGid {
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        let gid = Self(bytes.try_into().ok()?);
        (gid.0 != [0; 16]).then_some(gid)
    }

    /// Parses `fe80:0000:...:45c7`; the all-zero GID is unassigned.
    pub fn parse(text: &str) -> Option<Self> {
        let groups: Vec<_> = text.trim().split(':').collect();
        if groups.len() != 8 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (index, group) in groups.iter().enumerate() {
            if group.len() != 4 {
                return None;
            }
            bytes[index * 2..index * 2 + 2]
                .copy_from_slice(&u16::from_str_radix(group, 16).ok()?.to_be_bytes());
        }
        Self::from_slice(&bytes)
    }
}
impl fmt::Display for IbGid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, pair) in self.0.chunks(2).enumerate() {
            if index > 0 {
                f.write_str(":")?;
            }
            write!(f, "{:02x}{:02x}", pair[0], pair[1])?;
        }
        Ok(())
    }
}

/// A connection manager endpoint address from `__kernel_sockaddr_storage`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RdmaAddress {
    Inet {
        ip: IpAddr,
        port: u16,
    },
    /// `struct sockaddr_ib`; the 64-bit service ID is a hexadecimal string.
    Ib {
        gid: IbGid,
        service_id: u64,
        pkey: u16,
    },
    Unknown {
        family: u16,
    },
}
const AF_IB: u16 = 27;
impl RdmaAddress {
    /// Decodes a CM address. A zero family means no address was assigned.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let family = u16::from_ne_bytes(bytes.get(0..2)?.try_into().ok()?);
        let port = || Some(u16::from_be_bytes(bytes.get(2..4)?.try_into().ok()?));
        Some(match family {
            0 => return None,
            f if f == libc::AF_INET as u16 => Self::Inet {
                ip: IpAddr::from(<[u8; 4]>::try_from(bytes.get(4..8)?).ok()?),
                port: port()?,
            },
            f if f == libc::AF_INET6 as u16 => {
                let ip = std::net::Ipv6Addr::from(<[u8; 16]>::try_from(bytes.get(8..24)?).ok()?);
                Self::Inet {
                    ip: ip
                        .to_ipv4_mapped()
                        .map(IpAddr::V4)
                        .unwrap_or(IpAddr::V6(ip)),
                    port: port()?,
                }
            }
            AF_IB => Self::Ib {
                pkey: port()?,
                gid: IbGid::from_slice(bytes.get(8..24)?)?,
                service_id: u64::from_be_bytes(bytes.get(24..32)?.try_into().ok()?),
            },
            family => Self::Unknown { family },
        })
    }

    pub fn ip(&self) -> Option<IpAddr> {
        match self {
            Self::Inet { ip, .. } => Some(*ip),
            _ => None,
        }
    }
}
impl fmt::Display for RdmaAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inet { ip, port } => write!(f, "{}", std::net::SocketAddr::new(*ip, *port)),
            Self::Ib {
                gid, service_id, ..
            } => write!(f, "{gid} SID 0x{service_id:016x}"),
            Self::Unknown { family } => write!(f, "AF {family}"),
        }
    }
}

/// One port of an RDMA device. Unassigned LIDs and GIDs are null.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RdmaPortInfo {
    pub device: String,
    pub port: u32,
    /// `InfiniBand` or `Ethernet` (RoCE), as written by the kernel.
    pub link_layer: Option<String>,
    /// Logical port state, such as `ACTIVE`.
    pub state: Option<String>,
    pub lid: Option<u32>,
    pub gid: Option<IbGid>,
    pub rate: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RdmaEndpointOwner {
    Process {
        process_id: ProcessId,
    },
    /// Kernel consumer, such as `rpcrdma` or `ib_ipoib`.
    Kernel {
        name: String,
    },
    /// A PID absent from this snapshot, or no owner information.
    Unknown {
        pid: Option<u32>,
    },
}

/// A local queue pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RdmaEndpoint {
    pub owner: RdmaEndpointOwner,
    pub device: String,
    /// Null until the QP is bound to a port.
    pub port: Option<u32>,
    pub qpn: u32,
    pub qp_type: RdmaQpType,
    pub state: RdmaQpState,
}

/// The connection manager ID bound to a queue pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RdmaCmInfo {
    pub id: Option<u32>,
    /// The CM consumer, which can differ from the QP owner (for example
    /// `rpcrdma` uses QPs created by `rdma_cm`).
    pub owner: RdmaEndpointOwner,
    pub state: RdmaCmState,
    pub port_space: RdmaPortSpace,
    pub source: Option<RdmaAddress>,
    pub destination: Option<RdmaAddress>,
}

/// A queue pair and, when it is connected to another QP on this host, that peer.
/// Remote fields describe the endpoint's destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RdmaRelation {
    pub id: String,
    pub endpoint: RdmaEndpoint,
    pub peer: Option<RdmaEndpoint>,
    /// Destination QPN of a connected QP; null for datagram types and before connection.
    pub remote_qpn: Option<u32>,
    pub cm: Option<RdmaCmInfo>,
    /// Remote port GID from an IB CM destination or the IPoIB neighbor of an IP destination.
    pub remote_gid: Option<IbGid>,
    /// Reverse DNS of an IP CM destination, filled after collection.
    pub remote_hostname: Option<String>,
    pub label: String,
}

crate::ipc::codec::wire_enum!(RdmaQpType {0=>Smi,1=>Gsi,2=>Rc,3=>Uc,4=>Ud,5=>RawIpv6,6=>RawEthertype,7=>RawPacket,8=>XrcIni,9=>XrcTgt,10=>Driver,11=>Unknown(value)});

crate::ipc::codec::wire_enum!(RdmaQpState {0=>Reset,1=>Init,2=>Rtr,3=>Rts,4=>Sqd,5=>Sqe,6=>Err,7=>Unknown(value)});

crate::ipc::codec::wire_enum!(RdmaCmState {0=>Idle,1=>AddrQuery,2=>AddrResolved,3=>RouteQuery,4=>RouteResolved,5=>Connect,6=>Disconnect,7=>AddrBound,8=>Listen,9=>DeviceRemoval,10=>Destroying,11=>Unknown(value)});

crate::ipc::codec::wire_enum!(RdmaPortSpace {0=>Ipoib,1=>Tcp,2=>Udp,3=>Ib,4=>Unknown(value)});

crate::ipc::codec::wire_enum!(RdmaAddress {0=>Inet{ip,port},1=>Ib{gid,service_id,pkey},2=>Unknown{family}});

crate::ipc::codec::wire_struct!(RdmaPortInfo {
    device,
    port,
    link_layer,
    state,
    lid,
    gid,
    rate
});

crate::ipc::codec::wire_enum!(RdmaEndpointOwner {0=>Process{process_id},1=>Kernel{name},2=>Unknown{pid}});

crate::ipc::codec::wire_struct!(RdmaEndpoint {
    owner,
    device,
    port,
    qpn,
    qp_type,
    state
});

crate::ipc::codec::wire_struct!(RdmaCmInfo {
    id,
    owner,
    state,
    port_space,
    source,
    destination
});

crate::ipc::codec::wire_struct!(RdmaRelation {
    id,
    endpoint,
    peer,
    remote_qpn,
    cm,
    remote_gid,
    remote_hostname,
    label
});

impl crate::ipc::codec::Wire for IbGid {
    fn write(&self, output: &mut crate::ipc::codec::Encoder) -> anyhow::Result<()> {
        crate::ipc::codec::Wire::write(&self.0, output)
    }
    fn read(input: &mut crate::ipc::codec::Decoder<'_>) -> anyhow::Result<Self> {
        Ok(Self(crate::ipc::codec::Wire::read(input)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_shared_display_labels() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../privileged_helper/test_support/rdma-codes.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let code = case["code"].as_u64().unwrap();
            let label = match case["group"].as_str().unwrap() {
                "qp_type" => {
                    let kind = RdmaQpType::from_code(code as u8);
                    kind.to_string()
                }
                "qp_state" => {
                    let kind = RdmaQpState::from_code(code as u8);
                    kind.to_string()
                }
                "cm_state" => {
                    let kind = RdmaCmState::from_code(code as u8);
                    kind.to_string()
                }
                "port_space" => {
                    let kind = RdmaPortSpace::from_code(code as u32);
                    kind.to_string()
                }
                group => panic!("unknown group {group}"),
            };
            assert_eq!(label, case["label"].as_str().unwrap(), "{case}");
        }
    }

    #[test]
    fn gids_round_trip_and_zero_is_unassigned() {
        let gid = IbGid::parse("fe80:0000:0000:0000:0002:c903:0028:45c7").unwrap();
        assert_eq!(gid.to_string(), "fe80:0000:0000:0000:0002:c903:0028:45c7");
        assert!(IbGid::parse("0000:0000:0000:0000:0000:0000:0000:0000").is_none());
        assert!(IbGid::parse("fe80::1").is_none());
        assert!(IbGid::from_slice(&[1; 15]).is_none());
    }

    #[test]
    fn cm_addresses_decode_inet_ib_and_unknown_families() {
        let mut v4 = [0u8; 128];
        v4[0..2].copy_from_slice(&(libc::AF_INET as u16).to_ne_bytes());
        v4[2..4].copy_from_slice(&20049u16.to_be_bytes());
        v4[4..8].copy_from_slice(&[10, 10, 10, 1]);
        let address = RdmaAddress::decode(&v4).unwrap();
        assert_eq!(address.to_string(), "10.10.10.1:20049");
        assert_eq!(
            address,
            RdmaAddress::Inet {
                ip: "10.10.10.1".parse().unwrap(),
                port: 20049,
            }
        );
        let mut v6 = [0u8; 128];
        v6[0..2].copy_from_slice(&(libc::AF_INET6 as u16).to_ne_bytes());
        v6[2..4].copy_from_slice(&4791u16.to_be_bytes());
        v6[8..24].copy_from_slice(&"fe80::1".parse::<std::net::Ipv6Addr>().unwrap().octets());
        assert_eq!(
            RdmaAddress::decode(&v6).unwrap().to_string(),
            "[fe80::1]:4791"
        );
        let mut ib = [0u8; 128];
        ib[0..2].copy_from_slice(&27u16.to_ne_bytes());
        ib[2..4].copy_from_slice(&0xffffu16.to_be_bytes());
        ib[8] = 0xfe;
        ib[9] = 0x80;
        ib[23] = 0x51;
        ib[24..32].copy_from_slice(&0x0000_0000_0106_4e51u64.to_be_bytes());
        assert_eq!(
            RdmaAddress::decode(&ib),
            Some(RdmaAddress::Ib {
                gid: IbGid::parse("fe80:0000:0000:0000:0000:0000:0000:0051").unwrap(),
                service_id: 0x0000_0000_0106_4e51,
                pkey: 65535,
            })
        );
        assert!(RdmaAddress::decode(&[0u8; 128]).is_none());
        assert_eq!(
            RdmaAddress::decode(&[42, 0, 0, 0]),
            Some(RdmaAddress::Unknown { family: 42 })
        );
        assert!(RdmaAddress::decode(&v4[..6]).is_none());
    }
}
