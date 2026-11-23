//! Gesichter einer Person unter „Personen“: die unbestätigten, die sie wahrscheinlich zeigen
//! (Seite `probable`), ihre bestätigten (Seite `person-confirmed`) und die als „nicht X“
//! abgelehnten (Seite `person-rejected`). Alle nutzen die Gruppenkarten, Rücknahme und das
//! sofortige Ausblenden der Ansicht „Gesichter“.
use super::*;

impl PhotoApp {
    pub(super) fn person_faces_view(&mut self, ui: &mut egui::Ui) {
        self.hover_face = None;
        self.person_faces_selector(ui);
        let Some(person) = self
            .selected_person()
            .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
            .cloned()
        else {
            if !self.face_page_loading(ui) {
                ui.label(RichText::new("Wähle oben oder in der Übersicht eine Person.").weak());
            }
            return;
        };
        self.person_faces_tabs(ui, &person);
        self.hover_preview_footer(ui);
        self.face_toolbar(ui);
        let hint = if self.person_face_mode == "person-rejected" {
            format!(
                "Gesichter, die als „nicht {0}“ abgelehnt sind, die {0} ähnlichsten zuerst – dort \
                 stehen versehentliche Ablehnungen. Haken: doch {0}; Kreuz: nur die Ablehnung \
                 aufheben, das Gesicht wird wieder vorgeschlagen.",
                person.name
            )
        } else if self.person_face_mode == "probable" {
            let minimum = crate::core::progress::decimal(
                config::model_scale(&self.face_page.status.embedder).candidate_min,
                2,
            );
            format!(
                "Unbestätigte Gesichter, nach Ähnlichkeit zu {0} geordnet, wahrscheinlichste zuerst \
                 (ab {minimum}); darunter die, die einer anderen Person noch ähnlicher sind als \
                 {0}. Bestätigen und Ablehnen wie in „Vorschläge prüfen“; die Tasten J, \
                 N und Entf gelten für die oberste Gruppe.",
                person.name
            )
        } else {
            format!(
                "Klicke falsch zugeordnete Gesichter an, um sie abzuwählen, und entferne sie dann \
                 von {}. Sie erscheinen danach unter „Benennen“.",
                person.name
            )
        };
        ui.label(RichText::new(hint).small().weak());
        if self.face_page.groups.is_empty() {
            if !self.face_page_loading(ui) {
                self.empty_person_faces(ui, &person.name);
            }
            return;
        }
        self.face_group_list(ui);
    }

    /// Personenauswahl über der Seite; sie gilt wie die Auswahl in der Übersicht für alle
    /// Ansichten unter „Personen“.
    fn person_faces_selector(&mut self, ui: &mut egui::Ui) {
        let items = select::person_choices(&self.face_page.people, None);
        let current = self.selected_person();
        let mut chosen = current;
        ui.horizontal(|ui| {
            ui.label("Person");
            select::FilterSelect::new("person-faces-person", &items)
                .placeholder("Person wählen …")
                .width(260.0)
                .show(ui, &mut chosen);
        });
        if chosen != current
            && let Some(id) = chosen
        {
            self.select_person(id);
            self.clear_face_groups();
            self.refresh_faces();
        }
    }

    /// Umschalter „Wahrscheinlich X | Bestätigt | Nicht X“ mit der Zahl der Gesichter.
    fn person_faces_tabs(&mut self, ui: &mut egui::Ui, person: &faces::Person) {
        let loading = self.tasks.iter().any(|task| task.kind == JobKind::Faces);
        let total = (!loading).then_some(self.face_page.faces_total);
        let count = |mode: &str, fallback: Option<usize>| {
            if self.person_face_mode == mode {
                total
            } else {
                fallback
            }
        };
        let others = self.face_page.probable_others;
        let probable = match count("probable", None) {
            Some(total) => format!(
                "Wahrscheinlich {}{}",
                person.name,
                probable_count(total, others)
            ),
            None => format!("Wahrscheinlich {}", person.name),
        };
        let tabs = [
            ("probable", probable, None),
            (
                "person-confirmed",
                "Bestätigt".to_string(),
                count(
                    "person-confirmed",
                    // Mit Ordnerfilter die bestätigten Gesichter der Person im Ordner.
                    Some(
                        self.face_page
                            .folder
                            .as_ref()
                            .map_or(person.count, |folder| folder.person_confirmed),
                    ),
                ),
            ),
            (
                "person-rejected",
                format!("Nicht {}", person.name),
                count(
                    "person-rejected",
                    (!loading).then_some(self.face_page.rejected_total),
                ),
            ),
        ];
        ui.horizontal(|ui| {
            for (mode, label, number) in tabs {
                let label = match number {
                    Some(number) => format!("{label} · {number}"),
                    None => label,
                };
                if ui
                    .add(egui::Button::selectable(
                        self.person_face_mode == mode,
                        label,
                    ))
                    .clicked()
                    && self.person_face_mode != mode
                {
                    self.clear_face_groups();
                    self.set_person_face_mode(mode);
                }
            }
        });
    }

    fn empty_person_faces(&self, ui: &mut egui::Ui, name: &str) {
        if let Some(folder) = &self.face_folder {
            self.empty(
                ui,
                &format!("Im Ordner „{folder}“ zeigt diese Seite keine Gesichter."),
                "Wähle eine andere Seite, einen anderen Ordner oder „Alle Ordner“ (✕ neben dem \
                 Ordner).",
            );
        } else if self.person_face_mode == "person-rejected" {
            self.empty(
                ui,
                &format!("Kein Gesicht ist als „nicht {name}“ abgelehnt."),
                "Hier erscheinen Gesichter, die bei „Keines ist …“, mit dem Kreuz oder beim \
                 Abwählen als nicht diese Person markiert wurden.",
            );
        } else if self.person_face_mode == "probable" {
            self.empty(
                ui,
                &format!("Keine wahrscheinlichen Gesichter für {name}."),
                "Kein unbestätigtes Gesicht ist ähnlich genug. Ohne bestätigte Gesichter mit \
                 Namensmerkmalen kann die App nicht vergleichen; bestätige dann zuerst Gesichter \
                 dieser Person.",
            );
        } else {
            self.empty(
                ui,
                &format!("Noch keine bestätigten Gesichter von {name}."),
                "Bestätige unter „Wahrscheinlich“ oder in „Vorschläge prüfen“ Gesichter dieser Person.",
            );
        }
    }

    /// Verwirft die angezeigten Gruppen, damit bis zum neuen Lesen keine Karten einer anderen
    /// Seite oder Person zu sehen (und zu entscheiden) sind.
    pub(super) fn clear_face_groups(&mut self) {
        self.face_page.groups.clear();
        self.face_page.total_groups = 0;
        self.face_page.faces_total = 0;
        self.face_page.probable_others = 0;
        self.face_page.better.clear();
        self.face_page.rejected_total = 0;
        self.face_page.confirmed_as.clear();
        self.reset_face_page();
    }

    /// Überschrift vor der ersten Gruppe des zweiten Abschnitts der Seite `probable`.
    pub(super) fn others_heading(&self, ui: &mut egui::Ui, group: &[Face]) {
        let name = group
            .first()
            .and_then(|f| f.person)
            .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
            .map(|p| p.name.as_str())
            .unwrap_or_default();
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Eher jemand anderes – {name} ist nicht am wahrscheinlichsten"
            ))
            .size(16.0)
            .strong(),
        );
        ui.label(
            RichText::new(format!(
                "Diese Gesichter ähneln {name}, eine andere Person ähnelt ihnen aber noch mehr. \
                 Unter jedem Gesicht steht, wer."
            ))
            .small()
            .weak(),
        );
        ui.add_space(6.0);
    }
}

/// Zahl im Reiter „Wahrscheinlich X“: erster Abschnitt, dazu der zweite in Klammern.
pub(super) fn probable_count(total: usize, others: usize) -> String {
    let first = total.saturating_sub(others);
    if others == 0 {
        format!(" · {first}")
    } else {
        format!(" · {first} (+{others} eher andere)")
    }
}

/// Beginnt mit `index` der zweite Abschnitt der Seite `probable`? Gruppen mischen die
/// Abschnitte nie, daher entscheidet das erste Gesicht.
pub(super) fn starts_others(
    groups: &[Vec<Face>],
    index: usize,
    better: &BTreeMap<i64, (i64, f32)>,
) -> bool {
    let others = |group: &Vec<Face>| group.first().is_some_and(|f| better.contains_key(&f.id));
    groups.get(index).is_some_and(others) && (index == 0 || !others(&groups[index - 1]))
}

fn person_name(people: &[faces::Person], id: i64) -> String {
    people
        .iter()
        .find(|p| p.id == id)
        .map_or_else(|| format!("#{id}"), |p| p.name.clone())
}

/// „eher: Y (0,62)“ unter einem Gesicht des zweiten Abschnitts.
pub(super) fn better_note(better: Option<&(i64, f32)>, people: &[faces::Person]) -> Option<String> {
    let (person, score) = better?;
    Some(format!(
        "eher: {} ({})",
        person_name(people, *person),
        crate::core::progress::decimal(*score, 2)
    ))
}

/// Zusammenfassung für die Karte: die anderen Personen nach Häufigkeit (bis zu drei), etwa
/// „eher: Anna (12×), Ben (3×)“.
pub(super) fn better_summary(
    group: &[Face],
    better: &BTreeMap<i64, (i64, f32)>,
    people: &[faces::Person],
) -> Option<String> {
    let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
    for face in group {
        if let Some((person, _)) = better.get(&face.id) {
            *counts.entry(*person).or_default() += 1;
        }
    }
    let mut counts: Vec<(i64, usize)> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut parts: Vec<String> = counts
        .iter()
        .take(3)
        .map(|(person, count)| format!("{} ({count}×)", person_name(people, *person)))
        .collect();
    if counts.len() > 3 {
        parts.push("…".into());
    }
    (!parts.is_empty()).then(|| format!("eher: {}", parts.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: i64, name: &str) -> faces::Person {
        faces::Person {
            id,
            name: name.into(),
            birth: String::new(),
            count: 0,
            gender: String::new(),
            note: String::new(),
        }
    }
    fn face(id: i64) -> Face {
        Face {
            id,
            photo: id,
            source: String::new(),
            bbox: [0.0; 4],
            person: Some(1),
            assignment: None,
            ignored: false,
            score: 0.5,
        }
    }

    #[test]
    fn tab_count_shows_the_second_section_only_when_present() {
        assert_eq!(probable_count(10, 0), " · 10");
        assert_eq!(probable_count(10, 4), " · 6 (+4 eher andere)");
    }

    #[test]
    fn heading_comes_once_before_the_first_group_of_the_second_section() {
        let groups = vec![vec![face(1)], vec![face(2)], vec![face(3)]];
        let better = BTreeMap::from([(2, (7, 0.6)), (3, (8, 0.5))]);
        let starts: Vec<bool> = (0..3).map(|i| starts_others(&groups, i, &better)).collect();
        assert_eq!(starts, vec![false, true, false]);
        // Eine Seite ganz im zweiten Abschnitt beginnt mit der Überschrift.
        assert!(starts_others(&groups[1..], 0, &better));
    }

    #[test]
    fn notes_name_the_better_person_with_a_decimal_comma() {
        let people = vec![person(7, "Anna"), person(8, "Ben")];
        assert_eq!(
            better_note(Some(&(7, 0.6234)), &people).as_deref(),
            Some("eher: Anna (0,62)")
        );
        assert_eq!(better_note(None, &people), None);
        let better = BTreeMap::from([(1, (8, 0.5)), (2, (7, 0.6)), (3, (8, 0.55))]);
        let group = vec![face(1), face(2), face(3), face(4)];
        assert_eq!(
            better_summary(&group, &better, &people).as_deref(),
            Some("eher: Ben (2×), Anna (1×)")
        );
        assert_eq!(better_summary(&[face(4)], &better, &people), None);
    }
}
