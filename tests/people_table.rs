//! Zählungen der Personentabelle aus der Sammlung (ADR 0063).
use foto_importer::core::{
    collection::Collection,
    faces::{self, NewPerson},
    people_table::{self, Column, SortKey},
    relations::{self, Role},
};
use rusqlite::params;

fn add_face(
    c: &Collection,
    photo: i64,
    person: Option<i64>,
    assignment: Option<&str>,
    ignored: i64,
) -> anyhow::Result<()> {
    c.db.execute(
        "INSERT INTO faces(photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,person_id,assignment,ignored,retained)
         VALUES(?,?,'det',0.1,0.1,0.9,0.9,?,?,?,0)",
        params![photo, format!("{photo}.jpg"), person, assignment, ignored],
    )?;
    Ok(())
}

#[test]
fn counts_photos_faces_dates_hints_and_relations_for_every_person() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    let named = |name: &str, year| NewPerson {
        name: name.into(),
        birth_year: year,
        ..Default::default()
    };
    let anna = faces::add_person(&mut c, &named("Anna", Some(1980)))?;
    let ben = faces::add_person(&mut c, &named("Ben", None))?;
    let ohne = faces::add_person(&mut c, &named("Ohne Foto", None))?;
    for (photo, date) in [
        (1, "2024:05:01 10:00:00"),
        (2, "2021:12:24 18:30:00"),
        (3, "2026-02-03T08:00:00+01:00"),
    ] {
        c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [photo])?;
        c.db.execute(
            "INSERT INTO metadata_photos(file_id,photo_id,relpath,name,ext,dt_original) VALUES(?,?,?,?,'jpg',?)",
            params![photo, photo, format!("{photo}.jpg"), format!("{photo}.jpg"), date],
        )?;
    }
    // Anna: zwei Gesichter auf einem Foto, eins auf einem zweiten, dazu Vorschlag und Ignoriertes.
    add_face(&c, 1, Some(anna), Some("user"), 0)?;
    add_face(&c, 1, Some(anna), Some("user"), 0)?;
    add_face(&c, 2, Some(anna), Some("user"), 0)?;
    add_face(&c, 3, Some(anna), Some("suggested"), 0)?;
    add_face(&c, 3, Some(anna), Some("user"), 1)?;
    add_face(&c, 3, Some(ben), Some("user"), 0)?;
    add_face(&c, 3, None, None, 0)?;
    relations::add(&mut c, anna, Role::Parent, ben)?;

    let stats = people_table::load_stats(&c)?;
    let a = &stats[&anna];
    assert_eq!(
        (a.faces, a.photos),
        (3, 2),
        "nur bestätigte, nicht ignorierte"
    );
    assert_eq!(a.first.as_deref(), Some("2021-12-24"));
    assert_eq!(a.last.as_deref(), Some("2024-05-01"));
    let b = &stats[&ben];
    assert_eq!((b.faces, b.photos), (1, 1));
    assert_eq!(
        b.first.as_deref(),
        Some("2026-02-03"),
        "ISO-Datum wird vereinheitlicht"
    );
    assert!(
        !stats.contains_key(&ohne),
        "Person ohne Gesichter hat keine Zeile"
    );

    let people = faces::people(&c)?;
    let edges = relations::load_edges(&c)?;
    let family = relations::load_family(&c)?;
    let rows = people_table::build_rows(&people, &stats, &edges, &family, &[], 2026);
    let row = |id| rows.iter().find(|r| r.id == id).expect("Zeile");
    assert_eq!(row(anna).age, Some(46));
    assert_eq!(row(anna).relations, 1);
    assert_eq!(row(anna).family, "1 Kind");
    assert_eq!(row(ben).family, "1 Elternteil");
    assert_eq!(row(ohne).photos, 0);
    assert_eq!(row(ohne).first_photo, None);

    // Sortierung nach Fotos absteigend: Anna, Ben, dann die Person ohne Foto; leere Daten zuletzt.
    let order = people_table::sort_indices(
        &rows,
        &[SortKey {
            column: Column::LastPhoto,
            descending: true,
        }],
    );
    let ids: Vec<i64> = order.iter().map(|&i| rows[i].id).collect();
    assert_eq!(ids, [ben, anna, ohne]);
    Ok(())
}
