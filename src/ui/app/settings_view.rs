use super::*;
impl PhotoApp {
    pub(super) fn settings_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "DEIN ARBEITSPLATZ",
            "Alles lokal. Alles an seinem Platz.",
            "Lokale Werkzeuge und Modelle konfigurieren. Adobe-Versionen werden höchstens einmal täglich abgefragt.",
        );
        let mut chosen = None;
        egui::ScrollArea::vertical().show(ui,|ui|{
            theme::card().show(ui,|ui|{
                ui.label(RichText::new("Externe Werkzeuge").size(20.0).strong());
                for(pick,label,value)in [(Pick::Converter,"Adobe DNG Converter",&mut self.settings.converter),(Pick::Exiftool,"ExifTool",&mut self.settings.exiftool),(Pick::Runtime,"ONNX Runtime DLL",&mut self.settings.onnx_runtime),(Pick::Models,"Modellordner",&mut self.settings.model_dir)]{
                    ui.label(label);ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(value).desired_width(570.0));if ui.button("Auswählen …").clicked(){chosen=Some(pick);}});
                }
                ui.label(&self.dng_status);
                if ui.add_enabled(!self.tasks.iter().any(|t|t.kind=="dngupdates"),egui::Button::new("Converter-Version prüfen")).clicked(){self.check_dng();}
                ui.label(RichText::new("Nur eine HEAD-Anfrage an Adobe; kein Installer-Download und keine Übertragung von Fotos.").small().weak());
                ui.hyperlink_to("Adobe DNG Converter herunterladen",crate::core::dng_updates::DOWNLOAD);ui.hyperlink_to("ExifTool installieren","https://exiftool.org/");
            });
            ui.add_space(16.0);
            theme::card().show(ui,|ui|{
                ui.label(RichText::new("Lokale Gesichtserkennung").size(20.0).strong());
                for(kind,selected)in [("detector",&mut self.detector),("embedder",&mut self.embedder)]{egui::ComboBox::from_id_salt(kind).selected_text(selected.clone()).show_ui(ui,|ui|{for model in self.models.iter().filter(|m|m.kind==kind){ui.selectable_value(selected,model.id.clone(),&model.name);}});}
                for model in &self.models{ui.label(format!("{} · {}",model.name,model.license));}
                ui.label(RichText::new("Keine automatischen Gewichtsdownloads. SHA-256 wird vor dem Laden geprüft. Rust-Adapter verwenden getrennte Modellschlüssel.").small().weak());
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
            theme::card().show(ui, |ui| {
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
