use super::{
    collection::Collection,
    files::{self, ImportReport, SourceFile},
    jobs::Control,
    metadata,
};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub enum ImportState {
    New,
    Imported,
    ImportedMissing,
    Likely,
}
#[derive(Debug)]
pub struct ImportCheck {
    pub path: PathBuf,
    pub state: ImportState,
    pub photo: Option<i64>,
}
pub fn quick_hash(path: &Path, control: &Control) -> Result<String> {
    control.checkpoint()?;
    let mut file = File::open(path)?;
    let before = file.metadata()?;
    let mut hash = blake3::Hasher::new();
    hash.update(format!("{}\0", before.len()).as_bytes());
    let mut buffer = vec![0; 65536];
    let n = file.read(&mut buffer)?;
    hash.update(&buffer[..n]);
    file.seek(SeekFrom::Start(before.len().saturating_sub(65536)))?;
    let n = file.read(&mut buffer)?;
    hash.update(&buffer[..n]);
    let after = file.metadata()?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        bail!("Datei während der Duplikatprüfung verändert");
    }
    Ok(hash.finalize().to_hex().to_string())
}
pub fn check(
    collection: &Collection,
    files: &[SourceFile],
    exiftool: &str,
    control: &Control,
) -> Result<Vec<ImportCheck>> {
    let mut results = Vec::new();
    for batch in files.chunks(128) {
        control.checkpoint()?;
        let paths = batch
            .iter()
            .map(|f| f.path.canonicalize())
            .collect::<std::io::Result<Vec<_>>>()?;
        let raw = metadata::read_metadata(&paths, exiftool, control)?;
        let raw: BTreeMap<_, _> = raw
            .into_iter()
            .filter_map(|v| {
                Some((
                    files::fold(&files::normalize(Path::new(v.get("SourceFile")?.as_str()?))),
                    v,
                ))
            })
            .collect();
        for (file, path) in batch.iter().zip(&paths) {
            control.checkpoint()?;
            let raw = raw
                .get(&files::fold(&files::normalize(path)))
                .context("Metadaten der Kartendatei fehlen; Import bleibt zur Prüfung offen")?;
            if raw
                .as_object()
                .is_some_and(|m| m.keys().any(|k| k.rsplit(':').next() == Some("Error")))
            {
                bail!("Metadaten von {} nicht lesbar", path.display());
            }
            let key = metadata::identity(raw);
            let photo: Option<(i64, String)> = if let Some(key) = &key {
                collection
                    .db
                    .query_row(
                        "SELECT id,status FROM photos WHERE identity_key=?",
                        [key],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?
            } else {
                None
            };
            let (state, photo) = if let Some((id, status)) = photo {
                (
                    if status == "present" {
                        ImportState::Imported
                    } else {
                        ImportState::ImportedMissing
                    },
                    Some(id),
                )
            } else {
                let name = file.path.file_name().unwrap_or_default().to_string_lossy();
                let candidates=collection.db.prepare("SELECT f.relpath,f.photo_id FROM files f JOIN photos p ON p.id=f.photo_id WHERE f.name=? AND f.size=? AND f.status='present' AND f.meta_state IN ('pending','stale','error') AND (? IS NULL OR p.identity_key IS NULL) ORDER BY f.id")?.query_map(params![name.as_ref(),file.size as i64,key],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                let mut found = None;
                if !candidates.is_empty() {
                    let hash = quick_hash(path, control)?;
                    for (relative, id) in candidates {
                        if quick_hash(&collection.safe_path(&relative)?, control)? == hash {
                            found = Some(id);
                            break;
                        }
                    }
                }
                (
                    if found.is_some() {
                        ImportState::Likely
                    } else {
                        ImportState::New
                    },
                    found,
                )
            };
            results.push(ImportCheck {
                path: file.path.clone(),
                state,
                photo,
            });
        }
    }
    Ok(results)
}

pub fn execute(
    files: &[SourceFile],
    target: &Path,
    name: &str,
    converter: Option<&Path>,
    skip: bool,
    exiftool: &str,
    control: &Control,
) -> Result<ImportReport> {
    if let Some(converter) = converter
        && !converter.is_file()
    {
        bail!("Adobe DNG Converter nicht gefunden; Pfad in Einstellungen prüfen");
    }
    let mut skipped = Vec::new();
    let mut selected = files.to_vec();
    if skip && target.join(".myfotomanager").is_file() {
        let collection = Collection::open(target, false)?;
        let checks = check(&collection, files, exiftool, control)?;
        skipped = checks
            .into_iter()
            .filter(|c| matches!(c.state, ImportState::Imported | ImportState::Likely))
            .map(|c| c.path)
            .collect();
        selected.retain(|f| !skipped.contains(&f.path));
    }
    if selected.is_empty() {
        return Ok(ImportReport {
            skipped,
            ..Default::default()
        });
    }
    let mut report = files::import(&selected, target, name, converter, control)?;
    report.skipped = skipped;
    if !report.cancelled && !report.copied.is_empty() && target.join(".myfotomanager").is_file() {
        let root = target.canonicalize()?;
        let _slot = control.lane(&format!("writer:{}", files::fold(&files::normalize(&root))))?;
        if let Err(error) =
            Collection::open(&root, true).and_then(|mut c| c.quick_scan(false, control))
        {
            report
                .errors
                .push(format!("Dateien kopiert; Indexaktualisierung: {error}"));
        }
    }
    Ok(report)
}
