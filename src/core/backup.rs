//! Datensicherung der App-Daten (nicht der Fotos): Sammlungsdatenbank, zentraler
//! Personenbestand und Einstellungen als ZIP `.myimporter.backup.NNN` im Sammlungsordner.
//!
//! Datenbanken werden über die SQLite-Online-Sicherung in einer festen Lesetransaktion kopiert,
//! damit die Kopie auch dann in sich stimmig ist, wenn die App währenddessen schreibt. Alles
//! entsteht zuerst unter einem Zwischennamen im Sammlungsordner; erst das fertige ZIP erhält
//! den endgültigen Namen, und zwar ohne je eine vorhandene Datei zu ersetzen. Abbruch und
//! Fehler räumen die Zwischenstände weg.
use super::jobs::Control;
use crate::config;
use anyhow::{Context, Result, bail};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

/// Arbeitsplatzdateien, die mitgesichert werden (fehlende stehen im Manifest unter `skipped`).
#[derive(Clone, Debug, Default)]
pub struct AppData {
    pub settings: Option<PathBuf>,
    pub central: Option<PathBuf>,
}

/// Was gesichert wird und wie viele Sicherungen liegen bleiben.
#[derive(Clone, Debug)]
pub struct Plan {
    pub app: AppData,
    pub keep: usize,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            app: AppData::default(),
            keep: config::BACKUP_KEEP,
        }
    }
}

/// Plan für automatische Sicherungen (vor Migrationen und vor dem Entfernen fehlender
/// Einträge). Die Oberfläche setzt ihn beim Start und bei jeder Einstellungsänderung; ohne
/// ihn (z. B. in Tests) wird nur die Sammlungsdatenbank gesichert.
static AUTOMATIC: Mutex<Option<Plan>> = Mutex::new(None);

pub fn configure(plan: Plan) {
    if let Ok(mut current) = AUTOMATIC.lock() {
        *current = Some(plan);
    }
}

fn configured() -> Plan {
    AUTOMATIC
        .lock()
        .ok()
        .and_then(|plan| plan.clone())
        .unwrap_or_default()
}

/// Eine vorhandene Sicherung im Sammlungsordner.
#[derive(Clone, Debug, PartialEq)]
pub struct BackupFile {
    pub number: u32,
    pub path: PathBuf,
    pub bytes: u64,
    pub modified: Option<SystemTime>,
}

/// Ergebnis einer neuen Sicherung.
#[derive(Clone, Debug)]
pub struct Created {
    pub number: u32,
    pub path: PathBuf,
    pub bytes: u64,
    /// Nummern der beim Aufräumen gelöschten alten Sicherungen.
    pub removed: Vec<u32>,
    pub elapsed: Duration,
}

impl Created {
    pub fn summary(&self) -> String {
        let mut text = format!(
            "Sicherung {} erstellt ({})",
            file_name(self.number)
                .trim_start_matches(config::BACKUP_PREFIX)
                .to_owned(),
            megabytes(self.bytes)
        );
        match self.removed.len() {
            0 => {}
            1 => text.push_str(" · 1 alte Sicherung gelöscht"),
            n => text.push_str(&format!(" · {n} alte Sicherungen gelöscht")),
        }
        text
    }
}

pub fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

/// Nummer einer Sicherungsdatei, nur bei genau dem Muster `.myimporter.backup.` plus
/// mindestens drei Ziffern.
pub fn number_of(name: &str) -> Option<u32> {
    let digits = name.strip_prefix(config::BACKUP_PREFIX)?;
    if digits.len() < config::BACKUP_DIGITS || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

pub fn file_name(number: u32) -> String {
    format!(
        "{}{:0width$}",
        config::BACKUP_PREFIX,
        number,
        width = config::BACKUP_DIGITS
    )
}

/// Vorhandene Sicherungen, nach Nummer aufsteigend. Nur gewöhnliche Dateien mit genau dem
/// Namensmuster zählen; Verknüpfungen, Ordner und Zwischenstände nicht.
pub fn list(root: &Path) -> Result<Vec<BackupFile>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).context("Sammlungsordner nicht lesbar")? {
        let entry = entry?;
        let Some(number) = entry.file_name().to_str().and_then(number_of) else {
            continue;
        };
        let meta = entry.path().symlink_metadata()?;
        if !meta.file_type().is_file() {
            continue;
        }
        found.push(BackupFile {
            number,
            path: entry.path(),
            bytes: meta.len(),
            modified: meta.modified().ok(),
        });
    }
    found.sort_by_key(|b| b.number);
    Ok(found)
}

/// Automatische Sicherung vor einem riskanten Schritt mit dem eingestellten Plan. Scheitert
/// sie, darf der Schritt nicht stattfinden; der Aufrufer bricht ab.
pub fn automatic(root: &Path, reason: &str, control: &Control) -> Result<Created> {
    create(root, &configured(), reason, control)
        .context("Automatische Datensicherung fehlgeschlagen")
}

enum Kind {
    Sqlite,
    File,
}

struct Source {
    entry: &'static str,
    label: &'static str,
    path: PathBuf,
    kind: Kind,
}

/// Erstellt eine neue Sicherung und löscht danach die ältesten über `plan.keep` hinaus.
pub fn create(root: &Path, plan: &Plan, reason: &str, control: &Control) -> Result<Created> {
    let started = Instant::now();
    let result = create_inner(root, plan, reason, control, started);
    match &result {
        Ok(created) => log::info!(
            target: "sicherung",
            "erstellt nr={} grund={reason:?} bytes={} dauer_ms={:.1} geloescht={:?}",
            created.number,
            created.bytes,
            super::diag::millis(created.elapsed),
            created.removed
        ),
        Err(e) => log::warn!(
            target: "sicherung",
            "fehlgeschlagen grund={reason:?} dauer_ms={:.1} fehler={:?}",
            super::diag::millis(started.elapsed()),
            format!("{e:#}")
        ),
    }
    result
}

fn create_inner(
    root: &Path,
    plan: &Plan,
    reason: &str,
    control: &Control,
    started: Instant,
) -> Result<Created> {
    let root = root
        .canonicalize()
        .context("Sammlungsordner ist nicht erreichbar")?;
    let database = root.join(".myfotomanager");
    if !database.is_file() {
        bail!("Im Sammlungsordner liegt keine Sammlungsdatenbank");
    }
    remove_stale_temps(&root);
    let mut sources = vec![Source {
        entry: "sammlung/.myfotomanager",
        label: "Sammlungsdatenbank",
        path: database,
        kind: Kind::Sqlite,
    }];
    let mut skipped = Vec::new();
    for (path, entry, label, kind) in [
        (
            &plan.app.central,
            "arbeitsplatz/central.sqlite",
            "Zentraler Personenbestand",
            Kind::Sqlite,
        ),
        (
            &plan.app.settings,
            "arbeitsplatz/settings.json",
            "Einstellungen",
            Kind::File,
        ),
    ] {
        let Some(path) = path else { continue };
        if path.is_file() {
            sources.push(Source {
                entry,
                label,
                path: path.clone(),
                kind,
            });
        } else {
            skipped.push(serde_json::json!({
                "name": entry,
                "source": path.display().to_string(),
                "reason": "nicht vorhanden",
            }));
        }
    }
    // Platzbedarf: Zwischenkopien plus ZIP (höchstens etwa so groß wie die Kopien).
    let raw: u64 = sources
        .iter()
        .map(|s| size(&s.path) + size(&with_suffix(&s.path, "-wal")))
        .sum();
    let needed = raw * 2 + config::BACKUP_SPACE_MARGIN;
    if let Some(free) = free_space(&root)
        && free < needed
    {
        bail!(
            "Zu wenig freier Speicher im Sammlungsordner: benötigt etwa {}, frei {}. Es wurde nichts geschrieben.",
            megabytes(needed),
            megabytes(free)
        );
    }
    let staging = tempfile::Builder::new()
        .prefix(config::BACKUP_TEMP_PREFIX)
        .tempdir_in(&root)
        .context("Zwischenordner der Sicherung nicht anlegbar")?;
    let mut staged = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        control.checkpoint()?;
        let target = staging.path().join(format!("{index}"));
        let schema = match source.kind {
            Kind::Sqlite => {
                control.phase(&format!("Sicherung · {} kopieren", source.label), "MB");
                Some(
                    copy_database(&source.path, &target, control)
                        .with_context(|| format!("{} nicht kopierbar", source.label))?,
                )
            }
            Kind::File => {
                fs::copy(&source.path, &target)
                    .with_context(|| format!("{} nicht kopierbar", source.label))?;
                None
            }
        };
        staged.push((source, target, schema));
    }
    control.phase("Sicherung · Packen", "MB");
    let total: u64 = staged.iter().map(|(_, path, _)| size(path)).sum();
    let temp = tempfile::Builder::new()
        .prefix(config::BACKUP_TEMP_PREFIX)
        .suffix(".zip")
        .tempfile_in(&root)
        .context("Zwischendatei der Sicherung nicht anlegbar")?;
    let mut zip = zip::ZipWriter::new(BufWriter::with_capacity(1 << 20, temp));
    let mut entries = Vec::new();
    let mut done = 0u64;
    let mut buffer = vec![0u8; config::COPY_CHUNK];
    for (source, path, schema) in &staged {
        let bytes = size(path);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(config::BACKUP_COMPRESSION_LEVEL))
            .large_file(bytes >= u32::MAX as u64);
        zip.start_file(source.entry, options)?;
        let mut input = File::open(path)?;
        let mut hasher = Sha256::new();
        loop {
            control.checkpoint()?;
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            zip.write_all(&buffer[..read])?;
            done += read as u64;
            control.progress(mb(done), mb(total).max(1));
        }
        let mut entry = serde_json::json!({
            "name": source.entry,
            "source": source.path.display().to_string(),
            "kind": match source.kind { Kind::Sqlite => "sqlite", Kind::File => "file" },
            "bytes": bytes,
            "sha256": hex(&hasher.finalize()),
        });
        if let Some(schema) = schema {
            entry["schema_version"] = (*schema).into();
        }
        entries.push(entry);
    }
    let manifest = serde_json::json!({
        "format": 1,
        "app": "foto-importer",
        "version": env!("CARGO_PKG_VERSION"),
        "build": format!("{} {}", env!("FOTO_IMPORTER_GIT_BRANCH"), env!("FOTO_IMPORTER_GIT_COMMIT")),
        "created_at": chrono::Local::now().to_rfc3339(),
        "reason": reason,
        "collection": root.display().to_string(),
        "entries": entries,
        "skipped": skipped,
    });
    zip.start_file(
        "manifest.json",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    let temp = zip
        .finish()?
        .into_inner()
        .map_err(|e| anyhow::anyhow!("Sicherung nicht vollständig geschrieben: {}", e.error()))?;
    temp.as_file().sync_all()?;
    drop(staging);
    control.checkpoint()?;
    let bytes = temp.as_file().metadata()?.len();
    let (number, path) = persist(&root, temp)?;
    control.phase("Sicherung · Alte Sicherungen aufräumen", "");
    let removed = prune(&root, plan.keep, number);
    Ok(Created {
        number,
        path,
        bytes,
        removed,
        elapsed: started.elapsed(),
    })
}

/// Gibt der fertigen Zwischendatei die nächste freie Nummer, ohne je eine vorhandene Datei zu
/// ersetzen. Kommt eine gleichzeitige Sicherung zuvor, wird die folgende Nummer versucht.
fn persist(root: &Path, mut temp: tempfile::NamedTempFile) -> Result<(u32, PathBuf)> {
    for _ in 0..16 {
        let number = list(root)?.last().map_or(1, |b| b.number + 1);
        let path = root.join(file_name(number));
        match temp.persist_noclobber(&path) {
            Ok(_) => return Ok((number, path)),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists || path.exists() => {
                temp = e.file;
            }
            Err(e) => return Err(e.error).context("Sicherung nicht umbenennbar"),
        }
    }
    bail!("Keine freie Sicherungsnummer gefunden")
}

/// Löscht die ältesten Sicherungen über `keep` hinaus; die neue bleibt immer. Nur Dateien mit
/// genau dem Namensmuster werden angefasst. Fehler beim Löschen stehen im Log.
fn prune(root: &Path, keep: usize, newest: u32) -> Vec<u32> {
    let keep = keep.max(1);
    let Ok(found) = list(root) else {
        return Vec::new();
    };
    let excess = found.len().saturating_sub(keep);
    let mut removed = Vec::new();
    for old in found.iter().filter(|b| b.number != newest).take(excess) {
        match fs::remove_file(&old.path) {
            Ok(()) => removed.push(old.number),
            Err(e) => log::warn!(
                target: "sicherung",
                "alte_sicherung_nicht_geloescht nr={} fehler={:?}",
                old.number,
                e.to_string()
            ),
        }
    }
    if !removed.is_empty() {
        log::info!(target: "sicherung", "aufgeraeumt geloescht={removed:?} behalten={keep}");
    }
    removed
}

/// Kopiert eine SQLite-Datenbank über die Online-Sicherung in eine eigenständige Datei
/// (Journalmodus DELETE, ohne `-wal`). Die Lesetransaktion bleibt über alle Schritte offen,
/// damit Schreiber anderer Verbindungen die Kopie weder verändern noch neu starten.
fn copy_database(source: &Path, target: &Path, control: &Control) -> Result<i64> {
    let src = Connection::open_with_flags(
        source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    src.busy_timeout(Duration::from_secs(10))?;
    src.execute_batch("BEGIN")?;
    let schema: i64 = src.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    src.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })?;
    let page: i64 = src.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let page = page.max(512) as u64;
    let mut dst = Connection::open(target)?;
    {
        let backup = Backup::new(&src, &mut dst)?;
        loop {
            control.checkpoint()?;
            match backup.step(config::BACKUP_STEP_PAGES)? {
                StepResult::Done => break,
                StepResult::More => {}
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
            let progress = backup.progress();
            let all = progress.pagecount.max(0) as u64;
            let left = progress.remaining.max(0) as u64;
            control.progress(mb((all - left.min(all)) * page), mb(all * page).max(1));
        }
    }
    src.execute_batch("COMMIT")?;
    dst.query_row("PRAGMA journal_mode=DELETE", [], |_| Ok(()))?;
    dst.close().map_err(|(_, e)| e)?;
    Ok(schema)
}

fn mb(bytes: u64) -> usize {
    (bytes / (1024 * 1024)) as usize
}

fn size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Entfernt Zwischenstände abgestürzter Läufe (älter als `BACKUP_STALE_TEMP`); jüngere
/// können zu einer gleichzeitig laufenden Sicherung gehören.
fn remove_stale_temps(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name
            .to_str()
            .is_some_and(|n| n.starts_with(config::BACKUP_TEMP_PREFIX))
        {
            continue;
        }
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        let old = meta
            .modified()
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age > config::BACKUP_STALE_TEMP);
        if !old {
            continue;
        }
        let removed = if meta.file_type().is_dir() {
            fs::remove_dir_all(entry.path())
        } else if meta.file_type().is_file() {
            fs::remove_file(entry.path())
        } else {
            continue;
        };
        if let Err(e) = removed {
            log::warn!(target: "sicherung", "zwischenstand_nicht_entfernt fehler={:?}", e.to_string());
        }
    }
}

/// Freier Speicher für den aktuellen Benutzer auf dem Laufwerk des Ordners; `None`, wenn das
/// System ihn nicht nennt (dann wird ohne Vorprüfung gesichert).
fn free_space(folder: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(
                directory: *const u16,
                available: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let wide: Vec<u16> = folder.as_os_str().encode_wide().chain([0]).collect();
        let mut available = 0u64;
        // SAFETY: nullterminierter Pfad; Ausgabezeiger zeigen auf gültige u64, die anderen
        // dürfen laut Win32-Dokumentation null sein.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(available)
    }
    #[cfg(not(windows))]
    {
        let _ = folder;
        None
    }
}
