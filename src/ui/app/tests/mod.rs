//! UI-Tests von `PhotoApp`, nach Themen aufgeteilt. Hier stehen die gemeinsamen Helfer
//! (`settle_until` und Verwandte, Frame- und Klickhelfer).
use super::*;
use eframe::App;

mod background_tests;
mod collection_tests;
mod faces_tests;
mod import_tests;
mod notices_tests;
mod people_tests;
mod settings_tests;

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
fn notice_app() -> PhotoApp {
    let context = egui::Context::default();
    let creation = eframe::CreationContext::_new_kittest(context);
    PhotoApp::with_settings(&creation, Settings::default())
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
