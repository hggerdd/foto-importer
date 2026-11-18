//! Rückfrage „Möglicherweise dieselbe Person“: Personen, deren Referenzgesichter einander
//! sehr ähnlich sind, etwa nach einem Tippfehler im Namen. Die App führt nie selbst zusammen;
//! sie fragt. „Zusammenführen …“ nutzt den bestehenden Weg mit Bestätigung, „Verschieden“
//! speichert eine Ablehnung je Paar in `face_settings` (keine Schemaänderung), damit dasselbe
//! Paar nie wieder gefragt wird.
//!
//! Ähnlichkeit zweier Personen: Für jedes Referenzgesicht der einen Person das Mittel seiner
//! bis zu drei besten Kosinuswerte gegen die Referenzen der anderen (wie der Abgleich eine
//! Person bewertet), beide Richtungen zusammen, davon der Median. Der Median verhindert, dass
//! wenige ähnliche Gesichter (Geschwister, Kinderfotos) ein Paar tragen.
//!
//! Nie gefragt werden Paare, die gemeinsam auf einem Foto bestätigt sind, Paare mit einer
//! eingetragenen Beziehung und abgelehnte Paare.
use super::{collection::Collection, faces, gaps, recognition};
use crate::config::{DUPLICATE_PERSON_MAX, DUPLICATE_PERSON_MIN};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

const SETTING_KEY: &str = "people.declined_duplicates";

/// Ein Paar, das möglicherweise dieselbe Person ist; `a < b`.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub a: i64,
    pub b: i64,
    pub similarity: f32,
}

fn pair(a: i64, b: i64) -> (i64, i64) {
    (a.min(b), a.max(b))
}

/// Median der Werte (Mittel der beiden mittleren bei gerader Anzahl).
fn median(values: &mut [f32]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

/// Mittel der bis zu drei größten Werte.
fn top3_mean(values: impl Iterator<Item = f32>) -> Option<f32> {
    let mut top = [f32::NEG_INFINITY; 3];
    let mut len = 0;
    for value in values {
        len += 1;
        if value > top[2] {
            top[2] = value;
            top.sort_by(|x, y| y.total_cmp(x));
        }
    }
    let used = len.min(3);
    (used > 0).then(|| top[..used].iter().sum::<f32>() / used as f32)
}

/// Ähnlichkeit zweier Personen aus ihren normierten Referenzvektoren (siehe Moduldoku).
/// `None`, wenn eine Seite keine vergleichbaren Vektoren hat.
pub fn similarity(a: &[Vec<f32>], b: &[Vec<f32>]) -> Option<f32> {
    let dim = a.first()?.len();
    let a: Vec<&Vec<f32>> = a.iter().filter(|v| v.len() == dim).collect();
    let b: Vec<&Vec<f32>> = b.iter().filter(|v| v.len() == dim).collect();
    if b.is_empty() {
        return None;
    }
    // Eine Matrix für beide Richtungen.
    let scores: Vec<Vec<f32>> = a
        .iter()
        .map(|x| {
            b.iter()
                .map(|y| x.iter().zip(y.iter()).map(|(p, q)| p * q).sum::<f32>())
                .map(|s| {
                    if s.is_finite() {
                        s.clamp(-1.0, 1.0)
                    } else {
                        -1.0
                    }
                })
                .collect()
        })
        .collect();
    let mut values: Vec<f32> = scores
        .iter()
        .filter_map(|row| top3_mean(row.iter().copied()))
        .collect();
    values.extend((0..b.len()).filter_map(|j| top3_mean(scores.iter().map(|row| row[j]))));
    median(&mut values)
}

/// Rein: alle Paare ab `min`, die `excluded` nicht ausschließt; ähnlichste zuerst
/// (Gleichstand: kleinere IDs zuerst). Unbegrenzt; begrenzt wird nach allen Ausschlüssen.
pub fn rank(
    people: &BTreeMap<i64, Vec<Vec<f32>>>,
    min: f32,
    excluded: impl Fn(i64, i64) -> bool,
) -> Vec<Candidate> {
    let entries: Vec<(&i64, &Vec<Vec<f32>>)> =
        people.iter().filter(|(_, v)| !v.is_empty()).collect();
    let mut out = Vec::new();
    for (i, (a, first)) in entries.iter().enumerate() {
        for (b, second) in &entries[i + 1..] {
            if excluded(**a, **b) {
                continue;
            }
            if let Some(similarity) = similarity(first, second)
                && similarity >= min
            {
                out.push(Candidate {
                    a: **a,
                    b: **b,
                    similarity,
                });
            }
        }
    }
    out.sort_by(|x, y| {
        y.similarity
            .total_cmp(&x.similarity)
            .then((x.a, x.b).cmp(&(y.a, y.b)))
    });
    out
}

/// Ausdrücklich als verschieden markierte Paare (kleinere ID, größere ID).
pub fn load_declined(collection: &Collection) -> Result<BTreeSet<(i64, i64)>> {
    Ok(faces::setting(collection, SETTING_KEY)?
        .map(|text| gaps::decode_pairs(&text))
        .unwrap_or_default())
}

/// „Verschieden“: Dieses Paar wird nie wieder gefragt.
pub fn decline(collection: &mut Collection, a: i64, b: i64) -> Result<()> {
    collection.writable()?;
    let mut declined = load_declined(collection)?;
    declined.insert(pair(a, b));
    faces::save_setting(&collection.db, SETTING_KEY, &gaps::encode_pairs(&declined))
}

/// Offene Rückfragen der Sammlung; ohne Gesichtsmodell keine.
pub fn load(collection: &Collection) -> Result<Vec<Candidate>> {
    let key = faces::setting(collection, "rust.embedder_key")?.unwrap_or_default();
    if key.is_empty() {
        return Ok(Vec::new());
    }
    let active: BTreeSet<i64> = collection
        .db
        .prepare("SELECT id FROM persons WHERE merged_into IS NULL")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut vectors = recognition::reference_vectors(collection, &key)?;
    vectors.retain(|person, _| active.contains(person));
    let declined = load_declined(collection)?;
    let related: BTreeSet<(i64, i64)> = collection
        .db
        .prepare("SELECT from_person,to_person FROM relations")?
        .query_map([], |r| Ok(pair(r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let candidates = rank(&vectors, DUPLICATE_PERSON_MIN, |a, b| {
        declined.contains(&pair(a, b)) || related.contains(&pair(a, b))
    });
    let mut together = collection.db.prepare(
        "SELECT EXISTS(SELECT 1 FROM faces x JOIN faces y ON y.photo_id=x.photo_id
         WHERE x.person_id=?1 AND x.assignment='user' AND x.retained=0
           AND y.person_id=?2 AND y.assignment='user' AND y.retained=0)",
    )?;
    let mut out = Vec::new();
    for candidate in candidates {
        if out.len() >= DUPLICATE_PERSON_MAX {
            break;
        }
        // Zwei bestätigte Gesichter auf einem Foto sind zwei Menschen.
        if !together.query_row([candidate.a, candidate.b], |r| r.get::<_, bool>(0))? {
            out.push(candidate);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(values: &[f32]) -> Vec<f32> {
        let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
        values.iter().map(|v| v / norm).collect()
    }

    #[test]
    fn similar_reference_sets_score_high_and_different_ones_low() {
        let anna = vec![unit(&[1.0, 0.1, 0.0]), unit(&[1.0, 0.0, 0.1])];
        let anna_again = vec![unit(&[1.0, 0.05, 0.05])];
        let ben = vec![unit(&[0.0, 1.0, 0.0]), unit(&[0.1, 1.0, 0.0])];
        assert!(similarity(&anna, &anna_again).expect("Wert") > 0.95);
        assert!(similarity(&anna, &ben).expect("Wert") < 0.3);
        assert_eq!(
            similarity(&anna, &anna_again),
            similarity(&anna_again, &anna),
            "symmetrisch"
        );
    }

    #[test]
    fn a_few_lookalike_faces_do_not_carry_a_pair() {
        // Zwei von acht Gesichtern ähneln B, die Mehrheit nicht: der Median bleibt niedrig.
        let mut a = vec![unit(&[0.0, 1.0, 0.0]); 2];
        a.extend(vec![unit(&[1.0, 0.0, 0.0]); 6]);
        let b = vec![unit(&[0.0, 1.0, 0.0]); 2];
        assert!(similarity(&a, &b).expect("Wert") < 0.5);
    }

    #[test]
    fn rank_orders_by_similarity_and_respects_threshold_and_exclusions() {
        let people = BTreeMap::from([
            (1, vec![unit(&[1.0, 0.0, 0.0])]),
            (2, vec![unit(&[1.0, 0.02, 0.0])]),
            (3, vec![unit(&[1.0, 0.3, 0.0])]),
            (4, vec![unit(&[0.0, 0.0, 1.0])]),
            (5, Vec::new()),
        ]);
        let all = rank(&people, 0.9, |_, _| false);
        let pairs: Vec<(i64, i64)> = all.iter().map(|c| (c.a, c.b)).collect();
        assert_eq!(pairs, [(1, 2), (2, 3), (1, 3)]);
        assert!(all.windows(2).all(|w| w[0].similarity >= w[1].similarity));
        let without = rank(&people, 0.9, |a, b| (a, b) == (1, 2));
        assert_eq!(without.len(), 2);
        assert!(without.iter().all(|c| (c.a, c.b) != (1, 2)));
    }

    #[test]
    fn medians_and_top_three_follow_their_definitions() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(&mut []), None);
        let best = top3_mean([0.1, 0.9, 0.5, 0.7].into_iter()).expect("Wert");
        assert!((best - 0.7).abs() < 1e-6);
        assert_eq!(top3_mean([0.4].into_iter()), Some(0.4));
        assert_eq!(top3_mean(std::iter::empty()), None);
    }
}
