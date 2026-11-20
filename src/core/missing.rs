//! Fehlende Dateien der Sammlung: Übersicht je Ordner, neu verknüpfen, Einträge entfernen.
//!
//! Fehlend heißt `files.status='missing'`: ein vollständiger Bestandslauf hat die Datei nicht
//! gesehen, oder Metadaten- bzw. Gesichtslauf fanden sie nicht mehr. Hier wird nur die
//! Datenbank geändert, nie eine Datei auf der Platte.
use super::{collection::Collection, collection::family_directory, files, jobs::Control};
use anyhow::{Result, bail};
use rusqlite::params;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Fehlende Dateien eines Ordners. Varianten-Unterordner (`jpg`, `nef` …) zählen zum
/// übergeordneten Ordner, wie bei der Paarung der Varianten.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MissingFolder {
    /// Relativer Ordner („“ = Wurzel).
    pub dir: String,
    pub files: usize,
    /// Fotos, die ohne diese Dateien keine vorhandene Datei mehr hätten.
    pub orphaned_photos: usize,
    /// Bestätigte Gesichter (Nutzerzuordnung) auf diesen Fotos.
    pub confirmed_faces: usize,
    /// Ordner, in dem jede fehlende Datei genau eine Entsprechung (Name, Größe, Änderungszeit)
    /// hat, die schon als eigene Datei im Bestand steht: Neu verknüpfen ist möglich.
    pub relink: Option<String>,
}

/// relpath_fold, stem, ext, dir, inode, quick_hash der neuen Zeile.
type NewFields = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);
/// Zielordner und Paare (fehlende Zeile, vorhandene Entsprechung).
type Pairing = (String, Vec<(Row, Row)>);

#[derive(Clone)]
struct Row {
    id: i64,
    relpath: String,
    dir: String,
    name: String,
    size: i64,
    mtime: i64,
    photo: Option<i64>,
}

fn extension_folders(collection: &Collection) -> Result<Vec<String>> {
    collection.list_option("extension_folders", crate::config::SUPPORTED)
}

fn rows(collection: &Collection, status: &str, control: &Control) -> Result<Vec<Row>> {
    let mut statement = collection.db.prepare(
        "SELECT id,relpath,dir,name,size,mtime_ns,photo_id FROM files WHERE status=? ORDER BY relpath",
    )?;
    let mut cursor = statement.query([status])?;
    let mut rows = Vec::new();
    while let Some(r) = cursor.next()? {
        if rows.len() % 4096 == 0 {
            control.checkpoint()?;
        }
        rows.push(Row {
            id: r.get(0)?,
            relpath: r.get(1)?,
            dir: r.get(2)?,
            name: r.get(3)?,
            size: r.get(4)?,
            mtime: r.get(5)?,
            photo: r.get(6)?,
        });
    }
    Ok(rows)
}

fn signature(row: &Row) -> (String, i64, i64) {
    (files::fold(&row.name), row.size, row.mtime)
}

/// Fehlende Dateien je Ordner, mit Auswirkungen eines Entfernens und möglicher Neuverknüpfung.
pub fn report(collection: &Collection, control: &Control) -> Result<Vec<MissingFolder>> {
    let missing = rows(collection, "missing", control)?;
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let extensions = extension_folders(collection)?;
    let mut groups: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for row in missing {
        groups
            .entry(family_directory(&row.dir, &extensions))
            .or_default()
            .push(row);
    }
    let present = rows(collection, "present", control)?;
    let mut by_signature: HashMap<(String, i64, i64), Vec<&Row>> = HashMap::new();
    for row in &present {
        by_signature.entry(signature(row)).or_default().push(row);
    }
    let mut folders = Vec::with_capacity(groups.len());
    for (dir, rows) in groups {
        control.checkpoint()?;
        let photos: BTreeSet<i64> = rows.iter().filter_map(|r| r.photo).collect();
        let (orphaned_photos, confirmed_faces) = impact(collection, &photos)?;
        let relink = relink_target(&rows, &by_signature, &extensions)
            .ok()
            .map(|(target, _)| target)
            .filter(|target| *target != dir);
        folders.push(MissingFolder {
            dir,
            files: rows.len(),
            orphaned_photos,
            confirmed_faces,
            relink,
        });
    }
    Ok(folders)
}

/// Fotos ohne vorhandene Datei und deren bestätigte Gesichter.
fn impact(collection: &Collection, photos: &BTreeSet<i64>) -> Result<(usize, usize)> {
    let (mut orphaned, mut confirmed) = (0, 0);
    let mut alive = collection.db.prepare_cached(
        "SELECT EXISTS(SELECT 1 FROM files WHERE photo_id=? AND status='present')",
    )?;
    let mut faces = collection.db.prepare_cached(
        "SELECT count(*) FROM faces WHERE photo_id=? AND assignment='user' AND person_id IS NOT NULL",
    )?;
    for photo in photos {
        if !alive.query_row([photo], |r| r.get::<_, bool>(0))? {
            orphaned += 1;
            confirmed += faces.query_row([photo], |r| r.get::<_, usize>(0))?;
        }
    }
    Ok((orphaned, confirmed))
}

/// Der eine Zielordner, in dem jede fehlende Datei der Gruppe genau eine vorhandene
/// Entsprechung hat, samt Paaren (fehlend, vorhanden). Sonst ein Grund.
fn relink_target(
    rows: &[Row],
    by_signature: &HashMap<(String, i64, i64), Vec<&Row>>,
    extensions: &[String],
) -> std::result::Result<Pairing, &'static str> {
    let mut target: Option<String> = None;
    let mut pairs = Vec::with_capacity(rows.len());
    let mut used = BTreeSet::new();
    for row in rows {
        let matches = by_signature
            .get(&signature(row))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let [found] = matches else {
            return Err(if matches.is_empty() {
                "Nicht zu jeder fehlenden Datei gibt es eine gleiche Datei im Bestand."
            } else {
                "Mehrdeutig: zu einer fehlenden Datei passen mehrere Dateien."
            });
        };
        let family = family_directory(&found.dir, extensions);
        if target.get_or_insert_with(|| family.clone()) != &family {
            return Err("Die passenden Dateien liegen in verschiedenen Ordnern.");
        }
        if !used.insert(found.id) {
            return Err("Mehrdeutig: zwei fehlende Dateien passen zur selben Datei.");
        }
        pairs.push((row.clone(), (*found).clone()));
    }
    target.map(|t| (t, pairs)).ok_or("Keine fehlenden Dateien.")
}

/// Verknüpft die fehlenden Dateien von `from` mit ihren Entsprechungen in `to`: Die alten
/// Zeilen (mit Foto-ID, Gesichtern und Entscheidungen) übernehmen die neuen Pfade, die
/// doppelt eingetragenen neuen Zeilen entfallen. Nur zulässig, solange an den Fotos der neuen
/// Zeilen noch keine Gesichter hängen; sonst wird nichts geändert.
pub fn relink(
    collection: &mut Collection,
    from: &str,
    to: &str,
    control: &Control,
) -> Result<String> {
    collection.writable()?;
    if !collection.reachable() {
        bail!("Sammlung ist nicht erreichbar");
    }
    let extensions = extension_folders(collection)?;
    let group: Vec<Row> = rows(collection, "missing", control)?
        .into_iter()
        .filter(|r| family_directory(&r.dir, &extensions) == from)
        .collect();
    let present = rows(collection, "present", control)?;
    let mut by_signature: HashMap<(String, i64, i64), Vec<&Row>> = HashMap::new();
    for row in &present {
        by_signature.entry(signature(row)).or_default().push(row);
    }
    let (target, pairs) = relink_target(&group, &by_signature, &extensions)
        .map_err(|reason| anyhow::anyhow!("Neu verknüpfen nicht möglich: {reason}"))?;
    if target != to {
        bail!("Neu verknüpfen nicht möglich: die passenden Dateien liegen in „{target}“");
    }
    for (old, new) in &pairs {
        if collection.root.join(&old.relpath).exists() {
            bail!(
                "„{}“ ist wieder vorhanden; bitte Bestand aktualisieren",
                old.relpath
            );
        }
        if !collection.root.join(&new.relpath).is_file() {
            bail!(
                "„{}“ fehlt auf der Platte; bitte Bestand aktualisieren",
                new.relpath
            );
        }
    }
    let new_photos: BTreeSet<i64> = pairs.iter().filter_map(|(_, n)| n.photo).collect();
    let tx = collection.db.transaction()?;
    for photo in &new_photos {
        let faces: i64 = tx.query_row(
            "SELECT count(*) FROM faces WHERE photo_id=?",
            [photo],
            |r| r.get(0),
        )?;
        if faces > 0 {
            bail!(
                "Neu verknüpfen nicht möglich: an den Fotos in „{to}“ hängen schon Gesichter. \
                 Ohne Verknüpfung „Einträge entfernen“ wählen; bestätigte Zuordnungen der alten \
                 Fotos bleiben dabei gespeichert."
            );
        }
    }
    tx.execute("INSERT INTO scans(kind,scope,started_at,status) VALUES('relink',?,datetime('now'),'running')", [from])?;
    let scan = tx.last_insert_rowid();
    for (old, new) in &pairs {
        let fields: NewFields = tx.query_row(
            "SELECT relpath_fold,stem,ext,dir,inode,quick_hash FROM files WHERE id=?",
            [new.id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )?;
        tx.execute("DELETE FROM meta WHERE file_id=?", [new.id])?;
        tx.execute("DELETE FROM files WHERE id=?", [new.id])?;
        tx.execute(
            "UPDATE files SET relpath=?,relpath_fold=?,dir=?,name=?,stem=?,ext=?,size=?,mtime_ns=?,inode=?,quick_hash=coalesce(quick_hash,?),last_seen_scan=?,status='present' WHERE id=?",
            params![new.relpath, fields.0, fields.3, new.name, fields.1, fields.2, new.size, new.mtime, fields.4, fields.5, scan, old.id],
        )?;
        tx.execute(
            "UPDATE faces SET source_relpath=? WHERE source_file_id=? AND source_relpath=?",
            params![new.relpath, old.id, old.relpath],
        )?;
        tx.execute("INSERT INTO changes(scan_id,kind,file_id,old_relpath,new_relpath,at) VALUES(?,'moved',?,?,?,datetime('now'))", params![scan, old.id, old.relpath, new.relpath])?;
    }
    // Leere neue Fotos (ohne Dateien, ohne Gesichter) entfallen; ihre Kameraidentität geht an
    // das alte Foto über, falls es noch keine hat.
    for photo in &new_photos {
        let empty: bool = tx.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM files WHERE photo_id=?)",
            [photo],
            |r| r.get(0),
        )?;
        if !empty {
            continue;
        }
        let key: Option<String> =
            tx.query_row("SELECT identity_key FROM photos WHERE id=?", [photo], |r| {
                r.get(0)
            })?;
        tx.execute("DELETE FROM face_jobs WHERE photo_id=?", [photo])?;
        tx.execute("DELETE FROM photos WHERE id=?", [photo])?;
        if let Some(key) = key
            && let Some(old) = pairs
                .iter()
                .find(|(_, n)| n.photo == Some(*photo))
                .and_then(|(o, _)| o.photo)
        {
            tx.execute(
                "UPDATE photos SET identity_key=? WHERE id=? AND identity_key IS NULL",
                params![key, old],
            )?;
        }
    }
    tx.execute("UPDATE photos SET status=CASE WHEN EXISTS(SELECT 1 FROM files WHERE photo_id=photos.id AND status='present') THEN 'present' ELSE 'missing' END WHERE status IS NOT CASE WHEN EXISTS(SELECT 1 FROM files WHERE photo_id=photos.id AND status='present') THEN 'present' ELSE 'missing' END",[])?;
    tx.execute(
        "UPDATE scans SET status='done',finished_at=datetime('now') WHERE id=?",
        [scan],
    )?;
    tx.commit()?;
    collection.backup()?;
    log::info!(target: "sammlung", "neu_verknuepft von={from:?} nach={to:?} dateien={}", pairs.len());
    Ok(format!(
        "{} Dateien von „{from}“ mit „{to}“ verknüpft; Foto-IDs, Gesichter und Zuordnungen bleiben",
        pairs.len()
    ))
}

/// Entfernt die Einträge fehlender Dateien in den gewählten Ordnern aus der Datenbank. Jede
/// Datei wird vorher auf der Platte gesucht: was wieder da ist, bleibt. Fotos, Gesichter und
/// Personenzuordnungen bleiben erhalten (wie beim Entfernen nach Scanabstand).
pub fn remove(collection: &mut Collection, dirs: &[String], control: &Control) -> Result<String> {
    collection.writable()?;
    if !collection.reachable() {
        bail!("Sammlung ist nicht erreichbar; nichts entfernt");
    }
    let extensions = extension_folders(collection)?;
    let chosen: BTreeSet<&str> = dirs.iter().map(String::as_str).collect();
    let mut gone = Vec::new();
    let mut back = 0;
    for row in rows(collection, "missing", control)? {
        if !chosen.contains(family_directory(&row.dir, &extensions).as_str()) {
            continue;
        }
        if collection
            .root
            .join(&row.relpath)
            .symlink_metadata()
            .is_ok()
        {
            back += 1;
        } else {
            gone.push(row);
        }
    }
    let tx = collection.db.transaction()?;
    for row in &gone {
        tx.execute("DELETE FROM meta WHERE file_id=?", [row.id])?;
        tx.execute(
            "DELETE FROM files WHERE id=? AND status='missing'",
            [row.id],
        )?;
        tx.execute("INSERT INTO changes(scan_id,kind,file_id,old_relpath,new_relpath,at) VALUES(NULL,'removed',?,?,NULL,datetime('now'))", params![row.id, row.relpath])?;
    }
    tx.commit()?;
    collection.backup()?;
    log::info!(target: "sammlung", "fehlende_entfernt anzahl={} wieder_da={back}", gone.len());
    Ok(format!(
        "{} fehlende Dateieinträge entfernt; Foto-IDs, Gesichter und Personen bleiben erhalten{}",
        gone.len(),
        if back > 0 {
            format!(" · {back} Dateien sind wieder da und blieben (Bestand aktualisieren)")
        } else {
            String::new()
        }
    ))
}
