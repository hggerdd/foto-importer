//! Datenstand der Sammlung für Caches (ADR Sammlung und Daten, „Datenrevision“).
//!
//! `data_revision(domain, revision)` zählt je Bereich jede Änderung; Trigger erhöhen den
//! Zähler in derselben Transaktion wie die Änderung. Ein Cache merkt sich Identität
//! (`collection.id`) und die Zähler der Bereiche, aus denen er gebaut ist ([`Key`]); ist der
//! Schlüssel gleich, gilt er weiter. Die Zähler steigen nur; nach dem Zurückspielen einer
//! Sicherung können sie kleiner werden, deshalb vergleichen Caches auf Gleichheit.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Bereiche mit eigenem Zähler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Domain {
    /// Bestand und Projektion: `files` und `meta`, soweit `metadata_photos` und
    /// `face_sources` davon abhängen (dieselben Trigger pflegen die Projektion).
    Files,
    /// Zuordnungen und Entscheidungen zu Gesichtern: Gesichter mit Person (Einfügen,
    /// Löschen, Ändern von Person, Zuordnungsart, Ignorieren, Behalten), `face_rejections`,
    /// `training_set` und `face_settings`. Nicht die Erkennung selbst (neue Gesichter ohne
    /// Person, Merkmale, Gruppen, Aufträge): sie entsteht in Massen und kein Cache hängt
    /// daran.
    Faces,
    /// Personen, Personenangaben, Beziehungen und Beziehungstypen.
    People,
}

impl Domain {
    pub const ALL: [Domain; 3] = [Domain::Files, Domain::Faces, Domain::People];

    /// Name in `data_revision.domain`.
    pub fn name(self) -> &'static str {
        match self {
            Domain::Files => "files",
            Domain::Faces => "faces",
            Domain::People => "people",
        }
    }
}

/// Tabellen, deren Änderungen einen Bereich erhöhen (Trigger in `schema.sql`, Test prüft
/// jede Tabelle). `faces` erhöht nur bei Zeilen mit Person, `files` und `meta` nur bei
/// Spalten der Projektion.
pub const TABLES: &[(&str, Domain)] = &[
    ("files", Domain::Files),
    ("meta", Domain::Files),
    ("faces", Domain::Faces),
    ("face_rejections", Domain::Faces),
    ("training_set", Domain::Faces),
    ("face_settings", Domain::Faces),
    ("persons", Domain::People),
    ("person_attrs", Domain::People),
    ("relations", Domain::People),
    ("relation_types", Domain::People),
];

/// Stand einer Sammlung: Identität und Zähler aller Bereiche.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Revisions {
    pub identity: String,
    pub files: i64,
    pub faces: i64,
    pub people: i64,
}

/// Cache-Schlüssel: Identität und die Zähler der gewählten Bereiche.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Key {
    pub identity: String,
    pub revisions: Vec<(Domain, i64)>,
}

impl Revisions {
    pub fn get(&self, domain: Domain) -> i64 {
        match domain {
            Domain::Files => self.files,
            Domain::Faces => self.faces,
            Domain::People => self.people,
        }
    }

    /// Schlüssel für einen Cache, der aus diesen Bereichen gebaut ist.
    pub fn key(&self, domains: &[Domain]) -> Key {
        Key {
            identity: self.identity.clone(),
            revisions: domains.iter().map(|&d| (d, self.get(d))).collect(),
        }
    }
}

impl Key {
    /// Ist dieser Stand älter als `other` (dieselbe Sammlung, ein Zähler kleiner, keiner
    /// größer)? Dann ist ein Ergebnis zu diesem Stand überholt.
    pub fn older_than(&self, other: &Key) -> bool {
        self.identity == other.identity
            && self.revisions.len() == other.revisions.len()
            && self != other
            && self
                .revisions
                .iter()
                .zip(&other.revisions)
                .all(|((a, x), (b, y))| a == b && x <= y)
    }
}

/// Liest Identität und alle Zähler mit einer Abfrage. `None`, wenn die Sammlung keine
/// Identität hat oder noch kein `data_revision` kennt (Schema 6, nur lesend geöffnet): dann
/// wird nichts wiederverwendet.
pub fn read(db: &Connection) -> Result<Option<Revisions>> {
    let row = db
        .query_row(
            "SELECT (SELECT id FROM collection LIMIT 1),
                (SELECT revision FROM data_revision WHERE domain='files'),
                (SELECT revision FROM data_revision WHERE domain='faces'),
                (SELECT revision FROM data_revision WHERE domain='people')",
            [],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .optional();
    let row = match row {
        Ok(row) => row,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    Ok(match row {
        Some((Some(identity), Some(files), Some(faces), Some(people))) if !identity.is_empty() => {
            Some(Revisions {
                identity,
                files,
                faces,
                people,
            })
        }
        _ => None,
    })
}

/// Identität der Sammlung (`collection.id`), leer ohne.
pub fn identity(db: &Connection) -> Result<String> {
    Ok(db
        .query_row("SELECT id FROM collection LIMIT 1", [], |r| {
            r.get::<_, Option<String>>(0)
        })
        .optional()?
        .flatten()
        .unwrap_or_default())
}

/// Schlüssel der gewählten Bereiche, falls die Sammlung Revisionen hat.
pub fn key(db: &Connection, domains: &[Domain]) -> Result<Option<Key>> {
    Ok(read(db)?.map(|r| r.key(domains)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(files: i64, people: i64) -> Key {
        Revisions {
            identity: "a".into(),
            files,
            faces: 0,
            people,
        }
        .key(&[Domain::Files, Domain::People])
    }

    #[test]
    fn older_needs_same_collection_and_no_counter_ahead() {
        assert!(key(1, 1).older_than(&key(2, 1)));
        assert!(!key(2, 1).older_than(&key(2, 1)));
        assert!(!key(2, 1).older_than(&key(1, 2)));
        let mut other = key(1, 1);
        other.identity = "b".into();
        assert!(!other.older_than(&key(2, 2)));
    }
}
