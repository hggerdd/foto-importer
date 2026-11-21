//! Referenzgrenze je Person: sättigende Funktion, Wirkung in der Auswahl und Neulauf bei
//! geänderter Regel. Synthetische Vektoren, keine echte Sammlung.
mod common;
use foto_importer::{
    config,
    core::{collection::Collection, faces, jobs::Control, recognition},
};
use rusqlite::params;

#[test]
fn reference_limit_grows_monotonically_and_saturates() {
    let mut last = 0;
    for n in 0..20_000 {
        let limit = config::reference_limit(n);
        assert!(limit >= last, "monoton bei {n}");
        assert!(limit <= n, "nie mehr als bestätigt bei {n}");
        assert!(limit as f32 <= config::REFERENCE_SATURATION);
        last = limit;
    }
    // Keine feste Untergrenze, aber seltene Personen behalten (fast) alle Gesichter.
    for n in 0..=30 {
        assert_eq!(config::reference_limit(n), n);
    }
    assert!(config::reference_limit(100) >= 90);
    // Häufige Personen erhalten deutlich mehr als die frühere feste Grenze 100.
    assert!(config::reference_limit(600) >= 400);
    assert!(config::reference_limit(5_000) as f32 >= config::REFERENCE_SATURATION - 2.0);
    assert_eq!(
        config::reference_limit(20_000),
        config::reference_limit(40_000)
    );
}

#[test]
fn validation_spares_rare_people_and_is_capped() {
    for chosen in 0..config::REFERENCE_VALIDATION_FROM {
        assert_eq!(config::reference_validation(chosen), 0);
    }
    for chosen in config::REFERENCE_VALIDATION_FROM..2_000 {
        let validation = config::reference_validation(chosen);
        assert!((1..=config::REFERENCE_VALIDATION_MAX).contains(&validation));
        assert!(validation < chosen);
    }
}

/// Unterscheidbare, aber ähnliche Einheitsvektoren (Kosinus untereinander um 0,5): weder
/// Ausreißer noch Dubletten.
fn vector(seed: u64) -> Vec<f32> {
    let bytes = blake3::hash(&seed.to_le_bytes());
    let mut state = u64::from_le_bytes(bytes.as_bytes()[..8].try_into().expect("acht Bytes"));
    let mut v: Vec<f32> = (0..64)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let noise = (state % 2001) as f32 / 1000.0 - 1.0;
            if i == 0 { 4.0 + noise } else { noise }
        })
        .collect();
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter_mut().for_each(|x| *x /= norm);
    v
}

/// Person 1 mit `count` bestätigten Gesichtern (je ein Foto, Aufnahmen eine Stunde auseinander).
fn confirmed_person(c: &Collection, count: i64) -> anyhow::Result<()> {
    c.db.execute("INSERT INTO persons(id) VALUES(1)", [])?;
    c.db.execute(
        "INSERT INTO face_settings VALUES('rust.embedder_key','\"test\"')",
        [],
    )?;
    for id in 1..=count {
        c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
        common::face(c, id, id)?;
        let captured = (chrono::DateTime::UNIX_EPOCH + chrono::Duration::hours(id)).naive_utc();
        c.db.execute(
            "UPDATE faces SET person_id=1,assignment='user',captured_at=? WHERE id=?",
            params![captured.format("%Y:%m:%d %H:%M:%S").to_string(), id],
        )?;
        c.db.execute(
            "INSERT INTO face_embeddings VALUES(?,'test',64,?)",
            params![id, faces::encode(&vector(id as u64))],
        )?;
    }
    Ok(())
}

fn count(c: &Collection, sql: &str) -> anyhow::Result<usize> {
    Ok(c.db.query_row(sql, [], |r| r.get::<_, i64>(0))? as usize)
}

#[test]
fn frequent_person_gets_more_than_the_former_hundred_references() -> anyhow::Result<()> {
    let (_temp, mut c) = common::collection()?;
    confirmed_person(&c, 700)?;
    c.db.execute(
        "INSERT OR IGNORE INTO face_reference_dirty VALUES('test',1)",
        [],
    )?;
    recognition::update_references(&mut c, "test", &Control::default())?;
    let chosen = config::reference_limit(700);
    let validation = config::reference_validation(chosen);
    assert_eq!(
        count(&c, "SELECT count(*) FROM training_set WHERE split='val'")?,
        validation
    );
    let references = count(&c, "SELECT count(*) FROM training_set WHERE split='ref'")?;
    assert_eq!(references, chosen - validation);
    assert!(references > 100, "{references} Referenzen");
    // Die Auswahl merkt sich die Regel, nach der sie entstand.
    assert_eq!(
        faces::setting(&c, "rust.reference_policy:test")?.as_deref(),
        Some(config::REFERENCE_POLICY)
    );
    Ok(())
}

#[test]
fn rare_person_keeps_every_face_as_reference() -> anyhow::Result<()> {
    let (_temp, mut c) = common::collection()?;
    confirmed_person(&c, 12)?;
    c.db.execute(
        "INSERT OR IGNORE INTO face_reference_dirty VALUES('test',1)",
        [],
    )?;
    recognition::update_references(&mut c, "test", &Control::default())?;
    assert_eq!(
        count(&c, "SELECT count(*) FROM training_set WHERE split='ref'")?,
        12
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM training_set WHERE split='val'")?,
        0
    );
    Ok(())
}

#[test]
fn outdated_reference_policy_marks_all_people_for_a_new_selection() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    {
        let c = Collection::open(temp.path(), true)?;
        confirmed_person(&c, 5)?;
        c.db.execute("INSERT INTO persons(id) VALUES(2)", [])?;
        // Auswahl nach der alten Regel (ohne gespeicherte Kennung), mit Pin und Ausschluss.
        for (person, face, pinned) in [(1, 1, 1), (1, 2, 0), (2, 3, 0)] {
            c.db.execute(
                "INSERT INTO training_set VALUES(?,?,'test','ref',datetime('now'),'x',?)",
                params![person, face, pinned],
            )?;
        }
        c.db.execute("INSERT INTO training_exclusions VALUES(4,'test')", [])?;
        c.db.execute("DELETE FROM face_reference_dirty", [])?;
        c.db.execute("DELETE FROM face_maintenance", [])?;
    }
    let mut c = Collection::open(temp.path(), true)?;
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM face_reference_dirty WHERE embedder_id='test'"
        )?,
        2
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM face_maintenance WHERE embedder_id='test'"
        )?,
        1
    );
    // Nutzerentscheidungen bleiben unberührt.
    assert_eq!(
        count(&c, "SELECT count(*) FROM training_set WHERE pinned=1")?,
        1
    );
    assert_eq!(count(&c, "SELECT count(*) FROM training_exclusions")?, 1);
    // Ein zweites Öffnen markiert nichts erneut.
    c.db.execute("DELETE FROM face_reference_dirty", [])?;
    drop(c);
    c = Collection::open(temp.path(), true)?;
    assert_eq!(count(&c, "SELECT count(*) FROM face_reference_dirty")?, 0);
    // Der Neulauf behält den Pin und lässt das ausgeschlossene Gesicht weg.
    c.db.execute("INSERT INTO face_reference_dirty VALUES('test',1)", [])?;
    recognition::update_references(&mut c, "test", &Control::default())?;
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM training_set WHERE face_id=1 AND pinned=1"
        )?,
        1
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM training_set WHERE face_id=4")?,
        0
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM training_set WHERE person_id=1")?,
        4
    );
    Ok(())
}
