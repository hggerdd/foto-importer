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
    Search(u64, CollectionStatus, SearchPage),
    Faces(u64, FacePage),
    Detail(u64, String, Vec<Face>),
    Done(String),
    Named(Undo),
    Pick(Pick, Option<PathBuf>),
}
struct Running {
    task: Task<Outcome>,
    write: bool,
    kind: &'static str,
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
}
impl PhotoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();
        theme::apply(&cc.egui_ctx, settings.dark);
        let models =
            crate::core::inference::manifests(Path::new(&settings.model_dir)).unwrap_or_default();
        Self {
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
        }
    }
    fn busy(&self) -> bool {
        self.tasks.iter().any(|t| t.write)
    }
    fn launch(
        &mut self,
        label: &str,
        write: bool,
        kind: &'static str,
        work: impl FnOnce(Control) -> Result<Outcome> + Send + 'static,
    ) {
        self.tasks.push(Running {
            task: Task::start(label, work),
            write,
            kind,
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
    fn save(&mut self) {
        if let Err(e) = self.settings.save() {
            self.message(
                format!("Einstellungen konnten nicht gespeichert werden: {e}"),
                true,
            );
        }
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
        self.applied = filters.clone();
        self.launch("Sammlung durchsuchen", false, "search", move |_| {
            let c = Collection::open(&root, false)?;
            let detector = faces::setting(&c, "rust.detector_key")?.unwrap_or_default();
            Ok(Outcome::Search(
                revision,
                c.status()?,
                metadata::search(&c, &filters, &detector)?,
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
            _ => "Sammlung öffnen",
        };
        self.launch(label, true, "mutation", move |control| {
            let mut c = Collection::open(Path::new(&settings.collection), true)?;
            let message = match action.as_str() {
                "scan" => c.quick_scan(false, &control)?,
                "metadata" => metadata::enrich(&mut c, &settings.exiftool, &control)?,
                "verify" => c.verify(&control)?,
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
                Ok(Outcome::Search(revision, status, page)) if revision == self.query_revision => {
                    self.status = status;
                    self.page = page;
                    self.thumbnails.clear();
                }
                Ok(Outcome::Faces(revision, page)) if revision == self.face_revision => {
                    self.face_page = page;
                }
                Ok(Outcome::Detail(revision, detail, faces))
                    if revision == self.detail_revision =>
                {
                    self.detail = detail;
                    self.detail_faces = faces;
                }
                Ok(Outcome::Done(message)) => {
                    self.message(message, false);
                    if running.write {
                        self.refresh_collection();
                        self.refresh_faces();
                    }
                }
                Ok(Outcome::Named(undo)) => {
                    self.undo.push(undo);
                    if self.undo.len() > 20 {
                        self.undo.remove(0);
                    }
                    self.message("Gesichtsentscheidung gespeichert".into(), false);
                    self.refresh_faces();
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
    fn import_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "DEIN NÄCHSTER IMPORT",
            "Erinnerungen sicher ankommen lassen.",
            "Quelle wählen, Aufnahmetage sichten und das Ziel vor dem Kopieren prüfen.",
        );
        theme::card().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("01  QUELLE").color(theme::ACCENT).strong());
                if ui.button("Ordner wählen …").clicked() {
                    self.pick(Pick::Source);
                }
                if ui
                    .add_enabled(
                        !self.settings.source.is_empty(),
                        egui::Button::new("Neu einlesen"),
                    )
                    .clicked()
                {
                    self.begin_scan();
                }
                ui.add(
                    egui::Label::new(if self.settings.source.is_empty() {
                        "Speicherkarte oder Fotoordner"
                    } else {
                        &self.settings.source
                    })
                    .truncate(),
                );
            });
            let changed = ui
                .checkbox(
                    &mut self.settings.metadata_date,
                    "Nach Aufnahmedatum gruppieren · ohne Metadaten gilt das Dateisystemdatum",
                )
                .changed();
            if changed {
                self.save();
                if !self.settings.source.is_empty() {
                    self.begin_scan();
                }
            }
        });
        ui.add_space(12.0);
        if self.scan.files.is_empty() {
            self.empty(ui,"Deine nächste Geschichte beginnt hier.","Wähle eine Speicherkarte oder einen Ordner. Es werden keine Dateien verschoben oder gelöscht.");
            return;
        }
        let selected_files: Vec<_> = self
            .scan
            .files
            .iter()
            .filter(|f| self.selected.contains(&f.day))
            .cloned()
            .collect();
        let days: BTreeMap<String, usize> =
            self.scan.files.iter().fold(BTreeMap::new(), |mut map, f| {
                *map.entry(f.day.clone()).or_default() += 1;
                map
            });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("02  AUFNAHMETAGE").strong());
            if ui.small_button("Alle auswählen").clicked() {
                self.selected = days.keys().cloned().collect();
            }
            if ui.small_button("Auswahl leeren").clicked() {
                self.selected.clear();
            }
            ui.label(format!("{} Dateien ausgewählt", selected_files.len()));
        });
        egui::ScrollArea::horizontal()
            .id_salt("days")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (day, count) in days.iter().rev() {
                        let mut selected = self.selected.contains(day);
                        ui.vertical(|ui| {
                            if ui
                                .selectable_label(
                                    self.focused == *day,
                                    RichText::new(format!("{day}\n{count} Dateien")).strong(),
                                )
                                .clicked()
                            {
                                self.focused = day.clone();
                            }
                            if ui.checkbox(&mut selected, "Importieren").changed() {
                                if selected {
                                    self.selected.insert(day.clone());
                                } else {
                                    self.selected.remove(day);
                                }
                            }
                        });
                        ui.add_space(8.0);
                    }
                });
            });
        ui.separator();
        let right_width = 280.0;
        let width = (ui.available_width() - right_width - 20.0).max(300.0);
        ui.horizontal_top(|ui|{
            ui.allocate_ui(egui::vec2(width,ui.available_height()),|ui|{ui.horizontal(|ui|{ui.label(RichText::new(format!("{} · Vorschau",self.focused)).strong());ui.add(egui::Slider::new(&mut self.zoom,120.0..=240.0).show_value(false).text("Bildgröße"));});let files:Vec<_>=self.scan.files.iter().filter(|f|f.day==self.focused).cloned().collect();self.source_gallery(ui,&files);});
            ui.allocate_ui(egui::vec2(right_width,ui.available_height()),|ui|{egui::ScrollArea::vertical().id_salt("import-plan").show(ui,|ui|{theme::card().show(ui,|ui|{
                theme::eyebrow(ui,"03  ZIEL & IMPORT");ui.heading("Alles am richtigen Ort");if ui.button("Zielordner wählen …").clicked(){self.pick(Pick::Target);}ui.add(egui::Label::new(if self.settings.target.is_empty(){"Noch kein Ziel gewählt"}else{&self.settings.target}).wrap());ui.label("Neuer Ordner");ui.add(egui::TextEdit::singleline(&mut self.folder_name).hint_text("z. B. 2026-11 Herbsturlaub").desired_width(f32::INFINITY));
                let validation=files::validate_folder(&self.folder_name);if let Err(e)=&validation{ui.label(RichText::new(e.to_string()).size(12.0).color(theme::WARM));}
                ui.add_space(10.0);ui.label(RichText::new("Geplante Ablage").strong());ui.label(format!("{}/",if self.folder_name.is_empty(){"Ordnername"}else{&self.folder_name}));for(ext,count)in files::counts(&selected_files){ui.label(format!("    {ext}/  ·  {count} Dateien"));}
                if ui.checkbox(&mut self.settings.dng,"NEF zusätzlich als DNG").changed(){self.save();}
if self.settings.dng{ui.label(RichText::new("DNGs liegen direkt im neuen Ordner. Adobe Converter erforderlich.").small().weak());}
                ui.separator();let bytes:u64=selected_files.iter().map(|f|f.size).sum();ui.label(RichText::new(format!("{} Dateien · {}",selected_files.len(),human_size(bytes))).strong());ui.label(RichText::new("Originale bleiben erhalten. Namenskollisionen erhalten einen Nummernsuffix.").small().weak());
                if theme::primary(ui,"Sicher importieren →",validation.is_ok()&&!selected_files.is_empty()&&!self.settings.target.is_empty()&&!self.tasks.iter().any(|t|t.kind=="copy")){
                    let target=PathBuf::from(&self.settings.target);let name=self.folder_name.clone();let converter=self.settings.dng.then(||PathBuf::from(&self.settings.converter));let files=selected_files.clone();self.save();self.launch("Fotos sicher kopieren",true,"copy",move |control|{let report=files::import(&files,&target,&name,converter.as_deref(),&control)?;Ok(Outcome::Done(format!("{} Dateien kopiert · {} DNGs · {} Fehler{}",report.copied.len(),report.converted,report.errors.len(),if report.errors.is_empty(){String::new()}else{format!(": {}",report.errors.join("; "))}))) });
                }
            });});});
        });
    }
    fn source_gallery(&mut self, ui: &mut egui::Ui, files: &[SourceFile]) {
        let columns = (ui.available_width() / (self.zoom + 12.0)).floor().max(1.0) as usize;
        let rows = files.len().div_ceil(columns);
        let height = self.zoom * 0.72 + 45.0;
        egui::ScrollArea::vertical()
            .id_salt("source-gallery")
            .show_rows(ui, height, rows, |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        for file in files.iter().skip(row * columns).take(columns) {
                            ui.vertical(|ui| {
                                let request = PreviewRequest {
                                    path: file.path.clone(),
                                    orientation: 1,
                                    crop: None,
                                    edge: 320,
                                    generation: 0,
                                };
                                let response = self.thumbnails.image(
                                    ui,
                                    request,
                                    &self.settings.exiftool,
                                    egui::vec2(self.zoom, self.zoom * 0.72),
                                );
                                response.on_hover_text(file.path.display().to_string());
                                ui.add_sized(
                                    [self.zoom, 25.0],
                                    egui::Label::new(
                                        file.path.file_name().unwrap_or_default().to_string_lossy(),
                                    )
                                    .truncate(),
                                );
                            });
                        }
                    });
                }
            });
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
    fn collection_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "DEINE FOTOBIBLIOTHEK",
            "Wiederfinden statt durchsuchen.",
            "Aufnahmen, Metadaten und Personen in einer gemeinsamen Sammlung.",
        );
        ui.horizontal(|ui| {
            if ui.button("Sammlung wählen …").clicked() {
                self.pick(Pick::Collection);
            }
            ui.add(egui::Label::new(&self.settings.collection).truncate());
        });
        if self.settings.collection.is_empty() {
            self.empty(ui,"Ein Zuhause für deine Fotos.","Wähle den Wurzelordner deiner Sammlung. Vorhandene Schema-6-Sammlungen bleiben erhalten.");
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (label, value) in [
                ("Aufnahmen", self.status.photos),
                ("Dateien", self.status.files),
                ("Metadaten offen", self.status.pending),
                ("Fehlend", self.status.missing),
            ] {
                theme::card().show(ui, |ui| {
                    ui.label(RichText::new(value.to_string()).size(25.0).strong());
                    ui.label(RichText::new(label).small().weak());
                });
            }
            ui.separator();
            for (action, label) in [
                ("scan", "Bestand aktualisieren"),
                ("metadata", "Metadaten lesen"),
                ("verify", "Integrität prüfen"),
            ] {
                if ui
                    .add_enabled(!self.busy(), egui::Button::new(label))
                    .clicked()
                {
                    self.collection_action(action);
                }
            }
        });
        ui.add_space(10.0);
        self.search_builder(ui);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} Aufnahmen", self.page.total)).strong());
            if ui
                .add_enabled(self.filters.page > 0, egui::Button::new("←"))
                .clicked()
            {
                self.filters.page -= 1;
                self.refresh_collection();
            }
            ui.label(format!(
                "Seite {} / {}",
                self.filters.page + 1,
                self.page.total.div_ceil(config::PAGE_SIZE).max(1)
            ));
            if ui
                .add_enabled(
                    (self.filters.page + 1) * config::PAGE_SIZE < self.page.total,
                    egui::Button::new("→"),
                )
                .clicked()
            {
                self.filters.page += 1;
                self.refresh_collection();
            }
            ui.checkbox(&mut self.inspector, "Details");
        });
        let available = ui.available_width();
        let inspect = self.inspector && available > 650.0;
        let gallery_width = if inspect {
            available - 320.0
        } else {
            available
        };
        ui.horizontal_top(|ui| {
            ui.allocate_ui(egui::vec2(gallery_width, ui.available_height()), |ui| {
                self.photo_gallery(ui);
            });
            if inspect {
                ui.allocate_ui(egui::vec2(300.0, ui.available_height()), |ui| {
                    self.inspector(ui);
                });
            }
        });
    }
    fn search_builder(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filters.text)
                    .hint_text("Pfad, Kamera oder Objektiv …")
                    .desired_width(300.0),
            );
            if theme::primary(ui, "Suchen", true) {
                self.filters.page = 0;
                self.refresh_collection();
            }
            if ui.button("Zurücksetzen").clicked() {
                self.filters = Search::default();
                self.refresh_collection();
            }
            if self.filters != self.applied {
                ui.label(
                    RichText::new("Änderungen noch nicht angewendet")
                        .small()
                        .color(theme::WARM),
                );
            }
        });
        egui::CollapsingHeader::new("Zeitraum, Personen & weitere Kriterien").id_salt("search-builder").show(ui,|ui|{
            ui.horizontal(|ui|{ui.label("Von");ui.add(egui::TextEdit::singleline(&mut self.filters.start).hint_text("JJJJ-MM-TT").desired_width(110.0));ui.label("bis");ui.add(egui::TextEdit::singleline(&mut self.filters.end).hint_text("JJJJ-MM-TT").desired_width(110.0));ui.checkbox(&mut self.filters.oldest,"Älteste zuerst");});
            ui.horizontal_wrapped(|ui|{for(month,label)in ["Jan","Feb","Mär","Apr","Mai","Jun","Jul","Aug","Sep","Okt","Nov","Dez"].iter().enumerate(){let number=month as u32+1;let mut checked=self.filters.months.contains(&number);if ui.checkbox(&mut checked,*label).changed(){if checked{self.filters.months.push(number);}else{self.filters.months.retain(|m|*m!=number);}}}});
            for(label,group)in [("Alle diese Personen",&mut self.filters.people_all),("Mindestens eine",&mut self.filters.people_any),("Keine dieser Personen",&mut self.filters.people_none)]{
                ui.horizontal_wrapped(|ui|{ui.label(label);egui::ComboBox::from_id_salt(label).selected_text("Person hinzufügen …").show_ui(ui,|ui|{for person in &self.face_page.people{if ui.selectable_label(false,format!("{} · #{}",person.name,person.id)).clicked()&&!group.contains(&person.id){group.push(person.id);}}});let mut remove=None;for(id,person)in group.iter().enumerate(){let name=self.face_page.people.iter().find(|p|p.id==*person).map(|p|p.name.clone()).unwrap_or_else(||format!("#{person}"));if ui.small_button(format!("{name} ×")).clicked(){remove=Some(id);}}
if let Some(id)=remove{group.remove(id);}});
            }
            ui.horizontal_wrapped(|ui|{ui.checkbox(&mut self.filters.only_people,"Nur diese Personen");ui.checkbox(&mut self.filters.suggestions,"Unbestätigte Vorschläge einbeziehen");egui::ComboBox::from_id_salt("face-state").selected_text(face_label(&self.filters.face_state)).show_ui(ui,|ui|{for(value,label)in [("any","Alle Fotos"),("with_faces","Mit Gesichtern"),("none","Ohne erkannte Gesichter"),("unknown","Unbekannte Gesichter"),("unanalysed","Noch nicht analysiert")]{ui.selectable_value(&mut self.filters.face_state,value.into(),label);}});});
            ui.label(RichText::new("Ausschlüsse benötigen eine erfolgreiche Analyse. Ignorierte Gesichter zählen weiterhin als anwesend.").small().weak());
            let mut remove=None;for(index,rule)in self.filters.rules.iter_mut().enumerate(){ui.horizontal(|ui|{egui::ComboBox::from_id_salt(("field",index)).selected_text(metadata::FIELDS.iter().find(|f|f.0==rule.field).map(|f|f.1).unwrap_or("Kriterium")).show_ui(ui,|ui|{for(value,label)in metadata::FIELDS{ui.selectable_value(&mut rule.field,value.to_string(),*label);}});egui::ComboBox::from_id_salt(("operator",index)).selected_text(operator_label(&rule.operator)).show_ui(ui,|ui|{for(value,label)in [("is","ist"),("not","ist nicht"),("contains","enthält"),("min","mindestens"),("max","höchstens"),("present","vorhanden"),("empty","fehlt")]{ui.selectable_value(&mut rule.operator,value.into(),label);}});ui.add(egui::TextEdit::singleline(&mut rule.value).desired_width(150.0));if ui.small_button("Entfernen").clicked(){remove=Some(index);}});}
if let Some(index)=remove{self.filters.rules.remove(index);}
            ui.horizontal(|ui|{if ui.button("+ Kriterium").clicked(){self.filters.rules.push(Rule{field:"camera".into(),operator:"contains".into(),value:String::new()});}ui.checkbox(&mut self.filters.any_rule,"Mindestens ein Metadatenkriterium erfüllt");});
        });
    }
    fn photo_gallery(&mut self, ui: &mut egui::Ui) {
        if self.page.photos.is_empty() {
            self.empty(
                ui,
                "Platz für neue Entdeckungen.",
                "Bestand aktualisieren oder die Suchkriterien anpassen.",
            );
            return;
        }
        let columns = (ui.available_width() / 190.0).floor().max(1.0) as usize;
        let width = (ui.available_width() / columns as f32 - 12.0).min(250.0);
        let photos = self.page.photos.clone();
        egui::ScrollArea::vertical()
            .id_salt("collection-gallery")
            .show_rows(
                ui,
                width * 0.7 + 60.0,
                photos.len().div_ceil(columns),
                |ui, range| {
                    for row in range {
                        ui.horizontal(|ui| {
                            for photo in photos.iter().skip(row * columns).take(columns) {
                                ui.vertical(|ui| {
                                    let request = PreviewRequest {
                                        path: Path::new(&self.settings.collection)
                                            .join(&photo.source),
                                        orientation: photo.orientation,
                                        crop: None,
                                        edge: 400,
                                        generation: 0,
                                    };
                                    let response = self.thumbnails.image(
                                        ui,
                                        request,
                                        &self.settings.exiftool,
                                        egui::vec2(width, width * 0.7),
                                    );
                                    if self
                                        .selected_photo
                                        .as_ref()
                                        .is_some_and(|p| p.id == photo.id)
                                    {
                                        ui.painter().rect_stroke(
                                            response.rect,
                                            8,
                                            egui::Stroke::new(2.0_f32, theme::ACCENT),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                    if response.clicked() {
                                        self.choose_photo(photo.clone());
                                    }
                                    ui.add_sized(
                                        [width, 20.0],
                                        egui::Label::new(RichText::new(&photo.name).strong())
                                            .truncate(),
                                    );
                                    ui.add_sized(
                                        [width, 18.0],
                                        egui::Label::new(
                                            RichText::new(if photo.date.is_empty() {
                                                "Datum unbekannt"
                                            } else {
                                                &photo.date
                                            })
                                            .small()
                                            .weak(),
                                        )
                                        .truncate(),
                                    );
                                });
                            }
                        });
                    }
                },
            );
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("inspector")
            .show(ui, |ui| {
                let Some(photo) = self.selected_photo.clone() else {
                    ui.label(
                        RichText::new("Ein Bild. Seine Geschichte.")
                            .size(18.0)
                            .strong(),
                    );
                    ui.label("Wähle eine Aufnahme für Vorschau und Metadaten.");
                    return;
                };
                theme::eyebrow(ui, "BILDDETAILS");
                ui.label(RichText::new(&photo.name).size(20.0).strong());
                let request = PreviewRequest {
                    path: Path::new(&self.settings.collection).join(&photo.source),
                    orientation: photo.orientation,
                    crop: None,
                    edge: 960,
                    generation: 0,
                };
                let response = self.thumbnails.image(
                    ui,
                    request,
                    &self.settings.exiftool,
                    egui::vec2(ui.available_width(), 200.0),
                );
                self.overlay(ui, response.rect, &photo);
                if response.clicked() {
                    self.large = true;
                }
                ui.checkbox(&mut self.show_frames, "Gesichtsrahmen anzeigen");
                ui.label(&photo.date);
                ui.label(format!("{} · {}", photo.camera, photo.lens));
                ui.label(format!(
                    "ISO {} · {}",
                    photo
                        .iso
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unbekannt".into()),
                    human_size(photo.size)
                ));
                ui.label(RichText::new(&photo.path).small().weak());
                for face in self.detail_faces.clone() {
                    ui.horizontal(|ui| {
                        let name = face
                            .person
                            .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| "Unbekannt".into());
                        if ui.button(format!("{name} · #{}", face.id)).clicked() {
                            self.naming = Some(face.clone());
                            self.name_input.clear();
                            self.name_person = None;
                            self.name_suggestions.clear();
                        }
                        if ui
                            .add_enabled(!self.busy(), egui::Button::new("Löschen"))
                            .on_hover_text(
                                "Nur diese Fehldetektion löschen, Original bleibt erhalten",
                            )
                            .clicked()
                        {
                            let root = self.settings.collection.clone();
                            let id = face.id;
                            let photo = face.photo;
                            self.undo.clear();
                            self.launch("Fehldetektion löschen", true, "mutation", move |_| {
                                let mut c = Collection::open(Path::new(&root), true)?;
                                faces::delete_face(&mut c, photo, id)?;
                                Ok(Outcome::Done("Fehldetektion gelöscht".into()))
                            });
                        }
                    });
                }
                ui.separator();
                egui::CollapsingHeader::new("Alle gespeicherten Metadaten")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&self.detail).monospace().size(11.0))
                                .wrap(),
                        );
                    });
            });
    }
    fn overlay(&self, ui: &egui::Ui, rect: egui::Rect, photo: &Photo) {
        if !self.show_frames {
            return;
        }
        for face in self
            .detail_faces
            .iter()
            .filter(|f| f.source == photo.source)
        {
            let b = face.bbox;
            let frame = egui::Rect::from_min_max(
                egui::pos2(
                    rect.left() + b[0] * rect.width(),
                    rect.top() + b[1] * rect.height(),
                ),
                egui::pos2(
                    rect.left() + b[2] * rect.width(),
                    rect.top() + b[3] * rect.height(),
                ),
            );
            ui.painter().rect_stroke(
                frame,
                2,
                egui::Stroke::new(1.5_f32, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
    }
    fn people_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "GESICHTER & PERSONEN",
            "Aus Gesichtern werden Geschichten.",
            "Lokal analysieren, gemeinsam benennen und bestätigtes Wissen anwenden.",
        );
        ui.horizontal_wrapped(|ui| {
            if ui.button("Sammlung wählen …").clicked() {
                self.pick(Pick::Collection);
            }
            for (action, label) in [
                ("analyze", "1  Fotos analysieren"),
                ("knowledge", "2  Lernen & Namen vorschlagen"),
            ] {
                if ui
                    .add_enabled(
                        !self.busy() && !self.settings.collection.is_empty(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    self.collection_action(action);
                }
            }
            if ui
                .add_enabled(
                    !self.busy() && !self.undo.is_empty(),
                    egui::Button::new("Rückgängig"),
                )
                .clicked()
                && let Some(undo) = self.undo.last().cloned() {
                    let root = self.settings.collection.clone();
                    self.launch(
                        "Gruppenentscheidung zurücknehmen",
                        true,
                        "mutation",
                        move |_| {
                            let mut c = Collection::open(Path::new(&root), true)?;
                            faces::undo(&mut c, &undo)?;
                            Ok(Outcome::Done("Gruppenentscheidung zurückgenommen".into()))
                        },
                    );
                    self.undo.pop();
                }
        });
        if self.settings.collection.is_empty() {
            self.empty(ui,"Wer gehört zu deiner Geschichte?","Wähle zuerst eine Sammlung. Gesichtsbilder und Vektoren bleiben auf diesem Rechner.");
            return;
        }
        let status = &self.face_page.status;
        ui.horizontal_wrapped(|ui| {
            for (label, value) in [
                ("Bestätigt", status.confirmed),
                ("Unbekannt", status.unknown),
                ("Vorschläge", status.review),
                ("Ignoriert", status.ignored),
                ("Referenzen", status.references),
            ] {
                theme::card().show(ui, |ui| {
                    ui.label(RichText::new(value.to_string()).size(22.0).strong());
                    ui.label(label);
                });
            }
        });
        ui.label(
            RichText::new(format!(
                "Letzter Abgleich: {}{}",
                status.applied,
                if status.pending > 0 {
                    " · Nacharbeit offen"
                } else {
                    ""
                }
            ))
            .small()
            .weak(),
        );
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                ("unknown", "Benennen"),
                ("review", "Prüfen"),
                ("confirmed", "Bestätigte"),
                ("ignored", "Ignorierte"),
                ("persons", "Personen & Beziehungen"),
            ] {
                if ui.selectable_label(self.face_mode == mode, label).clicked() {
                    self.face_mode = mode.into();
                    self.face_index = 0;
                    self.refresh_faces();
                }
            }
        });
        ui.separator();
        if self.face_mode == "persons" {
            self.person_manager(ui);
            return;
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.face_index > 0, egui::Button::new("← Zurück"))
                .clicked()
            {
                self.face_index -= 1;
                self.refresh_faces();
            }
            ui.label(format!("Seite {}", self.face_index + 1));
            if ui
                .add_enabled(
                    !self.face_page.groups.is_empty(),
                    egui::Button::new("Weiter →"),
                )
                .clicked()
            {
                self.face_index += 1;
                self.refresh_faces();
            }
            if ui.button("Aktualisieren").clicked() {
                self.refresh_faces();
            }
        });
        if self.face_page.groups.is_empty() {
            self.empty(ui,"Hier ist gerade nichts zu benennen.","Fotos analysieren oder eine andere Ansicht wählen. Neue Referenzen entstehen nur aus bestätigten Namen.");
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("face-groups")
            .show(ui, |ui| {
                for group in self.face_page.groups.clone() {
                    self.group(ui, &group);
                    ui.add_space(12.0);
                }
            });
    }
    fn group(&mut self, ui: &mut egui::Ui, group: &[Face]) {
        let key = group
            .iter()
            .map(|f| f.id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let busy = self.busy();
        let root = self.settings.collection.clone();
        let exiftool = self.settings.exiftool.clone();
        let draft = self.drafts.entry(key.clone()).or_insert_with(|| {
            let person = group.first().and_then(|f| f.person);
            Draft {
                person,
                name: person
                    .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
                    .map(|p| p.name.clone())
                    .unwrap_or_default(),
                excluded: BTreeSet::new(),
            }
        });
        let mut submit = false;
        let mut ignore = false;
        theme::card().show(ui, |ui| {
            ui.push_id(&key, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} Gesichter", group.len())).strong());
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut draft.name)
                            .hint_text("Name eingeben, Enter bestätigen")
                            .desired_width(240.0),
                    );
                    if response.changed() {
                        draft.person = None;
                    }
                    submit = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    egui::ComboBox::from_id_salt("person")
                        .selected_text("Person wählen …")
                        .show_ui(ui, |ui| {
                            for person in self
                                .face_page
                                .people
                                .iter()
                                .filter(|p| {
                                    files::fold(&p.name).contains(&files::fold(&draft.name))
                                })
                                .take(40)
                                .cloned()
                                .collect::<Vec<_>>()
                            {
                                if ui
                                    .selectable_label(
                                        draft.person == Some(person.id),
                                        format!("{} · #{}", person.name, person.id),
                                    )
                                    .clicked()
                                {
                                    draft.person = Some(person.id);
                                    draft.name = person.name.clone();
                                }
                            }
                        });
                    if ui
                        .add_enabled(!busy, egui::Button::new("Bestätigen"))
                        .clicked()
                    {
                        submit = true;
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Nicht interessiert"))
                        .clicked()
                    {
                        ignore = true;
                    }
                });
                let size = ((ui.available_width() - 6.0 * 10.0) / 7.0).clamp(42.0, 110.0);
                for chunk in group.chunks(7) {
                    ui.horizontal(|ui| {
                        for face in chunk {
                            ui.vertical(|ui| {
                                let request = PreviewRequest {
                                    path: Path::new(&root).join(&face.source),
                                    orientation: 1,
                                    crop: Some(face.bbox),
                                    edge: 160,
                                    generation: 0,
                                };
                                let response = self.thumbnails.image(
                                    ui,
                                    request,
                                    &exiftool,
                                    egui::vec2(size, size),
                                );
                                let mut included = !draft.excluded.contains(&face.id);
                                if response.clicked() {
                                    included = !included;
                                    if included {
                                        draft.excluded.remove(&face.id);
                                    } else {
                                        draft.excluded.insert(face.id);
                                    }
                                }
                                if ui
                                    .checkbox(&mut included, format!("#{}", face.id))
                                    .changed()
                                {
                                    if included {
                                        draft.excluded.remove(&face.id);
                                    } else {
                                        draft.excluded.insert(face.id);
                                    }
                                }
                            });
                        }
                    });
                }
            });
        });
        if (submit || ignore) && !busy {
            let ids: Vec<_> = group.iter().map(|f| f.id).collect();
            let selected = ids
                .iter()
                .filter(|id| !draft.excluded.contains(id))
                .copied()
                .collect();
            let name = draft.name.clone();
            let person = draft.person;
            self.launch("Gesichtsgruppe speichern", true, "naming", move |_| {
                let mut c = Collection::open(Path::new(&root), true)?;
                let undo = faces::name_group(&mut c, &ids, &selected, person, &name, ignore)?;
                Ok(Outcome::Named(undo))
            });
        }
    }
    fn person_manager(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Personen haben stabile IDs. Gleiche Namen bleiben unterscheidbar.")
                .weak(),
        );
        egui::ScrollArea::vertical()
            .id_salt("persons")
            .max_height(280.0)
            .show(ui, |ui| {
                for person in &self.face_page.people {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&person.name).strong());
                        ui.label(format!(
                            "#{} · {} bestätigte Gesichter · Geburtsjahr {}",
                            person.id, person.count, person.birth
                        ));
                    });
                }
            });
        ui.separator();
        ui.label(RichText::new("Familienbeziehung ergänzen").strong());
        ui.horizontal_wrapped(|ui| {
            for (id, selected) in [
                ("Von", &mut self.relation_from),
                ("Zu", &mut self.relation_to),
            ] {
                egui::ComboBox::from_id_salt(id)
                    .selected_text(
                        selected
                            .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
                            .map(|p| format!("{} #{}", p.name, p.id))
                            .unwrap_or_else(|| id.into()),
                    )
                    .show_ui(ui, |ui| {
                        for person in &self.face_page.people {
                            ui.selectable_value(
                                selected,
                                Some(person.id),
                                format!("{} #{}", person.name, person.id),
                            );
                        }
                    });
            }
            egui::ComboBox::from_id_salt("relation-kind")
                .selected_text(&self.relation_kind)
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        ("parent", "Elternteil von"),
                        ("child", "Kind von"),
                        ("partner", "Partner von"),
                        ("sibling", "Geschwister von"),
                    ] {
                        ui.selectable_value(&mut self.relation_kind, value.into(), label);
                    }
                });
            if ui
                .add_enabled(
                    !self.busy() && self.relation_from.is_some() && self.relation_to.is_some(),
                    egui::Button::new("Speichern"),
                )
                .clicked()
            {
                let root = self.settings.collection.clone();
                let from = self.relation_from.unwrap_or_default();
                let to = self.relation_to.unwrap_or_default();
                let kind = self.relation_kind.clone();
                self.launch("Beziehung speichern", true, "mutation", move |_| {
                    let mut c = Collection::open(Path::new(&root), true)?;
                    faces::add_relation(&mut c, from, &kind, to)?;
                    Ok(Outcome::Done("Beziehung gespeichert".into()))
                });
            }
        });
    }
    fn settings_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "DEIN ARBEITSPLATZ",
            "Alles lokal. Alles an seinem Platz.",
            "Werkzeuge und Modelle werden nur verwendet, wenn du die entsprechende Funktion startest.",
        );
        let mut chosen = None;
        egui::ScrollArea::vertical().show(ui,|ui|{theme::card().show(ui,|ui|{ui.label(RichText::new("Externe Werkzeuge").size(20.0).strong());for(pick,label,value)in [(Pick::Converter,"Adobe DNG Converter",&mut self.settings.converter),(Pick::Exiftool,"ExifTool",&mut self.settings.exiftool),(Pick::Runtime,"ONNX Runtime DLL",&mut self.settings.onnx_runtime),(Pick::Models,"Modellordner",&mut self.settings.model_dir)]{ui.label(label);let mut browse=false;ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(value).desired_width(600.0));browse=ui.button("Auswählen …").clicked();});if browse{chosen=Some(pick);break;}}});
            ui.add_space(16.0);theme::card().show(ui,|ui|{ui.label(RichText::new("Lokale Gesichtserkennung").size(20.0).strong());for(kind,selected)in [("detector",&mut self.detector),("embedder",&mut self.embedder)]{egui::ComboBox::from_id_salt(kind).selected_text(selected.clone()).show_ui(ui,|ui|{for model in self.models.iter().filter(|m|m.kind==kind){ui.selectable_value(selected,model.id.clone(),&model.name);}});}for model in &self.models{ui.label(format!("{} · {}",model.name,model.license));}ui.label(RichText::new("Keine automatischen Gewichtsdownloads. SHA-256 wird vor dem Laden geprüft. Rust-Adapter verwenden getrennte Modellschlüssel.").small().weak());});
            ui.add_space(16.0);if ui.checkbox(&mut self.settings.dark,"Dunkle Oberfläche").changed(){theme::apply(ui.ctx(),self.settings.dark);}
if theme::primary(ui,"Einstellungen speichern",true){self.save();self.models=crate::core::inference::manifests(Path::new(&self.settings.model_dir)).unwrap_or_default();self.message("Einstellungen gespeichert".into(),false);}ui.hyperlink_to("Adobe DNG Converter herunterladen","https://adobe.com/go/dng_converter_win/");ui.hyperlink_to("ExifTool installieren","https://exiftool.org/");
        });
    }
    fn tasks_view(&mut self, ui: &mut egui::Ui) {
        Self::header(
            ui,
            "FORTSCHRITT & VERLAUF",
            "Du behältst den Überblick.",
            "Aufträge laufen im Hintergrund. Abbrechen beendet laufende Arbeit kooperativ.",
        );
        for running in &self.tasks {
            theme::card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(&running.task.label);
                    if ui.button("Abbrechen").clicked() {
                        running.task.control.stop();
                    }
                });
            });
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for line in self.history.iter().rev() {
                ui.label(line);
            }
        });
    }
    fn windows(&mut self, ctx: &egui::Context) {
        if self.large {
            let mut open = true;
            egui::Window::new("Große Bildansicht")
                .open(&mut open)
                .default_size([980.0, 720.0])
                .show(ctx, |ui| {
                    if let Some(photo) = self.selected_photo.clone() {
                        let request = PreviewRequest {
                            path: Path::new(&self.settings.collection).join(&photo.source),
                            orientation: photo.orientation,
                            crop: None,
                            edge: 1920,
                            generation: 0,
                        };
                        let size = egui::vec2(
                            ui.available_width(),
                            (ui.available_height() - 45.0).max(200.0),
                        );
                        let response =
                            self.thumbnails
                                .image(ui, request, &self.settings.exiftool, size);
                        self.overlay(ui, response.rect, &photo);
                        ui.checkbox(&mut self.show_frames, "Gesichtsrahmen anzeigen");
                    }
                });
            self.large = open;
        }
        if let Some(face) = self.naming.clone() {
            let mut open = true;
            egui::Window::new(format!("Gesicht #{} benennen", face.id))
                .open(&mut open)
                .default_width(600.0)
                .show(ctx, |ui| {
                    ui.label("Bestehende Person wählen oder neuen Namen eingeben.");
                    ui.text_edit_singleline(&mut self.name_input);
                    for person in self
                        .face_page
                        .people
                        .iter()
                        .filter(|p| files::fold(&p.name).contains(&files::fold(&self.name_input)))
                        .take(5)
                        .cloned()
                        .collect::<Vec<_>>()
                    {
                        if ui
                            .selectable_label(
                                self.name_person == Some(person.id),
                                format!("{} · #{}", person.name, person.id),
                            )
                            .clicked()
                        {
                            self.name_person = Some(person.id);
                            self.name_input = person.name.clone();
                        }
                    }
                    if theme::primary(
                        ui,
                        "Übernehmen",
                        !self.busy() && !self.name_input.trim().is_empty(),
                    ) {
                        let root = self.settings.collection.clone();
                        let name = self.name_input.clone();
                        let person = self.name_person;
                        self.launch("Gesicht benennen", true, "naming", move |_| {
                            let mut c = Collection::open(Path::new(&root), true)?;
                            Ok(Outcome::Named(faces::name_group(
                                &mut c,
                                &[face.id],
                                &BTreeSet::from([face.id]),
                                person,
                                &name,
                                false,
                            )?))
                        });
                        self.naming = None;
                    }
                });
            if !open {
                self.naming = None;
            }
        }
    }
}
impl eframe::App for PhotoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
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
