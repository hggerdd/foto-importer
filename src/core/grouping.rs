//! Kleine unbekannte Gesichtsgruppen zusammenführen (ADR 0053).
//!
//! `faces::prepare_groups` bildet strenge Gruppen (Complete-Link ab Kosinus 0,6).
//! Dieselbe Person zerfällt dadurch bei Pose-, Licht- oder Altersunterschieden in
//! viele Gruppen mit ein oder zwei Bildern. Dieser Nachlauf hängt solche Gruppen
//! an die ähnlichste größere Gruppe, sofern die mittlere Ähnlichkeit hoch genug
//! ist. Er verändert nur abgeleitete Gruppen, keine Namen oder Gesichter.
use super::{
    collection::Collection,
    faces::{self, decode},
    inference,
    jobs::Control,
};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

pub const SMALL_GROUP: usize = 2;
const DEFAULT_MEAN: f32 = 0.5;
const MIN_PAIR: f32 = 0.3;
const CENTROID_CANDIDATES: usize = 8;

struct Group {
    members: Vec<(i64, i64, Vec<f32>)>,
    centroid: Vec<f32>,
}

fn centroid(members: &[(i64, i64, Vec<f32>)]) -> Vec<f32> {
    let dim = members.first().map_or(0, |m| m.2.len());
    let mut sum = vec![0.0f32; dim];
    for (_, _, v) in members {
        for (s, x) in sum.iter_mut().zip(v) {
            *s += x;
        }
    }
    inference::normalize(&sum).unwrap_or(sum)
}

/// Mittlere Ähnlichkeit, falls kein Foto doppelt vorkommt und kein Paar zu schwach ist.
fn link(a: &Group, b: &Group) -> Option<f32> {
    let photos: BTreeSet<i64> = a.members.iter().map(|m| m.1).collect();
    if b.members.iter().any(|m| photos.contains(&m.1)) {
        return None;
    }
    let (mut sum, mut count) = (0.0f32, 0usize);
    for (_, _, x) in &a.members {
        for (_, _, y) in &b.members {
            let score = inference::cosine(x, y)?;
            if score < MIN_PAIR {
                return None;
            }
            sum += score;
            count += 1;
        }
    }
    (count > 0).then(|| sum / count as f32)
}

fn threshold(collection: &Collection, key: &str) -> f32 {
    let name = format!("rust.thresholds.{key}.group_merge");
    collection
        .db
        .query_row(
            "SELECT value FROM face_settings WHERE key=?",
            [&name],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MEAN)
}

/// Gibt die Zahl der verschwundenen kleinen Gruppen zurück.
pub fn merge_small_groups(
    collection: &mut Collection,
    key: &str,
    control: &Control,
) -> Result<usize> {
    collection.writable()?;
    let minimum = threshold(collection, key);
    let mut groups: BTreeMap<i64, Group> = BTreeMap::new();
    {
        let mut statement = collection.db.prepare(
            "SELECT g.group_id,f.id,f.photo_id,e.vec FROM face_group_members g
             JOIN faces f ON f.id=g.face_id
             JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=g.embedder_id
             WHERE g.embedder_id=? AND f.person_id IS NULL AND f.ignored=0 AND f.retained=0
             ORDER BY g.group_id,f.id",
        )?;
        let mut rows = statement.query([key])?;
        while let Some(row) = rows.next()? {
            let vector = inference::normalize(&decode(&row.get::<_, Vec<u8>>(3)?)?)?;
            groups
                .entry(row.get(0)?)
                .or_insert_with(|| Group {
                    members: Vec::new(),
                    centroid: Vec::new(),
                })
                .members
                .push((row.get(1)?, row.get(2)?, vector));
        }
    }
    for group in groups.values_mut() {
        group.centroid = centroid(&group.members);
    }
    let small: Vec<i64> = groups
        .iter()
        .filter(|(_, g)| g.members.len() <= SMALL_GROUP)
        .map(|(id, _)| *id)
        .collect();
    let mut merged = 0;
    for source_id in small {
        control.checkpoint()?;
        let Some(source) = groups.get(&source_id) else {
            continue;
        };
        let mut nearest: Vec<(f32, i64)> = groups
            .iter()
            .filter(|(id, g)| {
                **id != source_id
                    && g.members.len() + source.members.len() <= crate::config::GROUP_MAX
            })
            .filter_map(|(id, g)| Some((inference::cosine(&source.centroid, &g.centroid)?, *id)))
            .collect();
        nearest.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        nearest.truncate(CENTROID_CANDIDATES);
        let best = nearest
            .iter()
            .filter_map(|(_, id)| Some((link(source, &groups[id])?, *id)))
            .filter(|(score, _)| *score >= minimum)
            .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
        let Some((_, target_id)) = best else {
            continue;
        };
        let tx = collection.db.transaction()?;
        tx.execute(
            "UPDATE face_group_members SET group_id=? WHERE group_id=? AND embedder_id=?",
            params![target_id, source_id, key],
        )?;
        tx.execute(
            "UPDATE face_group_catalog SET members=(SELECT count(*) FROM face_group_members WHERE group_id=?) WHERE group_id=?",
            params![target_id, target_id],
        )?;
        tx.execute(
            "DELETE FROM face_group_catalog WHERE group_id=?",
            [source_id],
        )?;
        tx.commit()?;
        let moved = groups
            .remove(&source_id)
            .map(|g| g.members)
            .unwrap_or_default();
        if let Some(target) = groups.get_mut(&target_id) {
            target.members.extend(moved);
            target.centroid = centroid(&target.members);
        }
        merged += 1;
    }
    Ok(merged)
}

/// Gruppen bilden und anschließend kleine Gruppen zusammenführen.
pub fn rebuild_groups(collection: &mut Collection, key: &str, control: &Control) -> Result<usize> {
    while faces::prepare_groups(collection, key, control)? > 0 {}
    merge_small_groups(collection, key, control)
}
