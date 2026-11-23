//! Auswertung fertiger Aufträge (`PhotoApp::poll`): Fehler und Ergebnisse je Bereich,
//! danach die fälligen Hintergrundaufträge. Ergebnisse werden über Revisionen und die
//! Sammlung des Auftrags geprüft; veraltete Antworten werden verworfen.
use super::*;

impl PhotoApp {
    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        self.frame_meter.current.textures += self.thumbnails.poll(ctx);
        let mut completed = Vec::new();
        for (index, running) in self.tasks.iter().enumerate() {
            if let Some(result) = running.task.poll() {
                completed.push((index, result));
            }
        }
        self.frame_meter.current.jobs += completed.len();
        for (index, result) in completed.into_iter().rev() {
            let running = self.tasks.remove(index);
            self.log_job_end(&running, &result);
            self.notice_origin = Some(running.view);
            // Die Einrichtung geht nur nach einer erfolgreich geladenen Laufzeit weiter.
            let chained =
                running.kind == JobKind::RuntimeDownload && std::mem::take(&mut self.setup_chain);
            let result = match result {
                Ok(Outcome::Synced(inner, note)) => {
                    if running.collection == self.settings.collection {
                        if let Some(warning) = &note.warning {
                            self.message(warning.clone(), true);
                        }
                        self.central_ui.accept(note);
                        // Nach einem Abgleich mit offenen Punkten den Bereich zeigen.
                        if self
                            .central_ui
                            .report
                            .as_ref()
                            .is_some_and(|r| r.pending() > 0)
                        {
                            self.people_ui.panel = people_table::Panel::Central;
                        }
                    }
                    Ok(*inner)
                }
                other => other,
            };
            // Meldungen eines Imports nennen Zielordner und Tage.
            let mut index_later = false;
            let label = if running.kind == JobKind::Copy {
                let cancelled = running.task.control.cancel.load(Ordering::Relaxed);
                self.finish_import(&result, cancelled)
                    .map(|(label, later)| {
                        index_later = later;
                        label
                    })
            } else {
                None
            }
            .unwrap_or_else(|| running.task.label.clone());
            match result {
                Err(e) => self.job_failed(e, &running, &label),
                Ok(outcome) => {
                    // Was kein Bereich annimmt (veraltet, andere Sammlung), wird verworfen.
                    let _ignored = self
                        .accept_collection_outcome(outcome, &running)
                        .and_then(|o| self.accept_face_outcome(o, &running))
                        .and_then(|o| self.accept_import_outcome(o, &label, index_later))
                        .and_then(|o| self.accept_setup_outcome(o, &running, chained));
                }
            }
        }
        self.notice_origin = None;
        self.start_due_work(ctx);
    }

    /// Fehlgeschlagener oder abgebrochener Auftrag.
    fn job_failed(&mut self, error: anyhow::Error, running: &Running, label: &str) {
        match error {
            e if running.kind == JobKind::IndexNew => {
                let cancelled = running.task.control.cancel.load(Ordering::Relaxed);
                self.index_failed_or_paused(e, cancelled);
            }
            e if running.kind == JobKind::MetadataBg => {
                // Abbruch durch einen Nutzerauftrag ist kein Fehler: der Lauf setzt später
                // fort. Jeder andere Fehler (z. B. ExifTool fehlt) stoppt ihn für die
                // Sitzung, damit nichts in einer Schleife neu startet.
                if !running.task.control.cancel.load(Ordering::Relaxed) {
                    self.metadata_bg_failed = true;
                    self.message(
                            format!(
                                "Metadaten im Hintergrund lesen gestoppt: {e:#} Den Rest mit „Metadaten lesen“ nachholen."
                            ),
                            true,
                        );
                }
                if running.collection == self.settings.collection && !self.closing {
                    self.requery_collection();
                }
            }
            e if running.kind == JobKind::Timeline => {
                if !running.task.control.cancel.load(Ordering::Relaxed) {
                    self.time_ui.failed(format!("{e:#}"));
                }
            }
            // Fehlende Zusatzangaben der Personendetails sind kein Fehler für den Nutzer;
            // die Abschnitte bleiben dann leer und werden mit dem nächsten Stand neu gelesen.
            // Abgelöste (abgebrochene) Lesevorgänge sind normal, alles andere wird geloggt.
            e if matches!(
                running.kind,
                JobKind::PersonInsights | JobKind::PersonDuplicates | JobKind::TimelinePeople
            ) =>
            {
                if !running.task.control.cancel.load(Ordering::Relaxed) {
                    log::warn!(
                        target: "personen",
                        "zusatzangaben_nicht_gelesen art={} fehler={e:#}",
                        running.kind
                    );
                }
            }
            // Ein neueres Lesen der Sicherungen hat dieses abgelöst; Fehler beim Lesen selbst
            // kommen als `Outcome::Backups`.
            _ if running.kind == JobKind::BackupList => {}
            e if running.kind == JobKind::BackupDaily => {
                // Abbruch (Nutzer, Programmende) ist kein Fehler; die Sicherung folgt beim
                // nächsten Öffnen. Ein Fehler erscheint als Hinweis.
                if running.task.control.cancel.load(Ordering::Relaxed) {
                    log::info!(target: "sicherung", "taeglich abgebrochen");
                } else {
                    self.message(format!("Tägliche Datensicherung: {e:#}"), true);
                }
            }
            _ if running.kind == JobKind::Folders => {
                // Der Worker liefert Fehler als `Outcome::Folders`; hier landet nur ein
                // unerwartetes Ende. Nur das erwartete Lesen der aktuellen Sammlung zählt.
                if running.collection == self.settings.collection
                    && self.folder_pending.take().is_some()
                {
                    self.folder_tree.fail();
                }
            }
            e => {
                // Misslungene Benennung: die Gesichter erscheinen beim Neulesen wieder.
                if running.kind == JobKind::Naming
                    && running.collection == self.settings.collection
                    && !self.closing
                    && !self.naming_pending()
                {
                    self.refresh_faces();
                }
                if (running.is_face_matching() || running.kind == JobKind::FaceAnalysis)
                    && running.collection == self.settings.collection
                {
                    if running.is_face_matching()
                        && !running.task.control.cancel.load(Ordering::Relaxed)
                    {
                        self.maintenance_paused = true;
                    }
                    if !self.closing {
                        self.refresh_faces();
                    }
                }
                if !running.task.control.cancel.load(Ordering::Relaxed) {
                    self.message(format!("{label}: {e:#}"), true);
                } else {
                    self.message(
                        format!(
                            "{label} abgebrochen. Bereits gespeicherte Ergebnisse bleiben erhalten."
                        ),
                        false,
                    );
                }
                if running.write
                    && running.collection == self.settings.collection
                    && !self.closing
                    && running.task.label == "Sammlung indizieren"
                {
                    // Entered batches of a cancelled or failed index run stay usable.
                    self.requery_collection();
                }
            }
        }
    }

    /// Ergebnisse zu Sammlung, Suche, Ordnern, Bilddetails und Schreibaufträgen.
    /// Gibt fremde Ergebnisse unverändert zurück.
    fn accept_collection_outcome(
        &mut self,
        outcome: Outcome,
        running: &Running,
    ) -> Option<Outcome> {
        match outcome {
            Outcome::Indexed(found) => self.index_finished(found),
            Outcome::Time(outcome) => {
                if running.collection == self.settings.collection
                    || matches!(outcome, time_view::TimeOutcome::Exported(_))
                {
                    self.accept_time(outcome);
                }
            }
            Outcome::Missing(revision, folders)
                if revision == self.missing.revision
                    && running.collection == self.settings.collection =>
            {
                self.missing.accept(folders);
            }
            Outcome::Search(revision, _, photos, counts, filters)
                if revision == self.query_revision =>
            {
                self.page.photos = photos;
                self.page_from_snapshot = false;
                match counts {
                    Some(counts) => {
                        self.status = counts.status;
                        self.page.total = counts.total;
                        self.page.cameras = counts.cameras;
                        self.counts_fresh = true;
                    }
                    // Bis zur Zählung bleiben die bisherigen Zahlen sichtbar, markiert.
                    None => {
                        self.counts_fresh = false;
                        self.count_collection(filters.clone(), self.page.photos.clone());
                    }
                }
                self.filters.folder.clone_from(&filters.folder);
                self.filters.page = filters.page;
                self.applied = filters;
                // Kopfzeile der Bilddetails (Aufnahme, Kamera, Objektiv) folgt neuen Werten.
                if let Some(selected) = &mut self.selected_photo
                    && let Some(fresh) = self.page.photos.iter().find(|p| p.id == selected.id)
                {
                    *selected = fresh.clone();
                }
                if !self.large
                    && self.selected_photo.as_ref().is_some_and(|selected| {
                        !self.page.photos.iter().any(|photo| photo.id == selected.id)
                    })
                {
                    self.selected_photo = None;
                    self.detail_revision += 1;
                }
            }
            Outcome::Counts(revision, stamp, counts, filters, first_page)
                if revision == self.query_revision =>
            {
                self.status = counts.status.clone();
                self.page.total = counts.total;
                self.page.cameras = counts.cameras.clone();
                self.counts_fresh = true;
                self.known_counts = stamp.map(|stamp| {
                    let mut filters = filters;
                    filters.page = 0;
                    Known {
                        stamp,
                        filters,
                        counts,
                        first_page,
                    }
                });
            }
            Outcome::Folders(revision, result)
                if self.folder_pending == Some(revision)
                    && running.collection == self.settings.collection =>
            {
                // Auch wenn sich der Bestand inzwischen geändert hat: Neueres gibt es nicht.
                // Der Baum bleibt dann veraltet und liest genau einmal nach.
                self.folder_pending = None;
                match result {
                    Ok(levels) => self.folder_tree.apply(&levels),
                    // Never retry a failing or cancelled read in a loop; "↻" retries.
                    Err(_) => self.folder_tree.fail(),
                }
            }
            Outcome::Detail(revision, detail, faces, analysed)
                if revision == self.detail_revision =>
            {
                self.detail = detail;
                self.detail_rows.clear();
                if let Ok(serde_json::Value::Array(files)) = serde_json::from_str(&self.detail) {
                    for file in files {
                        if let Some(path) = file.get("Datei").and_then(|v| v.as_str()) {
                            self.detail_rows.push(("Datei".into(), path.into()));
                        }
                        if let Some(tags) = file.get("Metadaten").and_then(|v| v.as_object()) {
                            for (key, value) in tags {
                                self.detail_rows.push((
                                    key.clone(),
                                    value
                                        .as_str()
                                        .map(str::to_string)
                                        .unwrap_or_else(|| value.to_string()),
                                ));
                            }
                        } else {
                            self.detail_rows.push((
                                "Status".into(),
                                "Metadaten noch nicht angereichert".into(),
                            ));
                        }
                    }
                }
                self.detail_faces = faces;
                self.detail_analysis_done = Some(analysed);
            }
            Outcome::Done(message) => {
                if running.kind == JobKind::Undo && running.collection == self.settings.collection {
                    self.undo.pop();
                }
                let background = running.kind == JobKind::MetadataBg;
                if !background {
                    self.message(message, false);
                }
                if running.task.label == "Sammlung indizieren"
                    && self.settings.auto_metadata
                    && running.collection == self.settings.collection
                {
                    self.auto_metadata_pending = true;
                }
                if running.write && running.collection == self.settings.collection {
                    self.relationship_graph.stale = true;
                    self.requery_collection();
                    self.refresh_faces();
                    // Der Hintergrundlauf lädt die Details nur neu, wenn sie noch fehlen.
                    let missing = self.detail_rows.iter().any(|(key, _)| key == "Status");
                    if let Some(photo) = self.selected_photo.clone()
                        && (!background || missing)
                    {
                        self.choose_photo(photo);
                    }
                }
            }
            Outcome::Report(text) => self.report = Some(text),
            other => return Some(other),
        }
        None
    }

    /// Ergebnisse zu Gesichtern, Personen, Benennungen und Beziehungsgraph.
    /// Gibt fremde Ergebnisse unverändert zurück.
    fn accept_face_outcome(&mut self, outcome: Outcome, running: &Running) -> Option<Outcome> {
        match outcome {
            Outcome::Duplicates(revision, duplicates)
                if running.collection == self.settings.collection =>
            {
                self.people_ui.accept_duplicates(revision, duplicates);
            }
            Outcome::PersonInsights(revision, insights)
                if running.collection == self.settings.collection =>
            {
                self.people_ui.accept_insights(revision, insights);
            }
            Outcome::Faces(revision, index, page) if revision == self.face_revision => {
                if self.view != View::Settings
                    || page.status.detector_id != self.face_page.status.detector_id
                    || page.status.embedder_id != self.face_page.status.embedder_id
                {
                    self.detector = page.status.detector_id.clone();
                    self.embedder = page.status.embedder_id.clone();
                }
                if page.groups.is_empty()
                    || page.status.embedder != self.face_page.status.embedder
                    || page.status.detector != self.face_page.status.detector
                {
                    self.face_advance = None;
                } else if let Some(advance) = &mut self.face_advance {
                    advance.ready = true;
                    if index != self.face_index {
                        advance.following.clear();
                        advance.slot = page.groups.len() - 1;
                    }
                }
                self.face_index = index;
                // Neuer Datenstand: Caches der Zeitreise prüfen sich daran.
                if let Some(revisions) = &page.revisions {
                    self.time_ui.observe(revisions);
                }
                self.face_page = page;
                // Noch nicht geschriebene Entscheidungen bleiben ausgeblendet (ADR 0068).
                self.hide_pending_faces();
                self.face_page_serial += 1;
            }
            Outcome::PersonAdded(id) => {
                self.message("Person angelegt".into(), false);
                if running.collection == self.settings.collection {
                    self.family_ui.just_added = Some(id);
                    self.select_person(id);
                    self.people_ui.scroll_to_person(id);
                    self.relationship_graph.stale = true;
                    self.time_ui.need_people = true;
                    self.refresh_faces();
                }
            }
            Outcome::Named(undo) if running.collection == self.settings.collection => {
                self.relationship_graph.stale = true;
                // Die Zeitreise folgt dem Datenstand der neu gelesenen Gesichterseite.
                self.prepare_group_advance(&undo);
                let saved = optimistic::decision_message(undo.summary());
                self.undo.push(undo);
                if self.undo.len() > 20 {
                    self.undo.remove(0);
                }
                self.message(saved, false);
                // Erst nach dem letzten vorgemerkten Auftrag neu lesen (ADR 0068).
                if !self.naming_pending() {
                    self.refresh_faces();
                }
                if let Some(photo) = self.selected_photo.clone() {
                    self.choose_photo(photo);
                }
            }
            Outcome::RelationshipGraph(revision, signature, graph)
                if running.collection == self.settings.collection
                    && revision == self.relationship_graph.revision =>
            {
                self.relationship_graph.refreshed(signature, graph);
            }
            Outcome::PersonStats(revision, signature, stats)
                if running.collection == self.settings.collection
                    && revision == self.person_stats.revision =>
            {
                self.person_stats.refreshed(signature, stats);
            }
            Outcome::Suggestions(revision, id, suggestions)
                if revision == self.naming_revision && self.naming == Some(id) =>
            {
                self.name_portraits = suggestions.portraits;
                self.name_suggestions = suggestions.items
            }
            other => return Some(other),
        }
        None
    }

    /// Ergebnisse zu Quelle, Import und DNG-Prüfung.
    /// Gibt fremde Ergebnisse unverändert zurück.
    fn accept_import_outcome(
        &mut self,
        outcome: Outcome,
        label: &str,
        index_later: bool,
    ) -> Option<Outcome> {
        match outcome {
            Outcome::Scan(revision, scan) if revision == self.scan_revision => {
                self.focused = scan
                    .files
                    .first()
                    .map(|f| f.day.clone())
                    .unwrap_or_default();
                self.message(
                    format!(
                        "{} gefunden · {} Hinweise",
                        files_and_days(
                            scan.files.len(),
                            scan.files
                                .iter()
                                .map(|f| &f.day)
                                .collect::<BTreeSet<_>>()
                                .len(),
                        ),
                        scan.warnings.len()
                    ),
                    !scan.warnings.is_empty(),
                );
                self.scan = scan;
            }
            Outcome::SourceOffline(revision, path) if revision == self.scan_revision => {
                log::info!(target: "import", "quelle_nicht_verbunden pfad={path:?}");
                self.source_offline = Some(path);
            }
            Outcome::DngUpdate(converter, installed, cache) => {
                self.settings.dng_update = cache.clone();
                self.save();
                if converter == self.settings.converter {
                    let newer = installed
                        .as_deref()
                        .zip(cache.latest.as_deref())
                        .and_then(|(a, b)| {
                            Some(
                                crate::core::dng_updates::version(b).ok()?
                                    > crate::core::dng_updates::version(a).ok()?,
                            )
                        })
                        .unwrap_or(false);
                    self.dng_status = format!(
                        "Installiert: {} · Adobe: {}{}",
                        installed.unwrap_or_else(|| "nicht gefunden / Version unbekannt".into()),
                        cache.latest.unwrap_or_else(|| {
                            "Prüfung nicht verfügbar; nächster Versuch morgen".into()
                        }),
                        if newer { " · Update verfügbar" } else { "" }
                    );
                }
            }
            Outcome::Imported(report) => {
                // Kein Neulesen der Sammlung je Import: die neuen Dateien kommen erst mit
                // der Aufnahme nach den Imports hinzu (`start_index_update`).
                self.message(
                    format!(
                        "{} {} · {} Dateien kopiert · {} übersprungen · {} DNGs · {} Fehler{}{}",
                        label,
                        if report.cancelled {
                            "abgebrochen"
                        } else {
                            "beendet"
                        },
                        report.copied.len(),
                        report.skipped.len(),
                        report.converted,
                        report.errors.len(),
                        if report.errors.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", report.errors.join("; "))
                        },
                        if index_later {
                            " · wird nach den Imports in die Sammlung aufgenommen"
                        } else {
                            ""
                        }
                    ),
                    !report.errors.is_empty(),
                );
            }
            other => return Some(other),
        }
        None
    }

    /// Ergebnisse zu Datensicherung, Laufzeit, Modellen und gewählten Pfaden.
    /// Gibt fremde Ergebnisse unverändert zurück.
    fn accept_setup_outcome(
        &mut self,
        outcome: Outcome,
        running: &Running,
        chained: bool,
    ) -> Option<Outcome> {
        match outcome {
            Outcome::Backups(revision, files)
                if revision == self.backups.revision
                    && running.collection == self.settings.collection =>
            {
                self.backups.accept(files);
            }
            Outcome::DailyBackup(collection, date, summary) => {
                log::info!(target: "sicherung", "taeglich {summary}");
                self.settings
                    .daily_backup_dates
                    .insert(collection.clone(), date);
                self.save();
                if collection == self.settings.collection && !self.backups.loading {
                    self.backups.files = None;
                }
            }
            Outcome::BackupCreated(message) => {
                self.message(message, false);
                if running.collection == self.settings.collection && !self.backups.loading {
                    self.backups.files = None;
                }
            }
            Outcome::RuntimeDownloaded(path) => {
                let previous = std::mem::replace(
                    &mut self.settings.onnx_runtime,
                    path.to_string_lossy().into_owned(),
                );
                if self.save() {
                    self.path_drafts
                        .remove(&settings_view::PathSetting::Runtime);
                    self.message(
                            "Die Laufzeit ist installiert und ausgewählt. Falls zuvor eine andere geladen wurde, bitte die App neu starten.".into(),
                            false,
                        );
                    if chained {
                        self.continue_face_setup(running.view);
                    }
                } else {
                    self.settings.onnx_runtime = previous;
                }
            }
            Outcome::ModelsInstalled(path, models) => {
                let previous = std::mem::replace(
                    &mut self.settings.model_dir,
                    path.to_string_lossy().into_owned(),
                );
                if self.save() {
                    self.path_drafts.remove(&settings_view::PathSetting::Models);
                    self.models = models;
                    self.message(
                        format!(
                            "Die Erkennungsmodelle sind installiert. Modellordner: {}",
                            path.display()
                        ),
                        false,
                    );
                } else {
                    self.settings.model_dir = previous;
                }
            }
            Outcome::Pick(pick, Some(path)) => {
                let value = path.to_string_lossy().into_owned();
                let field = match pick {
                    Pick::Source => {
                        self.settings.source = value;
                        self.begin_scan();
                        None
                    }
                    Pick::Target => {
                        self.settings.target = value;
                        None
                    }
                    Pick::Collection => {
                        self.choose_collection(value);
                        None
                    }
                    Pick::Converter => Some((settings_view::PathSetting::Converter, value)),
                    Pick::Exiftool => Some((settings_view::PathSetting::Exiftool, value)),
                    Pick::Models => Some((settings_view::PathSetting::Models, value)),
                    Pick::Runtime => Some((settings_view::PathSetting::Runtime, value)),
                };
                match field {
                    // Werkzeug- und Modellpfade werden geprüft und wirken sofort.
                    Some((field, value)) => {
                        self.set_path_setting(field, value);
                    }
                    None => {
                        self.save();
                    }
                }
            }
            other => return Some(other),
        }
        None
    }

    /// Startet nach der Auswertung, was jetzt fällig ist: nächster Import, Aufnahme,
    /// Metadaten, Analyse, Gesichter, Sicherung, Nachtlauf und Wartung.
    fn start_due_work(&mut self, ctx: &egui::Context) {
        // Der nächste festgelegte Import beginnt, sobald der vorige fertig ist; erst danach
        // werden die neuen Dateien in die Sammlung aufgenommen.
        self.start_next_import();
        self.start_index_update();
        if !self.tasks.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if !self.closing && !self.busy() && self.auto_metadata_pending {
            // Stage 2 of the index: metadata in portions (ADR 0059) after the quick entry.
            self.auto_metadata_pending = false;
            self.collection_action("metadata");
        }
        self.live_gallery(ctx);
        // Die Analyse wartet, bis alle festgelegten Imports durch sind; vorgemerkt wird sie
        // erst nach der Aufnahme der neuen Dateien in die offene Sammlung.
        if !self.closing
            && !self.busy()
            && !self.models_installing()
            && self.imports_pending() == 0
            && self.auto_analysis_pending
        {
            self.auto_analysis_pending = false;
            self.collection_action("automatic");
        }
        // Beim Start: Personen und Gesichter lesen, sobald Seite und Zählung der Sammlung da
        // sind (oder gescheitert).
        if self.faces_after_search
            && !self
                .tasks
                .iter()
                .any(|task| matches!(task.kind, JobKind::Search | JobKind::Counts))
        {
            self.refresh_faces();
        }
        self.start_metadata_background();
        self.start_daily_backup();
        let now = chrono::Local::now();
        if !self.closing
            && !self.busy()
            && !self.models_installing()
            && !self.settings.collection.is_empty()
            && (self.face_page.status.nightly || self.settings.auto_metadata)
            && chrono::Timelike::hour(&now) == 3
            && self.last_night != now.date_naive().to_string()
        {
            self.last_night = now.date_naive().to_string();
            self.collection_action("nightly");
        }
        ctx.request_repaint_after(Duration::from_secs(30));
        if !self.closing
            && !self.maintenance_paused
            && !self.busy()
            && self.imports_pending() == 0
            && !self.index_due()
            && self.face_page.status.pending > 0
            && !self.tasks.iter().any(|t| {
                matches!(
                    t.kind,
                    JobKind::Maintenance | JobKind::MetadataBg | JobKind::Faces
                )
            })
            && !self.settings.collection.is_empty()
        {
            self.launch_write(
                "Erkennungswissen im Hintergrund aktualisieren",
                JobKind::Maintenance,
                move |mut c, control| {
                    Ok(Outcome::Done(crate::core::recognition::maintain(
                        &mut c, &control,
                    )?))
                },
            );
        }
    }
}
