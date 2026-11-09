use foto_importer::core::{
    collection::Collection,
    jobs::Control,
    metadata::{self, Search},
    relationship_graph::{self, NEIGHBOR_LIMIT},
};

#[test]
fn weights_match_joint_photo_search_and_include_recorded_zero_links() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    for name in ["a.jpg", "b.jpg"] {
        std::fs::write(root.path().join(name), b"photo")?;
    }
    let mut collection = Collection::open(root.path(), true)?;
    collection.quick_scan(false, &Control::default())?;
    let photos = metadata::search(&collection, &Search::default(), "")?.photos;
    assert_eq!(photos.len(), 2);
    let (first, second) = (photos[0].id, photos[1].id);
    collection.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2),(3),(4),(5);
         UPDATE persons SET merged_into=2 WHERE id=5;
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben');
         INSERT INTO relations VALUES(4,1,1);",
    )?;
    for (photo, person, assignment, retained) in [
        (first, 1, "user", 0),
        (first, 1, "user", 0),
        (first, 2, "user", 0),
        (first, 2, "user", 0),
        (first, 3, "suggested", 0),
        (first, 5, "user", 0),
        (second, 1, "user", 0),
        (second, 2, "user", 0),
        (second, 3, "user", 1),
        (999, 1, "user", 0),
        (999, 2, "user", 0),
    ] {
        collection.db.execute(
            "INSERT INTO faces(photo_id,source_relpath,detector_id,person_id,assignment,retained,bbox_x1,bbox_y1,bbox_x2,bbox_y2)
             VALUES(?,'a.jpg','det',?,?,?,0.1,0.1,0.5,0.5)",
            rusqlite::params![photo, person, assignment, retained],
        )?;
    }
    let graph = relationship_graph::for_person(&collection, 1)?;
    assert_eq!(graph.name, "Anna");
    assert_eq!(graph.connections.len(), 2);
    let edge = &graph.connections[0];
    assert_eq!((edge.person, edge.photos), (2, 2));
    let search = Search {
        people_all: vec![1, edge.person],
        ..Search::default()
    };
    assert_eq!(
        metadata::search(&collection, &search, "")?.total,
        edge.photos
    );
    let recorded = &graph.connections[1];
    assert_eq!((recorded.person, recorded.photos), (4, 0));
    assert_eq!(recorded.recorded.as_deref(), Some("kind von"));
    assert!(!graph.truncated);
    Ok(())
}

#[test]
fn only_the_bounded_neighborhood_is_returned() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let collection = Collection::open(root.path(), true)?;
    collection
        .db
        .execute("INSERT INTO persons(id) VALUES(1)", [])?;
    for person in 2..=NEIGHBOR_LIMIT + 3 {
        collection
            .db
            .execute("INSERT INTO persons(id) VALUES(?)", [person])?;
        collection
            .db
            .execute("INSERT INTO relations VALUES(1,3,?)", [person])?;
    }
    let graph = relationship_graph::for_person(&collection, 1)?;
    assert!(graph.truncated);
    assert_eq!(graph.connections.len(), NEIGHBOR_LIMIT);
    assert_eq!(graph.connections[0].person, 2);
    Ok(())
}

#[test]
fn network_contains_all_people_and_matches_the_photo_counts() -> anyhow::Result<()> {
    use foto_importer::core::relationship_graph::Recorded;
    let root = tempfile::tempdir()?;
    for name in ["a.jpg", "b.jpg"] {
        std::fs::write(root.path().join(name), b"photo")?;
    }
    let mut collection = Collection::open(root.path(), true)?;
    collection.quick_scan(false, &Control::default())?;
    let photos = metadata::search(&collection, &Search::default(), "")?.photos;
    let (first, second) = (photos[0].id, photos[1].id);
    collection.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2),(3),(4),(5);
         UPDATE persons SET merged_into=2 WHERE id=5;
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben');
         INSERT INTO relations VALUES(1,3,2),(2,2,1),(3,3,4);",
    )?;
    for (photo, person, assignment, retained) in [
        (first, 1, "user", 0),
        (first, 1, "user", 0),
        (first, 2, "user", 0),
        (first, 3, "suggested", 0),
        (first, 5, "user", 0),
        (second, 1, "user", 0),
        (second, 2, "user", 0),
        (second, 3, "user", 1),
    ] {
        collection.db.execute(
            "INSERT INTO faces(photo_id,source_relpath,detector_id,person_id,assignment,retained,bbox_x1,bbox_y1,bbox_x2,bbox_y2)
             VALUES(?,'a.jpg','det',?,?,?,0.1,0.1,0.5,0.5)",
            rusqlite::params![photo, person, assignment, retained],
        )?;
    }
    let network = relationship_graph::network(&collection)?;
    assert!(
        network
            .nodes
            .iter()
            .all(|n| n.photos == 0 || n.portrait.is_some()),
        "jede Person mit Fotos hat ein Porträt"
    );
    // Alle nicht zusammengeführten Personen sind Knoten, auch ohne Fotos.
    assert_eq!(network.nodes.len(), 4);
    assert_eq!(network.positions.len(), 4);
    assert_eq!(network.hidden_people, 0);
    let anna = network.index_of(1).expect("Anna");
    let ben = network.index_of(2).expect("Ben");
    assert!(network.index_of(5).is_none());
    assert_eq!(network.nodes[anna].photos, 2);
    // Gemeinsame Fotos stimmen mit der Sammlungssuche überein.
    let edge = network
        .edges
        .iter()
        .find(|e| (e.a, e.b) == (anna.min(ben), anna.max(ben)))
        .expect("Kante Anna-Ben");
    let search = Search {
        people_all: vec![1, 2],
        ..Search::default()
    };
    assert_eq!(
        metadata::search(&collection, &search, "")?.total,
        edge.photos
    );
    assert_eq!(edge.photos, 2);
    // Partner und Eltern-Kind liegen auf einer Kante; Richtung aus Sicht von `a`.
    assert_eq!(edge.recorded.len(), 2);
    let from_anna: Vec<_> = if edge.a == anna {
        edge.recorded.clone()
    } else {
        edge.recorded.iter().map(|r| r.reversed()).collect()
    };
    assert!(from_anna.contains(&Recorded::Parent) && from_anna.contains(&Recorded::Partner));
    // Eingetragene Beziehung ohne Foto bleibt als Kante erhalten.
    let (c3, c4) = (
        network.index_of(3).expect("3"),
        network.index_of(4).expect("4"),
    );
    let recorded_only = network
        .edges
        .iter()
        .find(|e| (e.a, e.b) == (c3.min(c4), c3.max(c4)))
        .expect("Kante 3-4");
    assert_eq!(recorded_only.photos, 0);
    let again = relationship_graph::network(&collection)?;
    assert_eq!(network.positions, again.positions);
    Ok(())
}

#[test]
fn siblings_follow_from_shared_parents_without_being_recorded() -> anyhow::Result<()> {
    use foto_importer::core::relationship_graph::Recorded;
    let root = tempfile::tempdir()?;
    let collection = Collection::open(root.path(), true)?;
    collection.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2),(3),(4),(5),(6),(8),(9),(11),(12),(13);
         -- 3 und 4: beide Eltern gemeinsam. 5 und 6: nur Elternteil 1 gemeinsam, beide haben
         -- einen weiteren Elternteil (Halbgeschwister). 11 und 12: gemeinsames Elternteil 13
         -- und zusätzlich direkt als Geschwister eingetragen.
         INSERT INTO relations VALUES(1,1,3),(2,1,3),(1,1,4),(2,1,4),
            (1,1,5),(8,1,5),(1,1,6),(9,1,6),(13,1,11),(13,1,12),(11,4,12);",
    )?;
    let network = relationship_graph::network(&collection)?;
    let kinds = |a: i64, b: i64| -> Vec<Recorded> {
        let (x, y) = (
            network.index_of(a).expect("Person a"),
            network.index_of(b).expect("Person b"),
        );
        let (low, high) = (x.min(y), x.max(y));
        network
            .edges
            .iter()
            .find(|e| e.a == low && e.b == high)
            .map(|e| e.recorded.clone())
            .unwrap_or_default()
    };
    assert!(kinds(3, 4).contains(&Recorded::DerivedSibling { half: false }));
    assert!(kinds(5, 6).contains(&Recorded::DerivedSibling { half: true }));
    // Eine eingetragene Geschwisterkante wird nicht zusätzlich abgeleitet.
    let recorded = kinds(11, 12);
    assert!(recorded.contains(&Recorded::Sibling), "{recorded:?}");
    assert!(
        !recorded
            .iter()
            .any(|k| matches!(k, Recorded::DerivedSibling { .. })),
        "{recorded:?}"
    );
    // Eltern und Kind sind keine Geschwister.
    assert!(
        !kinds(1, 3)
            .iter()
            .any(|k| matches!(k, Recorded::DerivedSibling { .. }))
    );
    // Die abgeleitete Linie zieht Geschwister im Layout näher zusammen als Unverbundene.
    let distance = |a: i64, b: i64| {
        let (p, q) = (
            network.positions[network.index_of(a).expect("Person a")],
            network.positions[network.index_of(b).expect("Person b")],
        );
        ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
    };
    assert!(distance(3, 4) < distance(3, 9));
    Ok(())
}
