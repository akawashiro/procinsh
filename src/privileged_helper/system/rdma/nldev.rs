//! RDMA netlink (nldev) dumps of devices, queue pairs and connection manager IDs.
//!
//! Resource dumps require a device index and reply with one nested table per
//! message. Resources of processes outside the caller's PID namespace are omitted
//! by the kernel; kernel-owned resources are always reported.

use super::codes::{RdmaAddress, RdmaCmState, RdmaPortSpace, RdmaQpState, RdmaQpType};
use crate::privileged_helper::netlink::{self, NetlinkDumpRequest};
use anyhow::{Context, Result, ensure};
use std::time::Instant;

const NETLINK_RDMA: i32 = 20;
const RDMA_NL_NLDEV: u16 = 5;
const CMD_GET: u16 = 1;
const CMD_RES_QP_GET: u16 = 10;
const CMD_RES_CM_ID_GET: u16 = 11;
const DEV_INDEX: u16 = 1;
const DEV_NAME: u16 = 2;
const PORT_INDEX: u16 = 3;
const RES_QP: u16 = 19;
const RES_QP_ENTRY: u16 = 20;
const RES_LQPN: u16 = 21;
const RES_RQPN: u16 = 22;
const RES_TYPE: u16 = 26;
const RES_STATE: u16 = 27;
const RES_PID: u16 = 28;
const RES_KERN_NAME: u16 = 29;
const RES_CM_ID: u16 = 30;
const RES_CM_ID_ENTRY: u16 = 31;
const RES_PS: u16 = 32;
const RES_SRC_ADDR: u16 = 33;
const RES_DST_ADDR: u16 = 34;
const RES_CM_IDN: u16 = 63;
/// Queue pairs kept per snapshot across all devices.
pub(super) const QUEUE_PAIR_LIMIT: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ResourceOwner {
    /// PID in the collector's PID namespace.
    Process(u32),
    Kernel(String),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct QueuePairEntry {
    pub(super) device: String,
    pub(super) port: Option<u32>,
    pub(super) qpn: u32,
    /// Destination QPN for connected types; zero until the QP is connected.
    pub(super) remote_qpn: Option<u32>,
    pub(super) qp_type: RdmaQpType,
    pub(super) state: RdmaQpState,
    pub(super) owner: ResourceOwner,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CmIdEntry {
    pub(super) device: String,
    pub(super) id: Option<u32>,
    /// QP bound to this CM ID; absent for listeners and unbound IDs.
    pub(super) qpn: Option<u32>,
    pub(super) state: RdmaCmState,
    pub(super) port_space: RdmaPortSpace,
    pub(super) source: Option<RdmaAddress>,
    pub(super) destination: Option<RdmaAddress>,
    /// The CM consumer, such as `rpcrdma`; it can differ from the QP owner.
    pub(super) owner: ResourceOwner,
}

#[derive(Default)]
pub(super) struct NldevResources {
    pub(super) queue_pairs: Vec<QueuePairEntry>,
    pub(super) cm_ids: Vec<CmIdEntry>,
    /// Per-device failures; other devices are still reported.
    pub(super) errors: Vec<String>,
    pub(super) truncated: bool,
}

fn message_type(command: u16) -> u16 {
    (RDMA_NL_NLDEV << 10) + command
}

fn string(data: &[u8]) -> String {
    let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).into_owned()
}

fn u32_value(data: &[u8]) -> Result<u32> {
    netlink::u32_at(data, 0)
}

fn u8_value(data: &[u8]) -> Result<u8> {
    data.first().copied().context("short RDMA attribute")
}

fn owner(pid: Option<u32>, kernel: Option<String>) -> ResourceOwner {
    match (pid, kernel) {
        (_, Some(name)) => ResourceOwner::Kernel(name),
        (Some(pid), None) if pid != 0 => ResourceOwner::Process(pid),
        _ => ResourceOwner::Unknown,
    }
}

/// Device name and the resource entries of one dump message. A message holding
/// the entry attributes at top level is treated as a single entry.
fn entries(body: &[u8], table: u16, entry: u16) -> Result<(String, Vec<&[u8]>)> {
    let mut device = None;
    let mut found = Vec::new();
    let mut flat = false;
    for (kind, data) in netlink::attributes(body)? {
        match kind {
            DEV_NAME => device = Some(string(data)),
            RES_LQPN | RES_PS => flat = true,
            kind if kind == table => {
                for (kind, data) in netlink::attributes(data)? {
                    if kind == entry {
                        found.push(data);
                    }
                }
            }
            _ => {}
        }
    }
    if flat {
        found.push(body);
    }
    Ok((device.context("RDMA resource without device name")?, found))
}

fn parse_queue_pair(device: &str, data: &[u8]) -> Result<QueuePairEntry> {
    let (mut port, mut qpn, mut remote_qpn, mut qp_type, mut state) =
        (None, None, None, None, None);
    let (mut pid, mut kernel) = (None, None);
    for (kind, data) in netlink::attributes(data)? {
        match kind {
            PORT_INDEX => port = Some(u32_value(data)?),
            RES_LQPN => qpn = Some(u32_value(data)?),
            RES_RQPN => remote_qpn = Some(u32_value(data)?),
            RES_TYPE => qp_type = Some(RdmaQpType::from_code(u8_value(data)?)),
            RES_STATE => state = Some(RdmaQpState::from_code(u8_value(data)?)),
            RES_PID => pid = Some(u32_value(data)?),
            RES_KERN_NAME => kernel = Some(string(data)),
            _ => {}
        }
    }
    Ok(QueuePairEntry {
        device: device.to_owned(),
        port: port.filter(|p| *p != 0),
        qpn: qpn.context("RDMA QP without QPN")?,
        // QPN 0 is the SMI QP and never a connected destination.
        remote_qpn: remote_qpn.filter(|q| *q != 0),
        qp_type: qp_type.context("RDMA QP without type")?,
        state: state.context("RDMA QP without state")?,
        owner: owner(pid, kernel),
    })
}

fn parse_cm_id(device: &str, data: &[u8]) -> Result<CmIdEntry> {
    let (mut id, mut qpn, mut state, mut port_space) = (None, None, None, None);
    let (mut source, mut destination) = (None, None);
    let (mut pid, mut kernel) = (None, None);
    for (kind, data) in netlink::attributes(data)? {
        match kind {
            RES_PID => pid = Some(u32_value(data)?),
            RES_KERN_NAME => kernel = Some(string(data)),
            RES_CM_IDN => id = Some(u32_value(data)?),
            RES_LQPN => qpn = Some(u32_value(data)?),
            RES_STATE => state = Some(RdmaCmState::from_code(u8_value(data)?)),
            RES_PS => port_space = Some(RdmaPortSpace::from_code(u32_value(data)?)),
            RES_SRC_ADDR => source = RdmaAddress::decode(data),
            RES_DST_ADDR => destination = RdmaAddress::decode(data),
            _ => {}
        }
    }
    Ok(CmIdEntry {
        device: device.to_owned(),
        id,
        qpn,
        state: state.context("RDMA CM ID without state")?,
        port_space: port_space.context("RDMA CM ID without port space")?,
        source,
        destination,
        owner: owner(pid, kernel),
    })
}

/// Device indexes and names from a `RDMA_NLDEV_CMD_GET` reply body.
fn parse_device(body: &[u8]) -> Result<(u32, String)> {
    let (mut index, mut name) = (None, None);
    for (kind, data) in netlink::attributes(body)? {
        match kind {
            DEV_INDEX => index = Some(u32_value(data)?),
            DEV_NAME => name = Some(string(data)),
            _ => {}
        }
    }
    Ok((
        index.context("RDMA device without index")?,
        name.context("RDMA device without name")?,
    ))
}

/// Reads queue pairs and CM IDs of every RDMA device visible in this network namespace.
/// Whether a dump failed because the device does not implement it.
fn unsupported(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .and_then(std::io::Error::raw_os_error)
        .is_some_and(|code| code == libc::EOPNOTSUPP || code == libc::ENOTSUP)
}

pub(super) fn read_resources(deadline: Instant) -> Result<NldevResources> {
    let fd = netlink::open_socket(NETLINK_RDMA, "NETLINK_RDMA")?;
    let mut sequence = 1;
    let mut devices = Vec::new();
    NetlinkDumpRequest {
        label: "RDMA device dump",
        message_type: message_type(CMD_GET),
        reply_type: message_type(CMD_GET),
        sequence,
        payload: &[],
    }
    .run(&fd, deadline, |body| {
        devices.push(parse_device(body)?);
        Ok(())
    })?;
    devices.sort_by(|a, b| a.1.cmp(&b.1));
    let mut result = NldevResources::default();
    for (index, name) in devices {
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&8u16.to_ne_bytes());
        payload.extend_from_slice(&DEV_INDEX.to_ne_bytes());
        payload.extend_from_slice(&index.to_ne_bytes());
        for (command, label) in [
            (CMD_RES_QP_GET, "RDMA QP dump"),
            (CMD_RES_CM_ID_GET, "RDMA CM ID dump"),
        ] {
            sequence += 1;
            let dumped = NetlinkDumpRequest {
                label,
                message_type: message_type(command),
                reply_type: message_type(command),
                sequence,
                payload: &payload,
            }
            .run(&fd, deadline, |body| {
                if command == CMD_RES_QP_GET {
                    let (device, list) = entries(body, RES_QP, RES_QP_ENTRY)?;
                    for data in list {
                        if result.queue_pairs.len() >= QUEUE_PAIR_LIMIT {
                            result.truncated = true;
                            break;
                        }
                        result.queue_pairs.push(parse_queue_pair(&device, data)?);
                    }
                } else {
                    let (device, list) = entries(body, RES_CM_ID, RES_CM_ID_ENTRY)?;
                    for data in list {
                        result.cm_ids.push(parse_cm_id(&device, data)?);
                    }
                }
                Ok(())
            });
            if let Err(error) = dumped {
                ensure!(Instant::now() < deadline, "{error:#}");
                // Some drivers, such as Microsoft MANA, cannot report queue pairs or
                // CM IDs; that is an empty table, not a failed observation.
                if unsupported(&error) {
                    log::debug!("{name}: {error:#}");
                } else {
                    result.errors.push(format!("{name}: {error:#}"));
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    #[allow(clippy::wildcard_imports)]
    use super::*;

    #[test]
    fn unsupported_dumps_are_recognized_through_context() {
        use anyhow::Context;
        let failed = |code| -> anyhow::Error {
            Err::<(), _>(std::io::Error::from_raw_os_error(code))
                .context("RDMA QP dump dump failed")
                .unwrap_err()
        };
        assert!(unsupported(&failed(libc::EOPNOTSUPP)));
        assert!(!unsupported(&failed(libc::EPERM)));
        assert!(!unsupported(&anyhow::anyhow!(
            "truncated RDMA QP dump packet"
        )));
    }

    fn attribute(kind: u16, data: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&((4 + data.len()) as u16).to_ne_bytes());
        bytes.extend_from_slice(&kind.to_ne_bytes());
        bytes.extend_from_slice(data);
        bytes.resize((bytes.len() + 3) & !3, 0);
        bytes
    }

    fn queue_pair(fields: &[(u16, Vec<u8>)]) -> Vec<u8> {
        fields
            .iter()
            .flat_map(|(kind, data)| attribute(*kind, data))
            .collect()
    }

    #[test]
    fn nested_qp_tables_keep_owners_and_unknown_codes() {
        let user = queue_pair(&[
            (PORT_INDEX, 1u32.to_ne_bytes().to_vec()),
            (RES_LQPN, 524u32.to_ne_bytes().to_vec()),
            (RES_RQPN, 650u32.to_ne_bytes().to_vec()),
            (RES_TYPE, vec![2]),
            (RES_STATE, vec![3]),
            (RES_PID, 4242u32.to_ne_bytes().to_vec()),
        ]);
        let kernel = queue_pair(&[
            (RES_LQPN, 1u32.to_ne_bytes().to_vec()),
            (RES_RQPN, 0u32.to_ne_bytes().to_vec()),
            (RES_TYPE, vec![77]),
            (RES_STATE, vec![99]),
            (RES_KERN_NAME, b"ib_core\0".to_vec()),
        ]);
        let mut table = attribute(RES_QP_ENTRY, &user);
        table.extend(attribute(RES_QP_ENTRY, &kernel));
        let mut body = attribute(DEV_INDEX, &0u32.to_ne_bytes());
        body.extend(attribute(DEV_NAME, b"mlx4_0\0"));
        body.extend(attribute(0x8000 | RES_QP, &table));
        let (device, list) = entries(&body, RES_QP, RES_QP_ENTRY).unwrap();
        assert_eq!(device, "mlx4_0");
        let parsed: Vec<_> = list
            .iter()
            .map(|data| parse_queue_pair(&device, data).unwrap())
            .collect();
        assert_eq!(
            parsed[0],
            QueuePairEntry {
                device: "mlx4_0".into(),
                port: Some(1),
                qpn: 524,
                remote_qpn: Some(650),
                qp_type: RdmaQpType::Rc,
                state: RdmaQpState::Rts,
                owner: ResourceOwner::Process(4242),
            }
        );
        assert_eq!(parsed[1].port, None);
        assert_eq!(parsed[1].remote_qpn, None);
        assert_eq!(parsed[1].qp_type, RdmaQpType::Unknown(77));
        assert_eq!(parsed[1].state, RdmaQpState::Unknown(99));
        assert_eq!(parsed[1].owner, ResourceOwner::Kernel("ib_core".into()));
    }

    #[test]
    fn flat_cm_ids_decode_addresses_and_missing_fields_fail() {
        let mut destination = [0u8; 128];
        destination[0..2].copy_from_slice(&(libc::AF_INET as u16).to_ne_bytes());
        destination[2..4].copy_from_slice(&20049u16.to_be_bytes());
        destination[4..8].copy_from_slice(&[10, 10, 10, 1]);
        let mut body = attribute(DEV_NAME, b"ibp133s0\0");
        body.extend(queue_pair(&[
            (RES_LQPN, 524u32.to_ne_bytes().to_vec()),
            (RES_TYPE, vec![2]),
            (RES_PS, 0x106u32.to_ne_bytes().to_vec()),
            (RES_STATE, vec![5]),
            (RES_DST_ADDR, destination.to_vec()),
            (RES_CM_IDN, 0u32.to_ne_bytes().to_vec()),
            (RES_KERN_NAME, b"rpcrdma\0".to_vec()),
        ]));
        let (device, list) = entries(&body, RES_CM_ID, RES_CM_ID_ENTRY).unwrap();
        let cm = parse_cm_id(&device, list[0]).unwrap();
        assert_eq!(cm.device, "ibp133s0");
        assert_eq!(cm.qpn, Some(524));
        assert_eq!(cm.id, Some(0));
        assert_eq!(cm.state, RdmaCmState::Connect);
        assert_eq!(cm.port_space, RdmaPortSpace::Tcp);
        assert_eq!(cm.source, None);
        assert_eq!(cm.destination.unwrap().to_string(), "10.10.10.1:20049");
        assert_eq!(cm.owner, ResourceOwner::Kernel("rpcrdma".into()));
        assert!(parse_cm_id("x", &attribute(RES_STATE, &[5])).is_err());
        assert!(parse_queue_pair("x", &attribute(RES_LQPN, &[1, 0, 0, 0])).is_err());
        assert!(entries(&attribute(RES_LQPN, &[1, 0, 0, 0]), RES_QP, RES_QP_ENTRY).is_err());
        assert_eq!(
            parse_device(
                &[
                    attribute(DEV_INDEX, &3u32.to_ne_bytes()),
                    attribute(DEV_NAME, b"rxe0\0")
                ]
                .concat()
            )
            .unwrap(),
            (3, "rxe0".into())
        );
    }
}
