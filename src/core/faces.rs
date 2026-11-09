pub use super::recognition::{apply_knowledge, rematch, suggestions};
use super::{
    collection::Collection,
    inference::{self, Models},
    jobs::Control,
    previews,
    scope::Scope,
};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Clone, Debug)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub birth: String,
    pub count: usize,
    pub gender: String,
    pub note: String,
}
#[derive(Clone, Debug)]
pub struct Face {
    pub id: i64,
    pub photo: i64,
    pub source: String,
    pub bbox: [f32; 4],
    pub person: Option<i64>,
    pub assignment: Option<String>,
    pub ignored: bool,
    pub score: f32,
}
#[derive(Clone, Default)]
pub struct Status {
    pub total: usize,
    pub confirmed: usize,
    pub unknown: usize,
    pub review: usize,
    pub ignored: usize,
    pub references: usize,
    pub reference_people: usize,
    pub dirty_people: usize,
    pub analysed_photos: usize,
    pub present_photos: usize,
    pub classifier: Option<super::classifier::Metrics>,
    pub nameable: usize,
    pub pending: usize,
    pub applied: String,
    pub detector: String,
    pub embedder: String,
    pub auto_import: bool,
    pub nightly: bool,
    pub detector_id: String,
    pub embedder_id: String,
}
#[derive(Default)]
pub struct FacePage {
    pub groups: Vec<Vec<Face>>,
    pub people: Vec<Person>,
    pub status: Status,
    /// Namensvorschläge je Gruppe, Schlüssel ist die ID des ersten Gesichts.
    pub suggestions: BTreeMap<i64, super::recognition::Suggestions>,
    /// Gruppen insgesamt im gewählten Reiter (für die Seitenzahl).
    pub total_groups: usize,
    /// Alle gespeicherten Beziehungen, kanonisch (für die Personenübersicht).
    pub relations: Vec<super::relations::Edge>,
    /// Stammbaum für Verwandtschaftsrechner und Stammbaumansicht (ADR 0061).
    pub family: Box<super::kinship::Family>,
    /// Offene Rückfragen zu wahrscheinlich fehlenden Eltern (ADR 0061).
    pub questions: Vec<super::gaps::Suggestion>,
    /// Zeilen der Personentabelle; nur im Reiter „persons“ gefüllt (ADR 0063).
    pub rows: Vec<super::people_table::PersonRow>,
    /// Porträt je Person (Gesicht mit bester Qualität, ohne Merkmale berechnet);
    /// nur im Reiter „persons“.
    pub portraits: BTreeMap<i64, Face>,
}

/// Tabellenzeilen und Porträts für den Reiter „persons“, sonst leer.
fn table_state(
    collection: &Collection,
    mode: &str,
    people: &[Person],
    edges: &[super::relations::Edge],
    family: &super::kinship::Family,
    questions: &[super::gaps::Suggestion],
) -> Result<(Vec<super::people_table::PersonRow>, BTreeMap<i64, Face>)> {
    if mode != "persons" {
        return Ok((Vec::new(), BTreeMap::new()));
    }
    let stats = super::people_table::load_stats(collection)?;
    let rows = super::people_table::build_rows(
        people,
        &stats,
        edges,
        family,
        questions,
        super::people_table::current_year(),
    );
    let ids: Vec<i64> = people.iter().map(|p| p.id).collect();
    // Ohne Merkmale: eine Fensterabfrage, keine Vektoren dekodieren (Tabelle bleibt schnell).
    let portraits = person_portraits(collection, "", &ids).unwrap_or_default();
    Ok((rows, portraits))
}

/// Kanten, Stammbaum und Rückfragen in einem Zug laden.
fn tree_state(
    collection: &Collection,
) -> Result<(
    Vec<super::relations::Edge>,
    super::kinship::Family,
    Vec<super::gaps::Suggestion>,
)> {
    let edges = super::relations::load_edges(collection)?;
    let family = super::kinship::Family::new(&edges, &super::relations::load_genders(collection)?);
    let questions = super::gaps::suggestions(&edges, &super::gaps::load_decisions(collection)?);
    Ok((edges, family, questions))
}

pub fn setting(collection: &Collection, key: &str) -> Result<Option<String>> {
    let raw: Option<String> = collection
        .db
        .query_row("SELECT value FROM face_settings WHERE key=?", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(raw.and_then(|v| serde_json::from_str::<String>(&v).ok()))
}
pub(crate) fn save_setting(db: &rusqlite::Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}
pub fn people(collection: &Collection) -> Result<Vec<Person>> {
    // Bestätigte Gesichter je Person in einer Gruppierung (Teilindex ix_faces_user_counts)
    // statt einer korrelierten Unterabfrage je Person (ADR 0066).
    let counts: BTreeMap<i64, usize> = collection
        .db
        .prepare("SELECT person_id,count(*) FROM faces WHERE assignment='user' AND retained=0 AND person_id IS NOT NULL GROUP BY person_id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(collection.db.prepare("SELECT p.id,coalesce(a.value,'Unbenannt'),coalesce(b.value,''),coalesce((SELECT value FROM person_attrs WHERE person_id=p.id AND key='gender'),''),coalesce((SELECT value FROM person_attrs WHERE person_id=p.id AND key='note'),'') FROM persons p LEFT JOIN person_attrs a ON a.person_id=p.id AND a.key='name' LEFT JOIN person_attrs b ON b.person_id=p.id AND b.key='birth_year' WHERE p.merged_into IS NULL ORDER BY a.value,p.id")?
        .query_map([],|r|{let id:i64=r.get(0)?;Ok(Person{id,name:r.get(1)?,birth:r.get(2)?,count:counts.get(&id).copied().unwrap_or(0),gender:r.get(3)?,note:r.get(4)?})})?.collect::<rusqlite::Result<_>>()?)
}
pub fn configure_automatic(
    collection: &mut Collection,
    auto_import: bool,
    nightly: bool,
) -> Result<()> {
    collection.writable()?;
    let tx = collection.db.transaction()?;
    for (key, value) in [("auto_import", auto_import), ("nightly", nightly)] {
        tx.execute(
            "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
            params![key, value.to_string()],
        )?;
    }
    tx.commit()?;
    Ok(())
}
pub fn configure_models(
    collection: &mut Collection,
    folder: &Path,
    detector: &str,
    embedder: &str,
) -> Result<()> {
    collection.writable()?;
    let manifests = inference::manifests(folder)?;
    if !manifests
        .iter()
        .any(|m| m.id == detector && m.kind == "detector")
        || !manifests
            .iter()
            .any(|m| m.id == embedder && m.kind == "embedder")
    {
        bail!("Modellauswahl ist im gewählten Modellordner nicht verfügbar");
    }
    let tx = collection.db.transaction()?;
    save_setting(&tx, "active_detector", detector)?;
    save_setting(&tx, "active_embedder", embedder)?;
    tx.commit()?;
    Ok(())
}
pub fn status(collection: &Collection) -> Result<Status> {
    let count =
        |sql: &str| -> Result<usize> { Ok(collection.db.query_row(sql, [], |r| r.get(0))?) };
    // Jede Zählung nutzt einen passenden Teilindex statt die ganze Gesichtstabelle zu lesen
    // (ADR 0066); die Bedingungen entsprechen wörtlich den Indexdefinitionen.
    let mut status = Status {
        total: count(
            "SELECT (SELECT count(*) FROM faces)-(SELECT count(*) FROM faces WHERE retained=1)",
        )?,
        confirmed: count("SELECT count(*) FROM faces WHERE assignment='user' AND retained=0")?,
        unknown: count(
            "SELECT count(*) FROM faces WHERE ignored=0 AND retained=0 AND person_id IS NULL",
        )?,
        review: count(
            "SELECT count(*) FROM faces WHERE ignored=0 AND retained=0 AND assignment IN ('suggested','auto')",
        )?,
        ignored: count("SELECT count(*) FROM faces WHERE ignored=1 AND retained=0")?,
        ..Default::default()
    };
    status.detector = active_detector_key(collection)?.unwrap_or_default();
    status.embedder = setting(collection, "rust.embedder_key")?.unwrap_or_default();
    status.detector_id = setting(collection, "active_detector")?.unwrap_or_else(|| "yunet".into());
    status.embedder_id = setting(collection, "active_embedder")?.unwrap_or_else(|| "sface".into());
    for (key, target) in [
        ("auto_import", &mut status.auto_import),
        ("nightly", &mut status.nightly),
    ] {
        let raw: Option<String> = collection
            .db
            .query_row("SELECT value FROM face_settings WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?;
        *target = raw
            .and_then(|v| serde_json::from_str::<bool>(&v).ok())
            .unwrap_or(false);
    }
    (status.references, status.reference_people) = collection.db.query_row(
        "SELECT count(*),count(DISTINCT person_id) FROM training_set WHERE embedder_id=? AND split='ref'",
        [&status.embedder],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    status.dirty_people = collection.db.query_row(
        "SELECT count(*) FROM face_reference_dirty WHERE embedder_id=?",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.nameable = collection.db.query_row(
        "SELECT coalesce(sum(members),0) FROM face_group_catalog WHERE embedder_id=?",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.analysed_photos = collection.db.query_row(
        "SELECT count(*) FROM face_jobs j WHERE j.status='done' AND j.stage='detect' AND j.model_id=? AND EXISTS(SELECT 1 FROM photos p WHERE p.id=j.photo_id AND p.status='present')",
        [&status.detector], |r| r.get(0),
    )?;
    status.present_photos = collection.db.query_row(
        "SELECT count(*) FROM photos WHERE status='present'",
        [],
        |r| r.get(0),
    )?;
    status.classifier =
        super::classifier::load(collection, &status.embedder)?.and_then(|m| m.metrics);
    status.pending = collection.db.query_row(
        "SELECT count(*) FROM face_maintenance WHERE embedder_id=?",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.applied = setting(
        collection,
        &super::recognition::applied_key(&status.embedder),
    )?
    .unwrap_or_default();
    Ok(status)
}
fn face_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Face> {
    Ok(Face {
        id: r.get(0)?,
        photo: r.get(1)?,
        source: r.get(2)?,
        bbox: [r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?],
        person: r.get(7)?,
        assignment: r.get(8)?,
        ignored: r.get(9)?,
        score: r.get::<_, Option<f32>>(10)?.unwrap_or_default(),
    })
}
const FACE_COLUMNS: &str = "f.id,f.photo_id,f.source_relpath,f.bbox_x1,f.bbox_y1,f.bbox_x2,f.bbox_y2,f.person_id,f.assignment,f.ignored,f.confidence";
/// Das „durchschnittlichste“ bestätigte Gesicht einer Person: das Medoid ihrer
/// hochwertigen Embeddings (höchste mittlere Ähnlichkeit zu den übrigen).
pub fn person_portrait(
    collection: &Collection,
    person: i64,
    embedder: &str,
) -> Result<Option<Face>> {
    let rows: Vec<(i64, Vec<u8>)> = collection
        .db
        .prepare(
            "SELECT f.id,e.vec FROM faces f JOIN face_embeddings e ON e.face_id=f.id
             WHERE f.person_id=? AND f.assignment='user' AND f.ignored=0 AND f.retained=0
               AND f.quality>=0.5 AND e.embedder_id=?
             ORDER BY f.quality DESC,f.id LIMIT 100",
        )?
        .query_map(params![person, embedder], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let vectors: Vec<(i64, Vec<f32>)> = rows
        .into_iter()
        .filter_map(|(id, bytes)| Some((id, inference::normalize(&decode(&bytes).ok()?).ok()?)))
        .collect();
    let best = medoid(&vectors);
    let Some(id) = best else {
        return Ok(None);
    };
    Ok(collection
        .db
        .query_row(
            &format!("SELECT {FACE_COLUMNS} FROM faces f WHERE f.id=?"),
            [id],
            face_row,
        )
        .optional()?)
}
/// ID des Vektors mit der höchsten mittleren Ähnlichkeit zu den übrigen (Medoid);
/// bei Gleichstand die kleinere ID.
fn medoid(vectors: &[(i64, Vec<f32>)]) -> Option<i64> {
    vectors
        .iter()
        .map(|(id, v)| {
            let total: f32 = vectors
                .iter()
                .filter(|(other, _)| other != id)
                .filter_map(|(_, w)| inference::cosine(v, w))
                .sum();
            (*id, total)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(id, _)| id)
}
/// Typischstes Gesicht für viele Personen auf einmal (Personengraph): je Person das
/// Medoid der höchstens 40 besten bestätigten Gesichter mit Merkmalen. Personen
/// ohne Merkmale erhalten ersatzweise ihr Gesicht mit der besten Qualität.
pub fn person_portraits(
    collection: &Collection,
    embedder: &str,
    persons: &[i64],
) -> Result<BTreeMap<i64, Face>> {
    let wanted: BTreeSet<i64> = persons.iter().copied().collect();
    let mut chosen: BTreeMap<i64, i64> = BTreeMap::new();
    if !embedder.is_empty() {
        let rows: Vec<(i64, i64, Vec<u8>)> = collection
            .db
            .prepare(
                "SELECT person_id,id,vec FROM (
                   SELECT f.person_id,f.id,e.vec,
                          row_number() OVER (PARTITION BY f.person_id ORDER BY f.quality DESC,f.id) rn
                   FROM faces f JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=?
                   WHERE f.assignment='user' AND f.ignored=0 AND f.retained=0
                     AND f.quality>=0.5 AND f.person_id IS NOT NULL AND f.bbox_x1 IS NOT NULL
                 ) WHERE rn<=40 ORDER BY person_id,rn",
            )?
            .query_map([embedder], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut by_person: BTreeMap<i64, Vec<(i64, Vec<f32>)>> = BTreeMap::new();
        for (person, id, bytes) in rows {
            if !wanted.contains(&person) {
                continue;
            }
            if let Some(vector) = decode(&bytes)
                .ok()
                .and_then(|v| inference::normalize(&v).ok())
            {
                by_person.entry(person).or_default().push((id, vector));
            }
        }
        for (person, vectors) in by_person {
            if let Some(id) = medoid(&vectors) {
                chosen.insert(person, id);
            }
        }
    }
    let fallback: Vec<(i64, i64)> = collection
        .db
        .prepare(
            "SELECT person_id,id FROM (
               SELECT person_id,id,
                      row_number() OVER (PARTITION BY person_id ORDER BY coalesce(quality,0) DESC,id) rn
               FROM faces WHERE assignment='user' AND ignored=0 AND retained=0 AND person_id IS NOT NULL
                 AND bbox_x1 IS NOT NULL AND bbox_y1 IS NOT NULL AND bbox_x2 IS NOT NULL AND bbox_y2 IS NOT NULL
             ) WHERE rn=1",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (person, id) in fallback {
        if wanted.contains(&person) {
            chosen.entry(person).or_insert(id);
        }
    }
    let mut result = BTreeMap::new();
    for (person, id) in chosen {
        // Ein einzelnes defektes Gesicht darf die übrigen Porträts nicht verhindern.
        if let Some(face) = collection
            .db
            .query_row(
                &format!("SELECT {FACE_COLUMNS} FROM faces f WHERE f.id=?"),
                [id],
                face_row,
            )
            .optional()
            .ok()
            .flatten()
        {
            result.insert(person, face);
        }
    }
    Ok(result)
}
pub fn photo_faces(collection: &Collection, photo: i64) -> Result<Vec<Face>> {
    Ok(collection
        .db
        .prepare(&format!(
            "SELECT {FACE_COLUMNS} FROM faces f WHERE f.photo_id=? AND f.retained=0 ORDER BY f.id"
        ))?
        .query_map([photo], face_row)?
        .collect::<rusqlite::Result<_>>()?)
}
pub fn photo_analysis_done(collection: &Collection, photo: i64) -> Result<bool> {
    let Some(detector) = active_detector_key(collection)? else {
        return Ok(false);
    };
    Ok(collection.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM face_jobs WHERE photo_id=? AND stage='detect' AND model_id=? AND status='done')",
        params![photo, detector],
        |row| row.get(0),
    )?)
}
pub fn active_detector_key(collection: &Collection) -> Result<Option<String>> {
    Ok(setting(collection, "rust.detector_key")?.map(|key| inference::presence_detector_key(&key)))
}
pub fn page(collection: &Collection, mode: &str, page: usize) -> Result<FacePage> {
    let max = crate::config::GROUP_MAX as i64;
    let over = max + 1;
    let per_page = crate::config::FACE_GROUPS_PER_PAGE as i64;
    let status = status(collection)?;
    let mut groups = Vec::new();
    let total_groups: usize;
    if matches!(mode, "persons" | "tree" | "graph") {
        // Diese Reiter zeigen keine Gesichtsgruppen: die teure Gruppenabfrage entfällt.
        let (relations, family, questions) = tree_state(collection)?;
        let people = people(collection)?;
        let (rows, portraits) =
            table_state(collection, mode, &people, &relations, &family, &questions)?;
        return Ok(FacePage {
            groups,
            people,
            status,
            suggestions: BTreeMap::new(),
            total_groups: 0,
            relations,
            family: Box::new(family),
            questions,
            rows,
            portraits,
        });
    }
    if mode == "unknown" && status.embedder.is_empty() {
        let (relations, family, questions) = tree_state(collection)?;
        return Ok(FacePage {
            groups,
            people: people(collection)?,
            status,
            suggestions: BTreeMap::new(),
            total_groups: 0,
            relations,
            family: Box::new(family),
            questions,
            rows: Vec::new(),
            portraits: BTreeMap::new(),
        });
    }
    if mode == "unknown" && !status.embedder.is_empty() {
        total_groups = collection.db.query_row(
            "SELECT count(*) FROM face_group_catalog WHERE embedder_id=? AND members>0",
            [&status.embedder],
            |r| r.get::<_, i64>(0),
        )? as usize;
        let ids=collection.db.prepare("SELECT group_id FROM face_group_catalog WHERE embedder_id=? AND members>0 ORDER BY members DESC,group_id LIMIT ? OFFSET ?")?.query_map(params![status.embedder,per_page,(page as i64)*per_page],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            let group=collection.db.prepare(&format!("SELECT {FACE_COLUMNS} FROM faces f JOIN face_group_members g ON g.face_id=f.id WHERE g.group_id=? AND f.retained=0 AND f.ignored=0 AND f.person_id IS NULL ORDER BY f.id LIMIT {over}"))?.query_map([id],face_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            if !group.is_empty() && group.len() <= crate::config::GROUP_MAX {
                groups.push(group);
            }
        }
    } else {
        // Teilgruppen je Person zu höchstens `max` Gesichtern; die Reihenfolge (größte zuerst,
        // dann Person, dann Teil) entsteht aus den Zählungen je Person statt aus Fensterfunktionen
        // über alle Gesichter des Reiters (ADR 0066).
        let predicate = match mode {
            "review" => "f.assignment IN ('suggested','auto') AND f.ignored=0",
            "ignored" => "f.ignored=1",
            "confirmed" => "f.assignment='user' AND f.ignored=0",
            _ => "f.person_id IS NULL AND f.ignored=0",
        };
        let counts: Vec<(Option<i64>, i64)> = collection
            .db
            .prepare(&format!("SELECT f.person_id,count(*) FROM faces f WHERE f.retained=0 AND {predicate} GROUP BY f.person_id ORDER BY f.person_id"))?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut parts: Vec<(i64, Option<i64>, i64)> = Vec::new();
        for (person, count) in &counts {
            for part in 0..(count + max - 1) / max {
                parts.push(((count - part * max).min(max), *person, part));
            }
        }
        // SQLite ordnet NULL vor Zahlen; `Option` verhält sich ebenso.
        parts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        total_groups = parts.len();
        for (_, person, part) in parts
            .into_iter()
            .skip(page * crate::config::FACE_GROUPS_PER_PAGE)
            .take(crate::config::FACE_GROUPS_PER_PAGE)
        {
            let rows=collection.db.prepare_cached(&format!("SELECT {FACE_COLUMNS} FROM faces f WHERE f.person_id IS ? AND f.retained=0 AND {predicate} ORDER BY f.id LIMIT {max} OFFSET ?"))?.query_map(params![person,part*max],face_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            if !rows.is_empty() {
                groups.push(rows);
            }
        }
    }
    let mut suggestions = BTreeMap::new();
    if !status.embedder.is_empty() {
        let mut cache = super::recognition::SuggestionCache::default();
        for group in groups.iter().filter(|g| g[0].person.is_none()) {
            let ids: Vec<i64> = group.iter().map(|f| f.id).collect();
            // Fehlende Vorschläge dürfen die Gruppenansicht nicht blockieren.
            if let Ok(found) = super::recognition::group_suggestions_cached(
                collection,
                &mut cache,
                &ids,
                &status.embedder,
            ) && !found.items.is_empty()
            {
                suggestions.insert(group[0].id, found);
            }
        }
    }
    let (relations, family, questions) = tree_state(collection)?;
    let people = people(collection)?;
    let (rows, portraits) =
        table_state(collection, mode, &people, &relations, &family, &questions)?;
    Ok(FacePage {
        groups,
        people,
        status,
        suggestions,
        total_groups,
        relations,
        family: Box::new(family),
        questions,
        rows,
        portraits,
    })
}

pub fn analyze(
    collection: &mut Collection,
    model_dir: &Path,
    dll: &Path,
    detector: &str,
    embedder: &str,
    exiftool: &str,
    control: &Control,
) -> Result<String> {
    analyze_scoped(
        collection,
        model_dir,
        dll,
        detector,
        embedder,
        exiftool,
        control,
        &Scope::default(),
    )
}
/// Analyse neuester Fotos zuerst, optional nur eine Portion oder ein Ordner (ADR 0059).
#[allow(clippy::too_many_arguments)]
pub fn analyze_scoped(
    collection: &mut Collection,
    model_dir: &Path,
    dll: &Path,
    detector: &str,
    embedder: &str,
    exiftool: &str,
    control: &Control,
    scope: &Scope,
) -> Result<String> {
    collection.writable()?;
    control.set_face_phase(super::jobs::FacePhase::Analysis);
    let mut models = Models::load(model_dir, dll, detector, embedder)?;
    let detector_key = inference::presence_detector_key(&models.detector.key());
    let embedder_key = models.embedder.key();
    if setting(collection, "rust.detector_key")?.is_some_and(|key| key != detector_key) {
        collection.db.backup(
            "main",
            collection.root.join(format!(
                ".myfotomanager.before-detector-{}.bak",
                uuid::Uuid::new_v4()
            )),
            None,
        )?;
    }
    for (key, value) in [
        ("rust.auto", models.embedder.thresholds.auto),
        ("rust.suggest", models.embedder.thresholds.suggest),
        ("rust.duplicate", models.embedder.thresholds.duplicate),
    ] {
        collection.db.execute(
            "INSERT OR IGNORE INTO face_settings VALUES(?,?)",
            params![
                format!(
                    "rust.thresholds.{embedder_key}.{}",
                    key.strip_prefix("rust.").unwrap_or(key)
                ),
                value.to_string()
            ],
        )?;
    }
    save_setting(&collection.db, "rust.detector_key", &detector_key)?;
    save_setting(&collection.db, "rust.embedder_key", &embedder_key)?;
    save_setting(&collection.db, "active_detector", detector)?;
    save_setting(&collection.db, "active_embedder", embedder)?;
    let order = super::scope::analysis_order(collection, scope)?;
    let total = scope.limit.map_or(order.len(), |l| l.min(order.len()));
    let (mut position, mut done, mut worked) = (0usize, 0usize, 0usize);
    let (mut found, mut failed, mut without_vector) = (0, 0, 0);
    let mut portion_full = false;
    loop {
        control.checkpoint()?;
        let ids = &order[position..(position + 32).min(order.len())];
        if ids.is_empty() || portion_full {
            break;
        }
        position += ids.len();
        let rows = analysis_rows(collection, ids)?;
        for (photo, file, relative, size, mtime, orientation, date) in rows {
            control.checkpoint()?;
            let fingerprint = format!("rust-v1:{file}:{size}:{mtime}:{orientation}:{embedder_key}");
            let complete:bool=collection.db.query_row("SELECT EXISTS(SELECT 1 FROM face_jobs WHERE photo_id=? AND stage='detect' AND model_id=? AND status='done' AND fingerprint=?)",params![photo,detector_key,fingerprint],|r|r.get(0))?;
            if !complete && scope.limit.is_some_and(|l| worked >= l) {
                portion_full = true;
                break;
            }
            if !complete {
                worked += 1;
            }
            if !complete {
                let result = (|| -> Result<(usize, usize)> {
                    let image = previews::load(
                        &collection.safe_path(&relative)?,
                        exiftool,
                        orientation,
                        1600,
                        control,
                    )?;
                    let detections = models.detect_with_control(&image, control)?;
                    let mut embedded = Vec::with_capacity(detections.len());
                    let mut embedding_errors = 0;
                    for detection in &detections {
                        control.checkpoint()?;
                        let quality = detection.embedding_quality();
                        if quality < 0.5 {
                            embedded.push((quality, None));
                            continue;
                        }
                        match models.embed(&image, detection) {
                            Ok(vector) => embedded.push((quality, Some(vector))),
                            Err(_) => {
                                embedding_errors += 1;
                                embedded.push((0.0, None));
                            }
                        }
                    }
                    let small: Vec<usize> = detections
                        .iter()
                        .enumerate()
                        .filter(|(i, d)| embedded[*i].1.is_none() && is_small_but_clear(d))
                        .map(|(i, _)| i)
                        .collect();
                    let hires_tried = !small.is_empty();
                    if hires_tried
                        && let Ok(hi) = previews::load(
                            &collection.safe_path(&relative)?,
                            exiftool,
                            orientation,
                            HIRES_EDGE,
                            control,
                        )
                    {
                        let factor = hi.width() as f32 / image.width() as f32;
                        if factor > 1.05 {
                            for i in small {
                                control.checkpoint()?;
                                if let Some(found) =
                                    embed_hires(&mut models, &hi, &detections[i], factor)
                                {
                                    embedded[i] = (found.0, Some(found.1));
                                }
                            }
                        }
                    }
                    let old=collection.db.prepare(&format!("SELECT {FACE_COLUMNS} FROM faces f WHERE f.photo_id=? ORDER BY f.assignment='user' DESC,f.retained,f.id"))?.query_map([photo],face_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
                    let tx = collection.db.transaction()?;
                    let mut used = BTreeSet::new();
                    for (detection, (quality, vector)) in detections.iter().zip(&embedded) {
                        let bbox = [
                            detection.bbox[0] / image.width() as f32,
                            detection.bbox[1] / image.height() as f32,
                            detection.bbox[2] / image.width() as f32,
                            detection.bbox[3] / image.height() as f32,
                        ];
                        let previous = old
                            .iter()
                            .filter(|f| f.source == relative && !used.contains(&f.id))
                            .map(|f| (inference::iou(f.bbox, bbox), f))
                            .max_by(|a, b| a.0.total_cmp(&b.0))
                            .filter(|x| x.0 >= 0.5)
                            .map(|x| x.1);
                        let landmarks = serde_json::to_string(&detection.landmarks)?;
                        let id = if let Some(previous) = previous {
                            used.insert(previous.id);
                            tx.execute("UPDATE faces SET detector_id=?,bbox_x1=?,bbox_y1=?,bbox_x2=?,bbox_y2=?,landmarks=?,det_score=?,quality=?,retained=0,source_fingerprint=? WHERE id=?",params![detector_key,bbox[0],bbox[1],bbox[2],bbox[3],landmarks,detection.score,quality,fingerprint,previous.id])?;
                            previous.id
                        } else {
                            tx.execute("INSERT INTO faces(photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,captured_at,source_fingerprint) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",params![photo,file,relative,detector_key,bbox[0],bbox[1],bbox[2],bbox[3],landmarks,detection.score,quality,date,fingerprint])?;
                            tx.last_insert_rowid()
                        };
                        if let Some(vector) = vector {
                            tx.execute(
                                "INSERT OR REPLACE INTO face_embeddings VALUES(?,?,?,?)",
                                params![id, embedder_key, vector.len() as i64, encode(vector)],
                            )?;
                        } else {
                            // A weaker detection may retain an explicit label or pin, never an old suggestion.
                            tx.execute(
                                "UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL,assigned_at=NULL WHERE id=? AND assignment IN ('auto','suggested')",
                                [id],
                            )?;
                        }
                    }
                    for face in old.iter().filter(|f| !used.contains(&f.id)) {
                        if face.assignment.as_deref() == Some("user") || face.ignored {
                            tx.execute("UPDATE faces SET retained=1 WHERE id=?", [face.id])?;
                        } else {
                            delete_face_rows(&tx, face.id)?;
                        }
                    }
                    tx.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'detect',?,'done',NULL,datetime('now'),?)",params![photo,detector_key,fingerprint])?;
                    if hires_tried {
                        tx.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'hires',?,'done',NULL,datetime('now'),?)",params![photo,embedder_key,fingerprint])?;
                    }
                    let embed_status = if embedding_errors == 0 {
                        "done"
                    } else {
                        "error"
                    };
                    let embed_error = (embedding_errors > 0).then(|| {
                        format!("{embedding_errors} Gesichter ohne berechenbare Namensmerkmale")
                    });
                    tx.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'embed',?,?,?,datetime('now'),?)",params![photo,embedder_key,embed_status,embed_error,fingerprint])?;
                    control.checkpoint()?;
                    tx.commit()?;
                    Ok((
                        detections.len(),
                        embedded
                            .iter()
                            .filter(|(_, vector)| vector.is_none())
                            .count(),
                    ))
                })();
                match result {
                    Ok((n, presence_only)) => {
                        found += n;
                        without_vector += presence_only;
                    }
                    Err(e) => {
                        control.checkpoint()?;
                        failed += 1;
                        collection.db.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'detect',?,'error',?,datetime('now'),?)",params![photo,detector_key,e.to_string(),fingerprint])?;
                    }
                }
            }
            if complete {
                // Eine fehlgeschlagene Nachberechnung bricht die Analyse nicht ab.
                let _ = upgrade_small_faces(
                    collection,
                    &mut models,
                    (photo, &relative, orientation),
                    (&embedder_key, &fingerprint),
                    exiftool,
                    control,
                );
                control.checkpoint()?;
            }
            done += 1;
            control.progress(if scope.limit.is_some() { worked } else { done }, total);
        }
        prepare_groups(collection, &embedder_key, control)?;
    }
    super::grouping::rebuild_groups(collection, &embedder_key, control)?;
    // Die Sicherung kostet bei großen Sammlungen spürbar Zeit: nur nach neuer Arbeit.
    if worked > 0 {
        collection.backup()?;
    }
    Ok(format!(
        "{done} Fotos geprüft, {worked} davon neu analysiert · {found} Gesichter erkannt · {without_vector} ohne Namensmerkmale · {failed} Analysefehler"
    ))
}
type AnalysisRow = (i64, i64, String, i64, i64, u32, String);
/// Einzelheiten der Fotos mit diesen IDs, in der Reihenfolge von `ids`.
fn analysis_rows(collection: &Collection, ids: &[i64]) -> Result<Vec<AnalysisRow>> {
    let marks = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT f.photo_id,f.id,f.relpath,f.size,f.mtime_ns,coalesce(m.orientation,1),coalesce(m.dt_original,'') FROM face_sources s JOIN files f ON f.id=s.file_id LEFT JOIN meta m ON m.file_id=f.id WHERE f.photo_id IN ({marks}) AND f.status='present'"
    );
    let mut rows: Vec<AnalysisRow> = collection
        .db
        .prepare(&sql)?
        .query_map(rusqlite::params_from_iter(ids.iter()), |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let rank: BTreeMap<i64, usize> = ids.iter().enumerate().map(|(i, p)| (*p, i)).collect();
    rows.sort_by_key(|r| rank.get(&r.0).copied().unwrap_or(usize::MAX));
    Ok(rows)
}
/// Längere Bildkante für die Nachberechnung kleiner, klar erkannter Gesichter.
const HIRES_EDGE: u32 = 3200;

fn is_small_but_clear(detection: &inference::Detection) -> bool {
    detection.score >= 0.8 && detection.embedding_quality() < 0.5
}
/// Größe und Namensmerkmale aus dem höher aufgelösten Bild; `None`, wenn das
/// Gesicht auch dort zu klein oder nicht berechenbar ist.
fn embed_hires(
    models: &mut Models,
    hi: &image::DynamicImage,
    detection: &inference::Detection,
    factor: f32,
) -> Option<(f32, Vec<f32>)> {
    let scaled = detection.scaled(factor);
    let quality = scaled.embedding_quality();
    if quality < 0.5 {
        return None;
    }
    models
        .embed(hi, &scaled)
        .ok()
        .map(|vector| (quality, vector))
}
/// Bereits analysierte Fotos nachrechnen: kleine, klar erkannte Gesichter ohne
/// Namensmerkmale erhalten sie aus dem höher aufgelösten Bild. Pro Foto und
/// Analysestand höchstens einmal.
fn upgrade_small_faces(
    collection: &mut Collection,
    models: &mut Models,
    (photo, relative, orientation): (i64, &str, u32),
    (embedder_key, fingerprint): (&str, &str),
    exiftool: &str,
    control: &Control,
) -> Result<()> {
    let tried: bool = collection.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM face_jobs WHERE photo_id=? AND stage='hires' AND model_id=? AND fingerprint=?)",
        params![photo, embedder_key, fingerprint],
        |r| r.get(0),
    )?;
    if tried {
        return Ok(());
    }
    let candidates: Vec<(i64, f32, String, [f32; 4])> = collection
        .db
        .prepare(
            "SELECT f.id,f.det_score,f.landmarks,f.bbox_x1,f.bbox_y1,f.bbox_x2,f.bbox_y2 FROM faces f
             WHERE f.photo_id=? AND f.retained=0 AND f.det_score>=0.8 AND f.quality<0.5
               AND f.landmarks IS NOT NULL
               AND NOT EXISTS(SELECT 1 FROM face_embeddings e WHERE e.face_id=f.id AND e.embedder_id=?)",
        )?
        .query_map(params![photo, embedder_key], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                [r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?],
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    if candidates.is_empty() {
        return Ok(());
    }
    let hi = previews::load(
        &collection.safe_path(relative)?,
        exiftool,
        orientation,
        HIRES_EDGE,
        control,
    )?;
    let long = hi.width().max(hi.height()) as f32;
    // Gespeicherte Punkte stammen aus dem auf 1600 begrenzten Analysebild.
    let factor = long / long.min(1600.0);
    let (width, height) = (hi.width() as f32, hi.height() as f32);
    let mut upgraded = Vec::new();
    if factor > 1.05 {
        for (id, score, landmarks, bbox) in candidates {
            control.checkpoint()?;
            let Ok(landmarks) = serde_json::from_str::<[[f32; 2]; 5]>(&landmarks) else {
                continue;
            };
            let detection = inference::Detection {
                bbox: [
                    bbox[0] * width,
                    bbox[1] * height,
                    bbox[2] * width,
                    bbox[3] * height,
                ],
                landmarks: landmarks.map(|p| p.map(|v| v * factor)),
                score,
            };
            if let Some((quality, vector)) = embed_hires(models, &hi, &detection, 1.0) {
                upgraded.push((id, quality, vector));
            }
        }
    }
    let tx = collection.db.transaction()?;
    for (id, quality, vector) in &upgraded {
        tx.execute(
            "UPDATE faces SET quality=? WHERE id=?",
            params![quality, id],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO face_embeddings VALUES(?,?,?,?)",
            params![id, embedder_key, vector.len() as i64, encode(vector)],
        )?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO face_jobs VALUES(?,'hires',?,'done',NULL,datetime('now'),?)",
        params![photo, embedder_key, fingerprint],
    )?;
    tx.commit()?;
    Ok(())
}
pub fn encode(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}
pub fn decode(bytes: &[u8]) -> Result<Vec<f32>> {
    if !bytes.len().is_multiple_of(4) {
        bail!("Ungültige Vektorlänge");
    }
    let vector: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    inference::normalize(&vector)
}
fn signature(vector: &[f32]) -> i64 {
    vector
        .iter()
        .take(12)
        .enumerate()
        .fold(0, |bits, (i, v)| bits | (((*v >= 0.0) as i64) << i))
}

pub fn prepare_groups(collection: &mut Collection, key: &str, control: &Control) -> Result<usize> {
    collection.writable()?;
    let rows=collection.db.prepare("SELECT f.id,f.photo_id,e.vec FROM face_group_dirty d JOIN faces f ON f.id=d.face_id JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=d.embedder_id WHERE d.embedder_id=? ORDER BY f.id LIMIT 128")?.query_map([key],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let tx = collection.db.transaction()?;
    for (id, photo, bytes) in &rows {
        control.checkpoint()?;
        let eligible:bool=tx.query_row("SELECT person_id IS NULL AND ignored=0 AND retained=0 AND quality>=0.5 FROM faces WHERE id=?",[id],|r|r.get(0))?;
        if eligible {
            let vector = decode(bytes)?;
            let sig = signature(&vector);
            let candidates=tx.prepare("SELECT group_id FROM face_group_catalog WHERE embedder_id=? AND members<? AND (signature=? OR signature IN (?,?,?,?,?,?,?,?,?,?,?,?)) ORDER BY group_id LIMIT 32")?
                .query_map(params![key,crate::config::GROUP_MAX as i64,sig,sig^1,sig^2,sig^4,sig^8,sig^16,sig^32,sig^64,sig^128,sig^256,sig^512,sig^1024,sig^2048],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut matched = None;
            for group in candidates {
                let members=tx.prepare("SELECT f.photo_id,e.vec FROM face_group_members g JOIN faces f ON f.id=g.face_id JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=g.embedder_id WHERE g.group_id=?")?.query_map([group],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Vec<u8>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                if members.iter().all(|(p, v)| {
                    p != photo
                        && decode(v)
                            .ok()
                            .and_then(|v| inference::cosine(&vector, &v))
                            .is_some_and(|s| s >= 0.6)
                }) {
                    matched = Some(group);
                    break;
                }
            }
            let group = if let Some(group) = matched {
                group
            } else {
                tx.execute("INSERT INTO face_group_catalog(embedder_id,signature,representative,members) VALUES(?,?,?,0)",params![key,sig,bytes])?;
                tx.last_insert_rowid()
            };
            tx.execute(
                "INSERT OR IGNORE INTO face_group_members VALUES(?,?,?)",
                params![key, id, group],
            )?;
            tx.execute("UPDATE face_group_catalog SET members=(SELECT count(*) FROM face_group_members WHERE group_id=?) WHERE group_id=?",params![group,group])?;
        }
        tx.execute(
            "DELETE FROM face_group_dirty WHERE embedder_id=? AND face_id=?",
            params![key, id],
        )?;
    }
    tx.commit()?;
    Ok(rows.len())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FaceSnapshot {
    pub id: i64,
    pub person: Option<i64>,
    pub assignment: Option<String>,
    pub confidence: Option<f64>,
    pub ignored: bool,
    pub rejections: Vec<i64>,
    pub pins: Vec<(i64, String)>,
}
#[derive(Clone)]
pub struct Undo {
    pub before: Vec<FaceSnapshot>,
    pub after: Vec<FaceSnapshot>,
    pub created: Option<i64>,
}
fn snapshot(db: &rusqlite::Connection, id: i64) -> Result<FaceSnapshot> {
    let mut value = db.query_row(
        "SELECT person_id,assignment,confidence,ignored FROM faces WHERE id=? AND retained=0",
        [id],
        |r| {
            Ok(FaceSnapshot {
                id,
                person: r.get(0)?,
                assignment: r.get(1)?,
                confidence: r.get(2)?,
                ignored: r.get(3)?,
                rejections: Vec::new(),
                pins: Vec::new(),
            })
        },
    )?;
    value.rejections = db
        .prepare("SELECT person_id FROM face_rejections WHERE face_id=? ORDER BY person_id")?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    value.pins=db.prepare("SELECT person_id,embedder_id FROM training_set WHERE face_id=? AND pinned=1 ORDER BY person_id,embedder_id")?.query_map([id],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(value)
}
pub fn name_group(
    collection: &mut Collection,
    ids: &[i64],
    selected: &BTreeSet<i64>,
    person: Option<i64>,
    name: &str,
    ignore: bool,
) -> Result<Undo> {
    collection.writable()?;
    // Dieselbe Obergrenze wie bei der Gruppenbildung und der Anzeige (ADR 0056).
    let max = crate::config::GROUP_MAX;
    if ids.is_empty() || ids.len() > max {
        bail!("Gruppe muss 1 bis {max} sichtbare Gesichter enthalten");
    }
    if !ignore && selected.is_empty() {
        bail!("Bitte mindestens ein Gesicht auswählen");
    }
    let tx = collection.db.transaction()?;
    let before = ids
        .iter()
        .map(|id| snapshot(&tx, *id))
        .collect::<Result<Vec<_>>>()?;
    let mut created = None;
    let person = if ignore {
        None
    } else if let Some(person) = person {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM persons WHERE id=? AND merged_into IS NULL)",
            [person],
            |r| r.get(0),
        )?;
        if !exists {
            bail!("Person ist nicht mehr verfügbar");
        }
        Some(person)
    } else {
        let name = name.trim();
        if name.is_empty() {
            bail!("Bitte einen Namen eingeben");
        }
        let matches = tx
            .prepare("SELECT person_id FROM person_attrs WHERE key='name' AND casefold(value)=?")?
            .query_map([super::files::fold(name)], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        match matches.as_slice() {
            [id] => Some(*id),
            [] => {
                tx.execute("INSERT INTO persons DEFAULT VALUES", [])?;
                let id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO person_attrs VALUES(?,'name',?)",
                    params![id, name],
                )?;
                created = Some(id);
                Some(id)
            }
            _ => bail!("Mehrere Personen heißen so. Bitte die konkrete Personen-ID wählen."),
        }
    };
    for id in ids {
        if ignore {
            tx.execute("UPDATE faces SET ignored=1,person_id=CASE WHEN assignment='user' THEN person_id ELSE NULL END,assignment=CASE WHEN assignment='user' THEN assignment ELSE NULL END WHERE id=?",[id])?;
        } else if selected.contains(id) {
            tx.execute("UPDATE faces SET person_id=?,assignment='user',confidence=1,ignored=0,assigned_at=datetime('now') WHERE id=?",params![person,id])?;
            tx.execute(
                "DELETE FROM face_rejections WHERE face_id=? AND person_id=?",
                params![id, person],
            )?;
        } else {
            tx.execute(
                "INSERT OR IGNORE INTO face_rejections VALUES(?,?)",
                params![id, person],
            )?;
        }
        tx.execute("DELETE FROM training_set WHERE face_id=?", [id])?;
    }
    let after = ids
        .iter()
        .map(|id| snapshot(&tx, *id))
        .collect::<Result<Vec<_>>>()?;
    tx.commit()?;
    Ok(Undo {
        before,
        after,
        created,
    })
}
pub fn undo(collection: &mut Collection, undo: &Undo) -> Result<()> {
    collection.writable()?;
    let tx = collection.db.transaction()?;
    for expected in &undo.after {
        let current = snapshot(&tx, expected.id)?;
        if current.person != expected.person
            || current.assignment != expected.assignment
            || current.ignored != expected.ignored
            || current.rejections != expected.rejections
            || current.pins != expected.pins
        {
            bail!("Spätere Nutzeränderung verhindert Rücknahme");
        }
    }
    for before in &undo.before {
        tx.execute(
            "UPDATE faces SET person_id=?,assignment=?,confidence=?,ignored=? WHERE id=?",
            params![
                before.person,
                before.assignment,
                before.confidence,
                before.ignored,
                before.id
            ],
        )?;
        tx.execute("DELETE FROM face_rejections WHERE face_id=?", [before.id])?;
        for person in &before.rejections {
            tx.execute(
                "INSERT INTO face_rejections VALUES(?,?)",
                params![before.id, person],
            )?;
        }
        tx.execute("DELETE FROM training_set WHERE face_id=?", [before.id])?;
        for (person, key) in &before.pins {
            tx.execute(
                "INSERT INTO training_set VALUES(?,?,?,'ref',datetime('now'),'pin',1)",
                params![person, before.id, key],
            )?;
        }
    }
    if let Some(id) = undo.created {
        let used:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM faces WHERE person_id=?) OR EXISTS(SELECT 1 FROM relations WHERE from_person=? OR to_person=?)",params![id,id,id],|r|r.get(0))?;
        if !used {
            tx.execute("DELETE FROM person_attrs WHERE person_id=?", [id])?;
            tx.execute("DELETE FROM persons WHERE id=?", [id])?;
        }
    }
    tx.commit()?;
    Ok(())
}
fn delete_face_rows(db: &rusqlite::Connection, id: i64) -> Result<()> {
    for table in [
        "face_embeddings",
        "face_rejections",
        "training_set",
        "training_exclusions",
        "face_review",
    ] {
        db.execute(&format!("DELETE FROM {table} WHERE face_id=?"), [id])?;
    }
    db.execute("DELETE FROM faces WHERE id=?", [id])?;
    Ok(())
}
pub fn delete_face(collection: &mut Collection, photo: i64, id: i64) -> Result<()> {
    collection.writable()?;
    let tx = collection.db.transaction()?;
    let valid: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM faces WHERE id=? AND photo_id=? AND retained=0)",
        params![id, photo],
        |r| r.get(0),
    )?;
    if !valid {
        bail!("Gesicht gehört nicht mehr zum ausgewählten Foto");
    }
    tx.execute("INSERT OR IGNORE INTO face_reference_dirty SELECT e.embedder_id,f.person_id FROM face_embeddings e JOIN faces f ON f.id=e.face_id WHERE f.id=? AND f.person_id IS NOT NULL",[id])?;
    tx.execute("INSERT INTO face_maintenance SELECT embedder_id,0,1 FROM face_embeddings WHERE face_id=? ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[id])?;
    delete_face_rows(&tx, id)?;
    tx.commit()?;
    Ok(())
}

/// Speichert "`source` ist `kind` von `target`" kanonisch und einmalig.
/// `kind`: parent, child, partner, sibling, cousin, friend, acquaintance (ADR 0060).
pub fn add_relation(
    collection: &mut Collection,
    source: i64,
    kind: &str,
    target: i64,
) -> Result<()> {
    let Some(role) = super::relations::Role::from_key(kind) else {
        bail!("Unbekannter Beziehungstyp");
    };
    super::relations::add(collection, source, role, target)?;
    Ok(())
}
pub fn remove_relation(
    collection: &Collection,
    source: i64,
    kind: &str,
    target: i64,
) -> Result<()> {
    let Some(role) = super::relations::Role::from_key(kind) else {
        bail!("Unbekannter Beziehungstyp");
    };
    super::relations::remove(collection, source, role, target)
}

/// Längenbegrenzungen für manuell angelegte Personen.
pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_NOTE_CHARS: usize = 500;

/// Eingaben für eine Person ohne Gesichtsbezug (z. B. verstorbene Großeltern).
#[derive(Clone, Debug, Default)]
pub struct NewPerson {
    pub name: String,
    /// "weiblich", "männlich" oder leer (divers/unbekannt).
    pub gender: String,
    pub birth_year: Option<i32>,
    pub note: String,
}

/// Prüft die Eingaben und liefert den bereinigten Namen.
pub fn validate_new_person(new: &NewPerson) -> Result<String> {
    let name = new.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        bail!("Bitte einen Namen eingeben");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        bail!("Der Name darf höchstens {MAX_NAME_CHARS} Zeichen lang sein");
    }
    if new.note.chars().count() > MAX_NOTE_CHARS {
        bail!("Die Notiz darf höchstens {MAX_NOTE_CHARS} Zeichen lang sein");
    }
    if !matches!(new.gender.as_str(), "" | "weiblich" | "männlich") {
        bail!("Unbekanntes Geschlecht");
    }
    if let Some(year) = new.birth_year
        && !(1000..=3000).contains(&year)
    {
        bail!("Das Geburtsjahr muss zwischen 1000 und 3000 liegen");
    }
    Ok(name)
}

/// Legt eine Person ohne Gesichter an. Namensdubletten sind erlaubt (die Oberfläche
/// warnt; siehe [`people_named`]). Gesichtszuordnungen bleiben unberührt.
pub fn add_person(collection: &mut Collection, new: &NewPerson) -> Result<i64> {
    collection.writable()?;
    let name = validate_new_person(new)?;
    let tx = collection.db.transaction()?;
    tx.execute("INSERT INTO persons DEFAULT VALUES", [])?;
    let id = tx.last_insert_rowid();
    let mut attrs = vec![("name", name)];
    if !new.gender.is_empty() {
        attrs.push(("gender", new.gender.clone()));
    }
    if let Some(year) = new.birth_year {
        attrs.push(("birth_year", year.to_string()));
    }
    let note = new.note.trim();
    if !note.is_empty() {
        attrs.push(("note", note.to_string()));
    }
    for (key, value) in attrs {
        tx.execute(
            "INSERT INTO person_attrs VALUES(?,?,?)",
            params![id, key, value],
        )?;
    }
    tx.commit()?;
    Ok(id)
}

/// Aktive Personen mit diesem Namen (ohne Groß-/Kleinschreibung), für die Dublettenwarnung.
pub fn people_named(people: &[Person], name: &str) -> Vec<i64> {
    let normal =
        |text: &str| super::files::fold(&text.split_whitespace().collect::<Vec<_>>().join(" "));
    let wanted = normal(name);
    people
        .iter()
        .filter(|p| normal(&p.name) == wanted)
        .map(|p| p.id)
        .collect()
}
