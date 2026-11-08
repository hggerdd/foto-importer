//! Verwandtschaftsrechner (ADR 0061).
//!
//! Gespeichert werden nur Basisfakten: "A ist Elternteil von B" und "A und B sind
//! Partner" (plus Freund/Bekannter). Alles andere wird hier aus dem Baum berechnet:
//! Blutsverwandtschaft über den nächsten gemeinsamen Vorfahren (Schritte aufwärts,
//! Schritte abwärts), Verschwägerung über genau einen Partnerschritt und
//! Stiefgeschwister über Elternpartner. Reine Funktionen ohne Datenbankzugriff.
use super::relations::{Edge, Gender, Kind};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Höchstzahl Generationen je Richtung (aufwärts zum gemeinsamen Vorfahren und abwärts).
/// Cousins bis zum 7. Grades sind damit auffindbar.
pub const MAX_GENERATIONS: u8 = 8;
/// Höchstzahl Einträge je Gruppe in [`Family::relatives_of`]; der Rest steht in `more`.
pub const MAX_PER_GROUP: usize = 100;
/// Verschwägerung wird nur über nahe Blutsverwandte berechnet (Summe der Schritte).
const AFFINITY_MAX_STEPS: u8 = 4;

/// Berechnete Beziehung "a ist <Relation> von b". Nichts davon wird gespeichert.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Relation {
    Partner,
    FormerPartner,
    /// Blutsverwandt: `a_gen` Generationen unter dem gemeinsamen Vorfahren ist a,
    /// `b_gen` ist b. Kind von b: (1, 0); Großmutter von b: (0, 2); Geschwister: (1, 1);
    /// Onkel: (1, 2); Cousin 1. Grades: (2, 2). `half`: nur ein gemeinsamer Elternteil
    /// (nur bei Geschwistern, nur wenn beide Seiten einen weiteren Elternteil haben).
    Blood {
        a_gen: u8,
        b_gen: u8,
        half: bool,
    },
    /// Eltern eines Partners sind verschieden von denen der Person, aber ein Elternteil
    /// der einen Seite ist Partner eines Elternteils der anderen.
    StepSibling,
    /// a ist Blutsverwandter (`a_gen`, `p_gen`) des Partners p von b.
    ThroughPartnerOfB {
        a_gen: u8,
        p_gen: u8,
    },
    /// Der Partner p von a ist Blutsverwandter (`p_gen`, `b_gen`) von b.
    ThroughPartnerOfA {
        p_gen: u8,
        b_gen: u8,
    },
    /// Direkt erfasste Behelfsverbindung (Altzeile), nicht aus dem Baum ableitbar.
    RecordedSibling,
    RecordedCousin,
}

fn pick(g: Gender, f: &str, m: &str, n: &str) -> String {
    match g {
        Gender::Female => f,
        Gender::Male => m,
        Gender::Unknown => n,
    }
    .to_string()
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// "", "Groß", "Urgroß", "Ururgroß" ... für `k` Stufen oberhalb der einfachen Form.
fn great(k: u8) -> String {
    match k {
        0 => String::new(),
        k => format!("{}groß", urs(k - 1)),
    }
}

/// "", "Ur", "Urur", "Ururur" ...
fn urs(count: u8) -> String {
    match count {
        0 => String::new(),
        n => format!("Ur{}", "ur".repeat(n as usize - 1)),
    }
}

/// Bezeichnung für eine Blutsverwandtschaft aus Sicht von a ("a ist ...").
pub fn blood_label(a_gen: u8, b_gen: u8, half: bool, a: Gender) -> String {
    match (a_gen, b_gen) {
        (0, 0) => String::new(),
        (1, 0) => pick(a, "Tochter", "Sohn", "Kind"),
        (n, 0) if n <= 5 => {
            let base = format!("{}enkel", urs(n - 2));
            let base = capitalize(&base);
            match a {
                Gender::Female => format!("{base}in"),
                Gender::Male => base,
                Gender::Unknown => format!("{base}kind"),
            }
        }
        (n, 0) => pick(
            a,
            &format!("Nachfahrin in {n}. Generation"),
            &format!("Nachfahre in {n}. Generation"),
            &format!("Nachfahr/in in {n}. Generation"),
        ),
        (0, 1) => pick(a, "Mutter", "Vater", "Elternteil"),
        (0, n) if n <= 5 => {
            let g = great(n - 1);
            pick(
                a,
                &format!("{g}mutter"),
                &format!("{g}vater"),
                &format!("{g}elternteil"),
            )
            .pipe_capitalize()
        }
        (0, n) => pick(
            a,
            &format!("Vorfahrin in {n}. Generation"),
            &format!("Vorfahre in {n}. Generation"),
            &format!("Vorfahr/in in {n}. Generation"),
        ),
        (1, 1) if half => pick(a, "Halbschwester", "Halbbruder", "Halbgeschwisterteil"),
        (1, 1) => pick(a, "Schwester", "Bruder", "Geschwisterteil"),
        (1, d) if d <= 5 => {
            let g = great(d - 2);
            pick(
                a,
                &format!("{g}tante"),
                &format!("{g}onkel"),
                &if g.is_empty() {
                    "Onkel/Tante".to_string()
                } else {
                    format!("{g}onkel/-tante")
                },
            )
            .pipe_capitalize()
        }
        (1, d) => pick(
            a,
            &format!("Urgroßtante ({d} Generationen älter)"),
            &format!("Urgroßonkel ({d} Generationen älter)"),
            &format!("Urgroßonkel/-tante ({d} Generationen älter)"),
        ),
        (n, 1) if n <= 5 => {
            let g = great(n - 2);
            pick(
                a,
                &format!("{g}nichte"),
                &format!("{g}neffe"),
                &if g.is_empty() {
                    "Nichte/Neffe".to_string()
                } else {
                    format!("{g}nichte/-neffe")
                },
            )
            .pipe_capitalize()
        }
        (n, 1) => pick(
            a,
            &format!("Urgroßnichte ({n} Generationen jünger)"),
            &format!("Urgroßneffe ({n} Generationen jünger)"),
            &format!("Urgroßnichte/-neffe ({n} Generationen jünger)"),
        ),
        (x, y) => {
            let degree = x.min(y) - 1;
            let removed = x.abs_diff(y);
            let base = pick(a, "Cousine", "Cousin", "Cousin/Cousine");
            if removed == 0 {
                format!("{base} {degree}. Grades")
            } else {
                format!("{base} {degree}. Grades, {removed}x entfernt")
            }
        }
    }
}

trait PipeCapitalize {
    fn pipe_capitalize(self) -> String;
}
impl PipeCapitalize for String {
    fn pipe_capitalize(self) -> String {
        capitalize(&self)
    }
}

/// Mehrzahl-Überschrift für eine Blutsverwandtschaft (Gruppenname).
pub fn blood_group_title(a_gen: u8, b_gen: u8, half: bool) -> String {
    match (a_gen, b_gen) {
        (1, 0) => "Kinder".into(),
        (n, 0) if n <= 5 => format!("{}enkel", urs(n - 2)).pipe_capitalize(),
        (n, 0) => format!("Nachfahren in {n}. Generation"),
        (0, 1) => "Eltern".into(),
        (0, n) if n <= 5 => format!("{}eltern", great(n - 1)).pipe_capitalize(),
        (0, n) => format!("Vorfahren in {n}. Generation"),
        (1, 1) if half => "Halbgeschwister".into(),
        (1, 1) => "Geschwister".into(),
        (1, d) if d <= 5 => {
            let g = great(d - 2);
            format!(
                "{} und {}",
                capitalize(&format!("{g}onkel")),
                capitalize(&format!("{g}tanten"))
            )
        }
        (1, d) => format!("Urgroßonkel und -tanten ({d} Generationen älter)"),
        (n, 1) if n <= 5 => {
            let g = great(n - 2);
            format!(
                "{} und {}",
                capitalize(&format!("{g}nichten")),
                capitalize(&format!("{g}neffen"))
            )
        }
        (n, 1) => format!("Urgroßnichten und -neffen ({n} Generationen jünger)"),
        (x, y) => {
            let degree = x.min(y) - 1;
            let removed = x.abs_diff(y);
            if removed == 0 {
                format!("Cousins und Cousinen {degree}. Grades")
            } else {
                let side = if x < y { "ältere" } else { "jüngere" };
                format!(
                    "Cousins und Cousinen {degree}. Grades, {removed}x entfernt ({side} Generation)"
                )
            }
        }
    }
}

impl Relation {
    /// Gleiche Bezeichnung in beide Richtungen (Cousins, Geschwister, Partner ...).
    pub fn symmetric(self) -> bool {
        match self {
            Relation::Partner
            | Relation::FormerPartner
            | Relation::StepSibling
            | Relation::RecordedSibling
            | Relation::RecordedCousin => true,
            Relation::Blood { a_gen, b_gen, .. } => a_gen == b_gen,
            Relation::ThroughPartnerOfB { a_gen, p_gen } => a_gen == 1 && p_gen == 1,
            Relation::ThroughPartnerOfA { p_gen, b_gen } => p_gen == 1 && b_gen == 1,
        }
    }

    /// Die Beziehung aus Sicht der anderen Person: relationship(b, a).
    pub fn inverse(self) -> Relation {
        match self {
            Relation::Blood { a_gen, b_gen, half } => Relation::Blood {
                a_gen: b_gen,
                b_gen: a_gen,
                half,
            },
            Relation::ThroughPartnerOfB { a_gen, p_gen } => Relation::ThroughPartnerOfA {
                p_gen,
                b_gen: a_gen,
            },
            Relation::ThroughPartnerOfA { p_gen, b_gen } => Relation::ThroughPartnerOfB {
                a_gen: b_gen,
                p_gen,
            },
            other => other,
        }
    }

    /// Bezeichnung für a (Geschlecht von a): "a ist <Bezeichnung> von b".
    pub fn label(self, a: Gender) -> String {
        match self {
            Relation::Partner => pick(a, "Partnerin", "Partner", "Partner/in"),
            Relation::FormerPartner => pick(a, "Ex-Partnerin", "Ex-Partner", "Ex-Partner/in"),
            Relation::Blood { a_gen, b_gen, half } => blood_label(a_gen, b_gen, half, a),
            Relation::StepSibling => {
                pick(a, "Stiefschwester", "Stiefbruder", "Stiefgeschwisterteil")
            }
            Relation::RecordedSibling => {
                format!("{} (direkt erfasst)", blood_label(1, 1, false, a))
            }
            Relation::RecordedCousin => {
                format!(
                    "{} (direkt erfasst)",
                    pick(a, "Cousine", "Cousin", "Cousin/Cousine")
                )
            }
            Relation::ThroughPartnerOfB { a_gen: 0, p_gen: 1 } => pick(
                a,
                "Schwiegermutter",
                "Schwiegervater",
                "Schwiegerelternteil",
            ),
            Relation::ThroughPartnerOfB { a_gen: 1, p_gen: 0 } => {
                pick(a, "Stieftochter", "Stiefsohn", "Stiefkind")
            }
            Relation::ThroughPartnerOfA { p_gen: 0, b_gen: 1 } => {
                pick(a, "Stiefmutter", "Stiefvater", "Stiefelternteil")
            }
            Relation::ThroughPartnerOfA { p_gen: 1, b_gen: 0 } => {
                pick(a, "Schwiegertochter", "Schwiegersohn", "Schwiegerkind")
            }
            Relation::ThroughPartnerOfB { a_gen: 1, p_gen: 1 }
            | Relation::ThroughPartnerOfA { p_gen: 1, b_gen: 1 } => {
                pick(a, "Schwägerin", "Schwager", "Schwager/Schwägerin")
            }
            Relation::ThroughPartnerOfB { a_gen, p_gen } => {
                format!("{} (angeheiratet)", blood_label(a_gen, p_gen, false, a))
            }
            Relation::ThroughPartnerOfA { p_gen, b_gen } => {
                format!("{} (angeheiratet)", blood_label(p_gen, b_gen, false, a))
            }
        }
    }

    /// Mehrzahl für "a und b sind ..." (nur bei symmetrischen Beziehungen).
    pub fn plural(self) -> Option<String> {
        if !self.symmetric() {
            return None;
        }
        Some(match self {
            Relation::Partner => "Partner".into(),
            Relation::FormerPartner => "frühere Partner".into(),
            Relation::StepSibling => "Stiefgeschwister".into(),
            Relation::RecordedSibling => "Geschwister (direkt erfasst)".into(),
            Relation::RecordedCousin => "Cousins (direkt erfasst)".into(),
            Relation::Blood {
                a_gen: 1,
                b_gen: 1,
                half,
            } => if half {
                "Halbgeschwister"
            } else {
                "Geschwister"
            }
            .into(),
            Relation::Blood { a_gen, .. } => {
                format!("Cousins {}. Grades", a_gen - 1)
            }
            _ => "Schwäger".into(),
        })
    }

    /// Überschrift der Gruppe in der gruppierten Übersicht.
    pub fn group_title(self) -> String {
        match self {
            Relation::Partner => "Partner".into(),
            Relation::FormerPartner => "Frühere Partner".into(),
            Relation::Blood { a_gen, b_gen, half } => blood_group_title(a_gen, b_gen, half),
            Relation::StepSibling => "Stiefgeschwister".into(),
            Relation::RecordedSibling => "Geschwister (direkt erfasst)".into(),
            Relation::RecordedCousin => "Cousins und Cousinen (direkt erfasst)".into(),
            Relation::ThroughPartnerOfB { a_gen: 0, p_gen: 1 } => "Schwiegereltern".into(),
            Relation::ThroughPartnerOfB { a_gen: 1, p_gen: 0 } => "Stiefkinder".into(),
            Relation::ThroughPartnerOfA { p_gen: 0, b_gen: 1 } => "Stiefeltern".into(),
            Relation::ThroughPartnerOfA { p_gen: 1, b_gen: 0 } => "Schwiegerkinder".into(),
            Relation::ThroughPartnerOfB { a_gen: 1, p_gen: 1 }
            | Relation::ThroughPartnerOfA { p_gen: 1, b_gen: 1 } => {
                "Schwäger und Schwägerinnen".into()
            }
            Relation::ThroughPartnerOfB { a_gen, p_gen } => {
                format!("{} (angeheiratet)", blood_group_title(a_gen, p_gen, false))
            }
            Relation::ThroughPartnerOfA { p_gen, b_gen } => {
                format!("{} (angeheiratet)", blood_group_title(p_gen, b_gen, false))
            }
        }
    }

    /// Sortierschlüssel der Gruppen: Partner, Blutsverwandte nach Abstand (Vorfahren
    /// zuerst), Stief, direkt Erfasste, Angeheiratete.
    fn order(self) -> Order {
        match self {
            Relation::Partner => (0, 0, 0, 0),
            Relation::FormerPartner => (0, 1, 0, 0),
            Relation::Blood { a_gen, b_gen, half } => (1, a_gen + b_gen, a_gen, half as u8),
            Relation::StepSibling => (2, 0, 0, 0),
            Relation::RecordedSibling => (3, 0, 0, 0),
            Relation::RecordedCousin => (3, 1, 0, 0),
            Relation::ThroughPartnerOfB { a_gen, p_gen } => (4, a_gen + p_gen, a_gen, 0),
            Relation::ThroughPartnerOfA { p_gen, b_gen } => (4, p_gen + b_gen, p_gen, 1),
        }
    }

    /// Blutsverwandtschaft der Cousinenart (beide mindestens zwei Generationen unter dem Vorfahren).
    pub fn is_cousin(self) -> bool {
        matches!(self, Relation::Blood { a_gen, b_gen, .. } if a_gen >= 2 && b_gen >= 2)
    }
}

/// Eine Beziehung samt Weg. `path` beginnt bei a und endet bei b.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub relation: Relation,
    pub path: Vec<i64>,
}

/// Mitglied einer Gruppe von Verwandten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub person: i64,
    /// "person ist <relation> von der Bezugsperson".
    pub relation: Relation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub title: String,
    pub members: Vec<Member>,
    /// Weitere Treffer, die wegen [`MAX_PER_GROUP`] nicht aufgeführt sind.
    pub more: usize,
}

/// Der Stammbaum: Eltern-Kind-Kanten und Partner, dazu Geschlechter. Billig aufzubauen,
/// danach unveränderlich.
#[derive(Clone, Debug, Default)]
pub struct Family {
    parents: BTreeMap<i64, BTreeSet<i64>>,
    children: BTreeMap<i64, BTreeSet<i64>>,
    partners: BTreeMap<i64, BTreeSet<i64>>,
    former: BTreeMap<i64, BTreeSet<i64>>,
    recorded_sibling: BTreeMap<i64, BTreeSet<i64>>,
    recorded_cousin: BTreeMap<i64, BTreeSet<i64>>,
    genders: BTreeMap<i64, Gender>,
    /// Ausdrückliche Entscheidungen "Elternteil von Kind ist es nicht" (Kind, Elternteil).
    declined: BTreeSet<(i64, i64)>,
    empty: BTreeSet<i64>,
}

/// Sortierschlüssel einer Gruppe (siehe [`Relation::order`]).
type Order = (u8, u8, u8, u8);

/// Aufwärts-Karte: Person -> (Abstand, Vorgänger Richtung Startperson).
type Ancestry = BTreeMap<i64, (u8, i64)>;

impl Family {
    pub fn new(edges: &[Edge], genders: &BTreeMap<i64, Gender>) -> Family {
        let mut f = Family {
            genders: genders.clone(),
            ..Default::default()
        };
        for e in edges {
            let both = |m: &mut BTreeMap<i64, BTreeSet<i64>>| {
                m.entry(e.from).or_default().insert(e.to);
                m.entry(e.to).or_default().insert(e.from);
            };
            match e.kind {
                Kind::Parent => {
                    f.children.entry(e.from).or_default().insert(e.to);
                    f.parents.entry(e.to).or_default().insert(e.from);
                }
                Kind::Partner => both(&mut f.partners),
                Kind::FormerPartner => both(&mut f.former),
                Kind::Sibling => both(&mut f.recorded_sibling),
                Kind::Cousin => both(&mut f.recorded_cousin),
                Kind::Friend | Kind::Acquaintance => {}
            }
        }
        f
    }

    /// Berücksichtigt gespeicherte Antworten "nicht Elternteil" (siehe [`super::gaps`]):
    /// Sie machen aus Geschwistern mit einem gemeinsamen Elternteil sichere Halbgeschwister.
    pub fn with_declined(mut self, declined: BTreeSet<(i64, i64)>) -> Family {
        self.declined = declined;
        self
    }

    pub fn gender(&self, person: i64) -> Gender {
        self.genders
            .get(&person)
            .copied()
            .unwrap_or(Gender::Unknown)
    }
    pub fn parents_of(&self, person: i64) -> &BTreeSet<i64> {
        self.parents.get(&person).unwrap_or(&self.empty)
    }
    pub fn children_of(&self, person: i64) -> &BTreeSet<i64> {
        self.children.get(&person).unwrap_or(&self.empty)
    }
    pub fn partners_of(&self, person: i64) -> &BTreeSet<i64> {
        self.partners.get(&person).unwrap_or(&self.empty)
    }
    pub fn former_partners_of(&self, person: i64) -> &BTreeSet<i64> {
        self.former.get(&person).unwrap_or(&self.empty)
    }
    /// Alle im Baum vorkommenden Personen (mit mindestens einer Kante).
    pub fn people(&self) -> BTreeSet<i64> {
        self.parents
            .keys()
            .chain(self.children.keys())
            .chain(self.partners.keys())
            .chain(self.former.keys())
            .copied()
            .collect()
    }

    /// Vorfahren bis [`MAX_GENERATIONS`] (kürzester Weg), die Person selbst bei 0.
    fn ancestry(&self, person: i64) -> Ancestry {
        let mut seen: Ancestry = BTreeMap::new();
        seen.insert(person, (0, person));
        let mut open = VecDeque::from([person]);
        while let Some(next) = open.pop_front() {
            let depth = seen[&next].0;
            if depth >= MAX_GENERATIONS {
                continue;
            }
            for &p in self.parents_of(next) {
                if let std::collections::btree_map::Entry::Vacant(e) = seen.entry(p) {
                    e.insert((depth + 1, next));
                    open.push_back(p);
                }
            }
        }
        seen
    }

    /// Nachkommen bis `limit` Generationen unter `person`, ohne die Person selbst.
    pub fn descendants(&self, person: i64, limit: u8) -> BTreeMap<i64, u8> {
        let mut seen = BTreeMap::new();
        seen.insert(person, 0u8);
        let mut open = VecDeque::from([person]);
        while let Some(next) = open.pop_front() {
            let depth = seen[&next];
            if depth >= limit {
                continue;
            }
            for &c in self.children_of(next) {
                if let std::collections::btree_map::Entry::Vacant(e) = seen.entry(c) {
                    e.insert(depth + 1);
                    open.push_back(c);
                }
            }
        }
        seen.remove(&person);
        seen
    }

    /// Alle Vorfahren (ohne die Person) mit Abstand.
    pub fn ancestors(&self, person: i64) -> BTreeMap<i64, u8> {
        let mut map: BTreeMap<i64, u8> = self
            .ancestry(person)
            .into_iter()
            .map(|(k, (d, _))| (k, d))
            .collect();
        map.remove(&person);
        map
    }

    fn chain(map: &Ancestry, from: i64, to: i64) -> Vec<i64> {
        // `from` ist der Vorfahr, `to` die Startperson der Karte: Weg from .. to.
        let mut out = vec![from];
        let mut at = from;
        while at != to {
            at = map[&at].1;
            out.push(at);
        }
        out
    }

    /// Blutsverwandtschaft "a ist ... von b" über den nächsten gemeinsamen Vorfahren.
    fn blood(&self, a: i64, b: i64) -> Option<Link> {
        let up_a = self.ancestry(a);
        let up_b = self.ancestry(b);
        let (x, (u, d)) = up_a
            .iter()
            .filter_map(|(x, (u, _))| up_b.get(x).map(|(d, _)| (*x, (*u, *d))))
            .min_by_key(|(x, (u, d))| (u + d, u.abs_diff(*d), *x))?;
        let mut half = false;
        if (u, d) == (1, 1) {
            let mine = self.parents_of(a);
            let theirs = self.parents_of(b);
            let shared = mine.intersection(theirs).count();
            let only_a: Vec<i64> = mine.difference(theirs).copied().collect();
            let only_b: Vec<i64> = theirs.difference(mine).copied().collect();
            let a_not_b =
                !only_a.is_empty() && only_a.iter().all(|p| self.declined.contains(&(b, *p)));
            let b_not_a =
                !only_b.is_empty() && only_b.iter().all(|p| self.declined.contains(&(a, *p)));
            half = shared >= 1
                && ((shared == 1 && !only_a.is_empty() && !only_b.is_empty())
                    || a_not_b
                    || b_not_a);
        }
        // Weg a .. x (aufwärts) und x .. b (abwärts).
        let mut path = Self::chain(&up_a, x, a);
        path.reverse();
        let mut down = Self::chain(&up_b, x, b);
        down.remove(0);
        path.extend(down);
        Some(Link {
            relation: Relation::Blood {
                a_gen: u,
                b_gen: d,
                half,
            },
            path,
        })
    }

    fn raw(&self, a: i64, b: i64) -> Option<Link> {
        if self.partners_of(a).contains(&b) {
            return Some(Link {
                relation: Relation::Partner,
                path: vec![a, b],
            });
        }
        if self.former_partners_of(a).contains(&b) {
            return Some(Link {
                relation: Relation::FormerPartner,
                path: vec![a, b],
            });
        }
        if let Some(link) = self.blood(a, b) {
            return Some(link);
        }
        for &pa in self.parents_of(a) {
            for &pb in self.parents_of(b) {
                if self.partners_of(pa).contains(&pb) {
                    return Some(Link {
                        relation: Relation::StepSibling,
                        path: vec![a, pa, pb, b],
                    });
                }
            }
        }
        if self
            .recorded_sibling
            .get(&a)
            .is_some_and(|s| s.contains(&b))
        {
            return Some(Link {
                relation: Relation::RecordedSibling,
                path: vec![a, b],
            });
        }
        if self.recorded_cousin.get(&a).is_some_and(|s| s.contains(&b)) {
            return Some(Link {
                relation: Relation::RecordedCousin,
                path: vec![a, b],
            });
        }
        // Verschwägerung: genau ein Partnerschritt.
        let mut best: Option<((u8, u8, i64), Link)> = None;
        let mut offer = |key: (u8, u8, i64), link: Link| {
            if best.as_ref().is_none_or(|(k, _)| key < *k) {
                best = Some((key, link));
            }
        };
        for &p in self.partners_of(b) {
            if let Some(Link {
                relation:
                    Relation::Blood {
                        a_gen,
                        b_gen: p_gen,
                        ..
                    },
                mut path,
            }) = self.blood(a, p)
                && a_gen + p_gen <= AFFINITY_MAX_STEPS
            {
                path.push(b);
                offer(
                    (a_gen + p_gen, 0, p),
                    Link {
                        relation: Relation::ThroughPartnerOfB { a_gen, p_gen },
                        path,
                    },
                );
            }
        }
        for &p in self.partners_of(a) {
            if let Some(Link {
                relation:
                    Relation::Blood {
                        a_gen: p_gen,
                        b_gen,
                        ..
                    },
                path,
            }) = self.blood(p, b)
                && p_gen + b_gen <= AFFINITY_MAX_STEPS
            {
                let mut full = vec![a];
                full.extend(path);
                offer(
                    (p_gen + b_gen, 1, p),
                    Link {
                        relation: Relation::ThroughPartnerOfA { p_gen, b_gen },
                        path: full,
                    },
                );
            }
        }
        best.map(|(_, link)| link)
    }

    /// Beziehung "a ist ... von b" samt Weg, oder `None` (nicht verwandt, nicht
    /// im Baum, außerhalb der Tiefe). Es wird stets von der kleineren ID aus
    /// gerechnet und für die Gegenrichtung umgekehrt; so sind beide Richtungen
    /// immer zueinander invers, auch bei gleich nahen Wegen.
    pub fn relationship(&self, a: i64, b: i64) -> Option<Link> {
        if a == b {
            return None;
        }
        if a < b {
            self.raw(a, b)
        } else {
            self.raw(b, a).map(|mut link| {
                link.relation = link.relation.inverse();
                link.path.reverse();
                link
            })
        }
    }

    /// Ein Satz wie "Anna ist Großmutter von Leo" bzw. "Ben und Dora sind Cousins 1. Grades".
    pub fn sentence(&self, a: i64, b: i64, name: &dyn Fn(i64) -> String) -> String {
        match self.relationship(a, b) {
            None => format!(
                "Zwischen {} und {} ist im Stammbaum keine Verwandtschaft erfasst.",
                name(a),
                name(b)
            ),
            Some(link) => match link.relation.plural() {
                Some(plural) => format!("{} und {} sind {plural}.", name(a), name(b)),
                None => format!(
                    "{} ist {} von {}.",
                    name(a),
                    link.relation.label(self.gender(a)),
                    name(b)
                ),
            },
        }
    }

    /// Weg als Text: "Ben → Vater Tom → Großvater Hans → Sohn Karl → Tochter Dora".
    pub fn path_text(&self, link: &Link, name: &dyn Fn(i64) -> String) -> String {
        let mut out = name(link.path[0]);
        for pair in link.path.windows(2) {
            let (prev, next) = (pair[0], pair[1]);
            let step = if self.parents_of(prev).contains(&next) {
                Relation::Blood {
                    a_gen: 0,
                    b_gen: 1,
                    half: false,
                }
            } else if self.children_of(prev).contains(&next) {
                Relation::Blood {
                    a_gen: 1,
                    b_gen: 0,
                    half: false,
                }
            } else if self.former_partners_of(prev).contains(&next) {
                Relation::FormerPartner
            } else if self.partners_of(prev).contains(&next) {
                Relation::Partner
            } else {
                Relation::RecordedSibling
            };
            out.push_str(&format!(
                " → {} {}",
                step.label(self.gender(next)),
                name(next)
            ));
        }
        out
    }

    /// Kandidaten für Verwandte: Blutsverwandte (Vorfahren und deren Nachkommen).
    fn blood_candidates(&self, person: i64) -> BTreeSet<i64> {
        let mut out = BTreeSet::new();
        for x in self.ancestry(person).into_keys() {
            out.insert(x);
            out.extend(self.descendants(x, MAX_GENERATIONS).into_keys());
        }
        out
    }

    /// Alle Verwandten einer Person, gruppiert und sortiert; je Gruppe höchstens
    /// [`MAX_PER_GROUP`] Einträge (Rest in `more`). Nichts davon ist gespeichert.
    pub fn relatives_of(&self, person: i64) -> Vec<Group> {
        let mut groups: BTreeMap<String, (Order, Vec<Member>)> = BTreeMap::new();
        for other in self.candidates(person) {
            if let Some(link) = self.relationship(other, person) {
                let entry = groups
                    .entry(link.relation.group_title())
                    .or_insert((link.relation.order(), Vec::new()));
                entry.0 = entry.0.min(link.relation.order());
                entry.1.push(Member {
                    person: other,
                    relation: link.relation,
                });
            }
        }
        let mut sorted: Vec<_> = groups.into_iter().collect();
        sorted.sort_by_key(|(title, (order, _))| (*order, title.clone()));
        sorted
            .into_iter()
            .map(|(title, (_, mut members))| {
                members.sort_by_key(|m| m.person);
                let more = members.len().saturating_sub(MAX_PER_GROUP);
                members.truncate(MAX_PER_GROUP);
                Group {
                    title,
                    members,
                    more,
                }
            })
            .collect()
    }

    fn candidates(&self, person: i64) -> BTreeSet<i64> {
        let mut set = self.blood_candidates(person);
        let mut seeds: BTreeSet<i64> = set.clone();
        for &p in self.partners_of(person) {
            // Blutsverwandte des Partners (für Schwiegereltern, Stiefkinder, Schwäger).
            seeds.extend(self.blood_candidates(p));
        }
        for &parent in self.parents_of(person) {
            for &q in self.partners_of(parent) {
                set.extend(self.children_of(q).iter().copied());
            }
        }
        // Partner von Blutsverwandten und Partner von Partner-Verwandten.
        for &z in &seeds {
            set.insert(z);
            set.extend(self.partners_of(z).iter().copied());
        }
        set.extend(self.partners_of(person).iter().copied());
        set.extend(self.former_partners_of(person).iter().copied());
        for map in [&self.recorded_sibling, &self.recorded_cousin] {
            if let Some(s) = map.get(&person) {
                set.extend(s.iter().copied());
            }
        }
        set.remove(&person);
        set
    }

    /// Suche ohne gespeicherte Kanten: alle Personen, deren Beziehung zu `person`
    /// das Prädikat erfüllt, z. B. `|r| r.is_cousin()` für "alle Cousins von X".
    pub fn find(&self, person: i64, wanted: &dyn Fn(Relation) -> bool) -> Vec<Member> {
        let mut out: Vec<Member> = self
            .candidates(person)
            .into_iter()
            .filter_map(|other| {
                self.relationship(other, person).map(|l| Member {
                    person: other,
                    relation: l.relation,
                })
            })
            .filter(|m| wanted(m.relation))
            .collect();
        out.sort_by_key(|m| (m.relation.order(), m.person));
        out
    }

    /// Alle Cousins und Cousinen einer Person (alle Grade), nur aus dem Baum.
    pub fn cousins_of(&self, person: i64) -> Vec<Member> {
        self.find(person, &|r| r.is_cousin())
    }

    /// Direkt erfasste Geschwister- und Cousin-Zeilen, die inzwischen aus dem Baum
    /// folgen (die Behelfsverbindung ist überflüssig und kann entfernt werden).
    pub fn redundant_recorded(&self) -> Vec<(i64, Kind, i64)> {
        let mut out = Vec::new();
        for (map, kind) in [
            (&self.recorded_sibling, Kind::Sibling),
            (&self.recorded_cousin, Kind::Cousin),
        ] {
            for (&a, others) in map {
                for &b in others.iter().filter(|&&b| a < b) {
                    let tree = self.tree_relation(a, b);
                    let redundant = match (kind, tree) {
                        (
                            Kind::Sibling,
                            Some(Relation::Blood {
                                a_gen: 1, b_gen: 1, ..
                            }),
                        ) => true,
                        (Kind::Cousin, Some(r)) => r.is_cousin(),
                        _ => false,
                    };
                    if redundant {
                        out.push((a, kind, b));
                    }
                }
            }
        }
        out
    }

    /// Beziehung allein aus dem Baum (ohne Behelfszeilen und Verschwägerung).
    pub fn tree_relation(&self, a: i64, b: i64) -> Option<Relation> {
        if a == b {
            return None;
        }
        let (lo, hi) = (a.min(b), a.max(b));
        let link = self.blood(lo, hi)?;
        Some(if a < b {
            link.relation
        } else {
            link.relation.inverse()
        })
    }
}
