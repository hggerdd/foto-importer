//! Gemeinsame Bausteine der Integrationstests: temporäre Sammlungen und Zeilen, die viele
//! Tests gleich anlegen. Jede Testdatei bindet das Modul mit `mod common;` ein und nutzt nur
//! einen Teil davon (deshalb `dead_code` erlaubt).
#![allow(dead_code)]

use foto_importer::core::{
    collection::Collection,
    faces::NewPerson,
    relations::{Edge, Role, canonical},
};
use rusqlite::params;

/// Leere, schreibend geöffnete Sammlung in einem temporären Ordner.
pub fn collection() -> anyhow::Result<(tempfile::TempDir, Collection)> {
    collection_with_people(0)
}

/// Sammlung mit den Personen `1..=people` (nur IDs, ohne Namen).
pub fn collection_with_people(people: i64) -> anyhow::Result<(tempfile::TempDir, Collection)> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    for id in 1..=people {
        c.db.execute("INSERT INTO persons(id) VALUES(?)", [id])?;
    }
    Ok((temp, c))
}

/// Neue Person nur mit Namen.
pub fn person(name: &str) -> NewPerson {
    NewPerson {
        name: name.into(),
        ..Default::default()
    }
}

/// „`a` ist Elternteil von `b`“.
pub fn parent(a: i64, b: i64) -> Edge {
    canonical(a, Role::Parent, b)
}

/// „`a` ist Partner von `b`“.
pub fn partner(a: i64, b: i64) -> Edge {
    canonical(a, Role::Partner, b)
}

/// Unbenannte Erkennung `id` auf Foto `photo` (Quelle `<id>.jpg`, voller Rahmen, Qualität 1).
pub fn face(c: &Collection, id: i64, photo: i64) -> anyhow::Result<()> {
    c.db.execute(
        "INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality)
         VALUES(?,?,?,'det',0,0,1,1,1)",
        params![id, photo, format!("{id}.jpg")],
    )?;
    Ok(())
}

/// Erkennung auf Foto `photo` (Quelle `a.jpg`), der Person `person` mit `assignment`
/// zugeordnet; `retained` markiert eine aufbewahrte Erkennung.
pub fn assigned_face(
    c: &Collection,
    photo: i64,
    person: i64,
    assignment: &str,
    retained: i64,
) -> anyhow::Result<()> {
    c.db.execute(
        "INSERT INTO faces(photo_id,source_relpath,detector_id,person_id,assignment,retained,bbox_x1,bbox_y1,bbox_x2,bbox_y2)
         VALUES(?,'a.jpg','det',?,?,?,0.1,0.1,0.5,0.5)",
        params![photo, person, assignment, retained],
    )?;
    Ok(())
}
