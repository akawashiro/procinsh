//! Socket observations used by acquisition and main-side relation construction.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`SocketInfo`] | `pub(crate)` | `struct SocketInfo` |
//! | [`connection_status_socket_info`] | `pub(crate)` | `fn connection_status_socket_info( info: &`[`SocketInfo`]`, observed: `[`SocketConnectionStatus`]`, ) -> `[`SocketConnectionStatus`] |

use crate::ipc::model::socket_types::{SocketConnectionStatus, SocketProtocol, SocketState};
use std::net::SocketAddr;
#[derive(Clone, Debug)]
/// Socket kind and independently observed connection metadata.
/// A known protocol does not imply a known state, address or peer.
pub(crate) struct SocketInfo {
    pub(crate) protocol: SocketProtocol,
    pub(crate) state: Option<SocketState>,
    pub(crate) local: Option<SocketAddr>,
    pub(crate) remote: Option<SocketAddr>,
    pub(crate) path: Option<String>,
    pub(crate) peer_inode: Option<u64>,
}

/// Preserve direct observations; table fallbacks only confirm positive facts.
/// CLOSE, zero addresses, and UNIX state names never prove ENOTCONN.
pub(crate) fn connection_status_socket_info(
    info: &SocketInfo,
    observed: SocketConnectionStatus,
) -> SocketConnectionStatus {
    use crate::ipc::model::socket_types::socket_protocol_is_tcp_or_udp;

    if observed != SocketConnectionStatus::Unknown {
        observed
    } else if info.state == Some(SocketState::Listen) {
        SocketConnectionStatus::Listening
    } else if info.peer_inode.is_some()
        || (socket_protocol_is_tcp_or_udp(info.protocol)
            && info
                .remote
                .is_some_and(|remote| remote.port() != 0 && !remote.ip().is_unspecified()))
    {
        SocketConnectionStatus::PeerPresent
    } else {
        SocketConnectionStatus::Unknown
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
