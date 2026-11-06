use super::jobs::Control;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const PACKAGE: &str = "onnxruntime-win-x64-1.22.0";
const URL: &str = "https://github.com/microsoft/onnxruntime/releases/download/v1.22.0/onnxruntime-win-x64-1.22.0.zip";
const SHA256: &str = "174c616efc0271194488642a72f1a514e01487da4dfe84c49296d66e40ebe0da";
const DOWNLOAD_BYTES: usize = 72_368_545;
const DLL: &str = "onnxruntime.dll";
const FILES: &[(&str, &str)] = &[
    ("lib/onnxruntime.dll", DLL),
    (
        "lib/onnxruntime_providers_shared.dll",
        "onnxruntime_providers_shared.dll",
    ),
    ("LICENSE", "LICENSE"),
    ("ThirdPartyNotices.txt", "ThirdPartyNotices.txt"),
];

pub const SUPPORTED: bool = cfg!(all(target_os = "windows", target_arch = "x86_64"));

pub fn download(destination: &Path, control: &Control) -> Result<PathBuf> {
    ensure!(
        SUPPORTED,
        "Der Runtime-Download ist für Windows x64 verfügbar."
    );
    control.checkpoint()?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() < 5
                && url.scheme() == "https"
                && matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.follow()
            } else {
                attempt.error("Unerwartete Weiterleitung beim ONNX-Download")
            }
        }))
        .build()?;
    let response = client
        .get(URL)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .context("Microsoft-Paket konnte nicht geladen werden")?;
    install(response, destination, DOWNLOAD_BYTES, SHA256, control)
}

fn install(
    mut source: impl Read,
    destination: &Path,
    expected_bytes: usize,
    expected_hash: &str,
    control: &Control,
) -> Result<PathBuf> {
    let mut archive = tempfile::tempfile()?;
    let mut digest = Sha256::new();
    let mut bytes = 0;
    let mut buffer = vec![0; crate::config::COPY_CHUNK];
    control.progress(0, expected_bytes);
    loop {
        control.checkpoint()?;
        let count = source.read(&mut buffer).context("Download unterbrochen")?;
        if count == 0 {
            break;
        }
        bytes += count;
        ensure!(
            bytes <= expected_bytes,
            "ONNX-Paket ist größer als erwartet"
        );
        digest.update(&buffer[..count]);
        archive.write_all(&buffer[..count])?;
        control.progress(bytes, expected_bytes);
    }
    ensure!(
        bytes == expected_bytes && format!("{:x}", digest.finalize()) == expected_hash,
        "ONNX-Paket ist unvollständig oder die SHA-256-Prüfsumme stimmt nicht"
    );
    archive.rewind()?;
    let mut zip = zip::ZipArchive::new(archive).context("ONNX-Paket ist kein gültiges ZIP")?;
    fs::create_dir_all(destination).context("Runtime-Ordner konnte nicht erstellt werden")?;
    let stage = tempfile::Builder::new()
        .prefix(PACKAGE)
        .rand_bytes(10)
        .tempdir_in(destination)?;
    for (member, name) in FILES {
        control.checkpoint()?;
        let mut input = zip
            .by_name(&format!("{PACKAGE}/{member}"))
            .with_context(|| format!("{name} fehlt im ONNX-Paket"))?;
        let mut file = File::create(stage.path().join(name))?;
        std::io::copy(&mut input, &mut file)?;
        file.sync_all()?;
    }
    control.checkpoint()?;
    Ok(stage.keep().join(DLL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn fixture(members: &[(&str, &str)]) -> Result<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (member, name) in members {
            zip.start_file(
                format!("{PACKAGE}/{member}"),
                zip::write::SimpleFileOptions::default(),
            )?;
            zip.write_all(name.as_bytes())?;
        }
        Ok(zip.finish()?.into_inner())
    }

    #[test]
    fn install_selects_only_expected_members_and_keeps_existing_dlls() -> Result<()> {
        let destination = tempfile::tempdir()?;
        let mut members = FILES.to_vec();
        members.push(("../../unwanted.dll", "unexpected"));
        let bytes = fixture(&members)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let install_fixture = || {
            install(
                &bytes[..],
                destination.path(),
                bytes.len(),
                &hash,
                &Control::default(),
            )
        };
        let first = install_fixture()?;
        let second = install_fixture()?;
        assert_ne!(first, second);
        assert_eq!(fs::read(&first)?, DLL.as_bytes());
        let parent = second.parent().context("Kein Runtime-Verzeichnis")?;
        assert_eq!(fs::read_dir(parent)?.count(), FILES.len());
        Ok(())
    }

    #[test]
    fn invalid_or_cancelled_packages_leave_no_runtime() -> Result<()> {
        let destination = tempfile::tempdir()?;
        let bytes = fixture(&FILES[..1])?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let control = Control::default();
        assert!(
            install(
                &bytes[..],
                destination.path(),
                bytes.len(),
                "falscher Hash",
                &control
            )
            .is_err()
        );
        assert!(install(&bytes[..], destination.path(), bytes.len(), &hash, &control).is_err());
        control.stop();
        assert!(install(&bytes[..], destination.path(), bytes.len(), &hash, &control).is_err());
        assert_eq!(fs::read_dir(destination.path())?.count(), 0);
        Ok(())
    }
}
