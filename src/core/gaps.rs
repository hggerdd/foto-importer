//! Rückfragen zu wahrscheinlich fehlenden Eltern-Kanten und Freundschaften (ADR 0061).
//!
//! Die App ergänzt nie stillschweigend. Sie findet Lücken und stellt eine Frage;
//! die Antwort wird gespeichert: "Ja" legt die Kanten an, "Nein" speichert eine
//! Ablehnung je Paar in `face_settings` (keine Schemaänderung), damit dieselbe Frage
//! nie wiederkehrt. Freundschaften: Ist Z mit X befreundet und X hat einen Partner Y,
//! wird gefragt, ob Y auch mit Z befreundet ist.
use super::collection::Collection;
use super::kinship::Family;
use super::relations::{self, Edge, Kind, Role, validate_add};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

/// Höchstzahl gleichzeitig angezeigter Rückfragen.
pub const MAX_SUGGESTIONS: usize = 30;
const SETTING_KEY: &str = "family.declined_parents";
const FRIENDS_KEY: &str = "family.declined_friends";

/// Gespeicherte Nutzerentscheidungen: (Kind, Elternteil) ist ausdrücklich abgelehnt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Decisions {
    pub declined: BTreeSet<(i64, i64)>,
    /// Ausdrücklich verneinte Freundschaften als Paar (kleinere ID, größere ID).
    pub declined_friends: BTreeSet<(i64, i64)>,
}

impl Decisions {
    fn encode(&self) -> String {
        self.declined
            .iter()
            .map(|(child, parent)| format!("{child}:{parent}"))
            .collect::<Vec<_>>()
            .join(";")
    }
    fn decode(text: &str) -> Decisions {
        Decisions {
            declined: decode_pairs(text),
            ..Default::default()
        }
    }
}

fn decode_pairs(text: &str) -> BTreeSet<(i64, i64)> {
    text.split(';')
        .filter_map(|pair| {
            let (first, second) = pair.split_once(':')?;
            Some((first.parse().ok()?, second.parse().ok()?))
        })
        .collect()
}

fn encode_pairs(pairs: &BTreeSet<(i64, i64)>) -> String {
    pairs
        .iter()
        .map(|(first, second)| format!("{first}:{second}"))
        .collect::<Vec<_>>()
        .join(";")
}

fn friend_pair(a: i64, b: i64) -> (i64, i64) {
    (a.min(b), a.max(b))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SuggestionKind {
    /// Geschwister A und B; A hat Eltern, die für B fehlen.
    SiblingParents,
    /// C hat Partner D und Kinder, D hat keine eigenen Kinder: ist D Elternteil?
    PartnerParent,
    /// Z ist mit X befreundet, Y ist Partner von X: ist Y auch mit Z befreundet?
    /// (`candidates` = Y, `targets` = Z, `via` = X.)
    FriendOfPartner,
}

/// Eine Rückfrage: "Sind `candidates` auch Eltern von `targets`?"
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    pub candidates: Vec<i64>,
    pub targets: Vec<i64>,
    /// Auslösende Personen (Geschwister bzw. der Elternteil C).
    pub via: Vec<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Alle vorgeschlagenen Eltern anlegen.
    All,
    /// Nur dieser Elternteil ist gemeinsam (Halbgeschwister); die anderen werden abgelehnt.
    Only(i64),
    /// Keiner ist Elternteil; alle Paare werden abgelehnt.
    None,
}

/// Was eine Antwort bewirkt: Eltern-Kanten (Elternteil, Kind) und Ablehnungen (Kind, Elternteil).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Kanten: (Elternteil, Kind) bzw. bei Freundschaften (Person, Person).
    pub add: Vec<(i64, i64)>,
    pub decline: Vec<(i64, i64)>,
    /// Es geht um Freundschaften statt um Eltern-Kanten.
    pub friend: bool,
}

pub fn plan(suggestion: &Suggestion, answer: Answer) -> Plan {
    let mut plan = Plan {
        add: vec![],
        decline: vec![],
        friend: suggestion.kind == SuggestionKind::FriendOfPartner,
    };
    for &child in &suggestion.targets {
        for &parent in &suggestion.candidates {
            let add = match answer {
                Answer::All => true,
                Answer::Only(only) => parent == only,
                Answer::None => false,
            };
            if add {
                plan.add.push((parent, child));
            } else {
                plan.decline.push((child, parent));
            }
        }
    }
    plan
}

/// Reine Funktion: alle Rückfragen zu einem Baum, geordnet und begrenzt. Nie
/// vorgeschlagen werden bereits entschiedene Paare und alles, was `validate_add`
/// ablehnen würde (Zyklen, Selbstbezug, Widersprüche).
pub fn suggestions(edges: &[Edge], decisions: &Decisions) -> Vec<Suggestion> {
    let family = Family::new(edges, &BTreeMap::new());
    let allowed = |parent: i64, child: i64| {
        !decisions.declined.contains(&(child, parent))
            && matches!(
                validate_add(edges, relations::canonical(parent, Role::Parent, child)),
                Ok(true)
            )
    };
    let mut out: Vec<Suggestion> = Vec::new();
    // 1. Geschwister mit unterschiedlich vollständigen Eltern.
    let people = family.people();
    let mut siblings_of: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
    for &person in &people {
        for &parent in family.parents_of(person) {
            for &other in family.children_of(parent) {
                if other != person {
                    siblings_of.entry(person).or_default().insert(other);
                }
            }
        }
    }
    for e in edges.iter().filter(|e| e.kind == Kind::Sibling) {
        siblings_of.entry(e.from).or_default().insert(e.to);
        siblings_of.entry(e.to).or_default().insert(e.from);
    }
    let mut grouped: BTreeMap<(i64, Vec<i64>), BTreeSet<i64>> = BTreeMap::new();
    for (&b, others) in &siblings_of {
        let mine = family.parents_of(b);
        if mine.len() >= 2 {
            continue;
        }
        for &a in others {
            let theirs = family.parents_of(a);
            if !mine.is_empty() && mine.intersection(theirs).next().is_none() {
                continue;
            }
            let missing: Vec<i64> = theirs
                .difference(mine)
                .copied()
                .filter(|&parent| allowed(parent, b))
                .collect();
            if !missing.is_empty() && mine.len() + missing.len() <= 2 {
                grouped.entry((b, missing)).or_default().insert(a);
            }
        }
    }
    for ((target, candidates), via) in grouped {
        out.push(Suggestion {
            kind: SuggestionKind::SiblingParents,
            candidates,
            targets: vec![target],
            via: via.into_iter().take(3).collect(),
        });
    }
    // 2. Partner eines Elternteils ohne eigene Kinder.
    let mut by_partner: BTreeMap<(i64, i64), Vec<i64>> = BTreeMap::new();
    for &c in &people {
        let partners = family.partners_of(c);
        if partners.len() != 1 {
            continue;
        }
        let d = *partners.iter().next().unwrap_or(&c);
        if !family.children_of(d).is_empty() {
            continue;
        }
        for &x in family.children_of(c) {
            if family.parents_of(x).len() == 1 && allowed(d, x) {
                by_partner.entry((c, d)).or_default().push(x);
            }
        }
    }
    for ((c, d), targets) in by_partner {
        out.push(Suggestion {
            kind: SuggestionKind::PartnerParent,
            candidates: vec![d],
            targets,
            via: vec![c],
        });
    }
    // 3. Freunde von Partnern: Z ist Freund von X, Y ist Partner von X.
    let mut friends_of: BTreeMap<(i64, i64), BTreeSet<i64>> = BTreeMap::new();
    for e in edges.iter().filter(|e| e.kind == Kind::Friend) {
        for (x, z) in [(e.from, e.to), (e.to, e.from)] {
            for &y in family.partners_of(x) {
                if y != z
                    && !decisions.declined_friends.contains(&friend_pair(y, z))
                    && matches!(
                        validate_add(edges, relations::canonical(y, Role::Friend, z)),
                        Ok(true)
                    )
                {
                    friends_of.entry((y, z)).or_default().insert(x);
                }
            }
        }
    }
    for ((partner, friend), via) in friends_of {
        out.push(Suggestion {
            kind: SuggestionKind::FriendOfPartner,
            candidates: vec![partner],
            targets: vec![friend],
            via: via.into_iter().collect(),
        });
    }
    out.sort_by(|x, y| {
        (x.kind, &x.targets, &x.candidates).cmp(&(y.kind, &y.targets, &y.candidates))
    });
    out.truncate(MAX_SUGGESTIONS);
    out
}

pub fn load_decisions(collection: &Collection) -> Result<Decisions> {
    let mut decisions = super::faces::setting(collection, SETTING_KEY)?
        .map(|text| Decisions::decode(&text))
        .unwrap_or_default();
    decisions.declined_friends = super::faces::setting(collection, FRIENDS_KEY)?
        .map(|text| decode_pairs(&text))
        .unwrap_or_default();
    Ok(decisions)
}

fn save_decisions(collection: &Collection, decisions: &Decisions) -> Result<()> {
    super::faces::save_setting(&collection.db, SETTING_KEY, &decisions.encode())?;
    super::faces::save_setting(
        &collection.db,
        FRIENDS_KEY,
        &encode_pairs(&decisions.declined_friends),
    )
}

/// Wendet eine Antwort an: legt Eltern-Kanten an (mit voller Prüfung) und speichert
/// Ablehnungen. Schlägt eine Kante fehl, wird nichts abgelehnt gespeichert.
pub fn apply(collection: &mut Collection, suggestion: &Suggestion, answer: Answer) -> Result<()> {
    collection.writable()?;
    let plan = plan(suggestion, answer);
    let role = if plan.friend {
        Role::Friend
    } else {
        Role::Parent
    };
    for (subject, object) in &plan.add {
        relations::add(collection, *subject, role, *object)?;
    }
    if !plan.decline.is_empty() {
        let mut decisions = load_decisions(collection)?;
        if plan.friend {
            decisions
                .declined_friends
                .extend(plan.decline.iter().map(|(a, b)| friend_pair(*a, *b)));
        } else {
            decisions.declined.extend(plan.decline);
        }
        save_decisions(collection, &decisions)?;
    }
    Ok(())
}
