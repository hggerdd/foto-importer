//! Erzeugt eine isolierte Demo-Sammlung mit gezeichneten Gesichtern, Personen,
//! Referenzen und trainiertem Modell für Sichtprüfungen des Personenbereichs.
//! Es werden nur Kunstbilder aus `tmp/demo` kopiert, nie private Sammlungen.
//!
//! `cargo run --example demo -q` (einmal), dann `cargo run --example demo_faces`.
//! Start: `FOTO_IMPORTER_SETTINGS=tmp/demo-faces/settings.json FOTO_IMPORTER_VIEW=people
//! FOTO_IMPORTER_FACE_MODE=unknown cargo run --release`.
use foto_importer::{
    config::Settings,
    core::{collection::Collection, faces, inference::normalize, jobs::Control, recognition},
};
use image::{Rgb, RgbImage};
use rusqlite::params;
use std::path::Path;

/// (Name, Hautfarbe, Haarfarbe, Mittelpunkt der Merkmalsachsen)
type Look = (&'static str, [u8; 3], [u8; 3], [usize; 2]);
const PEOPLE: [Look; 4] = [
    ("Anna", [244, 208, 180], [96, 62, 38], [0, 1]),
    ("Ben", [222, 176, 140], [30, 28, 30], [4, 5]),
    ("Clara", [250, 222, 200], [226, 188, 96], [8, 9]),
    ("Dora", [196, 148, 118], [168, 70, 40], [12, 13]),
];

fn noise(state: &mut u64) -> f32 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 33) as f32 / (1u64 << 31) as f32) - 0.5
}

fn vector(person: usize, spread: f32, state: &mut u64) -> Vec<f32> {
    let mut v = vec![0.0f32; 16];
    let axes = if person < PEOPLE.len() {
        PEOPLE[person].3
    } else {
        [person * 2 % 16, (person * 2 + 1) % 16]
    };
    v[axes[0]] = 1.0;
    v[axes[1]] = 0.55;
    for x in v.iter_mut() {
        *x += noise(state) * spread;
    }
    normalize(&v).unwrap_or(v)
}

fn draw_face(image: &mut RgbImage, center: (f32, f32), radius: f32, person: usize) {
    let (_, skin, hair, _) = PEOPLE[person % PEOPLE.len()];
    for y in 0..image.height() {
        for x in 0..image.width() {
            let (dx, dy) = (x as f32 - center.0, y as f32 - center.1);
            let inside = (dx / (radius * 0.82)).powi(2) + (dy / radius).powi(2) <= 1.0;
            if !inside {
                continue;
            }
            let colour = if dy < -radius * 0.45 { hair } else { skin };
            image.put_pixel(x, y, Rgb(colour));
        }
    }
    for side in [-1.0f32, 1.0] {
        let (ex, ey) = (center.0 + side * radius * 0.34, center.1 - radius * 0.1);
        for y in (ey - 4.0) as u32..=(ey + 4.0) as u32 {
            for x in (ex - 6.0) as u32..=(ex + 6.0) as u32 {
                image.put_pixel(x, y, Rgb([40, 40, 48]));
            }
        }
    }
    for x in (center.0 - radius * 0.25) as u32..=(center.0 + radius * 0.25) as u32 {
        let y = (center.1 + radius * 0.45) as u32;
        image.put_pixel(x, y, Rgb([150, 70, 70]));
        image.put_pixel(x, y + 1, Rgb([150, 70, 70]));
    }
}

fn main() -> anyhow::Result<()> {
    let root = Path::new("tmp/demo-faces");
    let source = root.join("Quelle");
    std::fs::create_dir_all(&source)?;
    // Plan: (Person, Anzahl, bestätigt?, Streuung)
    let mut plan: Vec<(usize, usize, bool, f32)> = vec![
        (0, 16, true, 0.25),
        (1, 12, true, 0.25),
        (2, 10, true, 0.25),
        (0, 6, false, 0.35),
        (1, 4, false, 0.35),
        (3, 4, false, 0.25),
    ];
    // Weitere Personen mit je drei bestätigten Gesichtern: mehrere Seiten im Reiter „Bestätigt“.
    for guest in 4..14 {
        plan.push((guest, 3, true, 0.25));
    }
    let mut sources = Vec::new();
    for folder in std::fs::read_dir("tmp/demo/Quelle")? {
        let folder = folder?.path();
        if folder.is_dir() {
            for file in std::fs::read_dir(&folder)? {
                let file = file?.path();
                if file.extension().is_some_and(|e| e == "jpg") {
                    sources.push(file);
                }
            }
        }
    }
    sources.sort();
    // Zusätzliche Fotos ohne Gesichter, damit die Sammlung mehrere Seiten hat.
    for k in 0..60 {
        let from = &sources[k % sources.len()];
        let to = source.join(format!("KOPIE_{k:02}.jpg"));
        if !to.exists() {
            std::fs::copy(from, to)?;
        }
    }
    let mut state = 11u64;
    let mut slots = Vec::new();
    for (person, count, confirmed, spread) in plan {
        for _ in 0..count {
            slots.push((person, confirmed, spread));
        }
    }
    let mut placed = Vec::new();
    for (index, (person, confirmed, spread)) in slots.iter().enumerate() {
        let from = &sources[index % sources.len()];
        let to = source.join(from.file_name().unwrap_or_default());
        let mut image = image::open(if to.exists() { &to } else { from })?.to_rgb8();
        // Bis zu drei Gesichter je Foto nebeneinander.
        let center = (
            140.0 + (index / sources.len()) as f32 * 260.0 + noise(&mut state) * 30.0,
            200.0 + noise(&mut state) * 60.0,
        );
        let radius = 44.0 + (index % 4) as f32 * 7.0;
        draw_face(&mut image, center, radius, *person);
        image.save(&to)?;
        placed.push((to, center, radius, *person, *confirmed, *spread));
    }
    let source = source.canonicalize()?;
    let mut collection = Collection::open(&source, true)?;
    collection.quick_scan(false, &Control::default())?;
    let names: Vec<String> = PEOPLE
        .iter()
        .map(|look| look.0.to_string())
        .chain((1..=10).map(|n| format!("Gast {n}")))
        .collect();
    for (person, name) in names.iter().enumerate() {
        let id = person as i64 + 1;
        collection
            .db
            .execute("INSERT OR IGNORE INTO persons(id) VALUES(?)", [id])?;
        collection.db.execute(
            "INSERT OR REPLACE INTO person_attrs VALUES(?,'name',?)",
            params![id, name],
        )?;
    }
    // Beispiel-Familie für die Beziehungsübersicht (Personen-IDs: Anna 1, Ben 2,
    // Clara 3, Dora 4, Gast n = 4 + n).
    for (id, gender) in [
        (1, "weiblich"),
        (2, "männlich"),
        (3, "weiblich"),
        (4, "weiblich"),
        (5, "männlich"),
        (6, "weiblich"),
        (7, "männlich"),
        (9, "männlich"),
        (10, "weiblich"),
        (11, "männlich"),
    ] {
        collection.db.execute(
            "INSERT OR REPLACE INTO person_attrs VALUES(?,'gender',?)",
            params![id, gender],
        )?;
    }
    for (subject, kind, object) in [
        (1, "partner", 2),
        (1, "parent", 3),
        (2, "parent", 3),
        (5, "child", 1),
        (5, "child", 2),
        (9, "parent", 1),
        (10, "parent", 1),
        (9, "parent", 6),
        (10, "parent", 6),
        (7, "child", 6),
        (4, "friend", 3),
        (8, "acquaintance", 1),
        (11, "cousin", 2),
    ] {
        faces::add_relation(&mut collection, subject, kind, object)?;
    }
    let embedder = "demo-embedder";
    collection.db.execute_batch(&format!(
        "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key','\"{embedder}\"'),
         ('rust.detector_key','\"demo-detector\"'),('active_embedder','\"sface\"'),('active_detector','\"yunet\"');"
    ))?;
    for (to, center, radius, person, confirmed, spread) in &placed {
        let relpath = to
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let found: Option<(i64, i64)> = collection
            .db
            .query_row(
                "SELECT id,photo_id FROM files WHERE relpath=?",
                [&relpath],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        let Some((file, photo)) = found else {
            continue;
        };
        let (w, h) = (800.0f32, 520.0f32);
        collection.db.execute(
            "INSERT INTO faces(photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,det_score,quality,person_id,assignment,captured_at) VALUES(?,?,?,'demo-detector',?,?,?,?,0.95,1.0,?,?,'2026:10:20 12:00:00')",
            params![
                photo,
                file,
                relpath,
                (center.0 - radius) / w,
                (center.1 - radius) / h,
                (center.0 + radius) / w,
                (center.1 + radius) / h,
                confirmed.then_some(*person as i64 + 1),
                confirmed.then_some("user"),
            ],
        )?;
        let id = collection.db.last_insert_rowid();
        collection.db.execute(
            "INSERT INTO face_embeddings VALUES(?,?,16,?)",
            params![
                id,
                embedder,
                faces::encode(&vector(*person, *spread, &mut state))
            ],
        )?;
    }
    recognition::apply_knowledge(&mut collection, &Control::default())?;
    let settings = Settings {
        source: source.to_string_lossy().into_owned(),
        collection: source.to_string_lossy().into_owned(),
        target: source.to_string_lossy().into_owned(),
        window_size: [1600.0, 960.0],
        ..Default::default()
    };
    let path = root.join("settings.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&settings)?)?;
    let status = faces::status(&collection)?;
    println!(
        "{} · bestätigt {} · unbekannt {} · Vorschläge {} · Modell: {}",
        path.canonicalize()?.display(),
        status.confirmed,
        status.unknown,
        status.review,
        status.classifier.is_some()
    );
    Ok(())
}
