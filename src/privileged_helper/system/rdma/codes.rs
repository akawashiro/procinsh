//! Shared RDMA codes and their acquisition contract tests.

pub(super) use crate::ipc::rdma::*;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_shared_display_contract() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../test_support/rdma-codes.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let code = case["code"].as_u64().unwrap();
            let (value, label) = match case["group"].as_str().unwrap() {
                "qp_type" => {
                    let kind = RdmaQpType::from_code(code as u8);
                    (serde_json::to_value(kind).unwrap(), kind.to_string())
                }
                "qp_state" => {
                    let kind = RdmaQpState::from_code(code as u8);
                    (serde_json::to_value(kind).unwrap(), kind.to_string())
                }
                "cm_state" => {
                    let kind = RdmaCmState::from_code(code as u8);
                    (serde_json::to_value(kind).unwrap(), kind.to_string())
                }
                "port_space" => {
                    let kind = RdmaPortSpace::from_code(code as u32);
                    (serde_json::to_value(kind).unwrap(), kind.to_string())
                }
                group => panic!("unknown group {group}"),
            };
            assert_eq!(value, case["value"], "{case}");
            assert_eq!(label, case["label"].as_str().unwrap(), "{case}");
        }
    }

    #[test]
    fn gids_round_trip_and_zero_is_unassigned() {
        let gid = IbGid::parse("fe80:0000:0000:0000:0002:c903:0028:45c7").unwrap();
        assert_eq!(gid.to_string(), "fe80:0000:0000:0000:0002:c903:0028:45c7");
        assert_eq!(
            serde_json::to_value(gid).unwrap(),
            "fe80:0000:0000:0000:0002:c903:0028:45c7"
        );
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
            serde_json::to_value(address).unwrap(),
            serde_json::json!({"kind":"inet","ip":"10.10.10.1","port":20049})
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
            serde_json::to_value(RdmaAddress::decode(&ib).unwrap()).unwrap(),
            serde_json::json!({
                "kind": "ib",
                "gid": "fe80:0000:0000:0000:0000:0000:0000:0051",
                "service_id": "0x0000000001064e51",
                "pkey": 65535,
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
