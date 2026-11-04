use foto_importer::core::{
    inference,
    jobs::Control,
    matching::{self, Candidate},
};
#[test]
fn joint_assignment_avoids_greedy_identity_error() -> anyhow::Result<()> {
    let scores = vec![vec![0.9, 0.89, 0.3, 0.3], vec![0.88, 0.1, 0.3, 0.3]];
    assert_eq!(matching::assignment(&scores)?, vec![1, 0]);
    Ok(())
}
#[test]
fn unknown_slots_allow_multiple_unknown_faces() -> anyhow::Result<()> {
    assert_eq!(
        matching::assignment(&[vec![0.1, 0.3, 0.3], vec![0.2, 0.3, 0.3]])?
            .iter()
            .filter(|c| **c == 0)
            .count(),
        0
    );
    Ok(())
}
#[test]
fn references_preserve_pins_years_and_reproducible_validation() -> anyhow::Result<()> {
    let mut rows = Vec::new();
    for id in 1i64..=16 {
        let angle = id as f32 * 0.17;
        let mut vector = vec![1.0, angle.sin() * 0.8, angle.cos() * 0.8];
        vector = inference::normalize(&vector)?;
        rows.push(Candidate {
            id,
            quality: 0.9,
            captured: format!("{}:06:{:02} 12:00:00", 2000 + id % 4, id),
            pinned: id == 1,
            vector,
        });
    }
    let a = matching::select_references(&rows, 8, 42, 0.999, &Control::default())?;
    let b = matching::select_references(&rows, 8, 42, 0.999, &Control::default())?;
    assert_eq!(a.references, b.references);
    assert_eq!(a.validation, b.validation);
    assert!(a.references.contains(&1) || a.validation.contains(&1));
    assert_eq!(a.references.len() + a.validation.len(), 8);
    assert_eq!(a.validation.len(), 2);
    Ok(())
}
#[test]
fn over_limit_pins_are_an_explicit_error() {
    let rows = (0..3)
        .map(|id| Candidate {
            id,
            quality: 1.0,
            captured: String::new(),
            pinned: true,
            vector: vec![1.0, 0.0],
        })
        .collect::<Vec<_>>();
    assert!(matching::select_references(&rows, 2, 42, 0.92, &Control::default()).is_err());
}
