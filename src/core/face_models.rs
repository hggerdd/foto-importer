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

#[derive(Deserialize)]
struct Download {
    #[serde(flatten)]
    model: Manifest,
    size: usize,
    download_url: String,
}

pub fn install(
    destination: &Path,
    candidates: &[PathBuf],
    control: &Control,
) -> Result<(PathBuf, Vec<Manifest>)> {
    let mut client = None;
    install_with(destination, candidates, control, |url| {
        let client = match &mut client {
            Some(client) => client,
            slot @ None => slot.insert(
                reqwest::blocking::Client::builder()
                    .connect_timeout(Duration::from_secs(10))
                    .timeout(Duration::from_secs(180))
                    .https_only(true)
                    .redirect(reqwest::redirect::Policy::none())
                    .build()?,
            ),
        };
        Ok(Box::new(client.get(url).send()?.error_for_status()?))
    })
}

fn install_with(
    destination: &Path,
    candidates: &[PathBuf],
    control: &Control,
    mut download: impl FnMut(&str) -> Result<Box<dyn Read>>,
) -> Result<(PathBuf, Vec<Manifest>)> {
    let specs = DEFAULTS
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
            None => download(&spec.download_url)
                .with_context(|| format!("{} konnte nicht geladen werden", spec.model.name))?,
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
        let result = install_with(destination.path(), &[], &Control::default(), |_| {
            Ok(Box::new(&b"incomplete"[..]))
        });
        assert!(format!("{:#}", result.expect_err("truncated download")).contains("unvollständig"));
        assert_eq!(fs::read_dir(destination.path())?.count(), 1);
        assert!(previous.is_dir());
        Ok(())
    }
}
