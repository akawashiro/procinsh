//! Multiple hosts: a hub accepts agent links and serves each agent's processes,
//! process details and SPACE observations; an agent links to a hub and answers
//! the hub's requests with its own privileged helper.
//!
//! The hub sends the agent the same closed helper requests it sends its own
//! helper, so remote hosts need no separate API. Agent links use length-prefixed
//! frames over TCP without TLS. A hub on a non-loopback address requires a shared
//! token. Agents collect only while the hub has viewers for them.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`run_hub_connector`] | `pub(super)` | `async fn run_hub_connector( address: `[`String`]`, info: `[`HostInfo`](identity::HostInfo)`, token: `[`Option`]`<`[`Arc`](std::sync::Arc)`<str>>, client: `[`MonitorClient`](crate::http_server::client::MonitorClient)`, mut shutdown: `[`watch::Receiver`](tokio::sync::watch::Receiver)`<bool>, )` |
//! | [`correlate_host_connections`] | `pub(super)` | `fn correlate_host_connections( hosts: &[(&str, &`[`Value`](serde_json::Value)`)], ) -> `[`CrossHostLinks`](correlate::CrossHostLinks) |
//! | [`ALL_HOSTS`] | `pub(super)` | `const ALL_HOSTS: &str` | //! | [`valid_host_name`] | `pub(crate)` | `fn valid_host_name(name: &str) -> bool` | //! | [`protect_remote_host_info`] | `pub(super)` | `fn protect_remote_host_info(state: &mut `[`HostInfo`]`)` | //! | [`local_host_info`] | `pub(in crate::http_server)` | `fn local_host_info(name: `[`Option`]`<`[`String`]`>) -> `[`HostInfo`] | use serde::Serialize` | //! | [`valid_host_name`] | `pub(crate)` | `fn valid_host_name(name: &str) -> bool` | //! | [`protect_remote_host_info`] | `pub(super)` | `fn protect_remote_host_info(state: &mut `[`HostInfo`]`)` | //! | [`local_host_info`] | `pub(in crate::http_server)` | `fn local_host_info(name: `[`Option`]`<`[`String`]`>) -> `[`HostInfo`] | use serde::Serialize` | //! | [`valid_host_name`] | `pub(crate)` | `fn valid_host_name(name: &str) -> bool` | //! | [`protect_remote_host_info`] | `pub(super)` | `fn protect_remote_host_info(state: &mut `[`HostInfo`]`)` | //! | [`local_host_info`] | `pub(in crate::http_server)` | `fn local_host_info(name: `[`Option`]`<`[`String`]`>) -> `[`HostInfo`] | use serde::Serialize` |
//! | [`valid_host_name`] | `pub(crate)` | `fn valid_host_name(name: &str) -> bool` |
//! | [`AgentLink`] | `pub(super)` | `struct AgentLink` |
//! | [`run_agent_listener`] | `pub(super)` | `async fn run_agent_listener( listener: `[`TcpListener`](tokio::net::TcpListener)`, registry: `[`Arc`](std::sync::Arc)`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`](std::sync::Arc)`<str>>, mut shutdown: `[`watch::Receiver`](tokio::sync::watch::Receiver)`<bool>, )` |
//! | [`HostRegistry`] | `pub(super)` | `struct HostRegistry` |
//! | [`RemoteHost`] | `pub(super)` | `enum RemoteHost` |
//! | [`local_host_info`] | `pub(in crate::http_server)` | `fn local_host_info(name: `[`Option`]`<`[`String`]`>) -> `[`HostInfo`](identity::HostInfo) |
//! | [`close_session_agent_link`] | `pub(in crate::http_server)` | `fn close_session_agent_link(state: &`[`AgentLink`]`, session: u64)` |
//! | [`request_agent_link`] | `pub(in crate::http_server)` | `async fn request_agent_link( state: &`[`AgentLink`]`, request: `[`MonitorRequest`](crate::ipc::schema::MonitorRequest)`, ) -> `[`Result`](anyhow::Result)`<`[`MonitorResponse`](crate::http_server::protocol::MonitorResponse)`>` |
//! | [`changes_host_registry`] | `pub(in crate::http_server)` | `fn changes_host_registry(state: &`[`HostRegistry`]`) -> `[`watch::Receiver`](tokio::sync::watch::Receiver)`<u64>` |
//! | [`create_host_registry`] | `pub(in crate::http_server)` | `fn create_host_registry(local: `[`HostInfo`](identity::HostInfo)`) -> `[`HostRegistry`] |
//! | [`local_host_registry`] | `pub(in crate::http_server)` | `fn local_host_registry(state: &`[`HostRegistry`]`) -> &`[`HostInfo`](identity::HostInfo) |
//! | [`remote_host_registry`] | `pub(in crate::http_server)` | `fn remote_host_registry(state: &`[`HostRegistry`]`, name: &str) -> `[`RemoteHost`] |
//! | [`online_host_registry`] | `pub(in crate::http_server)` | `fn online_host_registry( state: &`[`HostRegistry`]`, ) -> `[`Vec`]`<(`[`String`]`, `[`Arc`](std::sync::Arc)`<`[`AgentLink`]`>)>` |
//! | [`summaries_host_registry`] | `pub(in crate::http_server)` | `fn summaries_host_registry(state: &`[`HostRegistry`]`) -> `[`Vec`]`<`[`HostSummary`](registry::HostSummary)`>` |

mod connector;
mod correlate;
mod identity;
mod link;
mod listener;
mod registry;
mod wire;
pub(super) use connector::run_hub_connector;
pub(super) use correlate::correlate_host_connections;
pub(super) use identity::ALL_HOSTS;
pub(crate) use identity::valid_host_name;
pub(super) use link::AgentLink;
pub(super) use listener::run_agent_listener;
pub(super) use registry::{HostRegistry, RemoteHost};

pub(in crate::http_server) use identity::local_host_info;

pub(in crate::http_server) use link::{close_session_agent_link, request_agent_link};

pub(in crate::http_server) use registry::{
    changes_host_registry, create_host_registry, local_host_registry, online_host_registry,
    remote_host_registry, summaries_host_registry,
};

#[cfg(test)]
pub(super) use identity::HostInfo;
