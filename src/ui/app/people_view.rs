//! Ansicht „Personen“: die Personentabelle als Übersicht, dazu Stammbaum, Graph, Zeitreise
//! und Gesichter als Ansichten der in der Tabelle gewählten Person.
use super::*;

/// Umschalter der Ansicht in seiner Reihenfolge.
pub(super) const PERSON_MODES: [(PersonMode, &str); 6] = [
    (PersonMode::Overview, "Übersicht"),
    (PersonMode::Tree, "Stammbaum"),
    (PersonMode::Graph, "Graph"),
    (PersonMode::Time, "Zeitreise"),
    (PersonMode::Faces, "Gesichter"),
    (PersonMode::Stats, "Statistik"),
];

impl PhotoApp {
    pub(super) fn people_view(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Personen").size(23.0).strong());
        if self.settings.collection.is_empty() {
            self.empty(
                ui,
                "Wer gehört zu deiner Sammlung?",
                "Wähle links unter „Sammlung“ einen Ordner, um Personen und ihre Beziehungen zu sehen.",
            );
            return;
        }
        let selected = self
            .selected_person()
            .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
            .map(|p| (p.id, p.name.clone()));
        self.ensure_duplicates();
        let questions = self.all_questions();
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in PERSON_MODES {
                let label = if mode == PersonMode::Overview && questions > 0 {
                    format!("{label} · {questions} Rückfragen")
                } else {
                    label.to_string()
                };
                if ui
                    .selectable_label(self.person_mode == mode, label)
                    .clicked()
                {
                    self.set_person_mode(mode);
                }
            }
            ui.separator();
            match &selected {
                Some((id, name)) => {
                    ui.label(RichText::new(name).strong())
                        .on_hover_text(select::person_caption(&self.face_page.people, *id, name));
                }
                None => {
                    ui.label(RichText::new("Keine Person gewählt").weak());
                }
            }
        });
        ui.separator();
        if !matches!(
            self.person_mode,
            PersonMode::Overview | PersonMode::Faces | PersonMode::Stats
        ) && selected.is_none()
        {
            ui.label(
                RichText::new(
                    "Wähle in der Übersicht eine Person, dann steht sie hier im Mittelpunkt.",
                )
                .weak(),
            );
        }
        match self.person_mode {
            PersonMode::Overview => self.persons_page(ui),
            PersonMode::Tree => self.tree_tab(ui),
            PersonMode::Graph => self.relationship_graph_tab(ui),
            PersonMode::Time => self.time_view(ui),
            PersonMode::Faces => self.person_faces_view(ui),
            PersonMode::Stats => self.person_stats_tab(ui),
        }
    }
}
