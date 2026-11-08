//! Folder tree side panel of the collection view (ADR-0057). Loading and expansion state live in
//! `core::folders::FolderTree`; this file only draws rows and starts workers.

use super::*;
use crate::core::folders::{self, Row};

const ROW_HEIGHT: f32 = 24.0;
const INDENT: f32 = 14.0;

/// What a click on a row asks for.
enum Action {
    Select(Option<String>),
    Toggle(String),
    More(String),
    Retry,
}

impl PhotoApp {
    /// Keeps the tree in step with the collection, the selected folder and running workers.
    /// Slow work only ever runs on a worker; stale answers are dropped by revision.
    pub(super) fn sync_folder_tree(&mut self) {
        if self.status.files == 0 {
            return;
        }
        let key = (
            self.settings.collection.clone(),
            self.status.photos,
            self.status.files,
        );
        if key != self.folder_key {
            for task in &self.tasks {
                if task.kind == "folders" {
                    task.task.control.stop();
                }
            }
            if key.0 != self.folder_key.0 {
                self.folder_tree = folders::FolderTree::default();
            } else {
                self.folder_tree.reset();
            }
            self.folder_revision += 1;
            self.folder_key = key;
            self.folder_revealed = None;
        }
        if self.folder_revealed != self.applied.folder {
            if let Some(folder) = &self.applied.folder {
                self.folder_tree.reveal(folder);
            }
            self.folder_revealed = self.applied.folder.clone();
        }
        if self.folder_tree.is_loading() || self.tasks.iter().any(|task| task.kind == "folders") {
            return;
        }
        let batch = self.folder_tree.wanted();
        if batch.is_empty() {
            return;
        }
        self.folder_tree.begin(&batch);
        let revision = self.folder_revision;
        let root = PathBuf::from(&self.settings.collection);
        self.launch("Ordner lesen", false, "folders", move |control| {
            let collection = Collection::open(&root, false)?;
            collection.cancel_with(&control);
            let mut levels = Vec::new();
            for (parent, page) in batch {
                control.checkpoint()?;
                let loaded = folders::children(&collection, &parent, page)?;
                levels.push((parent, page, loaded));
            }
            Ok(Outcome::Folders(revision, levels))
        });
    }

    pub(super) fn folder_panel(&mut self, ui: &mut egui::Ui, width: f32, height: f32) {
        ui.allocate_ui_with_layout(
            egui::vec2(width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Ordner").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("‹")
                            .on_hover_text("Ordnerbaum einklappen")
                            .clicked()
                        {
                            self.settings.folder_tree_open = false;
                            self.save();
                        }
                        let busy = self.folder_tree.is_loading();
                        if ui
                            .add_enabled(!busy, egui::Button::new("↻").small())
                            .on_hover_text("Ordner neu lesen")
                            .clicked()
                        {
                            self.folder_tree.reset();
                            self.folder_revision += 1;
                        }
                    });
                });
                let selected = self.applied.folder.clone();
                let root_name = Path::new(&self.settings.collection)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Sammlung")
                    .to_owned();
                let rows = self.folder_tree.rows();
                let mut action = None;
                egui::ScrollArea::vertical()
                    .id_salt("folder-tree")
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW_HEIGHT, rows.len() + 1, |ui, range| {
                        for index in range {
                            let Some(row) = index.checked_sub(1).map(|i| &rows[i]) else {
                                let (response, _) = tree_row(
                                    ui,
                                    0,
                                    &root_name,
                                    Some(self.status.photos),
                                    None,
                                    selected.is_none(),
                                    "Gesamte Sammlung",
                                );
                                if response.clicked() && selected.is_some() {
                                    action = Some(Action::Select(None));
                                }
                                continue;
                            };
                            match row {
                                Row::Node {
                                    path,
                                    name,
                                    depth,
                                    photos,
                                    has_children,
                                    expanded,
                                } => {
                                    let (response, arrow) = tree_row(
                                        ui,
                                        depth + 1,
                                        name,
                                        Some(*photos),
                                        has_children.then_some(*expanded),
                                        selected.as_deref() == Some(path.as_str()),
                                        path,
                                    );
                                    if response.has_focus() {
                                        let key = |k| ui.input(|i| i.key_pressed(k));
                                        if *has_children
                                            && (key(egui::Key::ArrowRight) && !*expanded
                                                || key(egui::Key::ArrowLeft) && *expanded)
                                        {
                                            action = Some(Action::Toggle(path.clone()));
                                        }
                                    }
                                    if response.clicked() {
                                        action = Some(if arrow && *has_children {
                                            Action::Toggle(path.clone())
                                        } else if selected.as_deref() == Some(path.as_str()) {
                                            continue;
                                        } else {
                                            Action::Select(Some(path.clone()))
                                        });
                                    }
                                }
                                Row::More { parent, depth } => {
                                    let enabled = !self.folder_tree.is_loading();
                                    ui.horizontal(|ui| {
                                        ui.add_space(8.0 + (*depth + 1) as f32 * INDENT);
                                        if ui
                                            .add_enabled(
                                                enabled,
                                                egui::Button::new("weitere …").small(),
                                            )
                                            .on_hover_text("Nächste Ordner dieser Ebene laden")
                                            .clicked()
                                        {
                                            action = Some(Action::More(parent.clone()));
                                        }
                                    });
                                }
                                Row::Loading { depth } => {
                                    ui.horizontal(|ui| {
                                        ui.add_space(8.0 + (*depth + 1) as f32 * INDENT);
                                        ui.spinner();
                                        ui.label(RichText::new("lädt …").color(theme::MUTED));
                                    });
                                }
                                Row::Failed { depth } => {
                                    ui.horizontal(|ui| {
                                        ui.add_space(8.0 + (*depth + 1) as f32 * INDENT);
                                        if ui
                                            .small_button("Ordner nicht lesbar · erneut versuchen")
                                            .clicked()
                                        {
                                            action = Some(Action::Retry);
                                        }
                                    });
                                }
                            }
                        }
                    });
                match action {
                    Some(Action::Select(folder)) => self.navigate_collection(0, folder),
                    Some(Action::Toggle(path)) => self.folder_tree.toggle(&path),
                    Some(Action::More(parent)) => self.folder_tree.request_more(&parent),
                    Some(Action::Retry) => self.folder_tree.retry_failed(),
                    None => {}
                }
            },
        );
    }

    /// Draggable edge between tree and gallery; the width is clamped and persisted.
    pub(super) fn folder_tree_handle(&mut self, ui: &mut egui::Ui, height: f32) {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(6.0, height), egui::Sense::click_and_drag());
        let response = response
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
            .on_hover_text("Breite des Ordnerbaums ändern");
        if response.dragged() {
            let (min, max) = config::FOLDER_TREE_WIDTH;
            self.settings.folder_tree_width =
                (self.settings.folder_tree_width + response.drag_delta().x).clamp(min, max);
        }
        if response.drag_stopped() {
            self.save();
        }
        let color = if response.hovered() || response.dragged() {
            theme::ACCENT
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke.color
        };
        ui.painter().line_segment(
            [rect.center_top(), rect.center_bottom()],
            egui::Stroke::new(1.0_f32, color),
        );
    }
}

/// One tree line. Returns the response of the whole row and whether the click hit the arrow.
fn tree_row(
    ui: &mut egui::Ui,
    depth: usize,
    name: &str,
    count: Option<usize>,
    expanded: Option<bool>,
    selected: bool,
    description: &str,
) -> (egui::Response, bool) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );
    let arrow_rect = egui::Rect::from_min_size(
        rect.min + egui::vec2(2.0 + depth as f32 * INDENT, 0.0),
        egui::vec2(18.0, ROW_HEIGHT),
    );
    let on_arrow = response
        .interact_pointer_pos()
        .is_some_and(|pos| arrow_rect.contains(pos));
    let state = match expanded {
        Some(true) => ", aufgeklappt",
        Some(false) => ", zugeklappt",
        None => "",
    };
    let label = format!(
        "{description}, {} Fotos{state}",
        count.map_or_else(|| "?".to_owned(), |n| n.to_string())
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &label)
    });
    let visuals = ui.visuals();
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 4.0, theme::ACCENT.gamma_multiply(0.28));
    } else if response.hovered() || response.has_focus() {
        painter.rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let font = egui::TextStyle::Body.resolve(ui.style());
    let text_color = if selected {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    if let Some(open) = expanded {
        // Drawn as shapes: the bundled fonts have no triangle glyphs.
        let c = arrow_rect.center();
        let points = if open {
            vec![
                c + egui::vec2(-4.5, -2.5),
                c + egui::vec2(4.5, -2.5),
                c + egui::vec2(0.0, 3.5),
            ]
        } else {
            vec![
                c + egui::vec2(-2.5, -4.5),
                c + egui::vec2(-2.5, 4.5),
                c + egui::vec2(3.5, 0.0),
            ]
        };
        painter.add(egui::Shape::convex_polygon(
            points,
            theme::MUTED,
            egui::Stroke::NONE,
        ));
    }
    let mut right = rect.right() - 8.0;
    if let Some(count) = count {
        let galley = painter.layout_no_wrap(count.to_string(), font.clone(), theme::MUTED);
        right -= galley.size().x;
        painter.galley(
            egui::pos2(right, rect.center().y - galley.size().y / 2.0),
            galley,
            theme::MUTED,
        );
        right -= 8.0;
    }
    let left = arrow_rect.right() + 2.0;
    let mut job =
        egui::text::LayoutJob::simple(name.to_owned(), font, text_color, (right - left).max(20.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    let pos = egui::pos2(left, rect.center().y - galley.size().y / 2.0);
    painter.galley(pos, galley, text_color);
    let response = response.on_hover_text(label);
    (response, on_arrow)
}
