use super::*;
use eframe::App;

#[test]
fn collection_keeps_preview_and_metadata_visible_when_filters_open() {
    for size in [egui::vec2(980.0, 680.0), egui::vec2(1280.0, 800.0)] {
        let context = egui::Context::default();
        let creation = eframe::CreationContext::_new_kittest(context.clone());
        let mut app = PhotoApp::with_settings(&creation, Settings::default());
        app.view = View::Collection;
        app.settings.collection = "tmp/synthetic-collection".into();
        let photo = Photo {
            id: 1,
            file_id: 1,
            path: "photo.jpg".into(),
            name: "photo.jpg".into(),
            source: "photo.jpg".into(),
            date: "2026:11:06 08:00:00".into(),
            camera: "Nikon Z6".into(),
            lens: "24–70mm".into(),
            iso: Some(100.0),
            size: 900_000,
            orientation: 1,
        };
        app.page.total = 102;
        app.page.photos = vec![photo.clone()];
        app.selected_photo = Some(photo);
        app.detail_rows = (0..60)
            .map(|index| (format!("Tag {index}"), "Wert".into()))
            .collect();
        let mut previous = None;
        for dialog in [
            None,
            Some(CollectionDialog::Filters),
            Some(CollectionDialog::Inventory),
        ] {
            app.collection_dialog = dialog;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            };
            let _ = context.run(input, |ctx| {
                app.update(ctx, &mut eframe::Frame::_new_kittest());
            });
            let (image, metadata) = app
                .collection_bounds
                .expect("preview and metadata were drawn");
            assert!(image.top() < 165.0 && image.height() >= 200.0, "{image:?}");
            assert!(metadata.height() >= 150.0, "{metadata:?}");
            assert!(
                metadata.bottom() <= size.y - 35.0,
                "{metadata:?} at {size:?}"
            );
            assert!(image.right() <= size.x && metadata.right() <= size.x);
            assert!(
                image.right() >= size.x - 60.0,
                "inspector lost its right edge: {image:?}"
            );
            if let Some(previous) = previous {
                assert_eq!(previous, (image, metadata), "dialog moved the inspector");
            }
            previous = Some((image, metadata));
        }
    }
}

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
#[test]
fn late_naming_result_from_another_collection_cannot_enter_undo() {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = "old collection".into();
    app.launch("Late result", false, "naming", |_| {
        Ok(Outcome::Named(Undo {
            before: Vec::new(),
            after: Vec::new(),
            created: None,
        }))
    });
    app.settings.collection = "new collection".into();
    for _ in 0..1000 {
        app.poll(&context);
        if app.tasks.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(app.tasks.is_empty());
    assert!(app.undo.is_empty());
}
#[test]
fn repeated_frames_reuse_large_source_selection() {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context);
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.scan.files = (0..100_000)
        .map(|id| SourceFile {
            path: PathBuf::from(format!("{id}.jpg")),
            day: "2026-11-04".into(),
            size: 5,
        })
        .collect();
    app.focused = "2026-11-04".into();
    app.selected.insert(app.focused.clone());
    app.refresh_import_cache();
    let before = app.import_cache.selection.clone();
    let focused = app.import_cache.focus_files.clone();
    for _ in 0..20 {
        app.refresh_import_cache();
    }
    assert!(std::sync::Arc::ptr_eq(&before, &app.import_cache.selection));
    assert!(std::sync::Arc::ptr_eq(
        &focused,
        &app.import_cache.focus_files
    ));
    assert_eq!(app.import_cache.bytes, 500_000);
    assert_eq!(app.import_cache.counts["jpg"], 100_000);
}
