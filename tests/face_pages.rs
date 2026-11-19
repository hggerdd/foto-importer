//! Reiter „Ungeeignet“ mit „Kein Gesicht“ und die Gesichterseite einer Person
//! („Wahrscheinlich X“, „Bestätigt“). Nur temporäre Sammlungen.
use foto_importer::core::{collection::Collection, faces, jobs::Control, recognition};
use rusqlite::params;
use std::collections::BTreeSet;

const KEY: &str = "test";

/// Ein Gesicht in einem eigenen Foto `photo`; `vector` = None heißt ohne Namensmerkmale.
fn face(c: &Collection, id: i64, photo: i64, quality: f64, vector: Option<[f32; 2]>) {
    c.db.execute(
        "INSERT OR IGNORE INTO photos(id,status) VALUES(?,'present')",
        [photo],
    )
    .expect("Foto");
    c.db.execute(
        "INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality,captured_at) VALUES(?,?,?,'det',0,0,1,1,?,'2020-01-01')",
        params![id, photo, format!("{photo}.jpg"), quality],
    )
    .expect("Gesicht");
    if let Some(vector) = vector {
        c.db.execute(
            "INSERT INTO face_embeddings VALUES(?,?,2,?)",
            params![id, KEY, faces::encode(&vector)],
        )
        .expect("Vektor");
    }
}
fn open(temp: &tempfile::TempDir) -> anyhow::Result<Collection> {
    let c = Collection::open(temp.path(), true)?;
    c.db.execute(
        "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key',?)",
        [serde_json::to_string(KEY)?],
    )?;
    Ok(c)
}
fn ids(page: &faces::FacePage) -> Vec<Vec<i64>> {
    page.groups
        .iter()
        .map(|g| g.iter().map(|f| f.id).collect())
        .collect()
}
fn group_all(c: &mut Collection) -> anyhow::Result<()> {
    while faces::prepare_groups(c, KEY, &Control::default())? > 0 {}
    Ok(())
}

/// Vier gruppierbare offene Gesichter, zwei ohne Namensmerkmale, ein bestätigtes.
fn unsuitable_collection(temp: &tempfile::TempDir) -> anyhow::Result<Collection> {
    let mut c = open(temp)?;
    for id in 1..=4 {
        face(&c, id, id, 1.0, Some([1.0, 0.0]));
    }
    face(&c, 5, 5, 0.2, None);
    face(&c, 6, 6, 0.3, None);
    face(&c, 7, 7, 1.0, Some([1.0, 0.0]));
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1);
         INSERT INTO person_attrs VALUES(1,'name','Anna');
         UPDATE faces SET person_id=1,assignment='user' WHERE id=7;",
    )?;
    group_all(&mut c)?;
    Ok(c)
}

#[test]
fn unsuitable_page_lists_exactly_the_faces_of_the_hint() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = unsuitable_collection(&temp)?;
    let status = faces::status(&c)?;
    assert_eq!((status.unknown, status.nameable), (6, 4));
    assert_eq!(status.unsuitable(), 2);
    let page = faces::page(&c, "unsuitable", 0)?;
    assert_eq!(ids(&page), vec![vec![6, 5]], "neueste Fotos zuerst");
    assert_eq!((page.faces_total, page.total_groups), (2, 1));
    assert!(page.suggestions.is_empty());
    Ok(())
}

#[test]
fn unsuitable_faces_are_cut_into_groups_of_28_and_pages_of_8() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = open(&temp)?;
    let count = 28 * 8 + 3;
    for id in 1..=count {
        face(&c, id, id, 0.1, None);
    }
    let first = faces::page(&c, "unsuitable", 0)?;
    assert_eq!(first.groups.len(), 8);
    assert!(first.groups.iter().all(|g| g.len() == 28));
    assert_eq!(first.groups[0][0].id, count);
    assert_eq!((first.faces_total, first.total_groups), (count as usize, 9));
    let second = faces::page(&c, "unsuitable", 1)?;
    assert_eq!(ids(&second), vec![vec![3, 2, 1]]);
    Ok(())
}

#[test]
fn not_a_face_is_a_lasting_decision_outside_every_count() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = unsuitable_collection(&temp)?;
    let before = faces::status(&c)?;
    let undo = faces::mark_not_face(&mut c, &[5, 2])?;
    assert_eq!(undo.summary().not_face, 2);
    assert_eq!(undo.summary().ignored, 0);
    let status = faces::status(&c)?;
    assert_eq!(status.total, before.total - 2);
    assert_eq!(status.unknown, before.unknown - 2);
    assert_eq!(
        status.nameable, 3,
        "das Gruppenmitglied verlässt seine Gruppe"
    );
    assert_eq!(status.unsuitable(), 1);
    assert_eq!(status.ignored, 0, "kein Gesicht ist nicht „ignoriert“");
    assert_eq!(ids(&faces::page(&c, "unsuitable", 0)?), vec![vec![6]]);
    assert!(ids(&faces::page(&c, "ignored", 0)?).is_empty());
    assert!(faces::photo_faces(&c, 5)?.is_empty());
    let unknown: Vec<i64> = ids(&faces::page(&c, "unknown", 0)?).concat();
    assert!(!unknown.contains(&2));
    // Rücknahme stellt den alten Stand her.
    faces::undo(&mut c, &undo)?;
    group_all(&mut c)?;
    assert_eq!(faces::status(&c)?.unsuitable(), 2);
    assert_eq!(faces::photo_faces(&c, 5)?.len(), 1);
    // Eine neue Analyse findet das Vorkommen wieder (neuer Vektor, neue Qualität): die
    // Entscheidung bleibt, das Gesicht wird weder gruppiert noch verglichen.
    faces::mark_not_face(&mut c, &[5])?;
    c.db.execute("UPDATE faces SET quality=1 WHERE id=5", [])?;
    c.db.execute(
        "INSERT OR REPLACE INTO face_embeddings VALUES(5,?,2,?)",
        params![KEY, faces::encode(&[1.0, 0.0])],
    )?;
    group_all(&mut c)?;
    let status = faces::status(&c)?;
    assert_eq!((status.unsuitable(), status.unknown), (1, 5));
    assert_eq!(
        c.db.query_row("SELECT ignored FROM faces WHERE id=5", [], |r| r
            .get::<_, i64>(0))?,
        faces::NOT_A_FACE
    );
    recognition::rematch(&mut c, KEY, &Control::default())?;
    let found = recognition::person_candidates(&c, KEY, 1, 0.0, &Control::default())?;
    assert!(found.iter().all(|(id, _)| *id != 5));
    let assigned: Option<i64> =
        c.db.query_row("SELECT person_id FROM faces WHERE id=5", [], |r| r.get(0))?;
    assert_eq!(assigned, None, "kein Vorschlag für „Kein Gesicht“");
    Ok(())
}

/// Anna (Person 1) mit drei bestätigten Gesichtern in Richtung (1,0); 70 offene Gesichter
/// mit abnehmender Ähnlichkeit (Kosinus = Personenscore) und einige Ausschlüsse.
fn probable_collection(temp: &tempfile::TempDir) -> anyhow::Result<Collection> {
    let c = open(temp)?;
    for id in 1..=3 {
        face(&c, id, id, 1.0, Some([1.0, 0.0]));
    }
    for n in 0..70 {
        let angle = 0.05 + n as f32 * 0.02;
        face(&c, 100 + n, 100 + n, 1.0, Some([angle.cos(), angle.sin()]));
    }
    // 200: im Foto von Anna; 201: für Anna abgelehnt; 202: ignoriert; 203: zu schlecht;
    // 204: Ben vorgeschlagen (bleibt Kandidat).
    face(&c, 200, 1, 1.0, Some([1.0, 0.0]));
    for id in 201..=204 {
        face(&c, id, id, 1.0, Some([1.0, 0.0]));
    }
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2);
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben');
         UPDATE faces SET person_id=1,assignment='user',confidence=1 WHERE id IN (1,2,3);
         INSERT INTO face_rejections VALUES(201,1);
         UPDATE faces SET ignored=1 WHERE id=202;
         UPDATE faces SET quality=0.3 WHERE id=203;
         UPDATE faces SET person_id=2,assignment='suggested',confidence=0.7 WHERE id=204;",
    )?;
    Ok(c)
}

#[test]
fn probable_faces_come_most_likely_first_in_groups_of_28() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = probable_collection(&temp)?;
    let found = recognition::person_candidates(&c, KEY, 1, 0.40, &Control::default())?;
    let expected: Vec<i64> = std::iter::once(204)
        .chain((0..70).filter_map(|n| {
            let angle = 0.05 + n as f32 * 0.02;
            (angle.cos() >= 0.40).then_some(100 + n)
        }))
        .collect();
    assert_eq!(found.iter().map(|f| f.0).collect::<Vec<_>>(), expected);
    assert!(found.windows(2).all(|w| w[0].1 >= w[1].1));
    assert!((found[1].1 - 0.05_f32.cos()).abs() < 1e-6);
    // Dieselbe Liste aus dem Zwischenspeicher des Abgleichs.
    recognition::rematch(&mut c, KEY, &Control::default())?;
    assert_eq!(
        recognition::person_candidates(&c, KEY, 1, 0.40, &Control::default())?,
        found
    );
    let page = faces::page_filtered(&c, "probable", 0, Some(1))?;
    let sizes: Vec<usize> = page.groups.iter().map(Vec::len).collect();
    assert_eq!(sizes, vec![28, 28, found.len() - 56]);
    assert_eq!(
        page.groups
            .concat()
            .iter()
            .map(|f| f.id)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!((page.faces_total, page.total_groups), (found.len(), 3));
    assert!(page.groups.concat().iter().all(|f| f.person == Some(1)));
    assert!(page.portraits.contains_key(&1), "Porträt für die Karte");
    // Ohne Person gibt es nichts zu zeigen.
    assert!(
        faces::page_filtered(&c, "probable", 0, None)?
            .groups
            .is_empty()
    );
    assert!(
        faces::page_filtered(&c, "person-confirmed", 0, None)?
            .groups
            .is_empty()
    );
    Ok(())
}

#[test]
fn confirming_and_rejecting_on_the_person_page_updates_both_lists() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = probable_collection(&temp)?;
    let page = faces::page_filtered(&c, "probable", 0, Some(1))?;
    let top: Vec<i64> = page.groups[0].iter().map(|f| f.id).collect();
    // „27 sind Anna“: das abgewählte erhält eine Ablehnung.
    let kept: BTreeSet<i64> = top[..27].iter().copied().collect();
    faces::name_group(&mut c, &top, &kept, Some(1), "", false)?;
    let after = faces::page_filtered(&c, "probable", 0, Some(1))?;
    let open: Vec<i64> = after.groups.concat().iter().map(|f| f.id).collect();
    assert!(top.iter().all(|id| !open.contains(id)));
    // Ohne gespeicherte Referenzauswahl zählen alle bestätigten Gesichter als Referenz; die
    // neuen machen weitere Gesichter wahrscheinlich.
    assert!(after.faces_total > 0);
    let confirmed = faces::page_filtered(&c, "person-confirmed", 0, Some(1))?;
    assert_eq!(confirmed.faces_total, 3 + 27);
    // Eine falsche Zuordnung entfernen: nur dieses Gesicht wird abgelehnt und unbenannt;
    // die übrigen bestätigten Gesichter bleiben samt Referenzauswahl unberührt.
    c.db.execute(
        "INSERT INTO training_set VALUES(1,1,?,'ref',datetime('now'),'pin',1)",
        [KEY],
    )?;
    let wrong = top[0];
    faces::name_group(&mut c, &[wrong], &BTreeSet::new(), Some(1), "", false)?;
    let confirmed = faces::page_filtered(&c, "person-confirmed", 0, Some(1))?;
    assert_eq!(confirmed.faces_total, 3 + 26);
    assert!(confirmed.groups.concat().iter().all(|f| f.id != wrong));
    let pins: i64 = c.db.query_row(
        "SELECT count(*) FROM training_set WHERE face_id=1",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(pins, 1);
    let candidates = recognition::person_candidates(&c, KEY, 1, 0.0, &Control::default())?;
    assert!(candidates.iter().all(|(id, _)| *id != wrong), "abgelehnt");
    Ok(())
}
