//! Socket observations used by acquisition and main-side relation construction.

//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`SocketInfo`] | `pub` | `struct SocketInfo` |

use crate::http_server::model::socket_types::{SocketProtocol, SocketState};
pub use crate::ipc::model::sockets::*;
use std::net::SocketAddr;
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::sockets::SocketInfo")]
/// Socket kind and independently observed connection metadata.
/// A known protocol does not imply a known state, address or peer.
struct SocketInfoJson {
    pub protocol: SocketProtocol,
    pub state: Option<SocketState>,
    pub local: Option<SocketAddr>,
    pub remote: Option<SocketAddr>,
    pub path: Option<String>,
    pub peer_inode: Option<u64>,
}
impl serde::Serialize for crate::http_server::model::sockets::SocketInfo {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SocketInfoJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::sockets::SocketInfo {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SocketInfoJson::deserialize(d)
    }
}
