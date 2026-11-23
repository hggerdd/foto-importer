//! Invariante „Originale niemals verändern“ (AGENTS.md, ADR Import/Sammlung): Indizieren,
//! Metadatenlauf, Gesichtsanalyse, Vorschauen samt Vorschau-Cache und Gesichtskacheln sowie
//! das Entfernen fehlender Einträge lassen jede Originaldatei byte-gleich (Hash, Größe,
//! Änderungszeit) und legen neben den Originalen nur eigene Dateien der App an.
//!
//! Offline: Der Metadatenlauf nutzt das ExifTool-Ersatzprogramm aus `tests/fixtures`. Die
//! Gesichtsanalyse braucht echte Modelle und ONNX Runtime (`models/`, `runtime/`); fehlen sie,
//! prüft der Test stattdessen den Lesepfad der Analyse (`previews::load`) und meldet das
//! deutlich.
use foto_importer::core::{
    collection::Collection,
    faces,
    jobs::Control,
    metadata, missing,
    previews::{self, PreviewRequest},
    thumb_cache,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Inhalt, Größe und Änderungszeit einer Datei.
#[derive(Debug, PartialEq)]
struct Fingerprint {
    sha256: Vec<u8>,
    size: u64,
    modified: SystemTime,
}

fn fingerprint(path: &Path) -> anyhow::Result<Fingerprint> {
    let bytes = fs::read(path)?;
    let meta = fs::metadata(path)?;
    Ok(Fingerprint {
        sha256: Sha256::digest(&bytes).to_vec(),
        size: meta.len(),
        modified: meta.modified()?,
    })
}

/// Eigene Dateien der App im Sammlungsordner: Datenbankordner und Sicherungen.
fn app_owned(relative: &str) -> bool {
    relative.starts_with(".myfotomanager") || relative.starts_with(".myimporter.backup.")
}

/// Alle Dateien unter `root` außer den eigenen der App, mit Fingerabdruck.
fn originals(root: &Path) -> anyhow::Result<BTreeMap<String, Fingerprint>> {
    let mut found = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            if app_owned(&relative) {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                found.insert(relative, fingerprint(&path)?);
            }
        }
    }
    Ok(found)
}

fn exiftool_fixture() -> anyhow::Result<PathBuf> {
    let folder = tempfile::tempdir()?.keep();
    let exe = folder.join(if cfg!(windows) {
        "exiftool-fixture.exe"
    } else {
        "exiftool-fixture"
    });
    let status = std::process::Command::new("rustc")
        .arg("tests/fixtures/exiftool.rs")
        .arg("-o")
        .arg(&exe)
        .status()?;
    anyhow::ensure!(status.success(), "ExifTool-Ersatz nicht gebaut");
    Ok(exe)
}

/// Beispielbilder: JPEG und PNG mit Verlauf, verteilt auf Tagesordner; dazu ein Ordner, der
/// später „verschwindet“ (für das Entfernen fehlender Einträge).
fn sample_collection(root: &Path) -> anyhow::Result<()> {
    // Klein gehalten: Vorschauen und Analyse skalieren im Debug-Build spürbar langsam.
    let gradient = image::RgbImage::from_fn(320, 240, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    for (dir, name) in [
        ("2024-05-01 Ausflug", "IMG_0001.jpg"),
        ("2025-01-10", "IMG_0002.png"),
        ("Weg", "IMG_0900.jpg"),
    ] {
        let folder = root.join(dir);
        fs::create_dir_all(&folder)?;
        gradient.save(folder.join(name))?;
    }
    Ok(())
}

#[test]
fn processing_never_changes_or_removes_originals() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("Sammlung");
    fs::create_dir_all(&root)?;
    sample_collection(&root)?;
    // Änderungszeiten in der Vergangenheit, damit ein versehentliches Schreiben sicher auffällt.
    let past = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    for relative in originals(&root)?.keys() {
        fs::File::options()
            .write(true)
            .open(root.join(relative))?
            .set_modified(past)?;
    }
    let before = originals(&root)?;
    assert_eq!(before.len(), 3);
    let control = Control::default();

    // Indizieren.
    let mut c = Collection::open(&root, true)?;
    c.quick_scan(false, &control)?;
    assert_eq!(c.status()?.files, 3);

    // Metadatenlauf (ExifTool-Ersatz, liest nur).
    let exiftool = exiftool_fixture()?;
    let exiftool = exiftool.to_string_lossy();
    metadata::enrich(&mut c, &exiftool, &control)?;
    let photo: i64 =
        c.db.query_row("SELECT MIN(photo_id) FROM files", [], |r| r.get(0))?;
    metadata::enrich_photo(&mut c, &exiftool, &control, photo)?;
    assert_eq!(originals(&root)?, before, "nach dem Metadatenlauf");

    // Gesichtsanalyse: echt, wenn Modelle und Laufzeit da sind, sonst ihr Lesepfad.
    let (models, runtime) = (Path::new("models"), Path::new("runtime/onnxruntime.dll"));
    let analysed = runtime.is_file()
        && faces::analyze(
            &mut c, models, runtime, "yunet", "sface", &exiftool, &control,
        )
        .inspect_err(|e| eprintln!("Gesichtsanalyse nicht ausführbar: {e:#}"))
        .is_ok();
    if !analysed {
        eprintln!(
            "HINWEIS: keine Modelle/ONNX Runtime (models/, runtime/onnxruntime.dll); geprüft wird \
             nur der Lesepfad der Analyse (previews::load)."
        );
        for relative in before.keys() {
            previews::load(&root.join(relative), &exiftool, 1, 1600, &control)?;
        }
    }
    assert_eq!(originals(&root)?, before, "nach der Gesichtsanalyse");

    // Genauer berechnen: liest die Analysequellen erneut (Ersatz-Embedder ohne Modelldatei).
    let source: String = c.db.query_row(
        "SELECT relpath FROM files WHERE ext='jpg' LIMIT 1",
        [],
        |r| r.get(0),
    )?;
    c.db.execute(
        "INSERT INTO faces(photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality)
         SELECT photo_id,relpath,'det',0.2,0.2,0.6,0.7,'[[100,90],[135,90],[118,110],[103,131],[132,131]]',0.95,0.9 FROM files WHERE relpath=?",
        [&source],
    )?;
    let computed = foto_importer::core::strong::compute_with(
        &mut c,
        &exiftool,
        &control,
        |_| Ok(vec![1.0, 0.0]),
        "arcface_buffalo_l@test",
    )?;
    assert!(
        computed.contains("1 Gesichter genauer berechnet"),
        "{computed}"
    );
    assert_eq!(originals(&root)?, before, "nach der genaueren Berechnung");

    // Vorschauen: lokaler Vorschau-Cache (Fehlschlag, dann Treffer) und Gesichtskacheln.
    let cache = temp.path().join("cache");
    thumb_cache::configure(Some(cache.clone()));
    for relative in before.keys() {
        for edge in foto_importer::config::THUMB_CACHE_EDGES {
            for _ in 0..2 {
                previews::cached_preview(
                    PreviewRequest {
                        root: Some(root.clone()),
                        path: root.join(relative),
                        orientation: 1,
                        crop: None,
                        edge: *edge,
                        generation: 0,
                    },
                    &exiftool,
                    &control,
                )?;
            }
        }
        for _ in 0..2 {
            previews::cached_preview(
                PreviewRequest {
                    root: Some(root.clone()),
                    path: root.join(relative),
                    orientation: 6,
                    crop: Some([0.2, 0.2, 0.5, 0.6]),
                    edge: 128,
                    generation: 0,
                },
                &exiftool,
                &control,
            )?;
        }
    }
    thumb_cache::configure(None);
    assert!(
        fs::read_dir(&cache)?.next().is_some(),
        "Vorschau-Cache wurde benutzt"
    );
    assert_eq!(originals(&root)?, before, "nach den Vorschauen");

    // Fehlende Einträge entfernen: nur der verschwundene Ordner fehlt, alles andere bleibt.
    fs::remove_dir_all(root.join("Weg"))?;
    let mut expected = before;
    expected.retain(|relative, _| !relative.starts_with("Weg/"));
    c.quick_scan(true, &control)?;
    assert_eq!(c.status()?.missing, 1);
    missing::remove(&mut c, &["Weg".into()], &control)?;
    assert_eq!(c.status()?.missing, 0);
    assert_eq!(
        originals(&root)?,
        expected,
        "nach dem Entfernen fehlender Einträge"
    );
    Ok(())
}
