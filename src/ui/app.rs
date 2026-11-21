mod backup_view;
mod central_view;
mod collection_view;
mod errors;
mod faces_view;
mod folder_tree;
mod import_queue;
mod import_view;
mod missing_view;
mod optimistic;
mod people_table;
mod people_view;
mod person_faces_view;
mod progress_view;
mod recognition_view;
mod relationship_view;
mod settings_view;
#[cfg(test)]
mod tests;
mod time_view;
mod tree_view;
mod windows;

use super::{frame_meter::FrameMeter, select, theme, thumbnails::Thumbnails};
use crate::{
    config::{self, Settings},
    core::{
        collection::{Collection, CollectionStatus},
        diag,
        faces::{self, Face, FacePage, Undo},
        files::{self, Scan, SourceFile},
        folders::FolderTree,
        jobs::{Control, FacePhase, Task},
        metadata::{self, Photo, Rule, Search, SearchPage},
        previews::PreviewRequest,
        start_view::{self, Counts, Known, Stamp},
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
    /// Alle Gesichter der Person: bestätigte und wahrscheinliche (`person_faces_view`).
    Faces,
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
        "Personen, Beziehungen, Stammbaum, Graph, Zeitreise und Gesichter einer Person",
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
        Some("unsuitable") => Some("unsuitable"),
        _ => None,
    };
    let person_mode = match (view, mode) {
        (Some("time"), _) | (_, Some("time")) => Some(PersonMode::Time),
        (_, Some("person-faces")) => Some(PersonMode::Faces),
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
    /// Fehlende Dateien je Ordner (`missing_view`).
    Missing,
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
    /// Die Quelle war beim Start nicht erreichbar (Revision des Einlesens, Pfad).
    SourceOffline(u64, String),
    /// Fehlende Dateien je Ordner (Revision der Ansicht).
    Missing(u64, Vec<crate::core::missing::MissingFolder>),
    /// Vorhandene Datensicherungen der Sammlung (Revision des Lesens).
    Backups(
        u64,
        std::result::Result<Vec<crate::core::backup::BackupFile>, String>,
    ),
    /// Neue Datensicherung erstellt (Hinweistext).
    BackupCreated(String),
    /// Tägliche Datensicherung fertig: Sammlung, Datum, Zusammenfassung.
    DailyBackup(String, String, String),
    /// Frisch gelesene Seite einer Suche mit Stand; Zählungen nur, wenn eine bekannte Zählung
    /// für diesen Stand gilt (sonst zählt danach `Counts`).
    Search(u64, Option<Stamp>, Vec<Photo>, Option<Counts>, Search),
    /// Zählungen zur Suche derselben Revision samt Stand und, auf der ersten Seite, den
    /// Fotos, gegen die sie geprüft wurden.
    Counts(u64, Option<Stamp>, Counts, Search, Option<Vec<i64>>),
    /// Ordnerübersicht eines Lesens; ein Fehler wird mitgeliefert, damit er dem Lesen zuzuordnen ist.
    Folders(u64, std::result::Result<Vec<(String, usize)>, String>),
    Faces(u64, usize, FacePage),
    Detail(u64, String, Vec<Face>, bool),
    Done(String),
    Imported(files::ImportReport),
    /// Aufnahme neuer Dateien beendet; `false`: im Ordner gibt es keine Sammlung (mehr).
    Indexed(bool),
    Suggestions(u64, i64, crate::core::recognition::Suggestions),
    Report(String),
    /// Revision, Fingerabdruck und neu berechneter Graph (`None`: unverändert).
    RelationshipGraph(
        u64,
        String,
        Option<crate::core::relationship_graph::Network>,
    ),
    /// Zusätzliche Angaben der Personendetails (Revision der Personenseite).
    PersonInsights(u64, crate::core::person_insights::Insights),
    /// Rückfragen „Möglicherweise dieselbe Person“ (Revision der Personenseite).
    Duplicates(u64, Vec<crate::core::duplicates::Candidate>),
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
    /// Nummer des zuletzt gestarteten Ordnerbaum-Lesens (jeder Start erhöht sie).
    folder_revision: u64,
    /// Lesen, dessen Antwort der aktuelle Baum erwartet; ein Sammlungswechsel verwirft es.
    folder_pending: Option<u64>,
    /// Sammlung und Bestand, für die der Baum geladen wurde.
    folder_key: (String, usize, usize),
    /// Zuletzt im Baum aufgeklappter Ordnerfilter.
    folder_revealed: Option<String>,
    page: SearchPage,
    status: CollectionStatus,
    /// Gehören Gesamtzahl, Kameras und Bestand zur gezeigten Suche und zum aktuellen Stand der
    /// Sammlung? Sonst zeigt die Kopfzeile „wird aktualisiert …“ und der Hintergrundlauf der
    /// Metadaten wartet.
    counts_fresh: bool,
    /// Die Seite stammt aus der gespeicherten Startansicht und wird gerade neu gelesen.
    page_from_snapshot: bool,
    /// Zuletzt gezählte Suche mit Stand; gilt weiter, solange der Stand gleich bleibt.
    known_counts: Option<Known>,
    /// Lokaler Cache-Ordner (Startansicht, Vorschauen).
    cache_dir: PathBuf,
    /// Beim Start liest „Personen und Gesichter lesen“ erst nach Seite und Zählung der
    /// Sammlung, damit beide nicht um die Sammlungsplatte konkurrieren.
    faces_after_search: bool,
    face_revision: u64,
    face_page: FacePage,
    /// Reiter der Ansicht „Gesichter“: `unknown`, `review`, `confirmed`, `ignored` oder
    /// `unsuitable`.
    face_mode: String,
    /// Zuletzt gewählte Seite im Reiter „Erledigt“ (`confirmed` oder `ignored`).
    done_mode: &'static str,
    /// Ansicht der gewählten Person unter „Personen“.
    person_mode: PersonMode,
    /// Seite der Ansicht „Gesichter“ unter „Personen“: `probable` oder `person-confirmed`.
    person_face_mode: &'static str,
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
    dng_status: String,
    /// Importquelle, die beim Start nicht erreichbar war (z. B. Karte nicht eingesteckt).
    source_offline: Option<String>,
    /// Ansicht „Fehlende Dateien“: gelesener Stand, Revision und Auswahl.
    missing: missing_view::MissingUi,
    /// Abschnitt „Datensicherung“ der Einstellungen.
    backups: backup_view::BackupUi,
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
    /// Die geöffnete Sammlung wartet auf die Prüfung der täglichen Datensicherung (Start oder
    /// Wechsel der Sammlung); sie folgt, sobald Startseite und Zählung geladen sind.
    daily_backup_due: bool,
    capture_started: std::time::Instant,
    capture_requested: bool,
    /// Frame-Zeiten fürs Diagnose-Log.
    frame_meter: FrameMeter,
    #[cfg(test)]
    plan_bounds: Option<egui::Rect>,
    #[cfg(test)]
    collection_bounds: Option<(egui::Rect, egui::Rect)>,
}
impl PhotoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let _t = diag::span("start", "app_aufbau");
        let mut app = Self::with_settings(cc, Settings::load());
        app.check_dng();
        app
    }
    fn with_settings(cc: &eframe::CreationContext<'_>, settings: Settings) -> Self {
        theme::apply(&cc.egui_ctx, settings.dark);
        let models = crate::core::inference::manifests(Path::new(&settings.model_dir))
            .inspect_err(|e| {
                log::warn!(target: "start", "modellbeschreibungen fehler={:?}", format!("{e:#}"));
            })
            .unwrap_or_default();
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
            folder_pending: None,
            folder_key: Default::default(),
            folder_revealed: None,
            page: SearchPage::default(),
            status: CollectionStatus::default(),
            counts_fresh: false,
            page_from_snapshot: false,
            known_counts: None,
            cache_dir: Settings::cache_dir(),
            faces_after_search: false,
            face_revision: 0,
            face_page: FacePage::default(),
            face_mode: "unknown".into(),
            done_mode: "confirmed",
            person_mode: PersonMode::default(),
            person_face_mode: "probable",
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
            dng_status: "Converter-Version wird gelesen …".into(),
            source_offline: None,
            missing: Default::default(),
            backups: Default::default(),
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
            daily_backup_due: false,
            capture_started: std::time::Instant::now(),
            capture_requested: false,
            frame_meter: FrameMeter::default(),
            #[cfg(test)]
            plan_bounds: None,
            #[cfg(test)]
            collection_bounds: None,
        };
        app.configure_backup();
        if !app.settings.source.is_empty() {
            // Eine nicht eingesteckte Karte ist beim Start kein Fehler (`source_offline`).
            app.begin_scan_with(true);
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
        crate::core::thumb_cache::configure(Some(app.cache_dir.join("vorschau")));
        if !app.settings.collection.is_empty() {
            app.show_start_view();
            app.refresh_collection();
            // Personen und Gesichter erst nach der Sammlung (siehe `faces_after_search`).
            app.faces_after_search = true;
            app.daily_backup_due = true;
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
        log::info!(
            target: "job",
            "start art={kind} name={label:?} schreibend={write} laufend={}",
            self.tasks.len()
        );
        let writer_root = self.settings.collection.clone();
        self.tasks.push(Running {
            task: Task::start_control(
                label,
                Control {
                    // Nutzeraktionen (Benennen, Rücknahme, Einzeländerung) warten nicht auf
                    // laufende Lesevorgänge: nur lange Hintergrundaufträge geben nach (ADR 0066).
                    // Die tägliche Sicherung schreibt nicht in die Sammlung, gibt aber ebenso nach.
                    background: write && !matches!(kind, "naming" | "undo" | "mutation")
                        || kind == "backup-daily",
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
                        && !matches!(
                            kind,
                            "picker" | "dngupdates" | "runtime-download" | "backup-daily"
                        ) {
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
        let view = self.notice_origin.unwrap_or(self.view);
        if error {
            log::warn!(target: "hinweis", "fehler ansicht={view:?} text={text:?}");
        } else {
            log::info!(target: "hinweis", "ansicht={view:?} text={text:?}");
        }
        self.notice = Some(Notice {
            text: notice,
            error,
            view: Some(view),
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
        self.configure_backup();
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
    /// Liest die Quelle ausdrücklich (Nutzer): Ist sie nicht erreichbar, ist das ein Fehler.
    fn begin_scan(&mut self) {
        self.begin_scan_with(false);
    }
    /// `quiet`: beim Start. Eine nicht erreichbare Quelle (Wechseldatenträger nicht verbunden)
    /// ergibt dann keinen Fehlerhinweis, sondern den ruhigen Zustand „Quelle nicht verbunden“.
    fn begin_scan_with(&mut self, quiet: bool) {
        self.source_offline = None;
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
            if quiet && !root.is_dir() {
                return Ok(Outcome::SourceOffline(
                    revision,
                    root.to_string_lossy().into_owned(),
                ));
            }
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
        // Eine laufende Zählung gehört zur alten Suche.
        for task in &self.tasks {
            if task.kind == "counts" {
                task.task.control.stop();
            }
        }
        let root = PathBuf::from(&self.settings.collection);
        let known = self.known_counts.clone();
        self.launch("Sammlung durchsuchen", false, "search", move |control| {
            let c = Collection::open(&root, false)?;
            c.cancel_with(&control);
            let detector = faces::active_detector_key(&c)?.unwrap_or_default();
            // Erst die Seite (kalt auf der USB-Platte rund 1 s); die Zählungen lesen alle
            // Treffer und kommen danach, außer eine bekannte Zählung gilt noch.
            control.phase("seite", "");
            let (stamp, photos) = start_view::page(&c, &filters, &detector)?;
            let counts = known
                .filter(|known| known.reusable(stamp.as_ref(), &filters, &photos))
                .map(|known| known.counts);
            Ok(Outcome::Search(revision, stamp, photos, counts, filters))
        });
    }
    /// Zählt die gezeigte Suche (Gesamtzahl, Kameras, Bestand) auf einem Worker. Die erste
    /// Seite ohne Filter wird mit den Zählungen als Startansicht lokal gespeichert.
    fn count_collection(&mut self, filters: Search, photos: Vec<Photo>) {
        let revision = self.query_revision;
        let root = PathBuf::from(&self.settings.collection);
        let collection = self.settings.collection.clone();
        let cache = self.cache_dir.clone();
        self.launch("Sammlung zählen", false, "counts", move |control| {
            let c = Collection::open(&root, false)?;
            c.cancel_with(&control);
            let detector = faces::active_detector_key(&c)?.unwrap_or_default();
            control.phase("zaehlen", "");
            let (stamp, counts) = start_view::count(&c, &filters, &detector)?;
            let first_page =
                (filters.page == 0).then(|| photos.iter().map(|photo| photo.id).collect());
            if let Some(stamp) = &stamp {
                let mut start = filters.clone();
                start.page = 0;
                let known = Known {
                    stamp: stamp.clone(),
                    filters: start,
                    counts: counts.clone(),
                    first_page: first_page.clone(),
                };
                if filters.page == 0
                    && let Some(snapshot) = start_view::Snapshot::new(&collection, known, photos)
                {
                    start_view::store(&cache, &snapshot);
                }
            }
            Ok(Outcome::Counts(
                revision, stamp, counts, filters, first_page,
            ))
        });
    }
    /// Zeigt die gespeicherte Startansicht der Sammlung sofort, bis die frisch gelesene Seite
    /// kommt. Ihre Zählungen gelten erst, wenn der Stand der Sammlung noch derselbe ist.
    fn show_start_view(&mut self) {
        let started = std::time::Instant::now();
        let Some(snapshot) = start_view::load(&self.cache_dir, &self.settings.collection) else {
            return;
        };
        let counts = snapshot.known.counts.clone();
        self.page = SearchPage {
            photos: snapshot.photos,
            total: counts.total,
            cameras: counts.cameras,
        };
        self.status = counts.status;
        self.known_counts = Some(snapshot.known);
        self.page_from_snapshot = true;
        self.counts_fresh = false;
        log::info!(
            target: "sammlung",
            "startansicht gezeigt fotos={} dauer_ms={:.1}",
            self.page.photos.len(),
            diag::millis(started.elapsed())
        );
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
            PersonMode::Faces => self.person_face_mode,
        }
    }
    /// Person, auf die sich die gelesene Seite beschränkt: der Filter in „Vorschläge prüfen“,
    /// auf der Gesichterseite einer Person die gewählte Person.
    fn page_person(&self) -> Option<i64> {
        match self.page_mode() {
            "review" => self.review_person,
            "probable" | "person-confirmed" => self.relation_from,
            _ => None,
        }
    }
    /// Zeigt die Ansicht Gesichtsgruppen (Gesichtsreiter oder Gesichterseite einer Person)?
    fn shows_face_groups(&self) -> bool {
        self.view == View::Faces
            || self.view == View::People && self.person_mode == PersonMode::Faces
    }
    /// Wechselt die Hauptansicht und liest, was sie zeigt, neu.
    fn open_view(&mut self, view: View) {
        if self.view == View::Settings && view != View::Settings {
            // Ein Pfad, der beim Verlassen noch im Feld steht, gilt wie bei Fokusverlust.
            self.commit_path_drafts();
        }
        if view != self.view {
            // Gruppen der Gesichterseite einer Person gehören nicht in die Gesichtsreiter
            // (und umgekehrt); bis zum neuen Lesen bleibt die Seite leer.
            self.face_page.groups.clear();
            self.face_advance = None;
        }
        self.view = view;
        self.recognition_tools_open = false;
        match view {
            // Eine laufende Suche ist schon die neueste Anfrage (jede neue bricht die alte ab),
            // z. B. die vom Start: nicht abbrechen und von vorn beginnen.
            View::Collection
                if self.tasks.iter().any(|t| {
                    t.kind == "search"
                        && t.collection == self.settings.collection
                        && !t.task.control.cancel.load(Ordering::Relaxed)
                }) => {}
            View::Collection => self.requery_collection(),
            View::Faces | View::People => self.refresh_faces(),
            // Automatische Sicherungen können inzwischen entstanden sein: beim Zeigen neu lesen.
            View::Settings if !self.backups.loading => self.backups.files = None,
            _ => {}
        }
    }
    /// Wählt einen Reiter der Ansicht „Gesichter“ und beginnt auf seiner ersten Seite.
    fn set_face_mode(&mut self, mode: &'static str) {
        if matches!(mode, "confirmed" | "ignored") {
            self.done_mode = mode;
        }
        self.face_mode = mode.into();
        self.reset_face_page();
        self.refresh_faces();
    }
    /// Wählt die Seite der Gesichterseite einer Person (`probable` oder `person-confirmed`).
    fn set_person_face_mode(&mut self, mode: &'static str) {
        self.person_face_mode = mode;
        self.reset_face_page();
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
                PersonMode::Faces => self.clear_face_groups(),
            }
        }
        self.refresh_faces();
    }
    /// Beginnt die angezeigten Gesichtsgruppen auf der ersten Seite, ohne gemerkte Position.
    fn reset_face_page(&mut self) {
        self.face_index = 0;
        self.face_advance = None;
        self.face_row_tops.clear();
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
        self.folder_pending = None;
        self.folder_revealed = None;
        self.filters = Search::default();
        self.applied = Search::default();
        self.queried = Search::default();
        self.page = SearchPage::default();
        self.status = CollectionStatus::default();
        self.counts_fresh = false;
        self.page_from_snapshot = false;
        self.known_counts = None;
        self.faces_after_search = false;
        self.selected_photo = None;
        self.large = false;
        self.collection_dialog = None;
        self.missing = Default::default();
        self.backups = Default::default();
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
        self.daily_backup_due = !self.settings.collection.is_empty();
        self.thumbnails.clear();
        if Path::new(&self.settings.collection)
            .join(".myfotomanager")
            .exists()
        {
            self.show_start_view();
            self.refresh_collection();
            self.refresh_faces();
        } else {
            self.collection_action("scan");
        }
    }
    fn refresh_faces(&mut self) {
        self.faces_after_search = false;
        if self.settings.collection.is_empty() {
            return;
        }
        self.face_revision += 1;
        let revision = self.face_revision;
        let root = PathBuf::from(&self.settings.collection);
        // Außerhalb von „Gesichter“ und „Personen“ braucht die Oberfläche nur Personen und
        // Zählungen. Gruppen samt Namensvorschlägen (alle Referenzmerkmale lesen) kämen sonst
        // schon beim Start und konkurrierten mit der ersten Sammlungssuche um die Platte; die
        // Ansicht liest sie beim Öffnen (`open_view`).
        let groups = matches!(self.view, View::Faces | View::People);
        let mode = if groups { self.page_mode() } else { "tree" }.to_string();
        let page = self.face_index;
        let person = self.page_person();
        self.launch(
            "Personen und Gesichter lesen",
            false,
            "faces",
            move |control| {
                let c = Collection::open(&root, false)?;
                c.cancel_with(&control);
                c.db.execute_batch("BEGIN")?;
                let read = |index| faces::page_with(&c, &mode, index, person, &control);
                let mut index = page;
                let mut snapshot = read(index)?;
                if groups && index > 0 && snapshot.groups.is_empty() {
                    index -= 1;
                    snapshot = read(index)?;
                    if index > 0 && snapshot.groups.is_empty() {
                        index = 0;
                        snapshot = read(index)?;
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
            || !self.counts_fresh
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
        if ["scan", "metadata", "analyze", "confirm_missing", "resolve"].contains(&action) {
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
        let label = match action.as_str() {
            "scan" => "Sammlung indizieren",
            "confirm_missing" => "Fehlende nach Prüfung markieren",
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
            let key = faces::setting(&c, faces::EMBEDDER_KEY)?.unwrap_or_default();
            Ok(Outcome::Suggestions(
                revision,
                id,
                faces::suggestions(&c, id, &key)?,
            ))
        });
    }
    /// Ergebnis eines Auftrags, dessen Revision nicht mehr gilt: es wird verworfen.
    fn stale(&self, outcome: &Outcome, running: &Running) -> bool {
        match outcome {
            Outcome::Synced(inner, _) => self.stale(inner, running),
            Outcome::Scan(revision, _) | Outcome::SourceOffline(revision, _) => {
                *revision != self.scan_revision
            }
            Outcome::Missing(revision, _) => {
                *revision != self.missing.revision || running.collection != self.settings.collection
            }
            Outcome::Backups(revision, _) => {
                *revision != self.backups.revision || running.collection != self.settings.collection
            }
            Outcome::Search(revision, ..) | Outcome::Counts(revision, ..) => {
                *revision != self.query_revision
            }
            Outcome::Folders(revision, _) => {
                self.folder_pending != Some(*revision)
                    || running.collection != self.settings.collection
            }
            Outcome::Faces(revision, ..) => *revision != self.face_revision,
            Outcome::Detail(revision, ..) => *revision != self.detail_revision,
            Outcome::Suggestions(revision, id, _) => {
                *revision != self.naming_revision || self.naming != Some(*id)
            }
            Outcome::RelationshipGraph(revision, ..) => {
                *revision != self.relationship_graph.revision
                    || running.collection != self.settings.collection
            }
            // Antworten der Zeitreise prüfen ihre Revisionen selbst; die Sammlung muss stimmen.
            Outcome::Time(
                time_view::TimeOutcome::People(..)
                | time_view::TimeOutcome::Loaded(_)
                | time_view::TimeOutcome::Computed { .. },
            ) => running.collection != self.settings.collection,
            Outcome::PersonInsights(..) | Outcome::Duplicates(..) | Outcome::Named(_) => {
                running.collection != self.settings.collection
            }
            _ => false,
        }
    }
    /// Eine Zeile je beendetem Auftrag: Art, Dauer seit dem Start (mit Wartezeit), Ergebnis,
    /// Fortschritt, Phasenzeiten und bei Fehlern der Text.
    fn log_job_end(&self, running: &Running, result: &Result<Outcome>) {
        if !diag::enabled() {
            return;
        }
        let control = &running.task.control;
        let cancelled = control.cancel.load(Ordering::Relaxed);
        let (verdict, error) = match result {
            Err(e) if cancelled => ("abgebrochen", Some(format!("{e:#}"))),
            Err(e) => ("fehler", Some(format!("{e:#}"))),
            Ok(outcome) if self.stale(outcome, running) => ("verworfen", None),
            Ok(_) => ("ok", None),
        };
        let items = match result {
            Ok(Outcome::Scan(_, scan)) => format!(" dateien={}", scan.files.len()),
            Ok(Outcome::Search(_, stamp, photos, counts, _)) => format!(
                " fotos={} zaehlung={} revision={}",
                photos.len(),
                if counts.is_some() {
                    "wiederverwendet"
                } else {
                    "folgt"
                },
                stamp.as_ref().map_or(-1, |s| s.revision)
            ),
            Ok(Outcome::Counts(_, stamp, counts, _, _)) => format!(
                " gesamt={} revision={}",
                counts.total,
                stamp.as_ref().map_or(-1, |s| s.revision)
            ),
            Ok(Outcome::Faces(_, _, page)) => format!(" gruppen={}", page.groups.len()),
            Ok(Outcome::Detail(_, _, faces, _)) => format!(" gesichter={}", faces.len()),
            Ok(Outcome::Imported(report)) => format!(
                " kopiert={} übersprungen={} fehler_dateien={}",
                report.copied.len(),
                report.skipped.len(),
                report.errors.len()
            ),
            _ => String::new(),
        };
        let progress = control.snapshot();
        let phases: Vec<String> = control
            .timings()
            .into_iter()
            .filter(|(name, _)| !name.is_empty())
            .map(|(name, seconds)| format!("{name}:{:.0}", seconds * 1000.0))
            .collect();
        let level = if verdict == "fehler" {
            log::Level::Warn
        } else {
            log::Level::Info
        };
        log::log!(
            target: "job",
            level,
            "ende art={} name={:?} dauer_ms={:.1} ergebnis={verdict} fortschritt={}/{}{items}{}{}",
            running.kind,
            running.task.label,
            diag::millis(running.task.started.elapsed()),
            progress.done,
            progress.total,
            if phases.is_empty() {
                String::new()
            } else {
                format!(" phasen_ms={:?}", phases.join(","))
            },
            error.map_or_else(String::new, |e| format!(" fehler={e:?}")),
        );
    }
    fn poll(&mut self, ctx: &egui::Context) {
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
                Ok(Outcome::Time(outcome)) => {
                    if running.collection == self.settings.collection
                        || matches!(outcome, time_view::TimeOutcome::Exported(_))
                    {
                        self.accept_time(outcome);
                    }
                }
                // Fehlende Zusatzangaben der Personendetails sind kein Fehler für den Nutzer;
                // die Abschnitte bleiben dann leer und werden mit dem nächsten Stand neu gelesen.
                // Abgelöste (abgebrochene) Lesevorgänge sind normal, alles andere wird geloggt.
                Err(e)
                    if matches!(
                        running.kind,
                        "person-insights" | "person-duplicates" | "timeline-people"
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
                Ok(Outcome::Duplicates(revision, duplicates))
                    if running.collection == self.settings.collection =>
                {
                    self.people_ui.accept_duplicates(revision, duplicates);
                }
                Ok(Outcome::PersonInsights(revision, insights))
                    if running.collection == self.settings.collection =>
                {
                    self.people_ui.accept_insights(revision, insights);
                }
                // Ein neueres Lesen der Sicherungen hat dieses abgelöst; Fehler beim Lesen selbst
                // kommen als `Outcome::Backups`.
                Err(_) if running.kind == "backup-list" => {}
                Err(e) if running.kind == "backup-daily" => {
                    // Abbruch (Nutzer, Programmende) ist kein Fehler; die Sicherung folgt beim
                    // nächsten Öffnen. Ein Fehler erscheint als Hinweis.
                    if running.task.control.cancel.load(Ordering::Relaxed) {
                        log::info!(target: "sicherung", "taeglich abgebrochen");
                    } else {
                        self.message(format!("Tägliche Datensicherung: {e:#}"), true);
                    }
                }
                Err(_) if running.kind == "folders" => {
                    // Der Worker liefert Fehler als `Outcome::Folders`; hier landet nur ein
                    // unerwartetes Ende. Nur das erwartete Lesen der aktuellen Sammlung zählt.
                    if running.collection == self.settings.collection
                        && self.folder_pending.take().is_some()
                    {
                        self.folder_tree.fail();
                    }
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
                Ok(Outcome::SourceOffline(revision, path)) if revision == self.scan_revision => {
                    log::info!(target: "import", "quelle_nicht_verbunden pfad={path:?}");
                    self.source_offline = Some(path);
                }
                Ok(Outcome::Missing(revision, folders))
                    if revision == self.missing.revision
                        && running.collection == self.settings.collection =>
                {
                    self.missing.accept(folders);
                }
                Ok(Outcome::Backups(revision, files))
                    if revision == self.backups.revision
                        && running.collection == self.settings.collection =>
                {
                    self.backups.accept(files);
                }
                Ok(Outcome::DailyBackup(collection, date, summary)) => {
                    log::info!(target: "sicherung", "taeglich {summary}");
                    self.settings
                        .daily_backup_dates
                        .insert(collection.clone(), date);
                    self.save();
                    if collection == self.settings.collection && !self.backups.loading {
                        self.backups.files = None;
                    }
                }
                Ok(Outcome::BackupCreated(message)) => {
                    self.message(message, false);
                    if running.collection == self.settings.collection && !self.backups.loading {
                        self.backups.files = None;
                    }
                }
                Ok(Outcome::Search(revision, _, photos, counts, filters))
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
                Ok(Outcome::Counts(revision, stamp, counts, filters, first_page))
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
                Ok(Outcome::Folders(revision, result))
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
                            self.time_ui.invalidate();
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
                        self.time_ui.need_people = true;
                        self.refresh_faces();
                    }
                }
                Ok(Outcome::Named(undo)) if running.collection == self.settings.collection => {
                    self.relationship_graph.stale = true;
                    // Die Zeitreise zeigt bestätigte Gesichter; beim nächsten Zeigen neu lesen.
                    self.time_ui.invalidate();
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
                Ok(Outcome::RelationshipGraph(revision, signature, graph))
                    if running.collection == self.settings.collection
                        && revision == self.relationship_graph.revision =>
                {
                    self.relationship_graph.refreshed(signature, graph);
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
        // Beim Start: Personen und Gesichter lesen, sobald Seite und Zählung der Sammlung da
        // sind (oder gescheitert).
        if self.faces_after_search
            && !self
                .tasks
                .iter()
                .any(|task| matches!(task.kind, "search" | "counts"))
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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let update_started = std::time::Instant::now();
        self.measure_frame(ctx, frame, update_started);
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
        if diag::enabled() {
            self.frame_meter.current.place = self.place_label();
            self.frame_meter.current.update_ms = update_started.elapsed().as_secs_f32() * 1000.0;
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for task in &self.tasks {
            log::info!(target: "job", "beim_beenden_gestoppt art={} name={:?}", task.kind, task.task.label);
            task.task.control.stop();
        }
        // Beim Beenden gibt es keine Oberfläche mehr für einen Hinweis: nur loggen.
        if let Err(error) = self.settings.save() {
            log::warn!(target: "start", "einstellungen_nicht_gespeichert beim_beenden fehler={error:#}");
        }
        log::info!(target: "start", "app_beendet laufzeit_s={}", diag::uptime().as_secs());
        diag::flush(Duration::from_secs(2));
    }
}
impl PhotoApp {
    /// Ansicht und Reiter fürs Diagnose-Log, z. B. `faces/unknown` oder `people/Time`.
    fn place_label(&self) -> String {
        let view = format!("{:?}", self.view).to_lowercase();
        match self.view {
            View::Faces => format!("{view}/{}", self.face_mode),
            View::People if self.person_mode == PersonMode::Faces => {
                format!("{view}/Faces/{}", self.person_face_mode)
            }
            View::People => format!("{view}/{:?}", self.person_mode),
            _ => view,
        }
    }
    /// Verbucht den vorigen Frame: eframe misst Aufbau, Tessellierung und Zeichnen
    /// (`cpu_usage`, ohne Warten auf den Bildschirm). Langsame Frames und die Zusammenfassung
    /// gehen ins Diagnose-Log.
    fn measure_frame(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        now: std::time::Instant,
    ) {
        if !diag::enabled() {
            self.frame_meter.current = Default::default();
            return;
        }
        if ctx.cumulative_frame_nr() == 1 {
            log::info!(
                target: "start",
                "erster_frame_fertig seit_start_ms={:.1}",
                diag::millis(diag::uptime())
            );
        }
        let Some(seconds) = frame.info().cpu_usage else {
            return;
        };
        let (slow, summary) = self.frame_meter.record(now, seconds * 1000.0);
        if let Some(note) = slow {
            log::info!(
                target: "ui.frame",
                "langsam dauer_ms={:.1} update_ms={:.1} ansicht={} texturen={} auftraege={} laufend={}",
                seconds * 1000.0,
                note.update_ms,
                note.place,
                note.textures,
                note.jobs,
                self.tasks.len()
            );
        }
        if let Some(s) = summary {
            log::info!(
                target: "ui.frame",
                "zusammenfassung frames={} avg_ms={:.1} p95_ms={:.1} max_ms={:.1} langsam={} texturen={} fenster_s={:.1}",
                s.frames,
                s.avg_ms,
                s.p95_ms,
                s.max_ms,
                s.slow,
                s.textures,
                s.span.as_secs_f32()
            );
        }
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
