//! Import: Tage wählen, Warteschlange, Aufnahme in die Sammlung, nicht erreichbare Quelle.
use super::*;

#[test]
fn import_plan_stays_inside_minimum_and_default_windows() {
    for size in [[980.0, 680.0], [1280.0, 800.0]] {
        let context = egui::Context::default();
        let creation = eframe::CreationContext::_new_kittest(context.clone());
        let mut app = PhotoApp::with_settings(&creation, Settings::default());
        app.scan.files = (0..36)
            .map(|id| SourceFile {
                path: PathBuf::from(format!("tmp/nonexistent/{id}.jpg")),
                day: "2026-11-04".into(),
                size: 123,
            })
            .collect();
        app.focused = "2026-11-04".into();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size[0], size[1]),
            )),
            ..Default::default()
        };
        let _ = context.run(input, |ctx| {
            app.update(ctx, &mut eframe::Frame::_new_kittest());
        });
        let bounds = app.plan_bounds.expect("import plan was drawn");
        assert!(
            bounds.right() <= size[0],
            "plan extends beyond window: {bounds:?} at {size:?}"
        );
        assert!(bounds.left() >= 190.0);
    }
}
#[test]
fn the_first_click_shows_a_day_and_the_second_toggles_its_import() {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.focused = "2026-11-03".into();
    app.click_day("2026-11-04");
    assert_eq!(app.focused, "2026-11-04");
    assert!(app.selected.is_empty(), "der erste Klick wählt nichts aus");
    app.click_day("2026-11-04");
    assert!(app.selected.contains("2026-11-04"));
    app.click_day("2026-11-04");
    assert!(app.selected.is_empty());
    assert_eq!(app.focused, "2026-11-04");
    assert_eq!(
        import_view::day_label("2026-11-04", 1, true, None),
        "✔ 2026-11-04\n1 Datei"
    );
    assert_eq!(
        import_view::day_label("2026-11-04", 2, false, None),
        "2026-11-04\n2 Dateien"
    );
    assert_eq!(
        import_view::day_label("2026-11-04", 2, false, Some(import_queue::DayMark::Queued)),
        "2026-11-04\n2 Dateien · ⏳ eingeplant"
    );
    // Häkchen und Markierungen müssen in den Schriften der Anwendung vorhanden sein.
    let _ = context.run(egui::RawInput::default(), |ctx| {
        for glyph in ["✔", "⏳", "⚠"] {
            assert!(
                ctx.fonts_mut(|fonts| fonts.has_glyphs(&egui::FontId::proportional(14.0), glyph)),
                "{glyph} fehlt"
            );
        }
    });
}

/// Quelle mit je zwei JPEGs an drei Tagen und ein Zielordner, beide temporär.
fn queue_app(source: &Path, target: &Path) -> anyhow::Result<PhotoApp> {
    let creation = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.source = source.to_string_lossy().into_owned();
    app.settings.target = target.to_string_lossy().into_owned();
    app.settings.dng = false;
    app.settings.skip_imported = false;
    for day in ["2026-11-01", "2026-11-02", "2026-11-03"] {
        for n in 0..2 {
            let path = source.join(format!("{day}-{n}.jpg"));
            std::fs::write(&path, format!("{day} Bild {n}"))?;
            app.scan.files.push(SourceFile {
                path,
                day: day.into(),
                size: 12,
            });
        }
    }
    app.scan_revision += 1;
    Ok(app)
}

fn queue_days(app: &mut PhotoApp, days: &[&str], folder: &str) {
    for day in days {
        app.click_day(day);
        app.click_day(day);
    }
    app.folder_name = folder.into();
    app.queue_import();
}

fn batch_states(app: &PhotoApp) -> Vec<import_queue::BatchState> {
    app.imports.iter().map(|b| b.state.clone()).collect()
}

#[test]
fn queuing_an_import_clears_the_selection_and_marks_the_days() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    let mut app = queue_app(source.path(), target.path())?;
    queue_days(&mut app, &["2026-11-01", "2026-11-02"], "2026-11 Herbst");
    assert!(app.selected.is_empty(), "nichts bleibt ausgewählt");
    assert!(app.focused.is_empty(), "kein Tag bleibt angezeigt");
    assert!(
        app.folder_name.is_empty(),
        "der nächste Ordner beginnt leer"
    );
    assert_eq!(
        app.notice.as_ref().map(|n| n.text.as_str()),
        Some("2 Tage für „2026-11 Herbst“ eingeplant")
    );
    let marks = app.day_marks();
    assert!(
        marks
            .get("2026-11-01")
            .is_some_and(|m| m.locked() && m.greyed())
    );
    assert!(marks.get("2026-11-02").is_some_and(|m| m.locked()));
    assert!(!marks.contains_key("2026-11-03"));
    // Ein festgelegter Tag lässt sich nicht erneut wählen, der nächste schon.
    app.click_day("2026-11-01");
    app.click_day("2026-11-01");
    assert!(app.selected.is_empty());
    app.click_day("2026-11-03");
    app.click_day("2026-11-03");
    assert!(app.selected.contains("2026-11-03"));
    wait_for_tasks(&mut app);
    assert_eq!(batch_states(&app), [import_queue::BatchState::Done]);
    assert_eq!(
        app.day_marks().get("2026-11-01"),
        Some(&import_queue::DayMark::Done)
    );
    assert_eq!(
        std::fs::read_dir(target.path().join("2026-11 Herbst").join("jpg"))?.count(),
        4
    );
    Ok(())
}

#[test]
fn imports_queued_back_to_back_run_one_after_the_other() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    let mut app = queue_app(source.path(), target.path())?;
    queue_days(&mut app, &["2026-11-01"], "Erster");
    queue_days(&mut app, &["2026-11-02"], "Zweiter");
    queue_days(&mut app, &["2026-11-03"], "Dritter");
    use import_queue::BatchState::*;
    // Nur einer läuft, die anderen warten in der festgelegten Reihenfolge.
    assert_eq!(batch_states(&app), [Running, Queued, Queued]);
    assert_eq!(tasks_of(&app, "copy"), 1);
    assert_eq!(app.tasks[0].task.label, "Import „Erster“");
    let mut seen = Vec::new();
    settle_until(
        &mut app,
        "alle Imports fertig",
        |app| {
            app.poll(&egui::Context::default());
            assert!(tasks_of(app, "copy") <= 1, "nie zwei Imports gleichzeitig");
            if let Some(task) = app.tasks.iter().find(|t| t.kind == JobKind::Copy)
                && seen.last() != Some(&task.task.label)
            {
                seen.push(task.task.label.clone());
            }
        },
        |app| app.tasks.is_empty(),
    );
    assert_eq!(
        seen,
        ["Import „Erster“", "Import „Zweiter“", "Import „Dritter“"]
    );
    assert_eq!(batch_states(&app), [Done, Done, Done]);
    // Aufgaben und Verlauf nennen Zielordner und Tage jedes Imports.
    let destination =
        import_view::display_path(&target.path().join("Zweiter").to_string_lossy()).into_owned();
    assert_eq!(app.imports[1].destination(), destination);
    assert_eq!(app.imports[1].days_text(), "Tag: 2026-11-02");
    assert!(app.history.iter().any(|line| {
        line.contains("Import „Zweiter“")
            && line.contains(&destination)
            && line.contains("2026-11-02")
            && line.contains("2 Dateien kopiert")
    }));
    for folder in ["Erster", "Zweiter", "Dritter"] {
        assert_eq!(
            std::fs::read_dir(target.path().join(folder).join("jpg"))?.count(),
            2,
            "{folder}"
        );
    }
    // Die Originale bleiben unverändert.
    assert_eq!(std::fs::read_dir(source.path())?.count(), 6);
    Ok(())
}

#[test]
fn a_failing_import_does_not_stop_the_next_one() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    let mut app = queue_app(source.path(), target.path())?;
    // Das erste Ziel gibt es nicht: der Import schlägt fehl.
    app.settings.target = target.path().join("fehlt").to_string_lossy().into_owned();
    queue_days(&mut app, &["2026-11-01"], "Erster");
    app.settings.target = target.path().to_string_lossy().into_owned();
    queue_days(&mut app, &["2026-11-02"], "Zweiter");
    wait_for_tasks(&mut app);
    use import_queue::BatchState::*;
    assert_eq!(batch_states(&app), [Failed, Done]);
    assert_eq!(
        app.day_marks().get("2026-11-01"),
        Some(&import_queue::DayMark::Failed)
    );
    assert!(
        app.history
            .iter()
            .any(|line| line.contains("Import „Erster“"))
    );
    assert_eq!(
        std::fs::read_dir(target.path().join("Zweiter").join("jpg"))?.count(),
        2
    );
    // Ein fehlgeschlagener Tag ist wieder wählbar.
    app.click_day("2026-11-01");
    app.click_day("2026-11-01");
    assert!(app.selected.contains("2026-11-01"));
    Ok(())
}

#[test]
fn an_import_task_names_its_folder_and_dng() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    let mut app = queue_app(source.path(), target.path())?;
    app.settings.dng = true;
    app.settings.converter = target
        .path()
        .join("fehlt.exe")
        .to_string_lossy()
        .into_owned();
    queue_days(&mut app, &["2026-11-01"], "Mit Umwandlung");
    assert_eq!(app.tasks[0].task.label, "Import „Mit Umwandlung“ · mit DNG");
    wait_for_tasks(&mut app);
    // Ohne Converter schlägt der Import vor dem Kopieren fehl; nichts wurde angelegt.
    assert_eq!(batch_states(&app), [import_queue::BatchState::Failed]);
    assert!(!target.path().join("Mit Umwandlung").exists());
    Ok(())
}

#[test]
fn a_queued_import_can_be_taken_back_before_it_starts() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    let mut app = queue_app(source.path(), target.path())?;
    queue_days(&mut app, &["2026-11-01"], "Erster");
    queue_days(&mut app, &["2026-11-02"], "Zweiter");
    let id = app.imports[1].id;
    app.remove_queued_import(id);
    assert!(!app.day_marks().contains_key("2026-11-02"));
    wait_for_tasks(&mut app);
    assert_eq!(app.imports.len(), 1);
    assert!(!target.path().join("Zweiter").exists());
    Ok(())
}

fn indexed_files(root: &Path) -> anyhow::Result<usize> {
    Ok(Collection::open(root, false)?.status()?.files)
}

#[test]
fn imports_only_copy_and_the_collection_follows_after_the_last_import() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    drop(Collection::open(target.path(), true)?);
    let mut app = queue_app(source.path(), target.path())?;
    queue_days(&mut app, &["2026-11-01"], "Erster");
    queue_days(&mut app, &["2026-11-02"], "Zweiter");
    let mut checked_between = false;
    settle_until(
        &mut app,
        "Imports und Aufnahme fertig",
        |app| {
            app.poll(&egui::Context::default());
            if tasks_of(app, "index-new") > 0 {
                assert_eq!(app.imports_pending(), 0, "Aufnahme erst nach allen Imports");
            }
            if app.imports[0].state == import_queue::BatchState::Done
                && app.imports_pending() > 0
                && !checked_between
            {
                // Der erste Import ist fertig: das Ziel ist gemerkt, der Index noch unverändert.
                checked_between = true;
                assert_eq!(app.settings.index_pending.len(), 1);
                assert_eq!(indexed_files(target.path()).expect("Index lesen"), 0);
                assert!(app.notice.as_ref().is_some_and(|n| {
                    n.text
                        .contains("wird nach den Imports in die Sammlung aufgenommen")
                }));
            }
        },
        |app| app.tasks.is_empty(),
    );
    assert!(checked_between);
    use import_queue::BatchState::*;
    assert_eq!(batch_states(&app), [Done, Done]);
    assert!(app.settings.index_pending.is_empty());
    assert!(Settings::load().index_pending.is_empty(), "gespeichert");
    assert_eq!(indexed_files(target.path())?, 4);
    Ok(())
}

#[test]
fn a_pending_index_update_runs_on_the_next_start_and_is_cleared() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    std::fs::write(root.path().join("a.jpg"), b"x")?;
    let mut settings = Settings::default();
    settings.collection = root.path().to_string_lossy().into_owned();
    settings.index_pending = vec![settings.collection.clone()];
    settings.save()?;
    let creation = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = PhotoApp::with_settings(&creation, Settings::load());
    wait_for_tasks(&mut app);
    assert!(app.settings.index_pending.is_empty());
    assert!(Settings::load().index_pending.is_empty());
    assert_eq!(indexed_files(root.path())?, 1);
    // Die offene Sammlung wurde danach neu gelesen.
    assert_eq!(app.status.files, 1);
    Ok(())
}

#[test]
fn a_pending_folder_without_collection_is_dropped_and_never_created() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut app = queue_app(root.path(), root.path())?;
    app.settings.index_pending = vec![root.path().to_string_lossy().into_owned()];
    wait_for_tasks(&mut app);
    app.start_index_update();
    wait_for_tasks(&mut app);
    assert!(app.settings.index_pending.is_empty());
    assert!(!root.path().join(".myfotomanager").exists());
    Ok(())
}

#[test]
fn a_failing_index_update_is_reported_once_and_kept() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let missing = root.path().join("fehlt").to_string_lossy().into_owned();
    let mut app = queue_app(root.path(), root.path())?;
    app.settings.index_pending = vec![missing.clone()];
    app.start_index_update();
    wait_for_tasks(&mut app);
    assert!(app.notice.as_ref().is_some_and(|n| n.error));
    assert_eq!(app.settings.index_pending, [missing]);
    // Kein neuer Versuch in einer Schleife.
    app.start_index_update();
    assert_eq!(tasks_of(&app, "index-new"), 0);
    assert!(!app.index_due());
    Ok(())
}

#[test]
fn a_new_import_stops_the_index_update_which_resumes_afterwards() -> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    drop(Collection::open(target.path(), true)?);
    std::fs::write(target.path().join("alt.jpg"), b"alt")?;
    let mut app = queue_app(source.path(), target.path())?;
    app.settings.index_pending = vec![app.settings.target.clone()];
    app.start_index_update();
    assert_eq!(tasks_of(&app, "index-new"), 1);
    // Niedrige Priorität: belegt keine Knöpfe.
    assert!(!app.busy());
    queue_days(&mut app, &["2026-11-01"], "Neu");
    assert!(
        app.tasks
            .iter()
            .any(|t| t.kind == JobKind::IndexNew && t.task.control.cancel.load(Ordering::Relaxed)),
        "Import geht vor"
    );
    assert_eq!(batch_states(&app), [import_queue::BatchState::Running]);
    wait_for_tasks(&mut app);
    assert_eq!(batch_states(&app), [import_queue::BatchState::Done]);
    assert!(app.settings.index_pending.is_empty());
    assert!(app.index_failed.is_empty());
    assert_eq!(indexed_files(target.path())?, 3);
    Ok(())
}

#[test]
fn files_copied_earlier_in_the_session_are_skipped_before_the_index_knows_them()
-> anyhow::Result<()> {
    let source = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    drop(Collection::open(target.path(), true)?);
    let mut app = queue_app(source.path(), target.path())?;
    // Ein früherer Import dieser Sitzung hat Tag 1 kopiert; der Index kennt es noch nicht.
    let copied: Vec<PathBuf> = app
        .scan
        .files
        .iter()
        .filter(|f| f.day == "2026-11-01")
        .map(|f| f.path.clone())
        .collect();
    app.imports.push(import_queue::ImportBatch {
        id: 1,
        source: app.settings.source.clone(),
        days: ["2026-11-01".to_owned()].into(),
        files: Vec::new(),
        target: target.path().to_path_buf(),
        name: "Erster".into(),
        converter: None,
        skip: false,
        exiftool: String::new(),
        state: import_queue::BatchState::Done,
        copied,
    });
    app.import_serial = 1;
    assert_eq!(indexed_files(target.path())?, 0);
    app.settings.skip_imported = true;
    queue_days(&mut app, &["2026-11-01"], "Nochmal");
    wait_for_tasks(&mut app);
    use import_queue::BatchState::*;
    assert_eq!(batch_states(&app), [Done, Done]);
    assert!(!target.path().join("Nochmal").exists());
    assert!(
        app.history
            .iter()
            .any(|line| line.contains("Import „Nochmal“") && line.contains("2 übersprungen"))
    );
    Ok(())
}

#[test]
fn an_unreachable_source_at_startup_is_a_quiet_state_not_an_error() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let card = root.path().join("DCIM").join("103NZ6_2");
    let source = card.to_string_lossy().into_owned();
    let creation = eframe::CreationContext::_new_kittest(egui::Context::default());
    let settings = Settings {
        source: source.clone(),
        ..Settings::default()
    };
    let mut app = PhotoApp::with_settings(&creation, settings);
    wait_for_tasks(&mut app);
    assert!(
        app.notice.as_ref().is_none_or(|n| !n.error),
        "kein Fehlerhinweis beim Start"
    );
    assert_eq!(app.source_offline.as_deref(), Some(source.as_str()));
    // Liest der Nutzer die Quelle ausdrücklich ein, bleibt es ein Fehler.
    app.begin_scan();
    assert!(app.source_offline.is_none());
    wait_for_tasks(&mut app);
    assert!(app.notice.as_ref().is_some_and(|n| n.error));
    // Ist die Karte da, wird sie beim Start ganz normal eingelesen.
    std::fs::create_dir_all(&card)?;
    std::fs::write(card.join("a.jpg"), b"jpeg")?;
    let creation = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = PhotoApp::with_settings(
        &creation,
        Settings {
            source,
            ..Settings::default()
        },
    );
    wait_for_tasks(&mut app);
    assert!(app.source_offline.is_none());
    assert_eq!(app.scan.files.len(), 1);
    Ok(())
}
