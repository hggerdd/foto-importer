use super::*;
use crate::core::relationship_graph::{self, Connection, Graph};
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke};

#[derive(Default)]
pub(super) struct RelationshipView {
    open: bool,
    focus: Option<i64>,
    filter: String,
    pub(super) revision: u64,
    pub(super) data: Option<Graph>,
}

impl RelationshipView {
    pub(super) fn clear(&mut self) {
        *self = Self {
            revision: self.revision + 1,
            ..Self::default()
        };
    }
}

impl PhotoApp {
    pub(super) fn open_relationship_graph(&mut self, person: i64) {
        self.relationship_graph.open = true;
        self.relationship_graph.focus = Some(person);
        self.relationship_graph.data = None;
        self.relationship_graph.revision += 1;
        let revision = self.relationship_graph.revision;
        let root = self.settings.collection.clone();
        self.launch(
            "Beziehungsgraph berechnen",
            false,
            "relationship-graph",
            move |control| {
                let collection = Collection::open(Path::new(&root), false)?;
                collection.cancel_with(&control);
                collection.db.execute_batch("BEGIN")?;
                Ok(Outcome::RelationshipGraph(
                    revision,
                    relationship_graph::for_person(&collection, person)?,
                ))
            },
        );
    }

    pub(super) fn show_relationship_graph(&mut self, ctx: &egui::Context) {
        if !self.relationship_graph.open {
            return;
        }
        let mut next = None;
        let mut pair = None;
        let response = egui::Modal::new(egui::Id::new("relationship-graph")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 72.0).clamp(300.0, 880.0));
            ui.heading("Beziehungsgraph");
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.relationship_graph.filter)
                    .hint_text("Person finden …").desired_width(170.0));
                let filter = files::fold(self.relationship_graph.filter.trim());
                egui::ComboBox::from_id_salt("graph-person")
                    .selected_text("Mittelpunkt wechseln …")
                    .show_ui(ui, |ui| {
                        let people = self.face_page.people.iter().filter(|p| files::fold(&p.name).contains(&filter));
                        let mut found = false;
                        for p in people.take(50) {
                            found = true;
                            if ui.selectable_label(self.relationship_graph.focus == Some(p.id), format!("{} · #{}", p.name, p.id)).clicked() {
                                next = Some(p.id);
                            }
                        }
                        if !found {
                            ui.label("Keine passende Person");
                        }
                    });
                if ui.button("Aktualisieren").clicked() {
                    next = self.relationship_graph.focus;
                }
            });
            ui.label(RichText::new(
                "Dicker = mehr gemeinsame Fotos in dieser Ansicht · Gestrichelt = eingetragene Beziehung ohne gemeinsames Foto",
            ).small());
            if let Some(graph) = &self.relationship_graph.data {
                ui.label(RichText::new(format!("Mittelpunkt: {} · #{}", graph.name, graph.focus)).strong());
                let height = (ctx.content_rect().height() - 270.0).clamp(170.0, 430.0);
                if graph.connections.is_empty() {
                    ui.label(format!("Für {} sind keine Verbindungen gespeichert.", graph.name));
                    ui.label("Bestätige die Namen von Personen auf gemeinsamen Fotos.");
                } else {
                    next = draw_graph(ui, graph, height).or(next);
                    ui.menu_button("Verbindungen und gemeinsame Fotos …", |ui| {
                        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                            for edge in &graph.connections {
                                ui.horizontal_wrapped(|ui| {
                                    let label = edge_description(edge);
                                    ui.label(label);
                                    if edge.photos > 0 && ui.small_button("Fotos anzeigen").clicked() {
                                        pair = Some([graph.focus, edge.person]);
                                        ui.close();
                                    }
                                    if ui.small_button("Als Mittelpunkt").clicked() {
                                        next = Some(edge.person);
                                        ui.close();
                                    }
                                });
                            }
                        });
                    });
                    if graph.truncated {
                        ui.label(RichText::new(format!("Die {} stärksten Verbindungen werden gezeigt.", relationship_graph::NEIGHBOR_LIMIT)).small());
                    }
                }
            } else if self.tasks.iter().any(|task| task.kind == "relationship-graph") {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Gemeinsame Fotos werden gezählt …");
                });
            } else {
                ui.label("Der Graph konnte nicht geladen werden. Versuche es über Aktualisieren.");
            }
            if ui.button("Schließen").clicked() {
                self.relationship_graph.open = false;
            }
        });
        if response.should_close() {
            self.relationship_graph.open = false;
        }
        if let Some(persons) = pair {
            self.relationship_graph.open = false;
            self.filters = Search {
                people_all: persons.to_vec(),
                ..Search::default()
            };
            self.view = View::Collection;
            self.refresh_collection();
        } else if let Some(person) = next {
            self.open_relationship_graph(person);
        }
    }
}

fn edge_description(edge: &Connection) -> String {
    let mut label = format!(
        "{} · #{} · {} gemeinsame Fotos",
        edge.name, edge.person, edge.photos
    );
    if let Some(kind) = &edge.recorded {
        label.push_str(" · Beziehung vom Mittelpunkt: ");
        label.push_str(kind);
    }
    label
}

fn stroke_width(photos: usize, max: usize) -> f32 {
    if max <= 1 {
        1.5
    } else {
        1.5 + 6.0 * (photos.max(1) as f32).ln() / (max as f32).ln()
    }
}

fn draw_graph(ui: &mut egui::Ui, graph: &Graph, height: f32) -> Option<i64> {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 10, ui.visuals().faint_bg_color);
    let center = rect.center() - egui::vec2(0.0, 7.0);
    let accent = if ui.visuals().dark_mode {
        theme::ACCENT
    } else {
        Color32::from_rgb(24, 105, 93)
    };
    let max = graph
        .connections
        .iter()
        .map(|edge| edge.photos)
        .max()
        .unwrap_or(1);
    let positions: Vec<_> = (0..graph.connections.len())
        .map(|index| {
            let angle = std::f32::consts::TAU * index as f32 / graph.connections.len() as f32
                - std::f32::consts::FRAC_PI_2;
            center
                + egui::vec2(
                    angle.cos() * rect.width() * 0.37,
                    angle.sin() * rect.height() * 0.35,
                )
        })
        .collect();
    for (edge, &position) in graph.connections.iter().zip(&positions) {
        if edge.photos == 0 {
            painter.extend(egui::Shape::dashed_line(
                &[center, position],
                Stroke::new(1.5_f32, ui.visuals().weak_text_color()),
                5.0,
                5.0,
            ));
        } else {
            painter.line_segment(
                [center, position],
                Stroke::new(stroke_width(edge.photos, max), accent),
            );
        }
    }
    paint_node(ui, &painter, center, &graph.name, 19.0, accent, true);
    let mut selected = None;
    for (edge, &position) in graph.connections.iter().zip(&positions) {
        let response = ui.interact(
            Rect::from_center_size(position, egui::vec2(44.0, 37.0)),
            ui.id().with(("graph-person", edge.person)),
            Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                format!("{} als Mittelpunkt", edge_description(edge)),
            )
        });
        paint_node(
            ui,
            &painter,
            position,
            &edge.name,
            13.0,
            accent,
            response.hovered() || response.has_focus(),
        );
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!(
                "{} · Klick wechselt den Mittelpunkt",
                edge_description(edge)
            ))
            .clicked()
        {
            selected = Some(edge.person);
        }
    }
    selected
}

fn paint_node(
    ui: &egui::Ui,
    painter: &egui::Painter,
    position: Pos2,
    name: &str,
    radius: f32,
    accent: Color32,
    highlighted: bool,
) {
    let fill = if highlighted {
        accent
    } else {
        ui.visuals().window_fill
    };
    painter.circle(position, radius, fill, Stroke::new(2.0_f32, accent));
    let text = if name.chars().count() > 13 {
        format!("{}…", name.chars().take(12).collect::<String>())
    } else {
        name.to_owned()
    };
    painter.text(
        position + egui::vec2(0.0, radius + 4.0),
        Align2::CENTER_TOP,
        text,
        FontId::proportional(if radius > 15.0 { 14.0 } else { 12.0 }),
        ui.visuals().text_color(),
    );
}
