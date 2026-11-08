//! Track sensor availability transitions for system monitoring diagnostics.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`StatusLog`] | `pub(super)` | `struct StatusLog` |
//! | [`observe_status_log`] | `pub(super)` | `fn observe_status_log( state: &mut `[`StatusLog`]`, status: &`[`crate::ipc::acquisition::SystemMonitorStatus`]`, )` |

/// Remembers only operational states, not continuously changing counters.
#[derive(Default)]
pub(super) struct StatusLog(
    std::collections::HashMap<&'static str, crate::ipc::acquisition::SensorState>,
);
fn changes_status_log(
    log: &mut StatusLog,
    status: &crate::ipc::acquisition::SystemMonitorStatus,
) -> Vec<(&'static str, crate::ipc::acquisition::SensorState)> {
    let mut changes = Vec::new();
    for (key, state) in [
        ("ipc", &status.ipc),
        ("cpu", &status.cpu),
        ("files", &status.files),
        ("signals", &status.signals),
    ] {
        if log.0.get(key) != Some(state) {
            log.0.insert(key, state.clone());
            changes.push((key, state.clone()));
        }
    }
    changes
}

pub(super) fn observe_status_log(
    state: &mut StatusLog,
    status: &crate::ipc::acquisition::SystemMonitorStatus,
) {
    for (key, value) in changes_status_log(state, status) {
        if matches!(
            value,
            crate::ipc::acquisition::SensorState::Unavailable(_)
                | crate::ipc::acquisition::SensorState::Error(_)
        ) {
            log::warn!("System {key}: {value}");
        } else {
            log::info!("System {key}: {value}");
        }
    }
}
