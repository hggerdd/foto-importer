//! Umbenannte Ordner, fehlende Dateien beim Lesen, Übersicht und Entfernen fehlender Einträge.
//! Nur temporäre Sammlungen.
use anyhow::{Result, bail};
use foto_importer::core::{
    collection::Collection,
    jobs::Control,
    metadata::{self, ExifToolUnavailable},
    missing, scope,
};
use rusqlite::params;
use serde_json::{Value, json};
use std::{fs, path::Path, path::PathBuf, time::SystemTime};

/// Ordner wie aus dem Import: DNG im Ordner, JPG und NEF in Unterordnern.
fn day_folder(root: &Path, name: &str, stems: &[&str]) -> Result<()> {
    let dir = root.join(name);
    fs::create_dir_all(dir.join("jpg"))?;
    fs::create_dir_all(dir.join("nef"))?;
    for stem in stems {
        fs::write(
            dir.join(format!("{stem}.dng")),
            format!("dng {name} {stem}"),
        )?;
        fs::write(
            dir.join("jpg").join(format!("{stem}.JPG")),
            format!("jpg {stem}"),
        )?;
        fs::write(
            dir.join("nef").join(format!("{stem}.NEF")),
            format!("nef {stem}"),
        )?;
    }
    Ok(())
}

fn ids(c: &Collection) -> Result<Vec<(i64, i64, String, String)>> {
    Ok(c.db
        .prepare("SELECT id,photo_id,name,status FROM files ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Eine bestätigte Zuordnung am Foto der DNG-Datei `relpath`.
fn confirm_face(c: &Collection, relpath: &str) -> Result<i64> {
    let (file, photo): (i64, i64) = c.db.query_row(
        "SELECT id,photo_id FROM files WHERE relpath=?",
        [relpath],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    c.db.execute("INSERT OR IGNORE INTO persons(id) VALUES(7)", [])?;
    c.db.execute(
        "INSERT INTO faces(photo_id,source_file_id,source_relpath,detector_id,person_id,assignment) VALUES(?,?,?,'test',7,'user')",
        params![photo, file, relpath],
    )?;
    Ok(c.db.last_insert_rowid())
}

fn other_files(root: &Path) -> Result<()> {
    // Genug weitere Dateien, damit die 20-%-Schwelle nicht greift.
    fs::create_dir_all(root.join("Andere"))?;
    for n in 0..40 {
        fs::write(
            root.join("Andere").join(format!("{n}.jpg")),
            format!("andere {n}"),
        )?;
    }
    Ok(())
}

#[test]
fn a_renamed_folder_keeps_file_and_photo_ids_and_confirmed_faces() -> Result<()> {
    let temp = tempfile::tempdir()?;
    other_files(temp.path())?;
    day_folder(
        temp.path(),
        "260822_Hochzeit_Tag_2",
        &["DSC_0220", "DSC_0221"],
    )?;
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    let face = confirm_face(&c, "260822_Hochzeit_Tag_2/DSC_0220.dng")?;
    let before = ids(&c)?;
    let photos_before = c.status()?.photos;
    fs::rename(
        temp.path().join("260822_Hochzeit_Tag_2"),
        temp.path().join("260822_Geburtstag_Martin"),
    )?;
    let message = c.quick_scan(false, &Control::default())?;
    assert!(message.contains("6 verschoben"), "{message}");
    let after = ids(&c)?;
    assert_eq!(
        before
            .iter()
            .map(|r| (r.0, r.1, r.2.clone()))
            .collect::<Vec<_>>(),
        after
            .iter()
            .map(|r| (r.0, r.1, r.2.clone()))
            .collect::<Vec<_>>(),
        "gleiche Datei- und Foto-IDs, keine neuen Zeilen"
    );
    assert!(after.iter().all(|r| r.3 == "present"));
    assert_eq!(c.status()?.photos, photos_before, "keine doppelten Fotos");
    let (person, source): (i64, String) = c.db.query_row(
        "SELECT person_id,source_relpath FROM faces WHERE id=?",
        [face],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(person, 7);
    assert_eq!(source, "260822_Geburtstag_Martin/DSC_0220.dng");
    Ok(())
}

#[test]
fn without_file_ids_a_rename_is_found_by_name_size_and_time() -> Result<()> {
    let temp = tempfile::tempdir()?;
    other_files(temp.path())?;
    day_folder(temp.path(), "Alt", &["DSC_0001"])?;
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    let face = confirm_face(&c, "Alt/DSC_0001.dng")?;
    // Dateisystem ohne stabile Datei-IDs (oder Wechsel des Datenträgers).
    c.db.execute("UPDATE files SET inode=NULL", [])?;
    let before = ids(&c)?;
    fs::rename(temp.path().join("Alt"), temp.path().join("Neu"))?;
    c.quick_scan(false, &Control::default())?;
    let after = ids(&c)?;
    assert_eq!(before.len(), after.len());
    assert!(after.iter().all(|r| r.3 == "present"));
    assert_eq!(
        c.db.query_row("SELECT source_relpath FROM faces WHERE id=?", [face], |r| r
            .get::<_, String>(0))?,
        "Neu/DSC_0001.dng"
    );
    Ok(())
}

#[test]
fn an_ambiguous_rename_is_not_guessed() -> Result<()> {
    let temp = tempfile::tempdir()?;
    other_files(temp.path())?;
    let time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    for dir in ["A", "B"] {
        fs::create_dir_all(temp.path().join(dir))?;
        let path = temp.path().join(dir).join("x.jpg");
        fs::write(&path, b"same bytes")?;
        fs::File::options()
            .write(true)
            .open(&path)?
            .set_modified(time)?;
    }
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    c.db.execute("UPDATE files SET inode=NULL", [])?;
    let old: Vec<i64> =
        c.db.prepare("SELECT id FROM files WHERE name='x.jpg' ORDER BY id")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
    fs::rename(temp.path().join("A"), temp.path().join("C"))?;
    fs::rename(temp.path().join("B"), temp.path().join("D"))?;
    let message = c.quick_scan(false, &Control::default())?;
    assert!(message.contains("2 mehrdeutig"), "{message}");
    for id in old {
        assert_eq!(
            c.db.query_row("SELECT status FROM files WHERE id=?", [id], |r| r
                .get::<_, String>(0))?,
            "missing",
            "nicht geraten: die alten Zeilen bleiben fehlend"
        );
    }
    Ok(())
}

fn source_of(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Liest jede Datei; scheitert am ganzen Block, sobald `bad` darin ist (wie ExifTool bei einer
/// beschädigten Datei mit Exit-Code > 1).
fn fake_reader(
    bad: Option<&'static str>,
) -> impl FnMut(&[PathBuf], &Control) -> Result<Vec<Value>> {
    move |paths, _| {
        if let Some(bad) = bad
            && paths.iter().any(|p| p.ends_with(bad))
        {
            bail!("ExifTool: Fehler beim Lesen von {bad}");
        }
        Ok(paths
            .iter()
            .map(|p| json!({"SourceFile": source_of(p), "Make": "Nikon", "Model": "Z 6_2"}))
            .collect())
    }
}

fn file_ids(c: &Collection) -> Result<Vec<i64>> {
    Ok(c.db
        .prepare("SELECT id FROM files ORDER BY id")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn state(c: &Collection, name: &str) -> Result<(String, String)> {
    Ok(c.db.query_row(
        "SELECT status,meta_state FROM files WHERE name=?",
        [name],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

#[test]
fn a_missing_file_does_not_stop_the_metadata_batch() -> Result<()> {
    let temp = tempfile::tempdir()?;
    fs::create_dir_all(temp.path().join("Ordner"))?;
    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        fs::write(temp.path().join("Ordner").join(name), name)?;
    }
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    fs::remove_dir_all(temp.path().join("Ordner"))?;
    fs::create_dir_all(temp.path().join("Ordner"))?;
    for name in ["a.jpg", "c.jpg"] {
        fs::write(temp.path().join("Ordner").join(name), name)?;
    }
    let order = file_ids(&c)?;
    let result = metadata::enrich_with(
        &mut c,
        &order,
        512,
        &Control::default(),
        &mut fake_reader(None),
    )?;
    assert_eq!((result.done, result.missing), (2, 1));
    assert_eq!(state(&c, "a.jpg")?, ("present".into(), "done".into()));
    assert_eq!(state(&c, "c.jpg")?, ("present".into(), "done".into()));
    assert_eq!(state(&c, "b.jpg")?.0, "missing");
    assert!(
        scope::metadata_unread(&c, 100)?.is_empty(),
        "fehlende Dateien warten nicht"
    );
    assert_eq!(c.status()?.missing, 1);
    // Taucht der Ordner wieder auf, findet der Bestandslauf die Datei mit derselben ID.
    let id: i64 =
        c.db.query_row("SELECT id FROM files WHERE name='b.jpg'", [], |r| r.get(0))?;
    fs::write(temp.path().join("Ordner").join("b.jpg"), "b.jpg")?;
    c.quick_scan(false, &Control::default())?;
    assert_eq!(
        c.db.query_row(
            "SELECT id,status FROM files WHERE name='b.jpg'",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        )?,
        (id, "present".into())
    );
    Ok(())
}

#[test]
fn an_exiftool_error_on_one_file_only_marks_that_file() -> Result<()> {
    let temp = tempfile::tempdir()?;
    for name in ["a.jpg", "bad.jpg", "c.jpg", "d.jpg", "e.jpg"] {
        fs::write(temp.path().join(name), name)?;
    }
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    let order = file_ids(&c)?;
    let result = metadata::enrich_with(
        &mut c,
        &order,
        512,
        &Control::default(),
        &mut fake_reader(Some("bad.jpg")),
    )?;
    assert_eq!((result.done, result.missing), (4, 0));
    let (status, meta) = state(&c, "bad.jpg")?;
    assert_eq!((status.as_str(), meta.as_str()), ("present", "error"));
    for name in ["a.jpg", "c.jpg", "d.jpg", "e.jpg"] {
        assert_eq!(state(&c, name)?.1, "done", "{name}");
    }
    // Fehlt ExifTool selbst, wird nicht geteilt: der Lauf endet sichtbar.
    c.db.execute("UPDATE files SET meta_state='pending'", [])?;
    let mut calls = 0;
    let error = metadata::enrich_with(&mut c, &order, 512, &Control::default(), &mut |_, _| {
        calls += 1;
        Err(anyhow::Error::from(std::io::Error::other("spawn")).context(ExifToolUnavailable))
    })
    .expect_err("ohne ExifTool endet der Lauf");
    assert!(error.downcast_ref::<ExifToolUnavailable>().is_some());
    assert_eq!(calls, 1);
    Ok(())
}

#[test]
fn removing_missing_entries_touches_only_missing_rows_and_keeps_decisions() -> Result<()> {
    let temp = tempfile::tempdir()?;
    other_files(temp.path())?;
    day_folder(temp.path(), "Weg", &["DSC_0001", "DSC_0002"])?;
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    let face = confirm_face(&c, "Weg/DSC_0001.dng")?;
    let photo: i64 =
        c.db.query_row("SELECT photo_id FROM faces WHERE id=?", [face], |r| {
            r.get(0)
        })?;
    let present_before = c.status()?.files;
    fs::remove_dir_all(temp.path().join("Weg"))?;
    c.quick_scan(false, &Control::default())?;
    assert_eq!(c.status()?.missing, 6);
    let report = missing::report(&c, &Control::default())?;
    assert_eq!(report.len(), 1, "Variantenordner zählen zum Tagesordner");
    assert_eq!(report[0].dir, "Weg");
    assert_eq!(report[0].files, 6);
    assert_eq!(report[0].orphaned_photos, 2);
    assert_eq!(report[0].confirmed_faces, 1);
    assert_eq!(report[0].relink, None);
    // Eine Datei ist wieder da: sie bleibt im Bestand.
    fs::create_dir_all(temp.path().join("Weg"))?;
    fs::write(
        temp.path().join("Weg").join("DSC_0002.dng"),
        "dng Weg DSC_0002",
    )?;
    let message = missing::remove(&mut c, &["Weg".into()], &Control::default())?;
    assert!(message.starts_with("5 fehlende"), "{message}");
    assert_eq!(
        c.status()?.files,
        present_before - 6,
        "vorhandene Dateien unberührt"
    );
    assert_eq!(
        c.db.query_row(
            "SELECT count(*) FROM files WHERE relpath='Weg/DSC_0002.dng'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        1
    );
    // Foto und bestätigte Zuordnung bleiben gespeichert.
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM photos WHERE id=?", [photo], |r| r
            .get::<_, i64>(0))?,
        1
    );
    assert_eq!(
        c.db.query_row("SELECT person_id FROM faces WHERE id=?", [face], |r| r
            .get::<_, i64>(0))?,
        7
    );
    assert!(temp.path().join("Andere").join("0.jpg").exists());
    // Vorher entstand automatisch eine Datensicherung mit dem Stand vor dem Entfernen.
    let backups = foto_importer::core::backup::list(temp.path())?;
    assert_eq!(backups.len(), 1);
    let mut zip = zip::ZipArchive::new(fs::File::open(&backups[0].path)?)?;
    let restored = tempfile::tempdir()?;
    let db = restored.path().join("db");
    std::io::copy(
        &mut zip.by_name("sammlung/.myfotomanager")?,
        &mut fs::File::create(&db)?,
    )?;
    let old = rusqlite::Connection::open(&db)?;
    assert_eq!(
        old.query_row(
            "SELECT count(*) FROM files WHERE status='missing'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        6
    );
    Ok(())
}

#[test]
fn a_folder_entered_twice_can_be_relinked_to_the_old_identities() -> Result<()> {
    let temp = tempfile::tempdir()?;
    other_files(temp.path())?;
    day_folder(temp.path(), "Alt", &["DSC_0001", "DSC_0002"])?;
    let mut c = Collection::open(temp.path(), true)?;
    c.quick_scan(false, &Control::default())?;
    let face = confirm_face(&c, "Alt/DSC_0001.dng")?;
    let old: Vec<(i64, i64)> =
        c.db.prepare("SELECT id,photo_id FROM files WHERE dir LIKE 'Alt%' ORDER BY relpath")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
    // Kopie mit gleichen Zeiten (neue Datei-IDs), dann das Original weg: wie ein Ordner, der
    // von einer älteren Version doppelt eingetragen wurde.
    copy_tree(&temp.path().join("Alt"), &temp.path().join("Neu"))?;
    c.quick_scan(false, &Control::default())?;
    fs::remove_dir_all(temp.path().join("Alt"))?;
    c.quick_scan(false, &Control::default())?;
    let report = missing::report(&c, &Control::default())?;
    assert_eq!(report[0].relink.as_deref(), Some("Neu"));
    missing::relink(&mut c, "Alt", "Neu", &Control::default())?;
    let new: Vec<(i64, i64)> =
        c.db.prepare("SELECT id,photo_id FROM files WHERE dir LIKE 'Neu%' ORDER BY relpath")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
    assert_eq!(old, new, "alte Datei- und Foto-IDs an den neuen Pfaden");
    assert_eq!(c.status()?.missing, 0);
    assert_eq!(
        c.db.query_row("SELECT source_relpath FROM faces WHERE id=?", [face], |r| r
            .get::<_, String>(0))?,
        "Neu/DSC_0001.dng"
    );
    assert_eq!(c.status()?.photos, 42, "keine doppelten Fotos");
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
            let modified = entry.metadata()?.modified()?;
            fs::File::options()
                .write(true)
                .open(&target)?
                .set_modified(modified)?;
        }
    }
    Ok(())
}
