//! Typed identities shared by process discovery and activity collection.
//!
//! File and IPC inodes serialize as decimal strings to preserve integer precision.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`deserialize_decimal`] | `pub(crate)` | `fn deserialize_decimal<'de, D: `[`serde::Deserializer`]`<'de>>( deserializer: D, ) -> `[`Result`](anyhow::Result)`<u64, D::Error>` |
//! | [`decimal`] | `pub(crate)` | `fn decimal<S: `[`serde::Serializer`]`>(value: &u64, serializer: S) -> `[`Result`](anyhow::Result)`<S::Ok, S::Error>` |

use crate::ipc::model::resource::{DeviceId, IpcKind};
use serde::Deserialize;

pub(crate) fn deserialize_decimal<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u64, D::Error> {
    let value = String::deserialize(deserializer)?;
    value.parse().map_err(serde::de::Error::custom)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::resource::DeviceId")]
struct DeviceIdJson {
    major: u32,
    minor: u32,
}
impl serde::Serialize for crate::ipc::model::resource::DeviceId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        DeviceIdJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::resource::DeviceId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        DeviceIdJson::deserialize(d)
    }
}

pub(crate) fn decimal<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::resource::IpcKind")]
#[serde(rename_all = "snake_case")]
enum IpcKindJson {
    Pipe,
    Socket,
}
impl serde::Serialize for crate::ipc::model::resource::IpcKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        IpcKindJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::resource::IpcKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        IpcKindJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::resource::IpcIdentity")]
struct IpcIdentityJson {
    kind: IpcKind,
    device: DeviceId,
    #[serde(serialize_with = "decimal", deserialize_with = "deserialize_decimal")]
    inode: u64,
}
impl serde::Serialize for crate::ipc::model::resource::IpcIdentity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        IpcIdentityJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::resource::IpcIdentity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        IpcIdentityJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::resource::FileIdentity")]
struct FileIdentityJson {
    device: DeviceId,
    #[serde(serialize_with = "decimal", deserialize_with = "deserialize_decimal")]
    inode: u64,
    generation: u32,
}
impl serde::Serialize for crate::ipc::model::resource::FileIdentity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FileIdentityJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::resource::FileIdentity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        FileIdentityJson::deserialize(d)
    }
}

#[cfg(test)]
mod tests {
    use crate::ipc::model::resource::{DeviceId, FileIdentity, IpcIdentity, IpcKind};

    #[test]
    fn devices_and_large_inodes_keep_identity() {
        use crate::ipc::model::resource::{device_id_from_kernel, device_id_from_stat};

        let device = DeviceId {
            major: 259,
            minor: 1048575,
        };
        assert_eq!(
            device_id_from_stat(libc::makedev(device.major, device.minor)),
            device
        );
        assert_eq!(
            device_id_from_kernel(((device.major as u64) << 20) | device.minor as u64),
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
