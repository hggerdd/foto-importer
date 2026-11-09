//! Auswahlfeld mit Tippfilter (ADR 0063): Klick öffnet ein Popup, das Suchfeld
//! erhält sofort den Fokus, Tippen filtert die Treffer ohne Verzögerung.
//!
//! Hauptziel ist die Maus: große Trefferzeilen, ein Klick wählt. Pfeiltasten,
//! Enter und Esc sind Zusatz. Filterlogik und Reihenfolge stehen in
//! `core::choice_filter` (Präfixtreffer vor Teiltreffern, stabil).
use super::theme;
use crate::core::{
    choice_filter::{self, Matcher},
    faces::Person,
};
use egui::{Align2, Color32, FontId, Id, Key, Modifiers, Popup, PopupCloseBehavior, RichText};

/// Höchstens so viele Treffer werden gezeichnet; der Rest steht als „… weitere“.
pub const MAX_HITS: usize = 60;
/// Ab so vielen Einträgen erscheint das Suchfeld; kürzere Listen zeigen nur Zeilen.
pub const SEARCH_FROM: usize = 8;
const ROW_HEIGHT: f32 = 36.0;
const LIST_HEIGHT: f32 = 288.0;

/// Ein Eintrag: `value` wird ausgewählt, `label` und `detail` sind sichtbar und
/// durchsuchbar.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice<T> {
    pub value: T,
    pub label: String,
    pub detail: String,
}

impl<T> Choice<T> {
    pub fn new(value: T, label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            detail: detail.into(),
        }
    }
    fn search_text(&self) -> String {
        format!("{} {}", self.label, self.detail)
    }
}

/// Einträge für eine Personenliste: Name, dazu „#ID · N Gesichter · * Jahr“.
pub fn person_choices(people: &[Person], exclude: Option<i64>) -> Vec<Choice<i64>> {
    people
        .iter()
        .filter(|p| Some(p.id) != exclude)
        .map(|p| {
            let mut detail = format!("#{}", p.id);
            if p.count > 0 {
                detail.push_str(&format!(" · {} Gesichter", p.count));
            }
            if !p.birth.is_empty() {
                detail.push_str(&format!(" · * {}", p.birth));
            }
            Choice::new(p.id, p.name.clone(), detail)
        })
        .collect()
}

/// Zustand des Popups: wird beim Öffnen zurückgesetzt.
#[derive(Clone, Default)]
struct State {
    query: String,
    highlight: usize,
    focus_pending: bool,
}

/// Einstellungen eines Auswahlfelds; `show` zeichnet es und meldet eine Änderung.
pub struct FilterSelect<'a, T> {
    id: Id,
    items: &'a [Choice<T>],
    placeholder: &'a str,
    width: f32,
    open_with: Option<&'a str>,
}

impl<'a, T: Clone + PartialEq> FilterSelect<'a, T> {
    pub fn new(id_salt: impl std::hash::Hash, items: &'a [Choice<T>]) -> Self {
        Self {
            id: Id::new(("filter-select", id_salt)),
            items,
            placeholder: "Auswählen …",
            width: 240.0,
            open_with: None,
        }
    }
    pub fn placeholder(mut self, text: &'a str) -> Self {
        self.placeholder = text;
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
    /// Nur für Sichtprüfungen: das Popup mit diesem Suchtext geöffnet halten.
    pub fn open_with(mut self, query: Option<&'a str>) -> Self {
        self.open_with = query;
        self
    }

    /// Zeichnet das Feld. Gibt `true` zurück, wenn `selected` geändert wurde.
    pub fn show(self, ui: &mut egui::Ui, selected: &mut Option<T>) -> bool {
        let current = selected
            .as_ref()
            .and_then(|value| self.items.iter().find(|c| &c.value == value));
        let caption = current.map_or(self.placeholder.to_string(), |c| c.label.clone());
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(self.width, theme::CONTROL_HEIGHT),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), &caption)
        });
        let popup_id = Popup::default_response_id(&response);
        let open = Popup::is_id_open(ui.ctx(), popup_id);
        if ui.is_rect_visible(rect) {
            let visuals = ui.visuals();
            let widget = if open || response.hovered() || response.has_focus() {
                &visuals.widgets.hovered
            } else {
                &visuals.widgets.inactive
            };
            ui.painter().rect(
                rect,
                8,
                widget.weak_bg_fill,
                widget.bg_stroke,
                egui::StrokeKind::Inside,
            );
            let colour = if current.is_some() {
                visuals.text_color()
            } else {
                visuals.weak_text_color()
            };
            let text_rect = rect.shrink2(egui::vec2(12.0, 0.0));
            let clip = egui::Rect::from_min_max(
                text_rect.min,
                egui::pos2(rect.right() - 26.0, rect.bottom()),
            );
            ui.painter().with_clip_rect(clip).text(
                egui::pos2(text_rect.left(), rect.center().y),
                Align2::LEFT_CENTER,
                &caption,
                FontId::proportional(15.0),
                colour,
            );
            let (x, y) = (rect.right() - 16.0, rect.center().y);
            let points = if open {
                vec![
                    egui::pos2(x - 5.0, y + 3.0),
                    egui::pos2(x + 5.0, y + 3.0),
                    egui::pos2(x, y - 3.0),
                ]
            } else {
                vec![
                    egui::pos2(x - 5.0, y - 3.0),
                    egui::pos2(x + 5.0, y - 3.0),
                    egui::pos2(x, y + 3.0),
                ]
            };
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                visuals.weak_text_color(),
                egui::Stroke::NONE,
            ));
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let state_id = self.id.with("state");
        if response.clicked() && !open {
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    state_id,
                    State {
                        focus_pending: true,
                        ..Default::default()
                    },
                )
            });
        }
        let forced = self.open_with.is_some();
        if let Some(query) = self.open_with
            && !open
        {
            Popup::open_id(ui.ctx(), popup_id);
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    state_id,
                    State {
                        query: query.to_string(),
                        highlight: 0,
                        focus_pending: false,
                    },
                )
            });
        }
        let mut chosen: Option<Option<T>> = None;
        Popup::from_toggle_button_response(&response)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .width(self.width.max(320.0))
            .show(|ui| {
                ui.set_min_width(self.width.max(320.0));
                let mut state: State = ui
                    .ctx()
                    .data_mut(|d| d.get_temp(state_id))
                    .unwrap_or_default();
                chosen = self.popup_content(ui, &mut state, selected.as_ref());
                ui.ctx().data_mut(|d| d.insert_temp(state_id, state));
            });
        if let Some(choice) = chosen {
            let changed = *selected != choice;
            *selected = choice;
            if !forced {
                Popup::close_id(ui.ctx(), popup_id);
            }
            return changed;
        }
        false
    }

    fn popup_content(
        &self,
        ui: &mut egui::Ui,
        state: &mut State,
        selected: Option<&T>,
    ) -> Option<Option<T>> {
        let mut chosen = None;
        let searchable = self.items.len() >= SEARCH_FROM || !state.query.is_empty();
        let mut moved = false;
        if searchable {
            let (down, up) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                )
            });
            let hits_before = self.hits(&state.query).len().min(MAX_HITS);
            if down {
                state.highlight = choice_filter::move_highlight(state.highlight, hits_before, 1);
                moved = true;
            }
            if up {
                state.highlight = choice_filter::move_highlight(state.highlight, hits_before, -1);
                moved = true;
            }
            let edit = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Tippen zum Filtern …")
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(10, 8)),
            );
            if state.focus_pending || !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
                state.focus_pending = false;
            }
            if edit.changed() {
                state.highlight = 0;
            }
        }
        let hits = self.hits(&state.query);
        state.highlight = state
            .highlight
            .min(hits.len().saturating_sub(1).min(MAX_HITS - 1));
        let enter = searchable && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        if enter && let Some(&index) = hits.get(state.highlight) {
            chosen = Some(Some(self.items[index].value.clone()));
        }
        ui.add_space(4.0);
        if hits.is_empty() {
            ui.label(
                RichText::new(if self.items.is_empty() {
                    "Keine Einträge vorhanden."
                } else {
                    "Keine Treffer. Anderen Suchbegriff versuchen."
                })
                .color(theme::MUTED),
            );
            return chosen;
        }
        let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        egui::ScrollArea::vertical()
            .id_salt("filter-select-list")
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (position, &index) in hits.iter().take(MAX_HITS).enumerate() {
                    let item = &self.items[index];
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), ROW_HEIGHT),
                        egui::Sense::click(),
                    );
                    let is_selected = selected == Some(&item.value);
                    let highlighted = position == state.highlight && searchable;
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            true,
                            is_selected,
                            format!("{} {}", item.label, item.detail),
                        )
                    });
                    paint_row(
                        ui,
                        rect,
                        &response,
                        is_selected,
                        highlighted,
                        &item.label,
                        &item.detail,
                    );
                    if highlighted && moved {
                        response.scroll_to_me(None);
                    }
                    if response.hovered() && pointer_moved {
                        state.highlight = position;
                    }
                    if response.clicked() {
                        chosen = Some(Some(item.value.clone()));
                    }
                }
                if hits.len() > MAX_HITS {
                    ui.label(
                        RichText::new(format!(
                            "… {} weitere. Mehr Buchstaben eingeben, um einzugrenzen.",
                            hits.len() - MAX_HITS
                        ))
                        .color(theme::MUTED),
                    );
                }
            });
        chosen
    }

    fn hits(&self, query: &str) -> Vec<usize> {
        if Matcher::new(query).is_empty() {
            return (0..self.items.len()).collect();
        }
        choice_filter::filter_ranked(self.items, query, Choice::search_text)
    }
}

fn paint_row(
    ui: &egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    selected: bool,
    highlighted: bool,
    label: &str,
    detail: &str,
) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let visuals = ui.visuals();
    let fill = if highlighted || response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else if selected {
        visuals.selection.bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), 6, fill);
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
            2,
            theme::ACCENT,
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let detail_galley = (!detail.is_empty()).then(|| {
        ui.painter().layout_no_wrap(
            detail.to_string(),
            FontId::proportional(13.0),
            visuals.weak_text_color(),
        )
    });
    let detail_width = detail_galley.as_ref().map_or(0.0, |g| g.size().x + 12.0);
    let label_clip = rect.with_max_x((rect.right() - detail_width - 8.0).max(rect.left() + 40.0));
    ui.painter().with_clip_rect(label_clip).text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(15.0),
        if selected {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        },
    );
    if let Some(galley) = detail_galley {
        ui.painter().galley(
            egui::pos2(
                rect.right() - 10.0 - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            visuals.weak_text_color(),
        );
    }
}

/// Auswahlfeld für Personen: Name und ID, Tippen filtert.
pub fn person_select(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    people: &[Person],
    exclude: Option<i64>,
    selected: &mut Option<i64>,
    placeholder: &str,
    width: f32,
) -> bool {
    let items = person_choices(people, exclude);
    FilterSelect::new(id_salt, &items)
        .placeholder(placeholder)
        .width(width)
        .show(ui, selected)
}
