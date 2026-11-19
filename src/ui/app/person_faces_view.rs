//! Gesichter einer Person unter „Personen“: die unbestätigten, die sie wahrscheinlich zeigen
//! (Seite `probable`), und ihre bestätigten (Seite `person-confirmed`). Beide nutzen die
//! Gruppenkarten, Tasten, Rücknahme und das sofortige Ausblenden der Ansicht „Gesichter“.
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
        let hint = if self.person_face_mode == "probable" {
            let minimum = format!("{:.2}", config::PERSON_CANDIDATE_MIN).replace('.', ",");
            format!(
                "Unbestätigte Gesichter, nach Ähnlichkeit zu {} geordnet, wahrscheinlichste zuerst \
                 (ab {minimum}). Bestätigen und Ablehnen wie in „Vorschläge prüfen“; die Tasten J, \
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

    /// Umschalter „Wahrscheinlich X | Bestätigt“ mit der Zahl der Gesichter.
    fn person_faces_tabs(&mut self, ui: &mut egui::Ui, person: &faces::Person) {
        let loading = self.tasks.iter().any(|task| task.kind == "faces");
        let total = (!loading).then_some(self.face_page.faces_total);
        let count = |mode: &str, fallback: Option<usize>| {
            if self.person_face_mode == mode {
                total
            } else {
                fallback
            }
        };
        let tabs = [
            (
                "probable",
                format!("Wahrscheinlich {}", person.name),
                count("probable", None),
            ),
            (
                "person-confirmed",
                "Bestätigt".to_string(),
                count("person-confirmed", Some(person.count)),
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
        if self.person_face_mode == "probable" {
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
        self.reset_face_page();
    }
}
