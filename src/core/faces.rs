pub use super::recognition::{apply_knowledge, rematch, suggestions};
use super::{
    collection::Collection,
    inference::{self, Models},
    jobs::Control,
    previews,
};
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Debug)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub birth: String,
    pub count: usize,
    pub gender: String,
    pub note: String,
}
#[derive(Clone, Debug)]
pub struct Face {
    pub id: i64,
    pub photo: i64,
    pub source: String,
    pub bbox: [f32; 4],
    pub person: Option<i64>,
    pub assignment: Option<String>,
    pub ignored: bool,
    pub score: f32,
}
#[derive(Clone, Default)]
pub struct Status {
    pub total: usize,
    pub confirmed: usize,
    pub unknown: usize,
    pub review: usize,
    pub ignored: usize,
    pub references: usize,
    pub reference_people: usize,
    pub dirty_people: usize,
    pub analysed_photos: usize,
    pub present_photos: usize,
    pub classifier: Option<super::classifier::Metrics>,
    pub nameable: usize,
    pub pending: usize,
    pub applied: String,
    pub detector: String,
    pub embedder: String,
    pub auto_import: bool,
    pub nightly: bool,
    pub detector_id: String,
    pub embedder_id: String,
}
#[derive(Default)]
pub struct FacePage {
    pub groups: Vec<Vec<Face>>,
    pub people: Vec<Person>,
    pub status: Status,
}

pub fn setting(collection: &Collection, key: &str) -> Result<Option<String>> {
    let raw: Option<String> = collection
        .db
        .query_row("SELECT value FROM face_settings WHERE key=?", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(raw.and_then(|v| serde_json::from_str::<String>(&v).ok()))
}
pub(crate) fn save_setting(db: &rusqlite::Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}
pub fn people(collection: &Collection) -> Result<Vec<Person>> {
    Ok(collection.db.prepare("SELECT p.id,coalesce(a.value,'Unbenannt'),coalesce(b.value,''),(SELECT count(*) FROM faces f WHERE f.person_id=p.id AND f.assignment='user' AND f.retained=0),coalesce((SELECT value FROM person_attrs WHERE person_id=p.id AND key='gender'),''),coalesce((SELECT value FROM person_attrs WHERE person_id=p.id AND key='note'),'') FROM persons p LEFT JOIN person_attrs a ON a.person_id=p.id AND a.key='name' LEFT JOIN person_attrs b ON b.person_id=p.id AND b.key='birth_year' WHERE p.merged_into IS NULL ORDER BY a.value,p.id")?
        .query_map([],|r|Ok(Person{id:r.get(0)?,name:r.get(1)?,birth:r.get(2)?,count:r.get(3)?,gender:r.get(4)?,note:r.get(5)?}))?.collect::<rusqlite::Result<_>>()?)
}
pub fn configure_automatic(
    collection: &mut Collection,
    auto_import: bool,
    nightly: bool,
) -> Result<()> {
    collection.writable()?;
    let tx = collection.db.transaction()?;
    for (key, value) in [("auto_import", auto_import), ("nightly", nightly)] {
        tx.execute(
            "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
            params![key, value.to_string()],
        )?;
    }
    tx.commit()?;
    Ok(())
}
pub fn configure_models(
    collection: &mut Collection,
    folder: &Path,
    detector: &str,
    embedder: &str,
) -> Result<()> {
    collection.writable()?;
    let manifests = inference::manifests(folder)?;
    if !manifests
        .iter()
        .any(|m| m.id == detector && m.kind == "detector")
        || !manifests
            .iter()
            .any(|m| m.id == embedder && m.kind == "embedder")
    {
        bail!("Modellauswahl ist im gewählten Modellordner nicht verfügbar");
    }
    let tx = collection.db.transaction()?;
    save_setting(&tx, "active_detector", detector)?;
    save_setting(&tx, "active_embedder", embedder)?;
    tx.commit()?;
    Ok(())
}
pub fn status(collection: &Collection) -> Result<Status> {
    let mut status=collection.db.query_row("SELECT count(*),coalesce(sum(assignment='user'),0),coalesce(sum(person_id IS NULL AND ignored=0),0),coalesce(sum(assignment IN ('auto','suggested') AND ignored=0),0),coalesce(sum(ignored=1),0) FROM faces WHERE retained=0",[],|r|Ok(Status{total:r.get(0)?,confirmed:r.get(1)?,unknown:r.get(2)?,review:r.get(3)?,ignored:r.get(4)?,..Default::default()}))?;
    status.detector = active_detector_key(collection)?.unwrap_or_default();
    status.embedder = setting(collection, "rust.embedder_key")?.unwrap_or_default();
    status.detector_id = setting(collection, "active_detector")?.unwrap_or_else(|| "yunet".into());
    status.embedder_id = setting(collection, "active_embedder")?.unwrap_or_else(|| "sface".into());
    for (key, target) in [
        ("auto_import", &mut status.auto_import),
        ("nightly", &mut status.nightly),
    ] {
        let raw: Option<String> = collection
            .db
            .query_row("SELECT value FROM face_settings WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?;
        *target = raw
            .and_then(|v| serde_json::from_str::<bool>(&v).ok())
            .unwrap_or(false);
    }
    (status.references, status.reference_people) = collection.db.query_row(
        "SELECT count(*),count(DISTINCT person_id) FROM training_set WHERE embedder_id=? AND split='ref'",
        [&status.embedder],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    status.dirty_people = collection.db.query_row(
        "SELECT count(*) FROM face_reference_dirty WHERE embedder_id=?",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.nameable = collection.db.query_row(
        "SELECT coalesce(sum(members),0) FROM face_group_catalog WHERE embedder_id=?",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.analysed_photos = collection.db.query_row(
        "SELECT count(*) FROM face_jobs j JOIN photos p ON p.id=j.photo_id WHERE p.status='present' AND j.stage='detect' AND j.model_id=? AND j.status='done'",
        [&status.detector], |r| r.get(0),
    )?;
    status.present_photos = collection.db.query_row(
        "SELECT count(*) FROM photos WHERE status='present'",
        [],
        |r| r.get(0),
    )?;
    status.classifier =
        super::classifier::load(collection, &status.embedder)?.and_then(|m| m.metrics);
    status.pending = collection.db.query_row(
        "SELECT count(*) FROM face_maintenance WHERE embedder_id=?",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.applied = setting(
        collection,
        &super::recognition::applied_key(&status.embedder),
    )?
    .unwrap_or_default();
    Ok(status)
}
fn face_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Face> {
    Ok(Face {
        id: r.get(0)?,
        photo: r.get(1)?,
        source: r.get(2)?,
        bbox: [r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?],
        person: r.get(7)?,
        assignment: r.get(8)?,
        ignored: r.get(9)?,
        score: r.get::<_, Option<f32>>(10)?.unwrap_or_default(),
    })
}
const FACE_COLUMNS: &str = "f.id,f.photo_id,f.source_relpath,f.bbox_x1,f.bbox_y1,f.bbox_x2,f.bbox_y2,f.person_id,f.assignment,f.ignored,f.confidence";
pub fn photo_faces(collection: &Collection, photo: i64) -> Result<Vec<Face>> {
    Ok(collection
        .db
        .prepare(&format!(
            "SELECT {FACE_COLUMNS} FROM faces f WHERE f.photo_id=? AND f.retained=0 ORDER BY f.id"
        ))?
        .query_map([photo], face_row)?
        .collect::<rusqlite::Result<_>>()?)
}
pub fn photo_analysis_done(collection: &Collection, photo: i64) -> Result<bool> {
    let Some(detector) = active_detector_key(collection)? else {
        return Ok(false);
    };
    Ok(collection.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM face_jobs WHERE photo_id=? AND stage='detect' AND model_id=? AND status='done')",
        params![photo, detector],
        |row| row.get(0),
    )?)
}
pub fn active_detector_key(collection: &Collection) -> Result<Option<String>> {
    Ok(setting(collection, "rust.detector_key")?.map(|key| inference::presence_detector_key(&key)))
}
pub fn page(collection: &Collection, mode: &str, page: usize) -> Result<FacePage> {
    let status = status(collection)?;
    let mut groups = Vec::new();
    if mode == "unknown" && status.embedder.is_empty() {
        return Ok(FacePage {
            groups,
            people: people(collection)?,
            status,
        });
    }
    if mode == "unknown" && !status.embedder.is_empty() {
        let ids=collection.db.prepare("SELECT group_id FROM face_group_catalog WHERE embedder_id=? AND members>0 ORDER BY members DESC,group_id LIMIT 8 OFFSET ?")?.query_map(params![status.embedder,(page*8) as i64],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            let group=collection.db.prepare(&format!("SELECT {FACE_COLUMNS} FROM faces f JOIN face_group_members g ON g.face_id=f.id WHERE g.group_id=? AND f.retained=0 AND f.ignored=0 AND f.person_id IS NULL ORDER BY f.id LIMIT 15"))?.query_map([id],face_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            if !group.is_empty() && group.len() <= crate::config::GROUP_MAX {
                groups.push(group);
            }
        }
    } else {
        let predicate = match mode {
            "review" => "f.assignment IN ('auto','suggested') AND f.ignored=0",
            "ignored" => "f.ignored=1",
            "confirmed" => "f.assignment='user' AND f.ignored=0",
            _ => "f.person_id IS NULL AND f.ignored=0",
        };
        let parts=collection.db.prepare(&format!("WITH ranked AS (SELECT f.person_id,(row_number() OVER(PARTITION BY f.person_id ORDER BY f.id)-1)/14 part FROM faces f WHERE f.retained=0 AND {predicate}) SELECT person_id,part FROM ranked GROUP BY person_id,part ORDER BY count(*) DESC,person_id,part LIMIT 8 OFFSET ?"))?.query_map([(page*8) as i64],|r|Ok((r.get::<_,Option<i64>>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for (person, part) in parts {
            let rows=collection.db.prepare(&format!("SELECT {FACE_COLUMNS} FROM faces f JOIN (SELECT f.id,row_number() OVER(ORDER BY f.id) rn FROM faces f WHERE f.person_id IS ? AND f.retained=0 AND {predicate}) r ON r.id=f.id WHERE r.rn>? AND r.rn<=? ORDER BY r.rn LIMIT 14"))?.query_map(params![person,part*14,(part+1)*14],face_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            if !rows.is_empty() {
                groups.push(rows);
            }
        }
    }
    Ok(FacePage {
        groups,
        people: people(collection)?,
        status,
    })
}

pub fn analyze(
    collection: &mut Collection,
    model_dir: &Path,
    dll: &Path,
    detector: &str,
    embedder: &str,
    exiftool: &str,
    control: &Control,
) -> Result<String> {
    collection.writable()?;
    control.set_face_phase(super::jobs::FacePhase::Analysis);
    let mut models = Models::load(model_dir, dll, detector, embedder)?;
    let detector_key = inference::presence_detector_key(&models.detector.key());
    let embedder_key = models.embedder.key();
    if setting(collection, "rust.detector_key")?.is_some_and(|key| key != detector_key) {
        collection.db.backup(
            "main",
            collection.root.join(format!(
                ".myfotomanager.before-detector-{}.bak",
                uuid::Uuid::new_v4()
            )),
            None,
        )?;
    }
    for (key, value) in [
        ("rust.auto", models.embedder.thresholds.auto),
        ("rust.suggest", models.embedder.thresholds.suggest),
        ("rust.duplicate", models.embedder.thresholds.duplicate),
    ] {
        collection.db.execute(
            "INSERT OR IGNORE INTO face_settings VALUES(?,?)",
            params![
                format!(
                    "rust.thresholds.{embedder_key}.{}",
                    key.strip_prefix("rust.").unwrap_or(key)
                ),
                value.to_string()
            ],
        )?;
    }
    save_setting(&collection.db, "rust.detector_key", &detector_key)?;
    save_setting(&collection.db, "rust.embedder_key", &embedder_key)?;
    save_setting(&collection.db, "active_detector", detector)?;
    save_setting(&collection.db, "active_embedder", embedder)?;
    let total: usize = collection
        .db
        .query_row("SELECT count(*) FROM face_sources", [], |r| r.get(0))?;
    let (mut cursor, mut done, mut found, mut failed, mut without_vector) = (0, 0, 0, 0, 0);
    loop {
        control.checkpoint()?;
        let rows=collection.db.prepare("SELECT f.photo_id,f.id,f.relpath,f.size,f.mtime_ns,coalesce(m.orientation,1),coalesce(m.dt_original,'') FROM face_sources s JOIN files f ON f.id=s.file_id LEFT JOIN meta m ON m.file_id=f.id WHERE f.photo_id>? AND f.status='present' ORDER BY f.photo_id LIMIT 32")?.query_map([cursor],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,u32>(5)?,r.get::<_,String>(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.is_empty() {
            break;
        }
        for (photo, file, relative, size, mtime, orientation, date) in rows {
            control.checkpoint()?;
            cursor = photo;
            let fingerprint = format!("rust-v1:{file}:{size}:{mtime}:{orientation}:{embedder_key}");
            let complete:bool=collection.db.query_row("SELECT EXISTS(SELECT 1 FROM face_jobs WHERE photo_id=? AND stage='detect' AND model_id=? AND status='done' AND fingerprint=?)",params![photo,detector_key,fingerprint],|r|r.get(0))?;
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
                        if quality < 0.5 {
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
                    Err(e) => {
                        control.checkpoint()?;
                        failed += 1;
                        collection.db.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'detect',?,'error',?,datetime('now'),?)",params![photo,detector_key,e.to_string(),fingerprint])?;
                    }
                }
            }
            done += 1;
            control.progress(done, total);
        }
        prepare_groups(collection, &embedder_key, control)?;
    }
    while prepare_groups(collection, &embedder_key, control)? > 0 {}
    collection.backup()?;
    Ok(format!(
        "{done} Fotos geprüft · {found} Gesichter erkannt · {without_vector} ohne Namensmerkmale · {failed} Analysefehler"
    ))
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
            let candidates=tx.prepare("SELECT group_id FROM face_group_catalog WHERE embedder_id=? AND members<14 AND (signature=? OR signature IN (?,?,?,?,?,?,?,?,?,?,?,?)) ORDER BY group_id LIMIT 32")?
                .query_map(params![key,sig,sig^1,sig^2,sig^4,sig^8,sig^16,sig^32,sig^64,sig^128,sig^256,sig^512,sig^1024,sig^2048],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut matched = None;
            for group in candidates {
                let members=tx.prepare("SELECT f.photo_id,e.vec FROM face_group_members g JOIN faces f ON f.id=g.face_id JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=g.embedder_id WHERE g.group_id=?")?.query_map([group],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Vec<u8>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                if members.iter().all(|(p, v)| {
                    p != photo
                        && decode(v)
                            .ok()
                            .and_then(|v| inference::cosine(&vector, &v))
                            .is_some_and(|s| s >= 0.6)
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

#[derive(Clone, Serialize, Deserialize)]
pub struct FaceSnapshot {
    pub id: i64,
    pub person: Option<i64>,
    pub assignment: Option<String>,
    pub confidence: Option<f64>,
    pub ignored: bool,
    pub rejections: Vec<i64>,
    pub pins: Vec<(i64, String)>,
}
#[derive(Clone)]
pub struct Undo {
    pub before: Vec<FaceSnapshot>,
    pub after: Vec<FaceSnapshot>,
    pub created: Option<i64>,
}
fn snapshot(db: &rusqlite::Connection, id: i64) -> Result<FaceSnapshot> {
    let mut value = db.query_row(
        "SELECT person_id,assignment,confidence,ignored FROM faces WHERE id=? AND retained=0",
        [id],
        |r| {
            Ok(FaceSnapshot {
                id,
                person: r.get(0)?,
                assignment: r.get(1)?,
                confidence: r.get(2)?,
                ignored: r.get(3)?,
                rejections: Vec::new(),
                pins: Vec::new(),
            })
        },
    )?;
    value.rejections = db
        .prepare("SELECT person_id FROM face_rejections WHERE face_id=? ORDER BY person_id")?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    value.pins=db.prepare("SELECT person_id,embedder_id FROM training_set WHERE face_id=? AND pinned=1 ORDER BY person_id,embedder_id")?.query_map([id],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(value)
}
pub fn name_group(
    collection: &mut Collection,
    ids: &[i64],
    selected: &BTreeSet<i64>,
    person: Option<i64>,
    name: &str,
    ignore: bool,
) -> Result<Undo> {
    collection.writable()?;
    if ids.is_empty() || ids.len() > 14 {
        bail!("Gruppe muss 1 bis 14 sichtbare Gesichter enthalten");
    }
    if !ignore && selected.is_empty() {
        bail!("Bitte mindestens ein Gesicht auswählen");
    }
    let tx = collection.db.transaction()?;
    let before = ids
        .iter()
        .map(|id| snapshot(&tx, *id))
        .collect::<Result<Vec<_>>>()?;
    let mut created = None;
    let person = if ignore {
        None
    } else if let Some(person) = person {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM persons WHERE id=? AND merged_into IS NULL)",
            [person],
            |r| r.get(0),
        )?;
        if !exists {
            bail!("Person ist nicht mehr verfügbar");
        }
        Some(person)
    } else {
        let name = name.trim();
        if name.is_empty() {
            bail!("Bitte einen Namen eingeben");
        }
        let matches = tx
            .prepare("SELECT person_id FROM person_attrs WHERE key='name' AND casefold(value)=?")?
            .query_map([super::files::fold(name)], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        match matches.as_slice() {
            [id] => Some(*id),
            [] => {
                tx.execute("INSERT INTO persons DEFAULT VALUES", [])?;
                let id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO person_attrs VALUES(?,'name',?)",
                    params![id, name],
                )?;
                created = Some(id);
                Some(id)
            }
            _ => bail!("Mehrere Personen heißen so. Bitte die konkrete Personen-ID wählen."),
        }
    };
    for id in ids {
        if ignore {
            tx.execute("UPDATE faces SET ignored=1,person_id=CASE WHEN assignment='user' THEN person_id ELSE NULL END,assignment=CASE WHEN assignment='user' THEN assignment ELSE NULL END WHERE id=?",[id])?;
        } else if selected.contains(id) {
            tx.execute("UPDATE faces SET person_id=?,assignment='user',confidence=1,ignored=0,assigned_at=datetime('now') WHERE id=?",params![person,id])?;
            tx.execute(
                "DELETE FROM face_rejections WHERE face_id=? AND person_id=?",
                params![id, person],
            )?;
        } else {
            tx.execute(
                "INSERT OR IGNORE INTO face_rejections VALUES(?,?)",
                params![id, person],
            )?;
        }
        tx.execute("DELETE FROM training_set WHERE face_id=?", [id])?;
    }
    let after = ids
        .iter()
        .map(|id| snapshot(&tx, *id))
        .collect::<Result<Vec<_>>>()?;
    tx.commit()?;
    Ok(Undo {
        before,
        after,
        created,
    })
}
pub fn undo(collection: &mut Collection, undo: &Undo) -> Result<()> {
    collection.writable()?;
    let tx = collection.db.transaction()?;
    for expected in &undo.after {
        let current = snapshot(&tx, expected.id)?;
        if current.person != expected.person
            || current.assignment != expected.assignment
            || current.ignored != expected.ignored
            || current.rejections != expected.rejections
            || current.pins != expected.pins
        {
            bail!("Spätere Nutzeränderung verhindert Rücknahme");
        }
    }
    for before in &undo.before {
        tx.execute(
            "UPDATE faces SET person_id=?,assignment=?,confidence=?,ignored=? WHERE id=?",
            params![
                before.person,
                before.assignment,
                before.confidence,
                before.ignored,
                before.id
            ],
        )?;
        tx.execute("DELETE FROM face_rejections WHERE face_id=?", [before.id])?;
        for person in &before.rejections {
            tx.execute(
                "INSERT INTO face_rejections VALUES(?,?)",
                params![before.id, person],
            )?;
        }
        tx.execute("DELETE FROM training_set WHERE face_id=?", [before.id])?;
        for (person, key) in &before.pins {
            tx.execute(
                "INSERT INTO training_set VALUES(?,?,?,'ref',datetime('now'),'pin',1)",
                params![person, before.id, key],
            )?;
        }
    }
    if let Some(id) = undo.created {
        let used:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM faces WHERE person_id=?) OR EXISTS(SELECT 1 FROM relations WHERE from_person=? OR to_person=?)",params![id,id,id],|r|r.get(0))?;
        if !used {
            tx.execute("DELETE FROM person_attrs WHERE person_id=?", [id])?;
            tx.execute("DELETE FROM persons WHERE id=?", [id])?;
        }
    }
    tx.commit()?;
    Ok(())
}
fn delete_face_rows(db: &rusqlite::Connection, id: i64) -> Result<()> {
    for table in [
        "face_embeddings",
        "face_rejections",
        "training_set",
        "training_exclusions",
        "face_review",
    ] {
        db.execute(&format!("DELETE FROM {table} WHERE face_id=?"), [id])?;
    }
    db.execute("DELETE FROM faces WHERE id=?", [id])?;
    Ok(())
}
pub fn delete_face(collection: &mut Collection, photo: i64, id: i64) -> Result<()> {
    collection.writable()?;
    let tx = collection.db.transaction()?;
    let valid: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM faces WHERE id=? AND photo_id=? AND retained=0)",
        params![id, photo],
        |r| r.get(0),
    )?;
    if !valid {
        bail!("Gesicht gehört nicht mehr zum ausgewählten Foto");
    }
    tx.execute("INSERT OR IGNORE INTO face_reference_dirty SELECT e.embedder_id,f.person_id FROM face_embeddings e JOIN faces f ON f.id=e.face_id WHERE f.id=? AND f.person_id IS NOT NULL",[id])?;
    tx.execute("INSERT INTO face_maintenance SELECT embedder_id,0,1 FROM face_embeddings WHERE face_id=? ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[id])?;
    delete_face_rows(&tx, id)?;
    tx.commit()?;
    Ok(())
}

pub fn add_relation(
    collection: &mut Collection,
    source: i64,
    kind: &str,
    target: i64,
) -> Result<()> {
    collection.writable()?;
    if source == target {
        bail!("Eine Person kann keine Beziehung zu sich selbst haben");
    }
    for person in [source, target] {
        let valid: bool = collection.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM persons WHERE id=? AND merged_into IS NULL)",
            [person],
            |r| r.get(0),
        )?;
        if !valid {
            bail!("Person nicht verfügbar");
        }
    }
    let (mut from, mut to) = (source, target);
    let id = match kind {
        "parent" => 1,
        "child" => {
            std::mem::swap(&mut from, &mut to);
            1
        }
        "partner" => 3,
        "sibling" => 4,
        _ => bail!("Unbekannter Beziehungstyp"),
    };
    if id == 1 {
        let cycle:bool=collection.db.query_row("WITH RECURSIVE descendants(id) AS (SELECT to_person FROM relations WHERE type_id=1 AND from_person=? UNION SELECT r.to_person FROM relations r JOIN descendants d ON r.from_person=d.id WHERE r.type_id=1) SELECT EXISTS(SELECT 1 FROM descendants WHERE id=?)",params![to,from],|r|r.get(0))?;
        if cycle {
            bail!("Diese Beziehung würde einen Abstammungszyklus erzeugen");
        }
    }
    if id == 3 || id == 4 {
        (from, to) = (from.min(to), from.max(to));
    }
    collection.db.execute(
        "INSERT OR IGNORE INTO relations VALUES(?,?,?)",
        params![from, id, to],
    )?;
    Ok(())
}
pub fn remove_relation(
    collection: &Collection,
    source: i64,
    kind: &str,
    target: i64,
) -> Result<()> {
    collection.writable()?;
    let (mut source, mut target) = (source, target);
    let id = match kind {
        "parent" => 1,
        "child" => {
            std::mem::swap(&mut source, &mut target);
            1
        }
        "partner" => 3,
        "sibling" => 4,
        _ => bail!("Unbekannter Beziehungstyp"),
    };
    if id == 3 || id == 4 {
        (source, target) = (source.min(target), source.max(target));
    }
    collection.db.execute(
        "DELETE FROM relations WHERE from_person=? AND type_id=? AND to_person=?",
        params![source, id, target],
    )?;
    Ok(())
}
