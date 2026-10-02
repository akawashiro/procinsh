/// Remembers only operational states, not continuously changing counters.
#[derive(Default)]
pub(super) struct StatusLog(std::collections::HashMap<&'static str, super::model::SensorState>);
impl StatusLog {
    fn changes(
        &mut self,
        status: &super::model::SystemMonitorStatus,
    ) -> Vec<(&'static str, super::model::SensorState)> {
        let mut changes = Vec::new();
        for (key, state) in [
            ("ipc", &status.ipc),
            ("cpu", &status.cpu),
            ("files", &status.files),
        ] {
            if self.0.get(key) != Some(state) {
                self.0.insert(key, state.clone());
                changes.push((key, state.clone()));
            }
        }
        changes
    }

    pub(super) fn observe(&mut self, status: &super::model::SystemMonitorStatus) {
        for (key, value) in self.changes(status) {
            if matches!(
                value,
                super::model::SensorState::Unavailable(_) | super::model::SensorState::Error(_)
            ) {
                log::warn!("System {key}: {value}");
            } else {
                log::info!("System {key}: {value}");
            }
        }
    }
}
