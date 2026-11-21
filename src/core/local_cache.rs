//! Kleine JSON-Ableitungen einer Sammlung im lokalen Cache-Ordner des Arbeitsplatzes
//! (`Settings::cache_dir`, nie in der Sammlung): Startansicht, Personenliste der Zeitreise
//! und Personengraph. Sie sind reine Beschleunigung: fehlt, verdirbt oder veraltet eine Datei,
//! wird nur langsamer gelesen. Jede Datei gehört über den Pfad zu genau einer Sammlung und
//! trägt eine Formatnummer; andere Formate, fremde Sammlungen und zu große Dateien gelten
//! als fehlend.

use super::files;
use anyhow::Result;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Entry<T> {
    format: u32,
    collection: String,
    value: T,
}

/// Datei `<art>-<Hash des Sammlungspfads>.json` im Cache-Ordner.
pub fn file(dir: &Path, kind: &str, collection: &str) -> PathBuf {
    let key = blake3::hash(files::fold(collection).as_bytes()).to_hex();
    dir.join(format!("{kind}-{}.json", &key[..20]))
}

/// Liest den Wert; `None` bei fehlender, fremder, zu großer oder kaputter Datei.
pub fn load<T: DeserializeOwned>(
    dir: &Path,
    kind: &str,
    format: u32,
    collection: &str,
) -> Option<T> {
    let path = file(dir, kind, collection);
    if path.metadata().ok()?.len() > crate::config::LOCAL_CACHE_MAX_BYTES {
        return None;
    }
    let entry: Entry<T> = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (entry.format == format && entry.collection == collection).then_some(entry.value)
}

/// Schreibt den Wert atomar (Zwischendatei, dann Umbenennen).
pub fn save<T: Serialize>(
    dir: &Path,
    kind: &str,
    format: u32,
    collection: &str,
    value: &T,
) -> Result<()> {
    let bytes = serde_json::to_vec(&Entry {
        format,
        collection: collection.to_owned(),
        value,
    })?;
    files::write_atomic(
        &file(dir, kind, collection),
        &bytes,
        files::Mode::Replace,
        false,
    )?;
    Ok(())
}

/// Wie [`save`], Fehler nur ins Diagnose-Log: der Cache ist eine Beschleunigung.
pub fn store<T: Serialize>(dir: &Path, kind: &str, format: u32, collection: &str, value: &T) {
    if let Err(e) = save(dir, kind, format, collection, value) {
        log::warn!(target: "sammlung", "cache_speichern art={kind} fehler={:?}", format!("{e:#}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_per_collection_and_format() {
        let dir = tempfile::tempdir().expect("Ordner");
        store(dir.path(), "probe", 1, "E:\\Bilder", &vec![1, 2, 3]);
        assert_eq!(
            load::<Vec<i32>>(dir.path(), "probe", 1, "E:\\Bilder"),
            Some(vec![1, 2, 3])
        );
        // Anderes Format, andere Sammlung oder andere Art: gilt als fehlend.
        assert_eq!(load::<Vec<i32>>(dir.path(), "probe", 2, "E:\\Bilder"), None);
        assert_eq!(load::<Vec<i32>>(dir.path(), "probe", 1, "F:\\Bilder"), None);
        assert_eq!(
            load::<Vec<i32>>(dir.path(), "andere", 1, "E:\\Bilder"),
            None
        );
        // Kaputte Datei: gilt als fehlend.
        std::fs::write(file(dir.path(), "probe", "E:\\Bilder"), b"{kaputt").expect("schreiben");
        assert_eq!(load::<Vec<i32>>(dir.path(), "probe", 1, "E:\\Bilder"), None);
    }
}
