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
                egui::Button::new("YuNet und SFace installieren"),
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
                                "ONNX Runtime wird geladen"
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
                egui::Button::new("ONNX Runtime herunterladen"),
            )
            .on_hover_text("Offizielles Microsoft-Paket für Windows x64 · ca. 72 MB")
            .clicked()
        {
            self.launch(
                "ONNX Runtime herunterladen",
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

    pub(super) fn settings_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "DEIN ARBEITSPLATZ",
            "Alles lokal. Alles an seinem Platz.",
            "Lokale Werkzeuge und Modelle konfigurieren. Adobe-Versionen werden höchstens einmal täglich abgefragt.",
        );
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
                ui.label(RichText::new("Nur eine HEAD-Anfrage an Adobe; kein Installer-Download und keine Übertragung von Fotos.").small().weak());
                ui.hyperlink_to("Adobe DNG Converter herunterladen",crate::core::dng_updates::DOWNLOAD);ui.hyperlink_to("ExifTool installieren","https://exiftool.org/");
            });
            ui.add_space(16.0);
            theme::card(ui).show(ui,|ui|{
                ui.label(RichText::new("Lokale Gesichtserkennung").size(20.0).strong());
                ui.label(if Path::new(&self.settings.onnx_runtime).is_file() {
                    "ONNX Runtime · DLL ausgewählt"
                } else {
                    "ONNX Runtime · DLL fehlt"
                });
                self.runtime_download_button(ui);
                self.model_install_button(ui);
                ui.collapsing("Runtime und Modellordner manuell auswählen", |ui| {
                    for (pick, label, value, enabled) in [
                        (Pick::Runtime, "ONNX Runtime DLL", &mut self.settings.onnx_runtime, !downloading),
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
                });
                ui.add_enabled_ui(!installing, |ui| {
                    for(kind,selected)in [("detector",&mut self.detector),("embedder",&mut self.embedder)]{egui::ComboBox::from_id_salt(kind).selected_text(selected.clone()).show_ui(ui,|ui|{for model in self.models.iter().filter(|m|m.kind==kind){ui.selectable_value(selected,model.id.clone(),&model.name);}});}
                });
                for model in &self.models{ui.label(format!("{} · {}",model.name,model.license));}
                ui.label(RichText::new("Die Installation prüft die Gewichte mit SHA-256. Rust-Adapter verwenden getrennte Modellschlüssel.").small().weak());
            });
            ui.add_space(16.0);
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
        Self::header(
            ui,
            "FORTSCHRITT & VERLAUF",
            "Du behältst den Überblick.",
            "Aufträge laufen im Hintergrund. Abbrechen beendet laufende Arbeit kooperativ.",
        );
        for running in &self.tasks {
            theme::card(ui).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(&running.task.label);
                    if ui.button("Abbrechen").clicked() {
                        if running.kind == "maintenance" {
                            self.maintenance_paused = true;
                        }
                        running.task.control.stop();
                    }
                });
            });
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in self.history.iter().rev() {
                ui.label(line);
            }
        });
    }
}
