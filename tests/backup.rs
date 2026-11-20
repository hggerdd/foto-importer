//! Datensicherung (`core::backup`): Inhalt, Wiederherstellbarkeit, Nummern, Aufräumen, Abbruch.
//! Nur temporäre Ordner; keine echte Sammlung.
use anyhow::Result;
use foto_importer::core::{
    backup::{self, AppData, Plan},
    central::Central,
    collection::Collection,
    jobs::Control,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

fn collection(root: &Path) -> Result<()> {
    let c = Collection::open(root, true)?;
    c.db.execute_batch(
        "INSERT INTO persons(id) VALUES(77);
         INSERT INTO person_attrs VALUES(77,'name','Oma Berta');
         INSERT INTO settings VALUES('entscheidung','\"behalten\"');",
    )?;
    Ok(())
}

fn workplace(dir: &Path) -> Result<AppData> {
    let central = dir.join("central.sqlite");
    drop(Central::open(&central, false)?);
    let settings = dir.join("settings.json");
    fs::write(&settings, br#"{"collection":"X:\\Bilder","backup_keep":5}"#)?;
    Ok(AppData {
        settings: Some(settings),
        central: Some(central),
    })
}

fn names(root: &Path) -> Result<Vec<String>> {
    let mut names: Vec<String> = fs::read_dir(root)?
        .map(|e| Ok(e?.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_>>()?;
    names.sort();
    Ok(names)
}

fn no_leftovers(root: &Path) -> Result<()> {
    let left: Vec<String> = names(root)?
        .into_iter()
        .filter(|n| n.starts_with(foto_importer::config::BACKUP_TEMP_PREFIX))
        .collect();
    assert!(left.is_empty(), "Zwischenstände übrig: {left:?}");
    Ok(())
}

fn integrity(db: &Connection) -> Result<String> {
    Ok(db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?)
}

/// Packt eine Sicherung aus, prüft Prüfsummen und Datenbanken und liefert das Manifest.
fn unpack(zip_path: &Path, into: &Path) -> Result<serde_json::Value> {
    let mut zip = zip::ZipArchive::new(fs::File::open(zip_path)?)?;
    let mut text = String::new();
    zip.by_name("manifest.json")?.read_to_string(&mut text)?;
    let manifest: serde_json::Value = serde_json::from_str(&text)?;
    for entry in manifest["entries"].as_array().expect("Einträge") {
        let name = entry["name"].as_str().expect("Name");
        let target = into.join(name);
        fs::create_dir_all(target.parent().expect("Ordner"))?;
        std::io::copy(&mut zip.by_name(name)?, &mut fs::File::create(&target)?)?;
        let bytes = fs::read(&target)?;
        assert_eq!(bytes.len() as u64, entry["bytes"].as_u64().expect("Größe"));
        let sum: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(sum, entry["sha256"].as_str().expect("Prüfsumme"), "{name}");
    }
    Ok(manifest)
}

#[test]
fn a_backup_restores_collection_central_and_settings() -> Result<()> {
    let root = tempfile::tempdir()?;
    let work = tempfile::tempdir()?;
    collection(root.path())?;
    let app = workplace(work.path())?;
    // Die Sammlung ist während der Sicherung schreibend geöffnet (wie in der App).
    let open = Collection::open(root.path(), true)?;
    let created = backup::create(
        root.path(),
        &Plan { app, keep: 5 },
        "test",
        &Control::default(),
    )?;
    drop(open);
    assert_eq!(created.number, 1);
    assert!(root.path().join(".myimporter.backup.001").is_file());
    assert_eq!(created.bytes, fs::metadata(&created.path)?.len());
    no_leftovers(root.path())?;

    let restored = tempfile::tempdir()?;
    let manifest = unpack(&created.path, restored.path())?;
    assert_eq!(manifest["format"], 1);
    assert_eq!(manifest["reason"], "test");
    assert_eq!(manifest["entries"].as_array().map(Vec::len), Some(3));
    assert_eq!(manifest["entries"][0]["schema_version"], 6);

    let db = Connection::open(restored.path().join("sammlung/.myfotomanager"))?;
    assert_eq!(integrity(&db)?, "ok");
    assert_eq!(
        db.query_row(
            "SELECT value FROM person_attrs WHERE person_id=77 AND key='name'",
            [],
            |r| r.get::<_, String>(0)
        )?,
        "Oma Berta"
    );
    assert_eq!(
        db.query_row(
            "SELECT value FROM settings WHERE key='entscheidung'",
            [],
            |r| r.get::<_, String>(0)
        )?,
        "\"behalten\""
    );
    // Die Kopie ist eigenständig (kein WAL nötig).
    assert_eq!(
        db.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))?,
        "delete"
    );
    drop(db);
    // Die wiederhergestellte Sammlung öffnet die App normal.
    let again = tempfile::tempdir()?;
    fs::copy(
        restored.path().join("sammlung/.myfotomanager"),
        again.path().join(".myfotomanager"),
    )?;
    assert_eq!(Collection::open(again.path(), false)?.status()?.photos, 0);

    let central = Connection::open(restored.path().join("arbeitsplatz/central.sqlite"))?;
    assert_eq!(integrity(&central)?, "ok");
    assert_eq!(
        central.query_row("SELECT value FROM meta WHERE key='created_by'", [], |r| {
            r.get::<_, String>(0)
        })?,
        "foto-importer"
    );
    assert_eq!(
        fs::read(restored.path().join("arbeitsplatz/settings.json"))?,
        fs::read(work.path().join("settings.json"))?
    );
    Ok(())
}

#[test]
fn numbers_continue_never_replace_and_old_backups_are_pruned() -> Result<()> {
    let root = tempfile::tempdir()?;
    collection(root.path())?;
    // Fremde Dateien mit ähnlichem Namen bleiben unberührt; eine vorhandene Nummer wird nie
    // ersetzt, die nächste Sicherung nimmt die folgende.
    for name in [
        ".myimporter.backup.01",
        ".myimporter.backup.abc",
        ".myimporter.backup.004.txt",
        "myimporter.backup.002",
    ] {
        fs::write(root.path().join(name), b"fremd")?;
    }
    fs::write(root.path().join(".myimporter.backup.004"), b"alt")?;
    let plan = Plan {
        app: AppData::default(),
        keep: 2,
    };
    let first = backup::create(root.path(), &plan, "test", &Control::default())?;
    assert_eq!(first.number, 5);
    assert!(first.removed.is_empty());
    assert_eq!(
        fs::read(root.path().join(".myimporter.backup.004"))?,
        b"alt"
    );
    let second = backup::create(root.path(), &plan, "test", &Control::default())?;
    assert_eq!(second.number, 6);
    assert_eq!(second.removed, vec![4]);
    let third = backup::create(root.path(), &plan, "test", &Control::default())?;
    assert_eq!(third.removed, vec![5]);
    let kept: Vec<u32> = backup::list(root.path())?
        .iter()
        .map(|b| b.number)
        .collect();
    assert_eq!(kept, vec![6, 7]);
    for name in [
        ".myimporter.backup.01",
        ".myimporter.backup.abc",
        ".myimporter.backup.004.txt",
        "myimporter.backup.002",
    ] {
        assert_eq!(fs::read(root.path().join(name))?, b"fremd", "{name}");
    }
    assert!(names(root.path())?.contains(&".myfotomanager".to_string()));
    no_leftovers(root.path())?;
    Ok(())
}

#[test]
fn a_cancelled_backup_leaves_nothing_behind() -> Result<()> {
    let root = tempfile::tempdir()?;
    collection(root.path())?;
    let before = names(root.path())?;
    let control = Control::default();
    control.stop();
    assert!(backup::create(root.path(), &Plan::default(), "test", &control).is_err());
    assert_eq!(names(root.path())?, before);
    assert!(backup::list(root.path())?.is_empty());
    Ok(())
}

#[test]
fn a_folder_without_collection_is_not_backed_up() -> Result<()> {
    let root = tempfile::tempdir()?;
    assert!(backup::create(root.path(), &Plan::default(), "test", &Control::default()).is_err());
    assert!(names(root.path())?.is_empty());
    Ok(())
}

/// Ein Schreiber auf einer anderen Verbindung ändert die Sammlung während der Sicherung: die
/// Kopie bleibt in sich stimmig und die Sicherung endet (kein endloser Neustart).
#[test]
fn concurrent_writes_do_not_break_or_restart_the_copy() -> Result<()> {
    let root = tempfile::tempdir()?;
    collection(root.path())?;
    {
        let db = Connection::open(root.path().join(".myfotomanager"))?;
        db.execute_batch("CREATE TABLE ballast(id INTEGER PRIMARY KEY, data BLOB)")?;
        let tx = db.unchecked_transaction()?;
        for _ in 0..400 {
            tx.execute("INSERT INTO ballast(data) VALUES(randomblob(65536))", [])?;
        }
        tx.commit()?;
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let stop = stop.clone();
        let path = root.path().join(".myfotomanager");
        std::thread::spawn(move || -> Result<usize> {
            let db = Connection::open(path)?;
            db.busy_timeout(std::time::Duration::from_secs(5))?;
            let started = std::time::Instant::now();
            let mut writes = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed)
                && started.elapsed() < std::time::Duration::from_secs(60)
            {
                db.execute(
                    "INSERT INTO settings VALUES(?, 'x')",
                    [format!("w{writes}")],
                )?;
                writes += 1;
            }
            Ok(writes)
        })
    };
    let created = backup::create(root.path(), &Plan::default(), "test", &Control::default())?;
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let writes = writer.join().expect("Schreiber")?;
    assert!(writes > 0);
    let restored = tempfile::tempdir()?;
    unpack(&created.path, restored.path())?;
    let db = Connection::open(restored.path().join("sammlung/.myfotomanager"))?;
    assert_eq!(integrity(&db)?, "ok");
    assert_eq!(
        db.query_row("SELECT count(*) FROM ballast", [], |r| r.get::<_, i64>(0))?,
        400
    );
    Ok(())
}
