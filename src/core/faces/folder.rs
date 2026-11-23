//! Ordnerfilter der Gesichterseiten: zeigt nur Gesichter, deren Foto (Analysequelle
//! `faces.source_relpath`) im gewählten Ordner oder darunter liegt. Vorschläge, Referenzen,
//! Werte und Reihenfolge bleiben sammlungsweit; der Filter schränkt nur die Anzeige ein.
use super::*;
use std::collections::HashSet;

/// Ein Ordner der Sammlung (relativer Pfad mit `/`), samt Unterordnern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceFolder {
    folder: String,
}

impl FaceFolder {
    /// Normalisiert `folder` (Schrägstriche, Ränder); leer heißt „alle Ordner“ (`None`).
    pub fn new(folder: &str) -> Option<Self> {
        let folder = folder.replace('\\', "/").trim_matches('/').to_string();
        (!folder.is_empty()).then_some(Self { folder })
    }
    pub fn path(&self) -> &str {
        &self.folder
    }
    /// Bereichsgrenzen für Pfade unter dem Ordner: `ordner/` ≤ Pfad < `ordner0` (`0` folgt
    /// in der Byte-Ordnung direkt auf `/`). So nutzt die Abfrage den Index auf `files.relpath`
    /// und „Party2/…“ gehört nie zu „Party“.
    fn bounds(&self) -> (String, String) {
        (format!("{}/", self.folder), format!("{}0", self.folder))
    }
    /// Liegt der relative Pfad `relpath` im Ordner oder darunter? (Gleiche Regel wie SQL.)
    pub fn contains(&self, relpath: &str) -> bool {
        let (lower, upper) = self.bounds();
        relpath >= lower.as_str() && relpath < upper.as_str()
    }
    /// SQL-Bedingung auf ein Gesicht `f`: seine Analysequelle liegt im Ordner. Der Umweg über
    /// die Fotos des Ordners (Index auf `files.relpath`, dann `ix_faces_photo`) liest nur die
    /// Gesichter dieses Ordners statt aller. Die Grenzen sind als Literale eingesetzt
    /// (Anführungszeichen verdoppelt), damit jede Seitenabfrage ihre Parameter behält.
    pub(super) fn predicate(&self) -> String {
        let (lower, upper) = self.bounds();
        let (lower, upper) = (quote(&lower), quote(&upper));
        format!(
            "f.photo_id IN (SELECT fi.photo_id FROM files fi WHERE fi.relpath>={lower} AND fi.relpath<{upper}) AND f.source_relpath>={lower} AND f.source_relpath<{upper}"
        )
    }
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Zahlen der Gesichterseiten im gewählten Ordner (statt der sammlungsweiten aus [`Status`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderCounts {
    /// Der gefilterte Ordner (normalisiert).
    pub folder: String,
    /// Vorhandene Fotos des Ordners mit Analysequelle (dieselbe Menge wie die Fotoanalyse mit
    /// Ordnerbezug).
    pub photos: usize,
    /// Davon noch nicht mit dem aktiven Detektor analysiert.
    pub unanalysed: usize,
    /// „Benennen“: offene Gesichter in einer Ähnlichkeitsgruppe.
    pub nameable: usize,
    /// Offene Gesichter insgesamt (wie [`Status::unknown`]).
    pub unknown: usize,
    pub review: usize,
    pub confirmed: usize,
    pub ignored: usize,
    /// Bestätigte Gesichter der gewählten Person (Umschalter „Bestätigt“ unter Personen).
    pub person_confirmed: usize,
}

impl FolderCounts {
    /// Offene Gesichter ohne Gruppe (Reiter „Ungeeignet“), wie [`Status::unsuitable`].
    pub fn unsuitable(&self) -> usize {
        self.unknown.saturating_sub(self.nameable)
    }
}

/// Zählt die Gesichter des Ordners je Reiter in einem Durchgang über seine Gesichter, dazu
/// die noch nicht analysierten Fotos (aktiver Detektor `detector`).
pub(super) fn counts(
    collection: &Collection,
    folder: &FaceFolder,
    embedder: &str,
    detector: &str,
    person: Option<i64>,
) -> Result<FolderCounts> {
    let predicate = folder.predicate();
    let mut counts = FolderCounts {
        folder: folder.path().to_string(),
        ..FolderCounts::default()
    };
    // Bedingungen wie in `status` (sammlungsweit) bzw. wie die Seiten.
    let sql = format!(
        "SELECT
           coalesce(sum(f.ignored=0 AND f.person_id IS NULL),0),
           coalesce(sum(f.ignored=0 AND f.person_id IS NULL AND EXISTS(SELECT 1 FROM face_group_members g WHERE g.face_id=f.id AND g.embedder_id=?1)),0),
           coalesce(sum(f.ignored=0 AND f.assignment IN ('suggested','auto')),0),
           coalesce(sum(f.assignment='user'),0),
           coalesce(sum(f.ignored=1),0),
           coalesce(sum(f.assignment='user' AND f.ignored=0 AND f.person_id IS ?2),0)
         FROM faces f WHERE f.retained=0 AND {predicate}"
    );
    (
        counts.unknown,
        counts.nameable,
        counts.review,
        counts.confirmed,
        counts.ignored,
        counts.person_confirmed,
    ) = collection
        .db
        .query_row(&sql, params![embedder, person], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?;
    if person.is_none() {
        counts.person_confirmed = 0;
    }
    let (lower, upper) = folder.bounds();
    // Dieselbe Fotomenge wie `scope::analysis_order` (Analysequelle vorhanden).
    (counts.photos, counts.unanalysed) = collection.db.query_row(
        "SELECT count(*),coalesce(sum(NOT EXISTS(SELECT 1 FROM face_jobs j WHERE j.photo_id=s.photo_id AND j.stage='detect' AND j.model_id=?1 AND j.status='done')),0)
         FROM files fi JOIN face_sources s ON s.photo_id=fi.photo_id AND s.file_id=fi.id
         WHERE fi.status='present' AND fi.relpath>=?2 AND fi.relpath<?3",
        params![detector, lower, upper],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(counts)
}

/// IDs aller aktiven Gesichter des Ordners (zum Filtern von Listen, die sammlungsweit
/// berechnet werden: Vorschläge, wahrscheinliche Gesichter).
pub(super) fn face_ids(collection: &Collection, folder: &FaceFolder) -> Result<HashSet<i64>> {
    Ok(collection
        .db
        .prepare(&format!(
            "SELECT f.id FROM faces f WHERE f.retained=0 AND {}",
            folder.predicate()
        ))?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Gruppenmitglieder (Gruppe, Gesicht) der offenen Gesichter des Ordners. Feste Reihenfolge
/// (CROSS JOIN) und `+` vor den Spalten der Teilindizes: SQLite läse sonst alle
/// Gruppenmitglieder oder alle offenen Gesichter (verstreut, an `E:\Bilder` kalt über 3 s)
/// statt nur der Gesichter des Ordners über `ix_faces_photo`.
fn unknown_sql(folder: &FaceFolder) -> String {
    format!(
        "SELECT g.group_id,f.id FROM faces f CROSS JOIN face_group_members g CROSS JOIN face_group_catalog c
         WHERE g.embedder_id=?1 AND g.face_id=f.id AND c.group_id=g.group_id AND c.embedder_id=?1
           AND f.retained=0 AND +f.ignored=0 AND +f.person_id IS NULL AND c.members>0 AND c.members<=?2 AND {}",
        folder.predicate()
    )
}

/// Zeilen von EXPLAIN QUERY PLAN der Gruppenabfrage von „Benennen“ im Ordner
/// (Regressionstest: Gesichter nur über die Fotos des Ordners).
pub fn unknown_plan(collection: &Collection, folder: &FaceFolder) -> Result<Vec<String>> {
    let mut statement = collection
        .db
        .prepare(&format!("EXPLAIN QUERY PLAN {}", unknown_sql(folder)))?;
    let rows = statement.query_map(params!["", 0], |r| r.get::<_, String>(3))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Seite „Benennen“ im Ordner: die gespeicherten Ähnlichkeitsgruppen, jede nur mit ihren
/// offenen Gesichtern aus dem Ordner; Gruppen ohne solche entfallen. Größte (im Ordner) zuerst,
/// dann nach Gruppen-ID; Teile zu `size` nach Gesichts-ID wie ohne Filter. Gruppen werden nie
/// gemischt. Gruppen über `CLUSTER_MAX` (veralteter Katalog) bleiben wie ohne Filter aus.
/// Ergebnis: Gruppen der Seite und Zahl aller Teile.
pub(super) fn unknown_page(
    collection: &Collection,
    folder: &FaceFolder,
    embedder: &str,
    page: usize,
    size: usize,
) -> Result<(Vec<Vec<Face>>, usize)> {
    let mut members: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let mut statement = collection.db.prepare(&unknown_sql(folder))?;
    let mut rows = statement.query(params![embedder, crate::config::CLUSTER_MAX as i64])?;
    while let Some(row) = rows.next()? {
        members.entry(row.get(0)?).or_default().push(row.get(1)?);
    }
    let mut clusters: Vec<(i64, Vec<i64>)> = members.into_iter().collect();
    for (_, faces) in &mut clusters {
        faces.sort_unstable();
    }
    clusters.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    let parts: Vec<&[i64]> = clusters
        .iter()
        .flat_map(|(_, faces)| faces.chunks(size))
        .collect();
    let per_page = crate::config::face_groups_per_page(size);
    let mut groups = Vec::new();
    for ids in parts.iter().skip(page * per_page).take(per_page) {
        let rows = super::pages::faces_in_order(collection, ids)?;
        if !rows.is_empty() {
            groups.push(rows);
        }
    }
    Ok((groups, parts.len()))
}

#[cfg(test)]
mod tests {
    use super::FaceFolder;

    #[test]
    fn folder_contains_its_subfolders_but_not_siblings_with_the_same_prefix() {
        let party = FaceFolder::new("\\Party/").expect("Ordner");
        assert_eq!(party.path(), "Party");
        assert!(party.contains("Party/a.jpg"));
        assert!(party.contains("Party/jpg/a.jpg"));
        assert!(!party.contains("Party2/a.jpg"));
        assert!(!party.contains("Party.jpg"));
        assert!(!party.contains("Party"));
        assert!(!party.contains("Partx/a.jpg"));
        assert!(FaceFolder::new("/").is_none());
        assert!(FaceFolder::new("").is_none());
        let quoted = FaceFolder::new("Anna's Feier").expect("Ordner");
        assert!(quoted.predicate().contains("'Anna''s Feier/'"));
    }
}
