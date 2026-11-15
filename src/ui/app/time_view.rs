//! Seite „06 Zeitreise“ (ADR 0069): Gesichter einer Person über die Zeit. Domainlogik steht in
//! `core::timeline`; diese Datei zeichnet und startet Worker. Abfrage und Berechnung laufen nie
//! auf dem UI-Thread; veraltete Antworten werden über Revisionen verworfen. Kacheln laden über
//! den vorhandenen Vorschau-Cache nur, wenn sie sichtbar sind.

use super::*;
use crate::core::timeline_export;
use crate::core::{
    faces::Person,
    relations,
    timeline::{
        self, Axis, Birth, Computed, DateQuality, Decisions, Grain, Params, Section, Thresholds,
        TimeFace,
    },
};
use crate::ui::select::{self, Choice, FilterSelect};
use std::sync::Arc;

const TILE: f32 = 112.0;

#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum Tab {
    #[default]
    Strip,
    Gaps,
    Lapse,
}

/// Ergebnis des Ladeauftrags: Personen, Gesichter der Person und die erste Berechnung.
pub(super) struct Loaded {
    revision: u64,
    person: Option<i64>,
    people: Vec<Person>,
    relatives: Vec<(i64, &'static str)>,
    raw: Arc<Vec<TimeFace>>,
    birth: Option<Birth>,
    decisions: Decisions,
    computed: Computed,
}

pub(super) enum TimeOutcome {
    Loaded(Box<Loaded>),
    Computed {
        revision: u64,
        computed: Computed,
    },
    /// Eine Nutzerentscheidung ist gespeichert; neu laden.
    Decided,
    /// Ein Export ist fertig (Pfad) oder wurde im Ordnerdialog abgebrochen.
    Exported(Option<PathBuf>),
}

/// Nutzerentscheidung, die auf einem Schreib-Worker gespeichert wird.
enum Decision {
    Pin(String, i64),
    Unpin(String),
    Hide(i64, bool),
    BirthMonth(Option<u8>),
}

pub(super) struct TimeUi {
    collection: String,
    pub person: Option<i64>,
    loaded_person: Option<i64>,
    people: Vec<Person>,
    relatives: Vec<(i64, &'static str)>,
    raw: Option<Arc<Vec<TimeFace>>>,
    birth: Option<Birth>,
    decisions: Decisions,
    computed: Option<Arc<Computed>>,
    pub axis: Axis,
    pub grain: Option<Grain>,
    include_auto: bool,
    min_date: DateQuality,
    revision: u64,
    need_load: bool,
    need_compute: bool,
    error: String,
    selected: Option<String>,
    focus: Option<i64>,
    scroll_to: Option<String>,
    pub tab: Tab,
    lapse_index: usize,
    lapse_playing: bool,
    lapse_tick: Option<std::time::Instant>,
    lapse_fps: f32,
    lapse_aligned: bool,
}

impl Default for TimeUi {
    fn default() -> Self {
        Self {
            collection: String::new(),
            person: None,
            loaded_person: None,
            people: Vec::new(),
            relatives: Vec::new(),
            raw: None,
            birth: None,
            decisions: Decisions::default(),
            computed: None,
            axis: Axis::Date,
            grain: None,
            include_auto: false,
            min_date: DateQuality::Folder,
            revision: 0,
            need_load: true,
            need_compute: false,
            error: String::new(),
            selected: None,
            focus: None,
            scroll_to: None,
            tab: Tab::Strip,
            lapse_index: 0,
            lapse_playing: false,
            lapse_tick: None,
            lapse_fps: 4.0,
            lapse_aligned: true,
        }
    }
}

impl TimeUi {
    /// Nur für Sichtprüfungen: Voreinstellungen aus Umgebungsvariablen.
    pub(super) fn from_env() -> Self {
        let mut ui = Self::default();
        if let Ok(person) = std::env::var("FOTO_IMPORTER_TIME_PERSON")
            && let Ok(person) = person.parse()
        {
            ui.person = Some(person);
        }
        if std::env::var("FOTO_IMPORTER_TIME_AXIS").as_deref() == Ok("age") {
            ui.axis = Axis::Age;
        }
        match std::env::var("FOTO_IMPORTER_TIME_GRAIN").as_deref() {
            Ok("year") => ui.grain = Some(Grain::Year),
            Ok("month") => ui.grain = Some(Grain::Month),
            _ => {}
        }
        ui
    }
    fn params(&self, min_px: u32) -> Params {
        Params {
            thresholds: Thresholds {
                include_auto: self.include_auto,
                min_date: self.min_date,
                ..Thresholds::with_min_px(min_px)
            },
            axis: self.axis,
            grain: self.grain,
            birth: self.birth,
            decisions: self.decisions.clone(),
        }
    }
    pub(super) fn failed(&mut self, error: String) {
        self.error = error;
    }
}

fn face_request(root: &str, face: &TimeFace, edge: u32) -> PreviewRequest {
    PreviewRequest {
        root: Some(PathBuf::from(root)),
        path: Path::new(root).join(&face.source),
        orientation: 1,
        crop: Some(face.bbox),
        edge,
        generation: 0,
    }
}

fn face_info(face: &TimeFace, computed: &Computed) -> String {
    let date = face.date.map_or_else(|| "ohne Datum".into(), |d| d.iso());
    let age = computed
        .age_years(face)
        .map_or(String::new(), |a| format!(" · {a} Jahre"));
    format!(
        "{date}{age}\nDatum: {}\n{} px · Frontalität {:.2} · Wert {:.2}\n{}",
        face.date_quality.label(),
        face.px.map_or("?".into(), |p| p.to_string()),
        face.frontal.unwrap_or(0.0),
        face.value,
        face.source
    )
}

/// Gestrichelter Rand für Gesichter mit unsicherem Datum.
fn dashed_border(ui: &egui::Ui, rect: egui::Rect) {
    let stroke = egui::Stroke::new(1.5_f32, theme::WARM);
    let points = vec![
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    ui.painter()
        .extend(egui::Shape::dashed_line(&points, stroke, 5.0, 4.0));
}

impl PhotoApp {
    /// Hält die Zeitreise mit Sammlung und Einstellungen im Gleichklang. Langsames läuft nur
    /// auf einem Worker; ein zweiter Auftrag startet erst, wenn der erste zurück ist.
    pub(super) fn sync_time(&mut self) {
        if self.settings.collection.is_empty() {
            return;
        }
        if self.time_ui.collection != self.settings.collection {
            let (person, axis, grain) =
                (self.time_ui.person, self.time_ui.axis, self.time_ui.grain);
            let keep = self.time_ui.collection.is_empty();
            self.time_ui = TimeUi {
                collection: self.settings.collection.clone(),
                person: if keep { person } else { None },
                axis: if keep { axis } else { Axis::Date },
                grain: if keep { grain } else { None },
                ..TimeUi::default()
            };
        }
        if self.tasks.iter().any(|t| t.kind == "timeline") {
            return;
        }
        let min_px = self.settings.timeline_min_px;
        let root = PathBuf::from(&self.settings.collection);
        if self.time_ui.need_load {
            self.time_ui.revision += 1;
            let revision = self.time_ui.revision;
            self.time_ui.need_load = false;
            self.time_ui.need_compute = false;
            let person = self.time_ui.person;
            let params = self.time_ui.params(min_px);
            self.launch("Zeitreise lesen", false, "timeline", move |control| {
                let c = Collection::open(&root, false)?;
                c.cancel_with(&control);
                control.checkpoint()?;
                let people = faces::people(&c)?;
                let person = person
                    .filter(|p| people.iter().any(|x| x.id == *p))
                    .or_else(|| people.iter().max_by_key(|p| p.count).map(|p| p.id));
                let (raw, birth, decisions, relatives) = match person {
                    Some(person) => (
                        timeline::load(&c, person)?,
                        timeline::birth(&c, person)?,
                        timeline::load_decisions(&c, person)?,
                        timeline::relatives(&relations::load_family(&c)?, person),
                    ),
                    None => Default::default(),
                };
                control.checkpoint()?;
                let params = Params {
                    birth,
                    decisions: decisions.clone(),
                    ..params
                };
                let computed = timeline::compute(&raw, &params);
                Ok(Outcome::Time(TimeOutcome::Loaded(Box::new(Loaded {
                    revision,
                    person,
                    people,
                    relatives,
                    raw: Arc::new(raw),
                    birth,
                    decisions,
                    computed,
                }))))
            });
        } else if self.time_ui.need_compute
            && let Some(raw) = self.time_ui.raw.clone()
        {
            self.time_ui.need_compute = false;
            self.time_ui.revision += 1;
            let revision = self.time_ui.revision;
            let params = self.time_ui.params(min_px);
            self.launch("Zeitreise berechnen", false, "timeline", move |control| {
                control.checkpoint()?;
                Ok(Outcome::Time(TimeOutcome::Computed {
                    revision,
                    computed: timeline::compute(&raw, &params),
                }))
            });
        }
    }

    /// Übernimmt die Antwort eines Workers der Zeitreise.
    pub(super) fn accept_time(&mut self, outcome: TimeOutcome) {
        let ui = &mut self.time_ui;
        match outcome {
            TimeOutcome::Loaded(loaded) => {
                if loaded.revision != ui.revision {
                    return;
                }
                let loaded = *loaded;
                ui.error.clear();
                ui.people = loaded.people;
                ui.relatives = loaded.relatives;
                ui.raw = Some(loaded.raw);
                ui.birth = loaded.birth;
                ui.decisions = loaded.decisions;
                ui.loaded_person = loaded.person;
                if ui.person.is_none() || ui.person == loaded.person {
                    ui.person = loaded.person;
                }
                // Die Person wurde während des Ladens gewechselt: noch einmal laden.
                ui.need_load = ui.need_load || ui.person != ui.loaded_person;
                ui.axis = loaded.computed.axis;
                ui.apply(loaded.computed);
            }
            TimeOutcome::Computed { revision, computed } => {
                if revision != ui.revision {
                    return;
                }
                ui.axis = computed.axis;
                ui.apply(computed);
            }
            TimeOutcome::Decided => ui.need_load = true,
            TimeOutcome::Exported(path) => {
                let text = path.map_or_else(
                    || "Export abgebrochen.".to_owned(),
                    |p| format!("Export gespeichert: {}", p.display()),
                );
                self.message(text, false);
            }
        }
    }

    pub(super) fn time_view(&mut self, ui: &mut egui::Ui) {
        if self.settings.collection.is_empty() {
            self.empty(
                ui,
                "Wie hat sich jemand verändert?",
                "Wähle eine Sammlung mit analysierten Gesichtern, um Personen über die Zeit zu sehen.",
            );
            return;
        }
        self.sync_time();
        self.time_header(ui);
        ui.horizontal(|ui| {
            for (tab, label) in [
                (Tab::Strip, "Zeitstrahl"),
                (Tab::Gaps, "Lücken"),
                (Tab::Lapse, "Zeitraffer & Export"),
            ] {
                if ui
                    .selectable_label(self.time_ui.tab == tab, label)
                    .clicked()
                {
                    self.time_ui.tab = tab;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("time-page")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.time_ui.tab {
                Tab::Strip => self.strip_tab(ui),
                Tab::Gaps => self.gaps_tab(ui),
                Tab::Lapse => self.lapse_tab(ui),
            });
    }

    fn decide(&mut self, decision: Decision) {
        let Some(person) = self.time_ui.loaded_person else {
            return;
        };
        let root = PathBuf::from(&self.settings.collection);
        self.launch("Zeitreise speichern", true, "mutation", move |_| {
            let c = Collection::open(&root, true)?;
            match decision {
                Decision::Pin(key, face) => timeline::pin(&c, person, &key, Some(face))?,
                Decision::Unpin(key) => timeline::pin(&c, person, &key, None)?,
                Decision::Hide(face, hidden) => timeline::set_hidden(&c, person, face, hidden)?,
                Decision::BirthMonth(month) => timeline::set_birth_month(&c, person, month)?,
            }
            Ok(Outcome::Time(TimeOutcome::Decided))
        });
    }

    fn time_header(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Zeitreise").size(23.0).strong());
        ui.add_space(8.0);
        let items: Vec<Choice<i64>> = select::person_choices(&self.time_ui.people, None);
        let mut person = self.time_ui.person;
        let (mut load, mut compute) = (false, false);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Person").strong());
            if FilterSelect::new("time-person", &items)
                .placeholder("Person wählen")
                .width(260.0)
                .show(ui, &mut person)
            {
                load = true;
            }
            let birth = self.time_ui.birth;
            ui.separator();
            ui.label("Achse");
            let age_ok = birth.is_some();
            if ui
                .selectable_label(self.time_ui.axis == Axis::Date, "Datum")
                .clicked()
                && self.time_ui.axis != Axis::Date
            {
                self.time_ui.axis = Axis::Date;
                compute = true;
            }
            if ui
                .add_enabled(
                    age_ok,
                    egui::Button::selectable(self.time_ui.axis == Axis::Age, "Alter"),
                )
                .on_disabled_hover_text(
                    "Für das Alter braucht die Person ein Geburtsjahr (Personen & Beziehungen).",
                )
                .clicked()
                && self.time_ui.axis != Axis::Age
            {
                self.time_ui.axis = Axis::Age;
                compute = true;
            }
            ui.separator();
            ui.label("Körnung");
            for (grain, label) in [
                (None, "Automatisch"),
                (Some(Grain::Year), "Jahre"),
                (Some(Grain::Month), "Monate"),
            ] {
                if ui
                    .selectable_label(self.time_ui.grain == grain, label)
                    .clicked()
                    && self.time_ui.grain != grain
                {
                    self.time_ui.grain = grain;
                    compute = true;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Mindestauflösung");
            let (low, _, high) = config::TIMELINE_MIN_PX;
            let response = ui
                .add(egui::Slider::new(&mut self.settings.timeline_min_px, low..=high).suffix(" px"))
                .on_hover_text("Kürzere Gesichtsseite im Originalbild; kleinere Gesichter werden nicht gezeigt.");
            if response.changed() {
                compute = true;
            }
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                self.save();
            }
            if ui
                .checkbox(&mut self.time_ui.include_auto, "auch automatisch zugeordnete")
                .changed()
            {
                compute = true;
            }
            ui.label("Datum mindestens");
            let before = self.time_ui.min_date;
            egui::ComboBox::from_id_salt("time-date-quality")
                .selected_text(self.time_ui.min_date.label())
                .show_ui(ui, |ui| {
                    for quality in [DateQuality::Exif, DateQuality::Folder, DateQuality::File] {
                        ui.selectable_value(&mut self.time_ui.min_date, quality, quality.label());
                    }
                });
            if before != self.time_ui.min_date {
                compute = true;
            }
            if let Some(birth) = self.time_ui.birth {
                ui.separator();
                ui.label(format!("Geboren {}", birth.year));
                let mut month = birth.month;
                egui::ComboBox::from_id_salt("time-birth-month")
                    .selected_text(month.map_or("Monat unbekannt".to_owned(), |m| {
                        timeline::label(Axis::Date, Grain::Month, 2000 * 12 + m as i32 - 1)
                            .split(' ')
                            .next()
                            .unwrap_or("")
                            .to_owned()
                    }))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut month, None, "Monat unbekannt");
                        for m in 1..=12u8 {
                            let name = timeline::label(Axis::Date, Grain::Month, 2000 * 12 + m as i32 - 1);
                            ui.selectable_value(&mut month, Some(m), name.split(' ').next().unwrap_or(""));
                        }
                    })
                    .response
                    .on_hover_text("Optional: macht das Alter auf Monate genau. Gilt nur lokal in dieser Sammlung.");
                if month != birth.month {
                    self.decide(Decision::BirthMonth(month));
                }
            }
        });
        if person != self.time_ui.person {
            self.time_ui.person = person;
            self.time_ui.selected = None;
            self.time_ui.focus = None;
        }
        if load {
            self.time_ui.need_load = true;
        }
        if compute {
            self.time_ui.need_compute = true;
        }
        let working = self.tasks.iter().any(|t| t.kind == "timeline");
        ui.horizontal_wrapped(|ui| {
            if working {
                ui.spinner();
            }
            if !self.time_ui.error.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_RED, &self.time_ui.error);
            }
            if let Some(computed) = &self.time_ui.computed {
                let rejected: Vec<String> = computed
                    .rejected
                    .iter()
                    .map(|(reason, n)| format!("{n} {}", reason.label()))
                    .collect();
                ui.label(
                    RichText::new(format!(
                        "{} von {} Gesichtern geeignet{}{}",
                        computed.items.len(),
                        computed.total,
                        if rejected.is_empty() {
                            ""
                        } else {
                            " · aussortiert: "
                        },
                        rejected.join(", ")
                    ))
                    .color(theme::MUTED),
                );
            }
        });
    }

    /// Eine Gesichtskachel; lädt nur, wenn sie sichtbar ist.
    fn face_tile(
        &mut self,
        ui: &mut egui::Ui,
        face: &TimeFace,
        size: f32,
        info: &str,
    ) -> egui::Response {
        let (root, exiftool) = (
            self.settings.collection.clone(),
            self.settings.exiftool.clone(),
        );
        let response = self.thumbnails.image(
            ui,
            face_request(&root, face, 160),
            &exiftool,
            egui::vec2(size, size),
        );
        if face.date_quality.uncertain() {
            dashed_border(ui, response.rect);
        }
        response.on_hover_text(info)
    }

    fn strip_tab(&mut self, ui: &mut egui::Ui) {
        let Some(computed) = self.time_ui.computed.clone() else {
            ui.add_space(30.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Zeitreise wird gelesen …");
            });
            return;
        };
        if computed.sections.is_empty() {
            ui.add_space(20.0);
            ui.label(
                "Für diese Person gibt es mit den gewählten Schwellen kein geeignetes Gesicht. \
                 Senke die Mindestauflösung oder lasse unsicherere Datumsquellen zu.",
            );
            return;
        }
        self.density_curve(ui, &computed);
        ui.add_space(6.0);
        let scroll_to = self.time_ui.scroll_to.take();
        let mut clicked = None;
        egui::ScrollArea::horizontal()
            .id_salt("time-strip")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    for section in &computed.sections {
                        let selected = self.time_ui.selected.as_deref() == Some(&section.key);
                        let rect = ui
                            .vertical(|ui| {
                                ui.set_width(TILE + 8.0);
                                self.strip_cell(ui, &computed, section, selected, &mut clicked);
                            })
                            .response
                            .rect;
                        if scroll_to.as_deref() == Some(&section.key) {
                            ui.scroll_to_rect(rect, Some(egui::Align::Center));
                        }
                    }
                });
            });
        if let Some(key) = clicked {
            self.time_ui.focus = None;
            self.time_ui.selected = Some(key);
        }
        ui.separator();
        self.section_detail(ui, &computed);
    }

    fn strip_cell(
        &mut self,
        ui: &mut egui::Ui,
        computed: &Computed,
        section: &Section,
        selected: bool,
        clicked: &mut Option<String>,
    ) {
        match section.best {
            Some(best) => {
                let face = computed.face(best).clone();
                let info = face_info(&face, computed);
                let response = self.face_tile(ui, &face, TILE, &info);
                if selected {
                    ui.painter().rect_stroke(
                        response.rect.expand(2.0),
                        6,
                        egui::Stroke::new(2.0_f32, theme::ACCENT),
                        egui::StrokeKind::Outside,
                    );
                }
                if response.clicked() {
                    *clicked = Some(section.key.clone());
                }
            }
            None => {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(TILE, TILE), egui::Sense::click());
                ui.painter().rect_stroke(
                    rect,
                    8,
                    egui::Stroke::new(1.0_f32, theme::MUTED),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "ausgeblendet",
                    egui::FontId::proportional(12.0),
                    theme::MUTED,
                );
                if response.clicked() {
                    *clicked = Some(section.key.clone());
                }
            }
        }
        let visible = section
            .members
            .iter()
            .filter(|m| !computed.items[**m].hidden)
            .count();
        let mut text = RichText::new(&section.label).strong();
        if selected {
            text = text.color(theme::ACCENT);
        }
        ui.label(text);
        ui.label(
            RichText::new(format!(
                "{visible} Gesicht{}{}",
                if visible == 1 { "" } else { "er" },
                if section.pinned {
                    " · festgenagelt"
                } else {
                    ""
                }
            ))
            .size(theme::MIN_TEXT)
            .color(theme::MUTED),
        );
    }

    /// Dichtekurve: Gesichter je Abschnitt; Lücken sind unten markiert; Klick wählt.
    fn density_curve(&mut self, ui: &mut egui::Ui, computed: &Computed) {
        let density = &computed.density;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 72.0), egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 6, ui.visuals().extreme_bg_color);
        let bins = density.counts.len().max(1);
        let peak = density.counts.iter().copied().max().unwrap_or(1).max(1) as f32;
        let plot = rect.shrink2(egui::vec2(6.0, 8.0));
        let step = plot.width() / bins as f32;
        for (index, count) in density.counts.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            let height = (plot.height() - 6.0) * (*count as f32 / peak);
            let x = plot.left() + step * index as f32;
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x, plot.bottom() - height),
                    egui::pos2((x + step).max(x + 1.0), plot.bottom()),
                ),
                1,
                theme::ACCENT.gamma_multiply(0.75),
            );
        }
        for gap in &computed.gaps {
            let from = (gap.from - density.first).max(0) as f32;
            let to = (gap.to - density.first + 1) as f32;
            painter.line_segment(
                [
                    egui::pos2(plot.left() + step * from, plot.bottom() + 4.0),
                    egui::pos2(plot.left() + step * to, plot.bottom() + 4.0),
                ],
                egui::Stroke::new(3.0_f32, theme::WARM),
            );
        }
        if let Some(key) = &self.time_ui.selected
            && let Some(section) = computed.sections.iter().find(|s| &s.key == key)
        {
            let x = plot.left() + step * (section.ord - density.first) as f32;
            painter.rect_stroke(
                egui::Rect::from_min_max(
                    egui::pos2(x - 1.0, rect.top() + 2.0),
                    egui::pos2(x + step.max(2.0) + 1.0, rect.bottom() - 2.0),
                ),
                2,
                egui::Stroke::new(1.5_f32, theme::WARM),
                egui::StrokeKind::Inside,
            );
        }
        let hovered = response.hover_pos().and_then(|pos| {
            let index = ((pos.x - plot.left()) / step).floor();
            (index >= 0.0 && (index as usize) < bins).then_some(index as i32 + density.first)
        });
        if let Some(ord) = hovered {
            let count = density.counts[(ord - density.first) as usize];
            let label = timeline::label(computed.axis, computed.grain, ord);
            let response = response.clone().on_hover_text(format!(
                "{label} · {count} geeignete Gesicht{}",
                if count == 1 { "" } else { "er" }
            ));
            if response.clicked()
                && let Some(section) = computed.sections.iter().find(|s| s.ord == ord)
            {
                self.time_ui.selected = Some(section.key.clone());
                self.time_ui.scroll_to = Some(section.key.clone());
                self.time_ui.focus = None;
            }
        }
        ui.label(
            RichText::new(
                "Gesichter je Abschnitt · orange Linie unten: Lücke ohne brauchbares Gesicht · gestrichelter Rand: Datum unsicher",
            )
            .size(theme::MIN_TEXT)
            .color(theme::MUTED),
        );
    }

    fn section_detail(&mut self, ui: &mut egui::Ui, computed: &Computed) {
        let Some(section) = self
            .time_ui
            .selected
            .as_ref()
            .and_then(|key| computed.sections.iter().find(|s| &s.key == key))
            .cloned()
        else {
            ui.label(
                RichText::new("Klicke eine Kachel oder die Kurve, um alle Gesichter eines Abschnitts zu sehen.")
                    .color(theme::MUTED),
            );
            return;
        };
        let mut decision = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&section.label).size(20.0).strong());
            ui.label(
                RichText::new(format!("{} geeignete Gesichter", section.members.len()))
                    .color(theme::MUTED),
            );
            if section.pinned
                && ui
                    .button("Festlegung aufheben")
                    .on_hover_text("Wieder das automatisch beste Gesicht wählen")
                    .clicked()
            {
                decision = Some(Decision::Unpin(section.key.clone()));
            }
        });
        let focus = self.time_ui.focus;
        let mut focus_next = None;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width((ui.available_width() - 400.0).max(260.0));
                ui.horizontal_wrapped(|ui| {
                    for member in &section.members {
                        let item = &computed.items[*member];
                        let face = item.face.clone();
                        ui.vertical(|ui| {
                            ui.set_width(TILE);
                            let info = face_info(&face, computed);
                            let response = self.face_tile(ui, &face, TILE, &info);
                            if item.hidden {
                                ui.painter().rect_filled(
                                    response.rect,
                                    8,
                                    egui::Color32::from_black_alpha(170),
                                );
                            }
                            if Some(face.id) == focus || Some(*member) == section.best {
                                ui.painter().rect_stroke(
                                    response.rect.expand(2.0),
                                    6,
                                    egui::Stroke::new(
                                        2.0_f32,
                                        if Some(face.id) == focus { theme::ACCENT } else { theme::WARM },
                                    ),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            if response.clicked() {
                                focus_next = Some(face.id);
                            }
                            ui.horizontal(|ui| {
                                if item.hidden {
                                    if ui.small_button("Einblenden").clicked() {
                                        decision = Some(Decision::Hide(face.id, false));
                                    }
                                } else {
                                    let is_best = Some(*member) == section.best;
                                    if ui
                                        .add_enabled(!(is_best && section.pinned), egui::Button::new("Festnageln").small())
                                        .on_hover_text("Dieses Gesicht für den Abschnitt wählen (bleibt erhalten)")
                                        .clicked()
                                    {
                                        decision = Some(Decision::Pin(section.key.clone(), face.id));
                                    }
                                    if ui
                                        .small_button("Aus")
                                        .on_hover_text("Gesicht aus der Zeitreise ausblenden")
                                        .clicked()
                                    {
                                        decision = Some(Decision::Hide(face.id, true));
                                    }
                                }
                            });
                        });
                    }
                });
            });
            if let Some(face_id) = focus
                && let Some(item) = computed.items.iter().find(|i| i.face.id == face_id)
            {
                let (root, exiftool) = (self.settings.collection.clone(), self.settings.exiftool.clone());
                ui.vertical(|ui| {
                    self.thumbnails.image(
                        ui,
                        PreviewRequest {
                            root: Some(PathBuf::from(&root)),
                            path: Path::new(&root).join(&item.face.source),
                            orientation: 1,
                            crop: None,
                            edge: 800,
                            generation: 0,
                        },
                        &exiftool,
                        egui::vec2(380.0, 285.0),
                    );
                    ui.label(RichText::new(face_info(&item.face, computed)).size(theme::MIN_TEXT).color(theme::MUTED));
                });
            }
        });
        if focus_next.is_some() {
            self.time_ui.focus = focus_next;
        }
        if let Some(decision) = decision {
            self.decide(decision);
        }
    }
}

impl TimeUi {
    fn apply(&mut self, computed: Computed) {
        if let Some(key) = &self.selected
            && !computed.sections.iter().any(|s| &s.key == key)
        {
            self.selected = None;
            self.focus = None;
        }
        self.computed = Some(Arc::new(computed));
    }
}

/// Art eines Exports.
#[derive(Clone, Copy)]
enum Export {
    Collage,
    Contact,
    Gif,
    Sequence,
}

impl PhotoApp {
    /// Die gewählten Gesichter (je Abschnitt eines, ausgeblendete fehlen), älteste zuerst.
    fn lapse_tiles(&self, computed: &Computed) -> Vec<timeline_export::Tile> {
        computed
            .sections
            .iter()
            .filter_map(|s| {
                let face = computed.face(s.best?).clone();
                Some(timeline_export::Tile {
                    label: timeline_export::plain_label(computed.axis, computed.grain, s.ord),
                    face,
                })
            })
            .collect()
    }

    fn start_export(&mut self, computed: &Computed, kind: Export) {
        if self.tasks.iter().any(|t| t.kind == "timeline-export") {
            return;
        }
        let tiles = self.lapse_tiles(computed);
        let rows: Vec<(String, Vec<TimeFace>)> = computed
            .years
            .iter()
            .map(|s| {
                let faces: Vec<TimeFace> = s
                    .members
                    .iter()
                    .filter(|m| !computed.items[**m].hidden)
                    .take(10)
                    .map(|m| computed.items[*m].face.clone())
                    .collect();
                (
                    timeline_export::plain_label(computed.axis, Grain::Year, s.ord),
                    faces,
                )
            })
            .filter(|r| !r.1.is_empty())
            .collect();
        let name = self
            .time_ui
            .person
            .and_then(|id| self.time_ui.people.iter().find(|p| p.id == id))
            .map_or_else(|| "Person".to_owned(), |p| p.name.clone());
        let stem = format!("Zeitreise-{}", timeline_export::safe_stem(&name));
        let (root, exiftool) = (
            PathBuf::from(&self.settings.collection),
            self.settings.exiftool.clone(),
        );
        let (fps, aligned) = (self.time_ui.lapse_fps, self.time_ui.lapse_aligned);
        self.launch(
            "Zeitreise exportieren",
            false,
            "timeline-export",
            move |control| {
                let Some(dir) = rfd::FileDialog::new()
                    .set_title("Ordner für den Export wählen")
                    .pick_folder()
                else {
                    return Ok(Outcome::Time(TimeOutcome::Exported(None)));
                };
                let path = match kind {
                    Export::Collage => {
                        let image = timeline_export::collage(
                            &root, &tiles, 6, 160, aligned, &exiftool, &control,
                        )?;
                        timeline_export::save_png(&image, &dir, &stem)?
                    }
                    Export::Contact => {
                        let image =
                            timeline_export::contact_sheet(&root, &rows, 120, &exiftool, &control)?;
                        timeline_export::save_png(&image, &dir, &format!("{stem}-Kontaktbogen"))?
                    }
                    Export::Gif | Export::Sequence => {
                        let mut frames = Vec::new();
                        control
                            .total
                            .store(tiles.len(), std::sync::atomic::Ordering::Relaxed);
                        for (index, tile) in tiles.iter().enumerate() {
                            control.checkpoint()?;
                            control
                                .done
                                .store(index, std::sync::atomic::Ordering::Relaxed);
                            let mut frame = timeline_export::render_face(
                                &root, &tile.face, 360, aligned, &exiftool, &control,
                            )?;
                            timeline_export::draw_text(&mut frame, 10, 10, &tile.label, 4);
                            frames.push(frame);
                        }
                        if matches!(kind, Export::Gif) {
                            timeline_export::timelapse_gif(&frames, fps, &dir, &stem)?
                        } else {
                            timeline_export::timelapse_sequence(&frames, &dir, &stem)?
                        }
                    }
                };
                Ok(Outcome::Time(TimeOutcome::Exported(Some(path))))
            },
        );
    }

    fn lapse_tab(&mut self, ui: &mut egui::Ui) {
        let Some(computed) = self.time_ui.computed.clone() else {
            ui.add_space(20.0);
            ui.label("Zeitreise wird gelesen …");
            return;
        };
        let tiles = self.lapse_tiles(&computed);
        if tiles.is_empty() {
            ui.add_space(20.0);
            ui.label("Keine geeigneten Gesichter für Zeitraffer oder Export.");
            return;
        }
        self.time_ui.lapse_index = self.time_ui.lapse_index.min(tiles.len() - 1);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let label = if self.time_ui.lapse_playing {
                "Pause"
            } else {
                "Abspielen"
            };
            if ui.button(label).clicked() {
                self.time_ui.lapse_playing = !self.time_ui.lapse_playing;
                self.time_ui.lapse_tick = Some(std::time::Instant::now());
            }
            let mut index = self.time_ui.lapse_index;
            ui.add(egui::Slider::new(&mut index, 0..=tiles.len() - 1).show_value(false));
            self.time_ui.lapse_index = index;
            ui.label("Bilder je Sekunde");
            ui.add(egui::Slider::new(&mut self.time_ui.lapse_fps, 1.0..=12.0));
            ui.checkbox(&mut self.time_ui.lapse_aligned, "Augen ausrichten")
                .on_hover_text(
                    "Im Export: Gesichter an den Augen ausrichten, sofern Landmarks vorliegen.",
                );
        });
        if self.time_ui.lapse_playing {
            let step = std::time::Duration::from_secs_f32(1.0 / self.time_ui.lapse_fps);
            let due = self.time_ui.lapse_tick.is_none_or(|t| t.elapsed() >= step);
            if due {
                self.time_ui.lapse_index = (self.time_ui.lapse_index + 1) % tiles.len();
                self.time_ui.lapse_tick = Some(std::time::Instant::now());
            }
            ui.ctx().request_repaint_after(step);
        }
        let tile = &tiles[self.time_ui.lapse_index];
        let info = face_info(&tile.face, &computed);
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            self.face_tile(ui, &tile.face, 280.0, &info);
            ui.vertical(|ui| {
                ui.label(RichText::new(&tile.label).size(24.0).strong());
                ui.label(
                    RichText::new(format!(
                        "Bild {} von {}",
                        self.time_ui.lapse_index + 1,
                        tiles.len()
                    ))
                    .color(theme::MUTED),
                );
            });
        });
        ui.add_space(14.0);
        ui.separator();
        ui.label(RichText::new("Export").strong());
        ui.label(
            RichText::new(
                "Es entstehen nur neue Dateien im gewählten Ordner; nichts wird ersetzt oder verändert.",
            )
            .size(theme::MIN_TEXT)
            .color(theme::MUTED),
        );
        let busy = self.tasks.iter().any(|t| t.kind == "timeline-export");
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!busy, |ui| {
                for (kind, label) in [
                    (Export::Collage, "Collage (PNG)"),
                    (Export::Contact, "Kontaktbogen (PNG)"),
                    (Export::Gif, "Zeitraffer (GIF)"),
                    (Export::Sequence, "Zeitraffer (PNG-Bildfolge)"),
                ] {
                    if ui.button(label).clicked() {
                        self.start_export(&computed, kind);
                    }
                }
            });
            if busy {
                ui.spinner();
            }
        });
    }

    fn gaps_tab(&mut self, ui: &mut egui::Ui) {
        let Some(computed) = self.time_ui.computed.clone() else {
            ui.add_space(20.0);
            ui.label("Zeitreise wird gelesen …");
            return;
        };
        ui.add_space(8.0);
        if computed.gaps.is_empty() {
            ui.label("Keine Lücken: Die Person hat in jedem Abschnitt ein geeignetes Gesicht.");
            return;
        }
        ui.label(
            RichText::new(
                "Zeiträume ohne brauchbares Gesicht, mit der Zahl vorhandener, aber aussortierter Gesichter (zu klein, geringe Güte oder nicht frontal).",
            )
            .color(theme::MUTED),
        );
        ui.add_space(6.0);
        for gap in &computed.gaps {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} bis {}", gap.before, gap.after)).strong());
                ui.label(format!("Lücke {} bis {}", gap.start, gap.end));
                let (text, color) = if gap.candidates == 0 {
                    ("keine aussortierten Gesichter".to_owned(), theme::MUTED)
                } else {
                    (
                        format!(
                            "{} aussortierte Gesichter in diesem Zeitraum",
                            gap.candidates
                        ),
                        theme::WARM,
                    )
                };
                ui.label(RichText::new(text).color(color));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn settle(app: &mut PhotoApp, ctx: &egui::Context) {
        for _ in 0..3000 {
            app.sync_time();
            app.poll(ctx);
            if app.tasks.is_empty() && !app.time_ui.need_load && !app.time_ui.need_compute {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn timeline_loads_and_recomputes_on_workers_and_drops_stale_answers() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        {
            let c = Collection::open(root.path(), true)?;
            c.db.execute("INSERT INTO persons(id) VALUES(1)", [])?;
            c.db.execute("INSERT INTO person_attrs VALUES(1,?,?)", ["name", "Anna"])?;
            c.db.execute(
                "INSERT INTO person_attrs VALUES(1,?,?)",
                ["birth_year", "2010"],
            )?;
            for (id, year) in [(1, 2015), (2, 2017), (3, 2020)] {
                let rel = format!("{year}/{id}.jpg");
                c.db.execute(
                    "INSERT INTO photos(id,status) VALUES(?,?)",
                    rusqlite::params![id, "present"],
                )?;
                c.db.execute("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status,meta_state) VALUES(?,?,?,?,?,?,?,1,0,?,?,?)", rusqlite::params![id, rel, rel, year.to_string(), format!("{id}.jpg"), id.to_string(), "jpg", id, "present", "done"])?;
                c.db.execute("INSERT INTO meta(file_id,dt_original,width,height,orientation) VALUES(?,?,800,600,1)", rusqlite::params![id, format!("{year}:06:01 12:00:00")])?;
                c.db.execute("INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,person_id,assignment) VALUES(?,?,?,?,?,0.3,0.2,0.7,0.8,?,0.95,0.9,1,?)", rusqlite::params![id, id, id, rel, "d", "[[300.0,200.0],[400.0,200.0],[350.0,260.0],[310.0,320.0],[390.0,320.0]]", "user"])?;
            }
        }
        let ctx = egui::Context::default();
        let creation = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = PhotoApp::with_settings(&creation, Settings::default());
        app.settings.collection = root.path().to_string_lossy().into_owned();
        settle(&mut app, &ctx);
        // Die Person mit den meisten Gesichtern wird vorgewählt, alles kam vom Worker.
        assert_eq!(app.time_ui.person, Some(1));
        let computed = app.time_ui.computed.clone().expect("computed");
        assert_eq!(computed.sections.len(), 3);
        assert_eq!(computed.axis, Axis::Date);
        // Achse wechseln rechnet auf einem Worker neu, ohne die Datenbank zu lesen.
        app.time_ui.axis = Axis::Age;
        app.time_ui.need_compute = true;
        settle(&mut app, &ctx);
        assert_eq!(
            app.time_ui.computed.as_ref().map(|c| c.axis),
            Some(Axis::Age)
        );
        // Eine Antwort mit veralteter Revision wird verworfen.
        let before = app.time_ui.computed.clone().expect("computed");
        let stale = timeline::compute(app.time_ui.raw.as_ref().expect("raw"), &Params::default());
        let revision = app.time_ui.revision.saturating_sub(1);
        app.accept_time(TimeOutcome::Computed {
            revision,
            computed: stale,
        });
        assert!(Arc::ptr_eq(
            &before,
            app.time_ui.computed.as_ref().expect("computed")
        ));
        Ok(())
    }
}
