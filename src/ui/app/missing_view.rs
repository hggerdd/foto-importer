//! Dialog „Fehlende Dateien“ unter Sammlung → Bestand: fehlende Dateien je Ordner, erneut
//! suchen, neu verknüpfen, Einträge nach Rückfrage entfernen. Gelesen und geschrieben wird nur
//! auf Workern.
use super::*;
use crate::core::missing::{self, MissingFolder};

#[derive(Default)]
pub(super) struct MissingUi {
    /// Revision des zuletzt gestarteten Lesens; ältere Antworten werden verworfen.
    pub(super) revision: u64,
    /// Gelesener Stand; `None`, solange noch nichts gelesen ist.
    pub(super) folders: Option<Vec<MissingFolder>>,
    /// Zum Entfernen gewählte Ordner.
    pub(super) selected: BTreeSet<String>,
    /// Rückfrage vor dem Entfernen sichtbar.
    pub(super) confirm: bool,
}

impl MissingUi {
    pub(super) fn accept(&mut self, folders: Vec<MissingFolder>) {
        self.selected
            .retain(|dir| folders.iter().any(|f| f.dir == *dir));
        self.folders = Some(folders);
    }
    /// Summen der gewählten Ordner: Dateien, Fotos ohne Datei, bestätigte Gesichter.
    pub(super) fn chosen_impact(&self) -> (usize, usize, usize) {
        self.folders
            .iter()
            .flatten()
            .filter(|f| self.selected.contains(&f.dir))
            .fold((0, 0, 0), |sum, f| {
                (
                    sum.0 + f.files,
                    sum.1 + f.orphaned_photos,
                    sum.2 + f.confirmed_faces,
                )
            })
    }
}

fn folder_label(dir: &str) -> &str {
    if dir.is_empty() {
        "(Sammlungsordner)"
    } else {
        dir
    }
}

impl PhotoApp {
    /// Öffnet „Fehlende Dateien“ und liest den Stand neu.
    pub(super) fn open_missing(&mut self) {
        self.missing.confirm = false;
        self.collection_dialog = Some(CollectionDialog::Missing);
        self.load_missing();
    }

    pub(super) fn load_missing(&mut self) {
        if self.settings.collection.is_empty() {
            return;
        }
        self.missing.revision += 1;
        let revision = self.missing.revision;
        let root = PathBuf::from(&self.settings.collection);
        self.launch(
            "Fehlende Dateien lesen",
            false,
            JobKind::Missing,
            move |control| {
                let c = Collection::open(&root, false)?;
                c.cancel_with(&control);
                Ok(Outcome::Missing(revision, missing::report(&c, &control)?))
            },
        );
    }

    /// Entfernt die Einträge der gewählten Ordner (nach der Rückfrage).
    pub(super) fn remove_missing(&mut self) {
        let dirs: Vec<String> = self.missing.selected.iter().cloned().collect();
        if dirs.is_empty() || self.busy() {
            return;
        }
        self.undo.clear();
        self.missing.confirm = false;
        self.missing.selected.clear();
        self.collection_dialog = None;
        self.launch_write(
            "Fehlende Einträge entfernen",
            JobKind::Mutation,
            move |mut c, control| Ok(Outcome::Done(missing::remove(&mut c, &dirs, &control)?)),
        );
    }

    fn relink_missing(&mut self, from: String, to: String) {
        if self.busy() {
            return;
        }
        self.undo.clear();
        self.collection_dialog = None;
        self.launch_write(
            "Fehlende Dateien neu verknüpfen",
            JobKind::Mutation,
            move |mut c, control| {
                Ok(Outcome::Done(missing::relink(
                    &mut c, &from, &to, &control,
                )?))
            },
        );
    }

    pub(super) fn missing_controls(&mut self, ui: &mut egui::Ui) {
        if self.missing.confirm {
            self.missing_confirmation(ui);
            return;
        }
        ui.label(
            "Diese Dateien standen im Bestand, wurden aber nicht mehr gefunden. Wurde ein \
             Ordner umbenannt oder verschoben, ordnet „Erneut suchen“ die Dateien samt Foto-ID, \
             Gesichtern und Zuordnungen wieder zu.",
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Erneut suchen"))
                .on_hover_text(
                    "Liest die ganze Sammlung neu (Bestand aktualisieren). Wiedergefundene \
                     Dateien gelten danach wieder als vorhanden.",
                )
                .clicked()
            {
                self.collection_dialog = None;
                self.collection_action("scan");
            }
            if ui.button("Neu lesen").clicked() {
                self.load_missing();
            }
        });
        ui.separator();
        let Some(folders) = self.missing.folders.clone() else {
            ui.label("Wird gelesen …");
            return;
        };
        if folders.is_empty() {
            ui.label("Keine fehlenden Dateien.");
            return;
        }
        let mut relink = None;
        egui::Grid::new("missing-folders")
            .num_columns(3)
            .striped(true)
            .spacing(egui::vec2(12.0, 6.0))
            .show(ui, |ui| {
                for folder in &folders {
                    let mut chosen = self.missing.selected.contains(&folder.dir);
                    if ui
                        .checkbox(&mut chosen, folder_label(&folder.dir))
                        .changed()
                    {
                        if chosen {
                            self.missing.selected.insert(folder.dir.clone());
                        } else {
                            self.missing.selected.remove(&folder.dir);
                        }
                    }
                    let mut text = plural(folder.files, "Datei", "Dateien");
                    if folder.confirmed_faces > 0 {
                        text.push_str(&format!(
                            " · {} bestätigte Gesichter",
                            folder.confirmed_faces
                        ));
                    }
                    ui.label(text);
                    match &folder.relink {
                        Some(target) => {
                            if ui
                                .add_enabled(
                                    !self.busy(),
                                    egui::Button::new(format!(
                                        "Mit „{}“ neu verknüpfen",
                                        folder_label(target)
                                    )),
                                )
                                .on_hover_text(
                                    "Jede fehlende Datei hat dort genau eine gleiche Datei \
                                     (Name, Größe, Änderungszeit). Die alten Einträge mit Foto-ID, \
                                     Gesichtern und Zuordnungen übernehmen die neuen Pfade.",
                                )
                                .clicked()
                            {
                                relink = Some((folder.dir.clone(), target.clone()));
                            }
                        }
                        None => {
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
        if let Some((from, to)) = relink {
            self.relink_missing(from, to);
            return;
        }
        ui.add_space(6.0);
        let count = self.missing.selected.len();
        if ui
            .add_enabled(
                count > 0 && !self.busy(),
                egui::Button::new(format!(
                    "Einträge entfernen ({})",
                    plural(count, "Ordner", "Ordner")
                )),
            )
            .on_hover_text("Fragt vorher nach. Dateien auf der Platte werden nie angefasst.")
            .clicked()
        {
            self.missing.confirm = true;
        }
    }

    fn missing_confirmation(&mut self, ui: &mut egui::Ui) {
        let (files, photos, faces) = self.missing.chosen_impact();
        ui.label(RichText::new("Einträge fehlender Dateien entfernen?").strong());
        ui.label(format!(
            "{} aus {} werden aus der Datenbank entfernt. Dateien auf der Platte werden nicht \
             angefasst; eine Datei, die inzwischen wieder da ist, bleibt im Bestand.",
            plural(files, "Dateieintrag", "Dateieinträge"),
            plural(self.missing.selected.len(), "Ordner", "Ordnern"),
        ));
        if photos > 0 {
            ui.label(format!(
                "{} haben danach keine Datei mehr und sind in der Sammlung nicht mehr zu sehen. \
                 Foto-IDs, Gesichter und Personenzuordnungen bleiben gespeichert.",
                plural(photos, "Foto", "Fotos")
            ));
        }
        if faces > 0 {
            ui.label(
                RichText::new(format!(
                    "Achtung: {faces} bestätigte Gesichtszuordnungen hängen an diesen Fotos. Sie \
                     bleiben gespeichert, sind aber ohne Bild. Wurden die Dateien nur umbenannt, \
                     besser zuerst „Erneut suchen“."
                ))
                .color(theme::WARM),
            );
        }
        ui.label(
            RichText::new(
                "Vorher wird automatisch eine Datensicherung erstellt (Einstellungen → \
                 Datensicherung). Scheitert sie oder wird sie abgebrochen, wird nichts entfernt.",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Endgültig entfernen"))
                .clicked()
            {
                self.remove_missing();
            }
            if ui.button("Zurück").clicked() {
                self.missing.confirm = false;
            }
        });
    }
}
