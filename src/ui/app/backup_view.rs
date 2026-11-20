//! Abschnitt „Datensicherung“ der Einstellungen: Sicherung erstellen (Worker mit Fortschritt
//! und Abbruch), vorhandene Sicherungen der Sammlung, Anzahl der aufbewahrten Sicherungen.
//! Lesen und Schreiben geschieht nur auf Workern (`core::backup`).
use super::*;
use crate::core::backup::{self, BackupFile};

#[derive(Default)]
pub(super) struct BackupUi {
    /// Revision des zuletzt gestarteten Lesens; ältere Antworten werden verworfen.
    pub(super) revision: u64,
    /// Gelesene Sicherungen; `None`, solange (neu) gelesen werden muss.
    pub(super) files: Option<Vec<BackupFile>>,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
}

impl BackupUi {
    pub(super) fn accept(&mut self, result: std::result::Result<Vec<BackupFile>, String>) {
        self.loading = false;
        match result {
            Ok(files) => {
                self.files = Some(files);
                self.error = None;
            }
            Err(error) => {
                self.files = Some(Vec::new());
                self.error = Some(error);
            }
        }
    }
}

impl PhotoApp {
    pub(super) fn backup_plan(&self) -> backup::Plan {
        backup::Plan {
            app: self.settings.backup_app_data(),
            keep: self.settings.backup_keep,
        }
    }

    /// Plan der automatischen Sicherungen (vor Migration, vor „Einträge entfernen“) auf den
    /// Stand der Einstellungen bringen.
    pub(super) fn configure_backup(&self) {
        backup::configure(self.backup_plan());
    }

    pub(super) fn load_backups(&mut self) {
        if self.settings.collection.is_empty() {
            return;
        }
        self.backups.revision += 1;
        self.backups.loading = true;
        let revision = self.backups.revision;
        let root = PathBuf::from(&self.settings.collection);
        self.launch("Sicherungen lesen", false, "backup-list", move |_| {
            Ok(Outcome::Backups(
                revision,
                backup::list(&root).map_err(|e| format!("{e:#}")),
            ))
        });
    }

    fn start_backup(&mut self) {
        if self.settings.collection.is_empty() || self.backup_running() {
            return;
        }
        let root = PathBuf::from(&self.settings.collection);
        let plan = self.backup_plan();
        self.launch(
            "Datensicherung erstellen",
            false,
            "backup",
            move |control| {
                Ok(Outcome::BackupCreated(
                    backup::create(&root, &plan, "manuell", &control)?.summary(),
                ))
            },
        );
    }

    pub(super) fn backup_running(&self) -> bool {
        self.tasks.iter().any(|t| t.kind == "backup")
    }

    /// Karte „Datensicherung“ in den Einstellungen.
    pub(super) fn backup_settings(&mut self, ui: &mut egui::Ui) {
        theme::card(ui).show(ui, |ui| {
            ui.label(RichText::new("Datensicherung").size(20.0).strong());
            ui.label(
                "Sichert die Sammlungsdatenbank (Gesichter, Personen, Beziehungen, Entscheidungen), \
                 den zentralen Personenbestand und die Einstellungen als ZIP-Datei im \
                 Sammlungsordner. Die Fotos selbst werden nicht gesichert.",
            );
            ui.label(
                RichText::new(
                    "Vor einer Schema-Migration und vor „Einträge entfernen“ entsteht automatisch \
                     eine Sicherung; scheitert sie, unterbleibt der Schritt.",
                )
                .small()
                .weak(),
            );
            if self.settings.collection.is_empty() {
                ui.label("Erst eine Sammlung wählen.");
                return;
            }
            if self.backups.files.is_none() && !self.backups.loading {
                self.load_backups();
            }
            ui.add_space(4.0);
            let running = self
                .tasks
                .iter()
                .find(|t| t.kind == "backup")
                .map(|t| (t.task.control.clone(), t.task.control.snapshot()));
            match running {
                Some((control, progress)) => {
                    let mut stop = false;
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(if progress.phase.is_empty() {
                            "Sicherung wird vorbereitet …".to_owned()
                        } else {
                            progress.phase.clone()
                        });
                        stop = ui
                            .add_enabled(
                                !control.cancel.load(Ordering::Relaxed),
                                egui::Button::new("Abbrechen"),
                            )
                            .clicked();
                    });
                    Self::progress_bar(ui, &progress, ui.available_width().min(520.0));
                    if stop {
                        control.stop();
                    }
                }
                None => {
                    if ui
                        .button("Sicherung erstellen")
                        .on_hover_text(format!(
                            "Legt {}… im Sammlungsordner an. Eine vorhandene Datei wird nie ersetzt.",
                            crate::config::BACKUP_PREFIX
                        ))
                        .clicked()
                    {
                        self.start_backup();
                    }
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Sicherungen behalten");
                let (low, high) = crate::config::BACKUP_KEEP_RANGE;
                let mut keep = self.settings.backup_keep.clamp(low, high);
                let response = ui.add(egui::DragValue::new(&mut keep).range(low..=high));
                if response.changed() && keep != self.settings.backup_keep {
                    self.settings.backup_keep = keep;
                    self.save();
                }
                ui.label(
                    RichText::new("Ältere werden nach der nächsten Sicherung gelöscht.")
                        .small()
                        .weak(),
                );
            });
            ui.add_space(6.0);
            if let Some(error) = &self.backups.error {
                ui.label(RichText::new(format!("Sicherungen nicht lesbar: {error}")).color(theme::WARM));
            }
            match &self.backups.files {
                None => {
                    ui.label("Wird gelesen …");
                }
                Some(files) if files.is_empty() => {
                    if self.backups.error.is_none() {
                        ui.label("Noch keine Sicherung in dieser Sammlung.");
                    }
                }
                Some(files) => {
                    egui::Grid::new("backup-files")
                        .num_columns(3)
                        .striped(true)
                        .spacing(egui::vec2(16.0, 4.0))
                        .show(ui, |ui| {
                            for file in files.iter().rev() {
                                ui.label(format!("Nr. {:03}", file.number));
                                ui.label(file.modified.map_or_else(String::new, |time| {
                                    chrono::DateTime::<chrono::Local>::from(time)
                                        .format("%d.%m.%Y %H:%M")
                                        .to_string()
                                }));
                                ui.label(backup::megabytes(file.bytes));
                                ui.end_row();
                            }
                        });
                }
            }
        });
    }
}
