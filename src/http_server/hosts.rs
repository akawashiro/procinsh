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
//! | [`HostRegistry`] | `pub(super)` | `struct HostRegistry` |
//! | [`HostInfo`] | `pub(super)` | `struct HostInfo` |
//! | [`RemoteHost`] | `pub(super)` | `enum RemoteHost` |
//! | [`AgentLink`] | `pub(super)` | `struct AgentLink` |
//! | [`correlate`](fn@correlate) | `pub(super)` | `fn correlate(hosts: &[(&`[`str`]`, &`[`Value`](serde_json::Value)`)]) -> `[`correlate::CrossHostLinks`] |
//! | [`ALL_HOSTS`] | `pub(super)` | `const ALL_HOSTS: &str = "all"` |
//! | [`valid_host_name`] | `pub(crate)` | `fn valid_host_name(name: &`[`str`]`) -> `[`bool`] |
//! | [`run_agent_listener`] | `pub(super)` | `async fn run_agent_listener(listener: `[`TcpListener`](tokio::net::TcpListener)`, registry: `[`Arc`](std::sync::Arc)`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`](std::sync::Arc)`<`[`str`]`>>, shutdown: `[`watch::Receiver`](tokio::sync::watch::Receiver)`<`[`bool`]`>)` |
//! | [`run_hub_connector`] | `pub(super)` | `async fn run_hub_connector(address: `[`String`]`, info: `[`HostInfo`]`, token: `[`Option`]`<`[`Arc`](std::sync::Arc)`<`[`str`]`>>, client: `[`MonitorClient`](super::client::MonitorClient)`, shutdown: `[`watch::Receiver`](tokio::sync::watch::Receiver)`<`[`bool`]`>)` |
//!
//! [`HostRegistry::summaries`] lists [`registry::HostSummary`].

mod connector;
mod correlate;
mod identity;
mod link;
mod listener;
mod registry;
mod wire;
pub(super) use connector::run_hub_connector;
pub(super) use correlate::correlate;
pub(super) use identity::ALL_HOSTS;
pub(super) use identity::HostInfo;
pub(crate) use identity::valid_host_name;
pub(super) use link::AgentLink;
pub(super) use listener::run_agent_listener;
pub(super) use registry::{HostRegistry, RemoteHost};
