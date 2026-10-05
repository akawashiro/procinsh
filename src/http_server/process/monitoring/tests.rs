use super::ProcessMonitor as AppState;
use crate::http_server::process::{self as process, ProcessScanner, test_support::Target};
use std::{sync::Arc, time::Duration};

#[test]
fn discovery_rates_history_and_process_exit() {
    let mut target = Target::new("busy_loop");
    let mut discovery = ProcessScanner::default();
    assert!(
        discovery
            .collect()
            .unwrap()
            .iter()
            .any(|p| p.identity == target.id)
    );
    let app = Arc::new(AppState::new(Duration::from_millis(100)));
    let session = app.observe(target.id, app.reserve().unwrap()).unwrap();
    let other_id = process::identity(std::process::id() as i32).unwrap();
    let other = app.observe(other_id, app.reserve().unwrap()).unwrap();
    std::thread::sleep(Duration::from_millis(350));
    {
        let t = session.receiver.borrow();
        let o = t.observation.as_ref().unwrap();
        assert!(o.cpu_percent.unwrap() > 0.0);
        assert!(o.rss_bytes > 0);
        assert!(t.history.len() >= 2);
        assert!(o.threads.iter().any(|t| t.tid == target.id.pid));
        assert!(!t.maps.is_empty());
    }
    assert!(
        discovery
            .collect()
            .unwrap()
            .iter()
            .find(|p| p.identity == target.id)
            .unwrap()
            .cpu_percent
            .unwrap()
            > 0.0
    );
    target.child.kill().unwrap();
    target.child.wait().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(session.receiver.borrow().exited);
    assert!(!other.receiver.borrow().exited);
    assert_eq!(other.receiver.borrow().summary.identity, other_id);
    assert!(app.observe(target.id, app.reserve().unwrap()).is_err());
    app.stop();
    drop(session);
    drop(other);
    app.join_collectors().unwrap();
}

#[test]
fn initial_watch_value_contains_sleeping_thread_snapshot() {
    let target = Target::new("sleeping");
    let app = Arc::new(AppState::new(Duration::from_secs(60)));
    let session = app.observe(target.id, app.reserve().unwrap()).unwrap();
    let value = serde_json::to_value(&*session.receiver.borrow()).unwrap();
    let sample = &value["live_samples"][0];
    assert!(sample["sampled_at"].is_u64(), "{sample}");
    assert_eq!(sample["registers"].as_array().unwrap().len(), 18);
    assert!(!sample["call_stack"].as_array().unwrap().is_empty());
    assert!(sample.get("sample_source").is_none());
    target.assert_detached();
    app.stop();
    drop(session);
    app.join_collectors().unwrap();
}
