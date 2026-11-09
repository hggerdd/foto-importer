//! Zentraler Personenbestand im Benutzerprofil (ADR 0062).
//!
//! Eine eigene SQLite-Datei hält Personen mit stabiler globaler ID. Sammlungen
//! verweisen über das Personenattribut `global_id` darauf; ihr Schema bleibt unverändert.
//! Abgleich ist nie destruktiv: leere Felder werden ergänzt, widersprüchliche Felder
//! werden dem Nutzer als [`Conflict`] vorgelegt.
use super::{
    collection::Collection,
    faces::{self, NewPerson, Undo},
    files::fold,
};
mod rels;
pub use rels::{
    CentralDecisions, CentralEdge, RelChoice, RelConflict, RelationReport, add_relation_synced,
    remove_relation_synced, resolve_relation, sync_relations,
};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, TransactionBehavior, params};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

/// Schema 2 ergänzt Beziehungen (ADR 0065); Schema 1 bleibt als Teilmenge erhalten.
pub const SCHEMA_VERSION: i64 = 2;
/// Zentral geführte Felder, identisch mit den Personenattributen der Sammlung.
pub const FIELDS: [&str; 4] = ["name", "gender", "birth_year", "note"];
const GLOBAL_ID: &str = "global_id";
const DISTINCT: &str = "global_distinct";
/// Obergrenzen für zentrale Referenzausschnitte (klein halten, Datenschutz).
pub const MAX_REFERENCE_BYTES: usize = 128 * 1024;
pub const MAX_REFERENCES_PER_PERSON: i64 = 64;

/// Ort und Optionen des zentralen Bestands; stammt aus den Einstellungen.
#[derive(Clone, Debug)]
pub struct CentralConfig {
    pub enabled: bool,
    pub path: PathBuf,
    /// Opt-in: Gesichtsausschnitte zentral speichern (Standard aus).
    pub images: bool,
    /// Beziehungen zentral mitführen (ADR 0065); wirkt nur bei `enabled`.
    pub relations: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CentralPerson {
    pub gid: String,
    pub name: String,
    pub gender: String,
    pub birth_year: String,
    pub note: String,
    pub updated_at: String,
}
impl CentralPerson {
    fn field(&self, index: usize) -> &str {
        [&self.name, &self.gender, &self.birth_year, &self.note][index]
    }
}

enum Op {
    Create(CentralPerson),
    /// Nur setzen, wenn das zentrale Feld noch leer ist (sicher gegen parallele Schreiber).
    Fill(String, &'static str, String),
    /// Ausdrückliche Nutzerwahl.
    Set(String, &'static str, String),
}

pub struct Central {
    db: Connection,
    images: bool,
    relations: bool,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn norm(text: &str) -> String {
    fold(&text.split_whitespace().collect::<Vec<_>>().join(" "))
}
fn is_network(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.starts_with("\\\\") || text.starts_with("//")
}
fn field_name(name: &str) -> Result<&'static str> {
    FIELDS
        .iter()
        .find(|f| **f == name)
        .copied()
        .context("Unbekanntes Personenfeld")
}

impl Central {
    /// Öffnet oder erstellt die Zentraldatei. Kurze Sperrwartezeit, WAL (auf
    /// Netzfreigaben stattdessen ein Rollback-Journal, weil WAL dort unzuverlässig ist).
    pub fn open(path: &Path, images: bool) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).context("Ordner des zentralen Bestands fehlt")?;
        }
        let db = Connection::open(path).context("Zentraler Personenbestand nicht erreichbar")?;
        db.busy_timeout(Duration::from_secs(5))?;
        let mode = if is_network(path) { "DELETE" } else { "WAL" };
        db.query_row(&format!("PRAGMA journal_mode={mode}"), [], |_| Ok(()))
            .context("Zentraler Personenbestand ist gesperrt oder nicht beschreibbar")?;
        db.execute_batch("PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            bail!(
                "Der zentrale Bestand hat Schema {version}; diese Version kennt höchstens {SCHEMA_VERSION}"
            );
        }
        let mut central = Self {
            db,
            images,
            relations: false,
        };
        if version < SCHEMA_VERSION {
            central.migrate(path, version)?;
        }
        Ok(central)
    }

    pub fn open_config(config: &CentralConfig) -> Result<Self> {
        Ok(Self::open(&config.path, config.images)?.with_relations(config.relations))
    }

    /// Schaltet den Beziehungsabgleich für diese Verbindung ein (ADR 0065).
    pub fn with_relations(mut self, on: bool) -> Self {
        self.relations = on;
        self
    }
    pub fn relations_enabled(&self) -> bool {
        self.relations
    }

    /// Legt Schema 1 an bzw. erweitert es additiv auf Schema 2. Eine bestehende Version-1-Datei
    /// wird vorher best-effort mit `VACUUM INTO` gesichert; scheitert die Sperre oder fehlt das
    /// Schreibrecht, bleibt die Datei unverändert (die Transaktion wird nicht committet).
    fn migrate(&mut self, path: &Path, version: i64) -> Result<()> {
        if version == 1 {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let backup = path.with_file_name(format!("{name}.v1.bak"));
            if !backup.exists() {
                let target = backup.to_string_lossy().replace('\'', "''");
                let _ = self.db.execute_batch(&format!("VACUUM INTO '{target}'"));
            }
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if current == 0 {
            tx.execute_batch(
                "CREATE TABLE people(gid TEXT PRIMARY KEY, name TEXT NOT NULL,
                    gender TEXT NOT NULL DEFAULT '', birth_year TEXT NOT NULL DEFAULT '',
                    note TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL, merged_into TEXT);
                 CREATE TABLE refs(id INTEGER PRIMARY KEY, gid TEXT NOT NULL,
                    model_key TEXT NOT NULL, jpeg BLOB NOT NULL, created_at TEXT NOT NULL);
                 CREATE INDEX refs_gid ON refs(gid);
                 CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO meta VALUES('created_by','foto-importer');
                 PRAGMA user_version=1;",
            )?;
        }
        if current < 2 {
            rels::create_tables(&tx)?;
            tx.execute_batch("PRAGMA user_version=2;")?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Aktive (nicht zusammengeführte) Personen, nach Name sortiert.
    pub fn people(&self) -> Result<Vec<CentralPerson>> {
        Ok(self
            .db
            .prepare("SELECT gid,name,gender,birth_year,note,updated_at FROM people WHERE merged_into IS NULL ORDER BY name COLLATE NOCASE,gid")?
            .query_map([], |r| {
                Ok(CentralPerson {
                    gid: r.get(0)?,
                    name: r.get(1)?,
                    gender: r.get(2)?,
                    birth_year: r.get(3)?,
                    note: r.get(4)?,
                    updated_at: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Alias → endgültige ID für zusammengeführte Personen.
    pub fn aliases(&self) -> Result<HashMap<String, String>> {
        let direct: HashMap<String, String> = self
            .db
            .prepare("SELECT gid,merged_into FROM people WHERE merged_into IS NOT NULL")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut result = HashMap::new();
        for from in direct.keys() {
            let mut to = from;
            for _ in 0..direct.len() + 1 {
                match direct.get(to) {
                    Some(next) => to = next,
                    None => break,
                }
            }
            result.insert(from.clone(), to.clone());
        }
        Ok(result)
    }

    /// Fügt eine Person neu hinzu und liefert die globale ID.
    pub fn add(&mut self, new: &NewPerson) -> Result<String> {
        let name = faces::validate_new_person(new)?;
        let gid = uuid::Uuid::new_v4().to_string();
        let person = CentralPerson {
            gid: gid.clone(),
            name,
            gender: new.gender.clone(),
            birth_year: new.birth_year.map(|y| y.to_string()).unwrap_or_default(),
            note: new.note.trim().to_string(),
            updated_at: String::new(),
        };
        self.apply(vec![Op::Create(person)])?;
        Ok(gid)
    }

    /// Fasst zwei zentrale Personen zusammen; die alte ID bleibt als Alias erhalten.
    pub fn merge(&mut self, from: &str, to: &str) -> Result<()> {
        if from == to {
            bail!("Bitte zwei verschiedene Personen auswählen");
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active = |gid: &str| -> rusqlite::Result<bool> {
            tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM people WHERE gid=? AND merged_into IS NULL)",
                [gid],
                |r| r.get(0),
            )
        };
        if !active(from)? || !active(to)? {
            bail!("Zentrale Person ist nicht verfügbar");
        }
        // Lücken füllen, ohne Vorhandenes zu ersetzen.
        for field in &FIELDS[1..] {
            tx.execute(
                &format!(
                    "UPDATE people SET {field}=(SELECT {field} FROM people WHERE gid=?1) WHERE gid=?2 AND {field}='' "
                ),
                params![from, to],
            )?;
        }
        tx.execute("UPDATE refs SET gid=? WHERE gid=?", params![to, from])?;
        tx.execute(
            "UPDATE people SET merged_into=?,updated_at=? WHERE gid=?",
            params![to, now(), from],
        )?;
        rels::rehang(&tx, from, to)?;
        tx.commit()?;
        Ok(())
    }

    fn apply(&mut self, ops: Vec<Op>) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stamp = now();
        for op in ops {
            match op {
                Op::Create(p) => {
                    tx.execute(
                        "INSERT OR IGNORE INTO people(gid,name,gender,birth_year,note,created_at,updated_at) VALUES(?,?,?,?,?,?,?)",
                        params![p.gid, p.name, p.gender, p.birth_year, p.note, stamp, stamp],
                    )?;
                }
                Op::Fill(gid, field, value) => {
                    tx.execute(
                        &format!(
                            "UPDATE people SET {field}=?,updated_at=? WHERE gid=? AND {field}=''"
                        ),
                        params![value, stamp, gid],
                    )?;
                }
                Op::Set(gid, field, value) => {
                    tx.execute(
                        &format!("UPDATE people SET {field}=?,updated_at=? WHERE gid=?"),
                        params![value, stamp, gid],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ---- Referenzbilder (Phase 2, Opt-in) ----

    /// Speichert einen kleinen Gesichtsausschnitt (JPEG) mit Modellschlüssel.
    /// Ohne Opt-in wird nichts gespeichert. Embeddings werden bewusst nicht abgelegt.
    pub fn add_reference(&mut self, gid: &str, model_key: &str, jpeg: &[u8]) -> Result<()> {
        if !self.images {
            bail!("Zentrale Referenzbilder sind nicht aktiviert");
        }
        if jpeg.is_empty() || jpeg.len() > MAX_REFERENCE_BYTES {
            bail!("Referenzausschnitt hat eine unzulässige Größe");
        }
        if model_key.trim().is_empty() {
            bail!("Modellschlüssel fehlt");
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let known: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM people WHERE gid=? AND merged_into IS NULL)",
            [gid],
            |r| r.get(0),
        )?;
        if !known {
            bail!("Zentrale Person ist nicht verfügbar");
        }
        let count: i64 =
            tx.query_row("SELECT count(*) FROM refs WHERE gid=?", [gid], |r| r.get(0))?;
        if count >= MAX_REFERENCES_PER_PERSON {
            bail!(
                "Je Person sind höchstens {MAX_REFERENCES_PER_PERSON} zentrale Ausschnitte erlaubt"
            );
        }
        tx.execute(
            "INSERT INTO refs(gid,model_key,jpeg,created_at) VALUES(?,?,?,?)",
            params![gid, model_key, jpeg, now()],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Ausschnitte einer Person (nur wenn der Opt-in aktiv ist).
    pub fn references(&self, gid: &str) -> Result<Vec<(String, Vec<u8>)>> {
        if !self.images {
            return Ok(Vec::new());
        }
        Ok(self
            .db
            .prepare("SELECT model_key,jpeg FROM refs WHERE gid=? ORDER BY id")?
            .query_map([gid], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn reference_count(&self) -> Result<usize> {
        Ok(self
            .db
            .query_row("SELECT count(*) FROM refs", [], |r| r.get::<_, i64>(0))?
            as usize)
    }
    /// Löscht alle zentralen Referenzausschnitte endgültig (secure_delete, VACUUM).
    pub fn delete_references(&mut self) -> Result<usize> {
        let count = self.reference_count()?;
        self.db.execute_batch(
            "PRAGMA secure_delete=ON; DELETE FROM refs; PRAGMA wal_checkpoint(TRUNCATE); VACUUM;",
        )?;
        Ok(count)
    }
}

/// Löscht zentrale Referenzbilder der Konfiguration; fehlende Datei zählt als leer.
pub fn wipe_references(config: &CentralConfig) -> Result<usize> {
    if !config.path.exists() {
        return Ok(0);
    }
    Central::open(&config.path, config.images)?.delete_references()
}

// ---- Sammlungsseite ----

#[derive(Clone, Debug, Default)]
struct Local {
    id: i64,
    gid: Option<String>,
    distinct: BTreeSet<String>,
    fields: [String; 4],
}

fn load_local(db: &Connection) -> Result<Vec<Local>> {
    let mut map: BTreeMap<i64, Local> = BTreeMap::new();
    for id in db
        .prepare("SELECT id FROM persons WHERE merged_into IS NULL ORDER BY id")?
        .query_map([], |r| r.get::<_, i64>(0))?
    {
        let id = id?;
        map.insert(
            id,
            Local {
                id,
                ..Default::default()
            },
        );
    }
    let mut statement = db.prepare(
        "SELECT a.person_id,a.key,a.value FROM person_attrs a JOIN persons p ON p.id=a.person_id
         WHERE p.merged_into IS NULL AND a.key IN ('name','gender','birth_year','note','global_id','global_distinct')",
    )?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?.unwrap_or_default(),
        ))
    })?;
    for row in rows {
        let (id, key, value) = row?;
        let Some(local) = map.get_mut(&id) else {
            continue;
        };
        match key.as_str() {
            GLOBAL_ID if !value.is_empty() => local.gid = Some(value),
            DISTINCT => local.distinct = value.split(',').map(str::to_string).collect(),
            other => {
                if let Some(index) = FIELDS.iter().position(|f| *f == other) {
                    local.fields[index] = value;
                }
            }
        }
    }
    Ok(map.into_values().collect())
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldDiff {
    pub field: &'static str,
    pub local: String,
    pub central: String,
}
/// Rückfrage: lokale und zentrale Person widersprechen sich.
#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    pub local_id: i64,
    pub gid: String,
    /// Schon verknüpft (nur Felder strittig) oder nur Namenstreffer (auch die Identität ist offen).
    pub linked: bool,
    pub name: String,
    pub diffs: Vec<FieldDiff>,
}
#[derive(Clone, Debug, Default)]
pub struct SyncReport {
    pub linked: usize,
    pub created_central: usize,
    pub filled_local: usize,
    pub filled_central: usize,
    pub relinked: usize,
    pub ambiguous: usize,
    pub conflicts: Vec<Conflict>,
    /// Zentrale Personen, die in dieser Sammlung noch fehlen.
    pub missing: Vec<CentralPerson>,
    /// Ergebnis des Beziehungsabgleichs (leer, wenn zentrale Beziehungen aus sind).
    pub relations: RelationReport,
}
impl SyncReport {
    /// Offene Punkte, die der Nutzer entscheiden muss (für Zähler und Hinweis).
    pub fn pending(&self) -> usize {
        self.missing.len() + self.conflicts.len() + self.ambiguous + self.relations.conflicts.len()
    }
    pub fn summary(&self) -> String {
        let mut text = format!(
            "Abgleich: {} verknüpft, {} zentral angelegt, {} lokal ergänzt, {} zentral ergänzt, {} Rückfragen, {} fehlen lokal",
            self.linked,
            self.created_central,
            self.filled_local,
            self.filled_central,
            self.conflicts.len(),
            self.missing.len()
        );
        if self.relations.active {
            text.push_str(". ");
            text.push_str(&self.relations.summary());
        }
        text
    }
}
/// Ergebnis der Abgleiche rund ums Anlegen: Hinweis statt Blockade.
#[derive(Clone, Debug, Default)]
pub struct SyncNote {
    pub warning: Option<String>,
    pub report: Option<SyncReport>,
}

fn person_from(local: &Local, gid: String) -> CentralPerson {
    CentralPerson {
        gid,
        name: local.fields[0].clone(),
        gender: local.fields[1].clone(),
        birth_year: local.fields[2].clone(),
        note: local.fields[3].clone(),
        updated_at: String::new(),
    }
}

fn set_local(db: &Connection, id: i64, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO person_attrs VALUES(?,?,?)",
        params![id, key, value],
    )?;
    Ok(())
}

type Fill = Vec<(&'static str, String)>;
/// Vergleicht Felder: (lokal ergänzen, zentral ergänzen, Widersprüche).
fn compare(local: &Local, central: &CentralPerson) -> (Fill, Fill, Vec<FieldDiff>) {
    let (mut to_local, mut to_central, mut diffs) = (vec![], vec![], vec![]);
    for (index, field) in FIELDS.iter().enumerate() {
        let (l, c) = (local.fields[index].trim(), central.field(index).trim());
        if norm(l) == norm(c) {
            continue;
        }
        if l.is_empty() {
            to_local.push((*field, c.to_string()));
        } else if c.is_empty() {
            to_central.push((*field, l.to_string()));
        } else {
            diffs.push(FieldDiff {
                field,
                local: l.to_string(),
                central: c.to_string(),
            });
        }
    }
    (to_local, to_central, diffs)
}

/// Gleicht die Sammlung mit dem zentralen Bestand ab. Nichts wird überschrieben:
/// Leeres wird ergänzt, Widersprüche kommen als Rückfrage in den Bericht.
/// Zentrale Schreibzugriffe laufen in einer kurzen Transaktion; erst danach die lokalen.
pub fn sync(collection: &mut Collection, central: &mut Central) -> Result<SyncReport> {
    collection.writable()?;
    let centrals = central.people()?;
    let aliases = central.aliases()?;
    let by_gid: HashMap<&str, &CentralPerson> =
        centrals.iter().map(|p| (p.gid.as_str(), p)).collect();
    let locals = load_local(&collection.db)?;
    let mut report = SyncReport::default();
    let mut ops: Vec<Op> = Vec::new();
    let mut writes: Vec<(i64, &'static str, String)> = Vec::new();
    let mut linked: HashSet<String> = HashSet::new();

    for local in locals.iter().filter(|l| l.gid.is_some()) {
        let stored = local.gid.clone().unwrap_or_default();
        let gid = aliases.get(&stored).cloned().unwrap_or(stored.clone());
        if gid != stored {
            writes.push((local.id, GLOBAL_ID, gid.clone()));
            report.relinked += 1;
        }
        linked.insert(gid.clone());
        match by_gid.get(gid.as_str()) {
            Some(c) => {
                let (to_local, to_central, diffs) = compare(local, c);
                report.filled_local += to_local.len();
                report.filled_central += to_central.len();
                writes.extend(to_local.into_iter().map(|(f, v)| (local.id, f, v)));
                ops.extend(
                    to_central
                        .into_iter()
                        .map(|(f, v)| Op::Fill(gid.clone(), f, v)),
                );
                if !diffs.is_empty() {
                    report.conflicts.push(Conflict {
                        local_id: local.id,
                        gid,
                        linked: true,
                        name: local.fields[0].clone(),
                        diffs,
                    });
                }
            }
            None if !local.fields[0].trim().is_empty() => {
                // Zentraldatei ohne diese ID (z. B. neu angelegt): Identität erhalten.
                ops.push(Op::Create(person_from(local, gid)));
                report.created_central += 1;
            }
            None => {}
        }
    }
    let mut by_name: HashMap<String, Vec<&CentralPerson>> = HashMap::new();
    for c in &centrals {
        by_name.entry(norm(&c.name)).or_default().push(c);
    }
    let mut pending_names: HashSet<String> = HashSet::new();
    for local in locals.iter().filter(|l| l.gid.is_none()) {
        let name = norm(&local.fields[0]);
        if name.is_empty() {
            continue;
        }
        let candidates: Vec<&CentralPerson> = by_name
            .get(&name)
            .into_iter()
            .flatten()
            .copied()
            .filter(|c| !linked.contains(&c.gid) && !local.distinct.contains(&c.gid))
            .collect();
        match candidates.as_slice() {
            [] => {
                let gid = uuid::Uuid::new_v4().to_string();
                ops.push(Op::Create(person_from(local, gid.clone())));
                writes.push((local.id, GLOBAL_ID, gid.clone()));
                linked.insert(gid.clone());
                report.created_central += 1;
            }
            [c] => {
                let (to_local, to_central, diffs) = compare(local, c);
                if diffs.is_empty() {
                    writes.push((local.id, GLOBAL_ID, c.gid.clone()));
                    linked.insert(c.gid.clone());
                    report.linked += 1;
                    report.filled_local += to_local.len();
                    report.filled_central += to_central.len();
                    writes.extend(to_local.into_iter().map(|(f, v)| (local.id, f, v)));
                    ops.extend(
                        to_central
                            .into_iter()
                            .map(|(f, v)| Op::Fill(c.gid.clone(), f, v)),
                    );
                } else {
                    pending_names.insert(name);
                    report.conflicts.push(Conflict {
                        local_id: local.id,
                        gid: c.gid.clone(),
                        linked: false,
                        name: local.fields[0].clone(),
                        diffs,
                    });
                }
            }
            _ => {
                pending_names.insert(name);
                report.ambiguous += 1;
            }
        }
    }
    // Erst zentral (kurze Sperre), dann lokal; bei lokalem Fehler heilt der nächste Abgleich.
    central.apply(ops)?;
    if !writes.is_empty() {
        let tx = collection.db.transaction()?;
        for (id, key, value) in &writes {
            set_local(&tx, *id, key, value)?;
        }
        tx.commit()?;
    }
    report.missing = centrals
        .into_iter()
        .filter(|c| !linked.contains(&c.gid) && !pending_names.contains(&norm(&c.name)))
        .collect();
    if central.relations {
        // Ein Fehler hier darf den Personenabgleich nicht entwerten.
        report.relations = match sync_relations(collection, central) {
            Ok(relations) => relations,
            Err(error) => RelationReport {
                active: true,
                error: Some(format!("{error:#}")),
                ..Default::default()
            },
        };
    }
    Ok(report)
}

/// Zentrale Personen, die in der Sammlung noch fehlen (auch lesend nutzbar).
pub fn missing(collection: &Collection, central: &Central) -> Result<Vec<CentralPerson>> {
    let aliases = central.aliases()?;
    let locals = load_local(&collection.db)?;
    let linked: HashSet<String> = locals
        .iter()
        .filter_map(|l| l.gid.as_ref())
        .map(|g| aliases.get(g).unwrap_or(g).clone())
        .collect();
    let unlinked: HashSet<String> = locals
        .iter()
        .filter(|l| l.gid.is_none())
        .map(|l| norm(&l.fields[0]))
        .collect();
    Ok(central
        .people()?
        .into_iter()
        .filter(|c| !linked.contains(&c.gid) && !unlinked.contains(&norm(&c.name)))
        .collect())
}

fn insert_local(tx: &Connection, p: &CentralPerson) -> Result<i64> {
    tx.execute("INSERT INTO persons DEFAULT VALUES", [])?;
    let id = tx.last_insert_rowid();
    for (index, field) in FIELDS.iter().enumerate() {
        if !p.field(index).is_empty() {
            set_local(tx, id, field, p.field(index))?;
        }
    }
    set_local(tx, id, GLOBAL_ID, &p.gid)?;
    Ok(id)
}

/// Übernimmt zentrale Personen als neue lokale Personen (schon verknüpfte werden übersprungen).
pub fn adopt(collection: &mut Collection, central: &Central, gids: &[String]) -> Result<Vec<i64>> {
    collection.writable()?;
    let aliases = central.aliases()?;
    let wanted: BTreeSet<&String> = gids.iter().collect();
    let locals = load_local(&collection.db)?;
    let linked: HashSet<String> = locals
        .iter()
        .filter_map(|l| l.gid.as_ref())
        .map(|g| aliases.get(g).unwrap_or(g).clone())
        .collect();
    let tx = collection.db.transaction()?;
    let mut ids = Vec::new();
    for person in central.people()? {
        if wanted.contains(&person.gid) && !linked.contains(&person.gid) {
            ids.push(insert_local(&tx, &person)?);
        }
    }
    tx.commit()?;
    if central.relations && !ids.is_empty() {
        // Beziehungen der übernommenen Personen zu vorhandenen Personen direkt nachziehen;
        // Widersprüche erscheinen beim nächsten Abgleich als Rückfrage.
        rels::pull_after_adopt(collection, central)?;
    }
    Ok(ids)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Local,
    Central,
}

/// Beantwortet eine Rückfrage. `same=false` heißt „andere Person“: sie wird nie wieder mit
/// dieser zentralen Person verknüpft und bekommt eine eigene zentrale ID. Bei `same=true`
/// entscheidet `picks` je strittigem Feld; ohne Wahl bleibt das Feld unverändert strittig.
pub fn resolve(
    collection: &mut Collection,
    central: &mut Central,
    local_id: i64,
    gid: &str,
    same: bool,
    picks: &BTreeMap<String, Side>,
) -> Result<SyncReport> {
    collection.writable()?;
    let locals = load_local(&collection.db)?;
    let local = locals
        .iter()
        .find(|l| l.id == local_id)
        .context("Person ist nicht mehr verfügbar")?;
    if !same {
        let mut distinct = local.distinct.clone();
        distinct.insert(gid.to_string());
        let joined = distinct.into_iter().collect::<Vec<_>>().join(",");
        set_local(&collection.db, local_id, DISTINCT, &joined)?;
        return sync(collection, central);
    }
    let remote = central
        .people()?
        .into_iter()
        .find(|p| p.gid == gid)
        .context("Zentrale Person ist nicht mehr verfügbar")?;
    let mut ops = Vec::new();
    let mut writes = Vec::new();
    for (index, field) in FIELDS.iter().enumerate() {
        match picks.get(*field) {
            Some(Side::Local) => ops.push(Op::Set(
                gid.to_string(),
                field_name(field)?,
                local.fields[index].clone(),
            )),
            Some(Side::Central) => writes.push((*field, remote.field(index).to_string())),
            None => {}
        }
    }
    central.apply(ops)?;
    let tx = collection.db.transaction()?;
    set_local(&tx, local_id, GLOBAL_ID, gid)?;
    for (field, value) in writes {
        set_local(&tx, local_id, field, &value)?;
    }
    tx.commit()?;
    sync(collection, central)
}

fn warn(error: &anyhow::Error) -> String {
    format!(
        "Zentraler Personenbestand nicht erreichbar ({error:#}). Lokal gespeichert; der Abgleich folgt später."
    )
}

fn run_sync(collection: &mut Collection, config: &CentralConfig) -> SyncNote {
    if !config.enabled {
        return SyncNote::default();
    }
    match Central::open_config(config).and_then(|mut c| sync(collection, &mut c)) {
        Ok(report) => SyncNote {
            warning: None,
            report: Some(report),
        },
        Err(error) => SyncNote {
            warning: Some(warn(&error)),
            report: None,
        },
    }
}

/// Wie [`faces::name_group`], gleicht aber vorher ab und legt neue Personen danach zentral an.
/// Fehler des zentralen Bestands blockieren das Benennen nie.
pub fn name_group_synced(
    collection: &mut Collection,
    config: &CentralConfig,
    ids: &[i64],
    selected: &BTreeSet<i64>,
    person: Option<i64>,
    name: &str,
    ignore: bool,
) -> Result<(Undo, SyncNote)> {
    collection.writable()?;
    let creating = person.is_none() && !ignore && !name.trim().is_empty();
    let mut note = SyncNote::default();
    if creating && config.enabled {
        note = run_sync(collection, config);
        // Namenstreffer im zentralen Bestand übernehmen, statt doppelt anzulegen.
        if let Some(report) = &note.report {
            let wanted = norm(name);
            let local = load_local(&collection.db)?;
            let known = local.iter().any(|l| norm(&l.fields[0]) == wanted);
            let hits: Vec<String> = report
                .missing
                .iter()
                .filter(|c| norm(&c.name) == wanted)
                .map(|c| c.gid.clone())
                .collect();
            if !known && let [gid] = hits.as_slice() {
                let adopted = Central::open_config(config)
                    .and_then(|c| adopt(collection, &c, std::slice::from_ref(gid)));
                if let Err(error) = adopted {
                    note.warning = Some(warn(&error));
                }
            }
        }
    }
    let undo = faces::name_group(collection, ids, selected, person, name, ignore)?;
    if undo.created.is_some() && config.enabled {
        let after = run_sync(collection, config);
        note.warning = after.warning.or(note.warning);
        if after.report.is_some() {
            note.report = after.report;
        }
    }
    Ok((undo, note))
}

/// Wie [`faces::add_person`] mit Abgleich vor und zentraler Anlage nach dem lokalen Anlegen.
pub fn add_person_synced(
    collection: &mut Collection,
    config: &CentralConfig,
    new: &NewPerson,
) -> Result<(i64, SyncNote)> {
    collection.writable()?;
    faces::validate_new_person(new)?;
    let mut note = run_sync(collection, config);
    let id = faces::add_person(collection, new)?;
    if config.enabled {
        let after = run_sync(collection, config);
        note.warning = after.warning.or(note.warning);
        if after.report.is_some() {
            note.report = after.report;
        }
    }
    Ok((id, note))
}

/// Wie [`super::maintenance::merge_people`]; zentral wird die Zusammenführung als Alias vermerkt.
pub fn merge_people_synced(
    collection: &mut Collection,
    config: &CentralConfig,
    source: i64,
    target: i64,
) -> Result<SyncNote> {
    let locals = load_local(&collection.db)?;
    let gid_of = |id: i64| {
        locals
            .iter()
            .find(|l| l.id == id)
            .and_then(|l| l.gid.clone())
    };
    let (from, to) = (gid_of(source), gid_of(target));
    super::maintenance::merge_people(collection, source, target)?;
    let mut note = SyncNote::default();
    if config.enabled
        && let (Some(from), Some(to)) = (from, to)
        && from != to
        && let Err(error) = Central::open_config(config).and_then(|mut c| c.merge(&from, &to))
    {
        note.warning = Some(warn(&error));
    }
    Ok(note)
}

/// Anzahl der Personen mit Verknüpfung (für die Statusanzeige).
pub fn linked_count(collection: &Collection) -> Result<usize> {
    Ok(collection.db.query_row(
        "SELECT count(*) FROM person_attrs a JOIN persons p ON p.id=a.person_id WHERE a.key='global_id' AND p.merged_into IS NULL",
        [],
        |r| r.get::<_, i64>(0),
    )? as usize)
}
