//! JSON representations of native RDMA observations.
//!
//! # Interface
//! Installs serde implementations; no named items are exported.
use crate::ipc::rdma::*;
use crate::ipc::schema::ProcessId;
use std::net::IpAddr;

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::rdma::RdmaQpType")]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
enum RdmaQpTypeJson {
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
impl serde::Serialize for RdmaQpType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaQpTypeJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RdmaQpType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaQpTypeJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::rdma::RdmaQpState")]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
enum RdmaQpStateJson {
    Reset,
    Init,
    Rtr,
    Rts,
    Sqd,
    Sqe,
    Err,
    Unknown(u8),
}
impl serde::Serialize for RdmaQpState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaQpStateJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RdmaQpState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaQpStateJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::rdma::RdmaCmState")]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
enum RdmaCmStateJson {
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
impl serde::Serialize for RdmaCmState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaCmStateJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RdmaCmState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaCmStateJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::rdma::RdmaPortSpace")]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
enum RdmaPortSpaceJson {
    Ipoib,
    Tcp,
    Udp,
    Ib,
    Unknown(u32),
}
impl serde::Serialize for RdmaPortSpace {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaPortSpaceJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RdmaPortSpace {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaPortSpaceJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::rdma::RdmaAddress")]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RdmaAddressJson {
    Inet {
        ip: IpAddr,
        port: u16,
    },
    /// `struct sockaddr_ib`; the 64-bit service ID is a hexadecimal string.
    Ib {
        gid: IbGid,
        #[serde(serialize_with = "hex", deserialize_with = "unhex")]
        service_id: u64,
        pkey: u16,
    },
    Unknown {
        family: u16,
    },
}
impl serde::Serialize for RdmaAddress {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaAddressJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RdmaAddress {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaAddressJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::rdma::RdmaPortInfo")]
struct RdmaPortInfoJson {
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
impl serde::Serialize for RdmaPortInfo {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaPortInfoJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RdmaPortInfo {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaPortInfoJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::rdma::RdmaEndpointOwner")]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RdmaEndpointOwnerJson {
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
impl serde::Serialize for RdmaEndpointOwner {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaEndpointOwnerJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RdmaEndpointOwner {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaEndpointOwnerJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::rdma::RdmaEndpoint")]
struct RdmaEndpointJson {
    pub owner: RdmaEndpointOwner,
    pub device: String,
    /// Null until the QP is bound to a port.
    pub port: Option<u32>,
    pub qpn: u32,
    pub qp_type: RdmaQpType,
    pub state: RdmaQpState,
}
impl serde::Serialize for RdmaEndpoint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaEndpointJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RdmaEndpoint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaEndpointJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::rdma::RdmaCmInfo")]
struct RdmaCmInfoJson {
    pub id: Option<u32>,
    /// The CM consumer, which can differ from the QP owner (for example
    /// `rpcrdma` uses QPs created by `rdma_cm`).
    pub owner: RdmaEndpointOwner,
    pub state: RdmaCmState,
    pub port_space: RdmaPortSpace,
    pub source: Option<RdmaAddress>,
    pub destination: Option<RdmaAddress>,
}
impl serde::Serialize for RdmaCmInfo {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaCmInfoJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RdmaCmInfo {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaCmInfoJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::rdma::RdmaRelation")]
struct RdmaRelationJson {
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
impl serde::Serialize for RdmaRelation {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaRelationJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RdmaRelation {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaRelationJson::deserialize(d)
    }
}

impl serde::Serialize for IbGid {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}
impl<'de> serde::Deserialize<'de> for IbGid {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(d)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("invalid IB GID"))
    }
}

fn hex<S: serde::Serializer>(value: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(&format_args!("0x{value:016x}"))
}

fn unhex<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let s = <String as serde::Deserialize>::deserialize(d)?;
    u64::from_str_radix(s.trim_start_matches("0x"), 16).map_err(serde::de::Error::custom)
}
