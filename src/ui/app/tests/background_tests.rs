//! Hintergrundaufträge: tägliche Sicherung, Metadaten im Hintergrund, Inspektor.
use super::*;

#[test]
fn daily_backup_runs_once_per_day_in_the_background() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let mut app = metadata_app(root.path());
    app.status.unread = 0;
    // Aus: nichts startet.
    app.settings.daily_backup = false;
    app.daily_backup_due = true;
    app.start_daily_backup();
    assert_eq!(tasks_of(&app, "backup-daily"), 0);
    app.settings.daily_backup = true;
    app.start_daily_backup();
    app.start_daily_backup();
    assert_eq!(tasks_of(&app, "backup-daily"), 1);
    // Niedrige Priorität: belegt keine Knöpfe und gibt Lesevorgängen nach.
    assert!(!app.busy());
    assert!(app.tasks[0].task.control.background);
    wait_for_tasks(&mut app);
    let backups = crate::core::backup::list(root.path())?;
    assert_eq!(backups.len(), 1);
    let today = chrono::Local::now().date_naive().to_string();
    assert_eq!(
        app.settings
            .daily_backup_dates
            .get(&app.settings.collection),
        Some(&today)
    );
    // Am selben Tag (etwa beim erneuten Öffnen) keine zweite Sicherung.
    app.daily_backup_due = true;
    app.start_daily_backup();
    assert_eq!(tasks_of(&app, "backup-daily"), 0);
    assert!(!app.daily_backup_due);
    Ok(())
}

#[test]
fn background_metadata_starts_when_idle_and_only_once() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let mut app = metadata_app(root.path());
    app.start_metadata_background();
    app.start_metadata_background();
    assert_eq!(tasks_of(&app, "metadata-bg"), 1);
    // Niedrige Priorität: belegt die Knöpfe nicht und gibt Nutzerarbeit Vorrang.
    assert!(!app.busy());
    assert!(app.tasks[0].task.control.background);
    assert!(app.tasks[0].can_pause());
    assert!(
        app.catch_up_hint()
            .is_some_and(|h| !h.contains("nachholen"))
    );
    Ok(())
}

#[test]
fn background_metadata_waits_for_user_writes_and_without_unread_files() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let mut app = metadata_app(root.path());
    app.launch("Benennen", true, JobKind::Mutation, |control| {
        while !control.cancel.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(Outcome::Done(String::new()))
    });
    app.start_metadata_background();
    assert_eq!(tasks_of(&app, "metadata-bg"), 0);
    app.tasks[0].task.control.stop();
    wait_for_tasks(&mut app);
    app.status.files = 9;
    app.status.pending = 5;
    app.status.unread = 0;
    app.start_metadata_background();
    assert_eq!(tasks_of(&app, "metadata-bg"), 0);
    // Pausiert oder nach einem Fehler startet nichts von selbst.
    app.status.unread = 3;
    app.metadata_paused = true;
    app.start_metadata_background();
    app.metadata_paused = false;
    app.metadata_bg_failed = true;
    app.start_metadata_background();
    assert_eq!(tasks_of(&app, "metadata-bg"), 0);
    assert!(app.catch_up_hint().is_some_and(|h| h.contains("nachholen")));
    Ok(())
}

#[test]
fn a_user_write_task_stops_the_background_metadata_run() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let mut app = metadata_app(root.path());
    app.start_metadata_background();
    assert!(!app.tasks[0].task.control.cancel.load(Ordering::Relaxed));
    app.launch("Benennen", true, JobKind::Mutation, |_| {
        Ok(Outcome::Done(String::new()))
    });
    assert!(app.tasks[0].task.control.cancel.load(Ordering::Relaxed));
    assert!(app.busy());
    wait_for_tasks(&mut app);
    // Ein abgebrochener Hintergrundlauf ist kein Fehler: er setzt später fort.
    assert!(!app.metadata_bg_failed);
    Ok(())
}

#[test]
fn a_failing_background_run_stops_for_the_session() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut c = Collection::open(root.path(), true)?;
    std::fs::write(root.path().join("a.jpg"), b"x")?;
    c.quick_scan(false, &Control::default())?;
    assert_eq!(c.status()?.unread, 1);
    drop(c);
    let mut app = metadata_app(root.path());
    app.settings.exiftool = root.path().join("fehlt").to_string_lossy().into_owned();
    app.start_metadata_background();
    wait_for_tasks(&mut app);
    assert!(app.metadata_bg_failed);
    app.start_metadata_background();
    assert_eq!(tasks_of(&app, "metadata-bg"), 0);
    Ok(())
}

#[test]
fn inspector_button_starts_exactly_one_write_task() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let mut app = metadata_app(root.path());
    app.selected_photo = Some(Photo {
        id: 1,
        file_id: 1,
        path: "a.jpg".into(),
        name: "a.jpg".into(),
        date: String::new(),
        camera: String::new(),
        lens: String::new(),
        iso: None,
        size: 1,
        source: "a.jpg".into(),
        orientation: 1,
    });
    app.start_metadata_background();
    app.read_photo_metadata();
    app.read_photo_metadata();
    let writes = app
        .tasks
        .iter()
        .filter(|t| t.write && !low_priority(t.kind))
        .count();
    assert_eq!(writes, 1);
    assert!(app.tasks.iter().any(|t| t.task.label == "Metadaten lesen"));
    // Der Hintergrundlauf weicht dem Nutzerauftrag.
    assert!(
        app.tasks
            .iter()
            .filter(|t| t.kind == JobKind::MetadataBg)
            .all(|t| t.task.control.cancel.load(Ordering::Relaxed))
    );
    Ok(())
}
