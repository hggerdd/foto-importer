//! Gesichtsanalyse auf Workern: Erkennen, Einbetten, Nachbessern kleiner Gesichter,
//! Vektor-Kodierung und Gruppenvorbereitung.
use super::*;

pub fn analyze(
    collection: &mut Collection,
    model_dir: &Path,
    dll: &Path,
    detector: &str,
    embedder: &str,
    exiftool: &str,
    control: &Control,
) -> Result<String> {
    analyze_scoped(
        collection,
        model_dir,
        dll,
        detector,
        embedder,
        exiftool,
        control,
        &Scope::default(),
    )
}
/// Analyse neuester Fotos zuerst, optional nur eine Portion oder ein Ordner (ADR 0059).
#[allow(clippy::too_many_arguments)]
pub fn analyze_scoped(
    collection: &mut Collection,
    model_dir: &Path,
    dll: &Path,
    detector: &str,
    embedder: &str,
    exiftool: &str,
    control: &Control,
    scope: &Scope,
) -> Result<String> {
    collection.writable()?;
    control.set_face_phase(crate::core::jobs::FacePhase::Analysis);
    let mut models = Models::load(model_dir, dll, detector, embedder)?;
    let detector_key = inference::presence_detector_key(&models.detector.key());
    let embedder_key = models.embedder.key();
    // Ein Detektorwechsel verändert alle späteren Erkennungen: vorher sichern, sonst unterbleibt
    // die Analyse.
    if setting(collection, "rust.detector_key")?.is_some_and(|key| key != detector_key) {
        crate::core::backup::automatic(&collection.root, "Detektorwechsel", control)
            .context("Analyse nicht gestartet")?;
        control.set_face_phase(crate::core::jobs::FacePhase::Analysis);
    }
    collection.db.execute(
        "INSERT OR IGNORE INTO face_settings VALUES(?,?)",
        params![
            format!("rust.thresholds.{embedder_key}.duplicate"),
            models.embedder.thresholds.duplicate.to_string()
        ],
    )?;
    save_setting(&collection.db, "rust.detector_key", &detector_key)?;
    save_setting(&collection.db, EMBEDDER_KEY, &embedder_key)?;
    save_setting(&collection.db, "active_detector", detector)?;
    save_setting(&collection.db, "active_embedder", embedder)?;
    let order = crate::core::scope::analysis_order(collection, scope)?;
    let total = scope.limit.map_or(order.len(), |l| l.min(order.len()));
    let (mut position, mut done, mut worked) = (0usize, 0usize, 0usize);
    let (mut found, mut failed, mut without_vector) = (0, 0, 0);
    let (mut missing, mut missing_example) = (0, None::<String>);
    let mut portion_full = false;
    loop {
        control.checkpoint()?;
        let ids = &order[position..(position + 32).min(order.len())];
        if ids.is_empty() || portion_full {
            break;
        }
        position += ids.len();
        let rows = analysis_rows(collection, ids)?;
        for (photo, file, relative, size, mtime, orientation, date) in rows {
            control.checkpoint()?;
            let fingerprint = format!("rust-v1:{file}:{size}:{mtime}:{orientation}:{embedder_key}");
            let complete:bool=collection.db.query_row("SELECT EXISTS(SELECT 1 FROM face_jobs WHERE photo_id=? AND stage='detect' AND model_id=? AND status='done' AND fingerprint=?)",params![photo,detector_key,fingerprint],|r|r.get(0))?;
            if !complete && scope.limit.is_some_and(|l| worked >= l) {
                portion_full = true;
                break;
            }
            if !complete {
                worked += 1;
            }
            if !complete {
                let result = (|| -> Result<(usize, usize)> {
                    let image = previews::load(
                        &collection.safe_path(&relative)?,
                        exiftool,
                        orientation,
                        1600,
                        control,
                    )?;
                    let detections = models.detect_with_control(&image, control)?;
                    let mut embedded = Vec::with_capacity(detections.len());
                    let mut embedding_errors = 0;
                    for detection in &detections {
                        control.checkpoint()?;
                        let quality = detection.embedding_quality();
                        if quality < crate::config::FACE_QUALITY_MIN {
                            embedded.push((quality, None));
                            continue;
                        }
                        match models.embed(&image, detection) {
                            Ok(vector) => embedded.push((quality, Some(vector))),
                            Err(_) => {
                                embedding_errors += 1;
                                embedded.push((0.0, None));
                            }
                        }
                    }
                    let small: Vec<usize> = detections
                        .iter()
                        .enumerate()
                        .filter(|(i, d)| embedded[*i].1.is_none() && is_small_but_clear(d))
                        .map(|(i, _)| i)
                        .collect();
                    let hires_tried = !small.is_empty();
                    if hires_tried
                        && let Ok(hi) = previews::load(
                            &collection.safe_path(&relative)?,
                            exiftool,
                            orientation,
                            crate::config::FACE_HIRES_EDGE,
                            control,
                        )
                    {
                        let factor = hi.width() as f32 / image.width() as f32;
                        if factor > 1.05 {
                            for i in small {
                                control.checkpoint()?;
                                if let Some(found) =
                                    embed_hires(&mut models, &hi, &detections[i], factor)
                                {
                                    embedded[i] = (found.0, Some(found.1));
                                }
                            }
                        }
                    }
                    let old=collection.db.prepare(&format!("SELECT {FACE_COLUMNS} FROM faces f WHERE f.photo_id=? ORDER BY f.assignment='user' DESC,f.retained,f.id"))?.query_map([photo],face_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
                    let tx = collection.db.transaction()?;
                    let mut used = BTreeSet::new();
                    for (detection, (quality, vector)) in detections.iter().zip(&embedded) {
                        let bbox = [
                            detection.bbox[0] / image.width() as f32,
                            detection.bbox[1] / image.height() as f32,
                            detection.bbox[2] / image.width() as f32,
                            detection.bbox[3] / image.height() as f32,
                        ];
                        let previous = old
                            .iter()
                            .filter(|f| f.source == relative && !used.contains(&f.id))
                            .map(|f| (inference::iou(f.bbox, bbox), f))
                            .max_by(|a, b| a.0.total_cmp(&b.0))
                            .filter(|x| x.0 >= 0.5)
                            .map(|x| x.1);
                        let landmarks = serde_json::to_string(&detection.landmarks)?;
                        let id = if let Some(previous) = previous {
                            used.insert(previous.id);
                            tx.execute("UPDATE faces SET detector_id=?,bbox_x1=?,bbox_y1=?,bbox_x2=?,bbox_y2=?,landmarks=?,det_score=?,quality=?,retained=0,source_fingerprint=? WHERE id=?",params![detector_key,bbox[0],bbox[1],bbox[2],bbox[3],landmarks,detection.score,quality,fingerprint,previous.id])?;
                            previous.id
                        } else {
                            tx.execute("INSERT INTO faces(photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,captured_at,source_fingerprint) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",params![photo,file,relative,detector_key,bbox[0],bbox[1],bbox[2],bbox[3],landmarks,detection.score,quality,date,fingerprint])?;
                            tx.last_insert_rowid()
                        };
                        if let Some(vector) = vector {
                            tx.execute(
                                "INSERT OR REPLACE INTO face_embeddings VALUES(?,?,?,?)",
                                params![id, embedder_key, vector.len() as i64, encode(vector)],
                            )?;
                        } else {
                            // A weaker detection may retain an explicit label or pin, never an old suggestion.
                            tx.execute(
                                "UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL,assigned_at=NULL WHERE id=? AND assignment IN ('auto','suggested')",
                                [id],
                            )?;
                        }
                    }
                    for face in old.iter().filter(|f| !used.contains(&f.id)) {
                        if face.assignment.as_deref() == Some("user") || face.ignored {
                            tx.execute("UPDATE faces SET retained=1 WHERE id=?", [face.id])?;
                        } else {
                            delete_face_rows(&tx, face.id)?;
                        }
                    }
                    tx.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'detect',?,'done',NULL,datetime('now'),?)",params![photo,detector_key,fingerprint])?;
                    if hires_tried {
                        tx.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'hires',?,'done',NULL,datetime('now'),?)",params![photo,embedder_key,fingerprint])?;
                    }
                    let embed_status = if embedding_errors == 0 {
                        "done"
                    } else {
                        "error"
                    };
                    let embed_error = (embedding_errors > 0).then(|| {
                        format!("{embedding_errors} Gesichter ohne berechenbare Namensmerkmale")
                    });
                    tx.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'embed',?,?,?,datetime('now'),?)",params![photo,embedder_key,embed_status,embed_error,fingerprint])?;
                    control.checkpoint()?;
                    tx.commit()?;
                    Ok((
                        detections.len(),
                        embedded
                            .iter()
                            .filter(|(_, vector)| vector.is_none())
                            .count(),
                    ))
                })();
                match result {
                    Ok((n, presence_only)) => {
                        found += n;
                        without_vector += presence_only;
                    }
                    Err(e) if crate::core::collection::is_not_found(&e) => {
                        // Datei fehlt: kein Analysefehler, sondern ein Fall für den Bestand.
                        control.checkpoint()?;
                        missing +=
                            collection.mark_missing(&[(file, relative.clone())], "gesichter")?;
                        missing_example.get_or_insert_with(|| relative.clone());
                    }
                    Err(e) => {
                        control.checkpoint()?;
                        failed += 1;
                        collection.db.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'detect',?,'error',?,datetime('now'),?)",params![photo,detector_key,e.to_string(),fingerprint])?;
                    }
                }
            }
            if complete {
                // Eine fehlgeschlagene Nachberechnung bricht die Analyse nicht ab, wird aber
                // geloggt (ein Abbruch meldet sich gleich danach selbst).
                if let Err(error) = upgrade_small_faces(
                    collection,
                    &mut models,
                    (photo, &relative, orientation),
                    (&embedder_key, &fingerprint),
                    exiftool,
                    control,
                ) && !control.cancel.load(std::sync::atomic::Ordering::Relaxed)
                {
                    log::warn!(
                        target: "gesichter",
                        "nachberechnung_fehlgeschlagen foto={photo} datei={relative:?} fehler={error:#}"
                    );
                }
                control.checkpoint()?;
            }
            done += 1;
            control.progress(if scope.limit.is_some() { worked } else { done }, total);
        }
        prepare_groups(collection, &embedder_key, control)?;
    }
    crate::core::grouping::rebuild_groups(collection, &embedder_key, control)?;
    if let Some(example) = missing_example {
        log::warn!(target: "gesichter", "dateien_fehlen anzahl={missing} beispiel={example:?}");
    }
    Ok(format!(
        "{done} Fotos geprüft, {worked} davon neu analysiert · {found} Gesichter erkannt · {without_vector} ohne Namensmerkmale · {failed} Analysefehler{}",
        if missing > 0 {
            format!(" · {missing} Dateien nicht gefunden, als fehlend markiert")
        } else {
            String::new()
        }
    ))
}
type AnalysisRow = (i64, i64, String, i64, i64, u32, String);
/// Einzelheiten der Fotos mit diesen IDs, in der Reihenfolge von `ids`.
fn analysis_rows(collection: &Collection, ids: &[i64]) -> Result<Vec<AnalysisRow>> {
    let marks = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT f.photo_id,f.id,f.relpath,f.size,f.mtime_ns,coalesce(m.orientation,1),coalesce(m.dt_original,'') FROM face_sources s JOIN files f ON f.id=s.file_id LEFT JOIN meta m ON m.file_id=f.id WHERE f.photo_id IN ({marks}) AND f.status='present'"
    );
    let mut rows: Vec<AnalysisRow> = collection
        .db
        .prepare(&sql)?
        .query_map(rusqlite::params_from_iter(ids.iter()), |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let rank: BTreeMap<i64, usize> = ids.iter().enumerate().map(|(i, p)| (*p, i)).collect();
    rows.sort_by_key(|r| rank.get(&r.0).copied().unwrap_or(usize::MAX));
    Ok(rows)
}

fn is_small_but_clear(detection: &inference::Detection) -> bool {
    detection.score >= 0.8 && detection.embedding_quality() < crate::config::FACE_QUALITY_MIN
}
/// Größe und Namensmerkmale aus dem höher aufgelösten Bild; `None`, wenn das
/// Gesicht auch dort zu klein oder nicht berechenbar ist.
fn embed_hires(
    models: &mut Models,
    hi: &image::DynamicImage,
    detection: &inference::Detection,
    factor: f32,
) -> Option<(f32, Vec<f32>)> {
    let scaled = detection.scaled(factor);
    let quality = scaled.embedding_quality();
    if quality < crate::config::FACE_QUALITY_MIN {
        return None;
    }
    models
        .embed(hi, &scaled)
        .ok()
        .map(|vector| (quality, vector))
}
/// Bereits analysierte Fotos nachrechnen: kleine, klar erkannte Gesichter ohne
/// Namensmerkmale erhalten sie aus dem höher aufgelösten Bild. Pro Foto und
/// Analysestand höchstens einmal.
fn upgrade_small_faces(
    collection: &mut Collection,
    models: &mut Models,
    (photo, relative, orientation): (i64, &str, u32),
    (embedder_key, fingerprint): (&str, &str),
    exiftool: &str,
    control: &Control,
) -> Result<()> {
    let tried: bool = collection.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM face_jobs WHERE photo_id=? AND stage='hires' AND model_id=? AND fingerprint=?)",
        params![photo, embedder_key, fingerprint],
        |r| r.get(0),
    )?;
    if tried {
        return Ok(());
    }
    let candidates: Vec<(i64, f32, String, [f32; 4])> = collection
        .db
        .prepare(
            "SELECT f.id,f.det_score,f.landmarks,f.bbox_x1,f.bbox_y1,f.bbox_x2,f.bbox_y2 FROM faces f
             WHERE f.photo_id=? AND f.retained=0 AND f.det_score>=0.8 AND f.quality<0.5
               AND f.landmarks IS NOT NULL
               AND NOT EXISTS(SELECT 1 FROM face_embeddings e WHERE e.face_id=f.id AND e.embedder_id=?)",
        )?
        .query_map(params![photo, embedder_key], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                [r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?],
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    if candidates.is_empty() {
        return Ok(());
    }
    let hi = previews::load(
        &collection.safe_path(relative)?,
        exiftool,
        orientation,
        crate::config::FACE_HIRES_EDGE,
        control,
    )?;
    let long = hi.width().max(hi.height()) as f32;
    // Gespeicherte Punkte stammen aus dem auf 1600 begrenzten Analysebild.
    let factor = long / long.min(1600.0);
    let (width, height) = (hi.width() as f32, hi.height() as f32);
    let mut upgraded = Vec::new();
    if factor > 1.05 {
        for (id, score, landmarks, bbox) in candidates {
            control.checkpoint()?;
            let Ok(landmarks) = serde_json::from_str::<[[f32; 2]; 5]>(&landmarks) else {
                continue;
            };
            let detection = inference::Detection {
                bbox: [
                    bbox[0] * width,
                    bbox[1] * height,
                    bbox[2] * width,
                    bbox[3] * height,
                ],
                landmarks: landmarks.map(|p| p.map(|v| v * factor)),
                score,
            };
            if let Some((quality, vector)) = embed_hires(models, &hi, &detection, 1.0) {
                upgraded.push((id, quality, vector));
            }
        }
    }
    let tx = collection.db.transaction()?;
    for (id, quality, vector) in &upgraded {
        tx.execute(
            "UPDATE faces SET quality=? WHERE id=?",
            params![quality, id],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO face_embeddings VALUES(?,?,?,?)",
            params![id, embedder_key, vector.len() as i64, encode(vector)],
        )?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO face_jobs VALUES(?,'hires',?,'done',NULL,datetime('now'),?)",
        params![photo, embedder_key, fingerprint],
    )?;
    tx.commit()?;
    Ok(())
}
pub fn encode(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}
pub fn decode(bytes: &[u8]) -> Result<Vec<f32>> {
    if !bytes.len().is_multiple_of(4) {
        bail!("Ungültige Vektorlänge");
    }
    let vector: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    inference::normalize(&vector)
}
fn signature(vector: &[f32]) -> i64 {
    vector
        .iter()
        .take(12)
        .enumerate()
        .fold(0, |bits, (i, v)| bits | (((*v >= 0.0) as i64) << i))
}

pub fn prepare_groups(collection: &mut Collection, key: &str, control: &Control) -> Result<usize> {
    collection.writable()?;
    let rows=collection.db.prepare("SELECT f.id,f.photo_id,e.vec FROM face_group_dirty d JOIN faces f ON f.id=d.face_id JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=d.embedder_id WHERE d.embedder_id=? ORDER BY f.id LIMIT 128")?.query_map([key],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let tx = collection.db.transaction()?;
    for (id, photo, bytes) in &rows {
        control.checkpoint()?;
        let eligible:bool=tx.query_row("SELECT person_id IS NULL AND ignored=0 AND retained=0 AND quality>=0.5 FROM faces WHERE id=?",[id],|r|r.get(0))?;
        if eligible {
            let vector = decode(bytes)?;
            let sig = signature(&vector);
            let candidates=tx.prepare("SELECT group_id FROM face_group_catalog WHERE embedder_id=? AND members<? AND (signature=? OR signature IN (?,?,?,?,?,?,?,?,?,?,?,?)) ORDER BY group_id LIMIT 32")?
                .query_map(params![key,crate::config::GROUP_MAX as i64,sig,sig^1,sig^2,sig^4,sig^8,sig^16,sig^32,sig^64,sig^128,sig^256,sig^512,sig^1024,sig^2048],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut matched = None;
            for group in candidates {
                let members=tx.prepare("SELECT f.photo_id,e.vec FROM face_group_members g JOIN faces f ON f.id=g.face_id JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=g.embedder_id WHERE g.group_id=?")?.query_map([group],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Vec<u8>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                if members.iter().all(|(p, v)| {
                    p != photo
                        && decode(v)
                            .ok()
                            .and_then(|v| inference::cosine(&vector, &v))
                            .is_some_and(|s| s >= crate::config::GROUP_JOIN_MIN)
                }) {
                    matched = Some(group);
                    break;
                }
            }
            let group = if let Some(group) = matched {
                group
            } else {
                tx.execute("INSERT INTO face_group_catalog(embedder_id,signature,representative,members) VALUES(?,?,?,0)",params![key,sig,bytes])?;
                tx.last_insert_rowid()
            };
            tx.execute(
                "INSERT OR IGNORE INTO face_group_members VALUES(?,?,?)",
                params![key, id, group],
            )?;
            tx.execute("UPDATE face_group_catalog SET members=(SELECT count(*) FROM face_group_members WHERE group_id=?) WHERE group_id=?",params![group,group])?;
        }
        tx.execute(
            "DELETE FROM face_group_dirty WHERE embedder_id=? AND face_id=?",
            params![key, id],
        )?;
    }
    tx.commit()?;
    Ok(rows.len())
}
