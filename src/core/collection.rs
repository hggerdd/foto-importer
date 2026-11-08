use super::{files, jobs::Control};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

pub struct Collection {
    pub db: Connection,
    pub root: PathBuf,
    lock: Option<WriterLock>,
}
struct WriterLock(PathBuf);
impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
impl Collection {
    pub fn open(root: &Path, write: bool) -> Result<Self> {
        let root = root
            .canonicalize()
            .context("Sammlung ist nicht erreichbar")?;
        if !root.is_dir() {
            bail!("Sammlung ist kein Ordner");
        }
        let path = root.join(".myfotomanager");
        let lock_path = root.join(".myfotomanager.lock");
        let lock = if write {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&lock_path)
                .context("Sammlung ist durch eine andere Instanz gesperrt; vorhandene Sperren bleiben erhalten")?;
            if let Err(error) = write!(
                file,
                "{}",
                serde_json::json!({"host": std::env::var("COMPUTERNAME").unwrap_or_default(), "pid": std::process::id()})
            ) {
                drop(file);
                let _ = fs::remove_file(&lock_path);
                return Err(error.into());
            }
            Some(lock_path)
        } else {
            None
        };
        let opened = (|| -> Result<Connection> {
            let db = Connection::open_with_flags(
                &path,
                if write {
                    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
                } else {
                    OpenFlags::SQLITE_OPEN_READ_ONLY
                },
            )?;
            db.busy_timeout(Duration::from_secs(5))?;
            db.execute_batch("PRAGMA cache_size=-16384; PRAGMA temp_store=FILE;")?;
            let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            if version == 0 && write {
                let tables:i64=db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",[],|r|r.get(0))?;
                if tables > 0 {
                    bail!("Unversionierte Bestandsdatenbank wird nicht automatisch verändert");
                }
                db.execute_batch("PRAGMA journal_mode=WAL; BEGIN IMMEDIATE;")?;
                db.execute_batch(include_str!("schema.sql"))?;
                db.execute(
                    "INSERT INTO collection VALUES(?,datetime('now'),'rust-0.1')",
                    [uuid::Uuid::new_v4().to_string()],
                )?;
                db.execute("INSERT INTO projection_revision VALUES(0)", [])?;
                db.execute_batch(&format!("{} COMMIT;", super::relations::seed_sql()))?;
            } else if write && (1..=5).contains(&version) {
                super::migrations::upgrade(&db, &root, version)?;
            } else if version != 6 && !(!write && (4..=5).contains(&version)) {
                bail!(
                    "Schema {version} benötigt ein unterstütztes Upgrade. Sammlung über Bestand aktualisieren öffnen."
                );
            }
            if write {
                db.execute_batch("PRAGMA journal_mode=WAL;")?;
                // Neue Beziehungstypen sind nur Zeilen; ältere Sammlungen erhalten sie beim
                // Öffnen mit Schreibrecht. Schreibgeschützt bleibt alles unverändert.
                db.execute_batch(&super::relations::seed_sql())?;
            }
            let flags = rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
                | rusqlite::functions::FunctionFlags::SQLITE_UTF8;
            db.create_scalar_function("casefold", 1, flags, |ctx| {
                Ok(files::fold(
                    &ctx.get::<Option<String>>(0)?.unwrap_or_default(),
                ))
            })?;
            // Missing schema objects are a visible error, never silently rebuilt.
            db.prepare("SELECT photo_id FROM metadata_photos LIMIT 1")?;
            Ok(db)
        })();
        match opened {
            Ok(db) => Ok(Self {
                db,
                root,
                lock: lock.map(WriterLock),
            }),
            Err(e) => {
                if let Some(lock) = lock {
                    let _ = fs::remove_file(lock);
                }
                Err(e)
            }
        }
    }
    pub fn writable(&self) -> Result<()> {
        if self.lock.is_none() {
            bail!("Sammlung ist nur lesbar");
        }
        Ok(())
    }
    pub fn cancel_with(&self, control: &Control) {
        let cancel = control.cancel.clone();
        self.db.progress_handler(
            1000,
            Some(move || cancel.load(std::sync::atomic::Ordering::Relaxed)),
        );
    }
    pub fn safe_path(&self, relative: &str) -> Result<PathBuf> {
        let path = Path::new(relative);
        if path
            .components()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
        {
            bail!("Ungültiger relativer Dateipfad");
        }
        let candidate = self.root.join(path);
        if candidate.symlink_metadata()?.file_type().is_symlink() {
            bail!("Symbolische Links werden nicht geöffnet");
        }
        let resolved = candidate.canonicalize()?;
        if !resolved.starts_with(&self.root) {
            bail!("Datei liegt außerhalb der Sammlung");
        }
        Ok(resolved)
    }
    pub fn backup(&self) -> Result<()> {
        self.writable()?;
        let temp = tempfile::NamedTempFile::new_in(&self.root)?;
        self.db.backup("main", temp.path(), None)?;
        temp.persist(self.root.join(".myfotomanager.backup"))?;
        Ok(())
    }
    pub fn status(&self) -> Result<CollectionStatus> {
        Ok(CollectionStatus {
            photos: self
                .db
                .query_row("SELECT count(*) FROM metadata_photos", [], |r| r.get(0))?,
            files: self.db.query_row(
                "SELECT count(*) FROM files WHERE status='present'",
                [],
                |r| r.get(0),
            )?,
            missing: self.db.query_row(
                "SELECT count(*) FROM files WHERE status='missing'",
                [],
                |r| r.get(0),
            )?,
            pending: self.db.query_row(
                "SELECT count(*) FROM files WHERE status='present' AND meta_state!='done'",
                [],
                |r| r.get(0),
            )?,
        })
    }
    pub fn list_option(&self, key: &str, default: &[&str]) -> Result<Vec<String>> {
        let raw: Option<String> = self
            .db
            .query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(raw
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_else(|| default.iter().map(|v| v.to_string()).collect()))
    }
    pub fn configure(&self, key: &str, value: serde_json::Value) -> Result<()> {
        self.writable()?;
        self.db.execute(
            "INSERT OR REPLACE INTO settings VALUES(?,?)",
            params![key, value.to_string()],
        )?;
        Ok(())
    }

    /// An incomplete scan never marks unvisited files as missing.
    pub fn quick_scan(&mut self, allow_missing: bool, control: &Control) -> Result<String> {
        self.writable()?;
        let mut extensions = self.list_option(
            "extensions",
            &[
                "nef", "nrw", "dng", "jpg", "jpeg", "heic", "tif", "tiff", "png", "mov", "mp4",
            ],
        )?;
        if !extensions.iter().any(|e| e == "xmp") {
            extensions.push("xmp".into());
        }
        let exclusions = self.list_option("exclusions", &[])?;
        let extension_folders = self.list_option("extension_folders", crate::config::SUPPORTED)?;
        let export_folders = self.list_option("export_folders", &["Export"])?;
        let scan =
            files::scan_filtered(&self.root, false, &extensions, &exclusions, true, control)?;
        if scan.files.is_empty() {
            bail!("Keine unterstützten Dateien gefunden; vorhandener Bestand bleibt unverändert.");
        }
        let tx = self.db.transaction()?;
        tx.execute("INSERT INTO scans(kind,scope,started_at,status) VALUES('quick','',datetime('now'),'running')", [])?;
        let scan_id = tx.last_insert_rowid();
        let mut added = 0;
        let mut moved = 0;
        for (index, source) in scan.files.iter().enumerate() {
            control.checkpoint()?;
            let rel = files::normalize(source.path.strip_prefix(&self.root)?);
            let stat = source.path.metadata()?;
            let mtime = stat
                .modified()?
                .duration_since(UNIX_EPOCH)?
                .as_nanos()
                .min(i64::MAX as u128) as i64;
            let name = source
                .path
                .file_name()
                .context("Dateiname fehlt")?
                .to_string_lossy()
                .into_owned();
            let stem = source
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let ext = files::extension(&source.path);
            let dir = rel.rsplit_once('/').map(|v| v.0).unwrap_or("");
            let inode = file_id(&source.path);
            let mut old: Option<(i64, i64, i64)> = tx
                .query_row(
                    "SELECT id,size,mtime_ns FROM files WHERE relpath=?",
                    [&rel],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            if old.is_none() && inode.is_some() {
                let candidates = tx
                    .prepare("SELECT id,size,mtime_ns,relpath FROM files WHERE inode=? AND size=?")?
                    .query_map(params![inode, source.size as i64], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let candidates: Vec<_> = candidates
                    .into_iter()
                    .filter(|r| !self.root.join(&r.3).exists())
                    .collect();
                if let [candidate] = candidates.as_slice() {
                    old = Some((candidate.0, candidate.1, candidate.2));
                    moved += 1;
                    tx.execute("INSERT INTO changes(scan_id,kind,file_id,old_relpath,new_relpath,at) VALUES(?,'moved',?,?,?,datetime('now'))", params![scan_id,candidate.0,candidate.3,rel])?;
                }
            }
            if old.is_none() {
                let candidates=tx.prepare("SELECT id,size,mtime_ns,relpath,full_hash FROM files WHERE size=? AND full_hash IS NOT NULL")?.query_map([source.size as i64],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                let candidates: Vec<_> = candidates
                    .into_iter()
                    .filter(|r| !self.root.join(&r.3).exists())
                    .collect();
                if !candidates.is_empty() {
                    let hash = content_hash(&source.path, control)?;
                    let matches: Vec<_> = candidates.into_iter().filter(|r| r.4 == hash).collect();
                    if let [candidate] = matches.as_slice() {
                        old = Some((candidate.0, candidate.1, candidate.2));
                        moved += 1;
                        tx.execute("INSERT INTO changes(scan_id,kind,file_id,old_relpath,new_relpath,at) VALUES(?,'moved',?,?,?,datetime('now'))",params![scan_id,candidate.0,candidate.3,rel])?;
                    }
                }
            }
            if let Some((id, size, time)) = old {
                let previous: String =
                    tx.query_row("SELECT relpath FROM files WHERE id=?", [id], |r| r.get(0))?;
                let changed = size != source.size as i64 || time != mtime;
                tx.execute("UPDATE files SET relpath=?,relpath_fold=?,dir=?,name=?,stem=?,ext=?,size=?,mtime_ns=?,inode=?,last_seen_scan=?,status='present',meta_state=CASE WHEN ? THEN 'stale' ELSE meta_state END,quick_hash=CASE WHEN ? THEN NULL ELSE quick_hash END WHERE id=?", params![rel,files::fold(&rel),dir,name,stem,ext,source.size as i64,mtime,inode,scan_id,changed,changed,id])?;
                if previous != rel {
                    tx.execute("UPDATE faces SET source_relpath=? WHERE source_file_id=? AND source_relpath=?",params![rel,id,previous])?;
                }
            } else {
                // Pair variants only in the same family, never in Export folders.
                let family = family_directory(dir, &extension_folders);
                let photo: Option<i64> = if dir.split('/').any(|v| {
                    export_folders
                        .iter()
                        .any(|f| files::fold(f) == files::fold(v))
                }) {
                    None
                } else {
                    let candidates=tx.prepare("SELECT photo_id,dir FROM files WHERE casefold(stem)=? AND photo_id IS NOT NULL ORDER BY id")?.query_map([files::fold(&stem)],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                    candidates
                        .into_iter()
                        .find(|(_, directory)| {
                            files::fold(&family_directory(directory, &extension_folders))
                                == files::fold(&family)
                        })
                        .map(|r| r.0)
                };
                let photo = match photo {
                    Some(p) => p,
                    None => {
                        tx.execute(
                            "INSERT INTO photos(status,created_scan) VALUES('present',?)",
                            [scan_id],
                        )?;
                        tx.last_insert_rowid()
                    }
                };
                tx.execute("INSERT INTO files(relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,inode,photo_id,status,first_seen_scan,last_seen_scan) VALUES(?,?,?,?,?,?,?,?,?,?,'present',?,?)", params![rel,files::fold(&rel),dir,name,stem,ext,source.size as i64,mtime,inode,photo,scan_id,scan_id])?;
                added += 1;
            }
            control.progress(index + 1, scan.files.len());
        }
        let missing: i64 = tx.query_row(
            "SELECT count(*) FROM files WHERE status='present' AND last_seen_scan!=?",
            [scan_id],
            |r| r.get(0),
        )?;
        let total: i64 = tx.query_row(
            "SELECT count(*) FROM files WHERE status='present'",
            [],
            |r| r.get(0),
        )?;
        let mark_missing = scan.warnings.is_empty() && (allow_missing || missing * 5 <= total);
        if mark_missing {
            tx.execute(
                "UPDATE files SET status='missing' WHERE last_seen_scan!=?",
                [scan_id],
            )?;
        }
        tx.execute("UPDATE photos SET status=CASE WHEN EXISTS(SELECT 1 FROM files WHERE photo_id=photos.id AND status='present') THEN 'present' ELSE 'missing' END",[])?;
        tx.execute("UPDATE scans SET status='done',finished_at=datetime('now'),stats_json=? WHERE id=?", params![serde_json::json!({"added":added,"moved":moved,"unvisited":missing,"marked_missing":mark_missing}).to_string(),scan_id])?;
        control.checkpoint()?;
        tx.commit()?;
        self.backup()?;
        Ok(format!(
            "{} Dateien · {added} neu · {moved} verschoben · {missing} nicht gesehen{}{}",
            scan.files.len(),
            if mark_missing {
                ""
            } else {
                " (nicht als fehlend markiert; Prüfung erforderlich)"
            },
            if scan.warnings.is_empty() {
                String::new()
            } else {
                format!(" · {} Lesefehler", scan.warnings.len())
            }
        ))
    }

    pub fn verify(&mut self, control: &Control) -> Result<String> {
        self.writable()?;
        let rows = self
            .db
            .prepare("SELECT id,relpath,full_hash FROM files WHERE status='present' ORDER BY id")?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut mismatches = Vec::new();
        for (index, (id, rel, old)) in rows.iter().enumerate() {
            control.checkpoint()?;
            let hash = content_hash(&self.safe_path(rel)?, control)?;
            if let Some(old) = old {
                if old != &hash {
                    mismatches.push(rel.clone());
                }
            } else {
                self.db
                    .execute("UPDATE files SET full_hash=? WHERE id=?", params![hash, id])?;
            }
            control.progress(index + 1, rows.len());
        }
        self.backup()?;
        if !mismatches.is_empty() {
            bail!(
                "{} Abweichungen; Vergleichswerte blieben erhalten: {}",
                mismatches.len(),
                mismatches.join(", ")
            );
        }
        Ok(format!(
            "{} Dateien mit BLAKE3 geprüft; keine Abweichung",
            rows.len()
        ))
    }
    pub fn purge_missing(&mut self, age: i64) -> Result<String> {
        self.writable()?;
        if age < 0 {
            bail!("Scanabstand darf nicht negativ sein");
        }
        let tx = self.db.transaction()?;
        let threshold: i64 =
            tx.query_row("SELECT coalesce(max(id),0)-? FROM scans", [age], |r| {
                r.get(0)
            })?;
        tx.execute("DELETE FROM meta WHERE file_id IN (SELECT id FROM files WHERE status='missing' AND last_seen_scan<=?)",[threshold])?;
        let count = tx.execute(
            "DELETE FROM files WHERE status='missing' AND last_seen_scan<=?",
            [threshold],
        )?;
        tx.commit()?;
        self.backup()?;
        Ok(format!(
            "{count} fehlende Dateieinträge entfernt; Foto-Identitäten bleiben erhalten"
        ))
    }
    pub fn inventory_report(&self) -> Result<String> {
        let missing = self
            .db
            .prepare("SELECT relpath FROM files WHERE status='missing' ORDER BY relpath LIMIT 200")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let changes=self.db.prepare("SELECT kind,coalesce(old_relpath,''),coalesce(new_relpath,'') FROM changes ORDER BY id DESC LIMIT 200")?.query_map([],|r|Ok(format!("{}: {} → {}",r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(format!(
            "Fehlende Dateien (höchstens 200):\n{}\n\nLetzte Änderungen (höchstens 200):\n{}",
            missing.join("\n"),
            changes.join("\n")
        ))
    }
    pub fn resolve_move(&mut self, old: &str, new: &str, control: &Control) -> Result<String> {
        self.writable()?;
        let old = files::normalize(Path::new(old));
        let new = files::normalize(Path::new(new));
        let old_row: (i64, i64, Option<String>, Option<String>) = self.db.query_row(
            "SELECT id,size,quick_hash,full_hash FROM files WHERE relpath=?",
            [&old],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let new_row: (i64, i64, i64, Option<String>) = self.db.query_row(
            "SELECT id,size,mtime_ns,inode FROM files WHERE relpath=? AND status='present'",
            [&new],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        if old_row.0 == new_row.0 || self.root.join(&old).exists() || old_row.1 != new_row.1 {
            bail!("Alter Pfad muss fehlen; neuer Pfad und Größe müssen passen");
        }
        let path = self.safe_path(&new)?;
        let verified = if let Some(hash) = old_row.2 {
            hash == super::import_service::quick_hash(&path, control)?
        } else if let Some(hash) = old_row.3 {
            hash == content_hash(&path, control)?
        } else {
            false
        };
        if !verified {
            bail!("Kein passender früher gespeicherter Inhaltsfingerabdruck");
        }
        let directory = new.rsplit_once('/').map(|v| v.0).unwrap_or("");
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let tx = self.db.transaction()?;
        tx.execute("INSERT INTO scans(kind,scope,started_at,status) VALUES('quick','',datetime('now'),'running')",[])?;
        let scan = tx.last_insert_rowid();
        tx.execute("DELETE FROM meta WHERE file_id=?", [new_row.0])?;
        tx.execute("DELETE FROM files WHERE id=?", [new_row.0])?;
        tx.execute("UPDATE files SET relpath=?,relpath_fold=?,dir=?,name=?,stem=?,ext=?,size=?,mtime_ns=?,inode=?,last_seen_scan=?,status='present' WHERE id=?",params![new,files::fold(&new),directory,name,stem,files::extension(&path),new_row.1,new_row.2,new_row.3,scan,old_row.0])?;
        tx.execute(
            "UPDATE faces SET source_relpath=? WHERE source_file_id=? AND source_relpath=?",
            params![new, old_row.0, old],
        )?;
        tx.execute("INSERT INTO changes(scan_id,kind,file_id,old_relpath,new_relpath,at) VALUES(?,'moved',?,?,?,datetime('now'))",params![scan,old_row.0,old,new])?;
        tx.execute("UPDATE photos SET status=CASE WHEN EXISTS(SELECT 1 FROM files WHERE photo_id=photos.id AND status='present') THEN 'present' ELSE 'missing' END",[])?;
        tx.execute(
            "UPDATE scans SET status='done',finished_at=datetime('now') WHERE id=?",
            [scan],
        )?;
        tx.commit()?;
        self.backup()?;
        Ok("Verschiebung mit bestehender Foto-Identität zugeordnet".into())
    }
}
fn family_directory(directory: &str, extensions: &[String]) -> String {
    let leaf = directory.rsplit('/').next().unwrap_or("");
    if extensions
        .iter()
        .any(|e| files::fold(e) == files::fold(leaf))
    {
        directory
            .rsplit_once('/')
            .map(|v| v.0)
            .unwrap_or("")
            .to_string()
    } else {
        directory.to_string()
    }
}

#[derive(Default, Clone)]
pub struct CollectionStatus {
    pub photos: usize,
    pub files: usize,
    pub missing: usize,
    pub pending: usize,
}

pub fn content_hash(path: &Path, control: &Control) -> Result<String> {
    let mut file = File::open(path)?;
    let before = file.metadata()?;
    let mut hash = blake3::Hasher::new();
    let mut buffer = vec![0u8; crate::config::COPY_CHUNK];
    loop {
        control.checkpoint()?;
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let after = file.metadata()?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        bail!("Datei wurde während der Prüfung verändert");
    }
    Ok(hash.finalize().to_hex().to_string())
}

fn file_id(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        struct Info {
            attributes: u32,
            creation: [u32; 2],
            access: [u32; 2],
            write: [u32; 2],
            volume: u32,
            size_high: u32,
            size_low: u32,
            links: u32,
            index_high: u32,
            index_low: u32,
        }
        unsafe extern "system" {
            fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut Info) -> i32;
        }
        let file = File::open(path).ok()?;
        let mut info = std::mem::MaybeUninit::<Info>::uninit();
        // SAFETY: valid owned file handle and writable, correctly laid-out Win32 structure.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return None;
        }
        // SAFETY: successful Win32 call initialized the structure.
        let info = unsafe { info.assume_init() };
        Some((((info.index_high as u64) << 32) | info.index_low as u64).to_string())
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::MetadataExt;
        path.metadata().ok().map(|m| m.ino().to_string())
    }
}
