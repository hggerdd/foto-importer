//! Arbeitsumfang für lange Läufe auf großen Sammlungen (ADR 0059).
//!
//! Analyse und Metadaten-Anreicherung arbeiten in einer vorab festgelegten
//! Reihenfolge „neueste zuerst“ und optional nur eine Portion (`limit`) oder nur
//! einen Ordner ab. So lässt sich eine Sammlung mit sehr vielen Fotos schrittweise
//! abarbeiten; fertige Fotos werden übersprungen und zählen nicht zur Portion.
use super::{collection::Collection, files};
use anyhow::Result;
use rusqlite::params_from_iter;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    /// Höchstzahl neu zu bearbeitender Fotos bzw. Dateien; `None` bedeutet alle.
    pub limit: Option<usize>,
    /// Nur Dateien unterhalb dieses Ordners (relativer Pfad der Sammlung).
    pub folder: Option<String>,
}

impl Scope {
    pub fn new(limit: Option<usize>, folder: Option<String>) -> Self {
        Self {
            limit: limit.filter(|l| *l > 0),
            folder: folder
                .map(|f| f.trim_matches(['/', '\\']).replace('\\', "/"))
                .filter(|f| !f.is_empty()),
        }
    }
    /// SQL-Zusatzbedingung auf `f.relpath_fold` samt Parameter.
    fn folder_clause(&self) -> (&'static str, Vec<String>) {
        match &self.folder {
            Some(folder) => {
                let escaped = files::fold(folder)
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                (
                    " AND f.relpath_fold LIKE ? ESCAPE '\\'",
                    vec![format!("{escaped}/%")],
                )
            }
            None => ("", Vec::new()),
        }
    }
}

/// Aufnahmedatum, solange bekannt, sonst das Dateidatum im selben Format.
const NEWEST_FIRST: &str = "coalesce(nullif(m.dt_original,''), strftime('%Y:%m:%d %H:%M:%S', f.mtime_ns/1000000000, 'unixepoch'))";

/// Foto-IDs für die Gesichtsanalyse, neueste zuerst.
pub fn analysis_order(collection: &Collection, scope: &Scope) -> Result<Vec<i64>> {
    let (clause, args) = scope.folder_clause();
    let sql = format!(
        "SELECT s.photo_id FROM face_sources s JOIN files f ON f.id=s.file_id
         LEFT JOIN meta m ON m.file_id=f.id
         WHERE f.status='present'{clause} ORDER BY {NEWEST_FIRST} DESC, s.photo_id DESC"
    );
    Ok(collection
        .db
        .prepare(&sql)?
        .query_map(params_from_iter(args.iter()), |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Datei-IDs ohne fertige Metadaten, neueste zuerst. Dateien mit früherem Fehler
/// folgen zuletzt, damit sie eine Portion nicht immer wieder belegen.
pub fn metadata_order(collection: &Collection, scope: &Scope) -> Result<Vec<i64>> {
    let (clause, args) = scope.folder_clause();
    let limit = scope
        .limit
        .map_or(String::new(), |l| format!(" LIMIT {}", l));
    let sql = format!(
        "SELECT f.id FROM files f WHERE f.status='present' AND f.meta_state!='done'{clause}
         ORDER BY (f.meta_state='error') ASC, f.mtime_ns DESC, f.id DESC{limit}"
    );
    Ok(collection
        .db
        .prepare(&sql)?
        .query_map(params_from_iter(args.iter()), |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<_>>()?)
}
