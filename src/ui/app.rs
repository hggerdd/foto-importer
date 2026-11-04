mod collection_view;
mod import_view;
mod people_view;
mod settings_view;
#[cfg(test)]
mod tests;
mod windows;

use super::{theme, thumbnails::Thumbnails};
use crate::{
    config::{self, Settings},
    core::{
        collection::{Collection, CollectionStatus},
        faces::{self, Face, FacePage, Undo},
        files::{self, Scan, SourceFile},
        jobs::{Control, Task},
        metadata::{self, Photo, Rule, Search, SearchPage},
        previews::PreviewRequest,
    },
};
use anyhow::Result;
use egui::{Color32, RichText};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};

#[derive(Clone, Copy, PartialEq)]
enum View {
    Import,
    Collection,
    People,
    Tasks,
    Settings,
}
#[derive(Clone, Copy)]
enum Pick {
    Source,
    Target,
    Collection,
    Converter,
    Exiftool,
    Models,
    Runtime,
}
enum Outcome {
    Scan(u64, Scan),
    Search(u64, CollectionStatus, SearchPage, Search),
    Faces(u64, FacePage),
    Detail(u64, String, Vec<Face>),
    Done(String),
    Imported(files::ImportReport),
    Suggestions(u64, i64, Vec<(i64, String, f32)>),
    Report(String),
    DngUpdate(
        String,
        Option<String>,
        crate::core::dng_updates::UpdateCache,
    ),
    Named(Undo),
    Pick(Pick, Option<PathBuf>),
}
struct Running {
    task: Task<Outcome>,
    write: bool,
    kind: &'static str,
    collection: String,
}
#[derive(Default)]
struct Draft {
    name: String,
    person: Option<i64>,
    excluded: BTreeSet<i64>,
}

pub struct PhotoApp {
    settings: Settings,
    view: View,
    tasks: Vec<Running>,
    history: Vec<String>,
    notice: String,
    error: bool,
    scan: Scan,
    scan_revision: u64,
    selected: BTreeSet<String>,
    focused: String,
    folder_name: String,
    filters: Search,
    applied: Search,
    query_revision: u64,
    page: SearchPage,
    status: CollectionStatus,
    face_revision: u64,
    face_page: FacePage,
    face_mode: String,
    face_index: usize,
    drafts: BTreeMap<String, Draft>,
    undo: Vec<Undo>,
    thumbnails: Thumbnails,
    selected_photo: Option<Photo>,
    detail: String,
    detail_faces: Vec<Face>,
    detail_revision: u64,
    large: bool,
    show_frames: bool,
    inspector: bool,
    zoom: f32,
    detector: String,
    embedder: String,
    models: Vec<crate::core::inference::Manifest>,
    naming: Option<Face>,
    name_input: String,
    name_person: Option<i64>,
    name_suggestions: Vec<(i64, String, f32)>,
    relation_from: Option<i64>,
    relation_to: Option<i64>,
    relation_kind: String,
    person_edit: Option<faces::Person>,
    report: Option<String>,
    delete_phrase: String,
    dng_status: String,
    closing: bool,
    maintenance_paused: bool,
    auto_analysis_pending: bool,
    last_night: String,
    capture_started: std::time::Instant,
    capture_requested: bool,
    #[cfg(test)]
    plan_bounds: Option<egui::Rect>,
}
impl PhotoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self::with_settings(cc, Settings::load());
        app.check_dng();
        app
    }
    fn with_settings(cc: &eframe::CreationContext<'_>, settings: Settings) -> Self {
        theme::apply(&cc.egui_ctx, settings.dark);
        let models =
            crate::core::inference::manifests(Path::new(&settings.model_dir)).unwrap_or_default();
        let mut app = Self {
            settings,
            view: View::Import,
            tasks: Vec::new(),
            history: Vec::new(),
            notice: "Bereit für deine nächsten Erinnerungen".into(),
            error: false,
            scan: Scan::default(),
            scan_revision: 0,
            selected: BTreeSet::new(),
            focused: String::new(),
            folder_name: String::new(),
            filters: Search::default(),
            applied: Search::default(),
            query_revision: 0,
            page: SearchPage::default(),
            status: CollectionStatus::default(),
            face_revision: 0,
            face_page: FacePage::default(),
            face_mode: "unknown".into(),
            face_index: 0,
            drafts: BTreeMap::new(),
            undo: Vec::new(),
            thumbnails: Thumbnails::new(cc.egui_ctx.clone()),
            selected_photo: None,
            detail: String::new(),
            detail_faces: Vec::new(),
            detail_revision: 0,
            large: false,
            show_frames: true,
            inspector: true,
            zoom: 172.0,
            detector: "yunet".into(),
            embedder: "sface".into(),
            models,
            naming: None,
            name_input: String::new(),
            name_person: None,
            name_suggestions: Vec::new(),
            relation_from: None,
            relation_to: None,
            relation_kind: "parent".into(),
            person_edit: None,
            report: None,
            delete_phrase: String::new(),
            dng_status: "Converter-Version wird gelesen …".into(),
            closing: false,
            maintenance_paused: false,
            auto_analysis_pending: false,
            last_night: String::new(),
            capture_started: std::time::Instant::now(),
            capture_requested: false,
            #[cfg(test)]
            plan_bounds: None,
        };
        if !app.settings.source.is_empty() {
            app.begin_scan();
        }
        if !app.settings.collection.is_empty() {
            app.refresh_collection();
            app.refresh_faces();
        }
        if std::env::var("FOTO_IMPORTER_VIEW").as_deref() == Ok("collection") {
            app.view = View::Collection;
        }
        app
    }
    fn busy(&self) -> bool {
        self.tasks
            .iter()
            .any(|t| t.write && t.kind != "copy" && t.kind != "maintenance")
    }
    fn launch(
        &mut self,
        label: &str,
        write: bool,
        kind: &'static str,
        work: impl FnOnce(Control) -> Result<Outcome> + Send + 'static,
    ) {
        if write && kind != "copy" && kind != "maintenance" {
            for task in &self.tasks {
                if task.kind == "maintenance" {
                    task.task.control.stop();
                }
            }
        }
        if !write && kind != "picker" {
            for task in &self.tasks {
                if task.kind == kind {
                    task.task.control.stop();
                }
            }
        }
        let writer_root = self.settings.collection.clone();
        self.tasks.push(Running {
            task: Task::start_control(
                label,
                Control {
                    background: write,
                    ..Default::default()
                },
                move |control| {
                    let _slot = if write && kind != "copy" {
                        let resolved = Path::new(&writer_root).canonicalize()?;
                        Some(control.lane(&format!(
                            "writer:{}",
                            files::fold(&files::normalize(&resolved))
                        ))?)
                    } else if !write && kind != "picker" {
                        Some(control.lane(kind)?)
                    } else {
                        None
                    };
                    let _foreground = if !write && kind != "picker" && kind != "dngupdates" {
                        Some(crate::core::jobs::foreground())
                    } else {
                        None
                    };
                    control.checkpoint()?;
                    work(control)
                },
            ),
            write,
            kind,
            collection: self.settings.collection.clone(),
        });
    }
    fn message(&mut self, text: String, error: bool) {
        self.notice = text.clone();
        self.error = error;
        self.history.push(format!(
            "{} · {text}",
            chrono::Local::now().format("%H:%M:%S")
        ));
        if self.history.len() > 100 {
            self.history.remove(0);
        }
    }
    fn save(&mut self) -> bool {
        if let Err(e) = self.settings.save() {
            self.message(
                format!("Einstellungen konnten nicht gespeichert werden: {e}"),
                true,
            );
            return false;
        }
        true
    }
    fn check_dng(&mut self) {
        if self.tasks.iter().any(|t| t.kind == "dngupdates") {
            return;
        }
        let cache = self.settings.dng_update.clone();
        let today = chrono::Local::now().date_naive().to_string();
        if cache.date != today {
            self.settings.dng_update = crate::core::dng_updates::UpdateCache {
                date: today,
                ..Default::default()
            };
            if !self.save() {
                return;
            }
        }
        let converter = self.settings.converter.clone();
        self.launch(
            "DNG-Converter-Version prüfen",
            false,
            "dngupdates",
            move |_| {
                let installed = crate::core::dng_updates::installed(Path::new(&converter));
                Ok(Outcome::DngUpdate(
                    converter,
                    installed,
                    crate::core::dng_updates::check(cache),
                ))
            },
        );
    }
    fn pick(&mut self, pick: Pick) {
        if self.tasks.iter().any(|t| t.kind == "picker") {
            return;
        }
        self.launch("Ordner oder Werkzeug wählen", false, "picker", move |_| {
            let dialog = rfd::FileDialog::new();
            let path = match pick {
                Pick::Source | Pick::Target | Pick::Collection | Pick::Models => {
                    dialog.pick_folder()
                }
                _ => dialog.pick_file(),
            };
            Ok(Outcome::Pick(pick, path))
        });
    }
    fn begin_scan(&mut self) {
        self.scan_revision += 1;
        let revision = self.scan_revision;
        let root = PathBuf::from(&self.settings.source);
        let metadata = self.settings.metadata_date;
        for task in &self.tasks {
            if task.kind == "scan" {
                task.task.control.stop();
            }
        }
        self.scan = Scan::default();
        self.selected.clear();
        self.focused.clear();
        self.thumbnails.clear();
        self.save();
        self.launch("Quelle einlesen", false, "scan", move |control| {
            Ok(Outcome::Scan(
                revision,
                files::scan(&root, metadata, &control)?,
            ))
        });
    }
    fn refresh_collection(&mut self) {
        if self.settings.collection.is_empty() {
            return;
        }
        self.query_revision += 1;
        let revision = self.query_revision;
        let root = PathBuf::from(&self.settings.collection);
        let filters = self.filters.clone();
        self.launch("Sammlung durchsuchen", false, "search", move |_| {
            let c = Collection::open(&root, false)?;
            let detector = faces::setting(&c, "rust.detector_key")?.unwrap_or_default();
            Ok(Outcome::Search(
                revision,
                c.status()?,
                metadata::search(&c, &filters, &detector)?,
                filters,
            ))
        });
    }
    fn refresh_faces(&mut self) {
        if self.settings.collection.is_empty() {
            return;
        }
        self.face_revision += 1;
        let revision = self.face_revision;
        let root = PathBuf::from(&self.settings.collection);
        let mode = self.face_mode.clone();
        let page = self.face_index;
        self.launch("Personen und Gesichter lesen", false, "faces", move |_| {
            let c = Collection::open(&root, false)?;
            Ok(Outcome::Faces(revision, faces::page(&c, &mode, page)?))
        });
    }
    fn collection_action(&mut self, action: &str) {
        if action == "resume" {
            self.maintenance_paused = false;
        }
        if self.busy() || self.settings.collection.is_empty() {
            return;
        }
        let settings = self.settings.clone();
        let action = action.to_string();
        let detector = self.detector.clone();
        let embedder = self.embedder.clone();
        let label = match action.as_str() {
            "scan" => "Sammlung indizieren",
            "metadata" => "Metadaten lesen",
            "verify" => "Dateien mit BLAKE3 prüfen",
            "analyze" => "Fotos lokal analysieren",
            "knowledge" => "Erkennungswissen anwenden",
            "automatic" | "nightly" => "Automatische Sammlungsanalyse",
            "export" => "Referenzausschnitte exportieren",
            "calibrate" => "Referenzen validieren",
            "resume" => "Erkennungsabgleich fortsetzen",
            "delete_biometrics" => "Biometrie bereinigen",
            _ => "Sammlung öffnen",
        };
        self.launch(label, true, "mutation", move |control| {
            let mut c = Collection::open(Path::new(&settings.collection), true)?;
            let message = match action.as_str() {
                "scan" => c.quick_scan(false, &control)?,
                "metadata" => metadata::enrich(&mut c, &settings.exiftool, &control)?,
                "automatic" | "nightly" => {
                    if settings.auto_metadata {
                        metadata::enrich(&mut c, &settings.exiftool, &control)?;
                    }
                    if action == "automatic" || faces::status(&c)?.nightly {
                        faces::analyze(
                            &mut c,
                            Path::new(&settings.model_dir),
                            Path::new(&settings.onnx_runtime),
                            &detector,
                            &embedder,
                            &settings.exiftool,
                            &control,
                        )?
                    } else {
                        "Nächtliche Metadatenanreicherung abgeschlossen".into()
                    }
                }
                "verify" => c.verify(&control)?,
                "export" => crate::core::maintenance::export(&c, &settings.exiftool, &control)?,
                "delete_biometrics" => crate::core::maintenance::delete_biometrics(&c)?,
                "calibrate" => {
                    return Ok(Outcome::Report(serde_json::to_string_pretty(
                        &crate::core::maintenance::calibrate(&c, &control)?,
                    )?));
                }
                "resume" => {
                    let key = faces::setting(&c, "rust.embedder_key")?
                        .ok_or_else(|| anyhow::anyhow!("Kein Modellraum"))?;
                    faces::rematch(&mut c, &key, &control)?;
                    "Erkennungsabgleich abgeschlossen".into()
                }
                "analyze" => faces::analyze(
                    &mut c,
                    Path::new(&settings.model_dir),
                    Path::new(&settings.onnx_runtime),
                    &detector,
                    &embedder,
                    &settings.exiftool,
                    &control,
                )?,
                "knowledge" => faces::apply_knowledge(&mut c, &control)?,
                _ => "Sammlung geöffnet".into(),
            };
            Ok(Outcome::Done(message))
        });
    }
    fn choose_photo(&mut self, photo: Photo) {
        self.detail_revision += 1;
        let revision = self.detail_revision;
        self.detail.clear();
        self.detail_faces.clear();
        let root = PathBuf::from(&self.settings.collection);
        let id = photo.id;
        self.selected_photo = Some(photo);
        self.launch("Bilddetails lesen", false, "detail", move |_| {
            let c = Collection::open(&root, false)?;
            Ok(Outcome::Detail(
                revision,
                serde_json::to_string_pretty(&metadata::detail(&c, id)?)?,
                faces::photo_faces(&c, id)?,
            ))
        });
    }
    fn open_naming(&mut self, face: Face) {
        let root = self.settings.collection.clone();
        let id = face.id;
        let revision = self.detail_revision;
        self.naming = Some(face);
        self.name_input.clear();
        self.name_person = None;
        self.name_suggestions.clear();
        self.launch("Namensvorschläge lesen", false, "suggestions", move |_| {
            let c = Collection::open(Path::new(&root), false)?;
            let key = faces::setting(&c, "rust.embedder_key")?.unwrap_or_default();
            Ok(Outcome::Suggestions(
                revision,
                id,
                faces::suggestions(&c, id, &key)?,
            ))
        });
    }
    fn poll(&mut self, ctx: &egui::Context) {
        self.thumbnails.poll(ctx);
        let mut completed = Vec::new();
        for (index, running) in self.tasks.iter().enumerate() {
            if let Some(result) = running.task.poll() {
                completed.push((index, result));
            }
        }
        for (index, result) in completed.into_iter().rev() {
            let running = self.tasks.remove(index);
            match result {
                Err(e) => {
                    if running.kind == "maintenance"
                        && !running.task.control.cancel.load(Ordering::Relaxed)
                    {
                        self.maintenance_paused = true;
                    }
                    if !running.task.control.cancel.load(Ordering::Relaxed) {
                        self.message(format!("{}: {e:#}", running.task.label), true);
                    } else {
                        self.message(
                            format!(
                                "{} abgebrochen. Bereits gespeicherte Ergebnisse bleiben erhalten.",
                                running.task.label
                            ),
                            false,
                        );
                    }
                }
                Ok(Outcome::Scan(revision, scan)) if revision == self.scan_revision => {
                    self.focused = scan
                        .files
                        .first()
                        .map(|f| f.day.clone())
                        .unwrap_or_default();
                    self.message(
                        format!(
                            "{} Dateien an {} Tagen gefunden · {} Hinweise",
                            scan.files.len(),
                            scan.files
                                .iter()
                                .map(|f| &f.day)
                                .collect::<BTreeSet<_>>()
                                .len(),
                            scan.warnings.len()
                        ),
                        !scan.warnings.is_empty(),
                    );
                    self.scan = scan;
                }
                Ok(Outcome::Search(revision, status, page, filters))
                    if revision == self.query_revision =>
                {
                    self.status = status;
                    self.page = page;
                    self.applied = filters;
                    self.thumbnails.clear();
                }
                Ok(Outcome::Faces(revision, page)) if revision == self.face_revision => {
                    if self.view != View::Settings
                        || page.status.detector_id != self.face_page.status.detector_id
                        || page.status.embedder_id != self.face_page.status.embedder_id
                    {
                        self.detector = page.status.detector_id.clone();
                        self.embedder = page.status.embedder_id.clone();
                    }
                    self.face_page = page;
                }
                Ok(Outcome::Detail(revision, detail, faces))
                    if revision == self.detail_revision =>
                {
                    self.detail = detail;
                    self.detail_faces = faces;
                }
                Ok(Outcome::Done(message)) => {
                    if running.kind == "undo" && running.collection == self.settings.collection {
                        self.undo.pop();
                    }
                    self.message(message, false);
                    if running.write && running.collection == self.settings.collection {
                        self.refresh_collection();
                        self.refresh_faces();
                        if let Some(photo) = self.selected_photo.clone() {
                            self.choose_photo(photo);
                        }
                    }
                }
                Ok(Outcome::Named(undo)) if running.collection == self.settings.collection => {
                    self.naming = None;
                    self.undo.push(undo);
                    if self.undo.len() > 20 {
                        self.undo.remove(0);
                    }
                    self.message("Gesichtsentscheidung gespeichert".into(), false);
                    self.refresh_faces();
                    if let Some(photo) = self.selected_photo.clone() {
                        self.choose_photo(photo);
                    }
                }
                Ok(Outcome::Report(text)) => self.report = Some(text),
                Ok(Outcome::DngUpdate(converter, installed, cache)) => {
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
                            installed
                                .unwrap_or_else(|| "nicht gefunden / Version unbekannt".into()),
                            cache.latest.unwrap_or_else(|| {
                                "Prüfung nicht verfügbar; nächster Versuch morgen".into()
                            }),
                            if newer { " · Update verfügbar" } else { "" }
                        );
                    }
                }
                Ok(Outcome::Imported(report)) => {
                    if !report.cancelled
                        && !report.copied.is_empty()
                        && self.face_page.status.auto_import
                        && self
                            .settings
                            .target
                            .trim_start_matches(r"\\?\")
                            .eq_ignore_ascii_case(
                                self.settings.collection.trim_start_matches(r"\\?\"),
                            )
                    {
                        self.auto_analysis_pending = true;
                    }
                    self.message(
                        format!(
                            "{} · {} Dateien kopiert · {} übersprungen · {} DNGs · {} Fehler{}",
                            if report.cancelled {
                                "Import abgebrochen"
                            } else {
                                "Import beendet"
                            },
                            report.copied.len(),
                            report.skipped.len(),
                            report.converted,
                            report.errors.len(),
                            if report.errors.is_empty() {
                                String::new()
                            } else {
                                format!(": {}", report.errors.join("; "))
                            }
                        ),
                        !report.errors.is_empty(),
                    );
                    self.refresh_collection();
                    self.refresh_faces();
                }
                Ok(Outcome::Suggestions(revision, id, suggestions))
                    if revision == self.detail_revision
                        && self.naming.as_ref().is_some_and(|f| f.id == id) =>
                {
                    self.name_suggestions = suggestions
                }
                Ok(Outcome::Pick(pick, Some(path))) => {
                    let value = path.to_string_lossy().into_owned();
                    match pick {
                        Pick::Source => {
                            self.settings.source = value;
                            self.begin_scan();
                        }
                        Pick::Target => self.settings.target = value,
                        Pick::Collection => {
                            self.settings.collection = value;
                            self.query_revision += 1;
                            self.face_revision += 1;
                            self.detail_revision += 1;
                            self.page = SearchPage::default();
                            self.selected_photo = None;
                            self.face_page = FacePage::default();
                            self.undo.clear();
                            self.drafts.clear();
                            self.thumbnails.clear();
                            if Path::new(&self.settings.collection)
                                .join(".myfotomanager")
                                .exists()
                            {
                                self.refresh_collection();
                                self.refresh_faces();
                            } else {
                                self.collection_action("scan");
                            }
                        }
                        Pick::Converter => self.settings.converter = value,
                        Pick::Exiftool => self.settings.exiftool = value,
                        Pick::Models => {
                            self.settings.model_dir = value;
                            self.models = crate::core::inference::manifests(Path::new(
                                &self.settings.model_dir,
                            ))
                            .unwrap_or_default();
                        }
                        Pick::Runtime => self.settings.onnx_runtime = value,
                    };
                    self.save();
                }
                _ => {}
            }
        }
        if !self.tasks.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if !self.closing && !self.busy() && self.auto_analysis_pending {
            self.auto_analysis_pending = false;
            self.collection_action("automatic");
        }
        let now = chrono::Local::now();
        if !self.closing
            && !self.busy()
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
            && self.face_page.status.pending > 0
            && !self.tasks.iter().any(|t| t.kind == "maintenance")
            && !self.settings.collection.is_empty()
        {
            self.face_page.status.pending = 0;
            let root = self.settings.collection.clone();
            self.launch(
                "Erkennungswissen im Hintergrund aktualisieren",
                true,
                "maintenance",
                move |control| {
                    let mut c = Collection::open(Path::new(&root), true)?;
                    Ok(Outcome::Done(crate::core::recognition::maintain(
                        &mut c, &control,
                    )?))
                },
            );
        }
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(190.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(14, 19, 25))
                    .inner_margin(20),
            )
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.label(
                    RichText::new("F / I")
                        .size(28.0)
                        .strong()
                        .color(theme::ACCENT),
                );
                ui.label(RichText::new("FOTO-IMPORTER").size(13.0).strong());
                ui.label(RichText::new("Raum für Erinnerungen").size(11.0).weak());
                ui.add_space(40.0);
                theme::eyebrow(ui, "ARBEITSBEREICH");
                for (view, title, hint) in [
                    (View::Import, "01  Importieren", "Karte → Sammlung"),
                    (View::Collection, "02  Sammlung", "Fotos finden & verstehen"),
                    (View::People, "03  Personen", "Namen & Beziehungen"),
                    (View::Tasks, "04  Aufgaben", "Fortschritt & Verlauf"),
                ] {
                    let response = ui
                        .add_sized(
                            [150.0, 42.0],
                            egui::Button::new(RichText::new(title).strong())
                                .selected(self.view == view),
                        )
                        .on_hover_text(hint);
                    if response.clicked() {
                        self.view = view;
                        if view == View::Collection {
                            self.refresh_collection();
                        }
                        if view == View::People {
                            self.refresh_faces();
                        }
                    }
                }
                ui.add_space(30.0);
                ui.separator();
                if ui
                    .selectable_label(self.view == View::Settings, "Einstellungen")
                    .clicked()
                {
                    self.view = View::Settings;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new("Rust · lokal · deine Fotos")
                            .size(11.0)
                            .weak(),
                    );
                    ui.label(
                        RichText::new("ORIGINALE BLEIBEN ERHALTEN")
                            .size(10.0)
                            .color(theme::ACCENT),
                    );
                });
            });
    }
    fn footer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("jobs")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(14, 19, 25))
                    .inner_margin(egui::Margin::symmetric(22, 12)),
            )
            .show(ctx, |ui| {
                if let Some(running) = self
                    .tasks
                    .iter()
                    .find(|t| t.write)
                    .or_else(|| self.tasks.first())
                {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(&running.task.label).strong());
                        let done = running.task.control.done.load(Ordering::Relaxed);
                        let total = running.task.control.total.load(Ordering::Relaxed);
                        if total > 0 {
                            ui.add(
                                egui::ProgressBar::new(done as f32 / total as f32)
                                    .desired_width(240.0)
                                    .text(format!("{done} / {total}")),
                            );
                        } else if done > 0 {
                            ui.label(format!("{done} gefunden"));
                        }
                        if ui.button("Abbrechen").clicked() {
                            if running.kind == "maintenance" {
                                self.maintenance_paused = true;
                            }
                            running.task.control.stop();
                        }
                        ui.label(format!("{} s", running.task.started.elapsed().as_secs()));
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if self.error { "Hinweis" } else { "● Bereit" }).color(
                                if self.error {
                                    theme::WARM
                                } else {
                                    theme::ACCENT
                                },
                            ),
                        );
                        ui.add(egui::Label::new(&self.notice).truncate())
                            .on_hover_text(&self.notice);
                    });
                }
            });
    }
    fn header(ui: &mut egui::Ui, kicker: &str, title: &str, subtitle: &str) {
        theme::eyebrow(ui, kicker);
        ui.heading(title);
        ui.label(RichText::new(subtitle).weak());
        ui.add_space(20.0);
    }
    fn empty(&self, ui: &mut egui::Ui, title: &str, text: &str) {
        ui.add_space(50.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("▧").size(64.0).color(theme::ACCENT));
            ui.add_space(16.0);
            ui.label(RichText::new(title).size(24.0).strong());
            ui.add_space(10.0);
            ui.label(RichText::new(text).weak());
        });
    }
}
impl eframe::App for PhotoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.tasks.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
            for task in &self.tasks {
                task.task.control.stop();
            }
        }
        self.poll(ctx);
        if self.closing && self.tasks.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.sidebar(ctx);
        self.footer(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .inner_margin(24)
                    .fill(ctx.style().visuals.panel_fill),
            )
            .show(ctx, |ui| match self.view {
                View::Import => self.import_view(ui),
                View::Collection => self.collection_view(ui),
                View::People => self.people_view(ui),
                View::Tasks => self.tasks_view(ui),
                View::Settings => self.settings_view(ui),
            });
        self.windows(ctx);
        if let Ok(path) = std::env::var("FOTO_IMPORTER_CAPTURE") {
            ctx.request_repaint_after(Duration::from_millis(100));
            if self.capture_started.elapsed() > Duration::from_secs(5) && !self.capture_requested {
                self.capture_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    if let Err(e) = image::save_buffer_with_format(
                        path.clone(),
                        &bytes,
                        image.width() as u32,
                        image.height() as u32,
                        image::ColorType::Rgba8,
                        image::ImageFormat::Png,
                    ) {
                        self.message(format!("Sichtprüfungsbild: {e}"), true);
                    }
                }
            }
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for task in &self.tasks {
            task.task.control.stop();
        }
        let _ = self.settings.save();
    }
}
fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / 1073741824.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1048576.0)
    }
}
fn face_label(value: &str) -> &str {
    match value {
        "with_faces" => "Mit Gesichtern",
        "none" => "Ohne erkannte Gesichter",
        "unknown" => "Unbekannte Gesichter",
        "unanalysed" => "Noch nicht analysiert",
        _ => "Alle Fotos",
    }
}
fn operator_label(value: &str) -> &str {
    match value {
        "not" => "ist nicht",
        "contains" => "enthält",
        "min" => "mindestens",
        "max" => "höchstens",
        "present" => "vorhanden",
        "empty" => "fehlt",
        _ => "ist",
    }
}
