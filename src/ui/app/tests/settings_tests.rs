//! Einstellungen: Pfade, Strenge, Einrichtung der Gesichtserkennung.
use super::*;

#[test]
fn changing_the_strictness_starts_one_write_task() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.path().to_string_lossy().into_owned();
    app.save_review_threshold(0.65);
    assert_eq!(
        app.tasks
            .iter()
            .filter(|t| t.kind == JobKind::Mutation)
            .count(),
        1
    );
    assert_eq!(
        app.review_draft,
        Some((app.settings.collection.clone(), 0.65))
    );
    Ok(())
}

#[test]
fn switching_burst_suggestions_starts_one_write_task() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.path().to_string_lossy().into_owned();
    app.save_burst(false);
    assert_eq!(
        app.tasks
            .iter()
            .filter(|t| t.kind == JobKind::Mutation)
            .count(),
        1
    );
    assert_eq!(
        app.burst_draft,
        Some((app.settings.collection.clone(), false))
    );
    Ok(())
}

fn settings_frame(app: &mut PhotoApp, context: &egui::Context) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };
    let _ = context.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.settings_view(ui));
    });
}

#[test]
fn a_typed_path_is_saved_when_the_field_loses_focus_without_a_save_click() -> anyhow::Result<()> {
    let folder = tempfile::tempdir()?;
    let exiftool = folder.path().join("exiftool.exe");
    std::fs::write(&exiftool, b"")?;
    let exiftool = exiftool.to_string_lossy().into_owned();
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.view = View::Settings;
    app.metadata_bg_failed = true;
    // Getippt, aber noch nicht übernommen: der Hintergrundlauf sieht den alten Pfad.
    app.path_drafts.insert(
        settings_view::PathSetting::Exiftool,
        settings_view::PathDraft {
            text: exiftool.clone(),
            pending: true,
        },
    );
    assert_eq!(app.settings.exiftool, "exiftool");
    // Das Feld hat keinen Fokus (wie nach Enter oder einem Klick daneben): es gilt sofort.
    settings_frame(&mut app, &context);
    assert_eq!(app.settings.exiftool, exiftool);
    assert!(app.path_drafts.is_empty());
    assert!(!app.metadata_bg_failed, "neuer Pfad, neuer Versuch");
    assert_eq!(
        Settings::load().exiftool,
        exiftool,
        "ohne Speichern-Knopf gespeichert"
    );
    // Ein Schalter speichert ebenfalls sofort (gleicher Weg wie in der Ansicht).
    app.settings.central_images = true;
    assert!(app.save());
    assert!(Settings::load().central_images);
    Ok(())
}

#[test]
fn an_invalid_path_stays_in_the_field_and_is_not_saved() -> anyhow::Result<()> {
    let folder = tempfile::tempdir()?;
    let mut app = notice_app();
    app.view = View::Settings;
    let before = app.settings.onnx_runtime.clone();
    let missing = folder
        .path()
        .join("fehlt.dll")
        .to_string_lossy()
        .into_owned();
    assert!(!app.set_path_setting(settings_view::PathSetting::Runtime, missing.clone()));
    assert_eq!(app.settings.onnx_runtime, before);
    assert_eq!(
        app.path_drafts
            .get(&settings_view::PathSetting::Runtime)
            .map(|d| d.text.as_str()),
        Some(missing.as_str())
    );
    assert!(app.notice.as_ref().is_some_and(|n| n.error));
    assert_ne!(Settings::load().onnx_runtime, missing);
    // Ein Modellordner ohne Modellbeschreibungen wird ebenso abgelehnt.
    assert!(!app.set_path_setting(
        settings_view::PathSetting::Models,
        folder.path().to_string_lossy().into_owned()
    ));
    assert_ne!(app.settings.model_dir, folder.path().to_string_lossy());
    Ok(())
}

#[test]
fn unchanged_settings_start_no_collection_write() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let mut app = notice_app();
    app.settings.collection = root.path().to_string_lossy().into_owned();
    app.view = View::Settings;
    let notice = app.notice.as_ref().map(|n| n.text.clone());
    let models = app.settings.model_dir.clone();
    assert!(app.set_path_setting(settings_view::PathSetting::Models, models));
    app.choose_model("detector", app.detector.clone());
    app.choose_model("embedder", app.embedder.clone());
    assert!(app.tasks.is_empty(), "nothing changed, nothing is written");
    assert_eq!(app.notice.as_ref().map(|n| n.text.clone()), notice);
    // Eine echte Änderung der Modellauswahl schreibt genau einmal in die Sammlung.
    app.choose_model("detector", "anderer".into());
    assert_eq!(tasks_of(&app, "mutation"), 1);
    wait_for_tasks(&mut app);
    Ok(())
}

#[test]
fn face_setup_installs_models_after_the_runtime_download() -> anyhow::Result<()> {
    let folder = tempfile::tempdir()?;
    let runtime = folder.path().join("onnxruntime.dll");
    std::fs::write(&runtime, b"")?;
    let models = tempfile::tempdir()?;
    let mut app = notice_app();
    app.view = View::Settings;
    app.settings.model_dir = models.path().to_string_lossy().into_owned();
    // Die Modellinstallation wartet hinter dieser Bahn und lädt im Test nichts.
    let lane = Control::default().lane("model-install")?;
    // Schritt 1 ohne Netz: ein Auftrag derselben Art liefert die geladene Laufzeit.
    app.setup_chain = true;
    let downloaded = runtime.clone();
    app.launch(
        "Laufzeit herunterladen",
        false,
        JobKind::RuntimeDownload,
        move |_| Ok(Outcome::RuntimeDownloaded(downloaded)),
    );
    settle_until(
        &mut app,
        "Laufzeit heruntergeladen",
        |app| app.poll(&egui::Context::default()),
        |app| tasks_of(app, "runtime-download") == 0,
    );
    assert_eq!(app.settings.onnx_runtime, runtime.to_string_lossy());
    assert!(!app.setup_chain);
    assert_eq!(
        tasks_of(&app, "model-install"),
        1,
        "Schritt 2 folgt von selbst"
    );
    assert_eq!(app.tasks[0].view, View::Settings);
    app.tasks[0].task.control.stop();
    drop(lane);
    wait_for_tasks(&mut app);
    // Ein gescheiterter Download beendet die Kette.
    app.setup_chain = true;
    app.launch(
        "Laufzeit herunterladen",
        false,
        JobKind::RuntimeDownload,
        |_| anyhow::bail!("kein Netz"),
    );
    wait_for_tasks(&mut app);
    assert!(!app.setup_chain);
    assert_eq!(tasks_of(&app, "model-install"), 0);
    Ok(())
}

#[test]
fn the_setup_check_is_cached_and_repeated_when_the_paths_change() -> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    let dir = tempfile::tempdir()?;
    let runtime = dir.path().join("onnxruntime.dll");
    app.settings.onnx_runtime = runtime.to_string_lossy().into_owned();
    let checked = |app: &PhotoApp| app.setup_files.borrow().as_ref().map(|f| f.checked);
    assert!(app.setup_needed());
    let first = checked(&app);
    assert!(first.is_some());
    // Weitere Frames fragen das Dateisystem nicht erneut.
    assert!(app.setup_needed());
    assert_eq!(checked(&app), first);
    // Ein anderer Pfad wird sofort neu geprüft.
    app.settings.onnx_runtime = dir.path().join("anders.dll").to_string_lossy().into_owned();
    assert!(app.setup_needed());
    assert_ne!(checked(&app), first);
    Ok(())
}
