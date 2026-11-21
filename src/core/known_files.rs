//! Bestand im Speicher für einen Indexlauf (ADR 0064).
//!
//! Früher fragte der Lauf SQLite je neuer Datei nach Verschiebungskandidaten und
//! Variantenpartnern; keine dieser Spalten ist indiziert, jede Frage war also ein voller
//! Tabellendurchlauf. Die Antworten hängen nur vom Bestand vor dem Lauf und den eigenen
//! Änderungen des Laufs ab; deshalb werden sie einmal geladen und hier aktuell gehalten. Die
//! Ergebnisse entsprechen den früheren Abfragen je Datei.
use super::{collection::family_directory, files, jobs::Control};
use anyhow::Result;
use rusqlite::Connection;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

/// Bestandszeile, deren Pfad der Durchlauf nicht besucht hat: die einzig mögliche Quelle einer
/// Verschiebung.
#[derive(Clone, Debug)]
pub struct Unseen {
    slot: usize,
    pub id: i64,
    pub size: i64,
    pub mtime: i64,
    pub relpath: String,
    pub inode: Option<String>,
    pub full_hash: Option<String>,
    pub quick_hash: Option<String>,
    pub stem: String,
    pub dir: String,
    photo: Option<i64>,
}

/// Ergebnis der Suche nach einer verschwundenen Datei über Name, Größe und Änderungszeit.
pub enum Signature {
    None,
    Ambiguous,
    Unique(Unseen),
}

/// Gefalteter Dateiname eines relativen Pfads.
fn file_name_key(relpath: &str) -> String {
    files::fold(relpath.rsplit('/').next().unwrap_or(relpath))
}

pub struct KnownFiles {
    unseen: Vec<Option<Unseen>>,
    by_inode: HashMap<(String, i64), Vec<usize>>,
    hashed_by_size: HashMap<i64, Vec<usize>>,
    /// (gefalteter Dateiname, Größe, Änderungszeit) -> Plätze in `unseen`
    by_signature: HashMap<(String, i64, i64), Vec<usize>>,
    /// Alle Pfade des Bestands vor dem Lauf.
    paths: HashSet<String>,
    /// (gefalteter Dateiname, Größe) -> Zahl gefundener Dateien ohne Bestandszeile
    new_by_name: HashMap<(String, i64), usize>,
    /// gefalteter Stamm -> (Datei-ID, Foto-ID, gefaltetes Familienverzeichnis), nach Datei-ID
    /// geordnet
    families: HashMap<String, Vec<(i64, i64, String)>>,
}
impl KnownFiles {
    pub fn load(
        db: &Connection,
        walked: &[String],
        extension_folders: &[String],
        control: &Control,
    ) -> Result<Self> {
        let walked: HashSet<&str> = walked.iter().map(String::as_str).collect();
        let rows: usize = db.query_row("SELECT count(*) FROM files", [], |r| r.get(0))?;
        control.progress(0, rows);
        let mut known = Self {
            unseen: Vec::new(),
            by_inode: HashMap::new(),
            hashed_by_size: HashMap::new(),
            by_signature: HashMap::new(),
            paths: HashSet::new(),
            new_by_name: HashMap::new(),
            families: HashMap::new(),
        };
        let mut statement = db.prepare(
            "SELECT id,photo_id,size,mtime_ns,relpath,inode,full_hash,stem,dir,quick_hash FROM files ORDER BY id",
        )?;
        let mut cursor = statement.query([])?;
        let mut count = 0;
        while let Some(r) = cursor.next()? {
            count += 1;
            if count % 2048 == 0 {
                control.checkpoint()?;
                control.progress(count, rows);
            }
            let row = Unseen {
                slot: 0,
                id: r.get(0)?,
                photo: r.get(1)?,
                size: r.get(2)?,
                mtime: r.get(3)?,
                relpath: r.get(4)?,
                inode: r.get(5)?,
                full_hash: r.get(6)?,
                stem: r.get(7)?,
                dir: r.get(8)?,
                quick_hash: r.get(9)?,
            };
            if let Some(photo) = row.photo {
                known
                    .families
                    .entry(files::fold(&row.stem))
                    .or_default()
                    .push((
                        row.id,
                        photo,
                        files::fold(&family_directory(&row.dir, extension_folders)),
                    ));
            }
            known.paths.insert(row.relpath.clone());
            if !walked.contains(row.relpath.as_str()) {
                known.push_unseen(row);
            }
        }
        Ok(known)
    }
    fn push_unseen(&mut self, mut row: Unseen) {
        row.slot = self.unseen.len();
        if let Some(inode) = &row.inode {
            self.by_inode
                .entry((inode.clone(), row.size))
                .or_default()
                .push(row.slot);
        }
        self.by_signature
            .entry((file_name_key(&row.relpath), row.size, row.mtime))
            .or_default()
            .push(row.slot);
        if row.full_hash.is_some() {
            self.hashed_by_size
                .entry(row.size)
                .or_default()
                .push(row.slot);
        }
        self.unseen.push(Some(row));
    }
    fn gone(&self, root: &Path, slots: &[usize]) -> Vec<Unseen> {
        slots
            .iter()
            .filter_map(|slot| self.unseen[*slot].as_ref())
            .filter(|row| !root.join(&row.relpath).exists())
            .cloned()
            .collect()
    }
    /// Die einzige verschwundene Datei mit dieser Datei-ID und Größe, falls eindeutig.
    pub fn moved_by_inode(&self, root: &Path, inode: Option<&str>, size: i64) -> Option<Unseen> {
        let slots = self.by_inode.get(&(inode?.to_string(), size))?;
        let mut found = self.gone(root, slots);
        (found.len() == 1).then(|| found.remove(0))
    }
    /// Zählt die gefundenen Dateien ohne Bestandszeile je Name und Größe. Gibt es zu einem
    /// Paar mehr als eine, wird über die Signatur nichts zugeordnet (mehrdeutig).
    pub fn note_walked(&mut self, walked: &[String], sizes: impl Iterator<Item = i64>) {
        self.new_by_name.clear();
        for (relpath, size) in walked.iter().zip(sizes) {
            if !self.paths.contains(relpath) {
                *self
                    .new_by_name
                    .entry((file_name_key(relpath), size))
                    .or_default() += 1;
            }
        }
    }
    /// Umbenannter oder verschobener Ordner ohne passende Datei-ID (z. B. auf anderem
    /// Datenträger oder Dateisystem ohne stabile IDs): die einzige verschwundene Datei mit
    /// gleichem Namen, gleicher Größe und gleicher Änderungszeit. Mehrdeutig ist es, wenn
    /// mehrere verschwundene Zeilen oder mehrere neue Dateien dieselbe Signatur tragen; dann
    /// wird nicht geraten.
    pub fn moved_by_signature(
        &self,
        root: &Path,
        relpath: &str,
        size: i64,
        mtime: i64,
    ) -> Signature {
        let name = file_name_key(relpath);
        let Some(slots) = self.by_signature.get(&(name.clone(), size, mtime)) else {
            return Signature::None;
        };
        let mut found = self.gone(root, slots);
        if found.is_empty() {
            return Signature::None;
        }
        if found.len() > 1 || self.new_by_name.get(&(name, size)).copied().unwrap_or(0) > 1 {
            return Signature::Ambiguous;
        }
        Signature::Unique(found.remove(0))
    }
    pub fn has_hashed(&self, size: i64) -> bool {
        self.hashed_by_size.contains_key(&size)
    }
    /// Verschwundene Dateien dieser Größe mit gespeichertem vollständigem Hash.
    pub fn hashed_unseen(&self, root: &Path, size: i64) -> Vec<Unseen> {
        self.hashed_by_size
            .get(&size)
            .map(|slots| self.gone(root, slots))
            .unwrap_or_default()
    }
    /// Eine zugeordnete Zeile liegt jetzt unter einem neuen Pfad: sie ist kein Kandidat mehr,
    /// und ihr Familieneintrag folgt dem neuen Stamm und Verzeichnis.
    pub fn relocate(
        &mut self,
        found: &Unseen,
        stem: &str,
        dir: &str,
        extension_folders: &[String],
    ) {
        self.unseen[found.slot] = None;
        let Some(photo) = found.photo else { return };
        if let Some(list) = self.families.get_mut(&files::fold(&found.stem)) {
            list.retain(|entry| entry.0 != found.id);
        }
        let list = self.families.entry(files::fold(stem)).or_default();
        let at = list.partition_point(|entry| entry.0 < found.id);
        list.insert(
            at,
            (
                found.id,
                photo,
                files::fold(&family_directory(dir, extension_folders)),
            ),
        );
    }
    /// Das Foto der ältesten Datei mit diesem Stamm im selben Familienverzeichnis.
    pub fn family_photo(&self, stem: &str, family: &str) -> Option<i64> {
        let family = files::fold(family);
        self.families
            .get(&files::fold(stem))?
            .iter()
            .find(|entry| entry.2 == family)
            .map(|entry| entry.1)
    }
    pub fn add(&mut self, id: i64, photo: i64, stem: &str, family: &str) {
        self.families
            .entry(files::fold(stem))
            .or_default()
            .push((id, photo, files::fold(family)));
    }
}
