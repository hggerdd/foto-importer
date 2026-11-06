use foto_importer::core::{
    classifier, collection::Collection, faces, inference, jobs::Control, recognition,
};
use rusqlite::params;
use std::collections::BTreeSet;

fn noise(state: &mut u64) -> f32 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 33) as f32 / (1u64 << 31) as f32) - 0.5
}
fn sample(person: i64, state: &mut u64) -> Vec<f32> {
    let mut v = vec![0.0f32; 16];
    v[(person as usize * 3) % 16] = 1.0;
    v[(person as usize * 3 + 1) % 16] = 0.5;
    for x in v.iter_mut() {
        *x += noise(state) * 0.6;
    }
    inference::normalize(&v).unwrap_or_default()
}
fn data(people: &[i64], per: usize, state: &mut u64) -> Vec<(i64, Vec<f32>)> {
    people
        .iter()
        .flat_map(|p| (0..per).map(|_| *p).collect::<Vec<_>>())
        .map(|p| (p, sample(p, state)))
        .collect()
}

#[test]
fn classifier_separates_people_and_respects_exclusions() -> anyhow::Result<()> {
    let mut state = 7;
    let people = [1, 2, 3, 4];
    let train = data(&people, 20, &mut state);
    let validation = data(&people, 8, &mut state);
    let model = classifier::fit(&train, &Control::default())?.expect("model");
    let metrics = classifier::evaluate(&model, &train, &validation).expect("metrics");
    assert!(metrics.top1 > 0.9, "top1 {}", metrics.top1);
    assert_eq!(metrics.top3, 1.0);
    let all: BTreeSet<i64> = people.into();
    let ranking = model.predict_among(&validation[0].1, &all);
    assert_eq!(ranking[0].0, validation[0].0);
    assert!((ranking.iter().map(|r| r.1).sum::<f32>() - 1.0).abs() < 1e-4);
    // Ausgeschlossene Personen erscheinen nie; der Rest bleibt normiert.
    let without: BTreeSet<i64> = all
        .iter()
        .copied()
        .filter(|p| *p != validation[0].0)
        .collect();
    let ranking = model.predict_among(&validation[0].1, &without);
    assert!(ranking.iter().all(|r| r.0 != validation[0].0));
    assert!((ranking.iter().map(|r| r.1).sum::<f32>() - 1.0).abs() < 1e-4);
    Ok(())
}

#[test]
fn training_is_deterministic_and_needs_two_people() -> anyhow::Result<()> {
    let mut state = 3;
    let train = data(&[1, 2], 6, &mut state);
    let a = classifier::fit(&train, &Control::default())?.expect("model");
    let b = classifier::fit(&train, &Control::default())?.expect("model");
    assert_eq!(a.weights, b.weights);
    assert!(classifier::fit(&data(&[1], 6, &mut state), &Control::default())?.is_none());
    assert!(classifier::fit(&data(&[1, 2], 1, &mut state), &Control::default())?.is_none());
    Ok(())
}

#[test]
fn stored_model_orders_collection_suggestions_with_fallback() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut c = Collection::open(temp.path(), true)?;
    let mut state = 11;
    let mut face_id = 0i64;
    for person in 1..=3i64 {
        c.db.execute("INSERT INTO persons(id) VALUES(?)", [person])?;
        c.db.execute(
            "INSERT INTO person_attrs VALUES(?,'name',?)",
            params![person, format!("Person {person}")],
        )?;
        for n in 0..10 {
            face_id += 1;
            c.db.execute(
                "INSERT INTO photos(id,status) VALUES(?,'present')",
                [face_id],
            )?;
            c.db.execute("INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality,person_id,assignment) VALUES(?,?,?,'det',0,0,1,1,1,?,'user')", params![face_id, face_id, "a.jpg", person])?;
            c.db.execute(
                "INSERT INTO face_embeddings VALUES(?,'test',16,?)",
                params![face_id, faces::encode(&sample(person, &mut state))],
            )?;
            c.db.execute(
                "INSERT INTO training_set VALUES(?,?,'test',?,datetime('now'),'t',0)",
                params![person, face_id, if n < 7 { "ref" } else { "val" }],
            )?;
        }
    }
    // Unbekanntes Gesicht von Person 2.
    face_id += 1;
    c.db.execute(
        "INSERT INTO photos(id,status) VALUES(?,'present')",
        [face_id],
    )?;
    c.db.execute("INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality) VALUES(?,?,?,'det',0,0,1,1,1)", params![face_id, face_id, "b.jpg"])?;
    c.db.execute(
        "INSERT INTO face_embeddings VALUES(?,'test',16,?)",
        params![face_id, faces::encode(&sample(2, &mut state))],
    )?;
    let before = recognition::suggestions(&c, face_id, "test")?;
    assert!(!before.by_model);
    let metrics = classifier::train(&mut c, "test", &Control::default())?.expect("metrics");
    assert_eq!(metrics.people, 3);
    let after = recognition::suggestions(&c, face_id, "test")?;
    assert!(after.by_model);
    assert_eq!(after.items.len(), 3);
    assert_eq!(after.items[0].0, 2);
    // Eine Ablehnung entfernt die Person auch aus den Modellvorschlägen.
    c.db.execute("INSERT INTO face_rejections VALUES(?,2)", [face_id])?;
    let rejected = recognition::suggestions(&c, face_id, "test")?;
    assert!(rejected.items.iter().all(|i| i.0 != 2));
    Ok(())
}
