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
impl References {
    pub fn load(collection: &Collection, key: &str) -> Result<Self> {
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
        Ok(Self::from_entries(entries, people))
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
        let mut top = vec![Top3::default(); self.people.len()];
        let allowed: Vec<bool> = self.people.iter().map(|p| !excluded(p.0)).collect();
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
                    if self.faces[index] != own && allowed[*slot] && sum.is_finite() {
                        top[*slot].push(sum.clamp(-1.0, 1.0));
                    }
                }
            }
        }
        for (face, slot, vector) in &self.other {
            if *face != own
                && allowed[*slot]
                && let Some(score) = inference::cosine(query, vector)
            {
                top[*slot].push(score);
            }
        }
        let mut result: Vec<(usize, f32)> = top
            .iter()
            .enumerate()
            .filter(|(_, t)| t.len > 0)
            .map(|(slot, t)| (slot, t.mean()))
            .collect();
        result.sort_by(|a, b| {
            let (x, y) = (&self.people[a.0], &self.people[b.0]);
            b.1.total_cmp(&a.1).then(x.1.cmp(&y.1)).then(x.0.cmp(&y.0))
        });
        result
            .into_iter()
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
    rematch(collection, &key, control)?;
    control.set_face_phase(super::jobs::FacePhase::Groups);
    super::grouping::rebuild_groups(collection, &key, control)?;
    Ok("Referenzen balanciert und Namen vorgeschlagen".into())
}

/// Maschinelle Zuordnung: Person, `auto` oder `suggested`, Wert.
type Proposal = (i64, &'static str, f32);
/// Ein Gesicht eines Abgleichschritts mit allem, was der Abgleich braucht.
struct MatchFace {
    id: i64,
    person: Option<i64>,
    assignment: Option<String>,
    confidence: Option<f64>,
    ignored: bool,
    retained: bool,
    /// Jahr der Aufnahme (erste vier Zeichen von `captured_at`).
    year: Option<i32>,
    /// Vektor nur für nicht historische Gesichter ab Qualität 0,5 (wie `ranked_with`).
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
                    f.captured_at,CASE WHEN f.retained=0 AND f.quality>=0.5 THEN e.vec END
             FROM faces f LEFT JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=?1
             WHERE f.photo_id IN ({marks}) ORDER BY f.photo_id,f.id"
        ))?;
        let mut rows = statement.query(rusqlite::params_from_iter(values.iter()))?;
        while let Some(row) = rows.next()? {
            let bytes: Option<Vec<u8>> = row.get(8)?;
            let captured: Option<String> = row.get(7)?;
            faces.entry(row.get(1)?).or_default().push(MatchFace {
                id: row.get(0)?,
                person: row.get(2)?,
                assignment: row.get(3)?,
                confidence: row.get(4)?,
                ignored: row.get(5)?,
                retained: row.get(6)?,
                year: captured
                    .as_deref()
                    .and_then(|v| v.get(..4))
                    .and_then(|v| v.parse().ok()),
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

pub fn rematch(collection: &mut Collection, key: &str, control: &Control) -> Result<()> {
    control.set_face_phase(super::jobs::FacePhase::Matching);
    // Eine Nutzeränderung während des Laufs setzt per Trigger den Cursor auf 0 und erhöht
    // `generation`. Der Lauf schreibt seinen Cursor und das Ende nur für die Generation, mit
    // der er begann; sonst bleibt der Auftrag stehen und der nächste Lauf beginnt von vorn.
    let (mut cursor, generation): (i64, Option<i64>) = collection
        .db
        .query_row(
            "SELECT after_photo,generation FROM face_maintenance WHERE embedder_id=?",
            [key],
            |r| Ok((r.get(0)?, Some(r.get(1)?))),
        )
        .optional()?
        .unwrap_or((0, None));
    let limits = Limits::load(collection)?;
    let births: BTreeMap<i64, i32> = collection
        .db
        .prepare("SELECT person_id,value FROM person_attrs WHERE key='birth_year'")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok().and_then(|(id, year)| Some((id, year.parse().ok()?))))
        .collect();
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
            // Wie `faces::photo_faces`: nur nicht historische Gesichter, nach ID.
            let candidates: Vec<&MatchFace> = faces
                .iter()
                .filter(|f| !f.retained && !f.ignored && f.assignment.as_deref() != Some("user"))
                .collect();
            // Je Gesicht höchstens ein Kandidat: die beste zulässige Person, wenn sie die
            // Schwelle erreicht und klar vor der zweitbesten liegt.
            let mut proposals: Vec<Option<(i64, f32)>> = Vec::with_capacity(candidates.len());
            for face in &candidates {
                // Wie `ranked_with`: Gesichter ohne Vektor oder unter Qualität 0,5 erhalten
                // keinen Vorschlag; abgelehnte und im Foto bestätigte Personen zählen nicht.
                let Some(query) = face.vector.as_deref() else {
                    proposals.push(None);
                    continue;
                };
                let rejected = batch.rejections.get(&face.id);
                let ranking = references.rank(query, face.id, |person| {
                    rejected.is_some_and(|r| r.contains(&person))
                        || faces.iter().any(|o| {
                            o.id != face.id
                                && o.person == Some(person)
                                && o.assignment.as_deref() == Some("user")
                        })
                });
                let allowed: Vec<(i64, f32)> = ranking
                    .into_iter()
                    .filter(|(person, _)| {
                        !face
                            .year
                            .zip(births.get(person))
                            .is_some_and(|(y, b)| y < *b)
                    })
                    .collect();
                proposals.push(limits.accept(&allowed));
            }
            // Gemeinsame Entscheidung je Foto: Dieselbe Person kommt höchstens einmal vor;
            // das Gesicht mit dem höheren Wert erhält sie, das andere bleibt unbenannt.
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
        }
        if writes.is_empty() && saved.elapsed() < std::time::Duration::from_secs(1) {
            cursor = last;
            continue;
        }
        let tx = collection.db.transaction()?;
        for (id, value) in &writes {
            match value {
                Some((person, kind, score)) => {
                    tx.execute("UPDATE faces SET person_id=?,assignment=?,confidence=?,assigned_at=datetime('now') WHERE id=? AND assignment IS NOT 'user'",params![person,kind,score,id])?;
                }
                None => {
                    tx.execute("UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE id=? AND assignment IN ('auto','suggested')",[id])?;
                }
            }
        }
        tx.execute(
            "UPDATE face_maintenance SET after_photo=? WHERE embedder_id=? AND generation IS ?",
            params![last, key, generation],
        )?;
        tx.commit()?;
        saved = std::time::Instant::now();
        cursor = last;
    }
    control.checkpoint()?;
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
