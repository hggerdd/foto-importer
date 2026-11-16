use super::*;
impl PhotoApp {
    pub(super) fn models_installing(&self) -> bool {
        self.tasks.iter().any(|task| task.kind == "model-install")
    }

    pub(super) fn model_weights_present(&self) -> bool {
        crate::core::face_models::weights_present(
            &self.models,
            Path::new(&self.settings.model_dir),
            &self.detector,
            &self.embedder,
        )
    }

    pub(super) fn model_install_button(&mut self, ui: &mut egui::Ui) {
        if let Some(running) = self.tasks.iter().find(|task| task.kind == "model-install") {
            let done = running.task.control.done.load(Ordering::Relaxed);
            let total = running.task.control.total.load(Ordering::Relaxed);
            ui.horizontal(|ui| {
                if total == 0 {
                    ui.spinner();
                } else {
                    ui.add(
                        egui::ProgressBar::new(done as f32 / total as f32)
                            .desired_width(230.0)
                            .text("Modellgewichte werden installiert"),
                    );
                }
                if ui.button("Abbrechen").clicked() {
                    running.task.control.stop();
                }
            });
            return;
        }
        if self.detector != "yunet" || self.embedder != "sface" {
            ui.label(
                "Für diese Modellauswahl bitte einen Ordner mit den passenden Gewichten wählen.",
            );
            return;
        }
        if ui
            .add_enabled(
                !self.tasks.iter().any(|task| task.kind == "picker"),
                egui::Button::new("Erkennungsmodelle installieren"),
            )
            .on_hover_text("Vorhandene Gewichte verwenden oder rund 39 MB von OpenCV Zoo laden")
            .clicked()
        {
            let candidates = config::model_sources(&self.settings.model_dir);
            self.launch(
                "Modellgewichte installieren",
                false,
                "model-install",
                move |control| {
                    let settings = std::path::absolute(Settings::path())?;
                    let destination = settings
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("Einstellungspfad ungültig"))?
                        .join("models");
                    let (path, models) =
                        crate::core::face_models::install(&destination, &candidates, &control)?;
                    Ok(Outcome::ModelsInstalled(path, models))
                },
            );
        }
    }

    pub(super) fn runtime_downloading(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.kind == "runtime-download")
    }

    pub(super) fn runtime_download_button(&mut self, ui: &mut egui::Ui) {
        if !crate::core::onnx_runtime::SUPPORTED {
            return;
        }
        if let Some(running) = self
            .tasks
            .iter()
            .find(|task| task.kind == "runtime-download")
        {
            let done = running.task.control.done.load(Ordering::Relaxed);
            let total = running.task.control.total.load(Ordering::Relaxed);
            ui.horizontal(|ui| {
                if total == 0 {
                    ui.spinner();
                    ui.label("Verbindung zu Microsoft …");
                } else {
                    ui.add(
                        egui::ProgressBar::new(done as f32 / total as f32)
                            .desired_width(230.0)
                            .text(if done < total {
                                "Laufzeit wird geladen"
                            } else {
                                "Paket wird geprüft und installiert"
                            }),
                    );
                }
                if ui.button("Abbrechen").clicked() {
                    running.task.control.stop();
                }
            });
        } else if ui
            .add_enabled(
                !self.tasks.iter().any(|task| task.kind == "picker"),
                egui::Button::new("Laufzeit herunterladen"),
            )
            .on_hover_text("Offizielles Microsoft-Paket für Windows x64 · ca. 72 MB")
            .clicked()
        {
            self.launch(
                "Laufzeit herunterladen",
                false,
                "runtime-download",
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
    }

    /// Regler für die Vorschlagsschwelle der aktiven Sammlung; gespeichert wird beim Loslassen.
    fn review_threshold_slider(&mut self, ui: &mut egui::Ui) {
        use crate::core::recognition::REVIEW_RANGE;
        let ready =
            !self.settings.collection.is_empty() && !self.face_page.status.embedder.is_empty();
        let stored = self.face_page.status.review_threshold;
        let mut value = self
            .review_draft
            .as_ref()
            .filter(|(root, _)| *root == self.settings.collection)
            .map_or(stored, |(_, value)| *value)
            .clamp(REVIEW_RANGE.0, REVIEW_RANGE.1);
        ui.add_space(8.0);
        ui.label("Strenge der Namensvorschläge");
        let response = ui.add_enabled(
            ready && !self.busy(),
            egui::Slider::new(&mut value, REVIEW_RANGE.0..=REVIEW_RANGE.1)
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
        self.launch(
            "Strenge der Namensvorschläge speichern",
            true,
            "mutation",
            move |_| {
                let mut collection = Collection::open(Path::new(&root), true)?;
                crate::core::recognition::set_review_threshold(&mut collection, value)?;
                Ok(Outcome::Done(
                    "Strenge gespeichert. Die Namensvorschläge werden neu berechnet.".into(),
                ))
            },
        );
    }

    pub(super) fn settings_view(&mut self, ui: &mut egui::Ui) {
        Self::header(ui, "Einstellungen");
        let mut chosen = None;
        let downloading = self.runtime_downloading();
        let installing = self.models_installing();
        egui::ScrollArea::vertical().show(ui,|ui|{
            theme::card(ui).show(ui,|ui|{
                ui.label(RichText::new("Externe Werkzeuge").size(20.0).strong());
                for(pick,label,value)in [(Pick::Converter,"Adobe DNG Converter",&mut self.settings.converter),(Pick::Exiftool,"ExifTool",&mut self.settings.exiftool)]{
                    ui.label(label);ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(value).desired_width(570.0));if ui.button("Auswählen …").clicked(){chosen=Some(pick);}});
                }
                ui.label(&self.dng_status);
                if ui.add_enabled(!self.tasks.iter().any(|t|t.kind=="dngupdates"),egui::Button::new("Converter-Version prüfen")).clicked(){self.check_dng();}
                ui.label(RichText::new("Es wird nur die Versionsnummer bei Adobe abgefragt. Keine Fotos werden übertragen.").small().weak());
                ui.hyperlink_to("Adobe DNG Converter herunterladen",crate::core::dng_updates::DOWNLOAD);ui.hyperlink_to("ExifTool installieren","https://exiftool.org/");
            });
            ui.add_space(16.0);
            theme::card(ui).show(ui,|ui|{
                ui.label(RichText::new("Lokale Gesichtserkennung").size(20.0).strong());
                ui.label(if Path::new(&self.settings.onnx_runtime).is_file() {
                    "Gesichtserkennung: Laufzeit installiert"
                } else {
                    "Gesichtserkennung: Laufzeit fehlt"
                });
                self.runtime_download_button(ui);
                self.model_install_button(ui);
                ui.collapsing("Erweitert", |ui| {
                    for (pick, label, value, enabled) in [
                        (Pick::Runtime, "Laufzeit (Programmdatei)", &mut self.settings.onnx_runtime, !downloading),
                        (Pick::Models, "Modellordner", &mut self.settings.model_dir, !installing),
                    ] {
                        ui.label(label);
                        ui.add_enabled_ui(enabled, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(value).desired_width(570.0));
                                if ui.button("Auswählen …").clicked() {
                                    chosen = Some(pick);
                                }
                            });
                        });
                    }
                    ui.add_enabled_ui(!installing, |ui| {
                        for(kind,label,selected)in [("detector","Gesichter finden",&mut self.detector),("embedder","Gesichter vergleichen",&mut self.embedder)]{ui.label(label);egui::ComboBox::from_id_salt(kind).selected_text(selected.clone()).show_ui(ui,|ui|{for model in self.models.iter().filter(|m|m.kind==kind){ui.selectable_value(selected,model.id.clone(),&model.name);}});}
                    });
                    for model in &self.models{ui.label(RichText::new(format!("{} · {}",model.name,model.license)).small().weak());}
                    self.review_threshold_slider(ui);
                });
            });
            ui.add_space(16.0);
            self.central_settings_card(ui);
            if ui.checkbox(&mut self.settings.dark,"Dunkle Oberfläche").changed(){theme::apply(ui.ctx(),self.settings.dark);}
            if theme::primary(ui,"Einstellungen speichern",true)&&self.save(){
                match crate::core::inference::manifests(Path::new(&self.settings.model_dir)){
                    Ok(models)=>{
                        self.models=models;
                        if self.settings.collection.is_empty(){self.message("Einstellungen gespeichert".into(),false);}
                        else{let root=self.settings.collection.clone();let folder=self.settings.model_dir.clone();let detector=self.detector.clone();let embedder=self.embedder.clone();self.launch("Sammlungsmodelle speichern",true,"mutation",move |_|{let mut c=Collection::open(Path::new(&root),true)?;faces::configure_models(&mut c,Path::new(&folder),&detector,&embedder)?;Ok(Outcome::Done("Einstellungen und Sammlungsmodelle gespeichert".into()))});}
                    },
                    Err(error)=>self.message(format!("Einstellungen gespeichert; Modellordner: {error}"),true),
                }
            }
        });
        if let Some(pick) = chosen {
            self.pick(pick);
        }
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
                            if running.kind == "metadata-bg" {
                                self.metadata_paused = true;
                            } else {
                                self.maintenance_paused = true;
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
