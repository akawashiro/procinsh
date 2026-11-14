use crate::privileged_helper::process;
use std::{sync::Arc, time::Duration};

#[test]
fn discovery_rates_history_and_process_exit() {
    use crate::privileged_helper::process::{
        create_process_monitor, create_target, join_process_collectors, observe_process_monitor,
        reserve_process_monitor, stop_process_monitor,
    };

    let mut target = create_target("busy_loop");
    let initial = process::collect_processes().unwrap();
    assert!(initial.iter().any(|p| p.identity == target.id));
    let app = Arc::new(create_process_monitor(Duration::from_millis(100)));
    let session =
        observe_process_monitor(&app, target.id, reserve_process_monitor(&app).unwrap()).unwrap();
    let other_id = process::read_process_id(std::process::id() as i32).unwrap();
    let other =
        observe_process_monitor(&app, other_id, reserve_process_monitor(&app).unwrap()).unwrap();
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
    let current = process::collect_processes().unwrap();
    let before = initial.iter().find(|p| p.identity == target.id).unwrap();
    let after = current.iter().find(|p| p.identity == target.id).unwrap();
    assert!(before.cpu_percent.is_none());
    assert!(after.cpu_percent.is_none());
    let elapsed = (after.raw_measured_ns - before.raw_measured_ns) as f64 / 1_000_000_000.0;
    let cpu_percent = after.raw_ticks.saturating_sub(before.raw_ticks) as f64
        / process::ticks_per_second()
        / elapsed
        * 100.0;
    assert!(cpu_percent > 0.0);
    target.child.kill().unwrap();
    target.child.wait().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(session.receiver.borrow().exited);
    assert!(!other.receiver.borrow().exited);
    assert_eq!(other.receiver.borrow().summary.identity, other_id);
    assert!(
        observe_process_monitor(&app, target.id, reserve_process_monitor(&app).unwrap()).is_err()
    );
    stop_process_monitor(&app);
    drop(session);
    drop(other);
    join_process_collectors(&app).unwrap();
}

#[test]
fn initial_watch_value_contains_sleeping_thread_snapshot() {
    use crate::privileged_helper::process::{
        create_process_monitor, create_target, join_process_collectors, observe_process_monitor,
        reserve_process_monitor, stop_process_monitor, test_support::assert_detached_target,
    };

    let target = create_target("sleeping");
    let app = Arc::new(create_process_monitor(Duration::from_secs(60)));
    let session =
        observe_process_monitor(&app, target.id, reserve_process_monitor(&app).unwrap()).unwrap();
    let value = serde_json::to_value(&*session.receiver.borrow()).unwrap();
    let sample = &value["live_samples"][0];
    assert!(sample["sampled_at"].is_u64(), "{sample}");
    assert_eq!(
        sample["registers"].as_array().unwrap().len(),
        if cfg!(target_arch = "aarch64") {
            34
        } else {
            18
        }
    );
    assert!(!sample["call_stack"].as_array().unwrap().is_empty());
    assert!(sample.get("sample_source").is_none());
    assert_detached_target(&target);
    stop_process_monitor(&app);
    drop(session);
    join_process_collectors(&app).unwrap();
}
