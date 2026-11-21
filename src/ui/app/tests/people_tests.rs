//! Personen: Tabelle, Details, Beziehungsgraph, Zusatzangaben, Duplikate.
use super::*;

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
