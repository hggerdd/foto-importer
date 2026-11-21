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
        if self.large {
            let mut open = true;
            egui::Window::new("Große Bildansicht")
                .open(&mut open)
                .default_size([980.0, 720.0])
                .show(ctx, |ui| {
                    if let Some(path) = self.source_preview.clone() {
                        ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                        let request = PreviewRequest {
                            root: Some(PathBuf::from(&self.settings.source)),
                            path,
                            orientation: 1,
                            crop: None,
                            edge: 1920,
                            generation: 0,
                        };
                        self.thumbnails.image(
                            ui,
                            request,
                            &self.settings.exiftool,
                            egui::vec2(
                                ui.available_width(),
                                (ui.available_height() - 35.0).max(200.0),
                            ),
                        );
                    } else if let Some(photo) = self.selected_photo.clone() {
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
                        let image_ready = self.thumbnails.ready(&request);
                        let response =
                            self.thumbnails
                                .image(ui, request, &self.settings.exiftool, size);
                        if image_ready {
                            self.overlay(ui, response.rect, &photo);
                        }
                        ui.checkbox(&mut self.show_frames, "Gesichtsrahmen anzeigen");
                    }
                });
            self.large = open;
        }
        if let Some(face) = self.naming {
            let mut open = true;
            let mut submitted = false;
            egui::Window::new(format!("Gesicht #{face} benennen"))
                .open(&mut open)
                .default_width(600.0)
                .show(ctx, |ui| {
                    let mut submit = false;
                    ui.label(
                        "Bestehende Person wählen oder neuen Namen eingeben.                          Nur dieses Gesicht wird benannt.",
                    );
                    let busy = self.naming_blocked();
                    ui.label(
                        RichText::new("Ähnlichkeitsvorschläge · die Werte sind keine Garantie")
                            .small()
                            .weak(),
                    );
                    ui.horizontal_wrapped(|ui| {
                        for index in 0..3 {
                            if let Some((person, name, score)) =
                                self.name_suggestions.get(index).cloned()
                            {
                                let detail = crate::core::progress::decimal(score, 2);
                                if Self::suggestion_chip(
                                    &mut self.thumbnails,
                                    ui,
                                    &self.settings.collection,
                                    &self.settings.exiftool,
                                    person,
                                    self.name_portraits.get(&person),
                                    &name,
                                    &detail,
                                    None,
                                    !busy,
                                ) {
                                    self.name_person = Some(person);
                                    self.name_input = name;
                                    submit = true;
                                }
                            } else {
                                ui.add_enabled(false, egui::Button::new("Keine Referenz"));
                            }
                        }
                    });
                    let input = ui.text_edit_singleline(&mut self.name_input);
                    if input.changed() {
                        self.name_person = None;
                    }
                    if input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
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
                                select::person_caption(
                                    &self.face_page.people,
                                    person.id,
                                    &person.name,
                                ),
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
                        !busy && !self.name_input.trim().is_empty(),
                    ) || submit && !busy
                    {
                        let name = self.name_input.clone();
                        let person = self.name_person;
                        let config = self.settings.central();
                        let hidden = vec![face];
                        // Der Dialog schließt sofort; das Gesicht verschwindet aus der Seite,
                        // geschrieben wird dahinter (wie bei Gruppenkarten).
                        submitted = true;
                        let work = self.on_collection(move |mut c, _| {
                            let (undo, note) = crate::core::central::name_group_synced(
                                &mut c,
                                &config,
                                &[face],
                                &BTreeSet::from([face]),
                                person,
                                &name,
                                false,
                            )?;
                            Ok(Outcome::Synced(Box::new(Outcome::Named(undo)), note))
                        });
                        self.queue_hiding("Gesicht benennen", hidden, work);
                    }
                });
            if !open || submitted {
                self.naming = None;
            }
        }
    }
}
