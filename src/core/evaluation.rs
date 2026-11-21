//! Rein lesendes Messwerkzeug für Erkennungsschwellen: wie oft erreicht die eigene
//! Person eine Schwelle und wie oft eine andere. Die Rechnung folgt
//! `recognition::ranked_with` (Mittel der bis zu drei besten Kosinuswerte je Person),
//! zählt aber nie das abgefragte Gesicht als eigene Referenz.
use super::{
    collection::Collection,
    faces::{decode, setting},
    inference::cosine,
    recognition::top3_means,
};
use anyhow::{Result, bail};
use std::collections::{BTreeMap, BTreeSet};

/// (Gesichts-ID, Personen-ID, normalisierter Vektor)
pub type Entry = (i64, i64, Vec<f32>);

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub threshold: f32,
    /// Anteil Abfragen mit `own >= t`.
    pub own_reaches: f32,
    /// Anteil Abfragen mit `imp >= t`.
    pub other_reaches: f32,
    /// Anteil mit `own >= t` und `own - imp >= margin`.
    pub own_accepted: f32,
    /// Anteil mit `imp >= t` und `imp - zweitbester_anderer >= margin`.
    pub other_accepted: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// Ausgewertete Abfragen.
    pub queries: usize,
    pub references: usize,
    pub persons: usize,
    /// Abfragen ohne eigene Referenz (nach Ausschluss) oder ohne andere Person.
    pub skipped: usize,
    pub rows: Vec<Row>,
}

/// (own, imp, zweitbester anderer)
type Outcome = (f32, f32, Option<f32>);

/// Personenscores eines Vektors; `skip` ist das abgefragte Gesicht.
fn scores(query: &[f32], skip: i64, references: &[Entry]) -> BTreeMap<i64, f32> {
    top3_means(
        references
            .iter()
            .filter(|(id, _, _)| *id != skip)
            .filter_map(|(_, person, vector)| cosine(query, vector).map(|s| (*person, s))),
    )
}

pub fn evaluate(
    references: &[Entry],
    queries: &[Entry],
    thresholds: &[f32],
    margin: f32,
) -> Report {
    // (own, imp, zweitbester anderer)
    let mut results: Vec<Outcome> = Vec::new();
    let mut skipped = 0;
    for (face, person, vector) in queries {
        let all = scores(vector, *face, references);
        let Some(own) = all.get(person).copied() else {
            skipped += 1;
            continue;
        };
        let mut others: Vec<f32> = all
            .iter()
            .filter(|(p, _)| **p != *person)
            .map(|(_, s)| *s)
            .collect();
        others.sort_by(|a, b| b.total_cmp(a));
        let Some(imp) = others.first().copied() else {
            skipped += 1;
            continue;
        };
        results.push((own, imp, others.get(1).copied()));
    }
    let total = results.len();
    let share = |keep: &dyn Fn(&Outcome) -> bool| {
        if total == 0 {
            0.0
        } else {
            results.iter().filter(|r| keep(r)).count() as f32 / total as f32
        }
    };
    let rows = thresholds
        .iter()
        .map(|&t| Row {
            threshold: t,
            own_reaches: share(&|r| r.0 >= t),
            other_reaches: share(&|r| r.1 >= t),
            own_accepted: share(&|r| r.0 >= t && r.0 - r.1 >= margin),
            other_accepted: share(&|r| r.1 >= t && r.2.is_none_or(|second| r.1 - second >= margin)),
        })
        .collect();
    let persons = references
        .iter()
        .map(|r| r.1)
        .collect::<BTreeSet<_>>()
        .len();
    Report {
        queries: total,
        references: references.len(),
        persons,
        skipped,
        rows,
    }
}

type Raw = (i64, i64, Vec<u8>);

/// Liest Referenzen und Abfragen für den eingestellten Embedder (nur lesend).
pub fn load(collection: &Collection) -> Result<(Vec<Entry>, Vec<Entry>)> {
    let Some(key) = setting(collection, super::faces::EMBEDDER_KEY)? else {
        bail!("Kein Embedder in der Sammlung eingestellt (rust.embedder_key)");
    };
    let rows = |sql: &str| -> Result<Vec<Entry>> {
        collection
            .db
            .prepare(sql)?
            .query_map([&key], |r| Ok::<Raw, _>((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|(f, p, v)| Ok((f, p, decode(&v)?)))
            .collect()
    };
    let references = rows(
        "SELECT t.face_id,t.person_id,e.vec FROM training_set t
         JOIN face_embeddings e ON e.face_id=t.face_id AND e.embedder_id=t.embedder_id
         WHERE t.embedder_id=?1 AND t.split='ref' ORDER BY t.person_id,t.face_id",
    )?;
    let with_references: BTreeSet<i64> = references.iter().map(|r| r.1).collect();
    let queries = rows(
        "SELECT f.id,f.person_id,e.vec FROM faces f
         JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=?1
         WHERE f.assignment='user' AND f.ignored=0 AND f.retained=0 AND f.quality>=0.5
           AND f.person_id IS NOT NULL ORDER BY f.id",
    )?
    .into_iter()
    .filter(|q| with_references.contains(&q.1))
    .collect();
    Ok((references, queries))
}
