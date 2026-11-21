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
    /// Quelldateien, die dieser Import tatsächlich kopiert hat.
    pub(super) copied: Vec<PathBuf>,
}

/// Gleicher Ordner trotz `\\?\`-Präfix und Groß-/Kleinschreibung (Windows).
pub(super) fn same_folder(a: &str, b: &str) -> bool {
    let plain = |s: &str| {
        s.trim_start_matches(r"\\?\")
            .trim_end_matches(['\\', '/'])
            .to_owned()
    };
    plain(a).eq_ignore_ascii_case(&plain(b))
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
            copied: Vec::new(),
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
            || self.tasks.iter().any(|t| t.kind == JobKind::Copy)
        {
            return;
        }
        let Some(index) = self
            .imports
            .iter()
            .position(|b| b.state == BatchState::Queued)
        else {
            return;
        };
        // Der Sammlungsindex kennt Kopien dieser Sitzung erst nach der Aufnahme. Damit
        // „Bereits Importiertes überspringen“ sie trotzdem erkennt, zählen die von früheren
        // Imports dieser Sitzung ins selbe Ziel kopierten Quelldateien als importiert.
        let (earlier, rest) = self.imports.split_at_mut(index);
        let batch = &mut rest[0];
        let known: BTreeSet<PathBuf> = if batch.skip {
            let target = batch.target.to_string_lossy();
            earlier
                .iter()
                .filter(|b| same_folder(&b.target.to_string_lossy(), &target))
                .flat_map(|b| b.copied.iter().cloned())
                .collect()
        } else {
            BTreeSet::new()
        };
        batch.state = BatchState::Running;
        let label = batch.label();
        let (files, known): (Vec<SourceFile>, Vec<SourceFile>) = batch
            .files
            .iter()
            .cloned()
            .partition(|f| !known.contains(&f.path));
        let target = batch.target.clone();
        let name = batch.name.clone();
        let converter = batch.converter.clone();
        let skip = batch.skip;
        let exiftool = batch.exiftool.clone();
        self.launch(&label, true, JobKind::Copy, move |control| {
            let mut report = crate::core::import_service::execute(
                &files,
                &target,
                &name,
                converter.as_deref(),
                skip,
                &exiftool,
                &control,
            )?;
            report.skipped.extend(known.into_iter().map(|f| f.path));
            Ok(Outcome::Imported(report))
        });
    }

    /// Merkt den Zielordner für die spätere Aufnahme in die Sammlung, dauerhaft in den
    /// Einstellungen. Nur Ordner mit Sammlung; eine Sammlung wird nie angelegt.
    pub(super) fn mark_index_pending(&mut self, target: &Path) -> bool {
        if !target.join(".myfotomanager").is_file() {
            return false;
        }
        let target = target.to_string_lossy().into_owned();
        self.index_failed.retain(|root| !same_folder(root, &target));
        self.index_paused = false;
        if !self
            .settings
            .index_pending
            .iter()
            .any(|root| same_folder(root, &target))
        {
            self.settings.index_pending.push(target);
            self.save();
        }
        true
    }

    /// Wartet noch ein Ordner auf die Aufnahme neuer Dateien oder läuft sie gerade?
    /// Hintergrundschreiber der Sammlung starten erst danach.
    pub(super) fn index_due(&self) -> bool {
        self.indexing.is_some()
            || !self.index_paused
                && self
                    .settings
                    .index_pending
                    .iter()
                    .any(|root| !self.index_failed.contains(root))
    }

    /// Nimmt neue Dateien des ältesten gemerkten Ordners in die Sammlung auf, sobald kein
    /// Import mehr wartet oder läuft und kein Nutzerauftrag schreibt. Niedrige Priorität: jeder
    /// Nutzerschreibauftrag und jeder neue Import stoppt sie, danach setzt sie neu an.
    pub(super) fn start_index_update(&mut self) {
        if self.closing
            || self.index_paused
            || self.indexing.is_some()
            || self.imports_pending() > 0
            || self
                .tasks
                .iter()
                .any(|t| t.kind == JobKind::IndexNew || t.write && !low_priority(t.kind))
        {
            return;
        }
        let Some(root) = self
            .settings
            .index_pending
            .iter()
            .find(|root| !self.index_failed.contains(*root))
            .cloned()
        else {
            return;
        };
        // Die Aufnahme geht anderen Hintergrundschreibern vor.
        for task in &self.tasks {
            if low_priority(task.kind) {
                task.task.control.stop();
            }
        }
        self.indexing = Some(root.clone());
        self.launch(
            "Neue Dateien in die Sammlung aufnehmen",
            true,
            JobKind::IndexNew,
            move |control| {
                Ok(Outcome::Indexed(
                    crate::core::import_service::index_new_files(Path::new(&root), &control)?,
                ))
            },
        );
    }

    /// Aufnahme beendet: Ordner aus der Liste nehmen; war es die offene Sammlung, Ansicht und
    /// Gesichter neu lesen und die automatische Analyse vormerken.
    pub(super) fn index_finished(&mut self, found: bool) {
        let Some(root) = self.indexing.take() else {
            return;
        };
        self.settings
            .index_pending
            .retain(|pending| !same_folder(pending, &root));
        self.save();
        if found && same_folder(&root, &self.settings.collection) && !self.closing {
            self.message("Neue Dateien sind in der Sammlung".into(), false);
            if self.face_page.status.auto_import {
                self.auto_analysis_pending = true;
            }
            self.relationship_graph.stale = true;
            self.requery_collection();
            self.refresh_faces();
        }
    }

    /// Aufnahme abgebrochen oder fehlgeschlagen: Der Ordner bleibt gemerkt. Ein Abbruch
    /// (Nutzerauftrag, Import, Pause) setzt später fort; ein Fehler wird einmal gemeldet und
    /// erst nach dem nächsten Import dorthin oder beim nächsten Start erneut versucht.
    pub(super) fn index_failed_or_paused(&mut self, error: anyhow::Error, cancelled: bool) {
        let Some(root) = self.indexing.take() else {
            return;
        };
        if !cancelled {
            self.message(
                format!(
                    "Neue Dateien in {} konnten nicht in die Sammlung aufgenommen werden; \
                     nächster Versuch nach dem nächsten Import dorthin oder beim nächsten Start: \
                     {error:#}",
                    import_view::display_path(&root)
                ),
                true,
            );
            self.index_failed.insert(root.clone());
        }
        // Schon Eingetragenes bleibt sichtbar.
        if same_folder(&root, &self.settings.collection) && !self.closing {
            self.requery_collection();
        }
    }

    /// Hält das Ergebnis des laufenden Imports fest und merkt das Ziel für die spätere Aufnahme
    /// in die Sammlung. Liefert die Überschrift für Hinweis und Verlauf (Auftrag, Zielordner,
    /// Tage) und ob die Aufnahme noch folgt.
    pub(super) fn finish_import(
        &mut self,
        result: &Result<Outcome>,
        cancelled: bool,
    ) -> Option<(String, bool)> {
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
        // Auch abgebrochene oder teilweise fehlgeschlagene Imports hinterlassen Kopien. Ein
        // Fehler ohne Bericht kann nach dem Anlegen des Zielordners liegen.
        let copied = match result {
            Ok(Outcome::Imported(report)) => {
                batch.copied.clone_from(&report.sources);
                !report.copied.is_empty()
            }
            Ok(_) => false,
            Err(_) => batch.target.join(&batch.name).is_dir(),
        };
        let label = format!(
            "{} nach {} ({})",
            batch.label(),
            batch.destination(),
            batch.days_text()
        );
        let target = batch.target.clone();
        let later = copied && self.mark_index_pending(&target);
        Some((label, later))
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
