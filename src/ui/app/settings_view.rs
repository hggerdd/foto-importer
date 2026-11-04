use super::*;

impl PhotoApp {
    pub(super) fn settings_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "DEIN ARBEITSPLATZ",
            "Alles lokal. Alles an seinem Platz.",
            "Werkzeuge und Modelle werden nur verwendet, wenn du die entsprechende Funktion startest.",
        );
        let mut chosen = None;
        egui::ScrollArea::vertical().show(ui,|ui|{theme::card().show(ui,|ui|{ui.label(RichText::new("Externe Werkzeuge").size(20.0).strong());for(pick,label,value)in [(Pick::Converter,"Adobe DNG Converter",&mut self.settings.converter),(Pick::Exiftool,"ExifTool",&mut self.settings.exiftool),(Pick::Runtime,"ONNX Runtime DLL",&mut self.settings.onnx_runtime),(Pick::Models,"Modellordner",&mut self.settings.model_dir)]{ui.label(label);let mut browse=false;ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(value).desired_width(600.0));browse=ui.button("Auswählen …").clicked();});if browse{chosen=Some(pick);break;}}});
            ui.add_space(16.0);theme::card().show(ui,|ui|{ui.label(RichText::new("Lokale Gesichtserkennung").size(20.0).strong());for(kind,selected)in [("detector",&mut self.detector),("embedder",&mut self.embedder)]{egui::ComboBox::from_id_salt(kind).selected_text(selected.clone()).show_ui(ui,|ui|{for model in self.models.iter().filter(|m|m.kind==kind){ui.selectable_value(selected,model.id.clone(),&model.name);}});}for model in &self.models{ui.label(format!("{} · {}",model.name,model.license));}ui.label(RichText::new("Keine automatischen Gewichtsdownloads. SHA-256 wird vor dem Laden geprüft. Rust-Adapter verwenden getrennte Modellschlüssel.").small().weak());});
            ui.add_space(16.0);if ui.checkbox(&mut self.settings.dark,"Dunkle Oberfläche").changed(){theme::apply(ui.ctx(),self.settings.dark);}
if theme::primary(ui,"Einstellungen speichern",true){self.save();self.models=crate::core::inference::manifests(Path::new(&self.settings.model_dir)).unwrap_or_default();self.message("Einstellungen gespeichert".into(),false);}ui.hyperlink_to("Adobe DNG Converter herunterladen","https://adobe.com/go/dng_converter_win/");ui.hyperlink_to("ExifTool installieren","https://exiftool.org/");
        });
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
