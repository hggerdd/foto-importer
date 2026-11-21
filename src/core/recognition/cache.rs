//! Zwischenspeicher des inkrementellen Abgleichs (`MatchCache`) und sein Zustand.
use super::*;

/// Zwischenspeicher des inkrementellen Abgleichs für eine Sammlung und ein Modell: je
/// Gesicht der Vektor und der Personenscore gegen jede Person mit Referenzen (`NaN` ohne
/// Wert), dazu der Referenzstand, gegen den gerechnet wurde. Ändern sich die Referenzen
/// einer Person, wird nur ihre Spalte für alle Gesichter neu gerechnet; Rangfolge,
/// Ausschlüsse und die Entscheidung je Foto entstehen in jedem Lauf neu aus dem aktuellen
/// Stand. Er lebt nur im Arbeitsspeicher und ist jederzeit verwerfbar.
#[derive(Default)]
pub struct MatchCache {
    /// Sammlungspfad, Identität (`collection.id`) und Modell, für die gerechnet ist.
    pub(super) owner: Option<(std::path::PathBuf, String, String)>,
    /// Referenzen je Person (Gesichts-ID, Vektor), gegen die die Spalten gerechnet sind.
    pub(super) references: BTreeMap<i64, Vec<(i64, Vec<f32>)>>,
    /// Personen-ID je Spalte, wie `References::people`.
    pub(super) people: Vec<i64>,
    pub(super) faces: std::collections::HashMap<i64, CachedFace>,
}
pub(super) struct CachedFace {
    /// `rowid` der Merkmalszeile: `INSERT OR REPLACE` vergibt eine neue, ein ersetzter
    /// Vektor wird so erkannt.
    rowid: i64,
    pub(super) vector: Option<Vec<f32>>,
    pub(super) scores: Vec<f32>,
}
impl MatchCache {
    /// Zahl der zwischengespeicherten Gesichter (für Messungen).
    pub fn len(&self) -> usize {
        self.faces.len()
    }
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}
/// Gesichter je Rechenrunde; zwischen den Runden weicht der Abgleich Nutzeraktionen.
pub(super) const SCORE_ROUND: usize = 4096;
/// Zuordnungen je Schreibtransaktion.
pub(super) const WRITE_CHUNK: usize = 512;

/// Personenscores mehrerer Gesichter, auf bis zu `config::MATCH_THREADS` Threads verteilt.
/// Jedes Gesicht wird für sich gerechnet; die Werte hängen nicht von der Aufteilung ab.
pub(super) fn score_rows(
    references: &References,
    faces: &[(i64, Option<&[f32]>)],
) -> Vec<Vec<f32>> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, crate::config::MATCH_THREADS);
    let one = |(id, vector): &(i64, Option<&[f32]>)| match vector {
        Some(vector) => references.scores(vector, *id),
        None => vec![f32::NAN; references.people.len()],
    };
    if threads == 1 || faces.len() < 64 {
        return faces.iter().map(one).collect();
    }
    let size = faces.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let handles: Vec<_> = faces
            .chunks(size)
            .map(|chunk| scope.spawn(move || chunk.iter().map(one).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    })
}

fn same_references(a: &[(i64, Vec<f32>)], b: &[(i64, Vec<f32>)]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.0 == y.0
                && x.1.len() == y.1.len()
                && x.1
                    .iter()
                    .zip(&y.1)
                    .all(|(p, q)| p.to_bits() == q.to_bits())
        })
}

/// Was ein Lauf aus einem konsistenten Lesestand braucht (ohne Vektoren).
pub(super) struct MatchState {
    pub(super) entries: Entries,
    /// Alle Gesichter, nach Foto und ID; `vector` bleibt leer.
    pub(super) faces: Vec<MatchFace>,
    /// `rowid` der Merkmalszeile je Gesicht des Modells.
    pub(super) embeddings: std::collections::HashMap<i64, i64>,
    pub(super) rejections: BTreeMap<i64, BTreeSet<i64>>,
}
impl MatchState {
    pub(super) fn load(collection: &Collection, key: &str) -> Result<Self> {
        // Ein Leser mit offener Transaktion (Seitenabfrage) hat seinen Lesestand schon.
        let tx = if collection.db.is_autocommit() {
            Some(collection.db.unchecked_transaction()?)
        } else {
            None
        };
        let entries = References::load_entries(collection, key)?;
        // Ohne ORDER BY liest SQLite die Tabelle der Reihe nach statt je Zeile über den
        // Fotoindex; sortiert wird im Speicher.
        let mut faces = Vec::new();
        let mut statement = collection.db.prepare(
            "SELECT id,photo_id,person_id,assignment,confidence,ignored,retained,captured_at,
                    coalesce(retained=0 AND quality>=0.5,0)
             FROM faces",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let captured: Option<String> = row.get(7)?;
            faces.push(MatchFace {
                id: row.get(0)?,
                photo: row.get(1)?,
                person: row.get(2)?,
                assignment: row.get(3)?,
                confidence: row.get(4)?,
                ignored: row.get(5)?,
                retained: row.get(6)?,
                year: captured
                    .as_deref()
                    .and_then(|v| v.get(..4))
                    .and_then(|v| v.parse().ok()),
                wanted: row.get(8)?,
                vector: None,
            });
        }
        drop(rows);
        drop(statement);
        faces.sort_unstable_by_key(|f| (f.photo, f.id));
        let embeddings = collection
            .db
            .prepare("SELECT face_id,rowid FROM face_embeddings WHERE embedder_id=?")?
            .query_map([key], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut rejections: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
        let mut statement = collection
            .db
            .prepare("SELECT face_id,person_id FROM face_rejections")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            rejections
                .entry(row.get(0)?)
                .or_default()
                .insert(row.get(1)?);
        }
        drop(rows);
        drop(statement);
        if let Some(tx) = tx {
            tx.commit()?;
        }
        Ok(Self {
            entries,
            faces,
            embeddings,
            rejections,
        })
    }
}

impl MatchCache {
    /// Bringt den Zwischenspeicher auf den Stand von `state` und liefert die aktuellen
    /// Referenzen. Jeder Schritt hinterlässt einen in sich stimmigen Zwischenspeicher; ein
    /// Abbruch dazwischen verliert nur die Arbeit des laufenden Schritts.
    pub(super) fn sync(
        &mut self,
        collection: &Collection,
        key: &str,
        state: &mut MatchState,
        control: &Control,
    ) -> Result<References> {
        let (entries, people) = std::mem::take(&mut state.entries);
        let mut grouped: BTreeMap<i64, Vec<(i64, Vec<f32>)>> = BTreeMap::new();
        for (face, slot, vector) in &entries {
            grouped
                .entry(people[*slot].0)
                .or_default()
                .push((*face, vector.clone()));
        }
        let references = References::from_entries(entries, people);
        let ids: Vec<i64> = references.people.iter().map(|p| p.0).collect();
        // 1. Spalten der Personen, deren Referenzen sich geändert haben (oder neu sind).
        let changed: Vec<usize> = ids
            .iter()
            .enumerate()
            .filter(|(_, id)| {
                self.references
                    .get(id)
                    .is_none_or(|old| !same_references(old, &grouped[id]))
            })
            .map(|(slot, _)| slot)
            .collect();
        if ids != self.people || !changed.is_empty() {
            let partial = References::from_entries(
                changed
                    .iter()
                    .enumerate()
                    .flat_map(|(column, slot)| {
                        grouped[&ids[*slot]]
                            .iter()
                            .map(move |(face, vector)| (*face, column, vector.clone()))
                    })
                    .collect(),
                changed
                    .iter()
                    .map(|slot| references.people[*slot].clone())
                    .collect(),
            );
            let mut order: Vec<i64> = self.faces.keys().copied().collect();
            order.sort_unstable();
            let mut computed: Vec<Vec<f32>> = Vec::with_capacity(order.len());
            for round in order.chunks(SCORE_ROUND) {
                control.checkpoint()?;
                let work: Vec<(i64, Option<&[f32]>)> = round
                    .iter()
                    .map(|id| (*id, self.faces[id].vector.as_deref()))
                    .collect();
                computed.extend(score_rows(&partial, &work));
            }
            // Erst jetzt übernehmen: Ein Abbruch oben lässt den alten Stand stehen.
            let previous: BTreeMap<i64, usize> = self
                .people
                .iter()
                .enumerate()
                .map(|(column, id)| (*id, column))
                .collect();
            let mut column_of: Vec<Option<usize>> = vec![None; ids.len()];
            for (column, slot) in changed.iter().enumerate() {
                column_of[*slot] = Some(column);
            }
            for (id, fresh) in order.iter().zip(computed) {
                let Some(face) = self.faces.get_mut(id) else {
                    continue;
                };
                face.scores = ids
                    .iter()
                    .enumerate()
                    .map(|(slot, person)| match column_of[slot] {
                        Some(column) => fresh[column],
                        None => face.scores[previous[person]],
                    })
                    .collect();
            }
            self.people = ids;
            self.references = grouped;
        }
        // 2. Gesichter: verschwundene, ersetzte oder nicht mehr verglichene entfernen,
        // fehlende mit allen Spalten rechnen.
        let wanted: std::collections::HashMap<i64, i64> = state
            .faces
            .iter()
            .filter(|f| f.wanted)
            .filter_map(|f| state.embeddings.get(&f.id).map(|rowid| (f.id, *rowid)))
            .collect();
        self.faces
            .retain(|id, face| wanted.get(id) == Some(&face.rowid));
        let mut missing: Vec<i64> = wanted
            .keys()
            .filter(|id| !self.faces.contains_key(id))
            .copied()
            .collect();
        missing.sort_unstable();
        let total = missing.len();
        control.progress(0, total);
        // Viele fehlende Gesichter (erster Lauf): Merkmale in einem Durchgang lesen; sonst
        // gezielt je Gesicht über den Primärschlüssel.
        let scan = missing.len() > 2048 && missing.len() * 4 > wanted.len();
        let mut done = 0;
        if scan {
            let need: BTreeSet<i64> = missing.iter().copied().collect();
            let mut statement = collection
                .db
                .prepare("SELECT face_id,rowid,vec FROM face_embeddings WHERE embedder_id=?")?;
            let mut rows = statement.query([key])?;
            let mut loaded = Vec::with_capacity(SCORE_ROUND);
            loop {
                let row = rows.next()?;
                let end = row.is_none();
                if let Some(row) = row {
                    let face: i64 = row.get(0)?;
                    if need.contains(&face) {
                        let bytes: Option<Vec<u8>> = row.get(2)?;
                        loaded.push((face, row.get(1)?, bytes.as_deref().map(decode).transpose()?));
                    }
                }
                if loaded.len() >= SCORE_ROUND || (end && !loaded.is_empty()) {
                    control.checkpoint()?;
                    done += loaded.len();
                    self.insert(&references, std::mem::take(&mut loaded));
                    control.progress(done, total);
                }
                if end {
                    break;
                }
            }
        } else {
            let mut statement = collection.db.prepare_cached(
                "SELECT rowid,vec FROM face_embeddings WHERE face_id=? AND embedder_id=?",
            )?;
            for round in missing.chunks(SCORE_ROUND) {
                control.checkpoint()?;
                let mut loaded = Vec::with_capacity(round.len());
                for id in round {
                    let found: Option<(i64, Option<Vec<u8>>)> = statement
                        .query_row(params![id, key], |r| Ok((r.get(0)?, r.get(1)?)))
                        .optional()?;
                    if let Some((rowid, bytes)) = found {
                        loaded.push((*id, rowid, bytes.as_deref().map(decode).transpose()?));
                    }
                }
                done += round.len();
                self.insert(&references, loaded);
                control.progress(done, total);
            }
        }
        Ok(references)
    }
    fn insert(&mut self, references: &References, loaded: Vec<(i64, i64, Option<Vec<f32>>)>) {
        let work: Vec<(i64, Option<&[f32]>)> = loaded
            .iter()
            .map(|(id, _, vector)| (*id, vector.as_deref()))
            .collect();
        let rows = score_rows(references, &work);
        for ((id, rowid, vector), scores) in loaded.into_iter().zip(rows) {
            self.faces.insert(
                id,
                CachedFace {
                    rowid,
                    vector,
                    scores,
                },
            );
        }
    }
}

/// Eigentümer eines Zwischenspeichers: Sammlungspfad, Identität (`collection.id`) und
/// Modell. Eine andere Sammlung unter demselben Pfad (ersetzte Datenbank) baut neu auf.
pub(super) fn cache_owner(
    collection: &Collection,
    key: &str,
) -> Result<(std::path::PathBuf, String, String)> {
    Ok((
        collection.root.clone(),
        crate::core::revision::identity(&collection.db)?,
        key.to_string(),
    ))
}

/// Zwischenspeicher des laufenden Programms (eine Sammlung, ein Modell zugleich).
static MATCH_CACHE: std::sync::Mutex<Option<MatchCache>> = std::sync::Mutex::new(None);

/// Sperre des Zwischenspeichers. Ist sie durch eine Panik vergiftet, kann der Inhalt halb
/// geändert sein: er wird verworfen (der nächste Abgleich baut ihn neu auf) und gewarnt.
pub(super) fn match_cache() -> std::sync::MutexGuard<'static, Option<MatchCache>> {
    MATCH_CACHE.lock().unwrap_or_else(|poisoned| {
        log::warn!(target: "gesichter", "abgleich_cache_vergiftet verworfen=ja");
        let mut guard = poisoned.into_inner();
        *guard = None;
        MATCH_CACHE.clear_poison();
        guard
    })
}
