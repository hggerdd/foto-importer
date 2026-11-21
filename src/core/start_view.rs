//! Schneller erster Blick auf die Sammlung (ADR Sammlungsansicht, „Startansicht“).
//!
//! Auf einer USB-Festplatte kosten die Zählungen der Sammlung (Gesamtzahl, Kameras, Bestand)
//! kalt zusammen viele Sekunden, die erste Seite nur rund eine. Deshalb liest die Oberfläche
//! die Seite zuerst und zählt danach. Eine Zählung gilt weiter, solange sich die
//! Projektionsrevision der Sammlung (`projection_revision`, von Triggern bei jeder Änderung an
//! Fotos, Dateien und Projektionsspalten erhöht) nicht geändert hat.
//!
//! Die Startansicht (erste Seite ohne Filter samt Zählungen) liegt zusätzlich als kleine
//! JSON-Datei im lokalen Cache-Ordner des Arbeitsplatzes, nie in der Sammlung. Sie wird beim
//! Start sofort gezeigt, als „wird aktualisiert“ markiert und durch die frisch gelesene Seite
//! ersetzt. Sie ist reine Ableitung: fehlt oder verdirbt sie, wird nur langsamer gestartet.

use super::{
    collection::{Collection, CollectionStatus},
    files,
    metadata::{self, Photo, Search},
};
use anyhow::Result;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Formatnummer der gespeicherten Startansicht; andere Nummern werden ignoriert.
const FORMAT: u32 = 1;

/// Stand einer Sammlung: Identität (`collection.id`) und Projektionsrevision.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub identity: String,
    pub revision: i64,
}

/// Liest den Stand mit zwei Zugriffen auf je eine kleine Tabelle. `None`, wenn die Sammlung
/// keine Identität oder Revision hat (dann wird nie wiederverwendet).
pub fn stamp(collection: &Collection) -> Result<Option<Stamp>> {
    let identity: Option<String> = collection
        .db
        .query_row("SELECT id FROM collection LIMIT 1", [], |r| r.get(0))
        .optional()?;
    let revision: Option<i64> = collection
        .db
        .query_row(
            "SELECT revision FROM projection_revision LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match (identity, revision) {
        (Some(identity), Some(revision)) if !identity.is_empty() => {
            Some(Stamp { identity, revision })
        }
        _ => None,
    })
}

/// Zählungen einer Suche: Bestand der Sammlung, Gesamtzahl und häufigste Kameras.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    pub status: CollectionStatus,
    pub total: usize,
    pub cameras: Vec<(String, usize)>,
}

/// Zählt in einer Lesetransaktion und liefert den Stand, zu dem die Zählung gehört.
pub fn count(
    collection: &Collection,
    filters: &Search,
    detector: &str,
) -> Result<(Option<Stamp>, Counts)> {
    collection.db.execute_batch("BEGIN")?;
    let result = (|| {
        let stamp = stamp(collection)?;
        let status = collection.status()?;
        let (total, cameras) = metadata::search_counts(collection, filters, detector)?;
        Ok((
            stamp,
            Counts {
                status,
                total,
                cameras,
            },
        ))
    })();
    collection.db.execute_batch("ROLLBACK")?;
    result
}

/// Liest die Seite in einer Lesetransaktion samt Stand.
pub fn page(
    collection: &Collection,
    filters: &Search,
    detector: &str,
) -> Result<(Option<Stamp>, Vec<Photo>)> {
    collection.db.execute_batch("BEGIN")?;
    let result = (|| {
        Ok((
            stamp(collection)?,
            metadata::search_page(collection, filters, detector)?,
        ))
    })();
    collection.db.execute_batch("ROLLBACK")?;
    result
}

/// Dieselbe Suche, unabhängig von der Seite?
pub fn same_query(a: &Search, b: &Search) -> bool {
    let mut a = a.clone();
    a.page = b.page;
    a == *b
}

/// Eine bekannte Zählung mit dem Stand, zu dem sie gehört.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Known {
    pub stamp: Stamp,
    /// Die gezählte Suche (die Seite spielt keine Rolle).
    pub filters: Search,
    pub counts: Counts,
    /// Fotos der ersten Seite beim Zählen, falls bekannt: zusätzliche Probe gegen eine
    /// zufällig gleiche Revision (z. B. nach dem Zurückspielen einer Sicherung).
    pub first_page: Option<Vec<i64>>,
}

impl Known {
    /// Darf diese Zählung für eine frisch gelesene Seite gelten?
    pub fn reusable(&self, stamp: Option<&Stamp>, filters: &Search, photos: &[Photo]) -> bool {
        stamp == Some(&self.stamp)
            && metadata::counts_follow_revision(filters)
            && same_query(&self.filters, filters)
            && (filters.page != 0
                || self.first_page.as_ref().is_none_or(|ids| {
                    ids.len() == photos.len() && ids.iter().zip(photos).all(|(id, p)| *id == p.id)
                }))
    }
}

/// Gespeicherte Startansicht einer Sammlung.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: u32,
    /// Sammlungspfad wie in den Einstellungen.
    pub collection: String,
    pub known: Known,
    pub photos: Vec<Photo>,
}

impl Snapshot {
    /// Nur die Suche ohne Filter auf ihrer ersten Seite ist eine Startansicht.
    pub fn new(collection: &str, known: Known, photos: Vec<Photo>) -> Option<Self> {
        (known.filters == Search::default()).then(|| Self {
            format: FORMAT,
            collection: collection.to_owned(),
            known,
            photos,
        })
    }
}

/// Datei der Startansicht einer Sammlung im Cache-Ordner.
pub fn file(dir: &Path, collection: &str) -> PathBuf {
    let key = blake3::hash(files::fold(collection).as_bytes()).to_hex();
    dir.join(format!("start-{}.json", &key[..20]))
}

/// Liest die Startansicht; `None` bei fehlender, fremder, zu großer oder kaputter Datei.
pub fn load(dir: &Path, collection: &str) -> Option<Snapshot> {
    let path = file(dir, collection);
    if path.metadata().ok()?.len() > crate::config::START_VIEW_MAX_BYTES {
        return None;
    }
    let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (snapshot.format == FORMAT
        && snapshot.collection == collection
        && snapshot.known.filters == Search::default()
        && snapshot.photos.len() <= crate::config::PAGE_SIZE)
        .then_some(snapshot)
}

/// Schreibt die Startansicht atomar (Zwischendatei, dann Umbenennen).
pub fn save(dir: &Path, snapshot: &Snapshot) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut temp, snapshot)?;
    temp.persist(file(dir, &snapshot.collection))?;
    Ok(())
}

/// Wie [`save`], Fehler nur ins Diagnose-Log: die Startansicht ist eine Beschleunigung.
pub fn store(dir: &Path, snapshot: &Snapshot) {
    if let Err(e) = save(dir, snapshot) {
        log::warn!(target: "sammlung", "startansicht_speichern fehler={:?}", format!("{e:#}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(id: i64) -> Photo {
        Photo {
            id,
            file_id: id,
            path: format!("a/{id}.jpg"),
            name: format!("{id}.jpg"),
            date: "2024:01:01 10:00:00".into(),
            camera: "Kamera".into(),
            lens: String::new(),
            iso: Some(100.0),
            size: 10,
            source: format!("a/{id}.jpg"),
            orientation: 1,
        }
    }
    fn known(revision: i64) -> Known {
        Known {
            stamp: Stamp {
                identity: "id".into(),
                revision,
            },
            filters: Search::default(),
            counts: Counts {
                total: 2,
                ..Default::default()
            },
            first_page: Some(vec![1, 2]),
        }
    }

    #[test]
    fn counts_are_reused_only_for_the_same_stamp_query_and_first_page() {
        let known = known(5);
        let photos = vec![photo(1), photo(2)];
        let same = Stamp {
            identity: "id".into(),
            revision: 5,
        };
        assert!(known.reusable(Some(&same), &Search::default(), &photos));
        let newer = Stamp {
            revision: 6,
            ..same.clone()
        };
        assert!(!known.reusable(Some(&newer), &Search::default(), &photos));
        assert!(!known.reusable(None, &Search::default(), &photos));
        let other = Stamp {
            identity: "anders".into(),
            ..same.clone()
        };
        assert!(!known.reusable(Some(&other), &Search::default(), &photos));
        // Andere erste Seite trotz gleicher Revision: nicht vertrauen.
        assert!(!known.reusable(Some(&same), &Search::default(), &[photo(1), photo(3)]));
        // Weitere Seiten derselben Suche nutzen die Zählung.
        let next = Search {
            page: 2,
            ..Default::default()
        };
        assert!(known.reusable(Some(&same), &next, &[photo(9)]));
        // Andere Suche oder eine, deren Treffer an Gesichtern hängen: nie.
        let folder = Search {
            folder: Some("a".into()),
            ..Default::default()
        };
        assert!(!known.reusable(Some(&same), &folder, &photos));
        let mut faces = known.clone();
        faces.filters.face_state = "with_faces".into();
        let with_faces = faces.filters.clone();
        assert!(!faces.reusable(Some(&same), &with_faces, &photos));
    }

    #[test]
    fn filters_that_read_meta_or_faces_do_not_follow_the_revision() {
        let rule = |field: &str, operator: &str, value: &str| Search {
            rules: vec![metadata::Rule {
                field: field.into(),
                operator: operator.into(),
                value: value.into(),
            }],
            ..Default::default()
        };
        assert!(metadata::counts_follow_revision(&Search::default()));
        assert!(metadata::counts_follow_revision(&rule("camera", "is", "x")));
        assert!(metadata::counts_follow_revision(&rule(
            "format", "is", "jpg"
        )));
        assert!(!metadata::counts_follow_revision(&rule(
            "rating", "min", "3"
        )));
        assert!(!metadata::counts_follow_revision(&rule(
            "aperture", "min", "2"
        )));
        assert!(!metadata::counts_follow_revision(&rule(
            "face_count",
            "min",
            "1"
        )));
        assert!(!metadata::counts_follow_revision(&Search {
            text: "anna".into(),
            ..Default::default()
        }));
        assert!(!metadata::counts_follow_revision(&Search {
            people_any: vec![1],
            ..Default::default()
        }));
    }

    #[test]
    fn snapshot_round_trips_and_rejects_foreign_or_broken_files() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let photos = vec![photo(1), photo(2)];
        let snapshot = Snapshot::new(r"E:\Bilder", known(3), photos.clone()).expect("Start");
        save(dir.path(), &snapshot)?;
        assert_eq!(load(dir.path(), r"E:\Bilder"), Some(snapshot.clone()));
        // Groß- und Kleinschreibung des Pfads ergeben dieselbe Datei.
        assert_eq!(
            file(dir.path(), r"e:\bilder"),
            file(dir.path(), r"E:\Bilder")
        );
        assert_eq!(load(dir.path(), r"F:\Andere"), None);
        // Eine gefilterte Suche ist keine Startansicht.
        let mut filtered = known(3);
        filtered.filters.text = "x".into();
        assert!(Snapshot::new(r"E:\Bilder", filtered, photos).is_none());
        std::fs::write(file(dir.path(), r"E:\Bilder"), b"{kaputt")?;
        assert_eq!(load(dir.path(), r"E:\Bilder"), None);
        Ok(())
    }
}
