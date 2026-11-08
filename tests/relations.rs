use foto_importer::core::{
    collection::Collection,
    faces, maintenance,
    relations::{self, Edge, Kind},
};
use rusqlite::params;

fn collection(people: i64) -> anyhow::Result<(tempfile::TempDir, Collection)> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    for id in 1..=people {
        c.db.execute("INSERT INTO persons(id) VALUES(?)", [id])?;
    }
    Ok((temp, c))
}

fn stored(c: &Collection) -> anyhow::Result<Vec<(i64, i64, i64)>> {
    Ok(c.db
        .prepare("SELECT from_person,type_id,to_person FROM relations ORDER BY 1,2,3")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

#[test]
fn new_collections_have_the_full_catalog() -> anyhow::Result<()> {
    let (_t, c) = collection(0)?;
    let names: Vec<String> =
        c.db.prepare("SELECT name FROM relation_types ORDER BY id")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
    assert_eq!(
        names,
        [
            "elternteil_von",
            "kind_von",
            "partner_von",
            "geschwister_von",
            "cousin_von",
            "freund_von",
            "bekannter_von"
        ]
    );
    Ok(())
}

#[test]
fn input_from_both_sides_stores_the_same_state() -> anyhow::Result<()> {
    for (role, inverse) in [
        ("parent", "child"),
        ("partner", "partner"),
        ("sibling", "sibling"),
        ("cousin", "cousin"),
        ("friend", "friend"),
        ("acquaintance", "acquaintance"),
    ] {
        let (_t1, mut a) = collection(2)?;
        let (_t2, mut b) = collection(2)?;
        faces::add_relation(&mut a, 1, role, 2)?;
        faces::add_relation(&mut b, 2, inverse, 1)?;
        assert_eq!(stored(&a)?, stored(&b)?, "{role}");
        assert_eq!(stored(&a)?.len(), 1);
        // Dublette in beliebiger Richtung ändert nichts.
        faces::add_relation(&mut a, 2, inverse, 1)?;
        faces::add_relation(&mut a, 1, role, 2)?;
        assert_eq!(stored(&a)?.len(), 1, "{role}");
        faces::remove_relation(&a, 2, inverse, 1)?;
        assert!(stored(&a)?.is_empty(), "{role}");
    }
    Ok(())
}

#[test]
fn contradictions_are_rejected_with_a_message() -> anyhow::Result<()> {
    let (_t, mut c) = collection(5)?;
    faces::add_relation(&mut c, 1, "parent", 2)?;
    faces::add_relation(&mut c, 2, "parent", 3)?;
    let err = |c: &mut Collection, a, k, b| match faces::add_relation(c, a, k, b) {
        Ok(()) => panic!("{a} {k} {b} wurde nicht abgelehnt"),
        Err(e) => e.to_string(),
    };
    assert!(err(&mut c, 1, "friend", 1).contains("sich selbst"));
    assert!(err(&mut c, 3, "parent", 1).contains("zyklus"));
    assert!(err(&mut c, 1, "child", 3).contains("zyklus"));
    assert!(err(&mut c, 1, "sibling", 2).contains("Eltern-Kind"));
    assert!(err(&mut c, 2, "partner", 1).contains("Eltern-Kind"));
    assert!(err(&mut c, 1, "partner", 3).contains("Abstammung"));
    assert!(err(&mut c, 1, "cousin", 3).contains("Abstammung"));
    assert!(err(&mut c, 1, "unknown", 3).contains("Unbekannter"));
    faces::add_relation(&mut c, 4, "friend", 5)?;
    assert!(err(&mut c, 5, "acquaintance", 4).contains("Freunde"));
    // Familie plus Freundschaft ist erlaubt, Partner plus Cousin ebenfalls.
    faces::add_relation(&mut c, 4, "partner", 5)?;
    faces::add_relation(&mut c, 4, "cousin", 5)?;
    assert!(err(&mut c, 4, "sibling", 5).contains("Partner"));
    // Nichts Unerwartetes wurde gespeichert.
    assert_eq!(stored(&c)?.len(), 5);
    Ok(())
}

#[test]
fn merged_or_missing_people_are_rejected() -> anyhow::Result<()> {
    let (_t, mut c) = collection(3)?;
    maintenance::merge_people(&mut c, 3, 2)?;
    assert!(faces::add_relation(&mut c, 1, "friend", 3).is_err());
    assert!(faces::add_relation(&mut c, 1, "friend", 9).is_err());
    Ok(())
}

#[test]
fn derived_relations_follow_the_rule_table() -> anyhow::Result<()> {
    let (_t, mut c) = collection(8)?;
    // 1 Großmutter; 2,3 ihre Kinder; 4 Kind von 2, 5 Kind von 3; 6 Halbgeschwister von 4.
    for (p, k) in [(1, 2), (1, 3), (2, 4), (3, 5), (2, 6)] {
        faces::add_relation(&mut c, p, "parent", k)?;
    }
    faces::add_relation(&mut c, 7, "parent", 4)?;
    faces::add_relation(&mut c, 8, "parent", 6)?;
    let r4 = maintenance::relations(&c, 4)?;
    assert_eq!(r4["grosseltern"], vec![1]);
    assert_eq!(r4["onkel_tanten"], vec![3]);
    assert_eq!(r4["cousins"], vec![5]);
    // 4 (Eltern 2, 7) und 6 (Eltern 2, 8) teilen genau einen Elternteil: Halbgeschwister.
    assert_eq!(r4["geschwister"], vec![6]);
    let r5 = maintenance::relations(&c, 5)?;
    assert_eq!(r5["cousins"], vec![4, 6]);
    assert_eq!(r5["onkel_tanten"], vec![2]);
    let edges = relations::load_edges(&c)?;
    let half: Vec<_> = relations::derive(&edges, 6)
        .into_iter()
        .filter(|d| d.role == relations::DerivedRole::HalfSibling)
        .map(|d| d.person)
        .collect();
    assert_eq!(half, vec![4]);
    // Abgeleitetes wird nicht gespeichert.
    assert_eq!(stored(&c)?.len(), 7);
    // Übernehmen als direkte Beziehung.
    relations::add(&mut c, 4, relations::Role::Cousin, 5)?;
    assert_eq!(stored(&c)?.len(), 8);
    assert!(
        !relations::derive(&relations::load_edges(&c)?, 4)
            .iter()
            .any(|d| d.role == relations::DerivedRole::Cousin)
    );
    Ok(())
}

#[test]
fn merge_keeps_symmetric_relations_canonical_and_unique() -> anyhow::Result<()> {
    let (_t, mut c) = collection(9)?;
    faces::add_relation(&mut c, 9, "partner", 5)?;
    faces::add_relation(&mut c, 2, "partner", 5)?;
    faces::add_relation(&mut c, 9, "friend", 2)?;
    faces::add_relation(&mut c, 9, "friend", 7)?;
    faces::add_relation(&mut c, 2, "acquaintance", 7)?;
    faces::add_relation(&mut c, 9, "cousin", 8)?;
    faces::add_relation(&mut c, 9, "parent", 6)?;
    maintenance::merge_people(&mut c, 9, 2)?;
    assert_eq!(
        stored(&c)?,
        vec![(2, 1, 6), (2, 3, 5), (2, 5, 8), (2, 6, 7)]
    );
    // Selbstbezug (Freundschaft 9-2) und Dubletten sind verschwunden.
    for (a, kind, b) in stored(&c)? {
        assert_ne!(a, b);
        if kind != 1 {
            assert!(a < b);
        }
    }
    // Widersprüchliche Zusammenführung wird abgelehnt und ändert nichts.
    let (_t2, mut d) = collection(3)?;
    faces::add_relation(&mut d, 1, "sibling", 3)?;
    faces::add_relation(&mut d, 2, "parent", 3)?;
    let before = stored(&d)?;
    assert!(maintenance::merge_people(&mut d, 1, 2).is_err());
    assert_eq!(stored(&d)?, before);
    Ok(())
}

#[test]
fn collections_without_the_new_type_rows_keep_working() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    {
        let mut c = Collection::open(temp.path(), true)?;
        for id in 1..=3 {
            c.db.execute("INSERT INTO persons(id) VALUES(?)", [id])?;
        }
        faces::add_relation(&mut c, 1, "parent", 2)?;
        faces::add_relation(&mut c, 2, "partner", 3)?;
        // Zustand einer älteren Sammlung: nur die Typen 1-4.
        c.db.execute("DELETE FROM relation_types WHERE id>4", [])?;
        // Legacy-Zeile mit Umkehrtyp (3 ist Kind von 1) und nicht kanonischem Partner.
        c.db.execute("INSERT INTO relations VALUES(3,2,1)", [])?;
        c.db.execute("INSERT INTO relations VALUES(3,3,2)", [])?;
    }
    {
        // Schreibgeschützt: lesbar und unverändert.
        let c = Collection::open(temp.path(), false)?;
        let n: i64 =
            c.db.query_row("SELECT count(*) FROM relation_types", [], |r| r.get(0))?;
        assert_eq!(n, 4);
        let edges = relations::load_edges(&c)?;
        assert_eq!(
            edges,
            vec![
                Edge {
                    from: 1,
                    kind: Kind::Parent,
                    to: 2
                },
                Edge {
                    from: 1,
                    kind: Kind::Parent,
                    to: 3
                },
                Edge {
                    from: 2,
                    kind: Kind::Partner,
                    to: 3
                },
            ]
        );
        assert_eq!(maintenance::relations(&c, 2)?["partner"], vec![3]);
    }
    {
        // Mit Schreibrecht kommen die Typzeilen dazu (INSERT OR IGNORE).
        let mut c = Collection::open(temp.path(), true)?;
        let n: i64 =
            c.db.query_row("SELECT count(*) FROM relation_types", [], |r| r.get(0))?;
        assert_eq!(n, 7);
        faces::add_relation(&mut c, 1, "friend", 3)?;
        let known: String =
            c.db.query_row("SELECT name FROM relation_types WHERE id=6", [], |r| {
                r.get(0)
            })?;
        assert_eq!(known, "freund_von");
        c.db.execute(
            "DELETE FROM relations WHERE type_id=?",
            params![Kind::Friend.id()],
        )?;
    }
    Ok(())
}
