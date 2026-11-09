//! Oberfläche für den zentralen Personenbestand (ADR 0062): Einstellung, Abgleich,
//! Rückfragen und Übernahme fehlender Personen. Alle Arbeit läuft auf Workern.
use super::*;
use crate::core::central::{self, Central, Conflict, Side, SyncNote, SyncReport};

/// Zustand der zentralen Ansicht.
#[derive(Default)]
pub(super) struct CentralUi {
    /// Letzter Abgleichsbericht dieser Sammlung.
    pub(super) report: Option<SyncReport>,
    pub(super) warning: Option<String>,
    /// Gewählte Seite je (lokale Person, Feld) bei Rückfragen.
    picks: BTreeMap<(i64, String), Side>,
    /// Beim Biometrie-Entfernen auch zentrale Referenzbilder löschen.
    pub(super) wipe_references: bool,
}

impl CentralUi {
    /// Übernimmt das Ergebnis eines Abgleichs (auch den Fallback-Hinweis).
    pub(super) fn accept(&mut self, note: SyncNote) {
        if let Some(report) = note.report {
            self.report = Some(report);
        }
        self.warning = note.warning;
    }
}

impl PhotoApp {
    /// Karte in den Einstellungen: Ort, Aktivierung und Opt-in für Referenzbilder.
    pub(super) fn central_settings_card(&mut self, ui: &mut egui::Ui) {
        theme::card(ui).show(ui, |ui| {
            ui.label(RichText::new("Zentraler Personenbestand").size(20.0).strong());
            ui.label("Personen (Name, Geschlecht, Geburtsjahr, Notiz) werden zwischen allen Sammlungen dieses Rechners abgeglichen.");
            ui.checkbox(&mut self.settings.central_enabled, "Zentralen Bestand verwenden");
            ui.label("Ort der Zentraldatei (leer = neben den Einstellungen; ein Netzlaufwerk ist möglich)");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.central_path)
                    .hint_text(crate::config::Settings::default_central_path().to_string_lossy())
                    .desired_width(570.0),
            );
            ui.checkbox(
                &mut self.settings.central_images,
                "Referenzbilder (Gesichtsausschnitte) zentral speichern",
            );
            ui.label(
                RichText::new("Standard aus. Gesichtsausschnitte sind biometrische Daten und verlassen die Sammlung nur mit dieser Zustimmung. Originalfotos werden nie kopiert. „Biometrie entfernen“ kann sie auch zentral löschen.")
                    .small()
                    .weak(),
            );
        });
        ui.add_space(16.0);
    }

    /// Abschnitt im Personenbereich: Abgleich, Rückfragen, fehlende Personen.
    pub(super) fn central_panel(&mut self, ui: &mut egui::Ui) {
        if !self.settings.central_enabled || self.settings.collection.is_empty() {
            return;
        }
        let busy = self.busy();
        egui::CollapsingHeader::new("Zentraler Personenbestand")
            .id_salt("central-panel")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(!busy, egui::Button::new("Mit zentralem Personenbestand abgleichen"))
                        .on_hover_text("Ergänzt leere Felder in beide Richtungen und fragt bei Widersprüchen nach")
                        .clicked()
                    {
                        self.run_central(central::sync);
                    }
                    if let Some(report) = &self.central_ui.report {
                        ui.label(RichText::new(report.summary()).small().weak());
                    }
                });
                if let Some(warning) = &self.central_ui.warning {
                    ui.colored_label(ui.visuals().warn_fg_color, warning);
                }
                self.central_conflicts(ui);
                self.central_missing(ui);
            });
    }

    fn central_conflicts(&mut self, ui: &mut egui::Ui) {
        let conflicts: Vec<Conflict> = self
            .central_ui
            .report
            .as_ref()
            .map(|r| r.conflicts.clone())
            .unwrap_or_default();
        if conflicts.is_empty() {
            return;
        }
        ui.label(RichText::new(format!("{} Rückfrage(n)", conflicts.len())).strong());
        for conflict in conflicts {
            ui.group(|ui| {
                ui.label(if conflict.linked {
                    format!("{}: lokal und zentral unterscheiden sich", conflict.name)
                } else {
                    format!(
                        "{}: gleicher Name, aber andere Angaben. Dieselbe Person?",
                        conflict.name
                    )
                });
                for diff in &conflict.diffs {
                    let key = (conflict.local_id, diff.field.to_string());
                    let mut side = self.central_ui.picks.get(&key).copied();
                    ui.horizontal(|ui| {
                        ui.label(field_label(diff.field));
                        ui.radio_value(
                            &mut side,
                            Some(Side::Local),
                            format!("lokal: {}", diff.local),
                        );
                        ui.radio_value(
                            &mut side,
                            Some(Side::Central),
                            format!("zentral: {}", diff.central),
                        );
                    });
                    if let Some(side) = side {
                        self.central_ui.picks.insert(key, side);
                    }
                }
                let all_picked = conflict.diffs.iter().all(|d| {
                    self.central_ui
                        .picks
                        .contains_key(&(conflict.local_id, d.field.to_string()))
                });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.busy() && all_picked,
                            egui::Button::new("Dieselbe Person, so übernehmen"),
                        )
                        .clicked()
                    {
                        let picks: BTreeMap<String, Side> = conflict
                            .diffs
                            .iter()
                            .filter_map(|d| {
                                self.central_ui
                                    .picks
                                    .get(&(conflict.local_id, d.field.to_string()))
                                    .map(|s| (d.field.to_string(), *s))
                            })
                            .collect();
                        let (id, gid) = (conflict.local_id, conflict.gid.clone());
                        self.run_central(move |c, central| {
                            central::resolve(c, central, id, &gid, true, &picks)
                        });
                    }
                    if !conflict.linked
                        && ui
                            .add_enabled(!self.busy(), egui::Button::new("Andere Person"))
                            .on_hover_text("Bleibt getrennt und wird nicht erneut gefragt")
                            .clicked()
                    {
                        let (id, gid) = (conflict.local_id, conflict.gid.clone());
                        self.run_central(move |c, central| {
                            central::resolve(c, central, id, &gid, false, &BTreeMap::new())
                        });
                    }
                });
            });
        }
    }

    fn central_missing(&mut self, ui: &mut egui::Ui) {
        let missing = self
            .central_ui
            .report
            .as_ref()
            .map(|r| r.missing.clone())
            .unwrap_or_default();
        if missing.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} zentrale Person(en) fehlen in dieser Sammlung",
                    missing.len()
                ))
                .strong(),
            );
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Alle übernehmen"))
                .clicked()
            {
                let gids = missing.iter().map(|p| p.gid.clone()).collect::<Vec<_>>();
                self.adopt_central(gids);
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("central-missing")
            .max_height(180.0)
            .show(ui, |ui| {
                for person in &missing {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "{}{}",
                            person.name,
                            if person.birth_year.is_empty() {
                                String::new()
                            } else {
                                format!(" ({})", person.birth_year)
                            }
                        ));
                        if ui
                            .add_enabled(
                                !self.busy(),
                                egui::Button::new("Aus zentralem Bestand übernehmen").small(),
                            )
                            .clicked()
                        {
                            self.adopt_central(vec![person.gid.clone()]);
                        }
                    });
                }
            });
    }

    fn adopt_central(&mut self, gids: Vec<String>) {
        self.run_central(move |c, central| {
            central::adopt(c, central, &gids)?;
            central::sync(c, central)
        });
    }

    /// Startet eine zentrale Operation auf dem Worker; Fehler werden angezeigt.
    pub(super) fn run_central(
        &mut self,
        work: impl FnOnce(&mut Collection, &mut Central) -> Result<SyncReport> + Send + 'static,
    ) {
        let root = self.settings.collection.clone();
        let config = self.settings.central();
        self.launch(
            "Zentralen Bestand abgleichen",
            true,
            "mutation",
            move |_| {
                let mut c = Collection::open(Path::new(&root), true)?;
                let mut central = Central::open_config(&config)?;
                let report = work(&mut c, &mut central)?;
                Ok(Outcome::Synced(
                    Box::new(Outcome::Done(report.summary())),
                    SyncNote {
                        warning: None,
                        report: Some(report),
                    },
                ))
            },
        );
    }
}

fn field_label(field: &str) -> &'static str {
    match field {
        "name" => "Name",
        "gender" => "Geschlecht",
        "birth_year" => "Geburtsjahr",
        _ => "Notiz",
    }
}
