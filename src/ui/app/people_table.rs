//! Personenseite „Personen & Beziehungen“ (ADR 0063): sortierbare Tabelle links,
//! Eigenschaften der gewählten Person rechts. Je Spalte genau ein Scrollbereich;
//! die Tabelle zeichnet nur sichtbare Zeilen. Gelesen wird auf dem Worker
//! (`faces::page`, Revisionsprüfung in `poll`); hier wird nur gezeichnet.
use super::relationship_view::{square_uv, textured_circle};
use super::*;
use crate::core::{
    duplicates::{self, Candidate},
    gaps::{self, Answer, Suggestion, SuggestionKind},
    people_table::{self as table, Column, PersonRow, SortKey},
    person_insights::{self, Insights},
    relations::{Gender, Role},
};
use crate::ui::select::{Choice, FilterSelect, person_choices};
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};

const ROW_HEIGHT: f32 = 40.0;
const HEADER_HEIGHT: f32 = 36.0;
const PORTRAIT_COLUMN: f32 = 48.0;
const PORTRAIT_SIZE: f32 = 30.0;

/// Datenstand, von dem die Rückfragen „dieselbe Person“ abhängen: Sammlung, Personen,
/// Beziehungen, bestätigte Gesichter, Referenzen und Modell. Ein Wechsel der Ansicht allein
/// liest nicht neu.
type DuplicatesKey = (String, usize, usize, usize, usize, usize, String);

/// Was die rechte Spalte zeigt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum Panel {
    /// Eigenschaften der ausgewählten Person.
    #[default]
    Details,
    /// Formular „Person hinzufügen“.
    NewPerson,
    /// Alle offenen Rückfragen.
    Questions,
    /// Zentraler Personenbestand: Abgleich, Rückfragen zu Widersprüchen, fehlende Personen.
    Central,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Fields {
    pub(super) name: String,
    pub(super) gender: String,
    pub(super) birth: String,
    pub(super) note: String,
}

impl Fields {
    fn of(person: &faces::Person) -> Self {
        Self {
            name: person.name.clone(),
            gender: person.gender.clone(),
            birth: person.birth.clone(),
            note: person.note.clone(),
        }
    }
}

/// Zustand der Personenseite. Auswahl ist `PhotoApp::relation_from`, damit
/// Stammbaum und Graph dieselbe Person verwenden.
pub(super) struct PeopleUi {
    pub(super) query: String,
    pub(super) sort: Vec<SortKey>,
    pub(super) only_hints: bool,
    pub(super) panel: Panel,
    pub(super) view: Vec<usize>,
    view_key: Option<(u64, usize, Vec<SortKey>, String, bool)>,
    /// (Person, gespeicherter Stand, Entwurf): ungespeicherte Eingaben überstehen Neuladen.
    draft: Option<(i64, Fields, Fields)>,
    merge_for: Option<i64>,
    merge_target: Option<i64>,
    merge_confirm: bool,
    scroll_to: Option<i64>,
    /// Zusatzangaben der gewählten Person (Worker-Ergebnis) und der Stand, für den sie
    /// angefragt wurden: (Person, `face_page_serial`).
    pub(super) insights: Option<Insights>,
    insights_key: Option<(i64, u64)>,
    insights_revision: u64,
    /// Rückfragen „Möglicherweise dieselbe Person“ (Worker-Ergebnis) und der Datenstand, für
    /// den sie gelesen wurden.
    pub(super) duplicates: Vec<Candidate>,
    duplicates_key: Option<DuplicatesKey>,
    duplicates_revision: u64,
    /// In dieser Sitzung als verschieden beantwortet; bleibt ausgeblendet, bis neu gelesen ist.
    declined_now: BTreeSet<(i64, i64)>,
    /// Die Bestätigung des Zusammenführens kam aus einer Rückfrage: einmal ins Bild scrollen.
    merge_reveal: bool,
    /// Nur Sichtprüfung: Suchtext, mit dem das Beziehungs-Auswahlfeld offen bleibt.
    pub(super) capture_popup: Option<String>,
    #[cfg(test)]
    pub(super) bounds: Option<Bounds>,
}

/// Messwerte der letzten Darstellung für Layout-Tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(super) struct Bounds {
    pub(super) table: Rect,
    pub(super) panel: Rect,
    pub(super) panel_content_width: f32,
    pub(super) columns: usize,
}

impl Default for PeopleUi {
    fn default() -> Self {
        Self {
            query: String::new(),
            sort: vec![SortKey {
                column: Column::Name,
                descending: false,
            }],
            only_hints: false,
            panel: Panel::Details,
            view: Vec::new(),
            view_key: None,
            draft: None,
            merge_for: None,
            merge_target: None,
            merge_confirm: false,
            scroll_to: None,
            insights: None,
            insights_key: None,
            insights_revision: 0,
            duplicates: Vec::new(),
            duplicates_key: None,
            duplicates_revision: 0,
            declined_now: BTreeSet::new(),
            merge_reveal: false,
            capture_popup: None,
            #[cfg(test)]
            bounds: None,
        }
    }
}

impl PeopleUi {
    /// Tabelle beim nächsten Zeichnen so scrollen, dass diese Person sichtbar ist.
    pub(super) fn scroll_to_person(&mut self, id: i64) {
        self.scroll_to = Some(id);
    }
    /// Beim Wechsel der Sammlung enden Suche, Entwürfe und begonnenes Zusammenführen; die
    /// Sortierung bleibt.
    pub(super) fn reset_for_collection(&mut self) {
        *self = Self {
            sort: std::mem::take(&mut self.sort),
            capture_popup: self.capture_popup.take(),
            // Die Revision läuft weiter, damit kein Ergebnis der alten Sammlung passt.
            insights_revision: self.insights_revision + 1,
            duplicates_revision: self.duplicates_revision + 1,
            ..Self::default()
        };
    }
    /// Übernimmt die Rückfragen „dieselbe Person“ der zuletzt angefragten Revision.
    pub(super) fn accept_duplicates(&mut self, revision: u64, duplicates: Vec<Candidate>) {
        // `declined_now` bleibt: ein Lesen, das vor dem Speichern begann, kennt die Antwort
        // noch nicht.
        if revision == self.duplicates_revision {
            self.duplicates = duplicates;
        }
    }
    /// Übernimmt Zusatzangaben nur, wenn sie zur zuletzt angefragten Revision gehören.
    pub(super) fn accept_insights(&mut self, revision: u64, insights: Insights) {
        if revision == self.insights_revision {
            self.insights = Some(insights);
        }
    }
    /// Nur für Sichtprüfungen: Sortierspalte aus der Umgebung (`name`, `photos`, …).
    pub(super) fn sort_from_env(&mut self) {
        let Ok(text) = std::env::var("FOTO_IMPORTER_PEOPLE_SORT") else {
            return;
        };
        let (name, descending) = match text.strip_suffix(":desc") {
            Some(name) => (name, true),
            None => (text.strip_suffix(":asc").unwrap_or(&text), false),
        };
        let column = match name {
            "name" => Column::Name,
            "photos" => Column::Photos,
            "faces" => Column::Faces,
            "birth" => Column::Birth,
            "gender" => Column::Gender,
            "relations" => Column::Relations,
            "family" => Column::Family,
            "first" => Column::FirstPhoto,
            "last" => Column::LastPhoto,
            "hints" => Column::Hints,
            _ => return,
        };
        self.sort = vec![SortKey { column, descending }];
    }
}

fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(theme::MUTED)
}

fn gender_button_label(gender: Gender) -> &'static str {
    match gender {
        Gender::Female => "weiblich",
        Gender::Male => "männlich",
        Gender::Unknown => "unbekannt",
    }
}

/// Auswahl des Geschlechts als drei große Schaltflächen (feste kurze Liste, daher
/// ohne Aufklappfeld). Gibt `true` zurück, wenn der Wert geändert wurde.
pub(super) fn gender_buttons(ui: &mut egui::Ui, value: &mut String) -> bool {
    let current = Gender::parse(value);
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (option, text) in [
            (Gender::Unknown, ""),
            (Gender::Female, "weiblich"),
            (Gender::Male, "männlich"),
        ] {
            let selected = current == option;
            if ui
                .add(
                    egui::Button::new(gender_button_label(option))
                        .selected(selected)
                        .min_size(Vec2::new(0.0, theme::CONTROL_HEIGHT)),
                )
                .clicked()
                && !selected
            {
                *value = text.to_string();
                changed = true;
            }
        }
    });
    changed
}

/// Sortierkennzeichen im Spaltenkopf: ▲ aufsteigend, ▼ absteigend, bei mehreren
/// Kriterien mit der Rangnummer.
fn sort_mark(keys: &[SortKey], column: Column) -> String {
    let Some(position) = keys.iter().position(|k| k.column == column) else {
        return String::new();
    };
    let arrow = if keys[position].descending { "v" } else { "^" };
    if keys.len() > 1 {
        format!("{arrow}{}", position + 1)
    } else {
        arrow.to_string()
    }
}

/// Ort und Breite jeder sichtbaren Spalte ab `left`; der Name füllt den Rest.
fn column_layout(left: f32, width: f32, columns: &[Column]) -> Vec<(Column, f32, f32)> {
    let fixed: f32 = columns
        .iter()
        .filter(|c| **c != Column::Name)
        .map(|c| c.width())
        .sum();
    let name = (width - PORTRAIT_COLUMN - fixed).max(Column::Name.width());
    let mut x = left + PORTRAIT_COLUMN;
    columns
        .iter()
        .map(|&column| {
            let w = if column == Column::Name {
                name
            } else {
                column.width()
            };
            let entry = (column, x, w);
            x += w;
            entry
        })
        .collect()
}

fn numeric(column: Column) -> bool {
    matches!(
        column,
        Column::Photos | Column::Faces | Column::Relations | Column::Hints
    )
}

fn cell_text(row: &PersonRow, column: Column) -> String {
    let dash = || "–".to_string();
    match column {
        Column::Name => row.name.clone(),
        Column::Photos => row.photos.to_string(),
        Column::Faces => row.faces.to_string(),
        Column::Birth => match (row.birth, row.age) {
            (Some(year), Some(age)) => format!("{year} · {age}"),
            (Some(year), None) => year.to_string(),
            _ => dash(),
        },
        Column::Gender => match row.gender_label() {
            "" => dash(),
            label => label.to_string(),
        },
        Column::Relations => row.relations.to_string(),
        Column::Family => {
            if row.family.is_empty() {
                dash()
            } else {
                row.family.clone()
            }
        }
        Column::FirstPhoto => row.first_photo.as_deref().map_or_else(dash, display_date),
        Column::LastPhoto => row.last_photo.as_deref().map_or_else(dash, display_date),
        Column::Hints => {
            if row.hints == 0 {
                dash()
            } else {
                format!("! {}", row.hints)
            }
        }
    }
}

/// Frage und Text der „Nein“-Antwort je Vorschlagstyp. Neue Typen aus `core::gaps`
/// erhalten ohne eigenen Zweig eine allgemeine Darstellung.
#[allow(unreachable_patterns)]
fn question_text(
    s: &Suggestion,
    family: &crate::core::kinship::Family,
    names: &dyn Fn(&[i64]) -> String,
    name: &dyn Fn(i64) -> String,
) -> (String, &'static str) {
    match s.kind {
        SuggestionKind::SiblingParents => {
            let target = s.targets[0];
            let has_parent = !family.parents_of(target).is_empty();
            (
                format!(
                    "Sind {} auch Eltern von {}? ({} und {} gelten als Geschwister, erfasst sind aber nicht dieselben Eltern.)",
                    names(&s.candidates),
                    name(target),
                    names(&s.via),
                    name(target)
                ),
                if has_parent {
                    "Nein – Halbgeschwister"
                } else {
                    "Nein"
                },
            )
        }
        SuggestionKind::PartnerParent => (
            format!(
                "Ist {} (Partner von {}) auch Elternteil von {}?",
                names(&s.candidates),
                names(&s.via),
                names(&s.targets)
            ),
            "Nein – Stiefbeziehung",
        ),
        SuggestionKind::FriendOfPartner => (
            format!(
                "{} ist mit {} befreundet. Ist {} (Partner von {}) auch mit {} befreundet?",
                names(&s.targets),
                names(&s.via),
                names(&s.candidates),
                names(&s.via),
                names(&s.targets)
            ),
            "Nein",
        ),
        #[allow(unreachable_patterns)]
        _ => (
            format!(
                "Passt eine Beziehung zwischen {} und {} (über {})?",
                names(&s.candidates),
                names(&s.targets),
                names(&s.via)
            ),
            "Nein",
        ),
    }
}

impl PhotoApp {
    /// Zeilenreihenfolge neu berechnen, wenn sich Daten, Sortierung oder Suche ändern.
    fn ensure_people_view(&mut self) {
        let key = (
            self.face_page_serial,
            self.face_page.rows.len(),
            self.people_ui.sort.clone(),
            self.people_ui.query.clone(),
            self.people_ui.only_hints,
        );
        if self.people_ui.view_key.as_ref() != Some(&key) {
            self.people_ui.view = table::view(
                &self.face_page.rows,
                &key.2,
                &key.3,
                self.people_ui.only_hints,
            );
            self.people_ui.view_key = Some(key);
        }
    }

    pub(super) fn select_person(&mut self, id: i64) {
        self.relation_from = Some(id);
        self.people_ui.panel = Panel::Details;
    }

    /// Reiter „Personen & Beziehungen“: Werkzeugleiste, Tabelle, Detailspalte.
    pub(super) fn persons_page(&mut self, ui: &mut egui::Ui) {
        self.ensure_people_view();
        // Eine ungültige Auswahl (z. B. nach dem Zusammenführen) wird verworfen,
        // eine gültige bleibt über jedes Neuladen erhalten.
        if let Some(id) = self.relation_from
            && !self.face_page.people.iter().any(|p| p.id == id)
            && !self.face_page.people.is_empty()
        {
            self.relation_from = None;
        }
        self.people_toolbar(ui);
        ui.add_space(2.0);
        let total = ui.available_width();
        let max = (total * 0.55).clamp(300.0, 560.0);
        let default = (total * 0.42).clamp(300.0, 400.0).min(max);
        #[cfg(test)]
        let mut panel_rect = Rect::NOTHING;
        egui::SidePanel::right("people-detail")
            .resizable(true)
            .default_width(default)
            .width_range(300.0_f32.min(max)..=max)
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 14,
                        right: 4,
                        top: 0,
                        bottom: 0,
                    })
                    .fill(Color32::TRANSPARENT),
            )
            .show_inside(ui, |ui| {
                #[cfg(test)]
                {
                    panel_rect = ui.max_rect();
                }
                self.person_panel(ui);
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                #[cfg(test)]
                let rect = ui.max_rect();
                let columns = self.people_table(ui);
                #[cfg(test)]
                {
                    self.people_ui.bounds = Some(Bounds {
                        table: rect,
                        panel: panel_rect,
                        panel_content_width: self
                            .people_ui
                            .bounds
                            .map_or(0.0, |b| b.panel_content_width),
                        columns,
                    });
                }
                #[cfg(not(test))]
                let _ = columns;
            });
    }

    fn people_toolbar(&mut self, ui: &mut egui::Ui) {
        let total = self.face_page.rows.len();
        let shown = self.people_ui.view.len();
        let questions = self.all_questions();
        ui.horizontal_wrapped(|ui| {
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.people_ui.query)
                    .hint_text("Suchen: Name, #ID, Geburtsjahr …")
                    .desired_width(240.0)
                    .margin(egui::Margin::symmetric(10, 9)),
            );
            if search.changed() {
                self.people_ui.scroll_to = self.relation_from;
            }
            if !self.people_ui.query.is_empty()
                && ui
                    .add(egui::Button::new("✕").min_size(Vec2::splat(theme::CONTROL_HEIGHT)))
                    .on_hover_text("Suche leeren")
                    .clicked()
            {
                self.people_ui.query.clear();
            }
            ui.label(muted(if shown == total {
                format!("{total} Personen")
            } else {
                format!("{shown} von {total} Personen")
            }));
            if ui
                .add(
                    egui::Button::new(format!("Rückfragen · {questions}"))
                        .selected(self.people_ui.panel == Panel::Questions),
                )
                .on_hover_text("Alle offenen Rückfragen rechts anzeigen")
                .clicked()
            {
                self.people_ui.panel = if self.people_ui.panel == Panel::Questions {
                    Panel::Details
                } else {
                    Panel::Questions
                };
            }
            if ui
                .add(
                    egui::Button::new("+ Person hinzufügen")
                        .selected(self.people_ui.panel == Panel::NewPerson),
                )
                .clicked()
            {
                self.people_ui.panel = Panel::NewPerson;
            }
            // Zentraler Bestand: Abgleich, Konflikte und fehlende Personen an einem Ort.
            if self.settings.central_enabled {
                let pending = self.central_ui.report.as_ref().map_or(0, |r| r.pending());
                let label = if pending > 0 {
                    format!("Zentraler Bestand · {pending}")
                } else {
                    "Zentraler Bestand".to_string()
                };
                if ui
                    .add(egui::Button::new(label).selected(self.people_ui.panel == Panel::Central))
                    .on_hover_text(
                        "Personen mit anderen Sammlungen teilen: abgleichen, fehlende zentrale Personen übernehmen",
                    )
                    .clicked()
                {
                    self.people_ui.panel = if self.people_ui.panel == Panel::Central {
                        Panel::Details
                    } else {
                        Panel::Central
                    };
                }
            }
        });
    }

    /// Spaltenköpfe und virtualisierte Zeilen. Gibt die Zahl der sichtbaren Spalten zurück.
    fn people_table(&mut self, ui: &mut egui::Ui) -> usize {
        let width = ui.available_width();
        if self.face_page.rows.is_empty() {
            if self.tasks.iter().any(|task| task.kind == "faces") {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Personen werden gelesen …");
                });
            } else {
                ui.add_space(24.0);
                ui.label(RichText::new("Noch keine Personen.").strong());
                ui.label(muted(
                    "Benenne Gesichter unter „Gesichter“ oder lege rechts eine Person ohne Foto an.",
                ));
            }
            return 0;
        }
        let scroll = egui::style::ScrollStyle::solid();
        let bar = scroll.allocated_width();
        ui.spacing_mut().scroll = scroll;
        ui.spacing_mut().item_spacing.y = 0.0;
        let columns = table::visible_columns(width - PORTRAIT_COLUMN - bar);
        let layout = column_layout(ui.cursor().left(), width - bar, &columns);
        self.table_header(ui, width - bar, &layout);

        if self.people_ui.view.is_empty() {
            ui.add_space(16.0);
            ui.label(RichText::new("Keine Person passt zu diesem Filter.").strong());
            if ui.button("Filter zurücksetzen").clicked() {
                self.people_ui.query.clear();
                self.people_ui.only_hints = false;
            }
            return columns.len();
        }

        let selected = self.relation_from;
        let count = self.people_ui.view.len();
        let mut clicked = None;
        // Pfeiltasten wählen die Nachbarzeile, solange kein Eingabefeld Tasten braucht.
        let step = if ui.ctx().wants_keyboard_input() {
            0
        } else {
            ui.input(|i| {
                i.key_pressed(egui::Key::ArrowDown) as i32
                    - i.key_pressed(egui::Key::ArrowUp) as i32
            })
        };
        if step != 0 {
            let position = selected
                .and_then(|id| {
                    self.people_ui
                        .view
                        .iter()
                        .position(|&r| self.face_page.rows[r].id == id)
                })
                .map_or(0, |p| (p as i32 + step).clamp(0, count as i32 - 1) as usize);
            clicked = Some(self.face_page.rows[self.people_ui.view[position]].id);
            self.people_ui.scroll_to = clicked;
        }
        let mut area = egui::ScrollArea::vertical()
            .id_salt("people-table")
            .auto_shrink([false, false]);
        if let Some(target) = self.people_ui.scroll_to.take()
            && let Some(position) = self
                .people_ui
                .view
                .iter()
                .position(|&r| self.face_page.rows[r].id == target)
        {
            let offset = (position as f32 * ROW_HEIGHT - ui.available_height() / 2.0
                + ROW_HEIGHT / 2.0)
                .max(0.0);
            area = area.vertical_scroll_offset(offset);
        }
        let root = self.settings.collection.clone();
        let exiftool = self.settings.exiftool.clone();
        let rows = &self.face_page.rows;
        let portraits = &self.face_page.portraits;
        let view = &self.people_ui.view;
        let thumbnails = &mut self.thumbnails;
        area.show_rows(ui, ROW_HEIGHT, count, |ui, range| {
            for position in range {
                let Some(row) = view.get(position).and_then(|&r| rows.get(r)) else {
                    continue;
                };
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), ROW_HEIGHT),
                    Sense::click(),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        true,
                        selected == Some(row.id),
                        format!("{}, #{}, {} Fotos", row.name, row.id, row.photos),
                    )
                });
                if response.clicked() {
                    clicked = Some(row.id);
                }
                if ui.is_rect_visible(rect) {
                    paint_row(
                        ui,
                        rect,
                        &response,
                        selected == Some(row.id),
                        row,
                        &layout,
                        portraits.get(&row.id),
                        thumbnails,
                        &root,
                        &exiftool,
                    );
                }
            }
        });
        if let Some(id) = clicked {
            self.select_person(id);
        }
        columns.len()
    }

    fn table_header(&mut self, ui: &mut egui::Ui, width: f32, layout: &[(Column, f32, f32)]) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, HEADER_HEIGHT), Sense::hover());
        let additive = ui.input(|i| i.modifiers.shift);
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius {
                nw: 8,
                ne: 8,
                sw: 0,
                se: 0,
            },
            ui.visuals().faint_bg_color,
        );
        let mut toggled = None;
        for &(column, x, w) in layout {
            let cell = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, HEADER_HEIGHT));
            let response = ui.interact(
                cell,
                ui.id().with(("people-header", column)),
                Sense::click(),
            );
            let mark = sort_mark(&self.people_ui.sort, column);
            let title = column.title();
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    format!("Sortieren nach {title}"),
                )
            });
            if response.hovered() {
                ui.painter()
                    .rect_filled(cell, 0, ui.visuals().widgets.hovered.weak_bg_fill);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let colour = if mark.is_empty() {
                ui.visuals().text_color()
            } else {
                theme::ACCENT
            };
            let text = if mark.len() > 1 {
                format!("{title} {}", &mark[1..])
            } else {
                title.to_string()
            };
            let painter = ui
                .painter()
                .with_clip_rect(cell.shrink2(Vec2::new(4.0, 0.0)));
            let galley = painter.layout_no_wrap(text, FontId::proportional(13.5), colour);
            let text_width = galley.size().x;
            let reserve = if mark.is_empty() { 0.0 } else { 14.0 };
            let left = if numeric(column) {
                cell.right() - 12.0 - reserve - text_width
            } else {
                cell.left() + 8.0
            };
            painter.galley(
                Pos2::new(left, cell.center().y - galley.size().y / 2.0),
                galley,
                colour,
            );
            if !mark.is_empty() {
                // Dreieck statt Zeichen: vermeidet Schriftlücken. ▲ aufsteigend, ▼ absteigend.
                let down = mark.starts_with('v');
                let x = left + text_width + 9.0;
                let y = cell.center().y;
                let points = if down {
                    vec![
                        Pos2::new(x - 4.0, y - 2.5),
                        Pos2::new(x + 4.0, y - 2.5),
                        Pos2::new(x, y + 3.5),
                    ]
                } else {
                    vec![
                        Pos2::new(x - 4.0, y + 2.5),
                        Pos2::new(x + 4.0, y + 2.5),
                        Pos2::new(x, y - 3.5),
                    ]
                };
                painter.add(egui::Shape::convex_polygon(points, colour, Stroke::NONE));
            }
            if response
                .on_hover_text(format!(
                    "{}\nKlick: sortieren · Umschalt+Klick: zusätzliches Kriterium",
                    column.tooltip()
                ))
                .clicked()
            {
                toggled = Some(column);
            }
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
        if let Some(column) = toggled {
            table::toggle_sort(&mut self.people_ui.sort, column, additive);
            self.people_ui.scroll_to = self.relation_from;
        }
    }

    // ------------------------------------------------------------ Detailspalte

    fn person_panel(&mut self, ui: &mut egui::Ui) {
        let mut area = egui::ScrollArea::vertical()
            .id_salt("people-detail-scroll")
            .auto_shrink([false, false]);
        if self.people_ui.capture_popup.is_some() {
            // Nur Sichtprüfung: Beziehungsformular ins Bild scrollen.
            area = area.vertical_scroll_offset(420.0);
        }
        #[cfg_attr(not(test), allow(unused_variables))]
        let output = area.show(ui, |ui| {
            ui.set_width(ui.available_width());
            match self.people_ui.panel {
                Panel::Questions => self.panel_questions(ui),
                Panel::Central => self.panel_central(ui),
                Panel::NewPerson => self.panel_new_person(ui),
                Panel::Details => self.panel_details(ui),
            }
        });
        #[cfg(test)]
        {
            let mut bounds = self.people_ui.bounds.unwrap_or(Bounds {
                table: Rect::NOTHING,
                panel: Rect::NOTHING,
                panel_content_width: 0.0,
                columns: 0,
            });
            bounds.panel_content_width = output.content_size.x;
            self.people_ui.bounds = Some(bounds);
        }
    }

    fn panel_questions(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Rückfragen").size(20.0).strong());
        if self.all_questions() == 0 {
            ui.label(muted(
                "Keine offenen Rückfragen. Die App schlägt fehlende Eltern und doppelte Personen nur vor und ändert nichts ohne deine Antwort.",
            ));
        }
        if self.open_questions() > 0 {
            ui.checkbox(
                &mut self.people_ui.only_hints,
                "Tabelle auf Personen mit Rückfragen beschränken",
            );
        }
        self.duplicates_list(ui, None);
        if !self.open_duplicates(None).is_empty() && self.open_questions() > 0 {
            ui.add_space(8.0);
            ui.label(RichText::new("Stammbaum").strong());
        }
        self.questions_list(ui, None);
    }

    fn panel_central(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Zentraler Personenbestand")
                .size(20.0)
                .strong(),
        );
        ui.label(muted(
            "Personen, die mehrere Sammlungen teilen. Der Abgleich legt hier vorhandene Personen zentral an und meldet, welche zentralen Personen in dieser Sammlung fehlen. Übernommen wird erst auf deinen Klick.",
        ));
        if !self.settings.central_enabled {
            ui.label("Der zentrale Bestand ist in den Einstellungen ausgeschaltet.");
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("central-panel-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| self.central_panel(ui));
    }

    fn panel_new_person(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Person hinzufügen").size(20.0).strong());
        ui.label(muted(
            "Für Personen ohne Foto, z. B. verstorbene Großeltern. Beziehungen trägst du danach in den Eigenschaften der Person ein.",
        ));
        self.add_person_form(ui);
    }

    fn panel_details(&mut self, ui: &mut egui::Ui) {
        let Some(person) = self
            .relation_from
            .and_then(|id| self.face_page.people.iter().find(|p| p.id == id))
            .cloned()
        else {
            ui.label(RichText::new("Keine Person ausgewählt").size(20.0).strong());
            ui.label(muted("Klicke links eine Zeile an, um Eigenschaften und Beziehungen zu sehen. Mit den Pfeiltasten wechselst du die Zeile."));
            let questions = self.all_questions();
            if questions > 0 {
                ui.add_space(8.0);
                ui.label(format!("{questions} Rückfragen warten auf deine Antwort."));
                if ui.button("Rückfragen anzeigen").clicked() {
                    self.people_ui.panel = Panel::Questions;
                }
            }
            return;
        };
        // Entwurf der Eigenschaften: bleibt über Neuladen erhalten, solange er geändert ist.
        let saved = Fields::of(&person);
        match &mut self.people_ui.draft {
            Some((id, base, edit)) if *id == person.id => {
                if *edit == *base && *base != saved {
                    *edit = saved.clone();
                }
                *base = saved;
            }
            slot => *slot = Some((person.id, saved.clone(), saved)),
        }
        if self.people_ui.merge_for != Some(person.id) {
            self.people_ui.merge_for = Some(person.id);
            self.people_ui.merge_target = None;
            self.people_ui.merge_confirm = false;
        }
        let row = self
            .face_page
            .rows
            .iter()
            .find(|r| r.id == person.id)
            .cloned();
        self.detail_header(ui, &person, row.as_ref());
        if row.as_ref().is_some_and(|r| r.photos > 0) {
            self.ensure_insights(person.id);
            ui.separator();
            self.detail_companions(ui, &person);
            ui.separator();
            self.detail_years(ui, &person, row.as_ref().and_then(|r| r.birth));
        }
        ui.separator();
        self.detail_properties(ui, &person);
        ui.separator();
        ui.label(RichText::new("Beziehungen").size(17.0).strong());
        self.relation_overview(ui);
        let has_questions = self
            .face_page
            .questions
            .iter()
            .any(|s| table::question_people(s).contains(&person.id))
            || !self.open_duplicates(Some(person.id)).is_empty();
        if has_questions {
            ui.separator();
            ui.label(
                RichText::new("Rückfragen zu dieser Person")
                    .size(17.0)
                    .strong(),
            );
            self.duplicates_list(ui, Some(person.id));
            self.questions_list(ui, Some(person.id));
        }
        ui.separator();
        self.detail_merge(ui, &person);
        ui.add_space(12.0);
    }

    fn detail_header(
        &mut self,
        ui: &mut egui::Ui,
        person: &faces::Person,
        row: Option<&PersonRow>,
    ) {
        let root = self.settings.collection.clone();
        let exiftool = self.settings.exiftool.clone();
        let portrait = self.face_page.portraits.get(&person.id).cloned();
        let mut open_faces = false;
        ui.horizontal(|ui| {
            let size = Vec2::splat(96.0);
            match portrait {
                Some(face) => {
                    let response = self
                        .thumbnails
                        .image(
                            ui,
                            PreviewRequest {
                                root: Some(PathBuf::from(&root)),
                                path: Path::new(&root).join(&face.source),
                                orientation: 1,
                                crop: Some(face.bbox),
                                edge: 256,
                                generation: 0,
                            },
                            &exiftool,
                            size,
                        )
                        .on_hover_text(format!(
                            "Gesicht {}\n{}",
                            face.id,
                            super::faces_view::SINGLE_NAMING_HINT
                        ));
                    if super::faces_view::single_naming_menu(&response) {
                        self.open_naming(face.id);
                    }
                }
                None => {
                    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 12, ui.visuals().faint_bg_color);
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        super::tree_view::initials(&person.name),
                        FontId::proportional(32.0),
                        theme::MUTED,
                    );
                }
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(RichText::new(&person.name).size(21.0).strong());
                let mut facts = vec![format!("#{}", person.id)];
                if let Some(row) = row {
                    if let (Some(year), age) = (row.birth, row.age) {
                        facts.push(match age {
                            Some(age) => format!("* {year} ({age} Jahre)"),
                            None => format!("* {year}"),
                        });
                    }
                    facts.push(format!("{} Fotos · {} Gesichter", row.photos, row.faces));
                    if let (Some(first), Some(last)) = (&row.first_photo, &row.last_photo) {
                        facts.push(if first == last {
                            display_date(first)
                        } else {
                            format!("{} bis {}", display_date(first), display_date(last))
                        });
                    }
                    if !row.family.is_empty() {
                        facts.push(row.family.clone());
                    }
                }
                ui.label(muted(facts.join(" · ")));
                open_faces = ui
                    .small_button("Gesichter prüfen")
                    .on_hover_text(
                        "Bestätigte Gesichter dieser Person und unbestätigte, die sie wahrscheinlich zeigen",
                    )
                    .clicked();
            });
        });
        // Die Person dieser Spalte ist die gewählte; die Gesichterseite öffnet sich für sie.
        if open_faces {
            self.set_person_mode(PersonMode::Faces);
        }
        if self.family_ui.just_added == Some(person.id) {
            ui.label(
                RichText::new("Neu angelegt. Trage unten Beziehungen ein.").color(theme::ACCENT),
            );
        }
        let mut photos = false;
        let mut graph = false;
        let mut tree = false;
        let mut time = false;
        ui.horizontal_wrapped(|ui| {
            photos = ui
                .add_enabled(
                    row.is_some_and(|r| r.photos > 0),
                    egui::Button::new("Fotos zeigen"),
                )
                .on_hover_text("Sammlung nach dieser Person filtern")
                .clicked();
            graph = ui
                .button("Im Graph zeigen")
                .on_hover_text("Gemeinsame Fotos und Beziehungen als Netzwerk")
                .clicked();
            tree = ui
                .button("Im Stammbaum zeigen")
                .on_hover_text("Stammbaum um diese Person")
                .clicked();
            time = ui
                .button("Zeitreise zeigen")
                .on_hover_text("Gesichter dieser Person über die Jahre")
                .clicked();
        });
        if photos {
            self.filters = Search {
                people_all: vec![person.id],
                ..Default::default()
            };
            self.view = View::Collection;
            self.refresh_collection();
        }
        // Die Person dieser Spalte ist die gewählte; die Ansichten öffnen sich für sie.
        if graph {
            self.set_person_mode(PersonMode::Graph);
        }
        if tree {
            self.set_person_mode(PersonMode::Tree);
        }
        if time {
            self.set_person_mode(PersonMode::Time);
        }
    }

    /// Liest die Zusatzangaben der Person auf einem Worker, einmal je Person und Datenstand.
    fn ensure_insights(&mut self, person: i64) {
        let key = (person, self.face_page_serial);
        if self.people_ui.insights_key == Some(key) || self.settings.collection.is_empty() {
            return;
        }
        self.people_ui.insights_key = Some(key);
        self.people_ui.insights_revision += 1;
        let revision = self.people_ui.insights_revision;
        let root = PathBuf::from(&self.settings.collection);
        self.launch(
            "Personendetails lesen",
            false,
            "person-insights",
            move |control| {
                let c = Collection::open(&root, false)?;
                c.cancel_with(&control);
                Ok(Outcome::PersonInsights(
                    revision,
                    person_insights::load(&c, person)?,
                ))
            },
        );
    }

    /// „Häufig zusammen mit“: Personen mit den meisten gemeinsamen Fotos. Der Name wählt die
    /// Person, die Zahl öffnet die Sammlung mit genau diesen Fotos.
    fn detail_companions(&mut self, ui: &mut egui::Ui, person: &faces::Person) {
        ui.label(RichText::new("Häufig zusammen mit").size(17.0).strong());
        let Some(insights) = self
            .people_ui
            .insights
            .as_ref()
            .filter(|i| i.person == person.id)
            .cloned()
        else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(muted("Gemeinsame Fotos werden gezählt …"));
            });
            return;
        };
        if insights.companions.is_empty() {
            ui.label(muted(
                "Auf keinem Foto zusammen mit einer anderen benannten Person.",
            ));
            return;
        }
        let root = self.settings.collection.clone();
        let exiftool = self.settings.exiftool.clone();
        let mut select = None;
        let mut photos = None;
        for companion in &insights.companions {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                paint_portrait(
                    ui,
                    rect.center(),
                    14.0,
                    self.face_page.portraits.get(&companion.person),
                    &companion.name,
                    &mut self.thumbnails,
                    &root,
                    &exiftool,
                );
                if ui
                    .link(&companion.name)
                    .on_hover_text("Diese Person wählen")
                    .clicked()
                {
                    select = Some(companion.person);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = super::plural(companion.photos, "Foto", "Fotos");
                    if ui
                        .button(label)
                        .on_hover_text(format!(
                            "Fotos mit {} und {} in der Sammlung zeigen",
                            person.name, companion.name
                        ))
                        .clicked()
                    {
                        photos = Some(companion.person);
                    }
                });
            });
        }
        if let Some(id) = select {
            self.select_person(id);
            self.people_ui.scroll_to_person(id);
        }
        if let Some(other) = photos {
            self.filters = Search {
                people_all: vec![person.id, other],
                ..Default::default()
            };
            self.view = View::Collection;
            self.refresh_collection();
        }
    }

    /// „Fotos je Jahr“: ein Balken je Jahr vom ersten bis zum letzten Foto, mit Alter bei
    /// bekanntem Geburtsjahr. Ein Klick öffnet die Sammlung mit dieser Person in diesem Jahr.
    fn detail_years(&mut self, ui: &mut egui::Ui, person: &faces::Person, birth: Option<i32>) {
        ui.label(RichText::new("Fotos je Jahr").size(17.0).strong());
        let Some(insights) = self
            .people_ui
            .insights
            .as_ref()
            .filter(|i| i.person == person.id)
        else {
            ui.label(muted("Wird gezählt …"));
            return;
        };
        let bars = person_insights::year_bars(&insights.years, birth);
        let undated = insights.undated;
        if bars.is_empty() {
            ui.label(muted("Keines der Fotos hat ein Aufnahmedatum."));
            return;
        }
        let height = 64.0;
        let width = ui.available_width();
        let (area, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let most = bars.iter().map(|b| b.photos).max().unwrap_or(1).max(1);
        let step = width / bars.len() as f32;
        let gap = if step >= 5.0 { 1.5 } else { 0.0 };
        let mut open = None;
        for (i, bar) in bars.iter().enumerate() {
            let column = Rect::from_min_size(
                Pos2::new(area.left() + i as f32 * step, area.top()),
                Vec2::new(step, height),
            );
            let response = ui.interact(
                column,
                ui.id().with(("person-year", bar.year)),
                if bar.photos > 0 {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            let label = format!(
                "{}: {}",
                bar.year,
                super::plural(bar.photos, "Foto", "Fotos")
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, bar.photos > 0, &label)
            });
            let fraction = bar.photos as f32 / most as f32;
            let top = area.bottom() - (height - 4.0) * fraction;
            let fill = if response.hovered() && bar.photos > 0 {
                theme::ACCENT
            } else {
                theme::ACCENT.gamma_multiply(0.6)
            };
            if bar.photos > 0 {
                ui.painter().rect_filled(
                    Rect::from_min_max(
                        Pos2::new(column.left() + gap / 2.0, top.min(area.bottom() - 2.0)),
                        Pos2::new(column.right() - gap / 2.0, area.bottom()),
                    ),
                    1,
                    fill,
                );
            }
            let age = bar
                .age
                .map(|a| format!(" · etwa {a} Jahre alt"))
                .unwrap_or_default();
            let hint = if bar.photos > 0 {
                format!("{label}{age}\nKlick: diese Fotos in der Sammlung zeigen")
            } else {
                format!("{label}{age}")
            };
            let response = response.on_hover_text(hint);
            if bar.photos > 0 {
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    open = Some(bar.year);
                }
            }
        }
        ui.painter().hline(
            area.x_range(),
            area.bottom(),
            Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
        let edge = |bar: &person_insights::YearBar| match bar.age {
            Some(age) => format!("{} · {age} J.", bar.year),
            None => bar.year.to_string(),
        };
        let (first, last) = (&bars[0], &bars[bars.len() - 1]);
        ui.horizontal(|ui| {
            ui.label(muted(edge(first)));
            if bars.len() > 1 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(muted(edge(last)));
                });
            }
        });
        let peak = bars.iter().max_by_key(|b| (b.photos, -b.year));
        let mut summary = peak
            .map(|b| format!("Meiste Fotos {} ({})", b.year, b.photos))
            .unwrap_or_default();
        if undated > 0 {
            summary.push_str(&format!(" · {undated} ohne Datum"));
        }
        ui.label(muted(summary));
        if let Some(year) = open {
            self.filters = Search {
                people_all: vec![person.id],
                start: format!("{year:04}-01-01"),
                end: format!("{year:04}-12-31"),
                ..Default::default()
            };
            self.view = View::Collection;
            self.refresh_collection();
        }
    }

    fn detail_properties(&mut self, ui: &mut egui::Ui, person: &faces::Person) {
        let busy = self.busy();
        let Some((id, base, edit)) = self.people_ui.draft.as_mut() else {
            return;
        };
        if *id != person.id {
            return;
        }
        ui.label(RichText::new("Eigenschaften").size(17.0).strong());
        let width = ui.available_width();
        ui.label("Name");
        ui.add(
            egui::TextEdit::singleline(&mut edit.name)
                .desired_width(width)
                .margin(egui::Margin::symmetric(10, 9)),
        );
        ui.label("Geschlecht");
        gender_buttons(ui, &mut edit.gender);
        ui.label(muted(
            "Bestimmt nur die Beschriftung von Beziehungen (Mutter, Vater, Elternteil …).",
        ));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label("Geburtsjahr");
                ui.add(
                    egui::TextEdit::singleline(&mut edit.birth)
                        .desired_width(100.0)
                        .margin(egui::Margin::symmetric(10, 9)),
                );
            });
        });
        ui.label("Notiz");
        ui.add(
            egui::TextEdit::multiline(&mut edit.note)
                .desired_width(width)
                .desired_rows(3),
        );
        let birth = edit.birth.trim().to_string();
        let year = if birth.is_empty() {
            Ok(None)
        } else {
            birth.parse::<i32>().map(Some)
        };
        let candidate = faces::NewPerson {
            name: edit.name.clone(),
            gender: String::new(),
            birth_year: year.clone().unwrap_or(None),
            note: edit.note.clone(),
        };
        let problem = if year.is_err() {
            Some("Das Geburtsjahr muss eine Zahl sein".to_string())
        } else {
            faces::validate_new_person(&candidate)
                .err()
                .map(|e| e.to_string())
        };
        if let Some(problem) = &problem {
            ui.colored_label(ui.visuals().warn_fg_color, problem);
        }
        let dirty = *edit != *base;
        let mut save = false;
        let mut reset = false;
        ui.horizontal_wrapped(|ui| {
            save = theme::primary(ui, "Speichern", dirty && problem.is_none() && !busy);
            reset = ui
                .add_enabled(
                    dirty,
                    egui::Button::new("Zurücksetzen")
                        .min_size(Vec2::new(0.0, theme::CONTROL_HEIGHT)),
                )
                .clicked();
            if dirty {
                ui.label(muted("Ungespeicherte Änderungen"));
            }
        });
        if reset {
            *edit = base.clone();
        }
        if save {
            // Nur geänderte Felder schreiben; das Geschlecht bleibt so auch in
            // freier Schreibweise unberührt, solange man es nicht ändert.
            let mut attrs = BTreeMap::new();
            if edit.name != base.name {
                attrs.insert(
                    "name".to_string(),
                    faces::validate_new_person(&candidate).unwrap_or_else(|_| edit.name.clone()),
                );
            }
            if edit.gender != base.gender {
                attrs.insert("gender".to_string(), edit.gender.clone());
            }
            if birth != base.birth.trim() {
                attrs.insert("birth_year".to_string(), birth);
            }
            if edit.note != base.note {
                attrs.insert("note".to_string(), edit.note.trim().to_string());
            }
            let root = self.settings.collection.clone();
            let person_id = person.id;
            self.undo.clear();
            self.launch("Person speichern", true, "mutation", move |_| {
                let c = Collection::open(Path::new(&root), true)?;
                crate::core::maintenance::set_person(&c, person_id, &attrs)?;
                Ok(Outcome::Done("Person gespeichert".into()))
            });
        }
    }

    fn detail_merge(&mut self, ui: &mut egui::Ui, person: &faces::Person) {
        let busy = self.busy();
        ui.label(RichText::new("Zusammenführen").size(17.0).strong());
        ui.label(muted(
            "Gehört dieselbe Person doppelt in der Liste? Alle Gesichter und Beziehungen dieser Person gehen auf die gewählte über. Die alte ID bleibt als Weiterleitung erhalten.",
        ));
        let people = self.face_page.people.clone();
        let mut target = self.people_ui.merge_target;
        let items = person_choices(&people, Some(person.id));
        let changed = FilterSelect::new("merge-target", &items)
            .placeholder("Person wählen, in die zusammengeführt wird …")
            .width(ui.available_width())
            .show(ui, &mut target);
        if changed {
            self.people_ui.merge_target = target;
            self.people_ui.merge_confirm = false;
        }
        let Some(target) = self.people_ui.merge_target else {
            return;
        };
        let target_name = people
            .iter()
            .find(|p| p.id == target)
            .map_or_else(|| format!("#{target}"), |p| p.name.clone());
        if !self.people_ui.merge_confirm {
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(format!("In „{target_name}“ zusammenführen …")),
                )
                .clicked()
            {
                self.people_ui.merge_confirm = true;
            }
            return;
        }
        let confirm = ui.group(|ui| {
            ui.label(
                RichText::new(format!(
                    "„{}“ (#{}) wirklich in „{target_name}“ (#{target}) zusammenführen? Das lässt sich hier nicht rückgängig machen.",
                    person.name, person.id
                ))
                .strong(),
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("Ja, zusammenführen"))
                    .clicked()
                {
                    let root = self.settings.collection.clone();
                    let source = person.id;
                    self.undo.clear();
                    self.people_ui.merge_confirm = false;
                    self.people_ui.merge_target = None;
                    self.relation_from = Some(target);
                    let config = self.settings.central();
                    self.launch("Personen zusammenführen", true, "mutation", move |_| {
                        let mut c = Collection::open(Path::new(&root), true)?;
                        let note =
                            crate::core::central::merge_people_synced(&mut c, &config, source, target)?;
                        Ok(Outcome::Synced(
                            Box::new(Outcome::Done(
                                "Personen zusammengeführt; alte ID bleibt als Weiterleitung erhalten"
                                    .into(),
                            )),
                            note,
                        ))
                    });
                }
                if ui.button("Abbrechen").clicked() {
                    self.people_ui.merge_confirm = false;
                }
            });
        });
        // Aus einer Rückfrage geöffnet: die Bestätigung einmal ins Bild holen.
        if std::mem::take(&mut self.people_ui.merge_reveal) {
            confirm.response.scroll_to_me(Some(egui::Align::Center));
        }
    }

    // ---------------------------------------------------------------- Rückfragen

    /// Alle offenen Rückfragen: Stammbaum und „Möglicherweise dieselbe Person“.
    pub(super) fn all_questions(&self) -> usize {
        self.open_questions() + self.open_duplicates(None).len()
    }

    /// Offene Rückfragen „dieselbe Person“, deren beide Personen noch bestehen; mit `only`
    /// nur die zu dieser Person.
    pub(super) fn open_duplicates(&self, only: Option<i64>) -> Vec<Candidate> {
        let exists = |id: i64| self.face_page.people.iter().any(|p| p.id == id);
        self.people_ui
            .duplicates
            .iter()
            .filter(|d| !self.people_ui.declined_now.contains(&(d.a, d.b)))
            .filter(|d| exists(d.a) && exists(d.b))
            .filter(|d| only.is_none_or(|p| d.a == p || d.b == p))
            .cloned()
            .collect()
    }

    /// Sucht ähnliche Personen auf einem Worker, sobald sich Personen, Beziehungen,
    /// bestätigte Gesichter oder Referenzen geändert haben; nie je Bild.
    pub(super) fn ensure_duplicates(&mut self) {
        if self.settings.collection.is_empty() || self.face_page.people.len() < 2 {
            return;
        }
        let status = &self.face_page.status;
        let key: DuplicatesKey = (
            self.settings.collection.clone(),
            self.face_page.people.len(),
            self.face_page.relations.len(),
            status.confirmed,
            status.references,
            status.dirty_people,
            status.embedder.clone(),
        );
        if self.people_ui.duplicates_key.as_ref() == Some(&key) {
            return;
        }
        self.people_ui.duplicates_key = Some(key);
        self.people_ui.duplicates_revision += 1;
        let revision = self.people_ui.duplicates_revision;
        let root = PathBuf::from(&self.settings.collection);
        self.launch(
            "Ähnliche Personen suchen",
            false,
            "person-duplicates",
            move |control| {
                let c = Collection::open(&root, false)?;
                c.cancel_with(&control);
                Ok(Outcome::Duplicates(revision, duplicates::load(&c)?))
            },
        );
    }

    /// Rückfragen „Möglicherweise dieselbe Person“ mit beiden Porträts. „Zusammenführen …“
    /// öffnet die bestehende Bestätigung in den Details, „Verschieden“ fragt nie wieder.
    pub(super) fn duplicates_list(&mut self, ui: &mut egui::Ui, only: Option<i64>) {
        let open = self.open_duplicates(only);
        if open.is_empty() {
            return;
        }
        let busy = self.busy();
        if only.is_none() {
            ui.label(RichText::new("Möglicherweise dieselbe Person").strong());
            ui.label(muted(
                "Die Gesichter dieser Personen sind einander sehr ähnlich, und sie stehen nie gemeinsam auf einem Foto. Zusammengeführt wird erst nach deiner Bestätigung.",
            ));
        }
        let root = self.settings.collection.clone();
        let exiftool = self.settings.exiftool.clone();
        let faces_of = |id: i64| {
            self.face_page
                .people
                .iter()
                .find(|p| p.id == id)
                .map_or(0, |p| p.count)
        };
        let mut merge = None;
        let mut decline = None;
        let mut photos = None;
        let mut select = None;
        for d in &open {
            let people = [
                (d.a, self.person_name(d.a), faces_of(d.a)),
                (d.b, self.person_name(d.b), faces_of(d.b)),
            ];
            // Die Person mit weniger bestätigten Gesichtern geht in die andere über.
            let (source, target) = if (people[1].2, d.a) > (people[0].2, d.b) {
                (&people[0], &people[1])
            } else {
                (&people[1], &people[0])
            };
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.label(format!(
                    "Sind {} und {} dieselbe Person?",
                    people[0].1, people[1].1
                ));
                ui.horizontal(|ui| {
                    for (id, name, count) in &people {
                        ui.vertical(|ui| {
                            let (rect, response) =
                                ui.allocate_exact_size(Vec2::splat(56.0), Sense::click());
                            paint_portrait(
                                ui,
                                rect.center(),
                                28.0,
                                self.face_page.portraits.get(id),
                                name,
                                &mut self.thumbnails,
                                &root,
                                &exiftool,
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("{name} wählen"),
                                )
                            });
                            if response
                                .on_hover_text(format!("{name} (#{id}) wählen"))
                                .clicked()
                            {
                                select = Some(*id);
                            }
                            ui.label(RichText::new(name.as_str()).strong());
                            ui.label(muted(format!(
                                "#{id} · {}",
                                super::plural(*count, "Gesicht", "Gesichter")
                            )));
                        });
                        ui.add_space(12.0);
                    }
                });
                ui.label(muted(format!(
                    "Ähnlichkeit {}",
                    crate::core::progress::decimal(d.similarity, 2)
                )));
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(!busy, egui::Button::new("Zusammenführen …"))
                        .on_hover_text(format!(
                            "Öffnet die Bestätigung: „{}“ (#{}) geht in „{}“ (#{}) über",
                            source.1, source.0, target.1, target.0
                        ))
                        .clicked()
                    {
                        merge = Some((source.0, target.0));
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Verschieden"))
                        .on_hover_text("Zwei verschiedene Personen; nie wieder fragen")
                        .clicked()
                    {
                        decline = Some((d.a, d.b));
                    }
                    if ui
                        .button("Fotos beider zeigen")
                        .on_hover_text(
                            "Sammlung mit Fotos, auf denen mindestens eine der beiden ist",
                        )
                        .clicked()
                    {
                        photos = Some(vec![d.a, d.b]);
                    }
                });
            });
        }
        if let Some(id) = select {
            self.select_person(id);
            self.people_ui.scroll_to_person(id);
        }
        if let Some((source, target)) = merge {
            // Bestehender Weg mit Bestätigung in den Details der aufgehenden Person.
            self.select_person(source);
            self.people_ui.scroll_to_person(source);
            self.people_ui.merge_for = Some(source);
            self.people_ui.merge_target = Some(target);
            self.people_ui.merge_confirm = true;
            self.people_ui.merge_reveal = true;
        }
        if let Some((a, b)) = decline {
            self.people_ui.declined_now.insert((a, b));
            let root = self.settings.collection.clone();
            self.launch("Als verschieden merken", true, "mutation", move |_| {
                let mut c = Collection::open(Path::new(&root), true)?;
                duplicates::decline(&mut c, a, b)?;
                Ok(Outcome::Done("Gemerkt: zwei verschiedene Personen".into()))
            });
        }
        if let Some(people) = photos {
            self.filters = Search {
                people_any: people,
                ..Default::default()
            };
            self.view = View::Collection;
            self.refresh_collection();
        }
    }

    /// Offene Rückfragen, alle oder nur die zu einer Person. Jeder Vorschlagstyp
    /// liefert seinen Text über `question_text`; Antworten sind für alle gleich.
    pub(super) fn questions_list(&mut self, ui: &mut egui::Ui, only: Option<i64>) {
        let open: Vec<Suggestion> = self
            .face_page
            .questions
            .iter()
            .filter(|s| !self.family_ui.is_snoozed(s))
            .filter(|s| only.is_none_or(|p| table::question_people(s).contains(&p)))
            .cloned()
            .collect();
        if open.is_empty() {
            return;
        }
        let busy = self.busy();
        let family = self.face_page.family.clone();
        if only.is_none() {
            ui.label(muted(
                "Das könnte im Stammbaum fehlen. Nichts wird ohne deine Antwort ergänzt.",
            ));
        }
        let mut answered: Option<(Suggestion, Answer)> = None;
        let mut snooze = None;
        let mut select = None;
        for s in &open {
            let name = |id: i64| self.person_name(id);
            let names = |ids: &[i64]| {
                ids.iter()
                    .map(|id| self.person_name(*id))
                    .collect::<Vec<_>>()
                    .join(" und ")
            };
            let (question, no_label) = question_text(s, &family, &names, &name);
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.label(question);
                ui.horizontal_wrapped(|ui| {
                    let yes = if s.candidates.len() > 1 {
                        "Ja, beide"
                    } else {
                        "Ja"
                    };
                    if ui.add_enabled(!busy, egui::Button::new(yes)).clicked() {
                        answered = Some((s.clone(), Answer::All));
                    }
                    if s.candidates.len() > 1 {
                        for &c in &s.candidates {
                            if ui
                                .add_enabled(
                                    !busy,
                                    egui::Button::new(format!(
                                        "Nur {} (Halbgeschwister)",
                                        self.person_name(c)
                                    )),
                                )
                                .clicked()
                            {
                                answered = Some((s.clone(), Answer::Only(c)));
                            }
                        }
                    }
                    if ui.add_enabled(!busy, egui::Button::new(no_label)).clicked() {
                        answered = Some((s.clone(), Answer::None));
                    }
                    if ui.button("Später").clicked() {
                        snooze = Some(s.clone());
                    }
                });
                if only.is_none() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(muted("Anzeigen:"));
                        for person in table::question_people(s)
                            .into_iter()
                            .collect::<BTreeSet<_>>()
                        {
                            if ui.small_button(self.person_name(person)).clicked() {
                                select = Some(person);
                            }
                        }
                    });
                }
            });
        }
        if let Some(s) = snooze {
            self.family_ui.snooze(&s);
        }
        if let Some(person) = select {
            self.select_person(person);
            self.people_ui.scroll_to = Some(person);
        }
        if let Some((suggestion, answer)) = answered {
            let root = self.settings.collection.clone();
            self.undo.clear();
            self.launch("Rückfrage beantworten", true, "mutation", move |_| {
                let mut c = Collection::open(Path::new(&root), true)?;
                gaps::apply(&mut c, &suggestion, answer)?;
                Ok(Outcome::Done("Antwort gespeichert".into()))
            });
        }
    }

    /// Rollen-Auswahleinträge für „X ist … von Y“, beschriftet nach dem Geschlecht von X.
    pub(super) fn role_choices(&self, gender: Gender) -> Vec<Choice<Role>> {
        [
            Role::Child,
            Role::Parent,
            Role::Partner,
            Role::FormerPartner,
            Role::Friend,
            Role::Acquaintance,
            Role::Sibling,
            Role::Cousin,
        ]
        .into_iter()
        .filter(|role| {
            !matches!(role, Role::Sibling | Role::Cousin) || self.family_ui.show_shortcuts
        })
        .map(|role| {
            let label = role.label(gender);
            if matches!(role, Role::Sibling | Role::Cousin) {
                Choice::new(
                    role,
                    format!("{label} (Behelf)"),
                    "nur ohne erfasste Eltern",
                )
            } else {
                Choice::new(role, label, "")
            }
        })
        .collect()
    }
}

/// Rundes Porträt aus dem Vorschau-Cache; Initialen, solange es fehlt oder lädt.
#[allow(clippy::too_many_arguments)]
fn paint_portrait(
    ui: &egui::Ui,
    centre: Pos2,
    radius: f32,
    portrait: Option<&Face>,
    name: &str,
    thumbnails: &mut Thumbnails,
    root: &str,
    exiftool: &str,
) {
    let texture = portrait.and_then(|face| {
        thumbnails.texture(
            ui.ctx(),
            PreviewRequest {
                root: Some(PathBuf::from(root)),
                path: Path::new(root).join(&face.source),
                orientation: 1,
                crop: Some(face.bbox),
                edge: 96,
                generation: 0,
            },
            exiftool,
        )
    });
    match texture {
        Some((id, size)) => textured_circle(
            ui.painter(),
            centre,
            radius,
            id,
            square_uv(size),
            Color32::WHITE,
        ),
        None => {
            ui.painter()
                .circle_filled(centre, radius, ui.visuals().faint_bg_color);
            ui.painter().text(
                centre,
                Align2::CENTER_CENTER,
                super::tree_view::initials(name),
                FontId::proportional((radius * 0.8).max(10.0)),
                theme::MUTED,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_row(
    ui: &mut egui::Ui,
    rect: Rect,
    response: &egui::Response,
    selected: bool,
    row: &PersonRow,
    layout: &[(Column, f32, f32)],
    portrait: Option<&Face>,
    thumbnails: &mut Thumbnails,
    root: &str,
    exiftool: &str,
) {
    let visuals = ui.visuals().clone();
    let fill = if selected {
        visuals.selection.bg_fill
    } else if response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 0, fill);
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height())),
            0,
            theme::ACCENT,
        );
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(
            1.0_f32,
            visuals
                .widgets
                .noninteractive
                .bg_stroke
                .color
                .gamma_multiply(0.5),
        ),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // Mini-Porträt: nur für sichtbare Zeilen geladen, Initialen bis es vorliegt.
    let centre = Pos2::new(
        rect.left() + 8.0 + PORTRAIT_SIZE / 2.0 + 2.0,
        rect.center().y,
    );
    paint_portrait(
        ui,
        centre,
        PORTRAIT_SIZE / 2.0,
        portrait,
        &row.name,
        thumbnails,
        root,
        exiftool,
    );
    for &(column, x, w) in layout {
        let cell = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, rect.height()));
        let text = cell_text(row, column);
        let empty = text == "–";
        let colour = if column == Column::Hints && row.hints > 0 {
            theme::WARM
        } else if empty {
            theme::MUTED.gamma_multiply(0.7)
        } else if column == Column::Name {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        };
        let painter = ui
            .painter()
            .with_clip_rect(cell.shrink2(Vec2::new(4.0, 0.0)));
        let (anchor, align) = if numeric(column) {
            (
                Pos2::new(cell.right() - 12.0, cell.center().y),
                Align2::RIGHT_CENTER,
            )
        } else {
            (
                Pos2::new(cell.left() + 8.0, cell.center().y),
                Align2::LEFT_CENTER,
            )
        };
        painter.text(
            anchor,
            align,
            text,
            FontId::proportional(if column == Column::Name { 15.0 } else { 14.0 }),
            colour,
        );
    }
}

#[cfg(test)]
impl PeopleUi {
    pub(super) fn draft_mut(&mut self) -> Option<&mut (i64, Fields, Fields)> {
        self.draft.as_mut()
    }
}
