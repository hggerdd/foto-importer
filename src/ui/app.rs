mod central_view;
mod collection_view;
mod errors;
mod faces_view;
mod folder_tree;
mod import_queue;
mod import_view;
mod optimistic;
mod people_table;
mod people_view;
mod progress_view;
mod recognition_view;
mod relationship_view;
mod settings_view;
#[cfg(test)]
mod tests;
mod time_view;
mod tree_view;
mod windows;

use super::{select, theme, thumbnails::Thumbnails};
use crate::{
    config::{self, Settings},
    core::{
        collection::{Collection, CollectionStatus},
        faces::{self, Face, FacePage, Undo},
        files::{self, Scan, SourceFile},
        folders::FolderTree,
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Import,
    Collection,
    Faces,
    People,
    Tasks,
    Settings,
}
/// Ansicht der in der Personentabelle gewählten Person.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
enum PersonMode {
    /// Tabelle aller Personen mit den Details der gewählten Person.
    #[default]
    Overview,
    Tree,
    Graph,
    Time,
}
/// Hauptpunkte der Navigation in ihrer Reihenfolge: Ansicht, Titel, Hinweistext.
const NAV_MAIN: [(View, &str, &str); 4] = [
    (
        View::Import,
        "Importieren",
        "Fotos von der Karte in die Sammlung kopieren",
    ),
    (View::Collection, "Sammlung", "Fotos suchen und ansehen"),
    (
        View::Faces,
        "Gesichter",
        "Gesichter benennen und Vorschläge prüfen",
    ),
    (
        View::People,
        "Personen",
        "Personen, Beziehungen, Stammbaum, Graph und Zeitreise",
    ),
];
/// Punkte am unteren Rand der Navigation.
const NAV_BOTTOM: [(View, &str, &str); 2] = [
    (View::Tasks, "Aufgaben", "Laufende Aufträge und Verlauf"),
    (
        View::Settings,
        "Einstellungen",
        "Programme, Modelle, Darstellung",
    ),
];
/// Startort aus den Umgebungsvariablen der Sichtprüfung (`FOTO_IMPORTER_VIEW`,
/// `FOTO_IMPORTER_FACE_MODE`). Werte aus der Zeit vor der Trennung von Gesichtern und
/// Personen gelten weiter: `people` mit einem Gesichtsreiter öffnet „Gesichter“, `time`
/// öffnet die Zeitreise unter „Personen“. Ergebnis: Ansicht, Gesichtsreiter, Personenansicht.
fn startup_place(
    view: Option<&str>,
    mode: Option<&str>,
) -> (Option<View>, Option<&'static str>, Option<PersonMode>) {
    let face_mode = match mode {
        Some("unknown") => Some("unknown"),
        Some("review") => Some("review"),
        Some("confirmed") => Some("confirmed"),
        Some("ignored") => Some("ignored"),
        _ => None,
    };
    let person_mode = match (view, mode) {
        (Some("time"), _) | (_, Some("time")) => Some(PersonMode::Time),
        (_, Some("persons")) => Some(PersonMode::Overview),
        (_, Some("tree")) => Some(PersonMode::Tree),
        (_, Some("graph")) => Some(PersonMode::Graph),
        _ => None,
    };
    let view = match view {
        Some("import") => Some(View::Import),
        Some("collection") => Some(View::Collection),
        Some("faces") => Some(View::Faces),
        Some("persons" | "time") => Some(View::People),
        // Früher lagen die Gesichtsreiter unter „Personen“.
        Some("people") if person_mode.is_some() => Some(View::People),
        Some("people") => Some(View::Faces),
        Some("tasks") => Some(View::Tasks),
        Some("settings") => Some(View::Settings),
        _ => None,
    };
    (view, face_mode, person_mode)
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
    Folders(u64, Vec<(String, usize)>),
    Faces(u64, usize, FacePage),
    Detail(u64, String, Vec<Face>, bool),
    Done(String),
    Imported(files::ImportReport),
    /// Aufnahme neuer Dateien beendet; `false`: im Ordner gibt es keine Sammlung (mehr).
    Indexed(bool),
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
    /// Ergebnis plus Hinweis/Bericht des zentralen Abgleichs (ADR 0062).
    Synced(Box<Outcome>, crate::core::central::SyncNote),
    Pick(Pick, Option<PathBuf>),
    Time(time_view::TimeOutcome),
}
/// Hinweis samt Ansicht, in der er entstand, und Zeitpunkt für das Ablaufen.
struct Notice {
    text: String,
    error: bool,
    /// `None`: ohne sinnvolle Herkunft, in jeder Ansicht sichtbar (z. B. beim Start).
    view: Option<View>,
    set: std::time::Instant,
}
struct Running {
    task: Task<Outcome>,
    write: bool,
    kind: &'static str,
    collection: String,
    /// Ansicht, in der der Auftrag gestartet wurde; Meldungen zum Ergebnis gehören dorthin.
    view: View,
    /// Gesichter, die bis zum Ende dieses Auftrags aus den Seiten ausgeblendet bleiben.
    hides: Vec<i64>,
}
/// Hintergrundarbeit mit niedriger Priorität: zählt nicht als belegt und weicht jedem
/// Nutzerschreibauftrag.
fn low_priority(kind: &str) -> bool {
    matches!(kind, "maintenance" | "metadata-bg" | "index-new")
}
impl Running {
    fn is_face_matching(&self) -> bool {
        matches!(self.kind, "maintenance" | "face-knowledge")
    }
    /// Lässt sich der Auftrag pausieren (statt abbrechen) und später fortsetzen?
    fn can_pause(&self) -> bool {
        self.is_face_matching() || matches!(self.kind, "metadata-bg" | "index-new")
    }
    /// Explains what the running job leaves for later, so a half-finished index is not a surprise.
    fn catch_up_note(&self) -> Option<&'static str> {
        (self.write && self.task.label == "Sammlung indizieren").then_some(
            "Schon eingetragene Dateien sind in der Sammlung sichtbar. Danach folgen als eigene \
             Läufe: Metadaten lesen (Aufnahmedatum, Kamera, Foto-Zuordnung) und die Prüfung der Dateien auf Fehler. \
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
    notice: Option<Notice>,
    /// Während der Ergebnisverarbeitung: Ansicht des Auftrags statt der aktuellen.
    notice_origin: Option<View>,
    scan: Scan,
    import_cache: ImportCache,
    scan_revision: u64,
    selected: BTreeSet<String>,
    focused: String,
    folder_name: String,
    /// Festgelegte Imports dieser Sitzung in Reihenfolge; höchstens einer läuft.
    imports: Vec<import_queue::ImportBatch>,
    import_serial: u64,
    filters: Search,
    applied: Search,
    /// Zuletzt angefragte Suche; `applied` folgt erst mit dem Ergebnis.
    queried: Search,
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
    /// Reiter der Ansicht „Gesichter“: `unknown`, `review`, `confirmed` oder `ignored`.
    face_mode: String,
    /// Zuletzt gewählte Seite im Reiter „Erledigt“ (`confirmed` oder `ignored`).
    done_mode: &'static str,
    /// Ansicht der gewählten Person unter „Personen“.
    person_mode: PersonMode,
    face_index: usize,
    /// Personenfilter im Reiter „Vorschläge prüfen“.
    review_person: Option<i64>,
    /// Die letzte Gruppenentscheidung kam von einer Taste: die nächste Gruppe erhält dann
    /// keinen Fokus im Namensfeld, damit die Tasten weiter gelten.
    face_keys: bool,
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
    /// Gewählter Ordner ohne Sammlung, der erst nach Bestätigung zur neuen Sammlung wird.
    new_collection_prompt: Option<String>,
    inspect_people: bool,
    zoom: f32,
    detector: String,
    embedder: String,
    models: Vec<crate::core::inference::Manifest>,
    /// Gesicht im Dialog „Gesicht benennen“; aus jeder Ansicht zu öffnen.
    naming: Option<i64>,
    naming_revision: u64,
    name_input: String,
    name_person: Option<i64>,
    name_suggestions: Vec<(i64, String, f32)>,
    name_portraits: BTreeMap<i64, Face>,
    /// Gesicht unter der Maus in diesem Frame (höchstens eines).
    hover_face: Option<Face>,
    relation_from: Option<i64>,
    relation_kind: String,
    family_ui: tree_view::FamilyUi,
    central_ui: central_view::CentralUi,
    people_ui: people_table::PeopleUi,
    time_ui: time_view::TimeUi,
    /// Zählt übernommene `face_page`-Stände; Tabellenansicht berechnet sich daran neu.
    face_page_serial: u64,
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
    /// Hintergrundlauf der Metadaten pausiert (Nutzer) bzw. für diese Sitzung gestoppt (Fehler).
    metadata_paused: bool,
    metadata_bg_failed: bool,
    /// Ordner, deren Aufnahme neuer Dateien gerade läuft (`index-new`).
    indexing: Option<String>,
    /// Aufnahme neuer Dateien vom Nutzer pausiert; ein weiterer Import hebt das auf.
    index_paused: bool,
    /// Ordner, deren Aufnahme in dieser Sitzung fehlschlug: kein neuer Versuch bis zum nächsten
    /// Import dorthin oder zum nächsten Start.
    index_failed: BTreeSet<String>,
    /// Am Regler gewählte Vorschlagsschwelle samt Sammlung, für die sie gilt.
    review_draft: Option<(String, f32)>,
    /// Pfadfelder der Einstellungen, deren Text noch nicht übernommen ist (oder ungültig war).
    path_drafts: BTreeMap<settings_view::PathSetting, settings_view::PathDraft>,
    /// „Gesichtserkennung einrichten“ hat die Laufzeit geladen; danach folgen die Modelle.
    setup_chain: bool,
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
            notice: Some(Notice {
                text: "Bereit für deine nächsten Erinnerungen".into(),
                error: false,
                view: None,
                set: std::time::Instant::now(),
            }),
            notice_origin: None,
            scan: Scan::default(),
            import_cache: ImportCache::default(),
            scan_revision: 0,
            selected: BTreeSet::new(),
            focused: String::new(),
            folder_name: String::new(),
            imports: Vec::new(),
            import_serial: 0,
            filters: Search::default(),
            applied: Search::default(),
            queried: Search::default(),
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
            done_mode: "confirmed",
            person_mode: PersonMode::default(),
            face_index: 0,
            review_person: None,
            face_keys: false,
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
            new_collection_prompt: None,
            inspect_people: false,
            zoom: 172.0,
            detector: "yunet".into(),
            embedder: "sface".into(),
            models,
            naming: None,
            naming_revision: 0,
            name_input: String::new(),
            name_person: None,
            name_suggestions: Vec::new(),
            name_portraits: BTreeMap::new(),
            hover_face: None,
            relation_from: None,
            relation_kind: "parent".into(),
            family_ui: Default::default(),
            central_ui: Default::default(),
            people_ui: Default::default(),
            time_ui: time_view::TimeUi::from_env(),
            face_page_serial: 0,
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
            metadata_paused: false,
            metadata_bg_failed: false,
            indexing: None,
            index_paused: false,
            index_failed: BTreeSet::new(),
            review_draft: None,
            path_drafts: BTreeMap::new(),
            setup_chain: false,
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
        // Nur für Sichtprüfungen: Ansicht und Reiter vor dem ersten Laden wählen.
        let (view, face_mode, person_mode) = startup_place(
            std::env::var("FOTO_IMPORTER_VIEW").ok().as_deref(),
            std::env::var("FOTO_IMPORTER_FACE_MODE").ok().as_deref(),
        );
        if let Some(mode) = face_mode {
            app.face_mode = mode.into();
            if matches!(mode, "confirmed" | "ignored") {
                app.done_mode = mode;
            }
        }
        if let Some(mode) = person_mode {
            app.person_mode = mode;
        }
        if let Some(view) = view {
            app.view = view;
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
        // Nur für Sichtprüfungen: Person, Sortierung und offenes Auswahlfeld der Personentabelle.
        if let Ok(person) = std::env::var("FOTO_IMPORTER_PEOPLE_SELECT")
            && let Ok(person) = person.parse()
        {
            app.relation_from = Some(person);
            app.people_ui.scroll_to_person(person);
        }
        app.people_ui.sort_from_env();
        app.people_ui.capture_popup = std::env::var("FOTO_IMPORTER_PEOPLE_POPUP").ok();
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
        // Sichtprüfung: startet z. B. "scan" gleich nach dem Öffnen (docs/validierung.md).
        if let Ok(action) = std::env::var("FOTO_IMPORTER_ACTION") {
            if action == "central_sync" {
                // Sichtprüfung des zentralen Bestands: Bereich öffnen und einmal abgleichen.
                app.people_ui.panel = people_table::Panel::Central;
                app.run_central(crate::core::central::sync);
            } else {
                app.collection_action(&action);
            }
        }
        app
    }
    /// Läuft ein schreibender Auftrag, der diese Sammlung betrifft? Aufträge anderer
    /// Sammlungen blockieren nicht (je Sammlung gibt es einen Schreiber, ADR 0008).
    fn busy(&self) -> bool {
        self.blocking_task().is_some()
    }
    fn blocking_task(&self) -> Option<&Running> {
        self.tasks.iter().find(|t| {
            t.write
                && t.kind != "copy"
                && !low_priority(t.kind)
                && t.collection == self.settings.collection
        })
    }
    /// Erklärt, worauf ein ausgegrauter Knopf wartet.
    fn busy_reason(&self) -> String {
        match self.blocking_task() {
            Some(task) => format!(
                "Gesperrt, solange „{}“ läuft. Der Knopf wird frei, sobald der Auftrag fertig ist (Aufgaben ansehen).",
                task.task.label
            ),
            None => "Gerade nicht verfügbar.".into(),
        }
    }
    fn launch(
        &mut self,
        label: &str,
        write: bool,
        kind: &'static str,
        work: impl FnOnce(Control) -> Result<Outcome> + Send + 'static,
    ) {
        self.launch_hiding(label, write, kind, Vec::new(), work);
    }
    fn launch_hiding(
        &mut self,
        label: &str,
        write: bool,
        kind: &'static str,
        hides: Vec<i64>,
        work: impl FnOnce(Control) -> Result<Outcome> + Send + 'static,
    ) {
        // Auch ein Import lässt Hintergrundschreiber (samt Aufnahme neuer Dateien) sofort
        // weichen; sie setzen nach der Warteschlange fort.
        if write && !low_priority(kind) {
            for task in &self.tasks {
                if low_priority(task.kind) {
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
                    // Nutzeraktionen (Benennen, Rücknahme, Einzeländerung) warten nicht auf
                    // laufende Lesevorgänge: nur lange Hintergrundaufträge geben nach (ADR 0066).
                    background: write && !matches!(kind, "naming" | "undo" | "mutation"),
                    ..Default::default()
                },
                move |control| {
                    // Import und Aufnahme neuer Dateien wählen ihre Spur selbst: die Aufnahme
                    // schreibt in den Zielordner, nicht unbedingt in die offene Sammlung.
                    let _slot = if write && !matches!(kind, "copy" | "index-new") {
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
            view: self.view,
            hides,
        });
    }
    fn message(&mut self, text: String, error: bool) {
        // Fehler: ein verständlicher Satz als Hinweis, der Originaltext im Verlauf.
        let (notice, text) = if error {
            let (sentence, detail) = errors::friendly(&text);
            if sentence == detail {
                (sentence, detail)
            } else {
                (sentence.clone(), format!("{sentence} · Details: {detail}"))
            }
        } else {
            (text.clone(), text)
        };
        self.notice = Some(Notice {
            text: notice,
            error,
            view: Some(self.notice_origin.unwrap_or(self.view)),
            set: std::time::Instant::now(),
        });
        self.history.push(format!(
            "{} · {text}",
            chrono::Local::now().format("%H:%M:%S")
        ));
        if self.history.len() > 100 {
            self.history.remove(0);
        }
    }
    /// Der Hinweis, der in der aktuellen Ansicht gezeigt wird: nur seine eigene Ansicht, normale
    /// Hinweise nur bis zum Ablauf der Anzeigedauer, Fehler bis zum Schließen.
    fn visible_notice(&self, now: std::time::Instant) -> Option<&Notice> {
        self.notice.as_ref().filter(|notice| {
            notice.view.is_none_or(|view| view == self.view)
                && (notice.error
                    || now.saturating_duration_since(notice.set) < config::NOTICE_SECONDS)
        })
    }
    fn dismiss_notice(&mut self) {
        self.notice = None;
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
        self.queried = filters.clone();
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
    /// Liest die zuletzt angefragte Suche neu, z. B. nach einer Änderung am Bestand. Eine noch
    /// laufende Filteränderung geht dabei nicht verloren.
    fn requery_collection(&mut self) {
        self.query_collection(self.queried.clone());
    }
    /// Seite, die `refresh_faces` liest: unter „Personen“ die Personenseite der gewählten
    /// Ansicht, sonst der Reiter der Ansicht „Gesichter“.
    fn page_mode(&self) -> &str {
        if self.view != View::People {
            return &self.face_mode;
        }
        match self.person_mode {
            PersonMode::Overview => "persons",
            PersonMode::Graph => "graph",
            // Stammbaum und Zeitreise brauchen Personen und Familie, aber keine Tabellenzeilen.
            PersonMode::Tree | PersonMode::Time => "tree",
        }
    }
    /// Wechselt die Hauptansicht und liest, was sie zeigt, neu.
    fn open_view(&mut self, view: View) {
        if self.view == View::Settings && view != View::Settings {
            // Ein Pfad, der beim Verlassen noch im Feld steht, gilt wie bei Fokusverlust.
            self.commit_path_drafts();
        }
        self.view = view;
        self.recognition_tools_open = false;
        match view {
            View::Collection => self.requery_collection(),
            View::Faces | View::People => self.refresh_faces(),
            _ => {}
        }
    }
    /// Wählt einen Reiter der Ansicht „Gesichter“ und beginnt auf seiner ersten Seite.
    fn set_face_mode(&mut self, mode: &'static str) {
        if matches!(mode, "confirmed" | "ignored") {
            self.done_mode = mode;
        }
        self.face_mode = mode.into();
        self.face_index = 0;
        self.face_advance = None;
        self.face_row_tops.clear();
        self.refresh_faces();
    }
    /// Reiter „Erledigt“: zeigt die zuletzt gewählte Seite, „Bestätigt“ oder „Ignoriert“.
    fn open_done(&mut self) {
        self.set_face_mode(self.done_mode);
    }
    /// Die gewählte Person, sofern es sie in der geladenen Sammlung gibt.
    fn selected_person(&self) -> Option<i64> {
        self.relation_from
            .filter(|id| self.face_page.people.iter().any(|p| p.id == *id))
    }
    /// Wählt die Ansicht der gewählten Person unter „Personen“. Stammbaum, Graph und
    /// Zeitreise stellen die gewählte Person in den Mittelpunkt.
    fn set_person_mode(&mut self, mode: PersonMode) {
        self.view = View::People;
        self.person_mode = mode;
        if let Some(person) = self.selected_person() {
            match mode {
                PersonMode::Overview => self.people_ui.scroll_to_person(person),
                PersonMode::Tree => self.family_ui.focus = Some(person),
                PersonMode::Graph => self.relationship_graph.show_person(person),
                PersonMode::Time => self.show_time_for(person),
            }
        }
        self.refresh_faces();
    }
    /// Ergebnis der Ordnerwahl für die Sammlung. Importquelle und Speicherkarte werden mit
    /// Hinweis abgelehnt; eine vorhandene Sammlung öffnet sofort, ein Ordner ohne Sammlung
    /// erst nach Bestätigung im Dialog (`new_collection_dialog`). Bis dahin wird nichts
    /// geschrieben.
    fn choose_collection(&mut self, folder: String) {
        let path = Path::new(&folder);
        let source = Path::new(&self.settings.source);
        if let Some(problem) =
            crate::core::collection::collection_folder_problem(path, Some(source))
        {
            self.message(problem, true);
            return;
        }
        if crate::core::collection::exists_at(path) {
            self.set_collection(folder);
        } else {
            self.new_collection_prompt = Some(folder);
        }
    }
    /// Rückfrage, bevor in einem Ordner ohne Sammlung eine neue entsteht.
    fn new_collection_dialog(&mut self, ctx: &egui::Context) {
        let Some(folder) = self.new_collection_prompt.as_deref() else {
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;
        let response = egui::Modal::new(egui::Id::new("new-collection")).show(ctx, |ui| {
            ui.set_width(520.0_f32.min(ctx.content_rect().width() - 64.0));
            ui.heading("Hier eine neue Sammlung anlegen?");
            ui.add_space(6.0);
            ui.label(RichText::new(import_view::display_path(folder).as_ref()).strong());
            ui.add_space(6.0);
            ui.label(
                "In diesem Ordner gibt es noch keine Sammlung. Die App legt hier ihre \
                 Datenbankdateien (.myfotomanager und Begleitdateien) an und liest die Fotos \
                 ein. Die Fotos selbst werden nicht verändert.",
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                confirmed = ui.button("Neue Sammlung anlegen").clicked();
                cancelled = ui.button("Abbrechen").clicked();
            });
        });
        if confirmed {
            self.confirm_new_collection();
        } else if cancelled || response.should_close() {
            self.new_collection_prompt = None;
        }
    }
    /// „Neue Sammlung anlegen“: erst jetzt wird gewechselt und indiziert.
    fn confirm_new_collection(&mut self) {
        if let Some(folder) = self.new_collection_prompt.take() {
            self.set_collection(folder);
            self.save();
        }
    }
    /// Stellt die App auf eine Sammlung um. Alles, was nur für die bisherige Sammlung galt
    /// (Entwürfe, Filter, Seiten, Auswahl, Rücknahmen), endet hier; danach wird neu gelesen
    /// oder, bei einem Ordner ohne Bestand, indiziert.
    fn set_collection(&mut self, path: String) {
        self.settings.collection = path;
        self.query_revision += 1;
        self.face_revision += 1;
        self.detail_revision += 1;
        self.folder_tree = FolderTree::default();
        self.folder_revision += 1;
        self.folder_revealed = None;
        self.filters = Search::default();
        self.applied = Search::default();
        self.queried = Search::default();
        self.page = SearchPage::default();
        self.status = CollectionStatus::default();
        self.selected_photo = None;
        self.large = false;
        self.collection_dialog = None;
        self.detail.clear();
        self.detail_rows.clear();
        self.detail_faces.clear();
        self.detail_analysis_done = None;
        self.naming = None;
        self.face_page = FacePage::default();
        self.face_page_serial += 1;
        self.face_index = 0;
        self.review_person = None;
        self.face_advance = None;
        self.face_row_tops.clear();
        self.undo.clear();
        self.drafts.clear();
        self.relation_from = None;
        self.family_ui = Default::default();
        self.people_ui.reset_for_collection();
        self.central_ui = Default::default();
        self.time_ui = Default::default();
        self.relationship_graph.clear();
        self.recognition_tools_open = false;
        self.delete_phrase.clear();
        self.maintenance_paused = false;
        self.metadata_paused = false;
        self.auto_analysis_pending = false;
        self.auto_metadata_pending = false;
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
    fn refresh_faces(&mut self) {
        if self.settings.collection.is_empty() {
            return;
        }
        self.face_revision += 1;
        let revision = self.face_revision;
        let root = PathBuf::from(&self.settings.collection);
        let mode = self.page_mode().to_string();
        let page = self.face_index;
        let person = self.review_person.filter(|_| mode == "review");
        self.launch(
            "Personen und Gesichter lesen",
            false,
            "faces",
            move |control| {
                let c = Collection::open(&root, false)?;
                c.cancel_with(&control);
                c.db.execute_batch("BEGIN")?;
                let mut index = page;
                let mut snapshot = faces::page_filtered(&c, &mode, index, person)?;
                if index > 0 && snapshot.groups.is_empty() {
                    index -= 1;
                    snapshot = faces::page_filtered(&c, &mode, index, person)?;
                    if index > 0 && snapshot.groups.is_empty() {
                        index = 0;
                        snapshot = faces::page_filtered(&c, &mode, index, person)?;
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
    /// Startet den Hintergrundlauf der Metadaten, solange ungelesene Dateien da sind und
    /// nichts anderes schreibt. Er hat niedrige Priorität: Nutzeraufträge stoppen ihn, danach
    /// setzt er fort. Höchstens ein Hintergrundschreiber je Sammlung (erst Metadaten, dann
    /// Gesichter). Fehlgeschlagene Dateien werden nicht erneut versucht.
    fn start_metadata_background(&mut self) {
        if self.closing
            || self.metadata_paused
            || self.metadata_bg_failed
            || self.busy()
            || self.imports_pending() > 0
            || self.settings.collection.is_empty()
            || self.settings.exiftool.trim().is_empty()
            || self.status.unread == 0
            || self.index_due()
            || self.tasks.iter().any(|t| low_priority(t.kind))
        {
            return;
        }
        let root = self.settings.collection.clone();
        let exiftool = self.settings.exiftool.clone();
        self.launch(
            "Metadaten im Hintergrund lesen",
            true,
            "metadata-bg",
            move |control| {
                let mut c = Collection::open(Path::new(&root), true)?;
                Ok(Outcome::Done(metadata::enrich_background(
                    &mut c, &exiftool, &control,
                )?))
            },
        );
    }
    /// Liest die Metadaten der Dateien des gewählten Fotos jetzt (Bilddetails).
    fn read_photo_metadata(&mut self) {
        let Some(photo) = self.selected_photo.as_ref().map(|p| p.id) else {
            return;
        };
        if self.busy() || self.settings.collection.is_empty() {
            return;
        }
        let root = self.settings.collection.clone();
        let exiftool = self.settings.exiftool.clone();
        self.launch("Metadaten lesen", true, "mutation", move |control| {
            let mut c = Collection::open(Path::new(&root), true)?;
            Ok(Outcome::Done(metadata::enrich_photo(
                &mut c, &exiftool, &control, photo,
            )?))
        });
    }
    fn collection_action(&mut self, action: &str) {
        if self.busy() || self.settings.collection.is_empty() {
            return;
        }
        if action == "resume" {
            self.maintenance_paused = false;
            self.metadata_paused = false;
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
        let wipe_central = self.central_ui.wipe_references;
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
            "verify" => "Dateien auf Fehler prüfen",
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
                "delete_biometrics" => {
                    let mut message = crate::core::maintenance::delete_biometrics(&c)?;
                    if wipe_central {
                        let removed = crate::core::central::wipe_references(&settings.central())?;
                        message.push_str(&format!(" Zentrale Referenzbilder entfernt: {removed}."));
                    }
                    message
                }
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
    /// Öffnet den Dialog für genau dieses Gesicht. Er hängt an keiner Ansicht; Vorschläge
    /// liest ein Worker, gültig nur für den zuletzt geöffneten Dialog.
    fn open_naming(&mut self, id: i64) {
        let root = self.settings.collection.clone();
        self.naming_revision += 1;
        let revision = self.naming_revision;
        self.naming = Some(id);
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
            self.notice_origin = Some(running.view);
            // Die Einrichtung geht nur nach einer erfolgreich geladenen Laufzeit weiter.
            let chained =
                running.kind == "runtime-download" && std::mem::take(&mut self.setup_chain);
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
            let label = if running.kind == "copy" {
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
                Err(e) if running.kind == "index-new" => {
                    let cancelled = running.task.control.cancel.load(Ordering::Relaxed);
                    self.index_failed_or_paused(e, cancelled);
                }
                Ok(Outcome::Indexed(found)) => self.index_finished(found),
                Err(e) if running.kind == "metadata-bg" => {
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
                Err(e) if running.kind == "timeline" => {
                    if !running.task.control.cancel.load(Ordering::Relaxed) {
                        self.time_ui.failed(format!("{e:#}"));
                    }
                }
                Ok(Outcome::Time(outcome)) => self.accept_time(outcome),
                Err(_) if running.kind == "folders" => {
                    // Never retry a failing or cancelled level in a loop; "Aktualisieren" retries.
                    self.folder_tree.fail();
                }
                Err(e) => {
                    // Misslungene Benennung: die Gesichter erscheinen beim Neulesen wieder.
                    if running.kind == "naming"
                        && running.collection == self.settings.collection
                        && !self.closing
                        && !self.naming_pending()
                    {
                        self.refresh_faces();
                    }
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
                Ok(Outcome::Scan(revision, scan)) if revision == self.scan_revision => {
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
                Ok(Outcome::Search(revision, status, page, filters))
                    if revision == self.query_revision =>
                {
                    self.status = status;
                    self.page = page;
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
                Ok(Outcome::Folders(revision, levels)) if revision == self.folder_revision => {
                    self.folder_tree.apply(&levels);
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
                    // Noch nicht geschriebene Entscheidungen bleiben ausgeblendet (ADR 0068).
                    self.hide_pending_faces();
                    self.face_page_serial += 1;
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
                    let background = running.kind == "metadata-bg";
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
                        if running.kind == "undo" {
                            self.time_ui.need_load = true;
                        }
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
                Ok(Outcome::PersonAdded(id)) => {
                    self.message("Person angelegt".into(), false);
                    if running.collection == self.settings.collection {
                        self.family_ui.just_added = Some(id);
                        self.select_person(id);
                        self.people_ui.scroll_to_person(id);
                        self.relationship_graph.stale = true;
                        self.refresh_faces();
                    }
                }
                Ok(Outcome::Named(undo)) if running.collection == self.settings.collection => {
                    self.relationship_graph.stale = true;
                    // Die Zeitreise zeigt bestätigte Gesichter; beim nächsten Zeigen neu lesen.
                    self.time_ui.need_load = true;
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
                Ok(Outcome::ModelsInstalled(path, models)) => {
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
                Ok(Outcome::Suggestions(revision, id, suggestions))
                    if revision == self.naming_revision && self.naming == Some(id) =>
                {
                    self.name_portraits = suggestions.portraits;
                    self.name_suggestions = suggestions.items
                }
                Ok(Outcome::Pick(pick, Some(path))) => {
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
                _ => {}
            }
        }
        self.notice_origin = None;
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
        self.start_metadata_background();
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
            && !self
                .tasks
                .iter()
                .any(|t| matches!(t.kind, "maintenance" | "metadata-bg" | "faces"))
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
                ui.add_space(18.0);
                self.sidebar_collection(ui);
                ui.add_space(18.0);
                theme::eyebrow(ui, "ARBEITSBEREICH");
                for (view, title, hint) in NAV_MAIN {
                    if theme::nav_item(ui, title, self.view == view, 150.0)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        self.open_view(view);
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    // Gebauter Stand, damit bei Fehlern ein Versionsproblem auszuschließen ist.
                    ui.label(RichText::new(build_version()).size(theme::MIN_TEXT).weak())
                        .on_hover_text(
                            "Git-Branch und Commit dieses Builds; „+“ = ungesicherte Änderungen",
                        );
                    ui.label(
                        RichText::new("Originale bleiben erhalten")
                            .size(theme::MIN_TEXT)
                            .color(theme::ACCENT),
                    );
                    ui.add_space(8.0);
                    // Von unten nach oben: der letzte Eintrag steht zuerst.
                    for (view, title, hint) in NAV_BOTTOM.into_iter().rev() {
                        if theme::nav_item(ui, title, self.view == view, 150.0)
                            .on_hover_text(hint)
                            .clicked()
                        {
                            self.open_view(view);
                        }
                    }
                    ui.separator();
                });
            });
    }
    /// Die aktive Sammlung in der Seitenleiste: Ordnername, voller Pfad im Hinweistext und
    /// der einzige Knopf der App, der die Sammlung wechselt.
    fn sidebar_collection(&mut self, ui: &mut egui::Ui) {
        theme::eyebrow(ui, "SAMMLUNG");
        let (name, path) = collection_caption(&self.settings.collection);
        match name {
            Some(name) => {
                ui.add(egui::Label::new(RichText::new(name).strong()).truncate())
                    .on_hover_text(path);
            }
            None => {
                ui.label(RichText::new("Keine Sammlung gewählt").weak());
            }
        }
        let empty = self.settings.collection.is_empty();
        if ui
            .button(if empty {
                "Sammlung wählen …"
            } else {
                "Sammlung wechseln …"
            })
            .on_hover_text(if empty {
                "Den Ordner wählen, in dem deine Fotos dauerhaft liegen (nicht die Speicherkarte)"
            } else {
                "Einen anderen Ordner als Sammlung öffnen; ohne Sammlung dort wird vorher gefragt"
            })
            .clicked()
        {
            self.pick(Pick::Collection);
        }
        self.new_collection_dialog(ui.ctx());
    }
    /// Der Auftrag, den die Fußleiste zeigt. In der Aufgabenansicht stehen die laufenden
    /// Aufträge schon als Karten da.
    fn footer_task(view: View, tasks: &[Running]) -> Option<&Running> {
        if view == View::Tasks {
            return None;
        }
        tasks.iter().find(|t| t.write).or_else(|| tasks.first())
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
                if let Some(running) = Self::footer_task(self.view, &self.tasks) {
                    let progress = running.task.control.snapshot();
                    let stopping = running.task.control.cancel.load(Ordering::Relaxed);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(&running.task.label).strong());
                        if !progress.phase.is_empty() {
                            ui.label(RichText::new(format!("· {}", progress.phase)).weak());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let can_pause = running.can_pause();
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
                                    match running.kind {
                                        "metadata-bg" => self.metadata_paused = true,
                                        "index-new" => self.index_paused = true,
                                        _ => self.maintenance_paused = true,
                                    }
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
                    let now = std::time::Instant::now();
                    let shown = self
                        .visible_notice(now)
                        .map(|n| (n.text.clone(), n.error, n.set));
                    let mut dismissed = false;
                    ui.horizontal(|ui| {
                        let error = shown.as_ref().is_some_and(|n| n.1);
                        ui.label(
                            RichText::new(if error { "Hinweis" } else { "Bereit" })
                                .color(if error { theme::WARM } else { theme::ACCENT }),
                        );
                        if let Some((text, error, set)) = &shown {
                            if *error {
                                if ui.small_button("✕").on_hover_text("Schließen").clicked() {
                                    dismissed = true;
                                }
                            } else {
                                ctx.request_repaint_after(
                                    config::NOTICE_SECONDS.saturating_sub(now.duration_since(*set)),
                                );
                            }
                            ui.add(egui::Label::new(text).truncate())
                                .on_hover_text(text);
                        }
                    });
                    if dismissed {
                        self.dismiss_notice();
                    }
                    if self.view != View::Tasks {
                        self.catch_up_row(ui);
                    }
                }
            });
    }
    fn header(ui: &mut egui::Ui, title: &str) {
        ui.label(RichText::new(title).size(23.0).strong());
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
                View::Faces => self.faces_view(ui),
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
/// Branch und Commit dieses Builds für die Seitenleiste, z. B. „main · 1af9a4e“ (aus `build.rs`).
fn build_version() -> String {
    format!(
        "{} · {}",
        env!("FOTO_IMPORTER_GIT_BRANCH"),
        env!("FOTO_IMPORTER_GIT_COMMIT")
    )
}

/// Name und voller Pfad der Sammlung für die Seitenleiste: der Ordnername (bei einem
/// Laufwerk der ganze Pfad) und der Pfad in der Schreibweise von `display_path`. Ohne
/// Sammlung gibt es keinen Namen.
fn collection_caption(collection: &str) -> (Option<String>, String) {
    let path = import_view::display_path(collection).into_owned();
    if path.is_empty() {
        return (None, path);
    }
    let name = Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(&path)
        .to_string();
    (Some(name), path)
}
/// „1 Datei“ / „2 Dateien“: Einzahl nur bei genau eins.
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// „1 Datei an 1 Tag“, „30 Dateien an 2 Tagen“.
fn files_and_days(files: usize, days: usize) -> String {
    format!(
        "{} an {}",
        plural(files, "Datei", "Dateien"),
        plural(days, "Tag", "Tagen")
    )
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
