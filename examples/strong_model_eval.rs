//! Vergleicht SFace mit dem genaueren Modell (ArcFace) auf derselben Stichprobe (nur lesend).
//!
//! `cargo run --release --example strong_model_eval -- <Sammlung> <Modellordner> <onnxruntime.dll>
//!     <Vektordatei> [--persons 39,19,…] [--compute-only] [--max-photos N]`
//!
//! Öffnet die Sammlung nur lesend. Für alle bestätigten Gesichter der gewählten Personen und
//! alle Gesichter mit einer Ablehnung dieser Personen berechnet es den Vektor des genaueren
//! Modells wie der Lauf der App (`strong::load_crops`, gespeicherte Landmarks) und hängt ihn an
//! die Vektordatei an (außerhalb der Sammlung, z. B. `worktrees/<aufgabe>/tmp`); ein neuer
//! Aufruf setzt dort fort. Bewertet wird wie `reference_limit_eval`: feste Testmenge (20 % der
//! Aufnahmetage je Person), Referenzen nach `config::reference_limit` aus dem Rest, Top-3-Mittel,
//! Vorschlag ab Schwelle mit Abstand; Person-auslassen-Test für unbekannte Personen; Ablehnungen:
//! Anteil abgelehnter Gesichter, denen die abgelehnte Person vorgeschlagen würde.
use foto_importer::config;
use foto_importer::core::{
    collection::{Collection, safe_path_in},
    faces::{self, decode},
    inference::Embedder,
    jobs::Control,
    matching::{self, Candidate},
    strong,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const DEFAULT_PERSONS: &[i64] = &[39, 19, 2, 21, 47, 15, 22, 23, 14, 99, 100, 101, 41, 58, 38];

struct Face {
    id: i64,
    person: i64,
    quality: f32,
    captured: String,
    retained: bool,
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
fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| x * y)
        .sum::<f32>()
        .clamp(-1.0, 1.0)
}
fn top3(query: &[f32], vectors: &[&Vec<f32>]) -> f32 {
    let mut top = [f32::NEG_INFINITY; 3];
    for v in vectors {
        let s = dot(query, v);
        if s > top[2] {
            top[2] = s;
            top.sort_by(|a, b| b.total_cmp(a));
        }
    }
    let used: Vec<f32> = top.iter().copied().filter(|v| v.is_finite()).collect();
    used.iter().sum::<f32>() / used.len().max(1) as f32
}
fn accepted(ranking: &[(i64, f32)], threshold: f32, margin: f32) -> Option<i64> {
    let (person, best) = *ranking.first()?;
    let second = ranking.get(1).map_or(f32::NEG_INFINITY, |r| r.1);
    (best >= threshold && best - second >= margin).then_some(person)
}

fn read_cache(path: &Path) -> anyhow::Result<BTreeMap<i64, Vec<f32>>> {
    let mut map = BTreeMap::new();
    let Ok(mut file) = std::fs::File::open(path) else {
        return Ok(map);
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let record = 8 + 512 * 4;
    for chunk in bytes.chunks_exact(record) {
        let id = i64::from_le_bytes(chunk[..8].try_into()?);
        let vector = chunk[8..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect();
        map.insert(id, vector);
    }
    Ok(map)
}

/// Personen mit Personenscore, absteigend.
type Ranking = Vec<(i64, f32)>;

struct Model<'a> {
    name: &'a str,
    vectors: &'a BTreeMap<i64, Vec<f32>>,
    duplicate: f32,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!(
            "Aufruf: strong_model_eval <Sammlung> <Modellordner> <onnxruntime.dll> <Vektordatei> [--persons 1,2] [--compute-only] [--max-photos N]"
        );
        std::process::exit(2);
    }
    let (root, model_dir, dll, cache) = (
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    );
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let persons: BTreeSet<i64> = option("--persons")
        .map(|s| s.split(',').filter_map(|p| p.parse().ok()).collect())
        .unwrap_or_else(|| DEFAULT_PERSONS.iter().copied().collect());
    let compute_only = args.iter().any(|a| a == "--compute-only");
    let exiftool = option("--exiftool").unwrap_or_else(|| "exiftool".into());
    let max_photos: usize = option("--max-photos")
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    let arc_duplicate: f32 = option("--arc-duplicate")
        .and_then(|s| s.parse().ok())
        .unwrap_or(config::ARCFACE_SCALE.duplicate);
    let collection = Collection::open(root, false)?;
    let base = faces::setting(&collection, faces::EMBEDDER_KEY)?.expect("Kein Embedder");
    let manifest = strong::manifest(model_dir).expect("Genaueres Modell fehlt");
    let names: BTreeMap<i64, String> = collection
        .db
        .prepare("SELECT person_id,upper(substr(value,1,3)) FROM person_attrs WHERE key='name'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    // Stichprobe: bestätigte Gesichter der Personen und ihre Ablehnungen.
    let marks = persons
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let confirmed: Vec<Face> = collection
        .db
        .prepare(&format!(
            "SELECT f.id,f.person_id,f.quality,coalesce(f.captured_at,''),f.retained FROM faces f
             WHERE f.assignment='user' AND f.ignored=0 AND f.quality>=0.5 AND f.landmarks IS NOT NULL
               AND f.person_id IN ({marks}) ORDER BY f.id"
        ))?
        .query_map([], |r| {
            Ok(Face {
                id: r.get(0)?,
                person: r.get(1)?,
                quality: r.get(2)?,
                captured: r.get(3)?,
                retained: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let rejections: Vec<(i64, i64)> = collection
        .db
        .prepare(&format!(
            "SELECT r.face_id,r.person_id FROM face_rejections r JOIN faces f ON f.id=r.face_id
             WHERE r.person_id IN ({marks}) AND f.quality>=0.5 AND f.landmarks IS NOT NULL AND f.ignored<>2"
        ))?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let wanted: BTreeSet<i64> = confirmed
        .iter()
        .map(|f| f.id)
        .chain(rejections.iter().map(|r| r.0))
        .collect();

    // Vektoren des genaueren Modells: aus der Datei, fehlende berechnen.
    let mut arc = read_cache(cache)?;
    let mut jobs = strong::pending(&collection.db, &manifest.key())?;
    if let Some(dir) = option("--dump") {
        // Referenzprüfung gegen InsightFace: Analysebild, Landmarks und Rust-Vektor einiger
        // Gesichter in einen Ordner außerhalb der Sammlung schreiben.
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir)?;
        let mut embedder = Embedder::load(model_dir, dll, config::STRONG_EMBEDDER, 4)?;
        let control = Control::default();
        let mut written = 0;
        for job in jobs
            .iter()
            .filter(|j| j.faces.iter().any(|f| wanted.contains(&f.id)))
        {
            if written >= 12 {
                break;
            }
            let path = safe_path_in(&collection.root, &job.relpath)?;
            let image = foto_importer::core::previews::load(
                &path,
                &exiftool,
                job.orientation,
                1600,
                &control,
            )?;
            let rgb = image.to_rgb8();
            let face = &job.faces[0];
            let detection = foto_importer::core::inference::Detection {
                bbox: [
                    face.bbox[0] * rgb.width() as f32,
                    face.bbox[1] * rgb.height() as f32,
                    face.bbox[2] * rgb.width() as f32,
                    face.bbox[3] * rgb.height() as f32,
                ],
                landmarks: face.landmarks,
                score: face.score,
            };
            if detection.embedding_quality() < 0.5 {
                continue;
            }
            let vector = embedder.embed(&rgb, face.landmarks)?;
            rgb.save(dir.join(format!("face_{}.png", face.id)))?;
            std::fs::write(
                dir.join(format!("face_{}.json", face.id)),
                serde_json::to_string(
                    &serde_json::json!({"landmarks": face.landmarks, "vector": vector}),
                )?,
            )?;
            written += 1;
        }
        println!("{written} Referenzgesichter geschrieben");
        return Ok(());
    }
    for job in &mut jobs {
        job.faces
            .retain(|f| wanted.contains(&f.id) && !arc.contains_key(&f.id));
    }
    jobs.retain(|j| !j.faces.is_empty());
    jobs.truncate(max_photos);
    let open_faces: usize = jobs.iter().map(|j| j.faces.len()).sum();
    println!(
        "Stichprobe: {} Personen, {} bestätigte Gesichter, {} Ablehnungen; vorhanden {}, zu berechnen {} Gesichter in {} Fotos",
        persons.len(),
        confirmed.len(),
        rejections.len(),
        arc.len(),
        open_faces,
        jobs.len()
    );
    if !jobs.is_empty() {
        let mut embedder = Embedder::load(
            model_dir,
            dll,
            config::STRONG_EMBEDDER,
            config::STRONG_THREADS,
        )?;
        let control = Control::default();
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(cache)?;
        let next = AtomicUsize::new(0);
        let started = Instant::now();
        let (mut load_wait, mut embed_time, mut done_faces, mut failed) =
            (Duration::ZERO, Duration::ZERO, 0usize, 0usize);
        let load_total = std::sync::Mutex::new(Duration::ZERO);
        std::thread::scope(|scope| -> anyhow::Result<()> {
            let (sender, receiver) = std::sync::mpsc::sync_channel(config::STRONG_LOAD_THREADS * 2);
            for _ in 0..config::STRONG_LOAD_THREADS {
                let sender = sender.clone();
                let exiftool = exiftool.as_str();
                let (next, jobs, control, load_total, root) =
                    (&next, &jobs, &control, &load_total, &collection.root);
                scope.spawn(move || {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(i) else { break };
                        let t = Instant::now();
                        let loaded = safe_path_in(root, &job.relpath)
                            .and_then(|p| strong::load_crops(&p, job, exiftool, control));
                        if let Ok(mut total) = load_total.lock() {
                            *total += t.elapsed();
                        }
                        if sender.send(loaded).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            let mut photos = 0;
            loop {
                let wait = Instant::now();
                let Ok(loaded) = receiver.recv() else { break };
                load_wait += wait.elapsed();
                photos += 1;
                match loaded {
                    Ok(crops) => {
                        for (id, crop) in crops {
                            let t = Instant::now();
                            match crop.and_then(|c| embedder.embed_aligned(&c)) {
                                Ok(v) => {
                                    embed_time += t.elapsed();
                                    let mut record = id.to_le_bytes().to_vec();
                                    record.extend(v.iter().flat_map(|x| x.to_le_bytes()));
                                    out.write_all(&record)?;
                                    arc.insert(id, v);
                                    done_faces += 1;
                                }
                                Err(_) => failed += 1,
                            }
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        eprintln!("Fehler: {e:#}");
                    }
                }
                if photos % 200 == 0 {
                    let s = started.elapsed().as_secs_f64();
                    println!(
                        "  {photos}/{} Fotos, {done_faces} Gesichter, {:.2} s/Foto, {:.0} ms/Gesicht Inferenz, Warten auf Laden {:.0} %",
                        jobs.len(),
                        s / photos as f64,
                        embed_time.as_secs_f64() * 1000.0 / done_faces.max(1) as f64,
                        100.0 * load_wait.as_secs_f64() / s
                    );
                }
            }
            Ok(())
        })?;
        let s = started.elapsed().as_secs_f64();
        println!(
            "Berechnet: {done_faces} Gesichter in {} Fotos, {failed} Fehler, {s:.0} s gesamt = {:.3} s/Foto, {:.1} ms/Gesicht Inferenz ({} Threads), Laden {:.2} s/Foto je Thread",
            jobs.len(),
            s / jobs.len() as f64,
            embed_time.as_secs_f64() * 1000.0 / done_faces.max(1) as f64,
            config::STRONG_THREADS,
            load_total.lock().map(|t| t.as_secs_f64()).unwrap_or(0.0) / jobs.len() as f64,
        );
    }
    if compute_only {
        return Ok(());
    }

    // SFace-Vektoren derselben Gesichter aus der Sammlung.
    let mut sface: BTreeMap<i64, Vec<f32>> = BTreeMap::new();
    {
        let mut statement = collection
            .db
            .prepare("SELECT vec FROM face_embeddings WHERE face_id=? AND embedder_id=?")?;
        for id in &wanted {
            if let Ok(bytes) =
                statement.query_row(rusqlite::params![id, base], |r| r.get::<_, Vec<u8>>(0))
            {
                sface.insert(*id, decode(&bytes)?);
            }
        }
    }
    let both: BTreeSet<i64> = wanted
        .iter()
        .copied()
        .filter(|id| sface.contains_key(id) && arc.contains_key(id))
        .collect();
    println!(
        "Mit beiden Vektoren: {} von {} Gesichtern",
        both.len(),
        wanted.len()
    );
    let confirmed: Vec<&Face> = confirmed.iter().filter(|f| both.contains(&f.id)).collect();
    let rejections: Vec<(i64, i64)> = rejections
        .into_iter()
        .filter(|r| both.contains(&r.0))
        .collect();
    let mut by_person: BTreeMap<i64, Vec<&Face>> = BTreeMap::new();
    for f in &confirmed {
        by_person.entry(f.person).or_default().push(f);
    }
    let tests: Vec<&Face> = confirmed
        .iter()
        .copied()
        .filter(|f| held_out(f) && !f.retained)
        .collect();
    println!(
        "Personen: {}",
        by_person
            .iter()
            .map(|(p, f)| format!("{} {}", names.get(p).cloned().unwrap_or_default(), f.len()))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "Testgesichter: {}, Ablehnungen bewertet: {}",
        tests.len(),
        rejections.len()
    );

    let models = [
        Model {
            name: "SFace",
            vectors: &sface,
            duplicate: 0.92,
        },
        Model {
            name: "ArcFace",
            vectors: &arc,
            duplicate: arc_duplicate,
        },
    ];
    let control = Control::default();
    for model in &models {
        // Referenzen wie die App, nur aus Gesichtern außerhalb der Testmenge.
        let mut refs: BTreeMap<i64, Vec<&Vec<f32>>> = BTreeMap::new();
        for (person, faces) in &by_person {
            let pool: Vec<&&Face> = faces.iter().filter(|f| !held_out(f)).collect();
            let mut by_year: BTreeMap<String, Vec<&&Face>> = BTreeMap::new();
            for f in &pool {
                by_year
                    .entry(f.captured.get(..4).unwrap_or("").to_string())
                    .or_default()
                    .push(f);
            }
            let mut ranked = Vec::new();
            for (year, list) in &mut by_year {
                list.sort_by(|a, b| b.quality.total_cmp(&a.quality).then(a.id.cmp(&b.id)));
                for (rank, f) in list.iter().enumerate() {
                    ranked.push((rank, year.clone(), f.id));
                }
            }
            ranked.sort();
            let allowed: BTreeSet<i64> = ranked
                .iter()
                .take(config::REFERENCE_CANDIDATES_MAX)
                .map(|r| r.2)
                .collect();
            let mut rows: Vec<Candidate> = pool
                .iter()
                .filter(|f| allowed.contains(&f.id))
                .map(|f| Candidate {
                    id: f.id,
                    quality: f.quality,
                    captured: f.captured.clone(),
                    pinned: false,
                    vector: model.vectors[&f.id].clone(),
                })
                .collect();
            rows.sort_by_key(|r| r.id);
            if rows.is_empty() {
                continue;
            }
            let selection = matching::select_references(
                &rows,
                config::reference_limit(faces.len()),
                config::reference_validation,
                42,
                model.duplicate,
                &control,
            )?;
            let chosen: BTreeSet<i64> = selection.references.into_iter().collect();
            refs.insert(
                *person,
                rows.iter()
                    .filter(|r| chosen.contains(&r.id))
                    .map(|r| &model.vectors[&r.id])
                    .collect(),
            );
        }
        let rank = |query: &[f32], without: Option<i64>| {
            let mut ranking: Vec<(i64, f32)> = refs
                .iter()
                .filter(|(p, _)| Some(**p) != without)
                .map(|(p, v)| (*p, top3(query, v)))
                .collect();
            ranking.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            ranking
        };
        let test_rankings: Vec<(i64, Ranking, Ranking)> = tests
            .iter()
            .filter(|f| refs.contains_key(&f.person))
            .map(|f| {
                let v = &model.vectors[&f.id];
                (f.person, rank(v, None), rank(v, Some(f.person)))
            })
            .collect();
        let rejected_rankings: Vec<(i64, Vec<(i64, f32)>)> = rejections
            .iter()
            .filter(|(_, p)| refs.contains_key(p))
            .map(|(id, p)| (*p, rank(&model.vectors[id], None)))
            .collect();
        let refs_total: usize = refs.values().map(Vec::len).sum();
        println!(
            "\n{} ({} Referenzen, Duplikatgrenze {}): Testgesichter {}, Ablehnungen {}",
            model.name,
            refs_total,
            model.duplicate,
            test_rankings.len(),
            rejected_rankings.len()
        );
        println!(
            "| Schwelle | Abstand | richtig | falsch | unbekannt über Schwelle | Ablehnung vorgeschlagen |"
        );
        println!("|---|---|---|---|---|---|");
        let grid: Vec<f32> = (0..=14).map(|i| 0.25 + 0.05 * i as f32).collect();
        for margin in [0.03f32, 0.05, 0.08] {
            for &t in &grid {
                let n = test_rankings.len().max(1) as f64;
                let right = test_rankings
                    .iter()
                    .filter(|(p, r, _)| accepted(r, t, margin) == Some(*p))
                    .count();
                let wrong = test_rankings
                    .iter()
                    .filter(|(p, r, _)| accepted(r, t, margin).is_some_and(|q| q != *p))
                    .count();
                let unknown = test_rankings
                    .iter()
                    .filter(|(_, _, o)| accepted(o, t, margin).is_some())
                    .count();
                let rejected = rejected_rankings
                    .iter()
                    .filter(|(p, r)| accepted(r, t, margin) == Some(*p))
                    .count();
                println!(
                    "| {t:.2} | {margin:.2} | {:.1} % | {:.2} % | {:.1} % | {:.1} % |",
                    100.0 * right as f64 / n,
                    100.0 * wrong as f64 / n,
                    100.0 * unknown as f64 / n,
                    100.0 * rejected as f64 / rejected_rankings.len().max(1) as f64
                );
            }
        }
        // Je Person: richtig bei der jeweils gewählten Schwelle wird separat berechnet.
        let per_person = |t: f32, margin: f32| {
            let mut table: BTreeMap<i64, (usize, usize, usize)> = BTreeMap::new();
            for (p, r, _) in &test_rankings {
                let e = table.entry(*p).or_default();
                e.0 += 1;
                match accepted(r, t, margin) {
                    Some(q) if q == *p => e.1 += 1,
                    Some(_) => e.2 += 1,
                    None => {}
                }
            }
            let mut rejected: BTreeMap<i64, (usize, usize)> = BTreeMap::new();
            for (p, r) in &rejected_rankings {
                let e = rejected.entry(*p).or_default();
                e.0 += 1;
                e.1 += (accepted(r, t, margin) == Some(*p)) as usize;
            }
            (table, rejected)
        };
        let (t, m) = match model.name {
            "SFace" => (0.60, 0.05),
            _ => (
                option("--arc-review")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(config::ARCFACE_SCALE.review),
                option("--arc-margin")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.05),
            ),
        };
        let (table, rejected) = per_person(t, m);
        println!(
            "Je Person bei {t:.2} / {m:.2}: Person: Tests richtig/falsch; Ablehnungen vorgeschlagen"
        );
        for (p, (n, right, wrong)) in table {
            let (rn, rv) = rejected.get(&p).copied().unwrap_or_default();
            println!(
                "  {} ({p}): {n} Tests {:.0} % / {:.1} %; Ablehnungen {rv} von {rn}",
                names.get(&p).cloned().unwrap_or_default(),
                100.0 * right as f64 / n.max(1) as f64,
                100.0 * wrong as f64 / n.max(1) as f64
            );
        }
        // Paarverteilungen für Gruppen- und Duplikatgrenzen (Testgesichter untereinander).
        let mut same = Vec::new();
        let mut same_day = Vec::new();
        let mut different = Vec::new();
        for (i, a) in tests.iter().enumerate() {
            for b in tests.iter().skip(i + 1).step_by(7) {
                let s = dot(&model.vectors[&a.id], &model.vectors[&b.id]);
                if a.person == b.person {
                    same.push(s);
                    if day(a) == day(b) {
                        same_day.push(s);
                    }
                } else {
                    different.push(s);
                }
            }
        }
        for v in [&mut same, &mut same_day, &mut different] {
            v.sort_by(|a, b| a.total_cmp(b));
        }
        let quantile = |v: &[f32], q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        println!(
            "Paare: gleiche Person {} (Median {:.3}), gleicher Tag {} (Median {:.3}, 90 % {:.3}, 99 % {:.3}), verschiedene {} (Median {:.3}, 99 % {:.3}, 99,9 % {:.3}, 99,99 % {:.3})",
            same.len(),
            quantile(&same, 0.5),
            same_day.len(),
            quantile(&same_day, 0.5),
            quantile(&same_day, 0.9),
            quantile(&same_day, 0.99),
            different.len(),
            quantile(&different, 0.5),
            quantile(&different, 0.99),
            quantile(&different, 0.999),
            quantile(&different, 0.9999)
        );
        let above = |v: &[f32], t: f32| {
            v.iter().filter(|x| **x >= t).count() as f64 / v.len().max(1) as f64
        };
        for t in [
            0.2f32, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6, 0.65, 0.7,
        ] {
            println!(
                "  Paar ab {t:.2}: gleiche Person {:.1} %, verschiedene {:.3} %",
                100.0 * above(&same, t),
                100.0 * above(&different, t)
            );
        }
    }
    Ok(())
}
