use super::{inference::Manifest, jobs::Control};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const DEFAULTS: &[(&str, &str)] = &[
    ("yunet.json", include_str!("../../models/yunet.json")),
    ("sface.json", include_str!("../../models/sface.json")),
];
/// Genaueres Vergleichsmodell (`config::STRONG_EMBEDDER`), auf Wunsch zusätzlich.
const STRONG: (&str, &str) = (
    "arcface_buffalo_l.json",
    include_str!("../../models/arcface_buffalo_l.json"),
);

#[derive(Deserialize)]
struct Download {
    #[serde(flatten)]
    model: Manifest,
    size: usize,
    download_url: String,
    /// Datei im ZIP-Archiv, falls `download_url` ein Archiv ist.
    #[serde(default)]
    archive_member: Option<String>,
}

/// Modellschlüssel des mitgelieferten Manifests des genaueren Modells (ohne Modellordner).
pub fn strong_key() -> Result<String> {
    Ok(serde_json::from_str::<Manifest>(STRONG.1)?.key())
}

/// Installiert YuNet und SFace in einen neuen Ordner unter `destination`.
pub fn install(
    destination: &Path,
    candidates: &[PathBuf],
    control: &Control,
) -> Result<(PathBuf, Vec<Manifest>)> {
    install_models(destination, candidates, control, false)
}

/// Wie [`install`], mit dem genaueren Modell (ArcFace aus InsightFace buffalo_l, nur nicht
/// kommerziell). Fehlt das Gewicht lokal, wird das Archiv (rund 280 MB) gestreamt und nur
/// `w600k_r50.onnx` (174 MB) gespeichert.
pub fn install_models(
    destination: &Path,
    candidates: &[PathBuf],
    control: &Control,
    strong: bool,
) -> Result<(PathBuf, Vec<Manifest>)> {
    let mut client = None;
    let mut specs = DEFAULTS.to_vec();
    if strong {
        specs.push(STRONG);
    }
    install_with(destination, candidates, control, &specs, |url| {
        let client = match &mut client {
            Some(client) => client,
            slot @ None => slot.insert(
                reqwest::blocking::Client::builder()
                    .connect_timeout(Duration::from_secs(10))
                    .timeout(Duration::from_secs(1800))
                    .https_only(true)
                    // GitHub-Releases leiten auf ihren Speicher weiter; nur diese Ziele.
                    .redirect(reqwest::redirect::Policy::custom(|attempt| {
                        let allowed = matches!(
                            attempt.url().host_str(),
                            Some(
                                "github.com"
                                    | "release-assets.githubusercontent.com"
                                    | "objects.githubusercontent.com"
                            )
                        );
                        if attempt.previous().len() < 5
                            && attempt.url().scheme() == "https"
                            && allowed
                        {
                            attempt.follow()
                        } else {
                            attempt.error("Unerwartete Weiterleitung beim Modelldownload")
                        }
                    }))
                    .build()?,
            ),
        };
        Ok(Box::new(client.get(url).send()?.error_for_status()?))
    })
}

/// Ein Datenstrom, der beim Abbruch des Auftrags mit einem Fehler endet.
struct Cancellable {
    inner: Box<dyn Read>,
    control: Control,
}
impl Read for Cancellable {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self
            .control
            .cancel
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err(std::io::Error::other("Auftrag abgebrochen"));
        }
        self.inner.read(buffer)
    }
}

/// Liest aus einem ZIP-Strom nur die Datei `member` (ohne das Archiv zu speichern).
fn archive_member(mut stream: Box<dyn Read>, member: &str) -> Result<Box<dyn Read>> {
    loop {
        let Some(mut file) = zip::read::read_zipfile_from_stream(&mut stream)? else {
            anyhow::bail!("{member} fehlt im Archiv");
        };
        if file.name() == member {
            // Die Datei wird vollständig gelesen; die Prüfsumme folgt danach.
            let mut bytes = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut bytes)?;
            return Ok(Box::new(std::io::Cursor::new(bytes)));
        }
        std::io::copy(&mut file, &mut std::io::sink())?;
    }
}

fn install_with(
    destination: &Path,
    candidates: &[PathBuf],
    control: &Control,
    models: &[(&str, &str)],
    mut download: impl FnMut(&str) -> Result<Box<dyn Read>>,
) -> Result<(PathBuf, Vec<Manifest>)> {
    let specs = models
        .iter()
        .map(|(name, json)| Ok((*name, *json, serde_json::from_str::<Download>(json)?)))
        .collect::<Result<Vec<_>>>()?;
    control.checkpoint()?;
    fs::create_dir_all(destination)?;
    let stage = tempfile::Builder::new()
        .prefix("opencv-")
        .tempdir_in(destination)?;
    fs::create_dir(stage.path().join("weights"))?;
    let total = specs.iter().map(|(_, _, spec)| spec.size).sum();
    let mut done = 0;
    control.progress(done, total);
    let mut models = Vec::new();
    let mut buffer = vec![0; crate::config::COPY_CHUNK];
    for (name, json, spec) in specs {
        control.checkpoint()?;
        let local = candidates.iter().find_map(|folder| {
            spec.model
                .verify(folder)
                .ok()
                .and_then(|path| File::open(path).ok())
        });
        control.checkpoint()?;
        let mut source: Box<dyn Read> = match local {
            Some(file) => Box::new(file),
            None => {
                let stream = download(&spec.download_url)
                    .with_context(|| format!("{} konnte nicht geladen werden", spec.model.name))?;
                match &spec.archive_member {
                    Some(member) => archive_member(
                        Box::new(Cancellable {
                            inner: stream,
                            control: control.clone(),
                        }),
                        member,
                    )
                    .with_context(|| format!("{} konnte nicht entpackt werden", spec.model.name))?,
                    None => stream,
                }
            }
        };
        let mut file = File::create(stage.path().join(&spec.model.file))?;
        let mut bytes = 0;
        loop {
            control.checkpoint()?;
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            bytes += count;
            ensure!(
                bytes <= spec.size,
                "{} ist größer als erwartet",
                spec.model.name
            );
            file.write_all(&buffer[..count])?;
            control.progress(done + bytes, total);
        }
        ensure!(bytes == spec.size, "{} ist unvollständig", spec.model.name);
        file.sync_all()?;
        drop(file);
        spec.model.verify(stage.path())?;
        fs::write(stage.path().join(name), json)?;
        models.push(spec.model);
        done += bytes;
    }
    control.checkpoint()?;
    Ok((stage.keep(), models))
}

/// Cheap availability check for the UI. The worker verifies hashes before loading.
pub fn weights_present(models: &[Manifest], folder: &Path, detector: &str, embedder: &str) -> bool {
    [("detector", detector), ("embedder", embedder)]
        .into_iter()
        .all(|(kind, id)| {
            models
                .iter()
                .find(|model| model.kind == kind && model.id == id)
                .is_some_and(|model| folder.join(&model.file).is_file())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_install_leaves_previous_folder_intact() -> Result<()> {
        let destination = tempfile::tempdir()?;
        let previous = destination.path().join("previous");
        fs::create_dir(&previous)?;
        let result = install_with(
            destination.path(),
            &[],
            &Control::default(),
            DEFAULTS,
            |_| Ok(Box::new(&b"incomplete"[..])),
        );
        assert!(format!("{:#}", result.expect_err("truncated download")).contains("unvollständig"));
        assert_eq!(fs::read_dir(destination.path())?.count(), 1);
        assert!(previous.is_dir());
        Ok(())
    }

    /// Aus einem ZIP-Strom kommt nur die genannte Datei; eine fehlende meldet sich.
    #[test]
    fn archive_member_is_read_from_a_stream() -> Result<()> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("det_10g.onnx", options)?;
            zip.write_all(b"detector")?;
            zip.start_file("w600k_r50.onnx", options)?;
            zip.write_all(b"embedder")?;
            zip.finish()?;
        }
        let bytes = buffer.into_inner();
        let mut member = archive_member(
            Box::new(std::io::Cursor::new(bytes.clone())),
            "w600k_r50.onnx",
        )?;
        let mut text = String::new();
        member.read_to_string(&mut text)?;
        assert_eq!(text, "embedder");
        assert!(archive_member(Box::new(std::io::Cursor::new(bytes)), "fehlt.onnx").is_err());
        Ok(())
    }

    #[test]
    fn strong_manifest_matches_the_configured_model() -> Result<()> {
        let manifest: Manifest = serde_json::from_str(STRONG.1)?;
        assert_eq!(manifest.id, crate::config::STRONG_EMBEDDER);
        assert_eq!((manifest.dim, manifest.input.color.as_str()), (512, "RGB"));
        assert_eq!((manifest.input.mean, manifest.input.std), (127.5, 127.5));
        assert!(strong_key()?.starts_with("arcface_buffalo_l@"));
        Ok(())
    }
}
