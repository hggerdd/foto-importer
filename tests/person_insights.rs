use foto_importer::core::{
    collection::Collection,
    jobs::Control,
    metadata::{self, Search},
    person_insights,
};

/// Drei Fotos; Anna (1) ist auf allen, Ben (2) auf zweien, Carla (3) auf einem.
fn sample() -> anyhow::Result<(tempfile::TempDir, Collection, Vec<i64>)> {
    let root = tempfile::tempdir()?;
    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        std::fs::write(root.path().join(name), b"photo")?;
    }
    let mut collection = Collection::open(root.path(), true)?;
    collection.quick_scan(false, &Control::default())?;
    let photos: Vec<i64> = metadata::search(&collection, &Search::default(), "")?
        .photos
        .iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(photos.len(), 3);
    collection.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2),(3),(4),(5);
         INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben'),(3,'name','Carla'),
             (4,'name','Dora'),(5,'name','Emil');
         INSERT INTO relations VALUES(4,1,1);",
    )?;
    let faces = [
        (photos[0], 1, "user", 0),
        (photos[1], 1, "user", 0),
        (photos[2], 1, "user", 0),
        (photos[0], 2, "user", 0),
        (photos[1], 2, "user", 0),
        (photos[1], 2, "user", 0),
        (photos[2], 3, "user", 0),
        // Vorschläge und historische Gesichter zählen nicht.
        (photos[0], 5, "suggested", 0),
        (photos[1], 3, "user", 1),
    ];
    for (photo, person, assignment, retained) in faces {
        collection.db.execute(
            "INSERT INTO faces(photo_id,source_relpath,detector_id,person_id,assignment,retained,bbox_x1,bbox_y1,bbox_x2,bbox_y2)
             VALUES(?,'a.jpg','det',?,?,?,0.1,0.1,0.5,0.5)",
            rusqlite::params![photo, person, assignment, retained],
        )?;
    }
    Ok((root, collection, photos))
}

#[test]
fn companions_count_like_the_joint_photo_search() -> anyhow::Result<()> {
    let (_root, collection, _) = sample()?;
    let insights = person_insights::load(&collection, 1)?;
    let found: Vec<(i64, &str, usize)> = insights
        .companions
        .iter()
        .map(|c| (c.person, c.name.as_str(), c.photos))
        .collect();
    // Dora ist nur eingetragen (keine Fotos), Emil nur vorgeschlagen: beide fehlen.
    assert_eq!(found, [(2, "Ben", 2), (3, "Carla", 1)]);
    for companion in &insights.companions {
        let search = Search {
            people_all: vec![1, companion.person],
            ..Search::default()
        };
        assert_eq!(
            metadata::search(&collection, &search, "")?.total,
            companion.photos,
            "Klick auf die Zahl findet genau diese Fotos"
        );
    }
    Ok(())
}
