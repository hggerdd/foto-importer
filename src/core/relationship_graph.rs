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
