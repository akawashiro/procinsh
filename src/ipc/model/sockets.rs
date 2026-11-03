//! Socket observations used by acquisition and main-side relation construction.

//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`SocketInfo`] | `pub` | `struct SocketInfo` |

use crate::ipc::model::socket_types::{SocketConnectionStatus, SocketProtocol, SocketState};
use std::net::SocketAddr;
#[derive(Clone, Debug)]
/// Socket kind and independently observed connection metadata.
/// A known protocol does not imply a known state, address or peer.
pub struct SocketInfo {
    pub protocol: SocketProtocol,
    pub state: Option<SocketState>,
    pub local: Option<SocketAddr>,
    pub remote: Option<SocketAddr>,
    pub path: Option<String>,
    pub peer_inode: Option<u64>,
}

impl SocketInfo {
    /// Preserve direct observations; table fallbacks only confirm positive facts.
    /// CLOSE, zero addresses, and UNIX state names never prove ENOTCONN.
    pub fn connection_status(&self, observed: SocketConnectionStatus) -> SocketConnectionStatus {
        if observed != SocketConnectionStatus::Unknown {
            observed
        } else if self.state == Some(SocketState::Listen) {
            SocketConnectionStatus::Listening
        } else if self.peer_inode.is_some()
            || (self.protocol.is_tcp_or_udp()
                && self
                    .remote
                    .is_some_and(|remote| remote.port() != 0 && !remote.ip().is_unspecified()))
        {
            SocketConnectionStatus::PeerPresent
        } else {
            SocketConnectionStatus::Unknown
        }
    }
}

crate::ipc::codec::wire_struct!(SocketInfo {
    protocol,
    state,
    local,
    remote,
    path,
    peer_inode
});
