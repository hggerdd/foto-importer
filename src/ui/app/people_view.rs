use super::*;
use crate::core::relations::{Gender, Role};

impl PhotoApp {
    pub(super) fn people_view(&mut self, ui: &mut egui::Ui) {
        self.hover_face = None;
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
            ("persons", {
                let questions = self.open_questions();
                if questions > 0 {
                    format!("Personen & Beziehungen · {questions} Rückfragen")
                } else {
                    "Personen & Beziehungen".to_string()
                }
            }),
            ("tree", "Stammbaum".into()),
            ("graph", "Graph".into()),
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
            return;
        }
        if self.face_mode == "tree" {
            self.tree_tab(ui);
            return;
        }
        if self.face_mode == "graph" {
            self.relationship_graph_tab(ui);
            return;
        }
        egui::TopBottomPanel::bottom("people-footer").show_inside(ui, |ui| {
            if ui
                .checkbox(
                    &mut self.settings.hover_preview,
                    "Foto in der Vorschau zeigen, wenn die Maus über einem Gesicht steht",
                )
                .changed()
            {
                let _ = self.settings.save();
            }
        });
        ui.horizontal(|ui| {
            let loading = self.tasks.iter().any(|task| task.kind == "faces");
            let pages = self
                .face_page
                .total_groups
                .div_ceil(config::FACE_GROUPS_PER_PAGE);
            if let Some(target) = theme::pager(ui, self.face_index, pages, !loading) {
                self.face_index = target;
                self.face_advance = None;
                self.face_row_tops.clear();
                self.refresh_faces();
            }
            self.undo_face_button(ui);
            if ui.button("Aktualisieren").clicked() {
                self.refresh_faces();
            }
            ui.add_space(12.0);
            ui.spacing_mut().slider_width = 200.0;
            let slider = ui
                .add(
                    egui::Slider::new(&mut self.settings.face_tile_scale, 0.0..=1.0)
                        .show_value(false)
                        .text("Größe"),
                )
                .on_hover_text("Größe der Gesichter. Mitte ist die Standardgröße.");
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                let _ = self.settings.save();
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
        self.hover_photo_preview(ui.ctx());
    }
    /// Das ganze Foto zum Gesicht unter der Maus, am Zeiger geführt. Es gibt nur
    /// `hover_face` aus diesem Frame, daher höchstens eine Vorschau; sie
    /// verschwindet, sobald die Maus das Gesicht verlässt.
    fn hover_photo_preview(&mut self, ctx: &egui::Context) {
        let (Some(face), Some(pointer)) = (self.hover_face.clone(), ctx.pointer_hover_pos()) else {
            return;
        };
        if !self.settings.hover_preview {
            return;
        }
        let root = self.settings.collection.clone();
        let exiftool = self.settings.exiftool.clone();
        let size = egui::vec2(PREVIEW_SIZE.0, PREVIEW_SIZE.1);
        let position = preview_position(pointer, size + egui::vec2(12.0, 12.0), ctx.content_rect());
        egui::Area::new(egui::Id::new("face-hover-photo"))
            .order(egui::Order::Tooltip)
            .fixed_pos(position)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    self.thumbnails.image(
                        ui,
                        PreviewRequest {
                            root: Some(PathBuf::from(&root)),
                            path: Path::new(&root).join(&face.source),
                            orientation: 1,
                            crop: None,
                            edge: 800,
                            generation: 0,
                        },
                        &exiftool,
                        size,
                    );
                });
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

    /// Kleine Vorschlagskarte: Porträt der Person, darunter Name und Wert.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn suggestion_chip(
        thumbnails: &mut Thumbnails,
        ui: &mut egui::Ui,
        root: &str,
        exiftool: &str,
        person: i64,
        portrait: Option<&Face>,
        name: &str,
        detail: &str,
        enabled: bool,
    ) -> bool {
        let inner = egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let size = egui::vec2(34.0, 34.0);
                match portrait {
                    Some(face) => {
                        thumbnails.image(
                            ui,
                            PreviewRequest {
                                root: Some(PathBuf::from(root)),
                                path: Path::new(root).join(&face.source),
                                orientation: 1,
                                crop: Some(face.bbox),
                                edge: 160,
                                generation: 0,
                            },
                            exiftool,
                            size,
                        );
                    }
                    None => {
                        ui.allocate_space(size);
                    }
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(name).small().strong());
                    ui.label(RichText::new(detail).small().weak());
                });
            });
        });
        let rect = inner.response.rect;
        let response = ui.interact(
            rect,
            ui.id().with(("suggestion", person)),
            if enabled {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        if enabled && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            ui.painter().rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(1.5_f32, ui.visuals().selection.bg_fill),
                egui::StrokeKind::Outside,
            );
        }
        enabled && response.clicked()
    }

    fn group(&mut self, ui: &mut egui::Ui, group: &[Face], focus: bool) {
        let key = draft_key(group);
        let busy = self.busy();
        let hover_enabled = self.settings.hover_preview;
        let tile_pref = tile_size_for(self.settings.face_tile_scale);
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
            // Auch kleine Gruppen belegen die volle Breite statt auf ihren Inhalt zu schrumpfen.
            ui.set_min_width(ui.available_width());
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
                            let detail = if proposals.by_model {
                                format!("{:.0} %", score * 100.0)
                            } else {
                                format!("{score:.2}")
                            };
                            if Self::suggestion_chip(
                                &mut self.thumbnails,
                                ui,
                                &root,
                                &exiftool,
                                *person,
                                proposals.portraits.get(person),
                                name,
                                &detail,
                                !busy,
                            ) {
                                draft.person = Some(*person);
                                draft.name = name.clone();
                                submit = true;
                            }
                        }
                    });
                }
                let (columns, size) = tile_layout(
                    ui.available_width(),
                    ui.spacing().item_spacing.x,
                    group.len(),
                    tile_pref,
                );
                for chunk in group.chunks(columns) {
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
                                if hover_enabled && response.hovered() {
                                    self.hover_face = Some(face.clone());
                                }
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
        if self.relation_from.is_none() {
            // Die Übersicht soll sofort etwas zeigen: Standard ist die Person mit den meisten Gesichtern.
            self.relation_from = self
                .face_page
                .people
                .iter()
                .max_by_key(|person| person.count)
                .map(|person| person.id);
        }
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
                    !self.busy()
                        && self.relation_from.is_some()
                        && self.relation_to.is_some()
                        && self.relation_from != self.relation_to,
                    egui::Button::new("Person A in Person B zusammenführen"),
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
            .max_height(130.0)
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
        self.questions_list(ui);
        self.add_person_form(ui);
        ui.label(RichText::new("Beziehung zwischen Personen").strong());
        self.relation_form(ui);
        ui.add_space(6.0);
        self.relation_overview(ui);
    }

    /// Satzform "[Person A] ist [Rolle] von [Person B]". Die Rollenbeschriftung
    /// folgt dem Geschlecht von Person A; die Gegenrichtung steht als Hinweis daneben.
    fn relation_form(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let people = &self.face_page.people;
        let gender_of = |id: Option<i64>| {
            id.and_then(|id| people.iter().find(|p| p.id == id))
                .map_or(Gender::Unknown, |p| Gender::parse(&p.gender))
        };
        let role = Role::from_key(&self.relation_kind).unwrap_or(Role::Parent);
        let (a, b) = (self.relation_from, self.relation_to);
        let mut chosen = role;
        ui.horizontal_wrapped(|ui| {
            Self::relationship_person(ui, "Person A", people, &mut self.relation_from);
            ui.label("ist");
            egui::ComboBox::from_id_salt("relation-kind")
                .selected_text(role.label(gender_of(a)))
                .show_ui(ui, |ui| {
                    for option in [
                        Role::Child,
                        Role::Parent,
                        Role::Partner,
                        Role::FormerPartner,
                        Role::Friend,
                        Role::Acquaintance,
                        Role::Sibling,
                        Role::Cousin,
                    ] {
                        let text = option.label(gender_of(a));
                        let text = if matches!(option, Role::Sibling | Role::Cousin) {
                            format!("{text} (Behelf)")
                        } else {
                            text.to_string()
                        };
                        ui.selectable_value(&mut chosen, option, text);
                    }
                });
            ui.label("von");
            Self::relationship_person(ui, "Person B", people, &mut self.relation_to);
        });
        self.relation_kind = chosen.key().into();
        if matches!(chosen, Role::Sibling | Role::Cousin) {
            ui.label(
                RichText::new(
                    "Behelf: nur nötig, solange die Eltern nicht erfasst sind. Mit erfassten Eltern ergeben sich Geschwister und Cousins automatisch aus dem Stammbaum.",
                )
                .weak(),
            );
        }
        let ready = a.is_some() && b.is_some() && a != b;
        if ready {
            let name = |id: Option<i64>| {
                id.and_then(|id| people.iter().find(|p| p.id == id))
                    .map_or_else(String::new, |p| p.name.clone())
            };
            ui.label(
                RichText::new(format!(
                    "Gleichbedeutend: {} ist {} von {}.",
                    name(b),
                    chosen.inverse().label(gender_of(b)),
                    name(a)
                ))
                .weak(),
            );
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!busy && ready, egui::Button::new("Beziehung speichern"))
                .clicked()
            {
                let root = self.settings.collection.clone();
                let (from, to) = (a.unwrap_or_default(), b.unwrap_or_default());
                let kind = self.relation_kind.clone();
                self.undo.clear();
                self.launch("Beziehung speichern", true, "mutation", move |_| {
                    let mut c = Collection::open(Path::new(&root), true)?;
                    faces::add_relation(&mut c, from, &kind, to)?;
                    Ok(Outcome::Done("Beziehung gespeichert".into()))
                });
            }
            if ui
                .add_enabled(!busy && ready, egui::Button::new("Beziehung entfernen"))
                .clicked()
            {
                let root = self.settings.collection.clone();
                let (from, to) = (a.unwrap_or_default(), b.unwrap_or_default());
                let kind = self.relation_kind.clone();
                self.undo.clear();
                self.launch("Beziehung entfernen", true, "mutation", move |_| {
                    let c = Collection::open(Path::new(&root), true)?;
                    faces::remove_relation(&c, from, &kind, to)?;
                    Ok(Outcome::Done("Beziehung entfernt".into()))
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

/// Entwürfe gelten für genau diese Gesichter *und* Person: Eine zuvor unbenannte
/// Gruppe, die später einer Person vorgeschlagen wird, darf ihren leeren
/// Namen nicht behalten.
fn draft_key(group: &[Face]) -> String {
    let ids = group
        .iter()
        .map(|f| f.id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    match group.first().and_then(|f| f.person) {
        Some(person) => format!("p{person}:{ids}"),
        None => ids,
    }
}
#[cfg(test)]
mod draft_tests {
    use super::*;
    fn face(id: i64, person: Option<i64>) -> Face {
        Face {
            id,
            photo: 1,
            source: String::new(),
            bbox: [0.0; 4],
            person,
            assignment: None,
            ignored: false,
            score: 0.0,
        }
    }
    #[test]
    fn draft_changes_when_the_group_gets_a_person() {
        let unnamed = [face(1, None), face(2, None)];
        let suggested = [face(1, Some(7)), face(2, Some(7))];
        assert_eq!(draft_key(&unnamed), "1,2");
        assert_ne!(draft_key(&unnamed), draft_key(&suggested));
    }
}

const PREVIEW_SIZE: (f32, f32) = (520.0, 390.0);

/// Ecke links oben der Vorschau: rechts unterhalb des Zeigers, bei Platzmangel
/// links bzw. oberhalb, immer innerhalb des Fensters.
fn preview_position(pointer: egui::Pos2, size: egui::Vec2, screen: egui::Rect) -> egui::Pos2 {
    let gap = 18.0;
    let x = if pointer.x + gap + size.x <= screen.right() {
        pointer.x + gap
    } else {
        pointer.x - gap - size.x
    };
    let y = if pointer.y + gap + size.y <= screen.bottom() {
        pointer.y + gap
    } else {
        pointer.y - gap - size.y
    };
    egui::pos2(
        x.clamp(screen.left(), (screen.right() - size.x).max(screen.left())),
        y.clamp(screen.top(), (screen.bottom() - size.y).max(screen.top())),
    )
}
#[cfg(test)]
mod hover_tests {
    use super::*;
    #[test]
    fn preview_follows_the_pointer_and_stays_inside_the_window() {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let size = egui::vec2(532.0, 402.0);
        let near = preview_position(egui::pos2(100.0, 100.0), size, screen);
        assert_eq!(near, egui::pos2(118.0, 118.0));
        let corner = preview_position(egui::pos2(1250.0, 780.0), size, screen);
        assert!(corner.x + size.x <= 1280.0 && corner.y + size.y <= 800.0);
        assert!(
            corner.x < 1250.0 && corner.y < 780.0,
            "weicht zum Zeiger hin aus"
        );
    }
}

/// Kachelbreite in Punkten zur Reglerstellung: links deutlich kleiner (60), Mitte
/// ist der bisherige Standard (150), rechts die größte Stufe (220).
pub(super) fn tile_size_for(scale: f32) -> f32 {
    let t = scale.clamp(0.0, 1.0);
    if t <= 0.5 {
        60.0 + (150.0 - 60.0) * (t / 0.5)
    } else {
        150.0 + (MAX_TILE - 150.0) * ((t - 0.5) / 0.5)
    }
}
const MAX_TILE: f32 = 220.0;
/// Spaltenzahl und Kachelgröße einer Gruppe bei gewünschter Größe `size`: so viele
/// Spalten, wie bei höchstens 96 Punkten (oder kleinerer Wunschgröße) passen; die
/// Kacheln füllen die Breite, werden aber nie größer als `size`.
fn tile_layout(width: f32, gap: f32, faces: usize, size: f32) -> (usize, f32) {
    let minimum = size.min(96.0);
    let fit = ((width + gap) / (minimum + gap)).floor() as usize;
    let columns = fit.clamp(1, crate::config::GROUP_MAX).min(faces.max(1));
    let tile = ((width - gap * (columns as f32 - 1.0)) / columns as f32).clamp(42.0, size);
    (columns, tile)
}
#[cfg(test)]
mod layout_tests {
    use super::*;
    const WIDTH: f32 = 1360.0;
    const GAP: f32 = 8.0;
    #[test]
    fn slider_middle_is_the_previous_size_and_the_ends_are_smaller_and_larger() {
        assert_eq!(tile_size_for(0.5), 150.0);
        assert_eq!(tile_size_for(1.0), MAX_TILE);
        assert_eq!(tile_size_for(0.0), 60.0);
        assert!(tile_size_for(0.25) > 60.0 && tile_size_for(0.25) < 150.0);
        assert_eq!(
            tile_size_for(7.0),
            MAX_TILE,
            "Werte außerhalb werden begrenzt"
        );
    }
    #[test]
    fn default_keeps_small_groups_at_the_previous_size() {
        let (columns, size) = tile_layout(WIDTH, GAP, 6, tile_size_for(0.5));
        assert_eq!((columns, size), (6, 150.0));
        // Große Gruppen füllen die Breite mit mehr, kleineren Kacheln.
        let (columns, size) = tile_layout(WIDTH, GAP, 28, tile_size_for(0.5));
        assert!(columns >= 12 && size < 150.0);
        assert!(columns as f32 * size + (columns as f32 - 1.0) * GAP <= WIDTH + 0.5);
    }
    #[test]
    fn largest_step_fills_the_row_and_smallest_packs_more_columns() {
        let (columns, size) = tile_layout(WIDTH, GAP, 6, tile_size_for(1.0));
        assert_eq!(columns, 6);
        assert!(columns as f32 * size + 5.0 * GAP > WIDTH * 0.95);
        let (small_columns, small) = tile_layout(WIDTH, GAP, 28, tile_size_for(0.0));
        let (default_columns, _) = tile_layout(WIDTH, GAP, 28, tile_size_for(0.5));
        assert!(small_columns > default_columns && small <= 60.0 + 0.5);
    }
    #[test]
    fn narrow_windows_and_single_faces_stay_valid() {
        let (columns, size) = tile_layout(300.0, GAP, 28, tile_size_for(1.0));
        assert!(columns >= 1 && size >= 42.0);
        assert_eq!(tile_layout(WIDTH, GAP, 1, tile_size_for(1.0)).0, 1);
        assert_eq!(tile_layout(WIDTH, GAP, 0, tile_size_for(0.5)).0, 1);
    }
}
