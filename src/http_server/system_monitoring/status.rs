/// Remembers only operational states, not continuously changing counters.
#[derive(Default)]
pub(super) struct StatusLog(std::collections::HashMap<&'static str, String>);
impl StatusLog {
    fn changes(&mut self, status: &serde_json::Value) -> Vec<(&'static str, String)> {
        let mut changes = Vec::new();
        for key in ["ipc", "cpu", "files"] {
            if let Some(value) = status[key].as_str()
                && self.0.get(key).is_none_or(|old| old != value)
            {
                self.0.insert(key, value.to_owned());
                changes.push((key, value.to_owned()));
            }
        }
        changes
    }
    pub(super) fn observe(&mut self, status: &serde_json::Value) {
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
    use serde_json::json;
    #[test]
    fn logs_changes_recovery_and_recurrence_without_repeating_errors() {
        let mut log = StatusLog::default();
        let error = json!({"cpu":"unavailable: permission denied", "lost":1});
        assert_eq!(log.changes(&error).len(), 1);
        assert!(log.changes(&error).is_empty());
        assert!(
            log.changes(&json!({"cpu":"unavailable: permission denied", "lost":2}))
                .is_empty()
        );
        assert_eq!(
            log.changes(&json!({"cpu":"unavailable: unsupported"}))
                .len(),
            1
        );
        assert_eq!(log.changes(&json!({"cpu":"observing"})).len(), 1);
        assert_eq!(log.changes(&error).len(), 1);
    }
}
