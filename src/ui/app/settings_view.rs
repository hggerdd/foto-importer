use super::*;

/// Pfadfelder der Einstellungen. Sie werden bei Fokusverlust, Enter oder nach „Auswählen …“
/// geprüft und sofort gespeichert; es gibt keinen Speichern-Knopf.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum PathSetting {
    Converter,
    Exiftool,
    Runtime,
    Models,
    Central,
}

impl PathSetting {
    fn label(self) -> &'static str {
        match self {
            Self::Converter => "Adobe DNG Converter",
            Self::Exiftool => "ExifTool",
            Self::Runtime => "Laufzeit",
            Self::Models => "Modellordner",
            Self::Central => "Ort der Zentraldatei",
        }
    }
}

/// Text eines Pfadfelds, der noch nicht gilt. `pending`: seit der letzten Prüfung geändert.
#[derive(Default)]
pub(super) struct PathDraft {
    pub(super) text: String,
    pub(super) pending: bool,
}

/// Prüft einen Pfad, bevor er gespeichert wird. Für den Modellordner kommen die gelesenen
/// Modellbeschreibungen mit zurück.
fn check_path_setting(
    field: PathSetting,
    value: &str,
) -> std::result::Result<Option<Vec<crate::core::inference::Manifest>>, String> {
    let path = Path::new(value);
    match field {
        PathSetting::Converter | PathSetting::Runtime => {
            if !path.is_file() {
                return Err(format!(
                    "{}: Datei nicht gefunden ({value}). Der bisherige Pfad bleibt gespeichert.",
                    field.label()
                ));
            }
        }
        PathSetting::Exiftool => {
            // Ein bloßer Befehlsname (Standard „exiftool“) wird über PATH gesucht.
            let command = !value.contains(['/', '\\']);
            if value.is_empty() || !command && !path.is_file() {
                return Err(format!(
                    "ExifTool: Programm nicht gefunden ({value}). Der bisherige Pfad bleibt gespeichert."
                ));
            }
        }
        PathSetting::Models => {
            let models = crate::core::inference::manifests(path)
                .map_err(|error| format!("Modellordner nicht übernommen: {error:#}"))?;
            if !["detector", "embedder"]
                .iter()
                .all(|kind| models.iter().any(|model| model.kind == *kind))
            {
                return Err(format!(
                    "Modellordner nicht übernommen: In {value} fehlen Modellbeschreibungen zum Finden und Vergleichen von Gesichtern."
                ));
            }
            return Ok(Some(models));
        }
        PathSetting::Central => {
            let folder = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty());
            if !value.is_empty() && (path.is_dir() || folder.is_none_or(|f| !f.is_dir())) {
                return Err(format!(
                    "Zentraldatei: {value} ist kein Dateipfad in einem vorhandenen Ordner. Der bisherige Ort bleibt gespeichert."
                ));
            }
        }
    }
    Ok(None)
}

impl PhotoApp {
    fn path_setting(&mut self, field: PathSetting) -> &mut String {
        match field {
            PathSetting::Converter => &mut self.settings.converter,
            PathSetting::Exiftool => &mut self.settings.exiftool,
            PathSetting::Runtime => &mut self.settings.onnx_runtime,
            PathSetting::Models => &mut self.settings.model_dir,
            PathSetting::Central => &mut self.settings.central_path,
        }
    }

    /// Prüft, speichert und wendet einen Pfad an. Ein unveränderter Wert löst nichts aus; ein
    /// ungültiger bleibt im Feld stehen, wird gemeldet und nicht gespeichert.
    pub(super) fn set_path_setting(&mut self, field: PathSetting, value: String) -> bool {
        let value = value.trim().to_owned();
        if *self.path_setting(field) == value {
            self.path_drafts.remove(&field);
            return true;
        }
        let models = match check_path_setting(field, &value) {
            Ok(models) => models,
            Err(problem) => {
                self.path_drafts.insert(
                    field,
                    PathDraft {
                        text: value,
                        pending: false,
                    },
                );
                self.message(problem, true);
                return false;
            }
        };
        let previous = std::mem::replace(self.path_setting(field), value.clone());
        if !self.save() {
            *self.path_setting(field) = previous;
            self.path_drafts.insert(
                field,
                PathDraft {
                    text: value,
                    pending: false,
                },
            );
            return false;
        }
        self.path_drafts.remove(&field);
        match field {
            // Ein neuer ExifTool-Pfad gibt dem gestoppten Hintergrundlauf eine neue Chance.
            PathSetting::Exiftool => self.metadata_bg_failed = false,
            // Liest nur die installierte Version neu; Adobe wird höchstens einmal täglich gefragt.
            PathSetting::Converter => self.check_dng(),
            PathSetting::Models => self.models = models.unwrap_or_default(),
            PathSetting::Runtime | PathSetting::Central => {}
        }
        self.message(format!("{} gespeichert", field.label()), false);
        true
    }

    /// Übernimmt geänderte, noch nicht geprüfte Pfadfelder (z. B. beim Verlassen der Ansicht).
    pub(super) fn commit_path_drafts(&mut self) {
        let pending: Vec<_> = self
            .path_drafts
            .iter()
            .filter(|(_, draft)| draft.pending)
            .map(|(field, _)| *field)
            .collect();
        for field in pending {
            self.commit_path_draft(field);
        }
    }

    fn commit_path_draft(&mut self, field: PathSetting) {
        if let Some(draft) = self.path_drafts.get_mut(&field) {
            draft.pending = false;
            let text = draft.text.clone();
            self.set_path_setting(field, text);
        }
    }

    /// Einzeiliges Pfadfeld: Tippen ändert nur den Entwurf; Fokusverlust oder Enter übernimmt.
    pub(super) fn path_field(
        &mut self,
        ui: &mut egui::Ui,
        field: PathSetting,
        hint: Option<String>,
    ) {
        let mut text = match self.path_drafts.get(&field) {
            Some(draft) => draft.text.clone(),
            None => self.path_setting(field).clone(),
        };
        let mut edit = egui::TextEdit::singleline(&mut text).desired_width(570.0);
        if let Some(hint) = hint {
            edit = edit.hint_text(hint);
        }
        let response = ui.add(edit);
        if response.changed() {
            self.path_drafts.insert(
                field,
                PathDraft {
                    text,
                    pending: true,
                },
            );
        }
        if !response.has_focus() && self.path_drafts.get(&field).is_some_and(|d| d.pending) {
            self.commit_path_draft(field);
        }
    }

    /// Wählt Detektor oder Embedder; nur eine echte Änderung schreibt in die Sammlung.
    pub(super) fn choose_model(&mut self, kind: &str, id: String) {
        let current = if kind == "detector" {
            &mut self.detector
        } else {
            &mut self.embedder
        };
        if *current == id {
            return;
        }
        *current = id;
        if self.settings.collection.is_empty() {
            return;
        }
        let folder = self.settings.model_dir.clone();
        let detector = self.detector.clone();
        let embedder = self.embedder.clone();
        self.launch_write(
            "Sammlungsmodelle speichern",
            JobKind::Mutation,
            move |mut c, _| {
                faces::configure_models(&mut c, Path::new(&folder), &detector, &embedder)?;
                Ok(Outcome::Done(
                    "Modellauswahl der Sammlung gespeichert".into(),
                ))
            },
        );
    }

    pub(super) fn models_installing(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.kind == JobKind::ModelInstall)
    }

    pub(super) fn model_weights_present(&self) -> bool {
        crate::core::face_models::weights_present(
            &self.models,
            Path::new(&self.settings.model_dir),
            &self.detector,
            &self.embedder,
        )
    }

    pub(super) fn runtime_downloading(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.kind == JobKind::RuntimeDownload)
    }

    /// Laufzeit vorhanden (oder auf dieser Plattform nicht ladbar)?
    pub(super) fn runtime_ready(&self) -> bool {
        !crate::core::onnx_runtime::SUPPORTED || Path::new(&self.settings.onnx_runtime).is_file()
    }

    /// Nur für die Standardmodelle gibt es eine Installation auf Klick.
    fn standard_models(&self) -> bool {
        self.detector == "yunet" && self.embedder == "sface"
    }

    /// Startet die Einrichtung: erst die Laufzeit, wenn sie fehlt, danach die Modelle.
    pub(super) fn start_face_setup(&mut self) {
        if self.runtime_downloading() || self.models_installing() {
            return;
        }
        if !self.runtime_ready() {
            self.setup_chain = true;
            self.start_runtime_download();
        } else if !self.model_weights_present() && self.standard_models() {
            self.start_model_install();
        }
    }

    /// Zweiter Schritt nach geladener Laufzeit; das Ergebnis gehört zur Ansicht des Starts.
    pub(super) fn continue_face_setup(&mut self, view: View) {
        if self.model_weights_present() || !self.standard_models() || self.models_installing() {
            return;
        }
        self.start_model_install();
        if let Some(task) = self.tasks.last_mut() {
            task.view = view;
        }
    }

    fn start_runtime_download(&mut self) {
        self.launch(
            "Laufzeit herunterladen",
            false,
            JobKind::RuntimeDownload,
            |control| {
                let settings = std::path::absolute(Settings::path())?;
                let destination = settings
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("Einstellungspfad ungültig"))?
                    .join("runtime");
                Ok(Outcome::RuntimeDownloaded(
                    crate::core::onnx_runtime::download(&destination, &control)?,
                ))
            },
        );
    }

    fn start_model_install(&mut self) {
        self.start_model_install_with(false);
    }

    /// Installiert die Standardmodelle und auf Wunsch das genauere Vergleichsmodell in einen
    /// neuen Modellordner (vorhandene Gewichte werden übernommen).
    pub(super) fn start_model_install_with(&mut self, strong: bool) {
        let candidates = config::model_sources(&self.settings.model_dir);
        self.launch(
            if strong {
                "Genaueres Modell installieren"
            } else {
                "Modellgewichte installieren"
            },
            false,
            JobKind::ModelInstall,
            move |control| {
                let settings = std::path::absolute(Settings::path())?;
                let destination = settings
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("Einstellungspfad ungültig"))?
                    .join("models");
                let (path, models) = crate::core::face_models::install_models(
                    &destination,
                    &candidates,
                    &control,
                    strong,
                )?;
                Ok(Outcome::ModelsInstalled(path, models))
            },
        );
    }

    /// Ein Knopf für Laufzeit und Modelle, mit Fortschritt und einem „Abbrechen“.
    pub(super) fn face_setup_button(&mut self, ui: &mut egui::Ui) {
        if let Some(running) = self
            .tasks
            .iter()
            .find(|task| matches!(task.kind, JobKind::RuntimeDownload | JobKind::ModelInstall))
        {
            let done = running.task.control.done.load(Ordering::Relaxed);
            let total = running.task.control.total.load(Ordering::Relaxed);
            let runtime = running.kind == JobKind::RuntimeDownload;
            let step = match (runtime, self.setup_chain) {
                (true, true) => "Schritt 1 von 2 · ",
                _ => "",
            };
            let text = if !runtime {
                "Modellgewichte werden installiert".to_owned()
            } else if done < total {
                format!("{step}Laufzeit wird geladen")
            } else {
                format!("{step}Paket wird geprüft und installiert")
            };
            let mut stop = false;
            ui.horizontal(|ui| {
                if total == 0 {
                    ui.spinner();
                    ui.label(if runtime {
                        "Verbindung zu Microsoft …"
                    } else {
                        "Modellgewichte werden vorbereitet …"
                    });
                } else {
                    ui.add(
                        egui::ProgressBar::new(done as f32 / total as f32)
                            .desired_width(230.0)
                            .text(text),
                    );
                }
                stop = ui.button("Abbrechen").clicked();
            });
            if stop {
                running.task.control.stop();
                self.setup_chain = false;
            }
            return;
        }
        let runtime = self.runtime_ready();
        let models = self.model_weights_present();
        if runtime && models {
            ui.label("Gesichtserkennung ist eingerichtet.");
            return;
        }
        if runtime && !self.standard_models() {
            ui.label(
                "Für diese Modellauswahl bitte einen Ordner mit den passenden Gewichten wählen.",
            );
            return;
        }
        let hint = match (runtime, models) {
            (false, false) => {
                "Lädt die Laufzeit von Microsoft (ca. 72 MB) und danach die Modelle (vorhandene Gewichte oder rund 39 MB von OpenCV Zoo)"
            }
            (false, true) => "Lädt die Laufzeit von Microsoft (Windows x64, ca. 72 MB)",
            _ => "Vorhandene Gewichte verwenden oder rund 39 MB von OpenCV Zoo laden",
        };
        if ui
            .add_enabled(
                !self.tasks.iter().any(|task| task.kind == JobKind::Picker),
                egui::Button::new("Gesichtserkennung einrichten"),
            )
            .on_hover_text(hint)
            .clicked()
        {
            self.start_face_setup();
        }
    }

    /// Regler für die Vorschlagsschwelle der aktiven Sammlung; gespeichert wird beim Loslassen.
    fn review_threshold_slider(&mut self, ui: &mut egui::Ui) {
        let ready =
            !self.settings.collection.is_empty() && !self.face_page.status.embedder.is_empty();
        let stored = self.face_page.status.review_threshold;
        // Bereich des aktiven Vergleichsmodells (Skalen verschiedener Modelle unterscheiden sich).
        let (low, high) = config::model_scale(&self.face_page.status.embedder).review_range;
        let mut value = self
            .review_draft
            .as_ref()
            .filter(|(root, _)| *root == self.settings.collection)
            .map_or(stored, |(_, value)| *value)
            .clamp(low, high);
        ui.add_space(8.0);
        ui.label("Strenge der Namensvorschläge");
        let response = ui.add_enabled(
            ready && !self.busy(),
            egui::Slider::new(&mut value, low..=high)
                .step_by(0.01)
                .fixed_decimals(2),
        );
        ui.label(
            RichText::new(if ready {
                "Höher: weniger, aber sicherere Vorschläge."
            } else {
                "Einstellbar, sobald die Fotos einer Sammlung analysiert sind."
            })
            .small()
            .weak(),
        );
        if response.changed() {
            self.review_draft = Some((self.settings.collection.clone(), value));
        }
        if (response.drag_stopped() || response.lost_focus()) && (value - stored).abs() > 0.004 {
            self.save_review_threshold(value);
        }
    }

    /// Speichert die Schwelle auf einem Worker; der Abgleich beginnt danach von vorn.
    pub(super) fn save_review_threshold(&mut self, value: f32) {
        let root = self.settings.collection.clone();
        self.review_draft = Some((root.clone(), value));
        self.launch_write(
            "Strenge der Namensvorschläge speichern",
            JobKind::Mutation,
            move |mut collection, _| {
                crate::core::recognition::set_review_threshold(&mut collection, value)?;
                Ok(Outcome::Done(
                    "Strenge gespeichert. Die Namensvorschläge werden neu berechnet.".into(),
                ))
            },
        );
    }

    /// Jede Einstellung wirkt sofort: Schalter beim Klick, Pfade bei Fokusverlust oder Enter.
    pub(super) fn settings_view(&mut self, ui: &mut egui::Ui) {
        Self::header(ui, "Einstellungen");
        let mut chosen = None;
        let downloading = self.runtime_downloading();
        let installing = self.models_installing();
        egui::ScrollArea::vertical().show(ui, |ui| {
            theme::card(ui).show(ui, |ui| {
                ui.label(RichText::new("Externe Werkzeuge").size(20.0).strong());
                for (pick, field) in [
                    (Pick::Converter, PathSetting::Converter),
                    (Pick::Exiftool, PathSetting::Exiftool),
                ] {
                    ui.label(field.label());
                    ui.horizontal(|ui| {
                        self.path_field(ui, field, None);
                        if ui.button("Auswählen …").clicked() {
                            chosen = Some(pick);
                        }
                    });
                }
                ui.label(&self.dng_status);
                if ui
                    .add_enabled(
                        !self.tasks.iter().any(|t| t.kind == JobKind::DngUpdates),
                        egui::Button::new("Converter-Version prüfen"),
                    )
                    .clicked()
                {
                    self.check_dng();
                }
                ui.label(
                    RichText::new("Es wird nur die Versionsnummer bei Adobe abgefragt. Keine Fotos werden übertragen.")
                        .small()
                        .weak(),
                );
                ui.hyperlink_to(
                    "Adobe DNG Converter herunterladen",
                    crate::core::dng_updates::DOWNLOAD,
                );
                ui.hyperlink_to("ExifTool installieren", "https://exiftool.org/");
            });
            ui.add_space(16.0);
            theme::card(ui).show(ui, |ui| {
                ui.label(RichText::new("Lokale Gesichtserkennung").size(20.0).strong());
                ui.label(format!(
                    "Laufzeit: {} · Modelle: {}",
                    if self.runtime_ready() { "installiert" } else { "fehlt" },
                    if self.model_weights_present() { "installiert" } else { "fehlen" },
                ));
                self.face_setup_button(ui);
                ui.collapsing("Erweitert", |ui| {
                    for (pick, field, label, enabled) in [
                        (Pick::Runtime, PathSetting::Runtime, "Laufzeit (Programmdatei)", !downloading),
                        (Pick::Models, PathSetting::Models, "Modellordner", !installing),
                    ] {
                        ui.label(label);
                        ui.add_enabled_ui(enabled, |ui| {
                            ui.horizontal(|ui| {
                                self.path_field(ui, field, None);
                                if ui.button("Auswählen …").clicked() {
                                    chosen = Some(pick);
                                }
                            });
                        });
                    }
                    let mut choice = None;
                    ui.add_enabled_ui(!installing, |ui| {
                        for (kind, label, current) in [
                            ("detector", "Gesichter finden", &self.detector),
                            ("embedder", "Gesichter vergleichen", &self.embedder),
                        ] {
                            ui.label(label);
                            let mut selected = current.clone();
                            egui::ComboBox::from_id_salt(kind)
                                .selected_text(selected.clone())
                                .show_ui(ui, |ui| {
                                    for model in self.models.iter().filter(|m| m.kind == kind) {
                                        ui.selectable_value(&mut selected, model.id.clone(), &model.name);
                                    }
                                });
                            if selected != *current {
                                choice = Some((kind, selected));
                            }
                        }
                    });
                    if let Some((kind, id)) = choice {
                        self.choose_model(kind, id);
                    }
                    for model in &self.models {
                        ui.label(
                            RichText::new(format!("{} · {}", model.name, model.license))
                                .small()
                                .weak(),
                        );
                    }
                    self.review_threshold_slider(ui);
                });
            });
            ui.add_space(16.0);
            self.central_settings_card(ui);
            if ui.checkbox(&mut self.settings.dark, "Dunkle Oberfläche").changed() {
                theme::apply(ui.ctx(), self.settings.dark);
                self.save();
            }
            ui.add_space(16.0);
            self.backup_settings(ui);
            ui.add_space(16.0);
            self.diagnostic_settings(ui);
        });
        if let Some(pick) = chosen {
            self.pick(pick);
        }
    }
    /// Schalter „Diagnose-Log schreiben“ samt Logordner (ADR Anwendung).
    fn diagnostic_settings(&mut self, ui: &mut egui::Ui) {
        if ui
            .checkbox(&mut self.settings.diagnostic_log, "Diagnose-Log schreiben")
            .on_hover_text(
                "Zeichnet Ruckeln, Dauer von Aufträgen und Fehler auf, höchstens 300 MB; \
                 die ältesten Einträge werden gelöscht. Es verlässt den Rechner nicht.",
            )
            .changed()
        {
            let on = self.settings.diagnostic_log;
            if on {
                crate::core::diag::set_enabled(true);
            }
            log::info!(target: "einstellungen", "diagnose_log={on}");
            if !on {
                crate::core::diag::set_enabled(false);
            }
            self.save();
        }
        let dir = Settings::log_dir();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Logordner: {}", dir.display()))
                    .small()
                    .weak(),
            );
            // Der Ordner entsteht mit der ersten Zeile; vorher meldet der Explorer ihn als fehlend.
            ui.hyperlink_to(
                "Öffnen",
                format!("file:///{}", dir.display().to_string().replace('\\', "/")),
            );
        });
    }
    pub(super) fn tasks_view(&mut self, ui: &mut egui::Ui) {
        Self::header(ui, "Aufgaben");
        for running in &self.tasks {
            let progress = running.task.control.snapshot();
            let stopping = running.task.control.cancel.load(Ordering::Relaxed);
            theme::card(ui).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(&running.task.label).strong());
                    if !progress.phase.is_empty() {
                        ui.label(RichText::new(format!("· {}", progress.phase)).weak());
                    }
                    let can_pause = running.can_pause();
                    if ui
                        .add_enabled(
                            !stopping,
                            egui::Button::new(if can_pause { "Pausieren" } else { "Abbrechen" }),
                        )
                        .clicked()
                    {
                        if can_pause {
                            match running.kind {
                                JobKind::MetadataBg => self.metadata_paused = true,
                                JobKind::IndexNew => self.index_paused = true,
                                _ => self.maintenance_paused = true,
                            }
                        }
                        running.task.control.stop();
                    }
                    ui.label(
                        RichText::new(format!(
                            "seit {}",
                            progress_view::clock(running.task.started.elapsed())
                        ))
                        .weak(),
                    );
                });
                if stopping {
                    ui.label(
                        RichText::new("Wird abgebrochen … bereits Gespeichertes bleibt erhalten")
                            .weak(),
                    );
                } else {
                    Self::progress_bar(ui, &progress, ui.available_width().min(640.0));
                    if let Some(hint) = running.catch_up_note() {
                        ui.label(RichText::new(hint).small().weak());
                    }
                }
            });
        }
        self.import_queue_list(ui);
        if self.tasks.is_empty() {
            self.catch_up_row(ui);
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in self.history.iter().rev() {
                ui.label(line);
            }
        });
    }
}
