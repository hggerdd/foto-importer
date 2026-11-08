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
            "INSERT INTO faces(photo_id,source_relpath,detector_id,person_id,assignment,retained)
             VALUES(?,'a.jpg','det',?,?,?)",
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
            "INSERT INTO faces(photo_id,source_relpath,detector_id,person_id,assignment,retained)
             VALUES(?,'a.jpg','det',?,?,?)",
            rusqlite::params![photo, person, assignment, retained],
        )?;
    }
    let network = relationship_graph::network(&collection)?;
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
