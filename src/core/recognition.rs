use super::{
    classifier,
    collection::Collection,
    faces::{self, decode},
    inference,
    jobs::Control,
    matching::{self, Candidate},
};
use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

fn ranked(collection: &Collection, face: i64, key: &str) -> Result<Vec<(i64, String, f32)>> {
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
    let mut statement=collection.db.prepare("SELECT f.person_id,e.vec,coalesce(a.value,'Unbenannt') FROM faces f JOIN face_embeddings e ON e.face_id=f.id LEFT JOIN person_attrs a ON a.person_id=f.person_id AND a.key='name' WHERE e.embedder_id=? AND f.id!=? AND f.assignment='user' AND f.ignored=0 AND f.quality>=0.5 AND (EXISTS(SELECT 1 FROM training_set t WHERE t.face_id=f.id AND t.embedder_id=? AND t.split='ref') OR NOT EXISTS(SELECT 1 FROM training_set t WHERE t.person_id=f.person_id AND t.embedder_id=?)) AND NOT EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=e.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_review v WHERE v.face_id=f.id AND v.embedder_id=e.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_rejections r WHERE r.face_id=? AND r.person_id=f.person_id) AND NOT EXISTS(SELECT 1 FROM faces o WHERE o.photo_id=? AND o.assignment='user' AND o.person_id=f.person_id AND o.id!=?) ORDER BY f.person_id,f.id")?;
    let mut rows = statement.query(params![key, face, key, key, face, photo, face])?;
    let mut scores: BTreeMap<i64, (String, Vec<f32>)> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let vector = decode(&row.get::<_, Vec<u8>>(1)?)?;
        if let Some(score) = inference::cosine(&query, &vector) {
            let entry = scores
                .entry(row.get(0)?)
                .or_insert_with(|| (row.get(2).unwrap_or_default(), Vec::new()));
            entry.1.push(score);
            entry.1.sort_by(|a, b| b.total_cmp(a));
            entry.1.truncate(3);
        }
    }
    let mut result: Vec<_> = scores
        .into_iter()
        .map(|(id, (name, scores))| (id, name, scores.iter().sum::<f32>() / scores.len() as f32))
        .collect();
    result.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
    Ok(result)
}
/// Namensvorschläge für ein Gesicht. `by_model` ist wahr, wenn der trainierte
/// Klassifikator die Reihenfolge bestimmt (Score = Wahrscheinlichkeit);
/// sonst gilt der Ähnlichkeitsvergleich (Score = Kosinus).
#[derive(Clone, Default)]
pub struct Suggestions {
    pub by_model: bool,
    pub items: Vec<(i64, String, f32)>,
}
pub fn suggestions(collection: &Collection, face: i64, key: &str) -> Result<Suggestions> {
    let mut all = scored(collection, face, key)?;
    all.items.truncate(if all.by_model { 3 } else { 6 });
    Ok(all)
}
/// Gemeinsamer Vorschlag für eine Gruppe: Mittel der Einzelwerte über
/// höchstens drei Gesichter (begrenzt die Lesezeit je Seite).
pub fn group_suggestions(collection: &Collection, faces: &[i64], key: &str) -> Result<Suggestions> {
    let mut totals: BTreeMap<i64, (String, f32)> = BTreeMap::new();
    let (mut used, mut by_model) = (0usize, true);
    for face in faces.iter().take(3) {
        let one = scored(collection, *face, key)?;
        if one.items.is_empty() {
            continue;
        }
        used += 1;
        by_model &= one.by_model;
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
    Ok(Suggestions { by_model, items })
}
fn scored(collection: &Collection, face: i64, key: &str) -> Result<Suggestions> {
    let result = ranked(collection, face, key)?;
    if let Some(model) = classifier::load(collection, key)? {
        let bytes: Option<Vec<u8>> = collection
            .db
            .query_row(
                "SELECT vec FROM face_embeddings WHERE face_id=? AND embedder_id=?",
                params![face, key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(query) = bytes.and_then(|b| inference::normalize(&decode(&b).ok()?).ok()) {
            // Dieselben Ausschlüsse wie beim Ähnlichkeitsvergleich bleiben maßgeblich.
            let allowed: BTreeSet<i64> = result.iter().map(|r| r.0).collect();
            let names: BTreeMap<i64, &str> = result.iter().map(|r| (r.0, r.1.as_str())).collect();
            let items: Vec<_> = model
                .predict_among(&query, &allowed)
                .into_iter()
                .map(|(id, p)| (id, names[&id].to_string(), p))
                .collect();
            if !items.is_empty() {
                return Ok(Suggestions {
                    by_model: true,
                    items,
                });
            }
        }
    }
    Ok(Suggestions {
        by_model: false,
        items: result,
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
        // Ein fehlgeschlagenes Training darf Abgleich und Gruppen nicht blockieren.
        if let Err(error) = classifier::train(collection, &key, control)
            && control.cancel.load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err(error);
        }
    }
    rematch(collection, &key, control)?;
    control.set_face_phase(super::jobs::FacePhase::Groups);
    while faces::prepare_groups(collection, &key, control)? > 0 {}
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
    let (auto, suggest) = (
        option(collection, "rust.auto", 0.6)?,
        option(collection, "rust.suggest", 0.363)?,
    );
    let births: BTreeMap<i64, i32> = collection
        .db
        .prepare("SELECT person_id,value FROM person_attrs WHERE key='birth_year'")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok().and_then(|(id, year)| Some((id, year.parse().ok()?))))
        .collect();
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
            let occupied:BTreeSet<i64>=collection.db.prepare("SELECT DISTINCT person_id FROM faces WHERE photo_id=? AND assignment='user' AND person_id IS NOT NULL")?.query_map([photo],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
            let mut priors: BTreeMap<i64, f32> = BTreeMap::new();
            for known in &occupied {
                let rows=collection.db.prepare("SELECT b.person_id,count(DISTINCT a.photo_id) FROM faces a JOIN faces b ON b.photo_id=a.photo_id WHERE a.person_id=? AND a.assignment='user' AND b.assignment='user' AND a.ignored=0 AND b.ignored=0 AND b.person_id!=? GROUP BY b.person_id")?.query_map(params![known,known],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                for (person, count) in rows {
                    let value = (count as f32 * 0.005).min(0.03);
                    priors
                        .entry(person)
                        .and_modify(|v| *v = v.max(value))
                        .or_insert(value);
                }
            }
            let mut rankings = Vec::new();
            let mut people = BTreeSet::new();
            for face in &candidates {
                control.checkpoint()?;
                let ranking = ranked(collection, face.id, key)?;
                for (person, _, _) in &ranking {
                    people.insert(*person);
                }
                rankings.push(ranking);
            }
            let people: Vec<_> = people.into_iter().collect();
            let mut matrix =
                vec![vec![suggest - 0.000001; people.len() + candidates.len()]; candidates.len()];
            for (row, (face, ranking)) in candidates.iter().zip(&rankings).enumerate() {
                let (quality, captured): (f32, Option<String>) = collection.db.query_row(
                    "SELECT quality,captured_at FROM faces WHERE id=?",
                    [face.id],
                    |r| Ok((r.get::<_, Option<f32>>(0)?.unwrap_or_default(), r.get(1)?)),
                )?;
                let year = captured
                    .as_deref()
                    .and_then(|v| v.get(..4))
                    .and_then(|v| v.parse::<i32>().ok());
                for (col, person) in people.iter().enumerate() {
                    let mut score = ranking
                        .iter()
                        .find(|r| r.0 == *person)
                        .map(|r| r.2)
                        .unwrap_or(-2.0);
                    if score >= suggest - 0.03 && score < auto {
                        score = (score + priors.get(person).copied().unwrap_or_default())
                            .min(auto - 0.000001);
                    }
                    if quality < 0.5
                        || occupied.contains(person)
                        || year.zip(births.get(person)).is_some_and(|(y, b)| y < *b)
                    {
                        score = -2.0;
                    }
                    matrix[row][col] = score;
                }
            }
            let decisions = matching::assignment(&matrix)?;
            let tx = collection.db.transaction()?;
            tx.execute("UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE photo_id=? AND assignment IN ('auto','suggested')",[photo])?;
            for (row, col) in decisions.into_iter().enumerate() {
                if col < people.len() && matrix[row][col] >= suggest {
                    let score = matrix[row][col];
                    tx.execute("UPDATE faces SET person_id=?,assignment=?,confidence=?,assigned_at=datetime('now') WHERE id=? AND assignment IS NOT 'user'",params![people[col],if score>=auto{"auto"}else{"suggested"},score,candidates[row].id])?;
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
