//! Misst, wie die Zahl der Referenzen je Person die Erkennung beeinflusst (nur lesend).
//!
//! `cargo run --release --example reference_limit_eval -- <Sammlungsordner> [--check]`
//!
//! Die Referenzauswahl (`matching::select_references`) läuft für jede Variante im Speicher;
//! in die Sammlung wird nichts geschrieben. Je Person bleiben reproduzierbar rund 20 % der
//! Aufnahmetage (BLAKE3 über Person und Tag) als feste Testmenge zurück: Diese Gesichter sind
//! in keiner Variante Kandidat und werden in allen Varianten gleich bewertet, Serienbilder
//! desselben Tages können also nicht in die Referenzen gelangen. Bewertet wird wie im Abgleich:
//! Personenscore = Mittel der drei besten Kosinuswerte, Vorschlag ab Schwelle mit 0,05 Abstand.
//! Person-auslassen: dieselbe Rechnung ohne die eigene Person (unbekannte Person).
//!
//! `--check` wählt die Referenzen jeder nicht geänderten Person mit der Vorgabe der App neu aus
//! (alle bestätigten Gesichter, ohne Testmenge) und vergleicht sie mit `training_set`.
use foto_importer::config;
use foto_importer::core::{
    collection::Collection,
    faces::{self, decode},
    jobs::Control,
    matching::{self, Candidate},
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};

const MARGIN: f32 = 0.05;
const BUCKETS: [(&str, usize, usize); 4] = [
    ("<30", 0, 30),
    ("30-100", 30, 100),
    ("100-500", 100, 500),
    (">500", 500, usize::MAX),
];

struct Face {
    id: i64,
    person: i64,
    quality: f32,
    captured: String,
    pinned: bool,
    excluded: bool,
    retained: bool,
    vector: Vec<f32>,
}

type LimitFn = Box<dyn Fn(usize) -> usize>;
type ValidationFn = fn(usize) -> usize;

struct Variant {
    name: String,
    limit: LimitFn,
    cap: Option<usize>,
    validation: ValidationFn,
    prototype: bool,
}

/// Validierung wie bis 2026-11-21: 20 %, mindestens zwei, ab drei Ausgewählten.
fn val_legacy(chosen: usize) -> usize {
    if chosen >= 3 {
        ((chosen as f32 * 0.2).round() as usize).max(2)
    } else {
        0
    }
}
fn val_none(_: usize) -> usize {
    0
}
fn saturating(max: f32) -> LimitFn {
    Box::new(move |n| ((max * (1.0 - (-(n as f32) / max).exp())).ceil() as usize).min(n))
}

fn day(face: &Face) -> String {
    match face.captured.get(..10) {
        Some(d) if d.chars().next().is_some_and(|c| c.is_ascii_digit()) => d.to_string(),
        _ => format!("id:{}", face.id),
    }
}
fn held_out(face: &Face) -> bool {
    let hash = blake3::hash(format!("holdout:{}:{}", face.person, day(face)).as_bytes());
    let value = u64::from_le_bytes(hash.as_bytes()[..8].try_into().expect("acht Bytes"));
    value % 5 == 0
}

/// Kandidaten wie die SQL-Abfrage der App: Pins und je Jahr nach Qualität im Wechsel,
/// höchstens `cap`.
fn candidates(pool: &[&Face], cap: Option<usize>) -> Vec<Candidate> {
    let mut by_year: BTreeMap<String, Vec<&Face>> = BTreeMap::new();
    for f in pool {
        by_year
            .entry(f.captured.get(..4).unwrap_or("").to_string())
            .or_default()
            .push(f);
    }
    let mut ranked = Vec::new();
    for (year, faces) in &mut by_year {
        faces.sort_by(|a, b| b.quality.total_cmp(&a.quality).then(a.id.cmp(&b.id)));
        for (rank, f) in faces.iter().enumerate() {
            ranked.push((rank, year.clone(), f.id));
        }
    }
    ranked.sort();
    let allowed: BTreeSet<i64> = ranked
        .iter()
        .take(cap.unwrap_or(usize::MAX))
        .map(|r| r.2)
        .collect();
    let mut rows: Vec<Candidate> = pool
        .iter()
        .filter(|f| !f.excluded && (f.pinned || allowed.contains(&f.id)))
        .map(|f| Candidate {
            id: f.id,
            quality: f.quality,
            captured: f.captured.clone(),
            pinned: f.pinned,
            vector: f.vector.clone(),
        })
        .collect();
    rows.sort_by_key(|r| r.id);
    rows
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| x * y)
        .sum::<f32>()
        .clamp(-1.0, 1.0)
}
fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

/// Personenscores (Top-3-Mittel) eines Testgesichts gegen alle Personen.
fn scores(query: &[f32], refs: &[(i64, Vec<Vec<f32>>)]) -> Vec<(i64, f32)> {
    refs.iter()
        .map(|(person, vectors)| {
            let mut top = [f32::NEG_INFINITY; 3];
            for v in vectors {
                let s = dot(query, v);
                if s > top[2] {
                    top[2] = s;
                    if top[2] > top[1] {
                        top.swap(1, 2);
                        if top[1] > top[0] {
                            top.swap(0, 1);
                        }
                    }
                }
            }
            let used: Vec<f32> = top.iter().copied().filter(|v| v.is_finite()).collect();
            (*person, used.iter().sum::<f32>() / used.len() as f32)
        })
        .collect()
}

#[derive(Default, Clone)]
struct Tally {
    tests: usize,
    right60: usize,
    right70: usize,
    wrong60: usize,
    wrong70: usize,
    unknown60: usize,
    unknown70: usize,
    refs: usize,
    people: usize,
}

fn accepted(ranking: &[(i64, f32)], threshold: f32) -> Option<i64> {
    let (person, best) = *ranking.first()?;
    let second = ranking.get(1).map_or(f32::NEG_INFINITY, |r| r.1);
    (best >= threshold && best - second >= MARGIN).then_some(person)
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!(
            "Aufruf: cargo run --release --example reference_limit_eval -- <Sammlungsordner> [--check]"
        );
        std::process::exit(2);
    };
    let check = args.any(|a| a == "--check");
    let collection = Collection::open(Path::new(&root), false)?;
    let key = faces::setting(&collection, faces::EMBEDDER_KEY)?.expect("Kein Embedder");
    let mut statement = collection.db.prepare(
        "SELECT f.id,f.person_id,f.quality,coalesce(f.captured_at,''),e.vec,
            EXISTS(SELECT 1 FROM training_set t WHERE t.face_id=f.id AND t.embedder_id=?1 AND t.pinned=1),
            EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=?1),
            f.retained
         FROM faces f JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=?1
         WHERE f.assignment='user' AND f.ignored=0 AND f.quality>=0.5 AND f.person_id IS NOT NULL
         ORDER BY f.id",
    )?;
    let all: Vec<Face> = statement
        .query_map([&key], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, f32>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Vec<u8>>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, bool>(6)?,
                r.get::<_, bool>(7)?,
            ))
        })?
        .map(|r| {
            let (id, person, quality, captured, bytes, pinned, excluded, retained) = r?;
            Ok(Face {
                id,
                person,
                quality,
                captured,
                pinned,
                excluded,
                retained,
                vector: decode(&bytes)?,
            })
        })
        .collect::<anyhow::Result<_>>()?;
    drop(statement);
    let open: usize = collection.db.query_row(
        "SELECT count(*) FROM faces f JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=?1
         WHERE f.ignored=0 AND f.retained=0 AND f.quality>=0.5 AND f.assignment IS NOT 'user'",
        [&key],
        |r| r.get(0),
    )?;
    let mut people: BTreeMap<i64, Vec<&Face>> = BTreeMap::new();
    for f in &all {
        people.entry(f.person).or_default().push(f);
    }
    let control = Control::default();

    if check {
        let dirty: BTreeSet<i64> = collection
            .db
            .prepare("SELECT person_id FROM face_reference_dirty WHERE embedder_id=?")?
            .query_map([&key], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut stored: BTreeMap<(i64, String), BTreeSet<i64>> = BTreeMap::new();
        for row in collection
            .db
            .prepare("SELECT person_id,split,face_id FROM training_set WHERE embedder_id=?")?
            .query_map([&key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        {
            let (p, s, f): (i64, String, i64) = row?;
            stored.entry((p, s)).or_default().insert(f);
        }
        let (mut same, mut differ) = (0, 0);
        for (person, faces) in &people {
            if dirty.contains(person) {
                continue;
            }
            let rows = candidates(faces, Some(2048));
            let selection =
                matching::select_references(&rows, 100, val_legacy, 42, 0.92, &control)?;
            let refs: BTreeSet<i64> = selection.references.into_iter().collect();
            let vals: BTreeSet<i64> = selection.validation.into_iter().collect();
            let empty = BTreeSet::new();
            if stored.get(&(*person, "ref".into())).unwrap_or(&empty) == &refs
                && stored.get(&(*person, "val".into())).unwrap_or(&empty) == &vals
            {
                same += 1;
            } else {
                differ += 1;
            }
        }
        println!(
            "Abgleich mit training_set: {same} Personen gleich, {differ} verschieden, {} geändert (übersprungen)",
            dirty.len()
        );
    }

    let sizes: BTreeMap<i64, usize> = people.iter().map(|(p, f)| (*p, f.len())).collect();
    let bucket = |n: usize| {
        BUCKETS
            .iter()
            .position(|b| n >= b.1 && n < b.2)
            .expect("Gruppen decken alle Größen ab")
    };
    let tests: Vec<&Face> = all.iter().filter(|f| held_out(f) && !f.retained).collect();
    println!(
        "{} bestätigte Gesichter (Qualität ab 0,5), {} Personen, {} Testgesichter, {} offene Gesichter",
        all.len(),
        people.len(),
        tests.len(),
        open
    );
    for (i, b) in BUCKETS.iter().enumerate() {
        let persons = sizes.values().filter(|n| bucket(**n) == i).count();
        let t = tests
            .iter()
            .filter(|f| bucket(sizes[&f.person]) == i)
            .count();
        println!("  Gruppe {:>8}: {persons} Personen, {t} Testgesichter", b.0);
    }

    let fixed = |l: usize| -> LimitFn { Box::new(move |_| l) };
    let variants = vec![
        Variant {
            name: "fest 100, 2048 Kand. (bisher)".into(),
            limit: fixed(100),
            cap: Some(2048),
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "fest 100 ohne Validierung".into(),
            limit: fixed(100),
            cap: Some(2048),
            validation: val_none,
            prototype: false,
        },
        Variant {
            name: "fest 100 + Prototyp".into(),
            limit: fixed(100),
            cap: Some(2048),
            validation: val_legacy,
            prototype: true,
        },
        Variant {
            name: "fest 200".into(),
            limit: fixed(200),
            cap: None,
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "fest 400".into(),
            limit: fixed(400),
            cap: None,
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "fest 400, 2048 Kand.".into(),
            limit: fixed(400),
            cap: Some(2048),
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "fest 800".into(),
            limit: fixed(800),
            cap: None,
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "alle".into(),
            limit: Box::new(|n| n),
            cap: None,
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "sätt. 400".into(),
            limit: saturating(400.0),
            cap: None,
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "sätt. 600".into(),
            limit: saturating(600.0),
            cap: None,
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "sätt. 800".into(),
            limit: saturating(800.0),
            cap: None,
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "sätt. 600, 2048 Kand.".into(),
            limit: saturating(600.0),
            cap: Some(2048),
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "sätt. 800, 2048 Kand.".into(),
            limit: saturating(800.0),
            cap: Some(2048),
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "sätt. 1200, 2048 Kand.".into(),
            limit: saturating(1200.0),
            cap: Some(2048),
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "sätt. 1200".into(),
            limit: saturating(1200.0),
            cap: None,
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "sätt. 1600, 2048 Kand.".into(),
            limit: saturating(1600.0),
            cap: Some(2048),
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "alle, 2048 Kand.".into(),
            limit: Box::new(|n| n),
            cap: Some(2048),
            validation: val_legacy,
            prototype: false,
        },
        Variant {
            name: "config (gewählt)".into(),
            limit: Box::new(config::reference_limit),
            cap: Some(config::REFERENCE_CANDIDATES_MAX),
            validation: config::reference_validation,
            prototype: false,
        },
        Variant {
            name: "config + Prototyp".into(),
            limit: Box::new(config::reference_limit),
            cap: Some(config::REFERENCE_CANDIDATES_MAX),
            validation: config::reference_validation,
            prototype: true,
        },
    ];

    println!(
        "\n{:<30} | {:>8} | {:>14} | {:>14} | {:>14} | {:>14} | {:>14} | {:>8} | {:>9} | {:>11}",
        "Variante",
        "Gruppe",
        "richtig 0,60",
        "richtig 0,70",
        "falsch 0,60",
        "falsch 0,70",
        "unbek. 0,60/70",
        "Refs",
        "Auswahl",
        "Abgleich*"
    );
    for variant in &variants {
        let mut refs: Vec<(i64, Vec<Vec<f32>>)> = Vec::new();
        let mut select_time = Duration::ZERO;
        for (person, faces) in &people {
            let pool: Vec<&Face> = faces.iter().copied().filter(|f| !held_out(f)).collect();
            if pool.is_empty() {
                continue;
            }
            let rows = candidates(&pool, variant.cap);
            let limit = (variant.limit)(faces.len()).max(rows.iter().filter(|r| r.pinned).count());
            let start = Instant::now();
            let selection =
                matching::select_references(&rows, limit, variant.validation, 42, 0.92, &control)?;
            select_time += start.elapsed();
            let chosen: BTreeSet<i64> = selection.references.into_iter().collect();
            let mut vectors: Vec<Vec<f32>> = rows
                .iter()
                .filter(|r| chosen.contains(&r.id))
                .map(|r| r.vector.clone())
                .collect();
            if variant.prototype && vectors.len() >= 2 {
                let mut mean = vec![0.0; vectors[0].len()];
                for v in &vectors {
                    mean.iter_mut().zip(v).for_each(|(m, x)| *m += x);
                }
                vectors.push(normalize(mean));
            }
            if !vectors.is_empty() {
                refs.push((*person, vectors));
            }
        }
        let mut tally = vec![Tally::default(); BUCKETS.len() + 1];
        for (person, vectors) in &refs {
            let b = bucket(sizes[person]);
            for t in [b, BUCKETS.len()] {
                tally[t].refs += vectors.len();
                tally[t].people += 1;
            }
        }
        let start = Instant::now();
        let results: Vec<(usize, [bool; 6])> = std::thread::scope(|scope| {
            let chunks: Vec<_> = tests
                .chunks(tests.len().div_ceil(config::MATCH_THREADS))
                .map(|chunk| {
                    let refs = &refs;
                    let sizes = &sizes;
                    let bucket = &bucket;
                    scope.spawn(move || {
                        chunk
                            .iter()
                            .filter(|f| refs.iter().any(|r| r.0 == f.person))
                            .map(|f| {
                                let mut ranking = scores(&f.vector, refs);
                                ranking.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
                                let a60 = accepted(&ranking, 0.60);
                                let a70 = accepted(&ranking, 0.70);
                                let others: Vec<(i64, f32)> = ranking
                                    .iter()
                                    .copied()
                                    .filter(|r| r.0 != f.person)
                                    .collect();
                                (
                                    bucket(sizes[&f.person]),
                                    [
                                        a60 == Some(f.person),
                                        a70 == Some(f.person),
                                        a60.is_some_and(|p| p != f.person),
                                        a70.is_some_and(|p| p != f.person),
                                        accepted(&others, 0.60).is_some(),
                                        accepted(&others, 0.70).is_some(),
                                    ],
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            chunks
                .into_iter()
                .flat_map(|c| c.join().expect("Rechenthread"))
                .collect()
        });
        let match_time = start.elapsed();
        for (b, flags) in &results {
            for t in [*b, BUCKETS.len()] {
                let y = &mut tally[t];
                y.tests += 1;
                y.right60 += flags[0] as usize;
                y.right70 += flags[1] as usize;
                y.wrong60 += flags[2] as usize;
                y.wrong70 += flags[3] as usize;
                y.unknown60 += flags[4] as usize;
                y.unknown70 += flags[5] as usize;
            }
        }
        let projected = match_time.as_secs_f64() / results.len().max(1) as f64 * open as f64;
        for (i, y) in tally.iter().enumerate() {
            let label = BUCKETS.get(i).map_or("gesamt", |b| b.0);
            let pct = |x: usize| format!("{:.1} %", 100.0 * x as f64 / y.tests.max(1) as f64);
            println!(
                "{:<30} | {:>8} | {:>14} | {:>14} | {:>14} | {:>14} | {:>14} | {:>8} | {:>9} | {:>11}",
                if i == 0 { variant.name.as_str() } else { "" },
                format!("{label} ({})", y.tests),
                pct(y.right60),
                pct(y.right70),
                pct(y.wrong60),
                pct(y.wrong70),
                format!("{}/{}", pct(y.unknown60), pct(y.unknown70)),
                y.refs,
                if i == BUCKETS.len() {
                    format!("{:.2} s", select_time.as_secs_f64())
                } else {
                    String::new()
                },
                if i == BUCKETS.len() {
                    format!("{projected:.1} s")
                } else {
                    String::new()
                },
            );
        }
    }
    println!(
        "\n* Abgleich: Zeit für alle Testgesichter mit {} Threads (einfache Skalarprodukte, nicht die Blockrechnung der App), hochgerechnet auf {open} offene Gesichter.",
        config::MATCH_THREADS
    );
    Ok(())
}
