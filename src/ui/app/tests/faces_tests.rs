//! Gesichter: Gruppen, Tasten, vorgemerkte Entscheidungen, Einzelbenennung, Personenseiten.
use super::*;

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

/// „Vorschläge prüfen“ mit einer Gruppe aus drei Vorschlägen für Anna (1) in einer
/// temporären Sammlung.
fn tile_icon_app(root: &Path) -> anyhow::Result<(egui::Context, PhotoApp)> {
    let c = Collection::open(root, true)?;
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1);
         INSERT INTO person_attrs(person_id,key,value) VALUES(1,'name','Anna');",
    )?;
    for id in 1..=3_i64 {
        c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
        c.db.execute(
            "INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality,person_id,assignment) VALUES(?,?,?,'det',0,0,1,1,1,1,'suggested')",
            rusqlite::params![id, id, format!("{id}.jpg")],
        )?;
    }
    drop(c);
    let (context, mut app) = review_app(root);
    app.face_page.groups = vec![
        (1..=3)
            .map(|id| Face {
                source: format!("{id}.jpg"),
                ..review_face(id, 0.6)
            })
            .collect(),
    ];
    app.face_page.total_groups = 1;
    app.face_page.status.review = 3;
    Ok((context, app))
}

/// Mitte des Kachelsymbols `icon` auf der Kachel des Gesichts aus `source`.
fn tile_icon_pos(
    context: &egui::Context,
    app: &mut PhotoApp,
    source: &str,
    icon: &str,
) -> egui::Pos2 {
    let widgets = frame_with(context, app, Vec::new());
    let tile = widgets
        .iter()
        .find(|(text, _)| *text == format!("Gesichtsausschnitt in {source}"))
        .unwrap_or_else(|| panic!("Kachel {source} nicht gezeichnet: {:?}", page_ids(app)))
        .1;
    widgets
        .iter()
        .find(|(text, rect)| text == icon && tile.contains(rect.center()))
        .unwrap_or_else(|| panic!("{icon} auf {source} nicht gezeichnet"))
        .1
        .center()
}

/// Ein (`clicks` = 1) oder zwei schnelle Linksklicks an `pos`.
fn click_at(context: &egui::Context, app: &mut PhotoApp, pos: egui::Pos2, clicks: usize) {
    let press = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    // Pause länger als die Doppelklickfrist (je Bild 1/60 s), damit frühere Klicks nicht
    // mitzählen.
    for _ in 0..30 {
        frame_with(context, app, vec![egui::Event::PointerGone]);
    }
    frame_with(context, app, vec![egui::Event::PointerMoved(pos)]);
    for _ in 0..clicks {
        frame_with(context, app, vec![press(true)]);
        frame_with(context, app, vec![press(false)]);
    }
    // Maus weg, damit spätere Bilder keinen Rest dieses Klicks sehen.
    frame_with(context, app, vec![egui::Event::PointerGone]);
}

fn face_rows(root: &Path) -> anyhow::Result<Vec<(i64, Option<i64>, String)>> {
    let c = Collection::open(root, false)?;
    let rows =
        c.db.prepare("SELECT id, person_id, IFNULL(assignment,'') FROM faces ORDER BY id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn rejections(root: &Path) -> anyhow::Result<Vec<(i64, i64)>> {
    let c = Collection::open(root, false)?;
    let rows =
        c.db.prepare("SELECT face_id, person_id FROM face_rejections ORDER BY face_id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Kachelsymbole: Einfachklick auf Haken oder Kreuz bewirkt nichts und schaltet keine Auswahl
/// um; Doppelklick bestätigt bzw. lehnt genau dieses Gesicht ab, die Gruppe bleibt mit den
/// übrigen Gesichtern (samt Abwahl) stehen; Strg+Z nimmt es zurück; „?“ öffnet den Dialog.
#[test]
fn tile_icons_decide_exactly_one_face_on_double_click() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let (context, mut app) = tile_icon_app(root.path())?;
    let is = "Doppelklick: das ist Anna";
    let is_not = "Doppelklick: das ist nicht Anna";
    // Ein Klick auf die Kachel selbst wählt sie weiterhin ab.
    let tile = frame_with(&context, &mut app, Vec::new())
        .into_iter()
        .find(|(text, _)| text == "Gesichtsausschnitt in 1.jpg")
        .expect("Kachel 1")
        .1;
    click_at(&context, &mut app, tile.center(), 1);
    assert_eq!(app.drafts["p1:1,2,3"].excluded, BTreeSet::from([1]));
    // Einfachklicks auf Haken und Kreuz: nichts gespeichert, Auswahl unverändert.
    for icon in [is, is_not] {
        let pos = tile_icon_pos(&context, &mut app, "2.jpg", icon);
        click_at(&context, &mut app, pos, 1);
    }
    assert_eq!(naming_tasks(&app) + app.undo.len(), 0);
    assert_eq!(app.drafts["p1:1,2,3"].excluded, BTreeSet::from([1]));
    assert_eq!(page_ids(&app), vec![vec![1, 2, 3]]);
    // Doppelklick auf den Haken von Gesicht 2: nur dieses verschwindet.
    let pos = tile_icon_pos(&context, &mut app, "2.jpg", is);
    click_at(&context, &mut app, pos, 2);
    // Der Auftrag kann schon fertig sein (dann liegt er im Rücknahmestapel).
    assert_eq!(naming_tasks(&app) + app.undo.len(), 1);
    assert_eq!(page_ids(&app), vec![vec![1, 3]], "die Gruppe bleibt stehen");
    assert_eq!(
        app.drafts["p1:1,3"].excluded,
        BTreeSet::from([1]),
        "die Abwahl der übrigen bleibt"
    );
    // Doppelklick auf das Kreuz von Gesicht 3.
    let pos = tile_icon_pos(&context, &mut app, "3.jpg", is_not);
    click_at(&context, &mut app, pos, 2);
    assert_eq!(naming_tasks(&app) + app.undo.len(), 2);
    assert_eq!(page_ids(&app), vec![vec![1]]);
    wait_for_tasks(&mut app);
    assert_eq!(
        face_rows(root.path())?,
        vec![
            (1, Some(1), "suggested".into()),
            (2, Some(1), "user".into()),
            (3, None, String::new()),
        ]
    );
    assert_eq!(rejections(root.path())?, vec![(3, 1)]);
    assert_eq!(app.undo.len(), 2, "beide Entscheidungen sind zurücknehmbar");
    // Strg+Z nimmt die Ablehnung zurück, ein zweites die Bestätigung.
    app.undo_last_face_decision();
    wait_for_tasks(&mut app);
    assert_eq!(face_rows(root.path())?[2], (3, Some(1), "suggested".into()));
    assert!(rejections(root.path())?.is_empty());
    app.undo_last_face_decision();
    wait_for_tasks(&mut app);
    assert_eq!(face_rows(root.path())?[1], (2, Some(1), "suggested".into()));
    // „?“ öffnet den Dialog „Einzeln benennen“ für genau dieses Gesicht.
    app.face_page.groups = vec![
        (1..=3)
            .map(|id| Face {
                source: format!("{id}.jpg"),
                ..review_face(id, 0.6)
            })
            .collect(),
    ];
    let pos = tile_icon_pos(&context, &mut app, "3.jpg", "Namen wählen …");
    click_at(&context, &mut app, pos, 1);
    assert_eq!(app.naming, Some(3));
    assert_eq!(naming_tasks(&app), 0);
    Ok(())
}

/// Welche Symbole erscheinen: Haken und Kreuz nur mit Zielperson, auf bestätigten Seiten nur
/// das Kreuz, ohne Zielperson nur das Fragezeichen.
#[test]
fn tile_icons_follow_the_target_person_of_the_group() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    drop(Collection::open(root.path(), true)?);
    let labels = |context: &egui::Context, app: &mut PhotoApp| {
        let widgets = frame_with(context, app, Vec::new());
        let count = |label: &str| widgets.iter().filter(|(text, _)| text == label).count();
        (
            count("Doppelklick: das ist nicht Anna"),
            count("Doppelklick: das ist Anna"),
            count("Namen wählen …"),
        )
    };
    // „Benennen“ mit sicherem Vorschlag (zwei Gesichter) und ohne (ein Gesicht).
    let (context, mut app) = sure_app(root.path());
    assert_eq!(labels(&context, &mut app), (2, 2, 3));
    app.face_page.groups.truncate(1);
    app.face_page.suggestions.clear();
    assert_eq!(
        labels(&context, &mut app),
        (0, 0, 2),
        "nur das Fragezeichen"
    );
    // Ohne Zielperson bleibt der Doppelklick wirkungslos.
    let group = app.face_page.groups[0].clone();
    assert!(!app.decide_group(&group, faces_view::Decision::OneIs(5)));
    // Bestätigte Gesichter: nur Kreuz und Fragezeichen; ein Haken wäre wirkungslos.
    let confirmed = |id| Face {
        assignment: Some("user".into()),
        ..review_face(id, 1.0)
    };
    let people = app.face_page.people.clone();
    for mode in ["confirmed", "person-confirmed"] {
        if mode == "confirmed" {
            app.face_mode = mode.into();
        } else {
            app.relation_from = Some(1);
            app.set_person_mode(PersonMode::Faces);
            app.set_person_face_mode(mode);
            assert_eq!(app.page_mode(), mode);
        }
        // Keine Leseaufträge: die Seite bleibt die von Hand gesetzte.
        app.tasks.clear();
        app.face_page.people = people.clone();
        app.face_page.groups = vec![vec![confirmed(8), confirmed(9)]];
        assert_eq!(labels(&context, &mut app), (2, 0, 2), "{mode}");
        let group = app.face_page.groups[0].clone();
        assert!(!app.decide_group(&group, faces_view::Decision::OneIs(8)));
    }
    // „Ungeeignet“ und „Ignoriert“: nur das Fragezeichen.
    app.view = View::Faces;
    for mode in ["unsuitable", "ignored"] {
        app.face_mode = mode.into();
        app.face_page.groups = vec![vec![confirmed(8)]];
        assert_eq!(labels(&context, &mut app), (0, 0, 1), "{mode}");
    }
    // Der Schalter in der Fußzeile (Standard an) blendet alle Symbole aus.
    assert!(Settings::default().tile_icons);
    let (context, mut app) = sure_app(root.path());
    app.settings.tile_icons = false;
    assert_eq!(labels(&context, &mut app), (0, 0, 0));
    let widgets = frame_with(&context, &mut app, Vec::new());
    assert!(
        widgets
            .iter()
            .any(|(text, _)| text == "Symbole auf den Gesichtern"),
        "Schalter neben der Fotovorschau"
    );
    assert_eq!(naming_tasks(&app), 0);
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
