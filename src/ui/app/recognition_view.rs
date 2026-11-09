use super::*;

impl PhotoApp {
    pub(super) fn analysing_current_collection(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.kind == "face-analysis" && task.collection == self.settings.collection)
    }

    /// `compact`: nur Titelzeile (und laufender Auftrag) statt der drei Schritte; die
    /// Reiter Personen, Stammbaum und Graph brauchen die Höhe für ihre Inhalte (ADR 0063).
    pub(super) fn people_workflow(&mut self, ui: &mut egui::Ui, compact: bool) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Personen").size(23.0).strong());
            if ui.button("Sammlung wählen …").clicked() {
                self.pick(Pick::Collection);
            }
            if !self.settings.collection.is_empty()
                && ui.button("Automatik & Werkzeuge …").clicked()
            {
                self.recognition_tools_open = true;
            }
        });
        if self.settings.collection.is_empty() {
            return;
        }
        if compact {
            if self.analysing_current_collection()
                || self
                    .tasks
                    .iter()
                    .any(|t| t.is_face_matching() && t.collection == self.settings.collection)
            {
                self.recognition_progress(ui);
            }
            self.recognition_tools(ui.ctx());
            return;
        }
        let status = self.face_page.status.clone();
        let runtime = Path::new(&self.settings.onnx_runtime).is_file();
        let setup_needed = !runtime
            || !self.model_weights_present()
            || self.models_installing()
            || self.runtime_downloading();
        let matching = self
            .tasks
            .iter()
            .any(|task| task.is_face_matching() && task.collection == self.settings.collection);
        let analysing = self.analysing_current_collection();
        let can_match = !self.busy()
            && !matching
            && status.pending == 0
            && !status.embedder.is_empty()
            && status.confirmed > 0;
        ui.label(RichText::new(
            "Vortrainierte Modelle erkennen Gesichter. Deine Bestätigungen liefern Beispiele für Namensvorschläge.",
        ).small());
        ui.columns(3, |columns| {
            Self::recognition_step(&mut columns[0], "1  Fotoanalyse", |ui| {
                ui.label(if setup_needed {
                    "Einrichtung erforderlich".into()
                } else if analysing {
                    "Analyse läuft".into()
                } else {
                    format!(
                        "{} von {} Fotos mit aktueller Erkennung analysiert",
                        status.analysed_photos, status.present_photos
                    )
                });
                if setup_needed {
                    if ui.button("Einrichten …").clicked() {
                        self.recognition_tools_open = true;
                    }
                } else {
                    let label = if self.settings.work_portion == 0 {
                        "Alle Fotos analysieren".to_string()
                    } else {
                        format!("Nächste {} Fotos analysieren", self.settings.work_portion)
                    };
                    if ui.add_enabled(!self.busy(), egui::Button::new(label))
                        .on_hover_text("Beginnt mit den neuesten Fotos, erkennt Gesichter, berechnet Merkmale und bildet Gruppen. Bereits analysierte Fotos werden übersprungen und zählen nicht zur Portion.")
                        .clicked()
                    {
                        self.collection_action("analyze");
                    }
                    self.portion_controls(ui);
                }
            });
            Self::recognition_step(&mut columns[1], "2  Aus Namen lernen", |ui| {
                ui.label(format!("{} bestätigt", status.confirmed));
                ui.label(format!("{} Referenzen · {} Personen", status.references, status.reference_people));
                match &status.classifier {
                    Some(m) if m.validated_on > 0 => {
                        ui.label(RichText::new(format!(
                            "Modell: richtig auf Platz 1 {:.0} %, unter den ersten drei {:.0} % (Ähnlichkeit: {:.0} % / {:.0} %, {} Prüfbeispiele)",
                            m.top1 * 100.0, m.top3 * 100.0, m.baseline_top1 * 100.0, m.baseline_top3 * 100.0, m.validated_on
                        )).small());
                    }
                    Some(m) => {
                        ui.label(RichText::new(format!("Modell für {} Personen trainiert, noch zu wenig Prüfbeispiele", m.people)).small());
                    }
                    None => {
                        ui.label(RichText::new(if status.reference_people >= 2 && status.references >= 4 { "Modell noch nicht trainiert · „Neu abgleichen“ in Schritt 3 trainiert es" } else { "Noch kein Modell · mindestens zwei Personen mit je zwei Referenzen nötig" }).small().weak());
                    }
                }
                if status.dirty_people > 0 {
                    ui.label(RichText::new(format!("{} Personen offen", status.dirty_people)).small());
                } else {
                    ui.label(RichText::new("Automatisch nach Bestätigung").small().weak());
                }
            });
            Self::recognition_step(&mut columns[2], "3  Namen vorschlagen", |ui| {
                let last = chrono::DateTime::parse_from_rfc3339(&status.applied).ok()
                    .map(|date| date.with_timezone(&chrono::Local).format("%d.%m.%Y · %H:%M").to_string());
                let summary = if matching {
                    "Abgleich läuft".into()
                } else if status.pending > 0 {
                    if self.maintenance_paused { "Abgleich pausiert" } else { "Abgleich steht an" }.into()
                } else if let Some(last) = &last {
                    format!("Zuletzt am {last}")
                } else if status.confirmed == 0 {
                    "Zuerst Gesichter benennen".into()
                } else {
                    "Noch kein Abschluss erfasst".into()
                };
                ui.label(summary).on_hover_text(last.map_or_else(
                    || "Für das aktive Modell ist kein Abschluss protokolliert.".into(),
                    |time| format!("Letzter vollständiger Abgleich: {time}"),
                ));
                if status.pending > 0 && self.maintenance_paused && !matching {
                    if ui.add_enabled(!self.busy(), egui::Button::new("Fortsetzen")).clicked() {
                        self.collection_action("resume");
                    }
                } else if ui.add_enabled(can_match, egui::Button::new("Neu abgleichen"))
                    .on_hover_text("Wählt Referenzen aus bestätigten Namen und berechnet Vorschläge aus vorhandenen Merkmalen. Bestätigte Namen bleiben erhalten.")
                    .clicked()
                {
                    self.collection_action("knowledge");
                }
            });
        });
        ui.scope(|ui| {
            ui.set_min_height(ui.spacing().interact_size.y);
            self.recognition_progress(ui);
        });
        self.recognition_tools(ui.ctx());
    }

    fn recognition_step(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
        let width = ui.available_width();
        theme::card(ui).inner_margin(10).show(ui, |ui| {
            ui.set_width(width - 20.0);
            ui.set_min_height(94.0);
            ui.spacing_mut().item_spacing.y = 5.0;
            ui.label(RichText::new(title).strong());
            content(ui);
        });
    }

    fn recognition_progress(&mut self, ui: &mut egui::Ui) {
        let Some(running) = self.tasks.iter().find(|task| {
            task.collection == self.settings.collection
                && (task.is_face_matching() || task.kind == "face-analysis")
        }) else {
            let schedule = match (
                self.face_page.status.auto_import,
                self.face_page.status.nightly,
            ) {
                (true, true) => "Manuell, nach Import und um 03:00 Uhr",
                (true, false) => "Manuell und nach Import",
                (false, true) => "Manuell und um 03:00 Uhr",
                (false, false) => "Nur manuell",
            };
            ui.label(
                RichText::new(format!("Fotoanalyse · {schedule}"))
                    .small()
                    .weak(),
            );
            return;
        };
        let phase = running.task.control.face_phase();
        let done = running.task.control.done.load(Ordering::Relaxed);
        let total = running.task.control.total.load(Ordering::Relaxed);
        let stopping = running.task.control.cancel.load(Ordering::Relaxed);
        let can_pause = running.is_face_matching();
        let label = match (stopping, can_pause, phase) {
            (true, true, _) => "Abgleich wird angehalten …",
            (true, false, _) => "Analyse wird abgebrochen …",
            (_, _, Some(FacePhase::Analysis)) => "Fotos werden analysiert",
            (_, _, Some(FacePhase::References)) => "Geeignete Referenzen werden ausgewählt",
            (_, _, Some(FacePhase::Matching)) => "Namen werden vorgeschlagen",
            (_, _, Some(FacePhase::Groups)) => "Offene Gesichter werden gruppiert",
            _ => "Auftrag wird vorbereitet …",
        };
        ui.horizontal_wrapped(|ui| {
            ui.spinner();
            ui.label(label);
            if total > 0 && !stopping {
                ui.add(
                    egui::ProgressBar::new(done as f32 / total as f32)
                        .desired_width(132.0)
                        .text(format!("{done} / {total}")),
                );
            }
            if ui
                .add_enabled(
                    !stopping,
                    egui::Button::new(if can_pause { "Pausieren" } else { "Abbrechen" }),
                )
                .clicked()
            {
                if can_pause {
                    self.maintenance_paused = true;
                }
                running.task.control.stop();
            }
        });
    }

    fn recognition_tools(&mut self, ctx: &egui::Context) {
        if !self.recognition_tools_open {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("recognition-tools")).show(ctx, |ui| {
            ui.set_width(570.0_f32.min(ctx.content_rect().width() - 64.0));
            ui.heading("Erkennung einrichten");
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 180.0).clamp(160.0, 530.0)).show(ui, |ui| {
                ui.label(format!("Gewählte Modelle: {} · {}", self.detector, self.embedder));
                if self.runtime_downloading() || !Path::new(&self.settings.onnx_runtime).is_file() {
                    ui.label("Die ONNX Runtime führt die Modelle auf deinem Rechner aus.");
                    self.runtime_download_button(ui);
                }
                if self.models_installing() || !self.model_weights_present() {
                    self.model_install_button(ui);
                    if ui.add_enabled(!self.models_installing(), egui::Button::new("Modellordner wählen …")).clicked() {
                        self.pick(Pick::Models);
                    }
                }
                ui.separator();
                ui.label(RichText::new("Wann wird analysiert?").strong());
                let mut auto_import = self.face_page.status.auto_import;
                let mut nightly = self.face_page.status.nightly;
                let changed = ui.add_enabled(!self.busy(), egui::Checkbox::new(
                    &mut auto_import, "Nach einem Import in die gewählte Sammlung"
                )).changed() | ui.add_enabled(!self.busy(), egui::Checkbox::new(
                    &mut nightly, "Um 03:00 Uhr, solange die App läuft"
                )).changed();
                if changed {
                    let root = self.settings.collection.clone();
                    self.launch("Automatik speichern", true, "mutation", move |_| {
                        let mut collection = Collection::open(Path::new(&root), true)?;
                        faces::configure_automatic(&mut collection, auto_import, nightly)?;
                        Ok(Outcome::Done("Sammlungsautomatik gespeichert".into()))
                    });
                }
                if ui.checkbox(&mut self.settings.auto_metadata, "Bei automatischen Läufen zuerst Metadaten lesen").changed() {
                    self.save();
                }
                ui.label(RichText::new("Bestätigungen aktualisieren geeignete Referenzen und Vorschläge automatisch im Hintergrund. Vorschläge werden erst durch deine Bestätigung zu Beispielen.").small());
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    for (action, label) in [("export", "Referenzen exportieren"), ("calibrate", "Validierung auswerten")] {
                        if ui.add_enabled(!self.busy(), egui::Button::new(label)).clicked() {
                            self.recognition_tools_open = false;
                            self.collection_action(action);
                        }
                    }
                });
                egui::CollapsingHeader::new("Biometrie entfernen").show(ui, |ui| {
                    ui.label("Entfernt Gesichter, Personen und bekannte Biometrie-Sicherungen. Fotos und Inventar bleiben erhalten.");
                    ui.checkbox(
                        &mut self.central_ui.wipe_references,
                        "Auch zentral gespeicherte Referenzbilder löschen (Personen im zentralen Bestand bleiben)",
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.delete_phrase).hint_text("BIOMETRIE eingeben"));
                        if ui.add_enabled(!self.busy() && self.delete_phrase == "BIOMETRIE", egui::Button::new("Dauerhaft entfernen")).clicked() {
                            self.undo.clear();
                            self.delete_phrase.clear();
                            self.recognition_tools_open = false;
                            self.collection_action("delete_biometrics");
                        }
                    });
                });
            });
            if ui.button("Schließen").clicked() {
                self.recognition_tools_open = false;
            }
        });
        if response.should_close() {
            self.recognition_tools_open = false;
        }
    }

    pub(super) fn undo_face_button(&mut self, ui: &mut egui::Ui) {
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
                    let mut collection = Collection::open(Path::new(&root), true)?;
                    faces::undo(&mut collection, &undo)?;
                    Ok(Outcome::Done("Gruppenentscheidung zurückgenommen".into()))
                },
            );
        }
    }
}

impl PhotoApp {
    /// Portionsgröße und Ordnerbezug für Analyse und Metadaten.
    fn portion_controls(&mut self, ui: &mut egui::Ui) {
        let folder = self.applied.folder.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Portion").small().weak());
            egui::ComboBox::from_id_salt("work-portion")
                .selected_text(match self.settings.work_portion {
                    0 => "Alle".to_string(),
                    n => format!("{n} Fotos"),
                })
                .show_ui(ui, |ui| {
                    for n in [100usize, 500, 2000, 10000, 0] {
                        let text = if n == 0 {
                            "Alle".to_string()
                        } else {
                            format!("{n} Fotos")
                        };
                        if ui
                            .selectable_value(&mut self.settings.work_portion, n, text)
                            .changed()
                        {
                            let _ = self.settings.save();
                        }
                    }
                });
        });
        let has_folder = folder.is_some();
        let text = match &folder {
            Some(folder) => format!("Nur Ordner {folder}"),
            None => "Nur gewählter Ordner (in der Sammlung wählen)".to_string(),
        };
        if ui
            .add_enabled(
                has_folder,
                egui::Checkbox::new(&mut self.settings.work_in_folder, text),
            )
            .changed()
        {
            let _ = self.settings.save();
        }
    }
}
