use super::*;

impl PhotoApp {
    pub(super) fn collection_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "DEINE FOTOBIBLIOTHEK",
            "Wiederfinden statt durchsuchen.",
            "Aufnahmen, Metadaten und Personen in einer gemeinsamen Sammlung.",
        );
        ui.horizontal(|ui| {
            if ui.button("Sammlung wählen …").clicked() {
                self.pick(Pick::Collection);
            }
            ui.add(
                egui::Label::new(super::import_view::display_path(&self.settings.collection))
                    .truncate(),
            );
        });
        if self.settings.collection.is_empty() {
            self.empty(ui,"Ein Zuhause für deine Fotos.","Wähle den Wurzelordner deiner Sammlung. Vorhandene Schema-6-Sammlungen bleiben erhalten.");
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (label, value) in [
                ("Aufnahmen", self.status.photos),
                ("Dateien", self.status.files),
                ("Metadaten offen", self.status.pending),
                ("Fehlend", self.status.missing),
            ] {
                theme::card(ui).show(ui, |ui| {
                    ui.label(RichText::new(value.to_string()).size(25.0).strong());
                    ui.label(RichText::new(label).small().weak());
                });
            }
            ui.separator();
            for (action, label) in [
                ("scan", "Bestand aktualisieren"),
                ("metadata", "Metadaten lesen"),
                ("verify", "Integrität prüfen"),
            ] {
                if ui
                    .add_enabled(!self.busy(), egui::Button::new(label))
                    .clicked()
                {
                    self.collection_action(action);
                }
            }
        });
        ui.add_space(10.0);
        egui::CollapsingHeader::new("Bestand prüfen & pflegen").show(ui,|ui|{
            ui.horizontal_wrapped(|ui|{
                if ui.button("Fehlende & letzte Änderungen anzeigen").clicked(){let root=self.settings.collection.clone();self.launch("Bestandsbericht lesen",false,"report",move |_|{let c=Collection::open(Path::new(&root),false)?;Ok(Outcome::Report(c.inventory_report()?))});}
                if ui.add_enabled(!self.busy(),egui::Button::new("Fehlende nach erneuter Prüfung markieren")).on_hover_text("Bestätigt auch einen Anteil über 20 %. Bei Lesefehlern werden keine unbesuchten Dateien als fehlend markiert.").clicked(){self.collection_action("confirm_missing");}
            });
            ui.label("Geprüfte Verschiebung mit früherem Inhaltsfingerabdruck zuordnen");
            ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut self.move_old).hint_text("Alter relativer Pfad").desired_width(240.0));ui.add(egui::TextEdit::singleline(&mut self.move_new).hint_text("Neuer relativer Pfad").desired_width(240.0));if ui.add_enabled(!self.busy()&&!self.move_old.is_empty()&&!self.move_new.is_empty(),egui::Button::new("Zuordnen")).clicked(){self.collection_action("resolve");}});
            ui.horizontal_wrapped(|ui|{ui.checkbox(&mut self.purge_confirm,"Fehlende Dateieinträge aus dem Index entfernen; Foto-IDs und Personen bleiben");ui.add(egui::DragValue::new(&mut self.purge_scans).range(0..=10000).prefix("Scanabstand "));if ui.add_enabled(!self.busy()&&self.purge_confirm,egui::Button::new("Einträge entfernen")).clicked(){self.purge_confirm=false;self.collection_action("purge");}});
        });
        self.search_builder(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label("Ordner:");
            if ui.small_button("Gesamte Sammlung").clicked() {
                self.filters.folder = None;
                self.filters.folder_page = 0;
                self.filters.page = 0;
                self.refresh_collection();
            }
            if let Some(folder) = self.filters.folder.clone() {
                ui.label(&folder);
                if ui.small_button("Übergeordneter Ordner").clicked() {
                    self.filters.folder = folder.rsplit_once('/').map(|v| v.0.to_string());
                    self.filters.folder_page = 0;
                    self.filters.page = 0;
                    self.refresh_collection();
                }
            }
        });
        egui::ScrollArea::horizontal()
            .id_salt("folder-paths")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for folder in self.page.folders.clone() {
                        if ui.button(format!("{folder}/")).clicked() {
                            self.filters.folder = Some(
                                self.filters
                                    .folder
                                    .as_ref()
                                    .filter(|p| !p.is_empty())
                                    .map(|p| format!("{p}/{folder}"))
                                    .unwrap_or(folder),
                            );
                            self.filters.folder_page = 0;
                            self.filters.page = 0;
                            self.refresh_collection();
                        }
                    }
                    if self.filters.folder_page > 0 && ui.small_button("Vorherige Ordner").clicked()
                    {
                        self.filters.folder_page -= 1;
                        self.refresh_collection();
                    }
                    if self.page.folders_more && ui.small_button("Weitere Ordner").clicked() {
                        self.filters.folder_page += 1;
                        self.refresh_collection();
                    }
                });
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} Aufnahmen", self.page.total)).strong());
            if ui
                .add_enabled(
                    self.filters.page > 0 && !self.tasks.iter().any(|t| t.kind == "search"),
                    egui::Button::new("Zurück"),
                )
                .clicked()
            {
                self.filters.page -= 1;
                self.refresh_collection();
            }
            ui.label(format!(
                "Seite {} / {}",
                self.applied.page + 1,
                self.page.total.div_ceil(config::PAGE_SIZE).max(1)
            ));
            if ui
                .add_enabled(
                    (self.filters.page + 1) * config::PAGE_SIZE < self.page.total
                        && !self.tasks.iter().any(|t| t.kind == "search"),
                    egui::Button::new("Weiter"),
                )
                .clicked()
            {
                self.filters.page += 1;
                self.refresh_collection();
            }
            ui.checkbox(&mut self.inspector, "Details");
        });
        let available = ui.available_width();
        let inspect = self.inspector && available > 650.0;
        let gallery_width = if inspect {
            available - 320.0
        } else {
            available
        };
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(gallery_width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.photo_gallery(ui);
                },
            );
            if inspect {
                ui.allocate_ui_with_layout(
                    egui::vec2(300.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        self.inspector(ui);
                    },
                );
            }
        });
    }
    pub(super) fn search_builder(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filters.text)
                    .hint_text("Pfad, Kamera oder Objektiv …")
                    .desired_width(300.0),
            );
            if theme::primary(ui, "Suchen", true) {
                self.filters.page = 0;
                self.refresh_collection();
            }
            if ui.button("Zurücksetzen").clicked() {
                self.filters = Search::default();
                self.refresh_collection();
            }
            if self.filters != self.applied {
                ui.label(
                    RichText::new("Änderungen noch nicht angewendet")
                        .small()
                        .color(theme::WARM),
                );
            }
        });
        egui::CollapsingHeader::new("Zeitraum, Personen & weitere Kriterien").id_salt("search-builder").show(ui,|ui|{
            ui.horizontal(|ui|{ui.label("Von");ui.add(egui::TextEdit::singleline(&mut self.filters.start).hint_text("JJJJ-MM-TT").desired_width(110.0));ui.label("bis");ui.add(egui::TextEdit::singleline(&mut self.filters.end).hint_text("JJJJ-MM-TT").desired_width(110.0));ui.checkbox(&mut self.filters.oldest,"Älteste zuerst");});
            ui.horizontal_wrapped(|ui|{for(month,label)in ["Jan","Feb","Mär","Apr","Mai","Jun","Jul","Aug","Sep","Okt","Nov","Dez"].iter().enumerate(){let number=month as u32+1;let mut checked=self.filters.months.contains(&number);if ui.checkbox(&mut checked,*label).changed(){if checked{self.filters.months.push(number);}else{self.filters.months.retain(|m|*m!=number);}}}});
            for(label,group)in [("Alle diese Personen",&mut self.filters.people_all),("Mindestens eine",&mut self.filters.people_any),("Keine dieser Personen",&mut self.filters.people_none)]{
                ui.horizontal_wrapped(|ui|{ui.label(label);egui::ComboBox::from_id_salt(label).selected_text("Person hinzufügen …").show_ui(ui,|ui|{for person in &self.face_page.people{if ui.selectable_label(false,format!("{} · #{}",person.name,person.id)).clicked()&&!group.contains(&person.id){group.push(person.id);}}});let mut remove=None;for(id,person)in group.iter().enumerate(){let name=self.face_page.people.iter().find(|p|p.id==*person).map(|p|p.name.clone()).unwrap_or_else(||format!("#{person}"));if ui.small_button(format!("{name} ×")).clicked(){remove=Some(id);}}
if let Some(id)=remove{group.remove(id);}});
            }
            ui.horizontal_wrapped(|ui|{ui.checkbox(&mut self.filters.only_people,"Nur diese Personen");ui.checkbox(&mut self.filters.suggestions,"Unbestätigte Vorschläge einbeziehen");egui::ComboBox::from_id_salt("face-state").selected_text(face_label(&self.filters.face_state)).show_ui(ui,|ui|{for(value,label)in [("any","Alle Fotos"),("with_faces","Mit Gesichtern"),("none","Ohne erkannte Gesichter"),("unknown","Unbekannte Gesichter"),("unanalysed","Noch nicht analysiert")]{ui.selectable_value(&mut self.filters.face_state,value.into(),label);}});});
            ui.label(RichText::new("Ausschlüsse benötigen eine erfolgreiche Analyse. Ignorierte Gesichter zählen weiterhin als anwesend.").small().weak());
            let mut remove=None;for(index,rule)in self.filters.rules.iter_mut().enumerate(){ui.horizontal(|ui|{egui::ComboBox::from_id_salt(("field",index)).selected_text(metadata::FIELDS.iter().find(|f|f.0==rule.field).map(|f|f.1).unwrap_or("Kriterium")).show_ui(ui,|ui|{for(value,label)in metadata::FIELDS{ui.selectable_value(&mut rule.field,value.to_string(),*label);}});egui::ComboBox::from_id_salt(("operator",index)).selected_text(operator_label(&rule.operator)).show_ui(ui,|ui|{for(value,label)in [("is","ist"),("not","ist nicht"),("contains","enthält"),("min","mindestens"),("max","höchstens"),("present","vorhanden"),("empty","fehlt")]{ui.selectable_value(&mut rule.operator,value.into(),label);}});ui.add(egui::TextEdit::singleline(&mut rule.value).desired_width(150.0));if ui.small_button("Entfernen").clicked(){remove=Some(index);}});}
if let Some(index)=remove{self.filters.rules.remove(index);}
            ui.horizontal(|ui|{if ui.button("+ Kriterium").clicked(){self.filters.rules.push(Rule{field:"camera".into(),operator:"contains".into(),value:String::new()});}ui.checkbox(&mut self.filters.any_rule,"Mindestens ein Metadatenkriterium erfüllt");});
        });
    }
    pub(super) fn photo_gallery(&mut self, ui: &mut egui::Ui) {
        if self.page.photos.is_empty() {
            self.empty(
                ui,
                "Platz für neue Entdeckungen.",
                "Bestand aktualisieren oder die Suchkriterien anpassen.",
            );
            return;
        }
        let columns = (ui.available_width() / 190.0).floor().max(1.0) as usize;
        let width = (ui.available_width() / columns as f32 - 12.0).min(250.0);
        let photos = self.page.photos.clone();
        egui::ScrollArea::vertical()
            .id_salt("collection-gallery")
            .show_rows(
                ui,
                width * 0.7 + 60.0,
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
                                                "Datum unbekannt"
                                            } else {
                                                &photo.date
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
    pub(super) fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("inspector")
            .show(ui, |ui| {
                let Some(photo) = self.selected_photo.clone() else {
                    ui.label(
                        RichText::new("Ein Bild. Seine Geschichte.")
                            .size(18.0)
                            .strong(),
                    );
                    ui.label("Wähle eine Aufnahme für Vorschau und Metadaten.");
                    return;
                };
                theme::eyebrow(ui, "BILDDETAILS");
                ui.label(RichText::new(&photo.name).size(20.0).strong());
                let request = PreviewRequest {
                    root: Some(PathBuf::from(&self.settings.collection)),
                    path: Path::new(&self.settings.collection).join(&photo.source),
                    orientation: photo.orientation,
                    crop: None,
                    edge: 960,
                    generation: 0,
                };
                let image_ready = self.thumbnails.ready(&request);
                let response = self.thumbnails.image(
                    ui,
                    request,
                    &self.settings.exiftool,
                    egui::vec2(ui.available_width(), 200.0),
                );
                let face_action = image_ready && self.overlay(ui, response.rect, &photo);
                if response.clicked() && !face_action {
                    self.large = true;
                }
                ui.checkbox(&mut self.show_frames, "Gesichtsrahmen anzeigen");
                ui.label(&photo.date);
                ui.label(format!("{} · {}", photo.camera, photo.lens));
                ui.label(format!(
                    "ISO {} · {}",
                    photo
                        .iso
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unbekannt".into()),
                    human_size(photo.size)
                ));
                ui.label(RichText::new(&photo.path).small().weak());
                for face in self.detail_faces.clone() {
                    ui.horizontal(|ui| {
                        let name = face
                            .person
                            .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| "Unbekannt".into());
                        if ui.button(format!("{name} · #{}", face.id)).clicked() {
                            self.open_naming(face.clone());
                        }
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
                                                crate::core::maintenance::reference_action(
                                                    &c, id, action,
                                                )?;
                                                Ok(Outcome::Done(
                                                    "Referenzentscheidung gespeichert".into(),
                                                ))
                                            },
                                        );
                                        ui.close();
                                    }
                                }
                            });
                        }
                        if ui
                            .add_enabled(!self.busy(), egui::Button::new("Löschen"))
                            .on_hover_text(
                                "Nur diese Fehldetektion löschen, Original bleibt erhalten",
                            )
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
                ui.separator();
                egui::CollapsingHeader::new("Alle gespeicherten Metadaten")
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("metadata-values")
                            .max_height(280.0)
                            .show_rows(ui, 24.0, self.detail_rows.len(), |ui, range| {
                                for index in range {
                                    let (key, value) = &self.detail_rows[index];
                                    ui.horizontal(|ui| {
                                        ui.add_sized(
                                            [110.0, 24.0],
                                            egui::Label::new(RichText::new(key).small().weak())
                                                .truncate(),
                                        )
                                        .on_hover_text(key);
                                        ui.add_sized(
                                            [ui.available_width(), 24.0],
                                            egui::Label::new(value).truncate(),
                                        )
                                        .on_hover_text(value);
                                    });
                                }
                            });
                    });
            });
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
