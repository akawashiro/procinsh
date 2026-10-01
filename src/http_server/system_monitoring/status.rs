/// Remembers only operational states, not continuously changing counters.
#[derive(Default)]
pub(super) struct StatusLog(std::collections::HashMap<&'static str, super::model::SensorState>);
impl StatusLog {
    fn changes(&mut self, status: &super::model::SystemStatus) -> Vec<(&'static str, String)> {
        let mut changes = Vec::new();
        for (key, state) in [
            ("ipc", &status.ipc),
            ("cpu", &status.cpu),
            ("files", &status.files),
        ] {
            if self.0.get(key) != Some(state) {
                self.0.insert(key, state.clone());
                changes.push((key, state.to_string()));
            }
        }
        changes
    }
    pub(super) fn observe(&mut self, status: &super::model::SystemStatus) {
        for (key, value) in self.changes(status) {
            if value.starts_with("unavailable:") || value.starts_with("error:") {
                log::warn!("System {key}: {value}");
            } else {
                log::info!("System {key}: {value}");
            }
        }
    }
}

#[cfg(test)]
mod logging_tests {
    use super::*;
    use crate::http_server::system_monitoring::model::{SensorState, SystemStatus};
    #[test]
    fn logs_changes_recovery_and_recurrence_without_repeating_errors() {
        let mut log = StatusLog::default();
        let mut status = SystemStatus::default();
        assert_eq!(log.changes(&status).len(), 3);
        status.cpu = SensorState::Unavailable("permission denied".into());
        assert_eq!(log.changes(&status).len(), 1);
        assert!(log.changes(&status).is_empty());
        status.lost = Some(2);
        assert!(log.changes(&status).is_empty());
        status.cpu = SensorState::Unavailable("unsupported".into());
        assert_eq!(log.changes(&status).len(), 1);
        status.cpu = SensorState::Observing;
        assert_eq!(log.changes(&status).len(), 1);
        status.cpu = SensorState::Unavailable("permission denied".into());
        assert_eq!(log.changes(&status).len(), 1);
    }
}
