//! HTTP server wiring and shared API.
//!
//! # Interface
//!
//! | Definition | Visibility | Signature |
//! | --- | --- | --- |
//! | [`run`] | `pub(super)` | `async fn run(listen: `[`SocketAddr`](std::net::SocketAddr)`) -> `[`anyhow::Result`]`<()>` |
//!
//! Shared identities (all `pub(super)`, fields also `pub(super)`):
//! - [`resource::DeviceId`]: `major: u32, minor: u32`;
//!   [`resource::DeviceId::from_stat`]: `fn from_stat(device: u64) -> Self`;
//!   [`resource::DeviceId::from_kernel`]: `fn from_kernel(device: u64) -> Self`.
//! - [`resource::FileIdentity`]: `device: DeviceId, inode: u64, generation: u32`.
//! - [`resource::IpcIdentity`]: `kind: IpcKind, device: DeviceId, inode: u64`.
//! - [`resource::IpcKind`]: `Pipe, Socket`.
//! - [`resource::decimal`]: `fn decimal<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>`.
//!
//! Inodes serialize as decimal strings.
//!
//! Shared socket types (all `pub(super)`, fields also `pub(super)`) from [`socket_types`]:
//! - [`socket_types::AddressFamily`]: `Ipv4, Ipv6`.
//! - [`socket_types::SocketType`]: `Stream, Dgram, Seqpacket, Unknown(u32)`;
//!   [`socket_types::SocketType::from_code`]: `fn from_code(code: u32) -> Self`.
//! - [`socket_types::SocketProtocol`]: `Tcp { family: AddressFamily }, Udp { family: AddressFamily }, Unix { socket_type: SocketType }`;
//!   [`socket_types::SocketProtocol::is_tcp`]: `fn is_tcp(self) -> bool`;
//!   [`socket_types::SocketProtocol::is_inet`]: `fn is_inet(self) -> bool`.
//! - [`socket_types::SocketState`]: `Established, SynSent, SynRecv, FinWait1, FinWait2, TimeWait, Close, CloseWait, LastAck, Listen, Closing, NewSynRecv, Unconnected, Connecting, Connected, Disconnecting, UnknownInet(u32), UnknownUnix(u32)`;
//!   [`socket_types::SocketState::inet`]: `fn inet(code: u32) -> Self`;
//!   [`socket_types::SocketState::unix_proc`]: `fn unix_proc(code: u32, listening: bool) -> Self`;
//!   [`socket_types::SocketState::unix_diag`]: `fn unix_diag(code: u32) -> Self`.
//! - [`socket_types::FdKind`]: `Pipe, Socket, Fifo`.
//! - [`socket_types::FdAccess`]: `Read, Write, ReadWrite, Unknown`;
//!   [`socket_types::FdAccess::from_flags`]: `fn from_flags(flags: u32) -> Self`;
//!   [`socket_types::FdAccess::from_mode`]: `fn from_mode(mode: Option<u32>) -> Self`;
//!   [`socket_types::FdAccess::opposite`]: `fn opposite(self, other: Self) -> bool`.
//! - [`socket_types::InetAddress`]: `ip: std::net::IpAddr, port: u16`, with `From<std::net::SocketAddr>`.
//!
//! Shared state: [`state::AppState::system_monitor`] is
//! `pub(super) system_monitor: Arc<system::SystemMonitor>`
//! ([`system::SystemMonitor`]).
//!
mod api;
mod middleware;
mod process;
mod resource;
mod router;
mod server;
mod socket_types;
mod state;
mod system;
mod web;
pub(super) use server::run;
use state::AppState;
#[cfg(test)]
mod process_api_tests;
#[cfg(test)]
mod system_api_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
