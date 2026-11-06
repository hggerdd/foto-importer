//! Trainierter Namensklassifikator über den Gesichts-Embeddings (ADR 0051).
//!
//! Multinomiale logistische Regression (Softmax) auf L2-normalisierten
//! Vektoren. Trainiert wird ausschließlich auf den Referenzen (`split='ref'`);
//! der zurückgehaltene Split (`split='val'`) dient nur der Messung.
use super::{
    collection::Collection,
    faces::{self, decode},
    inference,
    jobs::Control,
};
use anyhow::{Result, bail};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const EPOCHS: usize = 250;
const LEARNING_RATE: f32 = 0.05;
const REGULARISATION: f32 = 1e-3;
const MIN_PEOPLE: usize = 2;
const MIN_SAMPLES_PER_PERSON: usize = 2;
const MIN_VALIDATION: usize = 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metrics {
    pub people: usize,
    pub trained_on: usize,
    pub validated_on: usize,
    /// Anteil richtiger Personen auf Platz 1 / unter den ersten drei (Klassifikator).
    pub top1: f32,
    pub top3: f32,
    /// Dieselben Werte für den bisherigen Ähnlichkeitsvergleich (Top-3-Mittel).
    pub baseline_top1: f32,
    pub baseline_top3: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub dim: usize,
    pub people: Vec<i64>,
    /// Zeilenweise `people.len() × dim`.
    pub weights: Vec<f32>,
    pub bias: Vec<f32>,
    pub metrics: Option<Metrics>,
}

fn setting_key(embedder: &str) -> String {
    format!("rust.classifier:{embedder}")
}

impl Model {
    fn logits(&self, x: &[f32], rows: &[usize]) -> Vec<f32> {
        rows.iter()
            .map(|&c| {
                self.bias[c]
                    + self.weights[c * self.dim..(c + 1) * self.dim]
                        .iter()
                        .zip(x)
                        .map(|(w, v)| w * v)
                        .sum::<f32>()
            })
            .collect()
    }
    /// Wahrscheinlichkeiten, normiert über die erlaubten Personen. Personen,
    /// die das Modell nicht kennt, erscheinen nicht.
    pub fn predict_among(&self, x: &[f32], allowed: &BTreeSet<i64>) -> Vec<(i64, f32)> {
        if x.len() != self.dim {
            return Vec::new();
        }
        let rows: Vec<usize> = (0..self.people.len())
            .filter(|&c| allowed.contains(&self.people[c]))
            .collect();
        if rows.is_empty() {
            return Vec::new();
        }
        let probabilities = softmax(&self.logits(x, &rows));
        let mut result: Vec<_> = rows
            .iter()
            .zip(probabilities)
            .map(|(&c, p)| (self.people[c], p))
            .collect();
        result.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        result
    }
}

fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exp: Vec<f32> = logits.iter().map(|v| (v - max).exp()).collect();
    let sum: f32 = exp.iter().sum();
    exp.into_iter().map(|v| v / sum).collect()
}

/// Trainiert deterministisch (Nullstart, Vollbatch-Adam). Eingaben müssen
/// normalisiert sein. Personen mit zu wenigen Beispielen werden ausgelassen.
pub fn fit(samples: &[(i64, Vec<f32>)], control: &Control) -> Result<Option<Model>> {
    let dim = samples.first().map_or(0, |s| s.1.len());
    if dim == 0 || samples.iter().any(|s| s.1.len() != dim) {
        bail!("Uneinheitliche Vektorlängen");
    }
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for (person, _) in samples {
        *counts.entry(*person).or_default() += 1;
    }
    let people: Vec<i64> = counts
        .iter()
        .filter(|(_, n)| **n >= MIN_SAMPLES_PER_PERSON)
        .map(|(p, _)| *p)
        .collect();
    if people.len() < MIN_PEOPLE {
        return Ok(None);
    }
    let class: BTreeMap<i64, usize> = people.iter().enumerate().map(|(i, p)| (*p, i)).collect();
    let used: Vec<(usize, &Vec<f32>)> = samples
        .iter()
        .filter_map(|(p, v)| class.get(p).map(|c| (*c, v)))
        .collect();
    // Gleiches Gesamtgewicht je Person: seltene Personen werden nicht verdrängt.
    let per_class: Vec<f32> = people.iter().map(|p| counts[p] as f32).collect();
    let classes = people.len();
    let mut model = Model {
        dim,
        people,
        weights: vec![0.0; classes * dim],
        bias: vec![0.0; classes],
        metrics: None,
    };
    let (mut m_w, mut v_w) = (vec![0.0f32; classes * dim], vec![0.0f32; classes * dim]);
    let (mut m_b, mut v_b) = (vec![0.0f32; classes], vec![0.0f32; classes]);
    let all_rows: Vec<usize> = (0..classes).collect();
    let weight_sum: f32 = used.iter().map(|(c, _)| 1.0 / per_class[*c]).sum();
    for epoch in 1..=EPOCHS {
        if epoch % 10 == 1 {
            control.checkpoint()?;
        }
        let mut grad_w = vec![0.0f32; classes * dim];
        let mut grad_b = vec![0.0f32; classes];
        for (target, x) in &used {
            let weight = (1.0 / per_class[*target]) / weight_sum;
            let mut p = softmax(&model.logits(x, &all_rows));
            p[*target] -= 1.0;
            for (c, delta) in p.iter().enumerate() {
                let g = delta * weight;
                grad_b[c] += g;
                for (slot, v) in grad_w[c * dim..(c + 1) * dim].iter_mut().zip(*x) {
                    *slot += g * v;
                }
            }
        }
        let (b1, b2, eps) = (0.9f32, 0.999f32, 1e-8f32);
        let (c1, c2) = (1.0 - b1.powi(epoch as i32), 1.0 - b2.powi(epoch as i32));
        for i in 0..grad_w.len() {
            let g = grad_w[i] + REGULARISATION * model.weights[i];
            m_w[i] = b1 * m_w[i] + (1.0 - b1) * g;
            v_w[i] = b2 * v_w[i] + (1.0 - b2) * g * g;
            model.weights[i] -= LEARNING_RATE * (m_w[i] / c1) / ((v_w[i] / c2).sqrt() + eps);
        }
        for i in 0..classes {
            let g = grad_b[i];
            m_b[i] = b1 * m_b[i] + (1.0 - b1) * g;
            v_b[i] = b2 * v_b[i] + (1.0 - b2) * g * g;
            model.bias[i] -= LEARNING_RATE * (m_b[i] / c1) / ((v_b[i] / c2).sqrt() + eps);
        }
    }
    Ok(Some(model))
}

fn rank_of(scores: &[(i64, f32)], truth: i64) -> Option<usize> {
    let mut sorted = scores.to_vec();
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    sorted.iter().position(|s| s.0 == truth)
}

/// Misst Klassifikator und Ähnlichkeitsvergleich auf demselben zurückgehaltenen Split.
pub fn evaluate(
    model: &Model,
    references: &[(i64, Vec<f32>)],
    validation: &[(i64, Vec<f32>)],
) -> Option<Metrics> {
    let known: BTreeSet<i64> = model.people.iter().copied().collect();
    let validation: Vec<_> = validation
        .iter()
        .filter(|(p, _)| known.contains(p))
        .collect();
    if validation.len() < MIN_VALIDATION {
        return None;
    }
    let mut by_person: BTreeMap<i64, Vec<&Vec<f32>>> = BTreeMap::new();
    for (person, vector) in references {
        if known.contains(person) {
            by_person.entry(*person).or_default().push(vector);
        }
    }
    let (mut top1, mut top3, mut base1, mut base3) = (0usize, 0usize, 0usize, 0usize);
    for (truth, x) in &validation {
        let ranked = model.predict_among(x, &known);
        if let Some(rank) = rank_of(&ranked, *truth) {
            top1 += usize::from(rank < 1);
            top3 += usize::from(rank < 3);
        }
        let baseline: Vec<(i64, f32)> = by_person
            .iter()
            .filter_map(|(person, vectors)| {
                let mut scores: Vec<f32> = vectors
                    .iter()
                    .filter_map(|v| inference::cosine(x, v))
                    .collect();
                scores.sort_by(|a, b| b.total_cmp(a));
                scores.truncate(3);
                (!scores.is_empty())
                    .then(|| (*person, scores.iter().sum::<f32>() / scores.len() as f32))
            })
            .collect();
        if let Some(rank) = rank_of(&baseline, *truth) {
            base1 += usize::from(rank < 1);
            base3 += usize::from(rank < 3);
        }
    }
    let n = validation.len() as f32;
    Some(Metrics {
        people: model.people.len(),
        trained_on: references.iter().filter(|(p, _)| known.contains(p)).count(),
        validated_on: validation.len(),
        top1: top1 as f32 / n,
        top3: top3 as f32 / n,
        baseline_top1: base1 as f32 / n,
        baseline_top3: base3 as f32 / n,
    })
}

fn split_samples(
    collection: &Collection,
    embedder: &str,
    split: &str,
) -> Result<Vec<(i64, Vec<f32>)>> {
    collection
        .db
        .prepare(
            "SELECT t.person_id,e.vec FROM training_set t
             JOIN face_embeddings e ON e.face_id=t.face_id AND e.embedder_id=t.embedder_id
             JOIN faces f ON f.id=t.face_id
             WHERE t.embedder_id=? AND t.split=? AND f.assignment='user' AND f.ignored=0
               AND f.retained=0 ORDER BY t.person_id,t.face_id",
        )?
        .query_map(params![embedder, split], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .map(|row| {
            let (person, bytes) = row?;
            Ok((person, inference::normalize(&decode(&bytes)?)?))
        })
        .collect()
}

/// Trainiert aus den aktuellen Referenzen und speichert das Ergebnis.
/// Ohne ausreichend Daten wird ein veraltetes Modell entfernt.
pub fn train(
    collection: &mut Collection,
    embedder: &str,
    control: &Control,
) -> Result<Option<Metrics>> {
    collection.writable()?;
    let references = split_samples(collection, embedder, "ref")?;
    let validation = split_samples(collection, embedder, "val")?;
    let model = fit(&references, control)?;
    let key = setting_key(embedder);
    let Some(mut model) = model else {
        collection
            .db
            .execute("DELETE FROM face_settings WHERE key=?", [&key])?;
        return Ok(None);
    };
    model.metrics = evaluate(&model, &references, &validation);
    faces::save_setting(&collection.db, &key, &serde_json::to_string(&model)?)?;
    Ok(model.metrics.clone())
}

pub fn load(collection: &Collection, embedder: &str) -> Result<Option<Model>> {
    Ok(faces::setting(collection, &setting_key(embedder))?
        .and_then(|raw| serde_json::from_str::<Model>(&raw).ok()))
}
