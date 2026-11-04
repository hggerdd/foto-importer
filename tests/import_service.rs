use foto_importer::core::{
    collection::Collection,
    files::SourceFile,
    import_service::{self, ImportState},
    jobs::Control,
};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};
fn exiftool() -> &'static Path {
    static TOOL: OnceLock<PathBuf> = OnceLock::new();
    TOOL.get_or_init(|| {
        let folder = tempfile::tempdir().expect("fixture directory").keep();
        let exe = folder.join(if cfg!(windows) {
            "exiftool-fixture.exe"
        } else {
            "exiftool-fixture"
        });
        let status = std::process::Command::new("rustc")
            .arg("tests/fixtures/exiftool.rs")
            .args(["-o"])
            .arg(&exe)
            .status()
            .expect("Rust compiler for process fixture");
        assert!(status.success());
        exe
    })
    .as_path()
}
fn source(root: &Path) -> anyhow::Result<SourceFile> {
    let path = root.join("card.jpg");
    std::fs::write(&path, b"camera-original")?;
    Ok(SourceFile {
        path,
        day: "2026-11-04".into(),
        size: 15,
    })
}
#[test]
fn strong_identity_skips_copy_without_creating_empty_album() -> anyhow::Result<()> {
    let card = tempfile::tempdir()?;
    let archive = tempfile::tempdir()?;
    let file = source(card.path())?;
    let c = Collection::open(archive.path(), true)?;
    c.db.execute(
        "INSERT INTO photos(identity_key,status) VALUES('cam:7','present')",
        [],
    )?;
    drop(c);
    let report = import_service::execute(
        &[file],
        archive.path(),
        "Already imported",
        None,
        true,
        &exiftool().to_string_lossy(),
        &Control::default(),
    )?;
    assert_eq!(report.skipped.len(), 1);
    assert!(report.copied.is_empty());
    assert!(!archive.path().join("Already imported").exists());
    Ok(())
}
#[test]
fn missing_known_photo_is_eligible_for_reimport() -> anyhow::Result<()> {
    let card = tempfile::tempdir()?;
    let archive = tempfile::tempdir()?;
    let file = source(card.path())?;
    let c = Collection::open(archive.path(), true)?;
    c.db.execute(
        "INSERT INTO photos(identity_key,status) VALUES('cam:7','missing')",
        [],
    )?;
    let result = import_service::check(
        &c,
        &[file],
        &exiftool().to_string_lossy(),
        &Control::default(),
    )?;
    assert_eq!(result[0].state, ImportState::ImportedMissing);
    Ok(())
}
#[test]
fn unreadable_exiftool_stops_import_before_any_copy() -> anyhow::Result<()> {
    let card = tempfile::tempdir()?;
    let archive = tempfile::tempdir()?;
    let file = source(card.path())?;
    drop(Collection::open(archive.path(), true)?);
    let result = import_service::execute(
        &[file],
        archive.path(),
        "Do not copy",
        None,
        true,
        "missing-exiftool-0123456789",
        &Control::default(),
    );
    assert!(result.is_err());
    assert!(!archive.path().join("Do not copy").exists());
    Ok(())
}
