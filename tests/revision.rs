//! Datenrevision (`core::revision`): jeder Bereich zählt genau die Änderungen seiner Tabellen,
//! Schema 6 wird ohne Datenverlust auf 7 gebracht, und die Caches folgen ihr.
use foto_importer::core::{
    collection::Collection,
    relationship_graph,
    revision::{self, Domain, Revisions, TABLES},
    start_view,
};
use rusqlite::Connection;

fn now(c: &Collection) -> anyhow::Result<Revisions> {
    Ok(revision::read(&c.db)?.expect("Revision"))
}

/// Führt `sql` aus und prüft, dass genau `domain` (oder keiner) gestiegen ist.
fn bumps(c: &Collection, sql: &str, domain: Option<Domain>) -> anyhow::Result<()> {
    let before = now(c)?;
    c.db.execute_batch(sql)?;
    let after = now(c)?;
    for d in Domain::ALL {
        let grew = after.get(d) > before.get(d);
        assert_eq!(grew, Some(d) == domain, "{sql}: Bereich {}", d.name());
        assert!(after.get(d) >= before.get(d), "{sql}");
    }
    assert_eq!(after.identity, before.identity);
    Ok(())
}

fn collection() -> anyhow::Result<(tempfile::TempDir, Collection)> {
    let root = tempfile::tempdir()?;
    let c = Collection::open(root.path(), true)?;
    c.db.execute_batch(
        "INSERT INTO photos(id,status) VALUES(1,'present');
         INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status)
            VALUES(1,'a/1.jpg','a/1.jpg','a','1.jpg','1','jpg',1,0,1,'present');
         INSERT INTO persons(id) VALUES(1),(2);",
    )?;
    Ok((root, c))
}

#[test]
fn each_domain_counts_insert_update_and_delete_of_its_tables_only() -> anyhow::Result<()> {
    let (_root, c) = collection()?;
    let cases: &[(&str, [&str; 3])] = &[
        (
            "files",
            [
                "INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(2,'a/2.jpg','a/2.jpg','a','2.jpg','2','jpg',1,0,1,'present')",
                "UPDATE files SET size=5 WHERE id=2",
                "DELETE FROM files WHERE id=2",
            ],
        ),
        (
            "meta",
            [
                "INSERT INTO meta(file_id,model) VALUES(1,'K')",
                "UPDATE meta SET model='L' WHERE file_id=1",
                "DELETE FROM meta WHERE file_id=1",
            ],
        ),
        (
            "faces",
            [
                "INSERT INTO faces(id,photo_id,source_relpath,detector_id,person_id,assignment) VALUES(7,1,'a/1.jpg','d',1,'user')",
                "UPDATE faces SET person_id=2 WHERE id=7",
                "DELETE FROM faces WHERE id=7",
            ],
        ),
        (
            "face_rejections",
            [
                "INSERT INTO face_rejections VALUES(7,1)",
                "UPDATE face_rejections SET person_id=2",
                "DELETE FROM face_rejections",
            ],
        ),
        (
            "training_set",
            [
                "INSERT INTO training_set VALUES(1,7,'e','ref','x','pin',1)",
                "UPDATE training_set SET pinned=0",
                "DELETE FROM training_set",
            ],
        ),
        (
            "face_settings",
            [
                "INSERT INTO face_settings VALUES('k','1')",
                "UPDATE face_settings SET value='2' WHERE key='k'",
                "DELETE FROM face_settings WHERE key='k'",
            ],
        ),
        (
            "persons",
            [
                "INSERT INTO persons(id) VALUES(3)",
                "UPDATE persons SET merged_into=1 WHERE id=3",
                "DELETE FROM persons WHERE id=3",
            ],
        ),
        (
            "person_attrs",
            [
                "INSERT INTO person_attrs VALUES(1,'name','Anna')",
                "UPDATE person_attrs SET value='Ann' WHERE person_id=1",
                "DELETE FROM person_attrs WHERE person_id=1",
            ],
        ),
        (
            "relations",
            [
                "INSERT INTO relations VALUES(1,1,2)",
                "UPDATE relations SET to_person=1",
                "DELETE FROM relations",
            ],
        ),
        (
            "relation_types",
            [
                "INSERT INTO relation_types(id,name) VALUES(999,'Test')",
                "UPDATE relation_types SET name='Probe' WHERE id=999",
                "DELETE FROM relation_types WHERE id=999",
            ],
        ),
    ];
    assert_eq!(cases.len(), TABLES.len(), "jede Tabelle geprüft");
    for (table, statements) in cases {
        let domain = TABLES
            .iter()
            .find(|(name, _)| name == table)
            .map(|(_, d)| *d)
            .expect("Tabelle in TABLES");
        for sql in statements {
            bumps(&c, sql, Some(domain))?;
        }
    }
    // Erkennung ohne Person, Merkmale, Gruppen und Aufträge zählen nicht.
    for sql in [
        "INSERT INTO faces(id,photo_id,source_relpath,detector_id) VALUES(8,1,'a/1.jpg','d')",
        "UPDATE faces SET quality=0.5,confidence=0.2,bbox_x1=0.1 WHERE id=8",
        "INSERT INTO face_embeddings VALUES(8,'e',2,x'0000')",
        "INSERT INTO face_jobs VALUES(1,'detect','d','done',NULL,NULL,'f')",
        "INSERT INTO face_review VALUES(8,'e','x')",
        "DELETE FROM faces WHERE id=8",
        // Nicht projizierte Spalten des Bestands.
        "UPDATE files SET full_hash='h',last_seen_scan=3 WHERE id=1",
        "INSERT INTO settings VALUES('x','y')",
    ] {
        bumps(&c, sql, None)?;
    }
    // Zuordnung einer Erkennung zählt; Ändern ohne Wirkung nicht.
    c.db.execute_batch(
        "INSERT INTO faces(id,photo_id,source_relpath,detector_id) VALUES(9,1,'a/1.jpg','d')",
    )?;
    bumps(
        &c,
        "UPDATE faces SET person_id=1,assignment='auto' WHERE id=9",
        Some(Domain::Faces),
    )?;
    bumps(&c, "UPDATE faces SET person_id=1 WHERE id=9", None)?;
    bumps(
        &c,
        "UPDATE faces SET ignored=1 WHERE id=9",
        Some(Domain::Faces),
    )?;
    Ok(())
}

#[test]
fn read_needs_one_query_and_keys_pick_domains() -> anyhow::Result<()> {
    let (_root, c) = collection()?;
    let r = now(&c)?;
    assert!(!r.identity.is_empty());
    let key = r.key(&[Domain::People]);
    c.db.execute_batch("UPDATE files SET size=9 WHERE id=1")?;
    assert_eq!(now(&c)?.key(&[Domain::People]), key);
    c.db.execute_batch("INSERT INTO person_attrs VALUES(2,'name','Ben')")?;
    let newer = now(&c)?.key(&[Domain::People]);
    assert_ne!(newer, key);
    assert!(key.older_than(&newer));
    Ok(())
}

#[test]
fn consumers_follow_only_their_domains() -> anyhow::Result<()> {
    let (_root, c) = collection()?;
    let stamp = start_view::stamp(&c)?.expect("Stand");
    let graph = relationship_graph::signature(&c)?;
    // Personen ändern den Graphen, nicht die Startansicht.
    c.db.execute_batch("INSERT INTO person_attrs VALUES(1,'name','Anna')")?;
    assert_eq!(start_view::stamp(&c)?.expect("Stand"), stamp);
    let named = relationship_graph::signature(&c)?;
    assert_ne!(named, graph);
    // Neue Erkennungen ohne Person ändern keinen der beiden.
    c.db.execute_batch(
        "INSERT INTO faces(id,photo_id,source_relpath,detector_id) VALUES(5,1,'a/1.jpg','d');
         INSERT INTO face_embeddings VALUES(5,'e',2,x'0000');",
    )?;
    assert_eq!(relationship_graph::signature(&c)?, named);
    assert_eq!(start_view::stamp(&c)?.expect("Stand"), stamp);
    // Eine Bestätigung ändert den Graphen.
    c.db.execute_batch("UPDATE faces SET person_id=1,assignment='user' WHERE id=5")?;
    let confirmed = relationship_graph::signature(&c)?;
    assert_ne!(confirmed, named);
    // Der Bestand ändert beide.
    c.db.execute_batch("UPDATE files SET name='x.jpg' WHERE id=1")?;
    assert_ne!(start_view::stamp(&c)?.expect("Stand"), stamp);
    assert_ne!(relationship_graph::signature(&c)?, confirmed);
    // Gleicher Stand: gleicher Fingerabdruck, der gespeicherte Graph gilt.
    let (signature, network) = relationship_graph::refresh(&c, None)?;
    assert!(network.is_some());
    let (again, unchanged) = relationship_graph::refresh(&c, Some(&signature))?;
    assert_eq!(again, signature);
    assert!(unchanged.is_none());
    Ok(())
}

/// Baut aus einer frischen Sammlung den Stand von Schema 6 nach: `projection_revision`
/// statt `data_revision`, Projektionstrigger mit dem alten Zähler, keine Revisionstrigger.
fn schema_six(root: &std::path::Path) -> anyhow::Result<()> {
    let c = Collection::open(root, true)?;
    c.db.execute_batch(
        "DELETE FROM collection; INSERT INTO collection VALUES('sechs','2025','rust-0.1');
         INSERT INTO photos(id,identity_key,status) VALUES(42,'cam:7','present');
         INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status)
            VALUES(91,'a/b.jpg','a/b.jpg','a','b.jpg','b','jpg',8,1,42,'present');
         INSERT INTO persons(id) VALUES(77); INSERT INTO person_attrs VALUES(77,'name','Alt');
         INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,person_id,assignment)
            VALUES(99,42,91,'a/b.jpg','d',77,'user');
         INSERT INTO face_rejections VALUES(99,5);
         INSERT INTO relations VALUES(77,1,77);",
    )?;
    drop(c);
    let db = Connection::open(root.join(".myfotomanager"))?;
    let triggers: Vec<(String, String)> = db
        .prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    db.execute_batch("BEGIN; CREATE TABLE projection_revision(revision INTEGER NOT NULL); INSERT INTO projection_revision VALUES(12);")?;
    for (name, sql) in triggers {
        db.execute_batch(&format!("DROP TRIGGER {name}"))?;
        if name.starts_with("revision_") {
            continue;
        }
        db.execute_batch(&sql.replace(
            "UPDATE data_revision SET revision=revision+1 WHERE domain='files'",
            "UPDATE projection_revision SET revision=revision+1",
        ))?;
    }
    db.execute_batch("DROP TABLE data_revision; PRAGMA user_version=6; COMMIT;")?;
    let left: i64 = db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE sql LIKE '%data_revision%'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(left, 0, "echter Stand von Schema 6");
    Ok(())
}

#[test]
fn schema_six_upgrades_to_seven_after_a_backup_and_keeps_data() -> anyhow::Result<()> {
    let folder = tempfile::tempdir()?;
    schema_six(folder.path())?;
    // Nur lesend: keine Migration, keine Revision, nichts wird wiederverwendet.
    {
        let c = Collection::open(folder.path(), false)?;
        assert_eq!(
            c.db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
            6
        );
        assert_eq!(revision::read(&c.db)?, None);
        assert_eq!(start_view::stamp(&c)?, None);
        assert_ne!(
            relationship_graph::signature(&c)?,
            relationship_graph::signature(&c)?
        );
        assert_eq!(c.status()?.photos, 1);
    }
    let c = Collection::open(folder.path(), true)?;
    assert_eq!(
        c.db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        foto_importer::core::migrations::SCHEMA_VERSION
    );
    let gone: i64 = c.db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name='projection_revision' OR sql LIKE '%projection_revision%'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(gone, 0);
    // Identitäten und Entscheidungen bleiben.
    let row: (String, i64, i64, String, i64) = c.db.query_row(
        "SELECT (SELECT id FROM collection),(SELECT photo_id FROM files WHERE id=91),
            (SELECT person_id FROM faces WHERE id=99),
            (SELECT value FROM person_attrs WHERE person_id=77 AND key='name'),
            (SELECT count(*) FROM face_rejections WHERE face_id=99 AND person_id=5)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    assert_eq!(row, ("sechs".into(), 42, 77, "Alt".into(), 1));
    assert_eq!(
        c.db.query_row(
            "SELECT file_id FROM metadata_photos WHERE photo_id=42",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        91
    );
    // Die Trigger zählen nach der Migration.
    let before = now(&c)?;
    assert_eq!(before.identity, "sechs");
    bumps(
        &c,
        "UPDATE files SET size=3 WHERE id=91",
        Some(Domain::Files),
    )?;
    bumps(
        &c,
        "UPDATE faces SET assignment='auto' WHERE id=99",
        Some(Domain::Faces),
    )?;
    bumps(
        &c,
        "INSERT INTO person_attrs VALUES(77,'note','n')",
        Some(Domain::People),
    )?;
    // Vor der Migration entstand eine Datensicherung mit Schema 6.
    let backups = foto_importer::core::backup::list(folder.path())?;
    assert_eq!(backups.len(), 1);
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&backups[0].path)?)?;
    let restored = tempfile::tempdir()?;
    let db = restored.path().join("db");
    std::io::copy(
        &mut zip.by_name("sammlung/.myfotomanager")?,
        &mut std::fs::File::create(&db)?,
    )?;
    assert_eq!(
        Connection::open(&db)?.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        6
    );
    drop(c);
    // Erneutes Öffnen migriert nicht noch einmal.
    let _c = Collection::open(folder.path(), true)?;
    assert_eq!(foto_importer::core::backup::list(folder.path())?.len(), 1);
    Ok(())
}
