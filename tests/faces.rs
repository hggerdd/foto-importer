use foto_importer::core::{collection::Collection, faces, inference, jobs::Control};
use rusqlite::params;
use std::collections::BTreeSet;
fn seed(c: &Collection, count: i64) -> anyhow::Result<()> {
    for id in 1..=count {
        c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
        c.db.execute("INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality) VALUES(?,?,?,'det',0,0,1,1,1)",params![id,id,format!("{id}.jpg")])?;
        c.db.execute(
            "INSERT INTO face_embeddings VALUES(?,'test',2,?)",
            params![id, faces::encode(&[1.0, 0.0])],
        )?;
    }
    Ok(())
}
#[test]
fn naming_ignore_and_undo_preserve_occurrences() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    seed(&c, 2)?;
    let ids = [1, 2];
    let undo = faces::name_group(&mut c, &ids, &BTreeSet::from([1]), None, "Alex", false)?;
    let person: i64 =
        c.db.query_row("SELECT person_id FROM faces WHERE id=1", [], |r| r.get(0))?;
    assert_eq!(
        c.db.query_row(
            "SELECT person_id FROM face_rejections WHERE face_id=2",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        person
    );
    faces::undo(&mut c, &undo)?;
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM persons", [], |r| r.get::<_, i64>(0))?,
        0
    );
    let undo = faces::name_group(&mut c, &ids, &BTreeSet::new(), None, "", true)?;
    assert_eq!(faces::status(&c)?.ignored, 2);
    faces::undo(&mut c, &undo)?;
    assert_eq!(faces::status(&c)?.total, 2);
    assert_eq!(faces::status(&c)?.ignored, 0);
    Ok(())
}
#[test]
fn undo_rejects_later_user_decision() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    seed(&c, 1)?;
    let undo = faces::name_group(&mut c, &[1], &BTreeSet::from([1]), None, "Alex", false)?;
    faces::name_group(&mut c, &[1], &BTreeSet::new(), None, "", true)?;
    assert!(faces::undo(&mut c, &undo).is_err());
    assert_eq!(faces::status(&c)?.ignored, 1);
    Ok(())
}
#[test]
fn complete_link_groups_never_exceed_fourteen() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    seed(&c, 40)?;
    while faces::prepare_groups(&mut c, "test", &Control::default())? > 0 {}
    let maximum: i64 =
        c.db.query_row("SELECT max(members) FROM face_group_catalog", [], |r| {
            r.get(0)
        })?;
    assert!(maximum <= foto_importer::config::GROUP_MAX as i64);
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM face_group_members", [], |r| r
            .get::<_, i64>(0))?,
        40
    );
    Ok(())
}
#[test]
fn delete_face_removes_all_vector_spaces_but_keeps_photo() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    seed(&c, 1)?;
    c.db.execute(
        "INSERT INTO face_embeddings VALUES(1,'other',2,?)",
        [faces::encode(&[0.0, 1.0])],
    )?;
    faces::delete_face(&mut c, 1, 1)?;
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM face_embeddings", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM photos", [], |r| r.get::<_, i64>(0))?,
        1
    );
    Ok(())
}
#[test]
fn parent_cycles_and_self_relations_are_rejected() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    for _ in 0..3 {
        c.db.execute("INSERT INTO persons DEFAULT VALUES", [])?;
    }
    faces::add_relation(&mut c, 1, "parent", 2)?;
    faces::add_relation(&mut c, 2, "parent", 3)?;
    assert!(faces::add_relation(&mut c, 3, "parent", 1).is_err());
    assert!(faces::add_relation(&mut c, 1, "partner", 1).is_err());
    Ok(())
}
#[test]
fn vector_spaces_and_invalid_values_are_rejected() {
    assert!(inference::cosine(&[1.0], &[1.0, 0.0]).is_none());
    assert!(inference::normalize(&[0.0, 0.0]).is_err());
    assert!(faces::decode(&[1, 2, 3]).is_err());
    assert!(inference::normalize(&[f32::NAN]).is_err());
}
#[test]
fn review_groups_come_most_certain_first_and_do_not_mix_people() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    seed(&c, 51)?;
    // Person 1: ein Gesicht (0,65); Person 2: 20 Gesichter (0,9); Person 3: 30 Gesichter
    // mit steigender Ähnlichkeit (0,622 bis 0,651).
    c.db.execute_batch("INSERT INTO persons(id) VALUES(1),(2),(3); UPDATE faces SET person_id=CASE WHEN id<=1 THEN 1 WHEN id<=21 THEN 2 ELSE 3 END,assignment='suggested',confidence=CASE WHEN id<=1 THEN 0.65 WHEN id<=21 THEN 0.9 ELSE 0.6+id*0.001 END;")?;
    let page = faces::page(&c, "review", 0)?;
    assert_eq!(
        page.groups
            .iter()
            .map(|g| (g[0].person, g.len()))
            .collect::<Vec<_>>(),
        vec![(Some(2), 20), (Some(1), 1), (Some(3), 28), (Some(3), 2)],
        "nach mittlerer Ähnlichkeit, sicherste Gruppe zuerst"
    );
    assert_eq!(page.total_groups, 4, "Seitenzahl kennt alle Gruppen");
    for group in &page.groups {
        assert!(group.iter().all(|f| f.person == group[0].person));
        assert!(
            group.windows(2).all(|pair| pair[0].score >= pair[1].score),
            "in der Gruppe stehen die sichersten Gesichter vorn"
        );
    }
    assert_eq!(page.groups[2][0].id, 51);
    assert_eq!(
        page.groups[3].iter().map(|f| f.id).collect::<Vec<_>>(),
        vec![23, 22],
        "der schwächste Rest bildet die letzte Gruppe"
    );
    assert_eq!(
        page.review_people,
        std::collections::BTreeMap::from([(1, 1), (2, 20), (3, 30)])
    );
    Ok(())
}
#[test]
fn review_filter_shows_one_person_and_decisions_leave_confirmed_faces_alone() -> anyhow::Result<()>
{
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    suggested_for_anna(&c, 6)?;
    // Anna: 1 und 2 vorgeschlagen (0,5), 3 sicher (0,75), 6 vom Nutzer bestätigt.
    // Ben: 4 und 5 vorgeschlagen (0,8).
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(2);
         INSERT INTO person_attrs VALUES(2,'name','Ben');
         UPDATE faces SET person_id=2,confidence=0.8 WHERE id IN (4,5);
         UPDATE faces SET assignment='auto',confidence=0.75 WHERE id=3;
         UPDATE faces SET assignment='user',confidence=1 WHERE id=6;",
    )?;
    let ids = |page: &faces::FacePage| -> Vec<Vec<i64>> {
        page.groups
            .iter()
            .map(|g| g.iter().map(|f| f.id).collect())
            .collect()
    };
    let all = faces::page(&c, "review", 0)?;
    assert_eq!(ids(&all), vec![vec![4, 5], vec![3, 1, 2]]);
    let anna = faces::page_filtered(&c, "review", 0, Some(1))?;
    assert_eq!(ids(&anna), vec![vec![3, 1, 2]]);
    assert_eq!(anna.total_groups, 1);
    assert_eq!(
        anna.review_people,
        std::collections::BTreeMap::from([(1, 3), (2, 2)]),
        "die Auswahlliste kennt weiter alle Personen mit Vorschlägen"
    );
    // „Bestätigt“ behält seine Reihenfolge und beachtet den Filter ebenfalls.
    assert_eq!(ids(&faces::page(&c, "confirmed", 0)?), vec![vec![6]]);
    assert_eq!(
        ids(&faces::page_filtered(&c, "confirmed", 0, Some(1))?),
        vec![vec![6]]
    );
    assert!(
        faces::page_filtered(&c, "confirmed", 0, Some(2))?
            .groups
            .is_empty()
    );
    // „Keines ist Anna“ für die Gruppe: nur die Vorschläge ändern sich.
    let undo = faces::name_group(&mut c, &[3, 1, 2], &BTreeSet::new(), Some(1), "", false)?;
    assert_eq!(undo.summary().rejected, 3);
    assert_eq!(state(&c, 6)?, (Some(1), Some("user".into())));
    assert_eq!(state(&c, 4)?, (Some(2), Some("suggested".into())));
    assert_eq!(
        count(&c, "SELECT count(*) FROM face_rejections WHERE person_id=1")?,
        3
    );
    let anna = faces::page_filtered(&c, "review", 0, Some(1))?;
    assert!(anna.groups.is_empty());
    assert_eq!(anna.total_groups, 0);
    // „Alle 2 sind Ben“ bestätigt Bens Gruppe; Annas bestätigtes Gesicht bleibt, wie es war.
    faces::name_group(&mut c, &[4, 5], &BTreeSet::from([4, 5]), Some(2), "", false)?;
    assert_eq!(state(&c, 4)?, (Some(2), Some("user".into())));
    assert_eq!(state(&c, 5)?, (Some(2), Some("user".into())));
    assert_eq!(state(&c, 6)?, (Some(1), Some("user".into())));
    assert_eq!(
        count(&c, "SELECT count(*) FROM faces WHERE id=6 AND confidence=1")?,
        1
    );
    assert!(faces::page(&c, "review", 0)?.groups.is_empty());
    Ok(())
}
#[test]
fn ignore_undo_restores_reference_pins_and_guards_later_pin_changes() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    seed(&c, 1)?;
    c.db.execute(
        "INSERT INTO face_settings VALUES('rust.embedder_key','\"test\"')",
        [],
    )?;
    let first = faces::name_group(&mut c, &[1], &BTreeSet::from([1]), None, "Alex", false)?;
    foto_importer::core::maintenance::reference_action(&c, 1, "pin")?;
    assert!(faces::undo(&mut c, &first).is_err());
    let ignored = faces::name_group(&mut c, &[1], &BTreeSet::new(), None, "", true)?;
    assert_eq!(
        c.db.query_row(
            "SELECT count(*) FROM training_set WHERE pinned=1",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        0
    );
    faces::undo(&mut c, &ignored)?;
    assert_eq!(
        c.db.query_row(
            "SELECT count(*) FROM training_set WHERE pinned=1",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        1
    );
    Ok(())
}
#[test]
fn analysis_coverage_counts_only_the_current_detector() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    seed(&c, 3)?;
    c.db.execute(
        "INSERT INTO face_settings(key,value) VALUES('rust.detector_key','\"yunet\"')",
        [],
    )?;
    let current = faces::active_detector_key(&c)?.ok_or_else(|| anyhow::anyhow!("detector"))?;
    for (photo, model) in [(1, current.as_str()), (2, "yunet")] {
        c.db.execute(
            "INSERT INTO face_jobs(photo_id,stage,model_id,status) VALUES(?,'detect',?,'done')",
            params![photo, model],
        )?;
    }
    let status = faces::status(&c)?;
    assert_eq!((status.analysed_photos, status.present_photos), (1, 3));
    Ok(())
}
fn polar(c: &Collection, id: i64, photo: i64, degrees: f32) -> anyhow::Result<()> {
    c.db.execute(
        "INSERT OR IGNORE INTO photos(id,status) VALUES(?,'present')",
        [photo],
    )?;
    c.db.execute("INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality) VALUES(?,?,?,'det',0,0,1,1,1)", params![id, photo, format!("{id}.jpg")])?;
    let r = degrees.to_radians();
    c.db.execute(
        "INSERT INTO face_embeddings VALUES(?,'test',2,?)",
        params![id, faces::encode(&[r.cos(), r.sin()])],
    )?;
    Ok(())
}
#[test]
fn small_groups_join_similar_groups_but_not_dissimilar_or_same_photo() -> anyhow::Result<()> {
    use foto_importer::core::grouping;
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    for (id, degrees) in [(1, 0.0), (2, 4.0), (3, -4.0), (4, 2.0)] {
        polar(&c, id, id, degrees)?;
    }
    polar(&c, 10, 10, 55.0)?; // ähnlich genug, aber unter dem strengen Wert
    polar(&c, 11, 1, 58.0)?; // teilt ein Foto mit Gesicht 1
    polar(&c, 12, 12, -55.0)?; // ähnlich genug und ohne Fotokonflikt
    polar(&c, 13, 13, -58.0)?;
    polar(&c, 20, 20, 85.0)?; // zu verschieden
    polar(&c, 21, 21, 88.0)?;
    while faces::prepare_groups(&mut c, "test", &Control::default())? > 0 {}
    let before: i64 = c.db.query_row(
        "SELECT count(*) FROM face_group_catalog WHERE members>0",
        [],
        |r| r.get(0),
    )?;
    let merged = grouping::merge_small_groups(&mut c, "test", &Control::default())?;
    let group_of = |face: i64| -> anyhow::Result<i64> {
        Ok(c.db.query_row(
            "SELECT group_id FROM face_group_members WHERE face_id=?",
            [face],
            |r| r.get(0),
        )?)
    };
    assert!(merged > 0);
    assert_eq!(group_of(12)?, group_of(13)?);
    assert_eq!(
        group_of(12)?,
        group_of(1)?,
        "Paar schließt sich der großen Gruppe an"
    );
    assert_ne!(
        group_of(10)?,
        group_of(1)?,
        "kein zweites Gesicht desselben Fotos"
    );
    assert_ne!(group_of(20)?, group_of(1)?);
    let after: i64 = c.db.query_row(
        "SELECT count(*) FROM face_group_catalog WHERE members>0",
        [],
        |r| r.get(0),
    )?;
    assert!(after < before);
    let total: i64 =
        c.db.query_row("SELECT count(*) FROM face_group_members", [], |r| r.get(0))?;
    assert_eq!(total, 10);
    let largest: i64 =
        c.db.query_row("SELECT max(members) FROM face_group_catalog", [], |r| {
            r.get(0)
        })?;
    assert!(largest <= foto_importer::config::GROUP_MAX as i64);
    Ok(())
}
#[test]
fn a_group_of_the_maximum_size_can_be_named_and_larger_ones_are_rejected() -> anyhow::Result<()> {
    use foto_importer::config::GROUP_MAX;
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    seed(&c, GROUP_MAX as i64 + 1)?;
    let all: Vec<i64> = (1..=GROUP_MAX as i64).collect();
    // Regression: die Obergrenze der Anzeige (28) muss auch beim Speichern gelten, nicht nur 14.
    let selected: BTreeSet<i64> = all.iter().copied().collect();
    let undo = faces::name_group(&mut c, &all, &selected, None, "Alex", false)?;
    let named: i64 = c.db.query_row(
        "SELECT count(*) FROM faces WHERE assignment='user' AND person_id IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(named, GROUP_MAX as i64);
    faces::undo(&mut c, &undo)?;
    let too_many: Vec<i64> = (1..=GROUP_MAX as i64 + 1).collect();
    let selected: BTreeSet<i64> = too_many.iter().copied().collect();
    let message = match faces::name_group(&mut c, &too_many, &selected, None, "Alex", false) {
        Ok(_) => anyhow::bail!("zu große Gruppe wurde akzeptiert"),
        Err(error) => error.to_string(),
    };
    assert!(message.contains(&GROUP_MAX.to_string()), "{message}");
    Ok(())
}
#[test]
fn a_large_group_can_be_confirmed_with_only_part_of_its_faces() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    // Mehr als die frühere Grenze von 14 sichtbaren Gesichtern.
    seed(&c, 20)?;
    let all: Vec<i64> = (1..=20).collect();
    let selected: BTreeSet<i64> = (1..=9).collect();
    faces::name_group(&mut c, &all, &selected, None, "Alex", false)?;
    let confirmed: i64 = c.db.query_row(
        "SELECT count(*) FROM faces WHERE assignment='user' AND person_id IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(confirmed, 9);
    // Abgewählte Gesichter werden für diese Person abgelehnt, nicht übernommen.
    let rejected: i64 =
        c.db.query_row("SELECT count(*) FROM face_rejections", [], |r| r.get(0))?;
    assert_eq!(rejected, 11);
    Ok(())
}

/// Drei Vorschläge für Person 1 („Anna“) im aktiven Modellraum `test`.
fn suggested_for_anna(c: &Collection, count: i64) -> anyhow::Result<()> {
    seed(c, count)?;
    c.db.execute_batch(
        "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key','\"test\"');
         INSERT INTO persons(id) VALUES(1);
         INSERT INTO person_attrs VALUES(1,'name','Anna');
         UPDATE faces SET person_id=1,assignment='suggested',confidence=0.5;",
    )?;
    Ok(())
}
fn state(c: &Collection, id: i64) -> anyhow::Result<(Option<i64>, Option<String>)> {
    Ok(c.db.query_row(
        "SELECT person_id,assignment FROM faces WHERE id=?",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}
fn count(c: &Collection, sql: &str) -> anyhow::Result<i64> {
    Ok(c.db.query_row(sql, [], |r| r.get(0))?)
}

#[test]
fn rejecting_a_suggestion_releases_the_face_at_once_and_undo_restores_it() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    suggested_for_anna(&c, 3)?;
    let undo = faces::name_group(
        &mut c,
        &[1, 2, 3],
        &BTreeSet::from([1, 2]),
        Some(1),
        "",
        false,
    )?;
    assert_eq!(
        undo.summary(),
        faces::DecisionSummary {
            confirmed: 2,
            rejected: 1,
            ignored: 0
        }
    );
    assert_eq!(state(&c, 1)?, (Some(1), Some("user".into())));
    assert_eq!(
        state(&c, 3)?,
        (None, None),
        "abgelehnt heißt sofort unbenannt"
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM face_rejections WHERE face_id=3 AND person_id=1"
        )?,
        1,
        "die Ablehnung bleibt gespeichert"
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM face_group_members WHERE face_id=3"
        )?,
        1,
        "das Gesicht steht ohne Hintergrundlauf in einer Gruppe unter Benennen"
    );
    let open: Vec<Vec<i64>> = faces::page(&c, "unknown", 0)?
        .groups
        .iter()
        .map(|group| group.iter().map(|face| face.id).collect())
        .collect();
    assert_eq!(open, vec![vec![3]]);
    faces::undo(&mut c, &undo)?;
    assert_eq!(state(&c, 3)?, (Some(1), Some("suggested".into())));
    assert_eq!(
        c.db.query_row("SELECT confidence FROM faces WHERE id=3", [], |r| r
            .get::<_, f64>(0))?,
        0.5
    );
    assert_eq!(count(&c, "SELECT count(*) FROM face_rejections")?, 0);
    Ok(())
}

#[test]
fn deselecting_a_confirmed_face_removes_the_confirmation() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    suggested_for_anna(&c, 2)?;
    c.db.execute_batch("UPDATE faces SET assignment='user',confidence=1")?;
    faces::name_group(&mut c, &[1, 2], &BTreeSet::from([1]), Some(1), "", false)?;
    assert_eq!(state(&c, 1)?, (Some(1), Some("user".into())));
    assert_eq!(state(&c, 2)?, (None, None));
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM face_reference_dirty WHERE person_id=1"
        )?,
        1,
        "die Referenzen der Person werden neu gewählt"
    );
    assert_eq!(count(&c, "SELECT count(*) FROM face_maintenance")?, 1);
    Ok(())
}

#[test]
fn none_of_them_needs_an_existing_person() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    suggested_for_anna(&c, 2)?;
    assert!(
        faces::name_group(&mut c, &[1, 2], &BTreeSet::new(), None, "Neu", false).is_err(),
        "ohne Auswahl entsteht keine Person ohne Gesicht"
    );
    assert_eq!(count(&c, "SELECT count(*) FROM persons")?, 1);
    let undo = faces::name_group(&mut c, &[1, 2], &BTreeSet::new(), Some(1), "", false)?;
    assert_eq!(undo.summary().rejected, 2);
    assert_eq!(state(&c, 1)?, (None, None));
    assert_eq!(state(&c, 2)?, (None, None));
    assert_eq!(
        count(&c, "SELECT count(*) FROM face_rejections WHERE person_id=1")?,
        2
    );
    Ok(())
}

#[test]
fn a_later_rematch_never_suggests_the_rejected_person_again() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    suggested_for_anna(&c, 4)?;
    // 1 und 2 bestätigt, 3 abgelehnt, 4 bleibt ein offener Vorschlag als Gegenprobe.
    faces::name_group(
        &mut c,
        &[1, 2, 3],
        &BTreeSet::from([1, 2]),
        Some(1),
        "",
        false,
    )?;
    faces::rematch(&mut c, "test", &Control::default())?;
    assert_eq!(state(&c, 3)?, (None, None));
    assert_eq!(
        state(&c, 4)?.0,
        Some(1),
        "nicht abgelehnte Gesichter erhalten weiter Vorschläge"
    );
    Ok(())
}

#[test]
fn page_lists_the_rejected_people_per_face() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    suggested_for_anna(&c, 3)?;
    faces::name_group(
        &mut c,
        &[1, 2, 3],
        &BTreeSet::from([1, 2]),
        Some(1),
        "",
        false,
    )?;
    let page = faces::page(&c, "unknown", 0)?;
    assert_eq!(page.rejections.get(&3), Some(&vec![1]));
    assert_eq!(
        page.rejections.len(),
        1,
        "nur abgelehnte Gesichter haben einen Eintrag"
    );
    let confirmed = faces::page(&c, "persons", 0)?;
    assert!(confirmed.rejections.is_empty());
    Ok(())
}

/// Anna bei 0°, Cara bei 30°, Ben bei 120°; je ein bestätigtes Gesicht als Referenz.
fn three_people(c: &Collection) -> anyhow::Result<()> {
    c.db.execute_batch(
        "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key','\"test\"');
         INSERT INTO persons(id) VALUES(1),(2),(3);
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben'),(3,'name','Cara');",
    )?;
    for (id, degrees) in [(1, 0.0), (2, 120.0), (3, 30.0)] {
        polar(c, id, id, degrees)?;
        c.db.execute(
            "UPDATE faces SET person_id=?,assignment='user' WHERE id=?",
            [id, id],
        )?;
    }
    Ok(())
}

#[test]
fn rematch_suggests_only_above_the_threshold_and_clear_of_the_runner_up() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    three_people(&c)?;
    polar(&c, 10, 10, 5.0)?; // Anna 0,996, Cara 0,906
    polar(&c, 11, 11, 13.0)?; // Anna 0,974, Cara 0,956: Abstand zu klein
    polar(&c, 12, 12, 70.0)?; // Ben 0,643, Cara 0,766: Abstand 0,12
    polar(&c, 13, 13, 170.0)?; // Ben 0,643, übrige negativ: unter „sicher“
    polar(&c, 14, 14, 175.0)?; // Ben 0,574: unter der Schwelle
    faces::rematch(&mut c, "test", &Control::default())?;
    assert_eq!(state(&c, 10)?, (Some(1), Some("auto".into())));
    assert_eq!(
        state(&c, 11)?,
        (None, None),
        "zwei fast gleich ähnliche Personen ergeben keinen Vorschlag"
    );
    assert_eq!(state(&c, 12)?, (Some(3), Some("auto".into())));
    assert_eq!(state(&c, 13)?, (Some(2), Some("suggested".into())));
    assert_eq!(
        state(&c, 14)?,
        (None, None),
        "0,57 lag über der alten Grenze 0,363 und bleibt jetzt unbenannt"
    );
    Ok(())
}

#[test]
fn rematch_gives_a_person_to_one_face_per_photo_only() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    three_people(&c)?;
    polar(&c, 10, 10, 4.0)?;
    polar(&c, 11, 10, 2.0)?;
    faces::rematch(&mut c, "test", &Control::default())?;
    assert_eq!(
        state(&c, 11)?.0,
        Some(1),
        "das ähnlichere Gesicht erhält Anna"
    );
    assert_eq!(
        state(&c, 10)?,
        (None, None),
        "das zweite Gesicht bekommt keinen Ersatzvorschlag"
    );
    Ok(())
}

#[test]
fn rematch_writes_only_changed_assignments() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    three_people(&c)?;
    polar(&c, 10, 10, 5.0)?; // Anna
    polar(&c, 12, 12, 70.0)?; // Cara
    polar(&c, 15, 15, 175.0)?; // unter der Schwelle
    faces::rematch(&mut c, "test", &Control::default())?;
    assert_eq!(state(&c, 10)?, (Some(1), Some("auto".into())));
    // Unveränderte Zuordnung: Zeitpunkt als Merkmal, ob die Zeile neu geschrieben wird.
    // Veraltete Zuordnungen von 12 und 15 müssen dagegen ersetzt bzw. entfernt werden.
    c.db.execute_batch(
        "UPDATE faces SET assigned_at='unverändert' WHERE id=10;
         UPDATE faces SET person_id=2,assignment='suggested',confidence=0.61,assigned_at='alt' WHERE id IN (12,15);
         DELETE FROM face_group_dirty;",
    )?;
    let before: f64 =
        c.db.query_row("SELECT confidence FROM faces WHERE id=10", [], |r| r.get(0))?;
    c.db.execute(
        "INSERT INTO face_maintenance VALUES('test',0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0",
        [],
    )?;
    faces::rematch(&mut c, "test", &Control::default())?;
    let (at, confidence): (String, f64) = c.db.query_row(
        "SELECT assigned_at,confidence FROM faces WHERE id=10",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(at, "unverändert", "gleiche Zuordnung bleibt unangetastet");
    assert_eq!(confidence.to_bits(), before.to_bits());
    assert_eq!(state(&c, 12)?, (Some(3), Some("auto".into())));
    assert_eq!(state(&c, 15)?, (None, None));
    assert_eq!(
        count(&c, "SELECT count(*) FROM face_group_dirty WHERE face_id=10")?,
        0,
        "keine Gruppenarbeit für unveränderte Gesichter"
    );
    Ok(())
}

#[test]
fn a_change_during_the_rematch_keeps_the_restart() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    three_people(&c)?;
    polar(&c, 10, 10, 5.0)?;
    polar(&c, 12, 12, 70.0)?;
    c.db.execute(
        "INSERT OR REPLACE INTO face_maintenance VALUES('test',0,7)",
        [],
    )?;
    // Simuliert eine Bestätigung, während der Lauf schreibt: wie der Trigger
    // `user_face_maintenance` Cursor zurücksetzen und Generation erhöhen.
    c.db.execute_batch(
        "CREATE TEMP TRIGGER change_during_run AFTER UPDATE OF person_id ON faces WHEN new.id=10 BEGIN
           UPDATE face_maintenance SET after_photo=0,generation=generation+1;
         END;",
    )?;
    faces::rematch(&mut c, "test", &Control::default())?;
    assert_eq!(state(&c, 10)?, (Some(1), Some("auto".into())));
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM face_maintenance WHERE after_photo=0 AND generation=8"
        )?,
        1,
        "die Änderung während des Laufs löst einen neuen Lauf aus"
    );
    c.db.execute_batch("DROP TRIGGER change_during_run")?;
    faces::rematch(&mut c, "test", &Control::default())?;
    assert_eq!(count(&c, "SELECT count(*) FROM face_maintenance")?, 0);
    Ok(())
}

#[test]
fn the_review_threshold_is_stored_per_collection_and_restarts_the_rematch() -> anyhow::Result<()> {
    use foto_importer::core::recognition;
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    three_people(&c)?;
    polar(&c, 13, 13, 170.0)?; // Ben 0,643
    faces::rematch(&mut c, "test", &Control::default())?;
    assert_eq!(state(&c, 13)?.0, Some(2));
    assert!(recognition::set_review_threshold(&mut c, 0.9).is_err());
    assert!(recognition::set_review_threshold(&mut c, f32::NAN).is_err());
    assert_eq!(count(&c, "SELECT count(*) FROM face_maintenance")?, 0);
    recognition::set_review_threshold(&mut c, 0.7)?;
    assert_eq!(recognition::Limits::load(&c)?.review, 0.7);
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM face_maintenance WHERE embedder_id='test' AND after_photo=0"
        )?,
        1,
        "der Abgleich beginnt von vorn"
    );
    faces::rematch(&mut c, "test", &Control::default())?;
    assert_eq!(state(&c, 13)?, (None, None));
    assert_eq!(
        state(&c, 2)?,
        (Some(2), Some("user".into())),
        "bestätigte Namen bleiben"
    );
    Ok(())
}

#[test]
fn a_group_suggestion_uses_more_than_the_first_three_faces() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    c.db.execute_batch(
        "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key','\"test\"');
         INSERT INTO persons(id) VALUES(1),(2);
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben');",
    )?;
    for (id, degrees) in [(1, 0.0), (2, 90.0)] {
        polar(&c, id, id, degrees)?;
        c.db.execute(
            "UPDATE faces SET person_id=?,assignment='user' WHERE id=?",
            [id, id],
        )?;
    }
    // Die ersten drei Gesichter ähneln Anna, die übrigen fünf Ben.
    let group: Vec<i64> = (10..18).collect();
    for (index, id) in group.iter().enumerate() {
        polar(&c, *id, *id, if index < 3 { 30.0 } else { 70.0 })?;
    }
    let found = foto_importer::core::recognition::group_suggestions(&c, &group, "test")?;
    assert_eq!(found.items[0].0, 2, "Ben überwiegt in der ganzen Gruppe");
    assert_eq!(found.items[1].0, 1);
    Ok(())
}

/// Kleiner Zufallsgenerator für reproduzierbare synthetische Vektoren.
struct Xorshift(u64);
impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn signed(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    }
}
fn unit(v: Vec<f32>) -> Vec<f32> {
    inference::normalize(&v).expect("Vektor")
}
/// Gesicht, Person, Art und Wert (Bits) einer Zuordnung.
type Assignment = (i64, Option<i64>, Option<String>, Option<u64>);
/// Alle Zuordnungen bitgenau (Person, Art, Wert).
fn assignments(c: &Collection) -> anyhow::Result<Vec<Assignment>> {
    Ok(c.db
        .prepare("SELECT id,person_id,assignment,confidence FROM faces ORDER BY id")?
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get::<_, Option<f64>>(3)?.map(f64::to_bits),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?)
}

/// Der inkrementelle Abgleich (Zwischenspeicher über viele Änderungen hinweg) ergibt nach
/// jeder Änderung bitgenau dasselbe wie ein vollständiger Lauf auf einer Kopie desselben
/// Stands: Bestätigungen, Ablehnungen, entfernte Bestätigungen, Ausschlüsse, Referenzauswahl,
/// neue und ersetzte Vektoren, Geburtsjahre, Umbenennungen (Gleichstand nach Name und ID),
/// Ignorieren, historische Gesichter und Qualität.
#[test]
fn incremental_rematch_equals_the_full_rematch() -> anyhow::Result<()> {
    use foto_importer::core::recognition;
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    let mut rng = Xorshift(0x9E37_79B9_7F4A_7C15);
    const DIM: usize = 8;
    let centers: Vec<Vec<f32>> = (0..6)
        .map(|_| unit((0..DIM).map(|_| rng.signed()).collect()))
        .collect();
    c.db.execute_batch(
        "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key','\"test\"');
         INSERT OR REPLACE INTO face_settings VALUES('rust.thresholds.test.review','0.55');",
    )?;
    // Personen 5 und 6 heißen gleich und teilen sich Referenzvektoren: exakter Gleichstand.
    for (id, name) in [
        (1, "Anna"),
        (2, "Ben"),
        (3, "Cara"),
        (4, "Dora"),
        (5, "Zoe"),
        (6, "Zoe"),
    ] {
        c.db.execute("INSERT INTO persons(id) VALUES(?)", [id])?;
        c.db.execute(
            "INSERT INTO person_attrs VALUES(?,'name',?)",
            params![id, name],
        )?;
    }
    let mut next_face = 1i64;
    let mut add_face = |c: &Collection,
                        rng: &mut Xorshift,
                        photo: i64,
                        vector: &[f32]|
     -> anyhow::Result<i64> {
        let id = next_face;
        next_face += 1;
        c.db.execute(
            "INSERT OR IGNORE INTO photos(id,status) VALUES(?,'present')",
            [photo],
        )?;
        let quality = if rng.below(10) == 0 { 0.3 } else { 0.9 };
        let year = 1990 + rng.below(30) as i64;
        c.db.execute("INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality,captured_at) VALUES(?,?,?,'det',0,0,1,1,?,?)", params![id, photo, format!("{photo}.jpg"), quality, format!("{year}-05-01")])?;
        c.db.execute(
            "INSERT INTO face_embeddings VALUES(?,'test',?,?)",
            params![id, vector.len() as i64, faces::encode(vector)],
        )?;
        Ok(id)
    };
    let near = |rng: &mut Xorshift, person: usize, spread: f32| -> Vec<f32> {
        unit(
            centers[person]
                .iter()
                .map(|v| v + spread * rng.signed())
                .collect(),
        )
    };
    let shared = near(&mut rng, 4, 0.2);
    let mut photo = 1i64;
    let mut all = Vec::new();
    // Referenzen: je Person drei bis fünf bestätigte Gesichter.
    for person in 0..6usize {
        for k in 0..(3 + person % 3) {
            let vector = if person >= 4 && k == 0 {
                shared.clone()
            } else {
                near(&mut rng, person.min(4), 0.25)
            };
            let id = add_face(&c, &mut rng, photo, &vector)?;
            c.db.execute(
                "UPDATE faces SET person_id=?,assignment='user',quality=0.9 WHERE id=?",
                params![person as i64 + 1, id],
            )?;
            photo += 1;
            all.push(id);
        }
    }
    // Offene Gesichter, meist zu zweit oder dritt im Foto, darunter Kopien des geteilten Vektors.
    for _ in 0..60 {
        for _ in 0..1 + rng.below(3) {
            let vector = match rng.below(8) {
                0 => shared.clone(),
                1 => unit((0..DIM).map(|_| rng.signed()).collect()),
                _ => {
                    let person = rng.below(6) as usize;
                    near(&mut rng, person, 0.3)
                }
            };
            all.push(add_face(&c, &mut rng, photo, &vector)?);
        }
        photo += 1;
    }
    c.db.execute("UPDATE faces SET retained=1 WHERE id=?", [all[40]])?;
    c.db.execute("UPDATE faces SET ignored=1 WHERE id=?", [all[41]])?;
    c.db.execute(
        "UPDATE face_embeddings SET vec=NULL WHERE face_id=?",
        [all[42]],
    )?;
    let control = Control::default();
    let mut cache = recognition::MatchCache::default();
    let copy = tempfile::tempdir()?;
    let mut check = |c: &mut Collection, step: &str| -> anyhow::Result<usize> {
        let target = copy.path().join(".myfotomanager");
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
        c.db.execute("VACUUM INTO ?", [target.to_string_lossy()])?;
        let mut full = Collection::open(copy.path(), true)?;
        recognition::rematch_full(&mut full, "test", &control)?;
        recognition::rematch_with(c, "test", &control, &mut cache)?;
        let (incremental, expected) = (assignments(c)?, assignments(&full)?);
        assert_eq!(incremental, expected, "Schritt {step}");
        drop(full);
        Ok(incremental
            .iter()
            .filter(|r| matches!(r.2.as_deref(), Some("auto" | "suggested")))
            .count())
    };
    let first = check(&mut c, "Start")?;
    assert!(first > 10, "genug Vorschläge zum Vergleichen: {first}");
    let mut seen_suggestions = BTreeSet::new();
    for step in 0..60 {
        let face = all[rng.below(all.len() as u64) as usize];
        let person = 1 + rng.below(6) as i64;
        let label = match rng.below(13) {
            0 | 1 => {
                c.db.execute(
                    "UPDATE faces SET person_id=?,assignment='user',confidence=1,ignored=0 WHERE id=? AND retained=0",
                    params![person, face],
                )?;
                "bestätigt"
            }
            2 => {
                c.db.execute(
                    "UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE id=? AND assignment='user'",
                    [face],
                )?;
                "Bestätigung entfernt"
            }
            3 => {
                c.db.execute(
                    "INSERT OR IGNORE INTO face_rejections SELECT id,coalesce(person_id,?) FROM faces WHERE id=?",
                    params![person, face],
                )?;
                c.db.execute("UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE id=? AND assignment IN ('auto','suggested')", [face])?;
                "abgelehnt"
            }
            4 => {
                c.db.execute(
                    "INSERT OR IGNORE INTO training_exclusions VALUES(?,'test')",
                    [face],
                )?;
                "Referenz ausgeschlossen"
            }
            5 => {
                // Referenzauswahl: nur ein Teil der Bestätigten zählt, ein Ausreißer gar nicht.
                c.db.execute("DELETE FROM training_set WHERE person_id=?", [person])?;
                c.db.execute("INSERT INTO training_set SELECT person_id,id,'test',CASE WHEN id%3=0 THEN 'val' ELSE 'ref' END,'',NULL,0 FROM faces WHERE person_id=? AND assignment='user'", [person])?;
                c.db.execute("INSERT OR IGNORE INTO face_review SELECT max(id),'test','outlier' FROM faces WHERE person_id=? AND assignment='user'", [person])?;
                "Referenzen ausgewählt"
            }
            6 => {
                let vector = near(&mut rng, (person - 1).min(4) as usize, 0.3);
                let target = if rng.below(2) == 0 {
                    photo
                } else {
                    1 + rng.below(photo as u64) as i64
                };
                photo += 1;
                all.push(add_face(&c, &mut rng, target, &vector)?);
                "neues Gesicht"
            }
            7 => {
                let vector = near(&mut rng, (person - 1).min(4) as usize, 0.3);
                c.db.execute(
                    "INSERT OR REPLACE INTO face_embeddings VALUES(?,'test',?,?)",
                    params![face, DIM as i64, faces::encode(&vector)],
                )?;
                "Vektor ersetzt"
            }
            8 => {
                c.db.execute(
                    "INSERT OR REPLACE INTO person_attrs VALUES(?,'birth_year',?)",
                    params![person, (2000 + rng.below(15)).to_string()],
                )?;
                "Geburtsjahr"
            }
            9 => {
                let names = ["Anna", "Zoe", "Ben", "Aaron"];
                c.db.execute(
                    "UPDATE person_attrs SET value=? WHERE person_id=? AND key='name'",
                    params![names[rng.below(4) as usize], person],
                )?;
                "umbenannt"
            }
            10 => {
                c.db.execute("UPDATE faces SET ignored=1-ignored WHERE id=?", [face])?;
                "ignoriert"
            }
            11 => {
                c.db.execute("UPDATE faces SET quality=CASE WHEN quality>=0.5 THEN 0.2 ELSE 0.8 END WHERE id=?", [face])?;
                "Qualität"
            }
            _ => {
                c.db.execute("UPDATE faces SET retained=1-retained WHERE id=?", [face])?;
                "historisch"
            }
        };
        let suggestions = check(&mut c, &format!("{step} {label}"))?;
        seen_suggestions.insert(suggestions);
    }
    assert!(
        seen_suggestions.len() > 3,
        "die Änderungen wirkten sich aus"
    );
    assert!(!cache.is_empty());
    Ok(())
}

/// Ein ersetzter Vektor gleicher Gesichts-ID wird erkannt (neue `rowid`), auch wenn er die
/// Zeile mit der höchsten `rowid` ersetzt.
#[test]
fn incremental_rematch_notices_a_replaced_vector() -> anyhow::Result<()> {
    use foto_importer::core::recognition;
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    three_people(&c)?;
    polar(&c, 10, 10, 5.0)?; // Anna
    let mut cache = recognition::MatchCache::default();
    let control = Control::default();
    recognition::rematch_with(&mut c, "test", &control, &mut cache)?;
    assert_eq!(state(&c, 10)?, (Some(1), Some("auto".into())));
    let r = 70f32.to_radians();
    c.db.execute(
        "INSERT OR REPLACE INTO face_embeddings VALUES(10,'test',2,?)",
        [faces::encode(&[r.cos(), r.sin()])],
    )?;
    recognition::rematch_with(&mut c, "test", &control, &mut cache)?;
    assert_eq!(state(&c, 10)?, (Some(3), Some("auto".into())), "jetzt Cara");
    Ok(())
}
