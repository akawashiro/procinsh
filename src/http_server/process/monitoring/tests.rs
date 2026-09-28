use super::super::{self as process, Discovery, test_support::Target};
use super::Monitoring as AppState;
use std::{sync::Arc, time::Duration};
#[test]
fn discovery_rates_history_and_process_exit() {
    let mut target = Target::new("busy_loop");
    let mut discovery = Discovery::default();
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
