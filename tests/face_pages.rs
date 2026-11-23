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
    // Gewählte Gruppengröße: 100er-Gruppen, zwei je Seite; 5er-Gruppen, acht je Seite.
    let sized = |page: usize, size: usize| {
        faces::page_with(&c, "unsuitable", page, None, size, &Control::default())
    };
    let large = sized(0, 100)?;
    assert_eq!(
        large.groups.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![100, 100]
    );
    assert_eq!((large.total_groups, large.group_size), (3, 100));
    assert_eq!(sized(1, 100)?.groups.concat().len(), count as usize - 200);
    let small = sized(0, 5)?;
    assert_eq!(small.groups.len(), 8);
    assert!(small.groups.iter().all(|g| g.len() == 5));
    assert_eq!(small.total_groups, (count as usize).div_ceil(5));
    // Außerhalb der Grenzen wird geklemmt.
    assert_eq!(sized(0, 1000)?.group_size, 100);
    assert_eq!(sized(0, 0)?.group_size, 5);
    Ok(())
}

#[test]
fn naming_splits_large_clusters_for_small_sizes_and_never_combines_them() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = open(&temp)?;
    for id in 1..=40 {
        face(&c, id, id, 1.0, Some([1.0, 0.0]));
    }
    group_all(&mut c)?;
    let cluster = foto_importer::config::CLUSTER_MAX;
    let sized = |page: usize, size: usize| {
        faces::page_with(&c, "unknown", page, None, size, &Control::default())
    };
    // Größe 100: die gespeicherten Gruppen (28 und 12) bleiben getrennt.
    let large = sized(0, 100)?;
    let lens: Vec<usize> = large.groups.iter().map(Vec::len).collect();
    assert_eq!(lens, vec![cluster, 40 - cluster]);
    assert_eq!(large.total_groups, 2);
    // Größe 5: jede Gruppe in Teilen zu 5, ohne Mitglieder zweier Gruppen zu mischen.
    let first = sized(0, 5)?;
    let second = sized(1, 5)?;
    let expected_total = cluster.div_ceil(5) + (40 - cluster).div_ceil(5);
    assert_eq!(first.total_groups, expected_total);
    assert_eq!(first.groups.len(), 8);
    let all: Vec<Vec<i64>> = [ids(&first), ids(&second)].concat();
    assert_eq!(all.len(), expected_total);
    assert!(all.iter().all(|g| g.len() <= 5));
    let members: BTreeSet<i64> = large.groups[0].iter().map(|f| f.id).collect();
    for group in &all {
        let inside = group.iter().filter(|id| members.contains(id)).count();
        assert!(
            inside == 0 || inside == group.len(),
            "Teil mischt zwei Gruppen"
        );
    }
    let mut seen: Vec<i64> = all.concat();
    seen.sort();
    assert_eq!(seen, (1..=40).collect::<Vec<_>>());
    // Die gespeicherten Gruppen hängen nicht von der Anzeigegröße ab.
    let largest: i64 =
        c.db.query_row("SELECT max(members) FROM face_group_catalog", [], |r| {
            r.get(0)
        })?;
    assert_eq!(largest as usize, cluster);
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
    assert!(found.iter().all(|f| f.face != 5));
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
    assert_eq!(found.iter().map(|f| f.face).collect::<Vec<_>>(), expected);
    assert!(found.windows(2).all(|w| w[0].score >= w[1].score));
    assert!((found[1].score - 0.05_f32.cos()).abs() < 1e-6);
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
    assert!(candidates.iter().all(|f| f.face != wrong), "abgelehnt");
    Ok(())
}

fn at(angle: f32) -> Option<[f32; 2]> {
    Some([angle.cos(), angle.sin()])
}
/// Anna (1) mit Referenzen bei Winkel 0, Ben (2) bei 0,6. 30 offene Gesichter liegen näher an
/// Anna, 33 näher an Ben; 300 und 301 (Winkel 0,5, gleicher Wert) lägen bei Ben, aber 300 ist
/// für Ben abgelehnt und 301 steht in einem Foto, in dem Ben bestätigt ist.
fn two_people_collection(temp: &tempfile::TempDir) -> anyhow::Result<(Collection, Vec<i64>)> {
    let c = open(temp)?;
    for id in 1..=3 {
        face(&c, id, id, 1.0, at(0.0));
    }
    for id in 11..=13 {
        face(&c, id, id, 1.0, at(0.6));
    }
    let mut anna = Vec::new();
    for n in 0..30 {
        face(&c, 100 + n, 100 + n, 1.0, at(0.01 + 0.009 * n as f32));
        anna.push(100 + n);
    }
    let mut ben = Vec::new();
    for n in 0..33 {
        face(&c, 200 + n, 200 + n, 1.0, at(0.32 + 0.02 * n as f32));
        ben.push(200 + n);
    }
    face(&c, 300, 300, 1.0, at(0.5));
    face(&c, 301, 11, 1.0, at(0.5));
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2);
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben');
         UPDATE faces SET person_id=1,assignment='user',confidence=1 WHERE id IN (1,2,3);
         UPDATE faces SET person_id=2,assignment='user',confidence=1 WHERE id IN (11,12,13);
         INSERT INTO face_rejections VALUES(300,2);",
    )?;
    // Erster Abschnitt: nach Wert absteigend, also nach Winkel; 300 und 301 haben denselben
    // Wert und folgen nach ID.
    anna.extend([300, 301]);
    Ok((c, [anna, ben].concat()))
}

#[test]
fn faces_that_look_more_like_someone_else_come_in_a_second_section() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let (mut c, expected) = two_people_collection(&temp)?;
    let control = Control::default();
    let streamed = recognition::person_candidates_with(&c, KEY, 1, 0.40, &control, None)?;
    assert_eq!(
        streamed.iter().map(|f| f.face).collect::<Vec<_>>(),
        expected
    );
    let (first, second) = streamed.split_at(32);
    assert!(first.iter().all(|f| f.better.is_none()));
    assert!(first.windows(2).all(|w| w[0].score >= w[1].score));
    assert_eq!(first[30].score.to_bits(), first[31].score.to_bits());
    assert!(second.windows(2).all(|w| w[0].score >= w[1].score));
    for found in second {
        let (person, score) = found.better.expect("Ben liegt vorn");
        assert_eq!(person, 2);
        assert!(score > found.score);
        let angle = 0.32 + 0.02 * (found.face - 200) as f32;
        assert!((score - (angle - 0.6).cos()).abs() < 1e-5);
    }
    // Der Weg über den Zwischenspeicher des Abgleichs liefert bitgleich dasselbe.
    let mut cache = recognition::MatchCache::default();
    recognition::rematch_with(&mut c, KEY, &control, &mut cache)?;
    assert!(!cache.is_empty());
    let cached = recognition::person_candidates_with(&c, KEY, 1, 0.40, &control, Some(&mut cache))?;
    let again = recognition::person_candidates_with(&c, KEY, 1, 0.40, &control, None)?;
    assert_eq!(cached, again);
    assert_eq!(
        cached, streamed,
        "Vorschläge des Abgleichs ändern die Liste nicht"
    );
    type Bits = (i64, u32, Option<(i64, u32)>);
    let bits = |list: &[recognition::PersonCandidate]| -> Vec<Bits> {
        list.iter()
            .map(|f| {
                (
                    f.face,
                    f.score.to_bits(),
                    f.better.map(|b| (b.0, b.1.to_bits())),
                )
            })
            .collect()
    };
    assert_eq!(bits(&cached), bits(&streamed));
    // Seite: 32 + 33 Gesichter in Gruppen zu 28, ohne die Abschnitte zu mischen.
    let page = faces::page_filtered(&c, "probable", 0, Some(1))?;
    let sizes: Vec<usize> = page.groups.iter().map(Vec::len).collect();
    assert_eq!(sizes, vec![28, 4, 28, 5]);
    assert_eq!(
        (page.faces_total, page.probable_others, page.total_groups),
        (65, 33, 4)
    );
    for group in &page.groups {
        let marked = group
            .iter()
            .filter(|f| page.better.contains_key(&f.id))
            .count();
        assert!(
            marked == 0 || marked == group.len(),
            "Gruppe mischt die Abschnitte"
        );
    }
    assert_eq!(
        page.groups
            .concat()
            .iter()
            .map(|f| f.id)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(page.better.get(&200).map(|b| b.0), Some(2));
    assert!(!page.better.contains_key(&300) && !page.better.contains_key(&301));
    // Mit der größten Gruppengröße bleibt jeder Abschnitt eine Gruppe, mit der kleinsten
    // werden beide für sich geschnitten.
    let sized =
        |size: usize| faces::page_with(&c, "probable", 0, Some(1), size, &Control::default());
    let large = sized(100)?;
    assert_eq!(
        large.groups.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![32, 33]
    );
    let small = sized(5)?;
    assert_eq!(
        small.groups.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![5, 5, 5, 5, 5, 5, 2, 5]
    );
    assert_eq!(small.total_groups, 7 + 7);
    Ok(())
}

/// Anna (1) mit Referenzen bei Winkel 0, Ben (2) bei 0,6. Abgelehnt „nicht Anna“: 400 (0,4),
/// 401 (0,1), 402 (0,2, Ben bestätigt), 403 ohne Merkmale; nicht gezeigt: 404 ignoriert,
/// 405 „Kein Gesicht“, 406 nur „nicht Ben“.
fn rejected_collection(temp: &tempfile::TempDir) -> anyhow::Result<Collection> {
    let c = open(temp)?;
    for id in 1..=3 {
        face(&c, id, id, 1.0, at(0.0));
    }
    for id in 11..=13 {
        face(&c, id, id, 1.0, at(0.6));
    }
    face(&c, 400, 400, 1.0, at(0.4));
    face(&c, 401, 401, 1.0, at(0.1));
    face(&c, 402, 402, 1.0, at(0.2));
    face(&c, 403, 403, 0.3, None);
    face(&c, 404, 404, 1.0, at(0.05));
    face(&c, 405, 405, 1.0, at(0.0));
    face(&c, 406, 406, 1.0, at(0.0));
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2);
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben');
         UPDATE faces SET person_id=1,assignment='user',confidence=1 WHERE id IN (1,2,3);
         UPDATE faces SET person_id=2,assignment='user',confidence=1 WHERE id IN (11,12,13,402);
         INSERT INTO face_rejections VALUES(400,1),(401,1),(402,1),(403,1),(404,1),(405,1),(406,2);
         UPDATE faces SET ignored=1 WHERE id=404;
         UPDATE faces SET ignored=2 WHERE id=405;",
    )?;
    Ok(c)
}

fn rejected_ids(c: &Collection) -> anyhow::Result<Vec<i64>> {
    Ok(ids(&faces::page_filtered(c, "person-rejected", 0, Some(1))?).concat())
}

/// Zustand eines Gesichts: Person, Zuordnungsart, abgelehnte Personen.
fn face_state(c: &Collection, id: i64) -> anyhow::Result<(Option<i64>, Option<String>, Vec<i64>)> {
    let (person, assignment) = c.db.query_row(
        "SELECT person_id,assignment FROM faces WHERE id=?",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let rejected =
        c.db.prepare("SELECT person_id FROM face_rejections WHERE face_id=? ORDER BY person_id")?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
    Ok((person, assignment, rejected))
}

/// „Nicht X“: genau die aktiven Gesichter mit Ablehnung „nicht Anna“, ähnlichste zuerst, mit
/// dem Wert der Personenstatistik; ohne Wert zuletzt. Die Zahl steht auf allen Personenseiten.
#[test]
fn rejected_page_lists_rejections_of_the_person_most_similar_first() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = rejected_collection(&temp)?;
    let page = faces::page_filtered(&c, "person-rejected", 0, Some(1))?;
    assert_eq!(ids(&page), vec![vec![401, 402, 400, 403]]);
    assert_eq!((page.faces_total, page.total_groups), (4, 1));
    assert_eq!(page.rejected_total, 4);
    assert!(page.groups[0].iter().all(|f| f.person == Some(1)));
    assert_eq!(page.confirmed_as, [(402, 2)].into_iter().collect());
    assert!(page.groups[0][3].score.is_nan(), "ohne Merkmale kein Wert");
    // Dieselben Werte wie die rote Hälfte der Personenstatistik.
    let stats = recognition::person_scores(&c, KEY, 0.40, &Control::default())?;
    let anna = stats.iter().find(|s| s.person == 1).expect("Anna");
    let mut expected = anna.rejected.clone();
    expected.sort_by(|a, b| b.total_cmp(a));
    let shown: Vec<u32> = page.groups[0][..3]
        .iter()
        .map(|f| f.score.to_bits())
        .collect();
    assert_eq!(
        shown,
        expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
    for mode in ["probable", "person-confirmed"] {
        let other = faces::page_filtered(&c, mode, 0, Some(1))?;
        assert_eq!(other.rejected_total, 4, "{mode}");
    }
    assert!(
        faces::page_filtered(&c, "person-rejected", 0, None)?
            .groups
            .is_empty()
    );
    // Ben: nur 406.
    assert_eq!(
        ids(&faces::page_filtered(&c, "person-rejected", 0, Some(2))?),
        vec![vec![406]]
    );
    // Gruppengröße wie überall.
    let small = faces::page_with(&c, "person-rejected", 0, Some(1), 5, &Control::default())?;
    assert_eq!(ids(&small), vec![vec![401, 402, 400, 403]]);
    Ok(())
}

/// Ablehnung aufheben: nur die Ablehnung entfällt, der Abgleich ist vorgemerkt, der Datenstand
/// ändert sich; Strg+Z stellt sie exakt wieder her, auch nach einem Vorschlag des Abgleichs.
/// „Doch X“ hebt auf und ordnet zu; die Rücknahme stellt auch die vorherige Zuordnung her.
#[test]
fn lifting_and_confirming_rejections_undo_exactly() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = rejected_collection(&temp)?;
    let pending = |c: &Collection| -> anyhow::Result<Option<i64>> {
        Ok(c.db
            .query_row(
                "SELECT generation FROM face_maintenance WHERE embedder_id=?",
                [KEY],
                |r| r.get(0),
            )
            .ok())
    };
    c.db.execute("DELETE FROM face_maintenance", [])?;
    let revision = foto_importer::core::revision::read(&c.db)?.expect("Revision");
    // Kreuz: nur die Ablehnung von 401.
    let undo = faces::lift_rejection(&mut c, &[401], 1)?;
    assert_eq!(undo.summary().lifted, 1);
    assert_eq!(undo.summary().confirmed, 0);
    assert_eq!(face_state(&c, 401)?, (None, None, vec![]));
    assert!(pending(&c)?.is_some(), "Abgleich vorgemerkt");
    let after = foto_importer::core::revision::read(&c.db)?.expect("Revision");
    assert!(
        after.faces > revision.faces,
        "Datenstand Gesichter geändert"
    );
    assert_eq!(rejected_ids(&c)?, vec![402, 400, 403]);
    // Inzwischen schlägt der Abgleich Anna vor; die Rücknahme gelingt trotzdem.
    c.db.execute(
        "UPDATE faces SET person_id=1,assignment='suggested',confidence=0.7 WHERE id=401",
        [],
    )?;
    faces::undo(&mut c, &undo)?;
    assert_eq!(face_state(&c, 401)?, (None, None, vec![1]));
    assert_eq!(rejected_ids(&c)?, vec![401, 402, 400, 403]);
    // Eine spätere Nutzerentscheidung verhindert die Rücknahme weiterhin.
    let undo = faces::lift_rejection(&mut c, &[400], 1)?;
    faces::name_group(&mut c, &[400], &BTreeSet::from([400]), Some(2), "", false)?;
    assert!(faces::undo(&mut c, &undo).is_err());
    // Haken: 402 (Ben bestätigt) ist doch Anna.
    let undo = faces::name_group(&mut c, &[402], &BTreeSet::from([402]), Some(1), "", false)?;
    assert_eq!(undo.summary().confirmed, 1);
    assert_eq!(face_state(&c, 402)?, (Some(1), Some("user".into()), vec![]));
    assert_eq!(rejected_ids(&c)?, vec![401, 403]);
    faces::undo(&mut c, &undo)?;
    assert_eq!(
        face_state(&c, 402)?,
        (Some(2), Some("user".into()), vec![1])
    );
    // Ein Gesicht ohne diese Ablehnung bleibt unverändert.
    let undo = faces::lift_rejection(&mut c, &[406], 1)?;
    assert_eq!(undo.summary().lifted, 0);
    assert_eq!(face_state(&c, 406)?, (None, None, vec![2]));
    Ok(())
}
