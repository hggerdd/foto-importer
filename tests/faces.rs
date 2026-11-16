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
fn review_groups_are_size_sorted_globally_and_do_not_mix_people() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    seed(&c, 51)?;
    c.db.execute_batch("INSERT INTO persons(id) VALUES(1),(2),(3); UPDATE faces SET person_id=CASE WHEN id<=1 THEN 1 WHEN id<=21 THEN 2 ELSE 3 END,assignment='suggested';")?;
    let page = faces::page(&c, "review", 0)?;
    assert_eq!(
        page.groups.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![28, 20, 2, 1]
    );
    assert_eq!(page.total_groups, 4, "Seitenzahl kennt alle Gruppen");
    for group in &page.groups {
        assert!(group.iter().all(|f| f.person == group[0].person));
    }
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
