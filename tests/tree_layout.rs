//! Stammbaum-Layout (ADR 0061): keine Überlappung, deterministisch, endlich, schnell.
#![allow(clippy::unwrap_used)]
use foto_importer::core::kinship::Family;
use foto_importer::core::relations::{Edge, Gender, Role, canonical};
use foto_importer::core::tree_layout::{LinkKind, MAX_TREE_NODES, NodeRole, TreeLayout, layout};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

fn parent(a: i64, b: i64) -> Edge {
    canonical(a, Role::Parent, b)
}
fn partner(a: i64, b: i64) -> Edge {
    canonical(a, Role::Partner, b)
}

/// Familie wie im Verwandtschaftstest: 4 Generationen, Geschwister, Halbgeschwister,
/// frühere Partner, mehrere Partner.
fn family() -> Family {
    let edges = [
        parent(1, 3),
        parent(2, 3),
        parent(1, 5),
        parent(2, 5),
        partner(3, 4),
        parent(3, 7),
        parent(4, 7),
        parent(3, 8),
        parent(4, 8),
        partner(8, 10),
        parent(12, 9),
        parent(13, 9),
        partner(7, 9),
        parent(7, 16),
        parent(9, 16),
        parent(7, 17),
        parent(9, 17),
        parent(8, 18),
        parent(10, 18),
        canonical(7, Role::FormerPartner, 21),
        parent(7, 20),
        parent(21, 20),
        parent(9, 22),
        parent(24, 22),
        partner(16, 30),
        parent(16, 29),
        parent(30, 29),
        parent(29, 40),
        parent(41, 40),
        partner(29, 41),
    ];
    let genders: BTreeMap<i64, Gender> = [(7, Gender::Male), (9, Gender::Female)]
        .into_iter()
        .collect();
    Family::new(&edges, &genders)
}

fn assert_sound(layout: &TreeLayout) {
    let mut rows: BTreeMap<i32, Vec<(f32, i64)>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for n in &layout.nodes {
        assert!(n.x.is_finite() && n.y.is_finite(), "endliche Koordinaten");
        assert!(n.x >= 0.5 - 1e-4 && n.y >= 0.0, "im positiven Bereich");
        assert!(n.x + 0.5 <= layout.width + 1e-4);
        assert!(n.y + 1.0 <= layout.height + 1e-4);
        assert!(seen.insert(n.person), "jede Person einmal");
        rows.entry(n.y as i32).or_default().push((n.x, n.person));
    }
    for (row, mut xs) in rows {
        xs.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in xs.windows(2) {
            assert!(
                pair[1].0 - pair[0].0 >= 1.0 - 1e-4,
                "Überlappung in Ebene {row}: {pair:?}"
            );
        }
    }
    for link in &layout.links {
        match link {
            LinkKind::Parents { child, parents } => {
                let c = layout.node(*child).expect("Kind dargestellt");
                for p in parents {
                    let p = layout.node(*p).expect("Elternteil dargestellt");
                    assert!(p.y < c.y, "Eltern liegen oberhalb der Kinder");
                }
            }
            LinkKind::Partner { a, b } | LinkKind::FormerPartner { a, b } => {
                assert_eq!(layout.node(*a).unwrap().y, layout.node(*b).unwrap().y);
            }
        }
    }
}

#[test]
fn every_focus_and_depth_is_sound_and_deterministic() {
    let f = family();
    for focus in f.people() {
        for (up, down) in [(0, 0), (1, 1), (2, 2), (3, 3), (6, 6)] {
            let a = layout(&f, focus, up, down);
            assert_sound(&a);
            let b = layout(&f, focus, up, down);
            let key = |l: &TreeLayout| -> Vec<(i64, u32, u32)> {
                l.nodes
                    .iter()
                    .map(|n| (n.person, n.x.to_bits(), n.y.to_bits()))
                    .collect()
            };
            assert_eq!(key(&a), key(&b), "deterministisch");
            assert!(a.node(focus).is_some_and(|n| n.role == NodeRole::Focus));
        }
    }
}

#[test]
fn the_neighbourhood_has_the_expected_generations() {
    let f = family();
    // Fokus Ben (16): Eltern 7+9, Großeltern 3+4 und 12+13, Geschwister 17, 20, 22.
    let l = layout(&f, 16, 2, 2);
    let generation_of = |p: i64| l.node(p).map(|n| n.generation);
    assert_eq!(generation_of(7), Some(-1));
    assert_eq!(generation_of(9), Some(-1));
    assert_eq!(generation_of(3), Some(-2));
    assert_eq!(generation_of(12), Some(-2));
    assert_eq!(
        generation_of(1),
        None,
        "Urgroßeltern liegen außerhalb von zwei Generationen"
    );
    // Geschwister (auch Halbgeschwister) stehen auf der Ebene der Person.
    for sibling in [17, 20, 22] {
        assert_eq!(generation_of(sibling), Some(0));
        assert_eq!(l.node(sibling).unwrap().y, l.node(16).unwrap().y);
        assert_eq!(l.node(sibling).unwrap().role, NodeRole::Sibling);
    }
    // Partner direkt neben der Person, Kinder und Enkel darunter.
    let ben = l.node(16).unwrap();
    let partner = l.node(30).unwrap();
    assert_eq!(partner.y, ben.y);
    assert!((partner.x - ben.x).abs() < 1.2);
    assert_eq!(generation_of(29), Some(1));
    assert_eq!(generation_of(40), Some(2));
    assert_eq!(
        generation_of(41),
        Some(1),
        "Partner der Kinder stehen neben dem Kind 29"
    );
    // Onkel/Tante, Cousins sind nicht Teil des Baumes (Verwandtschaftsprüfung).
    assert_eq!(generation_of(8), None);
    assert_eq!(generation_of(18), None);
    // Grenze der Generationen wird markiert.
    assert!(
        l.node(3).unwrap().more_up,
        "Eltern von 3 sind nicht dargestellt"
    );
    let small = layout(&f, 16, 1, 0);
    assert!(small.node(29).is_none());
    assert!(small.node(16).unwrap().more_down);
    // Familien sind verbunden: ein Link je Kind mit seinen Eltern.
    let parents_of_ben: Vec<&LinkKind> = l
        .links
        .iter()
        .filter(|k| matches!(k, LinkKind::Parents { child: 16, .. }))
        .collect();
    assert_eq!(parents_of_ben.len(), 1);
    assert_eq!(
        parents_of_ben[0],
        &LinkKind::Parents {
            child: 16,
            parents: vec![7, 9]
        }
    );
    assert!(l.links.contains(&LinkKind::Partner { a: 16, b: 30 }));
}

#[test]
fn former_partners_stay_next_to_the_person_with_a_dashed_link() {
    let f = family();
    let l = layout(&f, 7, 1, 1);
    assert_sound(&l);
    assert_eq!(l.node(21).unwrap().role, NodeRole::FormerPartner);
    assert!(l.links.contains(&LinkKind::FormerPartner { a: 7, b: 21 }));
    // Kinder mit dem früheren Partner stehen weiterhin unter der Person.
    assert_eq!(l.node(20).unwrap().generation, 1);
}

#[test]
fn people_without_relations_and_unknown_people_are_handled() {
    let f = family();
    let l = layout(&f, 99999, 3, 3);
    assert_eq!(l.nodes.len(), 1);
    assert_sound(&l);
    let empty = Family::default();
    let l = layout(&empty, 1, 6, 6);
    assert_eq!(l.nodes.len(), 1);
    assert!(l.width.is_finite() && l.height >= 1.0);
}

/// Große Familie: 7 Generationen mit je 3 Kindern und je einem Partner.
fn big_family() -> Family {
    let mut edges = Vec::new();
    let mut next = 2i64;
    let mut level = vec![1i64];
    for _ in 0..6 {
        let mut following = Vec::new();
        for &p in &level {
            let spouse = next;
            next += 1;
            edges.push(partner(p, spouse));
            for _ in 0..3 {
                let child = next;
                next += 1;
                edges.push(parent(p, child));
                edges.push(parent(spouse, child));
                following.push(child);
            }
        }
        level = following;
    }
    Family::new(&edges, &BTreeMap::new())
}

#[test]
fn hundreds_of_people_are_laid_out_within_the_time_limit() {
    let f = big_family();
    assert!(f.people().len() > 1000);
    let start = Instant::now();
    for focus in [1i64, 200, 700] {
        let l = layout(&f, focus, 3, 4);
        assert_sound(&l);
        assert!(l.nodes.len() <= MAX_TREE_NODES);
    }
    let big = layout(&f, 1, 6, 6);
    assert_sound(&big);
    assert_eq!(big.nodes.len(), MAX_TREE_NODES);
    assert!(big.truncated > 0);
    assert!(
        start.elapsed().as_secs() < 20,
        "Layout zu langsam: {:?}",
        start.elapsed()
    );
}
