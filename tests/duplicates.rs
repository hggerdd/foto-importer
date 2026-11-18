use foto_importer::core::{collection::Collection, duplicates, faces};

/// Einheitsvektor entlang `axis` (Dimension 8), leicht gestört je Gesicht.
fn vector(axis: usize, wobble: f32) -> Vec<f32> {
    let mut v = [0.0_f32; 8];
    v[axis] = 1.0;
    v[(axis + 4) % 8] = wobble;
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter().map(|x| x / norm).collect()
}

/// Personen je Achse: 1/2 dieselbe Person; 3/4 ähnlich, aber gemeinsam auf einem Foto;
/// 5/6 ähnlich, aber als Geschwister eingetragen; 7/8 ähnlich; 9 ohne Partner; 10 in 1
/// zusammengeführt (keine Gesichter mehr, darf nie erscheinen).
fn sample() -> anyhow::Result<(tempfile::TempDir, Collection)> {
    let root = tempfile::tempdir()?;
    let collection = Collection::open(root.path(), true)?;
    collection.db.execute_batch(
        "INSERT INTO persons(id) VALUES(1),(2),(3),(4),(5),(6),(7),(8),(9),(10);
         UPDATE persons SET merged_into=1 WHERE id=10;
         INSERT INTO relations VALUES(5,4,6);
         INSERT INTO face_settings VALUES('rust.embedder_key','\"emb\"');",
    )?;
    let people: [(i64, usize); 9] = [
        (1, 0),
        (2, 0),
        (3, 1),
        (4, 1),
        (5, 2),
        (6, 2),
        (7, 3),
        (8, 3),
        (9, 5),
    ];
    let mut face = 0_i64;
    for (person, axis) in people {
        for wobble in [0.0, 0.1, 0.2] {
            face += 1;
            // Eigenes Foto je Gesicht; 3 und 4 teilen sich zusätzlich Foto 1000.
            let photo = if matches!(person, 3 | 4) && wobble == 0.0 {
                1000
            } else {
                face
            };
            collection.db.execute(
                "INSERT INTO faces(id,photo_id,source_relpath,detector_id,quality,person_id,assignment)
                 VALUES(?,?,'a.jpg','det',1,?,'user')",
                rusqlite::params![face, photo, person],
            )?;
            collection.db.execute(
                "INSERT INTO face_embeddings VALUES(?,'emb',8,?)",
                rusqlite::params![face, faces::encode(&vector(axis, wobble))],
            )?;
        }
    }
    Ok((root, collection))
}

#[test]
fn similar_people_are_asked_once_unless_together_related_or_declined() -> anyhow::Result<()> {
    let (_root, mut collection) = sample()?;
    let pairs = |c: &Collection| -> anyhow::Result<Vec<(i64, i64)>> {
        Ok(duplicates::load(c)?.iter().map(|d| (d.a, d.b)).collect())
    };
    assert_eq!(pairs(&collection)?, [(1, 2), (7, 8)]);
    duplicates::decline(&mut collection, 8, 7)?;
    assert_eq!(pairs(&collection)?, [(1, 2)], "Verschieden gilt für immer");
    assert_eq!(
        duplicates::load_declined(&collection)?
            .into_iter()
            .collect::<Vec<_>>(),
        [(7, 8)]
    );
    Ok(())
}

#[test]
fn without_a_face_model_nothing_is_asked() -> anyhow::Result<()> {
    let (_root, collection) = sample()?;
    collection.db.execute(
        "DELETE FROM face_settings WHERE key='rust.embedder_key'",
        [],
    )?;
    assert!(duplicates::load(&collection)?.is_empty());
    Ok(())
}
