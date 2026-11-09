mod collection_view;
mod folder_tree;
mod import_view;
mod people_view;
mod progress_view;
mod recognition_view;
mod relationship_view;
mod settings_view;
#[cfg(test)]
mod tests;
mod tree_view;
mod windows;

use super::{theme, thumbnails::Thumbnails};
use crate::{
    config::{self, Settings},
    core::{
        collection::{Collection, CollectionStatus},
        faces::{self, Face, FacePage, Undo},
        files::{self, Scan, SourceFile},
        folders::{FolderPage, FolderTree},
        jobs::{Control, FacePhase, Task},
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
#[derive(Clone, Copy, PartialEq)]
enum CollectionDialog {
    Filters,
    Inventory,
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
    Folders(u64, Vec<(String, usize, FolderPage)>),
    Faces(u64, usize, FacePage),
    Detail(u64, String, Vec<Face>, bool),
    Done(String),
    Imported(files::ImportReport),
    Suggestions(u64, i64, crate::core::recognition::Suggestions),
    Report(String),
    RelationshipGraph(u64, crate::core::relationship_graph::Network),
    PersonAdded(i64),
    RuntimeDownloaded(PathBuf),
    ModelsInstalled(PathBuf, Vec<crate::core::inference::Manifest>),
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
impl Running {
    fn is_face_matching(&self) -> bool {
        matches!(self.kind, "maintenance" | "face-knowledge")
    }
    /// Explains what the running job leaves for later, so a half-finished index is not a surprise.
    fn catch_up_note(&self) -> Option<&'static str> {
        (self.write && self.task.label == "Sammlung indizieren").then_some(
            "Schon eingetragene Dateien sind in der Sammlung sichtbar. Danach folgen als eigene \
             Läufe: Metadaten lesen (Aufnahmedatum, Kamera, Foto-Zuordnung) und die BLAKE3-Prüfung. \
             Ein Abbruch behält alles bereits Eingetragene und markiert nichts als fehlend.",
        )
    }
}
#[derive(Default)]
struct Draft {
    name: String,
    person: Option<i64>,
    excluded: BTreeSet<i64>,
}
struct GroupAdvance {
    slot: usize,
    top: f32,
    following: Vec<i64>,
    ready: bool,
}

impl GroupAdvance {
    fn target(&self, groups: &[Vec<Face>]) -> usize {
        self.following
            .iter()
            .find_map(|id| {
                groups
                    .iter()
                    .position(|group| group.iter().any(|face| face.id == *id))
            })
            .unwrap_or_else(|| self.slot.min(groups.len().saturating_sub(1)))
    }
}
#[derive(Default)]
struct ImportCache {
    revision: u64,
    selected: BTreeSet<String>,
    focused: String,
    selection: std::sync::Arc<Vec<SourceFile>>,
    focus_files: std::sync::Arc<Vec<SourceFile>>,
    counts: BTreeMap<String, usize>,
    bytes: u64,
    days: BTreeMap<String, usize>,
}

pub struct PhotoApp {
    settings: Settings,
    view: View,
    tasks: Vec<Running>,
    history: Vec<String>,
    notice: String,
    error: bool,
    scan: Scan,
    import_cache: ImportCache,
    scan_revision: u64,
    selected: BTreeSet<String>,
    focused: String,
    folder_name: String,
    filters: Search,
    applied: Search,
    query_revision: u64,
    folder_tree: FolderTree,
    /// Revisionsprüfung für Ordnerbaum-Antworten; Wechsel der Sammlung oder ihres Inhalts erhöht sie.
    folder_revision: u64,
    /// Sammlung und Bestand, für die der Baum geladen wurde.
    folder_key: (String, usize, usize),
    /// Zuletzt im Baum aufgeklappter Ordnerfilter.
    folder_revealed: Option<String>,
    page: SearchPage,
    status: CollectionStatus,
    face_revision: u64,
    face_page: FacePage,
    face_mode: String,
    face_index: usize,
    face_row_tops: Vec<(i64, f32)>,
    face_advance: Option<GroupAdvance>,
    drafts: BTreeMap<String, Draft>,
    undo: Vec<Undo>,
    thumbnails: Thumbnails,
    selected_photo: Option<Photo>,
    source_preview: Option<PathBuf>,
    detail: String,
    detail_rows: Vec<(String, String)>,
    detail_faces: Vec<Face>,
    detail_analysis_done: Option<bool>,
    detail_revision: u64,
    large: bool,
    show_frames: bool,
    inspector: bool,
    collection_dialog: Option<CollectionDialog>,
    inspect_people: bool,
    zoom: f32,
    detector: String,
    embedder: String,
    models: Vec<crate::core::inference::Manifest>,
    naming: Option<Face>,
    name_input: String,
    name_person: Option<i64>,
    name_suggestions: Vec<(i64, String, f32)>,
    name_suggestions_by_model: bool,
    name_portraits: BTreeMap<i64, Face>,
    /// Gesicht unter der Maus in diesem Frame (höchstens eines).
    hover_face: Option<Face>,
    relation_from: Option<i64>,
    relation_to: Option<i64>,
    relation_kind: String,
    family_ui: tree_view::FamilyUi,
    person_edit: Option<faces::Person>,
    relationship_graph: relationship_view::RelationshipView,
    report: Option<String>,
    delete_phrase: String,
    move_old: String,
    move_new: String,
    purge_scans: u32,
    purge_confirm: bool,
    dng_status: String,
    closing: bool,
    maintenance_paused: bool,
    recognition_tools_open: bool,
    auto_analysis_pending: bool,
    /// Nach einem Indexlauf folgt der Metadatenlauf, wenn die Automatik an ist.
    auto_metadata_pending: bool,
    /// Zuletzt in der Galerie gezeigter Stand eines laufenden Indexlaufs (Stapelzahl, Zeitpunkt).
    live_refresh: (usize, std::time::Instant),
    last_night: String,
    capture_started: std::time::Instant,
    capture_requested: bool,
    #[cfg(test)]
    plan_bounds: Option<egui::Rect>,
    #[cfg(test)]
    collection_bounds: Option<(egui::Rect, egui::Rect)>,
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
            import_cache: ImportCache::default(),
            scan_revision: 0,
            selected: BTreeSet::new(),
            focused: String::new(),
            folder_name: String::new(),
            filters: Search::default(),
            applied: Search::default(),
            query_revision: 0,
            folder_tree: FolderTree::default(),
            folder_revision: 0,
            folder_key: Default::default(),
            folder_revealed: None,
            page: SearchPage::default(),
            status: CollectionStatus::default(),
            face_revision: 0,
            face_page: FacePage::default(),
            face_mode: "unknown".into(),
            face_index: 0,
            face_row_tops: Vec::new(),
            face_advance: None,
            drafts: BTreeMap::new(),
            undo: Vec::new(),
            thumbnails: Thumbnails::new(cc.egui_ctx.clone()),
            selected_photo: None,
            source_preview: None,
            detail: String::new(),
            detail_rows: Vec::new(),
            detail_faces: Vec::new(),
            detail_analysis_done: None,
            detail_revision: 0,
            large: false,
            show_frames: true,
            inspector: true,
            collection_dialog: None,
            inspect_people: false,
            zoom: 172.0,
            detector: "yunet".into(),
            embedder: "sface".into(),
            models,
            naming: None,
            name_input: String::new(),
            name_person: None,
            name_suggestions: Vec::new(),
            name_suggestions_by_model: false,
            name_portraits: BTreeMap::new(),
            hover_face: None,
            relation_from: None,
            relation_to: None,
            relation_kind: "parent".into(),
            family_ui: Default::default(),
            person_edit: None,
            relationship_graph: Default::default(),
            report: None,
            delete_phrase: String::new(),
            move_old: String::new(),
            move_new: String::new(),
            purge_scans: 0,
            purge_confirm: false,
            dng_status: "Converter-Version wird gelesen …".into(),
            closing: false,
            maintenance_paused: false,
            recognition_tools_open: false,
            auto_analysis_pending: false,
            auto_metadata_pending: false,
            live_refresh: (0, std::time::Instant::now()),
            last_night: String::new(),
            capture_started: std::time::Instant::now(),
            capture_requested: false,
            #[cfg(test)]
            plan_bounds: None,
            #[cfg(test)]
            collection_bounds: None,
        };
        if !app.settings.source.is_empty() {
            app.begin_scan();
        }
        // Nur für Sichtprüfungen: Reiter vor dem ersten Laden wählen.
        if let Ok(mode) = std::env::var("FOTO_IMPORTER_FACE_MODE")
            && [
                "unknown",
                "review",
                "confirmed",
                "ignored",
                "persons",
                "tree",
                "graph",
            ]
            .contains(&mode.as_str())
        {
            app.face_mode = mode;
        }
        // Nur für Sichtprüfungen: Ordnerfilter vor dem ersten Laden setzen.
        if let Ok(folder) = std::env::var("FOTO_IMPORTER_FOLDER") {
            app.filters.folder = Some(folder).filter(|f| !f.is_empty());
        }
        // Nur für Sichtprüfungen: Person im Graph vorauswählen.
        if let Ok(person) = std::env::var("FOTO_IMPORTER_GRAPH_PERSON")
            && let Ok(person) = person.parse()
        {
            app.relationship_graph.selected = Some(person);
        }
        // Nur für Sichtprüfungen: Fokus-Person und Generationen im Stammbaum.
        if let Ok(person) = std::env::var("FOTO_IMPORTER_TREE_PERSON")
            && let Ok(person) = person.parse()
        {
            app.family_ui.focus = Some(person);
        }
        if let Ok(up) = std::env::var("FOTO_IMPORTER_TREE_UP")
            && let Ok(up) = up.parse()
        {
            app.family_ui.up = up;
        }
        if let Ok(down) = std::env::var("FOTO_IMPORTER_TREE_DOWN")
            && let Ok(down) = down.parse()
        {
            app.family_ui.down = down;
        }
        if !app.settings.collection.is_empty() {
            app.refresh_collection();
            app.refresh_faces();
        }
        match std::env::var("FOTO_IMPORTER_VIEW").as_deref() {
            Ok("collection") => app.view = View::Collection,
            Ok("people") => app.view = View::People,
            Ok("tasks") => app.view = View::Tasks,
            Ok("settings") => app.view = View::Settings,
            _ => {}
        }
        // Sichtprüfung: startet z. B. "scan" gleich nach dem Öffnen (docs/validierung.md).
        if let Ok(action) = std::env::var("FOTO_IMPORTER_ACTION") {
            app.collection_action(&action);
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
                    let _foreground = if !write
                        && !matches!(kind, "picker" | "dngupdates" | "runtime-download")
                    {
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
        self.query_collection(self.filters.clone());
    }
    fn query_collection(&mut self, filters: Search) {
        if self.settings.collection.is_empty() {
            return;
        }
        self.query_revision += 1;
        let revision = self.query_revision;
        let root = PathBuf::from(&self.settings.collection);
        self.launch("Sammlung durchsuchen", false, "search", move |control| {
            let c = Collection::open(&root, false)?;
            c.cancel_with(&control);
            let detector = faces::active_detector_key(&c)?.unwrap_or_default();
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
        self.launch(
            "Personen und Gesichter lesen",
            false,
            "faces",
            move |control| {
                let c = Collection::open(&root, false)?;
                c.cancel_with(&control);
                c.db.execute_batch("BEGIN")?;
                let mut index = page;
                let mut snapshot = faces::page(&c, &mode, index)?;
                if index > 0 && snapshot.groups.is_empty() {
                    index -= 1;
                    snapshot = faces::page(&c, &mode, index)?;
                    if index > 0 && snapshot.groups.is_empty() {
                        index = 0;
                        snapshot = faces::page(&c, &mode, index)?;
                    }
                }
                Ok(Outcome::Faces(revision, index, snapshot))
            },
        );
    }
    /// Arbeitsumfang für Analyse und Metadaten: Portion und optional der gewählte Ordner.
    fn work_scope(&self) -> crate::core::scope::Scope {
        crate::core::scope::Scope::new(
            Some(self.settings.work_portion),
            self.settings
                .work_in_folder
                .then(|| self.applied.folder.clone())
                .flatten(),
        )
    }
    fn collection_action(&mut self, action: &str) {
        if self.busy() || self.settings.collection.is_empty() {
            return;
        }
        if action == "resume" {
            self.maintenance_paused = false;
            self.refresh_faces();
            return;
        }
        if [
            "scan",
            "metadata",
            "analyze",
            "confirm_missing",
            "purge",
            "resolve",
        ]
        .contains(&action)
        {
            self.undo.clear();
        }
        let settings = self.settings.clone();
        let scope = self.work_scope();
        let action = action.to_string();
        let detector = self.detector.clone();
        let embedder = self.embedder.clone();
        let old = self.move_old.clone();
        let new = self.move_new.clone();
        let age = self.purge_scans;
        let label = match action.as_str() {
            "scan" => "Sammlung indizieren",
            "confirm_missing" => "Fehlende nach Prüfung markieren",
            "purge" => "Fehlende Indexeinträge entfernen",
            "resolve" => "Verschiebung zuordnen",
            "metadata" => "Metadaten lesen",
            "verify" => "Dateien mit BLAKE3 prüfen",
            "analyze" => "Fotos lokal analysieren",
            "knowledge" => "Referenzen und Namensvorschläge aktualisieren",
            "automatic" | "nightly" => "Automatische Sammlungsanalyse",
            "export" => "Referenzausschnitte exportieren",
            "calibrate" => "Referenzen validieren",
            "delete_biometrics" => "Biometrie bereinigen",
            _ => "Sammlung öffnen",
        };
        let kind = match action.as_str() {
            "analyze" | "automatic" | "nightly" => "face-analysis",
            "knowledge" => "face-knowledge",
            _ => "mutation",
        };
        self.launch(label, true, kind, move |control| {
            let mut c = Collection::open(Path::new(&settings.collection), true)?;
            let message = match action.as_str() {
                "scan" => c.quick_scan(false, &control)?,
                "confirm_missing" => c.quick_scan(true, &control)?,
                "purge" => c.purge_missing(age as i64)?,
                "resolve" => c.resolve_move(&old, &new, &control)?,
                "metadata" => {
                    metadata::enrich_scoped(&mut c, &settings.exiftool, &control, &scope)?
                }
                "automatic" | "nightly" => {
                    if settings.auto_metadata {
                        metadata::enrich_scoped(&mut c, &settings.exiftool, &control, &scope)?;
                    }
                    if action == "automatic" || faces::status(&c)?.nightly {
                        faces::analyze_scoped(
                            &mut c,
                            Path::new(&settings.model_dir),
                            Path::new(&settings.onnx_runtime),
                            &detector,
                            &embedder,
                            &settings.exiftool,
                            &control,
                            &scope,
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
                "analyze" => faces::analyze_scoped(
                    &mut c,
                    Path::new(&settings.model_dir),
                    Path::new(&settings.onnx_runtime),
                    &detector,
                    &embedder,
                    &settings.exiftool,
                    &control,
                    &scope,
                )?,
                "knowledge" => faces::apply_knowledge(&mut c, &control)?,
                _ => "Sammlung geöffnet".into(),
            };
            Ok(Outcome::Done(message))
        });
    }
    fn choose_photo(&mut self, photo: Photo) {
        self.source_preview = None;
        self.detail_revision += 1;
        let revision = self.detail_revision;
        self.detail.clear();
        self.detail_rows.clear();
        self.detail_faces.clear();
        self.detail_analysis_done = None;
        let root = PathBuf::from(&self.settings.collection);
        let id = photo.id;
        self.selected_photo = Some(photo);
        self.launch("Bilddetails lesen", false, "detail", move |control| {
            let c = Collection::open(&root, false)?;
            c.cancel_with(&control);
            c.db.execute_batch("BEGIN")?;
            Ok(Outcome::Detail(
                revision,
                serde_json::to_string_pretty(&metadata::detail(&c, id)?)?,
                faces::photo_faces(&c, id)?,
                faces::photo_analysis_done(&c, id)?,
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
        self.name_portraits.clear();
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
                Err(_) if running.kind == "folders" => {
                    // Never retry a failing or cancelled level in a loop; "Aktualisieren" retries.
                    self.folder_tree.fail();
                }
                Err(e) => {
                    if (running.is_face_matching() || running.kind == "face-analysis")
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
                    if running.write
                        && running.collection == self.settings.collection
                        && !self.closing
                        && running.task.label == "Sammlung indizieren"
                    {
                        // Entered batches of a cancelled or failed index run stay usable.
                        self.query_collection(self.applied.clone());
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
                    self.filters.folder.clone_from(&filters.folder);
                    self.filters.page = filters.page;
                    self.applied = filters;
                    if !self.large
                        && self.selected_photo.as_ref().is_some_and(|selected| {
                            !self.page.photos.iter().any(|photo| photo.id == selected.id)
                        })
                    {
                        self.selected_photo = None;
                        self.detail_revision += 1;
                    }
                }
                Ok(Outcome::Folders(revision, levels)) if revision == self.folder_revision => {
                    self.folder_tree.apply(levels);
                }
                Ok(Outcome::Faces(revision, index, page)) if revision == self.face_revision => {
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
                    self.face_page = page;
                }
                Ok(Outcome::Detail(revision, detail, faces, analysed))
                    if revision == self.detail_revision =>
                {
                    self.detail = detail;
                    self.detail_rows.clear();
                    if let Ok(serde_json::Value::Array(files)) = serde_json::from_str(&self.detail)
                    {
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
                Ok(Outcome::Done(message)) => {
                    if running.kind == "undo" && running.collection == self.settings.collection {
                        self.undo.pop();
                    }
                    self.message(message, false);
                    if running.task.label == "Sammlung indizieren"
                        && self.settings.auto_metadata
                        && running.collection == self.settings.collection
                    {
                        self.auto_metadata_pending = true;
                    }
                    if running.write && running.collection == self.settings.collection {
                        self.relationship_graph.stale = true;
                        self.query_collection(self.applied.clone());
                        self.refresh_faces();
                        if let Some(photo) = self.selected_photo.clone() {
                            self.choose_photo(photo);
                        }
                    }
                }
                Ok(Outcome::PersonAdded(id)) => {
                    self.message("Person angelegt".into(), false);
                    if running.collection == self.settings.collection {
                        self.family_ui.just_added = Some(id);
                        self.relationship_graph.stale = true;
                        self.refresh_faces();
                    }
                }
                Ok(Outcome::Named(undo)) if running.collection == self.settings.collection => {
                    self.naming = None;
                    self.relationship_graph.stale = true;
                    self.prepare_group_advance(&undo);
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
                Ok(Outcome::RelationshipGraph(revision, graph))
                    if running.collection == self.settings.collection
                        && revision == self.relationship_graph.revision =>
                {
                    self.relationship_graph.accept(graph);
                }
                Ok(Outcome::RuntimeDownloaded(path)) => {
                    let previous = std::mem::replace(
                        &mut self.settings.onnx_runtime,
                        path.to_string_lossy().into_owned(),
                    );
                    if self.save() {
                        self.message(
                            "ONNX Runtime ist installiert und ausgewählt. Falls zuvor eine andere Runtime geladen wurde, bitte die App neu starten.".into(),
                            false,
                        );
                    } else {
                        self.settings.onnx_runtime = previous;
                    }
                }
                Ok(Outcome::ModelsInstalled(path, models)) => {
                    let previous = std::mem::replace(
                        &mut self.settings.model_dir,
                        path.to_string_lossy().into_owned(),
                    );
                    if self.save() {
                        self.models = models;
                        self.message(
                            format!(
                                "YuNet und SFace sind installiert. Modellordner: {}",
                                path.display()
                            ),
                            false,
                        );
                    } else {
                        self.settings.model_dir = previous;
                    }
                }
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
                    self.query_collection(self.applied.clone());
                    self.refresh_faces();
                }
                Ok(Outcome::Suggestions(revision, id, suggestions))
                    if revision == self.detail_revision
                        && self.naming.as_ref().is_some_and(|f| f.id == id) =>
                {
                    self.name_suggestions_by_model = suggestions.by_model;
                    self.name_portraits = suggestions.portraits;
                    self.name_suggestions = suggestions.items
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
                            self.folder_tree = FolderTree::default();
                            self.folder_revision += 1;
                            self.folder_revealed = None;
                            self.page = SearchPage::default();
                            self.selected_photo = None;
                            self.face_page = FacePage::default();
                            self.relationship_graph.clear();
                            self.maintenance_paused = false;
                            self.auto_analysis_pending = false;
                            self.face_index = 0;
                            self.face_advance = None;
                            self.face_row_tops.clear();
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
        if !self.closing && !self.busy() && self.auto_metadata_pending {
            // Stage 2 of the index: metadata in portions (ADR 0059) after the quick entry.
            self.auto_metadata_pending = false;
            self.collection_action("metadata");
        }
        self.live_gallery(ctx);
        if !self.closing && !self.busy() && !self.models_installing() && self.auto_analysis_pending
        {
            self.auto_analysis_pending = false;
            self.collection_action("automatic");
        }
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
            && self.face_page.status.pending > 0
            && !self
                .tasks
                .iter()
                .any(|t| matches!(t.kind, "maintenance" | "faces"))
            && !self.settings.collection.is_empty()
        {
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
                theme::dark_panel(ui);
                ui.label(
                    RichText::new("F / I")
                        .size(28.0)
                        .strong()
                        .color(theme::ACCENT),
                );
                ui.label(RichText::new("FOTO-IMPORTER").size(13.0).strong());
                ui.label(
                    RichText::new("Raum für Erinnerungen")
                        .size(theme::MIN_TEXT)
                        .weak(),
                );
                ui.add_space(40.0);
                theme::eyebrow(ui, "ARBEITSBEREICH");
                for (view, title, hint) in [
                    (
                        View::Import,
                        "01  Importieren",
                        "Von der Karte in die Sammlung",
                    ),
                    (View::Collection, "02  Sammlung", "Fotos finden & verstehen"),
                    (View::People, "03  Personen", "Namen & Beziehungen"),
                    (View::Tasks, "04  Aufgaben", "Fortschritt & Verlauf"),
                ] {
                    let response =
                        theme::nav_item(ui, title, self.view == view, 150.0).on_hover_text(hint);
                    if response.clicked() {
                        self.view = view;
                        if view == View::Collection {
                            self.query_collection(self.applied.clone());
                        }
                        if view == View::People {
                            self.refresh_faces();
                        }
                    }
                }
                ui.add_space(30.0);
                ui.separator();
                if theme::nav_item(ui, "Einstellungen", self.view == View::Settings, 150.0)
                    .clicked()
                {
                    self.view = View::Settings;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new("Rust · lokal · deine Fotos")
                            .size(theme::MIN_TEXT)
                            .weak(),
                    );
                    ui.label(
                        RichText::new("Originale bleiben erhalten")
                            .size(theme::MIN_TEXT)
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
                theme::dark_panel(ui);
                if let Some(running) = self
                    .tasks
                    .iter()
                    .find(|t| t.write)
                    .or_else(|| self.tasks.first())
                {
                    let progress = running.task.control.snapshot();
                    let stopping = running.task.control.cancel.load(Ordering::Relaxed);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(&running.task.label).strong());
                        if !progress.phase.is_empty() {
                            ui.label(RichText::new(format!("· {}", progress.phase)).weak());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let can_pause = running.is_face_matching();
                            if ui
                                .add_enabled(
                                    !stopping,
                                    egui::Button::new(if can_pause {
                                        "Pausieren"
                                    } else {
                                        "Abbrechen"
                                    }),
                                )
                                .clicked()
                            {
                                if can_pause {
                                    self.maintenance_paused = true;
                                }
                                running.task.control.stop();
                            }
                            ui.label(progress_view::clock(running.task.started.elapsed()));
                        });
                    });
                    if stopping {
                        ui.label(
                            RichText::new(
                                "Wird abgebrochen … bereits Gespeichertes bleibt erhalten",
                            )
                            .weak(),
                        );
                    } else {
                        Self::progress_bar(ui, &progress, ui.available_width());
                    }
                } else {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if self.error { "Hinweis" } else { "Bereit" }).color(
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
                    if let Some(hint) = self.catch_up_hint() {
                        ui.label(RichText::new(hint).small().weak());
                    }
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
            let (rect, _) = ui.allocate_exact_size(egui::vec2(64.0, 54.0), egui::Sense::hover());
            ui.painter().rect_stroke(
                rect.shrink(4.0),
                8,
                egui::Stroke::new(2.0_f32, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
            ui.painter().circle_stroke(
                rect.center(),
                14.0,
                egui::Stroke::new(2.0_f32, theme::ACCENT),
            );
            ui.add_space(16.0);
            ui.label(RichText::new(title).size(24.0).strong());
            ui.add_space(10.0);
            ui.label(RichText::new(text).weak());
        });
    }
}
impl eframe::App for PhotoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            let size = rect.size();
            if size.x >= 980.0 && size.y >= 680.0 {
                self.settings.window_size = [size.x, size.y];
            }
        }
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
            if self.capture_started.elapsed() > capture_delay() && !self.capture_requested
                || ctx.input(|i| i.key_pressed(egui::Key::F12))
            {
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
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1048576.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}
/// Datum für die Anzeige: `2026:10:25 15:30:00` oder `2026-10-25T15:30:00+01:00`
/// werden zu `25.10.2026 · 15:30:00`. Unbekannte Formate bleiben unverändert;
/// gespeicherte Werte und Suche sind nicht betroffen.
pub(super) fn display_date(raw: &str) -> String {
    let raw = raw.trim();
    let digits = |s: &str, n: usize| {
        (s.len() == n && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.to_string())
    };
    let (date, time) = match raw.split_once([' ', 'T']) {
        Some((date, time)) => (date, Some(time)),
        None => (raw, None),
    };
    let parts: Vec<&str> = date.split([':', '-']).collect();
    let [year, month, day] = parts[..] else {
        return raw.to_string();
    };
    let (Some(year), Some(month), Some(day)) = (digits(year, 4), digits(month, 2), digits(day, 2))
    else {
        return raw.to_string();
    };
    let clock = time
        .map(|t| t.split(['+', 'Z']).next().unwrap_or(t))
        .map(|t| t.split('-').next().unwrap_or(t))
        .map(|t| t.split('.').next().unwrap_or(t))
        .filter(|t| t.len() >= 5 && t.bytes().all(|b| b.is_ascii_digit() || b == b':'));
    match clock {
        Some(clock) => format!("{day}.{month}.{year} · {clock}"),
        None => format!("{day}.{month}.{year}"),
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
/// Wartezeit bis zur Fensteraufnahme (Sichtprüfung), Standard fünf Sekunden;
/// `FOTO_IMPORTER_CAPTURE_DELAY` setzt sie in Sekunden, z. B. wenn Bilder nachladen.
fn capture_delay() -> Duration {
    let seconds = std::env::var("FOTO_IMPORTER_CAPTURE_DELAY")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(5);
    Duration::from_secs(seconds.clamp(1, 300))
}
