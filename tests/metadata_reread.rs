use foto_importer::core::{
    collection::Collection,
    jobs::Control,
    metadata::{self, FileRow},
    scope,
};
use rusqlite::params;
use serde_json::json;

fn seed(c: &Collection, id: i64, state: &str) -> anyhow::Result<()> {
    c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
    let path = format!("{id}.jpg");
    c.db.execute(
        "INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status,meta_state) VALUES(?,?,?,'',?,?,'jpg',1,?,?,'present',?)",
        params![id, path, path, path, id.to_string(), id, id, state],
    )?;
    Ok(())
}

#[test]
fn unread_selection_excludes_done_and_error_but_keeps_stale() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    for (id, state) in [(1, "done"), (2, "error"), (3, "pending"), (4, "stale")] {
        seed(&c, id, state)?;
    }
    assert_eq!(scope::metadata_unread(&c, 10)?, vec![4, 3]);
    assert_eq!(scope::metadata_unread(&c, 1)?, vec![4]);
    assert_eq!(c.status()?.unread, 2);
    assert_eq!(c.status()?.pending, 3);
    Ok(())
}

#[test]
fn rereading_a_file_overwrites_its_row_and_keeps_the_photo_identity() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    seed(&c, 1, "pending")?;
    let file = temp.path().join("1.jpg");
    std::fs::write(&file, b"x")?;
    let rows: Vec<FileRow> = vec![(1, "1.jpg".into(), 1)];
    let paths = vec![c.safe_path("1.jpg")?];
    let source = paths[0].to_string_lossy().replace('\\', "/");
    let raw = |model: &str| {
        vec![json!({
            "SourceFile": source,
            "Make": "Nikon", "Model": model, "SerialNumber": "123", "ShutterCount": 77,
            "DateTimeOriginal": "2024:01:02 03:04:05"
        })]
    };
    let control = Control::default();
    assert_eq!(
        metadata::store_results(&mut c, &rows, &paths, raw("D1"), &control)?,
        1
    );
    let identity = |c: &Collection| -> rusqlite::Result<(Option<String>, i64)> {
        c.db.query_row(
            "SELECT p.identity_key,f.photo_id FROM files f JOIN photos p ON p.id=f.photo_id WHERE f.id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    };
    let first = identity(&c)?;
    assert_eq!(first.0.as_deref(), Some("123:77"));
    // Zweites Lesen mit geändertem Modell überschreibt nur die meta-Zeile.
    metadata::store_results(&mut c, &rows, &paths, raw("D2"), &control)?;
    let model: String =
        c.db.query_row("SELECT model FROM meta WHERE file_id=1", [], |r| r.get(0))?;
    assert_eq!(model, "D2");
    let rows_in_meta: i64 =
        c.db.query_row("SELECT count(*) FROM meta", [], |r| r.get(0))?;
    assert_eq!(rows_in_meta, 1);
    assert_eq!(identity(&c)?, first);
    assert_eq!(scope::photo_files(&c, 1)?, vec![1]);
    Ok(())
}
