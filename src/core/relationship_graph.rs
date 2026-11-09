use super::{collection::Collection, metadata::PHOTO_FORMATS};
use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

pub const NEIGHBOR_LIMIT: usize = 12;

pub struct Connection {
    pub person: i64,
    pub name: String,
    pub photos: usize,
    pub recorded: Option<String>,
}

pub struct Graph {
    pub focus: i64,
    pub name: String,
    pub connections: Vec<Connection>,
    pub truncated: bool,
}

/// Read a person's immediate relationships using the same photo IDs as collection search.
pub fn for_person(collection: &Collection, focus: i64) -> Result<Graph> {
    let name = collection
        .db
        .query_row(
            "SELECT coalesce(n.value,'Unbenannt') FROM persons x
             LEFT JOIN person_attrs n ON n.person_id=x.id AND n.key='name'
             WHERE x.id=? AND x.merged_into IS NULL",
            [focus],
            |row| row.get(0),
        )
        .optional()?
        .context("Diese Person ist in der Sammlung nicht mehr vorhanden")?;
    let query = format!(
        "WITH focus_photos AS (
            SELECT DISTINCT f.photo_id
            FROM faces f INDEXED BY ix_faces_person
            JOIN metadata_photos p ON p.photo_id=f.photo_id
            WHERE f.person_id=?1 AND f.retained=0 AND f.assignment='user' AND {PHOTO_FORMATS}
        ), shared AS (
            SELECT f.person_id AS id, count(DISTINCT f.photo_id) AS photos
            FROM focus_photos p CROSS JOIN faces f INDEXED BY ix_faces_photo
            WHERE f.photo_id=p.photo_id AND f.retained=0 AND f.assignment='user'
              AND f.person_id IS NOT NULL AND f.person_id<>?1
            GROUP BY f.person_id
        ), direct AS (
            SELECT r.to_person AS id, t.name AS kind
            FROM relations r JOIN relation_types t ON t.id=r.type_id WHERE r.from_person=?1
            UNION
            SELECT r.from_person AS id, inverse.name AS kind
            FROM relations r JOIN relation_types t ON t.id=r.type_id
            JOIN relation_types inverse ON inverse.id=t.inverse_id WHERE r.to_person=?1
        ), recorded AS (
            SELECT id, group_concat(replace(kind,'_',' '), ', ') AS kinds FROM direct GROUP BY id
        ), neighbors AS (
            SELECT id FROM shared UNION SELECT id FROM recorded
        )
        SELECT x.id, coalesce(n.value,'Unbenannt'), coalesce(s.photos,0) AS count, r.kinds
        FROM neighbors k JOIN persons x ON x.id=k.id
        LEFT JOIN shared s ON s.id=k.id
        LEFT JOIN recorded r ON r.id=k.id
        LEFT JOIN person_attrs n ON n.person_id=k.id AND n.key='name'
        WHERE x.merged_into IS NULL AND x.id<>?1
        ORDER BY count DESC, casefold(n.value), x.id LIMIT ?2"
    );
    let mut connections = collection
        .db
        .prepare(&query)?
        .query_map(params![focus, NEIGHBOR_LIMIT + 1], |row| {
            Ok(Connection {
                person: row.get(0)?,
                name: row.get(1)?,
                photos: row.get(2)?,
                recorded: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let truncated = connections.len() > NEIGHBOR_LIMIT;
    connections.truncate(NEIGHBOR_LIMIT);
    Ok(Graph {
        focus,
        name,
        connections,
        truncated,
    })
}

/// Höchstzahl gleichzeitig dargestellter Personen (das Layout ist O(n²)).
pub const MAX_NODES: usize = 600;
/// Höchstzahl gezeichneter Kanten; eingetragene Beziehungen haben Vorrang.
pub const MAX_EDGES: usize = 6000;

/// Eingetragene Beziehung aus Sicht der ersten Person einer Kante.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Recorded {
    /// Die erste Person ist Elternteil der zweiten.
    Parent,
    /// Die erste Person ist Kind der zweiten.
    Child,
    Partner,
    /// Direkt eingetragene Geschwisterverbindung (Behelf).
    Sibling,
    /// Aus gemeinsamen Eltern im Stammbaum abgeleitet, nicht gespeichert; `half`: nur
    /// ein gemeinsamer Elternteil.
    DerivedSibling {
        half: bool,
    },
    /// Weitere Typen aus `relation_types` (Name, Name der Umkehrung), neutral dargestellt.
    Other(String, String),
}

impl Recorded {
    fn from_type(name: &str, inverse: &str) -> Self {
        match name {
            "elternteil_von" => Self::Parent,
            "kind_von" => Self::Child,
            "partner_von" => Self::Partner,
            "geschwister_von" => Self::Sibling,
            _ => Self::Other(name.to_owned(), inverse.to_owned()),
        }
    }
    pub fn reversed(&self) -> Self {
        match self {
            Self::Parent => Self::Child,
            Self::Child => Self::Parent,
            Self::Other(name, inverse) => Self::Other(inverse.clone(), name.clone()),
            other => other.clone(),
        }
    }
}

/// Ausschnitt eines Gesichts für die Darstellung im Knoten.
#[derive(Clone, Debug, PartialEq)]
pub struct Portrait {
    /// Relativer Pfad des Fotos in der Sammlung.
    pub source: String,
    /// Normierter Rahmen `[x1, y1, x2, y2]`.
    pub bbox: [f32; 4],
}

pub struct Node {
    pub person: i64,
    pub name: String,
    /// Typischstes Gesicht der Person, falls vorhanden.
    pub portrait: Option<Portrait>,
    /// Eindeutige Fotos mit bestätigter Zuordnung (wie in der Sammlungssuche).
    pub photos: usize,
}

pub struct Edge {
    /// Indizes in `Network::nodes`, `a < b`.
    pub a: usize,
    pub b: usize,
    /// Gemeinsame Fotos, 0 bei rein eingetragener Beziehung.
    pub photos: usize,
    /// Eingetragene Beziehungen aus Sicht von `a`.
    pub recorded: Vec<Recorded>,
}

pub struct Network {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Weltkoordinaten je Knoten, Einheit ist der Sollabstand verbundener Knoten.
    pub positions: Vec<[f32; 2]>,
    /// Personen, die wegen `MAX_NODES` nicht dargestellt werden (wenigste Fotos).
    pub hidden_people: usize,
    pub hidden_edges: usize,
}

impl Network {
    pub fn index_of(&self, person: i64) -> Option<usize> {
        self.nodes.iter().position(|n| n.person == person)
    }
}

/// Bestätigte, eindeutige Paare aus Foto und Person wie in `for_person`.
fn photo_people() -> String {
    format!(
        "photo_people AS MATERIALIZED (
            SELECT DISTINCT f.photo_id AS photo, f.person_id AS person
            FROM faces f JOIN metadata_photos p ON p.photo_id=f.photo_id
            JOIN persons x ON x.id=f.person_id
            WHERE f.retained=0 AND f.assignment='user' AND x.merged_into IS NULL AND {PHOTO_FORMATS}
        )"
    )
}

/// Alle Personen mit gemeinsamen Fotos und eingetragenen Beziehungen samt Layout.
/// Zählregeln wie in `for_person`: eindeutige Fotos, bestätigte Zuordnungen,
/// keine historischen Gesichter, keine zusammengeführten Personen.
/// Höchstzahl geprüfter Geschwisterpaare (Schutz bei sehr großen Kinderlisten).
const MAX_SIBLING_PAIRS: usize = 20_000;

/// Geschwisterpaare `(kleinere ID, größere ID, Halbgeschwister)` aus gemeinsamen Eltern
/// des Stammbaums. Fehler beim Lesen des Stammbaums verhindern den Graphen nicht.
fn derived_siblings(collection: &Collection, people: &[(i64, String)]) -> Vec<(i64, i64, bool)> {
    use super::kinship::Relation;
    let Ok(family) = super::relations::load_family(collection) else {
        return Vec::new();
    };
    let mut candidates: std::collections::BTreeSet<(i64, i64)> = Default::default();
    'parents: for (parent, _) in people {
        let children: Vec<i64> = family.children_of(*parent).iter().copied().collect();
        for (i, a) in children.iter().enumerate() {
            for b in &children[i + 1..] {
                candidates.insert((*a.min(b), *a.max(b)));
                if candidates.len() >= MAX_SIBLING_PAIRS {
                    break 'parents;
                }
            }
        }
    }
    candidates
        .into_iter()
        .filter_map(|(a, b)| match family.relationship(a, b)?.relation {
            Relation::Blood {
                a_gen: 1,
                b_gen: 1,
                half,
            } => Some((a, b, half)),
            _ => None,
        })
        .collect()
}

pub fn network(collection: &Collection) -> Result<Network> {
    use std::collections::{BTreeMap, BTreeSet, HashMap};
    let people: Vec<(i64, String)> = collection
        .db
        .prepare(
            "SELECT x.id, coalesce(n.value,'Unbenannt') FROM persons x
             LEFT JOIN person_attrs n ON n.person_id=x.id AND n.key='name'
             WHERE x.merged_into IS NULL",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let counts: HashMap<i64, usize> = collection
        .db
        .prepare(&format!(
            "WITH {} SELECT person, count(*) FROM photo_people GROUP BY person",
            photo_people()
        ))?
        .query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as usize)))?
        .collect::<rusqlite::Result<_>>()?;
    let shared: Vec<(i64, i64, usize)> = collection
        .db
        .prepare(&format!(
            "WITH {} SELECT a.person, b.person, count(*) FROM photo_people a
             JOIN photo_people b ON a.photo=b.photo AND a.person<b.person
             GROUP BY a.person, b.person",
            photo_people()
        ))?
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as usize))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let relations: Vec<(i64, String, String, i64)> = collection
        .db
        .prepare(
            "SELECT r.from_person, t.name, coalesce(i.name,t.name), r.to_person FROM relations r
             JOIN relation_types t ON t.id=r.type_id
             LEFT JOIN relation_types i ON i.id=t.inverse_id",
        )?
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    // Kanten je Personenpaar (kleinere ID zuerst) zusammenführen.
    let mut pairs: BTreeMap<(i64, i64), (usize, BTreeSet<Recorded>)> = BTreeMap::new();
    for (a, b, photos) in shared {
        pairs.entry((a, b)).or_default().0 = photos;
    }
    for (from, kind, inverse, to) in relations {
        let kind = Recorded::from_type(&kind, &inverse);
        if from == to {
            continue;
        }
        let (key, kind) = if from < to {
            ((from, to), kind)
        } else {
            ((to, from), kind.reversed())
        };
        pairs.entry(key).or_default().1.insert(kind);
    }

    // Geschwister ergeben sich aus gemeinsamen Eltern und müssen nicht eingetragen sein.
    for (a, b, half) in derived_siblings(collection, &people) {
        let set = &mut pairs.entry((a, b)).or_default().1;
        if !set.contains(&Recorded::Sibling) {
            set.insert(Recorded::DerivedSibling { half });
        }
    }

    // Wichtigste Personen zuerst: Fotos, Beziehungen, Name, ID. Das ist auch die
    // Startreihenfolge des Layouts und damit Teil der Determinismus.
    let mut degree: HashMap<i64, usize> = HashMap::new();
    for (a, b) in pairs.keys() {
        *degree.entry(*a).or_default() += 1;
        *degree.entry(*b).or_default() += 1;
    }
    // Fehlende Gesichter dürfen den Graphen nicht verhindern.
    let embedder = super::faces::setting(collection, "rust.embedder_key")
        .ok()
        .flatten()
        .unwrap_or_default();
    let ids: Vec<i64> = people.iter().map(|(id, _)| *id).collect();
    let mut portraits =
        super::faces::person_portraits(collection, &embedder, &ids).unwrap_or_default();
    let mut nodes: Vec<Node> = people
        .into_iter()
        .map(|(person, name)| Node {
            photos: counts.get(&person).copied().unwrap_or(0),
            portrait: portraits.remove(&person).map(|face| Portrait {
                source: face.source,
                bbox: face.bbox,
            }),
            person,
            name,
        })
        .collect();
    nodes.sort_by_cached_key(|n| {
        (
            std::cmp::Reverse(n.photos),
            std::cmp::Reverse(degree.get(&n.person).copied().unwrap_or(0)),
            super::files::fold(&n.name),
            n.person,
        )
    });
    let hidden_people = nodes.len().saturating_sub(MAX_NODES);
    nodes.truncate(MAX_NODES);
    let index: HashMap<i64, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.person, i))
        .collect();
    let mut edges: Vec<Edge> = pairs
        .into_iter()
        .filter_map(|((a, b), (photos, recorded))| {
            let (a, b) = (*index.get(&a)?, *index.get(&b)?);
            let (a, b, recorded) = if a < b {
                (a, b, recorded.into_iter().collect::<Vec<_>>())
            } else {
                (b, a, recorded.iter().map(Recorded::reversed).collect())
            };
            Some(Edge {
                a,
                b,
                photos,
                recorded,
            })
        })
        .collect();
    edges.sort_by_key(|e| (e.recorded.is_empty(), std::cmp::Reverse(e.photos), e.a, e.b));
    let hidden_edges = edges.len().saturating_sub(MAX_EDGES);
    edges.truncate(MAX_EDGES);

    let layout_edges: Vec<_> = edges
        .iter()
        .map(|e| super::graph_layout::LayoutEdge {
            a: e.a,
            b: e.b,
            weight: if e.photos > 0 {
                1.0 + (e.photos as f32).ln()
            } else {
                0.0
            } + if e.recorded.is_empty() { 0.0 } else { 1.5 },
        })
        .collect();
    let positions = super::graph_layout::layout(nodes.len(), &layout_edges);
    Ok(Network {
        nodes,
        edges,
        positions,
        hidden_people,
        hidden_edges,
    })
}
