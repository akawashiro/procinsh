//! Typed identities shared by process discovery and activity collection.
//!
//! File and IPC inodes serialize as decimal strings to preserve integer precision.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`deserialize_decimal`] | `pub` | `fn deserialize_decimal<'de, D: serde::Deserializer<'de>>( deserializer: D, ) -> Result<u64, D::Error>` |
//! | [`DeviceId`] | `pub` | `struct DeviceId` |
//! | [`decimal`] | `pub` | `fn decimal<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>` |
//! | [`IpcKind`] | `pub` | `enum IpcKind` |
//! | [`IpcIdentity`] | `pub` | `struct IpcIdentity` |
//! | [`FileIdentity`] | `pub` | `struct FileIdentity` |

#[allow(unused_imports, reason = "Preserve the complete façade interface")]
pub use crate::ipc::model::resource::{DeviceId, FileIdentity, IpcIdentity, IpcKind};
use serde::Deserialize;

pub fn deserialize_decimal<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u64, D::Error> {
    let value = String::deserialize(deserializer)?;
    value.parse().map_err(serde::de::Error::custom)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::resource::DeviceId")]
struct DeviceIdJson {
    pub major: u32,
    pub minor: u32,
}
impl serde::Serialize for crate::http_server::model::resource::DeviceId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        DeviceIdJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::resource::DeviceId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        DeviceIdJson::deserialize(d)
    }
}

pub fn decimal<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::resource::IpcKind")]
#[serde(rename_all = "snake_case")]
enum IpcKindJson {
    Pipe,
    Socket,
}
impl serde::Serialize for crate::http_server::model::resource::IpcKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        IpcKindJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::resource::IpcKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        IpcKindJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::resource::IpcIdentity")]
struct IpcIdentityJson {
    pub kind: IpcKind,
    pub device: DeviceId,
    #[serde(serialize_with = "decimal", deserialize_with = "deserialize_decimal")]
    pub inode: u64,
}
impl serde::Serialize for crate::http_server::model::resource::IpcIdentity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        IpcIdentityJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::resource::IpcIdentity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        IpcIdentityJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::resource::FileIdentity")]
struct FileIdentityJson {
    pub device: DeviceId,
    #[serde(serialize_with = "decimal", deserialize_with = "deserialize_decimal")]
    pub inode: u64,
    pub generation: u32,
}
impl serde::Serialize for crate::http_server::model::resource::FileIdentity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FileIdentityJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::resource::FileIdentity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        FileIdentityJson::deserialize(d)
    }
}

#[cfg(test)]
mod tests {
    #[allow(clippy::wildcard_imports)]
    use super::*;

    #[test]
    fn devices_and_large_inodes_keep_identity() {
        let device = DeviceId {
            major: 259,
            minor: 1048575,
        };
        assert_eq!(
            DeviceId::from_stat(libc::makedev(device.major, device.minor)),
            device
        );
        assert_eq!(
            DeviceId::from_kernel(((device.major as u64) << 20) | device.minor as u64),
            device
        );
        let file = FileIdentity {
            device,
            inode: u64::MAX,
            generation: 42,
        };
        assert_eq!(
            serde_json::to_value(file).unwrap()["inode"],
            u64::MAX.to_string()
        );
        let ipc = IpcIdentity {
            kind: IpcKind::Socket,
            device,
            inode: u64::MAX,
        };
        assert_eq!(
            serde_json::to_value(ipc).unwrap()["inode"],
            u64::MAX.to_string()
        );
    }
}
