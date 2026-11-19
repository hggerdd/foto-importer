//! Diagnose-Log: eine Zeile je Ereignis mit Zeitstempel, Stufe, Bereich und Text, damit sich
//! Ruckeln, langsame Aufträge, Fehler und Abstürze später nachlesen lassen.
//!
//! Aufrufer nutzen die Makros des `log`-Crates mit ausdrücklichem Bereich
//! (`log::info!(target: "job", "…")`) oder [`span`] für Zeiten. Formatiert wird auf dem
//! aufrufenden Thread, geschrieben auf einem eigenen Thread: Die Oberfläche wartet nie auf
//! die Platte. Ist die Warteschlange voll, gehen Zeilen verloren und werden gezählt.

use crate::config;
use std::{
    fmt::{self, Write as _},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const PREFIX: &str = "foto-importer-";
const SUFFIX: &str = ".log";

enum Message {
    Line(String),
    Flush(mpsc::SyncSender<()>),
}

/// Grenzen der Rotation: Größe einer Datei und aller Logdateien zusammen.
#[derive(Clone, Copy)]
pub struct Limits {
    pub file_bytes: u64,
    pub total_bytes: u64,
}
impl Limits {
    pub const APP: Self = Self {
        file_bytes: config::DIAG_FILE_BYTES,
        total_bytes: config::DIAG_TOTAL_BYTES,
    };
}

/// Ein Log mit eigenem Schreib-Thread. Die App nutzt genau eines (siehe [`init`]).
pub struct Diag {
    enabled: AtomicBool,
    sender: mpsc::SyncSender<Message>,
    dropped: AtomicU64,
}
impl Diag {
    /// Startet den Schreib-Thread. Dateien entstehen erst mit der ersten Zeile.
    pub fn start(dir: PathBuf, enabled: bool, limits: Limits) -> Self {
        let (sender, receiver) = mpsc::sync_channel(config::DIAG_QUEUE);
        let mut roller = Roller::new(dir, limits);
        // Ohne Schreib-Thread schlägt jedes Senden fehl; die App läuft ohne Log weiter.
        let _ = std::thread::Builder::new()
            .name("diagnose-log".into())
            .spawn(move || roller.run(&receiver));
        Self {
            enabled: AtomicBool::new(enabled),
            sender,
            dropped: AtomicU64::new(0),
        }
    }
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }
    /// Reiht eine Zeile ein, ohne zu warten.
    pub fn write(&self, level: log::Level, area: &str, message: &fmt::Arguments) {
        if !self.enabled() {
            return;
        }
        let lost = self.dropped.load(Ordering::Relaxed);
        if lost > 0 {
            let note = format_line(
                chrono::Local::now(),
                log::Level::Warn,
                "diag",
                &format_args!("verloren zeilen={lost}"),
            );
            if self.sender.try_send(Message::Line(note)).is_ok() {
                self.dropped.fetch_sub(lost, Ordering::Relaxed);
            }
        }
        let line = format_line(chrono::Local::now(), level, area, message);
        if let Err(mpsc::TrySendError::Full(_)) = self.sender.try_send(Message::Line(line)) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    /// Wartet höchstens `timeout`, bis alles Eingereihte auf der Platte steht.
    pub fn flush(&self, timeout: Duration) -> bool {
        let (ack, done) = mpsc::sync_channel(1);
        self.sender.send(Message::Flush(ack)).is_ok() && done.recv_timeout(timeout).is_ok()
    }
}

/// `2026-11-19 18:25:03.123 INFO  job ende art=faces dauer_ms=73.4`: Zeilenumbrüche im Text
/// (z. B. Backtrace) werden zu „ | “, damit jedes Ereignis genau eine Zeile bleibt.
pub fn format_line(
    time: chrono::DateTime<chrono::Local>,
    level: log::Level,
    area: &str,
    message: &fmt::Arguments,
) -> String {
    let mut line = String::with_capacity(160);
    let _ = write!(
        line,
        "{} {:<5} {} ",
        time.format("%Y-%m-%d %H:%M:%S%.3f"),
        level,
        area
    );
    let start = line.len();
    let _ = write!(line, "{message}");
    if line[start..].contains(['\n', '\r']) {
        let text = line.split_off(start);
        line.push_str(&text.replace("\r\n", " | ").replace(['\n', '\r'], " | "));
    }
    line.push('\n');
    line
}

/// Schreibt Zeilen in Dateien `foto-importer-<Zeitstempel>.log` und beginnt bei
/// `file_bytes` eine neue. Danach werden die ältesten gelöscht, bis alle zusammen höchstens
/// `total_bytes` belegen. Andere Dateien im Ordner bleiben unberührt.
pub struct Roller {
    dir: PathBuf,
    limits: Limits,
    file: Option<io::BufWriter<fs::File>>,
    size: u64,
}
impl Roller {
    pub fn new(dir: PathBuf, limits: Limits) -> Self {
        Self {
            dir,
            limits,
            file: None,
            size: 0,
        }
    }
    fn run(&mut self, receiver: &mpsc::Receiver<Message>) {
        while let Ok(message) = receiver.recv() {
            self.handle(message);
            while let Ok(message) = receiver.try_recv() {
                self.handle(message);
            }
            self.flush();
        }
        self.flush();
    }
    fn handle(&mut self, message: Message) {
        match message {
            // Ein Schreibfehler (Platte voll) kostet nur diese Zeile; die nächste versucht
            // eine neue Datei.
            Message::Line(line) => drop(self.write_line(&line)),
            Message::Flush(ack) => {
                self.flush();
                let _ = ack.send(());
            }
        }
    }
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        let len = line.len() as u64;
        if self.file.is_none() || self.size > 0 && self.size + len > self.limits.file_bytes {
            self.rotate()?;
        }
        let Some(file) = self.file.as_mut() else {
            return Err(io::Error::other("keine Logdatei"));
        };
        if let Err(error) = file.write_all(line.as_bytes()) {
            self.file = None;
            return Err(error);
        }
        self.size += len;
        Ok(())
    }
    pub fn flush(&mut self) {
        if let Some(file) = &mut self.file
            && file.flush().is_err()
        {
            self.file = None;
        }
    }
    fn rotate(&mut self) -> io::Result<()> {
        self.flush();
        self.file = None;
        fs::create_dir_all(&self.dir)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
        let mut serial = 0;
        let (file, name) = loop {
            let name = format!("{PREFIX}{stamp}-{serial:02}{SUFFIX}");
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.dir.join(&name))
            {
                Ok(file) => break (file, name),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && serial < 99 => serial += 1,
                Err(e) => return Err(e),
            }
        };
        self.size = 0;
        self.file = Some(io::BufWriter::with_capacity(64 * 1024, file));
        prune(
            &self.dir,
            &name,
            self.limits
                .total_bytes
                .saturating_sub(self.limits.file_bytes),
        );
        Ok(())
    }
}

/// Löscht die ältesten Logdateien (der Name beginnt mit dem Zeitstempel), bis die übrigen
/// außer `current` höchstens `budget` Bytes belegen.
fn prune(dir: &Path, current: &str, budget: u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(String, u64)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            (name.starts_with(PREFIX) && name.ends_with(SUFFIX) && name != current)
                .then(|| Some((name, entry.metadata().ok()?.len())))?
        })
        .collect();
    files.sort();
    let mut total: u64 = files.iter().map(|(_, len)| len).sum();
    for (name, len) in files {
        if total <= budget {
            break;
        }
        if fs::remove_file(dir.join(&name)).is_ok() {
            total -= len;
        }
    }
}

static DIAG: OnceLock<Diag> = OnceLock::new();
static STARTED: OnceLock<Instant> = OnceLock::new();
static FACADE: Facade = Facade;

/// Verbindet `log` mit dem Diagnose-Log. Eigene Bereiche (ohne `::`) ab Info, fremde
/// Crates und Modulpfade nur Warnungen und Fehler.
struct Facade;
impl log::Log for Facade {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        !metadata.target().contains("::") || metadata.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata())
            && let Some(diag) = DIAG.get()
        {
            diag.write(record.level(), record.target(), record.args());
        }
    }
    fn flush(&self) {
        flush(Duration::from_secs(1));
    }
}

/// Einmal beim Programmstart: Schreib-Thread, `log`-Anbindung und Panik-Haken.
pub fn init(dir: PathBuf, enabled: bool) {
    STARTED.get_or_init(Instant::now);
    DIAG.get_or_init(|| Diag::start(dir, enabled, Limits::APP));
    if log::set_logger(&FACADE).is_ok() {
        set_enabled(enabled);
        install_panic_hook();
    }
}

/// Schalter „Diagnose-Log schreiben“; wirkt sofort.
pub fn set_enabled(on: bool) {
    if let Some(diag) = DIAG.get() {
        diag.set_enabled(on);
    }
    log::set_max_level(if on {
        log::LevelFilter::Info
    } else {
        log::LevelFilter::Off
    });
}

/// Schreibt das Log gerade? Für Angaben, die nur fürs Log berechnet werden.
pub fn enabled() -> bool {
    DIAG.get().is_some_and(Diag::enabled)
}

/// Zeit seit [`init`].
pub fn uptime() -> Duration {
    STARTED.get().map_or(Duration::ZERO, Instant::elapsed)
}

/// Wartet höchstens `timeout`, bis alles Eingereihte geschrieben ist (Programmende, Panik).
pub fn flush(timeout: Duration) {
    if let Some(diag) = DIAG.get() {
        diag.flush(timeout);
    }
}

/// Panik auf jedem Thread (auch Worker, deren Panik `jobs::Task` abfängt): Meldung, Ort und
/// Backtrace ins Log, dann der bisherige Haken.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if enabled() {
            let thread = std::thread::current();
            let location = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_default();
            log::error!(
                target: "panik",
                "thread={:?} ort={location} meldung={:?} backtrace={}",
                thread.name().unwrap_or("unbenannt"),
                info.payload()
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("unbekannt"),
                std::backtrace::Backtrace::force_capture()
            );
            flush(Duration::from_secs(2));
        }
        previous(info);
    }));
}

/// Misst eine Arbeit und schreibt beim Verlassen `<was> dauer_ms=… <Felder>` in `area`.
/// Bei ausgeschaltetem Log kostet sie nur einen `Instant::now()`.
pub fn span(area: &'static str, what: &'static str) -> Span {
    Span {
        area,
        what,
        started: Instant::now(),
        fields: enabled().then(String::new),
    }
}
pub struct Span {
    area: &'static str,
    what: &'static str,
    started: Instant,
    fields: Option<String>,
}
impl Span {
    /// Hängt ` key=value` an; Texte mit Leerzeichen als `{:?}` übergeben.
    pub fn field(&mut self, key: &str, value: impl fmt::Display) -> &mut Self {
        if let Some(fields) = &mut self.fields {
            let _ = write!(fields, " {key}={value}");
        }
        self
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some(fields) = &self.fields {
            log::info!(
                target: self.area,
                "{} dauer_ms={:.1}{fields}",
                self.what,
                millis(self.started.elapsed())
            );
        }
    }
}

/// Millisekunden mit Nachkommastellen, für `dauer_ms=`.
pub fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_files(dir: &Path) -> Vec<(String, u64)> {
        let mut files: Vec<_> = fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_str()?.to_owned();
                        name.ends_with(SUFFIX)
                            .then(|| Some((name, e.metadata().ok()?.len())))?
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }

    #[test]
    fn rolling_keeps_the_total_cap_and_deletes_the_oldest() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let dir = temp.path().join("logs");
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("fremd.txt"), "bleibt")?;
        let limits = Limits {
            file_bytes: 1000,
            total_bytes: 3000,
        };
        let mut roller = Roller::new(dir.clone(), limits);
        for n in 0..200 {
            // 100 Bytes je Zeile: 10 Zeilen je Datei, 20 Dateien insgesamt.
            roller.write_line(&format!("{:<98}\r\n", format!("zeile {n:03}")))?;
            roller.flush();
            let files = log_files(&dir);
            assert!(files.iter().map(|(_, len)| len).sum::<u64>() <= limits.total_bytes);
            assert!(files.iter().all(|(_, len)| *len <= limits.file_bytes));
        }
        let files = log_files(&dir);
        assert_eq!(files.len(), 3);
        let all: String = files
            .iter()
            .map(|(name, _)| fs::read_to_string(dir.join(name)))
            .collect::<io::Result<_>>()?;
        assert!(all.starts_with("zeile 170"), "älteste gelöscht: {all:.20}");
        assert!(all.contains("zeile 199"));
        assert!(!all.contains("zeile 169"));
        assert_eq!(fs::read_to_string(dir.join("fremd.txt"))?, "bleibt");
        Ok(())
    }

    #[test]
    fn a_disabled_log_writes_nothing() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let dir = temp.path().join("logs");
        let diag = Diag::start(dir.clone(), false, Limits::APP);
        diag.write(log::Level::Error, "test", &format_args!("aus"));
        assert!(diag.flush(Duration::from_secs(5)));
        assert!(
            !dir.exists(),
            "ausgeschaltet entsteht weder Ordner noch Datei"
        );
        diag.set_enabled(true);
        diag.write(log::Level::Info, "test", &format_args!("an"));
        assert!(diag.flush(Duration::from_secs(5)));
        let files = log_files(&dir);
        assert_eq!(files.len(), 1);
        let text = fs::read_to_string(dir.join(&files[0].0))?;
        assert!(text.ends_with(" INFO  test an\n"), "{text}");
        assert!(!text.contains("aus"));
        Ok(())
    }

    #[test]
    fn lines_are_one_per_event_and_parseable() -> anyhow::Result<()> {
        use chrono::TimeZone;
        let time = chrono::Local
            .with_ymd_and_hms(2026, 11, 19, 18, 25, 3)
            .single()
            .ok_or_else(|| anyhow::anyhow!("Zeit"))?
            + chrono::Duration::milliseconds(7);
        let line = format_line(
            time,
            log::Level::Warn,
            "job",
            &format_args!(
                "ende art=faces dauer_ms={:.1} fehler={:?}\nzweite Zeile",
                73.44, "a b"
            ),
        );
        assert_eq!(
            line,
            "2026-11-19 18:25:03.007 WARN  job ende art=faces dauer_ms=73.4 fehler=\"a b\" | zweite Zeile\n"
        );
        // Zerlegen wie beim Auswerten: Datum, Uhrzeit, Stufe, Bereich, Rest.
        let mut parts = line.trim_end().splitn(3, ' ');
        let (date, clock) = (parts.next(), parts.next());
        let rest = parts.next().map(str::trim_start).unwrap_or_default();
        let (level, rest) = rest.split_once(' ').unwrap_or_default();
        let (area, message) = rest.trim_start().split_once(' ').unwrap_or_default();
        assert!(
            chrono::NaiveDateTime::parse_from_str(
                &format!("{} {}", date.unwrap_or_default(), clock.unwrap_or_default()),
                "%Y-%m-%d %H:%M:%S%.3f"
            )
            .is_ok()
        );
        assert_eq!((level, area), ("WARN", "job"));
        let duration = message
            .split(' ')
            .find_map(|pair| pair.strip_prefix("dauer_ms="))
            .map(str::parse::<f64>);
        assert_eq!(duration.transpose()?, Some(73.4));
        Ok(())
    }

    #[test]
    fn a_span_collects_nothing_while_logging_is_off() {
        // In Tests ist kein Log eingerichtet: Felder werden gar nicht erst formatiert.
        let mut span = span("test", "messung");
        span.field("anzahl", 3);
        assert!(span.fields.is_none());
    }
}
