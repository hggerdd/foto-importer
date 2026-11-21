//! Meldungen und Grundzustand: leere Arbeitsbereiche, Vorrang von Aufträgen, Hinweise.
use super::*;

#[test]
fn all_empty_workspaces_render_without_external_tools() {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    for view in [
        View::Import,
        View::Collection,
        View::Faces,
        View::People,
        View::Settings,
        View::Tasks,
    ] {
        app.view = view;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let output = context.run(input, |ctx| {
            app.update(ctx, &mut eframe::Frame::_new_kittest())
        });
        assert!(!output.shapes.is_empty());
    }
}
#[test]
fn interactive_writes_do_not_yield_to_readers_but_maintenance_does() {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    let folder = tempfile::tempdir().expect("Ordner");
    app.settings.collection = folder.path().to_string_lossy().into_owned();
    app.launch("Benennen", true, JobKind::Naming, |_| {
        Ok(Outcome::Done(String::new()))
    });
    app.launch("Wartung", true, JobKind::Maintenance, |_| {
        Ok(Outcome::Done(String::new()))
    });
    let background = |kind: &str| {
        app.tasks
            .iter()
            .find(|t| t.kind.as_str() == kind)
            .map(|t| t.task.control.background)
    };
    assert_eq!(background("naming"), Some(false));
    assert_eq!(background("maintenance"), Some(true));
}
#[test]
fn an_error_shows_a_plain_sentence_and_keeps_the_original_in_the_history() {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context);
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.message("unable to open database file: X:/weg".into(), true);
    assert!(app.notice.as_ref().is_some_and(|n| {
        n.text
            .starts_with("Die Sammlung konnte nicht geöffnet werden.")
    }));
    assert!(
        app.history
            .last()
            .is_some_and(|line| line.contains("unable to open database file"))
    );
    app.message("unable to open database file".into(), false);
    assert_eq!(
        app.notice.as_ref().map(|n| n.text.as_str()),
        Some("unable to open database file")
    );
}

#[test]
fn a_notice_belongs_to_its_view() {
    let mut app = notice_app();
    let now = std::time::Instant::now();
    assert!(
        app.visible_notice(now).is_some(),
        "der Starthinweis ist global"
    );
    app.view = View::Settings;
    app.message("Einstellungen gespeichert".into(), false);
    assert!(app.visible_notice(now).is_some());
    app.view = View::Import;
    assert!(app.visible_notice(now).is_none());
    app.view = View::Settings;
    assert!(app.visible_notice(now).is_some());
}

#[test]
fn a_task_result_notice_goes_to_the_launching_view() {
    let mut app = notice_app();
    app.view = View::Settings;
    app.notice_origin = Some(View::Collection);
    app.message("fertig".into(), false);
    assert!(app.visible_notice(std::time::Instant::now()).is_none());
    app.view = View::Collection;
    assert!(app.visible_notice(std::time::Instant::now()).is_some());
}

#[test]
fn a_normal_notice_expires_but_an_error_stays() {
    let mut app = notice_app();
    app.message("fertig".into(), false);
    let set = app
        .notice
        .as_ref()
        .map(|n| n.set)
        .unwrap_or_else(std::time::Instant::now);
    let just_before = set + config::NOTICE_SECONDS - std::time::Duration::from_millis(1);
    let after = set + config::NOTICE_SECONDS;
    assert!(app.visible_notice(just_before).is_some());
    assert!(app.visible_notice(after).is_none());
    app.message("unable to open database file".into(), true);
    let set = app
        .notice
        .as_ref()
        .map(|n| n.set)
        .unwrap_or_else(std::time::Instant::now);
    assert!(
        app.visible_notice(set + config::NOTICE_SECONDS * 100)
            .is_some()
    );
}

#[test]
fn dismissing_an_error_removes_it() {
    let mut app = notice_app();
    app.message("unable to open database file".into(), true);
    assert!(app.visible_notice(std::time::Instant::now()).is_some());
    app.dismiss_notice();
    assert!(app.visible_notice(std::time::Instant::now()).is_none());
}
