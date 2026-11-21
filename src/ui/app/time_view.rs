//! Ansicht „Zeitreise“ der gewählten Person (ADR 0069): ihre Gesichter über die Zeit. Domainlogik steht in
//! `core::timeline`; diese Datei zeichnet und startet Worker. Abfrage und Berechnung laufen nie
//! auf dem UI-Thread; veraltete Antworten werden über Revisionen verworfen. Kacheln laden über
//! den vorhandenen Vorschau-Cache nur, wenn sie sichtbar sind.

use super::faces_view::{SINGLE_NAMING_HINT, single_naming_menu};
use super::*;
use crate::core::timeline_export;
use crate::core::{
    faces::Person,
    revision::{Domain, Key, Revisions},
    timeline::{
        self, Axis, Birth, Computed, DateQuality, Decisions, Grain, Params, PersonData, Section,
        Thresholds, TimeFace,
    },
};
use crate::ui::select::{self, Choice, FilterSelect};
use std::collections::VecDeque;
use std::sync::Arc;

const TILE: f32 = 112.0;

#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum Tab {
    #[default]
    Strip,
    Gaps,
    Lapse,
}

/// Bereiche der Datenrevision, aus denen die Zeitreise liest: Zuordnungen und Entscheidungen
/// (Gesichter, `face_settings`) sowie Personen, Geburtsangaben und Verwandte. Metadaten
/// (`files`) gehören nicht dazu: ein Hintergrundlauf der Metadaten soll die gezeigte Person
/// nicht laufend neu lesen.
const DOMAINS: &[Domain] = &[Domain::Faces, Domain::People];

/// Gelesene Daten einer Person; bleiben je Person im Speicher, solange der Datenstand
/// (`key`) der zuletzt gesehene ist.
#[derive(Clone)]
pub(super) struct PersonCache {
    raw: Arc<Vec<TimeFace>>,
    birth: Option<Birth>,
    decisions: Decisions,
    /// Datenstand beim Lesen; `None` ohne Revision (dann nie gemerkt).
    key: Option<Key>,
}

impl From<PersonData> for PersonCache {
    fn from(data: PersonData) -> Self {
        Self {
            raw: Arc::new(data.raw),
            birth: data.birth,
            decisions: data.decisions,
            key: data.revisions.map(|r| r.key(DOMAINS)),
        }
    }
}

/// Ergebnis des Ladeauftrags: Gesichter der gewählten Person und die erste Berechnung.
pub(super) struct Loaded {
    revision: u64,
    person: i64,
    data: PersonCache,
    computed: Computed,
}

pub(super) enum TimeOutcome {
    /// Personenliste der Auswahl (klein, eigener Auftrag, nie mit Gesichtern zusammen).
    People(u64, Vec<Person>),
    Loaded(Box<Loaded>),
    Computed {
        revision: u64,
        computed: Computed,
    },
    /// Eine Nutzerentscheidung ist gespeichert; Datenstand nach dem Schreiben.
    Decided(Option<Revisions>),
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
    pub(super) loaded_person: Option<i64>,
    people: Vec<Person>,
    /// Personenliste frisch aus der Sammlung (sonst aus dem lokalen Cache oder leer).
    people_fresh: bool,
    pub(super) need_people: bool,
    people_revision: u64,
    raw: Option<Arc<Vec<TimeFace>>>,
    birth: Option<Birth>,
    decisions: Decisions,
    computed: Option<Arc<Computed>>,
    /// Zuletzt gelesene Personen (neueste hinten), höchstens `TIMELINE_CACHED_PEOPLE`, alle
    /// zum Datenstand `known`.
    cache: VecDeque<(i64, PersonCache)>,
    /// Neuester gesehener Datenstand ([`DOMAINS`]); aus Gesichterseite, Ladeaufträgen und
    /// gespeicherten Entscheidungen.
    known: Option<Key>,
    /// Datenstand der gezeigten Person.
    shown_key: Option<Key>,
    pub axis: Axis,
    pub grain: Option<Grain>,
    include_auto: bool,
    min_date: DateQuality,
    revision: u64,
    pub(super) need_load: bool,
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
            people_fresh: false,
            need_people: true,
            people_revision: 0,
            raw: None,
            birth: None,
            decisions: Decisions::default(),
            computed: None,
            cache: VecDeque::new(),
            known: None,
            shown_key: None,
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
    /// Ein gelesener Datenstand der Sammlung. Ist er neuer als der bekannte, gelten gemerkte
    /// Personen und die Personenliste nicht mehr; die gezeigte Person wird neu gelesen (sie
    /// bleibt bis dahin sichtbar). Ältere oder gleiche Stände ändern nichts.
    pub(super) fn observe(&mut self, revisions: &Revisions) {
        self.observe_key(revisions.key(DOMAINS));
    }
    fn observe_key(&mut self, key: Key) {
        if let Some(known) = &self.known
            && (*known == key || key.older_than(known))
        {
            return;
        }
        let first = self.known.is_none();
        self.known = Some(key);
        let known = self.known.clone();
        self.cache.retain(|(_, data)| data.key == known);
        if !first {
            self.need_people = true;
        }
        if self.loaded_person.is_some() && self.shown_key != known {
            self.need_load = true;
        }
    }
    /// Ist dies der neueste gesehene Datenstand?
    #[cfg(test)]
    pub(super) fn knows(&self, revisions: &Revisions) -> bool {
        self.known.as_ref() == Some(&revisions.key(DOMAINS))
    }
    /// Darf ein Ergebnis zu diesem Stand gemerkt werden? Nur zum neuesten bekannten.
    fn current(&self, key: Option<&Key>) -> bool {
        key.is_some() && key == self.known.as_ref()
    }
    fn cached(&self, person: i64) -> Option<PersonCache> {
        self.cache
            .iter()
            .find(|(id, _)| *id == person)
            .map(|(_, data)| data.clone())
    }
    fn remember(&mut self, person: i64, data: PersonCache) {
        if !self.current(data.key.as_ref()) {
            return;
        }
        self.cache.retain(|(id, _)| *id != person);
        self.cache.push_back((person, data));
        while self.cache.len() > config::TIMELINE_CACHED_PEOPLE {
            self.cache.pop_front();
        }
    }
    /// Zeigt die Daten dieser Person (Berechnung folgt).
    fn show(&mut self, person: i64, data: PersonCache) {
        self.loaded_person = Some(person);
        self.shown_key = data.key;
        self.raw = Some(data.raw);
        self.birth = data.birth;
        self.decisions = data.decisions;
    }
    /// Nichts von einer anderen Person zeigen, solange die gewählte gelesen wird.
    fn clear_shown(&mut self) {
        self.loaded_person = None;
        self.shown_key = None;
        self.raw = None;
        self.birth = None;
        self.decisions = Decisions::default();
        self.computed = None;
    }
    /// Personen der Auswahl: frisch gelesen, sonst die der Personenansichten, sonst die
    /// zuletzt lokal gespeicherte Liste.
    fn choices<'a>(&'a self, shared: &'a [Person]) -> &'a [Person] {
        if self.people_fresh || shared.is_empty() {
            &self.people
        } else {
            shared
        }
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

/// Anzahl Kacheln je Zeile für die verfügbare Breite (mindestens eine).
fn tile_columns(width: f32, spacing: f32) -> usize {
    (((width + spacing) / (TILE + spacing)).floor() as usize).max(1)
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
    /// auf Workern: die Personenliste in einem eigenen kleinen Auftrag, die Gesichter nur für
    /// die gewählte Person. Ein neuer Ladeauftrag ersetzt den laufenden (Revision).
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
                people: timeline::cached_people(&self.cache_dir, &self.settings.collection)
                    .unwrap_or_default(),
                ..TimeUi::default()
            };
        }
        self.sync_time_people();
        let min_px = self.settings.timeline_min_px;
        if self.time_ui.need_load {
            self.time_ui.need_load = false;
            self.time_ui.need_compute = false;
            let Some(person) = self.time_ui.person else {
                // Ohne gewählte Person wird nichts Schweres gelesen.
                self.time_ui.revision += 1;
                self.time_ui.clear_shown();
                return;
            };
            match self.time_ui.cached(person) {
                Some(data) => {
                    self.time_ui.show(person, data);
                    self.launch_time_compute(min_px);
                }
                None => self.launch_time_load(person, min_px),
            }
            return;
        }
        // Eine Berechnung würde einen laufenden Ladeauftrag ersetzen: erst danach rechnen.
        if self.time_ui.need_compute && !self.tasks.iter().any(|t| t.kind == JobKind::Timeline) {
            self.launch_time_compute(min_px);
        }
    }

    /// Liest die Personenliste (kleine Tabellen, kalt auf der USB-Platte rund 0,15 s) und legt
    /// sie für den nächsten Start im lokalen Cache ab.
    fn sync_time_people(&mut self) {
        if !self.time_ui.need_people || self.tasks.iter().any(|t| t.kind == JobKind::TimelinePeople)
        {
            return;
        }
        self.time_ui.need_people = false;
        self.time_ui.people_revision += 1;
        let revision = self.time_ui.people_revision;
        let root = PathBuf::from(&self.settings.collection);
        let (cache, collection) = (self.cache_dir.clone(), self.settings.collection.clone());
        self.launch(
            "Personen für die Zeitreise lesen",
            false,
            JobKind::TimelinePeople,
            move |control| {
                let c = Collection::open(&root, false)?;
                c.cancel_with(&control);
                let people = faces::people(&c)?;
                timeline::store_people(&cache, &collection, &people);
                Ok(Outcome::Time(TimeOutcome::People(revision, people)))
            },
        );
    }

    /// Liest die Gesichter nur dieser Person; ersetzt einen laufenden Ladeauftrag.
    fn launch_time_load(&mut self, person: i64, min_px: u32) {
        self.time_ui.revision += 1;
        let revision = self.time_ui.revision;
        // Dieselbe Person bleibt sichtbar, bis ihr neuer Stand kommt; eine andere nie.
        if self.time_ui.loaded_person != Some(person) {
            self.time_ui.clear_shown();
        }
        let params = self.time_ui.params(min_px);
        let root = PathBuf::from(&self.settings.collection);
        self.launch(
            "Zeitreise lesen",
            false,
            JobKind::Timeline,
            move |control| {
                let c = Collection::open(&root, false)?;
                c.cancel_with(&control);
                control.phase("Gesichter der Person lesen", "");
                let data = timeline::load_person(&c, person)?;
                control.checkpoint()?;
                control.phase("Zeitreise berechnen", "");
                let params = Params {
                    birth: data.birth,
                    decisions: data.decisions.clone(),
                    ..params
                };
                let computed = timeline::compute(&data.raw, &params);
                Ok(Outcome::Time(TimeOutcome::Loaded(Box::new(Loaded {
                    revision,
                    person,
                    data: data.into(),
                    computed,
                }))))
            },
        );
    }

    /// Rechnet die geladenen Gesichter mit den aktuellen Einstellungen neu (ohne Datenbank).
    fn launch_time_compute(&mut self, min_px: u32) {
        let Some(raw) = self.time_ui.raw.clone() else {
            return;
        };
        self.time_ui.need_compute = false;
        self.time_ui.revision += 1;
        let revision = self.time_ui.revision;
        let params = self.time_ui.params(min_px);
        self.launch(
            "Zeitreise berechnen",
            false,
            JobKind::Timeline,
            move |control| {
                control.checkpoint()?;
                Ok(Outcome::Time(TimeOutcome::Computed {
                    revision,
                    computed: timeline::compute(&raw, &params),
                }))
            },
        );
    }

    /// Öffnet die Zeitreise für diese Person; geladen wird auf einem Worker.
    pub(super) fn show_time_for(&mut self, person: i64) {
        if self.time_ui.person != Some(person) {
            self.time_ui.person = Some(person);
            self.time_ui.selected = None;
            self.time_ui.focus = None;
            self.time_ui.need_load = true;
        }
    }

    /// Übernimmt die Antwort eines Workers der Zeitreise.
    pub(super) fn accept_time(&mut self, outcome: TimeOutcome) {
        let ui = &mut self.time_ui;
        match outcome {
            TimeOutcome::People(revision, people) => {
                if revision == ui.people_revision {
                    ui.people = people;
                    ui.people_fresh = true;
                }
            }
            TimeOutcome::Loaded(loaded) => {
                let loaded = *loaded;
                let pending = ui.need_load;
                if let Some(key) = &loaded.data.key {
                    ui.observe_key(key.clone());
                }
                // Auch eine überholte Antwort ist für ihre Person richtig: merken, wenn sie
                // zum neuesten Datenstand gehört.
                ui.remember(loaded.person, loaded.data.clone());
                if loaded.revision != ui.revision {
                    return;
                }
                ui.error.clear();
                // Diese Antwort ersetzt die gezeigte Person. Älter als der inzwischen bekannte
                // Stand: zeigen, aber gleich neu lesen.
                ui.need_load =
                    pending || loaded.data.key.is_some() && !ui.current(loaded.data.key.as_ref());
                ui.show(loaded.person, loaded.data);
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
            TimeOutcome::Decided(revisions) => match revisions {
                Some(revisions) => ui.observe(&revisions),
                // Ohne Revision: nichts gemerkt, die Person neu lesen.
                None => ui.need_load = true,
            },
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
                "Wähle links eine Sammlung mit analysierten Gesichtern, um Personen über die Zeit zu sehen.",
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

    /// Name der gewählten Person aus der Auswahlliste.
    fn time_person_name(&self) -> Option<String> {
        let person = self.time_ui.person?;
        self.time_ui
            .choices(&self.face_page.people)
            .iter()
            .find(|p| p.id == person)
            .map(|p| p.name.clone())
    }

    /// Platzhalter, solange nichts berechnet ist: ohne Person der Weg zur Auswahl, sonst der
    /// laufende Ladeauftrag der gewählten Person.
    fn time_waiting(&self, ui: &mut egui::Ui) {
        ui.add_space(24.0);
        if self.time_ui.person.is_none() {
            ui.label("Wähle oben eine Person. Ihre Gesichter werden erst dann gelesen.");
            return;
        }
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(match self.time_person_name() {
                Some(name) => format!("Gesichter von {name} werden gelesen …"),
                None => "Zeitreise wird gelesen …".to_owned(),
            });
        });
    }

    fn decide(&mut self, decision: Decision) {
        let Some(person) = self.time_ui.loaded_person else {
            return;
        };
        self.launch_write("Zeitreise speichern", JobKind::Mutation, move |c, _| {
            match decision {
                Decision::Pin(key, face) => timeline::pin(&c, person, &key, Some(face))?,
                Decision::Unpin(key) => timeline::pin(&c, person, &key, None)?,
                Decision::Hide(face, hidden) => timeline::set_hidden(&c, person, face, hidden)?,
                Decision::BirthMonth(month) => timeline::set_birth_month(&c, person, month)?,
            }
            Ok(Outcome::Time(TimeOutcome::Decided(
                crate::core::revision::read(&c.db)?,
            )))
        });
    }

    fn time_header(&mut self, ui: &mut egui::Ui) {
        let items: Vec<Choice<i64>> =
            select::person_choices(self.time_ui.choices(&self.face_page.people), None);
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
                    "Für das Alter braucht die Person ein Geburtsjahr. Du trägst es in der Übersicht ein.",
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
            // Wer hier eine Person wählt, wählt sie für alle Personenansichten.
            if let Some(person) = person {
                self.relation_from = Some(person);
            }
        }
        if load {
            self.time_ui.need_load = true;
        }
        if compute {
            self.time_ui.need_compute = true;
        }
        let working = self.tasks.iter().any(|t| t.kind == JobKind::Timeline);
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
        let response = response.on_hover_text(format!("{info}\n{SINGLE_NAMING_HINT}"));
        if single_naming_menu(&response) {
            self.open_naming(face.id);
        }
        response
    }

    fn strip_tab(&mut self, ui: &mut egui::Ui) {
        let Some(computed) = self.time_ui.computed.clone() else {
            self.time_waiting(ui);
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
            let response = response.on_hover_text(format!(
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
                // Spalte füllt den freien Platz (neben der großen Vorschau, falls offen);
                // die Kacheln werden zeilenweise selbst umgebrochen, weil verschachtelte
                // Spalten in `horizontal_wrapped` nicht umbrechen.
                let width = if focus.is_some() {
                    (ui.available_width() - 400.0).max(260.0)
                } else {
                    ui.available_width()
                };
                ui.set_width(width);
                let columns = tile_columns(width, ui.spacing().item_spacing.x);
                for row in section.members.chunks(columns) {
                    ui.horizontal_top(|ui| {
                    for member in row {
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
                }
            });
            if let Some(face_id) = focus
                && let Some(item) = computed.items.iter().find(|i| i.face.id == face_id)
            {
                let (root, exiftool) = (self.settings.collection.clone(), self.settings.exiftool.clone());
                ui.vertical(|ui| {
                    let response = self
                        .thumbnails
                        .image(
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
                        )
                        .on_hover_text(SINGLE_NAMING_HINT);
                    if single_naming_menu(&response) {
                        self.open_naming(face_id);
                    }
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
        if self.tasks.iter().any(|t| t.kind == JobKind::TimelineExport) {
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
            .time_person_name()
            .unwrap_or_else(|| "Person".to_owned());
        let stem = format!("Zeitreise-{}", timeline_export::safe_stem(&name));
        let (root, exiftool) = (
            PathBuf::from(&self.settings.collection),
            self.settings.exiftool.clone(),
        );
        let (fps, aligned) = (self.time_ui.lapse_fps, self.time_ui.lapse_aligned);
        self.launch(
            "Zeitreise exportieren",
            false,
            JobKind::TimelineExport,
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
            self.time_waiting(ui);
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
        let busy = self.tasks.iter().any(|t| t.kind == JobKind::TimelineExport);
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
            self.time_waiting(ui);
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

    #[test]
    fn tile_columns_follow_the_available_width() {
        assert_eq!(tile_columns(50.0, 8.0), 1);
        assert_eq!(tile_columns(TILE * 3.0 + 16.0, 8.0), 3);
        assert_eq!(tile_columns(TILE * 3.0 + 15.0, 8.0), 2);
        assert_eq!(tile_columns(1200.0, 8.0), 10);
    }

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
        // Ohne gewählte Person steht die Personenliste bereit, Gesichter werden nicht gelesen.
        assert_eq!(app.time_ui.person, None);
        assert_eq!(app.time_ui.people.len(), 1);
        assert!(app.time_ui.people_fresh);
        assert!(app.time_ui.raw.is_none() && app.time_ui.computed.is_none());
        // Die Liste liegt für den nächsten Start im lokalen Cache.
        let cached = timeline::cached_people(&app.cache_dir, &app.settings.collection);
        assert_eq!(cached.as_deref(), Some(app.time_ui.people.as_slice()));
        app.show_time_for(1);
        settle(&mut app, &ctx);
        assert_eq!(app.time_ui.loaded_person, Some(1));
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

    /// Zwei Personen mit je einem Gesicht in einer temporären Sammlung.
    fn two_people() -> anyhow::Result<tempfile::TempDir> {
        let root = tempfile::tempdir()?;
        let c = Collection::open(root.path(), true)?;
        for (id, name) in [(1, "Anna"), (2, "Ben")] {
            c.db.execute("INSERT INTO persons(id) VALUES(?)", [id])?;
            c.db.execute(
                "INSERT INTO person_attrs VALUES(?,?,?)",
                rusqlite::params![id, "name", name],
            )?;
        }
        for (id, person) in [(1, 1), (2, 2)] {
            let rel = format!("2020/{id}.jpg");
            c.db.execute(
                "INSERT INTO photos(id,status) VALUES(?,?)",
                rusqlite::params![id, "present"],
            )?;
            c.db.execute("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status,meta_state) VALUES(?,?,?,?,?,?,?,1,0,?,?,?)", rusqlite::params![id, rel, rel, "2020", format!("{id}.jpg"), id.to_string(), "jpg", id, "present", "done"])?;
            c.db.execute("INSERT INTO meta(file_id,dt_original,width,height,orientation) VALUES(?,?,800,600,1)", rusqlite::params![id, "2020:06:01 12:00:00"])?;
            c.db.execute("INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,person_id,assignment) VALUES(?,?,?,?,?,0.3,0.2,0.7,0.8,?,0.95,0.9,?,?)", rusqlite::params![id, id, id, rel, "d", "[[300.0,200.0],[400.0,200.0],[350.0,260.0],[310.0,320.0],[390.0,320.0]]", person, "user"])?;
        }
        Ok(root)
    }

    #[test]
    fn switching_back_uses_the_remembered_person_until_the_data_changes() -> anyhow::Result<()> {
        let root = two_people()?;
        let ctx = egui::Context::default();
        let creation = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = PhotoApp::with_settings(&creation, Settings::default());
        app.settings.collection = root.path().to_string_lossy().into_owned();
        app.show_time_for(1);
        settle(&mut app, &ctx);
        app.show_time_for(2);
        settle(&mut app, &ctx);
        assert_eq!(app.time_ui.loaded_person, Some(2));
        // Zurück zu Anna: aus dem Speicher, nur die Berechnung läuft auf einem Worker.
        app.show_time_for(1);
        app.sync_time();
        assert_eq!(
            app.time_ui.loaded_person,
            Some(1),
            "sofort aus dem Speicher"
        );
        let labels: Vec<&str> = app
            .tasks
            .iter()
            .filter(|t| t.kind == JobKind::Timeline)
            .map(|t| t.task.label.as_str())
            .collect();
        assert_eq!(labels, ["Zeitreise berechnen"], "keine Datenbankabfrage");
        settle(&mut app, &ctx);
        assert_eq!(app.time_ui.computed.as_ref().map(|c| c.total), Some(1));
        // Geänderte Metadaten (Bereich `files`) lassen Gemerktes stehen.
        let changed = |sql: &str| -> anyhow::Result<crate::core::revision::Revisions> {
            let c = Collection::open(root.path(), true)?;
            c.db.execute_batch(sql)?;
            Ok(crate::core::revision::read(&c.db)?.expect("Revision"))
        };
        app.time_ui.observe(&changed(
            "UPDATE meta SET dt_original='2021:06:01 12:00:00' WHERE file_id=2",
        )?);
        assert!(app.time_ui.cached(2).is_some());
        assert!(!app.time_ui.need_load);
        // Eine geänderte Zuordnung verwirft alles Gemerkte; Anna wird neu gelesen und bleibt
        // bis dahin sichtbar.
        app.time_ui
            .observe(&changed("UPDATE faces SET assignment='auto' WHERE id=2")?);
        assert!(app.time_ui.cached(2).is_none());
        app.sync_time();
        assert_eq!(app.time_ui.loaded_person, Some(1));
        let labels: Vec<&str> = app
            .tasks
            .iter()
            .filter(|t| t.kind == JobKind::Timeline)
            .map(|t| t.task.label.as_str())
            .collect();
        assert_eq!(labels, ["Zeitreise lesen"]);
        settle(&mut app, &ctx);
        assert_eq!(app.time_ui.loaded_person, Some(1));
        assert!(!app.time_ui.need_load, "kein zweites Lesen");
        assert!(app.time_ui.cached(1).is_some());
        Ok(())
    }

    #[test]
    fn a_new_choice_supersedes_the_running_load_and_old_answers_are_not_shown() -> anyhow::Result<()>
    {
        let root = two_people()?;
        let ctx = egui::Context::default();
        let creation = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = PhotoApp::with_settings(&creation, Settings::default());
        app.settings.collection = root.path().to_string_lossy().into_owned();
        app.show_time_for(1);
        app.sync_time();
        let first = app.time_ui.revision;
        // Wechsel während des Ladens: sofort ein neuer Auftrag, der alte wird abgebrochen.
        app.show_time_for(2);
        app.sync_time();
        assert!(app.time_ui.revision > first);
        let loads = app
            .tasks
            .iter()
            .filter(|t| t.task.label == "Zeitreise lesen")
            .count();
        assert!(loads >= 1);
        settle(&mut app, &ctx);
        assert_eq!(app.time_ui.loaded_person, Some(2));
        // Eine verspätete Antwort für Anna zum selben Datenstand wird gemerkt, aber nicht
        // gezeigt.
        let now = crate::core::revision::read(&Collection::open(root.path(), false)?.db)?
            .expect("Revision");
        let answer = |revision: u64, revisions: &crate::core::revision::Revisions| Loaded {
            revision,
            person: 1,
            data: PersonData {
                revisions: Some(revisions.clone()),
                ..Default::default()
            }
            .into(),
            computed: timeline::compute(&[], &Params::default()),
        };
        app.accept_time(TimeOutcome::Loaded(Box::new(answer(first, &now))));
        assert_eq!(app.time_ui.loaded_person, Some(2));
        assert!(app.time_ui.cached(1).is_some());
        // Ergebnisse von vor einer Änderung kommen nicht in den Speicher.
        let newer = crate::core::revision::Revisions {
            people: now.people + 1,
            ..now.clone()
        };
        app.time_ui.observe(&newer);
        assert!(app.time_ui.cached(1).is_none());
        app.accept_time(TimeOutcome::Loaded(Box::new(answer(0, &now))));
        assert!(app.time_ui.cached(1).is_none());
        // Ohne Revision wird nichts gemerkt.
        app.accept_time(TimeOutcome::Loaded(Box::new(Loaded {
            revision: 0,
            person: 1,
            data: PersonData::default().into(),
            computed: timeline::compute(&[], &Params::default()),
        })));
        assert!(app.time_ui.cached(1).is_none());
        Ok(())
    }

    #[test]
    fn timeline_opens_for_the_person_selected_under_personen() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let people = {
            let c = Collection::open(root.path(), true)?;
            // Anna hat die meisten Gesichter und wäre ohne Auswahl die Vorgabe; gewählt ist Ben.
            for (id, name) in [(1, "Anna"), (2, "Ben")] {
                c.db.execute("INSERT INTO persons(id) VALUES(?)", [id])?;
                c.db.execute(
                    "INSERT INTO person_attrs VALUES(?,?,?)",
                    rusqlite::params![id, "name", name],
                )?;
            }
            for (id, person) in [(1, 1), (2, 1), (3, 2)] {
                let rel = format!("2020/{id}.jpg");
                c.db.execute(
                    "INSERT INTO photos(id,status) VALUES(?,?)",
                    rusqlite::params![id, "present"],
                )?;
                c.db.execute("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status,meta_state) VALUES(?,?,?,?,?,?,?,1,0,?,?,?)", rusqlite::params![id, rel, rel, "2020", format!("{id}.jpg"), id.to_string(), "jpg", id, "present", "done"])?;
                c.db.execute("INSERT INTO meta(file_id,dt_original,width,height,orientation) VALUES(?,?,800,600,1)", rusqlite::params![id, "2020:06:01 12:00:00"])?;
                c.db.execute("INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,person_id,assignment) VALUES(?,?,?,?,?,0.3,0.2,0.7,0.8,?,0.95,0.9,?,?)", rusqlite::params![id, id, id, rel, "d", "[[300.0,200.0],[400.0,200.0],[350.0,260.0],[310.0,320.0],[390.0,320.0]]", person, "user"])?;
            }
            faces::people(&c)?
        };
        let ctx = egui::Context::default();
        let creation = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = PhotoApp::with_settings(&creation, Settings::default());
        app.settings.collection = root.path().to_string_lossy().into_owned();
        app.face_page.people = people;
        app.select_person(2);
        app.set_person_mode(PersonMode::Time);
        assert_eq!(
            (app.view, app.person_mode),
            (View::People, PersonMode::Time)
        );
        settle(&mut app, &ctx);
        assert_eq!(app.time_ui.person, Some(2));
        assert_eq!(app.time_ui.loaded_person, Some(2));
        let computed = app.time_ui.computed.clone().expect("computed");
        assert_eq!(computed.total, 1, "nur Bens Gesicht");
        assert_eq!(app.selected_person(), Some(2), "die Auswahl bleibt");
        // Ohne gewählte Person bleibt die Zeitreise bei ihrer eigenen Wahl.
        app.relation_from = None;
        app.set_person_mode(PersonMode::Time);
        settle(&mut app, &ctx);
        assert_eq!(app.time_ui.person, Some(2));
        Ok(())
    }
}
