use super::*;

impl PhotoApp {
    pub(super) fn people_view(&mut self, ui: &mut egui::Ui) {
        self.people_workflow(ui);
        if self.settings.collection.is_empty() {
            self.empty(
                ui,
                "Wer gehört zu deiner Sammlung?",
                "Wähle eine Sammlung, um Gesichter lokal zu analysieren.",
            );
            return;
        }
        let status = &self.face_page.status;
        let tabs = [
            ("unknown", format!("Benennen · {}", status.nameable)),
            ("review", format!("Vorschläge prüfen · {}", status.review)),
            ("confirmed", format!("Bestätigt · {}", status.confirmed)),
            ("ignored", format!("Ignoriert · {}", status.ignored)),
            ("persons", "Personen & Beziehungen".into()),
        ];
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in tabs {
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
        if self.face_mode == "persons" {
            egui::ScrollArea::vertical()
                .id_salt("person-management")
                .show(ui, |ui| self.person_manager(ui));
            self.show_relationship_graph(ui.ctx());
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
            self.undo_face_button(ui);
            if ui.button("Aktualisieren").clicked() {
                self.refresh_faces();
            }
        });
        if self.face_page.groups.is_empty() {
            if self.analysing_current_collection() {
                self.empty(
                    ui,
                    "Die Analyse läuft.",
                    "Offene Gesichtsgruppen erscheinen nach der Analyse unter Benennen.",
                );
            } else if self.face_page.status.embedder.is_empty() {
                self.empty(ui, "Bereit für die lokale Analyse.", "Starte mit Fotos analysieren. Anschließend kannst du erkannte Gesichter benennen.");
            } else if self.face_mode == "review" {
                self.empty(ui, "Keine offenen Namensvorschläge.", "Bestätige Gesichter, damit die App geeignete Beispiele für den nächsten Abgleich hat.");
            } else {
                self.empty(
                    ui,
                    "In diesem Reiter sind keine Gesichter.",
                    "Analysiere weitere Fotos oder wähle einen anderen Reiter.",
                );
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
        let proposals = group
            .first()
            .and_then(|f| self.face_page.suggestions.get(&f.id))
            .cloned();
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
                if let Some(proposals) = &proposals {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(if proposals.by_model {
                                "Passendste Personen (Modell):"
                            } else {
                                "Ähnlichste Personen:"
                            })
                            .small()
                            .weak(),
                        );
                        for (person, name, score) in &proposals.items {
                            let label = if proposals.by_model {
                                format!("{name} · {:.0} %", score * 100.0)
                            } else {
                                format!("{name} · {score:.2}")
                            };
                            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                                draft.person = Some(*person);
                                draft.name = name.clone();
                                submit = true;
                            }
                        }
                    });
                }
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
        let mut graph_from = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.face_page.people.is_empty(),
                    egui::Button::new("Beziehungsgraph …"),
                )
                .on_hover_text("Gemeinsame Fotos und eingetragene Beziehungen als Graph ansehen")
                .clicked()
            {
                graph_from = self.relation_from.or_else(|| {
                    self.face_page
                        .people
                        .iter()
                        .max_by_key(|person| person.count)
                        .map(|person| person.id)
                });
            }
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
                    egui::Button::new("Person „Von“ in Person „Nach“ zusammenführen"),
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
                        if ui.small_button("Graph").clicked() {
                            graph_from = Some(person.id);
                        }
                    });
                }
            });
        ui.separator();
        if let Some(person) = graph_from {
            self.open_relationship_graph(person);
        }
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
            let people = &self.face_page.people;
            Self::relationship_person(ui, "Von", people, &mut self.relation_from);
            let kinds = [
                ("parent", "Elternteil von"),
                ("child", "Kind von"),
                ("partner", "Partner von"),
                ("sibling", "Geschwister von"),
            ];
            let kind_label = kinds
                .iter()
                .find(|(value, _)| *value == self.relation_kind)
                .map_or("wählen …", |(_, label)| *label);
            egui::ComboBox::from_id_salt("relation-kind")
                .selected_text(format!("Beziehungstyp: {kind_label}"))
                .show_ui(ui, |ui| {
                    for (value, label) in kinds {
                        ui.selectable_value(&mut self.relation_kind, value.into(), label);
                    }
                });
            Self::relationship_person(ui, "Nach", people, &mut self.relation_to);
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

    fn relationship_person(
        ui: &mut egui::Ui,
        label: &str,
        people: &[faces::Person],
        selected: &mut Option<i64>,
    ) {
        let caption = selected
            .and_then(|id| people.iter().find(|person| person.id == id))
            .map_or_else(
                || format!("{label}: Person wählen …"),
                |person| format!("{label}: {} · #{}", person.name, person.id),
            );
        egui::ComboBox::from_id_salt(label)
            .selected_text(caption)
            .show_ui(ui, |ui| {
                for person in people {
                    ui.selectable_value(
                        selected,
                        Some(person.id),
                        format!("{} · #{}", person.name, person.id),
                    );
                }
            });
    }
}
