use super::{
    files,
    jobs::Control,
    known_files::{KnownFiles, Signature, Unseen},
};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
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
        let started = std::time::Instant::now();
        let opened = Self::open_inner(root, write);
        let elapsed = started.elapsed();
        match &opened {
            Err(e) => log::warn!(
                target: "db",
                "oeffnen_fehler schreibend={write} dauer_ms={:.1} fehler={:?}",
                super::diag::millis(elapsed),
                format!("{e:#}")
            ),
            Ok(_) if elapsed.as_millis() >= crate::config::DIAG_SLOW_DB_OPEN_MS => log::info!(
                target: "db",
                "oeffnen_langsam schreibend={write} dauer_ms={:.1}",
                super::diag::millis(elapsed)
            ),
            Ok(_) => {}
        }
        opened
    }
    fn open_inner(root: &Path, write: bool) -> Result<Self> {
        let root = root
            .canonicalize()
            .context("Sammlung ist nicht erreichbar")?;
        if !root.is_dir() {
            bail!("Sammlung ist kein Ordner");
        }
        let path = root.join(".myfotomanager");
        // Zweite Schutzschicht: auf einer Speicherkarte entsteht nie eine neue Sammlung,
        // auch nicht über Wege an der Ordnerwahl vorbei. Vorhandene öffnen weiter.
        if write && !path.is_file() && on_memory_card(&path_key(&root)) {
            bail!("{MEMORY_CARD}");
        }
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
            if !write {
                // Lesende Verbindungen sind kurzlebig und beginnen mit leerem Seitencache;
                // Speicherabbildung vermeidet das erneute Kopieren der Seiten (ADR 0066).
                db.execute_batch("PRAGMA mmap_size=536870912;")?;
            }
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
                let mut span = super::diag::span("db", "migration");
                span.field("von_schema", version);
                super::migrations::upgrade(&db, &root, version)?;
            } else if version != 6 && !(!write && (4..=5).contains(&version)) {
                bail!(
                    "Schema {version} benötigt ein unterstütztes Upgrade. Sammlung über Bestand aktualisieren öffnen."
                );
            }
            if write {
                db.execute_batch("PRAGMA journal_mode=WAL;")?;
                super::migrations::ensure_indexes(&db)?;
                super::migrations::ensure_folder_counts(&db)?;
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
    /// Ist die Sammlung selbst noch erreichbar? Fehlt die Datenbankdatei (Laufwerk getrennt),
    /// darf keine einzelne Datei als fehlend gelten.
    pub fn reachable(&self) -> bool {
        self.root.join(".myfotomanager").is_file()
    }

    /// Markiert Dateien, die beim Lesen nicht mehr gefunden wurden, als fehlend (`status`
    /// 'missing'). Zeilen, Foto-Identitäten, Gesichter und Entscheidungen bleiben; ein späterer
    /// Bestandslauf findet die Dateien am alten oder (über die Umbenennungserkennung) am neuen
    /// Ort wieder. `during` nennt den Lauf im Diagnose-Log.
    pub fn mark_missing(&mut self, rows: &[(i64, String)], during: &str) -> Result<usize> {
        self.writable()?;
        if rows.is_empty() {
            return Ok(0);
        }
        if !self.reachable() {
            bail!("Sammlung ist nicht mehr erreichbar; keine Datei wurde als fehlend markiert");
        }
        let tx = self.db.transaction()?;
        let mut marked = 0;
        for (id, relpath) in rows {
            let changed = tx.execute(
                "UPDATE files SET status='missing' WHERE id=? AND status='present'",
                [id],
            )?;
            if changed == 0 {
                continue;
            }
            marked += 1;
            tx.execute("INSERT INTO changes(scan_id,kind,file_id,old_relpath,new_relpath,at) VALUES(NULL,'missing',?,?,NULL,datetime('now'))", params![id, relpath])?;
            tx.execute("UPDATE photos SET status=CASE WHEN EXISTS(SELECT 1 FROM files WHERE photo_id=photos.id AND status='present') THEN 'present' ELSE 'missing' END WHERE id=(SELECT photo_id FROM files WHERE id=?)", [id])?;
        }
        tx.commit()?;
        if marked > 0 {
            log::warn!(
                target: "sammlung",
                "fehlend_markiert lauf={during:?} anzahl={marked} beispiel={:?}",
                rows[0].1
            );
        }
        Ok(marked)
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
            unread: self.db.query_row(
                "SELECT count(*) FROM files WHERE status='present' AND meta_state IN ('pending','stale')",
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
    ///
    /// Files are entered in batches of `config::SCAN_BATCH` per transaction (ADR 0064): the
    /// collection becomes usable during the run and a cancelled run keeps what was entered.
    pub fn quick_scan(&mut self, allow_missing: bool, control: &Control) -> Result<String> {
        self.quick_scan_batched(allow_missing, control, crate::config::SCAN_BATCH)
    }

    pub fn quick_scan_batched(
        &mut self,
        allow_missing: bool,
        control: &Control,
        batch: usize,
    ) -> Result<String> {
        self.writable()?;
        // Statements are interrupted on cancel too; the handler never outlives the run.
        self.cancel_with(control);
        let result = self.index_run(allow_missing, control, batch);
        self.clear_cancel();
        result
    }

    fn clear_cancel(&self) {
        self.db.progress_handler(0, None::<fn() -> bool>);
    }

    fn index_run(
        &mut self,
        allow_missing: bool,
        control: &Control,
        batch: usize,
    ) -> Result<String> {
        let batch = batch.max(1);
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
        control.phase("Schritt 1 von 4 · Ordner lesen", "Dateien");
        let scan =
            files::scan_filtered(&self.root, false, &extensions, &exclusions, true, control)?;
        if scan.files.is_empty() {
            bail!("Keine unterstützten Dateien gefunden; vorhandener Bestand bleibt unverändert.");
        }
        let total = scan.files.len();
        control.phase("Schritt 2 von 4 · Bestand vergleichen", "Dateien");
        let relpaths = scan
            .files
            .iter()
            .map(|source| Ok(files::normalize(source.path.strip_prefix(&self.root)?)))
            .collect::<Result<Vec<String>>>()?;
        let mut known = KnownFiles::load(&self.db, &relpaths, &extension_folders, control)?;
        known.note_walked(&relpaths, scan.files.iter().map(|f| f.size as i64));
        let scan_id = {
            let tx = self.db.transaction()?;
            tx.execute("INSERT INTO scans(kind,scope,started_at,status) VALUES('quick','',datetime('now'),'running')", [])?;
            let id = tx.last_insert_rowid();
            tx.commit()?;
            id
        };
        control.phase("Schritt 3 von 4 · Dateien eintragen", "Dateien");
        control.progress(0, total);
        let entered = self.enter_files(
            &scan,
            &relpaths,
            scan_id,
            batch,
            &mut known,
            &extension_folders,
            &export_folders,
            control,
        );
        let (added, moved, ambiguous) = match entered {
            Ok(counts) => counts,
            Err(error) => {
                // Entered batches stay; nothing is marked missing and the scan is closed visibly.
                self.clear_cancel();
                let status = if control.cancel.load(Ordering::Relaxed) {
                    "cancelled"
                } else {
                    "failed"
                };
                let _ = self.close_scan(scan_id, status);
                return Err(error);
            }
        };
        control.phase("Schritt 4 von 4 · Abschließen", "");
        control.checkpoint()?;
        let tx = self.db.transaction()?;
        let missing: i64 = tx.query_row(
            "SELECT count(*) FROM files WHERE status='present' AND last_seen_scan!=?",
            [scan_id],
            |r| r.get(0),
        )?;
        let present: i64 = tx.query_row(
            "SELECT count(*) FROM files WHERE status='present'",
            [],
            |r| r.get(0),
        )?;
        let mark_missing = scan.warnings.is_empty() && (allow_missing || missing * 5 <= present);
        if mark_missing {
            tx.execute(
                "UPDATE files SET status='missing' WHERE last_seen_scan!=?",
                [scan_id],
            )?;
        }
        // Only rows whose state changes are written; the result equals the full rewrite.
        tx.execute("UPDATE photos SET status=CASE WHEN EXISTS(SELECT 1 FROM files WHERE photo_id=photos.id AND status='present') THEN 'present' ELSE 'missing' END WHERE status IS NOT CASE WHEN EXISTS(SELECT 1 FROM files WHERE photo_id=photos.id AND status='present') THEN 'present' ELSE 'missing' END",[])?;
        tx.execute("UPDATE scans SET status='done',finished_at=datetime('now'),stats_json=? WHERE id=?", params![serde_json::json!({"added":added,"moved":moved,"ambiguous":ambiguous,"unvisited":missing,"marked_missing":mark_missing}).to_string(),scan_id])?;
        control.checkpoint()?;
        tx.commit()?;
        if moved > 0 || ambiguous > 0 {
            log::info!(
                target: "sammlung",
                "bestand verschoben={moved} mehrdeutig={ambiguous} nicht_gesehen={missing}"
            );
        }
        control.phase("Schritt 4 von 4 · Sicherung schreiben", "");
        self.backup()?;
        Ok(format!(
            "{} Dateien · {added} neu · {moved} verschoben{} · {missing} nicht gesehen{}{}",
            total,
            if ambiguous > 0 {
                format!(" · {ambiguous} mehrdeutig, nicht zugeordnet")
            } else {
                String::new()
            },
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

    /// Close an unfinished scan without marking anything missing.
    fn close_scan(&mut self, scan_id: i64, status: &str) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("UPDATE photos SET status=CASE WHEN EXISTS(SELECT 1 FROM files WHERE photo_id=photos.id AND status='present') THEN 'present' ELSE 'missing' END WHERE status IS NOT CASE WHEN EXISTS(SELECT 1 FROM files WHERE photo_id=photos.id AND status='present') THEN 'present' ELSE 'missing' END",[])?;
        tx.execute(
            "UPDATE scans SET status=?,finished_at=datetime('now') WHERE id=?",
            params![status, scan_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
    fn enter_files(
        &mut self,
        scan: &files::Scan,
        relpaths: &[String],
        scan_id: i64,
        batch: usize,
        known: &mut KnownFiles,
        extension_folders: &[String],
        export_folders: &[String],
        control: &Control,
    ) -> Result<(usize, usize, usize)> {
        let (mut added, mut moved, mut ambiguous) = (0, 0, 0);
        let total = scan.files.len();
        let mut position = 0;
        while position < total {
            let end = (position + batch).min(total);
            let tx = self.db.transaction()?;
            for index in position..end {
                control.checkpoint()?;
                let source = &scan.files[index];
                let rel = &relpaths[index];
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
                let size = source.size as i64;
                let mut old: Option<(i64, i64, i64)> = None;
                let mut stored: Option<(String, Option<String>)> = None;
                if let Some(row) = tx
                    .prepare_cached(
                        "SELECT id,size,mtime_ns,status,inode FROM files WHERE relpath=?",
                    )?
                    .query_row([rel], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                    })
                    .optional()?
                {
                    old = Some((row.0, row.1, row.2));
                    stored = Some((row.3, row.4));
                }
                let mut previous = rel.clone();
                if old.is_none()
                    && let Some(found) = known.moved_by_inode(&self.root, inode.as_deref(), size)
                {
                    old = Some((found.id, found.size, found.mtime));
                    previous = found.relpath.clone();
                    moved += 1;
                    tx.execute("INSERT INTO changes(scan_id,kind,file_id,old_relpath,new_relpath,at) VALUES(?,'moved',?,?,?,datetime('now'))", params![scan_id,found.id,found.relpath,rel])?;
                    known.relocate(&found, &stem, dir, extension_folders);
                }
                if old.is_none() {
                    match known.moved_by_signature(&self.root, rel, size, mtime) {
                        Signature::Unique(found)
                            if signature_confirmed(&found, &source.path, control)? =>
                        {
                            old = Some((found.id, found.size, found.mtime));
                            previous = found.relpath.clone();
                            moved += 1;
                            tx.execute("INSERT INTO changes(scan_id,kind,file_id,old_relpath,new_relpath,at) VALUES(?,'moved',?,?,?,datetime('now'))", params![scan_id,found.id,found.relpath,rel])?;
                            known.relocate(&found, &stem, dir, extension_folders);
                        }
                        Signature::Ambiguous => ambiguous += 1,
                        _ => {}
                    }
                }
                if old.is_none() && known.has_hashed(size) {
                    let candidates = known.hashed_unseen(&self.root, size);
                    if !candidates.is_empty() {
                        let hash = content_hash(&source.path, control)?;
                        let matches: Vec<_> = candidates
                            .into_iter()
                            .filter(|r| r.full_hash.as_deref() == Some(hash.as_str()))
                            .collect();
                        if let [found] = matches.as_slice() {
                            old = Some((found.id, found.size, found.mtime));
                            previous = found.relpath.clone();
                            moved += 1;
                            tx.execute("INSERT INTO changes(scan_id,kind,file_id,old_relpath,new_relpath,at) VALUES(?,'moved',?,?,?,datetime('now'))",params![scan_id,found.id,found.relpath,rel])?;
                            known.relocate(found, &stem, dir, extension_folders);
                        }
                    }
                }
                if let Some((id, old_size, time)) = old {
                    let changed = old_size != size || time != mtime;
                    let unchanged_in_place = !changed
                        && previous == *rel
                        && stored
                            .as_ref()
                            .is_some_and(|(status, node)| status == "present" && *node == inode);
                    if unchanged_in_place {
                        // Nothing but the visit mark changes; this does not fire the projection triggers.
                        tx.prepare_cached("UPDATE files SET last_seen_scan=? WHERE id=?")?
                            .execute(params![scan_id, id])?;
                    } else {
                        tx.prepare_cached("UPDATE files SET relpath=?,relpath_fold=?,dir=?,name=?,stem=?,ext=?,size=?,mtime_ns=?,inode=?,last_seen_scan=?,status='present',meta_state=CASE WHEN ? THEN 'stale' ELSE meta_state END,quick_hash=CASE WHEN ? THEN NULL ELSE quick_hash END WHERE id=?")?.execute(params![rel,files::fold(rel),dir,name,stem,ext,size,mtime,inode,scan_id,changed,changed,id])?;
                    }
                    if previous != *rel {
                        tx.execute("UPDATE faces SET source_relpath=? WHERE source_file_id=? AND source_relpath=?",params![rel,id,previous])?;
                    }
                } else {
                    // Pair variants only in the same family, never in Export folders.
                    let family = family_directory(dir, extension_folders);
                    let photo = if dir.split('/').any(|v| {
                        export_folders
                            .iter()
                            .any(|f| files::fold(f) == files::fold(v))
                    }) {
                        None
                    } else {
                        known.family_photo(&stem, &family)
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
                    tx.prepare_cached("INSERT INTO files(relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,inode,photo_id,status,first_seen_scan,last_seen_scan) VALUES(?,?,?,?,?,?,?,?,?,?,'present',?,?)")?.execute(params![rel,files::fold(rel),dir,name,stem,ext,size,mtime,inode,photo,scan_id,scan_id])?;
                    known.add(tx.last_insert_rowid(), photo, &stem, &family);
                    added += 1;
                }
                control.progress(index + 1, total);
            }
            tx.commit()?;
            control
                .committed
                .fetch_add(end - position, Ordering::Relaxed);
            position = end;
        }
        Ok((added, moved, ambiguous))
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
        control.phase("BLAKE3-Prüfsummen berechnen", "Dateien");
        control.progress(0, rows.len());
        // First-time hashes are stored per portion instead of one commit per file; what was
        // computed before a cancel is kept (the stored baseline is never overwritten).
        let mut pending: Vec<(i64, String)> = Vec::new();
        let outcome = (|| -> Result<()> {
            for (index, (id, rel, old)) in rows.iter().enumerate() {
                control.checkpoint()?;
                let hash = content_hash(&self.safe_path(rel)?, control)?;
                if let Some(old) = old {
                    if old != &hash {
                        mismatches.push(rel.clone());
                    }
                } else {
                    pending.push((*id, hash));
                    if pending.len() >= crate::config::HASH_PORTION {
                        self.store_hashes(&mut pending)?;
                    }
                }
                control.progress(index + 1, rows.len());
            }
            Ok(())
        })();
        let stored = self.store_hashes(&mut pending);
        outcome?;
        stored?;
        control.phase("Sicherung schreiben", "");
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
    fn store_hashes(&self, pending: &mut Vec<(i64, String)>) -> Result<()> {
        if pending.is_empty() {
            return Ok(());
        }
        let tx = self.db.unchecked_transaction()?;
        for (id, hash) in pending.iter() {
            tx.execute(
                "UPDATE files SET full_hash=? WHERE id=? AND full_hash IS NULL",
                params![hash, id],
            )?;
        }
        tx.commit()?;
        pending.clear();
        Ok(())
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
/// Eine Signaturzuordnung gilt nur, wenn ein früher gespeicherter Schnellhash (falls vorhanden)
/// zum Inhalt der neuen Datei passt.
fn signature_confirmed(found: &Unseen, path: &Path, control: &Control) -> Result<bool> {
    match &found.quick_hash {
        Some(hash) => Ok(*hash == super::import_service::quick_hash(path, control)?),
        None => Ok(true),
    }
}

pub(crate) fn family_directory(directory: &str, extensions: &[String]) -> String {
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

#[derive(Default, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CollectionStatus {
    pub photos: usize,
    pub files: usize,
    pub missing: usize,
    pub pending: usize,
    /// Davon noch nie gelesen (ohne frühere Fehler).
    pub unread: usize,
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
        // Attribute access only: opening for read makes on-access virus scanners inspect every
        // new file first (about 9 ms per file measured, ADR 0064); the file index needs no data.
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_READ_ATTRIBUTES: u32 = 0x80;
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(7)
            .open(path)
            .ok()?;
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

/// Fehlt die Datei oder ihr Ordner (`NotFound`, unter Windows Fehler 2 und 3)? Andere Fehler
/// (Zugriff verweigert, Gerät nicht bereit) zählen nicht als fehlend.
pub fn is_not_found(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
    })
}

/// Liegt im Ordner schon eine Sammlung? Ein einzelner Metadatenaufruf, nichts wird geöffnet.
pub fn exists_at(folder: &Path) -> bool {
    folder.join(".myfotomanager").is_file()
}

/// Grund, warum `folder` keine Sammlung sein darf, oder `None`. Abgelehnt werden die
/// Importquelle selbst und Ordner in ihr (die Datenbank läge sonst zwischen den Originalen)
/// sowie jeder Ordner auf einer Speicherkarte (ein Pfadteil heißt `DCIM`). Ein Ordner, der
/// die Quelle enthält, ist erlaubt: Die Sammlung darf einen Zwischenordner wie
/// `__einzuordnen` als Quelle haben. Pfade werden, wo möglich, kanonisch und unter Windows
/// ohne Groß-/Kleinschreibung verglichen.
pub fn collection_folder_problem(folder: &Path, source: Option<&Path>) -> Option<String> {
    let folder_key = path_key(&canonical(folder));
    if let Some(source) = source.filter(|s| !s.as_os_str().is_empty()) {
        let source_key = path_key(&canonical(source));
        if folder_key.starts_with(&source_key) {
            return Some(
                "Dieser Ordner ist die Importquelle oder liegt in ihr. Eine Sammlung dort würde \
                 ihre Datenbank zwischen die Originale schreiben. Bitte einen eigenen Ordner für \
                 die Sammlung wählen."
                    .into(),
            );
        }
    }
    if on_memory_card(&folder_key) {
        return Some(MEMORY_CARD.into());
    }
    None
}

const MEMORY_CARD: &str = "Dieser Ordner liegt auf einer Speicherkarte (DCIM). Dort entsteht \
                           keine Sammlung, damit die Karte unverändert bleibt. Bitte einen \
                           Ordner auf der Festplatte wählen.";

fn on_memory_card(key: &[String]) -> bool {
    key.iter().any(|part| part.eq_ignore_ascii_case("dcim"))
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Vergleichbare Pfadteile: ohne `\\?\`-Präfix, mit `/` getrennt, unter Windows gefaltet.
fn path_key(path: &Path) -> Vec<String> {
    let text = path.to_string_lossy();
    let text = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    };
    files::normalize(Path::new(&text))
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(|part| {
            if cfg!(windows) {
                files::fold(part)
            } else {
                part.to_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod folder_tests {
    use super::*;

    #[test]
    fn the_import_source_cannot_be_a_collection() {
        let source = Path::new(r"Q:\Bilder\Eingang");
        for folder in [
            r"Q:\Bilder\Eingang",
            r"Q:\Bilder\Eingang\",
            r"Q:\Bilder\Eingang\2024",
        ] {
            assert!(
                collection_folder_problem(Path::new(folder), Some(source)).is_some(),
                "{folder}"
            );
        }
        // Eine Sammlung darf ihren Zwischenordner (z. B. `__einzuordnen`) als Quelle haben.
        for folder in [r"Q:\Bilder", r"Q:\"] {
            assert!(
                collection_folder_problem(Path::new(folder), Some(source)).is_none(),
                "{folder}"
            );
        }
        assert!(
            collection_folder_problem(Path::new(r"Q:\Bilder\Eingang2"), Some(source)).is_none(),
            "ein Nachbarordner mit gleichem Namensanfang ist erlaubt"
        );
        if cfg!(windows) {
            assert!(
                collection_folder_problem(Path::new(r"q:\bilder\eingang"), Some(source)).is_some(),
                "unter Windows ohne Groß-/Kleinschreibung"
            );
        }
    }

    #[test]
    fn a_folder_on_a_memory_card_cannot_be_a_collection() {
        for folder in [r"Q:\DCIM\103NZ6_2", r"Q:\dcim", r"E:\Karte\Dcim\100CANON"] {
            assert_eq!(
                collection_folder_problem(Path::new(folder), None).as_deref(),
                Some(MEMORY_CARD),
                "{folder}"
            );
        }
        assert!(collection_folder_problem(Path::new(r"D:\DCIM-Sicherung"), None).is_none());
    }

    #[test]
    fn an_ordinary_folder_is_accepted() {
        assert!(collection_folder_problem(Path::new(r"D:\Fotos"), None).is_none());
        assert!(
            collection_folder_problem(Path::new(r"D:\Fotos"), Some(Path::new(r"Q:\DCIM\100")))
                .is_none()
        );
        assert!(collection_folder_problem(Path::new(r"D:\Fotos"), Some(Path::new(""))).is_none());
    }

    #[test]
    fn real_folders_are_compared_canonically() -> Result<()> {
        let root = tempfile::tempdir()?;
        let source = root.path().join("Quelle");
        fs::create_dir_all(source.join("Tag"))?;
        let inner = source.join("Tag").join("..").join("Tag");
        assert!(collection_folder_problem(&inner, Some(&source)).is_some());
        assert!(collection_folder_problem(root.path(), Some(&source)).is_none());
        let other = root.path().join("Sammlung");
        fs::create_dir(&other)?;
        assert!(collection_folder_problem(&other, Some(&source)).is_none());
        Ok(())
    }

    #[test]
    fn no_new_collection_is_created_on_a_memory_card() -> Result<()> {
        let root = tempfile::tempdir()?;
        let card = root.path().join("DCIM").join("100TEST");
        fs::create_dir_all(&card)?;
        assert!(Collection::open(&card, true).is_err());
        assert!(fs::read_dir(&card)?.next().is_none(), "nichts geschrieben");
        Ok(())
    }
}
