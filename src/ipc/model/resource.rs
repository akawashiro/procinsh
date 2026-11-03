//! Typed identities shared by process discovery and activity collection.

//!
//! Device and inode identities are transferred as native integer values.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`DeviceId`] | `pub` | `struct DeviceId` |
//! | [`IpcKind`] | `pub` | `enum IpcKind` |
//! | [`IpcIdentity`] | `pub` | `struct IpcIdentity` |
//! | [`FileIdentity`] | `pub` | `struct FileIdentity` |

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceId {
    pub major: u32,
    pub minor: u32,
}
impl DeviceId {
    pub fn from_stat(device: u64) -> Self {
        Self {
            major: libc::major(device),
            minor: libc::minor(device),
        }
    }

    pub fn from_kernel(device: u64) -> Self {
        Self {
            major: (device >> 20) as u32,
            minor: (device & ((1 << 20) - 1)) as u32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IpcKind {
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
pub struct IpcIdentity {
    pub kind: IpcKind,
    pub device: DeviceId,
    pub inode: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    pub device: DeviceId,
    pub inode: u64,
    pub generation: u32,
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
