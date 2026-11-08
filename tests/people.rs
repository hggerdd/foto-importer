//! Personen ohne Gesichtsbezug manuell anlegen (ADR 0061).
use foto_importer::core::{
    collection::Collection,
    faces::{self, MAX_NAME_CHARS, MAX_NOTE_CHARS, NewPerson},
    relations::{self, Gender, Role},
    relationship_graph,
};

fn collection() -> anyhow::Result<(tempfile::TempDir, Collection)> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    Ok((temp, c))
}

fn person(name: &str) -> NewPerson {
    NewPerson {
        name: name.into(),
        ..Default::default()
    }
}

#[test]
fn a_new_person_appears_everywhere_without_faces() -> anyhow::Result<()> {
    let (_t, mut c) = collection()?;
    let id = faces::add_person(
        &mut c,
        &NewPerson {
            name: "  Oma   Berta ".into(),
            gender: "weiblich".into(),
            birth_year: Some(1921),
            note: "verstorben 1999".into(),
        },
    )?;
    let people = faces::people(&c)?;
    let berta = people.iter().find(|p| p.id == id).expect("in people()");
    assert_eq!(berta.name, "Oma Berta", "Leerraum bereinigt");
    assert_eq!(berta.gender, "weiblich");
    assert_eq!(berta.birth, "1921");
    assert_eq!(berta.note, "verstorben 1999");
    assert_eq!(berta.count, 0, "keine Gesichter");
    // Im Personengraph erscheint sie als Knoten ohne Fotos.
    let network = relationship_graph::network(&c)?;
    let node = network.index_of(id).expect("Knoten");
    assert_eq!(network.nodes[node].photos, 0);
    // Sofort in Beziehungen, Stammbaum und Geschlechtsformen nutzbar.
    let enkel = faces::add_person(&mut c, &person("Leo"))?;
    relations::add(&mut c, id, Role::Parent, enkel)?;
    let family = relations::load_family(&c)?;
    assert_eq!(
        family
            .relationship(id, enkel)
            .map(|l| l.relation.label(Gender::parse(&berta.gender))),
        Some("Mutter".to_string())
    );
    // Keine Auswirkung auf Gesichtszuordnungen.
    let faces: i64 =
        c.db.query_row("SELECT count(*) FROM faces", [], |r| r.get(0))?;
    assert_eq!(faces, 0);
    Ok(())
}

#[test]
fn input_is_validated() -> anyhow::Result<()> {
    let (_t, mut c) = collection()?;
    for (bad, why) in [
        (person(""), "leer"),
        (person("   \t "), "nur Leerraum"),
        (person(&"x".repeat(MAX_NAME_CHARS + 1)), "zu langer Name"),
        (
            NewPerson {
                note: "n".repeat(MAX_NOTE_CHARS + 1),
                ..person("Anna")
            },
            "zu lange Notiz",
        ),
        (
            NewPerson {
                gender: "irgendwas".into(),
                ..person("Anna")
            },
            "Geschlecht außerhalb der Auswahl",
        ),
        (
            NewPerson {
                birth_year: Some(12),
                ..person("Anna")
            },
            "Geburtsjahr",
        ),
    ] {
        assert!(faces::add_person(&mut c, &bad).is_err(), "{why}");
    }
    let n: i64 =
        c.db.query_row("SELECT count(*) FROM persons", [], |r| r.get(0))?;
    assert_eq!(n, 0, "nichts wurde angelegt");
    // Grenzwerte sind erlaubt.
    faces::add_person(&mut c, &person(&"x".repeat(MAX_NAME_CHARS)))?;
    Ok(())
}

#[test]
fn duplicate_names_are_allowed_but_reported() -> anyhow::Result<()> {
    let (_t, mut c) = collection()?;
    let first = faces::add_person(&mut c, &person("Anna Müller"))?;
    let people = faces::people(&c)?;
    assert_eq!(faces::people_named(&people, "anna  müller "), vec![first]);
    assert!(faces::people_named(&people, "Ben").is_empty());
    let second = faces::add_person(&mut c, &person("Anna Müller"))?;
    assert_ne!(first, second, "IDs bleiben unterscheidbar");
    let people = faces::people(&c)?;
    assert_eq!(faces::people_named(&people, "Anna Müller").len(), 2);
    Ok(())
}

#[test]
fn read_only_collections_refuse() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    drop(Collection::open(temp.path(), true)?);
    let mut c = Collection::open(temp.path(), false)?;
    assert!(faces::add_person(&mut c, &person("Anna")).is_err());
    Ok(())
}
