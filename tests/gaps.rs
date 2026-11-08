//! Rückfragen zu wahrscheinlich fehlenden Eltern (ADR 0061): reine Funktion und
//! Anwendung auf eine Sammlung.
use foto_importer::core::{
    collection::Collection,
    gaps::{self, Answer, Decisions, MAX_SUGGESTIONS, Suggestion, SuggestionKind},
    relations::{self, Edge, Role, canonical},
};

fn parent(a: i64, b: i64) -> Edge {
    canonical(a, Role::Parent, b)
}

fn ask(edges: &[Edge]) -> Vec<Suggestion> {
    gaps::suggestions(edges, &Decisions::default())
}

fn collection(people: i64) -> anyhow::Result<(tempfile::TempDir, Collection)> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    for id in 1..=people {
        c.db.execute("INSERT INTO persons(id) VALUES(?)", [id])?;
    }
    Ok((temp, c))
}

#[test]
fn a_missing_second_parent_is_asked_not_added() {
    // 10 hat die Eltern 1 und 2, das Geschwister 11 nur 1.
    let edges = [parent(1, 10), parent(2, 10), parent(1, 11)];
    let found = ask(&edges);
    assert_eq!(
        found,
        vec![Suggestion {
            kind: SuggestionKind::SiblingParents,
            candidates: vec![2],
            targets: vec![11],
            via: vec![10],
        }]
    );
    // Die Funktion verändert nichts und ist deterministisch.
    assert_eq!(ask(&edges), found);
}

#[test]
fn recorded_sibling_shortcut_asks_for_both_parents() {
    let edges = [
        parent(1, 10),
        parent(2, 10),
        canonical(10, Role::Sibling, 11),
    ];
    let found = ask(&edges);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].candidates, vec![1, 2]);
    assert_eq!(found[0].targets, vec![11]);
    // Antworten ergeben Kanten bzw. Ablehnungen, nichts sonst.
    let all = gaps::plan(&found[0], Answer::All);
    assert_eq!(all.add, vec![(1, 11), (2, 11)]);
    assert!(all.decline.is_empty());
    let only = gaps::plan(&found[0], Answer::Only(1));
    assert_eq!(only.add, vec![(1, 11)]);
    assert_eq!(only.decline, vec![(11, 2)]);
    let none = gaps::plan(&found[0], Answer::None);
    assert!(none.add.is_empty());
    assert_eq!(none.decline, vec![(11, 1), (11, 2)]);
}

#[test]
fn yes_adds_the_edges_and_the_question_goes_away() -> anyhow::Result<()> {
    let (_t, mut c) = collection(11)?;
    for (p, k) in [(1, 10), (2, 10), (1, 11)] {
        relations::add(&mut c, p, Role::Parent, k)?;
    }
    let edges = relations::load_edges(&c)?;
    let found = ask(&edges);
    assert_eq!(found.len(), 1);
    gaps::apply(&mut c, &found[0], Answer::All)?;
    let edges = relations::load_edges(&c)?;
    assert!(edges.contains(&parent(2, 11)));
    assert!(gaps::suggestions(&edges, &gaps::load_decisions(&c)?).is_empty());
    Ok(())
}

#[test]
fn no_means_half_siblings_and_is_never_asked_again() -> anyhow::Result<()> {
    let (_t, mut c) = collection(11)?;
    for (p, k) in [(1, 10), (2, 10), (1, 11)] {
        relations::add(&mut c, p, Role::Parent, k)?;
    }
    let found = ask(&relations::load_edges(&c)?);
    gaps::apply(&mut c, &found[0], Answer::None)?;
    // Keine Kante wurde angelegt, die Entscheidung ist gespeichert.
    let edges = relations::load_edges(&c)?;
    assert!(!edges.contains(&parent(2, 11)));
    let decisions = gaps::load_decisions(&c)?;
    assert!(decisions.declined.contains(&(11, 2)));
    assert!(gaps::suggestions(&edges, &decisions).is_empty());
    // Auch nach erneutem Öffnen der Sammlung.
    let root = c.root.clone();
    drop(c);
    let c = Collection::open(&root, true)?;
    let edges = relations::load_edges(&c)?;
    assert!(gaps::suggestions(&edges, &gaps::load_decisions(&c)?).is_empty());
    // Im Baum sind 10 und 11 jetzt sicher Halbgeschwister.
    let family = relations::load_family(&c)?;
    assert_eq!(
        family.relationship(11, 10).map(|l| l
            .relation
            .label(foto_importer::core::relations::Gender::Unknown)),
        Some("Halbgeschwisterteil".to_string()),
        "die Antwort Nein macht aus Geschwistern sichere Halbgeschwister"
    );
    Ok(())
}

#[test]
fn choosing_one_shared_parent_declines_the_other() -> anyhow::Result<()> {
    let (_t, mut c) = collection(11)?;
    relations::add(&mut c, 1, Role::Parent, 10)?;
    relations::add(&mut c, 2, Role::Parent, 10)?;
    relations::add(&mut c, 10, Role::Sibling, 11)?;
    let found = ask(&relations::load_edges(&c)?);
    assert_eq!(found[0].candidates, vec![1, 2]);
    gaps::apply(&mut c, &found[0], Answer::Only(1))?;
    let edges = relations::load_edges(&c)?;
    assert!(edges.contains(&parent(1, 11)) && !edges.contains(&parent(2, 11)));
    assert!(gaps::suggestions(&edges, &gaps::load_decisions(&c)?).is_empty());
    Ok(())
}

#[test]
fn partner_of_a_parent_is_only_a_question() {
    // 1 hat zwei Kinder und den Partner 2, der keine eigenen Kinder hat.
    let edges = [parent(1, 10), parent(1, 11), canonical(1, Role::Partner, 2)];
    let found = ask(&edges);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, SuggestionKind::PartnerParent);
    assert_eq!(found[0].candidates, vec![2]);
    assert_eq!(found[0].targets, vec![10, 11]);
    // Hat der Partner eigene Kinder, wird nicht gefragt (Patchwork).
    let mut patch = edges.to_vec();
    patch.push(parent(2, 12));
    assert!(ask(&patch).is_empty());
    // Frühere Partner und mehrere Partner lösen keine Frage aus.
    let former = [parent(1, 10), canonical(1, Role::FormerPartner, 2)];
    assert!(ask(&former).is_empty());
    let two = [
        parent(1, 10),
        canonical(1, Role::Partner, 2),
        canonical(1, Role::Partner, 3),
    ];
    assert!(ask(&two).is_empty());
    // Abgelehnt = Stiefbeziehung bleibt, keine Wiederholung.
    let mut decisions = Decisions::default();
    decisions.declined.insert((10, 2));
    decisions.declined.insert((11, 2));
    assert!(gaps::suggestions(&edges, &decisions).is_empty());
}

#[test]
fn contradictions_are_never_suggested() {
    // 2 ist Kind von 11: 2 als Elternteil von 11 wäre ein Zyklus.
    let cycle = [parent(1, 10), parent(2, 10), parent(1, 11), parent(11, 2)];
    assert!(ask(&cycle).is_empty());
    // Geschwister dürfen nicht Eltern und Partner zugleich sein: 2 ist Partner von 11.
    let partner = [
        parent(1, 10),
        parent(2, 10),
        parent(1, 11),
        canonical(2, Role::Partner, 11),
    ];
    assert!(ask(&partner).is_empty());
    // Mehr als zwei bekannte Eltern: keine Frage.
    let three = [
        parent(1, 10),
        parent(2, 10),
        parent(3, 11),
        parent(4, 11),
        parent(1, 11),
    ];
    assert!(ask(&three).is_empty());
    // Zwei Geschwister mit unterschiedlichen Eltern (Halbgeschwister): keine Frage.
    let half = [parent(1, 10), parent(2, 10), parent(1, 11), parent(3, 11)];
    assert!(ask(&half).is_empty());
}

#[test]
fn questions_are_bounded_and_ordered() {
    let mut edges = Vec::new();
    for k in 0..(MAX_SUGGESTIONS as i64 + 10) {
        let base = 1000 + k * 10;
        edges.extend([
            parent(base, base + 3),
            parent(base + 1, base + 3),
            parent(base, base + 4),
        ]);
    }
    let found = ask(&edges);
    assert_eq!(found.len(), MAX_SUGGESTIONS);
    let targets: Vec<_> = found.iter().map(|s| s.targets[0]).collect();
    let mut sorted = targets.clone();
    sorted.sort();
    assert_eq!(targets, sorted);
}
