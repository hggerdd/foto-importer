use super::{inference, jobs::Control, recognition};
use anyhow::{Result, bail};
use std::collections::BTreeSet;

/// Maximum-weight assignment, with caller-supplied unknown columns.
pub fn assignment(scores: &[Vec<f32>]) -> Result<Vec<usize>> {
    let n = scores.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    let m = scores[0].len();
    if m < n
        || scores
            .iter()
            .any(|r| r.len() != m || r.iter().any(|v| !v.is_finite()))
    {
        bail!("Ungültige Zuordnungsmatrix");
    }
    let (mut u, mut v, mut p, mut way) = (
        vec![0.0f64; n + 1],
        vec![0.0f64; m + 1],
        vec![0usize; m + 1],
        vec![0usize; m + 1],
    );
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0;
        let mut minimum = vec![f64::INFINITY; m + 1];
        let mut used = vec![false; m + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0;
            for j in 1..=m {
                if !used[j] {
                    let cost = -(scores[i0 - 1][j - 1] as f64) - u[i0] - v[j];
                    if cost < minimum[j] {
                        minimum[j] = cost;
                        way[j] = j0;
                    }
                    if minimum[j] < delta {
                        delta = minimum[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=m {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minimum[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut result = vec![0; n];
    for j in 1..=m {
        if p[j] > 0 {
            result[p[j] - 1] = j - 1;
        }
    }
    Ok(result)
}

pub struct Candidate {
    pub id: i64,
    pub quality: f32,
    pub captured: String,
    pub pinned: bool,
    pub vector: Vec<f32>,
}
pub struct Selection {
    pub references: Vec<i64>,
    pub validation: Vec<i64>,
    pub outliers: Vec<i64>,
}
fn time(value: &str) -> Option<chrono::NaiveDateTime> {
    value
        .get(..19)
        .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").ok())
}
/// Wählt bis zu `limit` Gesichter aus `rows`; davon hält `validation(Anzahl ausgewählt)` als
/// Validierung zurück (höchstens alle bis auf eines).
pub fn select_references(
    rows: &[Candidate],
    limit: usize,
    validation: impl Fn(usize) -> usize,
    seed: u64,
    duplicate: f32,
    control: &Control,
) -> Result<Selection> {
    let n = rows.len();
    if n == 0 {
        return Ok(Selection {
            references: Vec::new(),
            validation: Vec::new(),
            outliers: Vec::new(),
        });
    }
    let pins: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.pinned)
        .map(|(i, _)| i)
        .collect();
    if pins.len() > limit {
        bail!("Mehr fixierte Gesichter als das Referenzlimit");
    }
    let dim = rows[0].vector.len();
    if rows.iter().any(|r| r.vector.len() != dim) {
        bail!("Gemischte Modellräume");
    }
    let mut center = vec![0.0; dim];
    for row in rows {
        for (sum, value) in center.iter_mut().zip(&row.vector) {
            *sum += value;
        }
    }
    let medoid = (0..n)
        .max_by(|a, b| {
            rows[*a]
                .vector
                .iter()
                .zip(&center)
                .map(|(x, y)| x * y)
                .sum::<f32>()
                .total_cmp(
                    &rows[*b]
                        .vector
                        .iter()
                        .zip(&center)
                        .map(|(x, y)| x * y)
                        .sum::<f32>(),
                )
        })
        .unwrap_or(0);
    let medoid_scores: Vec<_> = rows
        .iter()
        .map(|r| inference::cosine(&r.vector, &rows[medoid].vector).unwrap_or(-1.0))
        .collect();
    let mut outliers = BTreeSet::new();
    for (i, row) in rows.iter().enumerate() {
        control.checkpoint()?;
        if n < 4 || row.pinned || medoid_scores[i] >= 0.2 {
            continue;
        }
        let nearest = recognition::top3_mean(
            rows.iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .filter_map(|(_, r)| inference::cosine(&row.vector, &r.vector)),
        )
        .unwrap_or(0.0);
        if nearest < 0.2 {
            outliers.insert(i);
        }
    }
    let times: Vec<_> = rows.iter().map(|r| time(&r.captured)).collect();
    let mut order: Vec<_> = (0..n).collect();
    order.sort_by(|a, b| {
        rows[*b]
            .quality
            .total_cmp(&rows[*a].quality)
            .then(rows[*a].id.cmp(&rows[*b].id))
    });
    let mut keep = pins.clone();
    for i in order {
        if rows[i].pinned || outliers.contains(&i) {
            continue;
        }
        if keep.iter().any(|j| {
            inference::cosine(&rows[i].vector, &rows[*j].vector).is_some_and(|s| s > duplicate)
                || times[i]
                    .zip(times[*j])
                    .is_some_and(|(a, b)| (a - b).num_seconds().abs() < 2)
        }) {
            continue;
        }
        keep.push(i);
    }
    let mut chosen = pins;
    let mut distances = vec![2.0f32; n];
    let update = |i: usize, distances: &mut [f32]| {
        for (j, row) in rows.iter().enumerate() {
            distances[j] = distances[j]
                .min(1.0 - inference::cosine(&row.vector, &rows[i].vector).unwrap_or(-1.0));
        }
    };
    for i in &chosen {
        update(*i, &mut distances);
    }
    // Merker statt `chosen.contains` und Jahresfächer statt Filtern über alle Kandidaten: Die
    // Auswahl bleibt dieselbe (die Wahl ist eine strenge Ordnung mit der ID als letztem
    // Kriterium, also unabhängig von der Reihenfolge der Optionen), kostet bei mehreren hundert
    // Referenzen aber nicht mehr quadratisch viele Vergleiche je Schritt.
    let mut taken = vec![false; n];
    for i in &chosen {
        taken[*i] = true;
    }
    let year = |i: usize| rows[i].captured.get(..4).unwrap_or("");
    let mut by_year: std::collections::BTreeMap<&str, Vec<usize>> = Default::default();
    for i in &keep {
        by_year.entry(year(*i)).or_default().push(*i);
    }
    let quota = (limit / by_year.len().max(1)).max(1);
    let pick = |options: &mut dyn Iterator<Item = usize>, chosen: &[usize], distances: &[f32]| {
        options.max_by(|a, b| {
            let score = |i: usize| {
                if chosen.is_empty() {
                    medoid_scores[i]
                } else {
                    distances[i] * (0.5 + 0.5 * rows[i].quality)
                }
            };
            score(*a)
                .total_cmp(&score(*b))
                .then(rows[*b].id.cmp(&rows[*a].id))
        })
    };
    for _ in 0..quota {
        for options in by_year.values() {
            if chosen.len() >= limit {
                break;
            }
            let mut open = options.iter().copied().filter(|i| !taken[*i]);
            if let Some(i) = pick(&mut open, &chosen, &distances) {
                chosen.push(i);
                taken[i] = true;
                update(i, &mut distances);
            }
        }
        control.checkpoint()?;
    }
    while chosen.len() < limit.min(keep.len()) {
        let mut open = keep.iter().copied().filter(|i| !taken[*i]);
        let Some(i) = pick(&mut open, &chosen, &distances) else {
            break;
        };
        chosen.push(i);
        taken[i] = true;
        update(i, &mut distances);
    }
    // Nie alle Ausgewählten zurückhalten: mindestens eine Referenz bleibt.
    let count = validation(chosen.len()).min(chosen.len().saturating_sub(1));
    let mut permutation = chosen.clone();
    permutation.sort_by_key(|i| {
        blake3::hash(format!("{seed}:{}", rows[*i].id).as_bytes())
            .as_bytes()
            .to_owned()
    });
    let mut validation = Vec::new();
    let mut days = BTreeSet::new();
    for i in &permutation {
        let day = rows[*i].captured.get(..10).unwrap_or("");
        if validation.len() < count && days.insert(day) {
            validation.push(*i);
        }
    }
    for i in permutation {
        if validation.len() >= count {
            break;
        }
        if !validation.contains(&i) {
            validation.push(i);
        }
    }
    Ok(Selection {
        references: chosen
            .iter()
            .filter(|i| !validation.contains(i))
            .map(|i| rows[*i].id)
            .collect(),
        validation: validation.iter().map(|i| rows[*i].id).collect(),
        outliers: outliers.iter().map(|i| rows[*i].id).collect(),
    })
}
