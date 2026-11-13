use anyhow::{Result, bail};
use rusqlite::Connection;
use std::{collections::BTreeSet, path::Path};

const INVENTORY: &[&str] = &[
    "collection",
    "settings",
    "photos",
    "files",
    "meta",
    "scans",
    "changes",
];
const FACES: &[&str] = &[
    "face_settings",
    "persons",
    "person_attrs",
    "faces",
    "face_embeddings",
    "face_rejections",
    "training_set",
    "face_review",
    "relation_types",
    "relations",
    "face_jobs",
];
fn columns(db: &Connection, table: &str) -> Result<BTreeSet<String>> {
    Ok(db
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get(1))?
        .collect::<rusqlite::Result<_>>()?)
}

/// Upgrade only recognized historical source structures; source rows and IDs survive.
pub fn upgrade(db: &Connection, root: &Path, version: i64) -> Result<()> {
    if !(1..=5).contains(&version) {
        bail!("Nicht unterstützte Schema-Version {version}");
    }
    let canonical = Connection::open_in_memory()?;
    canonical.execute_batch(include_str!("schema.sql"))?;
    let mut required = INVENTORY.to_vec();
    if version >= 2 {
        required.extend_from_slice(FACES);
    }
    if version >= 3 {
        required.push("training_exclusions");
    }
    for table in required {
        let expected = columns(&canonical, table)?;
        let actual = columns(db, table)?;
        if !expected.is_subset(&actual) {
            bail!(
                "Schema {version}: Quelltabelle {table} fehlt oder ist unvollständig; keine automatische Rekonstruktion von Nutzerdaten"
            );
        }
    }
    let backup = root.join(format!(
        ".myfotomanager.before-rust-v{version}-{}.bak",
        uuid::Uuid::new_v4()
    ));
    db.backup("main", backup, None)?;
    let triggers = canonical
        .prepare("SELECT name FROM sqlite_master WHERE type='trigger' ORDER BY name")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let schema = include_str!("schema.sql")
        .replace("CREATE TABLE ", "CREATE TABLE IF NOT EXISTS ")
        .replace("CREATE INDEX ", "CREATE INDEX IF NOT EXISTS ")
        .replace("CREATE TRIGGER ", "CREATE TRIGGER IF NOT EXISTS ");
    db.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<()> {
        for trigger in triggers {
            if !trigger
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                bail!("Ungültiger kanonischer Triggername");
            }
            db.execute_batch(&format!("DROP TRIGGER IF EXISTS {trigger}"))?;
        }
        db.execute_batch("DROP INDEX IF EXISTS ix_group_page;")?;
        db.execute_batch(&schema)?;
        db.execute_batch(&super::relations::seed_sql())?;
        db.execute_batch("DELETE FROM metadata_photos;
            INSERT INTO metadata_photos SELECT file_id,photo_id,relpath,name,ext,size,meta_state,dt_original,model,lens,iso,camera FROM (
                SELECT f.id file_id,f.photo_id,f.relpath,f.name,f.ext,f.size,f.meta_state,m.dt_original,m.model,m.lens,m.iso,NULLIF(TRIM(m.model),'') camera,
                ROW_NUMBER() OVER(PARTITION BY f.photo_id ORDER BY (f.meta_state='done') DESC,(m.dt_original IS NOT NULL) DESC,f.ext IN ('nef','nrw','dng') DESC,f.id) preference
                FROM files f LEFT JOIN meta m ON m.file_id=f.id WHERE f.status='present' AND f.ext!='xmp' AND f.photo_id IS NOT NULL
            ) WHERE preference=1;
            DELETE FROM face_sources;
            INSERT INTO face_sources SELECT photo_id,id FROM (
                SELECT photo_id,id,ROW_NUMBER() OVER(PARTITION BY photo_id ORDER BY CASE WHEN ext IN ('jpg','jpeg') THEN 0 WHEN ext IN ('png','tif','tiff') THEN 1 ELSE 2 END,id) preference
                FROM files WHERE status='present' AND photo_id IS NOT NULL AND ext IN ('jpg','jpeg','png','tif','tiff','nef','nrw','dng')
            ) WHERE preference=1;
            INSERT INTO projection_revision SELECT 0 WHERE NOT EXISTS(SELECT 1 FROM projection_revision);
            UPDATE projection_revision SET revision=revision+1;
            INSERT OR IGNORE INTO face_group_dirty SELECT e.embedder_id,e.face_id FROM face_embeddings e JOIN faces f ON f.id=e.face_id WHERE f.person_id IS NULL AND f.ignored=0 AND f.retained=0;
            PRAGMA user_version=6;")?;
        Ok(())
    })();
    db.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
    result
}

/// Rein additive Zusatzindizes für schnelle Zählungen (ADR 0066). Sie enthalten nur
/// abgeleitete Daten, sind in Sekunden aufgebaut und verändern keine Tabelle; Schema 6
/// bleibt unverändert. Name und Definition müssen zu `schema.sql` passen (Test).
pub const ADDITIVE_INDEXES: &[(&str, &str)] = &[
    (
        "ix_files_dir_cover",
        "CREATE INDEX IF NOT EXISTS ix_files_dir_cover ON files(dir,photo_id,id)",
    ),
    (
        "ix_faces_user_counts",
        "CREATE INDEX IF NOT EXISTS ix_faces_user_counts ON faces(person_id) WHERE assignment='user' AND retained=0",
    ),
    (
        "ix_faces_ignored",
        "CREATE INDEX IF NOT EXISTS ix_faces_ignored ON faces(id) WHERE ignored=1 AND retained=0",
    ),
    (
        "ix_faces_timeline",
        "CREATE INDEX IF NOT EXISTS ix_faces_timeline ON faces(person_id,captured_at,id) WHERE ignored=0 AND retained=0 AND assignment IN ('user','auto')",
    ),
    (
        "ix_faces_retained",
        "CREATE INDEX IF NOT EXISTS ix_faces_retained ON faces(id) WHERE retained=1",
    ),
];

/// Legt fehlende Zusatzindizes an; wiederholbar. Nur mit Schreibrecht aufrufen.
/// Gibt die Zahl neu angelegter Indizes zurück.
pub fn ensure_indexes(db: &Connection) -> Result<usize> {
    let mut created = 0;
    for (name, ddl) in ADDITIVE_INDEXES {
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name=?)",
            [name],
            |r| r.get(0),
        )?;
        if !exists {
            db.execute_batch(ddl)?;
            created += 1;
        }
    }
    Ok(created)
}
