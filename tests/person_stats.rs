//! Personenstatistik (`recognition::person_scores`, `person_stats`): Same-Day-Ausschluss,
//! Ablehnungen als Gegenbeispiele, beste zulässige Person, Fingerabdruck. Nur temporäre
//! Sammlungen mit synthetischen 2D-Vektoren (Kosinus = Winkelabstand).
use foto_importer::core::{
    collection::Collection,
    faces,
    jobs::Control,
    person_stats::{self, Mode},
    recognition::{self, PersonScores},
};
use rusqlite::params;

const KEY: &str = "test";

fn at(angle: f32) -> [f32; 2] {
    [angle.cos(), angle.sin()]
}

/// Ein Gesicht im Foto `photo`, aufgenommen am Tag `day`.
fn face(c: &Collection, id: i64, photo: i64, day: &str, vector: [f32; 2]) {
    c.db.execute(
        "INSERT OR IGNORE INTO photos(id,status) VALUES(?,'present')",
        [photo],
    )
    .expect("Foto");
    c.db.execute(
        "INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality,captured_at) VALUES(?,?,?,'det',0,0,1,1,1.0,?)",
        params![id, photo, format!("{photo}.jpg"), day],
    )
    .expect("Gesicht");
    c.db.execute(
        "INSERT INTO face_embeddings VALUES(?,?,2,?)",
        params![id, KEY, faces::encode(&vector)],
    )
    .expect("Vektor");
}

/// Anna (1): Gesichter 1 und 2 am selben Tag in Richtung 0, Gesicht 3 an einem anderen Tag
/// bei 0,3. Ben (2): zwei Gesichter bei 2,0. Dazu Ablehnungen und offene Gesichter.
fn collection(root: &std::path::Path) -> anyhow::Result<Collection> {
    let c = Collection::open(root, true)?;
    c.db.execute(
        "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key',?)",
        [serde_json::to_string(KEY)?],
    )?;
    face(&c, 1, 1, "2020-01-01 10:00:00", at(0.0));
    face(&c, 2, 2, "2020-01-01 11:00:00", at(0.0));
    face(&c, 3, 3, "2021-05-05 10:00:00", at(0.3));
    face(&c, 20, 20, "2019-01-01", at(2.0));
    face(&c, 21, 21, "2019-02-01", at(2.0));
    // 10: „nicht Anna“; 11: ebenso, aber ignoriert (kein Gegenbeispiel).
    face(&c, 10, 10, "2022-01-01", at(0.5));
    face(&c, 11, 11, "2022-01-01", at(0.5));
    // Offen: 30 eher Anna, 31 eher Ben, 32 für Anna abgelehnt, 33 im Foto von Anna,
    // 34 als Anna vorgeschlagen.
    face(&c, 30, 30, "2023-01-01", at(0.1));
    face(&c, 31, 31, "2023-01-01", at(1.95));
    face(&c, 32, 32, "2023-01-01", at(0.05));
    face(&c, 33, 1, "2020-01-01", at(0.1));
    face(&c, 34, 34, "2023-01-01", at(0.0));
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2);
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben');
         UPDATE faces SET person_id=1,assignment='user',confidence=1 WHERE id IN (1,2,3);
         UPDATE faces SET person_id=2,assignment='user',confidence=1 WHERE id IN (20,21);
         INSERT INTO face_rejections VALUES(10,1),(11,1),(32,1);
         UPDATE faces SET ignored=1 WHERE id=11;
         UPDATE faces SET person_id=1,assignment='suggested',confidence=0.9 WHERE id=34;",
    )?;
    Ok(c)
}

fn top3(mut values: Vec<f32>) -> f32 {
    values.sort_by(|a, b| b.total_cmp(a));
    values.truncate(3);
    values.iter().sum::<f32>() / values.len() as f32
}

fn sorted(mut scores: Vec<PersonScores>) -> Vec<PersonScores> {
    for s in &mut scores {
        s.rated.sort_by(f32::total_cmp);
        s.confirmed.sort_by(f32::total_cmp);
        s.rejected.sort_by(f32::total_cmp);
    }
    scores
}

#[test]
fn confirmed_faces_ignore_references_of_the_same_day() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = collection(temp.path())?;
    let scores = recognition::person_scores(&c, KEY, 0.40, &Control::default())?;
    let anna = scores.iter().find(|s| s.person == 1).expect("Anna");
    // Gesicht 1: ohne sich selbst und ohne Gesicht 2 (selber Tag) bleibt nur Gesicht 3.
    // Gesicht 3: beide anderen liegen an einem anderen Tag.
    let expected = (0.3f32).cos();
    assert_eq!(anna.confirmed.len(), 3);
    for value in &anna.confirmed {
        assert!((value - expected).abs() < 1e-5, "{value} statt {expected}");
    }
    // Ohne den Ausschluss läge Gesicht 1 bei (1 + cos 0,3) / 2.
    assert!(anna.confirmed.iter().all(|v| *v < 0.97));
    Ok(())
}

#[test]
fn rejections_are_counter_examples_with_their_score() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = collection(temp.path())?;
    let scores = recognition::person_scores(&c, KEY, 0.40, &Control::default())?;
    let anna = scores.iter().find(|s| s.person == 1).expect("Anna");
    let mut rejected = anna.rejected.clone();
    rejected.sort_by(f32::total_cmp);
    // 10 und 32 (offen, abgelehnt); 11 ist ignoriert und zählt nicht.
    let ten = top3(vec![0.5f32.cos(), 0.5f32.cos(), 0.2f32.cos()]);
    let thirty_two = top3(vec![0.05f32.cos(), 0.05f32.cos(), 0.25f32.cos()]);
    assert_eq!(rejected.len(), 2);
    assert!((rejected[0] - ten).abs() < 1e-5);
    assert!((rejected[1] - thirty_two).abs() < 1e-5);
    let stats = person_stats::build(scores, recognition::Limits::default());
    let stat = stats.people.iter().find(|p| p.person == 1).expect("Anna");
    let t = stat.threshold.expect("Schwelle");
    assert!((t.t - thirty_two).abs() < 1e-5);
    assert_eq!(t.confirmed_above, 0, "cos 0,3 liegt unter dem Wert von 32");
    Ok(())
}

#[test]
fn rated_faces_belong_to_their_best_admissible_person() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = collection(temp.path())?;
    let scores = sorted(recognition::person_scores(
        &c,
        KEY,
        0.40,
        &Control::default(),
    )?);
    let anna = scores.iter().find(|s| s.person == 1).expect("Anna");
    let ben = scores.iter().find(|s| s.person == 2).expect("Ben");
    // 30 und 34 für Anna; 32 (abgelehnt) und 33 (Anna im Foto) fallen auf Ben unter 0,40.
    assert_eq!(anna.rated.len(), 2);
    let thirty = top3(vec![0.1f32.cos(), 0.1f32.cos(), 0.2f32.cos()]);
    let thirty_four = top3(vec![1.0, 1.0, 0.3f32.cos()]);
    // Aufsteigend: 34 (0,985) vor 30 (0,992).
    assert!((anna.rated[0] - thirty_four).abs() < 1e-5);
    assert!((anna.rated[1] - thirty).abs() < 1e-5);
    assert_eq!(ben.rated.len(), 1);
    assert!((ben.rated[0] - 0.05f32.cos()).abs() < 1e-5);
    // Ohne Untergrenze landen 10, 32 und 33 (Anna ausgeschlossen) bei Ben.
    let all = recognition::person_scores(&c, KEY, -1.0, &Control::default())?;
    assert_eq!(
        all.iter().find(|s| s.person == 2).expect("Ben").rated.len(),
        4
    );
    Ok(())
}

#[test]
fn cached_and_streamed_scores_are_identical() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = collection(temp.path())?;
    let control = Control::default();
    let mut cache = recognition::MatchCache::default();
    recognition::rematch_with(&mut c, KEY, &control, &mut cache)?;
    assert!(!cache.is_empty());
    let cached = sorted(recognition::person_scores_with(
        &c,
        KEY,
        0.40,
        &control,
        Some(&mut cache),
    )?);
    let streamed = sorted(recognition::person_scores_with(
        &c, KEY, 0.40, &control, None,
    )?);
    type Bits = (i64, Vec<u32>, Vec<u32>, Vec<u32>);
    let bits = |s: &[PersonScores]| -> Vec<Bits> {
        s.iter()
            .map(|p| {
                let b = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect();
                (p.person, b(&p.rated), b(&p.confirmed), b(&p.rejected))
            })
            .collect()
    };
    assert_eq!(bits(&cached), bits(&streamed));
    Ok(())
}

#[test]
fn modes_order_people_by_their_own_counts() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = collection(temp.path())?;
    let stats = person_stats::compute(&c, &Control::default())?;
    let names = |mode| -> Vec<String> {
        stats
            .order(mode)
            .iter()
            .map(|i| stats.people[*i].name.clone())
            .collect()
    };
    // Bewertet: Anna 2, Ben 1. Bestätigt samt Ablehnungen: Anna 3 + 2, Ben 2.
    assert_eq!(names(Mode::Rated), ["Anna", "Ben"]);
    assert_eq!(names(Mode::Confirmed), ["Anna", "Ben"]);
    let ben = stats.people.iter().find(|p| p.person == 2).expect("Ben");
    assert_eq!(ben.confirmed.points.len(), 2, "unter fünf Werten Punkte");
    assert!(ben.threshold.is_none(), "ohne Ablehnung keine Schwelle");
    assert_eq!((stats.review, stats.sure), (0.60, 0.70));
    Ok(())
}

#[test]
fn signature_changes_with_the_data_and_only_then_recomputes() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = collection(temp.path())?;
    drop(c);
    // Nur lesend geöffnet: die Statistik schreibt nichts.
    let reader = Collection::open(temp.path(), false)?;
    let control = Control::default();
    let (first, stats) = person_stats::refresh(&reader, None, &control)?;
    assert!(stats.is_some());
    let (same, again) = person_stats::refresh(&reader, Some(&first), &control)?;
    assert_eq!(same, first);
    assert!(
        again.is_none(),
        "unveränderte Daten werden nicht neu gerechnet"
    );
    // Eine Bestätigung ändert den Fingerabdruck.
    let writer = Collection::open(temp.path(), true)?;
    writer.db.execute(
        "UPDATE faces SET person_id=1,assignment='user' WHERE id=30",
        [],
    )?;
    let (changed, fresh) = person_stats::refresh(&reader, Some(&first), &control)?;
    assert_ne!(changed, first);
    let fresh = fresh.expect("neu gerechnet");
    let anna = fresh.people.iter().find(|p| p.person == 1).expect("Anna");
    assert_eq!(anna.confirmed.n, 4);
    // Ein neues Gesicht ohne Person (Analyse) ändert ihn ebenso.
    face(&writer, 40, 40, "2024-01-01", at(0.2));
    let (newer, _) = person_stats::refresh(&reader, Some(&changed), &control)?;
    assert_ne!(newer, changed);
    // Lokaler Cache: gespeichert und wieder gelesen.
    let dir = tempfile::tempdir()?;
    let cached = person_stats::Cached {
        signature: newer.clone(),
        stats: fresh.clone(),
    };
    person_stats::store(dir.path(), "C:\\Sammlung", &cached);
    assert_eq!(
        person_stats::cached(dir.path(), "C:\\Sammlung"),
        Some(cached)
    );
    Ok(())
}
