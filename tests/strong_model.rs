//! Genaueres Vergleichsmodell ohne Modelldatei: Lauf mit Ersatz-Embedder (abbrechen,
//! fortsetzen), Umschalten erhält Nutzerentscheidungen, Schwellen je Modell.
mod common;

use foto_importer::config;
use foto_importer::core::{
    collection::Collection,
    face_models,
    faces::{self, encode},
    jobs::Control,
    recognition, strong,
};
use rusqlite::params;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Gesicht, Person, Zuordnung und Ignoriert-Wert: die Nutzerentscheidungen eines Gesichts.
type Decision = (i64, Option<i64>, Option<String>, i64);

const BASE: &str = "sface@test:base:rust-ort-v1";

/// Einstellung der Sammlung wie `faces::save_setting` (JSON-Text).
fn save(c: &Collection, key: &str, value: &str) -> anyhow::Result<()> {
    c.db.execute(
        "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}

/// Fotos mit je zwei Gesichtern (echte JPEG-Dateien, Landmarks im Analysebild), dazu
/// SFace-Vektoren unter `BASE` als aktives Modell.
fn collection_with_photos(photos: i64) -> anyhow::Result<(tempfile::TempDir, Collection)> {
    let (temp, c) = common::collection_with_people(2)?;
    for photo in 1..=photos {
        let image = image::RgbImage::from_fn(400, 300, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, (photo * 40 % 256) as u8])
        });
        image.save(temp.path().join(format!("{photo}.jpg")))?;
        c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [photo])?;
        for (slot, (x, person)) in [(40.0f32, 1i64), (220.0, 2)].into_iter().enumerate() {
            let landmarks = [
                [x + 38.0, 101.0],
                [x + 73.0, 101.0],
                [x + 56.0, 121.0],
                [x + 41.0, 142.0],
                [x + 70.0, 142.0],
            ];
            let id = photo * 10 + slot as i64;
            c.db.execute(
                "INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,person_id,assignment,captured_at)
                 VALUES(?,?,?,'det',?,0.2,?,0.7,?,0.95,0.9,?,'user','2020-01-01')",
                params![
                    id,
                    photo,
                    format!("{photo}.jpg"),
                    x / 400.0,
                    (x + 110.0) / 400.0,
                    serde_json::to_string(&landmarks)?,
                    person
                ],
            )?;
            let vector: Vec<f32> = (0..8)
                .map(|i| {
                    if i == person as usize {
                        1.0
                    } else {
                        0.01 * i as f32
                    }
                })
                .collect();
            c.db.execute(
                "INSERT INTO face_embeddings VALUES(?,?,?,?)",
                params![
                    id,
                    BASE,
                    8,
                    encode(&foto_importer::core::inference::normalize(&vector)?)
                ],
            )?;
        }
    }
    save(&c, faces::EMBEDDER_KEY, BASE)?;
    save(&c, strong::BASE_KEY, BASE)?;
    Ok((temp, c))
}

/// Ersatz für das Modell: ein Vektor aus den mittleren Farben des ausgerichteten Gesichts.
fn fake(crop: &image::RgbImage) -> anyhow::Result<Vec<f32>> {
    assert_eq!(crop.dimensions(), (112, 112));
    let mut sums = vec![1.0f32; 4];
    for p in crop.pixels() {
        for c in 0..3 {
            sums[c] += p[c] as f32;
        }
    }
    foto_importer::core::inference::normalize(&sums)
}

fn strong_count(c: &Collection, key: &str) -> anyhow::Result<usize> {
    Ok(c.db.query_row(
        "SELECT count(*) FROM face_embeddings WHERE embedder_id=?",
        [key],
        |r| r.get(0),
    )?)
}

#[test]
fn run_can_be_cancelled_and_resumes_with_the_remaining_faces() -> anyhow::Result<()> {
    let (_temp, mut c) = collection_with_photos(6)?;
    let key = face_models::strong_key()?;
    let before = strong::coverage(&c, &key)?;
    assert_eq!(
        (before.eligible, before.embedded, before.open_photos),
        (12, 0, 6)
    );
    assert!(!before.ready());
    // Abbruch nach dem fünften Gesicht: Berechnetes bleibt erhalten.
    let control = Control::default();
    let calls = AtomicUsize::new(0);
    let result = strong::compute_with(
        &mut c,
        "exiftool",
        &control,
        |crop| {
            if calls.fetch_add(1, Ordering::Relaxed) == 4 {
                control.stop();
            }
            fake(crop)
        },
        &key,
    );
    assert!(result.is_err(), "abgebrochen");
    let partial = strong_count(&c, &key)?;
    assert!((1..12).contains(&partial), "{partial} von 12");
    // Fortsetzen rechnet nur die übrigen.
    let resumed = AtomicUsize::new(0);
    strong::compute_with(
        &mut c,
        "exiftool",
        &Control::default(),
        |crop| {
            resumed.fetch_add(1, Ordering::Relaxed);
            fake(crop)
        },
        &key,
    )?;
    assert_eq!(resumed.load(Ordering::Relaxed), 12 - partial);
    let after = strong::coverage(&c, &key)?;
    assert_eq!(
        (after.embedded, after.open_faces, after.open_photos),
        (12, 0, 0)
    );
    assert!(after.ready());
    // Ein weiterer Lauf hat nichts zu tun.
    let message = strong::compute_with(
        &mut c,
        "exiftool",
        &Control::default(),
        |_| unreachable!(),
        &key,
    )?;
    assert!(message.starts_with("0 Fotos"), "{message}");
    // Ein Foto, das sich nicht laden lässt, bricht den Lauf nicht ab.
    std::fs::remove_file(c.root.join("6.jpg"))?;
    c.db.execute(
        "DELETE FROM face_embeddings WHERE embedder_id=? AND face_id IN (60,61)",
        [&key],
    )?;
    let message = strong::compute_with(&mut c, "exiftool", &Control::default(), fake, &key)?;
    assert!(
        message.contains("2 Gesichter nicht berechenbar"),
        "{message}"
    );
    let error: String = c.db.query_row(
        "SELECT status FROM face_jobs WHERE photo_id=6 AND stage='embed' AND model_id=?",
        [&key],
        |r| r.get(0),
    )?;
    assert_eq!(error, "error");
    Ok(())
}

#[test]
fn changed_landmarks_are_not_overwritten_by_a_stale_vector() -> anyhow::Result<()> {
    let (_temp, mut c) = collection_with_photos(1)?;
    let key = face_models::strong_key()?;
    let control = Control::default();
    // Während des Laufs analysiert jemand das Foto neu (andere Landmarks für Gesicht 10).
    let db_path = c.root.join(".myfotomanager");
    let changed = std::cell::Cell::new(false);
    strong::compute_with(
        &mut c,
        "exiftool",
        &control,
        |crop| {
            if !changed.replace(true) {
                let other = rusqlite::Connection::open(&db_path).expect("zweite Verbindung");
                other
                .execute("UPDATE faces SET landmarks='[[1,1],[2,1],[1.5,2],[1,3],[2,3]]' WHERE id=10", [])
                .expect("Landmarks ändern");
            }
            fake(crop)
        },
        &key,
    )?;
    let ids: Vec<i64> =
        c.db.prepare("SELECT face_id FROM face_embeddings WHERE embedder_id=? ORDER BY face_id")?
            .query_map([&key], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
    assert_eq!(ids, vec![11]);
    Ok(())
}

#[test]
fn switching_the_active_model_keeps_every_user_decision() -> anyhow::Result<()> {
    let (temp, mut c) = collection_with_photos(4)?;
    let key = face_models::strong_key()?;
    // Nutzerentscheidungen: Ablehnung, Ignorieren, Kein Gesicht, Pin, Ausschluss.
    c.db.execute("INSERT INTO face_rejections VALUES(20,2)", [])?;
    c.db.execute(
        "UPDATE faces SET person_id=NULL,assignment=NULL,ignored=1 WHERE id=31",
        [],
    )?;
    c.db.execute(
        "UPDATE faces SET person_id=NULL,assignment=NULL,ignored=2 WHERE id=41",
        [],
    )?;
    c.db.execute(
        "INSERT INTO training_set VALUES(1,10,?,'ref',datetime('now'),'manual_pin',1)",
        [BASE],
    )?;
    c.db.execute("INSERT INTO training_exclusions VALUES(11,?)", [BASE])?;
    let snapshot = |c: &Collection| -> anyhow::Result<Vec<Decision>> {
        Ok(c.db
            .prepare("SELECT id,person_id,assignment,ignored FROM faces WHERE assignment='user' OR ignored>0 ORDER BY id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?)
    };
    let decisions = snapshot(&c)?;
    let models = temp.path().join("models");
    std::fs::create_dir(&models)?;
    std::fs::write(
        models.join("arcface_buffalo_l.json"),
        include_str!("../models/arcface_buffalo_l.json"),
    )?;
    // Ohne berechnete Vektoren ist kein Umschalten möglich.
    assert!(strong::activate(&mut c, &models, &Control::default()).is_err());
    assert_eq!(
        faces::setting(&c, faces::EMBEDDER_KEY)?.as_deref(),
        Some(BASE)
    );
    strong::compute_with(&mut c, "exiftool", &Control::default(), fake, &key)?;
    strong::activate(&mut c, &models, &Control::default())?;
    assert_eq!(
        faces::setting(&c, faces::EMBEDDER_KEY)?.as_deref(),
        Some(key.as_str())
    );
    assert_eq!(snapshot(&c)?, decisions);
    // Referenzen des neuen Modells entstehen nur aus bestätigten Gesichtern.
    let strong_refs: usize = c.db.query_row(
        "SELECT count(*) FROM training_set t JOIN faces f ON f.id=t.face_id WHERE t.embedder_id=? AND f.assignment='user'",
        [&key],
        |r| r.get(0),
    )?;
    let all_refs: usize = c.db.query_row(
        "SELECT count(*) FROM training_set WHERE embedder_id=?",
        [&key],
        |r| r.get(0),
    )?;
    assert!(strong_refs > 0 && strong_refs == all_refs);
    // Pin und Ausschluss gelten im neuen Modell weiter.
    let pinned: usize = c.db.query_row(
        "SELECT count(*) FROM training_set WHERE face_id=10 AND embedder_id=? AND pinned=1 AND split='ref'",
        [&key],
        |r| r.get(0),
    )?;
    assert_eq!(pinned, 1);
    let excluded_strong: usize = c.db.query_row(
        "SELECT count(*) FROM training_set WHERE face_id=11 AND embedder_id=?",
        [&key],
        |r| r.get(0),
    )?;
    assert_eq!(
        excluded_strong, 0,
        "ausgeschlossenes Gesicht ist keine Referenz"
    );
    let exclusion: usize = c.db.query_row(
        "SELECT count(*) FROM training_exclusions WHERE face_id=11 AND embedder_id=?",
        [&key],
        |r| r.get(0),
    )?;
    assert_eq!(exclusion, 1);
    // Vor dem Umschalten entstand eine Sicherung.
    assert!(
        std::fs::read_dir(temp.path())?.any(|e| e.ok().is_some_and(|e| e
            .file_name()
            .to_string_lossy()
            .starts_with(config::BACKUP_PREFIX)))
    );
    // Zurück zu SFace: dieselben Entscheidungen, Pin und Ausschluss des Analysemodells bleiben.
    strong::deactivate(&mut c, &Control::default())?;
    assert_eq!(
        faces::setting(&c, faces::EMBEDDER_KEY)?.as_deref(),
        Some(BASE)
    );
    assert_eq!(snapshot(&c)?, decisions);
    let rejection: usize = c.db.query_row(
        "SELECT count(*) FROM face_rejections WHERE face_id=20 AND person_id=2",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(rejection, 1);
    let pin: usize = c.db.query_row(
        "SELECT count(*) FROM training_set WHERE face_id=10 AND embedder_id=? AND pinned=1",
        [BASE],
        |r| r.get(0),
    )?;
    assert_eq!(pin, 1);
    let excluded: usize = c.db.query_row(
        "SELECT count(*) FROM training_exclusions WHERE face_id=11 AND embedder_id=?",
        [BASE],
        |r| r.get(0),
    )?;
    assert_eq!(excluded, 1);
    Ok(())
}

#[test]
fn thresholds_follow_the_active_model() -> anyhow::Result<()> {
    let (_temp, mut c) = collection_with_photos(1)?;
    let key = face_models::strong_key()?;
    let sface = recognition::Limits::load(&c)?;
    assert_eq!(
        (sface.review, sface.margin, sface.sure),
        (
            config::FACE_REVIEW_MIN,
            config::FACE_REVIEW_MARGIN,
            config::FACE_SURE_MIN
        )
    );
    save(&c, faces::EMBEDDER_KEY, &key)?;
    let arc = recognition::Limits::load(&c)?;
    let scale = config::model_scale(&key);
    assert_eq!(
        (arc.review, arc.margin, arc.sure),
        (scale.review, scale.margin, scale.sure)
    );
    assert!(arc.review < sface.review, "ArcFace hat eine andere Skala");
    // Je Modell überschreibbar, im zulässigen Bereich des Modells.
    let (low, high) = scale.review_range;
    assert!(recognition::set_review_threshold(&mut c, high + 0.05).is_err());
    recognition::set_review_threshold(&mut c, (low + high) / 2.0)?;
    assert_eq!(recognition::Limits::load(&c)?.review, (low + high) / 2.0);
    save(&c, faces::EMBEDDER_KEY, BASE)?;
    assert_eq!(
        recognition::Limits::load(&c)?.review,
        config::FACE_REVIEW_MIN
    );
    Ok(())
}
