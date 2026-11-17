//! Warteschlange „Sicher importieren“: jede Festlegung wird ein Auftrag, der genau einmal
//! und strikt nacheinander läuft (ADR Import).
use super::*;

/// Zustand eines eingeplanten Imports.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum BatchState {
    Queued,
    Running,
    Done,
    /// Beendet, aber einzelne Dateien oder die Indexaktualisierung schlugen fehl.
    Partial,
    Failed,
    Cancelled,
}

/// Ein festgelegter Import: Tage, Dateien und Einstellungen zum Zeitpunkt der Festlegung.
pub(super) struct ImportBatch {
    pub(super) id: u64,
    pub(super) source: String,
    pub(super) days: BTreeSet<String>,
    pub(super) files: Vec<SourceFile>,
    pub(super) target: PathBuf,
    pub(super) name: String,
    pub(super) converter: Option<PathBuf>,
    pub(super) skip: bool,
    pub(super) exiftool: String,
    pub(super) state: BatchState,
}

impl ImportBatch {
    /// Titel in Aufgaben und Fußleiste: Ordnername und ob DNGs entstehen. Die Phase
    /// („Kopieren“, „DNG erstellen“) steht daneben mit eigenem Zähler.
    pub(super) fn label(&self) -> String {
        let dng = if self.converter.is_some() {
            " · mit DNG"
        } else {
            ""
        };
        format!("Import „{}“{dng}", self.name)
    }
    /// Voller Zielordner, z. B. `D:\Fotos\2026-11 Herbst`.
    pub(super) fn destination(&self) -> String {
        import_view::display_path(&self.target.join(&self.name).to_string_lossy()).into_owned()
    }
    /// „Tage: 2026-11-01, 2026-11-02“.
    pub(super) fn days_text(&self) -> String {
        format!(
            "{}: {}",
            if self.days.len() == 1 { "Tag" } else { "Tage" },
            self.days.iter().cloned().collect::<Vec<_>>().join(", ")
        )
    }
    /// „3 Tage · 120 Dateien“.
    pub(super) fn summary(&self) -> String {
        format!(
            "{} · {}",
            plural(self.days.len(), "Tag", "Tage"),
            plural(self.files.len(), "Datei", "Dateien")
        )
    }
}

/// Markierung eines Tages im Tagesstreifen, abgeleitet aus dem letzten Import dieses Tages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum DayMark {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl DayMark {
    pub(super) fn text(self) -> &'static str {
        match self {
            DayMark::Queued => "⏳ eingeplant",
            DayMark::Running => "⏳ wird importiert",
            DayMark::Done => "✔ importiert",
            DayMark::Failed => "⚠ Fehler",
            DayMark::Cancelled => "abgebrochen",
        }
    }
    /// Ausgegraut: eingeplant, laufend oder fertig.
    pub(super) fn greyed(self) -> bool {
        matches!(self, DayMark::Queued | DayMark::Running | DayMark::Done)
    }
    /// Eingeplante und laufende Tage lassen sich nicht erneut wählen.
    pub(super) fn locked(self) -> bool {
        matches!(self, DayMark::Queued | DayMark::Running)
    }
}

impl PhotoApp {
    /// Markierungen der Tage der aktuellen Quelle; ein späterer Import eines Tages gilt.
    pub(super) fn day_marks(&self) -> BTreeMap<String, DayMark> {
        let mut marks = BTreeMap::new();
        for batch in self
            .imports
            .iter()
            .filter(|b| b.source == self.settings.source)
        {
            let mark = match batch.state {
                BatchState::Queued => DayMark::Queued,
                BatchState::Running => DayMark::Running,
                BatchState::Done => DayMark::Done,
                BatchState::Partial | BatchState::Failed => DayMark::Failed,
                BatchState::Cancelled => DayMark::Cancelled,
            };
            for day in &batch.days {
                marks.insert(day.clone(), mark);
            }
        }
        marks
    }

    pub(super) fn day_locked(&self, day: &str) -> bool {
        self.day_marks().get(day).is_some_and(|m| m.locked())
    }

    /// Wartende und laufende Imports.
    pub(super) fn imports_pending(&self) -> usize {
        self.imports
            .iter()
            .filter(|b| matches!(b.state, BatchState::Queued | BatchState::Running))
            .count()
    }

    /// Legt die ausgewählten Tage als Import fest. Die Auswahl, der angezeigte Tag und der
    /// Ordnername werden sofort geleert, damit gleich die nächsten Tage folgen können.
    pub(super) fn queue_import(&mut self) {
        let days: BTreeSet<String> = self
            .selected
            .iter()
            .filter(|day| !self.day_locked(day))
            .cloned()
            .collect();
        let files: Vec<SourceFile> = self
            .scan
            .files
            .iter()
            .filter(|f| days.contains(&f.day))
            .cloned()
            .collect();
        if files.is_empty()
            || self.settings.target.is_empty()
            || files::validate_folder(&self.folder_name).is_err()
        {
            return;
        }
        self.import_serial += 1;
        let batch = ImportBatch {
            id: self.import_serial,
            source: self.settings.source.clone(),
            days,
            files,
            target: PathBuf::from(&self.settings.target),
            name: std::mem::take(&mut self.folder_name),
            converter: self
                .settings
                .dng
                .then(|| PathBuf::from(&self.settings.converter)),
            skip: self.settings.skip_imported,
            exiftool: self.settings.exiftool.clone(),
            state: BatchState::Queued,
        };
        let waiting = self.imports_pending() > 0;
        let text = format!(
            "{} für „{}“ eingeplant{}",
            plural(batch.days.len(), "Tag", "Tage"),
            batch.name,
            if waiting {
                " · startet nach den vorherigen Imports"
            } else {
                ""
            }
        );
        self.imports.push(batch);
        self.selected.clear();
        self.focused.clear();
        self.save();
        self.message(text, false);
        self.start_next_import();
    }

    /// Startet den ältesten wartenden Import, wenn keiner läuft. Nie zwei gleichzeitig.
    pub(super) fn start_next_import(&mut self) {
        if self.closing
            || self.imports.iter().any(|b| b.state == BatchState::Running)
            || self.tasks.iter().any(|t| t.kind == "copy")
        {
            return;
        }
        let Some(batch) = self
            .imports
            .iter_mut()
            .find(|b| b.state == BatchState::Queued)
        else {
            return;
        };
        batch.state = BatchState::Running;
        let label = batch.label();
        let files = batch.files.clone();
        let target = batch.target.clone();
        let name = batch.name.clone();
        let converter = batch.converter.clone();
        let skip = batch.skip;
        let exiftool = batch.exiftool.clone();
        self.launch(&label, true, "copy", move |control| {
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

    /// Hält das Ergebnis des laufenden Imports fest. Liefert die Überschrift für Hinweis und
    /// Verlauf: Auftrag, Zielordner und Tage.
    pub(super) fn finish_import(
        &mut self,
        result: &Result<Outcome>,
        cancelled: bool,
    ) -> Option<String> {
        let batch = self
            .imports
            .iter_mut()
            .find(|b| b.state == BatchState::Running)?;
        batch.state = match result {
            Ok(Outcome::Imported(report)) if report.cancelled => BatchState::Cancelled,
            Ok(Outcome::Imported(report)) if !report.errors.is_empty() => BatchState::Partial,
            Ok(_) => BatchState::Done,
            Err(_) if cancelled => BatchState::Cancelled,
            Err(_) => BatchState::Failed,
        };
        Some(format!(
            "{} nach {} ({})",
            batch.label(),
            batch.destination(),
            batch.days_text()
        ))
    }

    /// Nimmt einen noch wartenden Import aus der Warteschlange; seine Tage sind wieder frei.
    pub(super) fn remove_queued_import(&mut self, id: u64) {
        if let Some(index) = self
            .imports
            .iter()
            .position(|b| b.id == id && b.state == BatchState::Queued)
        {
            let batch = self.imports.remove(index);
            self.message(
                format!("{} aus der Warteschlange genommen", batch.label()),
                false,
            );
        }
    }

    /// Liste der Imports dieser Sitzung in „Aufgaben“; der laufende steht zusätzlich als
    /// Karte mit Fortschritt darüber.
    pub(super) fn import_queue_list(&mut self, ui: &mut egui::Ui) {
        if self.imports.is_empty() {
            return;
        }
        let mut remove = None;
        theme::card(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width().min(640.0));
            ui.label(RichText::new("Import-Warteschlange").strong());
            for batch in &self.imports {
                ui.horizontal(|ui| {
                    let (state, color) = match batch.state {
                        BatchState::Queued => ("wartet", None),
                        BatchState::Running => ("läuft", Some(theme::ACCENT)),
                        BatchState::Done => ("✔ importiert", None),
                        BatchState::Partial => ("⚠ mit Fehlern", Some(theme::WARM)),
                        BatchState::Failed => ("⚠ fehlgeschlagen", Some(theme::WARM)),
                        BatchState::Cancelled => ("abgebrochen", None),
                    };
                    let mut text = RichText::new(state);
                    if let Some(color) = color {
                        text = text.color(color);
                    }
                    ui.label(text);
                    ui.label(RichText::new(batch.label()).strong())
                        .on_hover_text(batch.destination());
                    ui.label(RichText::new(batch.summary()).weak());
                    if batch.state == BatchState::Queued
                        && ui.small_button("Aus Warteschlange nehmen").clicked()
                    {
                        remove = Some(batch.id);
                    }
                });
                // Zielordner und Tage, damit sichtbar ist, was wohin geladen wird.
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("→ {} · {}", batch.destination(), batch.days_text()))
                            .small()
                            .weak(),
                    )
                    .wrap(),
                );
            }
        });
        if let Some(id) = remove {
            self.remove_queued_import(id);
        }
    }
}
