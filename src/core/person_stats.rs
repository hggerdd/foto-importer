//! Personenstatistik (ADR Personen, „Statistik“): je Person die Verteilung der Personenscores
//! des Modells als Violine. Nur Anzeige: nichts wird übernommen, keine Zuordnung geändert.
//!
//! Zwei Sichten: „Alle bewerteten Gesichter“ (offene Gesichter, deren beste zulässige Person X
//! ist) und „Nur bestätigte“ (bestätigte Gesichter gegen Referenzen ohne denselben
//! Aufnahmetag, dazu die abgelehnten „nicht X“ als Gegenbeispiele). Die Werte rechnet
//! `recognition::person_scores`; hier entstehen Kennzahlen, Kerndichte und die Schwelle t.

use super::collection::Collection;
use super::faces;
use super::jobs::Control;
use super::recognition::{self, Limits, PersonScores};
use super::revision::{self, Domain};
use crate::config;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Sicht der Statistik.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Alle bewerteten Gesichter: offene Gesichter, deren beste zulässige Person X ist.
    #[default]
    Rated,
    /// Nur bestätigte, dazu die Ablehnungen als zweite Hälfte.
    Confirmed,
}

/// Fünf-Punkte-Zusammenfassung einer Verteilung.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub min: f32,
    pub q1: f32,
    pub median: f32,
    pub q3: f32,
    pub max: f32,
}

/// Verteilung einer Wertemenge, fertig zum Zeichnen.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    pub n: usize,
    pub summary: Option<Summary>,
    /// Relative Kerndichte an den Stützstellen der Achse ([`grid`]), höchster Wert 1, außerhalb
    /// von Minimum und Maximum 0. Leer unter `config::PERSON_STATS_MIN_VIOLIN` Werten.
    pub density: Vec<f32>,
    /// Die Werte selbst, wenn es zu wenige für eine Violine sind.
    pub points: Vec<f32>,
}

/// Die Schwelle t der Sicht „Nur bestätigte“: der höchste Wert eines abgelehnten Gesichts.
/// Darüber liegt keine Ablehnung mehr.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Threshold {
    pub t: f32,
    /// Bestätigte Gesichter über t.
    pub confirmed_above: usize,
    /// Offene, als X bewertete Gesichter über t (nur zur Information).
    pub rated_above: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PersonStat {
    pub person: i64,
    pub name: String,
    pub rated: Distribution,
    pub confirmed: Distribution,
    pub rejected: Distribution,
    /// `None` ohne Ablehnungen.
    pub threshold: Option<Threshold>,
}

impl PersonStat {
    /// Zahl der Gesichter in der Sicht (Sortierung und Auswahl der Spalten).
    pub fn count(&self, mode: Mode) -> usize {
        match mode {
            Mode::Rated => self.rated.n,
            Mode::Confirmed => self.confirmed.n + self.rejected.n,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub people: Vec<PersonStat>,
    /// Schwellen der Sammlung: Vorschlag und „sicher“ (`recognition::Limits`).
    pub review: f32,
    pub sure: f32,
    /// Werteachse und Untergrenze der Werte des Modells (`config::ModelScale::stats_axis`,
    /// `candidate_min`); ältere Cache-Einträge ohne sie gelten als SFace.
    #[serde(default = "sface_axis")]
    pub axis: (f32, f32),
    #[serde(default = "sface_minimum")]
    pub minimum: f32,
}
fn sface_axis() -> (f32, f32) {
    config::SFACE_SCALE.stats_axis
}
fn sface_minimum() -> f32 {
    config::SFACE_SCALE.candidate_min
}

impl Stats {
    /// Personen mit Werten in der Sicht, nach Zahl der Gesichter absteigend, dann Name und ID.
    pub fn order(&self, mode: Mode) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.people.len())
            .filter(|i| self.people[*i].count(mode) > 0)
            .collect();
        order.sort_by(|a, b| {
            let (x, y) = (&self.people[*a], &self.people[*b]);
            y.count(mode)
                .cmp(&x.count(mode))
                .then(x.name.cmp(&y.name))
                .then(x.person.cmp(&y.person))
        });
        order
    }
}

/// Stützstellen der Achse (`config::PERSON_STATS_AXIS`, `PERSON_STATS_GRID` Punkte).
pub fn grid() -> Vec<f32> {
    grid_on(config::PERSON_STATS_AXIS)
}
/// Stützstellen der Achse `(low, high)` des Modells.
pub fn grid_on((low, high): (f32, f32)) -> Vec<f32> {
    let steps = config::PERSON_STATS_GRID.max(2) - 1;
    (0..=steps)
        .map(|i| low + (high - low) * i as f32 / steps as f32)
        .collect()
}

/// Quantil `p` (0 bis 1) einer aufsteigend sortierten Liste, linear interpoliert.
pub fn quantile(sorted: &[f32], p: f32) -> f32 {
    match sorted.len() {
        0 => f32::NAN,
        1 => sorted[0],
        n => {
            let at = p.clamp(0.0, 1.0) * (n - 1) as f32;
            let low = at.floor() as usize;
            let high = (low + 1).min(n - 1);
            sorted[low] + (sorted[high] - sorted[low]) * (at - low as f32)
        }
    }
}

/// Bandbreite nach Silverman: `0,9 · min(s, IQR/1,34) · n^(−1/5)`. Ist der Quartilsabstand 0,
/// zählt die Standardabweichung; sind alle Werte gleich, eine kleine Mindestbreite.
pub fn silverman(values: &[f32]) -> f32 {
    const FLOOR: f32 = 0.005;
    let n = values.len();
    if n < 2 {
        return FLOOR;
    }
    let mean = values.iter().map(|v| *v as f64).sum::<f64>() / n as f64;
    let variance = values
        .iter()
        .map(|v| (*v as f64 - mean).powi(2))
        .sum::<f64>()
        / (n - 1) as f64;
    let sd = variance.sqrt() as f32;
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let iqr = quantile(&sorted, 0.75) - quantile(&sorted, 0.25);
    let spread = if iqr > 0.0 { sd.min(iqr / 1.34) } else { sd };
    (0.9 * spread * (n as f32).powf(-0.2)).max(FLOOR)
}

/// Gaußsche Kerndichte der Werte an den Stellen `at` (Fläche 1 über die ganze Zahlengerade).
pub fn kde(values: &[f32], bandwidth: f32, at: &[f32]) -> Vec<f32> {
    if values.is_empty() || bandwidth <= 0.0 {
        return vec![0.0; at.len()];
    }
    let norm = 1.0 / (values.len() as f64 * bandwidth as f64 * (2.0 * std::f64::consts::PI).sqrt());
    at.iter()
        .map(|x| {
            let sum: f64 = values
                .iter()
                .map(|v| {
                    let u = (*x as f64 - *v as f64) / bandwidth as f64;
                    if u.abs() > 6.0 {
                        0.0
                    } else {
                        (-0.5 * u * u).exp()
                    }
                })
                .sum();
            (sum * norm) as f32
        })
        .collect()
}

/// Verteilung der Werte auf der SFace-Achse.
pub fn distribution(values: &[f32]) -> Distribution {
    distribution_on(values, config::PERSON_STATS_AXIS)
}
/// Verteilung der Werte: Kennzahlen, relative Dichte an den Stützstellen der Achse `axis` (auf
/// die Spanne der Werte beschnitten) oder bei wenigen Werten die Punkte.
pub fn distribution_on(values: &[f32], axis: (f32, f32)) -> Distribution {
    let mut sorted: Vec<f32> = values.iter().copied().filter(|v| v.is_finite()).collect();
    sorted.sort_by(f32::total_cmp);
    let n = sorted.len();
    if n == 0 {
        return Distribution::default();
    }
    let summary = Summary {
        min: sorted[0],
        q1: quantile(&sorted, 0.25),
        median: quantile(&sorted, 0.5),
        q3: quantile(&sorted, 0.75),
        max: sorted[n - 1],
    };
    if n < config::PERSON_STATS_MIN_VIOLIN {
        return Distribution {
            n,
            summary: Some(summary),
            density: Vec::new(),
            points: sorted,
        };
    }
    let at = grid_on(axis);
    let mut density = kde(&sorted, silverman(&sorted), &at);
    for (value, x) in density.iter_mut().zip(&at) {
        if *x < summary.min || *x > summary.max {
            *value = 0.0;
        }
    }
    let peak = density.iter().copied().fold(0.0f32, f32::max);
    if peak > 0.0 {
        density.iter_mut().for_each(|v| *v /= peak);
    }
    Distribution {
        n,
        summary: Some(summary),
        density,
        points: Vec::new(),
    }
}

/// Schwelle t: der höchste abgelehnte Wert; dazu die bestätigten und bewerteten Werte darüber.
/// `None` ohne Ablehnungen.
pub fn threshold(confirmed: &[f32], rejected: &[f32], rated: &[f32]) -> Option<Threshold> {
    let t = rejected
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .max_by(f32::total_cmp)?;
    Some(Threshold {
        t,
        confirmed_above: confirmed.iter().filter(|v| **v > t).count(),
        rated_above: rated.iter().filter(|v| **v > t).count(),
    })
}

/// Statistik aus den Personenscores (SFace-Achse).
pub fn build(scores: Vec<PersonScores>, limits: Limits) -> Stats {
    build_for(scores, limits, &config::SFACE_SCALE)
}
/// Statistik aus den Personenscores auf der Achse des Modells `scale`.
pub fn build_for(scores: Vec<PersonScores>, limits: Limits, scale: &config::ModelScale) -> Stats {
    let axis = scale.stats_axis;
    let mut people: Vec<PersonStat> = scores
        .into_iter()
        .map(|s| PersonStat {
            person: s.person,
            threshold: threshold(&s.confirmed, &s.rejected, &s.rated),
            rated: distribution_on(&s.rated, axis),
            confirmed: distribution_on(&s.confirmed, axis),
            rejected: distribution_on(&s.rejected, axis),
            name: s.name,
        })
        .filter(|p| p.rated.n + p.confirmed.n + p.rejected.n > 0)
        .collect();
    people.sort_by_key(|p| p.person);
    Stats {
        people,
        review: limits.review,
        sure: limits.sure,
        axis,
        minimum: scale.candidate_min,
    }
}

/// Berechnet die Statistik der Sammlung (Modell aus den Einstellungen; ohne Modell leer).
pub fn compute(collection: &Collection, control: &Control) -> Result<Stats> {
    let limits = Limits::load(collection)?;
    let Some(key) = faces::setting(collection, faces::EMBEDDER_KEY)? else {
        return Ok(build(Vec::new(), limits));
    };
    let scale = config::model_scale(&key);
    let scores = recognition::person_scores(collection, &key, scale.candidate_min, control)?;
    Ok(build_for(scores, limits, scale))
}

/// Bereiche, aus denen die Statistik entsteht: Zuordnungen, Ablehnungen, Referenzen und
/// Einstellungen (Faces) sowie Personen (Namen, Geburtsjahre).
const DOMAINS: &[Domain] = &[Domain::Faces, Domain::People];

/// Fingerabdruck der Daten: Datenrevision ([`DOMAINS`]), Modell und der Stand der Merkmale
/// (neue Gesichter ohne Person erhöhen keine Revision, verändern aber „alle bewerteten“).
/// Ohne Revision (Schema 6, nur lesend) ist er jedes Mal neu.
pub fn signature(collection: &Collection) -> Result<String> {
    let Some(key) = revision::key(&collection.db, DOMAINS)? else {
        return Ok(format!("ohne-revision-{}", uuid::Uuid::new_v4()));
    };
    let model = faces::setting(collection, faces::EMBEDDER_KEY)?.unwrap_or_default();
    let minimum = config::model_scale(&model).candidate_min;
    let (vectors, faces): (Option<i64>, Option<i64>) = collection.db.query_row(
        "SELECT (SELECT max(rowid) FROM face_embeddings),(SELECT max(id) FROM faces)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(serde_json::to_string(&(
        key, model, vectors, faces, minimum,
    ))?)
}

/// Statistik mit dem Fingerabdruck, zu dem sie gehört.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cached {
    pub signature: String,
    pub stats: Stats,
}

/// Art und Format im lokalen Cache (`core::local_cache`).
const CACHE: (&str, u32) = ("person-stats", 2);

/// Zuletzt berechnete Statistik der Sammlung aus dem lokalen Cache.
pub fn cached(dir: &std::path::Path, collection: &str) -> Option<Cached> {
    super::local_cache::load(dir, CACHE.0, CACHE.1, collection)
}

/// Legt die berechnete Statistik im lokalen Cache ab.
pub fn store(dir: &std::path::Path, collection: &str, cached: &Cached) {
    super::local_cache::store(dir, CACHE.0, CACHE.1, collection, cached);
}

/// Prüft in einer Lesetransaktion den Fingerabdruck und rechnet nur, wenn er sich gegenüber
/// `known` geändert hat (`None` = unverändert).
pub fn refresh(
    collection: &Collection,
    known: Option<&str>,
    control: &Control,
) -> Result<(String, Option<Stats>)> {
    collection.db.execute_batch("BEGIN")?;
    let result = (|| {
        let signature = signature(collection)?;
        if known == Some(signature.as_str()) {
            return Ok((signature, None));
        }
        let stats = compute(collection, control)?;
        Ok((signature, Some(stats)))
    })();
    collection.db.execute_batch("ROLLBACK")?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_density_integrates_to_one_and_peaks_at_the_data() {
        let values = [0.6, 0.62, 0.65, 0.7, 0.71, 0.72, 0.8];
        let h = silverman(&values);
        let step = 0.001;
        let at: Vec<f32> = (0..2000).map(|i| -0.5 + i as f32 * step).collect();
        let density = kde(&values, h, &at);
        let area: f32 = density.iter().sum::<f32>() * step;
        assert!((area - 1.0).abs() < 1e-3, "Fläche {area}");
        let peak = at[density
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .expect("Werte")
            .0];
        assert!((0.6..0.75).contains(&peak), "Gipfel bei {peak}");
    }

    #[test]
    fn silverman_bandwidth_follows_the_rule_of_thumb() {
        // s = 1,2910 (n−1), IQR = 1,5 → min(1,2910, 1,1194) = 1,1194; 0,9 · 1,1194 · 4^(−0,2).
        let values = [1.0, 2.0, 3.0, 4.0];
        let expected = 0.9 * (1.5f32 / 1.34) * 4f32.powf(-0.2);
        assert!((silverman(&values) - expected).abs() < 1e-5);
        // Gleiche Werte: Mindestbreite statt 0.
        assert!(silverman(&[0.7; 10]) > 0.0);
    }

    #[test]
    fn few_values_become_points_and_many_a_trimmed_violin() {
        let few = distribution(&[0.7, 0.6]);
        assert_eq!(few.n, 2);
        assert!(few.density.is_empty());
        assert_eq!(few.points, vec![0.6, 0.7]);
        let many = distribution(&[0.5, 0.6, 0.6, 0.65, 0.7, 0.9]);
        assert_eq!(many.density.len(), config::PERSON_STATS_GRID);
        let summary = many.summary.expect("Kennzahlen");
        assert!((summary.median - 0.625).abs() < 1e-6);
        for (x, d) in grid().iter().zip(&many.density) {
            if *x < 0.5 - 1e-6 || *x > 0.9 + 1e-6 {
                assert_eq!(*d, 0.0, "außerhalb der Werte bei {x}");
            }
        }
        assert_eq!(many.density.iter().copied().fold(0.0, f32::max), 1.0);
    }

    #[test]
    fn threshold_is_the_highest_rejection() {
        assert_eq!(threshold(&[0.9], &[], &[]), None);
        let t = threshold(&[0.5, 0.71, 0.8, 0.9], &[0.55, 0.7], &[0.69, 0.75]).expect("t");
        assert_eq!(t.t, 0.7);
        assert_eq!(t.confirmed_above, 3);
        assert_eq!(t.rated_above, 1);
    }

    #[test]
    fn order_follows_the_count_of_the_mode() {
        let scores = vec![
            PersonScores {
                person: 1,
                name: "Anna".into(),
                rated: vec![0.7; 2],
                confirmed: vec![0.8; 10],
                rejected: vec![],
            },
            PersonScores {
                person: 2,
                name: "Ben".into(),
                rated: vec![0.7; 5],
                confirmed: vec![0.8; 3],
                rejected: vec![0.5; 1],
            },
            PersonScores {
                person: 3,
                name: "Cleo".into(),
                rated: vec![],
                confirmed: vec![0.8; 1],
                rejected: vec![],
            },
        ];
        let stats = build(scores, Limits::default());
        assert_eq!(
            stats.order(Mode::Rated),
            vec![1, 0],
            "Cleo ohne Werte fehlt"
        );
        assert_eq!(stats.order(Mode::Confirmed), vec![0, 1, 2]);
        assert_eq!(stats.people[1].threshold.map(|t| t.t), Some(0.5));
        assert_eq!((stats.review, stats.sure), (0.60, 0.70));
    }
}
