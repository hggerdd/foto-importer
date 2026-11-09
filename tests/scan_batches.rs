//! Batched, observable indexing (ADR 0064): same result as one transaction, safe when cancelled.
use foto_importer::core::{collection::Collection, jobs::Control};
use std::{fs, path::Path, sync::atomic::Ordering, time::Duration};

type Inventory = (Vec<(i64, String, i64, String)>, Vec<(i64, String)>);

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    fs::write(path, content).expect("file");
}
/// Pairs (NEF/JPG), a RAW subfolder family, an Export copy and plain photos.
fn variants_tree(root: &Path) {
    write(root, "2024/Urlaub/IMG_0001.jpg", "j1");
    write(root, "2024/Urlaub/IMG_0001.nef", "n1");
    write(root, "2024/Urlaub/IMG_0002.JPG", "j2");
    write(root, "2024/Urlaub/nef/IMG_0002.nef", "n2");
    write(root, "2024/Urlaub/Export/IMG_0001.jpg", "export");
    write(root, "2024/Urlaub/IMG_0003.xmp", "x");
    write(root, "2024/Urlaub/IMG_0003.jpg", "j3");
    write(root, "Familie/IMG_0001.jpg", "other family");
    for index in 0..40 {
        write(
            root,
            &format!("Alltag/{}/B_{index:03}.jpg", index % 7),
            &format!("b{index}"),
        );
    }
}
fn inventory(collection: &Collection) -> rusqlite::Result<Inventory> {
    let files = collection
        .db
        .prepare("SELECT id,relpath,photo_id,status FROM files ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let photos = collection
        .db
        .prepare("SELECT id,status FROM photos ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok((files, photos))
}
fn projection(collection: &Collection) -> rusqlite::Result<Vec<(i64, i64, String)>> {
    collection
        .db
        .prepare("SELECT photo_id,file_id,relpath FROM metadata_photos ORDER BY photo_id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect()
}

#[test]
fn every_batch_size_gives_the_same_inventory_and_projection() -> anyhow::Result<()> {
    let tree = tempfile::tempdir()?;
    variants_tree(tree.path());
    let mut reference = None;
    for batch in [1, 3, 7, 500, 100_000] {
        let copy = tempfile::tempdir()?;
        copy_tree(tree.path(), copy.path());
        let mut collection = Collection::open(copy.path(), true)?;
        let control = Control::default();
        collection.quick_scan_batched(false, &control, batch)?;
        let state = (inventory(&collection)?, projection(&collection)?);
        match &reference {
            None => reference = Some(state),
            Some(first) => assert_eq!(first, &state, "batch size {batch}"),
        }
        let names: Vec<_> = control.timings().into_iter().map(|t| t.0).collect();
        assert_eq!(names.len(), 5, "{names:?}");
        let end = control.snapshot();
        assert_eq!(end.phase, "Schritt 4 von 4 · Sicherung schreiben");
    }
    let ((files, photos), _) = reference.expect("state");
    let photo_of = |relative: &str| {
        files
            .iter()
            .find(|f| f.1 == relative)
            .map(|f| f.2)
            .expect(relative)
    };
    assert_eq!(
        photo_of("2024/Urlaub/IMG_0001.jpg"),
        photo_of("2024/Urlaub/IMG_0001.nef")
    );
    assert_eq!(
        photo_of("2024/Urlaub/IMG_0002.JPG"),
        photo_of("2024/Urlaub/nef/IMG_0002.nef")
    );
    assert_ne!(
        photo_of("2024/Urlaub/IMG_0001.jpg"),
        photo_of("2024/Urlaub/Export/IMG_0001.jpg")
    );
    assert_ne!(
        photo_of("2024/Urlaub/IMG_0001.jpg"),
        photo_of("Familie/IMG_0001.jpg")
    );
    assert!(photos.iter().all(|p| p.1 == "present"));
    Ok(())
}
fn copy_tree(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).expect("dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            fs::create_dir_all(&target).expect("folder");
            copy_tree(&entry.path(), &target);
        } else if !entry.file_name().to_string_lossy().starts_with(".myfoto") {
            fs::copy(entry.path(), target).expect("copy");
        }
    }
}

#[test]
fn collection_is_usable_while_the_run_goes_on_and_after_a_cancel() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    // Earlier inventory whose files are gone from disk: unvisited by every run below.
    for index in 0..30 {
        write(
            temp.path(),
            &format!("alt/A_{index:03}.jpg"),
            &format!("a{index}"),
        );
    }
    let mut collection = Collection::open(temp.path(), true)?;
    collection.quick_scan(false, &Control::default())?;
    fs::remove_dir_all(temp.path().join("alt"))?;
    for index in 0..3000 {
        write(
            temp.path(),
            &format!("neu/{}/N_{index:04}.jpg", index % 30),
            &format!("n{index}"),
        );
    }
    let control = Control::default();
    let watcher = control.clone();
    let reader_root = temp.path().to_path_buf();
    let observer = std::thread::spawn(move || -> anyhow::Result<usize> {
        // A reader sees the first committed batches while the writer is still running.
        let mut seen = 0;
        while watcher.committed.load(Ordering::Relaxed) < 200 {
            std::thread::sleep(Duration::from_millis(1));
        }
        let reader = Collection::open(&reader_root, false)?;
        seen += reader.status()?.files;
        watcher.stop();
        Ok(seen)
    });
    let result = collection.quick_scan_batched(false, &control, 100);
    let seen = observer.join().expect("observer")?;
    assert!(result.is_err(), "the run was cancelled");
    assert!(
        seen >= 30 + 200,
        "reader saw {seen} files from committed batches"
    );
    let status = collection.status()?;
    assert!(
        status.files >= 230 && status.files < 3030,
        "{}",
        status.files
    );
    assert_eq!(
        status.missing, 0,
        "a cancelled scan never marks files as missing"
    );
    let state: String = collection.db.query_row(
        "SELECT status FROM scans ORDER BY id DESC LIMIT 1",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(state, "cancelled");
    let projected: i64 =
        collection
            .db
            .query_row("SELECT count(*) FROM metadata_photos", [], |r| r.get(0))?;
    assert_eq!(
        projected, status.photos as i64,
        "projection matches the entered files"
    );

    // The next full run continues: result equals an uninterrupted run on a fresh collection.
    let rows_before = inventory(&collection)?.0;
    collection.quick_scan(false, &Control::default())?;
    let after = inventory(&collection)?;
    let key = |rows: &[(i64, String, i64, String)]| {
        rows.iter()
            .map(|f| (f.0, f.1.clone(), f.2))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        key(&after.0[..rows_before.len()]),
        key(&rows_before),
        "entered rows keep ids"
    );
    assert_eq!(collection.status()?.files, 3000);
    assert_eq!(
        collection.status()?.missing,
        30,
        "a complete run marks the vanished files"
    );
    let fresh_root = tempfile::tempdir()?;
    copy_tree(temp.path(), fresh_root.path());
    let mut fresh = Collection::open(fresh_root.path(), true)?;
    fresh.quick_scan(false, &Control::default())?;
    let fresh_files: Vec<_> = inventory(&fresh)?.0.into_iter().map(|f| f.1).collect();
    let resumed_files: Vec<_> = after
        .0
        .into_iter()
        .filter(|f| f.3 == "present")
        .map(|f| f.1)
        .collect();
    let mut a = fresh_files;
    let mut b = resumed_files;
    a.sort();
    b.sort();
    assert_eq!(a, b);
    Ok(())
}

#[test]
fn unchanged_rescan_leaves_rows_and_projection_untouched() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    variants_tree(temp.path());
    let mut collection = Collection::open(temp.path(), true)?;
    collection.quick_scan(false, &Control::default())?;
    let before = (inventory(&collection)?, projection(&collection)?);
    let revision: i64 =
        collection
            .db
            .query_row("SELECT revision FROM projection_revision", [], |r| r.get(0))?;
    collection.quick_scan(false, &Control::default())?;
    assert_eq!(before, (inventory(&collection)?, projection(&collection)?));
    let after: i64 =
        collection
            .db
            .query_row("SELECT revision FROM projection_revision", [], |r| r.get(0))?;
    assert_eq!(revision, after, "nothing changed, so no projection rebuild");
    let seen: i64 = collection.db.query_row(
        "SELECT count(*) FROM files WHERE last_seen_scan=(SELECT max(id) FROM scans)",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(seen, collection.status()?.files as i64);
    Ok(())
}

#[test]
fn rename_keeps_file_and_photo_identity() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    write(temp.path(), "a/Foto_1.jpg", "one");
    write(temp.path(), "a/Foto_2.jpg", "two-two");
    let mut collection = Collection::open(temp.path(), true)?;
    collection.quick_scan(false, &Control::default())?;
    let before: (i64, i64) = collection.db.query_row(
        "SELECT id,photo_id FROM files WHERE relpath='a/Foto_1.jpg'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    fs::create_dir_all(temp.path().join("b"))?;
    fs::rename(
        temp.path().join("a/Foto_1.jpg"),
        temp.path().join("b/Umbenannt.jpg"),
    )?;
    let message = collection.quick_scan(false, &Control::default())?;
    assert!(message.contains("1 verschoben"), "{message}");
    let after: (i64, i64) = collection.db.query_row(
        "SELECT id,photo_id FROM files WHERE relpath='b/Umbenannt.jpg'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(before, after);
    assert_eq!(collection.status()?.files, 2);
    assert_eq!(collection.status()?.missing, 0);
    Ok(())
}
