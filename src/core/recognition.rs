use super::{
    collection::Collection,
    faces::{self, decode},
    inference,
    jobs::Control,
    matching::{self, Candidate},
};
use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

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
    people: Vec<(i64, String)>,
}
const LANES: usize = 8;
/// Referenzen (Gesichts-ID, Index in der Personenliste, Vektor) und Personen (ID, Name).
type Entries = (Vec<(i64, usize, Vec<f32>)>, Vec<(i64, String)>);
/// Die bis zu drei besten Werte einer Person, absteigend.
#[derive(Clone, Copy, Default)]
struct Top3 {
    values: [f32; 3],
    len: usize,
}
impl Top3 {
    /// Wie „anhängen, stabil absteigend sortieren, auf drei kürzen“.
    fn push(&mut self, score: f32) {
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
    fn mean(&self) -> f32 {
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
    fn load_entries(collection: &Collection, key: &str) -> Result<Entries> {
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
    fn from_entries(entries: Vec<(i64, usize, Vec<f32>)>, people: Vec<(i64, String)>) -> Self {
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
    fn rank(&self, query: &[f32], own: i64, excluded: impl Fn(i64) -> bool) -> Vec<(i64, f32)> {
        self.order(&self.scores(query, own), excluded)
    }
    /// Personenscore je Index in `people` (Mittel der bis zu drei besten Kosinuswerte), `NaN`
    /// für Personen ohne vergleichbare Referenz. Der Wert einer Person hängt nur von ihren
    /// eigenen Referenzen ab; Ausschlüsse filtern erst danach (`order`). Darauf beruht der
    /// inkrementelle Abgleich ([`MatchCache`]).
    fn scores(&self, query: &[f32], own: i64) -> Vec<f32> {
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
                    if self.faces[index] != own && sum.is_finite() {
                        top[*slot].push(sum.clamp(-1.0, 1.0));
                    }
                }
            }
        }
        for (face, slot, vector) in &self.other {
            if *face != own
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
    fn best_two(&self, scores: &[f32], excluded: impl Fn(i64) -> bool) -> Vec<(i64, f32)> {
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

fn ranked_with(
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
/// Namensvorschläge für ein Gesicht nach dem Ähnlichkeitsvergleich (Score = Kosinus).
#[derive(Clone, Default)]
pub struct Suggestions {
    pub items: Vec<(i64, String, f32)>,
    /// Repräsentatives Gesicht je vorgeschlagener Person (Medoid).
    pub portraits: BTreeMap<i64, faces::Face>,
}
/// Je Seite geteilter Zwischenspeicher (ADR 0066): Referenzvektoren und
/// Porträts werden einmal je konsistentem Lesestand geladen. Er lebt nur so lange wie
/// der Aufruf, daher kann er nie veraltete Zuordnungen zeigen.
#[derive(Default)]
pub struct SuggestionCache {
    references: Option<References>,
    portraits: BTreeMap<i64, Option<faces::Face>>,
}
impl SuggestionCache {
    fn references(&mut self, collection: &Collection, key: &str) -> Result<&References> {
        if self.references.is_none() {
            self.references = Some(References::load(collection, key)?);
        }
        Ok(self.references.as_ref().expect("geladen"))
    }
}
fn with_portraits(
    collection: &Collection,
    cache: &mut SuggestionCache,
    mut found: Suggestions,
    key: &str,
) -> Suggestions {
    for (person, _, _) in &found.items {
        // Ein fehlendes Bild darf den Vorschlag nicht verhindern.
        let face = cache.portraits.entry(*person).or_insert_with(|| {
            faces::person_portrait(collection, *person, key)
                .ok()
                .flatten()
        });
        if let Some(face) = face {
            found.portraits.insert(*person, face.clone());
        }
    }
    found
}
pub fn suggestions(collection: &Collection, face: i64, key: &str) -> Result<Suggestions> {
    let mut cache = SuggestionCache::default();
    let mut all = scored(collection, &mut cache, face, key)?;
    all.items.truncate(3);
    Ok(with_portraits(collection, &mut cache, all, key))
}
/// Gemeinsamer Vorschlag für eine Gruppe: Mittel der Einzelwerte über
/// höchstens `config::GROUP_SUGGESTION_FACES` Gesichter (begrenzt die Lesezeit je Seite).
pub fn group_suggestions(collection: &Collection, faces: &[i64], key: &str) -> Result<Suggestions> {
    group_suggestions_cached(collection, &mut SuggestionCache::default(), faces, key)
}
pub fn group_suggestions_cached(
    collection: &Collection,
    cache: &mut SuggestionCache,
    faces: &[i64],
    key: &str,
) -> Result<Suggestions> {
    let mut totals: BTreeMap<i64, (String, f32)> = BTreeMap::new();
    let mut used = 0usize;
    for face in faces.iter().take(crate::config::GROUP_SUGGESTION_FACES) {
        let one = scored(collection, cache, *face, key)?;
        if one.items.is_empty() {
            continue;
        }
        used += 1;
        for (id, name, score) in one.items {
            totals.entry(id).or_insert((name, 0.0)).1 += score;
        }
    }
    let mut items: Vec<_> = totals
        .into_iter()
        .map(|(id, (name, sum))| (id, name, sum / used.max(1) as f32))
        .collect();
    items.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
    items.truncate(3);
    Ok(with_portraits(
        collection,
        cache,
        Suggestions {
            items,
            portraits: BTreeMap::new(),
        },
        key,
    ))
}
fn scored(
    collection: &Collection,
    cache: &mut SuggestionCache,
    face: i64,
    key: &str,
) -> Result<Suggestions> {
    let items = ranked_with(collection, cache.references(collection, key)?, face, key)?;
    Ok(Suggestions {
        items,
        portraits: BTreeMap::new(),
    })
}

fn option(collection: &Collection, name: &str, default: f32) -> Result<f32> {
    let model = faces::setting(collection, "rust.embedder_key")?.unwrap_or_default();
    let name = format!(
        "rust.thresholds.{model}.{}",
        name.strip_prefix("rust.").unwrap_or(name)
    );
    let raw: Option<String> = collection
        .db
        .query_row(
            "SELECT value FROM face_settings WHERE key=?",
            [&name],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|v| v.parse().ok()).unwrap_or(default))
}

/// Grenzen für maschinelle Namensvorschläge im Abgleich. Vorgaben aus `config`, je Sammlung
/// und Modell überschreibbar (`rust.thresholds.<Modell>.review`, `.margin`, `.sure`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Mindestwert, ab dem ein Gesicht einen Vorschlag erhält.
    pub review: f32,
    /// Mindestabstand zur zweitbesten Person.
    pub margin: f32,
    /// Ab hier gilt ein Vorschlag als sicher (`auto`), darunter als `suggested`.
    pub sure: f32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            review: crate::config::FACE_REVIEW_MIN,
            margin: crate::config::FACE_REVIEW_MARGIN,
            sure: crate::config::FACE_SURE_MIN,
        }
    }
}
impl Limits {
    pub fn load(collection: &Collection) -> Result<Self> {
        let default = Self::default();
        Ok(Self {
            review: option(collection, "rust.review", default.review)?,
            margin: option(collection, "rust.margin", default.margin)?,
            sure: option(collection, "rust.sure", default.sure)?,
        })
    }
    /// Die Person, die ein Gesicht vorgeschlagen bekommt: die beste, wenn sie die Schwelle
    /// erreicht und mindestens `margin` vor der zweitbesten liegt. `ranking` ist absteigend
    /// sortiert und enthält nur zulässige Personen.
    pub fn accept(&self, ranking: &[(i64, f32)]) -> Option<(i64, f32)> {
        let (person, best) = *ranking.first()?;
        let second = ranking.get(1).map_or(f32::NEG_INFINITY, |r| r.1);
        (best >= self.review && best - second >= self.margin).then_some((person, best))
    }
    /// Der sichere Vorschlag einer Gruppe: der erste Eintrag, wenn er dieselbe Regel wie der
    /// Abgleich besteht (`accept`). `items` ist absteigend sortiert.
    pub fn sure_suggestion<'a>(
        &self,
        items: &'a [(i64, String, f32)],
    ) -> Option<&'a (i64, String, f32)> {
        let ranking: Vec<(i64, f32)> = items.iter().map(|(id, _, score)| (*id, *score)).collect();
        self.accept(&ranking)?;
        items.first()
    }
}
/// Zulässiger Bereich der einstellbaren Vorschlagsschwelle.
pub const REVIEW_RANGE: (f32, f32) = (0.5, 0.75);
/// Setzt die Vorschlagsschwelle der Sammlung für das aktive Modell und stößt den Abgleich
/// neu an. Bestätigte Namen bleiben unberührt.
pub fn set_review_threshold(collection: &mut Collection, value: f32) -> Result<()> {
    collection.writable()?;
    if !value.is_finite() || value < REVIEW_RANGE.0 || value > REVIEW_RANGE.1 {
        anyhow::bail!(
            "Die Schwelle muss zwischen {} und {} liegen",
            REVIEW_RANGE.0,
            REVIEW_RANGE.1
        );
    }
    let key =
        faces::setting(collection, "rust.embedder_key")?.context("Zuerst Fotos analysieren")?;
    let tx = collection.db.transaction()?;
    tx.execute(
        "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
        params![format!("rust.thresholds.{key}.review"), value.to_string()],
    )?;
    tx.execute("INSERT INTO face_maintenance VALUES(?,0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[&key])?;
    tx.commit()?;
    Ok(())
}

pub fn apply_knowledge(collection: &mut Collection, control: &Control) -> Result<String> {
    rebuild(collection, control, true)
}
pub fn maintain(collection: &mut Collection, control: &Control) -> Result<String> {
    rebuild(collection, control, false)
}
pub(super) fn applied_key(embedder: &str) -> String {
    format!("rust.recognition_applied:{embedder}")
}
fn rebuild(collection: &mut Collection, control: &Control, reset: bool) -> Result<String> {
    collection.writable()?;
    control.set_face_phase(super::jobs::FacePhase::References);
    let key =
        faces::setting(collection, "rust.embedder_key")?.context("Zuerst Fotos analysieren")?;
    // Offene Gesichter zuerst gruppieren: Abgelehnte und neu analysierte Gesichter sollen
    // nach Sekunden unter „Benennen“ stehen, nicht erst nach dem vollen Abgleich.
    while faces::prepare_groups(collection, &key, control)? > 0 {}
    if reset {
        collection.db.execute("INSERT INTO face_maintenance VALUES(?,0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[&key])?;
    }
    select_references(collection, &key, control, reset)?;
    rematch(collection, &key, control)?;
    control.set_face_phase(super::jobs::FacePhase::Groups);
    super::grouping::rebuild_groups(collection, &key, control)?;
    Ok("Referenzen balanciert und Namen vorgeschlagen".into())
}
/// Wählt die Referenzen der geänderten Personen (`face_reference_dirty`) neu aus, wie die
/// Wartung vor dem Abgleich (für Messungen ohne Gruppenarbeit).
pub fn update_references(collection: &mut Collection, key: &str, control: &Control) -> Result<()> {
    collection.writable()?;
    select_references(collection, key, control, false)
}
fn select_references(
    collection: &mut Collection,
    key: &str,
    control: &Control,
    reset: bool,
) -> Result<()> {
    let mut people = faces::people(collection)?;
    if !reset {
        let dirty: BTreeSet<i64> = collection
            .db
            .prepare("SELECT person_id FROM face_reference_dirty WHERE embedder_id=?")?
            .query_map([&key], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        people.retain(|p| dirty.contains(&p.id));
    }
    control.progress(0, people.len());
    for (index, person) in people.iter().enumerate() {
        control.checkpoint()?;
        let mut statement=collection.db.prepare("SELECT f.id,f.quality,coalesce(f.captured_at,''),e.vec,EXISTS(SELECT 1 FROM training_set t WHERE t.face_id=f.id AND t.embedder_id=? AND t.pinned=1) FROM faces f JOIN face_embeddings e ON e.face_id=f.id WHERE e.embedder_id=? AND f.person_id=? AND f.assignment='user' AND f.ignored=0 AND f.quality>=0.5 AND NOT EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=e.embedder_id) AND (f.id IN (SELECT face_id FROM training_set WHERE person_id=? AND embedder_id=? AND pinned=1) OR f.id IN (SELECT id FROM (SELECT id,substr(captured_at,1,4) year,ROW_NUMBER() OVER(PARTITION BY substr(captured_at,1,4) ORDER BY quality DESC,id) rank FROM faces WHERE person_id=? AND assignment='user' AND ignored=0 AND quality>=0.5) ORDER BY rank,year,id LIMIT 2048)) ORDER BY f.id")?;
        let candidates = statement
            .query_map(
                params![key, key, person.id, person.id, key, person.id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, f32>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, bool>(4)?,
                    ))
                },
            )?
            .map(|r| {
                let (id, quality, captured, bytes, pinned) = r?;
                Ok(Candidate {
                    id,
                    quality,
                    captured,
                    pinned,
                    vector: decode(&bytes)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        drop(statement);
        let selection = matching::select_references(
            &candidates,
            100,
            42,
            option(collection, "rust.duplicate", 0.92)?,
            control,
        )?;
        let tx = collection.db.transaction()?;
        tx.execute(
            "DELETE FROM training_set WHERE person_id=? AND embedder_id=?",
            params![person.id, key],
        )?;
        tx.execute("DELETE FROM face_review WHERE embedder_id=? AND face_id IN (SELECT id FROM faces WHERE person_id=?)",params![key,person.id])?;
        for (split, ids) in [("ref", selection.references), ("val", selection.validation)] {
            for id in ids {
                let pinned = candidates
                    .iter()
                    .find(|r| r.id == id)
                    .is_some_and(|r| r.pinned);
                tx.execute(
                    "INSERT INTO training_set VALUES(?,?,?,?,datetime('now'),?,?)",
                    params![
                        person.id,
                        id,
                        key,
                        split,
                        if pinned { "manual_pin" } else { "rust-fps-v1" },
                        pinned
                    ],
                )?;
            }
        }
        for id in selection.outliers {
            tx.execute(
                "INSERT OR REPLACE INTO face_review VALUES(?,?,'outlier')",
                params![id, key],
            )?;
        }
        tx.execute(
            "DELETE FROM face_reference_dirty WHERE embedder_id=? AND person_id=?",
            params![key, person.id],
        )?;
        control.checkpoint()?;
        tx.commit()?;
        control.progress(index + 1, people.len());
    }
    if reset || !people.is_empty() {
        // Gespeicherter Altwert des entfernten Klassifikators.
        collection.db.execute(
            "DELETE FROM face_settings WHERE key LIKE 'rust.classifier:%'",
            [],
        )?;
    }
    Ok(())
}

/// Maschinelle Zuordnung: Person, `auto` oder `suggested`, Wert.
type Proposal = (i64, &'static str, f32);
/// Ein Gesicht eines Abgleichschritts mit allem, was der Abgleich braucht.
struct MatchFace {
    id: i64,
    photo: i64,
    person: Option<i64>,
    assignment: Option<String>,
    confidence: Option<f64>,
    ignored: bool,
    retained: bool,
    /// Jahr der Aufnahme (erste vier Zeichen von `captured_at`).
    year: Option<i32>,
    /// Nicht historisch und Qualität ab 0,5: nur dann zählt ein Vektor (wie `ranked_with`).
    wanted: bool,
    /// Vektor nur für `wanted` (im inkrementellen Abgleich stets `None`, dort im Cache).
    vector: Option<Vec<f32>>,
}
/// Gesichter (auch historische) und Ablehnungen einiger Fotos, je Foto nach ID geordnet.
struct MatchBatch {
    faces: BTreeMap<i64, Vec<MatchFace>>,
    rejections: BTreeMap<i64, BTreeSet<i64>>,
}
impl MatchBatch {
    fn load(collection: &Collection, key: &str, photos: &[i64]) -> Result<Self> {
        let marks = vec!["?"; photos.len()].join(",");
        let mut values: Vec<rusqlite::types::Value> = vec![key.to_string().into()];
        values.extend(photos.iter().map(|p| rusqlite::types::Value::from(*p)));
        let mut faces: BTreeMap<i64, Vec<MatchFace>> = BTreeMap::new();
        let mut statement = collection.db.prepare_cached(&format!(
            "SELECT f.id,f.photo_id,f.person_id,f.assignment,f.confidence,f.ignored,f.retained,
                    f.captured_at,CASE WHEN f.retained=0 AND f.quality>=0.5 THEN e.vec END,
                    coalesce(f.retained=0 AND f.quality>=0.5,0)
             FROM faces f LEFT JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=?1
             WHERE f.photo_id IN ({marks}) ORDER BY f.photo_id,f.id"
        ))?;
        let mut rows = statement.query(rusqlite::params_from_iter(values.iter()))?;
        while let Some(row) = rows.next()? {
            let bytes: Option<Vec<u8>> = row.get(8)?;
            let captured: Option<String> = row.get(7)?;
            faces.entry(row.get(1)?).or_default().push(MatchFace {
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
                wanted: row.get(9)?,
                vector: bytes.as_deref().map(decode).transpose()?,
            });
        }
        drop(rows);
        let mut rejections: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
        let mut statement = collection.db.prepare_cached(&format!(
            "SELECT r.face_id,r.person_id FROM face_rejections r JOIN faces f ON f.id=r.face_id
             WHERE f.photo_id IN ({})",
            vec!["?"; photos.len()].join(",")
        ))?;
        let mut rows = statement.query(rusqlite::params_from_iter(photos.iter()))?;
        while let Some(row) = rows.next()? {
            rejections
                .entry(row.get(0)?)
                .or_default()
                .insert(row.get(1)?);
        }
        Ok(Self { faces, rejections })
    }
}

fn load_births(collection: &Collection) -> Result<BTreeMap<i64, i32>> {
    Ok(collection
        .db
        .prepare("SELECT person_id,value FROM person_attrs WHERE key='birth_year'")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok().and_then(|(id, year)| Some((id, year.parse().ok()?))))
        .collect())
}

/// Entscheidet die Gesichter eines Fotos (alle, auch historische, nach ID) und hängt
/// abweichende maschinelle Zuordnungen an `writes`. `rank` liefert die Rangfolge eines
/// Gesichts ohne die übergebenen Ausschlüsse (mindestens die ersten zwei Einträge), `None`
/// ohne vergleichbaren Vektor.
fn photo_writes(
    faces: &[MatchFace],
    rejections: &BTreeMap<i64, BTreeSet<i64>>,
    births: &BTreeMap<i64, i32>,
    limits: &Limits,
    mut rank: impl FnMut(&MatchFace, &dyn Fn(i64) -> bool) -> Option<Vec<(i64, f32)>>,
    writes: &mut Vec<(i64, Option<Proposal>)>,
) -> Result<()> {
    // Wie `faces::photo_faces`: nur nicht historische Gesichter, nach ID.
    let candidates: Vec<&MatchFace> = faces
        .iter()
        .filter(|f| !f.retained && !f.ignored && f.assignment.as_deref() != Some("user"))
        .collect();
    // Je Gesicht höchstens ein Kandidat: die beste zulässige Person, wenn sie die Schwelle
    // erreicht und klar vor der zweitbesten liegt.
    let mut proposals: Vec<Option<(i64, f32)>> = Vec::with_capacity(candidates.len());
    // Im Foto bestätigte Personen (auch historischer Gesichter). Kandidaten sind nie
    // bestätigt, ihr eigenes Gesicht ist also nie darunter.
    let occupied: Vec<i64> = faces
        .iter()
        .filter(|o| o.assignment.as_deref() == Some("user"))
        .filter_map(|o| o.person)
        .collect();
    for face in &candidates {
        // Wie `ranked_with`: Gesichter ohne Vektor oder unter Qualität 0,5 erhalten keinen
        // Vorschlag; abgelehnte und im Foto bestätigte Personen zählen nicht, ebenso wenig
        // Personen, die bei der Aufnahme noch nicht geboren waren. Ausschließen vor oder nach
        // dem Sortieren ergibt dieselbe Reihenfolge.
        let rejected = rejections.get(&face.id);
        let excluded = |person: i64| {
            rejected.is_some_and(|r| r.contains(&person))
                || occupied.contains(&person)
                || face
                    .year
                    .zip(births.get(&person))
                    .is_some_and(|(y, b)| y < *b)
        };
        let Some(ranking) = rank(face, &excluded) else {
            proposals.push(None);
            continue;
        };
        proposals.push(limits.accept(&ranking));
    }
    // Gemeinsame Entscheidung je Foto: Dieselbe Person kommt höchstens einmal vor; das
    // Gesicht mit dem höheren Wert erhält sie, das andere bleibt unbenannt.
    let people: Vec<i64> = proposals
        .iter()
        .flatten()
        .map(|p| p.0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let unknown = limits.review - 0.000001;
    let mut matrix = vec![vec![unknown; people.len() + candidates.len()]; candidates.len()];
    for (row, proposal) in proposals.iter().enumerate() {
        for (col, person) in people.iter().enumerate() {
            matrix[row][col] = match proposal {
                Some((id, score)) if id == person => *score,
                _ => -2.0,
            };
        }
    }
    let decisions = matching::assignment(&matrix)?;
    let mut wanted: BTreeMap<i64, Proposal> = BTreeMap::new();
    for (row, col) in decisions.into_iter().enumerate() {
        if col < people.len() && matrix[row][col] >= limits.review {
            let score = matrix[row][col];
            let kind = if score >= limits.sure {
                "auto"
            } else {
                "suggested"
            };
            wanted.insert(candidates[row].id, (people[col], kind, score));
        }
    }
    // Nur Abweichungen schreiben: Bisher wurden alle maschinellen Zuordnungen des Fotos
    // gelöscht und neu gesetzt, was je Gesicht Trigger der Gruppen auslöste.
    for face in faces {
        let machine = matches!(face.assignment.as_deref(), Some("auto" | "suggested"));
        match wanted.get(&face.id) {
            Some(&(person, kind, score)) => {
                let same = face.person == Some(person)
                    && face.assignment.as_deref() == Some(kind)
                    && face.confidence == Some(f64::from(score));
                if !same {
                    writes.push((face.id, Some((person, kind, score))));
                }
            }
            None if machine => writes.push((face.id, None)),
            None => {}
        }
    }
    Ok(())
}

fn write_proposals(
    tx: &rusqlite::Transaction<'_>,
    writes: &[(i64, Option<Proposal>)],
) -> Result<()> {
    for (id, value) in writes {
        match value {
            Some((person, kind, score)) => {
                tx.execute("UPDATE faces SET person_id=?,assignment=?,confidence=?,assigned_at=datetime('now') WHERE id=? AND assignment IS NOT 'user'",params![person,kind,score,id])?;
            }
            None => {
                tx.execute("UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE id=? AND assignment IN ('auto','suggested')",[id])?;
            }
        }
    }
    Ok(())
}

/// Schließt einen Abgleich ab, wenn sich seit seinem Beginn nichts geändert hat.
fn finish_rematch(collection: &mut Collection, key: &str, generation: Option<i64>) -> Result<()> {
    let tx = collection.db.transaction()?;
    tx.execute(
        "DELETE FROM face_maintenance WHERE embedder_id=? AND generation IS ?",
        params![key, generation],
    )?;
    let open: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM face_maintenance WHERE embedder_id=?)",
        [key],
        |r| r.get(0),
    )?;
    if !open {
        faces::save_setting(&tx, &applied_key(key), &chrono::Utc::now().to_rfc3339())?;
    }
    tx.commit()?;
    Ok(())
}

fn maintenance_state(collection: &Collection, key: &str) -> Result<(i64, Option<i64>)> {
    Ok(collection
        .db
        .query_row(
            "SELECT after_photo,generation FROM face_maintenance WHERE embedder_id=?",
            [key],
            |r| Ok((r.get(0)?, Some(r.get(1)?))),
        )
        .optional()?
        .unwrap_or((0, None)))
}

/// Zwischenspeicher des inkrementellen Abgleichs für eine Sammlung und ein Modell: je
/// Gesicht der Vektor und der Personenscore gegen jede Person mit Referenzen (`NaN` ohne
/// Wert), dazu der Referenzstand, gegen den gerechnet wurde. Ändern sich die Referenzen
/// einer Person, wird nur ihre Spalte für alle Gesichter neu gerechnet; Rangfolge,
/// Ausschlüsse und die Entscheidung je Foto entstehen in jedem Lauf neu aus dem aktuellen
/// Stand. Er lebt nur im Arbeitsspeicher und ist jederzeit verwerfbar.
#[derive(Default)]
pub struct MatchCache {
    owner: Option<(std::path::PathBuf, String)>,
    /// Referenzen je Person (Gesichts-ID, Vektor), gegen die die Spalten gerechnet sind.
    references: BTreeMap<i64, Vec<(i64, Vec<f32>)>>,
    /// Personen-ID je Spalte, wie `References::people`.
    people: Vec<i64>,
    faces: std::collections::HashMap<i64, CachedFace>,
}
struct CachedFace {
    /// `rowid` der Merkmalszeile: `INSERT OR REPLACE` vergibt eine neue, ein ersetzter
    /// Vektor wird so erkannt.
    rowid: i64,
    vector: Option<Vec<f32>>,
    scores: Vec<f32>,
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
const SCORE_ROUND: usize = 4096;
/// Zuordnungen je Schreibtransaktion.
const WRITE_CHUNK: usize = 512;

/// Personenscores mehrerer Gesichter, auf bis zu `config::MATCH_THREADS` Threads verteilt.
/// Jedes Gesicht wird für sich gerechnet; die Werte hängen nicht von der Aufteilung ab.
fn score_rows(references: &References, faces: &[(i64, Option<&[f32]>)]) -> Vec<Vec<f32>> {
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
struct MatchState {
    entries: Entries,
    /// Alle Gesichter, nach Foto und ID; `vector` bleibt leer.
    faces: Vec<MatchFace>,
    /// `rowid` der Merkmalszeile je Gesicht des Modells.
    embeddings: std::collections::HashMap<i64, i64>,
    rejections: BTreeMap<i64, BTreeSet<i64>>,
}
impl MatchState {
    fn load(collection: &Collection, key: &str) -> Result<Self> {
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
    fn sync(
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

/// Zwischenspeicher des laufenden Programms (eine Sammlung, ein Modell zugleich).
static MATCH_CACHE: std::sync::Mutex<Option<MatchCache>> = std::sync::Mutex::new(None);

/// Sperre des Zwischenspeichers. Ist sie durch eine Panik vergiftet, kann der Inhalt halb
/// geändert sein: er wird verworfen (der nächste Abgleich baut ihn neu auf) und gewarnt.
fn match_cache() -> std::sync::MutexGuard<'static, Option<MatchCache>> {
    MATCH_CACHE.lock().unwrap_or_else(|poisoned| {
        log::warn!(target: "gesichter", "abgleich_cache_vergiftet verworfen=ja");
        let mut guard = poisoned.into_inner();
        *guard = None;
        MATCH_CACHE.clear_poison();
        guard
    })
}

/// Abgleich nach Bestätigungen: inkrementell über den Zwischenspeicher des Programms
/// ([`MatchCache`]), Ergebnis bitgleich zum vollständigen Lauf ([`rematch_full`]).
pub fn rematch(collection: &mut Collection, key: &str, control: &Control) -> Result<()> {
    let mut cache = match_cache().take().unwrap_or_default();
    let mut span = super::diag::span("gesichter", "abgleich");
    span.field("cache_bereit", cache.owner.is_some());
    let result = rematch_with(collection, key, control, &mut cache);
    span.field("cache_gesichter", cache.len())
        .field("ok", result.is_ok());
    drop(span);
    *match_cache() = Some(cache);
    result
}

/// Abgleich mit ausdrücklichem Zwischenspeicher. Ein leerer oder fremder Zwischenspeicher
/// wird zuerst vollständig aufgebaut (alle Vektoren lesen, alle Spalten rechnen).
pub fn rematch_with(
    collection: &mut Collection,
    key: &str,
    control: &Control,
    cache: &mut MatchCache,
) -> Result<()> {
    control.set_face_phase(super::jobs::FacePhase::Matching);
    control.checkpoint()?;
    // Eine Nutzeränderung während des Laufs erhöht per Trigger `generation`. Geschrieben wird
    // nur, solange die Generation noch die vom Beginn ist; sonst bleibt der Auftrag für einen
    // neuen Lauf (nach der Referenzauswahl der Wartung) stehen.
    let (_, generation) = maintenance_state(collection, key)?;
    let limits = Limits::load(collection)?;
    let births = load_births(collection)?;
    let mut state = MatchState::load(collection, key)?;
    let owner = (collection.root.clone(), key.to_string());
    if cache.owner.as_ref() != Some(&owner) {
        *cache = MatchCache {
            owner: Some(owner),
            ..MatchCache::default()
        };
    }
    let compared = state
        .faces
        .iter()
        .filter(|f| f.wanted && state.embeddings.contains_key(&f.id))
        .count();
    let dim = state.entries.0.first().map_or(512, |e| e.2.len());
    let bytes = compared * ((dim + state.entries.1.len()) * 4 + 96);
    if bytes > crate::config::MATCH_CACHE_MAX_BYTES {
        log::info!(
            target: "gesichter",
            "abgleich_vollstaendig grund=cache_zu_gross bytes={bytes} gesichter={compared}"
        );
        *cache = MatchCache::default();
        return rematch_full(collection, key, control);
    }
    let references = cache.sync(collection, key, &mut state, control)?;
    control.checkpoint()?;
    let mut writes: Vec<(i64, Option<Proposal>)> = Vec::new();
    for photo in state.faces.chunk_by(|a, b| a.photo == b.photo) {
        // Wie der vollständige Lauf: nur Fotos mit mindestens einem aktuellen Gesicht.
        if photo.iter().all(|f| f.retained) {
            continue;
        }
        photo_writes(
            photo,
            &state.rejections,
            &births,
            &limits,
            |face, excluded| {
                let cached = cache.faces.get(&face.id)?;
                cached.vector.as_ref()?;
                Some(references.best_two(&cached.scores, excluded))
            },
            &mut writes,
        )?;
    }
    for chunk in writes.chunks(WRITE_CHUNK) {
        control.checkpoint()?;
        let tx = collection.db.transaction()?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT generation FROM face_maintenance WHERE embedder_id=?",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        if current != generation {
            // Neuer Auftrag: Die übrigen Zuordnungen beruhen auf einem veralteten Stand.
            return Ok(());
        }
        write_proposals(&tx, chunk)?;
        tx.commit()?;
    }
    control.checkpoint()?;
    finish_rematch(collection, key, generation)
}

/// Ein Gesicht der Liste „Wahrscheinlich X“ ([`person_candidates`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PersonCandidate {
    pub face: i64,
    /// Personenscore für die gewählte Person.
    pub score: f32,
    /// Person mit dem höchsten Personenscore und ihr Wert, wenn das nicht die gewählte Person
    /// ist (zweiter Abschnitt „Eher jemand anderes“); `None`, wenn die gewählte vorn liegt.
    pub better: Option<(i64, f32)>,
}
impl PersonCandidate {
    /// Reihenfolge der Liste: erst die Gesichter, deren beste Person die gewählte ist, dann
    /// die übrigen; in beiden Abschnitten nach Personenscore absteigend, bei Gleichstand nach
    /// Gesichts-ID. Streng, weil die IDs eindeutig sind.
    fn order(&self, other: &Self) -> std::cmp::Ordering {
        self.better
            .is_some()
            .cmp(&other.better.is_some())
            .then(other.score.total_cmp(&self.score))
            .then(self.face.cmp(&other.face))
    }
}

/// Unbestätigte Gesichter, die wahrscheinlich `person` zeigen (Personenseite, „Wahrscheinlich
/// X“): Personenscore ab `minimum`, dazu die Person mit dem höchsten Personenscore, wenn es
/// eine andere ist. Der Personenscore ist der des Abgleichs (Mittel der bis zu drei besten
/// Kosinuswerte gegen die Referenzen der Person, bitgleich). Zulässig sind die Gesichter, die
/// der Abgleich vergleicht (nicht historisch, nicht ignoriert, nicht vom Nutzer benannt,
/// Qualität ab 0,5 mit Vektor), ohne seine Ausschlüsse für diese Person: abgelehnt, im Foto
/// schon bestätigt, vor dem Geburtsjahr aufgenommen. Schwelle und Abstand gelten nicht;
/// Vorschläge für andere Personen stehen mit in der Liste.
///
/// Die beste Person bestimmt der Abgleich ebenso (`References::best_two`, Ordnung Score, Name,
/// ID) über alle Personen mit Referenzen, ohne die für dieses Gesicht ausgeschlossenen. Ist sie
/// nicht `person`, steht das Gesicht im zweiten Abschnitt ([`PersonCandidate::order`]).
///
/// Die Werte kommen aus dem Zwischenspeicher des Abgleichs ([`MatchCache`]), wenn er für diese
/// Sammlung und dieses Modell bereitliegt; er wird dabei auf den aktuellen Stand gebracht.
/// Sonst (Programmstart, Abgleich läuft gerade) rechnet ein Durchgang über die Vektoren die
/// Spalte dieser Person und nur für Gesichter ab `minimum` alle Spalten, ohne
/// Zwischenspeicher aufzubauen.
pub fn person_candidates(
    collection: &Collection,
    key: &str,
    person: i64,
    minimum: f32,
    control: &Control,
) -> Result<Vec<PersonCandidate>> {
    // Die Sperre gilt nur für diesen Leser und nur, wenn der Zwischenspeicher dieser Sammlung
    // bereitliegt; der Abgleich wartet ohne Rückgabepunkt auf sie, ein gegenseitiges Warten ist
    // daher ausgeschlossen. Ohne passenden Zwischenspeicher wird sie vor dem Rechnen frei.
    let owner = (collection.root.clone(), key.to_string());
    let mut slot = Some(match_cache());
    let ready = slot
        .as_ref()
        .and_then(|slot| slot.as_ref())
        .is_some_and(|cache| cache.owner.as_ref() == Some(&owner));
    if !ready {
        slot = None;
    }
    let cache = slot.as_mut().and_then(|slot| slot.as_mut());
    person_candidates_with(collection, key, person, minimum, control, cache)
}

/// Wie [`person_candidates`] mit ausdrücklichem Zwischenspeicher: `None` oder ein
/// Zwischenspeicher einer anderen Sammlung rechnet in einem Durchgang über die Vektoren.
pub fn person_candidates_with(
    collection: &Collection,
    key: &str,
    person: i64,
    minimum: f32,
    control: &Control,
    cache: Option<&mut MatchCache>,
) -> Result<Vec<PersonCandidate>> {
    let mut state = MatchState::load(collection, key)?;
    let births = load_births(collection)?;
    let ids = open_for_person(&state, person, births.get(&person).copied());
    let exclusions = Exclusions::new(&mut state, births, &ids);
    let wanted = Wanted {
        person,
        minimum,
        exclusions: &exclusions,
    };
    let owner = (collection.root.clone(), key.to_string());
    let mut found = match cache.filter(|cache| cache.owner.as_ref() == Some(&owner)) {
        Some(cache) => {
            cached_candidates(collection, key, &wanted, cache, &mut state, &ids, control)?
        }
        None => streamed_candidates(collection, key, &wanted, &ids, control)?,
    };
    found.sort_by(PersonCandidate::order);
    Ok(found)
}

/// Gesichter, die für `person` in Frage kommen (siehe [`person_candidates`]), nach ID.
fn open_for_person(state: &MatchState, person: i64, birth: Option<i32>) -> Vec<i64> {
    // Wie im Abgleich: im Foto bestätigt zählt auch für historische Gesichter.
    let occupied: BTreeSet<i64> = state
        .faces
        .iter()
        .filter(|f| f.assignment.as_deref() == Some("user") && f.person == Some(person))
        .map(|f| f.photo)
        .collect();
    let mut ids: Vec<i64> = state
        .faces
        .iter()
        .filter(|f| {
            f.wanted
                && !f.retained
                && !f.ignored
                && f.assignment.as_deref() != Some("user")
                && state.embeddings.contains_key(&f.id)
                && !occupied.contains(&f.photo)
                && !state
                    .rejections
                    .get(&f.id)
                    .is_some_and(|rejected| rejected.contains(&person))
                && !f.year.zip(birth).is_some_and(|(year, born)| year < born)
        })
        .map(|f| f.id)
        .collect();
    ids.sort_unstable();
    ids
}

/// Ausschlüsse des Abgleichs je Gesicht (wie `photo_writes`): abgelehnte Personen, im Foto
/// bestätigte Personen (auch historischer Gesichter), vor dem Geburtsjahr aufgenommen.
struct Exclusions {
    rejections: BTreeMap<i64, BTreeSet<i64>>,
    births: BTreeMap<i64, i32>,
    /// Bestätigte Personen je Foto.
    occupied: std::collections::HashMap<i64, Vec<i64>>,
    /// Foto und Aufnahmejahr je Kandidat.
    faces: std::collections::HashMap<i64, (i64, Option<i32>)>,
}
impl Exclusions {
    /// Übernimmt die Ablehnungen aus `state` (`sync` braucht sie nicht).
    fn new(state: &mut MatchState, births: BTreeMap<i64, i32>, ids: &[i64]) -> Self {
        let wanted: std::collections::HashSet<i64> = ids.iter().copied().collect();
        let mut occupied: std::collections::HashMap<i64, Vec<i64>> = Default::default();
        let mut faces = std::collections::HashMap::with_capacity(ids.len());
        for face in &state.faces {
            if face.assignment.as_deref() == Some("user")
                && let Some(person) = face.person
            {
                occupied.entry(face.photo).or_default().push(person);
            }
            if wanted.contains(&face.id) {
                faces.insert(face.id, (face.photo, face.year));
            }
        }
        Self {
            rejections: std::mem::take(&mut state.rejections),
            births,
            occupied,
            faces,
        }
    }
    fn excluded(&self, face: i64, person: i64) -> bool {
        let Some(&(photo, year)) = self.faces.get(&face) else {
            return false;
        };
        self.rejections
            .get(&face)
            .is_some_and(|rejected| rejected.contains(&person))
            || self
                .occupied
                .get(&photo)
                .is_some_and(|occupied| occupied.contains(&person))
            || year
                .zip(self.births.get(&person))
                .is_some_and(|(year, born)| year < *born)
    }
}

/// Was [`person_candidates`] sucht: Person, Mindestwert und Ausschlüsse.
struct Wanted<'a> {
    person: i64,
    minimum: f32,
    exclusions: &'a Exclusions,
}
impl Wanted<'_> {
    /// Eintrag für `face` aus seinen Personenscores (Spalten wie `references.people`, `column`
    /// die der gewählten Person); `None` unter dem Mindestwert oder ohne Wert.
    fn candidate(
        &self,
        references: &References,
        column: usize,
        face: i64,
        scores: &[f32],
    ) -> Option<PersonCandidate> {
        let score = scores[column];
        if score.is_nan() || score < self.minimum {
            return None;
        }
        // Die gewählte Person ist für ihre Kandidaten nie ausgeschlossen (`open_for_person`).
        let best = references
            .best_two(scores, |other| {
                other != self.person && self.exclusions.excluded(face, other)
            })
            .into_iter()
            .next()
            .filter(|best| best.0 != self.person);
        Some(PersonCandidate {
            face,
            score,
            better: best,
        })
    }
}

/// Kandidaten aus dem Zwischenspeicher dieser Sammlung; er wird dabei auf den Stand gebracht.
fn cached_candidates(
    collection: &Collection,
    key: &str,
    wanted: &Wanted,
    cache: &mut MatchCache,
    state: &mut MatchState,
    ids: &[i64],
    control: &Control,
) -> Result<Vec<PersonCandidate>> {
    let references = cache.sync(collection, key, state, control)?;
    let Some(column) = references.people.iter().position(|p| p.0 == wanted.person) else {
        return Ok(Vec::new());
    };
    Ok(ids
        .iter()
        .filter_map(|id| {
            let face = cache.faces.get(id)?;
            face.vector.as_ref()?;
            wanted.candidate(&references, column, *id, &face.scores)
        })
        .collect())
}

/// Kandidaten in einem Durchgang über die Vektoren des Modells: zuerst nur die Spalte der
/// gewählten Person, für Gesichter ab dem Mindestwert dann alle Spalten (die beste Person).
/// Gleicher Rechenweg wie der Zwischenspeicher; der Wert einer Person hängt nur von ihren
/// eigenen Referenzen ab.
fn streamed_candidates(
    collection: &Collection,
    key: &str,
    wanted: &Wanted,
    ids: &[i64],
    control: &Control,
) -> Result<Vec<PersonCandidate>> {
    let (entries, people) = References::load_entries(collection, key)?;
    let Some(column) = people.iter().position(|p| p.0 == wanted.person) else {
        return Ok(Vec::new());
    };
    let own = entries
        .iter()
        .filter(|entry| entry.1 == column)
        .map(|(face, _, vector)| (*face, 0, vector.clone()))
        .collect();
    let single = References::from_entries(own, vec![people[column].clone()]);
    let all = References::from_entries(entries, people);
    let open: std::collections::HashSet<i64> = ids.iter().copied().collect();
    let mut found = Vec::new();
    let mut statement = collection
        .db
        .prepare("SELECT face_id,vec FROM face_embeddings WHERE embedder_id=?")?;
    let mut rows = statement.query([key])?;
    let mut batch: Vec<(i64, Vec<f32>)> = Vec::with_capacity(SCORE_ROUND);
    loop {
        let row = rows.next()?;
        let end = row.is_none();
        if let Some(row) = row
            && open.contains(&row.get::<_, i64>(0)?)
            && let Some(bytes) = row.get::<_, Option<Vec<u8>>>(1)?
        {
            batch.push((row.get(0)?, decode(&bytes)?));
        }
        if batch.len() >= SCORE_ROUND || (end && !batch.is_empty()) {
            control.checkpoint()?;
            let work: Vec<(i64, Option<&[f32]>)> = batch
                .iter()
                .map(|(id, vector)| (*id, Some(vector.as_slice())))
                .collect();
            let first = score_rows(&single, &work);
            let near: Vec<(i64, Option<&[f32]>)> = work
                .into_iter()
                .zip(first)
                .filter(|(_, row)| row[0] >= wanted.minimum)
                .map(|(work, _)| work)
                .collect();
            let rows = score_rows(&all, &near);
            found.extend(
                near.iter()
                    .zip(rows)
                    .filter_map(|((id, _), row)| wanted.candidate(&all, column, *id, &row)),
            );
            batch.clear();
        }
        if end {
            break;
        }
    }
    Ok(found)
}

/// Vollständiger Abgleich ohne Zwischenspeicher, Foto für Foto mit Cursor. Rückfall, wenn der
/// Zwischenspeicher die Speichergrenze überschritte, und Maßstab für den inkrementellen Lauf.
pub fn rematch_full(collection: &mut Collection, key: &str, control: &Control) -> Result<()> {
    control.set_face_phase(super::jobs::FacePhase::Matching);
    // Eine Nutzeränderung während des Laufs setzt per Trigger den Cursor auf 0 und erhöht
    // `generation`. Der Lauf schreibt seinen Cursor und das Ende nur für die Generation, mit
    // der er begann; sonst bleibt der Auftrag stehen und der nächste Lauf beginnt von vorn.
    let (mut cursor, generation) = maintenance_state(collection, key)?;
    let limits = Limits::load(collection)?;
    let births = load_births(collection)?;
    let references = References::load(collection, key)?;
    // Höchstens so lange bleibt ein Fortschritt ohne Änderungen ungespeichert; Schritte mit
    // Änderungen werden sofort geschrieben.
    let mut saved = std::time::Instant::now();
    loop {
        control.checkpoint()?;
        let photos=collection.db.prepare_cached("SELECT DISTINCT photo_id FROM faces WHERE photo_id>? AND retained=0 ORDER BY photo_id LIMIT 32")?.query_map([cursor],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let Some(&last) = photos.last() else {
            break;
        };
        // Alle Gesichter, Vektoren und Ablehnungen des Schritts mit zwei Abfragen lesen und
        // erst danach schreiben: Die Schreibtransaktion bleibt kurz und wartet nie auf
        // `checkpoint`, während eine Nutzeraktion schreiben will.
        let batch = MatchBatch::load(collection, key, &photos)?;
        let mut writes: Vec<(i64, Option<Proposal>)> = Vec::new();
        for photo in &photos {
            control.checkpoint()?;
            let faces = batch.faces.get(photo).map_or(&[][..], Vec::as_slice);
            photo_writes(
                faces,
                &batch.rejections,
                &births,
                &limits,
                |face, excluded| {
                    face.vector
                        .as_deref()
                        .map(|query| references.rank(query, face.id, excluded))
                },
                &mut writes,
            )?;
        }
        if writes.is_empty() && saved.elapsed() < std::time::Duration::from_secs(1) {
            cursor = last;
            continue;
        }
        let tx = collection.db.transaction()?;
        write_proposals(&tx, &writes)?;
        tx.execute(
            "UPDATE face_maintenance SET after_photo=? WHERE embedder_id=? AND generation IS ?",
            params![last, key, generation],
        )?;
        tx.commit()?;
        saved = std::time::Instant::now();
        cursor = last;
    }
    control.checkpoint()?;
    finish_rematch(collection, key, generation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(scores: &[f32]) -> Vec<(i64, String, f32)> {
        scores
            .iter()
            .enumerate()
            .map(|(i, s)| (i as i64 + 1, format!("P{i}"), *s))
            .collect()
    }

    /// Bisheriger Vergleich je Referenz, als Maßstab für die Blockrechnung.
    fn naive_rank(
        entries: &[(i64, i64, Vec<f32>)],
        names: &BTreeMap<i64, String>,
        query: &[f32],
        own: i64,
        excluded: &BTreeSet<i64>,
    ) -> Vec<(i64, f32)> {
        let mut scores: BTreeMap<i64, Vec<f32>> = BTreeMap::new();
        for (id, person, vector) in entries {
            if *id == own || excluded.contains(person) {
                continue;
            }
            if let Some(score) = inference::cosine(query, vector) {
                let entry = scores.entry(*person).or_default();
                entry.push(score);
                entry.sort_by(|a, b| b.total_cmp(a));
                entry.truncate(3);
            }
        }
        let mut result: Vec<_> = scores
            .into_iter()
            .map(|(id, s)| {
                (
                    id,
                    names[&id].clone(),
                    s.iter().sum::<f32>() / s.len() as f32,
                )
            })
            .collect();
        result.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
        result.into_iter().map(|(id, _, s)| (id, s)).collect()
    }

    #[test]
    fn block_ranking_is_bit_identical_to_the_single_comparison() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5
        };
        let unit = |next: &mut dyn FnMut() -> f32, dim: usize| {
            let v: Vec<f32> = (0..dim).map(|_| next()).collect();
            inference::normalize(&v).expect("Vektor")
        };
        // 7 Personen, 61 Referenzen (kein Vielfaches von acht), dazu eine mit falscher Länge.
        let people: Vec<(i64, String)> = (0..7)
            .map(|p| (p * 3 + 1, format!("P{}", 6 - p % 4)))
            .collect();
        let names: BTreeMap<i64, String> = people.iter().cloned().collect();
        let mut entries: Vec<(i64, i64, Vec<f32>)> = Vec::new();
        for id in 0..61i64 {
            let slot = (id as usize * 5) % people.len();
            entries.push((100 + id, people[slot].0, unit(&mut next, 128)));
        }
        entries.push((999, people[2].0, unit(&mut next, 64)));
        // Doppelte Werte prüfen die Reihenfolge bei Gleichstand.
        let copy = entries[3].2.clone();
        entries.push((998, people[4].0, copy));
        entries.sort_by_key(|e| (e.1, e.0));
        let slots: BTreeMap<i64, usize> =
            people.iter().enumerate().map(|(i, p)| (p.0, i)).collect();
        let references = References::from_entries(
            entries
                .iter()
                .map(|(id, person, v)| (*id, slots[person], v.clone()))
                .collect(),
            people.clone(),
        );
        for round in 0..40 {
            let query = if round == 0 {
                entries[3].2.clone()
            } else {
                unit(&mut next, if round == 39 { 64 } else { 128 })
            };
            let own = if round % 3 == 0 { entries[round].0 } else { -1 };
            let excluded: BTreeSet<i64> = if round % 4 == 1 {
                [people[round % 7].0].into()
            } else {
                BTreeSet::new()
            };
            let fast = references.rank(&query, own, |p| excluded.contains(&p));
            let slow = naive_rank(&entries, &names, &query, own, &excluded);
            let two =
                references.best_two(&references.scores(&query, own), |p| excluded.contains(&p));
            assert_eq!(two.len(), slow.len().min(2), "Runde {round}");
            for (a, b) in two.iter().zip(&slow) {
                assert_eq!(a.0, b.0, "Runde {round}");
                assert_eq!(a.1.to_bits(), b.1.to_bits(), "Runde {round}");
            }
            assert_eq!(fast.len(), slow.len(), "Runde {round}");
            for (a, b) in fast.iter().zip(&slow) {
                assert_eq!(a.0, b.0, "Runde {round}");
                assert_eq!(a.1.to_bits(), b.1.to_bits(), "Runde {round}");
            }
        }
    }

    #[test]
    fn top_three_keeps_the_best_values_in_order() {
        let mut top = Top3::default();
        for v in [0.2, 0.9, 0.5, 0.9, 0.1, 0.7] {
            top.push(v);
        }
        assert_eq!(&top.values[..top.len], &[0.9, 0.9, 0.7]);
        assert_eq!(top.mean().to_bits(), ((0.9f32 + 0.9 + 0.7) / 3.0).to_bits());
    }

    /// Die früheren Kopien (sortieren, auf drei kürzen, summieren) und die gemeinsame
    /// Funktion ergeben auf Zufallsdaten bitgleiche Werte, auch bei Gleichständen.
    #[test]
    fn shared_top_three_matches_the_former_copies_bit_for_bit() {
        fn sorted(values: &[f32]) -> Option<f32> {
            let mut v = values.to_vec();
            v.sort_by(|a, b| b.total_cmp(a));
            v.truncate(3);
            (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
        }
        fn threshold(values: &[f32]) -> Option<f32> {
            let mut top = [f32::NEG_INFINITY; 3];
            for value in values {
                if *value > top[2] {
                    top[2] = *value;
                    top.sort_by(|x, y| y.total_cmp(x));
                }
            }
            let used = values.len().min(3);
            (used > 0).then(|| top[..used].iter().sum::<f32>() / used as f32)
        }
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..2000 {
            let len = (next() % 12) as usize;
            let values: Vec<f32> = (0..len)
                .map(|_| {
                    // Grobe Stufen erzeugen Gleichstände; `+ 0.0` vermeidet `-0.0`, das die
                    // `>`-Kopie anders einsortierte als `total_cmp` (Kosinuswerte sind praktisch
                    // nie exakt null).
                    let v = (next() >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0;
                    if round % 2 == 0 {
                        (v * 8.0).round() / 8.0 + 0.0
                    } else {
                        v
                    }
                })
                .collect();
            let shared = top3_mean(values.iter().copied()).map(f32::to_bits);
            assert_eq!(shared, sorted(&values).map(f32::to_bits), "Runde {round}");
            assert_eq!(
                shared,
                threshold(&values).map(f32::to_bits),
                "Runde {round}"
            );
            let keyed: Vec<(i64, f32)> = values
                .iter()
                .enumerate()
                .map(|(i, v)| ((i % 3) as i64, *v))
                .collect();
            for (key, mean) in top3_means(keyed.iter().copied()) {
                let own: Vec<f32> = keyed.iter().filter(|e| e.0 == key).map(|e| e.1).collect();
                assert_eq!(Some(mean.to_bits()), sorted(&own).map(f32::to_bits));
            }
        }
    }

    #[test]
    fn sure_suggestion_follows_the_rematch_rule() {
        let limits = Limits {
            review: 0.6,
            margin: 0.05,
            sure: 0.8,
        };
        let sure = |scores: &[f32]| limits.sure_suggestion(&items(scores)).map(|i| i.0);
        assert_eq!(sure(&[0.9, 0.7, 0.5]), Some(1), "klarer Vorsprung");
        assert_eq!(sure(&[0.55, 0.3]), None, "unter der Schwelle");
        assert_eq!(sure(&[0.8, 0.78]), None, "zwei knappe Kandidaten");
        assert_eq!(sure(&[0.7]), Some(1), "einziger Kandidat über der Schwelle");
        assert_eq!(sure(&[]), None);
    }
}
