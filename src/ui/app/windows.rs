use super::*;

impl PhotoApp {
    pub(super) fn windows(&mut self, ctx: &egui::Context) {
        if let Some(mut report) = self.report.clone() {
            let mut open = true;
            egui::Window::new("Auswertung")
                .open(&mut open)
                .default_size([720.0, 560.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut report)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY),
                        );
                    });
                });
            if !open {
                self.report = None;
            }
        }
        if let Some(mut person) = self.person_edit.clone() {
            let mut open = true;
            egui::Window::new(format!("Person #{} bearbeiten", person.id))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut person.name);
                    ui.label("Geburtsjahr");
                    ui.text_edit_singleline(&mut person.birth);
                    ui.label("Geschlecht");
                    ui.text_edit_singleline(&mut person.gender);
                    ui.label("Notiz");
                    ui.text_edit_multiline(&mut person.note);
                    if theme::primary(
                        ui,
                        "Änderungen speichern",
                        !self.busy() && !person.name.trim().is_empty(),
                    ) {
                        let root = self.settings.collection.clone();
                        let id = person.id;
                        let attrs = BTreeMap::from([
                            ("name".into(), person.name.clone()),
                            ("birth_year".into(), person.birth.clone()),
                            ("gender".into(), person.gender.clone()),
                            ("note".into(), person.note.clone()),
                        ]);
                        self.undo.clear();
                        self.launch("Person speichern", true, "mutation", move |_| {
                            let c = Collection::open(Path::new(&root), true)?;
                            crate::core::maintenance::set_person(&c, id, &attrs)?;
                            Ok(Outcome::Done("Person gespeichert".into()))
                        });
                    }
                });
            self.person_edit = if open { Some(person) } else { None };
        }
        if self.large {
            let mut open = true;
            egui::Window::new("Große Bildansicht")
                .open(&mut open)
                .default_size([980.0, 720.0])
                .show(ctx, |ui| {
                    if let Some(photo) = self.selected_photo.clone() {
                        let request = PreviewRequest {
                            root: Some(PathBuf::from(&self.settings.collection)),
                            path: Path::new(&self.settings.collection).join(&photo.source),
                            orientation: photo.orientation,
                            crop: None,
                            edge: 1920,
                            generation: 0,
                        };
                        let size = egui::vec2(
                            ui.available_width(),
                            (ui.available_height() - 45.0).max(200.0),
                        );
                        let response =
                            self.thumbnails
                                .image(ui, request, &self.settings.exiftool, size);
                        self.overlay(ui, response.rect, &photo);
                        ui.checkbox(&mut self.show_frames, "Gesichtsrahmen anzeigen");
                    }
                });
            self.large = open;
        }
        if let Some(face) = self.naming.clone() {
            let mut open = true;
            egui::Window::new(format!("Gesicht #{} benennen", face.id))
                .open(&mut open)
                .default_width(600.0)
                .show(ctx, |ui| {
                    ui.label("Bestehende Person wählen oder neuen Namen eingeben.");
                    ui.label(
                        RichText::new("Ähnlichkeitsvorschläge · keine Wahrscheinlichkeiten")
                            .small()
                            .weak(),
                    );
                    ui.horizontal_wrapped(|ui| {
                        for index in 0..6 {
                            if let Some((person, name, score)) =
                                self.name_suggestions.get(index).cloned()
                            {
                                if ui.button(format!("{name} · {score:.3}")).clicked() {
                                    self.name_person = Some(person);
                                    self.name_input = name;
                                }
                            } else {
                                ui.add_enabled(false, egui::Button::new("Keine Referenz"));
                            }
                        }
                    });
                    if ui.text_edit_singleline(&mut self.name_input).changed() {
                        self.name_person = None;
                    }
                    for person in self
                        .face_page
                        .people
                        .iter()
                        .filter(|p| files::fold(&p.name).contains(&files::fold(&self.name_input)))
                        .take(5)
                        .cloned()
                        .collect::<Vec<_>>()
                    {
                        if ui
                            .selectable_label(
                                self.name_person == Some(person.id),
                                format!("{} · #{}", person.name, person.id),
                            )
                            .clicked()
                        {
                            self.name_person = Some(person.id);
                            self.name_input = person.name.clone();
                        }
                    }
                    if theme::primary(
                        ui,
                        "Übernehmen",
                        !self.busy() && !self.name_input.trim().is_empty(),
                    ) {
                        let root = self.settings.collection.clone();
                        let name = self.name_input.clone();
                        let person = self.name_person;
                        self.launch("Gesicht benennen", true, "naming", move |_| {
                            let mut c = Collection::open(Path::new(&root), true)?;
                            Ok(Outcome::Named(faces::name_group(
                                &mut c,
                                &[face.id],
                                &BTreeSet::from([face.id]),
                                person,
                                &name,
                                false,
                            )?))
                        });
                    }
                });
            if !open {
                self.naming = None;
            }
        }
    }
}
