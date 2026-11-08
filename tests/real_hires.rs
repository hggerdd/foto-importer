//! Echtlauf auf einer Kopie einer Sammlung (ADR 0054). Nur mit
//! FOTO_REAL_ROOT (Ordner mit .myfotomanager-Kopie und Fotoverknüpfungen),
//! FOTO_MODEL_DIR, FOTO_ORT_DLL und FOTO_EXIFTOOL.
use foto_importer::core::{collection::Collection, faces, jobs::Control};
use std::path::Path;

#[test]
#[ignore = "Requires a private collection copy, local models and ONNX Runtime"]
fn small_but_clear_faces_get_name_features_from_the_larger_image() -> anyhow::Result<()> {
    let var = |name: &str| std::env::var(name).map_err(|_| anyhow::anyhow!("{name} fehlt"));
    let mut c = Collection::open(Path::new(&var("FOTO_REAL_ROOT")?), true)?;
    let count = |c: &Collection| -> anyhow::Result<i64> {
        Ok(c.db.query_row(
            "SELECT count(*) FROM faces WHERE retained=0 AND person_id IS NULL AND ignored=0 AND quality>=0.5",
            [],
            |r| r.get(0),
        )?)
    };
    let before = count(&c)?;
    let report = faces::analyze(
        &mut c,
        Path::new(&var("FOTO_MODEL_DIR")?),
        Path::new(&var("FOTO_ORT_DLL")?),
        "yunet",
        "sface",
        &var("FOTO_EXIFTOOL")?,
        &Control::default(),
    )?;
    let after = count(&c)?;
    println!("{report}\nbenennbare unbekannte Gesichter: {before} -> {after}");
    assert!(after >= before);
    Ok(())
}

#[test]
#[ignore = "Requires a private collection copy, local models and ONNX Runtime"]
fn portion_analyses_the_newest_unanalysed_photos_first() -> anyhow::Result<()> {
    use foto_importer::core::scope::{self, Scope};
    let var = |name: &str| std::env::var(name).map_err(|_| anyhow::anyhow!("{name} fehlt"));
    let mut c = Collection::open(Path::new(&var("FOTO_REAL_ROOT")?), true)?;
    let order = scope::analysis_order(&c, &Scope::default())?;
    anyhow::ensure!(order.len() >= 30, "zu wenige Fotos");
    let newest: Vec<i64> = order[..30].to_vec();
    for photo in &newest {
        c.db.execute("DELETE FROM face_jobs WHERE photo_id=?", [photo])?;
    }
    let report = faces::analyze_scoped(
        &mut c,
        Path::new(&var("FOTO_MODEL_DIR")?),
        Path::new(&var("FOTO_ORT_DLL")?),
        "yunet",
        "sface",
        &var("FOTO_EXIFTOOL")?,
        &Control::default(),
        &Scope::new(Some(10), None),
    )?;
    println!("{report}");
    let analysed = |photo: &i64| -> anyhow::Result<bool> {
        Ok(c.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM face_jobs WHERE photo_id=? AND stage='detect' AND status='done')",
            [photo],
            |r| r.get(0),
        )?)
    };
    for (index, photo) in newest.iter().enumerate() {
        assert_eq!(analysed(photo)?, index < 10, "Foto {index} der neuesten");
    }
    assert!(report.contains("10 davon neu analysiert"), "{report}");
    Ok(())
}
