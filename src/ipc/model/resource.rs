//! Typed identities shared by process discovery and activity collection.
//!
//! Device and inode identities are transferred as native integer values.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`DeviceId`] | `pub(crate)` | `struct DeviceId` |
//! | [`IpcKind`] | `pub(crate)` | `enum IpcKind` |
//! | [`IpcIdentity`] | `pub(crate)` | `struct IpcIdentity` |
//! | [`FileIdentity`] | `pub(crate)` | `struct FileIdentity` |
//! | [`device_id_from_stat`] | `pub(crate)` | `fn device_id_from_stat(device: u64) -> `[`DeviceId`] |
//! | [`device_id_from_kernel`] | `pub(crate)` | `fn device_id_from_kernel(device: u64) -> `[`DeviceId`] |

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DeviceId {
    pub(crate) major: u32,
    pub(crate) minor: u32,
}
pub(crate) fn device_id_from_stat(device: u64) -> DeviceId {
    DeviceId {
        major: libc::major(device),
        minor: libc::minor(device),
    }
}

pub(crate) fn device_id_from_kernel(device: u64) -> DeviceId {
    DeviceId {
        major: (device >> 20) as u32,
        minor: (device & ((1 << 20) - 1)) as u32,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum IpcKind {
    Pipe,
    Socket,
}
impl fmt::Display for IpcKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pipe => "pipe",
            Self::Socket => "socket",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct IpcIdentity {
    pub(crate) kind: IpcKind,
    pub(crate) device: DeviceId,
    pub(crate) inode: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FileIdentity {
    pub(crate) device: DeviceId,
    pub(crate) inode: u64,
    pub(crate) generation: u32,
}

crate::ipc::codec::wire_struct!(DeviceId { major, minor });

crate::ipc::codec::wire_enum!(IpcKind{0=>Pipe,1=>Socket});

crate::ipc::codec::wire_struct!(IpcIdentity {
    kind,
    device,
    inode
});

crate::ipc::codec::wire_struct!(FileIdentity {
    device,
    inode,
    generation
});
