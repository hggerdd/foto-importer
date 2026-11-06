use super::*;

impl PhotoApp {
    pub(super) fn people_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "GESICHTER & PERSONEN",
            "Aus Gesichtern werden Geschichten.",
            "Lokal analysieren, gemeinsam benennen und bestätigtes Wissen anwenden.",
        );
        if self.runtime_downloading() || !Path::new(&self.settings.onnx_runtime).is_file() {
            theme::card(ui).show(ui, |ui| {
                ui.label("Für die Gesichtsanalyse brauchst du die ONNX Runtime.");
                self.runtime_download_button(ui);
            });
        }
        let installing = self.models_installing();
        let weights_present = self.model_weights_present();
        if installing || !weights_present {
            theme::card(ui).show(ui, |ui| {
                ui.label("Für die Gesichtsanalyse fehlen Modellgewichte.");
                ui.horizontal_wrapped(|ui| {
                    self.model_install_button(ui);
                    if ui
                        .add_enabled(!installing, egui::Button::new("Modellordner wählen …"))
                        .clicked()
                    {
                        self.pick(Pick::Models);
                    }
                });
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Sammlung wählen …").clicked() {
                self.pick(Pick::Collection);
            }
            for (action, label) in [
                ("analyze", "1  Fotos analysieren"),
                ("knowledge", "2  Lernen & Namen vorschlagen"),
            ] {
                if ui
                    .add_enabled(
                        !self.busy()
                            && !self.settings.collection.is_empty()
                            && (action != "analyze" || (!installing && weights_present)),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    self.collection_action(action);
                }
            }
            if ui
                .add_enabled(
                    !self.busy() && !self.undo.is_empty(),
                    egui::Button::new("Rückgängig"),
                )
                .clicked()
                && let Some(undo) = self.undo.last().cloned()
            {
                let root = self.settings.collection.clone();
                self.launch(
                    "Gruppenentscheidung zurücknehmen",
                    true,
                    "undo",
                    move |_| {
                        let mut c = Collection::open(Path::new(&root), true)?;
                        faces::undo(&mut c, &undo)?;
                        Ok(Outcome::Done("Gruppenentscheidung zurückgenommen".into()))
                    },
                );
            }
        });
        if self.settings.collection.is_empty() {
            self.empty(ui,"Wer gehört zu deiner Geschichte?","Wähle zuerst eine Sammlung. Gesichtsbilder und Vektoren bleiben auf diesem Rechner.");
            return;
        }
        let status = &self.face_page.status;
        ui.horizontal_wrapped(|ui| {
            for (label, value) in [
                ("Bestätigt", status.confirmed),
                ("Unbekannt", status.unknown),
                ("Vorschläge", status.review),
                ("Ignoriert", status.ignored),
                ("Referenzen", status.references),
            ] {
                theme::card(ui).show(ui, |ui| {
                    ui.label(RichText::new(value.to_string()).size(22.0).strong());
                    ui.label(label);
                });
            }
        });
        ui.label(
            RichText::new(format!(
                "Letzter Abgleich: {}{}",
                status.applied,
                if status.pending > 0 {
                    " · Nacharbeit offen"
                } else {
                    ""
                }
            ))
            .small()
            .weak(),
        );
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                ("unknown", "Benennen"),
                ("review", "Prüfen"),
                ("confirmed", "Bestätigte"),
                ("ignored", "Ignorierte"),
                ("persons", "Personen & Beziehungen"),
            ] {
                if ui.selectable_label(self.face_mode == mode, label).clicked() {
                    self.face_mode = mode.into();
                    self.face_index = 0;
                    self.face_advance = None;
                    self.face_row_tops.clear();
                    self.refresh_faces();
                }
            }
        });
        ui.separator();
        egui::CollapsingHeader::new("Erkennungswissen verwalten").show(ui,|ui|{
            let mut auto_import=self.face_page.status.auto_import;let mut nightly=self.face_page.status.nightly;
            let changed=ui.add_enabled(!self.busy(),egui::Checkbox::new(&mut auto_import,"Nach Import automatisch analysieren")).changed()|ui.add_enabled(!self.busy(),egui::Checkbox::new(&mut nightly,"Um 03:00 Uhr analysieren, solange die App läuft")).changed();
            if changed{let root=self.settings.collection.clone();self.launch("Automatik speichern",true,"mutation",move |_|{let mut c=Collection::open(Path::new(&root),true)?;faces::configure_automatic(&mut c,auto_import,nightly)?;Ok(Outcome::Done("Sammlungsautomatik gespeichert".into()))});}
            if ui.checkbox(&mut self.settings.auto_metadata,"Metadaten beim automatischen Ablauf vorab anreichern").changed(){self.save();}
            ui.horizontal_wrapped(|ui|{for(action,label)in [("resume","Offenen Abgleich fortsetzen"),("export","Referenzen exportieren"),("calibrate","Validierung auswerten")]{if ui.add_enabled(!self.busy(),egui::Button::new(label)).clicked(){self.collection_action(action);}}});
            ui.label("Biometrie entfernen löscht Gesichter, Personen und bekannte Biometrie-Sicherungen. Foto-Dateien und Inventar bleiben erhalten.");
            ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut self.delete_phrase).hint_text("Zum Löschen BIOMETRIE eingeben"));if ui.add_enabled(!self.busy()&&self.delete_phrase=="BIOMETRIE",egui::Button::new("Biometrie dauerhaft entfernen")).clicked(){self.undo.clear();self.delete_phrase.clear();self.collection_action("delete_biometrics");}});
        });
        if self.face_mode == "persons" {
            self.person_manager(ui);
            return;
        }
        ui.horizontal(|ui| {
            let loading = self.tasks.iter().any(|task| task.kind == "faces");
            if ui
                .add_enabled(!loading && self.face_index > 0, egui::Button::new("Zurück"))
                .clicked()
            {
                self.face_index -= 1;
                self.face_advance = None;
                self.face_row_tops.clear();
                self.refresh_faces();
            }
            ui.label(format!("Seite {}", self.face_index + 1));
            if ui
                .add_enabled(
                    !loading && !self.face_page.groups.is_empty(),
                    egui::Button::new("Weiter"),
                )
                .clicked()
            {
                self.face_index += 1;
                self.face_advance = None;
                self.face_row_tops.clear();
                self.refresh_faces();
            }
            if ui.button("Aktualisieren").clicked() {
                self.refresh_faces();
            }
        });
        if self.face_page.groups.is_empty() {
            if self.face_page.status.embedder.is_empty() {
                self.empty(ui,"Bereit für die lokale Analyse.","Fotos einmal mit einem Rust-Modellraum analysieren. Vorhandene Personenentscheidungen bleiben erhalten.");
            } else {
                self.empty(ui,"Hier ist gerade nichts zu benennen.","Fotos analysieren oder eine andere Ansicht wählen. Neue Referenzen entstehen nur aus bestätigten Namen.");
            }
            return;
        }
        let advance = if self
            .face_advance
            .as_ref()
            .is_some_and(|advance| advance.ready)
        {
            self.face_advance.take()
        } else {
            None
        };
        let target = advance
            .as_ref()
            .map(|advance| advance.target(&self.face_page.groups));
        self.face_row_tops.clear();
        egui::ScrollArea::vertical()
            .id_salt("face-groups")
            .animated(false)
            .show(ui, |ui| {
                for (index, group) in self.face_page.groups.clone().into_iter().enumerate() {
                    let top = ui.next_widget_position().y - ui.clip_rect().top();
                    if let Some(face) = group.first() {
                        self.face_row_tops.push((face.id, top));
                    }
                    let focus = target == Some(index);
                    self.group(ui, &group, focus);
                    ui.add_space(12.0);
                }
                if let (Some(advance), Some(index)) = (&advance, target)
                    && let Some((_, top)) = self.face_row_tops.get(index)
                {
                    ui.scroll_with_delta(egui::vec2(0.0, advance.top - top));
                }
            });
    }
    pub(super) fn prepare_group_advance(&mut self, undo: &Undo) {
        if self.view != View::People || !matches!(self.face_mode.as_str(), "unknown" | "review") {
            return;
        }
        let Some(slot) = self.face_page.groups.iter().position(|group| {
            group
                .iter()
                .any(|face| undo.before.iter().any(|snapshot| snapshot.id == face.id))
        }) else {
            return;
        };
        let Some(first) = self.face_page.groups[slot].first() else {
            return;
        };
        let Some((_, top)) = self.face_row_tops.iter().find(|(id, _)| *id == first.id) else {
            return;
        };
        self.face_advance = Some(GroupAdvance {
            slot,
            top: top.max(0.0),
            following: self.face_page.groups[slot + 1..]
                .iter()
                .flatten()
                .map(|face| face.id)
                .collect(),
            ready: false,
        });
    }

    fn group(&mut self, ui: &mut egui::Ui, group: &[Face], focus: bool) {
        let key = group
            .iter()
            .map(|f| f.id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let busy = self.busy();
        let root = self.settings.collection.clone();
        let exiftool = self.settings.exiftool.clone();
        let draft = self.drafts.entry(key.clone()).or_insert_with(|| {
            let person = group.first().and_then(|f| f.person);
            Draft {
                person,
                name: person
                    .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
                    .map(|p| p.name.clone())
                    .unwrap_or_default(),
                excluded: BTreeSet::new(),
            }
        });
        let mut submit = false;
        let mut ignore = false;
        theme::card(ui).show(ui, |ui| {
            ui.push_id(&key, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} Gesichter", group.len())).strong());
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut draft.name)
                            .hint_text("Name eingeben, Enter bestätigen")
                            .desired_width(240.0),
                    );
                    if focus && !response.has_focus() {
                        response.request_focus();
                    }
                    if response.changed() {
                        draft.person = None;
                    }
                    submit = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    egui::ComboBox::from_id_salt("person")
                        .selected_text("Person wählen …")
                        .show_ui(ui, |ui| {
                            for person in self
                                .face_page
                                .people
                                .iter()
                                .filter(|p| {
                                    files::fold(&p.name).contains(&files::fold(&draft.name))
                                })
                                .take(40)
                                .cloned()
                                .collect::<Vec<_>>()
                            {
                                if ui
                                    .selectable_label(
                                        draft.person == Some(person.id),
                                        format!("{} · #{}", person.name, person.id),
                                    )
                                    .clicked()
                                {
                                    draft.person = Some(person.id);
                                    draft.name = person.name.clone();
                                }
                            }
                        });
                    if ui
                        .add_enabled(!busy, egui::Button::new("Bestätigen"))
                        .clicked()
                    {
                        submit = true;
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Nicht interessiert"))
                        .clicked()
                    {
                        ignore = true;
                    }
                });
                let size = ((ui.available_width() - 6.0 * 10.0) / 7.0).clamp(42.0, 110.0);
                for chunk in group.chunks(7) {
                    ui.horizontal(|ui| {
                        for face in chunk {
                            ui.vertical(|ui| {
                                let request = PreviewRequest {
                                    root: Some(PathBuf::from(&root)),
                                    path: Path::new(&root).join(&face.source),
                                    orientation: 1,
                                    crop: Some(face.bbox),
                                    edge: 160,
                                    generation: 0,
                                };
                                let response = self.thumbnails.image(
                                    ui,
                                    request,
                                    &exiftool,
                                    egui::vec2(size, size),
                                );
                                let mut included = !draft.excluded.contains(&face.id);
                                if response.clicked() {
                                    included = !included;
                                    if included {
                                        draft.excluded.remove(&face.id);
                                    } else {
                                        draft.excluded.insert(face.id);
                                    }
                                }
                                if ui
                                    .checkbox(&mut included, format!("#{}", face.id))
                                    .changed()
                                {
                                    if included {
                                        draft.excluded.remove(&face.id);
                                    } else {
                                        draft.excluded.insert(face.id);
                                    }
                                }
                            });
                        }
                    });
                }
            });
        });
        if (submit || ignore) && !busy {
            let ids: Vec<_> = group.iter().map(|f| f.id).collect();
            let selected = ids
                .iter()
                .filter(|id| !draft.excluded.contains(id))
                .copied()
                .collect();
            let name = draft.name.clone();
            let person = draft.person;
            self.launch("Gesichtsgruppe speichern", true, "naming", move |_| {
                let mut c = Collection::open(Path::new(&root), true)?;
                let undo = faces::name_group(&mut c, &ids, &selected, person, &name, ignore)?;
                Ok(Outcome::Named(undo))
            });
        }
    }
    pub(super) fn person_manager(&mut self, ui: &mut egui::Ui) {
        let mut photos = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.busy() && self.relation_from.is_some(),
                    egui::Button::new("Beziehungen der gewählten Person anzeigen"),
                )
                .clicked()
            {
                let root = self.settings.collection.clone();
                let id = self.relation_from.unwrap_or_default();
                self.launch("Beziehungen ableiten", false, "relations", move |_| {
                    let c = Collection::open(Path::new(&root), false)?;
                    let relations = crate::core::maintenance::relations(&c, id)?;
                    let people = faces::people(&c)?;
                    let report = relations
                        .into_iter()
                        .map(|(kind, ids)| {
                            format!(
                                "{kind}: {}",
                                ids.iter()
                                    .map(|id| people
                                        .iter()
                                        .find(|p| p.id == *id)
                                        .map(|p| format!("{} #{}", p.name, p.id))
                                        .unwrap_or_else(|| format!("#{id}")))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    Ok(Outcome::Report(report))
                });
            }
            if ui
                .add_enabled(
                    !self.busy()
                        && self.relation_from.is_some()
                        && self.relation_to.is_some()
                        && self.relation_from != self.relation_to,
                    egui::Button::new("Person Von in Person Zu zusammenführen"),
                )
                .clicked()
            {
                let root = self.settings.collection.clone();
                let source = self.relation_from.unwrap_or_default();
                let target = self.relation_to.unwrap_or_default();
                self.undo.clear();
                self.launch("Personen zusammenführen", true, "mutation", move |_| {
                    let mut c = Collection::open(Path::new(&root), true)?;
                    crate::core::maintenance::merge_people(&mut c, source, target)?;
                    Ok(Outcome::Done(
                        "Personen zusammengeführt; alte ID bleibt als Weiterleitung erhalten"
                            .into(),
                    ))
                });
            }
        });
        ui.label(
            RichText::new("Personen haben stabile IDs. Gleiche Namen bleiben unterscheidbar.")
                .weak(),
        );
        egui::ScrollArea::vertical()
            .id_salt("persons")
            .max_height(280.0)
            .show(ui, |ui| {
                for person in &self.face_page.people {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&person.name).strong());
                        ui.label(format!(
                            "#{} · {} bestätigte Gesichter · Geburtsjahr {}",
                            person.id, person.count, person.birth
                        ));
                        if ui.small_button("Bearbeiten").clicked() {
                            self.person_edit = Some(person.clone());
                        }
                        if ui.small_button("Fotos").clicked() {
                            photos = Some(person.id);
                        }
                    });
                }
            });
        ui.separator();
        if let Some(person) = photos {
            self.filters = Search {
                people_all: vec![person],
                ..Default::default()
            };
            self.view = View::Collection;
            self.refresh_collection();
        }
        ui.label(RichText::new("Familienbeziehung ergänzen").strong());
        if ui
            .add_enabled(
                !self.busy() && self.relation_from.is_some() && self.relation_to.is_some(),
                egui::Button::new("Ausgewählte Beziehung entfernen"),
            )
            .clicked()
        {
            let root = self.settings.collection.clone();
            let source = self.relation_from.unwrap_or_default();
            let target = self.relation_to.unwrap_or_default();
            let kind = self.relation_kind.clone();
            self.undo.clear();
            self.launch("Beziehung entfernen", true, "mutation", move |_| {
                let c = Collection::open(Path::new(&root), true)?;
                faces::remove_relation(&c, source, &kind, target)?;
                Ok(Outcome::Done("Beziehung entfernt".into()))
            });
        }
        ui.horizontal_wrapped(|ui| {
            for (id, selected) in [
                ("Von", &mut self.relation_from),
                ("Zu", &mut self.relation_to),
            ] {
                egui::ComboBox::from_id_salt(id)
                    .selected_text(
                        selected
                            .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
                            .map(|p| format!("{} #{}", p.name, p.id))
                            .unwrap_or_else(|| id.into()),
                    )
                    .show_ui(ui, |ui| {
                        for person in &self.face_page.people {
                            ui.selectable_value(
                                selected,
                                Some(person.id),
                                format!("{} #{}", person.name, person.id),
                            );
                        }
                    });
            }
            egui::ComboBox::from_id_salt("relation-kind")
                .selected_text(&self.relation_kind)
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        ("parent", "Elternteil von"),
                        ("child", "Kind von"),
                        ("partner", "Partner von"),
                        ("sibling", "Geschwister von"),
                    ] {
                        ui.selectable_value(&mut self.relation_kind, value.into(), label);
                    }
                });
            if ui
                .add_enabled(
                    !self.busy() && self.relation_from.is_some() && self.relation_to.is_some(),
                    egui::Button::new("Speichern"),
                )
                .clicked()
            {
                let root = self.settings.collection.clone();
                let from = self.relation_from.unwrap_or_default();
                let to = self.relation_to.unwrap_or_default();
                let kind = self.relation_kind.clone();
                self.undo.clear();
                self.launch("Beziehung speichern", true, "mutation", move |_| {
                    let mut c = Collection::open(Path::new(&root), true)?;
                    faces::add_relation(&mut c, from, &kind, to)?;
                    Ok(Outcome::Done("Beziehung gespeichert".into()))
                });
            }
        });
    }
}
