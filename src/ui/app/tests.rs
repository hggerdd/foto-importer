use super::*;
use eframe::App;

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
fn all_empty_workspaces_render_without_external_tools() {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    for view in [
        View::Import,
        View::Collection,
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
