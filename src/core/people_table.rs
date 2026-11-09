//! Zeilenmodell, Sortierung und Filter der Personentabelle (ADR 0063).
//!
//! Die Zählungen kommen aus einer einzigen indizierten Abfrage (`load_stats`,
//! Index `ix_faces_person`) und aus den bereits geladenen Kanten, dem Stammbaum
//! und den Rückfragen; `build_rows` ist rein. Sortieren und Filtern sind reine
//! Funktionen über den Zeilen, damit die Oberfläche nur Indizes zeichnet.
use super::{
    choice_filter::{self, Matcher},
    collection::Collection,
    faces::Person,
    gaps::Suggestion,
    kinship::Family,
    relations::{Edge, Gender},
};
use anyhow::Result;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

/// Spalten der Tabelle in Anzeigereihenfolge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Column {
    Name,
    Photos,
    Faces,
    Birth,
    Gender,
    Relations,
    Family,
    FirstPhoto,
    LastPhoto,
    Hints,
}

impl Column {
    pub const ALL: [Column; 10] = [
        Column::Name,
        Column::Photos,
        Column::Faces,
        Column::Birth,
        Column::Gender,
        Column::Relations,
        Column::Family,
        Column::FirstPhoto,
        Column::LastPhoto,
        Column::Hints,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Column::Name => "Name",
            Column::Photos => "Fotos",
            Column::Faces => "Gesichter",
            Column::Birth => "Geburt · Alter",
            Column::Gender => "Geschl.",
            Column::Relations => "Bez.",
            Column::Family => "Familie",
            Column::FirstPhoto => "Erstes Foto",
            Column::LastPhoto => "Letztes Foto",
            Column::Hints => "Hinweise",
        }
    }
    pub fn tooltip(self) -> &'static str {
        match self {
            Column::Name => "Name der Person",
            Column::Photos => "Fotos mit einem bestätigten Gesicht dieser Person",
            Column::Faces => "Bestätigte Gesichter dieser Person",
            Column::Birth => "Geburtsjahr und daraus berechnetes Alter",
            Column::Gender => "Geschlecht (steuert die Beschriftung von Beziehungen)",
            Column::Relations => "Direkt gespeicherte Beziehungen",
            Column::Family => "Eltern, Kinder und Partner im Stammbaum",
            Column::FirstPhoto => "Aufnahmedatum des ältesten bestätigten Fotos",
            Column::LastPhoto => "Aufnahmedatum des neuesten bestätigten Fotos",
            Column::Hints => "Offene Rückfragen, an denen diese Person beteiligt ist",
        }
    }
    /// Zahlen und Daten sortiert der erste Klick absteigend (Größtes zuerst).
    pub fn first_click_descending(self) -> bool {
        matches!(
            self,
            Column::Photos
                | Column::Faces
                | Column::Relations
                | Column::Family
                | Column::LastPhoto
                | Column::Hints
        )
    }
    /// Breite in Punkten; `Name` füllt den Rest.
    pub fn width(self) -> f32 {
        match self {
            Column::Name => 150.0,
            Column::Photos => 72.0,
            Column::Faces => 84.0,
            Column::Birth => 104.0,
            Column::Gender => 72.0,
            Column::Relations => 58.0,
            Column::Family => 168.0,
            Column::FirstPhoto => 100.0,
            Column::LastPhoto => 104.0,
            Column::Hints => 88.0,
        }
    }
    /// Reihenfolge, in der Spalten bei wenig Platz entfallen (klein = bleibt zuletzt).
    fn priority(self) -> u8 {
        match self {
            Column::Name => 0,
            Column::Photos => 1,
            Column::Birth => 2,
            Column::Hints => 3,
            Column::Faces => 4,
            Column::Relations => 5,
            Column::Gender => 6,
            Column::LastPhoto => 7,
            Column::FirstPhoto => 8,
            Column::Family => 9,
        }
    }
}

/// Spalten, die in `width` Punkten Platz finden (Name mindestens 150). Die Auswahl
/// folgt der Wichtigkeit, die Anzeige bleibt in Spaltenreihenfolge.
pub fn visible_columns(width: f32) -> Vec<Column> {
    let mut chosen = vec![Column::Name];
    let mut used = Column::Name.width();
    let mut by_priority = Column::ALL;
    by_priority.sort_by_key(|c| c.priority());
    for column in by_priority {
        if column != Column::Name && used + column.width() <= width {
            used += column.width();
            chosen.push(column);
        }
    }
    Column::ALL
        .into_iter()
        .filter(|c| chosen.contains(c))
        .collect()
}

/// Zählungen je Person aus den Gesichtszuordnungen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PersonStats {
    /// Bestätigte Gesichter.
    pub faces: usize,
    /// Fotos mit mindestens einem bestätigten Gesicht.
    pub photos: usize,
    /// Ältestes/neuestes Aufnahmedatum (`JJJJ-MM-TT`) bestätigter Fotos.
    pub first: Option<String>,
    pub last: Option<String>,
}

/// Eine Abfrage für alle Personen; benötigt nur lesenden Zugriff.
pub fn load_stats(collection: &Collection) -> Result<BTreeMap<i64, PersonStats>> {
    let mut statement = collection.db.prepare(
        "SELECT f.person_id,count(*),count(DISTINCT f.photo_id),
                min(NULLIF(replace(substr(m.dt_original,1,10),':','-'),'')),
                max(NULLIF(replace(substr(m.dt_original,1,10),':','-'),''))
         FROM faces f LEFT JOIN metadata_photos m ON m.photo_id=f.photo_id
         WHERE f.person_id IS NOT NULL AND f.assignment='user' AND f.ignored=0 AND f.retained=0
         GROUP BY f.person_id",
    )?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            PersonStats {
                faces: r.get::<_, i64>(1)? as usize,
                photos: r.get::<_, i64>(2)? as usize,
                first: r.get(3)?,
                last: r.get(4)?,
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Eine Tabellenzeile; alle Werte sind fertig berechnet.
#[derive(Clone, Debug, PartialEq)]
pub struct PersonRow {
    pub id: i64,
    pub name: String,
    pub gender: Gender,
    pub birth: Option<i32>,
    pub age: Option<i32>,
    pub photos: usize,
    pub faces: usize,
    pub relations: usize,
    /// Eltern + Kinder + Partner im Stammbaum.
    pub family_links: usize,
    pub family: String,
    pub first_photo: Option<String>,
    pub last_photo: Option<String>,
    pub hints: usize,
    /// Durchsuchbarer Text: Name, `#ID`, Geburtsjahr, Geschlecht.
    pub search: String,
}

impl PersonRow {
    pub fn gender_label(&self) -> &'static str {
        match self.gender {
            Gender::Female => "weiblich",
            Gender::Male => "männlich",
            Gender::Unknown => "",
        }
    }
}

/// Alter in Jahren, nur für plausible Geburtsjahre.
pub fn age(birth: i32, year: i32) -> Option<i32> {
    let age = year - birth;
    (0..=125).contains(&age).then_some(age)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Kurzinfo zur Familie: „2 Eltern · 3 Kinder · Partner“.
pub fn family_summary(parents: usize, children: usize, partners: usize) -> String {
    let mut parts = Vec::new();
    if parents > 0 {
        parts.push(plural(parents, "Elternteil", "Eltern"));
    }
    if children > 0 {
        parts.push(plural(children, "Kind", "Kinder"));
    }
    if partners > 0 {
        parts.push(if partners == 1 {
            "Partner".to_string()
        } else {
            format!("{partners} Partner")
        });
    }
    parts.join(" · ")
}

/// Baut alle Zeilen. `year` ist das aktuelle Jahr (für das Alter).
pub fn build_rows(
    people: &[Person],
    stats: &BTreeMap<i64, PersonStats>,
    edges: &[Edge],
    family: &Family,
    questions: &[Suggestion],
    year: i32,
) -> Vec<PersonRow> {
    let mut relations: BTreeMap<i64, usize> = BTreeMap::new();
    for edge in edges {
        *relations.entry(edge.from).or_default() += 1;
        if edge.to != edge.from {
            *relations.entry(edge.to).or_default() += 1;
        }
    }
    let mut hints: BTreeMap<i64, usize> = BTreeMap::new();
    for question in questions {
        let involved: BTreeSet<i64> = question_people(question).into_iter().collect();
        for id in involved {
            *hints.entry(id).or_default() += 1;
        }
    }
    people
        .iter()
        .map(|person| {
            let stat = stats.get(&person.id).cloned().unwrap_or_default();
            let birth = person.birth.trim().parse::<i32>().ok();
            let (parents, children, partners) = (
                family.parents_of(person.id).len(),
                family.children_of(person.id).len(),
                family.partners_of(person.id).len(),
            );
            let gender = Gender::parse(&person.gender);
            let mut row = PersonRow {
                id: person.id,
                name: person.name.clone(),
                gender,
                birth,
                age: birth.and_then(|b| age(b, year)),
                photos: stat.photos,
                // Der Sammlungszähler der Person gilt, falls die Abfrage fehlte.
                faces: if stats.is_empty() {
                    person.count
                } else {
                    stat.faces
                },
                relations: relations.get(&person.id).copied().unwrap_or(0),
                family_links: parents + children + partners,
                family: family_summary(parents, children, partners),
                first_photo: stat.first,
                last_photo: stat.last,
                hints: hints.get(&person.id).copied().unwrap_or(0),
                search: String::new(),
            };
            row.search = format!(
                "{} #{} {} {}",
                row.name,
                row.id,
                person.birth.trim(),
                row.gender_label()
            );
            row
        })
        .collect()
}

/// Alle an einer Rückfrage beteiligten Personen (typunabhängig).
pub fn question_people(question: &Suggestion) -> Vec<i64> {
    question
        .candidates
        .iter()
        .chain(&question.targets)
        .chain(&question.via)
        .copied()
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortKey {
    pub column: Column,
    pub descending: bool,
}

/// Klick auf einen Spaltenkopf. Ohne `additive` wird die Spalte zur einzigen
/// Sortierung (erneuter Klick kehrt um); mit `additive` (Umschalt-Klick) wird sie
/// als weiteres Kriterium angehängt bzw. umgekehrt.
pub fn toggle_sort(keys: &mut Vec<SortKey>, column: Column, additive: bool) {
    let existing = keys.iter().position(|k| k.column == column);
    match (existing, additive) {
        (Some(index), _) => keys[index].descending = !keys[index].descending,
        (None, true) => keys.push(SortKey {
            column,
            descending: column.first_click_descending(),
        }),
        (None, false) => {
            keys.clear();
            keys.push(SortKey {
                column,
                descending: column.first_click_descending(),
            });
        }
    }
    if !additive && keys.len() > 1 {
        let kept = keys.iter().copied().find(|k| k.column == column);
        keys.clear();
        keys.extend(kept);
    }
}

/// Sortierwert einer Zelle; `None` gilt als leer und steht in jeder Richtung zuletzt.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Value {
    Number(i64),
    Text(String),
}

fn value(row: &PersonRow, column: Column) -> Option<Value> {
    let text = |s: &str| (!s.is_empty()).then(|| Value::Text(choice_filter::sort_key(s)));
    match column {
        Column::Name => text(&row.name),
        Column::Photos => Some(Value::Number(row.photos as i64)),
        Column::Faces => Some(Value::Number(row.faces as i64)),
        Column::Birth => row.birth.map(|b| Value::Number(i64::from(b))),
        Column::Gender => text(row.gender_label()),
        Column::Relations => Some(Value::Number(row.relations as i64)),
        Column::Family => (row.family_links > 0).then_some(Value::Number(row.family_links as i64)),
        Column::FirstPhoto => row.first_photo.as_deref().and_then(text),
        Column::LastPhoto => row.last_photo.as_deref().and_then(text),
        Column::Hints => Some(Value::Number(row.hints as i64)),
    }
}

fn compare_column(a: &PersonRow, b: &PersonRow, key: SortKey) -> Ordering {
    match (value(a, key.column), value(b, key.column)) {
        (Some(x), Some(y)) => {
            if key.descending {
                y.cmp(&x)
            } else {
                x.cmp(&y)
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Zeilenreihenfolge als Indizes. Stabil: nach den gewählten Spalten folgen Name
/// (aufsteigend) und ID als feste Nebenordnung.
pub fn sort_indices(rows: &[PersonRow], keys: &[SortKey]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&x, &y| {
        let (a, b) = (&rows[x], &rows[y]);
        for key in keys {
            let ordering = compare_column(a, b, *key);
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        compare_column(
            a,
            b,
            SortKey {
                column: Column::Name,
                descending: false,
            },
        )
        .then(a.id.cmp(&b.id))
    });
    order
}

/// Zeilenfilter der Suchzeile: alle Wörter müssen im Suchtext stehen; optional nur
/// Personen mit offenen Rückfragen. Die Reihenfolge der Eingabe bleibt erhalten.
pub fn filter_indices(
    rows: &[PersonRow],
    order: &[usize],
    query: &str,
    only_hints: bool,
) -> Vec<usize> {
    let matcher = Matcher::new(query);
    order
        .iter()
        .copied()
        .filter(|&i| {
            let row = &rows[i];
            (!only_hints || row.hints > 0) && matcher.rank(&row.search).is_some()
        })
        .collect()
}

/// Sichtbare Zeilen: Sortierung, dann Filter.
pub fn view(rows: &[PersonRow], keys: &[SortKey], query: &str, only_hints: bool) -> Vec<usize> {
    filter_indices(rows, &sort_indices(rows, keys), query, only_hints)
}

/// Aktuelles Jahr (UTC) ohne zusätzliche Abhängigkeit.
pub fn current_year() -> i32 {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    1970 + (seconds / 31_556_952) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, name: &str, photos: usize, birth: Option<i32>) -> PersonRow {
        PersonRow {
            id,
            name: name.into(),
            gender: Gender::Unknown,
            birth,
            age: birth.and_then(|b| age(b, 2026)),
            photos,
            faces: photos * 2,
            relations: 0,
            family_links: 0,
            family: String::new(),
            first_photo: None,
            last_photo: None,
            hints: 0,
            search: format!(
                "{name} #{id} {}",
                birth.map_or(String::new(), |b| b.to_string())
            ),
        }
    }
    fn ids(rows: &[PersonRow], order: &[usize]) -> Vec<i64> {
        order.iter().map(|&i| rows[i].id).collect()
    }
    fn asc(column: Column) -> SortKey {
        SortKey {
            column,
            descending: false,
        }
    }
    fn desc(column: Column) -> SortKey {
        SortKey {
            column,
            descending: true,
        }
    }

    #[test]
    fn name_sort_ignores_case_and_umlauts_and_breaks_ties_by_id() {
        let rows = [
            row(5, "zoe", 1, None),
            row(3, "Ärztin Eva", 1, None),
            row(2, "anna", 1, None),
            row(1, "Anna", 1, None),
            row(4, "Berta", 1, None),
        ];
        let order = sort_indices(&rows, &[asc(Column::Name)]);
        assert_eq!(ids(&rows, &order), [1, 2, 3, 4, 5]);
        let order = sort_indices(&rows, &[desc(Column::Name)]);
        // Umgekehrt nach Name, gleiche Namen bleiben nach ID aufsteigend.
        assert_eq!(ids(&rows, &order), [5, 4, 3, 1, 2]);
    }

    #[test]
    fn empty_values_come_last_in_both_directions() {
        let rows = [
            row(1, "A", 1, None),
            row(2, "B", 1, Some(1990)),
            row(3, "C", 1, Some(1950)),
        ];
        let up = sort_indices(&rows, &[asc(Column::Birth)]);
        assert_eq!(ids(&rows, &up), [3, 2, 1]);
        let down = sort_indices(&rows, &[desc(Column::Birth)]);
        assert_eq!(ids(&rows, &down), [2, 3, 1]);
    }

    #[test]
    fn several_columns_sort_in_priority_order_and_stay_stable() {
        let rows = [
            row(1, "Carl", 5, None),
            row(2, "Anna", 5, None),
            row(3, "Bert", 9, None),
            row(4, "Dora", 1, None),
        ];
        let order = sort_indices(&rows, &[desc(Column::Photos), asc(Column::Name)]);
        assert_eq!(ids(&rows, &order), [3, 2, 1, 4]);
        // Ohne Zusatzkriterium greift automatisch der Name als Nebenordnung.
        let order = sort_indices(&rows, &[desc(Column::Photos)]);
        assert_eq!(ids(&rows, &order), [3, 2, 1, 4]);
        // Wiederholtes Sortieren liefert dieselbe Reihenfolge.
        assert_eq!(order, sort_indices(&rows, &[desc(Column::Photos)]));
    }

    #[test]
    fn header_clicks_toggle_replace_and_add_criteria() {
        let mut keys = Vec::new();
        toggle_sort(&mut keys, Column::Photos, false);
        assert_eq!(keys, [desc(Column::Photos)], "Zahlen zuerst absteigend");
        toggle_sort(&mut keys, Column::Photos, false);
        assert_eq!(keys, [asc(Column::Photos)]);
        toggle_sort(&mut keys, Column::Name, false);
        assert_eq!(keys, [asc(Column::Name)], "ein Klick ersetzt");
        toggle_sort(&mut keys, Column::Photos, true);
        assert_eq!(keys, [asc(Column::Name), desc(Column::Photos)]);
        toggle_sort(&mut keys, Column::Photos, true);
        assert_eq!(keys, [asc(Column::Name), asc(Column::Photos)]);
        toggle_sort(&mut keys, Column::Name, false);
        assert_eq!(
            keys,
            [desc(Column::Name)],
            "Klick ohne Umschalt behält nur diese Spalte"
        );
    }

    #[test]
    fn filter_matches_names_ids_and_years_and_keeps_the_sort_order() {
        let rows = [
            row(1, "Anna Müller", 1, Some(1980)),
            row(2, "Bernd Mueller", 1, None),
            row(3, "Carla", 1, Some(1980)),
        ];
        let order = [2, 0, 1];
        let hits = |q: &str| ids(&rows, &filter_indices(&rows, &order, q, false));
        assert_eq!(hits("mü"), [1, 2]);
        assert_eq!(hits("#3"), [3]);
        assert_eq!(hits("1980"), [3, 1]);
        assert_eq!(hits(""), [3, 1, 2]);
        assert!(hits("zzz").is_empty());
    }

    #[test]
    fn hint_filter_limits_rows_to_people_with_open_questions() {
        let mut rows = [row(1, "A", 1, None), row(2, "B", 1, None)];
        rows[1].hints = 2;
        assert_eq!(ids(&rows, &view(&rows, &[], "", true)), [2]);
        assert_eq!(ids(&rows, &view(&rows, &[], "", false)), [1, 2]);
    }

    #[test]
    fn narrow_windows_keep_the_important_columns_and_wide_ones_show_all() {
        let narrow = visible_columns(340.0);
        assert_eq!(narrow[0], Column::Name);
        assert!(narrow.contains(&Column::Photos));
        assert!(!narrow.contains(&Column::Family));
        let total: f32 = narrow.iter().map(|c| c.width()).sum();
        assert!(total <= 340.0);
        assert_eq!(visible_columns(1400.0), Column::ALL);
        assert_eq!(visible_columns(10.0), [Column::Name]);
    }

    #[test]
    fn family_summary_and_age_are_short_and_plausible() {
        assert_eq!(family_summary(2, 3, 1), "2 Eltern · 3 Kinder · Partner");
        assert_eq!(family_summary(1, 1, 2), "1 Elternteil · 1 Kind · 2 Partner");
        assert_eq!(family_summary(0, 0, 0), "");
        assert_eq!(age(1980, 2026), Some(46));
        assert_eq!(age(2030, 2026), None);
        assert_eq!(age(1800, 2026), None);
    }
}
