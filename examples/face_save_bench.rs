//! Messreihe für „Gesichtsgruppe speichern und nächste Gruppe zeigen" (ADR 0066).
//!
//! Ausschließlich synthetische Daten; es wird nie eine private Sammlung gelesen.
//!
//! ```text
//! cargo run --release --example face_save_bench -- gen tmp/bench-faces [fotos]
//! cargo run --release --example face_save_bench -- run tmp/bench-faces tmp/bench-faces-work [maintenance_sekunden]
//! cargo run --release --example face_save_bench -- plans tmp/bench-faces
//! ```
//! `gen` erzeugt eine Sammlung (Standard 90.000 Fotos, 200.000 Gesichter, 40 Personen,
//! 15.000 bestätigte Gesichter, Vorschläge, Gruppen, Referenzen, trainiertes Modell).
//! `run` kopiert sie nach `work` und misst die Kette Speichern, Nachladen und Wartung.
use foto_importer::core::{
    classifier,
    collection::Collection,
    faces::{self, encode},
    jobs::Control,
    recognition,
};
use rusqlite::params;
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    time::{Duration, Instant},
};

const EMBEDDER: &str = "bench-emb";
const DETECTOR: &str = "bench-det";
const DIM: usize = 128;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn unit(&mut self) -> f32 {
        ((self.next() >> 40) as f32 + 0.5) / (1u64 << 24) as f32
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn gauss(&mut self) -> f32 {
        let (a, b) = (self.unit(), self.unit());
        (-2.0 * a.ln()).sqrt() * (std::f32::consts::TAU * b).cos()
    }
}
fn unit_vector(rng: &mut Rng) -> Vec<f32> {
    normalize((0..DIM).map(|_| rng.gauss()).collect())
}
fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter_mut().for_each(|x| *x /= n);
    v
}
fn around(center: &[f32], spread: f32, rng: &mut Rng) -> Vec<f32> {
    normalize(center.iter().map(|c| c + spread * rng.gauss()).collect())
}
fn signature(vector: &[f32]) -> i64 {
    vector
        .iter()
        .take(12)
        .enumerate()
        .fold(0, |bits, (i, v)| bits | (((*v >= 0.0) as i64) << i))
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    User(usize),
    Auto(usize),
    Suggested(usize),
    Ignored,
    Unknown,
}

fn generate(root: &Path, photos: usize) -> anyhow::Result<()> {
    fs::create_dir_all(root)?;
    let started = Instant::now();
    let mut c = Collection::open(root, true)?;
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let faces_total = photos * 20 / 9;
    let (users, autos, suggested, ignored) = (
        faces_total * 3 / 40,
        faces_total / 25,
        faces_total * 3 / 50,
        faces_total / 40,
    );
    let people = 40usize;
    let centers: Vec<Vec<f32>> = (0..people).map(|_| unit_vector(&mut rng)).collect();
    let person_of = |rng: &mut Rng| {
        if rng.below(10) < 6 {
            rng.below(people)
        } else {
            (rng.unit().powi(2) * people as f32) as usize % people
        }
    };
    let mut kinds = vec![Kind::Unknown; faces_total];
    for (i, kind) in kinds.iter_mut().enumerate() {
        *kind = if i < users {
            Kind::User(person_of(&mut rng))
        } else if i < users + autos {
            Kind::Auto(person_of(&mut rng))
        } else if i < users + autos + suggested {
            Kind::Suggested(person_of(&mut rng))
        } else if i < users + autos + suggested + ignored {
            Kind::Ignored
        } else {
            Kind::Unknown
        };
    }
    for i in (1..faces_total).rev() {
        kinds.swap(i, rng.below(i + 1));
    }
    let quality: Vec<f32> = kinds
        .iter()
        .map(|k| {
            let good = if matches!(k, Kind::User(_)) { 92 } else { 85 };
            if rng.below(100) < good {
                0.5 + 0.5 * rng.unit()
            } else {
                0.1 + 0.39 * rng.unit()
            }
        })
        .collect();
    // Unbekannte Gesichter in Gruppen (wie prepare_groups/merge_small_groups sie hinterlassen).
    let unknown: Vec<usize> = (0..faces_total)
        .filter(|i| kinds[*i] == Kind::Unknown && quality[*i] >= 0.5)
        .collect();
    let mut group_of = vec![usize::MAX; faces_total];
    let mut group_centers: Vec<Vec<f32>> = Vec::new();
    let mut group_sizes: Vec<usize> = Vec::new();
    let mut at = 0;
    while at < unknown.len() {
        let size = match rng.below(10) {
            0 | 1 => 1 + rng.below(2),
            2..=6 => 3 + rng.below(10),
            _ => 20 + rng.below(9),
        }
        .min(unknown.len() - at);
        let center = if rng.below(4) == 0 {
            let near = &centers[rng.below(people)];
            normalize(
                near.iter()
                    .zip(unit_vector(&mut rng))
                    .map(|(a, b)| a + 0.6 * b)
                    .collect(),
            )
        } else {
            unit_vector(&mut rng)
        };
        for face in &unknown[at..at + size] {
            group_of[*face] = group_centers.len();
        }
        group_centers.push(center);
        group_sizes.push(size);
        at += size;
    }
    c.db.execute(
        "INSERT OR REPLACE INTO face_settings VALUES('rust.detector_key',?)",
        [serde_json::to_string(DETECTOR)?],
    )?;
    let detector = faces::active_detector_key(&c)?.unwrap_or_default();
    let tx = c.db.transaction()?;
    {
        let mut photo = tx.prepare("INSERT INTO photos VALUES(?,?, 'present',1)")?;
        let mut file = tx.prepare("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status,first_seen_scan,last_seen_scan,meta_state) VALUES(?,?,?,?,?,?, 'jpg',?,?,?, 'present',1,1,'done')")?;
        let mut meta = tx.prepare(
            "INSERT INTO meta(file_id,make,model,dt_original,width,height,iso) VALUES(?, 'Bench','Cam',?,4000,3000,200)",
        )?;
        let mut job =
            tx.prepare("INSERT INTO face_jobs VALUES(?, 'detect', ?, 'done', NULL, NULL, NULL)")?;
        tx.execute(
            "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key',?)",
            [serde_json::to_string(EMBEDDER)?],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO face_settings VALUES('rust.detector_key',?)",
            [serde_json::to_string(DETECTOR)?],
        )?;
        for id in 1..=photos as i64 {
            let year = 2005 + (id % 20);
            let dir = format!("{year}/{:02}", 1 + id % 12);
            let name = format!("IMG_{id:06}.jpg");
            let rel = format!("{dir}/{name}");
            photo.execute(params![id, format!("bench-{id}")])?;
            file.execute(params![
                id,
                rel,
                rel.to_lowercase(),
                dir,
                name,
                format!("IMG_{id:06}"),
                2_000_000 + id,
                1_600_000_000_000_000_000i64 + id,
                id
            ])?;
            meta.execute(params![
                id,
                format!("{year}:{:02}:{:02} 12:00:00", 1 + id % 12, 1 + id % 28)
            ])?;
            job.execute(params![id, detector])?;
        }
    }
    {
        let mut person = tx.prepare("INSERT INTO persons(id) VALUES(?)")?;
        let mut attr = tx.prepare("INSERT INTO person_attrs VALUES(?,?,?)")?;
        for p in 0..people as i64 {
            person.execute([p + 1])?;
            attr.execute(params![p + 1, "name", format!("Person {:02}", p + 1)])?;
            attr.execute(params![p + 1, "birth_year", (1940 + p).to_string()])?;
        }
    }
    {
        let mut face = tx.prepare("INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,det_score,quality,person_id,assignment,confidence,assigned_at,ignored,retained,captured_at) VALUES(?,?,?,?,?,100,100,300,300,0.9,?,?,?,?,?,?,0,?)")?;
        let mut embedding = tx.prepare("INSERT INTO face_embeddings VALUES(?,?,?,?)")?;
        let mut catalog = tx.prepare("INSERT INTO face_group_catalog(group_id,embedder_id,signature,representative,members) VALUES(?,?,?,?,?)")?;
        let mut member = tx.prepare("INSERT INTO face_group_members VALUES(?,?,?)")?;
        let mut group_vectors = Vec::new();
        for i in 0..faces_total {
            let id = i as i64 + 1;
            let photo = (i % photos) as i64 + 1;
            let rel = {
                let year = 2005 + (photo % 20);
                format!("{year}/{:02}/IMG_{photo:06}.jpg", 1 + photo % 12)
            };
            let (person, assignment, confidence, is_ignored, vector) = match kinds[i] {
                Kind::User(p) => (
                    Some(p as i64 + 1),
                    Some("user"),
                    Some(1.0),
                    0,
                    around(&centers[p], 0.065, &mut rng),
                ),
                Kind::Auto(p) => (
                    Some(p as i64 + 1),
                    Some("auto"),
                    Some(0.65f64),
                    0,
                    around(&centers[p], 0.065, &mut rng),
                ),
                Kind::Suggested(p) => (
                    Some(p as i64 + 1),
                    Some("suggested"),
                    Some(0.45f64),
                    0,
                    around(&centers[p], 0.07, &mut rng),
                ),
                Kind::Ignored => (None, None, None, 1, unit_vector(&mut rng)),
                Kind::Unknown if group_of[i] != usize::MAX => (
                    None,
                    None,
                    None,
                    0,
                    around(&group_centers[group_of[i]], 0.05, &mut rng),
                ),
                Kind::Unknown => (None, None, None, 0, unit_vector(&mut rng)),
            };
            face.execute(params![
                id,
                photo,
                photo,
                rel,
                DETECTOR,
                quality[i],
                person,
                assignment,
                confidence,
                assignment.map(|_| "2026-01-01 00:00:00"),
                is_ignored,
                format!("{}:01:01 12:00:00", 2005 + (photo % 20))
            ])?;
            embedding.execute(params![id, EMBEDDER, DIM as i64, encode(&vector)])?;
            if group_of[i] != usize::MAX {
                group_vectors.push((group_of[i], id, vector));
            }
        }
        let mut seen = BTreeSet::new();
        for (group, id, vector) in &group_vectors {
            if seen.insert(*group) {
                catalog.execute(params![
                    *group as i64 + 1,
                    EMBEDDER,
                    signature(vector),
                    encode(vector),
                    group_sizes[*group] as i64
                ])?;
            }
            member.execute(params![EMBEDDER, id, *group as i64 + 1])?;
        }
        // Referenzen wie recognition::rebuild: je Person bis 100 ref + 42 val.
        let mut per_person = vec![0usize; people];
        let mut train =
            tx.prepare("INSERT INTO training_set VALUES(?,?,?,?, '2026-01-01', 'rust-fps-v1', 0)")?;
        for (i, kind) in kinds.iter().enumerate() {
            if let Kind::User(p) = kind
                && quality[i] >= 0.5
                && per_person[*p] < 142
            {
                let split = if per_person[*p] < 100 { "ref" } else { "val" };
                train.execute(params![*p as i64 + 1, i as i64 + 1, EMBEDDER, split])?;
                per_person[*p] += 1;
            }
        }
    }
    tx.commit()?;
    println!("Daten eingefügt: {:.1?}", started.elapsed());
    let started = Instant::now();
    classifier::train(&mut c, EMBEDDER, &Control::default())?;
    println!("Klassifikator trainiert: {:.1?}", started.elapsed());
    c.db.execute_batch("ANALYZE; PRAGMA wal_checkpoint(TRUNCATE);")?;
    for (label, sql) in [
        ("Fotos", "SELECT count(*) FROM photos"),
        ("Gesichter", "SELECT count(*) FROM faces"),
        (
            "bestätigt",
            "SELECT count(*) FROM faces WHERE assignment='user'",
        ),
        (
            "auto/vorgeschlagen",
            "SELECT count(*) FROM faces WHERE assignment IN ('auto','suggested')",
        ),
        ("Embeddings", "SELECT count(*) FROM face_embeddings"),
        ("Referenzen", "SELECT count(*) FROM training_set"),
        ("Gruppen", "SELECT count(*) FROM face_group_catalog"),
        (
            "Gruppenmitglieder",
            "SELECT count(*) FROM face_group_members",
        ),
        ("metadata_photos", "SELECT count(*) FROM metadata_photos"),
    ] {
        let n: i64 = c.db.query_row(sql, [], |r| r.get(0))?;
        println!("  {label}: {n}");
    }
    Ok(())
}

fn timed<T>(label: &str, f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    let started = Instant::now();
    let value = f()?;
    println!(
        "  {label:<58} {:>9.1} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(value)
}

fn run(template: &Path, work: &Path, maintenance_seconds: u64) -> anyhow::Result<()> {
    if work.exists() {
        fs::remove_dir_all(work)?;
    }
    fs::create_dir_all(work)?;
    fs::copy(template.join(".myfotomanager"), work.join(".myfotomanager"))?;
    println!(
        "Profil: {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    println!("A. Öffnen");
    timed("Collection::open(lesen)", || {
        Collection::open(work, false).map(|_| ())
    })?;
    timed("Collection::open(schreiben)", || {
        Collection::open(work, true).map(|_| ())
    })?;
    println!("B. Seiten (kalt, dann warm)");
    let reader = Collection::open(work, false)?;
    for mode in ["unknown", "review", "confirmed", "ignored", "persons"] {
        let page = timed(&format!("page({mode}) kalt"), || {
            faces::page(&reader, mode, 0)
        })?;
        timed(&format!("page({mode}) warm"), || {
            faces::page(&reader, mode, 0).map(|_| ())
        })?;
        println!(
            "      Gruppen {} · Gruppen gesamt {} · Vorschläge {}",
            page.groups.len(),
            page.total_groups,
            page.suggestions.len()
        );
    }
    println!("B2. Teile");
    timed("status()", || faces::status(&reader).map(|_| ()))?;
    timed("people()", || faces::people(&reader).map(|_| ()))?;
    for (label, sql) in [
        (
            "  SQL total",
            "SELECT (SELECT count(*) FROM faces)-(SELECT count(*) FROM faces WHERE retained=1)",
        ),
        (
            "  SQL confirmed",
            "SELECT count(*) FROM faces WHERE assignment='user' AND retained=0",
        ),
        (
            "  SQL unknown",
            "SELECT count(*) FROM faces WHERE ignored=0 AND retained=0 AND person_id IS NULL",
        ),
        (
            "  SQL review",
            "SELECT count(*) FROM faces WHERE ignored=0 AND retained=0 AND assignment IN ('suggested','auto')",
        ),
        (
            "  SQL ignored",
            "SELECT count(*) FROM faces WHERE ignored=1 AND retained=0",
        ),
        (
            "  SQL analysierte Fotos",
            "SELECT count(*) FROM face_jobs j JOIN photos p ON p.id=j.photo_id WHERE p.status='present' AND j.stage='detect' AND j.model_id='bench-det' AND j.status='done'",
        ),
        (
            "  SQL vorhandene Fotos",
            "SELECT count(*) FROM photos WHERE status='present'",
        ),
        (
            "  SQL nameable",
            "SELECT coalesce(sum(members),0) FROM face_group_catalog WHERE embedder_id='bench-emb'",
        ),
    ] {
        timed(label, || {
            Ok(reader.db.query_row(sql, [], |r| r.get::<_, i64>(0))?)
        })?;
    }
    timed("relations::load_edges", || {
        foto_importer::core::relations::load_edges(&reader).map(|_| ())
    })?;
    let page = faces::page(&reader, "unknown", 0)?;
    let first: Vec<i64> = page.groups[0].iter().map(|f| f.id).collect();
    timed("group_suggestions(erste Gruppe)", || {
        recognition::group_suggestions(&reader, &first, EMBEDDER).map(|_| ())
    })?;
    timed("classifier::load", || {
        classifier::load(&reader, EMBEDDER).map(|_| ())
    })?;
    drop(reader);
    println!("C. Speichern (name_group) und Nachladen, 4 Durchgänge");
    for round in 0..4 {
        let reader = Collection::open(work, false)?;
        let page = faces::page(&reader, "unknown", 0)?;
        drop(reader);
        let ids: Vec<i64> = page.groups[0].iter().map(|f| f.id).collect();
        let selected: BTreeSet<i64> = ids.iter().copied().collect();
        let person = (round % 40) as i64 + 1;
        let started = Instant::now();
        let mut c = timed("  open(schreiben)", || Collection::open(work, true))?;
        let undo = timed(&format!("  name_group ({} Gesichter)", ids.len()), || {
            faces::name_group(&mut c, &ids, &selected, Some(person), "", false)
        })?;
        drop(c);
        println!(
            "    Speichern gesamt {:.1} ms (Vorher {} / Nachher {} Zeilen)",
            started.elapsed().as_secs_f64() * 1000.0,
            undo.before.len(),
            undo.after.len()
        );
        let reload = Instant::now();
        let reader = timed("  open(lesen)", || Collection::open(work, false))?;
        reader.db.execute_batch("BEGIN")?;
        timed("  page(unknown, 0) nach Speichern", || {
            faces::page(&reader, "unknown", 0).map(|_| ())
        })?;
        println!(
            "    Nachladen gesamt {:.1} ms",
            reload.elapsed().as_secs_f64() * 1000.0
        );
    }
    println!("D. Wartung nach Bestätigung ({maintenance_seconds} s laufen lassen, dann abbrechen)");
    let control = Control {
        background: true,
        ..Default::default()
    };
    let root = work.to_path_buf();
    let worker_control = control.clone();
    let started = Instant::now();
    let handle = std::thread::spawn(move || -> anyhow::Result<String> {
        let mut c = Collection::open(&root, true)?;
        recognition::maintain(&mut c, &worker_control)
    });
    let mut last = String::new();
    while started.elapsed() < Duration::from_secs(maintenance_seconds) && !handle.is_finished() {
        let phase = match control.face_phase() {
            None => "-",
            Some(foto_importer::core::jobs::FacePhase::Analysis) => "Analyse",
            Some(foto_importer::core::jobs::FacePhase::References) => "Referenzen",
            Some(foto_importer::core::jobs::FacePhase::Matching) => "Abgleich",
            Some(foto_importer::core::jobs::FacePhase::Groups) => "Gruppen",
        };
        if phase != last {
            println!("  +{:.1} s Phase {phase}", started.elapsed().as_secs_f64());
            last = phase.to_string();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let stop = Instant::now();
    control.stop();
    let result = handle
        .join()
        .map_err(|_| anyhow::anyhow!("Wartung abgestürzt"))?;
    println!(
        "  Wartung lief {:.1} s, Abbruch bis Rückkehr {:.1} ms, Ergebnis: {}",
        started.elapsed().as_secs_f64(),
        stop.elapsed().as_secs_f64() * 1000.0,
        match &result {
            Ok(message) => message.clone(),
            Err(error) => format!("{error:#}"),
        }
    );
    println!(
        "  Fortschritt {} / {} · Zeiten {:?}",
        control.done.load(std::sync::atomic::Ordering::Relaxed),
        control.total.load(std::sync::atomic::Ordering::Relaxed),
        control.timings()
    );
    let reader = Collection::open(work, false)?;
    let (dirty, cursor): (i64, i64) = (
        reader
            .db
            .query_row("SELECT count(*) FROM face_reference_dirty", [], |r| {
                r.get(0)
            })?,
        reader.db.query_row(
            "SELECT coalesce(max(after_photo),-1) FROM face_maintenance",
            [],
            |r| r.get(0),
        )?,
    );
    println!("  danach: schmutzige Personen {dirty}, Abgleich-Cursor {cursor}");
    Ok(())
}

fn plans(root: &Path) -> anyhow::Result<()> {
    let c = Collection::open(root, false)?;
    let queries = [
        (
            "ranked: Referenzvektoren",
            "SELECT f.person_id,e.vec,coalesce(a.value,'Unbenannt') FROM faces f JOIN face_embeddings e ON e.face_id=f.id LEFT JOIN person_attrs a ON a.person_id=f.person_id AND a.key='name' WHERE e.embedder_id='bench-emb' AND f.id!=5 AND f.assignment='user' AND f.ignored=0 AND f.quality>=0.5 AND (EXISTS(SELECT 1 FROM training_set t WHERE t.face_id=f.id AND t.embedder_id='bench-emb' AND t.split='ref') OR NOT EXISTS(SELECT 1 FROM training_set t WHERE t.person_id=f.person_id AND t.embedder_id='bench-emb')) AND NOT EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=e.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_review v WHERE v.face_id=f.id AND v.embedder_id=e.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_rejections r WHERE r.face_id=5 AND r.person_id=f.person_id) AND NOT EXISTS(SELECT 1 FROM faces o WHERE o.photo_id=7 AND o.assignment='user' AND o.person_id=f.person_id AND o.id!=5) ORDER BY f.person_id,f.id",
        ),
        (
            "status: Zähler",
            "SELECT count(*),coalesce(sum(assignment='user'),0),coalesce(sum(person_id IS NULL AND ignored=0),0),coalesce(sum(assignment IN ('auto','suggested') AND ignored=0),0),coalesce(sum(ignored=1),0) FROM faces WHERE retained=0",
        ),
        (
            "status: analysierte Fotos",
            "SELECT count(*) FROM face_jobs j JOIN photos p ON p.id=j.photo_id WHERE p.status='present' AND j.stage='detect' AND j.model_id='bench-det' AND j.status='done'",
        ),
        (
            "people: Zähler je Person",
            "SELECT p.id,(SELECT count(*) FROM faces f WHERE f.person_id=p.id AND f.assignment='user' AND f.retained=0) FROM persons p WHERE p.merged_into IS NULL",
        ),
        (
            "page(review): Teile",
            "WITH ranked AS (SELECT f.person_id,(row_number() OVER(PARTITION BY f.person_id ORDER BY f.id)-1)/28 part FROM faces f WHERE f.retained=0 AND f.assignment IN ('auto','suggested') AND f.ignored=0) SELECT person_id,part FROM ranked GROUP BY person_id,part ORDER BY count(*) DESC,person_id,part LIMIT 8 OFFSET 0",
        ),
        (
            "page(confirmed): Zeilen einer Person",
            "SELECT f.id FROM faces f JOIN (SELECT f.id,row_number() OVER(ORDER BY f.id) rn FROM faces f WHERE f.person_id IS 3 AND f.retained=0 AND f.assignment='user' AND f.ignored=0) r ON r.id=f.id WHERE r.rn>0 AND r.rn<=28 ORDER BY r.rn LIMIT 28",
        ),
        (
            "page(unknown): Gruppen",
            "SELECT group_id FROM face_group_catalog WHERE embedder_id='bench-emb' AND members>0 ORDER BY members DESC,group_id LIMIT 8 OFFSET 0",
        ),
        (
            "page(unknown): Gruppenmitglieder",
            "SELECT f.id FROM faces f JOIN face_group_members g ON g.face_id=f.id WHERE g.group_id=1 AND f.retained=0 AND f.ignored=0 AND f.person_id IS NULL ORDER BY f.id LIMIT 29",
        ),
        (
            "Trigger: Gruppenstatus",
            "SELECT group_id FROM face_group_members WHERE face_id=5",
        ),
        (
            "people_table: Statistik",
            "SELECT f.person_id,count(*),count(DISTINCT f.photo_id) FROM faces f LEFT JOIN metadata_photos m ON m.photo_id=f.photo_id WHERE f.person_id IS NOT NULL AND f.assignment='user' AND f.ignored=0 AND f.retained=0 GROUP BY f.person_id",
        ),
    ];
    for (label, sql) in queries {
        println!("-- {label}");
        let mut statement = c.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let rows = statement.query_map([], |r| r.get::<_, String>(3))?;
        for row in rows {
            println!("   {}", row?);
        }
    }
    Ok(())
}

/// Zeitmessung einzelner SQL-Varianten (Entwicklungswerkzeug).
fn probe(root: &Path) -> anyhow::Result<()> {
    let c = Collection::open(root, false)?;
    if let Ok(pragma) = std::env::var("PROBE_PRAGMA") {
        c.db.execute_batch(&pragma)?;
    }
    let base = "FROM faces f JOIN face_embeddings e ON e.face_id=f.id WHERE e.embedder_id='bench-emb' AND f.assignment='user' AND f.ignored=0 AND f.quality>=0.5";
    let ts = "(EXISTS(SELECT 1 FROM training_set t WHERE t.face_id=f.id AND t.embedder_id='bench-emb' AND t.split='ref') OR NOT EXISTS(SELECT 1 FROM training_set t WHERE t.person_id=f.person_id AND t.embedder_id='bench-emb'))";
    let rest = "AND NOT EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=e.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_review v WHERE v.face_id=f.id AND v.embedder_id=e.embedder_id)";
    for (label, sql) in [
        ("join only count", format!("SELECT count(*) {base}")),
        (
            "join + training",
            format!("SELECT count(*) {base} AND {ts}"),
        ),
        (
            "join + training + rest",
            format!("SELECT count(*) {base} AND {ts} {rest}"),
        ),
        (
            "join + training + rest + vec length",
            format!("SELECT sum(length(e.vec)) {base} AND {ts} {rest}"),
        ),
    ] {
        for round in 0..2 {
            let started = Instant::now();
            let n: i64 = c.db.query_row(&sql, [], |r| r.get(0))?;
            println!(
                "{label} #{round}: {n} rows {:.1} ms",
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gen") => generate(
            Path::new(args.get(1).map_or("tmp/bench-faces", String::as_str)),
            args.get(2).and_then(|v| v.parse().ok()).unwrap_or(90_000),
        ),
        Some("run") => run(
            Path::new(args.get(1).map_or("tmp/bench-faces", String::as_str)),
            Path::new(args.get(2).map_or("tmp/bench-faces-work", String::as_str)),
            args.get(3).and_then(|v| v.parse().ok()).unwrap_or(5),
        ),
        Some("probe") => probe(Path::new(
            args.get(1).map_or("tmp/bench-faces", String::as_str),
        )),
        Some("plans") => plans(Path::new(
            args.get(1).map_or("tmp/bench-faces", String::as_str),
        )),
        _ => {
            eprintln!("Aufruf: face_save_bench gen|run|plans <Ordner> ...");
            Ok(())
        }
    }
}
