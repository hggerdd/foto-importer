use super::*;
use eframe::App;

#[test]
fn empty_group_page_returns_to_first_page_when_no_groups_remain() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.path().to_string_lossy().into_owned();
    app.face_index = 3;
    app.refresh_faces();
    for _ in 0..1000 {
        app.poll(&ctx);
        if app.tasks.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(app.tasks.is_empty());
    assert_eq!(app.face_index, 0);
    assert!(app.face_page.groups.is_empty());
    Ok(())
}

#[test]
fn folder_tree_loads_levels_lazily_on_a_worker_and_reveals_the_selection() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    {
        let c = Collection::open(root.path(), true)?;
        for (id, dir) in [(1, "2024/05"), (2, "2024/06"), (3, "2025")] {
            let rel = format!("{dir}/{id}.jpg");
            c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
            c.db.execute("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,?,?,?,'jpg',1,1,?,'present')", rusqlite::params![id, rel, rel, dir, format!("{id}.jpg"), id.to_string(), id])?;
        }
    }
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.path().to_string_lossy().into_owned();
    app.status.files = 3;
    app.status.photos = 3;
    app.applied.folder = Some("2024/05".into());
    let settle = |app: &mut PhotoApp| {
        for _ in 0..2000 {
            app.sync_folder_tree();
            app.poll(&ctx);
            if app.tasks.is_empty() && app.folder_tree.wanted().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    settle(&mut app);
    let names = |app: &PhotoApp| {
        app.folder_tree
            .rows()
            .into_iter()
            .filter_map(|row| match row {
                crate::core::folders::Row::Node { path, .. } => Some(path),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // Root and the path to the selected folder are loaded, siblings stay collapsed.
    assert_eq!(names(&app), ["2024", "2024/05", "2024/06", "2025"]);
    app.folder_tree.collapse("2024");
    assert_eq!(names(&app), ["2024", "2025"]);
    Ok(())
}

#[test]
fn confirming_a_group_brings_its_successor_to_the_same_viewport_position() {
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.view = View::People;
    app.settings.collection = "tmp/synthetic-faces".into();
    let face = |id| Face {
        id,
        photo: id,
        source: format!("{id}.jpg"),
        bbox: [0.1, 0.1, 0.9, 0.9],
        person: None,
        assignment: None,
        ignored: false,
        score: 0.0,
    };
    let draw = |app: &mut PhotoApp| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| app.update(ctx, &mut eframe::Frame::_new_kittest()),
        );
    };
    app.face_page.groups = (1..=6).map(|id| vec![face(id)]).collect();
    draw(&mut app);
    let original_top = app.face_row_tops[0].1;
    app.drafts.get_mut("2").expect("second group rendered").name = "Schon eingetippt".into();
    app.prepare_group_advance(&Undo {
        before: vec![faces::FaceSnapshot {
            id: 1,
            person: None,
            assignment: None,
            confidence: None,
            ignored: false,
            rejections: Vec::new(),
            pins: Vec::new(),
        }],
        after: Vec::new(),
        created: None,
    });
    let advance = app
        .face_advance
        .as_mut()
        .expect("successor captured after commit");
    assert!(!advance.ready);
    advance.ready = true;
    // The former successor remains the target even when refresh inserts a row.
    app.face_page.groups = [7, 2, 3, 4, 5, 6]
        .into_iter()
        .map(|id| vec![face(id)])
        .collect();
    draw(&mut app);
    draw(&mut app);
    assert!(
        (app.face_row_tops[1].1 - original_top).abs() < 2.0,
        "{:?}",
        app.face_row_tops
    );
    assert_eq!(app.drafts["2"].name, "Schon eingetippt");
    assert!(app.face_advance.is_none());
}

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
            assert!(
                image.right() <= size.x && metadata.right() <= size.x,
                "{image:?} {metadata:?} {size:?}"
            );
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
fn interactive_writes_do_not_yield_to_readers_but_maintenance_does() {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    let folder = tempfile::tempdir().expect("Ordner");
    app.settings.collection = folder.path().to_string_lossy().into_owned();
    app.launch("Benennen", true, "naming", |_| {
        Ok(Outcome::Done(String::new()))
    });
    app.launch("Wartung", true, "maintenance", |_| {
        Ok(Outcome::Done(String::new()))
    });
    let background = |kind: &str| {
        app.tasks
            .iter()
            .find(|t| t.kind == kind)
            .map(|t| t.task.control.background)
    };
    assert_eq!(background("naming"), Some(false));
    assert_eq!(background("maintenance"), Some(true));
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

#[test]
fn hovering_a_face_shows_exactly_that_photo_and_leaving_hides_it() {
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    assert!(app.settings.hover_preview, "Standard ist eingeschaltet");
    app.view = View::People;
    app.settings.collection = "tmp/synthetic-faces".into();
    let face = |id| Face {
        id,
        photo: id,
        source: format!("{id}.jpg"),
        bbox: [0.1, 0.1, 0.9, 0.9],
        person: None,
        assignment: None,
        ignored: false,
        score: 0.0,
    };
    app.face_page.groups = (1..=3).map(|id| vec![face(id), face(id + 10)]).collect();
    let draw = |app: &mut PhotoApp, pointer: Option<egui::Pos2>| {
        let events = pointer
            .map(|pos| vec![egui::Event::PointerMoved(pos)])
            .unwrap_or_else(|| vec![egui::Event::PointerGone]);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.update(ctx, &mut eframe::Frame::_new_kittest()),
        );
    };
    let find = |app: &mut PhotoApp| {
        draw(app, None);
        for y in (60..780).step_by(24) {
            for x in (10..1270).step_by(24) {
                let at = Some(egui::pos2(x as f32, y as f32));
                draw(app, at);
                draw(app, at);
                if app.hover_face.is_some() {
                    return at;
                }
            }
        }
        None
    };
    // Mit eingeschaltetem Schalter wird genau ein Gesicht unter der Maus erfasst.
    let at = find(&mut app).expect("ein Gesicht ist erreichbar");
    let shown = app.hover_face.clone().expect("hover");
    assert!([1, 2, 3, 11, 12, 13].contains(&shown.id));
    // Verlässt die Maus das Fenster, bleibt keine Vorschau übrig.
    draw(&mut app, None);
    draw(&mut app, None);
    assert!(app.hover_face.is_none());
    // Ausgeschaltet wird nie etwas erfasst.
    app.settings.hover_preview = false;
    draw(&mut app, Some(at));
    draw(&mut app, Some(at));
    assert!(app.hover_face.is_none());
}

#[test]
fn dates_and_sizes_are_shown_in_german_notation_without_touching_unknown_formats() {
    assert_eq!(display_date("2026:10:25 15:30:00"), "25.10.2026 · 15:30:00");
    assert_eq!(
        display_date("2026-10-25T15:30:00+01:00"),
        "25.10.2026 · 15:30:00"
    );
    assert_eq!(display_date("2026:10:25"), "25.10.2026");
    assert_eq!(display_date("gestern Abend"), "gestern Abend");
    assert_eq!(display_date("2026:13"), "2026:13");
    assert_eq!(display_date(""), "");
    assert_eq!(human_size(300 * 1024), "300 KB");
    assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
}

fn people_app(count: i64) -> (egui::Context, PhotoApp) {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.view = View::People;
    app.face_mode = "persons".into();
    app.settings.collection = "tmp/synthetic-people".into();
    load_people(&mut app, count, 0);
    (context, app)
}

/// Übernimmt einen synthetischen Personenstand wie `poll` nach einem Worker-Ergebnis.
fn load_people(app: &mut PhotoApp, count: i64, offset: i64) {
    let people: Vec<faces::Person> = (1..=count)
        .map(|id| faces::Person {
            id,
            name: format!("Person {:03} Müller", (id + offset) % 997),
            birth: if id % 3 == 0 {
                "1980".into()
            } else {
                String::new()
            },
            count: (id % 7) as usize,
            gender: if id % 2 == 0 {
                "weiblich".into()
            } else {
                String::new()
            },
            note: String::new(),
        })
        .collect();
    let family = crate::core::kinship::Family::new(&[], &BTreeMap::new());
    app.face_page.rows =
        crate::core::people_table::build_rows(&people, &BTreeMap::new(), &[], &family, &[], 2026);
    app.face_page.people = people;
    app.face_page_serial += 1;
}

fn draw_people(context: &egui::Context, app: &mut PhotoApp, size: egui::Vec2) {
    for _ in 0..2 {
        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ctx| app.update(ctx, &mut eframe::Frame::_new_kittest()),
        );
    }
}

#[test]
fn people_table_and_detail_panel_are_not_cut_off_in_small_and_large_windows() {
    for size in [
        egui::vec2(980.0, 680.0),
        egui::vec2(1280.0, 800.0),
        egui::vec2(1920.0, 1080.0),
    ] {
        let (context, mut app) = people_app(300);
        app.relation_from = Some(42);
        draw_people(&context, &mut app, size);
        let bounds = app.people_ui.bounds.expect("page was drawn");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        assert!(screen.contains_rect(bounds.table), "{bounds:?} at {size:?}");
        assert!(screen.contains_rect(bounds.panel), "{bounds:?} at {size:?}");
        assert!(
            bounds.table.right() <= bounds.panel.left() + 1.0,
            "table and panel overlap: {bounds:?}"
        );
        assert!(
            bounds.table.width() >= 340.0,
            "table too narrow at {size:?}: {bounds:?}"
        );
        assert!(
            bounds.panel.width() >= 270.0,
            "panel too narrow at {size:?}: {bounds:?}"
        );
        assert!(
            bounds.panel_content_width <= bounds.panel.width() + 1.0,
            "panel content wider than the panel at {size:?}: {bounds:?}"
        );
        assert!(bounds.table.height() >= 300.0 && bounds.panel.height() >= 300.0);
        assert!(bounds.columns >= 2, "{bounds:?}");
        if size.x >= 1900.0 {
            assert!(
                bounds.columns >= 8,
                "wide window shows most columns: {bounds:?}"
            );
        }
    }
}

#[test]
fn people_filter_reduces_rows_and_the_selection_survives_a_reload() {
    let (context, mut app) = people_app(120);
    let size = egui::vec2(1280.0, 800.0);
    app.select_person(7);
    draw_people(&context, &mut app, size);
    assert_eq!(app.people_ui.view.len(), 120);
    // Tippen filtert sofort: Umschrift und #ID treffen dieselbe Zeile.
    app.people_ui.query = "mueller 007".into();
    draw_people(&context, &mut app, size);
    assert_eq!(app.people_ui.view.len(), 1);
    assert_eq!(app.face_page.rows[app.people_ui.view[0]].id, 7);
    app.people_ui.query = "#12".into();
    draw_people(&context, &mut app, size);
    assert!(!app.people_ui.view.is_empty() && app.people_ui.view.len() < 120);
    app.people_ui.query.clear();
    // Neuladen (neue Revision der Daten) behält Auswahl und Sortierung.
    app.people_ui.sort = vec![crate::core::people_table::SortKey {
        column: crate::core::people_table::Column::Faces,
        descending: true,
    }];
    draw_people(&context, &mut app, size);
    let first = app.face_page.rows[app.people_ui.view[0]].faces;
    assert_eq!(first, 6, "meiste Gesichter zuerst");
    load_people(&mut app, 120, 5);
    draw_people(&context, &mut app, size);
    assert_eq!(app.relation_from, Some(7));
    assert_eq!(app.people_ui.sort.len(), 1);
    assert_eq!(app.people_ui.view.len(), 120);
    // Verschwindet die Person (z. B. zusammengeführt), wird die Auswahl verworfen.
    app.face_page.people.retain(|p| p.id != 7);
    draw_people(&context, &mut app, size);
    assert_eq!(app.relation_from, None);
}

#[test]
fn unsaved_property_edits_survive_a_reload_but_clean_drafts_follow_the_data() {
    let (context, mut app) = people_app(5);
    let size = egui::vec2(1280.0, 800.0);
    app.select_person(2);
    draw_people(&context, &mut app, size);
    app.people_ui.draft_mut().expect("draft").2.note = "Notiz im Entwurf".into();
    load_people(&mut app, 5, 3);
    draw_people(&context, &mut app, size);
    let (_, base, edit) = app.people_ui.draft_mut().expect("draft").clone();
    assert_eq!(edit.note, "Notiz im Entwurf", "Eingabe bleibt erhalten");
    assert_eq!(
        base.name, app.face_page.people[1].name,
        "Stand folgt den Daten"
    );
}

#[test]
fn filter_select_popup_stays_open_for_the_visual_check() {
    let (context, mut app) = people_app(40);
    app.select_person(3);
    app.people_ui.capture_popup = Some("mül".into());
    draw_people(&context, &mut app, egui::vec2(1280.0, 800.0));
    assert!(egui::Popup::is_any_open(&context), "popup stays open");
}
