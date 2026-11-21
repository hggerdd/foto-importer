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
    app.view = View::Faces;
    app.face_index = 3;
    app.refresh_faces();
    settle_tasks(&mut app, &ctx);
    assert!(app.tasks.is_empty());
    assert_eq!(app.face_index, 0);
    assert!(app.face_page.groups.is_empty());
    Ok(())
}

#[test]
fn other_views_read_people_without_face_groups_and_keep_the_page() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    {
        let c = Collection::open(root.path(), true)?;
        c.db.execute_batch(
            "INSERT INTO persons(id) VALUES(1);
             INSERT INTO person_attrs(person_id,key,value) VALUES(1,'name','Anna');",
        )?;
    }
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.path().to_string_lossy().into_owned();
    app.view = View::Collection;
    app.face_index = 3;
    app.refresh_faces();
    settle_tasks(&mut app, &ctx);
    assert!(app.tasks.is_empty());
    assert_eq!(app.face_index, 3);
    assert_eq!(app.face_page.people.len(), 1);
    assert!(app.face_page.groups.is_empty());
    Ok(())
}

#[test]
fn opening_the_collection_keeps_a_running_search() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.path().to_string_lossy().into_owned();
    app.refresh_collection();
    let revision = app.query_revision;
    app.open_view(View::Collection);
    assert_eq!(app.query_revision, revision);
    settle_tasks(&mut app, &ctx);
    app.open_view(View::Collection);
    assert_eq!(app.query_revision, revision + 1);
    Ok(())
}

fn add_photo(c: &Collection, id: i64) -> anyhow::Result<()> {
    let rel = format!("2024/{id}.jpg");
    c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
    c.db.execute("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,'2024',?,?,'jpg',1,1,?,'present')", rusqlite::params![id, rel, rel, format!("{id}.jpg"), id.to_string(), id])?;
    Ok(())
}

/// Großzügige Frist der Warte-Helfer: unter Last (parallele Tests, langsame Platte) lieber
/// länger warten als flackern. Im Normalfall endet das Warten nach wenigen Millisekunden.
const SETTLE_DEADLINE: Duration = Duration::from_secs(30);

/// Einziger Warte-Helfer der UI-Tests: ruft `step` (z. B. `poll`) wiederholt auf, bis `done`
/// gilt, höchstens bis `SETTLE_DEADLINE`. Scheitert mit `what`, wenn die Frist abläuft.
fn settle_until(
    app: &mut PhotoApp,
    what: &str,
    mut step: impl FnMut(&mut PhotoApp),
    mut done: impl FnMut(&PhotoApp) -> bool,
) {
    let deadline = std::time::Instant::now() + SETTLE_DEADLINE;
    loop {
        step(app);
        if done(app) {
            return;
        }
        let running: Vec<_> = app.tasks.iter().map(|t| t.kind.as_str()).collect();
        assert!(
            std::time::Instant::now() < deadline,
            "{what}: nach {SETTLE_DEADLINE:?} nicht erreicht, laufend {running:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Wartet, bis kein Auftrag mehr läuft.
fn settle_tasks(app: &mut PhotoApp, ctx: &egui::Context) {
    settle_until(
        app,
        "alle Aufträge fertig",
        |app| app.poll(ctx),
        |app| app.tasks.is_empty(),
    );
}

/// Wartet auf alle Aufträge und liefert die Arten, die dabei liefen.
fn settle_kinds(ctx: &egui::Context, app: &mut PhotoApp) -> BTreeSet<&'static str> {
    let mut seen = BTreeSet::new();
    settle_until(
        app,
        "alle Aufträge fertig",
        |app| {
            seen.extend(app.tasks.iter().map(|t| t.kind.as_str()));
            app.poll(ctx);
            seen.extend(app.tasks.iter().map(|t| t.kind.as_str()));
        },
        |app| app.tasks.is_empty(),
    );
    seen
}

#[test]
fn the_page_comes_first_and_the_saved_start_view_shows_before_any_read() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    {
        let c = Collection::open(root.path(), true)?;
        for id in 1..=3 {
            add_photo(&c, id)?;
        }
    }
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let collection = root.path().to_string_lossy().into_owned();
    let open = |collection: &str| {
        let mut app = PhotoApp::with_settings(&creation, Settings::default());
        app.settings.collection = collection.to_owned();
        // Kein Hintergrundlauf der Metadaten: die Testdateien liegen nicht auf der Platte.
        app.settings.exiftool.clear();
        app
    };

    // Ohne gespeicherte Startansicht: Seite, dann Zählung; danach liegt die Startansicht lokal.
    let mut app = open(&collection);
    app.show_start_view();
    assert!(app.page.photos.is_empty() && !app.page_from_snapshot);
    app.refresh_collection();
    let kinds = settle_kinds(&ctx, &mut app);
    assert!(
        kinds.contains("search") && kinds.contains("counts"),
        "{kinds:?}"
    );
    assert!(app.counts_fresh);
    assert_eq!(
        (app.page.photos.len(), app.page.total, app.status.photos),
        (3, 3, 3)
    );
    let saved = start_view::load(&app.cache_dir, &collection).expect("Startansicht");
    assert!(
        !app.cache_dir.starts_with(root.path()),
        "nie in der Sammlung"
    );
    assert_eq!(saved.photos, app.page.photos);

    // Nächster Start: die Startansicht steht sofort da, ohne die Sammlung zu lesen, und ist
    // als veraltet markiert. Unveränderte Sammlung: die Zählung gilt weiter, keine neue.
    let mut app = open(&collection);
    let started = std::time::Instant::now();
    app.show_start_view();
    let shown = started.elapsed();
    assert_eq!(app.page.photos.len(), 3);
    assert_eq!(app.page.total, 3);
    assert!(app.page_from_snapshot && !app.counts_fresh);
    eprintln!("startansicht_ms={:.2}", shown.as_secs_f64() * 1000.0);
    app.refresh_collection();
    let kinds = settle_kinds(&ctx, &mut app);
    assert!(!kinds.contains("counts"), "{kinds:?}");
    assert!(app.counts_fresh && !app.page_from_snapshot);
    assert_eq!(app.page.total, 3);

    // Geänderte Sammlung: die gespeicherten Zahlen werden gezeigt, aber neu gezählt.
    {
        let c = Collection::open(root.path(), true)?;
        add_photo(&c, 4)?;
    }
    let mut app = open(&collection);
    app.show_start_view();
    assert_eq!(app.page.total, 3);
    app.refresh_collection();
    let kinds = settle_kinds(&ctx, &mut app);
    assert!(kinds.contains("counts"), "{kinds:?}");
    assert!(app.counts_fresh);
    assert_eq!(
        (app.page.photos.len(), app.page.total, app.status.photos),
        (4, 4, 4)
    );
    // Blättern in derselben Suche zählt nicht erneut.
    app.navigate_collection(0, None);
    let kinds = settle_kinds(&ctx, &mut app);
    assert!(!kinds.contains("counts"), "{kinds:?}");
    // Ein Ordnerfilter ist eine andere Suche und wird gezählt.
    app.navigate_collection(0, Some("2024".into()));
    let kinds = settle_kinds(&ctx, &mut app);
    assert!(kinds.contains("counts"), "{kinds:?}");
    assert_eq!(app.page.total, 4);
    Ok(())
}

#[test]
fn startup_reads_people_only_after_the_collection() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let settings = Settings {
        collection: root.path().to_string_lossy().into_owned(),
        ..Settings::default()
    };
    let mut app = PhotoApp::with_settings(&creation, settings);
    assert!(app.faces_after_search);
    assert_eq!(tasks_of(&app, "search"), 1);
    assert_eq!(tasks_of(&app, "faces"), 0);
    // Die Ansicht „Gesichter“ wartet nicht: sie liest sofort.
    app.open_view(View::Faces);
    assert!(!app.faces_after_search);
    assert_eq!(tasks_of(&app, "faces"), 1);
    settle_kinds(&ctx, &mut app);
    Ok(())
}

#[test]
fn folder_tree_loads_completely_on_a_worker_and_reveals_the_selection() -> anyhow::Result<()> {
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
    settle_folders(&mut app, &ctx);
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
    // The whole tree is loaded at once; only the path to the selected folder is expanded.
    assert_eq!(names(&app), ["2024", "2024/05", "2024/06", "2025"]);
    app.folder_tree.collapse("2024");
    assert_eq!(names(&app), ["2024", "2025"]);
    Ok(())
}

/// Synthetic collection with one photo per folder, and an app pointed at it.
fn folder_app(ctx: &egui::Context, dirs: &[&str]) -> anyhow::Result<(tempfile::TempDir, PhotoApp)> {
    let root = tempfile::tempdir()?;
    {
        let c = Collection::open(root.path(), true)?;
        for (index, dir) in dirs.iter().enumerate() {
            let id = index as i64 + 1;
            let rel = format!("{dir}/{id}.jpg");
            c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
            c.db.execute("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,?,?,?,'jpg',1,1,?,'present')", rusqlite::params![id, rel, rel, dir, format!("{id}.jpg"), id.to_string(), id])?;
        }
    }
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.path().to_string_lossy().into_owned();
    app.status.files = dirs.len();
    app.status.photos = dirs.len();
    Ok((root, app))
}

fn settle_folders(app: &mut PhotoApp, ctx: &egui::Context) {
    settle_until(
        app,
        "Ordnerbaum gelesen",
        |app| {
            app.sync_folder_tree();
            app.poll(ctx);
        },
        |app| app.tasks.is_empty() && !app.folder_tree.needs_load(),
    );
}

fn folder_names(app: &PhotoApp) -> Vec<String> {
    app.folder_tree
        .rows()
        .into_iter()
        .filter_map(|row| match row {
            crate::core::folders::Row::Node { path, .. } => Some(path),
            _ => None,
        })
        .collect()
}

#[test]
fn a_content_change_during_a_folder_read_does_not_restart_it() -> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let (_root, mut app) = folder_app(&ctx, &["2024", "2025"])?;
    app.sync_folder_tree();
    assert_eq!(app.folder_revision, 1);
    // Background work changes the counts while the read is still running (not yet polled).
    for change in 1..=3 {
        app.status.photos += change;
        app.sync_folder_tree();
    }
    let running: Vec<_> = app
        .tasks
        .iter()
        .filter(|t| t.kind == JobKind::Folders)
        .collect();
    assert_eq!(running.len(), 1);
    assert!(
        !running[0].task.control.cancel.load(Ordering::Relaxed),
        "the running read is not stopped"
    );
    assert_eq!(app.folder_revision, 1, "no second read while one runs");
    settle_folders(&mut app, &ctx);
    assert_eq!(app.folder_revision, 2, "exactly one follow-up read");
    assert_eq!(folder_names(&app), ["2024", "2025"]);
    Ok(())
}

#[test]
fn confirming_a_group_brings_its_successor_to_the_same_viewport_position() {
    let ctx = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.view = View::Faces;
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
            not_face: false,
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
fn late_naming_result_from_another_collection_cannot_enter_undo() {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = "old collection".into();
    app.launch("Late result", false, JobKind::Naming, |_| {
        Ok(Outcome::Named(Undo {
            before: Vec::new(),
            after: Vec::new(),
            created: None,
        }))
    });
    app.settings.collection = "new collection".into();
    settle_tasks(&mut app, &context);
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
    app.view = View::Faces;
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
    app.person_mode = PersonMode::Overview;
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

/// Alle gezeichneten Texte mit ihrer Position (auch aus verschachtelten Formen).
fn painted_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(egui::Pos2, String)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(egui::Pos2, String)>) {
        match shape {
            egui::Shape::Text(text) => out.push((text.pos, text.galley.text().to_string())),
            egui::Shape::Vec(list) => list.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

#[test]
fn person_details_group_relations_partner_first_then_family_then_friends() {
    use crate::core::relations::{Role, canonical};
    let (context, mut app) = people_app(10);
    // Person 1: Partnerin 4, Eltern 2 und 3, Bruder 7 (berechnet über die Eltern),
    // Freund 5, Bekannte 8.
    let edges = vec![
        canonical(4, Role::Partner, 1),
        canonical(2, Role::Parent, 1),
        canonical(3, Role::Parent, 1),
        canonical(2, Role::Parent, 7),
        canonical(3, Role::Parent, 7),
        canonical(5, Role::Friend, 1),
        canonical(8, Role::Acquaintance, 1),
    ];
    *app.face_page.family = crate::core::kinship::Family::new(&edges, &BTreeMap::new());
    app.face_page.relations = edges;
    app.relation_from = Some(1);
    let size = egui::vec2(1920.0, 3000.0);
    draw_people(&context, &mut app, size);
    let output = context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ctx| app.update(ctx, &mut eframe::Frame::_new_kittest()),
    );
    let panel = app.people_ui.bounds.expect("page was drawn").panel;
    let texts: Vec<_> = painted_texts(&output.shapes)
        .into_iter()
        .filter(|(pos, _)| panel.contains(*pos))
        .collect();
    let top = texts
        .iter()
        .find(|(_, t)| t == "Beziehungen")
        .map(|(pos, _)| pos.y)
        .expect("Abschnitt Beziehungen");
    let y = |heading: &str| {
        texts
            .iter()
            .filter(|(pos, t)| t == heading && pos.y > top)
            .map(|(pos, _)| pos.y)
            .reduce(f32::min)
            .unwrap_or_else(|| panic!("Überschrift {heading} fehlt"))
    };
    let order = ["Partner", "Eltern", "Geschwister", "Freunde", "Bekannte"].map(y);
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    // Die Eltern stehen gemeinsam unter einer Überschrift, genau einmal.
    assert_eq!(
        texts
            .iter()
            .filter(|(pos, t)| t == "Eltern" && pos.y > top)
            .count(),
        1
    );
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

fn test_face(id: i64) -> Face {
    Face {
        id,
        photo: id,
        source: String::new(),
        bbox: [0.0; 4],
        person: None,
        assignment: None,
        ignored: false,
        score: 0.0,
    }
}
fn page_ids(app: &PhotoApp) -> Vec<Vec<i64>> {
    app.face_page
        .groups
        .iter()
        .map(|g| g.iter().map(|f| f.id).collect())
        .collect()
}

/// ADR 0068: Die Gruppe verschwindet sofort, mehrere Aufträge dürfen gleichzeitig vorgemerkt
/// sein, eine neu gelesene Seite zeigt ausgeblendete Gesichter nicht, und nach dem Ende
/// (auch bei Fehlern) gilt wieder die Datenbank.
#[test]
fn queued_group_decisions_hide_faces_at_once_and_reappear_on_failure() {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    let folder = tempfile::tempdir().expect("Ordner");
    app.settings.collection = folder.path().to_string_lossy().into_owned();
    app.face_mode = "unknown".into();
    app.face_page.groups = vec![
        vec![test_face(1), test_face(2)],
        vec![test_face(3)],
        vec![test_face(4), test_face(5)],
    ];
    app.face_page.total_groups = 3;
    app.face_page.status.nameable = 5;
    let release = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let gate = release.clone();
    let started = std::time::Instant::now();
    app.queue_hiding("Erste", vec![1, 2], move |_| {
        while !gate.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(Outcome::Done(String::new()))
    });
    app.queue_hiding("Zweite", vec![3], |_| {
        anyhow::bail!("Schreiben fehlgeschlagen")
    });
    assert!(started.elapsed() < Duration::from_millis(100));
    // Beide Aufträge sind vorgemerkt und die Seite zeigt nur noch die verbleibende Gruppe.
    assert_eq!(app.tasks.len(), 2);
    assert!(!app.naming_blocked());
    assert_eq!(page_ids(&app), vec![vec![4, 5]]);
    assert_eq!(app.face_page.total_groups, 1);
    assert_eq!(app.face_page.status.nameable, 2);
    // Eine frisch gelesene Seite (noch ohne die Schreibvorgänge) wird gefiltert.
    app.face_page.groups = vec![vec![test_face(1), test_face(2)], vec![test_face(6)]];
    app.hide_pending_faces();
    assert_eq!(page_ids(&app), vec![vec![6]]);
    // Nach dem Ende der Aufträge endet das Ausblenden.
    release.store(true, Ordering::Relaxed);
    settle_until(
        &mut app,
        "Benennungen fertig",
        |app| app.poll(&context),
        |app| app.tasks.iter().all(|t| t.kind != JobKind::Naming),
    );
    assert!(app.hidden_faces().is_empty());
}

fn review_face(id: i64, score: f32) -> Face {
    Face {
        person: Some(1),
        assignment: Some("suggested".into()),
        score,
        ..test_face(id)
    }
}
/// Reiter „Vorschläge prüfen“ mit zwei Gruppen für Anna in einer leeren Sammlung unter `root`.
fn review_app(root: &Path) -> (egui::Context, PhotoApp) {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.view = View::Faces;
    app.face_mode = "review".into();
    app.settings.collection = root.to_string_lossy().into_owned();
    app.face_page.people = vec![faces::Person {
        id: 1,
        name: "Anna".into(),
        birth: String::new(),
        count: 4,
        gender: String::new(),
        note: String::new(),
    }];
    app.face_page.groups = vec![
        vec![review_face(1, 0.8), review_face(2, 0.62)],
        vec![review_face(3, 0.6)],
    ];
    app.face_page.total_groups = 2;
    app.face_page.status.review = 3;
    (context, app)
}
fn naming_tasks(app: &PhotoApp) -> usize {
    app.tasks
        .iter()
        .filter(|t| t.kind == JobKind::Naming)
        .count()
}

#[test]
fn none_of_them_starts_one_write_task_and_hides_the_group() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let (_context, mut app) = review_app(root.path());
    let group = app.face_page.groups[0].clone();
    assert!(app.decide_group(&group, faces_view::Decision::Reject));
    assert_eq!(naming_tasks(&app), 1);
    assert_eq!(page_ids(&app), vec![vec![3]]);
    assert_eq!(app.face_page.status.review, 1);
    // Ohne vorgeschlagene Person gibt es nichts abzulehnen.
    assert!(!app.decide_group(&[test_face(9)], faces_view::Decision::Reject));
    assert_eq!(naming_tasks(&app), 1);
    Ok(())
}

/// Ein Tastendruck in einem Frame, wahlweise mit Fokus in einem Textfeld.
fn press_face_key(context: &egui::Context, app: &mut PhotoApp, key: egui::Key, focus: bool) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        )),
        events: vec![egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let _ = context.run(input, |ctx| {
        if focus {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("text-field")));
        }
        app.face_shortcuts(ctx);
    });
    context.memory_mut(|m| m.surrender_focus(egui::Id::new("text-field")));
}

#[test]
fn j_confirms_the_top_group_once_unless_a_text_field_has_focus() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let (context, mut app) = review_app(root.path());
    // Ein Textfeld mit Fokus behält die Taste für sich.
    press_face_key(&context, &mut app, egui::Key::J, true);
    assert_eq!(naming_tasks(&app), 0);
    assert_eq!(page_ids(&app), vec![vec![1, 2], vec![3]]);
    press_face_key(&context, &mut app, egui::Key::J, false);
    assert_eq!(naming_tasks(&app), 1);
    assert_eq!(
        page_ids(&app),
        vec![vec![3]],
        "die oberste Gruppe ist vorgemerkt"
    );
    assert!(app.face_keys);
    // Sind alle Kacheln abgewählt, bestätigt J nichts; N lehnt die Gruppe ab.
    app.drafts.insert(
        "p1:3".into(),
        Draft {
            name: "Anna".into(),
            person: Some(1),
            excluded: BTreeSet::from([3]),
        },
    );
    press_face_key(&context, &mut app, egui::Key::J, false);
    assert_eq!(naming_tasks(&app), 1);
    press_face_key(&context, &mut app, egui::Key::N, false);
    assert_eq!(naming_tasks(&app), 2);
    assert!(app.face_page.groups.is_empty());
    // Ohne Gruppe bleibt jede Taste wirkungslos.
    press_face_key(&context, &mut app, egui::Key::J, false);
    press_face_key(&context, &mut app, egui::Key::Delete, false);
    assert_eq!(naming_tasks(&app), 2);
    Ok(())
}

#[test]
fn digits_pick_a_suggestion_and_delete_ignores_the_top_group_in_benennen() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let (context, mut app) = review_app(root.path());
    app.face_mode = "unknown".into();
    app.face_page.groups = vec![vec![test_face(5), test_face(6)], vec![test_face(7)]];
    app.face_page.suggestions.insert(
        5,
        crate::core::recognition::Suggestions {
            items: vec![(1, "Anna".into(), 0.8), (2, "Ben".into(), 0.7)],
            portraits: BTreeMap::new(),
        },
    );
    // Zu knapp für einen sicheren Vorschlag: J und N bleiben ohne Wirkung.
    app.face_page.status.review_threshold = 0.6;
    app.face_page.status.review_margin = 0.15;
    // J und N gelten nur für Vorschläge; einen dritten Vorschlag gibt es nicht.
    for key in [egui::Key::J, egui::Key::N, egui::Key::Num3] {
        press_face_key(&context, &mut app, key, false);
    }
    assert_eq!(naming_tasks(&app), 0);
    press_face_key(&context, &mut app, egui::Key::Num2, true);
    assert_eq!(naming_tasks(&app), 0, "im Textfeld bleibt die Ziffer Text");
    press_face_key(&context, &mut app, egui::Key::Num2, false);
    assert_eq!(naming_tasks(&app), 1);
    assert_eq!(page_ids(&app), vec![vec![7]]);
    press_face_key(&context, &mut app, egui::Key::Delete, false);
    assert_eq!(naming_tasks(&app), 2);
    assert!(app.face_page.groups.is_empty());
    // In „Bestätigt“ gelten die Tasten nicht.
    app.face_mode = "confirmed".into();
    app.face_page.groups = vec![vec![review_face(8, 1.0)]];
    for key in [egui::Key::J, egui::Key::N, egui::Key::Delete] {
        press_face_key(&context, &mut app, key, false);
    }
    assert_eq!(naming_tasks(&app), 2);
    Ok(())
}

fn sure_app(root: &Path) -> (egui::Context, PhotoApp) {
    let (context, mut app) = review_app(root);
    app.face_mode = "unknown".into();
    app.face_page.status.review_threshold = 0.6;
    app.face_page.status.review_margin = 0.05;
    app.face_page.groups = vec![vec![test_face(5), test_face(6)], vec![test_face(7)]];
    app.face_page.suggestions.insert(
        5,
        crate::core::recognition::Suggestions {
            items: vec![(1, "Anna".into(), 0.9), (2, "Ben".into(), 0.7)],
            portraits: BTreeMap::new(),
        },
    );
    app.face_page.suggestions.insert(
        7,
        crate::core::recognition::Suggestions {
            items: vec![(1, "Anna".into(), 0.8), (2, "Ben".into(), 0.79)],
            portraits: BTreeMap::new(),
        },
    );
    (context, app)
}

#[test]
fn j_confirms_a_sure_suggestion_in_benennen_and_only_then() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let (context, mut app) = sure_app(root.path());
    press_face_key(&context, &mut app, egui::Key::J, true);
    assert_eq!(naming_tasks(&app), 0, "im Textfeld bleibt die Taste Text");
    press_face_key(&context, &mut app, egui::Key::J, false);
    assert_eq!(naming_tasks(&app), 1);
    assert_eq!(page_ids(&app), vec![vec![7]]);
    // Die nächste Gruppe hat nur knappe Kandidaten: keine Taste wirkt.
    press_face_key(&context, &mut app, egui::Key::J, false);
    press_face_key(&context, &mut app, egui::Key::N, false);
    assert_eq!(naming_tasks(&app), 1);
    Ok(())
}

#[test]
fn n_rejects_a_sure_suggestion_in_benennen() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let (context, mut app) = sure_app(root.path());
    press_face_key(&context, &mut app, egui::Key::N, true);
    assert_eq!(naming_tasks(&app), 0);
    press_face_key(&context, &mut app, egui::Key::N, false);
    assert_eq!(naming_tasks(&app), 1);
    assert_eq!(page_ids(&app), vec![vec![7]]);
    Ok(())
}

#[test]
fn review_cards_render_with_portrait_filter_and_deselected_tiles() {
    let (context, mut app) = review_app(Path::new("tmp/synthetic-review"));
    app.face_page.portraits.insert(1, test_face(40));
    app.face_page.review_people.insert(1, 3);
    app.review_person = Some(1);
    draw_people(&context, &mut app, egui::vec2(1280.0, 800.0));
    assert_eq!(
        app.face_row_tops.len(),
        2,
        "beide Gruppen wurden gezeichnet"
    );
    let draft = app.drafts.get_mut("p1:1,2").expect("Karte gezeichnet");
    assert_eq!(draft.person, Some(1));
    draft.excluded.insert(2);
    draw_people(&context, &mut app, egui::vec2(980.0, 680.0));
    assert_eq!(naming_tasks(&app), 0, "Zeichnen allein speichert nichts");
}

#[test]
fn plural_uses_singular_only_for_exactly_one() {
    assert_eq!(files_and_days(0, 0), "0 Dateien an 0 Tagen");
    assert_eq!(files_and_days(1, 1), "1 Datei an 1 Tag");
    assert_eq!(files_and_days(30, 2), "30 Dateien an 2 Tagen");
}

#[test]
fn person_choices_show_ids_only_for_equal_names_but_keep_them_searchable() {
    use crate::ui::select::person_choices;
    let person = |id: i64, name: &str| crate::core::faces::Person {
        id,
        name: name.into(),
        birth: "1980".into(),
        count: 3,
        gender: String::new(),
        note: String::new(),
    };
    let people = vec![person(1, "Anna"), person(2, "Ben"), person(3, "anna")];
    let items = person_choices(&people, None);
    assert_eq!(items[0].detail, "* 1980 · 3 Gesichter · #1");
    assert_eq!(items[1].detail, "* 1980 · 3 Gesichter");
    assert_eq!(items[2].detail, "* 1980 · 3 Gesichter · #3");
    assert!(items[1].keywords.contains("#2"));
}

#[test]
fn ctrl_z_starts_exactly_one_undo_unless_a_text_field_has_focus() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.path().to_string_lossy().into_owned();
    app.undo.push(Undo {
        before: Vec::new(),
        after: Vec::new(),
        created: None,
    });
    let press = |app: &mut PhotoApp, focus: bool| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events: vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: Some(egui::Key::Z),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            modifiers: egui::Modifiers::COMMAND,
            ..Default::default()
        };
        let _ = context.run(input, |ctx| {
            if focus {
                ctx.memory_mut(|m| m.request_focus(egui::Id::new("text-field")));
            }
            app.undo_shortcut(ctx);
        });
    };
    let undos = |app: &PhotoApp| app.tasks.iter().filter(|t| t.kind == JobKind::Undo).count();
    // Ein Textfeld mit Fokus behält Strg+Z für sich.
    press(&mut app, true);
    assert_eq!(undos(&app), 0);
    context.memory_mut(|m| m.surrender_focus(egui::Id::new("text-field")));
    press(&mut app, false);
    assert_eq!(undos(&app), 1);
    // Während der Auftrag läuft, ist der Knopf gesperrt und die Taste wirkungslos.
    press(&mut app, false);
    assert_eq!(undos(&app), 1);
    Ok(())
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

fn notice_app() -> PhotoApp {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context);
    PhotoApp::with_settings(&creation, Settings::default())
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

#[test]
fn navigation_lists_four_main_items_and_two_at_the_bottom_without_zeitreise() {
    let main: Vec<&str> = NAV_MAIN.iter().map(|(_, title, _)| *title).collect();
    assert_eq!(main, ["Importieren", "Sammlung", "Gesichter", "Personen"]);
    let bottom: Vec<&str> = NAV_BOTTOM.iter().map(|(_, title, _)| *title).collect();
    assert_eq!(bottom, ["Aufgaben", "Einstellungen"]);
    assert!(
        !main
            .iter()
            .chain(&bottom)
            .any(|title| *title == "Zeitreise")
    );
    let views: Vec<View> = NAV_MAIN.iter().map(|(view, _, _)| *view).collect();
    assert_eq!(
        views,
        [View::Import, View::Collection, View::Faces, View::People]
    );
    // Die Zeitreise ist eine Ansicht der gewählten Person.
    let modes: Vec<&str> = people_view::PERSON_MODES
        .iter()
        .map(|(_, label)| *label)
        .collect();
    assert_eq!(
        modes,
        ["Übersicht", "Stammbaum", "Graph", "Zeitreise", "Gesichter"]
    );
}

#[test]
fn old_start_values_open_the_new_places() {
    use PersonMode::*;
    // Früher: Ansicht „people“ mit einem Reiter aus Gesichtern oder Personen.
    assert_eq!(
        startup_place(Some("people"), Some("review")),
        (Some(View::Faces), Some("review"), None)
    );
    assert_eq!(
        startup_place(Some("people"), None),
        (Some(View::Faces), None, None)
    );
    assert_eq!(
        startup_place(Some("people"), Some("persons")),
        (Some(View::People), None, Some(Overview))
    );
    assert_eq!(
        startup_place(Some("people"), Some("tree")),
        (Some(View::People), None, Some(Tree))
    );
    assert_eq!(
        startup_place(None, Some("graph")),
        (None, None, Some(Graph))
    );
    assert_eq!(
        startup_place(Some("time"), None),
        (Some(View::People), None, Some(Time))
    );
    assert_eq!(
        startup_place(Some("faces"), Some("ignored")),
        (Some(View::Faces), Some("ignored"), None)
    );
    assert_eq!(
        startup_place(Some("settings"), Some("unbekannt")),
        (Some(View::Settings), None, None)
    );
}

#[test]
fn the_sidebar_names_the_collection_folder_and_keeps_the_readable_path() {
    assert_eq!(collection_caption(""), (None, String::new()));
    assert_eq!(
        collection_caption(r"\\?\D:\Bilder\Familie"),
        (Some("Familie".into()), r"D:\Bilder\Familie".into())
    );
    assert_eq!(
        collection_caption(r"\\?\UNC\nas\fotos\Familie"),
        (Some("Familie".into()), r"\\nas\fotos\Familie".into())
    );
    // Ein Laufwerk hat keinen Ordnernamen: dann steht der Pfad da.
    assert_eq!(
        collection_caption(r"D:\"),
        (Some(r"D:\".into()), r"D:\".into())
    );
}

#[test]
fn choosing_a_collection_resets_everything_that_belonged_to_the_old_one() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let (context, mut app) = review_app(Path::new("tmp/old-collection"));
    app.filters.text = "Anna".into();
    app.filters.folder = Some("2024/05".into());
    app.applied = app.filters.clone();
    app.page.total = 7;
    app.face_index = 3;
    app.review_person = Some(1);
    app.drafts.insert("p1:1,2".into(), Draft::default());
    app.undo.push(Undo {
        before: Vec::new(),
        after: Vec::new(),
        created: None,
    });
    app.relation_from = Some(1);
    app.family_ui.focus = Some(1);
    app.relationship_graph.selected = Some(1);
    app.people_ui.query = "anna".into();
    app.people_ui.sort = vec![crate::core::people_table::SortKey {
        column: crate::core::people_table::Column::Faces,
        descending: true,
    }];
    app.show_time_for(1);
    app.maintenance_paused = true;
    app.recognition_tools_open = true;
    let (queries, pages) = (app.query_revision, app.face_revision);

    app.set_collection(root.path().to_string_lossy().into_owned());

    assert_eq!(
        app.settings.collection,
        root.path().to_string_lossy().as_ref()
    );
    assert!(app.filters.text.is_empty() && app.filters.folder.is_none());
    assert!(app.applied.text.is_empty() && app.applied.folder.is_none());
    assert_eq!(app.page.total, 0);
    assert_eq!(app.face_index, 0);
    assert_eq!(app.review_person, None);
    assert!(app.drafts.is_empty() && app.undo.is_empty());
    assert!(app.face_page.groups.is_empty() && app.face_page.people.is_empty());
    assert_eq!(app.relation_from, None);
    assert_eq!(app.family_ui.focus, None);
    assert_eq!(app.relationship_graph.selected, None);
    assert!(app.people_ui.query.is_empty());
    assert_eq!(app.people_ui.sort.len(), 1, "die Sortierung bleibt");
    assert_eq!(app.time_ui.person, None);
    assert!(!app.maintenance_paused && !app.recognition_tools_open);
    // Antworten für die alte Sammlung sind veraltet; die neue wird auf Workern gelesen.
    assert!(app.query_revision > queries && app.face_revision > pages);
    let kinds: Vec<&str> = app.tasks.iter().map(|t| t.kind.as_str()).collect();
    assert!(
        kinds.contains(&"search") && kinds.contains(&"faces"),
        "{kinds:?}"
    );
    assert!(app.tasks.iter().all(|t| !t.write), "nur gelesen");
    settle_tasks(&mut app, &context);
    assert!(app.tasks.is_empty());
    Ok(())
}

#[test]
fn erledigt_switches_between_confirmed_and_ignored_and_remembers_the_choice() {
    let (context, mut app) = review_app(Path::new("tmp/synthetic-review"));
    app.face_index = 2;
    app.open_done();
    assert_eq!(app.face_mode, "confirmed");
    assert_eq!(
        app.face_index, 0,
        "ein Reiter beginnt auf seiner ersten Seite"
    );
    assert!(faces_view::done_tab(&app.face_mode));
    app.set_face_mode("ignored");
    assert_eq!(app.face_mode, "ignored");
    assert_eq!(app.page_mode(), "ignored");
    assert!(faces_view::done_tab(&app.face_mode));
    app.set_face_mode("unknown");
    assert!(!faces_view::done_tab(&app.face_mode));
    app.open_done();
    assert_eq!(app.face_mode, "ignored", "die letzte Wahl gilt wieder");
    // Beide Seiten zeichnen sich mit dem Umschalter in beiden Fenstergrößen.
    for mode in ["confirmed", "ignored"] {
        app.face_mode = mode.into();
        app.face_page.groups = vec![vec![review_face(8, 1.0), review_face(9, 1.0)]];
        for size in [egui::vec2(980.0, 680.0), egui::vec2(1280.0, 800.0)] {
            draw_people(&context, &mut app, size);
            assert_eq!(app.face_row_tops.len(), 1, "{mode} at {size:?}");
        }
    }
    assert_eq!(naming_tasks(&app), 0);
}

#[test]
fn person_views_follow_the_selected_person_and_render_in_both_window_sizes() {
    let (context, mut app) = people_app(12);
    // Ohne Auswahl bleiben die Ansichten erreichbar und nennen den Weg zur Auswahl.
    for (mode, _) in people_view::PERSON_MODES {
        app.person_mode = mode;
        for size in [egui::vec2(980.0, 680.0), egui::vec2(1280.0, 800.0)] {
            draw_people(&context, &mut app, size);
        }
    }
    app.select_person(5);
    app.set_person_mode(PersonMode::Tree);
    assert_eq!(app.family_ui.focus, Some(5));
    assert_eq!(app.page_mode(), "tree");
    app.set_person_mode(PersonMode::Graph);
    assert_eq!(app.relationship_graph.selected, Some(5));
    assert_eq!(app.page_mode(), "graph");
    app.set_person_mode(PersonMode::Time);
    assert_eq!(app.time_ui.person, Some(5));
    app.set_person_mode(PersonMode::Overview);
    assert_eq!(app.page_mode(), "persons");
    assert_eq!(app.relation_from, Some(5));
    // Eine Person, die es in der Sammlung nicht gibt, öffnet nichts.
    app.relation_from = Some(999);
    app.set_person_mode(PersonMode::Time);
    assert_eq!(app.time_ui.person, Some(5));
    // Außerhalb von „Personen“ liest die App den Reiter der Ansicht „Gesichter“.
    app.view = View::Faces;
    assert_eq!(app.page_mode(), "unknown");
}

#[test]
fn a_running_face_task_is_shown_by_the_footer_in_the_faces_view() {
    let (context, mut app) = review_app(Path::new("tmp/synthetic-review"));
    app.launch(
        "Fotos lokal analysieren",
        true,
        JobKind::FaceAnalysis,
        |_| Ok(Outcome::Done(String::new())),
    );
    let shown = PhotoApp::footer_task(app.view, &app.tasks).map(|t| t.kind);
    assert_eq!(shown, Some(JobKind::FaceAnalysis));
    draw_people(&context, &mut app, egui::vec2(980.0, 680.0));
    // In der Aufgabenansicht steht der Auftrag als Karte da, nicht in der Fußleiste.
    assert!(PhotoApp::footer_task(View::Tasks, &app.tasks).is_none());
}

fn metadata_app(root: &Path) -> PhotoApp {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context);
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.to_string_lossy().into_owned();
    app.status.unread = 5;
    app.status.pending = 5;
    app.status.files = 9;
    // Von Hand gesetzte Zählungen gelten als frisch gezählt.
    app.counts_fresh = true;
    app
}
fn tasks_of(app: &PhotoApp, kind: &str) -> usize {
    app.tasks.iter().filter(|t| t.kind.as_str() == kind).count()
}
fn wait_for_tasks(app: &mut PhotoApp) {
    settle_tasks(app, &egui::Context::default());
}

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
fn filter_dialog_changes_apply_at_once_without_a_draft() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let mut app = metadata_app(root.path());
    app.filters.text = "Anna".into();
    app.apply_collection_search();
    wait_for_tasks(&mut app);
    assert_eq!(app.applied.text, "Anna");
    // Getippt, aber nicht gesucht: der Suchtext bleibt beim Filtern außen vor.
    app.filters.text = "Bert".into();
    app.filters.oldest = true;
    app.apply_filter_dialog();
    assert_eq!(tasks_of(&app, "search"), 1);
    // Eine Neuanfrage im Hintergrund (z. B. nach einem Lauf) verliert die Änderung nicht.
    app.requery_collection();
    wait_for_tasks(&mut app);
    assert!(app.applied.oldest);
    assert_eq!(app.applied.text, "Anna");
    // Ohne Unterschied und mit unvollständigem Kriterium wird nichts gelesen.
    app.apply_filter_dialog();
    app.filters.rules.push(Rule {
        field: "camera".into(),
        operator: "contains".into(),
        value: String::new(),
    });
    app.apply_filter_dialog();
    assert_eq!(tasks_of(&app, "search"), 0);
    app.filters.rules[0].value = "Canon".into();
    app.apply_filter_dialog();
    wait_for_tasks(&mut app);
    assert_eq!(app.applied.rules.len(), 1);
    // „Suchen“ übernimmt den getippten Text samt Kriterien.
    app.apply_collection_search();
    wait_for_tasks(&mut app);
    assert_eq!(app.applied.text, "Bert");
    assert!(app.applied.oldest && app.applied.rules.len() == 1);
    Ok(())
}

/// Ein Frame mit Eingabeereignissen; liefert Beschriftung und Grenzen aller Bedienelemente
/// (über AccessKit, damit Tests Kacheln und Menüeinträge wie ein Nutzer treffen).
fn frame_with(
    context: &egui::Context,
    app: &mut PhotoApp,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    context.enable_accesskit();
    let output = context.run(
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
    output
        .platform_output
        .accesskit_update
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let label = node.label()?.to_string();
                    let b = node.bounds()?;
                    Some((
                        label,
                        egui::Rect::from_min_max(
                            egui::pos2(b.x0 as f32, b.y0 as f32),
                            egui::pos2(b.x1 as f32, b.y1 as f32),
                        ),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Klickt das Bedienelement mit dieser Beschriftung (ein Frame drücken, einer loslassen).
fn click_labelled(
    context: &egui::Context,
    app: &mut PhotoApp,
    label: &str,
    button: egui::PointerButton,
) {
    let widgets = frame_with(context, app, Vec::new());
    let rect = widgets
        .iter()
        .find(|(text, _)| text == label)
        .unwrap_or_else(|| panic!("{label} nicht gezeichnet"))
        .1;
    let pos = rect.center();
    let press = |pressed| egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame_with(
        context,
        app,
        vec![egui::Event::PointerMoved(pos), press(true)],
    );
    frame_with(context, app, vec![press(false)]);
}

/// Unbenannte Gruppe aus drei Gesichtern in einer temporären Sammlung, Ansicht „Gesichter“.
fn single_naming_app(root: &Path) -> anyhow::Result<(egui::Context, PhotoApp)> {
    let c = Collection::open(root, true)?;
    for id in 1..=3_i64 {
        c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
        c.db.execute(
            "INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality) VALUES(?,?,?,'det',0,0,1,1,1)",
            rusqlite::params![id, id, format!("{id}.jpg")],
        )?;
    }
    drop(c);
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.to_string_lossy().into_owned();
    app.view = View::Faces;
    app.face_mode = "unknown".into();
    app.face_page.groups = vec![
        (1..=3)
            .map(|id| Face {
                source: format!("{id}.jpg"),
                ..test_face(id)
            })
            .collect(),
    ];
    app.face_page.total_groups = 1;
    app.face_page.status.nameable = 3;
    Ok((context, app))
}

#[test]
fn right_click_on_a_faces_tile_names_just_that_face_without_the_collection_view()
-> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let (context, mut app) = single_naming_app(root.path())?;
    // Rechtsklick auf die mittlere Kachel öffnet das Menü, der Eintrag den Dialog.
    click_labelled(
        &context,
        &mut app,
        "Gesichtsausschnitt in 2.jpg",
        egui::PointerButton::Secondary,
    );
    assert_eq!(app.naming, None, "erst der Menüeintrag öffnet den Dialog");
    click_labelled(
        &context,
        &mut app,
        "Einzeln benennen …",
        egui::PointerButton::Primary,
    );
    assert_eq!(app.naming, Some(2));
    assert!(app.selected_photo.is_none(), "Sammlung nie geöffnet");
    // Der Rechtsklick wählt die Kachel nicht ab.
    assert!(app.drafts.values().all(|d| d.excluded.is_empty()));
    assert!(app.tasks.iter().any(|t| t.kind == JobKind::Suggestions));
    wait_for_tasks(&mut app);
    // Name eintragen und übernehmen: nur dieses Gesicht, der Dialog schließt sofort.
    app.name_input = "Bert".into();
    click_labelled(
        &context,
        &mut app,
        "Übernehmen",
        egui::PointerButton::Primary,
    );
    assert_eq!(app.naming, None);
    assert_eq!(naming_tasks(&app), 1);
    assert_eq!(page_ids(&app), vec![vec![1, 3]], "sofort aus der Gruppe");
    let revisions = |root: &Path| -> anyhow::Result<crate::core::revision::Revisions> {
        let c = Collection::open(root, false)?;
        Ok(crate::core::revision::read(&c.db)?.expect("Revision"))
    };
    let before = revisions(root.path())?;
    app.time_ui.observe(&before);
    wait_for_tasks(&mut app);
    assert_eq!(app.undo.len(), 1, "Strg+Z kann die Benennung zurücknehmen");
    // Die neu gelesene Gesichterseite trägt den neuen Datenstand in die Zeitreise.
    let after = revisions(root.path())?;
    assert!(after.faces > before.faces);
    assert!(
        app.time_ui.knows(&after),
        "Zeitreise kennt den Stand nach der Benennung"
    );
    let c = Collection::open(root.path(), false)?;
    let rows = c
        .db
        .prepare("SELECT id, person_id IS NOT NULL, IFNULL(assignment,'') FROM faces ORDER BY id")?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert_eq!(
        rows,
        vec![
            (1, false, String::new()),
            (2, true, "user".into()),
            (3, false, String::new()),
        ]
    );
    let rejections: i64 =
        c.db.query_row("SELECT COUNT(*) FROM face_rejections", [], |r| r.get(0))?;
    assert_eq!(
        rejections, 0,
        "die übrigen Gesichter bekommen keine Ablehnung"
    );
    Ok(())
}

/// Zeichnet ein Bild und liefert alle darin gezeigten Texte.
fn drawn_texts(context: &egui::Context, app: &mut PhotoApp, events: Vec<egui::Event>) -> String {
    fn collect(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                out.push_str(text.galley.text());
                out.push('\n');
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    let output = context.run(input, |ctx| {
        app.update(ctx, &mut eframe::Frame::_new_kittest())
    });
    let mut texts = String::new();
    for clipped in &output.shapes {
        collect(&clipped.shape, &mut texts);
    }
    texts
}

#[test]
fn a_folder_without_collection_asks_before_anything_is_written() -> anyhow::Result<()> {
    let folder = tempfile::tempdir()?;
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = "tmp/old-collection".into();
    let chosen = folder.path().to_string_lossy().into_owned();

    app.choose_collection(chosen.clone());

    assert_eq!(app.settings.collection, "tmp/old-collection");
    assert_eq!(app.new_collection_prompt.as_deref(), Some(chosen.as_str()));
    assert!(!app.tasks.iter().any(|t| t.kind == JobKind::Scan));
    // Ein neues Fenster misst sich im ersten Bild nur aus und erscheint im zweiten.
    drawn_texts(&context, &mut app, Vec::new());
    let texts = drawn_texts(&context, &mut app, Vec::new());
    assert!(
        texts.contains("Hier eine neue Sammlung anlegen?"),
        "{texts}"
    );
    assert!(texts.contains("Neue Sammlung anlegen") && texts.contains("Abbrechen"));
    assert!(texts.contains("Sammlung wechseln …"));

    // Escape schließt den Dialog wie „Abbrechen“: kein Wechsel, keine Datei.
    let escape = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    drawn_texts(&context, &mut app, vec![escape]);
    assert_eq!(app.new_collection_prompt, None);
    assert_eq!(app.settings.collection, "tmp/old-collection");
    assert!(!folder.path().join(".myfotomanager").exists());
    assert!(std::fs::read_dir(folder.path())?.next().is_none());
    Ok(())
}

#[test]
fn confirming_the_new_collection_switches_to_it() -> anyhow::Result<()> {
    let folder = tempfile::tempdir()?;
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    let chosen = folder.path().to_string_lossy().into_owned();
    app.choose_collection(chosen.clone());
    app.confirm_new_collection();
    assert_eq!(app.settings.collection, chosen);
    assert_eq!(app.new_collection_prompt, None);
    settle_tasks(&mut app, &context);
    assert!(folder.path().join(".myfotomanager").is_file());
    Ok(())
}

#[test]
fn an_existing_collection_opens_without_asking() -> anyhow::Result<()> {
    let folder = tempfile::tempdir()?;
    drop(Collection::open(folder.path(), true)?);
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    let chosen = folder.path().to_string_lossy().into_owned();
    app.choose_collection(chosen.clone());
    assert_eq!(app.new_collection_prompt, None);
    assert_eq!(app.settings.collection, chosen);
    settle_tasks(&mut app, &context);
    Ok(())
}

#[test]
fn the_import_source_and_memory_cards_are_refused_as_collection() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("Quelle");
    let card = root.path().join("Karte").join("DCIM").join("103TEST");
    std::fs::create_dir_all(source.join("Tag"))?;
    std::fs::create_dir_all(&card)?;
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.source = source.to_string_lossy().into_owned();
    app.settings.collection = "tmp/old-collection".into();
    for folder in [source.clone(), source.join("Tag"), card.clone()] {
        app.notice = None;
        app.choose_collection(folder.to_string_lossy().into_owned());
        assert_eq!(app.settings.collection, "tmp/old-collection", "{folder:?}");
        assert_eq!(app.new_collection_prompt, None, "{folder:?}");
        let notice = app.notice.as_ref().expect("Fehlerhinweis");
        assert!(notice.error, "{folder:?}");
        assert!(
            notice.text.contains("Importquelle") || notice.text.contains("Speicherkarte"),
            "{}",
            notice.text
        );
        assert!(!folder.join(".myfotomanager").exists(), "{folder:?}");
    }
    assert!(!app.tasks.iter().any(|t| t.kind == JobKind::Scan));
    assert!(std::fs::read_dir(&card)?.next().is_none());
    // Der Ordner um die Quelle (Sammlung mit Zwischenordner wie `__einzuordnen`) ist erlaubt.
    app.notice = None;
    let parent = root.path().to_string_lossy().into_owned();
    app.choose_collection(parent.clone());
    assert!(app.notice.as_ref().is_none_or(|n| !n.error));
    assert_eq!(app.new_collection_prompt.as_deref(), Some(parent.as_str()));
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

/// Synthetische Sammlung mit Anna und Bernd, Graph geladen, Verbindungsmodus an.
fn connect_app(root: &Path) -> anyhow::Result<(egui::Context, PhotoApp, i64, i64)> {
    let mut collection = Collection::open(root, true)?;
    let mut person = |name: &str, gender: &str| {
        faces::add_person(
            &mut collection,
            &faces::NewPerson {
                name: name.into(),
                gender: gender.into(),
                birth_year: None,
                note: String::new(),
            },
        )
    };
    let anna = person("Anna", "weiblich")?;
    let bernd = person("Bernd", "männlich")?;
    let network = crate::core::relationship_graph::network(&collection)?;
    drop(collection);
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.to_string_lossy().into_owned();
    app.view = View::People;
    app.person_mode = PersonMode::Graph;
    app.relationship_graph.accept(network);
    app.relationship_graph.connect = true;
    // Geschlecht und Bestand für die Vorprüfung kommen wie beim Öffnen der Sammlung vom Worker.
    app.refresh_faces();
    wait_for_tasks(&mut app);
    graph_frame(&context, &mut app, Vec::new());
    wait_for_tasks(&mut app);
    graph_frame(&context, &mut app, Vec::new());
    Ok((context, app, anna, bernd))
}

fn graph_frame(
    context: &egui::Context,
    app: &mut PhotoApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    context.run(input, |ctx| {
        app.update(ctx, &mut eframe::Frame::_new_kittest())
    })
}

fn click_at(context: &egui::Context, app: &mut PhotoApp, at: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    graph_frame(context, app, vec![egui::Event::PointerMoved(at)]);
    graph_frame(context, app, vec![button(true)]);
    graph_frame(context, app, vec![button(false)]);
}

/// Bildschirmposition eines Knotens im zuletzt gezeichneten Graph.
fn node_on_screen(app: &PhotoApp, person: i64) -> egui::Pos2 {
    let view = &app.relationship_graph;
    let network = view.data.as_ref().expect("Graph geladen");
    let (rect, centre, scale) = view.drawn.expect("Graph gezeichnet");
    let p = network.positions[network.index_of(person).expect("Person im Graph")];
    egui::pos2(
        rect.center().x + (p[0] - centre[0]) * scale,
        rect.center().y + (p[1] - centre[1]) * scale,
    )
}

/// Mitte des ersten gezeichneten Textes, der genau `needle` lautet.
fn text_centre(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
    fn find(shape: &egui::Shape, needle: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == needle => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, needle)),
            _ => None,
        }
    }
    output.shapes.iter().find_map(|c| find(&c.shape, needle))
}

fn open_connect_menu(
    context: &egui::Context,
    app: &mut PhotoApp,
    first: i64,
    second: i64,
) -> egui::FullOutput {
    let at = node_on_screen(app, first);
    click_at(context, app, at);
    assert_eq!(app.relationship_graph.connect_first, Some(first));
    assert!(app.relationship_graph.connect_menu.is_none());
    let at = node_on_screen(app, second);
    click_at(context, app, at);
    let menu = app.relationship_graph.connect_menu.expect("Menü offen");
    assert_eq!((menu.first, menu.second), (first, second));
    // Ein neues Menü misst sich im ersten Bild nur aus.
    graph_frame(context, app, Vec::new());
    graph_frame(context, app, Vec::new())
}

#[test]
fn connect_mode_adds_the_chosen_relation_through_the_normal_path() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let (context, mut app, anna, bernd) = connect_app(root.path())?;
    let output = open_connect_menu(&context, &mut app, anna, bernd);
    // Beide Richtungen, nach Geschlecht beschriftet, dazu „Abbrechen“.
    assert!(text_centre(&output, "Bernd ist Vater von Anna").is_some());
    assert!(text_centre(&output, "Anna ist Partnerin von Bernd").is_some());
    assert!(text_centre(&output, "Abbrechen").is_some());
    let button = text_centre(&output, "Anna ist Mutter von Bernd").expect("Eintrag im Menü");
    click_at(&context, &mut app, button);
    assert!(
        app.tasks
            .iter()
            .any(|t| t.kind == JobKind::Mutation && t.write),
        "dieselbe Schreibaufgabe wie in der Übersicht"
    );
    wait_for_tasks(&mut app);
    let edges = crate::core::relations::load_edges(&Collection::open(root.path(), false)?)?;
    assert_eq!(
        edges,
        vec![crate::core::relations::canonical(
            anna,
            crate::core::relations::Role::Parent,
            bernd
        )]
    );
    assert_eq!(
        app.notice.as_ref().map(|n| n.text.as_str()),
        Some("Beziehung gespeichert")
    );
    // Der Modus bleibt für das nächste Paar an; der Graph wird neu berechnet.
    let view = &app.relationship_graph;
    assert!(view.connect && view.connect_first.is_none() && view.connect_menu.is_none());
    assert!(view.stale);
    // Was die Prüfung ablehnt (Partner zwischen Eltern und Kind), ist gesperrt.
    graph_frame(&context, &mut app, Vec::new());
    wait_for_tasks(&mut app);
    graph_frame(&context, &mut app, Vec::new());
    let output = open_connect_menu(&context, &mut app, anna, bernd);
    let partner = text_centre(&output, "Anna ist Partnerin von Bernd").expect("Eintrag im Menü");
    click_at(&context, &mut app, partner);
    assert!(
        app.relationship_graph.connect_menu.is_some(),
        "Menü bleibt offen"
    );
    assert!(!app.tasks.iter().any(|t| t.write), "nichts geschrieben");
    Ok(())
}

#[test]
fn connect_mode_cancels_without_writing() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let (context, mut app, anna, bernd) = connect_app(root.path())?;
    let escape = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    // „Abbrechen“ im Menü.
    let output = open_connect_menu(&context, &mut app, anna, bernd);
    let cancel = text_centre(&output, "Abbrechen").expect("Abbrechen im Menü");
    click_at(&context, &mut app, cancel);
    assert!(app.relationship_graph.connect_menu.is_none());
    assert!(app.relationship_graph.connect_first.is_none());
    // Esc bricht das offene Menü ab, ein zweites Esc beendet den Modus.
    open_connect_menu(&context, &mut app, bernd, anna);
    graph_frame(&context, &mut app, vec![escape.clone()]);
    assert!(app.relationship_graph.connect_menu.is_none());
    assert!(app.relationship_graph.connect);
    // Erneuter Klick auf dieselbe Person hebt die Wahl auf.
    let at = node_on_screen(&app, anna);
    click_at(&context, &mut app, at);
    assert_eq!(app.relationship_graph.connect_first, Some(anna));
    click_at(&context, &mut app, at);
    assert_eq!(app.relationship_graph.connect_first, None);
    // Klick auf den Hintergrund bricht ab; im Modus ändert ein Klick die Auswahl nicht.
    click_at(&context, &mut app, at);
    let view = &app.relationship_graph;
    let (rect, _, _) = view.drawn.expect("gezeichnet");
    click_at(
        &context,
        &mut app,
        rect.left_bottom() + egui::vec2(6.0, -6.0),
    );
    assert_eq!(app.relationship_graph.connect_first, None);
    assert_eq!(app.relationship_graph.selected, None);
    graph_frame(&context, &mut app, vec![escape]);
    assert!(!app.relationship_graph.connect);
    assert!(!app.tasks.iter().any(|t| t.write), "nichts geschrieben");
    let edges = crate::core::relations::load_edges(&Collection::open(root.path(), false)?)?;
    assert!(edges.is_empty());
    // Ohne Verbindungsmodus wählt ein Klick die Person wie bisher.
    click_at(&context, &mut app, at);
    assert_eq!(app.relationship_graph.selected, Some(anna));
    Ok(())
}

/// Temporäre Sammlung mit drei Fotos: Anna (1) auf allen, Ben (2) auf zweien, Carla (3) auf
/// einem; Ansicht „Personen“ mit gelesener Personenseite.
fn insights_app(root: &Path) -> anyhow::Result<(egui::Context, PhotoApp)> {
    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        std::fs::write(root.join(name), b"photo")?;
    }
    let mut c = Collection::open(root, true)?;
    c.quick_scan(false, &crate::core::jobs::Control::default())?;
    let photos: Vec<i64> = metadata::search(&c, &Search::default(), "")?
        .photos
        .iter()
        .map(|p| p.id)
        .collect();
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2),(3);
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben'),(3,'name','Carla'),
             (1,'birth_year','2000');",
    )?;
    for (photo, person) in [
        (photos[0], 1),
        (photos[1], 1),
        (photos[2], 1),
        (photos[0], 2),
        (photos[1], 2),
        (photos[2], 3),
    ] {
        c.db.execute(
            "INSERT INTO faces(photo_id,source_relpath,detector_id,person_id,assignment,bbox_x1,bbox_y1,bbox_x2,bbox_y2)
             VALUES(?,'a.jpg','det',?,'user',0.1,0.1,0.5,0.5)",
            rusqlite::params![photo, person],
        )?;
    }
    for (photo, date) in [
        (photos[0], "2015:05:01 10:00:00"),
        (photos[1], "2015:08:01 10:00:00"),
        (photos[2], "2017:01:01 10:00:00"),
    ] {
        c.db.execute(
            "UPDATE metadata_photos SET dt_original=? WHERE photo_id=?",
            rusqlite::params![date, photo],
        )?;
    }
    let page = faces::page(&c, "persons", 0)?;
    drop(c);
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.to_string_lossy().into_owned();
    app.view = View::People;
    app.person_mode = PersonMode::Overview;
    app.face_page = page;
    app.face_page_serial += 1;
    Ok((context, app))
}

#[test]
fn companions_list_the_most_shared_photos_and_open_exactly_those() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let (context, mut app) = insights_app(root.path())?;
    app.select_person(1);
    let texts = drawn_texts(&context, &mut app, Vec::new());
    assert!(texts.contains("Häufig zusammen mit"), "{texts}");
    assert_eq!(tasks_of(&app, "person-insights"), 1, "ein Leseauftrag");
    wait_for_tasks(&mut app);
    drawn_texts(&context, &mut app, Vec::new());
    let texts = drawn_texts(&context, &mut app, Vec::new());
    assert_eq!(tasks_of(&app, "person-insights"), 0, "nicht je Bild neu");
    let ben = texts.find("Ben").expect("Ben genannt");
    let carla = texts.find("Carla").expect("Carla genannt");
    assert!(ben < carla, "meiste gemeinsame Fotos zuerst: {texts}");
    click_labelled(&context, &mut app, "2 Fotos", egui::PointerButton::Primary);
    assert_eq!(app.view, View::Collection);
    assert_eq!(app.filters.people_all, vec![1, 2]);
    assert!(app.tasks.iter().all(|t| !t.write), "nur gelesen");
    Ok(())
}

#[test]
fn photos_per_year_show_the_age_and_open_that_year() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let (context, mut app) = insights_app(root.path())?;
    app.select_person(1);
    drawn_texts(&context, &mut app, Vec::new());
    wait_for_tasks(&mut app);
    let texts = drawn_texts(&context, &mut app, Vec::new());
    assert!(texts.contains("Fotos je Jahr"), "{texts}");
    // Anna ist 2000 geboren: Alter an den Rändern, Lückenjahr 2016 ohne Klick.
    assert!(texts.contains("2015 · 15 J."), "{texts}");
    assert!(texts.contains("2017 · 17 J."), "{texts}");
    assert!(texts.contains("Meiste Fotos 2015 (2)"), "{texts}");
    click_labelled(
        &context,
        &mut app,
        "2015: 2 Fotos",
        egui::PointerButton::Primary,
    );
    assert_eq!(app.view, View::Collection);
    assert_eq!(app.filters.people_all, vec![1]);
    assert_eq!(
        (app.filters.start.as_str(), app.filters.end.as_str()),
        ("2015-01-01", "2015-12-31")
    );
    Ok(())
}

/// Anna (1, drei Gesichter) und Ana (2, ein Gesicht) mit fast gleichen Gesichtsmerkmalen, Ben
/// (3) mit anderen; Ansicht „Personen“, Rückfragen offen.
fn duplicates_app(root: &Path) -> anyhow::Result<(egui::Context, PhotoApp)> {
    let c = Collection::open(root, true)?;
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2),(3);
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ana'),(3,'name','Ben');
         INSERT INTO face_settings VALUES('rust.embedder_key','\"emb\"');",
    )?;
    let faces_of = [(1, 0.0), (1, 0.1), (1, 0.2), (2, 0.05), (3, 5.0), (3, 6.0)];
    for (face, (person, wobble)) in (1_i64..).zip(faces_of) {
        c.db.execute(
            "INSERT INTO faces(id,photo_id,source_relpath,detector_id,quality,person_id,assignment,bbox_x1,bbox_y1,bbox_x2,bbox_y2)
             VALUES(?,?,'a.jpg','det',1,?,'user',0.1,0.1,0.5,0.5)",
            rusqlite::params![face, face, person],
        )?;
        let vector = faces::encode(&[1.0, wobble, 0.0, 0.0]);
        c.db.execute(
            "INSERT INTO face_embeddings VALUES(?,'emb',4,?)",
            rusqlite::params![face, vector],
        )?;
    }
    let page = faces::page(&c, "persons", 0)?;
    drop(c);
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context.clone());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.to_string_lossy().into_owned();
    app.view = View::People;
    app.person_mode = PersonMode::Overview;
    app.face_page = page;
    app.face_page_serial += 1;
    app.people_ui.panel = people_table::Panel::Questions;
    drawn_texts(&context, &mut app, Vec::new());
    assert_eq!(tasks_of(&app, "person-duplicates"), 1, "ein Leseauftrag");
    wait_for_tasks(&mut app);
    Ok((context, app))
}

#[test]
fn a_likely_duplicate_is_asked_and_merging_opens_the_existing_confirmation() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let (context, mut app) = duplicates_app(root.path())?;
    let texts = drawn_texts(&context, &mut app, Vec::new());
    assert!(
        texts.contains("Sind Anna und Ana dieselbe Person?"),
        "{texts}"
    );
    assert!(!texts.contains("Ben und"), "{texts}");
    assert_eq!(app.all_questions(), 1);
    assert_eq!(tasks_of(&app, "person-duplicates"), 0, "nicht je Bild neu");
    click_labelled(
        &context,
        &mut app,
        "Zusammenführen …",
        egui::PointerButton::Primary,
    );
    // Ana hat weniger Gesichter und geht in Anna über; erst die Bestätigung schreibt.
    assert_eq!(app.relation_from, Some(2));
    assert_eq!(app.people_ui.panel, people_table::Panel::Details);
    // Die Bestätigung scrollt (animiert) ins Sichtfeld.
    for _ in 0..60 {
        drawn_texts(&context, &mut app, Vec::new());
    }
    let texts = drawn_texts(&context, &mut app, Vec::new());
    assert!(
        texts.contains("„Ana“ (#2) wirklich in „Anna“ (#1) zusammenführen?"),
        "{texts}"
    );
    assert!(
        app.tasks.iter().all(|t| !t.write),
        "noch nichts geschrieben"
    );
    Ok(())
}

#[test]
fn different_hides_the_question_at_once_and_stores_the_answer() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let (context, mut app) = duplicates_app(root.path())?;
    click_labelled(
        &context,
        &mut app,
        "Verschieden",
        egui::PointerButton::Primary,
    );
    assert_eq!(app.all_questions(), 0, "sofort ausgeblendet");
    assert_eq!(app.tasks.iter().filter(|t| t.write).count(), 1);
    wait_for_tasks(&mut app);
    let c = Collection::open(root.path(), false)?;
    assert_eq!(
        crate::core::duplicates::load_declined(&c)?
            .into_iter()
            .collect::<Vec<_>>(),
        [(1, 2)]
    );
    assert!(crate::core::duplicates::load(&c)?.is_empty());
    Ok(())
}

/// Reiter „Ungeeignet“: Kacheln beginnen abgewählt; nur ausgewählte Gesichter werden benannt
/// oder als „Kein Gesicht“ gespeichert, die übrigen bleiben unberührt sichtbar.
#[test]
fn unsuitable_cards_act_only_on_the_selected_faces() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let (context, mut app) = review_app(root.path());
    app.face_mode = "unsuitable".into();
    app.face_page.groups = vec![vec![test_face(1), test_face(2), test_face(3)]];
    app.face_page.total_groups = 1;
    app.face_page.status.unknown = 3;
    draw_people(&context, &mut app, egui::vec2(1280.0, 800.0));
    let group = app.face_page.groups[0].clone();
    let draft = app.drafts.get_mut("1,2,3").expect("Karte gezeichnet");
    assert_eq!(
        draft.excluded,
        BTreeSet::from([1, 2, 3]),
        "nichts ausgewählt"
    );
    // Ohne Auswahl und ohne Namen geschieht nichts.
    assert!(!app.decide_group(&group, faces_view::Decision::NotFace));
    app.drafts.get_mut("1,2,3").expect("Entwurf").excluded = BTreeSet::from([1, 3]);
    assert!(!app.decide_group(&group, faces_view::Decision::NameSelected));
    assert!(app.decide_group(&group, faces_view::Decision::NotFace));
    assert_eq!(naming_tasks(&app), 1);
    assert_eq!(page_ids(&app), vec![vec![1, 3]]);
    assert_eq!(app.face_page.status.unknown, 2);
    // Die verkleinerte Karte hat einen eigenen Entwurf.
    let group = app.face_page.groups[0].clone();
    app.drafts.insert(
        "1,3".into(),
        Draft {
            person: Some(1),
            excluded: BTreeSet::from([3]),
            ..Default::default()
        },
    );
    assert!(app.decide_group(&group, faces_view::Decision::NameSelected));
    assert_eq!(app.hidden_faces(), BTreeSet::from([1, 2]));
    assert_eq!(page_ids(&app), vec![vec![3]]);
    Ok(())
}

/// Gesichterseite einer Person: „Wahrscheinlich X“ entscheidet wie „Vorschläge prüfen“ (auch
/// per Taste), „Bestätigt“ entfernt nur abgewählte Gesichter.
#[test]
fn person_faces_page_confirms_probable_and_removes_only_deselected_faces() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let (context, mut app) = review_app(root.path());
    app.relation_from = Some(1);
    app.set_person_mode(PersonMode::Faces);
    assert_eq!((app.page_mode(), app.page_person()), ("probable", Some(1)));
    assert!(
        app.face_page.groups.is_empty(),
        "keine Karten einer anderen Seite"
    );
    app.tasks.clear();
    app.face_page.groups = vec![
        vec![review_face(1, 0.8), review_face(2, 0.62)],
        vec![review_face(3, 0.6)],
    ];
    draw_people(&context, &mut app, egui::vec2(1280.0, 800.0));
    assert_eq!(app.face_row_tops.len(), 2, "beide Karten gezeichnet");
    press_face_key(&context, &mut app, egui::Key::J, false);
    assert_eq!(naming_tasks(&app), 1);
    assert_eq!(page_ids(&app), vec![vec![3]]);
    app.set_person_face_mode("person-confirmed");
    assert_eq!(app.page_mode(), "person-confirmed");
    app.tasks.retain(|t| t.kind == JobKind::Naming);
    let confirmed = |id| Face {
        assignment: Some("user".into()),
        ..review_face(id, 1.0)
    };
    app.face_page.groups = vec![vec![confirmed(8), confirmed(9)]];
    draw_people(&context, &mut app, egui::vec2(1280.0, 800.0));
    let group = app.face_page.groups[0].clone();
    // Ohne Abwahl gibt es nichts zu entfernen.
    assert!(!app.decide_group(&group, faces_view::Decision::RemoveExcluded));
    app.drafts
        .get_mut("p1:8,9")
        .expect("Karte gezeichnet")
        .excluded = BTreeSet::from([9]);
    assert!(app.decide_group(&group, faces_view::Decision::RemoveExcluded));
    assert_eq!(page_ids(&app), vec![vec![8]]);
    assert!(app.hidden_faces().contains(&9) && !app.hidden_faces().contains(&8));
    // Tasten gelten auf der Seite „Bestätigt“ nicht.
    press_face_key(&context, &mut app, egui::Key::N, false);
    assert_eq!(naming_tasks(&app), 2);
    Ok(())
}

#[test]
fn person_details_open_the_faces_page_for_that_person() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let (context, mut app) = insights_app(root.path())?;
    app.select_person(2);
    drawn_texts(&context, &mut app, Vec::new());
    click_labelled(
        &context,
        &mut app,
        "Gesichter prüfen",
        egui::PointerButton::Primary,
    );
    assert_eq!(
        (app.view, app.person_mode),
        (View::People, PersonMode::Faces)
    );
    assert_eq!((app.page_mode(), app.page_person()), ("probable", Some(2)));
    assert_eq!(
        tasks_of(&app, "faces"),
        1,
        "die Seite wird auf einem Worker gelesen"
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

#[test]
fn the_missing_files_dialog_lists_folders_and_removes_after_confirmation() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    for dir in ["Alt", "Bleibt"] {
        std::fs::create_dir_all(root.path().join(dir))?;
        for n in 0..3 {
            std::fs::write(
                root.path().join(dir).join(format!("{n}.jpg")),
                format!("{dir}{n}"),
            )?;
        }
    }
    {
        let mut c = Collection::open(root.path(), true)?;
        c.quick_scan(false, &Control::default())?;
        std::fs::remove_dir_all(root.path().join("Alt"))?;
        c.quick_scan(true, &Control::default())?;
        assert_eq!(c.status()?.missing, 3);
    }
    let creation = eframe::CreationContext::_new_kittest(egui::Context::default());
    let mut app = PhotoApp::with_settings(&creation, Settings::default());
    app.settings.collection = root.path().to_string_lossy().into_owned();
    wait_for_tasks(&mut app);
    app.view = View::Collection;
    app.open_missing();
    wait_for_tasks(&mut app);
    let folders = app.missing.folders.clone().expect("gelesen");
    assert_eq!(folders.len(), 1);
    assert_eq!((folders[0].dir.as_str(), folders[0].files), ("Alt", 3));
    let draw = |app: &mut PhotoApp| {
        let ctx = egui::Context::default();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(980.0, 680.0),
                )),
                ..Default::default()
            },
            |ctx| app.update(ctx, &mut eframe::Frame::_new_kittest()),
        );
    };
    draw(&mut app);
    app.missing.selected.insert("Alt".into());
    app.missing.confirm = true;
    draw(&mut app);
    assert_eq!(app.missing.chosen_impact(), (3, 3, 0));
    app.remove_missing();
    wait_for_tasks(&mut app);
    let c = Collection::open(root.path(), false)?;
    assert_eq!(c.status()?.missing, 0);
    assert_eq!(c.status()?.files, 3, "vorhandene Dateien bleiben");
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
