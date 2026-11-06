use foto_importer::core::{collection::Collection, faces, maintenance};
use rusqlite::params;
#[test]
fn completed_matching_status_belongs_to_its_model_and_cancellation_keeps_pending()
-> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut collection = Collection::open(root.path(), true)?;
    let select_model = |collection: &Collection, model: &str| -> anyhow::Result<()> {
        collection.db.execute(
            "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key',?)",
            [serde_json::to_string(model)?],
        )?;
        Ok(())
    };
    select_model(&collection, "first")?;
    faces::rematch(&mut collection, "first", &Default::default())?;
    assert!(!faces::status(&collection)?.applied.is_empty());
    select_model(&collection, "second")?;
    collection
        .db
        .execute("INSERT INTO face_maintenance VALUES('second',0,1)", [])?;
    let control = foto_importer::core::jobs::Control::default();
    control.stop();
    assert!(faces::rematch(&mut collection, "second", &control).is_err());
    let status = faces::status(&collection)?;
    assert!(status.applied.is_empty());
    assert_eq!(status.pending, 1);
    select_model(&collection, "first")?;
    assert!(!faces::status(&collection)?.applied.is_empty());
    Ok(())
}

#[test]
fn biometric_erasure_scrubs_backup_but_preserves_inventory() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("original.jpg"), b"photo")?;
    let c = Collection::open(temp.path(), true)?;
    c.db.execute_batch("INSERT INTO photos(id,status) VALUES(1,'present');INSERT INTO persons(id) VALUES(1);INSERT INTO faces(id,photo_id,source_relpath,detector_id,person_id,assignment) VALUES(1,1,'original.jpg','det',1,'user');")?;
    c.db.execute(
        "INSERT INTO face_embeddings VALUES(1,'test',2,?)",
        [faces::encode(&[1.0, 0.0])],
    )?;
    c.backup()?;
    maintenance::delete_biometrics(&c)?;
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM photos", [], |r| r.get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM faces", [], |r| r.get::<_, i64>(0))?,
        0
    );
    let backup = rusqlite::Connection::open(temp.path().join(".myfotomanager.backup"))?;
    assert_eq!(
        backup.query_row("SELECT count(*) FROM face_embeddings", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    assert_eq!(std::fs::read(temp.path().join("original.jpg"))?, b"photo");
    Ok(())
}
#[test]
fn merging_preserves_person_identity_redirect_and_labels() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    c.db.execute_batch("INSERT INTO persons(id) VALUES(1),(2); INSERT INTO person_attrs VALUES(1,'name','Alex'),(2,'name','Alexandra');INSERT INTO photos(id,status) VALUES(1,'present');INSERT INTO faces(id,photo_id,source_relpath,detector_id,person_id,assignment) VALUES(1,1,'x.jpg','det',1,'user');")?;
    maintenance::merge_people(&mut c, 1, 2)?;
    assert_eq!(
        c.db.query_row("SELECT merged_into FROM persons WHERE id=1", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    assert_eq!(
        c.db.query_row("SELECT person_id FROM faces WHERE id=1", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    Ok(())
}
#[test]
fn derived_family_relations_follow_parent_edges() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    for id in 1..=4 {
        c.db.execute("INSERT INTO persons(id) VALUES(?)", [id])?;
    }
    for (a, b) in [(1, 2), (1, 3), (2, 4)] {
        c.db.execute("INSERT INTO relations VALUES(?,1,?)", params![a, b])?;
    }
    let r = maintenance::relations(&c, 4)?;
    assert_eq!(r["grosseltern"], vec![1]);
    assert_eq!(r["onkel_tanten"], vec![3]);
    Ok(())
}
#[test]
fn automation_flags_are_collection_local_and_opt_in() -> anyhow::Result<()> {
    let a = tempfile::tempdir()?;
    let b = tempfile::tempdir()?;
    let mut first = Collection::open(a.path(), true)?;
    let second = Collection::open(b.path(), true)?;
    assert!(!faces::status(&first)?.auto_import);
    assert!(!faces::status(&first)?.nightly);
    faces::configure_automatic(&mut first, true, true)?;
    assert!(faces::status(&first)?.auto_import);
    assert!(faces::status(&first)?.nightly);
    assert!(!faces::status(&second)?.auto_import);
    Ok(())
}
