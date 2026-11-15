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
pub struct References {
    entries: Vec<(i64, i64, Vec<f32>)>,
    names: BTreeMap<i64, String>,
}
impl References {
    pub fn load(collection: &Collection, key: &str) -> Result<Self> {
        let mut statement=collection.db.prepare("SELECT f.id,f.person_id,e.vec,coalesce(a.value,'Unbenannt') FROM faces f JOIN face_embeddings e ON e.face_id=f.id LEFT JOIN person_attrs a ON a.person_id=f.person_id AND a.key='name' WHERE e.embedder_id=?1 AND f.assignment='user' AND f.ignored=0 AND f.quality>=0.5 AND (EXISTS(SELECT 1 FROM training_set t WHERE t.face_id=f.id AND t.embedder_id=?1 AND t.split='ref') OR NOT EXISTS(SELECT 1 FROM training_set t WHERE t.person_id=f.person_id AND t.embedder_id=?1)) AND NOT EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=e.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_review v WHERE v.face_id=f.id AND v.embedder_id=e.embedder_id) ORDER BY f.person_id,f.id")?;
        let mut rows = statement.query([key])?;
        let (mut entries, mut names) = (Vec::new(), BTreeMap::new());
        while let Some(row) = rows.next()? {
            let person: i64 = row.get(1)?;
            let vector = decode(&row.get::<_, Vec<u8>>(2)?)?;
            names
                .entry(person)
                .or_insert_with(|| row.get(3).unwrap_or_default());
            entries.push((row.get(0)?, person, vector));
        }
        Ok(Self { entries, names })
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
    let mut scores: BTreeMap<i64, Vec<f32>> = BTreeMap::new();
    for (id, person, vector) in &references.entries {
        if *id == face || rejected.contains(person) || occupied.contains(person) {
            continue;
        }
        if let Some(score) = inference::cosine(&query, vector) {
            let entry = scores.entry(*person).or_default();
            entry.push(score);
            entry.sort_by(|a, b| b.total_cmp(a));
            entry.truncate(3);
        }
    }
    let mut result: Vec<_> = scores
        .into_iter()
        .map(|(id, scores)| {
            (
                id,
                references.names.get(&id).cloned().unwrap_or_default(),
                scores.iter().sum::<f32>() / scores.len() as f32,
            )
        })
        .collect();
    result.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
    Ok(result)
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
/// höchstens drei Gesichter (begrenzt die Lesezeit je Seite).
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
    for face in faces.iter().take(3) {
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

pub fn rematch(collection: &mut Collection, key: &str, control: &Control) -> Result<()> {
    control.set_face_phase(super::jobs::FacePhase::Matching);
    let mut cursor: i64 = collection
        .db
        .query_row(
            "SELECT after_photo FROM face_maintenance WHERE embedder_id=?",
            [key],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let limits = Limits::load(collection)?;
    let births: BTreeMap<i64, i32> = collection
        .db
        .prepare("SELECT person_id,value FROM person_attrs WHERE key='birth_year'")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok().and_then(|(id, year)| Some((id, year.parse().ok()?))))
        .collect();
    let references = References::load(collection, key)?;
    loop {
        control.checkpoint()?;
        let photos=collection.db.prepare("SELECT DISTINCT photo_id FROM faces WHERE photo_id>? AND retained=0 ORDER BY photo_id LIMIT 32")?.query_map([cursor],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        if photos.is_empty() {
            break;
        }
        for photo in photos {
            control.checkpoint()?;
            let faces = faces::photo_faces(collection, photo)?;
            let candidates: Vec<_> = faces
                .iter()
                .filter(|f| !f.ignored && f.assignment.as_deref() != Some("user"))
                .collect();
            // Je Gesicht höchstens ein Kandidat: die beste zulässige Person, wenn sie die
            // Schwelle erreicht und klar vor der zweitbesten liegt.
            let mut proposals: Vec<Option<(i64, f32)>> = Vec::with_capacity(candidates.len());
            for face in &candidates {
                control.checkpoint()?;
                let year: Option<i32> = collection
                    .db
                    .query_row(
                        "SELECT captured_at FROM faces WHERE id=?",
                        [face.id],
                        |r| r.get::<_, Option<String>>(0),
                    )?
                    .as_deref()
                    .and_then(|v| v.get(..4))
                    .and_then(|v| v.parse().ok());
                // `ranked_with` schließt Ablehnungen, im Foto bestätigte Personen und Gesichter
                // ohne ausreichende Qualität bereits aus und liefert absteigend sortiert.
                let ranking = ranked_with(collection, &references, face.id, key)?;
                let allowed: Vec<(i64, f32)> = ranking
                    .into_iter()
                    .filter(|(person, _, _)| {
                        !year.zip(births.get(person)).is_some_and(|(y, b)| y < *b)
                    })
                    .map(|(person, _, score)| (person, score))
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
            let tx = collection.db.transaction()?;
            tx.execute("UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE photo_id=? AND assignment IN ('auto','suggested')",[photo])?;
            for (row, col) in decisions.into_iter().enumerate() {
                if col < people.len() && matrix[row][col] >= limits.review {
                    let score = matrix[row][col];
                    tx.execute("UPDATE faces SET person_id=?,assignment=?,confidence=?,assigned_at=datetime('now') WHERE id=? AND assignment IS NOT 'user'",params![people[col],if score>=limits.sure{"auto"}else{"suggested"},score,candidates[row].id])?;
                }
            }
            tx.execute(
                "UPDATE face_maintenance SET after_photo=? WHERE embedder_id=?",
                params![photo, key],
            )?;
            control.checkpoint()?;
            tx.commit()?;
            cursor = photo;
        }
    }
    control.checkpoint()?;
    let tx = collection.db.transaction()?;
    tx.execute("DELETE FROM face_maintenance WHERE embedder_id=?", [key])?;
    faces::save_setting(&tx, &applied_key(key), &chrono::Utc::now().to_rfc3339())?;
    tx.commit()?;
    Ok(())
}
