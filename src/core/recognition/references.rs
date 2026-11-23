//! Referenzgesichter je Embedder im Speicher, Top-3-Mittel und Rangfolge der Personen.
use super::*;

/// Referenzgesichter eines Embedders, die für jedes Vergleichsgesicht gleich sind (ADR 0066).
/// Einmal je Seite oder Abgleichslauf aus einem konsistenten Lesestand geladen, statt für
/// jedes Gesicht erneut aus SQLite zu lesen. Gesichtsabhängige Ausschlüsse (eigenes Gesicht,
/// Ablehnungen, Personen desselben Fotos) werden je Aufruf angewendet.
///
/// Die Vektoren liegen in Blöcken zu [`LANES`] Referenzen, je Dimension nebeneinander. Ein
/// Vergleich rechnet so acht Skalarprodukte gleichzeitig; jedes summiert in derselben
/// Reihenfolge wie `inference::cosine` und ergibt bitgleich denselben Wert.
pub struct References {
    /// Gesichts-ID und dichter Personenindex je Referenz der Blöcke.
    faces: Vec<i64>,
    slots: Vec<usize>,
    dim: usize,
    blocks: Vec<[f32; LANES]>,
    /// Referenzen mit abweichender Länge (nur bei beschädigten Daten); einzeln verglichen.
    other: Vec<(i64, usize, Vec<f32>)>,
    /// Personen-ID und Name je dichtem Index.
    pub(super) people: Vec<(i64, String)>,
}
const LANES: usize = 8;
/// Referenzen (Gesichts-ID, Index in der Personenliste, Vektor) und Personen (ID, Name).
pub(super) type Entries = (Vec<(i64, usize, Vec<f32>)>, Vec<(i64, String)>);
/// Die bis zu drei besten Werte einer Person, absteigend.
#[derive(Clone, Copy, Default)]
pub(super) struct Top3 {
    pub(super) values: [f32; 3],
    pub(super) len: usize,
}
impl Top3 {
    /// Wie „anhängen, stabil absteigend sortieren, auf drei kürzen“.
    pub(super) fn push(&mut self, score: f32) {
        let at = self.values[..self.len]
            .iter()
            .position(|v| v.total_cmp(&score).is_lt())
            .unwrap_or(self.len);
        if at >= 3 {
            return;
        }
        let end = self.len.min(2);
        self.values.copy_within(at..end, at + 1);
        self.values[at] = score;
        self.len = (self.len + 1).min(3);
    }
    pub(super) fn mean(&self) -> f32 {
        self.values[..self.len].iter().sum::<f32>() / self.len as f32
    }
}
/// Mittel der bis zu drei größten Werte (absteigend summiert), `None` ohne Werte. Die eine
/// Definition des Personenscores: Abgleich, Auswertung, Validierung und Dublettenprüfung
/// rechnen alle hierüber.
pub(crate) fn top3_mean(values: impl IntoIterator<Item = f32>) -> Option<f32> {
    let mut top = Top3::default();
    values.into_iter().for_each(|v| top.push(v));
    (top.len > 0).then(|| top.mean())
}
/// [`top3_mean`] je Schlüssel, etwa Personen-ID zu Kosinuswerten.
pub(crate) fn top3_means<K: Ord>(scores: impl IntoIterator<Item = (K, f32)>) -> BTreeMap<K, f32> {
    let mut map: BTreeMap<K, Top3> = BTreeMap::new();
    for (key, score) in scores {
        map.entry(key).or_default().push(score);
    }
    map.into_iter().map(|(k, t)| (k, t.mean())).collect()
}
impl References {
    pub fn load(collection: &Collection, key: &str) -> Result<Self> {
        let (entries, people) = Self::load_entries(collection, key)?;
        Ok(Self::from_entries(entries, people))
    }
    /// Referenzen als Gesichts-ID, Index in `people`, Vektor; nach Person, dann Gesicht.
    pub(super) fn load_entries(collection: &Collection, key: &str) -> Result<Entries> {
        // Die Teilnahme an `training_set` steht als nicht korrelierte Unterabfrage da: SQLite
        // bildet sie einmal. Korreliert wählte der Planer ohne `ANALYZE`-Statistik den Index
        // nach Modell und Teil und las je bestätigtem Gesicht alle Referenzzeilen. `CROSS JOIN`
        // hält die bestätigten Gesichter (Teilindex `ix_faces_reference`) außen, statt alle
        // Vektoren des Modells zu lesen.
        let mut statement=collection.db.prepare("SELECT f.id,f.person_id,e.vec,coalesce(a.value,'Unbenannt') FROM faces f CROSS JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=?1 LEFT JOIN person_attrs a ON a.person_id=f.person_id AND a.key='name' WHERE f.assignment='user' AND f.person_id IS NOT NULL AND f.ignored=0 AND f.quality>=0.5 AND (f.id IN (SELECT face_id FROM training_set WHERE embedder_id=?1 AND split='ref') OR f.person_id NOT IN (SELECT person_id FROM training_set WHERE embedder_id=?1 AND person_id IS NOT NULL)) AND NOT EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=e.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_review v WHERE v.face_id=f.id AND v.embedder_id=e.embedder_id) ORDER BY f.person_id,f.id")?;
        let mut rows = statement.query([key])?;
        let mut entries = Vec::new();
        let mut people: Vec<(i64, String)> = Vec::new();
        while let Some(row) = rows.next()? {
            let person: i64 = row.get(1)?;
            let vector = decode(&row.get::<_, Vec<u8>>(2)?)?;
            if people.last().is_none_or(|p| p.0 != person) {
                people.push((person, row.get(3).unwrap_or_default()));
            }
            entries.push((row.get::<_, i64>(0)?, people.len() - 1, vector));
        }
        Ok((entries, people))
    }
    /// `entries`: Gesichts-ID, Index in `people`, Vektor.
    pub(super) fn from_entries(
        entries: Vec<(i64, usize, Vec<f32>)>,
        people: Vec<(i64, String)>,
    ) -> Self {
        let dim = entries.first().map_or(0, |e| e.2.len());
        let mut references = Self {
            faces: Vec::new(),
            slots: Vec::new(),
            dim,
            blocks: Vec::new(),
            other: Vec::new(),
            people,
        };
        for (face, slot, vector) in entries {
            if dim == 0 || vector.len() != dim {
                references.other.push((face, slot, vector));
                continue;
            }
            let index = references.faces.len();
            if index.is_multiple_of(LANES) {
                references
                    .blocks
                    .extend(std::iter::repeat_n([0.0; LANES], dim));
            }
            let block = (index / LANES) * dim;
            for (d, value) in vector.iter().enumerate() {
                references.blocks[block + d][index % LANES] = *value;
            }
            references.faces.push(face);
            references.slots.push(slot);
        }
        references
    }
    /// Rangfolge der Personen für einen Vektor: Mittel der bis zu drei besten Kosinuswerte
    /// je Person, absteigend (Gleichstand: Name, dann ID). `own` zählt nie als eigene
    /// Referenz; `excluded` schließt Personen aus.
    pub(super) fn rank(
        &self,
        query: &[f32],
        own: i64,
        excluded: impl Fn(i64) -> bool,
    ) -> Vec<(i64, f32)> {
        self.order(&self.scores(query, own), excluded)
    }
    /// Personenscore je Index in `people` (Mittel der bis zu drei besten Kosinuswerte), `NaN`
    /// für Personen ohne vergleichbare Referenz. Der Wert einer Person hängt nur von ihren
    /// eigenen Referenzen ab; Ausschlüsse filtern erst danach (`order`). Darauf beruht der
    /// inkrementelle Abgleich ([`MatchCache`]).
    pub(super) fn scores(&self, query: &[f32], own: i64) -> Vec<f32> {
        self.scores_without(query, |face| face == own)
    }
    /// Wie [`Self::scores`], ohne die Referenzen, für die `skip(Gesichts-ID)` gilt (etwa das
    /// eigene Gesicht und Referenzen vom selben Aufnahmetag, Personenstatistik). Gleicher
    /// Rechenweg, daher bitgleich, wenn `skip` nur das eigene Gesicht trifft.
    pub(super) fn scores_without(&self, query: &[f32], skip: impl Fn(i64) -> bool) -> Vec<f32> {
        let mut top = vec![Top3::default(); self.people.len()];
        if self.dim > 0 && query.len() == self.dim {
            for (block, lanes) in self.blocks.chunks_exact(self.dim).enumerate() {
                let mut sums = [0.0f32; LANES];
                for (q, lane) in query.iter().zip(lanes) {
                    for k in 0..LANES {
                        sums[k] += q * lane[k];
                    }
                }
                for (k, sum) in sums.iter().enumerate() {
                    let index = block * LANES + k;
                    let Some(slot) = self.slots.get(index) else {
                        break;
                    };
                    if sum.is_finite() && !skip(self.faces[index]) {
                        top[*slot].push(sum.clamp(-1.0, 1.0));
                    }
                }
            }
        }
        for (face, slot, vector) in &self.other {
            if !skip(*face)
                && let Some(score) = inference::cosine(query, vector)
            {
                top[*slot].push(score);
            }
        }
        top.iter()
            .map(|t| if t.len > 0 { t.mean() } else { f32::NAN })
            .collect()
    }
    /// Rangfolge aus Personenscores (`scores`): zulässige Personen mit Wert, absteigend
    /// (Gleichstand: Name, dann ID).
    fn order(&self, scores: &[f32], excluded: impl Fn(i64) -> bool) -> Vec<(i64, f32)> {
        let mut result: Vec<(usize, f32)> = scores
            .iter()
            .enumerate()
            .filter(|(slot, score)| !score.is_nan() && !excluded(self.people[*slot].0))
            .map(|(slot, score)| (slot, *score))
            .collect();
        result.sort_by(|a, b| self.precedence(*a, *b));
        result
            .into_iter()
            .map(|(slot, score)| (self.people[slot].0, score))
            .collect()
    }
    /// Reihenfolge zweier Personen (Index, Score): höherer Score zuerst, dann Name, dann ID.
    /// Die IDs sind eindeutig, die Ordnung also streng.
    fn precedence(&self, a: (usize, f32), b: (usize, f32)) -> std::cmp::Ordering {
        let (x, y) = (&self.people[a.0], &self.people[b.0]);
        b.1.total_cmp(&a.1).then(x.1.cmp(&y.1)).then(x.0.cmp(&y.0))
    }
    /// Die ersten zwei Einträge von `order` ohne vollständiges Sortieren (mehr braucht
    /// `Limits::accept` nicht). Bei strenger Ordnung sind sie dieselben.
    pub(super) fn best_two(
        &self,
        scores: &[f32],
        excluded: impl Fn(i64) -> bool,
    ) -> Vec<(i64, f32)> {
        let mut best: [Option<(usize, f32)>; 2] = [None, None];
        for (slot, score) in scores.iter().enumerate() {
            if score.is_nan() || excluded(self.people[slot].0) {
                continue;
            }
            let entry = (slot, *score);
            let before = |other: Option<(usize, f32)>| {
                other.is_none_or(|o| self.precedence(entry, o).is_lt())
            };
            if before(best[0]) {
                best[1] = best[0];
                best[0] = Some(entry);
            } else if before(best[1]) {
                best[1] = Some(entry);
            }
        }
        best.into_iter()
            .flatten()
            .map(|(slot, score)| (self.people[slot].0, score))
            .collect()
    }
    fn name(&self, person: i64) -> String {
        self.people
            .iter()
            .find(|p| p.0 == person)
            .map(|p| p.1.clone())
            .unwrap_or_default()
    }
}

/// Referenzvektoren je Person (normiert), genau die Auswahl des Abgleichs (`References`).
pub fn reference_vectors(
    collection: &Collection,
    key: &str,
) -> Result<BTreeMap<i64, Vec<Vec<f32>>>> {
    let (entries, people) = References::load_entries(collection, key)?;
    let mut out: BTreeMap<i64, Vec<Vec<f32>>> = BTreeMap::new();
    for (_, slot, vector) in entries {
        out.entry(people[slot].0).or_default().push(vector);
    }
    Ok(out)
}

pub(super) fn ranked_with(
    collection: &Collection,
    references: &References,
    face: i64,
    key: &str,
) -> Result<Vec<(i64, String, f32)>> {
    let bytes: Option<Vec<u8>> = collection
        .db
        .query_row(
            "SELECT e.vec FROM face_embeddings e JOIN faces f ON f.id=e.face_id
             WHERE e.face_id=? AND e.embedder_id=? AND f.retained=0 AND f.quality>=0.5",
            params![face, key],
            |r| r.get(0),
        )
        .optional()?;
    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    let query = decode(&bytes)?;
    let photo: i64 =
        collection
            .db
            .query_row("SELECT photo_id FROM faces WHERE id=?", [face], |r| {
                r.get(0)
            })?;
    let rejected: BTreeSet<i64> = collection
        .db
        .prepare_cached("SELECT person_id FROM face_rejections WHERE face_id=?")?
        .query_map([face], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let occupied: BTreeSet<i64> = collection
        .db
        .prepare_cached("SELECT person_id FROM faces WHERE photo_id=? AND assignment='user' AND id!=? AND person_id IS NOT NULL")?
        .query_map(params![photo, face], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(references
        .rank(&query, face, |person| {
            rejected.contains(&person) || occupied.contains(&person)
        })
        .into_iter()
        .map(|(person, score)| (person, references.name(person), score))
        .collect())
}
