/// Remembers only operational states, not continuously changing counters.
#[derive(Default)]
pub(super) struct StatusLog(
    std::collections::HashMap<&'static str, crate::ipc::acquisition::SensorState>,
);
impl StatusLog {
    fn changes(
        &mut self,
        status: &crate::ipc::acquisition::SystemMonitorStatus,
    ) -> Vec<(&'static str, crate::ipc::acquisition::SensorState)> {
        let mut changes = Vec::new();
        for (key, state) in [
            ("ipc", &status.ipc),
            ("cpu", &status.cpu),
            ("files", &status.files),
            ("signals", &status.signals),
        ] {
            if self.0.get(key) != Some(state) {
                self.0.insert(key, state.clone());
                changes.push((key, state.clone()));
            }
        }
        changes
    }

    pub(super) fn observe(&mut self, status: &crate::ipc::acquisition::SystemMonitorStatus) {
        for (key, value) in self.changes(status) {
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
}
