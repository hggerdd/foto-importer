//! Beziehungen zwischen Personen: Typkatalog, Beschriftungen und Validierung
//! (ADR 0060, ADR 0061). Die Ableitung (Großeltern, Cousins, Schwäger ...) steht im
//! Verwandtschaftsrechner [`super::kinship`].
//!
//! Alles Fachliche steht hier als reine Funktionen ohne Datenbankzugriff. Nur
//! `load_edges`, `add` und `remove` berühren die Sammlung. Gespeichert wird jede
//! Beziehung genau einmal in kanonischer Form (siehe [`canonical`]); die Sicht
//! "x ist Mutter von y" und "y ist Tochter von x" ist derselbe Datensatz.
use super::collection::Collection;
use anyhow::{Result, bail};
use rusqlite::params;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Grammatisches Geschlecht aus dem Personenattribut `gender` (Freitext).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gender {
    Female,
    Male,
    Unknown,
}

impl Gender {
    /// Erkennt gängige Schreibweisen; alles andere ist `Unknown` (neutrale Form).
    pub fn parse(text: &str) -> Gender {
        match text.trim().to_lowercase().as_str() {
            "w" | "f" | "weiblich" | "frau" | "female" | "woman" | "mädchen" => Gender::Female,
            "m" | "männlich" | "maennlich" | "mann" | "male" | "man" | "junge" => Gender::Male,
            _ => Gender::Unknown,
        }
    }
}

/// Gespeicherter Beziehungstyp (Zeile von `relation_types`, ohne Umkehrtyp 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// `from` ist Elternteil von `to`.
    Parent,
    Partner,
    /// Frühere Partnerschaft (ID 8, ADR 0061): bleibt sichtbar, begründet aber
    /// keine Verschwägerung.
    FormerPartner,
    /// Behelf (ADR 0061): nur solange die Eltern nicht erfasst sind.
    Sibling,
    /// Behelf (ADR 0061).
    Cousin,
    Friend,
    Acquaintance,
}

/// Eine Zeile des Typkatalogs, identisch zu `relation_types`.
pub struct TypeRow {
    pub id: i64,
    pub name: &'static str,
    pub inverse_id: i64,
    pub symmetric: bool,
}

/// Einzige Quelle der Wahrheit für `relation_types`. Die IDs 1–4 sind
/// unveränderlich; ID 2 ist der Umkehrtyp von 1 und wird nie neu gespeichert.
pub const CATALOG: [TypeRow; 8] = [
    TypeRow {
        id: 1,
        name: "elternteil_von",
        inverse_id: 2,
        symmetric: false,
    },
    TypeRow {
        id: 2,
        name: "kind_von",
        inverse_id: 1,
        symmetric: false,
    },
    TypeRow {
        id: 3,
        name: "partner_von",
        inverse_id: 3,
        symmetric: true,
    },
    TypeRow {
        id: 4,
        name: "geschwister_von",
        inverse_id: 4,
        symmetric: true,
    },
    TypeRow {
        id: 5,
        name: "cousin_von",
        inverse_id: 5,
        symmetric: true,
    },
    TypeRow {
        id: 6,
        name: "freund_von",
        inverse_id: 6,
        symmetric: true,
    },
    TypeRow {
        id: 7,
        name: "bekannter_von",
        inverse_id: 7,
        symmetric: true,
    },
    TypeRow {
        id: 8,
        name: "ehemaliger_partner_von",
        inverse_id: 8,
        symmetric: true,
    },
];

/// SQL, das fehlende Katalogzeilen ergänzt, ohne vorhandene zu ändern.
pub fn seed_sql() -> String {
    let rows: Vec<String> = CATALOG
        .iter()
        .map(|t| {
            format!(
                "({},'{}',{},{})",
                t.id, t.name, t.inverse_id, t.symmetric as i64
            )
        })
        .collect();
    format!(
        "INSERT OR IGNORE INTO relation_types VALUES{};",
        rows.join(",")
    )
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Parent,
        Kind::Partner,
        Kind::FormerPartner,
        Kind::Sibling,
        Kind::Cousin,
        Kind::Friend,
        Kind::Acquaintance,
    ];
    pub fn id(self) -> i64 {
        match self {
            Kind::Parent => 1,
            Kind::Partner => 3,
            Kind::Sibling => 4,
            Kind::Cousin => 5,
            Kind::Friend => 6,
            Kind::Acquaintance => 7,
            Kind::FormerPartner => 8,
        }
    }
    /// Typ zu einer gespeicherten ID. ID 2 (`kind_von`) liefert `Parent`; dann
    /// sind `from`/`to` zu vertauschen (siehe [`normalize`]).
    pub fn from_id(id: i64) -> Option<Kind> {
        Some(match id {
            1 | 2 => Kind::Parent,
            3 => Kind::Partner,
            4 => Kind::Sibling,
            5 => Kind::Cousin,
            6 => Kind::Friend,
            7 => Kind::Acquaintance,
            8 => Kind::FormerPartner,
            _ => return None,
        })
    }
    pub fn symmetric(self) -> bool {
        self != Kind::Parent
    }
    /// Name in `relation_types`.
    pub fn name(self) -> &'static str {
        CATALOG
            .iter()
            .find(|t| t.id == self.id())
            .map_or("", |t| t.name)
    }
    /// Kurzer Name für Fehlermeldungen.
    pub fn title(self) -> &'static str {
        match self {
            Kind::Parent => "Eltern-Kind",
            Kind::Partner => "Partner",
            Kind::FormerPartner => "Frühere Partner",
            Kind::Sibling => "Geschwister",
            Kind::Cousin => "Cousin/Cousine",
            Kind::Friend => "Freunde",
            Kind::Acquaintance => "Bekannte",
        }
    }
}

/// Seite einer gespeicherten Beziehung.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Die Person steht in `from_person`.
    From,
    /// Die Person steht in `to_person`.
    To,
}

/// Rolle einer Person gegenüber einer anderen: "x ist <Rolle> von y".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Parent,
    Child,
    Partner,
    FormerPartner,
    Sibling,
    Cousin,
    Friend,
    Acquaintance,
}

impl Role {
    pub const ALL: [Role; 8] = [
        Role::Parent,
        Role::Child,
        Role::Partner,
        Role::FormerPartner,
        Role::Sibling,
        Role::Cousin,
        Role::Friend,
        Role::Acquaintance,
    ];
    /// Schlüssel der API (`faces::add_relation`).
    pub fn key(self) -> &'static str {
        match self {
            Role::Parent => "parent",
            Role::Child => "child",
            Role::Partner => "partner",
            Role::FormerPartner => "former_partner",
            Role::Sibling => "sibling",
            Role::Cousin => "cousin",
            Role::Friend => "friend",
            Role::Acquaintance => "acquaintance",
        }
    }
    pub fn from_key(key: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.key() == key)
    }
    /// Gegenrolle: "x ist Mutter von y" heißt "y ist Kind von x".
    pub fn inverse(self) -> Role {
        match self {
            Role::Parent => Role::Child,
            Role::Child => Role::Parent,
            other => other,
        }
    }
    pub fn kind(self) -> Kind {
        match self {
            Role::Parent | Role::Child => Kind::Parent,
            Role::Partner => Kind::Partner,
            Role::FormerPartner => Kind::FormerPartner,
            Role::Sibling => Kind::Sibling,
            Role::Cousin => Kind::Cousin,
            Role::Friend => Kind::Friend,
            Role::Acquaintance => Kind::Acquaintance,
        }
    }
    /// Beschriftung der Rolle für die Person, die sie trägt (Geschlecht des Subjekts).
    pub fn label(self, gender: Gender) -> &'static str {
        use Gender::*;
        let (f, m, n) = match self {
            Role::Parent => ("Mutter", "Vater", "Elternteil"),
            Role::Child => ("Tochter", "Sohn", "Kind"),
            Role::Partner => ("Partnerin", "Partner", "Partner/in"),
            Role::FormerPartner => ("Ex-Partnerin", "Ex-Partner", "Ex-Partner/in"),
            Role::Sibling => ("Schwester", "Bruder", "Geschwisterteil"),
            Role::Cousin => ("Cousine", "Cousin", "Cousin/Cousine"),
            Role::Friend => ("Freundin", "Freund", "Freund/in"),
            Role::Acquaintance => ("Bekannte", "Bekannter", "Bekannte/r"),
        };
        match gender {
            Female => f,
            Male => m,
            Unknown => n,
        }
    }
    /// Überschrift der gruppierten Übersicht (Mehrzahl).
    pub fn group_label(self) -> &'static str {
        match self {
            Role::Parent => "Eltern",
            Role::Child => "Kinder",
            Role::Partner => "Partner",
            Role::FormerPartner => "Frühere Partner",
            Role::Sibling => "Geschwister (direkt erfasst)",
            Role::Cousin => "Cousins und Cousinen (direkt erfasst)",
            Role::Friend => "Freunde",
            Role::Acquaintance => "Bekannte",
        }
    }
}

/// Beschriftung für eine gespeicherte Zeile aus Sicht der angegebenen Seite.
/// `type_id` ist die ID aus `relation_types` (auch 2 wird verstanden).
pub fn label(type_id: i64, side: Direction, subject: Gender) -> Option<&'static str> {
    stored_role(type_id, side).map(|role| role.label(subject))
}

/// Rolle der Person auf der angegebenen Seite einer gespeicherten Zeile.
pub fn stored_role(type_id: i64, side: Direction) -> Option<Role> {
    let kind = Kind::from_id(type_id)?;
    // Typ 2 (kind_von) liest `from` als Kind.
    let reversed = type_id == 2;
    Some(match (kind, side, reversed) {
        (Kind::Parent, Direction::From, false) | (Kind::Parent, Direction::To, true) => {
            Role::Parent
        }
        (Kind::Parent, _, _) => Role::Child,
        (Kind::Partner, _, _) => Role::Partner,
        (Kind::FormerPartner, _, _) => Role::FormerPartner,
        (Kind::Sibling, _, _) => Role::Sibling,
        (Kind::Cousin, _, _) => Role::Cousin,
        (Kind::Friend, _, _) => Role::Friend,
        (Kind::Acquaintance, _, _) => Role::Acquaintance,
    })
}

/// Kanonisch gespeicherte Beziehung. Eltern: `from` ist Elternteil von `to`.
/// Symmetrische Typen: `from < to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge {
    pub from: i64,
    pub kind: Kind,
    pub to: i64,
}

/// Kanonische Form von "`subject` ist `role` von `object`".
pub fn canonical(subject: i64, role: Role, object: i64) -> Edge {
    match role {
        Role::Parent => Edge {
            from: subject,
            kind: Kind::Parent,
            to: object,
        },
        Role::Child => Edge {
            from: object,
            kind: Kind::Parent,
            to: subject,
        },
        other => Edge {
            from: subject.min(object),
            kind: other.kind(),
            to: subject.max(object),
        },
    }
}

impl Edge {
    /// Rolle der Gegenseite aus Sicht von `person` ("other ist <Rolle> von person").
    pub fn role_of_other(&self, person: i64) -> Option<(Role, i64)> {
        if self.from == person {
            Some((
                match self.kind {
                    Kind::Parent => Role::Child,
                    k => role_of_symmetric(k),
                },
                self.to,
            ))
        } else if self.to == person {
            Some((
                match self.kind {
                    Kind::Parent => Role::Parent,
                    k => role_of_symmetric(k),
                },
                self.from,
            ))
        } else {
            None
        }
    }
}

fn role_of_symmetric(kind: Kind) -> Role {
    match kind {
        Kind::Partner => Role::Partner,
        Kind::FormerPartner => Role::FormerPartner,
        Kind::Sibling => Role::Sibling,
        Kind::Cousin => Role::Cousin,
        Kind::Friend => Role::Friend,
        _ => Role::Acquaintance,
    }
}

/// Gespeicherte Zeilen in kanonische, doppelfreie, sortierte Form bringen.
/// Unbekannte Typ-IDs und Selbstbezüge werden übergangen.
pub fn normalize(rows: &[(i64, i64, i64)]) -> Vec<Edge> {
    let set: BTreeSet<Edge> = rows
        .iter()
        .filter_map(|&(from, type_id, to)| {
            let kind = Kind::from_id(type_id)?;
            if from == to {
                return None;
            }
            let (from, to) = if type_id == 2 { (to, from) } else { (from, to) };
            Some(if kind.symmetric() {
                Edge {
                    from: from.min(to),
                    kind,
                    to: from.max(to),
                }
            } else {
                Edge { from, kind, to }
            })
        })
        .collect();
    set.into_iter().collect()
}

/// Alle gespeicherten Beziehungen der Sammlung, kanonisch.
pub fn load_edges(collection: &Collection) -> Result<Vec<Edge>> {
    let rows: Vec<(i64, i64, i64)> = collection
        .db
        .prepare("SELECT from_person,type_id,to_person FROM relations")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(normalize(&rows))
}

/// Geschlechter aller Personen aus dem Attribut `gender`.
pub fn load_genders(collection: &Collection) -> Result<BTreeMap<i64, Gender>> {
    let rows: Vec<(i64, String)> = collection
        .db
        .prepare("SELECT person_id,value FROM person_attrs WHERE key='gender'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows
        .into_iter()
        .map(|(id, text)| (id, Gender::parse(&text)))
        .collect())
}

/// Stammbaum der Sammlung für den Verwandtschaftsrechner.
pub fn load_family(collection: &Collection) -> Result<super::kinship::Family> {
    Ok(
        super::kinship::Family::new(&load_edges(collection)?, &load_genders(collection)?)
            .with_declined(super::gaps::load_decisions(collection)?.declined),
    )
}

/// Direkte Beziehungen einer Person: (Rolle der anderen Person, andere Person),
/// sortiert nach Rolle und ID.
pub fn direct(edges: &[Edge], person: i64) -> Vec<(Role, i64)> {
    let mut out: Vec<_> = edges
        .iter()
        .filter_map(|e| e.role_of_other(person))
        .collect();
    out.sort();
    out.dedup();
    out
}

// ---------------------------------------------------------------- Validierung

/// Grund, warum eine Beziehung abgelehnt wird; `Display` ist die Nutzermeldung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conflict {
    SelfRelation,
    Cycle,
    Pair(Kind, Kind),
    Lineage(Kind),
    Derived(Kind, String),
    /// Ergibt sich bereits aus dem Stammbaum (nur für Behelfsverbindungen).
    Redundant(Kind),
    Merge(Box<Conflict>),
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Conflict::SelfRelation => {
                write!(f, "Eine Person kann keine Beziehung zu sich selbst haben")
            }
            Conflict::Cycle => write!(f, "Diese Beziehung würde einen Abstammungszyklus erzeugen"),
            Conflict::Pair(existing, new) => write!(
                f,
                "Zwischen diesen Personen besteht bereits „{}“; das widerspricht „{}“. Bitte zuerst die vorhandene Beziehung entfernen",
                existing.title(),
                new.title()
            ),
            Conflict::Lineage(kind) => write!(
                f,
                "„{}“ widerspricht der erfassten Abstammung (direkte Vorfahren und Nachkommen)",
                kind.title()
            ),
            Conflict::Derived(kind, derived) => write!(
                f,
                "„{}“ widerspricht der abgeleiteten Beziehung „{derived}“",
                kind.title()
            ),
            Conflict::Redundant(kind) => write!(
                f,
                "„{}“ ergibt sich bereits aus dem Stammbaum und wird deshalb nicht zusätzlich gespeichert",
                kind.title()
            ),
            Conflict::Merge(inner) => {
                write!(
                    f,
                    "Zusammenführen würde Beziehungen widersprüchlich machen: {inner}"
                )
            }
        }
    }
}

impl std::error::Error for Conflict {}

fn kinds_conflict(a: Kind, b: Kind) -> bool {
    use Kind::*;
    let family = |k| matches!(k, Parent | Partner | FormerPartner | Sibling | Cousin);
    if (a == Friend && b == Acquaintance) || (a == Acquaintance && b == Friend) {
        return true;
    }
    if !family(a) || !family(b) || a == b {
        return false;
    }
    // Einzig erlaubte Kombination innerhalb der Familie: Partner und Cousin.
    !matches!(
        (a, b),
        (Partner | FormerPartner, Cousin) | (Cousin, Partner | FormerPartner)
    )
}

fn parents_of(edges: &[Edge], person: i64) -> BTreeSet<i64> {
    edges
        .iter()
        .filter(|e| e.kind == Kind::Parent && e.to == person)
        .map(|e| e.from)
        .collect()
}

fn children_of(edges: &[Edge], person: i64) -> BTreeSet<i64> {
    edges
        .iter()
        .filter(|e| e.kind == Kind::Parent && e.from == person)
        .map(|e| e.to)
        .collect()
}

fn reach(edges: &[Edge], start: i64, up: bool) -> BTreeSet<i64> {
    let mut seen = BTreeSet::new();
    let mut open = vec![start];
    while let Some(next) = open.pop() {
        let step = if up {
            parents_of(edges, next)
        } else {
            children_of(edges, next)
        };
        for id in step {
            if seen.insert(id) {
                open.push(id);
            }
        }
    }
    seen
}

fn is_family_symmetric(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Partner | Kind::FormerPartner | Kind::Sibling | Kind::Cousin
    )
}

fn is_sibling_relation(rel: super::kinship::Relation) -> bool {
    matches!(
        rel,
        super::kinship::Relation::Blood {
            a_gen: 1,
            b_gen: 1,
            ..
        }
    )
}

/// Behelfsverbindungen (Geschwister/Cousin), die dem Baum widersprechen.
fn inconsistent_shortcuts(edges: &[Edge]) -> BTreeMap<Edge, String> {
    let family = super::kinship::Family::new(edges, &BTreeMap::new());
    let mut out = BTreeMap::new();
    for e in edges
        .iter()
        .filter(|e| matches!(e.kind, Kind::Sibling | Kind::Cousin))
    {
        if let Some(rel) = family.tree_relation(e.from, e.to) {
            let fine = match e.kind {
                Kind::Sibling => is_sibling_relation(rel),
                _ => rel.is_cousin(),
            };
            if !fine {
                out.insert(*e, rel.group_title());
            }
        }
    }
    out
}

/// Prüft eine neue Beziehung gegen den vorhandenen Bestand.
/// `Ok(true)`: neu und widerspruchsfrei; `Ok(false)`: bereits vorhanden.
/// `Err(Conflict::Redundant)`: Behelfsverbindung, die schon aus dem Baum folgt.
pub fn validate_add(edges: &[Edge], new: Edge) -> Result<bool, Conflict> {
    if new.from == new.to {
        return Err(Conflict::SelfRelation);
    }
    if edges.contains(&new) {
        return Ok(false);
    }
    // Zyklus: `from` wäre bereits Nachkomme von `to` (deckt auch die Gegenrichtung ab).
    if new.kind == Kind::Parent && reach(edges, new.to, false).contains(&new.from) {
        return Err(Conflict::Cycle);
    }
    for e in edges {
        let same_pair =
            (e.from == new.from && e.to == new.to) || (e.from == new.to && e.to == new.from);
        if same_pair && kinds_conflict(e.kind, new.kind) {
            return Err(Conflict::Pair(e.kind, new.kind));
        }
    }
    if is_family_symmetric(new.kind) {
        let lineage = reach(edges, new.from, true).contains(&new.to)
            || reach(edges, new.to, true).contains(&new.from);
        if lineage {
            return Err(Conflict::Lineage(new.kind));
        }
        if matches!(new.kind, Kind::Cousin | Kind::Sibling) {
            let family = super::kinship::Family::new(edges, &BTreeMap::new());
            if let Some(rel) = family.tree_relation(new.from, new.to) {
                let same = if new.kind == Kind::Sibling {
                    is_sibling_relation(rel)
                } else {
                    rel.is_cousin()
                };
                return Err(if same {
                    Conflict::Redundant(new.kind)
                } else {
                    Conflict::Derived(new.kind, rel.group_title())
                });
            }
        }
    }
    if new.kind == Kind::Parent {
        // Eine neue Eltern-Kante darf keine Partner/Geschwister/Cousins zu
        // Vorfahr und Nachkomme machen.
        let mut ancestors = reach(edges, new.from, true);
        ancestors.insert(new.from);
        let mut descendants = reach(edges, new.to, false);
        descendants.insert(new.to);
        for e in edges.iter().filter(|e| is_family_symmetric(e.kind)) {
            let hit = (ancestors.contains(&e.from) && descendants.contains(&e.to))
                || (ancestors.contains(&e.to) && descendants.contains(&e.from));
            if hit {
                return Err(Conflict::Lineage(e.kind));
            }
        }
        // Und sie darf keine Behelfsverbindung (Geschwister/Cousin) in einen
        // Widerspruch zum Baum bringen. Schon vorher widersprüchliche Altdaten
        // blockieren dabei nichts.
        let before = inconsistent_shortcuts(edges);
        let mut with = edges.to_vec();
        with.push(new);
        if let Some((edge, title)) = inconsistent_shortcuts(&with)
            .into_iter()
            .find(|(e, _)| !before.contains_key(e))
        {
            return Err(Conflict::Derived(edge.kind, title));
        }
    }
    Ok(true)
}

/// Ergebnis der Beziehungsübernahme bei einer Personenzusammenführung.
pub struct MergePlan {
    /// Kanten, die auf die Zielperson umgehängt werden (kanonisch, doppelfrei).
    pub add: Vec<Edge>,
    /// Vorhandene Kanten, die dadurch überflüssig werden (Bekannte, wenn Freund).
    pub drop: Vec<Edge>,
}

/// Plant das Umhängen aller Beziehungen von `source` auf `target`. Selbstbezüge
/// und Dubletten entfallen; Widersprüche liefern `Err`.
pub fn plan_merge(edges: &[Edge], source: i64, target: i64) -> Result<MergePlan, Conflict> {
    let mut set: Vec<Edge> = edges
        .iter()
        .copied()
        .filter(|e| e.from != source && e.to != source)
        .collect();
    let mut touched: Vec<Edge> = edges
        .iter()
        .filter(|e| e.from == source || e.to == source)
        .filter_map(|e| {
            let map = |id: i64| if id == source { target } else { id };
            let (from, to) = (map(e.from), map(e.to));
            (from != to).then(|| {
                if e.kind.symmetric() {
                    Edge {
                        from: from.min(to),
                        kind: e.kind,
                        to: from.max(to),
                    }
                } else {
                    Edge {
                        from,
                        kind: e.kind,
                        to,
                    }
                }
            })
        })
        .collect();
    touched.sort();
    touched.dedup();
    let mut dropped = Vec::new();
    // Freund vor Bekannter: Der Freund ersetzt die schwächere Angabe.
    let mut add = Vec::new();
    for edge in touched.iter().filter(|e| e.kind == Kind::Friend) {
        let weaker = Edge {
            kind: Kind::Acquaintance,
            ..*edge
        };
        if let Some(index) = set.iter().position(|e| *e == weaker) {
            set.remove(index);
            dropped.push(weaker);
        }
    }
    for edge in touched {
        if edge.kind == Kind::Acquaintance
            && (set.contains(&Edge {
                kind: Kind::Friend,
                ..edge
            }) || add.contains(&Edge {
                kind: Kind::Friend,
                ..edge
            }))
        {
            continue;
        }
        let fresh = match validate_add(&set, edge) {
            Ok(fresh) => fresh,
            // Bereits erfasste Behelfsverbindungen gehen nie verloren (ADR 0061).
            Err(Conflict::Redundant(_)) => true,
            Err(c) => return Err(Conflict::Merge(Box::new(c))),
        };
        if fresh {
            set.push(edge);
            add.push(edge);
        }
    }
    Ok(MergePlan { add, drop: dropped })
}

// ------------------------------------------------------------- Sammlung (DB)

/// Ergänzt fehlende Katalogzeilen. Nur mit Schreibrecht aufrufen.
pub fn ensure_types(collection: &Collection) -> Result<()> {
    collection.writable()?;
    collection.db.execute_batch(&seed_sql())?;
    Ok(())
}

/// Speichert "`subject` ist `role` von `object`" einmal in kanonischer Form.
/// `Ok(true)`: gespeichert; `Ok(false)`: war schon vorhanden.
pub fn add(collection: &mut Collection, subject: i64, role: Role, object: i64) -> Result<bool> {
    collection.writable()?;
    for person in [subject, object] {
        let valid: bool = collection.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM persons WHERE id=? AND merged_into IS NULL)",
            [person],
            |r| r.get(0),
        )?;
        if !valid {
            bail!("Person nicht verfügbar");
        }
    }
    let tx = collection.db.transaction()?;
    tx.execute_batch(&seed_sql())?;
    let rows: Vec<(i64, i64, i64)> = tx
        .prepare("SELECT from_person,type_id,to_person FROM relations")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let edge = canonical(subject, role, object);
    let fresh = validate_add(&normalize(&rows), edge)?;
    if fresh {
        tx.execute(
            "INSERT OR IGNORE INTO relations VALUES(?,?,?)",
            params![edge.from, edge.kind.id(), edge.to],
        )?;
    }
    tx.commit()?;
    Ok(fresh)
}

/// Entfernt "`subject` ist `role` von `object`" (auch ältere, nicht kanonisch
/// gespeicherte Zeilen derselben Beziehung).
pub fn remove(collection: &Collection, subject: i64, role: Role, object: i64) -> Result<()> {
    collection.writable()?;
    let edge = canonical(subject, role, object);
    collection.db.execute(
        "DELETE FROM relations WHERE (from_person=?1 AND type_id=?2 AND to_person=?3)
            OR (?4 AND from_person=?3 AND type_id=?2 AND to_person=?1)
            OR (?5 AND from_person=?3 AND type_id=2 AND to_person=?1)",
        params![
            edge.from,
            edge.kind.id(),
            edge.to,
            edge.kind.symmetric(),
            edge.kind == Kind::Parent
        ],
    )?;
    if edge.kind == Kind::Parent {
        // Alte Umkehrzeile `kind_von`: from=Kind, to=Elternteil.
        collection.db.execute(
            "DELETE FROM relations WHERE from_person=?1 AND type_id=2 AND to_person=?2",
            params![edge.to, edge.from],
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn catalog_matches_kinds_and_keeps_legacy_ids() {
        for (i, name) in [
            "elternteil_von",
            "kind_von",
            "partner_von",
            "geschwister_von",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(CATALOG[i].id, i as i64 + 1);
            assert_eq!(CATALOG[i].name, *name);
        }
        for kind in Kind::ALL {
            let row = CATALOG.iter().find(|t| t.id == kind.id()).unwrap();
            assert_eq!(row.symmetric, kind.symmetric());
            assert_eq!(row.name, kind.name());
            let inverse = CATALOG.iter().find(|t| t.id == row.inverse_id).unwrap();
            assert_eq!(inverse.inverse_id, row.id, "Umkehrung der Umkehrung");
            if row.symmetric {
                assert_eq!(row.inverse_id, row.id);
            }
        }
        assert!(seed_sql().contains("(5,'cousin_von',5,1)"));
        assert!(seed_sql().contains("(8,'ehemaliger_partner_von',8,1)"));
    }

    #[test]
    fn labels_are_consistent_from_both_sides_for_every_gender() {
        let genders = [Gender::Female, Gender::Male, Gender::Unknown];
        for role in Role::ALL {
            assert_eq!(role.inverse().inverse(), role);
            assert_eq!(Role::from_key(role.key()), Some(role));
            for a in genders {
                assert!(!role.label(a).is_empty());
            }
            let edge = canonical(1, role, 2);
            // "1 ist <role> von 2", gelesen von beiden Seiten.
            assert_eq!(edge.role_of_other(2), Some((role, 1)));
            assert_eq!(edge.role_of_other(1), Some((role.inverse(), 2)));
            for side in [Direction::From, Direction::To] {
                let stored = stored_role(edge.kind.id(), side).unwrap();
                for a in genders {
                    assert_eq!(label(edge.kind.id(), side, a), Some(stored.label(a)));
                }
            }
            // Gleiche Speicherung aus beiden Eingaberichtungen.
            assert_eq!(canonical(1, role, 2), canonical(2, role.inverse(), 1));
        }
        assert_eq!(Role::Parent.label(Gender::Female), "Mutter");
        assert_eq!(Role::Child.label(Gender::Male), "Sohn");
        assert_eq!(Role::Cousin.label(Gender::Unknown), "Cousin/Cousine");
        assert_eq!(label(1, Direction::To, Gender::Female), Some("Tochter"));
        assert_eq!(label(2, Direction::From, Gender::Male), Some("Sohn"));
        assert_eq!(label(99, Direction::From, Gender::Male), None);
        assert_eq!(Gender::parse(" Weiblich "), Gender::Female);
        assert_eq!(Gender::parse("divers"), Gender::Unknown);
    }

    fn e(from: i64, kind: Kind, to: i64) -> Edge {
        canonical(
            from,
            match kind {
                Kind::Parent => Role::Parent,
                Kind::Partner => Role::Partner,
                Kind::FormerPartner => Role::FormerPartner,
                Kind::Sibling => Role::Sibling,
                Kind::Cousin => Role::Cousin,
                Kind::Friend => Role::Friend,
                Kind::Acquaintance => Role::Acquaintance,
            },
            to,
        )
    }

    #[test]
    fn legacy_rows_normalise() {
        let rows = [
            (2, 2, 1),
            (1, 1, 2),
            (9, 3, 4),
            (4, 3, 9),
            (5, 99, 6),
            (7, 3, 7),
        ];
        assert_eq!(
            normalize(&rows),
            vec![e(1, Kind::Parent, 2), e(4, Kind::Partner, 9)]
        );
    }

    #[test]
    fn validation_rules() {
        let base = [e(1, Kind::Parent, 2), e(2, Kind::Parent, 3)];
        assert_eq!(
            validate_add(&base, e(1, Kind::Friend, 1)),
            Err(Conflict::SelfRelation)
        );
        assert_eq!(validate_add(&base, e(1, Kind::Parent, 2)), Ok(false));
        assert_eq!(
            validate_add(&base, e(3, Kind::Parent, 1)),
            Err(Conflict::Cycle)
        );
        assert_eq!(
            validate_add(&base, e(2, Kind::Parent, 1)),
            Err(Conflict::Cycle)
        );
        assert!(matches!(
            validate_add(&base, e(1, Kind::Sibling, 2)),
            Err(Conflict::Pair(..))
        ));
        assert!(matches!(
            validate_add(&base, e(1, Kind::Partner, 3)),
            Err(Conflict::Lineage(_))
        ));
        assert_eq!(validate_add(&base, e(1, Kind::Friend, 3)), Ok(true));
        assert!(matches!(
            validate_add(&[e(1, Kind::Friend, 3)], e(3, Kind::Acquaintance, 1)),
            Err(Conflict::Pair(..))
        ));
        // Partner und Cousin dürfen zusammen vorkommen, Geschwister und Cousin nicht.
        let pc = [e(5, Kind::Partner, 6)];
        assert_eq!(validate_add(&pc, e(5, Kind::Cousin, 6)), Ok(true));
        assert!(validate_add(&[e(5, Kind::Sibling, 6)], e(5, Kind::Cousin, 6)).is_err());
        // Eine neue Eltern-Kante darf bestehende Gleichrangige nicht zu Vorfahr/Nachkomme machen.
        let sym = [e(1, Kind::Partner, 3)];
        assert!(matches!(
            validate_add(&[sym[0], e(2, Kind::Parent, 3)], e(1, Kind::Parent, 2)),
            Err(Conflict::Lineage(_))
        ));
        // Abgeleitete Widersprüche: gemeinsame Eltern machen Cousins unmöglich.
        let sib = [e(1, Kind::Parent, 10), e(1, Kind::Parent, 11)];
        assert!(matches!(
            validate_add(&sib, e(10, Kind::Cousin, 11)),
            Err(Conflict::Derived(..))
        ));
    }

    #[test]
    fn merge_plan_handles_symmetric_types_without_duplicates() {
        let g = [
            e(1, Kind::Partner, 5),
            e(2, Kind::Partner, 5),
            e(1, Kind::Friend, 2),
            e(1, Kind::Friend, 7),
            e(2, Kind::Acquaintance, 7),
            e(1, Kind::Cousin, 9),
        ];
        let plan = plan_merge(&g, 1, 2).unwrap();
        assert_eq!(plan.drop, vec![e(2, Kind::Acquaintance, 7)]);
        assert!(plan.add.contains(&e(2, Kind::Friend, 7)));
        assert!(plan.add.contains(&e(2, Kind::Cousin, 9)));
        // Partner 2–5 bestand schon: keine Dublette; Freundschaft 1–2 wird Selbstbezug.
        assert!(!plan.add.iter().any(|x| x.kind == Kind::Partner));
        assert!(plan.add.iter().all(|x| x.from != x.to));
        // Kanonische Reihenfolge bei der Umhängung auf eine kleinere ID.
        let plan = plan_merge(&[e(5, Kind::Friend, 9)], 9, 2).unwrap();
        assert_eq!(plan.add, vec![e(2, Kind::Friend, 5)]);
        assert!(plan.add[0].from < plan.add[0].to);
        // Widerspruch: Quelle ist Geschwister von X, Ziel Elternteil von X.
        let bad = [e(1, Kind::Sibling, 5), e(2, Kind::Parent, 5)];
        assert!(plan_merge(&bad, 1, 2).is_err());
    }
}
