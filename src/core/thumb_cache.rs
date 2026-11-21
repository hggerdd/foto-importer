//! Lokaler Vorschau-Cache für Galerie und Bildinspektor (ADR Sammlungsansicht).
//!
//! Ohne ihn dekodiert jede Galeriekachel nach dem Start das ganze Original von der
//! Sammlungsplatte (auf der USB-Festplatte 1 bis 2 s je Foto). Der Cache legt fertige,
//! orientierte Vorschauen als JPEG im lokalen Cache-Ordner des Arbeitsplatzes ab, nie in der
//! Sammlung. Schlüssel sind kanonischer Pfad, Größe und Änderungszeit des Originals sowie
//! Ausrichtung und Kantenlänge: ein geändertes Original ergibt einen neuen Eintrag. Die
//! Gesamtgröße ist begrenzt (`config::THUMB_CACHE_BYTES`); darüber werden die am längsten
//! nicht benutzten Einträge gelöscht. Alles ist Ableitung und jederzeit löschbar.

use anyhow::Result;
use image::DynamicImage;
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static WRITES: AtomicUsize = AtomicUsize::new(0);

/// Setzt den Cache-Ordner (`None` schaltet den Cache ab) und räumt ihn auf einem eigenen
/// Thread auf.
pub fn configure(dir: Option<PathBuf>) {
    if let Ok(mut current) = DIR.lock() {
        current.clone_from(&dir);
    }
    if let Some(dir) = dir {
        std::thread::spawn(move || trim(&dir, crate::config::THUMB_CACHE_BYTES));
    }
}

/// Der eingestellte Cache-Ordner.
pub fn dir() -> Option<PathBuf> {
    DIR.lock().ok().and_then(|dir| dir.clone())
}

/// Schlüssel eines Eintrags aus dem kanonischen Pfad und den Dateiangaben des Originals.
pub fn key(resolved: &Path, meta: &std::fs::Metadata, orientation: u32, edge: u32) -> String {
    let nanos = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_nanos());
    let mut hash = blake3::Hasher::new();
    hash.update(b"thumb-v1\0");
    hash.update(super::files::fold(&resolved.to_string_lossy()).as_bytes());
    hash.update(b"\0");
    hash.update(&meta.len().to_le_bytes());
    hash.update(&nanos.to_le_bytes());
    hash.update(&orientation.to_le_bytes());
    hash.update(&edge.to_le_bytes());
    hash.finalize().to_hex().to_string()
}

fn entry(dir: &Path, key: &str) -> PathBuf {
    dir.join(&key[..2]).join(format!("{key}.jpg"))
}

/// Liest einen Eintrag. Ein kaputter Eintrag wird gelöscht. Ein Treffer frischt die
/// Änderungszeit auf, höchstens einmal je `THUMB_CACHE_TOUCH`, damit das Aufräumen die
/// zuletzt benutzten Einträge behält.
pub fn get(dir: &Path, key: &str) -> Option<DynamicImage> {
    let path = entry(dir, key);
    let bytes = std::fs::read(&path).ok()?;
    match image::load_from_memory(&bytes) {
        Ok(image) => {
            let old = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age > crate::config::THUMB_CACHE_TOUCH);
            if old && let Ok(file) = std::fs::File::options().write(true).open(&path) {
                let _ = file.set_modified(SystemTime::now());
            }
            Some(image)
        }
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

/// Kodiert eine Vorschau so, wie sie gespeichert wird.
pub fn encode(image: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(
        &mut out,
        crate::config::THUMB_CACHE_QUALITY,
    )
    .encode_image(&DynamicImage::ImageRgb8(image.to_rgb8()))?;
    Ok(out)
}

/// Legt einen Eintrag atomar an; ein vorhandener bleibt (gleicher Schlüssel, gleicher Inhalt).
/// Fehler werden ignoriert. Nach `THUMB_CACHE_TRIM_EVERY` neuen Einträgen wird aufgeräumt.
pub fn put(dir: &Path, key: &str, bytes: &[u8]) {
    let path = entry(dir, key);
    if super::files::write_atomic(&path, bytes, super::files::Mode::NoClobber, false).is_ok() {
        let written = WRITES.fetch_add(1, Ordering::Relaxed) + 1;
        if written.is_multiple_of(crate::config::THUMB_CACHE_TRIM_EVERY) {
            trim(dir, crate::config::THUMB_CACHE_BYTES);
        }
    }
}

/// Hält den Cache unter `max_bytes`: löscht die am längsten nicht benutzten Einträge, bis
/// höchstens neun Zehntel übrig sind. Löscht nur `*.jpg` in den Unterordnern des Caches
/// sowie liegengebliebene Zwischendateien, die älter als eine Stunde sind. Liefert die Zahl
/// gelöschter Dateien.
pub fn trim(dir: &Path, max_bytes: u64) -> usize {
    let mut entries = Vec::new();
    let mut total = 0u64;
    let mut removed = 0;
    let Ok(folders) = std::fs::read_dir(dir) else {
        return 0;
    };
    for folder in folders.flatten() {
        let name = folder.file_name();
        let name = name.to_string_lossy();
        if name.len() != 2 || !folder.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(files) = std::fs::read_dir(folder.path()) else {
            continue;
        };
        for file in files.flatten() {
            let Ok(meta) = file.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            let path = file.path();
            if path.extension().is_some_and(|e| e == "jpg") {
                total += meta.len();
                entries.push((modified, meta.len(), path));
            } else if file.file_name().to_string_lossy().starts_with(".tmp")
                && modified
                    .elapsed()
                    .is_ok_and(|age| age > Duration::from_secs(3600))
                && std::fs::remove_file(&path).is_ok()
            {
                removed += 1;
            }
        }
    }
    if total <= max_bytes {
        return removed;
    }
    entries.sort_by_key(|(modified, _, _)| *modified);
    let goal = max_bytes / 10 * 9;
    for (_, len, path) in entries {
        if total <= goal {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
            removed += 1;
        }
    }
    if removed > 0 {
        log::info!(target: "vorschau", "cache_aufgeraeumt geloescht={removed} bytes={total}");
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(shade: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            40,
            30,
            image::Rgb([shade, 0, 0]),
        ))
    }

    #[test]
    fn entries_round_trip_and_keys_follow_the_original() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let original = dir.path().join("a.jpg");
        std::fs::write(&original, b"eins")?;
        let meta = original.metadata()?;
        let key = key(&original, &meta, 1, 400);
        assert_ne!(key, super::key(&original, &meta, 6, 400));
        assert_ne!(key, super::key(&original, &meta, 1, 960));
        std::fs::write(&original, b"anders lang")?;
        assert_ne!(key, super::key(&original, &original.metadata()?, 1, 400));

        let cache = dir.path().join("cache");
        assert!(get(&cache, &key).is_none());
        let bytes = encode(&image(200))?;
        put(&cache, &key, &bytes);
        let back = get(&cache, &key).expect("Eintrag");
        assert_eq!((back.width(), back.height()), (40, 30));
        // Vorhandene Einträge bleiben, kaputte werden gelöscht.
        put(&cache, &key, b"anderes");
        assert!(get(&cache, &key).is_some());
        std::fs::write(entry(&cache, &key), b"kaputt")?;
        assert!(get(&cache, &key).is_none());
        assert!(!entry(&cache, &key).exists());
        Ok(())
    }

    #[test]
    fn trim_removes_least_recently_used_entries_and_nothing_else() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let cache = dir.path();
        let bytes = encode(&image(10))?;
        let mut keys = Vec::new();
        for index in 0..6u8 {
            let key = blake3::hash(&[index]).to_hex().to_string();
            put(cache, &key, &bytes);
            let file = std::fs::File::options()
                .write(true)
                .open(entry(cache, &key))?;
            file.set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000 + index as u64))?;
            keys.push(key);
        }
        let foreign = cache.join("notiz.txt");
        std::fs::write(&foreign, b"bleibt")?;
        let size = bytes.len() as u64;
        // Platz für vier Einträge: die beiden ältesten (und bis neun Zehntel) gehen.
        let removed = trim(cache, size * 4);
        assert!(removed >= 2, "{removed}");
        assert!(!entry(cache, &keys[0]).exists() && !entry(cache, &keys[1]).exists());
        assert!(entry(cache, &keys[5]).exists());
        assert!(foreign.exists());
        assert_eq!(trim(cache, size * 100), 0);
        Ok(())
    }
}
