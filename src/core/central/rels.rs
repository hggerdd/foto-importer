//! Zentral geführte Beziehungen (ADR 0065, Phase 2 aus ADR 0062).
//!
//! Die Zentraldatei (Schema 2) speichert nur Basisfakten zwischen globalen Personen-IDs:
//! `rels` (kanonisch wie [`relations::Edge`]), `rel_tombstones` (bewusste Löschungen) und
//! `rel_decisions` (abgelehnte Rückfragen). Der Abgleich legt nie über, löscht nie still:
//! Fehlendes wird mit voller Prüfung ([`relations::validate_add`]) ergänzt, alles andere
//! wird als [`RelConflict`] gemeldet. Beziehungen haben keinen Gesichtsbezug (keine Biometrie).
use super::{Central, CentralConfig, Local, SyncNote, SyncReport, load_local, now, warn};
use crate::core::{
    collection::Collection,
    gaps,
    relations::{self, Conflict, Edge, Gender, Kind, Role, Validator, validate_add},
};
use anyhow::{Result, bail};
use rusqlite::{Connection, TransactionBehavior, params};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Zusätzliche Tabellen von Schema 2; ändert nichts an `people`/`refs`.
pub(super) fn create_tables(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS rels(a TEXT NOT NULL, kind INTEGER NOT NULL, b TEXT NOT NULL,
            created TEXT NOT NULL, PRIMARY KEY(a,kind,b)) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS rels_b ON rels(b);
         CREATE TABLE IF NOT EXISTS rel_tombstones(a TEXT NOT NULL, kind INTEGER NOT NULL,
            b TEXT NOT NULL, deleted TEXT NOT NULL, PRIMARY KEY(a,kind,b)) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS rel_decisions(kind TEXT NOT NULL, a TEXT NOT NULL,
            b TEXT NOT NULL, PRIMARY KEY(kind,a,b)) WITHOUT ROWID;",
    )?;
    Ok(())
}

/// Kanonische Beziehung zwischen globalen IDs. Eltern: `a` ist Elternteil von `b`;
/// symmetrische Typen: `a < b`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CentralEdge {
    pub a: String,
    pub kind: Kind,
    pub b: String,
}

impl CentralEdge {
    pub fn new(a: &str, kind: Kind, b: &str) -> Self {
        let (a, b) = if kind.symmetric() && a > b {
            (b, a)
        } else {
            (a, b)
        };
        Self {
            a: a.to_string(),
            kind,
            b: b.to_string(),
        }
    }
    /// Folgt Aliasen zusammengeführter Personen; Selbstbezüge entfallen.
    fn resolve(&self, aliases: &HashMap<String, String>) -> Option<Self> {
        let map = |g: &String| aliases.get(g).unwrap_or(g).clone();
        let (a, b) = (map(&self.a), map(&self.b));
        (a != b).then(|| Self::new(&a, self.kind, &b))
    }
}

/// Zentral gespeicherte Rückfrage-Entscheidungen (Paare von globalen IDs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CentralDecisions {
    /// (Kind, Elternteil) ausdrücklich abgelehnt.
    pub declined: BTreeSet<(String, String)>,
    /// Verneinte Freundschaften, Paar sortiert.
    pub friends: BTreeSet<(String, String)>,
}

fn sorted(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.into(), b.into())
    } else {
        (b.into(), a.into())
    }
}

impl Central {
    fn edge_set(
        &self,
        table: &str,
        aliases: &HashMap<String, String>,
    ) -> Result<BTreeSet<CentralEdge>> {
        let mut statement = self.db.prepare(&format!("SELECT a,kind,b FROM {table}"))?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = BTreeSet::new();
        for row in rows {
            let (a, kind, b) = row?;
            if let Some(kind) = Kind::from_id(kind)
                && let Some(edge) = CentralEdge::new(&a, kind, &b).resolve(aliases)
            {
                out.insert(edge);
            }
        }
        Ok(out)
    }

    /// Alle zentralen Beziehungen, Aliase aufgelöst, doppelfrei.
    pub fn relation_edges(&self) -> Result<BTreeSet<CentralEdge>> {
        self.edge_set("rels", &self.aliases()?)
    }
    /// Bewusst gelöschte Beziehungen (Grabsteine).
    pub fn relation_tombstones(&self) -> Result<BTreeSet<CentralEdge>> {
        self.edge_set("rel_tombstones", &self.aliases()?)
    }

    /// Fügt Beziehungen ein (bestehende bleiben). Ohne `lift` verhindert ein Grabstein das
    /// Anlegen; mit `lift` (bewusstes Neuanlegen) wird der Grabstein aufgehoben. Beide Personen
    /// müssen zentral aktiv sein. Liefert die Zahl neu angelegter Zeilen.
    pub fn add_relations(&mut self, edges: &[CentralEdge], lift: bool) -> Result<usize> {
        if edges.is_empty() {
            return Ok(0);
        }
        let aliases = self.aliases()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stamp = now();
        let mut added = 0;
        for edge in edges.iter().filter_map(|e| e.resolve(&aliases)) {
            let kind = edge.kind.id();
            if lift {
                tx.execute(
                    "DELETE FROM rel_tombstones WHERE a=? AND kind=? AND b=?",
                    params![edge.a, kind, edge.b],
                )?;
            }
            added += tx.execute(
                "INSERT OR IGNORE INTO rels(a,kind,b,created)
                 SELECT ?1,?2,?3,?4
                 WHERE EXISTS(SELECT 1 FROM people WHERE gid=?1 AND merged_into IS NULL)
                   AND EXISTS(SELECT 1 FROM people WHERE gid=?3 AND merged_into IS NULL)
                   AND NOT EXISTS(SELECT 1 FROM rel_tombstones WHERE a=?1 AND kind=?2 AND b=?3)",
                params![edge.a, kind, edge.b, stamp],
            )?;
        }
        tx.commit()?;
        Ok(added)
    }

    /// Löscht eine zentrale Beziehung und merkt sich das als Grabstein.
    pub fn remove_relation(&mut self, edge: &CentralEdge) -> Result<()> {
        let Some(edge) = edge.resolve(&self.aliases()?) else {
            return Ok(());
        };
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let kind = edge.kind.id();
        tx.execute(
            "DELETE FROM rels WHERE a=? AND kind=? AND b=?",
            params![edge.a, kind, edge.b],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO rel_tombstones(a,kind,b,deleted) VALUES(?,?,?,?)",
            params![edge.a, kind, edge.b, now()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn relation_decisions(&self) -> Result<CentralDecisions> {
        let aliases = self.aliases()?;
        let map = |g: String| aliases.get(&g).cloned().unwrap_or(g);
        let rows: Vec<(String, String, String)> = self
            .db
            .prepare("SELECT kind,a,b FROM rel_decisions")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = CentralDecisions::default();
        for (kind, a, b) in rows {
            let (a, b) = (map(a), map(b));
            if a == b {
                continue;
            }
            match kind.as_str() {
                "parent" => out.declined.insert((a, b)),
                _ => out.friends.insert(sorted(&a, &b)),
            };
        }
        Ok(out)
    }

    /// Vereinigt Entscheidungen mit den zentralen (nie entfernt); liefert die neuen Zeilen.
    pub fn add_relation_decisions(&mut self, decisions: &CentralDecisions) -> Result<usize> {
        if decisions.declined.is_empty() && decisions.friends.is_empty() {
            return Ok(0);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut added = 0;
        for (a, b) in &decisions.declined {
            added += tx.execute(
                "INSERT OR IGNORE INTO rel_decisions(kind,a,b) VALUES('parent',?,?)",
                params![a, b],
            )?;
        }
        for (a, b) in &decisions.friends {
            added += tx.execute(
                "INSERT OR IGNORE INTO rel_decisions(kind,a,b) VALUES('friend',?,?)",
                params![a, b],
            )?;
        }
        tx.commit()?;
        Ok(added)
    }
}

/// Hängt bei einer zentralen Zusammenführung Beziehungen, Grabsteine und Entscheidungen von
/// `from` auf `to` um. Beziehungen laufen durch [`relations::plan_merge`]; widerspricht der Plan
/// sich, wird nur umgehängt (Selbstbezüge und Dubletten entfallen), geprüft wird erst lokal.
pub(super) fn rehang(db: &Connection, from: &str, to: &str) -> Result<()> {
    let rows: Vec<(String, i64, String)> = db
        .prepare("SELECT a,kind,b FROM rels")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    if !rows.iter().any(|(a, _, b)| a == from || b == from) {
        return rehang_rest(db, from, to);
    }
    let mut graph = Graph::default();
    let (source, target) = (graph.id(from), graph.id(to));
    let edges: Vec<Edge> = rows
        .iter()
        .filter_map(|(a, kind, b)| Some(graph.edge(&CentralEdge::new(a, Kind::from_id(*kind)?, b))))
        .collect();
    let (add, drop): (Vec<CentralEdge>, Vec<CentralEdge>) =
        match relations::plan_merge(&edges, source, target) {
            Ok(plan) => (
                plan.add.iter().map(|e| graph.back(e)).collect(),
                plan.drop.iter().map(|e| graph.back(e)).collect(),
            ),
            Err(_) => (
                rows.iter()
                    .filter(|(a, _, b)| a == from || b == from)
                    .filter_map(|(a, kind, b)| {
                        let map = |g: &String| if g == from { to.to_string() } else { g.clone() };
                        let (a, b) = (map(a), map(b));
                        let kind = Kind::from_id(*kind)?;
                        (a != b).then(|| CentralEdge::new(&a, kind, &b))
                    })
                    .collect(),
                Vec::new(),
            ),
        };
    db.execute("DELETE FROM rels WHERE a=?1 OR b=?1", [from])?;
    let stamp = now();
    for edge in drop {
        db.execute(
            "DELETE FROM rels WHERE a=? AND kind=? AND b=?",
            params![edge.a, edge.kind.id(), edge.b],
        )?;
    }
    for edge in add {
        db.execute(
            "INSERT OR IGNORE INTO rels(a,kind,b,created) VALUES(?,?,?,?)",
            params![edge.a, edge.kind.id(), edge.b, stamp],
        )?;
    }
    rehang_rest(db, from, to)
}

fn rehang_rest(db: &Connection, from: &str, to: &str) -> Result<()> {
    let stamp = now();
    let tombs: Vec<(String, i64, String)> = db
        .prepare("SELECT a,kind,b FROM rel_tombstones WHERE a=?1 OR b=?1")?
        .query_map([from], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    db.execute("DELETE FROM rel_tombstones WHERE a=?1 OR b=?1", [from])?;
    for (a, kind, b) in tombs {
        let map = |g: String| if g == from { to.to_string() } else { g };
        let (a, b) = (map(a), map(b));
        if let (true, Some(kind)) = (a != b, Kind::from_id(kind)) {
            let edge = CentralEdge::new(&a, kind, &b);
            db.execute(
                "INSERT OR IGNORE INTO rel_tombstones(a,kind,b,deleted) VALUES(?,?,?,?)",
                params![edge.a, edge.kind.id(), edge.b, stamp],
            )?;
        }
    }
    let decisions: Vec<(String, String, String)> = db
        .prepare("SELECT kind,a,b FROM rel_decisions WHERE a=?1 OR b=?1")?
        .query_map([from], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    db.execute("DELETE FROM rel_decisions WHERE a=?1 OR b=?1", [from])?;
    for (kind, a, b) in decisions {
        let map = |g: String| if g == from { to.to_string() } else { g };
        let (a, b) = (map(a), map(b));
        if a != b {
            let (a, b) = if kind == "parent" {
                (a, b)
            } else {
                sorted(&a, &b)
            };
            db.execute(
                "INSERT OR IGNORE INTO rel_decisions(kind,a,b) VALUES(?,?,?)",
                params![kind, a, b],
            )?;
        }
    }
    Ok(())
}

/// Index-Abbild globaler IDs, damit die Prüfungen aus [`relations`] (i64) wiederverwendet werden.
#[derive(Default)]
struct Graph {
    index: HashMap<String, i64>,
    names: Vec<String>,
}

impl Graph {
    fn id(&mut self, gid: &str) -> i64 {
        if let Some(id) = self.index.get(gid) {
            return *id;
        }
        let id = self.names.len() as i64;
        self.index.insert(gid.to_string(), id);
        self.names.push(gid.to_string());
        id
    }
    fn edge(&mut self, e: &CentralEdge) -> Edge {
        let (a, b) = (self.id(&e.a), self.id(&e.b));
        local_edge(e.kind, a, b)
    }
    fn back(&self, e: &Edge) -> CentralEdge {
        CentralEdge::new(
            &self.names[e.from as usize],
            e.kind,
            &self.names[e.to as usize],
        )
    }
}

/// Prüfer über dem zentralen Graphen (globale IDs).
struct CentralCheck {
    graph: Graph,
    validator: Validator,
}

impl CentralCheck {
    fn new(edges: &BTreeSet<CentralEdge>) -> Self {
        let mut graph = Graph::default();
        let list: Vec<Edge> = edges.iter().map(|e| graph.edge(e)).collect();
        Self {
            graph,
            validator: Validator::new(&list),
        }
    }
    /// Wie [`validate_add`] auf dem zentralen Graphen; legt bei `Ok(true)` die Kante an.
    fn try_add(&mut self, e: &CentralEdge) -> Result<bool, Conflict> {
        let edge = self.graph.edge(e);
        self.validator.try_add(edge)
    }
}

// ---- Sammlungsseite ----

/// Ergebnis des Beziehungsabgleichs für den Bericht.
#[derive(Clone, Debug, Default)]
pub struct RelationReport {
    /// Zentrale Beziehungen wurden abgeglichen (sonst bleibt der Bericht leer).
    pub active: bool,
    pub central_added: usize,
    pub local_added: usize,
    /// Beziehungen, deren andere Person lokal (oder zentral) noch fehlt.
    pub skipped_missing: usize,
    /// Lokale Beziehungen, die dem zentralen Stammbaum widersprechen und nicht übernommen wurden.
    pub blocked: usize,
    /// Zentral neu vermerkte bzw. lokal neu übernommene Rückfrage-Entscheidungen.
    pub decisions_central: usize,
    pub decisions_local: usize,
    pub conflicts: Vec<RelConflict>,
    /// Fehler des Beziehungsabgleichs (der Personenabgleich bleibt gültig).
    pub error: Option<String>,
}

impl RelationReport {
    pub fn summary(&self) -> String {
        if let Some(error) = &self.error {
            return format!("Beziehungen: Abgleich nicht möglich ({error})");
        }
        let mut text = format!(
            "Beziehungen: {} zentral ergänzt, {} lokal ergänzt, {} Rückfragen/Konflikte, {} übersprungen (Person fehlt)",
            self.central_added,
            self.local_added,
            self.conflicts.len(),
            self.skipped_missing
        );
        if self.blocked > 0 {
            text.push_str(&format!(
                ", {} nicht zentral übernommen (Widerspruch)",
                self.blocked
            ));
        }
        text
    }
}

/// Widerspruch zwischen lokalem und zentralem Stand einer Beziehung. Ohne `central` ist die
/// lokale Beziehung zentral bewusst gelöscht (Grabstein) und lokal noch vorhanden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelConflict {
    /// Lokale Beziehungen zwischen denselben Personen (lokale IDs, kanonisch).
    pub local: Vec<Edge>,
    /// Zentrale Beziehung in lokalen IDs.
    pub central: Option<Edge>,
    pub local_text: Vec<String>,
    pub central_text: String,
    pub reason: String,
    /// Die zentrale Beziehung ließe sich nach Entfernen der lokalen widerspruchsfrei anlegen.
    pub can_take_central: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelChoice {
    /// Lokal behalten und zentral ersetzen (bzw. wiederherstellen).
    KeepLocal,
    /// Zentral übernehmen: lokale Beziehung entfernen, zentrale anlegen (bzw. nur entfernen).
    TakeCentral,
    /// Keine von beiden behalten.
    Neither,
}

struct View {
    gid_of: HashMap<i64, String>,
    local_of: HashMap<String, i64>,
    names: HashMap<i64, String>,
    genders: BTreeMap<i64, Gender>,
}

fn plural(kind: Kind) -> &'static str {
    match kind {
        Kind::Parent => "Eltern und Kind",
        Kind::Partner => "Partner",
        Kind::FormerPartner => "frühere Partner",
        Kind::Sibling => "Geschwister",
        Kind::Cousin => "Cousins",
        Kind::Friend => "Freunde",
        Kind::Acquaintance => "Bekannte",
    }
}

impl View {
    fn build(locals: &[Local], aliases: &HashMap<String, String>) -> Self {
        let mut view = View {
            gid_of: HashMap::new(),
            local_of: HashMap::new(),
            names: HashMap::new(),
            genders: BTreeMap::new(),
        };
        for local in locals {
            view.names.insert(local.id, local.fields[0].clone());
            view.genders
                .insert(local.id, Gender::parse(&local.fields[1]));
            if let Some(stored) = &local.gid {
                let gid = aliases.get(stored).unwrap_or(stored).clone();
                view.local_of.entry(gid.clone()).or_insert(local.id);
                view.gid_of.insert(local.id, gid);
            }
        }
        view
    }
    fn load(collection: &Collection, central: &Central) -> Result<Self> {
        Ok(Self::build(
            &load_local(&collection.db)?,
            &central.aliases()?,
        ))
    }
    fn name(&self, id: i64) -> String {
        self.names
            .get(&id)
            .filter(|n| !n.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Person {id}"))
    }
    /// Satz zur Beziehung, z. B. „Anna ist Mutter von Ben“.
    fn text(&self, e: &Edge) -> String {
        match e.kind {
            Kind::Parent => format!(
                "{} ist {} von {}",
                self.name(e.from),
                Role::Parent.label(
                    self.genders
                        .get(&e.from)
                        .copied()
                        .unwrap_or(Gender::Unknown)
                ),
                self.name(e.to)
            ),
            kind => format!(
                "{} und {} sind {}",
                self.name(e.from),
                self.name(e.to),
                plural(kind)
            ),
        }
    }
    fn gedge(&self, e: &Edge) -> Option<CentralEdge> {
        let (a, b) = (self.gid_of.get(&e.from)?, self.gid_of.get(&e.to)?);
        (a != b).then(|| CentralEdge::new(a, e.kind, b))
    }
}

fn pair(a: i64, b: i64) -> (i64, i64) {
    (a.min(b), a.max(b))
}

fn local_edge(kind: Kind, a: i64, b: i64) -> Edge {
    if kind.symmetric() {
        Edge {
            from: a.min(b),
            kind,
            to: a.max(b),
        }
    } else {
        Edge {
            from: a,
            kind,
            to: b,
        }
    }
}

/// (Subjekt, Rolle, Objekt) einer kanonischen Kante: „from ist <Rolle> von to“.
fn parts(e: &Edge) -> (i64, Role, i64) {
    let role = e.role_of_other(e.to).map_or(Role::Acquaintance, |(r, _)| r);
    (e.from, role, e.to)
}

fn edges_in(db: &Connection) -> Result<Vec<Edge>> {
    let rows: Vec<(i64, i64, i64)> = db
        .prepare("SELECT from_person,type_id,to_person FROM relations")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(relations::normalize(&rows))
}

#[derive(Default)]
struct Pull {
    added: usize,
    skipped: usize,
    conflicts: Vec<RelConflict>,
    /// Lokale Personenpaare mit Konflikt: ihre lokalen Kanten werden nicht zentral gespiegelt.
    pairs: HashSet<(i64, i64)>,
}

fn conflict(view: &View, edges: &[Edge], edge: Edge, reason: &Conflict) -> RelConflict {
    let key = pair(edge.from, edge.to);
    let (local, rest): (Vec<Edge>, Vec<Edge>) = edges
        .iter()
        .copied()
        .partition(|e| pair(e.from, e.to) == key);
    RelConflict {
        local_text: local.iter().map(|e| view.text(e)).collect(),
        central_text: format!("zentral: {}", view.text(&edge)),
        reason: reason.to_string(),
        can_take_central: matches!(
            validate_add(&rest, edge),
            Ok(_) | Err(Conflict::Redundant(_))
        ),
        local,
        central: Some(edge),
    }
}

/// Übernimmt fehlende zentrale Beziehungen lokal (eine kurze `IMMEDIATE`-Transaktion). Was
/// `validate_add` ablehnt, wird nicht angelegt, sondern als Konflikt gemeldet.
fn pull(collection: &mut Collection, view: &View, central: &BTreeSet<CentralEdge>) -> Result<Pull> {
    let tx = collection
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut validator = Validator::new(&edges_in(&tx)?);
    let mut out = Pull::default();
    let mut fresh = Vec::new();
    for ce in central {
        let (x, y) = (view.local_of.get(&ce.a), view.local_of.get(&ce.b));
        let (Some(&x), Some(&y)) = (x, y) else {
            out.skipped += usize::from(x.is_some() || y.is_some());
            continue;
        };
        if x == y {
            continue;
        }
        let edge = local_edge(ce.kind, x, y);
        match validator.try_add(edge) {
            Ok(true) => fresh.push(edge),
            Ok(false) | Err(Conflict::Redundant(_)) => {}
            Err(reason) => {
                out.pairs.insert(pair(x, y));
                out.conflicts
                    .push(conflict(view, validator.edges(), edge, &reason));
            }
        }
    }
    if !fresh.is_empty() {
        tx.execute_batch(&relations::seed_sql())?;
        let mut insert = tx.prepare("INSERT OR IGNORE INTO relations VALUES(?,?,?)")?;
        for edge in &fresh {
            out.added += insert.execute(params![edge.from, edge.kind.id(), edge.to])?;
        }
    }
    tx.commit()?;
    Ok(out)
}

/// Spiegelt lokale Beziehungen zentral. Grabsteine verhindern das Wiederauftauchen: die lokale
/// Kante bleibt dann bestehen und wird als Rückfrage gemeldet (kein stilles Löschen).
fn push(
    collection: &Collection,
    central: &mut Central,
    view: &View,
    (c_edges, tombs): (&BTreeSet<CentralEdge>, &BTreeSet<CentralEdge>),
    pairs: &HashSet<(i64, i64)>,
    report: &mut RelationReport,
) -> Result<()> {
    let mut graph = CentralCheck::new(c_edges);
    let mut fresh = Vec::new();
    for edge in relations::load_edges(collection)? {
        if pairs.contains(&pair(edge.from, edge.to)) {
            continue;
        }
        let Some(ge) = view.gedge(&edge) else {
            report.skipped_missing += usize::from(
                !(view.gid_of.contains_key(&edge.from) && view.gid_of.contains_key(&edge.to)),
            );
            continue;
        };
        if c_edges.contains(&ge) {
            continue;
        }
        if tombs.contains(&ge) {
            report.conflicts.push(RelConflict {
                local_text: vec![view.text(&edge)],
                local: vec![edge],
                central: None,
                central_text: "zentral: in einer Sammlung bewusst gelöscht".into(),
                reason: "Die Beziehung wurde gelöscht und wird nicht erneut zentral angelegt"
                    .into(),
                can_take_central: true,
            });
            continue;
        }
        match graph.try_add(&ge) {
            Ok(true) => fresh.push(ge),
            Ok(false) | Err(Conflict::Redundant(_)) => {}
            Err(_) => report.blocked += 1,
        }
    }
    report.central_added = central.add_relations(&fresh, false)?;
    Ok(())
}

/// Vereinigt Rückfrage-Entscheidungen (Mengenvereinigung, nie entfernt). Gibt die neu lokal
/// übernommene Zahl zurück und die zentral zu ergänzenden Entscheidungen.
fn decisions(
    collection: &Collection,
    central: &Central,
    view: &View,
) -> Result<(usize, CentralDecisions)> {
    let local = gaps::load_decisions(collection)?;
    let remote = central.relation_decisions()?;
    let mut merged = local.clone();
    let mut to_central = CentralDecisions::default();
    for (child, parent) in &local.declined {
        if let (Some(c), Some(p)) = (view.gid_of.get(child), view.gid_of.get(parent))
            && !remote.declined.contains(&(c.clone(), p.clone()))
        {
            to_central.declined.insert((c.clone(), p.clone()));
        }
    }
    for (a, b) in &local.declined_friends {
        if let (Some(x), Some(y)) = (view.gid_of.get(a), view.gid_of.get(b)) {
            let key = sorted(x, y);
            if !remote.friends.contains(&key) {
                to_central.friends.insert(key);
            }
        }
    }
    for (c, p) in &remote.declined {
        if let (Some(&c), Some(&p)) = (view.local_of.get(c), view.local_of.get(p)) {
            merged.declined.insert((c, p));
        }
    }
    for (a, b) in &remote.friends {
        if let (Some(&x), Some(&y)) = (view.local_of.get(a), view.local_of.get(b)) {
            merged.declined_friends.insert(pair(x, y));
        }
    }
    let pulled = merged.declined.len() - local.declined.len() + merged.declined_friends.len()
        - local.declined_friends.len();
    if pulled > 0 {
        gaps::save_decisions(collection, &merged)?;
    }
    Ok((pulled, to_central))
}

/// Gleicht Beziehungen und Rückfrage-Entscheidungen zwischen Sammlung und Zentrale ab.
/// Reihenfolge: zentralen Stand lesen, lokal ergänzen (eine Transaktion), danach zentral
/// ergänzen (eine Transaktion). Beides ist idempotent; ein Abbruch dazwischen heilt der nächste
/// Abgleich. Voraussetzung: Personen sind bereits verknüpft ([`super::sync`]).
pub fn sync_relations(
    collection: &mut Collection,
    central: &mut Central,
) -> Result<RelationReport> {
    collection.writable()?;
    let view = View::load(collection, central)?;
    let aliases = central.aliases()?;
    let c_edges = central.edge_set("rels", &aliases)?;
    let tombs = central.edge_set("rel_tombstones", &aliases)?;
    let pulled = pull(collection, &view, &c_edges)?;
    let mut report = RelationReport {
        active: true,
        local_added: pulled.added,
        skipped_missing: pulled.skipped,
        conflicts: pulled.conflicts,
        ..Default::default()
    };
    push(
        collection,
        central,
        &view,
        (&c_edges, &tombs),
        &pulled.pairs,
        &mut report,
    )?;
    let (local_new, to_central) = decisions(collection, central, &view)?;
    report.decisions_local = local_new;
    report.decisions_central = central.add_relation_decisions(&to_central)?;
    Ok(report)
}

/// Nach `adopt`: Beziehungen der neuen Personen zu vorhandenen lokal nachziehen. Fehler sind
/// unkritisch (die Personen sind schon übernommen), der nächste Abgleich holt nach.
pub(super) fn pull_after_adopt(collection: &mut Collection, central: &Central) -> Result<()> {
    let result = (|| -> Result<()> {
        let view = View::load(collection, central)?;
        pull(collection, &view, &central.relation_edges()?)?;
        decisions(collection, central, &view)?;
        Ok(())
    })();
    drop(result);
    Ok(())
}

/// Beantwortet einen Beziehungskonflikt ausdrücklich; es wird nur geändert, was die Wahl nennt.
/// Danach läuft ein Abgleich und liefert den aktuellen Bericht.
pub fn resolve_relation(
    collection: &mut Collection,
    central: &mut Central,
    conflict: &RelConflict,
    choice: RelChoice,
) -> Result<SyncReport> {
    collection.writable()?;
    let view = View::load(collection, central)?;
    let local_now = relations::load_edges(collection)?;
    if conflict.local.iter().any(|e| !local_now.contains(e)) {
        bail!("Die Beziehung wurde inzwischen geändert; bitte erneut abgleichen");
    }
    let gedge = |e: &Edge| {
        view.gedge(e).ok_or_else(|| {
            anyhow::anyhow!("Person ist nicht mehr mit dem zentralen Bestand verknüpft")
        })
    };
    let locals = conflict
        .local
        .iter()
        .map(gedge)
        .collect::<Result<Vec<_>>>()?;
    match choice {
        RelChoice::KeepLocal => {
            if let Some(c) = &conflict.central {
                central.remove_relation(&gedge(c)?)?;
            }
            central.add_relations(&locals, true)?;
        }
        RelChoice::TakeCentral | RelChoice::Neither => {
            let rest: Vec<Edge> = local_now
                .iter()
                .copied()
                .filter(|e| !conflict.local.contains(e))
                .collect();
            if choice == RelChoice::TakeCentral
                && let Some(c) = &conflict.central
                && let Err(reason) = validate_add(&rest, *c)
                && !matches!(reason, Conflict::Redundant(_))
            {
                bail!("{reason}");
            }
            for edge in &locals {
                central.remove_relation(edge)?;
            }
            if choice == RelChoice::Neither
                && let Some(c) = &conflict.central
            {
                central.remove_relation(&gedge(c)?)?;
            }
            for edge in &conflict.local {
                let (subject, role, object) = parts(edge);
                relations::remove(collection, subject, role, object)?;
            }
            if choice == RelChoice::TakeCentral
                && let Some(c) = &conflict.central
            {
                let (subject, role, object) = parts(c);
                relations::add(collection, subject, role, object)?;
            }
        }
    }
    super::sync(collection, central)
}

fn linked(collection: &Collection, ids: [i64; 2]) -> Result<Option<(String, String)>> {
    let locals = load_local(&collection.db)?;
    let gid = |id: i64| {
        locals
            .iter()
            .find(|l| l.id == id)
            .and_then(|l| l.gid.clone())
    };
    Ok(gid(ids[0]).zip(gid(ids[1])))
}

/// Wie [`relations::add`]; legt die Beziehung bei aktiven zentralen Beziehungen sofort zentral
/// an und hebt dabei einen Grabstein auf (bewusstes Neuanlegen).
pub fn add_relation_synced(
    collection: &mut Collection,
    config: &CentralConfig,
    subject: i64,
    role: Role,
    object: i64,
) -> Result<(bool, SyncNote)> {
    let fresh = relations::add(collection, subject, role, object)?;
    let mut note = SyncNote::default();
    if config.enabled
        && config.relations
        && let Some((a, b)) = linked(collection, [subject, object])?
    {
        let edge = relations::canonical(subject, role, object);
        let (ga, gb) = if edge.from == subject {
            (&a, &b)
        } else {
            (&b, &a)
        };
        let result = Central::open_config(config)
            .and_then(|mut c| c.add_relations(&[CentralEdge::new(ga, edge.kind, gb)], true));
        if let Err(error) = result {
            note.warning = Some(warn(&error));
        }
    }
    Ok((fresh, note))
}

/// Wie [`relations::remove`]; schreibt bei aktiven zentralen Beziehungen und verknüpften Personen
/// einen Grabstein, damit die Beziehung beim Abgleich anderer Sammlungen nicht wiederkehrt.
pub fn remove_relation_synced(
    collection: &Collection,
    config: &CentralConfig,
    subject: i64,
    role: Role,
    object: i64,
) -> Result<SyncNote> {
    let gids = linked(collection, [subject, object])?;
    relations::remove(collection, subject, role, object)?;
    let mut note = SyncNote::default();
    if config.enabled
        && config.relations
        && let Some((a, b)) = gids
    {
        let edge = relations::canonical(subject, role, object);
        let (ga, gb) = if edge.from == subject {
            (&a, &b)
        } else {
            (&b, &a)
        };
        let result = Central::open_config(config)
            .and_then(|mut c| c.remove_relation(&CentralEdge::new(ga, edge.kind, gb)));
        if let Err(error) = result {
            note.warning = Some(warn(&error));
        }
    }
    Ok(note)
}
