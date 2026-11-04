use super::*;

impl PhotoApp {
    pub(super) fn import_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "DEIN NÄCHSTER IMPORT",
            "Erinnerungen sicher ankommen lassen.",
            "Quelle wählen, Aufnahmetage sichten und das Ziel vor dem Kopieren prüfen.",
        );
        theme::card().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("01  QUELLE").color(theme::ACCENT).strong());
                if ui.button("Ordner wählen …").clicked() {
                    self.pick(Pick::Source);
                }
                if ui
                    .add_enabled(
                        !self.settings.source.is_empty(),
                        egui::Button::new("Neu einlesen"),
                    )
                    .clicked()
                {
                    self.begin_scan();
                }
                ui.add(
                    egui::Label::new(if self.settings.source.is_empty() {
                        "Speicherkarte oder Fotoordner"
                    } else {
                        &self.settings.source
                    })
                    .truncate(),
                );
            });
            let changed = ui
                .checkbox(
                    &mut self.settings.metadata_date,
                    "Nach Aufnahmedatum gruppieren · ohne Metadaten gilt das Dateisystemdatum",
                )
                .changed();
            if changed {
                self.save();
                if !self.settings.source.is_empty() {
                    self.begin_scan();
                }
            }
        });
        ui.add_space(12.0);
        if self.scan.files.is_empty() {
            self.empty(ui,"Deine nächste Geschichte beginnt hier.","Wähle eine Speicherkarte oder einen Ordner. Es werden keine Dateien verschoben oder gelöscht.");
            return;
        }
        let selected_files: Vec<_> = self
            .scan
            .files
            .iter()
            .filter(|f| self.selected.contains(&f.day))
            .cloned()
            .collect();
        let days: BTreeMap<String, usize> =
            self.scan.files.iter().fold(BTreeMap::new(), |mut map, f| {
                *map.entry(f.day.clone()).or_default() += 1;
                map
            });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("02  AUFNAHMETAGE").strong());
            if ui.small_button("Alle auswählen").clicked() {
                self.selected = days.keys().cloned().collect();
            }
            if ui.small_button("Auswahl leeren").clicked() {
                self.selected.clear();
            }
            ui.label(format!("{} Dateien ausgewählt", selected_files.len()));
        });
        egui::ScrollArea::horizontal()
            .id_salt("days")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (day, count) in days.iter().rev() {
                        let mut selected = self.selected.contains(day);
                        ui.vertical(|ui| {
                            if ui
                                .selectable_label(
                                    self.focused == *day,
                                    RichText::new(format!("{day}\n{count} Dateien")).strong(),
                                )
                                .clicked()
                            {
                                self.focused = day.clone();
                            }
                            if ui.checkbox(&mut selected, "Importieren").changed() {
                                if selected {
                                    self.selected.insert(day.clone());
                                } else {
                                    self.selected.remove(day);
                                }
                            }
                        });
                        ui.add_space(8.0);
                    }
                });
            });
        ui.separator();
        let right_width = 280.0;
        let width = (ui.available_width() - right_width - 20.0).max(300.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{} · Vorschau", self.focused)).strong());
                        ui.add(
                            egui::Slider::new(&mut self.zoom, 120.0..=240.0)
                                .show_value(false)
                                .text("Bildgröße"),
                        );
                    });
                    let files = self
                        .scan
                        .files
                        .iter()
                        .filter(|f| f.day == self.focused)
                        .cloned()
                        .collect::<Vec<_>>();
                    self.source_gallery(ui, &files);
                },
            );
            let plan = ui.allocate_ui_with_layout(
                egui::vec2(right_width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("import-plan")
                        .show(ui, |ui| {
                            self.import_plan(ui, &selected_files);
                        });
                },
            );
            #[cfg(test)]
            {
                self.plan_bounds = Some(plan.response.rect);
            }
            #[cfg(not(test))]
            {
                let _ = plan;
            }
        });
    }
    fn import_plan(&mut self, ui: &mut egui::Ui, selected_files: &[SourceFile]) {
        theme::card().show(ui, |ui| {
            ui.set_max_width(248.0);
            theme::eyebrow(ui, "03  ZIEL & IMPORT");
            ui.label(RichText::new("Alles am richtigen Ort").size(20.0).strong());
            if ui.button("Zielordner wählen …").clicked() {
                self.pick(Pick::Target);
            }
            ui.add(
                egui::Label::new(if self.settings.target.is_empty() {
                    "Noch kein Ziel gewählt"
                } else {
                    &self.settings.target
                })
                .wrap(),
            );
            ui.label("Neuer Ordner");
            ui.add(
                egui::TextEdit::singleline(&mut self.folder_name)
                    .hint_text("z. B. 2026-11 Herbsturlaub")
                    .desired_width(f32::INFINITY),
            );
            let validation = files::validate_folder(&self.folder_name);
            if let Err(e) = &validation {
                ui.label(RichText::new(e.to_string()).small().color(theme::WARM));
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Geplante Ablage").strong());
            ui.label(format!(
                "{}/",
                if self.folder_name.is_empty() {
                    "Ordnername"
                } else {
                    &self.folder_name
                }
            ));
            for (ext, count) in files::counts(selected_files) {
                ui.label(format!("    {ext}/  ·  {count} Dateien"));
            }
            if ui
                .checkbox(&mut self.settings.dng, "NEF zusätzlich als DNG")
                .changed()
            {
                self.save();
            }
            if self.settings.dng {
                ui.label(
                    RichText::new(
                        "DNGs liegen direkt im neuen Ordner. Adobe Converter erforderlich.",
                    )
                    .small()
                    .weak(),
                );
            }
            ui.separator();
            if ui
                .checkbox(
                    &mut self.settings.skip_imported,
                    "Bereits importierte Aufnahmen überspringen",
                )
                .changed()
            {
                self.save();
            }
            ui.label(
                RichText::new(
                    "Bei vorhandenem Zielindex wird vor dem Kopieren mit ExifTool geprüft.",
                )
                .small()
                .weak(),
            );
            let bytes = selected_files.iter().map(|f| f.size).sum();
            ui.label(
                RichText::new(format!(
                    "{} Dateien · {}",
                    selected_files.len(),
                    human_size(bytes)
                ))
                .strong(),
            );
            ui.label(
                RichText::new(
                    "Originale bleiben erhalten. Namenskollisionen erhalten einen Nummernsuffix.",
                )
                .small()
                .weak(),
            );
            let ready = validation.is_ok()
                && !selected_files.is_empty()
                && !self.settings.target.is_empty()
                && self.tasks.iter().filter(|t| t.kind == "copy").count() < 8;
            if theme::primary(ui, "Sicher importieren →", ready) {
                let target = PathBuf::from(&self.settings.target);
                let name = self.folder_name.clone();
                let converter = self
                    .settings
                    .dng
                    .then(|| PathBuf::from(&self.settings.converter));
                let files = selected_files.to_vec();
                let skip = self.settings.skip_imported;
                let exiftool = self.settings.exiftool.clone();
                self.save();
                self.launch("Fotos sicher kopieren", true, "copy", move |control| {
                    Ok(Outcome::Imported(crate::core::import_service::execute(
                        &files,
                        &target,
                        &name,
                        converter.as_deref(),
                        skip,
                        &exiftool,
                        &control,
                    )?))
                });
            }
        });
    }
    pub(super) fn source_gallery(&mut self, ui: &mut egui::Ui, files: &[SourceFile]) {
        let columns = (ui.available_width() / (self.zoom + 12.0)).floor().max(1.0) as usize;
        let rows = files.len().div_ceil(columns);
        let height = self.zoom * 0.72 + 45.0;
        egui::ScrollArea::vertical()
            .id_salt("source-gallery")
            .show_rows(ui, height, rows, |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        for file in files.iter().skip(row * columns).take(columns) {
                            ui.vertical(|ui| {
                                let request = PreviewRequest {
                                    root: Some(PathBuf::from(&self.settings.source)),
                                    path: file.path.clone(),
                                    orientation: 1,
                                    crop: None,
                                    edge: 320,
                                    generation: 0,
                                };
                                let response = self.thumbnails.image(
                                    ui,
                                    request,
                                    &self.settings.exiftool,
                                    egui::vec2(self.zoom, self.zoom * 0.72),
                                );
                                response.on_hover_text(file.path.display().to_string());
                                ui.add_sized(
                                    [self.zoom, 25.0],
                                    egui::Label::new(
                                        file.path.file_name().unwrap_or_default().to_string_lossy(),
                                    )
                                    .truncate(),
                                );
                            });
                        }
                    });
                }
            });
    }
}
