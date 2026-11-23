use super::*;

impl PhotoApp {
    /// Ansicht „Gesichter“: Statuszeile, dann die Reiter „Benennen“, „Vorschläge prüfen“,
    /// „Erledigt“ (dort der Umschalter „Bestätigt | Ignoriert“) und „Ungeeignet“.
    pub(super) fn faces_view(&mut self, ui: &mut egui::Ui) {
        self.hover_face = None;
        self.faces_header(ui);
        if self.settings.collection.is_empty() {
            self.empty(
                ui,
                "Wer gehört zu deiner Sammlung?",
                "Wähle links unter „Sammlung“ einen Ordner, um Gesichter auf diesem Rechner zu analysieren.",
            );
            return;
        }
        let status = &self.face_page.status;
        let (confirmed, ignored) = (status.confirmed, status.ignored);
        let done = done_tab(&self.face_mode);
        let tabs = [
            (
                Some("unknown"),
                format!("Benennen · {}", status.nameable),
                self.face_mode == "unknown",
            ),
            (
                Some("review"),
                format!("Vorschläge prüfen · {}", status.review),
                self.face_mode == "review",
            ),
            (None, format!("Erledigt · {}", confirmed + ignored), done),
            (
                Some("unsuitable"),
                format!("Ungeeignet · {}", status.unsuitable()),
                self.face_mode == "unsuitable",
            ),
        ];
        ui.horizontal_wrapped(|ui| {
            for (mode, label, selected) in tabs {
                if ui.selectable_label(selected, label).clicked() {
                    match mode {
                        Some(mode) => self.set_face_mode(mode),
                        None => self.open_done(),
                    }
                }
            }
        });
        ui.separator();
        if done {
            ui.horizontal(|ui| {
                for (mode, label) in [
                    ("confirmed", format!("Bestätigt · {confirmed}")),
                    ("ignored", format!("Ignoriert · {ignored}")),
                ] {
                    if ui
                        .add(egui::Button::selectable(self.face_mode == mode, label))
                        .clicked()
                        && self.face_mode != mode
                    {
                        self.set_face_mode(mode);
                    }
                }
            });
        }
        self.hover_preview_footer(ui);
        self.face_toolbar(ui);
        if self.face_mode == "review" {
            self.review_filter(ui);
        }
        if self.face_mode == "unknown"
            && let Some(hint) = unnamed_hint(self.face_page.status.unsuitable())
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(hint).small().weak());
                if ui.small_button("Ungeeignete Gesichter ansehen").clicked() {
                    self.set_face_mode("unsuitable");
                }
            });
        }
        if self.face_mode == "unsuitable" {
            ui.label(RichText::new(UNSUITABLE_HINT).small().weak());
        }
        if self.face_page.groups.is_empty() {
            if !self.face_page_loading(ui) {
                self.empty_face_tab(ui);
            }
            return;
        }
        self.face_group_list(ui);
    }
    /// Leerer Reiter der Ansicht „Gesichter“: sagt, warum nichts da ist und was zu tun ist.
    fn empty_face_tab(&self, ui: &mut egui::Ui) {
        let (title, text) = if self.face_mode == "unsuitable" {
            (
                "Keine ungeeigneten Gesichter.",
                "Alle offenen Gesichter haben Namensmerkmale und stehen unter Benennen.",
            )
        } else if self.analysing_current_collection() {
            (
                "Die Analyse läuft.",
                "Offene Gesichtsgruppen erscheinen nach der Analyse unter Benennen.",
            )
        } else if self.face_page.status.embedder.is_empty() {
            (
                "Noch keine Gesichter.",
                "Starte oben mit „Fotos analysieren“. Anschließend kannst du erkannte Gesichter benennen.",
            )
        } else if self.face_mode == "review" && self.review_person.is_some() {
            (
                "Für diese Person gibt es keine offenen Namensvorschläge.",
                "Wähle eine andere Person oder zeige wieder alle Personen.",
            )
        } else if self.face_mode == "review" {
            (
                "Keine offenen Namensvorschläge.",
                "Bestätige Gesichter, damit die App geeignete Beispiele für den nächsten Abgleich hat.",
            )
        } else {
            (
                "In diesem Reiter sind keine Gesichter.",
                "Analysiere weitere Fotos oder wähle einen anderen Reiter.",
            )
        };
        self.empty(ui, title, text);
    }
    /// Solange die Seite gelesen wird: Hinweis mit Spinner und `true`.
    pub(super) fn face_page_loading(&self, ui: &mut egui::Ui) -> bool {
        // Nach einem Wechsel der Ansicht oder des Reiters ist die Seite noch unterwegs.
        let loading = self.tasks.iter().any(|task| task.kind == JobKind::Faces);
        if loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Gesichter werden gelesen …");
            });
        }
        loading
    }
    /// Fußzeile mit den Schaltern für die Fotovorschau unter der Maus und die Kachelsymbole.
    pub(super) fn hover_preview_footer(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::bottom("people-footer").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let preview = ui.checkbox(
                    &mut self.settings.hover_preview,
                    "Foto in der Vorschau zeigen, wenn die Maus über einem Gesicht steht",
                );
                let icons = ui
                    .checkbox(&mut self.settings.tile_icons, "Symbole auf den Gesichtern")
                    .on_hover_text(
                        "Symbole oben rechts auf jedem Gesicht: Doppelklick auf den \
                         durchgestrichenen Kreis heißt „nicht interessiert“, auf Kreuz oder \
                         Haken lehnt ab bzw. bestätigt nur dieses Gesicht; das Fragezeichen \
                         öffnet die Namensauswahl.",
                    );
                if preview.changed() || icons.changed() {
                    self.save();
                }
            });
        });
    }
    /// Seitenwahl, Rücknahme, Neulesen und Kachelgröße über den Gesichtsgruppen; hier gelten
    /// auch die Tasten der obersten Gruppe und Strg+Z.
    pub(super) fn face_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let loading = self.tasks.iter().any(|task| task.kind == JobKind::Faces);
            let pages = self
                .face_page
                .total_groups
                .div_ceil(config::face_groups_per_page(self.face_page.group_size));
            if let Some(target) = theme::pager(ui, self.face_index, pages, !loading) {
                self.reset_face_page();
                self.face_index = target;
                self.refresh_faces();
            }
            self.undo_face_button(ui);
            self.undo_shortcut(ui.ctx());
            self.face_shortcuts(ui.ctx());
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
                self.save();
            }
            ui.add_space(12.0);
            self.group_size_slider(ui);
        });
    }
    /// Regler „Gruppengröße“: Gesichter je Gruppe auf allen Seiten mit Gesichtsgruppen. Erst
    /// beim Loslassen wird gespeichert und ab Seite 1 neu gelesen; Entwürfe hängen an genau
    /// ihren Gesichtern und gelten nur für eine Gruppe mit denselben Gesichtern weiter.
    pub(super) fn group_size_slider(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().slider_width = 160.0;
        self.settings.face_group_size = config::group_size(self.settings.face_group_size);
        let slider = ui
            .add(
                egui::Slider::new(
                    &mut self.settings.face_group_size,
                    config::GROUP_SIZE_MIN..=config::GROUP_SIZE_MAX,
                )
                .text("Gruppengröße"),
            )
            .on_hover_text(
                "Höchstens so viele Gesichter je Gruppe. Große Gruppen eignen sich für sichere \
                 Vorschläge; „Benennen“ teilt Ähnlichkeitsgruppen nur, legt sie aber nie \
                 zusammen.",
            );
        let released = slider.drag_stopped() || (slider.changed() && !slider.dragged());
        // Läuft noch ein Lesen mit anderer Größe, wird ebenfalls neu gelesen.
        let loading = self.tasks.iter().any(|task| task.kind == JobKind::Faces);
        if released && (self.settings.face_group_size != self.face_page.group_size || loading) {
            self.save();
            self.reset_face_page();
            self.refresh_faces();
        }
    }
    /// Die Gesichtsgruppen der Seite als Karten in einem Scrollbereich. Nach einer
    /// Entscheidung rückt die nächste Gruppe an die Position der entschiedenen.
    pub(super) fn face_group_list(&mut self, ui: &mut egui::Ui) {
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
                let probable = self.page_mode() == "probable";
                for (index, group) in self.face_page.groups.clone().into_iter().enumerate() {
                    if probable
                        && person_faces_view::starts_others(
                            &self.face_page.groups,
                            index,
                            &self.face_page.better,
                        )
                    {
                        self.others_heading(ui, &group);
                    }
                    let top = ui.next_widget_position().y - ui.clip_rect().top();
                    if let Some(face) = group.first() {
                        self.face_row_tops.push((face.id, top));
                    }
                    let focus = target == Some(index) && !self.face_keys;
                    self.group(ui, &group, focus, index == 0);
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
    /// Personenfilter im Reiter „Vorschläge prüfen“: nur Personen mit offenen Vorschlägen,
    /// daneben der Knopf, der wieder alle zeigt.
    fn review_filter(&mut self, ui: &mut egui::Ui) {
        let people = &self.face_page.people;
        let items: Vec<select::Choice<i64>> = people
            .iter()
            .filter_map(|p| {
                let open = self.face_page.review_people.get(&p.id).copied();
                if open.is_none() && self.review_person != Some(p.id) {
                    return None;
                }
                let mut detail = match open.unwrap_or(0) {
                    1 => "1 Vorschlag".to_string(),
                    n => format!("{n} Vorschläge"),
                };
                if select::needs_id(people, p.id) {
                    detail = format!("{detail} · #{}", p.id);
                }
                Some(
                    select::Choice::new(p.id, p.name.clone(), detail)
                        .with_keywords(format!("#{}", p.id)),
                )
            })
            .collect();
        let mut chosen = self.review_person;
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Vorschläge für");
            changed = select::FilterSelect::new("review-person", &items)
                .placeholder("alle Personen")
                .width(220.0)
                .show(ui, &mut chosen);
            if chosen.is_some() && ui.button("Alle Personen zeigen").clicked() {
                chosen = None;
                changed = true;
            }
        });
        if changed && chosen != self.review_person {
            self.review_person = chosen;
            self.reset_face_page();
            self.refresh_faces();
        }
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
                    // Dieselbe Datei und Ausrichtung wie die Kachel (`face.source`, die
                    // Analysequelle); der normierte Rahmen passt daher direkt auf das Bild.
                    let image = self.thumbnails.hover_image(
                        ui,
                        PreviewRequest {
                            root: Some(PathBuf::from(&root)),
                            path: Path::new(&root).join(&face.source),
                            orientation: 1,
                            crop: None,
                            edge: crate::config::HOVER_PREVIEW_EDGE,
                            generation: 0,
                        },
                        &exiftool,
                        size,
                    );
                    if let Some(frame) =
                        image.and_then(|image| face_frame(face.bbox, image, HOVER_FRAME_MARGIN))
                    {
                        paint_hover_frame(ui.painter(), frame);
                    }
                });
            });
    }
    pub(super) fn prepare_group_advance(&mut self, undo: &Undo) {
        if !self.shows_face_groups()
            || !matches!(
                self.page_mode(),
                "unknown" | "review" | "unsuitable" | "probable" | "person-confirmed"
            )
        {
            return;
        }
        let Some(slot) = self.face_page.groups.iter().position(|group| {
            group
                .iter()
                .any(|face| undo.before.iter().any(|snapshot| snapshot.id == face.id))
        }) else {
            return;
        };
        // Bleiben Gesichter der Gruppe übrig (Kachelsymbol, „N abgewählte sind nicht X“),
        // bleibt sie stehen: kein Sprung zur nächsten Gruppe.
        if self.face_page.groups[slot]
            .iter()
            .any(|face| !undo.before.iter().any(|snapshot| snapshot.id == face.id))
        {
            return;
        }
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

    /// Kleine Vorschlagskarte: Porträt der Person, darunter Name und Wert; `hint` ist der
    /// Hinweistext unter der Maus.
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
        hint: Option<&str>,
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
        let response = match hint {
            Some(hint) => response.on_hover_text(hint),
            None => response,
        };
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

    /// `top`: oberste Gruppe der Seite; nur ihre Knöpfe nennen die Tasten.
    fn group(&mut self, ui: &mut egui::Ui, group: &[Face], focus: bool, top: bool) {
        let key = draft_key(group);
        let mode = self.page_mode().to_string();
        let kind = card_kind(&mode);
        let naming = self.page_mode() == "unknown";
        // In „Vorschläge prüfen“ und auf der Gesichterseite einer Person steht die Person fest.
        let proposed = matches!(kind, CardKind::Review | CardKind::Assigned)
            .then(|| group.first().and_then(|f| f.person))
            .flatten()
            .and_then(|id| self.face_page.people.iter().find(|p| p.id == id).cloned());
        // Vorgemerkte Benennungen blockieren weitere Entscheidungen nicht (ADR 0068).
        let busy = self.naming_blocked();
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
                // Ungeeignete Gesichter wählt man einzeln aus; alle anderen Karten beginnen
                // mit allen Gesichtern.
                excluded: if kind == CardKind::Unsuitable {
                    group.iter().map(|f| f.id).collect()
                } else {
                    BTreeSet::new()
                },
            }
        });
        let mut decision = None;
        // Rechtsklick „Einzeln benennen …“ auf einer Kachel: Dialog für genau dieses Gesicht.
        let mut name_one = None;
        // Rechtsklick „Foto in der Sammlung zeigen“.
        let mut show_photo = None;
        let proposals = group
            .first()
            .and_then(|f| self.face_page.suggestions.get(&f.id))
            .cloned();
        // „Ist das X?“ gibt es nur in „Benennen“ und nur bei einem sicheren Vorschlag.
        let sure = naming
            .then(|| sure_suggestion(&self.face_page.status, proposals.as_ref()))
            .flatten();
        // Zielperson der Kachelsymbole (Haken/Kreuz); ohne sie gibt es nur das Fragezeichen.
        let target = tile_target(&mode, &self.face_page, group);
        let show_icons = self.settings.tile_icons;
        let tile_hint = tile_hint(show_icons, target.as_ref(), can_ignore(&mode));
        theme::card(ui).show(ui, |ui| {
            // Auch kleine Gruppen belegen die volle Breite statt auf ihren Inhalt zu schrumpfen.
            ui.set_min_width(ui.available_width());
            ui.push_id(&key, |ui| {
                if let Some(person) = &proposed {
                    let selected = group
                        .iter()
                        .filter(|f| !draft.excluded.contains(&f.id))
                        .count();
                    ui.horizontal(|ui| {
                        let size = egui::vec2(56.0, 56.0);
                        match self.face_page.portraits.get(&person.id) {
                            Some(face) => {
                                let response = self
                                    .thumbnails
                                    .image(
                                        ui,
                                        PreviewRequest {
                                            root: Some(PathBuf::from(&root)),
                                            path: Path::new(&root).join(&face.source),
                                            orientation: 1,
                                            crop: Some(face.bbox),
                                            edge: 160,
                                            generation: 0,
                                        },
                                        &exiftool,
                                        size,
                                    )
                                    .on_hover_text(format!(
                                        "Ein bestätigtes Bild von {}\n{SINGLE_NAMING_HINT}",
                                        person.name
                                    ));
                                if single_naming_menu(&response) {
                                    name_one = Some(face.id);
                                }
                            }
                            None => {
                                ui.allocate_space(size);
                            }
                        }
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&person.name).size(17.0).strong())
                                .on_hover_text(select::person_caption(
                                    &self.face_page.people,
                                    person.id,
                                    &person.name,
                                ));
                            let mut detail = match group.len() {
                                1 => "1 Gesicht".to_string(),
                                n => format!("{n} Gesichter"),
                            };
                            if let Some(range) = similarity_text(group.iter().map(|f| f.score)) {
                                detail = format!("{detail} · {range}");
                            }
                            if let Some(others) = person_faces_view::better_summary(
                                group,
                                &self.face_page.better,
                                &self.face_page.people,
                            ) {
                                detail = format!("{detail} · {others}");
                            }
                            ui.label(RichText::new(detail).small().weak());
                        });
                        ui.add_space(12.0);
                        if kind == CardKind::Assigned {
                            decision = decision.or(assigned_buttons(
                                ui,
                                busy,
                                group.len() - selected,
                                group.len(),
                                &person.name,
                            ));
                            return;
                        }
                        let confirm = ui.add_enabled(
                            !busy && selected > 0,
                            egui::Button::new(confirm_label(selected, group.len(), &person.name)),
                        );
                        if key_hint(confirm, top, "J").clicked() {
                            decision = Some(Decision::Confirm);
                        }
                        let reject = ui.add_enabled(
                            !busy,
                            egui::Button::new(reject_label(group.len(), &person.name)),
                        );
                        if key_hint(reject, top, "N").clicked() {
                            decision = Some(Decision::Reject);
                        }
                        let ignore = ui.add_enabled(!busy, egui::Button::new("Nicht interessiert"));
                        if key_hint(ignore, top, "Entf").clicked() {
                            decision = Some(Decision::Ignore);
                        }
                    });
                } else if kind == CardKind::Unsuitable {
                    decision = unsuitable_header(ui, draft, &self.face_page.people, group, busy);
                } else {
                    if let Some((person, name, _)) = &sure {
                        let fill = ui.visuals().selection.bg_fill.gamma_multiply(0.25_f32);
                        egui::Frame::new()
                            .fill(fill)
                            .corner_radius(4.0_f32)
                            .inner_margin(6.0_f32)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let size = egui::vec2(40.0, 40.0);
                                    match proposals.as_ref().and_then(|p| p.portraits.get(person)) {
                                        Some(face) => {
                                            let response = self
                                                .thumbnails
                                                .image(
                                                    ui,
                                                    PreviewRequest {
                                                        root: Some(PathBuf::from(&root)),
                                                        path: Path::new(&root).join(&face.source),
                                                        orientation: 1,
                                                        crop: Some(face.bbox),
                                                        edge: 160,
                                                        generation: 0,
                                                    },
                                                    &exiftool,
                                                    size,
                                                )
                                                .on_hover_text(format!(
                                                    "Ein bestätigtes Bild von {name}\n{SINGLE_NAMING_HINT}"
                                                ));
                                            if single_naming_menu(&response) {
                                                name_one = Some(face.id);
                                            }
                                        }
                                        None => {
                                            ui.allocate_space(size);
                                        }
                                    }
                                    ui.label(
                                        RichText::new(format!("Ist das {name}?"))
                                            .size(17.0)
                                            .strong(),
                                    );
                                    let yes = ui.add_enabled(!busy, egui::Button::new("Ja"));
                                    if key_hint(yes, top, "J").clicked() {
                                        draft.person = Some(*person);
                                        draft.name = name.clone();
                                        decision = Some(Decision::Pick(0));
                                    }
                                    let no = ui.add_enabled(!busy, egui::Button::new("Nein"));
                                    if key_hint(no, top, "N").clicked() {
                                        decision = Some(Decision::RejectSure);
                                    }
                                });
                            });
                    }
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
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            decision = Some(Decision::Confirm);
                        }
                        let mut chosen = draft.person;
                        let items = select::person_choices(&self.face_page.people, None);
                        if select::FilterSelect::new("group-person", &items)
                            .placeholder("Person wählen …")
                            .width(200.0)
                            .show(ui, &mut chosen)
                            && let Some(person) = chosen
                        {
                            draft.person = Some(person);
                            if let Some(found) =
                                self.face_page.people.iter().find(|p| p.id == person)
                            {
                                draft.name = found.name.clone();
                            }
                        }
                        if ui
                            .add_enabled(!busy, egui::Button::new("Bestätigen"))
                            .clicked()
                        {
                            decision = Some(Decision::Confirm);
                        }
                        let ignore = ui.add_enabled(!busy, egui::Button::new("Nicht interessiert"));
                        if key_hint(ignore, top, "Entf").clicked() {
                            decision = Some(Decision::Ignore);
                        }
                    });
                    if let Some(proposals) = &proposals {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("Ähnlichste Personen:").small().weak());
                            for (index, (person, name, score)) in proposals.items.iter().enumerate()
                            {
                                let detail = crate::core::progress::decimal(*score, 2);
                                let hint =
                                    top.then(|| format!("Taste {} (oberste Gruppe)", index + 1));
                                if Self::suggestion_chip(
                                    &mut self.thumbnails,
                                    ui,
                                    &root,
                                    &exiftool,
                                    *person,
                                    proposals.portraits.get(person),
                                    name,
                                    &detail,
                                    hint.as_deref(),
                                    !busy,
                                ) {
                                    draft.person = Some(*person);
                                    draft.name = name.clone();
                                    decision = Some(Decision::Pick(index));
                                }
                            }
                        });
                    }
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
                                ui.set_max_width(size);
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
                                let response = response
                                    .on_hover_text(format!("Gesicht {}\n{tile_hint}", face.id));
                                match tile_menu(&response) {
                                    Some(TileAction::Name) => name_one = Some(face.id),
                                    Some(TileAction::ShowPhoto) => show_photo = Some(face.clone()),
                                    None => {}
                                }
                                if hover_enabled && response.hovered() {
                                    self.hover_face = Some(face.clone());
                                }
                                if response.clicked() {
                                    toggle_excluded(&mut draft.excluded, face.id);
                                }
                                if kind == CardKind::Unsuitable {
                                    // Hier wird ausgewählt, nicht abgewählt: Auswahl hervorheben.
                                    if !draft.excluded.contains(&face.id) {
                                        ui.painter().rect_stroke(
                                            response.rect,
                                            2.0,
                                            egui::Stroke::new(
                                                3.0_f32,
                                                ui.visuals().selection.bg_fill,
                                            ),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                } else if draft.excluded.contains(&face.id) {
                                    let painter = ui.painter();
                                    let rect = response.rect;
                                    painter.rect_filled(
                                        rect,
                                        2.0,
                                        egui::Color32::from_black_alpha(150),
                                    );
                                    painter.line_segment(
                                        [rect.left_bottom(), rect.right_top()],
                                        egui::Stroke::new(2.0_f32, egui::Color32::from_gray(230)),
                                    );
                                }
                                // Nach der Kachel angemeldet, damit die Symbole über ihr liegen
                                // und ihr Klick die Auswahl nicht umschaltet.
                                // Abschaltbar in der Fußzeile („Symbole auf den Gesichtern“).
                                let icon = show_icons
                                    .then(|| {
                                        tile_icons(
                                            ui,
                                            response.rect,
                                            ui.id().with(("tile-icons", face.id)),
                                            target.as_ref(),
                                            can_ignore(&mode),
                                            !busy,
                                        )
                                    })
                                    .flatten();
                                match icon {
                                    Some(TileIcon::Ignore) => {
                                        decision = Some(Decision::OneIgnore(face.id))
                                    }
                                    Some(TileIcon::Is) => decision = Some(Decision::OneIs(face.id)),
                                    Some(TileIcon::IsNot) => {
                                        decision = Some(Decision::OneIsNot(face.id))
                                    }
                                    Some(TileIcon::Name) => name_one = Some(face.id),
                                    None => {}
                                }
                                if let Some(note) = person_faces_view::better_note(
                                    self.face_page.better.get(&face.id),
                                    &self.face_page.people,
                                ) {
                                    ui.add(
                                        egui::Label::new(RichText::new(note).small().weak())
                                            .truncate(),
                                    );
                                }
                                if let Some(note) = rejection_note(
                                    self.face_page.rejections.get(&face.id),
                                    &self.face_page.people,
                                ) {
                                    ui.add(
                                        egui::Label::new(RichText::new(note).small().weak())
                                            .truncate(),
                                    );
                                }
                            });
                        }
                    });
                }
            });
        });
        if let Some(decision) = decision
            && self.decide_group(group, decision)
        {
            self.face_keys = false;
        }
        if let Some(id) = name_one {
            self.open_naming(id);
        }
        if let Some(face) = show_photo {
            self.show_face_photo(&face);
        }
    }

    /// Öffnet die Sammlung mit dem Foto dieses Gesichts: Suche nach Ordner und Dateiname
    /// ohne Endung (trifft auch die RAW-Fassung), das erste Ergebnis ist gewählt.
    pub(super) fn show_face_photo(&mut self, face: &Face) {
        self.filters = Search {
            text: photo_search_text(&face.source),
            ..Default::default()
        };
        self.selected_photo = None;
        self.view = View::Collection;
        self.refresh_collection();
    }

    /// „Kein Gesicht“ für diese Gesichter: sofort ausgeblendet, gespeichert im Hintergrund,
    /// mit Strg+Z zurückzunehmen.
    fn mark_not_face(&mut self, ids: Vec<i64>) -> bool {
        if ids.is_empty() {
            return false;
        }
        let hide = ids.clone();
        let work = self
            .on_collection(move |mut c, _| Ok(Outcome::Named(faces::mark_not_face(&mut c, &ids)?)));
        self.queue_hiding("Als „Kein Gesicht“ speichern", hide, work);
        true
    }

    /// Tasten für die oberste Gruppe in „Benennen“ und „Vorschläge prüfen“: `J` bestätigt
    /// den Vorschlag, `N` lehnt ihn ab (beide nur in „Vorschläge prüfen“), `1` bis `3`
    /// übernehmen einen der Vorschläge in „Benennen“, `Entf` heißt „Nicht interessiert“.
    /// Nur ohne Fokus in einem Feld und solange die Knöpfe aktiv wären.
    pub(super) fn face_shortcuts(&mut self, ctx: &egui::Context) {
        let review = match self.page_mode() {
            "review" | "probable" => true,
            "unknown" => false,
            _ => return,
        };
        if self.naming_blocked()
            || self.recognition_tools_open
            || ctx.memory(|m| m.focused().is_some())
        {
            return;
        }
        let Some(group) = self.face_page.groups.first().cloned() else {
            return;
        };
        let pressed = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        let decision = if pressed(egui::Key::Delete) {
            Decision::Ignore
        } else if review && pressed(egui::Key::J) {
            // Wie der Knopf: ohne ausgewählte Kachel gibt es nichts zu bestätigen.
            let none_selected = self
                .drafts
                .get(&draft_key(&group))
                .is_some_and(|draft| group.iter().all(|f| draft.excluded.contains(&f.id)));
            if group[0].person.is_none() || none_selected {
                return;
            }
            Decision::Confirm
        } else if review && pressed(egui::Key::N) {
            Decision::Reject
        } else if !review
            && sure_for_group(&self.face_page, &group).is_some()
            && pressed(egui::Key::J)
        {
            Decision::Pick(0)
        } else if !review
            && sure_for_group(&self.face_page, &group).is_some()
            && pressed(egui::Key::N)
        {
            Decision::RejectSure
        } else if let Some(index) = [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3]
            .into_iter()
            .position(pressed)
            .filter(|_| !review)
        {
            Decision::Pick(index)
        } else {
            return;
        };
        if self.decide_group(&group, decision) {
            self.face_keys = true;
        }
    }

    /// Speichert eine Entscheidung für eine Gruppe; Knöpfe und Tasten nutzen denselben Weg.
    /// Abgewählte Kacheln und der Namensentwurf der Gruppe gelten mit. Gibt `true` zurück,
    /// wenn ein Schreibauftrag vorgemerkt wurde.
    pub(super) fn decide_group(&mut self, group: &[Face], decision: Decision) -> bool {
        if self.naming_blocked() || group.is_empty() {
            return false;
        }
        let ids: Vec<i64> = group.iter().map(|f| f.id).collect();
        let draft = self.drafts.get(&draft_key(group));
        let kept: BTreeSet<i64> = ids
            .iter()
            .filter(|id| draft.is_none_or(|draft| !draft.excluded.contains(id)))
            .copied()
            .collect();
        let proposed = group[0].person;
        let (person, name) = match draft {
            Some(draft) => (draft.person, draft.name.clone()),
            None => (proposed, String::new()),
        };
        let (ids, selected, person, name, ignore) = match decision {
            Decision::Confirm => (ids, kept, person, name, false),
            Decision::Reject => {
                // Ohne Auswahl gilt „keines davon ist diese Person“.
                let Some(person) = proposed else {
                    return false;
                };
                (ids, BTreeSet::new(), Some(person), String::new(), false)
            }
            Decision::RejectSure => {
                // „Nein“ auf „Ist das X?“: keines der Gesichter ist X.
                let Some((person, _, _)) = sure_for_group(&self.face_page, group) else {
                    return false;
                };
                (ids, BTreeSet::new(), Some(person), String::new(), false)
            }
            Decision::Ignore => (ids, kept, person, name, true),
            Decision::Pick(index) => {
                let Some((person, name, _)) = self
                    .face_page
                    .suggestions
                    .get(&ids[0])
                    .and_then(|found| found.items.get(index))
                    .cloned()
                else {
                    return false;
                };
                (ids, kept, Some(person), name, false)
            }
            Decision::NameSelected => {
                // Nur die ausgewählten Gesichter; die übrigen erhalten keine Ablehnung.
                if kept.is_empty() || person.is_none() && name.trim().is_empty() {
                    return false;
                }
                (kept.iter().copied().collect(), kept, person, name, false)
            }
            Decision::RemoveExcluded => {
                // Nur die abgewählten Gesichter verlieren die Person; die übrigen bleiben
                // unberührt (samt Referenzauswahl).
                let removed: Vec<i64> = ids.into_iter().filter(|id| !kept.contains(id)).collect();
                let Some(person) = proposed.filter(|_| !removed.is_empty()) else {
                    return false;
                };
                (removed, BTreeSet::new(), Some(person), String::new(), false)
            }
            Decision::NotFace => return self.mark_not_face(kept.into_iter().collect()),
            Decision::OneIgnore(id) => {
                // Wie „Nicht interessiert“, nur für dieses Gesicht; bestätigte Identität bleibt.
                if !can_ignore(self.page_mode()) || !ids.contains(&id) {
                    return false;
                }
                self.carry_draft(group, id);
                (vec![id], BTreeSet::new(), None, String::new(), true)
            }
            Decision::OneIs(id) | Decision::OneIsNot(id) => {
                // Kachelsymbol: dieselbe Entscheidung wie für die Gruppe, nur für dieses
                // Gesicht; die übrigen bleiben unberührt (keine Ablehnung, keine Bestätigung).
                let confirm = matches!(decision, Decision::OneIs(_));
                let Some(target) = tile_target(self.page_mode(), &self.face_page, group)
                    .filter(|target| ids.contains(&id) && (target.confirm || !confirm))
                else {
                    return false;
                };
                let selected = if confirm {
                    BTreeSet::from([id])
                } else {
                    BTreeSet::new()
                };
                self.carry_draft(group, id);
                (
                    vec![id],
                    selected,
                    Some(target.person),
                    String::new(),
                    false,
                )
            }
        };
        let config = self.settings.central();
        let hide = ids.clone();
        let work = self.on_collection(move |mut c, _| {
            let (undo, note) = crate::core::central::name_group_synced(
                &mut c, &config, &ids, &selected, person, &name, ignore,
            )?;
            Ok(Outcome::Synced(Box::new(Outcome::Named(undo)), note))
        });
        self.queue_hiding("Gesichtsgruppe speichern", hide, work);
        true
    }

    /// Nach einer Entscheidung über ein einzelnes Gesicht gehen Auswahl und Namensentwurf der
    /// übrigen Gesichter in die verkleinerte Karte über.
    fn carry_draft(&mut self, group: &[Face], id: i64) {
        let rest: Vec<Face> = group.iter().filter(|f| f.id != id).cloned().collect();
        let Some(draft) = self
            .drafts
            .get(&draft_key(group))
            .filter(|_| !rest.is_empty())
        else {
            return;
        };
        let carried = Draft {
            name: draft.name.clone(),
            person: draft.person,
            excluded: draft
                .excluded
                .iter()
                .copied()
                .filter(|x| *x != id)
                .collect(),
        };
        self.drafts.insert(draft_key(&rest), carried);
    }
}

/// Gehört der Reiter zu „Erledigt“ („Bestätigt“ oder „Ignoriert“)?
/// Hinweis unter der Maus für jede Gesichtskachel mit Rechtsklickmenü.
pub(super) const SINGLE_NAMING_HINT: &str = "Rechtsklick: einzeln benennen";

/// Rechtsklickmenü einer Gesichtskachel; `true`, wenn „Einzeln benennen …“ gewählt wurde.
/// Der Linksklick der Kachel bleibt unberührt.
pub(super) fn single_naming_menu(response: &egui::Response) -> bool {
    let mut chosen = false;
    response.context_menu(|ui| {
        if ui.button("Einzeln benennen …").clicked() {
            chosen = true;
            ui.close();
        }
    });
    chosen
}

pub(super) fn done_tab(mode: &str) -> bool {
    matches!(mode, "confirmed" | "ignored")
}

/// Entscheidung für eine Gesichtsgruppe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Decision {
    /// Ausgewählte Gesichter bestätigen (abgewählte werden abgelehnt).
    Confirm,
    /// Keines der Gesichter ist die vorgeschlagene Person.
    Reject,
    /// „Nein“ bei „Ist das X?“: der sichere Vorschlag trifft für kein Gesicht zu.
    RejectSure,
    /// „Nicht interessiert“.
    Ignore,
    /// Den Vorschlag mit dieser Nummer (ab 0) übernehmen.
    Pick(usize),
    /// Nur die ausgewählten Gesichter benennen („Ungeeignet“); die übrigen bleiben offen.
    NameSelected,
    /// Die ausgewählten Gesichter sind „Kein Gesicht“ („Ungeeignet“).
    NotFace,
    /// Abgewählte bestätigte Gesichter sind nicht die Person (Gesichterseite einer Person).
    RemoveExcluded,
    /// Kachelsymbol Haken: genau dieses Gesicht ist die Zielperson der Gruppe.
    OneIs(i64),
    /// Kachelsymbol Kreuz: genau dieses Gesicht ist nicht die Zielperson der Gruppe.
    OneIsNot(i64),
    /// Kachelsymbol „nicht interessiert“: nur dieses Gesicht ignorieren.
    OneIgnore(i64),
}

/// Lässt sich ein Gesicht dieser Seite einzeln ignorieren? Überall außer in „Ignoriert“.
fn can_ignore(mode: &str) -> bool {
    mode != "ignored"
}

/// Zielperson X einer Gruppe für die Kachelsymbole.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TileTarget {
    pub(super) person: i64,
    pub(super) name: String,
    /// Haken anbieten? Nur für noch nicht bestätigte Gesichter.
    pub(super) confirm: bool,
}

/// Zielperson der Gruppe für Haken und Kreuz: in „Vorschläge prüfen“ und „Wahrscheinlich X“
/// die vorgeschlagene Person, in „Benennen“ der sichere Vorschlag („Ist das X?“), auf Seiten
/// bestätigter Gesichter die Person (dort nur das Kreuz). Sonst keine („Ungeeignet“,
/// „Ignoriert“, unbenannte Gruppen ohne sicheren Vorschlag).
pub(super) fn tile_target(
    mode: &str,
    page: &faces::FacePage,
    group: &[Face],
) -> Option<TileTarget> {
    let first = group.first()?;
    let named = |person: i64, confirm: bool| {
        let name = page.people.iter().find(|p| p.id == person)?.name.clone();
        Some(TileTarget {
            person,
            name,
            confirm,
        })
    };
    match mode {
        "review" | "probable" => named(first.person?, true),
        "confirmed" | "person-confirmed" => named(first.person?, false),
        "unknown" if first.person.is_none() => {
            let (person, name, _) = sure_for_group(page, group)?;
            Some(TileTarget {
                person,
                name,
                confirm: true,
            })
        }
        _ => None,
    }
}

/// Art der Gruppenkarte, folgt aus der gelesenen Seite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CardKind {
    /// Unbenannte, bestätigte oder ignorierte Gesichter: Namensfeld, Auswahl, Vorschläge.
    Name,
    /// Vorschläge für eine Person („Vorschläge prüfen“, „Wahrscheinlich X“).
    Review,
    /// Bestätigte Gesichter einer Person: falsche abwählen und entfernen.
    Assigned,
    /// Ungeeignete Gesichter: einzelne auswählen, benennen oder „Kein Gesicht“.
    Unsuitable,
}

pub(super) fn card_kind(mode: &str) -> CardKind {
    match mode {
        "review" | "probable" => CardKind::Review,
        "person-confirmed" => CardKind::Assigned,
        "unsuitable" => CardKind::Unsuitable,
        _ => CardKind::Name,
    }
}

/// Knöpfe der Karte bestätigter Gesichter einer Person: abgewählte entfernen oder alle.
fn assigned_buttons(
    ui: &mut egui::Ui,
    busy: bool,
    excluded: usize,
    total: usize,
    name: &str,
) -> Option<Decision> {
    let mut decision = None;
    let remove = ui
        .add_enabled(
            !busy && excluded > 0,
            egui::Button::new(remove_label(excluded, name)),
        )
        .on_hover_text("Klicke falsch zugeordnete Gesichter an, um sie abzuwählen.");
    if remove.clicked() {
        decision = Some(Decision::RemoveExcluded);
    }
    if ui
        .add_enabled(!busy, egui::Button::new(reject_label(total, name)))
        .clicked()
    {
        decision = Some(Decision::Reject);
    }
    decision
}

/// „2 abgewählte sind nicht Anna“; ohne Abwahl ein Hinweis, was zu tun ist.
fn remove_label(excluded: usize, name: &str) -> String {
    match excluded {
        0 => format!("Falsche abwählen, dann: nicht {name}"),
        1 => format!("1 abgewähltes ist nicht {name}"),
        n => format!("{n} abgewählte sind nicht {name}"),
    }
}

/// Kopf einer Karte ungeeigneter Gesichter: Auswahl, Name oder Person für die Auswahl,
/// „N benennen“ und „Kein Gesicht“.
fn unsuitable_header(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    people: &[faces::Person],
    group: &[Face],
    busy: bool,
) -> Option<Decision> {
    let mut decision = None;
    let selected = group
        .iter()
        .filter(|f| !draft.excluded.contains(&f.id))
        .count();
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!("{} Gesichter · {selected} ausgewählt", group.len())).strong(),
        );
        if ui.small_button("Alle auswählen").clicked() {
            draft.excluded.clear();
        }
        if ui.small_button("Auswahl aufheben").clicked() {
            draft.excluded = group.iter().map(|f| f.id).collect();
        }
    });
    ui.horizontal_wrapped(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut draft.name)
                .hint_text("Name für die Auswahl")
                .desired_width(220.0),
        );
        if response.changed() {
            draft.person = None;
        }
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            decision = Some(Decision::NameSelected);
        }
        let mut chosen = draft.person;
        let items = select::person_choices(people, None);
        if select::FilterSelect::new("unsuitable-person", &items)
            .placeholder("Person wählen …")
            .width(200.0)
            .show(ui, &mut chosen)
            && let Some(person) = chosen
        {
            draft.person = Some(person);
            if let Some(found) = people.iter().find(|p| p.id == person) {
                draft.name = found.name.clone();
            }
        }
        let named = draft.person.is_some() || !draft.name.trim().is_empty();
        if ui
            .add_enabled(
                !busy && selected > 0 && named,
                egui::Button::new(format!("{selected} benennen")),
            )
            .clicked()
        {
            decision = Some(Decision::NameSelected);
        }
        if ui
            .add_enabled(!busy && selected > 0, egui::Button::new("Kein Gesicht"))
            .on_hover_text(
                "Die ausgewählten Ausschnitte zeigen kein Gesicht (z. B. ein Muster). Sie \
                 verschwinden aus allen Zählungen und kehren auch nach einer neuen Analyse \
                 nicht zurück. Strg+Z nimmt es zurück.",
            )
            .clicked()
        {
            decision = Some(Decision::NotFace);
        }
    });
    decision
}

/// Aktion aus dem Rechtsklickmenü einer Kachel in einer Gruppenkarte.
#[derive(Clone, Copy, Debug, PartialEq)]
enum TileAction {
    Name,
    ShowPhoto,
}
/// Hinweis unter der Maus für Gruppenkacheln: Rechtsklickmenü ([`tile_menu`]) und, wenn
/// eingeschaltet, die Symbole, die diese Kachel zeigt.
fn tile_hint(icons: bool, target: Option<&TileTarget>, ignore: bool) -> String {
    let mut lines = Vec::new();
    if icons {
        if let Some(target) = target {
            lines.push(if target.confirm {
                "Doppelklick auf Haken oder Kreuz oben rechts: nur dieses Gesicht ist bzw. ist \
                 nicht die Person"
            } else {
                "Doppelklick auf das Kreuz oben rechts: dieses Gesicht ist nicht die Person"
            });
        }
        if ignore {
            lines.push("Doppelklick auf den durchgestrichenen Kreis: nicht interessiert");
        }
        lines.push("Fragezeichen oben rechts: Namen wählen");
    }
    lines.push("Rechtsklick: einzeln benennen oder Foto in der Sammlung zeigen");
    lines.join("\n")
}

/// Symbol oben rechts auf einer Gruppenkachel.
#[derive(Clone, Copy, Debug, PartialEq)]
enum TileIcon {
    /// Durchgestrichener Kreis: ein Gesicht, aber für die Sammlung uninteressant
    /// („Nicht interessiert“ für nur dieses Gesicht, Doppelklick).
    Ignore,
    /// Kreuz: „das ist nicht X“ (Doppelklick).
    IsNot,
    /// Haken: „das ist X“ (Doppelklick).
    Is,
    /// Fragezeichen: Namen wählen (Klick).
    Name,
}

/// Hinweistext (und Beschriftung für Bedienhilfen) eines Kachelsymbols.
fn tile_icon_hint(icon: TileIcon, name: &str) -> String {
    match icon {
        TileIcon::Ignore => {
            "Doppelklick: ein Gesicht, aber nicht interessant (nur dieses ignorieren)".into()
        }
        TileIcon::IsNot => format!("Doppelklick: das ist nicht {name}"),
        TileIcon::Is => format!("Doppelklick: das ist {name}"),
        TileIcon::Name => "Namen wählen …".into(),
    }
}

/// Die Symbole oben rechts auf einer Gruppenkachel, für alle Seiten mit Gruppenkarten gleich:
/// von links „nicht interessiert“ (nur mit `ignore`), Kreuz, Haken (beide nur mit Zielperson;
/// der Haken nur, wenn `target.confirm`) und Fragezeichen. Alle außer dem Fragezeichen wirken
/// erst auf Doppelklick, das Fragezeichen auf Klick.
/// Die Symbole müssen nach der Kachel angemeldet werden: Dann liegen sie über ihr, und ein
/// Klick auf ein Symbol schaltet die Auswahl der Kachel nicht um. Dauerhaft dezent, unter der
/// Maus hervorgehoben; dunkler Grund mit hellem Zeichen, lesbar in hell und dunkel.
fn tile_icons(
    ui: &mut egui::Ui,
    tile: egui::Rect,
    id: egui::Id,
    target: Option<&TileTarget>,
    ignore: bool,
    enabled: bool,
) -> Option<TileIcon> {
    let mut icons = Vec::with_capacity(4);
    if ignore {
        icons.push(TileIcon::Ignore);
    }
    if let Some(target) = target {
        icons.push(TileIcon::IsNot);
        if target.confirm {
            icons.push(TileIcon::Is);
        }
    }
    icons.push(TileIcon::Name);
    let name = target.map(|t| t.name.as_str()).unwrap_or_default();
    // Vier Symbole passen auch auf die kleinste Kachel (60 Punkte).
    let edge = (tile.width() / 6.0).clamp(12.0, 20.0);
    let (margin, gap) = (3.0, 2.0);
    let mut chosen = None;
    for (index, icon) in icons.iter().rev().enumerate() {
        let right = tile.right() - margin - index as f32 * (edge + gap);
        let rect = egui::Rect::from_min_size(
            egui::pos2(right - edge, tile.top() + margin),
            egui::vec2(edge, edge),
        );
        // Das Fragezeichen öffnet nur den Dialog und bleibt deshalb auch während eines
        // Schreibauftrags bedienbar.
        let active = enabled || *icon == TileIcon::Name;
        let hint = tile_icon_hint(*icon, name);
        let response = ui.interact(rect, id.with(*icon as u8), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, active, hint.clone())
        });
        let response = response.on_hover_text(&hint);
        let hovered = response.hovered() && active;
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let painter = ui.painter();
        let center = rect.center();
        let radius = edge / 2.0;
        painter.circle_filled(
            center,
            radius,
            egui::Color32::from_black_alpha(if hovered { 210 } else { 120 }),
        );
        let color = match (hovered, active, icon) {
            (_, false, _) => egui::Color32::from_white_alpha(90),
            (true, _, TileIcon::Is) => egui::Color32::from_rgb(130, 230, 130),
            (true, _, TileIcon::IsNot) => egui::Color32::from_rgb(255, 130, 130),
            (true, _, TileIcon::Ignore) => egui::Color32::from_rgb(240, 200, 110),
            (true, _, TileIcon::Name) => egui::Color32::WHITE,
            (false, _, _) => egui::Color32::from_white_alpha(200),
        };
        let stroke = egui::Stroke::new(1.6_f32, color);
        let r = radius * 0.45;
        match icon {
            TileIcon::Ignore => {
                // Durchgestrichener Kreis wie ein Verbotszeichen: „egal für die Sammlung“.
                painter.circle_stroke(center, r * 1.2, stroke);
                painter.line_segment(
                    [
                        center + egui::vec2(-r * 0.85, r * 0.85),
                        center + egui::vec2(r * 0.85, -r * 0.85),
                    ],
                    stroke,
                );
            }
            TileIcon::IsNot => {
                painter.line_segment(
                    [center + egui::vec2(-r, -r), center + egui::vec2(r, r)],
                    stroke,
                );
                painter.line_segment(
                    [center + egui::vec2(-r, r), center + egui::vec2(r, -r)],
                    stroke,
                );
            }
            TileIcon::Is => {
                painter.add(egui::Shape::line(
                    vec![
                        center + egui::vec2(-r, 0.0),
                        center + egui::vec2(-r * 0.25, r * 0.8),
                        center + egui::vec2(r * 1.1, -r * 0.9),
                    ],
                    stroke,
                ));
            }
            TileIcon::Name => {
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    "?",
                    egui::FontId::proportional(edge * 0.75),
                    color,
                );
            }
        }
        if !active {
            continue;
        }
        let fired = match icon {
            TileIcon::Name => response.clicked(),
            // Eigener Doppelklick: beide Klicks müssen dieses Symbol treffen. egui zählt auch
            // einen schnellen Klick auf die Kachel und dann auf das Symbol als Doppelklick.
            TileIcon::Ignore | TileIcon::Is | TileIcon::IsNot => {
                response.clicked() && {
                    let now = ui.input(|i| i.time);
                    let delay = ui.ctx().options(|o| o.input_options.max_double_click_delay);
                    let key = response.id.with("last-click");
                    let last = ui.data(|d| d.get_temp::<f64>(key));
                    let double = last.is_some_and(|last| now - last <= delay);
                    ui.data_mut(|d| {
                        if double {
                            d.remove::<f64>(key);
                        } else {
                            d.insert_temp(key, now);
                        }
                    });
                    double
                }
            }
        };
        if fired {
            chosen = Some(*icon);
        }
    }
    chosen
}

/// Rechtsklickmenü einer Kachel in einer Gruppenkarte: „Einzeln benennen …“ und „Foto in
/// der Sammlung zeigen“. Der Linksklick bleibt unberührt.
fn tile_menu(response: &egui::Response) -> Option<TileAction> {
    let mut chosen = None;
    response.context_menu(|ui| {
        if ui.button("Einzeln benennen …").clicked() {
            chosen = Some(TileAction::Name);
            ui.close();
        }
        if ui.button("Foto in der Sammlung zeigen").clicked() {
            chosen = Some(TileAction::ShowPhoto);
            ui.close();
        }
    });
    chosen
}

/// Suchtext der Sammlung für das Foto einer Gesichtsquelle: Pfad ohne Endung, damit auch
/// eine RAW- oder DNG-Fassung desselben Fotos gefunden wird.
fn photo_search_text(source: &str) -> String {
    match source.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.contains('/') => stem.to_string(),
        _ => source.to_string(),
    }
}

/// Erklärung über dem Reiter „Ungeeignet“.
const UNSUITABLE_HINT: &str = "Diese Gesichter sind zu klein oder unscharf für Namensvorschläge. \
     Wähle Gesichter durch Anklicken aus und benenne sie oder markiere sie als „Kein Gesicht“. \
     Rechtsklick öffnet das Foto in der Sammlung.";

/// Nennt die Taste im Hinweistext, aber nur an der obersten Gruppe: nur für sie gilt sie.
/// Der sichere Vorschlag einer Gruppe nach denselben Grenzen wie der Abgleich.
fn sure_suggestion(
    status: &faces::Status,
    suggestions: Option<&crate::core::recognition::Suggestions>,
) -> Option<(i64, String, f32)> {
    let limits = crate::core::recognition::Limits {
        review: status.review_threshold,
        margin: status.review_margin,
        ..Default::default()
    };
    limits.sure_suggestion(&suggestions?.items).cloned()
}

fn sure_for_group(page: &faces::FacePage, group: &[Face]) -> Option<(i64, String, f32)> {
    sure_suggestion(&page.status, page.suggestions.get(&group.first()?.id))
}

fn key_hint(response: egui::Response, top: bool, key: &str) -> egui::Response {
    if top {
        response.on_hover_text(format!("Taste {key} (oberste Gruppe)"))
    } else {
        response
    }
}

/// Wertebereich einer Gruppe: „Ähnlichkeit 0,62 bis 0,81“, bei gleichen Werten nur einer.
/// Gesichter ohne Wert zählen nicht; ohne jeden Wert gibt es keinen Text.
fn similarity_text(scores: impl Iterator<Item = f32>) -> Option<String> {
    let (low, high) = scores
        .filter(|score| score.is_finite() && *score > 0.0)
        .fold(None, |range: Option<(f32, f32)>, score| {
            Some(range.map_or((score, score), |(low, high)| {
                (low.min(score), high.max(score))
            }))
        })?;
    let decimal = |value: f32| crate::core::progress::decimal(value, 2);
    let (low, high) = (decimal(low), decimal(high));
    Some(if low == high {
        format!("Ähnlichkeit {low}")
    } else {
        format!("Ähnlichkeit {low} bis {high}")
    })
}

/// Beschriftung des Bestätigen-Knopfs: „Alle 5 sind Anna“, mit abgewählten Kacheln
/// „3 sind Anna“.
fn confirm_label(selected: usize, total: usize, name: &str) -> String {
    match (selected, total) {
        (1, 1) => format!("Das ist {name}"),
        (1, _) => format!("1 ist {name}"),
        (s, t) if s == t => format!("Alle {t} sind {name}"),
        (s, _) => format!("{s} sind {name}"),
    }
}

/// Beschriftung des Ablehnen-Knopfs: „Keines ist Anna“.
fn reject_label(total: usize, name: &str) -> String {
    if total == 1 {
        format!("Das ist nicht {name}")
    } else {
        format!("Keines ist {name}")
    }
}
#[cfg(test)]
mod review_text_tests {
    use super::*;
    #[test]
    fn similarity_range_uses_a_decimal_comma_and_skips_missing_values() {
        assert_eq!(
            similarity_text([0.81_f32, 0.624, 0.7].into_iter()).as_deref(),
            Some("Ähnlichkeit 0,62 bis 0,81")
        );
        assert_eq!(
            similarity_text([0.0_f32, 0.7].into_iter()).as_deref(),
            Some("Ähnlichkeit 0,70")
        );
        assert_eq!(
            similarity_text([0.701_f32, 0.699].into_iter()).as_deref(),
            Some("Ähnlichkeit 0,70"),
            "gleich gerundete Werte stehen nur einmal da"
        );
        assert_eq!(similarity_text([0.0_f32, f32::NAN].into_iter()), None);
        assert_eq!(similarity_text(std::iter::empty()), None);
    }
    #[test]
    fn button_labels_follow_the_selection() {
        assert_eq!(confirm_label(5, 5, "Anna"), "Alle 5 sind Anna");
        assert_eq!(confirm_label(3, 5, "Anna"), "3 sind Anna");
        assert_eq!(confirm_label(1, 5, "Anna"), "1 ist Anna");
        assert_eq!(confirm_label(1, 1, "Anna"), "Das ist Anna");
        assert_eq!(reject_label(5, "Anna"), "Keines ist Anna");
        assert_eq!(reject_label(1, "Anna"), "Das ist nicht Anna");
    }
}

/// Klick auf eine Kachel: abgewählt wird angewählt und umgekehrt.
fn toggle_excluded(excluded: &mut BTreeSet<i64>, id: i64) {
    if !excluded.remove(&id) {
        excluded.insert(id);
    }
}

/// „nicht Anna, Ben“ unter einer Kachel; unbekannte Personen-IDs entfallen.
fn rejection_note(rejected: Option<&Vec<i64>>, people: &[faces::Person]) -> Option<String> {
    let names: Vec<&str> = rejected?
        .iter()
        .filter_map(|id| people.iter().find(|p| p.id == *id))
        .map(|p| p.name.as_str())
        .collect();
    (!names.is_empty()).then(|| format!("nicht {}", names.join(", ")))
}

/// Hinweis auf Gesichter, die zu schlecht für Namensvorschläge sind (`Status::unsuitable`);
/// daneben steht der Weg in den Reiter „Ungeeignet“.
fn unnamed_hint(unsuitable: usize) -> Option<String> {
    (unsuitable > 0).then(|| {
        format!("{unsuitable} weitere Gesichter sind zu klein oder unscharf für Namensvorschläge.")
    })
}
#[cfg(test)]
mod tile_note_tests {
    use super::*;
    use crate::core::faces::Person;
    fn person(id: i64, name: &str) -> Person {
        Person {
            id,
            name: name.into(),
            birth: String::new(),
            count: 0,
            gender: String::new(),
            note: String::new(),
        }
    }
    #[test]
    fn rejection_note_lists_known_people_only() {
        let people = [person(1, "Anna"), person(2, "Ben")];
        assert_eq!(rejection_note(None, &people), None);
        assert_eq!(rejection_note(Some(&vec![9]), &people), None);
        assert_eq!(
            rejection_note(Some(&vec![1]), &people).as_deref(),
            Some("nicht Anna")
        );
        assert_eq!(
            rejection_note(Some(&vec![1, 9, 2]), &people).as_deref(),
            Some("nicht Anna, Ben")
        );
    }
    #[test]
    fn clicking_a_tile_twice_selects_it_again() {
        let mut excluded = BTreeSet::new();
        toggle_excluded(&mut excluded, 4);
        assert!(excluded.contains(&4));
        toggle_excluded(&mut excluded, 4);
        assert!(excluded.is_empty());
    }
    #[test]
    fn hint_appears_only_when_faces_lack_name_features() {
        let status = faces::Status {
            unknown: 10,
            nameable: 4,
            ..Default::default()
        };
        assert!(
            unnamed_hint(status.unsuitable())
                .is_some_and(|text| text.starts_with("6 weitere Gesichter"))
        );
        assert_eq!(unnamed_hint(0), None);
    }
    #[test]
    fn photo_search_drops_only_the_extension() {
        assert_eq!(photo_search_text("2024/01/IMG_1.jpg"), "2024/01/IMG_1");
        assert_eq!(photo_search_text("a.b/IMG_1"), "a.b/IMG_1");
        assert_eq!(photo_search_text("IMG.1.NEF"), "IMG.1");
    }
    #[test]
    fn card_kind_follows_the_page() {
        assert_eq!(card_kind("review"), CardKind::Review);
        assert_eq!(card_kind("probable"), CardKind::Review);
        assert_eq!(card_kind("person-confirmed"), CardKind::Assigned);
        assert_eq!(card_kind("unsuitable"), CardKind::Unsuitable);
        assert_eq!(card_kind("confirmed"), CardKind::Name);
        assert_eq!(remove_label(2, "Anna"), "2 abgewählte sind nicht Anna");
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
/// Abstand des Rahmens um das Gesicht in der Mausvorschau, in Punkten.
pub(super) const HOVER_FRAME_MARGIN: f32 = 4.0;
/// Dunkler Außenstrich des Rahmens: hebt die Akzentlinie auf hellen wie dunklen Fotos ab.
pub(super) const HOVER_FRAME_SHADE: egui::Color32 = egui::Color32::from_rgb(8, 12, 16);

/// Bildschirmrechteck eines Gesichts: `bbox` ist auf das orientierte Analysebild der
/// Quelldatei normiert (0..1, links, oben, rechts, unten; siehe ADR Gesichtserkennung), `image`
/// die Fläche, auf der genau dieses Bild unverzerrt gezeichnet ist. `margin` erweitert den
/// Rahmen nach außen; das Ergebnis wird an die Bildfläche geklemmt. `None` bei ungültigem
/// oder leerem Rahmen. Gemeinsame Umrechnung für Bilddetails und Mausvorschau.
pub(super) fn face_frame(bbox: [f32; 4], image: egui::Rect, margin: f32) -> Option<egui::Rect> {
    if bbox.iter().any(|v| !v.is_finite()) || bbox[2] <= bbox[0] || bbox[3] <= bbox[1] {
        return None;
    }
    let at = |x: f32, y: f32| {
        egui::pos2(
            image.left() + x * image.width(),
            image.top() + y * image.height(),
        )
    };
    let frame = egui::Rect::from_min_max(at(bbox[0], bbox[1]), at(bbox[2], bbox[3]))
        .expand(margin.max(0.0))
        .intersect(image);
    frame.is_positive().then_some(frame)
}

/// Rahmen um das Gesicht in der Mausvorschau: 2 Punkte Akzentfarbe mit je 1 Punkt dunklem
/// Strich innen und außen, ganz innerhalb von `frame`. Ohne Namen.
fn paint_hover_frame(painter: &egui::Painter, frame: egui::Rect) {
    painter.rect_stroke(
        frame,
        2.0,
        egui::Stroke::new(4.0_f32, HOVER_FRAME_SHADE),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(
        frame.shrink(1.0),
        1.0,
        egui::Stroke::new(2.0_f32, theme::ACCENT),
        egui::StrokeKind::Inside,
    );
}

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
    #[test]
    fn face_frame_scales_expands_and_clamps_to_the_image() {
        let image = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(400.0, 300.0));
        let exact = face_frame([0.25, 0.5, 0.5, 0.75], image, 0.0).expect("gültig");
        assert_eq!(
            exact,
            egui::Rect::from_min_max(egui::pos2(200.0, 200.0), egui::pos2(300.0, 275.0))
        );
        let margin = face_frame([0.25, 0.5, 0.5, 0.75], image, 4.0).expect("gültig");
        assert_eq!(
            margin,
            egui::Rect::from_min_max(egui::pos2(196.0, 196.0), egui::pos2(304.0, 279.0))
        );
        // Am Bildrand wird der Rand abgeschnitten, nie über das Foto hinaus.
        let edge = face_frame([0.0, 0.9, 0.1, 1.0], image, 4.0).expect("gültig");
        assert_eq!(edge.min.x, 100.0);
        assert_eq!(edge.max.y, 350.0);
        assert_eq!(edge.max.x, 144.0);
        // Ungültige oder leere Rahmen ergeben keinen Rahmen.
        assert!(face_frame([0.5, 0.5, 0.5, 0.6], image, 4.0).is_none());
        assert!(face_frame([f32::NAN, 0.1, 0.2, 0.3], image, 4.0).is_none());
        assert!(face_frame([1.2, 1.2, 1.5, 1.5], image, 4.0).is_none());
    }
    /// Der gespeicherte Rahmen ist auf das orientierte, auf 1600 Pixel verkleinerte Analysebild
    /// normiert; die Mausvorschau zeigt dasselbe orientierte Bild mit 960 Pixeln, eingepasst in
    /// die Vorschaufläche. Der umgerechnete Rahmen muss genau auf der Markierung liegen.
    #[test]
    fn face_frame_hits_the_face_in_a_rotated_smaller_preview() {
        use crate::core::previews::orient;
        use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
        // Rohbild quer (Kamera hochkant gehalten, EXIF-Ausrichtung 6), rote Markierung.
        let mut raw = RgbaImage::from_pixel(2400, 1800, Rgba([20, 20, 20, 255]));
        for x in 300..500 {
            for y in 1200..1400 {
                raw.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        let raw = DynamicImage::ImageRgba8(raw);
        let marked = |image: &DynamicImage| {
            let (w, h) = image.dimensions();
            let red: Vec<(u32, u32)> = (0..w)
                .flat_map(|x| (0..h).map(move |y| (x, y)))
                .filter(|&(x, y)| image.get_pixel(x, y)[0] > 200)
                .collect();
            let x0 = red.iter().map(|p| p.0).min().expect("Markierung") as f32;
            let x1 = red.iter().map(|p| p.0).max().expect("Markierung") as f32 + 1.0;
            let y0 = red.iter().map(|p| p.1).min().expect("Markierung") as f32;
            let y1 = red.iter().map(|p| p.1).max().expect("Markierung") as f32 + 1.0;
            (x0, y0, x1, y1, w as f32, h as f32)
        };
        let analysis = orient(raw.clone(), 6).thumbnail(1600, 1600);
        let (x0, y0, x1, y1, w, h) = marked(&analysis);
        assert!(h > w, "orientiert hochkant");
        let bbox = [x0 / w, y0 / h, x1 / w, y1 / h];
        let preview = orient(raw, 6).thumbnail(
            crate::config::HOVER_PREVIEW_EDGE,
            crate::config::HOVER_PREVIEW_EDGE,
        );
        let area = egui::Rect::from_min_size(egui::pos2(30.0, 40.0), egui::vec2(520.0, 390.0));
        let shown = crate::ui::thumbnails::fit(
            area,
            egui::vec2(preview.width() as f32, preview.height() as f32),
        );
        let frame = face_frame(bbox, shown, 0.0).expect("Rahmen");
        // Zurück in Pixel der Vorschau: deckt die Markierung bis auf Rundung ab.
        let scale = preview.width() as f32 / shown.width();
        let (px0, py0, px1, py1, _, _) = marked(&preview);
        let to_px = |p: egui::Pos2| ((p.x - shown.left()) * scale, (p.y - shown.top()) * scale);
        let (fx0, fy0) = to_px(frame.min);
        let (fx1, fy1) = to_px(frame.max);
        for (a, b) in [(fx0, px0), (fy0, py0), (fx1, px1), (fy1, py1)] {
            assert!(
                (a - b).abs() <= 2.0,
                "Rahmen {a} weicht von Markierung {b} ab"
            );
        }
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
    let columns = fit
        .clamp(1, crate::config::GROUP_SIZE_MAX)
        .min(faces.max(1));
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
