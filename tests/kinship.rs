//! Verwandtschaftsrechner (ADR 0061): Fixture-Familien mit erwarteten Beziehungen,
//! Grenzen und ein Eigenschaftstest gegen eine naive Referenzdefinition.
use foto_importer::core::kinship::{Family, MAX_GENERATIONS, MAX_PER_GROUP, Relation};
use foto_importer::core::relations::{Edge, Gender, Kind, Role, canonical};
use std::collections::BTreeMap;

fn parent(a: i64, b: i64) -> Edge {
    canonical(a, Role::Parent, b)
}
fn partner(a: i64, b: i64) -> Edge {
    canonical(a, Role::Partner, b)
}
fn former(a: i64, b: i64) -> Edge {
    canonical(a, Role::FormerPartner, b)
}

fn genders(list: &[(i64, Gender)]) -> BTreeMap<i64, Gender> {
    list.iter().copied().collect()
}

/// Wortlaut "a ist <Bezeichnung> von b" ohne Namen; "-" bei fehlender Beziehung.
fn says(f: &Family, a: i64, b: i64) -> String {
    f.relationship(a, b)
        .map_or("-".to_string(), |l| l.relation.label(f.gender(a)))
}

use Gender::{Female as W, Male as M, Unknown as U};

/// Vier Generationen mit allen Konstellationen der Prüfung.
///
/// 1 Urgroßvater, 2 Urgroßmutter -> Kinder 3 (Opa), 5 (Großonkel Gustav)
/// 3+4 (Oma) -> 7 (Vater), 8 (Onkel); 8+10 (Tante durch Heirat)
/// 5+6 -> 11 Ruth. 12+13 -> 9 (Mutter), 14 (Tante der Mutterseite); 14+15
/// 7+9 -> 16 Ben, 17 Dora; 8+10 -> 18 Cem; 14+15 -> 19 Eva
/// Halbgeschwister: 7+21 (frühere Partnerin) -> 20 Hans; 9+24 -> 22 Mia.
/// 16+30 -> 29 Leo
fn fixture() -> Family {
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
        partner(5, 6),
        parent(5, 11),
        parent(6, 11),
        parent(12, 9),
        parent(13, 9),
        parent(12, 14),
        parent(13, 14),
        partner(14, 15),
        partner(7, 9),
        parent(7, 16),
        parent(9, 16),
        parent(7, 17),
        parent(9, 17),
        parent(8, 18),
        parent(10, 18),
        parent(14, 19),
        parent(15, 19),
        former(7, 21),
        parent(7, 20),
        parent(21, 20),
        parent(9, 22),
        parent(24, 22),
        partner(16, 30),
        parent(16, 29),
        parent(30, 29),
    ];
    Family::new(
        &edges,
        &genders(&[
            (1, M),
            (2, W),
            (3, M),
            (4, W),
            (5, M),
            (6, W),
            (7, M),
            (8, M),
            (9, W),
            (10, W),
            (11, W),
            (12, M),
            (13, W),
            (14, W),
            (15, M),
            (16, M),
            (17, W),
            (18, M),
            (19, W),
            (20, M),
            (21, W),
            (22, W),
            (24, M),
            (29, M),
            (30, W),
        ]),
    )
}

#[test]
fn four_generations_have_the_expected_names() {
    let f = fixture();
    let table: [(i64, i64, &str); 30] = [
        (1, 29, "Ururgroßvater"),
        (2, 29, "Ururgroßmutter"),
        (3, 29, "Urgroßvater"),
        (4, 29, "Urgroßmutter"),
        (7, 29, "Großvater"),
        (9, 29, "Großmutter"),
        (16, 29, "Vater"),
        (30, 29, "Mutter"),
        (29, 16, "Sohn"),
        (29, 7, "Enkel"),
        (29, 3, "Urenkel"),
        (29, 1, "Ururenkel"),
        (16, 17, "Bruder"),
        (17, 16, "Schwester"),
        (16, 20, "Halbbruder"),
        (20, 16, "Halbbruder"),
        (22, 16, "Halbschwester"),
        (16, 22, "Halbbruder"),
        (8, 16, "Onkel"),
        (10, 16, "Tante (angeheiratet)"),
        (14, 16, "Tante"),
        (16, 8, "Neffe"),
        (17, 8, "Nichte"),
        (5, 16, "Großonkel"),
        (16, 5, "Großneffe"),
        (11, 16, "Cousine 1. Grades, 1x entfernt"),
        (16, 11, "Cousin 1. Grades, 1x entfernt"),
        (16, 18, "Cousin 1. Grades"),
        (16, 19, "Cousin 1. Grades"),
        (19, 16, "Cousine 1. Grades"),
    ];
    for (a, b, label) in table {
        assert_eq!(says(&f, a, b), label, "{a} ist ? von {b}");
    }
}

#[test]
fn sentences_name_the_pair_and_the_path() {
    let f = fixture();
    let name = |id: i64| format!("P{id}");
    assert_eq!(f.sentence(3, 16, &name), "P3 ist Großvater von P16.");
    assert_eq!(
        f.sentence(16, 18, &name),
        "P16 und P18 sind Cousins 1. Grades."
    );
    assert_eq!(f.sentence(16, 17, &name), "P16 und P17 sind Geschwister.");
    assert_eq!(
        f.sentence(16, 20, &name),
        "P16 und P20 sind Halbgeschwister."
    );
    assert!(f.sentence(16, 99, &name).contains("keine Verwandtschaft"));
    let link = f.relationship(16, 18).expect("Cousins");
    assert_eq!(
        f.path_text(&link, &name),
        "P16 → Vater P7 → Vater P3 → Sohn P8 → Sohn P18"
    );
    assert_eq!(link.path, vec![16, 7, 3, 8, 18]);
}

#[test]
fn in_laws_and_the_limits_of_affinity() {
    let f = fixture();
    assert_eq!(says(&f, 12, 7), "Schwiegervater");
    assert_eq!(says(&f, 13, 7), "Schwiegermutter");
    assert_eq!(says(&f, 7, 12), "Schwiegersohn");
    assert_eq!(says(&f, 14, 7), "Schwägerin");
    assert_eq!(says(&f, 7, 14), "Schwager");
    assert_eq!(says(&f, 15, 9), "Schwager");
    assert_eq!(says(&f, 4, 9), "Schwiegermutter");
    assert_eq!(says(&f, 30, 7), "Schwiegertochter");
    // Der Partner des Schwagers (Schwippschwager) wird bewusst nicht benannt.
    assert_eq!(says(&f, 15, 7), "-");
    // Frühere Partner begründen keine Verschwägerung.
    assert_eq!(says(&f, 21, 7), "Ex-Partnerin");
    assert_eq!(says(&f, 21, 9), "-");
    assert_eq!(says(&f, 7, 21), "Ex-Partner");
    // Blutsverwandtschaft schlägt Verschwägerung; Partner stehen vor allem.
    assert_eq!(says(&f, 7, 9), "Partner");
    assert_eq!(says(&f, 9, 7), "Partnerin");
}

#[test]
fn patchwork_families_have_step_relations() {
    // 1+2 -> 10; 1 und 3 sind Partner; 3 -> 11. 5 ist frühere Partnerin von 1.
    let f = Family::new(
        &[
            parent(1, 10),
            parent(2, 10),
            partner(1, 3),
            parent(3, 11),
            former(1, 5),
        ],
        &genders(&[(1, M), (2, W), (3, W), (5, W), (10, M), (11, W)]),
    );
    assert_eq!(says(&f, 3, 10), "Stiefmutter");
    assert_eq!(says(&f, 10, 3), "Stiefsohn");
    assert_eq!(says(&f, 11, 1), "Stieftochter");
    assert_eq!(says(&f, 1, 11), "Stiefvater");
    assert_eq!(says(&f, 10, 11), "Stiefbruder");
    assert_eq!(says(&f, 11, 10), "Stiefschwester");
    // Die frühere Partnerin ist weder Stiefmutter noch sonst verwandt.
    assert_eq!(says(&f, 5, 10), "-");
    let step: Vec<i64> = f
        .relatives_of(10)
        .iter()
        .filter(|g| g.title == "Stiefgeschwister")
        .flat_map(|g| g.members.iter().map(|m| m.person))
        .collect();
    assert_eq!(step, vec![11]);
}

#[test]
fn several_partners_each_get_their_own_children_and_step_relations() {
    let f = Family::new(
        &[
            partner(1, 2),
            partner(1, 3),
            parent(1, 10),
            parent(2, 10),
            parent(1, 11),
            parent(3, 11),
        ],
        &genders(&[(1, M), (2, W), (3, W), (10, M), (11, M)]),
    );
    assert_eq!(says(&f, 10, 11), "Halbbruder");
    assert_eq!(says(&f, 2, 11), "Stiefmutter");
    assert_eq!(says(&f, 3, 10), "Stiefmutter");
    assert_eq!(says(&f, 2, 3), "-");
}

#[test]
fn one_recorded_parent_or_more_than_two_parents() {
    // 40 kennt nur 41; 42 hat 41 und 43; 44 hat drei Eltern (Adoption).
    let f = Family::new(
        &[
            parent(41, 40),
            parent(41, 42),
            parent(43, 42),
            parent(41, 44),
            parent(43, 44),
            parent(45, 44),
        ],
        &BTreeMap::new(),
    );
    // Unvollständige Eltern: im Zweifel Geschwister, nicht Halbgeschwister.
    assert_eq!(says(&f, 40, 42), "Geschwisterteil");
    // 42 und 44 teilen 41 und 43 (zwei von drei): Geschwister.
    assert_eq!(says(&f, 42, 44), "Geschwisterteil");
    assert_eq!(f.parents_of(44).len(), 3);
    // Unbekanntes Geschlecht: neutrale Bezeichnungen.
    assert_eq!(says(&f, 41, 40), "Elternteil");
    assert_eq!(says(&f, 40, 41), "Kind");
    assert_eq!(says(&f, 40, 99), "-");
    // Eine Person ohne Eltern hat nur Nachkommen und Partner.
    let groups = f.relatives_of(41);
    let titles: Vec<&str> = groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(titles, ["Kinder"]);
}

#[test]
fn generation_limit_and_group_cap_are_reported() {
    let mut edges = Vec::new();
    for i in 0..=(MAX_GENERATIONS as i64 + 1) {
        edges.push(parent(1000 + i, 1001 + i));
    }
    let f = Family::new(&edges, &BTreeMap::new());
    // Genau MAX_GENERATIONS Schritte werden noch gefunden, einer mehr nicht.
    assert!(
        f.relationship(1000, 1000 + MAX_GENERATIONS as i64)
            .is_some()
    );
    assert!(
        f.relationship(1000, 1001 + MAX_GENERATIONS as i64)
            .is_none()
    );
    // Viele Kinder: die Gruppe wird gekappt, der Rest steht in `more`.
    let many: Vec<Edge> = (0..(MAX_PER_GROUP as i64 + 20))
        .map(|k| parent(1, 100 + k))
        .collect();
    let f = Family::new(&many, &BTreeMap::new());
    let kids = &f.relatives_of(1)[0];
    assert_eq!(kids.title, "Kinder");
    assert_eq!(kids.members.len(), MAX_PER_GROUP);
    assert_eq!(kids.more, 20);
    // Die Suche ohne Anzeigegrenze liefert alle.
    assert_eq!(
        f.find(1, &|_| true).len(),
        MAX_PER_GROUP + 20,
        "find ist nicht gekappt"
    );
}

#[test]
fn deep_cousins_are_found_without_stored_edges() {
    // Ur-Ur-Großeltern 1; Linien a: 1->2->3->4->5, b: 1->12->13->14->15
    let f = Family::new(
        &[
            parent(1, 2),
            parent(2, 3),
            parent(3, 4),
            parent(4, 5),
            parent(1, 12),
            parent(12, 13),
            parent(13, 14),
            parent(14, 15),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(says(&f, 5, 15), "Cousin/Cousine 3. Grades");
    assert_eq!(says(&f, 4, 15), "Cousin/Cousine 2. Grades, 1x entfernt");
    let cousins: Vec<i64> = f.cousins_of(5).iter().map(|m| m.person).collect();
    assert_eq!(cousins, vec![13, 14, 15]);
}

#[test]
fn recorded_shortcuts_stay_readable_and_are_suppressed_when_the_tree_knows() {
    // Behelfsverbindungen ohne Eltern.
    let shortcut = [
        canonical(1, Role::Sibling, 2),
        canonical(3, Role::Cousin, 4),
    ];
    let f = Family::new(&shortcut, &BTreeMap::new());
    assert_eq!(says(&f, 1, 2), "Geschwisterteil (direkt erfasst)");
    assert_eq!(says(&f, 4, 3), "Cousin/Cousine (direkt erfasst)");
    assert!(f.redundant_recorded().is_empty());
    let groups = f.relatives_of(1);
    assert_eq!(groups[0].title, "Geschwister (direkt erfasst)");
    // Sobald die Eltern erfasst sind, folgt dasselbe aus dem Baum.
    let mut edges = shortcut.to_vec();
    edges.extend([parent(9, 1), parent(9, 2)]);
    let f = Family::new(&edges, &BTreeMap::new());
    assert_eq!(says(&f, 1, 2), "Geschwisterteil");
    assert_eq!(f.redundant_recorded(), vec![(1, Kind::Sibling, 2)]);
    let titles: Vec<String> = f.relatives_of(1).into_iter().map(|g| g.title).collect();
    assert!(!titles.iter().any(|t| t.contains("direkt erfasst")));
}

#[test]
fn corrupt_cycles_terminate() {
    let f = Family::new(
        &[parent(1, 2), parent(2, 3), parent(3, 1), partner(1, 4)],
        &BTreeMap::new(),
    );
    // Kein Hängenbleiben; irgendeine endliche Antwort.
    let _ = f.relationship(1, 3);
    let _ = f.relatives_of(2);
    let _ = f.cousins_of(1);
}

#[test]
fn unknown_people_have_no_relatives() {
    let f = fixture();
    assert!(f.relatives_of(9999).is_empty());
    assert!(f.relationship(9999, 1).is_none());
    assert!(f.relationship(1, 1).is_none());
}

#[test]
fn relatives_are_grouped_like_a_family_tree() {
    let f = fixture();
    let groups = f.relatives_of(16);
    let titles: Vec<&str> = groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Partner",
            "Eltern",
            "Kinder",
            "Großeltern",
            "Geschwister",
            "Halbgeschwister",
            "Urgroßeltern",
            "Onkel und Tanten",
            "Großonkel und Großtanten",
            "Cousins und Cousinen 1. Grades",
            "Cousins und Cousinen 1. Grades, 1x entfernt (ältere Generation)",
            "Onkel und Tanten (angeheiratet)",
            "Großonkel und Großtanten (angeheiratet)"
        ]
    );
    let by = |title: &str| -> Vec<i64> {
        groups
            .iter()
            .find(|g| g.title == title)
            .map(|g| g.members.iter().map(|m| m.person).collect())
            .unwrap_or_default()
    };
    assert_eq!(by("Eltern"), vec![7, 9]);
    assert_eq!(by("Geschwister"), vec![17]);
    assert_eq!(by("Halbgeschwister"), vec![20, 22]);
    assert_eq!(by("Großeltern"), vec![3, 4, 12, 13]);
    assert_eq!(by("Urgroßeltern"), vec![1, 2]);
    assert_eq!(by("Onkel und Tanten"), vec![8, 14]);
    assert_eq!(by("Cousins und Cousinen 1. Grades"), vec![18, 19]);
}

#[test]
fn all_cousins_of_a_person_without_stored_cousin_edges() {
    let f = fixture();
    let cousins: Vec<i64> = f.cousins_of(16).iter().map(|m| m.person).collect();
    assert_eq!(cousins, vec![18, 19, 11]);
    for m in f.cousins_of(16) {
        assert!(m.relation.is_cousin());
    }
}

// ------------------------------------------------------------ Eigenschaftstest

struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

/// Naive Referenz: kürzeste Aufwärtsabstände durch wiederholtes Entspannen aller Kanten.
fn naive_up(parents: &BTreeMap<i64, Vec<i64>>, start: i64) -> BTreeMap<i64, u8> {
    let mut dist = BTreeMap::from([(start, 0u8)]);
    loop {
        let mut changed = false;
        for (child, ps) in parents {
            if let Some(&d) = dist.get(child) {
                for &p in ps {
                    if d < MAX_GENERATIONS && dist.get(&p).is_none_or(|&old| d + 1 < old) {
                        dist.insert(p, d + 1);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            return dist;
        }
    }
}

fn naive_blood(parents: &BTreeMap<i64, Vec<i64>>, a: i64, b: i64) -> Option<(u8, u8)> {
    let (ua, ub) = (naive_up(parents, a), naive_up(parents, b));
    ua.iter()
        .filter_map(|(x, u)| ub.get(x).map(|d| (*u, *d, *x)))
        .min_by_key(|(u, d, x)| (u + d, u.abs_diff(*d), *x))
        .map(|(u, d, _)| (u, d))
}

#[test]
fn random_trees_match_the_naive_definition_and_are_mutually_inverse() {
    for seed in 0..40u64 {
        let mut rng = Lcg(seed * 7919 + 13);
        let n = 8 + rng.next(14) as i64;
        let mut edges = Vec::new();
        let mut parents: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
        for child in 2..=n {
            // Eltern nur aus kleineren IDs: zyklenfrei.
            let count = rng.next(3);
            for _ in 0..count {
                let p = 1 + rng.next(child as u64 - 1) as i64;
                let list = parents.entry(child).or_default();
                if !list.contains(&p) {
                    list.push(p);
                    edges.push(parent(p, child));
                }
            }
        }
        for _ in 0..rng.next(5) {
            let (a, b) = (1 + rng.next(n as u64) as i64, 1 + rng.next(n as u64) as i64);
            if a != b && !edges.contains(&canonical(a, Role::Partner, b)) {
                edges.push(partner(a, b));
            }
        }
        let genders: BTreeMap<i64, Gender> = (1..=n)
            .map(|i| (i, [M, W, U][rng.next(3) as usize]))
            .collect();
        let f = Family::new(&edges, &genders);
        for a in 1..=n {
            for b in 1..=n {
                if a == b {
                    continue;
                }
                let forward = f.relationship(a, b);
                let backward = f.relationship(b, a);
                assert_eq!(
                    forward.as_ref().map(|l| l.relation.inverse()),
                    backward.as_ref().map(|l| l.relation),
                    "seed {seed}: {a}<->{b} nicht invers"
                );
                if let (Some(fl), Some(bl)) = (&forward, &backward) {
                    let mut reversed = bl.path.clone();
                    reversed.reverse();
                    assert_eq!(fl.path, reversed, "Weg in Gegenrichtung");
                    assert_eq!(fl.path.first(), Some(&a));
                    assert_eq!(fl.path.last(), Some(&b));
                    if let Relation::Blood { .. } = fl.relation {
                        assert_eq!(
                            fl.relation.symmetric(),
                            fl.relation == fl.relation.inverse(),
                            "symmetrisch bedeutet gleich in beide Richtungen"
                        );
                    }
                }
                let expected = naive_blood(&parents, a, b);
                let tree = f.tree_relation(a, b);
                match (expected, tree) {
                    (None, None) => {}
                    (Some((u, d)), Some(Relation::Blood { a_gen, b_gen, .. })) => {
                        assert_eq!((u, d), (a_gen, b_gen), "seed {seed}: {a}->{b}");
                    }
                    other => panic!("seed {seed}: {a}->{b}: {other:?}"),
                }
                // Der Satz ist immer bildbar und nicht leer.
                let s = f.sentence(a, b, &|i| i.to_string());
                assert!(!s.is_empty());
            }
        }
    }
}

#[test]
fn labels_exist_for_every_blood_relation_and_gender() {
    for a_gen in 0..=MAX_GENERATIONS {
        for b_gen in 0..=MAX_GENERATIONS {
            if a_gen + b_gen == 0 {
                continue;
            }
            for g in [W, M, U] {
                for half in [false, true] {
                    let r = Relation::Blood { a_gen, b_gen, half };
                    assert!(!r.label(g).is_empty(), "{a_gen},{b_gen}");
                    assert!(!r.group_title().is_empty());
                }
            }
        }
    }
}
