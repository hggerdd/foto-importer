use foto_importer::core::{collection::Collection, evaluation, faces};

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn shares_match_hand_calculation() {
    // A bei 0 Grad, B bei 90 Grad, C ähnlich zu A (Kosinus 0,8).
    let references = vec![
        (1, 1, vec![1.0, 0.0]),
        (2, 1, vec![1.0, 0.0]),
        (3, 2, vec![0.0, 1.0]),
        (4, 2, vec![0.0, 1.0]),
        (5, 3, vec![0.8, 0.6]),
    ];
    let queries = vec![
        (10, 1, vec![1.0, 0.0]), // own 1.0, imp 0.8 (C), zweitbester 0.0
        (11, 2, vec![0.0, 1.0]), // own 1.0, imp 0.6 (C), zweitbester 0.0
        (12, 3, vec![0.8, 0.6]), // own 1.0, imp 0.8 (A), zweitbester 0.6 (B)
        (13, 9, vec![1.0, 0.0]), // Person ohne Referenz: übersprungen
    ];
    let report = evaluation::evaluate(&references, &queries, &[0.5, 0.7, 0.9], 0.3);
    assert_eq!(
        (
            report.queries,
            report.references,
            report.persons,
            report.skipped
        ),
        (3, 5, 3, 1)
    );
    let third = 1.0 / 3.0;
    let expected = [
        // Schwelle, own>=t, imp>=t, own mit Abstand, imp mit Abstand
        (0.5, 1.0, 1.0, third, 2.0 * third),
        (0.7, 1.0, 2.0 * third, third, third),
        (0.9, 1.0, 0.0, third, 0.0),
    ];
    for (row, (t, a, b, c, d)) in report.rows.iter().zip(expected) {
        assert!(near(row.threshold, t));
        assert!(near(row.own_reaches, a), "own_reaches bei {t}");
        assert!(near(row.other_reaches, b), "other_reaches bei {t}");
        assert!(near(row.own_accepted, c), "own_accepted bei {t}");
        assert!(near(row.other_accepted, d), "other_accepted bei {t}");
    }
}

#[test]
fn own_face_is_never_its_own_reference_and_only_top_three_count() {
    let angle = |cos: f32| vec![cos, (1.0 - cos * cos).sqrt()];
    // Gesicht 1 ist selbst Referenz von A: ohne Ausschluss wäre der Score 0,8.
    let references = vec![
        (1, 1, vec![1.0, 0.0]),
        (2, 1, vec![0.6, 0.8]),
        (3, 2, vec![0.0, 1.0]),
    ];
    let queries = vec![(1, 1, vec![1.0, 0.0])];
    let report = evaluation::evaluate(&references, &queries, &[0.5, 0.7], 0.05);
    assert_eq!(report.queries, 1);
    assert!(near(report.rows[0].own_reaches, 1.0));
    assert!(near(report.rows[1].own_reaches, 0.0), "own=0.6 < 0.7");
    // Nur ein anderer Kandidat: Abstand zum Zweiten gilt als erfüllt.
    let only_other = evaluation::evaluate(
        &[(1, 1, angle(0.9)), (2, 2, angle(0.8))],
        &[(9, 1, vec![1.0, 0.0])],
        &[0.7],
        0.5,
    );
    assert!(near(only_other.rows[0].other_accepted, 1.0));

    // Vier Referenzen mit 0,9 / 0,8 / 0,7 / 0,0: Mittel der besten drei = 0,8.
    let references = vec![
        (1, 1, angle(0.9)),
        (2, 1, angle(0.8)),
        (3, 1, angle(0.7)),
        (4, 1, vec![0.0, 1.0]),
        (5, 2, vec![0.0, 1.0]),
    ];
    let queries = vec![(99, 1, vec![1.0, 0.0])];
    let report = evaluation::evaluate(&references, &queries, &[0.79, 0.81], 0.05);
    assert!(near(report.rows[0].own_reaches, 1.0));
    assert!(near(report.rows[1].own_reaches, 0.0));
}

#[test]
fn load_selects_only_confirmed_usable_faces_with_references() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let collection = Collection::open(root.path(), true)?;
    let db = &collection.db;
    db.execute_batch(
        "INSERT INTO photos(id,status) VALUES(1,'present');
         INSERT INTO persons(id) VALUES(1),(2),(3);",
    )?;
    db.execute(
        "INSERT INTO face_settings VALUES('rust.embedder_key',?)",
        [serde_json::to_string("m")?],
    )?;
    // (id, person, assignment, quality, ignored, retained, Embedder)
    let faces_table = [
        (1, 1, "user", 0.9, 0, 0, "m"),      // Referenz
        (2, 1, "user", 0.9, 0, 0, "m"),      // Referenz
        (3, 2, "user", 0.9, 0, 0, "m"),      // Referenz
        (4, 1, "user", 0.9, 0, 0, "m"),      // bestätigt ohne Referenzstatus
        (5, 2, "user", 0.9, 1, 0, "m"),      // ignoriert
        (6, 1, "user", 0.3, 0, 0, "m"),      // Qualität zu niedrig
        (7, 1, "user", 0.9, 0, 1, "m"),      // historisch
        (8, 3, "user", 0.9, 0, 0, "m"),      // Person ohne Referenz
        (9, 1, "suggested", 0.9, 0, 0, "m"), // nur Vorschlag
        (10, 1, "user", 0.9, 0, 0, "x"),     // anderer Embedder
    ];
    for (id, person, assignment, quality, ignored, retained, key) in faces_table {
        db.execute(
            "INSERT INTO faces(id,photo_id,source_relpath,detector_id,person_id,assignment,quality,ignored,retained)
             VALUES(?,1,'a.jpg','det',?,?,?,?,?)",
            rusqlite::params![id, person, assignment, quality, ignored, retained],
        )?;
        db.execute(
            "INSERT INTO face_embeddings VALUES(?,?,2,?)",
            rusqlite::params![id, key, faces::encode(&[1.0, 0.0])],
        )?;
    }
    for (person, face, split, key) in [
        (1, 1, "ref", "m"),
        (1, 2, "ref", "m"),
        (2, 3, "ref", "m"),
        (1, 4, "val", "m"),
        (1, 10, "ref", "x"),
    ] {
        db.execute(
            "INSERT INTO training_set VALUES(?,?,?,?,'now','test',0)",
            rusqlite::params![person, face, key, split],
        )?;
    }
    let (references, queries) = evaluation::load(&collection)?;
    let ids = |list: &[(i64, i64, Vec<f32>)]| list.iter().map(|e| e.0).collect::<Vec<_>>();
    assert_eq!(ids(&references), vec![1, 2, 3]);
    assert_eq!(ids(&queries), vec![1, 2, 3, 4]);
    Ok(())
}
