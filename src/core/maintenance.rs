use super::{
    collection::Collection,
    faces, inference,
    jobs::Control,
    matching,
    previews::{self, PreviewRequest},
};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};

fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Ungültiger Ausgabepfad")?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}
fn generated_path(root: &Path, path: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    let mut current = path.to_path_buf();
    while !current.exists() {
        if !current.pop() {
            bail!("Ausgabepfad ist ungültig");
        }
    }
    if current.symlink_metadata()?.file_type().is_symlink()
        || !current.canonicalize()?.starts_with(root)
    {
        bail!("Verknüpfungen außerhalb der Sammlung sind nicht erlaubt");
    }
    Ok(())
}

pub fn export(collection: &Collection, exiftool: &str, control: &Control) -> Result<String> {
    collection.writable()?;
    let key =
        faces::setting(collection, "rust.embedder_key")?.context("Kein aktiver Rust-Modellraum")?;
    let root = collection.root.join(".myfotomanager_faces");
    generated_path(&collection.root, &root)?;
    std::fs::create_dir_all(&root)?;
    let mut entries = Vec::new();
    let mut expected = BTreeSet::new();
    let mut statement=collection.db.prepare("SELECT t.person_id,t.face_id,t.split,f.photo_id,f.source_relpath,f.bbox_x1,f.bbox_y1,f.bbox_x2,f.bbox_y2,f.quality,f.landmarks,f.captured_at,f.source_fingerprint FROM training_set t JOIN faces f ON f.id=t.face_id WHERE t.embedder_id=? ORDER BY t.person_id,t.face_id")?;
    let mut rows = statement.query([&key])?;
    while let Some(row) = rows.next()? {
        control.checkpoint()?;
        let person: i64 = row.get(0)?;
        let face: i64 = row.get(1)?;
        let split: String = row.get(2)?;
        if !["ref", "val"].contains(&split.as_str()) {
            bail!("Ungültiger Export-Split");
        }
        let source: String = row.get(4)?;
        let bbox = [row.get::<_, f32>(5)?, row.get(6)?, row.get(7)?, row.get(8)?];
        let relative = format!("persons/{person:06}/{split}/f_{face:07}");
        let image_path = root.join(format!("{relative}.jpg"));
        let meta_path = root.join(format!("{relative}.json"));
        generated_path(&collection.root, &image_path)?;
        let path = collection.safe_path(&source)?;
        let stat = path.metadata()?;
        let metadata = serde_json::json!({"face_id":face,"photo_id":row.get::<_,i64>(3)?,"relpath":source,"quality":row.get::<_,Option<f32>>(9)?,"landmarks":row.get::<_,Option<String>>(10)?.and_then(|v|serde_json::from_str::<serde_json::Value>(&v).ok()),"captured_at":row.get::<_,Option<String>>(11)?,"source_fingerprint":row.get::<_,Option<String>>(12)?,"bbox":bbox,"coordinate_frame":"oriented_source_normalized","crop":{"scale":1.6,"size":256,"aligned":false},"source_stat":[stat.len(),stat.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_nanos().to_string()]});
        let bytes = serde_json::to_vec_pretty(&metadata)?;
        if !image_path.is_file()
            || std::fs::read(&meta_path).ok().as_deref() != Some(bytes.as_slice())
        {
            let preview = previews::preview(
                PreviewRequest {
                    root: Some(collection.root.clone()),
                    path,
                    orientation: 1,
                    crop: Some(bbox),
                    edge: 256,
                    generation: 0,
                },
                exiftool,
                control,
            )?;
            let image = image::RgbaImage::from_raw(
                preview.width as u32,
                preview.height as u32,
                preview.rgba,
            )
            .context("Ungültige Exportvorschau")?;
            let mut jpeg = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
                .encode_image(&image::DynamicImage::ImageRgba8(image).to_rgb8())?;
            atomic(&image_path, &jpeg)?;
            atomic(&meta_path, &bytes)?;
        }
        expected.insert(format!("{relative}.jpg"));
        expected.insert(format!("{relative}.json"));
        entries.push(serde_json::json!({"person_id":person,"face_id":face,"split":split}));
    }
    let previous = std::fs::read(root.join("manifest.json"))
        .ok()
        .and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok());
    let manifest = serde_json::json!({"version":1,"embedder_id":key,"N":100,"seed":42,"validation_fraction":0.2,"entries":entries,"owned_files":expected});
    atomic(
        &root.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    if let Some(files) = previous
        .as_ref()
        .and_then(|v| v.get("owned_files"))
        .and_then(|v| v.as_array())
    {
        for relative in files.iter().filter_map(|v| v.as_str()) {
            if !expected.contains(relative) {
                let path = root.join(relative);
                if !Path::new(relative)
                    .components()
                    .all(|v| matches!(v, std::path::Component::Normal(_)))
                {
                    continue;
                }
                generated_path(&root, &path)?;
                if path.is_file() {
                    std::fs::remove_file(path)?;
                }
            }
        }
    }
    Ok(format!(
        "{} Referenz-/Validierungsausschnitte exportiert: {}",
        entries.len(),
        root.display()
    ))
}

pub fn calibrate(collection: &Collection, control: &Control) -> Result<serde_json::Value> {
    let key =
        faces::setting(collection, "rust.embedder_key")?.context("Kein aktiver Modellraum")?;
    let validations=collection.db.prepare("SELECT t.face_id,t.person_id,e.vec FROM training_set t JOIN faces f ON f.id=t.face_id JOIN face_embeddings e ON e.face_id=t.face_id AND e.embedder_id=t.embedder_id WHERE t.embedder_id=? AND t.split='val' AND f.assignment='user' AND f.ignored=0")?.query_map([&key],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let people: usize = collection.db.query_row(
        "SELECT count(DISTINCT person_id) FROM training_set WHERE embedder_id=? AND split='ref'",
        [&key],
        |r| r.get(0),
    )?;
    if people < 2 || validations.is_empty() {
        return Ok(
            serde_json::json!({"available":false,"reason":"Mindestens zwei Personen mit Referenz- und Validierungsbildern benötigt"}),
        );
    }
    let mut predictions = Vec::new();
    for (_, truth, bytes) in validations {
        control.checkpoint()?;
        let query = faces::decode(&bytes)?;
        let mut statement=collection.db.prepare("SELECT t.person_id,e.vec FROM training_set t JOIN face_embeddings e ON e.face_id=t.face_id AND e.embedder_id=t.embedder_id WHERE t.embedder_id=? AND t.split='ref' ORDER BY t.person_id,t.face_id")?;
        let mut rows = statement.query([&key])?;
        let mut similarities = Vec::new();
        while let Some(row) = rows.next()? {
            let vector = faces::decode(&row.get::<_, Vec<u8>>(1)?)?;
            if let Some(score) = inference::cosine(&query, &vector) {
                similarities.push((row.get::<_, i64>(0)?, score));
            }
        }
        if let Some((person, score)) = matching::top_three(similarities.into_iter())
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
        {
            predictions.push((truth, person, score));
        }
    }
    let mut curve = Vec::new();
    let mut proposal = None;
    for step in 0..=200 {
        let threshold = -1.0 + step as f32 * 0.01;
        let accepted: Vec<_> = predictions.iter().filter(|r| r.2 >= threshold).collect();
        let correct = accepted.iter().filter(|r| r.0 == r.1).count();
        let precision = if accepted.is_empty() {
            None
        } else {
            Some(correct as f64 / accepted.len() as f64)
        };
        let recall = correct as f64 / predictions.len().max(1) as f64;
        if precision.is_some_and(|v| v >= 0.98)
            && proposal.as_ref().is_none_or(|(best, _)| recall >= *best)
        {
            proposal = Some((recall, threshold));
        }
        curve.push(serde_json::json!({"threshold":threshold,"precision":precision,"recall":recall,"accepted":accepted.len()}));
    }
    Ok(
        serde_json::json!({"available":true,"embedder_id":key,"samples":predictions.len(),"curve":curve,"proposed_auto":proposal.map(|p|p.1),"note":"Empirische Schätzung; keine Präzisionsgarantie. Schwellen werden nicht automatisch geändert."}),
    )
}

const BIOMETRIC_TABLES: &[&str] = &[
    "face_group_members",
    "face_group_catalog",
    "face_group_dirty",
    "face_group_state",
    "face_maintenance",
    "face_reference_dirty",
    "face_embeddings",
    "face_rejections",
    "training_set",
    "training_exclusions",
    "face_review",
    "relations",
    "person_attrs",
    "persons",
    "faces",
    "face_jobs",
    "face_settings",
];
fn scrub(db: &rusqlite::Connection) -> Result<()> {
    db.execute_batch("PRAGMA secure_delete=ON; BEGIN IMMEDIATE;")?;
    for table in BIOMETRIC_TABLES {
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
            [table],
            |r| r.get(0),
        )?;
        if exists {
            db.execute(&format!("DELETE FROM {table}"), [])?;
        }
    }
    db.execute_batch("COMMIT; PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
    Ok(())
}
pub fn delete_biometrics(collection: &Collection) -> Result<String> {
    collection.writable()?;
    let folder = collection.root.join(".myfotomanager_faces");
    generated_path(&collection.root, &folder)?;
    if folder.exists() {
        validate_tree(&collection.root, &folder)?;
    }
    // Gesichtskacheln (ADR 0068) sind abgeleitete Gesichtsbilder und gehören dazu.
    let tiles = collection.root.join(super::previews::TILE_DIR);
    generated_path(&collection.root, &tiles)?;
    if tiles.exists() {
        validate_tree(&collection.root, &tiles)?;
    }
    scrub(&collection.db)?;
    let mut backups = Vec::new();
    for entry in std::fs::read_dir(&collection.root)? {
        let path = entry?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name == ".myfotomanager.backup"
            || name == ".myfotomanager.bak"
            || name.starts_with(".myfotomanager.before-detector-") && name.ends_with(".bak")
            || name.starts_with(".myfotomanager.before-rust-") && name.ends_with(".bak")
        {
            backups.push(path);
        }
    }
    for path in backups {
        generated_path(&collection.root, &path)?;
        if path.is_file() {
            scrub(&rusqlite::Connection::open(path)?)?;
        }
    }
    if folder.exists() {
        std::fs::remove_dir_all(&folder)?;
    }
    if tiles.exists() {
        std::fs::remove_dir_all(&tiles)?;
    }
    Ok(
        "Biometrie und bekannte App-Sicherungen bereinigt. Fotos und Inventar bleiben erhalten."
            .into(),
    )
}
fn validate_tree(root: &Path, path: &Path) -> Result<()> {
    generated_path(root, path)?;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            validate_tree(root, &entry?.path())?;
        }
    }
    Ok(())
}

pub fn set_person(
    collection: &Collection,
    id: i64,
    attrs: &BTreeMap<String, String>,
) -> Result<()> {
    collection.writable()?;
    let exists: bool = collection.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM persons WHERE id=? AND merged_into IS NULL)",
        [id],
        |r| r.get(0),
    )?;
    if !exists {
        bail!("Person ist nicht verfügbar");
    }
    for (key, value) in attrs {
        if !["name", "birth_year", "gender", "note"].contains(&key.as_str()) {
            bail!("Unbekanntes Personenattribut");
        }
        if key == "birth_year"
            && !value.is_empty()
            && value
                .parse::<u32>()
                .ok()
                .is_none_or(|y| !(1..=9999).contains(&y))
        {
            bail!("Geburtsjahr ist ungültig");
        }
    }
    collection.db.execute_batch("BEGIN IMMEDIATE;")?;
    let result = (|| -> Result<()> {
        for (key, value) in attrs {
            collection.db.execute(
                "INSERT OR REPLACE INTO person_attrs VALUES(?,?,?)",
                params![id, key, value],
            )?;
        }
        Ok(())
    })();
    if result.is_ok() {
        collection.db.execute_batch("COMMIT")?;
    } else {
        collection.db.execute_batch("ROLLBACK")?;
    }
    result
}
pub fn reference_action(collection: &Collection, face: i64, action: &str) -> Result<()> {
    collection.writable()?;
    let key =
        faces::setting(collection, "rust.embedder_key")?.context("Kein aktiver Modellraum")?;
    let person:Option<i64>=collection.db.query_row("SELECT person_id FROM faces WHERE id=? AND assignment='user' AND ignored=0 AND quality>=0.5",[face],|r|r.get(0)).optional()?.flatten();
    let person = person.context("Nur geeignete bestätigte Gesichter können Referenzen sein")?;
    collection.db.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<()> {
        match action {
            "pin" => {
                let count:usize=collection.db.query_row("SELECT count(*) FROM training_set WHERE person_id=? AND embedder_id=? AND pinned=1 AND face_id!=?",params![person,key,face],|r|r.get(0))?;
                if count >= 100 {
                    bail!("Referenzlimit erreicht");
                }
                collection.db.execute(
                    "DELETE FROM training_exclusions WHERE face_id=? AND embedder_id=?",
                    params![face, key],
                )?;
                collection.db.execute("INSERT OR REPLACE INTO training_set VALUES(?,?,?,'ref',datetime('now'),'manual_pin',1)",params![person,face,key])?;
            }
            "unpin" => {
                collection.db.execute(
                    "UPDATE training_set SET pinned=0 WHERE face_id=? AND embedder_id=?",
                    params![face, key],
                )?;
            }
            "exclude" => {
                collection.db.execute(
                    "INSERT OR IGNORE INTO training_exclusions VALUES(?,?)",
                    params![face, key],
                )?;
                collection.db.execute(
                    "DELETE FROM training_set WHERE face_id=? AND embedder_id=?",
                    params![face, key],
                )?;
            }
            _ => bail!("Unbekannte Referenzaktion"),
        }
        collection.db.execute(
            "INSERT OR IGNORE INTO face_reference_dirty VALUES(?,?)",
            params![key, person],
        )?;
        collection.db.execute("INSERT INTO face_maintenance VALUES(?,0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[&key])?;
        Ok(())
    })();
    collection
        .db
        .execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
    result
}

/// Direkte und berechnete Beziehungen einer Person als Schlüssel -> Personen-IDs
/// (Kompatibilitätssicht; die gruppierte Übersicht nutzt [`super::kinship`]).
pub fn relations(collection: &Collection, person: i64) -> Result<BTreeMap<String, Vec<i64>>> {
    use super::kinship::Relation as K;
    use super::relations::{Role as R, direct, load_edges, load_genders};
    let edges = load_edges(collection)?;
    let family = super::kinship::Family::new(&edges, &load_genders(collection)?);
    let direct_roles = direct(&edges, person);
    let found = family.find(person, &|_| true);
    let by_tree = |wanted: &dyn Fn(K) -> bool| -> Vec<i64> {
        let mut ids: Vec<i64> = found
            .iter()
            .filter(|m| wanted(m.relation))
            .map(|m| m.person)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let blood = |a: u8, b: u8| move |r: K| matches!(r, K::Blood { a_gen, b_gen, .. } if a_gen == a && b_gen == b);
    let roles = |role: R| -> Vec<i64> {
        direct_roles
            .iter()
            .filter(|(r, _)| *r == role)
            .map(|(_, id)| *id)
            .collect()
    };
    let merged = |mut ids: Vec<i64>, more: Vec<i64>| {
        ids.extend(more);
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let mut result: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    result.insert("eltern".into(), by_tree(&blood(0, 1)));
    result.insert("kinder".into(), by_tree(&blood(1, 0)));
    result.insert("partner".into(), roles(R::Partner));
    result.insert("ehemalige_partner".into(), roles(R::FormerPartner));
    result.insert("freunde".into(), roles(R::Friend));
    result.insert("bekannte".into(), roles(R::Acquaintance));
    result.insert(
        "geschwister".into(),
        merged(by_tree(&blood(1, 1)), by_tree(&|r| r == K::RecordedSibling)),
    );
    result.insert(
        "cousins".into(),
        merged(
            by_tree(&|r| r.is_cousin()),
            by_tree(&|r| r == K::RecordedCousin),
        ),
    );
    result.insert("grosseltern".into(), by_tree(&blood(0, 2)));
    result.insert("urgrosseltern".into(), by_tree(&blood(0, 3)));
    result.insert("enkel".into(), by_tree(&blood(2, 0)));
    result.insert("urenkel".into(), by_tree(&blood(3, 0)));
    result.insert(
        "onkel_tanten".into(),
        by_tree(&|r| matches!(r, K::Blood { a_gen: 1, b_gen, .. } if b_gen >= 2)),
    );
    result.insert(
        "nichten_neffen".into(),
        by_tree(&|r| matches!(r, K::Blood { a_gen, b_gen: 1, .. } if a_gen >= 2)),
    );
    result.insert(
        "schwiegereltern".into(),
        by_tree(&|r| r == K::ThroughPartnerOfB { a_gen: 0, p_gen: 1 }),
    );
    result.insert(
        "schwiegerkinder".into(),
        by_tree(&|r| r == K::ThroughPartnerOfA { p_gen: 1, b_gen: 0 }),
    );
    result.insert(
        "stiefkinder".into(),
        by_tree(&|r| r == K::ThroughPartnerOfB { a_gen: 1, p_gen: 0 }),
    );
    result.insert(
        "stiefeltern".into(),
        by_tree(&|r| r == K::ThroughPartnerOfA { p_gen: 0, b_gen: 1 }),
    );
    result.insert("stiefgeschwister".into(), by_tree(&|r| r == K::StepSibling));
    result.insert(
        "schwaeger".into(),
        by_tree(&|r| {
            matches!(
                r,
                K::ThroughPartnerOfB { a_gen: 1, p_gen: 1 }
                    | K::ThroughPartnerOfA { p_gen: 1, b_gen: 1 }
            )
        }),
    );
    Ok(result)
}
pub fn merge_people(collection: &mut Collection, source: i64, target: i64) -> Result<()> {
    collection.writable()?;
    if source == target {
        bail!("Bitte zwei verschiedene Personen auswählen");
    }
    for person in [source, target] {
        let exists: bool = collection.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM persons WHERE id=? AND merged_into IS NULL)",
            [person],
            |r| r.get(0),
        )?;
        if !exists {
            bail!("Person ist nicht verfügbar");
        }
    }
    let relation:bool=collection.db.query_row("WITH RECURSIVE ancestors(id) AS (SELECT to_person FROM relations WHERE type_id=1 AND from_person=? UNION SELECT r.to_person FROM relations r JOIN ancestors a ON r.from_person=a.id WHERE r.type_id=1) SELECT EXISTS(SELECT 1 FROM ancestors WHERE id=?)",params![source,target],|r|r.get(0))?;
    let inverse:bool=collection.db.query_row("WITH RECURSIVE ancestors(id) AS (SELECT to_person FROM relations WHERE type_id=1 AND from_person=? UNION SELECT r.to_person FROM relations r JOIN ancestors a ON r.from_person=a.id WHERE r.type_id=1) SELECT EXISTS(SELECT 1 FROM ancestors WHERE id=?)",params![target,source],|r|r.get(0))?;
    if relation || inverse {
        bail!("Zusammenführen würde die Abstammung widersprüchlich machen");
    }
    let tx = collection.db.transaction()?;
    tx.execute(
        "UPDATE faces SET person_id=? WHERE person_id=?",
        params![target, source],
    )?;
    for table in ["face_rejections", "training_set", "person_attrs"] {
        tx.execute(
            &format!("UPDATE OR IGNORE {table} SET person_id=? WHERE person_id=?"),
            params![target, source],
        )?;
        tx.execute(&format!("DELETE FROM {table} WHERE person_id=?"), [source])?;
    }
    let rows: Vec<(i64, i64, i64)> = tx
        .prepare("SELECT from_person,type_id,to_person FROM relations")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let plan = super::relations::plan_merge(&super::relations::normalize(&rows), source, target)?;
    tx.execute_batch(&super::relations::seed_sql())?;
    tx.execute(
        "DELETE FROM relations WHERE from_person=? OR to_person=?",
        params![source, source],
    )?;
    for edge in &plan.drop {
        tx.execute(
            "DELETE FROM relations WHERE type_id=?2 AND ((from_person=?1 AND to_person=?3) OR (from_person=?3 AND to_person=?1))",
            params![edge.from, edge.kind.id(), edge.to],
        )?;
    }
    for edge in &plan.add {
        tx.execute(
            "INSERT OR IGNORE INTO relations VALUES(?,?,?)",
            params![edge.from, edge.kind.id(), edge.to],
        )?;
    }
    tx.execute(
        "UPDATE persons SET merged_into=? WHERE id=?",
        params![target, source],
    )?;
    tx.commit()?;
    Ok(())
}
