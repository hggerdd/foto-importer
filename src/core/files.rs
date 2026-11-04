use super::jobs::{Control, command, wait_child};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: PathBuf,
    pub day: String,
    pub size: u64,
}
#[derive(Default, Clone)]
pub struct Scan {
    pub files: Vec<SourceFile>,
    pub warnings: Vec<String>,
}
pub fn extension(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}
pub fn normalize(path: &Path) -> String {
    use unicode_normalization::UnicodeNormalization;
    path.to_string_lossy().replace('\\', "/").nfc().collect()
}
pub fn fold(text: &str) -> String {
    use unicode_casefold::UnicodeCaseFold;
    text.case_fold().collect()
}

pub fn validate_folder(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        bail!("Bitte einen gültigen Ordnernamen ohne Pfadzeichen eingeben.");
    }
    let base = name.split('.').next().unwrap_or_default().to_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
        || (1..=9).any(|n| base == format!("COM{n}") || base == format!("LPT{n}"))
    {
        bail!("Dieser Ordnername ist unter Windows reserviert.");
    }
    Ok(())
}

pub fn exif_day(path: &Path) -> Option<String> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    let data = exif::Reader::new().read_from_container(&mut reader).ok()?;
    for tag in [
        exif::Tag::DateTimeOriginal,
        exif::Tag::DateTimeDigitized,
        exif::Tag::DateTime,
    ] {
        if let Some(field) = data.get_field(tag, exif::In::PRIMARY)
            && let exif::Value::Ascii(values) = &field.value
            && let Some(value) = values.first()
            && let Ok(date) = std::str::from_utf8(value)
            && let Some(day) = parse_date(date)
        {
            return Some(day);
        }
    }
    None
}
pub fn parse_date(value: &str) -> Option<String> {
    let text = value.trim_matches(char::from(0)).trim();
    for (length, format) in [(10, "%Y:%m:%d"), (10, "%Y-%m-%d"), (8, "%Y%m%d")] {
        if let Some(prefix) = text.get(..length)
            && let Ok(date) = NaiveDate::parse_from_str(prefix, format)
        {
            return Some(date.to_string());
        }
    }
    None
}

/// Walk without following symlinks; discovery errors remain visible to callers.
pub fn scan(root: &Path, metadata: bool, control: &Control) -> Result<Scan> {
    let extensions = crate::config::SUPPORTED
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>();
    scan_filtered(root, metadata, &extensions, &[], control)
}
pub fn scan_filtered(
    root: &Path,
    metadata: bool,
    extensions: &[String],
    exclusions: &[String],
    control: &Control,
) -> Result<Scan> {
    let root = root
        .canonicalize()
        .context("Quellordner ist nicht erreichbar")?;
    if !root.is_dir() {
        bail!("Quelle ist kein Ordner");
    }
    let mut scan = Scan::default();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        control.checkpoint()?;
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                scan.warnings.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        for entry in entries {
            control.checkpoint()?;
            let result = (|| -> Result<()> {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_symlink() {
                    return Ok(());
                }
                let path = entry.path();
                let relative = normalize(path.strip_prefix(&root)?);
                if exclusions
                    .iter()
                    .any(|pattern| wildcard(&fold(pattern), &fold(&relative)))
                {
                    return Ok(());
                }
                if kind.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if !name.starts_with('.')
                        && !["$recycle.bin", "system volume information"].contains(&name.as_str())
                    {
                        dirs.push(path);
                    }
                } else if kind.is_file() && extensions.contains(&extension(&path)) {
                    let stat = entry.metadata()?;
                    #[cfg(windows)]
                    let timestamp = stat.created().or_else(|_| stat.modified())?;
                    #[cfg(not(windows))]
                    let timestamp = stat.modified()?;
                    let filesystem = DateTime::<Local>::from(timestamp)
                        .format("%Y-%m-%d")
                        .to_string();
                    let day = if metadata {
                        exif_day(&path).unwrap_or(filesystem)
                    } else {
                        filesystem
                    };
                    scan.files.push(SourceFile {
                        path,
                        day,
                        size: stat.len(),
                    });
                    control.progress(scan.files.len(), 0);
                }
                Ok(())
            })();
            if let Err(e) = result {
                scan.warnings.push(e.to_string());
            }
        }
    }
    scan.files
        .sort_by(|a, b| (&a.day, &a.path).cmp(&(&b.day, &b.path)));
    Ok(scan)
}

fn wildcard(pattern: &str, text: &str) -> bool {
    let (pattern, text) = (
        pattern.chars().collect::<Vec<_>>(),
        text.chars().collect::<Vec<_>>(),
    );
    let (mut p, mut t, mut star, mut checkpoint) = (0, 0, None, 0);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            checkpoint = t;
        } else if let Some(index) = star {
            p = index + 1;
            checkpoint += 1;
            t = checkpoint;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

pub fn counts(files: &[SourceFile]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for file in files {
        *counts.entry(extension(&file.path)).or_default() += 1;
    }
    counts
}

/// Publish only complete files, never replacing an existing destination.
pub fn copy_one(source: &Path, folder: &Path, control: &Control) -> Result<PathBuf> {
    control.checkpoint()?;
    fs::create_dir_all(folder)?;
    let mut input = File::open(source)?;
    let stat = input.metadata()?;
    let mut temporary = tempfile::NamedTempFile::new_in(folder)?;
    let mut buffer = vec![0u8; crate::config::COPY_CHUNK];
    loop {
        control.checkpoint()?;
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        temporary.write_all(&buffer[..n])?;
    }
    let after = input.metadata()?;
    if stat.len() != after.len()
        || stat.modified()? != after.modified()?
        || temporary.as_file().metadata()?.len() != stat.len()
    {
        bail!("Quelldatei wurde während des Kopierens verändert");
    }
    temporary
        .as_file()
        .set_times(fs::FileTimes::new().set_modified(stat.modified()?))?;
    temporary.as_file().sync_all()?;
    control.checkpoint()?;
    let name = source.file_name().context("Dateiname fehlt")?;
    let stem = source.file_stem().unwrap_or(name).to_string_lossy();
    let ext = source
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for index in 0u64.. {
        control.checkpoint()?;
        let candidate = folder.join(if index == 0 {
            name.to_string_lossy().into_owned()
        } else {
            format!("{stem}_{index}{ext}")
        });
        match temporary.persist_noclobber(&candidate) {
            Ok(_) => return Ok(candidate),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => temporary = e.file,
            Err(e) => return Err(e.error.into()),
        }
    }
    unreachable!()
}

#[derive(Default, Debug)]
pub struct ImportReport {
    pub copied: Vec<PathBuf>,
    pub errors: Vec<String>,
    pub converted: usize,
    pub cancelled: bool,
}
pub fn import(
    files: &[SourceFile],
    target: &Path,
    name: &str,
    converter: Option<&Path>,
    control: &Control,
) -> Result<ImportReport> {
    let slot = control.lane("copy")?;
    validate_folder(name)?;
    let target = target
        .canonicalize()
        .context("Zielordner ist nicht erreichbar")?;
    if !target.is_dir() {
        bail!("Ziel ist kein Ordner");
    }
    let destination = target.join(name);
    fs::create_dir_all(&destination)?;
    if !destination.canonicalize()?.starts_with(&target) {
        bail!("Zielordner verweist außerhalb des gewählten Ziels");
    }
    let mut report = ImportReport::default();
    for (index, file) in files.iter().enumerate() {
        if control.checkpoint().is_err() {
            report.cancelled = true;
            break;
        }
        let ext = extension(&file.path);
        let folder = destination.join(if ext.is_empty() { "no_extension" } else { &ext });
        fs::create_dir_all(&folder)?;
        if !folder.canonicalize()?.starts_with(&target) {
            report.errors.push(format!(
                "{}: Zielordner verlässt das gewählte Ziel",
                file.path.display()
            ));
            continue;
        }
        match copy_one(&file.path, &folder, control) {
            Ok(path) => report.copied.push(path),
            Err(e) => report.errors.push(format!("{}: {e}", file.path.display())),
        }
        control.progress(index + 1, files.len());
    }
    drop(slot);
    if let Some(converter) = converter.filter(|_| !report.cancelled) {
        let _dng_slot = control.lane("dng")?;
        for path in &report.copied {
            if extension(path) == "nef" {
                if control.checkpoint().is_err() {
                    report.cancelled = true;
                    break;
                }
                match convert_dng(path, &destination, converter, control) {
                    Ok(_) => report.converted += 1,
                    Err(e) => report.errors.push(format!("DNG {}: {e}", path.display())),
                }
            }
        }
    }
    Ok(report)
}

pub fn convert_dng(
    source: &Path,
    output: &Path,
    executable: &Path,
    control: &Control,
) -> Result<PathBuf> {
    let staging = tempfile::tempdir_in(output)?;
    let log = tempfile::tempfile()?;
    let mut child = command(executable)
        .arg("-d")
        .arg(staging.path())
        .arg(source)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .context("Adobe DNG Converter konnte nicht gestartet werden")?;
    let status = wait_child(&mut child, control, Duration::from_secs(600))?;
    if !status.success() {
        bail!("Converter meldet {status}");
    }
    let generated = fs::read_dir(staging.path())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| extension(p) == "dng")
        .context("Converter hat keine DNG-Datei erzeugt")?;
    if generated.metadata()?.len() == 0 {
        bail!("DNG-Datei ist leer");
    }
    copy_one(&generated, output, control)
}
