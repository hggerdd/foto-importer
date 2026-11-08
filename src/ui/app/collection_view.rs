use super::*;

impl PhotoApp {
    pub(super) fn collection_view(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            let spacing = ui.spacing_mut();
            spacing.item_spacing = egui::vec2(8.0, 6.0);
            spacing.button_padding = egui::vec2(10.0, 5.0);
            spacing.interact_size.y = 30.0;
            self.collection_toolbar(ui);
            if self.settings.collection.is_empty() {
                self.empty(
                    ui,
                    "Ein Zuhause für deine Fotos.",
                    "Wähle den Wurzelordner deiner Sammlung.",
                );
                return;
            }
            self.collection_search(ui);
            ui.separator();
            let width = ui.available_width();
            let gap = ui.spacing().item_spacing.x;
            let detail_width = (width * 0.48).clamp(320.0, 720.0);
            let inspect = self.inspector && width >= 610.0;
            if inspect
                && self.selected_photo.is_none()
                && let Some(photo) = self.page.photos.first().cloned()
            {
                self.choose_photo(photo);
            }
            let gallery_width = if inspect {
                width - detail_width - gap
            } else {
                width
            };
            let height = ui.available_height();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(gallery_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(gallery_width);
                        self.collection_pagination(ui);
                        self.photo_gallery(ui);
                    },
                );
                if inspect {
                    ui.allocate_ui_with_layout(
                        egui::vec2(detail_width, height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(detail_width);
                            self.inspector(ui);
                        },
                    );
                }
            });
        });
        self.show_collection_dialog(ui.ctx());
    }

    fn collection_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Sammlung").size(23.0).strong());
            let root = super::import_view::display_path(&self.settings.collection);
            let name = Path::new(&self.settings.collection)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or(root.as_ref());
            let label = if name.is_empty() {
                "Sammlung wählen …"
            } else {
                name
            };
            if ui
                .add_sized([160.0, 30.0], egui::Button::new(label).truncate())
                .on_hover_text(if root.is_empty() {
                    "Sammlung wählen"
                } else {
                    root.as_ref()
                })
                .clicked()
            {
                self.pick(Pick::Collection);
            }
            if !self.settings.collection.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Bestand …").clicked() {
                        self.collection_dialog = Some(CollectionDialog::Inventory);
                    }
                    if ui.selectable_label(self.inspector, "Bilddetails").clicked() {
                        self.inspector = !self.inspector;
                    }
                });
            }
        });
    }

    fn collection_search(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Gleiche Höhe wie die Schaltflächen daneben, Text mittig; die Breite wächst mit dem Fenster.
            let height = ui.spacing().interact_size.y;
            let search = ui.add_sized(
                [(ui.available_width() - 345.0).clamp(160.0, 640.0), height],
                egui::TextEdit::singleline(&mut self.filters.text)
                    .hint_text("Person, Datei, Kamera oder Objektiv …")
                    .vertical_align(egui::Align::Center)
                    .margin(egui::Margin::symmetric(10, 6)),
            );
            let submit = search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if !self.filters.text.is_empty()
                && ui
                    .add(egui::Button::new("×").min_size(egui::vec2(height, height)))
                    .on_hover_text("Suchtext löschen")
                    .clicked()
            {
                self.filters.text.clear();
                self.apply_collection_search();
            }
            let search_button = egui::Button::new(
                RichText::new("Suchen")
                    .color(Color32::from_rgb(9, 35, 34))
                    .strong(),
            )
            .fill(theme::ACCENT);
            if ui.add(search_button).clicked() || submit {
                self.apply_collection_search();
            }
            let mut draft = self.filters.clone();
            draft.page = self.applied.page;
            draft.folder = self.applied.folder.clone();
            draft.folder_page = self.applied.folder_page;
            let filter_label = if draft != self.applied {
                "Filter · Entwurf"
            } else if Self::has_advanced_filters(&self.applied) {
                "Filter · aktiv"
            } else {
                "Filter …"
            };
            if ui.button(filter_label).clicked() {
                self.collection_dialog = Some(CollectionDialog::Filters);
            }
            self.collection_folders(ui);
        });
    }

    fn has_advanced_filters(filters: &Search) -> bool {
        let mut criteria = filters.clone();
        criteria.text.clear();
        criteria.folder = None;
        criteria.page = 0;
        criteria.folder_page = 0;
        if criteria.face_state == "any" {
            criteria.face_state.clear();
        }
        criteria != Search::default()
    }

    fn apply_collection_search(&mut self) {
        self.filters.folder = self.applied.folder.clone();
        self.filters.page = 0;
        self.filters.folder_page = 0;
        self.refresh_collection();
    }

    fn navigate_collection(&mut self, page: usize, folder: Option<String>, folder_page: usize) {
        let mut filters = self.applied.clone();
        filters.page = page;
        filters.folder = folder;
        filters.folder_page = folder_page;
        self.query_collection(filters);
    }

    fn collection_folders(&mut self, ui: &mut egui::Ui) {
        let active = self.applied.folder.clone();
        let name = active
            .as_deref()
            .and_then(|p| p.rsplit('/').next())
            .unwrap_or("Alle");
        let caption: String = name.chars().take(16).collect();
        let caption = if name.chars().count() > 16 {
            format!("{caption}…")
        } else {
            caption
        };
        let loading = self.tasks.iter().any(|task| task.kind == "search");
        ui.menu_button(format!("Ordner · {caption}"), |ui| {
            ui.set_min_width(230.0);
            ui.label(active.as_deref().unwrap_or("Gesamte Sammlung"));
            ui.add_enabled_ui(!loading, |ui| {
                if ui.button("Gesamte Sammlung").clicked() {
                    self.navigate_collection(0, None, 0);
                    ui.close();
                }
                if let Some(folder) = &active
                    && ui.button("Übergeordneter Ordner").clicked()
                {
                    self.navigate_collection(
                        0,
                        folder.rsplit_once('/').map(|(p, _)| p.to_owned()),
                        0,
                    );
                    ui.close();
                }
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for folder in self.page.folders.clone() {
                            if ui.button(format!("{folder}/")).clicked() {
                                let path = active
                                    .as_ref()
                                    .map_or_else(|| folder.clone(), |p| format!("{p}/{folder}"));
                                self.navigate_collection(0, Some(path), 0);
                                ui.close();
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    let page = self.applied.folder_page;
                    if page > 0 && ui.small_button("Vorherige Ordner").clicked() {
                        self.navigate_collection(0, active.clone(), page - 1);
                    }
                    if self.page.folders_more && ui.small_button("Weitere Ordner").clicked() {
                        self.navigate_collection(0, active.clone(), page + 1);
                    }
                });
            });
        })
        .response
        .on_hover_text(active.as_deref().unwrap_or("Gesamte Sammlung"));
    }

    fn collection_pagination(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let page = self.applied.page;
            let pages = self.page.total.div_ceil(config::PAGE_SIZE).max(1);
            let loading = self.tasks.iter().any(|task| task.kind == "search");
            ui.label(RichText::new(format!("{} Fotos", self.page.total)).strong());
            if pages > 1
                && let Some(target) = theme::pager(ui, page, pages, !loading)
            {
                self.navigate_collection(
                    target,
                    self.applied.folder.clone(),
                    self.applied.folder_page,
                );
            }
        });
    }

    fn show_collection_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.collection_dialog else {
            return;
        };
        let response = egui::Modal::new(egui::Id::new("collection-dialog")).show(ctx, |ui| {
            ui.set_width(625.0_f32.min(ctx.content_rect().width() - 64.0));
            ui.heading(match dialog {
                CollectionDialog::Filters => "Sammlung filtern",
                CollectionDialog::Inventory => "Sammlungsbestand",
            });
            egui::ScrollArea::vertical()
                .id_salt(match dialog {
                    CollectionDialog::Filters => "filters",
                    CollectionDialog::Inventory => "inventory",
                })
                .max_height((ctx.content_rect().height() - 190.0).clamp(100.0, 550.0))
                .show(ui, |ui| match dialog {
                    CollectionDialog::Filters => self.search_builder(ui),
                    CollectionDialog::Inventory => self.inventory_controls(ui),
                });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if dialog == CollectionDialog::Filters {
                    if theme::primary(ui, "Filter anwenden", true) {
                        self.apply_collection_search();
                        self.collection_dialog = None;
                    }
                    if ui.button("Filter zurücksetzen").clicked() {
                        self.filters = Search::default();
                        self.apply_collection_search();
                        self.collection_dialog = None;
                    }
                }
                if ui.button("Schließen").clicked() {
                    self.collection_dialog = None;
                }
            });
        });
        if response.should_close() {
            self.collection_dialog = None;
        }
    }

    fn inventory_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(super::import_view::display_path(&self.settings.collection));
        ui.label(format!(
            "{} Aufnahmen · {} Dateien · {} Metadaten offen · {} fehlend",
            self.status.photos, self.status.files, self.status.pending, self.status.missing
        ));
        ui.horizontal_wrapped(|ui| {
            for (action, label) in [
                ("scan", "Bestand aktualisieren"),
                ("metadata", "Metadaten lesen"),
                ("verify", "Integrität prüfen"),
            ] {
                if ui
                    .add_enabled(!self.busy(), egui::Button::new(label))
                    .clicked()
                {
                    self.collection_dialog = None;
                    self.collection_action(action);
                }
            }
        });
        ui.separator();
        ui.label(RichText::new("Fehlende Dateien").strong());
        ui.horizontal_wrapped(|ui| {
            if ui.button("Fehlende & letzte Änderungen anzeigen").clicked() {
                let root = self.settings.collection.clone();
                self.collection_dialog = None;
                self.launch("Bestandsbericht lesen", false, "report", move |_| {
                    let collection = Collection::open(Path::new(&root), false)?;
                    Ok(Outcome::Report(collection.inventory_report()?))
                });
            }
            if ui.add_enabled(!self.busy(), egui::Button::new("Fehlende nach erneuter Prüfung markieren"))
                .on_hover_text("Bestätigt auch einen Anteil über 20 %. Bei Lesefehlern werden keine unbesuchten Dateien als fehlend markiert.")
                .clicked()
            {
                self.collection_dialog = None;
                self.collection_action("confirm_missing");
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(
                &mut self.purge_confirm,
                "Fehlende Einträge aus dem Index entfernen",
            );
            ui.add(
                egui::DragValue::new(&mut self.purge_scans)
                    .range(0..=10000)
                    .prefix("Scanabstand "),
            );
            if ui
                .add_enabled(
                    !self.busy() && self.purge_confirm,
                    egui::Button::new("Einträge entfernen"),
                )
                .clicked()
            {
                self.purge_confirm = false;
                self.collection_dialog = None;
                self.collection_action("purge");
            }
        });
        ui.label(RichText::new("Foto-IDs und Personen bleiben erhalten.").small());
        ui.separator();
        ui.label(RichText::new("Geprüfte Verschiebung zuordnen").strong());
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.move_old)
                    .hint_text("Alter relativer Pfad")
                    .desired_width(200.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.move_new)
                    .hint_text("Neuer relativer Pfad")
                    .desired_width(200.0),
            );
            let ready = !self.busy() && !self.move_old.is_empty() && !self.move_new.is_empty();
            if ui
                .add_enabled(ready, egui::Button::new("Zuordnen"))
                .clicked()
            {
                self.collection_dialog = None;
                self.collection_action("resolve");
            }
        });
    }
    fn search_builder(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Aufnahmedatum").strong());
        ui.horizontal_wrapped(|ui| {
            for (label, date) in [
                ("Von", &mut self.filters.start),
                ("Bis", &mut self.filters.end),
            ] {
                ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(date)
                        .hint_text("JJJJ-MM-TT")
                        .desired_width(110.0),
                );
            }
            ui.checkbox(&mut self.filters.oldest, "Älteste zuerst");
        });
        ui.horizontal_wrapped(|ui| {
            for (month, label) in [
                "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
            ]
            .into_iter()
            .enumerate()
            {
                let number = month as u32 + 1;
                let mut checked = self.filters.months.contains(&number);
                if ui.checkbox(&mut checked, label).changed() {
                    if checked {
                        self.filters.months.push(number);
                    } else {
                        self.filters.months.retain(|m| *m != number);
                    }
                }
            }
        });
        ui.separator();
        ui.label(RichText::new("Personen").strong());
        for (label, group) in [
            ("Alle diese Personen", &mut self.filters.people_all),
            ("Mindestens eine", &mut self.filters.people_any),
            ("Keine dieser Personen", &mut self.filters.people_none),
        ] {
            ui.horizontal_wrapped(|ui| {
                ui.label(label);
                egui::ComboBox::from_id_salt(label)
                    .selected_text("Person hinzufügen …")
                    .show_ui(ui, |ui| {
                        for person in &self.face_page.people {
                            let included = group.contains(&person.id);
                            if ui
                                .selectable_label(
                                    included,
                                    format!("{} · #{}", person.name, person.id),
                                )
                                .clicked()
                                && !included
                            {
                                group.push(person.id);
                            }
                        }
                    });
                let mut remove = None;
                for &id in group.iter() {
                    let name = self
                        .face_page
                        .people
                        .iter()
                        .find(|p| p.id == id)
                        .map_or_else(|| format!("#{id}"), |p| p.name.clone());
                    if ui.small_button(format!("{name} entfernen")).clicked() {
                        remove = Some(id);
                    }
                }
                if let Some(id) = remove {
                    group.retain(|person| *person != id);
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(
                &mut self.filters.only_people,
                "Nur diese Personen · kein weiteres erkanntes Gesicht",
            );
            ui.checkbox(
                &mut self.filters.suggestions,
                "Unbestätigte Vorschläge einbeziehen",
            );
            egui::ComboBox::from_id_salt("face-state")
                .selected_text(face_label(&self.filters.face_state))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        ("any", "Alle Fotos"),
                        ("with_faces", "Mit Gesichtern"),
                        ("none", "Ohne erkannte Gesichter"),
                        ("unknown", "Unbekannte Gesichter"),
                        ("unanalysed", "Noch nicht analysiert"),
                    ] {
                        ui.selectable_value(&mut self.filters.face_state, value.into(), label);
                    }
                });
        });
        ui.label(RichText::new("Für Ausschlüsse und diese strenge Suche muss das Foto mit der aktuellen Erkennung analysiert sein. Auch schwache, unbekannte und ignorierte Gesichter zählen.").small());
        ui.separator();
        ui.label(RichText::new("Metadaten").strong());
        let mut remove = None;
        for (index, rule) in self.filters.rules.iter_mut().enumerate() {
            ui.horizontal_wrapped(|ui| {
                let field_label = metadata::FIELDS
                    .iter()
                    .find(|(field, _)| *field == rule.field)
                    .map_or("Kriterium", |(_, label)| *label);
                egui::ComboBox::from_id_salt(("field", index))
                    .selected_text(field_label)
                    .show_ui(ui, |ui| {
                        for (value, label) in metadata::FIELDS {
                            ui.selectable_value(&mut rule.field, (*value).into(), *label);
                        }
                    });
                let face_count = rule.field == "face_count";
                if face_count && rule.operator == "contains" {
                    rule.operator = "is".into();
                }
                egui::ComboBox::from_id_salt(("operator", index))
                    .selected_text(operator_label(&rule.operator))
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            ("is", "ist"),
                            ("not", "ist nicht"),
                            ("contains", "enthält"),
                            ("min", "mindestens"),
                            ("max", "höchstens"),
                            ("present", "vorhanden"),
                            ("empty", "fehlt"),
                        ] {
                            if !face_count || value != "contains" {
                                let label = match (face_count, value) {
                                    (true, "present") => "Analyse vorhanden",
                                    (true, "empty") => "Analyse fehlt",
                                    _ => label,
                                };
                                ui.selectable_value(&mut rule.operator, value.into(), label);
                            }
                        }
                    });
                if !matches!(rule.operator.as_str(), "present" | "empty") {
                    ui.add(egui::TextEdit::singleline(&mut rule.value).desired_width(130.0));
                }
                if ui.small_button("Entfernen").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            self.filters.rules.remove(index);
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ Kriterium").clicked() {
                self.filters.rules.push(Rule {
                    field: "camera".into(),
                    operator: "contains".into(),
                    value: String::new(),
                });
            }
            ui.checkbox(
                &mut self.filters.any_rule,
                "Mindestens ein Metadatenkriterium erfüllt",
            );
        });
    }
    fn photo_gallery(&mut self, ui: &mut egui::Ui) {
        if self.page.photos.is_empty() {
            self.empty(
                ui,
                "Platz für neue Entdeckungen.",
                "Bestand aktualisieren oder die Suchkriterien anpassen.",
            );
            return;
        }
        let available = ui.available_width() - 12.0;
        let columns = (available / 155.0).floor().max(1.0) as usize;
        let width = (available / columns as f32 - ui.spacing().item_spacing.x).min(250.0);
        let photos = self.page.photos.clone();
        egui::ScrollArea::vertical()
            .id_salt("collection-gallery")
            .show_rows(
                ui,
                width * 0.7 + 50.0,
                photos.len().div_ceil(columns),
                |ui, range| {
                    for row in range {
                        ui.horizontal(|ui| {
                            for photo in photos.iter().skip(row * columns).take(columns) {
                                ui.vertical(|ui| {
                                    let request = PreviewRequest {
                                        root: Some(PathBuf::from(&self.settings.collection)),
                                        path: Path::new(&self.settings.collection)
                                            .join(&photo.source),
                                        orientation: photo.orientation,
                                        crop: None,
                                        edge: 400,
                                        generation: 0,
                                    };
                                    let response = self.thumbnails.image(
                                        ui,
                                        request,
                                        &self.settings.exiftool,
                                        egui::vec2(width, width * 0.7),
                                    );
                                    if self
                                        .selected_photo
                                        .as_ref()
                                        .is_some_and(|p| p.id == photo.id)
                                    {
                                        ui.painter().rect_stroke(
                                            response.rect,
                                            8,
                                            egui::Stroke::new(2.0_f32, theme::ACCENT),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                    if response.clicked() {
                                        self.choose_photo(photo.clone());
                                    }
                                    ui.add_sized(
                                        [width, 20.0],
                                        egui::Label::new(RichText::new(&photo.name).strong())
                                            .truncate(),
                                    );
                                    ui.add_sized(
                                        [width, 18.0],
                                        egui::Label::new(
                                            RichText::new(if photo.date.is_empty() {
                                                "Datum unbekannt".to_string()
                                            } else {
                                                display_date(&photo.date)
                                            })
                                            .small()
                                            .weak(),
                                        )
                                        .truncate(),
                                    );
                                });
                            }
                        });
                    }
                },
            );
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let height = ui.available_height();
        theme::card(ui).inner_margin(12).show(ui, |ui| {
            ui.set_width(width - 24.0);
            ui.set_height((height - 24.0).max(0.0));
            let Some(photo) = self.selected_photo.clone() else {
                ui.label(RichText::new("Bilddetails").size(19.0).strong());
                ui.label("Wähle ein Foto für Vorschau und Metadaten.");
                return;
            };
            ui.horizontal(|ui| {
                let name_width = (ui.available_width() - 117.0).max(100.0);
                ui.add_sized(
                    [name_width, 30.0],
                    egui::Label::new(RichText::new(&photo.name).size(18.0).strong()).truncate(),
                )
                .on_hover_text(&photo.path);
                if ui.button("Vergrößern").clicked() {
                    self.large = true;
                }
            });
            let request = PreviewRequest {
                root: Some(PathBuf::from(&self.settings.collection)),
                path: Path::new(&self.settings.collection).join(&photo.source),
                orientation: photo.orientation,
                crop: None,
                edge: 960,
                generation: 0,
            };
            let image_ready = self.thumbnails.ready(&request);
            let image_height = (ui.available_height() * 0.5).clamp(150.0, 440.0);
            let response = self.thumbnails.image(
                ui,
                request,
                &self.settings.exiftool,
                egui::vec2(ui.available_width(), image_height),
            );
            let face_action = image_ready && self.overlay(ui, response.rect, &photo);
            if response.clicked() && !face_action {
                self.large = true;
            }
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(!self.inspect_people, "Metadaten")
                    .clicked()
                {
                    self.inspect_people = false;
                }
                if ui
                    .selectable_label(self.inspect_people, "Personen")
                    .clicked()
                {
                    self.inspect_people = true;
                }
                if !self.detail_faces.is_empty() || self.inspect_people {
                    ui.checkbox(&mut self.show_frames, "Rahmen");
                }
            });
            ui.separator();
            let scroller = egui::ScrollArea::vertical().id_salt((
                "photo-details",
                photo.id,
                self.inspect_people,
            ));
            if self.inspect_people {
                scroller.show(ui, |ui| self.photo_people(ui));
            } else {
                let bounds = self.photo_metadata(ui, scroller, &photo);
                #[cfg(test)]
                {
                    self.collection_bounds = Some((response.rect, bounds));
                }
                #[cfg(not(test))]
                let _ = bounds;
            }
        });
    }

    fn photo_metadata(
        &mut self,
        ui: &mut egui::Ui,
        scroller: egui::ScrollArea,
        photo: &Photo,
    ) -> egui::Rect {
        let taken = display_date(&photo.date);
        let iso = photo.iso.map(|iso| format!("{iso:.0}"));
        let size = human_size(photo.size);
        let mut summary = vec![
            ("Aufnahme", taken.as_str()),
            ("Kamera", photo.camera.as_str()),
            ("Objektiv", photo.lens.as_str()),
        ];
        if let Some(iso) = &iso {
            summary.push(("ISO", iso.as_str()));
        }
        summary.push(("Dateigröße", size.as_str()));
        const FACE_ROWS: usize = 2;
        let total = FACE_ROWS + summary.len() + self.detail_rows.len();
        scroller
            .show_rows(ui, 23.0, total, |ui, range| {
                let field_width = (ui.available_width() * 0.34).clamp(92.0, 170.0);
                for index in range {
                    if index < FACE_ROWS {
                        self.face_metadata_row(ui, index, field_width);
                        continue;
                    }
                    let index = index - FACE_ROWS;
                    let (key, value) = if let Some((key, value)) = summary.get(index) {
                        (
                            *key,
                            if value.is_empty() {
                                "Unbekannt"
                            } else {
                                *value
                            },
                        )
                    } else {
                        let (key, value) = &self.detail_rows[index - summary.len()];
                        (key.as_str(), value.as_str())
                    };
                    ui.horizontal(|ui| {
                        // Beides linksbündig; `add_sized` würde die Texte zentrieren.
                        let left = egui::Layout::left_to_right(egui::Align::Center);
                        ui.allocate_ui_with_layout(egui::vec2(field_width, 23.0), left, |ui| {
                            ui.set_min_size(egui::vec2(field_width, 23.0));
                            ui.add(egui::Label::new(RichText::new(key).small().weak()).truncate())
                                .on_hover_text(key);
                        });
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 23.0),
                            left,
                            |ui| {
                                ui.add(egui::Label::new(value).truncate())
                                    .on_hover_text(value);
                            },
                        );
                    });
                }
            })
            .inner_rect
    }

    fn face_metadata_row(&mut self, ui: &mut egui::Ui, row: usize, field_width: f32) {
        ui.horizontal(|ui| {
            ui.spacing_mut().interact_size.y = 23.0;
            let label = if row == 0 { "Gesichter" } else { "Personen" };
            ui.allocate_ui_with_layout(
                egui::vec2(field_width, 23.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_size(egui::vec2(field_width, 23.0));
                    ui.add(egui::Label::new(RichText::new(label).small().weak()));
                },
            );
            if row == 0 {
                let count = self.detail_faces.len();
                let text = match (self.detail_analysis_done, count) {
                    (Some(true), _) => format!("{count} erkannt"),
                    (Some(false), 0) => "Noch nicht analysiert".into(),
                    (Some(false), _) => format!("{count} gespeichert · Analyse offen"),
                    (None, _) => "–".into(),
                };
                if self.detail_analysis_done == Some(true) {
                    if ui
                        .small_button(text)
                        .on_hover_text("Anzahl als Suchkriterium übernehmen")
                        .clicked()
                    {
                        let rule = Rule {
                            field: "face_count".into(),
                            operator: "is".into(),
                            value: count.to_string(),
                        };
                        if !self.filters.rules.contains(&rule) {
                            self.filters.rules.push(rule);
                        }
                        self.collection_dialog = Some(CollectionDialog::Filters);
                    }
                } else {
                    ui.add(egui::Label::new(text).truncate());
                }
                return;
            }
            let confirmed: BTreeSet<_> = self
                .detail_faces
                .iter()
                .filter(|face| face.assignment.as_deref() == Some("user"))
                .filter_map(|face| face.person)
                .collect();
            let people: Vec<_> = confirmed
                .into_iter()
                .map(|id| {
                    let name = self
                        .face_page
                        .people
                        .iter()
                        .find(|person| person.id == id)
                        .map_or_else(|| format!("#{id}"), |person| person.name.clone());
                    (id, name)
                })
                .collect();
            if people.is_empty() {
                ui.label(if self.detail_analysis_done.is_some() {
                    "Keine bestätigt"
                } else {
                    "–"
                });
                return;
            }
            let names = people
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let caption = if names.chars().count() > 20 {
                format!("{}…", names.chars().take(19).collect::<String>())
            } else {
                names.clone()
            };
            ui.menu_button(caption, |ui| {
                ui.label("Person als Suchkriterium übernehmen");
                egui::ScrollArea::vertical()
                    .max_height(230.0)
                    .show(ui, |ui| {
                        for (id, name) in people {
                            if ui.button(format!("{name} · #{id}")).clicked() {
                                if !self.filters.people_all.contains(&id) {
                                    self.filters.people_all.push(id);
                                }
                                self.collection_dialog = Some(CollectionDialog::Filters);
                                ui.close();
                            }
                        }
                    });
            })
            .response
            .on_hover_text(names);
        });
    }

    fn photo_people(&mut self, ui: &mut egui::Ui) {
        if self.detail_faces.is_empty() {
            ui.label("Keine erkannten Gesichter für dieses Foto gespeichert.");
        }
        for face in self.detail_faces.clone() {
            ui.horizontal_wrapped(|ui| {
                let name = face
                    .person
                    .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "Unbekannt".into());
                if ui.button(format!("{name} · #{}", face.id)).clicked() {
                    self.open_naming(face.clone());
                }
                ui.label(
                    RichText::new(if face.ignored {
                        "Ignoriert"
                    } else if face.person.is_none() {
                        "Unbekannt"
                    } else if face.assignment.as_deref() == Some("user") {
                        "Bestätigt"
                    } else {
                        "Unbestätigt"
                    })
                    .small(),
                );
                if face.assignment.as_deref() == Some("user") {
                    ui.menu_button("Referenz", |ui| {
                        for (action, label) in [
                            ("pin", "Fixieren"),
                            ("unpin", "Fixierung entfernen"),
                            ("exclude", "Als Referenz ausschließen"),
                        ] {
                            if ui
                                .add_enabled(!self.busy(), egui::Button::new(label))
                                .clicked()
                            {
                                let root = self.settings.collection.clone();
                                let id = face.id;
                                self.undo.clear();
                                self.launch(
                                    "Referenzentscheidung speichern",
                                    true,
                                    "mutation",
                                    move |_| {
                                        let c = Collection::open(Path::new(&root), true)?;
                                        crate::core::maintenance::reference_action(&c, id, action)?;
                                        Ok(Outcome::Done("Referenzentscheidung gespeichert".into()))
                                    },
                                );
                                ui.close();
                            }
                        }
                    });
                }
                if ui
                    .add_enabled(!self.busy(), egui::Button::new("Löschen"))
                    .on_hover_text("Nur diese Fehldetektion löschen, Original bleibt erhalten")
                    .clicked()
                {
                    let root = self.settings.collection.clone();
                    let id = face.id;
                    let photo = face.photo;
                    self.undo.clear();
                    self.launch("Fehldetektion löschen", true, "mutation", move |_| {
                        let mut c = Collection::open(Path::new(&root), true)?;
                        faces::delete_face(&mut c, photo, id)?;
                        Ok(Outcome::Done("Fehldetektion gelöscht".into()))
                    });
                }
            });
        }
    }
    pub(super) fn overlay(&mut self, ui: &egui::Ui, rect: egui::Rect, photo: &Photo) -> bool {
        if !self.show_frames {
            return false;
        }
        let mut selected = None;
        for face in self
            .detail_faces
            .iter()
            .filter(|f| f.source == photo.source)
        {
            let b = face.bbox;
            if b.iter().any(|v| !v.is_finite()) || b[2] <= b[0] || b[3] <= b[1] {
                continue;
            }
            let frame = egui::Rect::from_min_max(
                egui::pos2(
                    rect.left() + b[0] * rect.width(),
                    rect.top() + b[1] * rect.height(),
                ),
                egui::pos2(
                    rect.left() + b[2] * rect.width(),
                    rect.top() + b[3] * rect.height(),
                ),
            );
            ui.painter().rect_stroke(
                frame,
                2,
                egui::Stroke::new(1.5_f32, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
            let name = face
                .person
                .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
                .map(|p| p.name.as_str())
                .unwrap_or("Unbekannt");
            let galley = ui.painter().layout_no_wrap(
                format!("{name} · #{}", face.id),
                egui::FontId::proportional(12.0),
                Color32::WHITE,
            );
            let label = egui::Rect::from_min_size(frame.min, galley.size() + egui::vec2(8.0, 4.0))
                .intersect(rect);
            if !label.is_positive() {
                continue;
            }
            ui.painter()
                .rect_filled(label, 2, Color32::from_black_alpha(210));
            ui.painter()
                .galley(label.min + egui::vec2(4.0, 2.0), galley, Color32::WHITE);
            let action = ui
                .interact(
                    label,
                    ui.id().with(("face-label", face.id)),
                    egui::Sense::click(),
                )
                .on_hover_text("Gesicht benennen oder Namen ändern");
            if action.clicked() || action.secondary_clicked() {
                selected = Some(face.clone());
            }
        }
        let handled = selected.is_some();
        if let Some(face) = selected {
            self.open_naming(face);
        }
        handled
    }
}
