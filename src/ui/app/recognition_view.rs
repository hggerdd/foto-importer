use super::*;

impl PhotoApp {
    pub(super) fn analysing_current_collection(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.kind == "face-analysis" && task.collection == self.settings.collection)
    }

    /// Ist das ein Analyse- oder Vorschlagsauftrag der aktiven Sammlung?
    fn is_face_task(&self, task: &Running) -> bool {
        task.collection == self.settings.collection
            && (task.is_face_matching() || task.kind == "face-analysis")
    }

    /// Fehlt noch etwas, bevor Fotos analysiert werden können (Laufzeit oder Modelle)?
    fn setup_needed(&self) -> bool {
        !Path::new(&self.settings.onnx_runtime).is_file()
            || !self.model_weights_present()
            || self.models_installing()
            || self.runtime_downloading()
    }

    fn face_stand(&self) -> FaceStand {
        let status = &self.face_page.status;
        FaceStand {
            setup_needed: self.setup_needed(),
            running: self
                .tasks
                .iter()
                .find(|task| self.is_face_task(task))
                .map(|task| FaceRun {
                    stopping: task.task.control.cancel.load(Ordering::Relaxed),
                    matching: task.is_face_matching(),
                    phase: task.task.control.face_phase(),
                }),
            analysed: status.analysed_photos,
            present: status.present_photos,
            pending: status.pending > 0,
            paused: self.maintenance_paused,
        }
    }

    /// Kopf der Ansicht „Gesichter“: Titel, dann eine Statuszeile mit höchstens einer
    /// Hauptaktion und dem Einstieg in „Automatik & Werkzeuge“.
    pub(super) fn faces_header(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Gesichter").size(23.0).strong());
        if self.settings.collection.is_empty() {
            return;
        }
        let stand = self.face_stand();
        let (text, action) = status_line(&stand);
        // Einen laufenden Gesichtsauftrag zeigt die Fußleiste mit Fortschritt und Abbruch.
        // Nur wenn sie einen anderen Auftrag zeigt, steht der Fortschritt hier.
        let in_footer =
            Self::footer_task(self.view, &self.tasks).is_some_and(|task| self.is_face_task(task));
        ui.horizontal_wrapped(|ui| {
            if stand.running.is_some() && !in_footer {
                self.recognition_progress(ui);
            } else {
                ui.label(text);
            }
            if let Some(action) = action {
                let enabled = action == MainAction::Setup || !self.busy();
                let button = egui::Button::new(
                    RichText::new(action.label())
                        .color(Color32::from_rgb(9, 35, 34))
                        .strong(),
                )
                .fill(theme::ACCENT);
                let hint = match action {
                    MainAction::Setup => {
                        "Laufzeit und Modelle für die Erkennung auf diesem Rechner einrichten"
                            .to_string()
                    }
                    MainAction::Analyze | MainAction::Continue => format!(
                        "Analysiert {}, die neuesten zuerst. Schon analysierte Fotos werden übersprungen. Die Menge änderst du unter „Automatik & Werkzeuge“.",
                        match self.settings.work_portion {
                            0 => "alle übrigen Fotos".to_string(),
                            n => format!("die nächsten {n} Fotos"),
                        }
                    ),
                    MainAction::Resume => {
                        "Setzt das Vorschlagen von Namen im Hintergrund fort".to_string()
                    }
                };
                let response = ui.add_enabled(enabled, button).on_hover_text(hint);
                let response = if enabled {
                    response
                } else {
                    response.on_disabled_hover_text(self.busy_reason())
                };
                if response.clicked() {
                    match action {
                        MainAction::Setup => self.recognition_tools_open = true,
                        MainAction::Analyze | MainAction::Continue => {
                            self.collection_action("analyze")
                        }
                        MainAction::Resume => self.collection_action("resume"),
                    }
                }
            }
            if ui.button("Automatik & Werkzeuge …").clicked() {
                self.recognition_tools_open = true;
            }
        });
        self.recognition_tools(ui.ctx());
    }

    /// Die drei Schritte der Erkennung mit Stand und ihren Aktionen, untereinander im Dialog
    /// „Automatik & Werkzeuge“.
    fn recognition_steps(&mut self, ui: &mut egui::Ui) {
        let status = self.face_page.status.clone();
        let setup_needed = self.setup_needed();
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
        Self::recognition_step(ui, "Fotoanalyse", |ui| {
            ui.label(if setup_needed {
                "Einrichtung erforderlich (siehe unten)".into()
            } else if analysing {
                "Analyse läuft".into()
            } else {
                format!(
                    "{} von {} Fotos mit aktueller Erkennung analysiert",
                    status.analysed_photos, status.present_photos
                )
            });
            if !setup_needed {
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
            let schedule = match (status.auto_import, status.nightly) {
                (true, true) => "Manuell, nach Import und um 03:00 Uhr",
                (true, false) => "Manuell und nach Import",
                (false, true) => "Manuell und um 03:00 Uhr",
                (false, false) => "Nur manuell",
            };
            ui.label(RichText::new(schedule).small().weak());
        });
        Self::recognition_step(ui, "Aus Namen lernen", |ui| {
            ui.label(format!(
                "{} bestätigt · {} Referenzen · {} Personen",
                status.confirmed, status.references, status.reference_people
            ));
            if status.dirty_people > 0 {
                ui.label(RichText::new(format!("{} Personen offen", status.dirty_people)).small());
            } else {
                ui.label(RichText::new("Automatisch nach Bestätigung").small().weak());
            }
        });
        Self::recognition_step(ui, "Namen vorschlagen", |ui| {
            let last = chrono::DateTime::parse_from_rfc3339(&status.applied)
                .ok()
                .map(|date| {
                    date.with_timezone(&chrono::Local)
                        .format("%d.%m.%Y · %H:%M")
                        .to_string()
                });
            let summary = if matching {
                "Abgleich läuft".into()
            } else if status.pending > 0 {
                if self.maintenance_paused {
                    "Abgleich pausiert"
                } else {
                    "Abgleich steht an"
                }
                .into()
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
    }

    fn recognition_step(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
        let width = ui.available_width();
        theme::card(ui).inner_margin(10).show(ui, |ui| {
            ui.set_width(width - 20.0);
            ui.spacing_mut().item_spacing.y = 5.0;
            ui.label(RichText::new(title).strong());
            content(ui);
        });
    }

    /// Fortschritt eines laufenden Gesichtsauftrags mit Abbruch. Erscheint nur, wenn die
    /// Fußleiste gerade einen anderen Auftrag zeigt.
    fn recognition_progress(&mut self, ui: &mut egui::Ui) {
        let Some(running) = self.tasks.iter().find(|task| {
            task.collection == self.settings.collection
                && (task.is_face_matching() || task.kind == "face-analysis")
        }) else {
            return;
        };
        let done = running.task.control.done.load(Ordering::Relaxed);
        let total = running.task.control.total.load(Ordering::Relaxed);
        let stopping = running.task.control.cancel.load(Ordering::Relaxed);
        let can_pause = running.is_face_matching();
        let label = run_text(FaceRun {
            stopping,
            matching: can_pause,
            phase: running.task.control.face_phase(),
        });
        ui.spinner();
        ui.label(label);
        if total > 0 && !stopping {
            ui.add(
                egui::ProgressBar::new(done as f32 / total as f32)
                    .desired_width(132.0)
                    .text(format!("{done} / {total}")),
            );
            let timing = running.task.control.snapshot().timing();
            if !timing.is_empty() {
                ui.label(RichText::new(timing).small().weak());
            }
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
    }

    fn recognition_tools(&mut self, ctx: &egui::Context) {
        if !self.recognition_tools_open {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("recognition-tools")).show(ctx, |ui| {
            ui.set_width(570.0_f32.min(ctx.content_rect().width() - 64.0));
            ui.heading("Automatik & Werkzeuge");
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 180.0).clamp(160.0, 530.0)).show(ui, |ui| {
                self.recognition_steps(ui);
                ui.separator();
                ui.label(RichText::new("Erkennung einrichten").strong());
                ui.label(format!("Gewählte Modelle: {} · {}", self.detector, self.embedder));
                if !self.runtime_ready() {
                    ui.label("Die Laufzeit führt die Modelle auf deinem Rechner aus.");
                }
                self.face_setup_button(ui);
                if (self.models_installing() || !self.model_weights_present())
                    && ui.add_enabled(!self.models_installing(), egui::Button::new("Modellordner wählen …")).clicked()
                {
                    self.pick(Pick::Models);
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

    fn can_undo_face(&self) -> bool {
        !self.busy() && !self.undo.is_empty()
    }

    /// Nimmt die letzte Gruppenentscheidung zurück (Knopf und Strg+Z).
    pub(super) fn undo_last_face_decision(&mut self) {
        if !self.can_undo_face() {
            return;
        }
        let Some(undo) = self.undo.last().cloned() else {
            return;
        };
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

    pub(super) fn undo_face_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(self.can_undo_face(), egui::Button::new("Rückgängig"))
            .on_hover_text("Strg+Z")
            .clicked()
        {
            self.undo_last_face_decision();
        }
    }

    /// Strg+Z, aber nur ohne Textfeldfokus und wenn der Knopf aktiv wäre.
    pub(super) fn undo_shortcut(&mut self, ctx: &egui::Context) {
        if !self.can_undo_face() || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            self.undo_last_face_decision();
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

/// Laufender Analyse- oder Vorschlagsauftrag, soweit ihn die Statuszeile braucht.
#[derive(Clone, Copy)]
pub(super) struct FaceRun {
    /// Abbruch oder Pause ist angefordert.
    pub(super) stopping: bool,
    /// Der Auftrag schlägt Namen vor (pausierbar), statt Fotos zu analysieren.
    pub(super) matching: bool,
    pub(super) phase: Option<FacePhase>,
}

/// Stand der Gesichtserkennung einer Sammlung für die Statuszeile der Ansicht „Gesichter“.
#[derive(Clone, Copy, Default)]
pub(super) struct FaceStand {
    /// Laufzeit oder Modelle fehlen noch.
    pub(super) setup_needed: bool,
    pub(super) running: Option<FaceRun>,
    pub(super) analysed: usize,
    pub(super) present: usize,
    /// Für bestätigte Namen stehen noch Vorschläge aus.
    pub(super) pending: bool,
    /// Das Vorschlagen im Hintergrund ist angehalten.
    pub(super) paused: bool,
}

/// Der nächste sinnvolle Schritt in der Ansicht „Gesichter“.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum MainAction {
    Setup,
    Analyze,
    Continue,
    Resume,
}

impl MainAction {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Setup => "Einrichten …",
            Self::Analyze => "Fotos analysieren",
            Self::Continue => "Weiter analysieren",
            Self::Resume => "Fortsetzen",
        }
    }
}

/// Was ein laufender Gesichtsauftrag gerade tut, als Satz.
fn run_text(run: FaceRun) -> &'static str {
    match (run.stopping, run.matching, run.phase) {
        (true, true, _) => "Das Vorschlagen von Namen wird angehalten …",
        (true, false, _) => "Die Analyse wird abgebrochen …",
        (_, _, Some(FacePhase::Analysis)) => "Fotos werden analysiert …",
        (_, _, Some(FacePhase::References)) => "Beispiele für die Vorschläge werden ausgewählt …",
        (_, _, Some(FacePhase::Matching)) => "Namen werden vorgeschlagen …",
        (_, _, Some(FacePhase::Groups)) => "Unbenannte Gesichter werden gruppiert …",
        (_, _, None) => "Der Auftrag wird vorbereitet …",
    }
}

/// Statuszeile der Ansicht „Gesichter“: wo die Erkennung steht und welcher eine Schritt als
/// Nächstes sinnvoll ist. Ohne Schritt gibt es keinen Hauptknopf.
pub(super) fn status_line(stand: &FaceStand) -> (String, Option<MainAction>) {
    use crate::core::progress::group;
    if let Some(run) = stand.running {
        return (run_text(run).into(), None);
    }
    if stand.setup_needed {
        return (
            "Die Gesichtserkennung ist noch nicht eingerichtet.".into(),
            Some(MainAction::Setup),
        );
    }
    let all = group(stand.present);
    if stand.present == 0 {
        ("Noch keine Fotos in der Sammlung".into(), None)
    } else if stand.analysed == 0 {
        (
            format!("0 von {all} Fotos analysiert"),
            Some(MainAction::Analyze),
        )
    } else if stand.analysed < stand.present {
        (
            format!("{} von {all} Fotos analysiert", group(stand.analysed)),
            Some(MainAction::Continue),
        )
    } else if stand.pending && stand.paused {
        (
            "Alle Fotos analysiert. Das Vorschlagen von Namen ist angehalten.".into(),
            Some(MainAction::Resume),
        )
    } else if stand.pending {
        (
            "Alle Fotos analysiert. Namen werden gleich vorgeschlagen …".into(),
            None,
        )
    } else {
        ("Alles auf dem Stand".into(), None)
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;
    fn stand(analysed: usize, present: usize) -> FaceStand {
        FaceStand {
            analysed,
            present,
            ..Default::default()
        }
    }
    #[test]
    fn nothing_analysed_offers_to_start() {
        assert_eq!(
            status_line(&stand(0, 3000)),
            (
                "0 von 3.000 Fotos analysiert".to_string(),
                Some(MainAction::Analyze)
            )
        );
        assert_eq!(MainAction::Analyze.label(), "Fotos analysieren");
    }
    #[test]
    fn partly_analysed_offers_to_continue() {
        assert_eq!(
            status_line(&stand(1240, 3000)),
            (
                "1.240 von 3.000 Fotos analysiert".to_string(),
                Some(MainAction::Continue)
            )
        );
        assert_eq!(MainAction::Continue.label(), "Weiter analysieren");
    }
    #[test]
    fn a_running_task_is_named_and_has_no_main_action() {
        let run = |matching, phase| FaceStand {
            running: Some(FaceRun {
                stopping: false,
                matching,
                phase,
            }),
            // Auch ohne fertige Einrichtung zählt, was gerade läuft.
            setup_needed: true,
            ..stand(1240, 3000)
        };
        assert_eq!(
            status_line(&run(false, Some(FacePhase::Analysis))),
            ("Fotos werden analysiert …".to_string(), None)
        );
        assert_eq!(
            status_line(&run(true, Some(FacePhase::Matching))),
            ("Namen werden vorgeschlagen …".to_string(), None)
        );
        assert_eq!(status_line(&run(false, None)).1, None);
    }
    #[test]
    fn everything_done_needs_no_action() {
        assert_eq!(
            status_line(&stand(3000, 3000)),
            ("Alles auf dem Stand".to_string(), None)
        );
    }
    #[test]
    fn setup_and_a_paused_matching_are_the_next_step_when_they_block() {
        let missing = FaceStand {
            setup_needed: true,
            ..stand(0, 3000)
        };
        assert_eq!(status_line(&missing).1, Some(MainAction::Setup));
        let waiting = FaceStand {
            pending: true,
            ..stand(3000, 3000)
        };
        assert_eq!(status_line(&waiting).1, None);
        let paused = FaceStand {
            paused: true,
            ..waiting
        };
        assert_eq!(status_line(&paused).1, Some(MainAction::Resume));
        // Solange Fotos fehlen, bleibt die Analyse der nächste Schritt.
        let partly = FaceStand {
            analysed: 10,
            ..paused
        };
        assert_eq!(status_line(&partly).1, Some(MainAction::Continue));
        assert_eq!(status_line(&stand(0, 0)).1, None);
    }
}
