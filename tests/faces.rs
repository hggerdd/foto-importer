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
