//! Sammlung: Start, Suche, Ordnerbaum, Navigation, Wechsel und Anlegen der Sammlung.
use super::*;

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
