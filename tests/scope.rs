use foto_importer::core::{
    collection::Collection,
    metadata,
    scope::{self, Scope},
};
use rusqlite::params;

/// (Datei-ID, Pfad, mtime in Sekunden, Aufnahmedatum)
fn seed(c: &Collection, rows: &[(i64, &str, i64, Option<&str>)]) -> anyhow::Result<()> {
    for (id, path, mtime, taken) in rows {
        c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
        c.db.execute(
            "INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status) VALUES(?,?,?,'d',?,?,'jpg',12,?,?,'present')",
            params![id, path, path.to_lowercase(), format!("{id}.jpg"), id.to_string(), mtime * 1_000_000_000, id],
        )?;
        if let Some(taken) = taken {
            c.db.execute(
                "INSERT INTO meta(file_id,dt_original,json) VALUES(?,?,'{}')",
                params![id, taken],
            )?;
        }
    }
    Ok(())
}

#[test]
fn analysis_runs_newest_first_by_capture_date_then_file_date() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    // 2022-01-01, 2023-01-01, 2024-01-01 als Dateidatum; Datei 1 wurde 2026 aufgenommen.
    seed(
        &c,
        &[
            (1, "a/1.jpg", 1_640_995_200, Some("2026:03:01 10:00:00")),
            (2, "a/2.jpg", 1_672_531_200, None),
            (3, "a/3.jpg", 1_704_067_200, None),
        ],
    )?;
    assert_eq!(scope::analysis_order(&c, &Scope::default())?, vec![1, 3, 2]);
    Ok(())
}

#[test]
fn folder_scope_only_matches_that_folder_and_escapes_wildcards() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    seed(
        &c,
        &[
            (1, "2025/Urlaub/1.jpg", 10, None),
            (2, "2025/Urlaub/tag1/2.jpg", 20, None),
            (3, "2025/UrlaubX/3.jpg", 30, None),
            (4, "a_b/4.jpg", 40, None),
            (5, "axb/5.jpg", 50, None),
        ],
    )?;
    let folder = |f: &str| Scope::new(None, Some(f.into()));
    assert_eq!(
        scope::analysis_order(&c, &folder("2025/urlaub"))?,
        vec![2, 1]
    );
    assert_eq!(
        scope::analysis_order(&c, &folder("a_b"))?,
        vec![4],
        "_ ist kein Platzhalter"
    );
    assert_eq!(
        scope::analysis_order(&c, &folder(r"2025\Urlaub\"))?,
        vec![2, 1]
    );
    assert_eq!(
        scope::analysis_order(&c, &Scope::new(Some(0), Some(String::new())))?.len(),
        5
    );
    Ok(())
}

#[test]
fn metadata_portion_takes_the_newest_files_and_defers_earlier_errors() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    seed(
        &c,
        &[
            (1, "a/1.jpg", 10, None),
            (2, "a/2.jpg", 20, None),
            (3, "a/3.jpg", 30, None),
            (4, "a/4.jpg", 40, None),
        ],
    )?;
    c.db.execute("UPDATE files SET meta_state='done' WHERE id=2", [])?;
    c.db.execute("UPDATE files SET meta_state='error' WHERE id=4", [])?;
    let all = scope::metadata_order(&c, &Scope::default())?;
    assert_eq!(all, vec![3, 1, 4], "fertige fehlen, Fehler stehen zuletzt");
    let portion = scope::metadata_order(&c, &Scope::new(Some(2), None))?;
    assert_eq!(portion, vec![3, 1]);
    let _ = metadata::FIELDS; // Anreicherung selbst braucht ExifTool und wird dort geprüft.
    Ok(())
}
