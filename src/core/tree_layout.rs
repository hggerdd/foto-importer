//! Stammbaum-Layout um eine Fokus-Person (ADR 0061).
//!
//! Reine, deterministische Funktion ohne Zufall und ohne Oberfläche. Es wird nur
//! die Umgebung der Fokus-Person ausgewählt und angeordnet: Vorfahren darüber,
//! Geschwister, Partner und Fokus-Person auf einer Ebene, Nachkommen darunter.
//! Personen sind Kästchen der Breite 1; Paare bilden einen Block. Je Ebene wird die
//! Reihenfolge aus der Verwandtschaft bestimmt und die Lage iterativ so verschoben,
//! dass Eltern über ihren Kindern stehen. Jede Verschiebung wird auf Mindestabstände
//! projiziert (Isotonische Regression), Überlappung ist daher ausgeschlossen.
use super::kinship::Family;
use super::relations::Gender;
use std::collections::{BTreeMap, BTreeSet};

/// Mehr Personen werden nicht angeordnet (Rest steht in `truncated`).
pub const MAX_TREE_NODES: usize = 300;
/// Höchstzahl wählbarer Generationen je Richtung.
pub const MAX_TREE_GENERATIONS: u8 = 6;
/// Abstand der Mittelpunkte innerhalb eines Paarblocks.
pub const PARTNER_STEP: f32 = 1.12;
/// Zusätzlicher Freiraum zwischen Blöcken einer Ebene.
pub const BLOCK_GAP: f32 = 0.4;
const SWEEPS: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeRole {
    Focus,
    Ancestor,
    Sibling,
    Partner,
    FormerPartner,
    Descendant,
    DescendantPartner,
}

#[derive(Clone, Debug)]
pub struct TreeNode {
    pub person: i64,
    /// Mittelpunkt des Kästchens in Einheiten (Kästchenbreite 1), >= 0.5.
    pub x: f32,
    /// Ebene von oben (0 = oberste), Einheiten.
    pub y: f32,
    /// Generation relativ zur Fokus-Person (Vorfahren negativ).
    pub generation: i32,
    pub role: NodeRole,
    /// Weitere Eltern sind vorhanden, aber nicht dargestellt (Grenze der Generationen).
    pub more_up: bool,
    /// Weitere Kinder sind vorhanden, aber nicht dargestellt.
    pub more_down: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// Kind mit seinen dargestellten Eltern.
    Parents {
        child: i64,
        parents: Vec<i64>,
    },
    Partner {
        a: i64,
        b: i64,
    },
    FormerPartner {
        a: i64,
        b: i64,
    },
}

#[derive(Clone, Debug, Default)]
pub struct TreeLayout {
    pub nodes: Vec<TreeNode>,
    pub links: Vec<LinkKind>,
    pub width: f32,
    pub height: f32,
    /// Wegen [`MAX_TREE_NODES`] nicht dargestellte Personen.
    pub truncated: usize,
}

impl TreeLayout {
    pub fn node(&self, person: i64) -> Option<&TreeNode> {
        self.nodes.iter().find(|n| n.person == person)
    }
}

struct Block {
    members: Vec<i64>,
    center: f32,
}

impl Block {
    fn width(&self) -> f32 {
        (self.members.len() as f32 - 1.0) * PARTNER_STEP + 1.0
    }
    fn member_x(&self, index: usize) -> f32 {
        self.center + (index as f32 - (self.members.len() as f32 - 1.0) / 2.0) * PARTNER_STEP
    }
}

fn add(
    info: &mut BTreeMap<i64, (i32, NodeRole)>,
    order: &mut Vec<i64>,
    person: i64,
    generation: i32,
    role: NodeRole,
) {
    if let std::collections::btree_map::Entry::Vacant(e) = info.entry(person) {
        e.insert((generation, role));
        order.push(person);
    }
}

/// Wählt die Umgebung von `focus` und ordnet sie an. `up` und `down` zählen
/// Generationen (begrenzt auf [`MAX_TREE_GENERATIONS`]).
pub fn layout(family: &Family, focus: i64, up: u8, down: u8) -> TreeLayout {
    let up = up.min(MAX_TREE_GENERATIONS) as i32;
    let down = down.min(MAX_TREE_GENERATIONS) as i32;
    // ---- Auswahl in Prioritätsreihenfolge
    let mut order: Vec<i64> = Vec::new();
    let mut info: BTreeMap<i64, (i32, NodeRole)> = BTreeMap::new();
    add(&mut info, &mut order, focus, 0, NodeRole::Focus);
    for &p in family.partners_of(focus) {
        add(&mut info, &mut order, p, 0, NodeRole::Partner);
    }
    for &p in family.former_partners_of(focus) {
        add(&mut info, &mut order, p, 0, NodeRole::FormerPartner);
    }
    let mut level = vec![focus];
    for g in 1..=up {
        let mut next = Vec::new();
        for &person in &level {
            for &parent in family.parents_of(person) {
                if !info.contains_key(&parent) {
                    add(&mut info, &mut order, parent, -g, NodeRole::Ancestor);
                    next.push(parent);
                }
            }
        }
        level = next;
    }
    if up >= 1 {
        for &parent in family.parents_of(focus) {
            for &sibling in family.children_of(parent) {
                add(&mut info, &mut order, sibling, 0, NodeRole::Sibling);
            }
        }
    }
    let mut line = vec![focus];
    for g in 1..=down {
        let mut next = Vec::new();
        for &person in &line {
            for &child in family.children_of(person) {
                if !info.contains_key(&child) {
                    add(&mut info, &mut order, child, g, NodeRole::Descendant);
                    next.push(child);
                }
            }
        }
        for &child in &next {
            for &partner in family.partners_of(child) {
                add(
                    &mut info,
                    &mut order,
                    partner,
                    g,
                    NodeRole::DescendantPartner,
                );
            }
        }
        line = next;
    }
    let truncated = order.len().saturating_sub(MAX_TREE_NODES);
    order.truncate(MAX_TREE_NODES);
    let kept: BTreeSet<i64> = order.iter().copied().collect();
    info.retain(|k, _| kept.contains(k));

    // ---- Reihenfolge je Ebene
    let gen_of = |p: i64| info[&p].0;
    let mut rows: BTreeMap<i32, Vec<Block>> = BTreeMap::new();
    let gender_rank = |p: i64| match family.gender(p) {
        Gender::Male => 0,
        Gender::Female => 1,
        Gender::Unknown => 2,
    };
    // Ebene 0: Geschwister links und rechts der Fokus-Block, Fokus in der Mitte.
    let mut focus_block: Vec<i64> = family
        .former_partners_of(focus)
        .iter()
        .copied()
        .filter(|p| kept.contains(p))
        .collect();
    focus_block.push(focus);
    focus_block.extend(
        family
            .partners_of(focus)
            .iter()
            .copied()
            .filter(|p| kept.contains(p)),
    );
    let siblings: Vec<i64> = order
        .iter()
        .copied()
        .filter(|p| info[p].1 == NodeRole::Sibling)
        .collect();
    let half = siblings.len() / 2;
    let mut row0: Vec<Block> = Vec::new();
    for &s in &siblings[..half] {
        row0.push(Block {
            members: vec![s],
            center: 0.0,
        });
    }
    row0.push(Block {
        members: focus_block,
        center: 0.0,
    });
    for &s in &siblings[half..] {
        row0.push(Block {
            members: vec![s],
            center: 0.0,
        });
    }
    rows.insert(0, row0);
    // Vorfahren: Eltern in der Reihenfolge der Kinder darunter, je Kind ein Block.
    let min_gen = info.values().map(|(g, _)| *g).min().unwrap_or(0);
    let mut placed: BTreeSet<i64> = BTreeSet::new();
    for g in (min_gen..0).rev() {
        let below = rows.get(&(g + 1)).map_or(&[][..], |r| r.as_slice());
        let mut blocks: Vec<Block> = Vec::new();
        // Zuerst die Linie zur Fokus-Person (damit ihre Eltern ein Paarblock bleiben),
        // dann alle übrigen in der Reihenfolge der Ebene darunter.
        let members: Vec<i64> = below
            .iter()
            .flat_map(|b| b.members.iter().copied())
            .collect();
        let lineage = |p: &i64| matches!(info[p].1, NodeRole::Focus | NodeRole::Ancestor);
        let first = members.iter().copied().filter(|p| lineage(p));
        let rest_members = members.iter().copied().filter(|p| !lineage(p));
        for member in first.chain(rest_members) {
            let mut parents: Vec<i64> = family
                .parents_of(member)
                .iter()
                .copied()
                .filter(|p| kept.contains(p) && gen_of(*p) == g && !placed.contains(p))
                .collect();
            parents.sort_by_key(|p| (gender_rank(*p), *p));
            if parents.is_empty() {
                continue;
            }
            placed.extend(parents.iter().copied());
            blocks.push(Block {
                members: parents,
                center: 0.0,
            });
        }
        // Übrige Vorfahren dieser Ebene (z. B. über Pfade außerhalb der Reihenfolge).
        let rest: Vec<i64> = order
            .iter()
            .copied()
            .filter(|p| info[p].0 == g && !placed.contains(p))
            .collect();
        for p in rest {
            placed.insert(p);
            blocks.push(Block {
                members: vec![p],
                center: 0.0,
            });
        }
        rows.insert(g, blocks);
    }
    // Nachkommen: Kinder (mit Partnern) gruppiert nach ihrem anderen Elternteil.
    let mut position: BTreeMap<i64, usize> = BTreeMap::new();
    let index_row = |blocks: &[Block], position: &mut BTreeMap<i64, usize>| {
        for (i, m) in blocks.iter().flat_map(|b| b.members.iter()).enumerate() {
            position.insert(*m, i);
        }
    };
    index_row(&rows[&0], &mut position);
    let mut claimed: BTreeSet<i64> = BTreeSet::new();
    for g in 1..=down {
        let above = rows.get(&(g - 1)).map_or(&[][..], |r| r.as_slice());
        let mut blocks: Vec<Block> = Vec::new();
        let mut kids: Vec<(usize, usize, i64)> = Vec::new();
        for member in above.iter().flat_map(|b| b.members.iter().copied()) {
            let role = info[&member].1;
            if !matches!(role, NodeRole::Focus | NodeRole::Descendant) {
                continue;
            }
            for &child in family.children_of(member) {
                if kept.contains(&child)
                    && gen_of(child) == g
                    && info[&child].1 == NodeRole::Descendant
                    && claimed.insert(child)
                {
                    let other = family
                        .parents_of(child)
                        .iter()
                        .filter(|p| **p != member)
                        .filter_map(|p| position.get(p).copied())
                        .min()
                        .unwrap_or(usize::MAX);
                    kids.push((position.get(&member).copied().unwrap_or(0), other, child));
                }
            }
        }
        // Gruppiert nach (Elternteil in der Ebene darüber, anderer Elternteil), dann ID.
        kids.sort_by_key(|(a, b, c)| (*a, *b, *c));
        for (_, _, child) in kids {
            let mut members: Vec<i64> = family
                .former_partners_of(child)
                .iter()
                .copied()
                .filter(|p| kept.contains(p) && info[p].1 == NodeRole::DescendantPartner)
                .collect();
            members.push(child);
            members.extend(
                family
                    .partners_of(child)
                    .iter()
                    .copied()
                    .filter(|p| kept.contains(p) && info[p].1 == NodeRole::DescendantPartner),
            );
            // Ein Partner steht nur in einem Block.
            members.retain(|m| *m == child || claimed.insert(*m));
            blocks.push(Block {
                members,
                center: 0.0,
            });
        }
        if blocks.is_empty() {
            break;
        }
        index_row(&blocks, &mut position);
        rows.insert(g, blocks);
    }

    // ---- Anfangslage: jede Ebene dicht gepackt, um 0 zentriert
    for blocks in rows.values_mut() {
        pack(blocks);
    }
    // ---- Iterative Annäherung an die Mittelwerte der Nachbarn, jeweils projiziert
    let keys: Vec<i32> = rows.keys().copied().collect();
    for sweep in 0..SWEEPS {
        let sequence: Vec<i32> = if sweep % 2 == 0 {
            keys.clone()
        } else {
            keys.iter().rev().copied().collect()
        };
        for g in sequence {
            // Lage aller Personen der Nachbarebenen.
            let mut xs: BTreeMap<i64, f32> = BTreeMap::new();
            for neighbor in [g - 1, g + 1] {
                if let Some(blocks) = rows.get(&neighbor) {
                    for b in blocks {
                        for (i, m) in b.members.iter().enumerate() {
                            xs.insert(*m, b.member_x(i));
                        }
                    }
                }
            }
            let Some(blocks) = rows.get_mut(&g) else {
                continue;
            };
            let desired: Vec<f32> = blocks
                .iter()
                .map(|b| {
                    let mut sum = 0.0f32;
                    let mut n = 0.0f32;
                    for (i, m) in b.members.iter().enumerate() {
                        let offset = b.member_x(i) - b.center;
                        let links = family
                            .parents_of(*m)
                            .iter()
                            .chain(family.children_of(*m).iter())
                            .copied();
                        for other in links {
                            if let Some(x) = xs.get(&other) {
                                sum += x - offset;
                                n += 1.0;
                            }
                        }
                    }
                    if n > 0.0 { sum / n } else { b.center }
                })
                .collect();
            project(blocks, &desired);
        }
    }
    // ---- Ausgabe: linke Kante der gesamten Zeichnung bei 0
    let shown: BTreeSet<i64> = rows
        .values()
        .flat_map(|blocks| blocks.iter().flat_map(|b| b.members.iter().copied()))
        .collect();
    let left = rows
        .values()
        .flat_map(|blocks| blocks.iter().map(|b| b.center - b.width() / 2.0))
        .fold(f32::INFINITY, f32::min);
    let left = if left.is_finite() { left } else { 0.0 };
    let mut nodes = Vec::new();
    let mut right = 0.0f32;
    for (g, blocks) in &rows {
        for b in blocks {
            for (i, &person) in b.members.iter().enumerate() {
                let x = b.member_x(i) - left;
                right = right.max(x + 0.5);
                let (generation, role) = info[&person];
                nodes.push(TreeNode {
                    person,
                    x,
                    y: (*g - min_gen) as f32,
                    generation,
                    role,
                    more_up: matches!(role, NodeRole::Focus | NodeRole::Ancestor)
                        && family.parents_of(person).iter().any(|p| !shown.contains(p)),
                    more_down: matches!(role, NodeRole::Focus | NodeRole::Descendant)
                        && family
                            .children_of(person)
                            .iter()
                            .any(|c| !shown.contains(c)),
                });
            }
        }
    }
    nodes.sort_by_key(|n| (n.generation, n.person));
    let max_gen = info.values().map(|(g, _)| *g).max().unwrap_or(0);
    let mut links = Vec::new();
    for &person in order.iter().filter(|p| shown.contains(p)) {
        let parents: Vec<i64> = family
            .parents_of(person)
            .iter()
            .copied()
            .filter(|p| shown.contains(p) && info[p].0 == info[&person].0 - 1)
            .collect();
        if !parents.is_empty() {
            links.push(LinkKind::Parents {
                child: person,
                parents,
            });
        }
        for &partner in family.partners_of(person).iter().filter(|p| **p > person) {
            if shown.contains(&partner) && info[&partner].0 == info[&person].0 {
                links.push(LinkKind::Partner {
                    a: person,
                    b: partner,
                });
            }
        }
        for &partner in family
            .former_partners_of(person)
            .iter()
            .filter(|p| **p > person)
        {
            if shown.contains(&partner) && info[&partner].0 == info[&person].0 {
                links.push(LinkKind::FormerPartner {
                    a: person,
                    b: partner,
                });
            }
        }
    }
    TreeLayout {
        nodes,
        links,
        width: right,
        height: (max_gen - min_gen + 1) as f32,
        truncated,
    }
}

/// Blöcke einer Ebene dicht nebeneinander setzen, die Ebene um 0 zentrieren.
fn pack(blocks: &mut [Block]) {
    let mut at = 0.0f32;
    for (i, b) in blocks.iter_mut().enumerate() {
        if i > 0 {
            at += BLOCK_GAP;
        }
        b.center = at + b.width() / 2.0;
        at += b.width();
    }
    let shift = at / 2.0;
    for b in blocks.iter_mut() {
        b.center -= shift;
    }
}

/// Setzt die Blockmitten möglichst nahe an `desired`, ohne dass sich Blöcke
/// überlappen oder ihre Reihenfolge ändern (Pool-Adjacent-Violators).
fn project(blocks: &mut [Block], desired: &[f32]) {
    let n = blocks.len();
    if n == 0 {
        return;
    }
    // s_i: Mindestlage der Mitte i relativ zu Block 0.
    let mut offset = vec![0.0f32; n];
    for i in 1..n {
        offset[i] = offset[i - 1] + (blocks[i - 1].width() + blocks[i].width()) / 2.0 + BLOCK_GAP;
    }
    // Gesucht: z_i = c_i - s_i nichtfallend, nahe an d_i - s_i.
    let mut stack: Vec<(f32, f32)> = Vec::new(); // (Summe, Anzahl)
    for i in 0..n {
        stack.push((desired[i] - offset[i], 1.0));
        while stack.len() > 1 {
            let (s2, n2) = stack[stack.len() - 1];
            let (s1, n1) = stack[stack.len() - 2];
            if s1 / n1 > s2 / n2 {
                stack.truncate(stack.len() - 2);
                stack.push((s1 + s2, n1 + n2));
            } else {
                break;
            }
        }
    }
    let mut i = 0;
    for (sum, count) in stack {
        let value = sum / count;
        for _ in 0..count as usize {
            blocks[i].center = value + offset[i];
            i += 1;
        }
    }
}
