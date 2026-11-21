use anyhow::{Context, Result, bail};
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

/// Aktuelle Schemaversion der Sammlungsdatenbank (`PRAGMA user_version` in `schema.sql`).
pub const SCHEMA_VERSION: i64 = 7;

/// Upgrade only recognized historical source structures; source rows and IDs survive.
/// Stände 1 bis 5 bauen dazu die Projektionen neu auf; Schema 6 erhält nur die
/// Datenrevision (`data_revision` samt Triggern, ersetzt `projection_revision`).
pub fn upgrade(db: &Connection, root: &Path, version: i64) -> Result<()> {
    if !(1..SCHEMA_VERSION).contains(&version) {
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
    // Datensicherung vor dem Umbau; scheitert sie, bleibt die Sammlung unverändert.
    super::backup::automatic(
        root,
        &format!("vor Schema-Migration {version} → {SCHEMA_VERSION}"),
        &super::jobs::Control::default(),
    )
    .context("Schema-Migration nicht begonnen; die Sammlung bleibt unverändert")?;
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
        // Die frühere Projektionsrevision ersetzt `data_revision`; ihre Trigger sind oben
        // durch die kanonischen ersetzt.
        db.execute_batch("DROP TABLE IF EXISTS projection_revision;")?;
        db.execute_batch("DROP INDEX IF EXISTS ix_group_page;")?;
        db.execute_batch(&schema)?;
        db.execute_batch(&super::relations::seed_sql())?;
        if version <= 5 {
            rebuild_metadata_photos(db)?;
            db.execute_batch("DELETE FROM face_sources;
            INSERT INTO face_sources SELECT photo_id,id FROM (
                SELECT photo_id,id,ROW_NUMBER() OVER(PARTITION BY photo_id ORDER BY CASE WHEN ext IN ('jpg','jpeg') THEN 0 WHEN ext IN ('png','tif','tiff') THEN 1 ELSE 2 END,id) preference
                FROM files WHERE status='present' AND photo_id IS NOT NULL AND ext IN ('jpg','jpeg','png','tif','tiff','nef','nrw','dng')
            ) WHERE preference=1;
            INSERT OR IGNORE INTO face_group_dirty SELECT e.embedder_id,e.face_id FROM face_embeddings e JOIN faces f ON f.id=e.face_id WHERE f.person_id IS NULL AND f.ignored=0 AND f.retained=0;")?;
        }
        db.execute_batch(&format!("PRAGMA user_version={SCHEMA_VERSION};"))?;
        Ok(())
    })();
    db.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
    result
}

/// Neuaufbau der Projektion `metadata_photos` (Migration), danach `folder_counts` in einer
/// Anweisung. Siehe [`rebuild_metadata_photos`].
const REBUILD_METADATA_PHOTOS: &str = "DELETE FROM metadata_photos;
            INSERT INTO metadata_photos SELECT file_id,photo_id,relpath,name,ext,size,meta_state,dt_original,model,lens,iso,camera FROM (
                SELECT f.id file_id,f.photo_id,f.relpath,f.name,f.ext,f.size,f.meta_state,m.dt_original,m.model,m.lens,m.iso,NULLIF(TRIM(m.model),'') camera,
                ROW_NUMBER() OVER(PARTITION BY f.photo_id ORDER BY (f.meta_state='done') DESC,(m.dt_original IS NOT NULL) DESC,f.ext IN ('nef','nrw','dng') DESC,f.id) preference
                FROM files f LEFT JOIN meta m ON m.file_id=f.id WHERE f.status='present' AND f.ext!='xmp' AND f.photo_id IS NOT NULL
            ) WHERE preference=1;
            DELETE FROM folder_counts;
            INSERT INTO folder_counts SELECT rtrim(rtrim(relpath,replace(relpath,'/','')),'/'),count(*) FROM metadata_photos GROUP BY 1;";

/// Baut `metadata_photos` vollständig neu auf und berechnet `folder_counts` danach in einer
/// Anweisung neu, sodass auch ein vorher inkonsistenter Stand nicht überlebt. Die
/// `folder_counts`-Trigger sind währenddessen entfernt: je Zeile gefeuert, verdoppelten sie die
/// Dauer (Messung in `examples/folder_bench.rs`). Nur innerhalb einer Schreibtransaktion
/// aufrufen; Tabelle und Trigger müssen bestehen.
pub fn rebuild_metadata_photos(db: &Connection) -> Result<()> {
    for (name, _) in &FOLDER_COUNTS[1..] {
        db.execute_batch(&format!("DROP TRIGGER IF EXISTS {name}"))?;
    }
    db.execute_batch(REBUILD_METADATA_PHOTOS)?;
    for (_, ddl) in &FOLDER_COUNTS[1..] {
        db.execute_batch(ddl)?;
    }
    Ok(())
}

/// Ordnerübersicht: Fotos (bevorzugte Variante) je Ordner, `""` ist die Wurzel. Rein additiv
/// und abgeleitet aus `metadata_photos`: Trigger auf `metadata_photos` halten sie in derselben
/// Transaktion aktuell, auch wenn eine ältere App-Version schreibt (die Trigger liegen in der
/// Datenbank). Der Ordner ist der Pfad bis zum letzten `/` von `relpath`; das entspricht
/// `files.dir`. Angelegt beim Öffnen mit Schreibrecht, ohne Versionssprung. Name und Definition
/// müssen zu `schema.sql` passen (Test). Die Tabelle steht vorn, dann die Trigger.
pub const FOLDER_COUNTS: &[(&str, &str)] = &[
    (
        "folder_counts",
        "CREATE TABLE folder_counts(dir TEXT PRIMARY KEY,photos INTEGER NOT NULL) WITHOUT ROWID",
    ),
    (
        "folder_counts_insert",
        "CREATE TRIGGER folder_counts_insert AFTER INSERT ON metadata_photos BEGIN INSERT INTO folder_counts VALUES(rtrim(rtrim(new.relpath,replace(new.relpath,'/','')),'/'),1) ON CONFLICT(dir) DO UPDATE SET photos=photos+1; END",
    ),
    (
        "folder_counts_delete",
        "CREATE TRIGGER folder_counts_delete AFTER DELETE ON metadata_photos BEGIN UPDATE folder_counts SET photos=photos-1 WHERE dir=rtrim(rtrim(old.relpath,replace(old.relpath,'/','')),'/'); DELETE FROM folder_counts WHERE dir=rtrim(rtrim(old.relpath,replace(old.relpath,'/','')),'/') AND photos<=0; END",
    ),
    (
        "folder_counts_update",
        "CREATE TRIGGER folder_counts_update AFTER UPDATE OF relpath ON metadata_photos BEGIN UPDATE folder_counts SET photos=photos-1 WHERE dir=rtrim(rtrim(old.relpath,replace(old.relpath,'/','')),'/'); DELETE FROM folder_counts WHERE dir=rtrim(rtrim(old.relpath,replace(old.relpath,'/','')),'/') AND photos<=0; INSERT INTO folder_counts VALUES(rtrim(rtrim(new.relpath,replace(new.relpath,'/','')),'/'),1) ON CONFLICT(dir) DO UPDATE SET photos=photos+1; END",
    ),
];

/// Erstbefüllung von `folder_counts` (eine Anweisung, gleiche Ordnerformel wie die Trigger).
const FOLDER_COUNTS_FILL: &str = "INSERT INTO folder_counts SELECT rtrim(rtrim(relpath,replace(relpath,'/','')),'/'),count(*) FROM metadata_photos GROUP BY 1";

/// Legt `folder_counts` samt Triggern an und befüllt sie, alles in einer Transaktion; nur mit
/// Schreibrecht aufrufen. Fehlt ein Teil (etwa ein von Hand gelöschter Trigger), wird alles neu
/// aufgebaut, weil die Zahlen dann nicht mehr verlässlich sind. Gibt zurück, ob gebaut wurde.
pub fn ensure_folder_counts(db: &Connection) -> Result<bool> {
    let mut present = 0;
    for (name, _) in FOLDER_COUNTS {
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type IN ('table','trigger') AND name=?)",
            [name],
            |r| r.get(0),
        )?;
        present += usize::from(exists);
    }
    if present == FOLDER_COUNTS.len() {
        return Ok(false);
    }
    db.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<()> {
        for (name, _) in &FOLDER_COUNTS[1..] {
            db.execute_batch(&format!("DROP TRIGGER IF EXISTS {name}"))?;
        }
        db.execute_batch("DROP TABLE IF EXISTS folder_counts")?;
        for (_, ddl) in FOLDER_COUNTS {
            db.execute_batch(ddl)?;
        }
        db.execute_batch(FOLDER_COUNTS_FILL)?;
        Ok(())
    })();
    db.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
    result.map(|()| true)
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
        "ix_faces_not_face",
        "CREATE INDEX IF NOT EXISTS ix_faces_not_face ON faces(id) WHERE ignored=2 AND retained=0",
    ),
    (
        "ix_faces_retained",
        "CREATE INDEX IF NOT EXISTS ix_faces_retained ON faces(id) WHERE retained=1",
    ),
    (
        "ix_jobs_detect_done",
        "CREATE INDEX IF NOT EXISTS ix_jobs_detect_done ON face_jobs(model_id,photo_id) WHERE stage='detect' AND status='done'",
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
