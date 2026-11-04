use foto_importer::core::jobs::{self, Control};
use std::{sync::mpsc, time::Duration};
#[test]
fn interactive_reservation_yields_background_work() -> anyhow::Result<()> {
    let guard = jobs::foreground();
    let control = Control {
        background: true,
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        tx.send(control.checkpoint().is_ok()).ok();
    });
    assert!(rx.recv_timeout(Duration::from_millis(60)).is_err());
    drop(guard);
    assert!(rx.recv_timeout(Duration::from_secs(2))?);
    Ok(())
}
#[test]
fn queued_job_can_cancel_without_executing() -> anyhow::Result<()> {
    let key = uuid::Uuid::new_v4().to_string();
    let control = Control::default();
    let first = control.lane(&key)?;
    let waiting = Control::default();
    let worker = waiting.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        tx.send(worker.lane(&key).is_ok()).ok();
    });
    waiting.stop();
    assert!(!rx.recv_timeout(Duration::from_secs(2))?);
    drop(first);
    Ok(())
}
