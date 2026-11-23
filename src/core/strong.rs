//! Genaueres Vergleichsmodell (ArcFace, `config::STRONG_EMBEDDER`): ausdrücklicher Lauf
//! „Gesichter genauer berechnen“, Umschalten des aktiven Vergleichsmodells und Wachhalten des
//! Rechners während des Laufs (ADR Gesichtserkennung).
//!
//! Erkennung und Gesichter bleiben unverändert: Aus den gespeicherten Landmarks und derselben
//! Analysequelle (`previews::load` mit 1600 bzw. 3200 Pixeln wie die Analyse) entsteht je
//! Gesicht ein zusätzlicher Vektor unter dem Modellschlüssel des starken Modells. Stand ist
//! der Vektor selbst: Ein abgebrochener Lauf setzt bei den Gesichtern ohne Vektor fort.
use super::{
    collection::{Collection, safe_path_in},
    faces::{self, encode},
    inference::{self, Detection, Embedder, Manifest},
    jobs::Control,
    previews,
};
use anyhow::{Context, Result, bail, ensure};
use image::RgbImage;
use rusqlite::params;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// Schlüssel in `face_settings`: Modellschlüssel des Analyse-Embedders (SFace), auf den
/// „Zurück zu SFace“ umschaltet. Die Analyse schreibt ihn bei jedem Lauf.
pub const BASE_KEY: &str = "rust.base_embedder_key";

/// Gesichter, die einen Vektor des starken Modells erhalten: mit Namensmerkmalen
/// (Qualität ab 0,5) und Landmarks, außer „Kein Gesicht“.
const ELIGIBLE: &str = "f.quality>=0.5 AND f.landmarks IS NOT NULL AND f.ignored<>2";
/// Aktive Gesichter, nach denen sich die Abdeckung fürs Umschalten richtet.
const ACTIVE: &str = "f.quality>=0.5 AND f.landmarks IS NOT NULL AND f.ignored=0 AND f.retained=0";

/// Manifest des starken Modells im Modellordner, falls vorhanden.
pub fn manifest(model_dir: &Path) -> Option<Manifest> {
    inference::manifests(model_dir)
        .ok()?
        .into_iter()
        .find(|m| m.id == crate::config::STRONG_EMBEDDER && m.kind == "embedder")
}

/// Ist `key` ein Modellschlüssel des starken Modells?
pub fn is_strong(key: &str) -> bool {
    key.starts_with(&format!("{}@", crate::config::STRONG_EMBEDDER))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    /// Aktive Gesichter mit Namensmerkmalen.
    pub eligible: usize,
    /// Davon mit Vektor des starken Modells.
    pub embedded: usize,
    /// Fotos, in denen noch ein Gesicht (auch historisch oder ignoriert) ohne Vektor ist.
    pub open_photos: usize,
    /// Gesichter ohne Vektor (alle, die der Lauf berechnen würde).
    pub open_faces: usize,
}
impl Coverage {
    /// Anteil der aktiven Gesichter mit Vektor (1,0 ohne Gesichter).
    pub fn share(&self) -> f32 {
        if self.eligible == 0 {
            1.0
        } else {
            self.embedded as f32 / self.eligible as f32
        }
    }
    /// Umschalten ist erst möglich, wenn fast alle aktiven Gesichter den Vektor haben
    /// (`config::STRONG_SWITCH_COVERAGE`).
    pub fn ready(&self) -> bool {
        self.eligible > 0 && self.share() >= crate::config::STRONG_SWITCH_COVERAGE
    }
}

pub fn coverage(collection: &Collection, key: &str) -> Result<Coverage> {
    let (eligible, embedded): (usize, usize) = collection.db.query_row(
        &format!(
            "SELECT count(*),count(e.face_id) FROM faces f LEFT JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=? WHERE {ACTIVE}"
        ),
        [key],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (open_faces, open_photos): (usize, usize) = collection.db.query_row(
        &format!(
            "SELECT count(*),count(DISTINCT f.photo_id) FROM faces f WHERE {ELIGIBLE} AND NOT EXISTS(SELECT 1 FROM face_embeddings e WHERE e.face_id=f.id AND e.embedder_id=?)"
        ),
        [key],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(Coverage {
        eligible,
        embedded,
        open_photos,
        open_faces,
    })
}

#[derive(Clone, Debug)]
pub struct FaceJob {
    pub id: i64,
    /// Rahmen normiert auf das Analysebild (0..1).
    pub bbox: [f32; 4],
    /// Landmarks wie gespeichert (Text), für die Prüfung beim Schreiben.
    pub raw_landmarks: String,
    pub landmarks: [[f32; 2]; 5],
    pub score: f32,
}
#[derive(Clone, Debug)]
pub struct PhotoJob {
    pub photo: i64,
    pub relpath: String,
    pub orientation: u32,
    pub faces: Vec<FaceJob>,
}

/// Alle Gesichter ohne Vektor von `key`, je Analysequelle zusammengefasst, nach Pfad geordnet
/// (die Platte liest Ordner für Ordner).
pub fn pending(db: &rusqlite::Connection, key: &str) -> Result<Vec<PhotoJob>> {
    let mut statement = db.prepare(&format!(
        "SELECT f.id,f.photo_id,f.source_relpath,coalesce(m.orientation,1),f.bbox_x1,f.bbox_y1,f.bbox_x2,f.bbox_y2,f.landmarks,coalesce(f.det_score,0)
         FROM faces f LEFT JOIN meta m ON m.file_id=f.source_file_id
         WHERE {ELIGIBLE} AND NOT EXISTS(SELECT 1 FROM face_embeddings e WHERE e.face_id=f.id AND e.embedder_id=?)
         ORDER BY f.source_relpath,f.photo_id,f.id"
    ))?;
    let rows = statement.query_map([key], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, u32>(3)?,
            [
                r.get::<_, f32>(4)?,
                r.get::<_, f32>(5)?,
                r.get::<_, f32>(6)?,
                r.get::<_, f32>(7)?,
            ],
            r.get::<_, String>(8)?,
            r.get::<_, f32>(9)?,
        ))
    })?;
    let mut jobs: Vec<PhotoJob> = Vec::new();
    for row in rows {
        let (id, photo, relpath, orientation, bbox, raw, score) = row?;
        let Ok(landmarks) = serde_json::from_str::<[[f32; 2]; 5]>(&raw) else {
            continue;
        };
        let face = FaceJob {
            id,
            bbox,
            raw_landmarks: raw,
            landmarks,
            score,
        };
        match jobs.last_mut() {
            Some(job) if job.photo == photo && job.relpath == relpath => job.faces.push(face),
            _ => jobs.push(PhotoJob {
                photo,
                relpath,
                orientation,
                faces: vec![face],
            }),
        }
    }
    Ok(jobs)
}

/// Ausgerichtete Gesichter (112×112) eines Fotos aus derselben Quelle wie die Analyse: das auf
/// 1600 Pixel begrenzte Bild, für kleine, aus der 3200-Pixel-Fassung nachgerechnete Gesichter
/// (`config::FACE_HIRES_EDGE`) diese Fassung. Ein Gesicht, das sich nicht ausrichten lässt,
/// trägt seinen Fehler.
pub fn load_crops(
    path: &Path,
    job: &PhotoJob,
    exiftool: &str,
    control: &Control,
) -> Result<Vec<(i64, Result<RgbImage>)>> {
    let image = previews::load(path, exiftool, job.orientation, 1600, control)?.to_rgb8();
    let (width, height) = (image.width() as f32, image.height() as f32);
    let mut hires: Option<Option<(RgbImage, f32)>> = None;
    let mut crops = Vec::with_capacity(job.faces.len());
    for face in &job.faces {
        control.checkpoint()?;
        let detection = Detection {
            bbox: [
                face.bbox[0] * width,
                face.bbox[1] * height,
                face.bbox[2] * width,
                face.bbox[3] * height,
            ],
            landmarks: face.landmarks,
            score: face.score,
        };
        let crop = if detection.embedding_quality() >= crate::config::FACE_QUALITY_MIN {
            inference::align(&image, face.landmarks)
        } else {
            let larger = hires.get_or_insert_with(|| {
                previews::load(
                    path,
                    exiftool,
                    job.orientation,
                    crate::config::FACE_HIRES_EDGE,
                    control,
                )
                .ok()
                .map(|hi| {
                    let factor = hi.width() as f32 / width;
                    (hi.to_rgb8(), factor)
                })
                .filter(|(_, factor)| *factor > 1.05)
            });
            match larger {
                Some((hi, factor)) => {
                    inference::align(hi, face.landmarks.map(|p| p.map(|v| v * *factor)))
                }
                None => inference::align(&image, face.landmarks),
            }
        };
        crops.push((face.id, crop));
    }
    Ok(crops)
}

/// Hält Windows wach (kein Ruhezustand), solange der Wert lebt; nur für den ausdrücklichen
/// Lauf. Bildschirm darf ausgehen. Gilt für den Thread, der ihn anlegt.
pub struct KeepAwake(());
impl KeepAwake {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        #[cfg(windows)]
        set_execution_state(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        Self(())
    }
}
impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(windows)]
        set_execution_state(ES_CONTINUOUS);
    }
}
#[cfg(windows)]
const ES_CONTINUOUS: u32 = 0x8000_0000;
#[cfg(windows)]
const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
#[cfg(windows)]
fn set_execution_state(flags: u32) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetThreadExecutionState(flags: u32) -> u32;
    }
    // SAFETY: reine Win32-Funktion ohne Zeiger; ein Fehlschlag (0) ändert nichts am Lauf.
    let previous = unsafe { SetThreadExecutionState(flags) };
    if previous == 0 {
        log::warn!(target: "gesichter", "wachhalten_fehlgeschlagen flags={flags:#x}");
    }
}

type Loaded = (usize, Result<Vec<(i64, Result<RgbImage>)>>);
/// Ergebnis eines Fotos vor dem Schreiben: Index des Auftrags, Vektoren je Gesicht, Fehler.
type Written = (usize, Vec<(i64, Vec<f32>)>, Option<String>);

/// Berechnet den Vektor des starken Modells für alle Gesichter ohne ihn. Lädt die Fotos auf
/// `config::STRONG_LOAD_THREADS` Threads, rechnet auf einer Sitzung mit
/// `config::STRONG_THREADS` Threads und schreibt in Portionen. Abbrechbar und fortsetzbar.
pub fn compute(
    collection: &mut Collection,
    model_dir: &Path,
    dll: &Path,
    exiftool: &str,
    control: &Control,
) -> Result<String> {
    collection.writable()?;
    let _awake = KeepAwake::new();
    let mut embedder = Embedder::load(
        model_dir,
        dll,
        crate::config::STRONG_EMBEDDER,
        crate::config::STRONG_THREADS,
    )?;
    let key = embedder.manifest.key();
    compute_with(
        collection,
        exiftool,
        control,
        |crop| embedder.embed_aligned(crop),
        &key,
    )
}

/// Wie [`compute`] mit beliebigem Rechenweg (Tests nutzen einen Ersatz ohne Modelldatei).
pub fn compute_with(
    collection: &mut Collection,
    exiftool: &str,
    control: &Control,
    mut embed: impl FnMut(&RgbImage) -> Result<Vec<f32>>,
    key: &str,
) -> Result<String> {
    collection.writable()?;
    control.set_face_phase(super::jobs::FacePhase::Precise);
    control.phase("Gesichter genauer berechnen", "Fotos");
    let jobs = pending(&collection.db, key)?;
    let total = jobs.len();
    control.progress(0, total);
    let root = collection.root.clone();
    let next = Arc::new(AtomicUsize::new(0));
    let started = Instant::now();
    let (mut faces_done, mut faces_failed, mut photos_failed, mut done) = (0usize, 0, 0, 0);
    let mut pending_writes: Vec<Written> = Vec::new();
    let mut last_write = Instant::now();
    let flush = |collection: &mut Collection, writes: &mut Vec<Written>| -> Result<()> {
        if writes.is_empty() {
            return Ok(());
        }
        let tx = collection.db.transaction()?;
        for (index, vectors, error) in writes.drain(..) {
            let job = &jobs[index];
            for (id, vector) in &vectors {
                let Some(face) = job.faces.iter().find(|f| f.id == *id) else {
                    continue;
                };
                // Nur, wenn das Gesicht noch dieselben Landmarks hat (keine Neuanalyse dazwischen).
                tx.execute(
                    "INSERT OR REPLACE INTO face_embeddings SELECT ?1,?2,?3,?4 WHERE EXISTS(SELECT 1 FROM faces WHERE id=?1 AND landmarks=?5)",
                    params![id, key, vector.len() as i64, encode(vector), face.raw_landmarks],
                )?;
            }
            tx.execute(
                "INSERT OR REPLACE INTO face_jobs VALUES(?,'embed',?,?,?,datetime('now'),?)",
                params![
                    job.photo,
                    key,
                    if error.is_some() { "error" } else { "done" },
                    error,
                    job.relpath
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    };
    let result = std::thread::scope(|scope| -> Result<()> {
        let (sender, receiver) =
            mpsc::sync_channel::<Loaded>(crate::config::STRONG_LOAD_THREADS * 2);
        for _ in 0..crate::config::STRONG_LOAD_THREADS.max(1) {
            let sender = sender.clone();
            let (next, jobs, root, control) = (next.clone(), &jobs, &root, control.clone());
            scope.spawn(move || {
                loop {
                    if control.cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(index) else {
                        break;
                    };
                    let loaded = safe_path_in(root, &job.relpath)
                        .and_then(|path| load_crops(&path, job, exiftool, &control));
                    if sender.send((index, loaded)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        let outcome = (|| -> Result<()> {
            for (index, loaded) in receiver.iter() {
                control.checkpoint()?;
                let mut vectors = Vec::new();
                let error = match loaded {
                    Ok(crops) => {
                        let mut failed = 0;
                        for (id, crop) in crops {
                            match crop.and_then(|c| embed(&c)) {
                                Ok(vector) => vectors.push((id, vector)),
                                Err(_) => failed += 1,
                            }
                        }
                        faces_done += vectors.len();
                        faces_failed += failed;
                        (failed > 0).then(|| format!("{failed} Gesichter nicht berechenbar"))
                    }
                    Err(error) => {
                        if control.cancel.load(Ordering::Relaxed) {
                            bail!("Auftrag abgebrochen");
                        }
                        photos_failed += 1;
                        faces_failed += jobs[index].faces.len();
                        log::warn!(target: "gesichter", "genauer_fehlgeschlagen datei={:?} fehler={error:#}", jobs[index].relpath);
                        Some(format!("{error:#}"))
                    }
                };
                pending_writes.push((index, vectors, error));
                done += 1;
                if pending_writes.len() >= crate::config::STRONG_WRITE_PHOTOS
                    || last_write.elapsed() >= Duration::from_secs(5)
                {
                    flush(collection, &mut pending_writes)?;
                    last_write = Instant::now();
                }
                control.progress(done, total);
            }
            Ok(())
        })();
        if outcome.is_err() {
            control.stop();
        }
        // Schon Berechnetes bleibt auch bei Abbruch erhalten.
        let written = flush(collection, &mut pending_writes);
        drop(receiver);
        outcome.and(written)
    });
    if faces::setting(collection, faces::EMBEDDER_KEY)?.as_deref() == Some(key) && faces_done > 0 {
        // Aktives Modell: neue Vektoren in Gruppen und Abgleich aufnehmen.
        collection.db.execute("INSERT INTO face_maintenance VALUES(?,0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[key])?;
    }
    log::info!(target: "gesichter", "genauer_berechnet fotos={done} gesichter={faces_done} fehler={faces_failed} sekunden={:.0}", started.elapsed().as_secs_f64());
    result?;
    Ok(format!(
        "{done} Fotos · {faces_done} Gesichter genauer berechnet{}",
        if faces_failed > 0 {
            format!(
                " · {faces_failed} Gesichter nicht berechenbar ({photos_failed} Fotos nicht lesbar)"
            )
        } else {
            String::new()
        }
    ))
}

/// Schaltet das aktive Vergleichsmodell auf das starke Modell um. Setzt voraus, dass fast alle
/// Gesichter seinen Vektor haben ([`Coverage::ready`]). Vorher entsteht eine Sicherung.
/// Danach werden Referenzen, Abgleich und Gruppen für das Modell neu gebildet; Bestätigungen,
/// Ablehnungen, Ignorieren, Pins und Ausschlüsse hängen an Gesichtern und bleiben.
pub fn activate(
    collection: &mut Collection,
    model_dir: &Path,
    control: &Control,
) -> Result<String> {
    collection.writable()?;
    let key = manifest(model_dir)
        .context("Das genauere Modell ist nicht eingerichtet")?
        .key();
    ensure!(
        coverage(collection, &key)?.ready(),
        "Zuerst „Gesichter genauer berechnen“ abschließen"
    );
    switch(collection, &key, true, control)
}

/// Schaltet zurück auf das Analysemodell (SFace). Jederzeit möglich.
pub fn deactivate(collection: &mut Collection, control: &Control) -> Result<String> {
    collection.writable()?;
    let base = faces::setting(collection, BASE_KEY)?
        .context("Kein Analysemodell bekannt; zuerst Fotos analysieren")?;
    switch(collection, &base, false, control)
}

fn switch(
    collection: &mut Collection,
    key: &str,
    backup: bool,
    control: &Control,
) -> Result<String> {
    let current = faces::setting(collection, faces::EMBEDDER_KEY)?;
    if current.as_deref() != Some(key) {
        if backup {
            control.phase("Sicherung vor dem Modellwechsel", "");
            crate::core::backup::automatic(
                &collection.root,
                "Wechsel des Vergleichsmodells",
                control,
            )
            .context("Modell nicht umgeschaltet")?;
        }
        if let Some(current) = &current
            && faces::setting(collection, BASE_KEY)?.is_none()
            && !is_strong(current)
        {
            faces::save_setting(&collection.db, BASE_KEY, current)?;
        }
        let tx = collection.db.transaction()?;
        if let Some(current) = &current {
            // Pins und Ausschlüsse sind Nutzerentscheidungen über Gesichter: Sie gelten im neuen
            // Modell weiter (je Modell gespeichert, beim Umschalten übernommen).
            tx.execute(
                "INSERT OR IGNORE INTO training_set SELECT person_id,face_id,?1,'ref',datetime('now'),'manual_pin',1 FROM training_set WHERE embedder_id=?2 AND pinned=1",
                params![key, current],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO training_exclusions SELECT face_id,?1 FROM training_exclusions WHERE embedder_id=?2",
                params![key, current],
            )?;
        }
        faces::save_setting(&tx, faces::EMBEDDER_KEY, key)?;
        // Alle Personen mit Bestätigungen wählen für dieses Modell neu aus.
        tx.execute(
            "INSERT OR IGNORE INTO face_reference_dirty SELECT DISTINCT ?,person_id FROM faces WHERE assignment='user' AND person_id IS NOT NULL",
            [key],
        )?;
        tx.execute("INSERT INTO face_maintenance VALUES(?,0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[key])?;
        tx.commit()?;
    }
    super::recognition::apply_knowledge(collection, control)?;
    Ok(if is_strong(key) {
        "Genaueres Modell aktiv; Vorschläge neu berechnet".into()
    } else {
        "SFace aktiv; Vorschläge neu berechnet".into()
    })
}

/// Lädt das starke Modell für die Analyse, wenn es aktiv ist (neue Gesichter erhalten seinen
/// Vektor gleich mit). Fehlt es, rechnet die Analyse ohne; der nächste ausdrückliche Lauf
/// ergänzt die Vektoren.
pub fn analysis_extra(
    collection: &Collection,
    model_dir: &Path,
    dll: &Path,
) -> Result<Option<Embedder>> {
    let Some(active) = faces::setting(collection, faces::EMBEDDER_KEY)? else {
        return Ok(None);
    };
    if !is_strong(&active) {
        return Ok(None);
    }
    match Embedder::load(model_dir, dll, crate::config::STRONG_EMBEDDER, 2) {
        Ok(embedder) if embedder.manifest.key() == active => Ok(Some(embedder)),
        Ok(_) => Ok(None),
        Err(error) => {
            log::warn!(target: "gesichter", "genaueres_modell_fehlt fehler={error:#}");
            Ok(None)
        }
    }
}

/// Liegen Manifest und Gewicht des starken Modells im Modellordner? (Billige Prüfung für die
/// Oberfläche; der Lauf prüft den Hash.)
pub fn weights_present(model_dir: &Path) -> bool {
    manifest(model_dir).is_some_and(|m| model_dir.join(&m.file).is_file())
}
