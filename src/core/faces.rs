use super::{
    collection::Collection,
    inference::{self, Models},
    jobs::Control,
    previews,
};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Clone, Debug)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub birth: String,
    pub count: usize,
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
    pub pending: usize,
    pub applied: String,
    pub detector: String,
    pub embedder: String,
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
fn save_setting(db: &rusqlite::Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}
pub fn people(collection: &Collection) -> Result<Vec<Person>> {
    Ok(collection.db.prepare("SELECT p.id,coalesce(a.value,'Unbenannt'),coalesce(b.value,''),(SELECT count(*) FROM faces f WHERE f.person_id=p.id AND f.assignment='user' AND f.retained=0) FROM persons p LEFT JOIN person_attrs a ON a.person_id=p.id AND a.key='name' LEFT JOIN person_attrs b ON b.person_id=p.id AND b.key='birth_year' WHERE p.merged_into IS NULL ORDER BY a.value,p.id")?
        .query_map([],|r|Ok(Person{id:r.get(0)?,name:r.get(1)?,birth:r.get(2)?,count:r.get(3)?}))?.collect::<rusqlite::Result<_>>()?)
}
pub fn status(collection: &Collection) -> Result<Status> {
    let mut status=collection.db.query_row("SELECT count(*),coalesce(sum(assignment='user'),0),coalesce(sum(person_id IS NULL AND ignored=0),0),coalesce(sum(assignment IN ('auto','suggested') AND ignored=0),0),coalesce(sum(ignored=1),0) FROM faces WHERE retained=0",[],|r|Ok(Status{total:r.get(0)?,confirmed:r.get(1)?,unknown:r.get(2)?,review:r.get(3)?,ignored:r.get(4)?,..Default::default()}))?;
    status.detector = setting(collection, "rust.detector_key")?.unwrap_or_default();
    status.embedder = setting(collection, "rust.embedder_key")?.unwrap_or_default();
    status.references = collection.db.query_row(
        "SELECT count(*) FROM training_set WHERE embedder_id=? AND split='ref'",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.pending = collection.db.query_row(
        "SELECT count(*) FROM face_maintenance WHERE embedder_id=?",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.applied = setting(collection, "rust.recognition_applied")?
        .unwrap_or_else(|| "Noch nicht protokolliert".into());
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
pub fn page(collection: &Collection, mode: &str, page: usize) -> Result<FacePage> {
    let status = status(collection)?;
    let mut groups = Vec::new();
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
        let rows=collection.db.prepare(&format!("SELECT {FACE_COLUMNS} FROM faces f WHERE f.retained=0 AND {predicate} ORDER BY f.person_id,f.id LIMIT 112 OFFSET ?"))?.query_map([(page*112) as i64],face_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut group = Vec::new();
        for face in rows {
            if group.len() == 14
                || group
                    .last()
                    .is_some_and(|last: &Face| last.person != face.person)
            {
                groups.push(std::mem::take(&mut group));
            }
            group.push(face);
        }
        if !group.is_empty() {
            groups.push(group);
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
    let mut models = Models::load(model_dir, dll, detector, embedder)?;
    let detector_key = models.detector.key();
    let embedder_key = models.embedder.key();
    save_setting(&collection.db, "rust.detector_key", &detector_key)?;
    save_setting(&collection.db, "rust.embedder_key", &embedder_key)?;
    let total: usize = collection
        .db
        .query_row("SELECT count(*) FROM face_sources", [], |r| r.get(0))?;
    let (mut cursor, mut done, mut found, mut failed) = (0, 0, 0, 0);
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
                let result = (|| -> Result<usize> {
                    let image = previews::load(
                        &collection.safe_path(&relative)?,
                        exiftool,
                        orientation,
                        1600,
                        control,
                    )?;
                    let detections = models.detect(&image)?;
                    let mut embedded = Vec::new();
                    for detection in &detections {
                        control.checkpoint()?;
                        embedded.push(models.embed(&image, detection)?);
                    }
                    let old = photo_faces(collection, photo)?;
                    let tx = collection.db.transaction()?;
                    let mut used = BTreeSet::new();
                    for (detection, vector) in detections.iter().zip(&embedded) {
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
                        let quality = (detection.score
                            * ((detection.bbox[2] - detection.bbox[0])
                                .min(detection.bbox[3] - detection.bbox[1])
                                / 80.0)
                                .min(1.0))
                        .clamp(0.0, 1.0);
                        let landmarks = serde_json::to_string(&detection.landmarks)?;
                        let id = if let Some(previous) = previous {
                            used.insert(previous.id);
                            tx.execute("UPDATE faces SET detector_id=?,bbox_x1=?,bbox_y1=?,bbox_x2=?,bbox_y2=?,landmarks=?,det_score=?,quality=?,retained=0,source_fingerprint=? WHERE id=?",params![detector_key,bbox[0],bbox[1],bbox[2],bbox[3],landmarks,detection.score,quality,fingerprint,previous.id])?;
                            previous.id
                        } else {
                            tx.execute("INSERT INTO faces(photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,captured_at,source_fingerprint) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",params![photo,file,relative,detector_key,bbox[0],bbox[1],bbox[2],bbox[3],landmarks,detection.score,quality,date,fingerprint])?;
                            tx.last_insert_rowid()
                        };
                        tx.execute(
                            "INSERT OR REPLACE INTO face_embeddings VALUES(?,?,?,?)",
                            params![id, embedder_key, vector.len() as i64, encode(vector)],
                        )?;
                    }
                    for face in old.iter().filter(|f| !used.contains(&f.id)) {
                        if face.assignment.as_deref() == Some("user") || face.ignored {
                            tx.execute("UPDATE faces SET retained=1 WHERE id=?", [face.id])?;
                        } else {
                            delete_face_rows(&tx, face.id)?;
                        }
                    }
                    tx.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'detect',?,'done',NULL,datetime('now'),?)",params![photo,detector_key,fingerprint])?;
                    tx.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,'embed',?,'done',NULL,datetime('now'),?)",params![photo,embedder_key,fingerprint])?;
                    control.checkpoint()?;
                    tx.commit()?;
                    Ok(detections.len())
                })();
                match result {
                    Ok(n) => found += n,
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
        "{done} Fotos geprüft · {found} Gesichter verarbeitet · {failed} Fehler"
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
        .as_chunks::<4>().0.iter()
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
        tx.execute(
            "DELETE FROM training_set WHERE face_id=? AND pinned=0",
            [id],
        )?;
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

/// Stream model-specific references; own face and rejected/occupied people are excluded.
pub fn suggestions(
    collection: &Collection,
    face: i64,
    key: &str,
) -> Result<Vec<(i64, String, f32)>> {
    let bytes: Option<Vec<u8>> = collection
        .db
        .query_row(
            "SELECT vec FROM face_embeddings WHERE face_id=? AND embedder_id=?",
            params![face, key],
            |r| r.get(0),
        )
        .optional()?;
    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    let query = decode(&bytes)?;
    let photo: i64 =
        collection
            .db
            .query_row("SELECT photo_id FROM faces WHERE id=?", [face], |r| {
                r.get(0)
            })?;
    let mut statement=collection.db.prepare("SELECT t.person_id,e.vec,coalesce(a.value,'Unbenannt') FROM training_set t JOIN face_embeddings e ON e.face_id=t.face_id AND e.embedder_id=t.embedder_id JOIN faces f ON f.id=t.face_id LEFT JOIN person_attrs a ON a.person_id=t.person_id AND a.key='name' WHERE t.embedder_id=? AND t.split='ref' AND t.face_id!=? AND f.assignment='user' AND f.ignored=0 AND f.retained=0 AND f.quality>=0.5 AND NOT EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=t.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_review v WHERE v.face_id=f.id AND v.embedder_id=t.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_rejections r WHERE r.face_id=? AND r.person_id=t.person_id) AND NOT EXISTS(SELECT 1 FROM faces o WHERE o.photo_id=? AND o.assignment='user' AND o.person_id=t.person_id AND o.id!=? AND o.retained=0) ORDER BY t.person_id,t.face_id")?;
    let mut rows = statement.query(params![key, face, face, photo, face])?;
    let mut scores: BTreeMap<i64, (String, Vec<f32>)> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let vector = decode(&row.get::<_, Vec<u8>>(1)?)?;
        if let Some(score) = inference::cosine(&query, &vector) {
            let entry = scores
                .entry(row.get(0)?)
                .or_insert_with(|| (row.get(2).unwrap_or_default(), Vec::new()));
            entry.1.push(score);
            entry.1.sort_by(|a, b| b.total_cmp(a));
            entry.1.truncate(3);
        }
    }
    let mut result: Vec<_> = scores
        .into_iter()
        .map(|(id, (name, scores))| (id, name, scores.iter().sum::<f32>() / scores.len() as f32))
        .collect();
    result.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
    result.truncate(6);
    Ok(result)
}

pub fn apply_knowledge(collection: &mut Collection, control: &Control) -> Result<String> {
    collection.writable()?;
    let key = setting(collection, "rust.embedder_key")?
        .context("Zuerst Fotos mit einem lokalen Modell analysieren")?;
    collection.db.execute("INSERT INTO face_maintenance VALUES(?,0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[&key])?;
    let people = people(collection)?;
    for person in people {
        control.checkpoint()?;
        let candidates=collection.db.prepare("SELECT f.id,e.vec,coalesce(f.captured_at,'') FROM faces f JOIN face_embeddings e ON e.face_id=f.id WHERE e.embedder_id=? AND f.person_id=? AND f.assignment='user' AND f.ignored=0 AND f.retained=0 AND f.quality>=0.5 AND NOT EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=?) ORDER BY f.quality DESC,f.id LIMIT 2048")?.query_map(params![key,person.id,key],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let tx = collection.db.transaction()?;
        tx.execute(
            "DELETE FROM training_set WHERE person_id=? AND embedder_id=? AND pinned=0",
            params![person.id, key],
        )?;
        let mut selected: Vec<Vec<f32>> = Vec::new();
        for (id, bytes, _) in candidates {
            let vector = decode(&bytes)?;
            if selected.len() >= 100 {
                break;
            }
            if selected
                .iter()
                .any(|v| inference::cosine(v, &vector).is_some_and(|s| s > 0.92))
            {
                continue;
            }
            let split = if selected.len() % 5 == 4 {
                "val"
            } else {
                "ref"
            };
            tx.execute("INSERT OR IGNORE INTO training_set VALUES(?,?,?,?,datetime('now'),'rust-quality-diversity',0)",params![person.id,id,key,split])?;
            selected.push(vector);
        }
        tx.execute(
            "DELETE FROM face_reference_dirty WHERE embedder_id=? AND person_id=?",
            params![key, person.id],
        )?;
        tx.commit()?;
    }
    rematch(collection, &key, control)?;
    while prepare_groups(collection, &key, control)? > 0 {}
    Ok("Referenzen ausgewählt und Namen vorgeschlagen".into())
}

pub fn rematch(collection: &mut Collection, key: &str, control: &Control) -> Result<()> {
    let mut cursor: i64 = collection
        .db
        .query_row(
            "SELECT after_photo FROM face_maintenance WHERE embedder_id=?",
            [key],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    loop {
        control.checkpoint()?;
        let photos=collection.db.prepare("SELECT DISTINCT photo_id FROM faces WHERE photo_id>? AND retained=0 ORDER BY photo_id LIMIT 32")?.query_map([cursor],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        if photos.is_empty() {
            break;
        }
        for photo in photos {
            control.checkpoint()?;
            let faces = photo_faces(collection, photo)?;
            let mut choices = Vec::new();
            for face in faces
                .iter()
                .filter(|f| !f.ignored && f.assignment.as_deref() != Some("user"))
            {
                for (person, _, score) in suggestions(collection, face.id, key)? {
                    if score >= 0.363 {
                        choices.push((score, face.id, person));
                    }
                }
            }
            choices.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let mut used_faces = BTreeSet::new();
            let mut used_people: BTreeSet<_> = faces
                .iter()
                .filter(|f| f.assignment.as_deref() == Some("user"))
                .filter_map(|f| f.person)
                .collect();
            let tx = collection.db.transaction()?;
            tx.execute("UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE photo_id=? AND assignment IS NOT 'user' AND retained=0",[photo])?;
            for (score, face, person) in choices {
                if !used_faces.contains(&face) && !used_people.contains(&person) {
                    tx.execute("UPDATE faces SET person_id=?,assignment=?,confidence=? WHERE id=? AND assignment IS NOT 'user'",params![person,if score>=0.6{"auto"}else{"suggested"},score,face])?;
                    used_faces.insert(face);
                    used_people.insert(person);
                }
            }
            tx.execute(
                "UPDATE face_maintenance SET after_photo=? WHERE embedder_id=?",
                params![photo, key],
            )?;
            tx.commit()?;
            cursor = photo;
        }
    }
    let tx = collection.db.transaction()?;
    tx.execute("DELETE FROM face_maintenance WHERE embedder_id=?", [key])?;
    save_setting(
        &tx,
        "rust.recognition_applied",
        &chrono::Utc::now().to_rfc3339(),
    )?;
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
