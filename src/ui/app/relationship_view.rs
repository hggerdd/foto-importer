//! Netzwerkgraph aller Personen als Reiter „Graph“ im Personenbereich (ADR-0058).
//!
//! Das Layout entsteht einmal auf dem Worker (`relationship_graph::network`);
//! die Oberfläche zeichnet nur und bewegt Ausschnitt und Zoom.
use super::*;
use crate::core::relationship_graph::{self, Edge, Network, Recorded};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::HashSet;

pub(super) struct RelationshipView {
    pub(super) revision: u64,
    pub(super) data: Option<Network>,
    /// Kleingeschriebene Namen in Knotenreihenfolge für die Suche.
    names: Vec<String>,
    /// Daten sind veraltet (Sammlung wurde geändert) und werden neu berechnet.
    pub(super) stale: bool,
    attempted: bool,
    pub(super) selected: Option<i64>,
    filter: String,
    centre: [f32; 2],
    scale: f32,
    fit: bool,
    focus_request: Option<i64>,
    show_recorded: bool,
    show_photos: bool,
    min_photos: usize,
}

impl Default for RelationshipView {
    fn default() -> Self {
        Self {
            revision: 0,
            data: None,
            names: Vec::new(),
            stale: false,
            attempted: false,
            selected: None,
            filter: String::new(),
            centre: [0.0, 0.0],
            scale: 40.0,
            fit: true,
            focus_request: None,
            show_recorded: true,
            show_photos: true,
            min_photos: 1,
        }
    }
}

impl RelationshipView {
    pub(super) fn clear(&mut self) {
        *self = Self {
            revision: self.revision + 1,
            ..Self::default()
        };
    }

    /// Übernimmt ein berechnetes Netz. Auswahl, Ausschnitt und Ebenen bleiben erhalten.
    pub(super) fn accept(&mut self, network: Network) {
        if self.data.is_none() {
            self.min_photos = default_threshold(&network);
        }
        self.names = network
            .nodes
            .iter()
            .map(|node| files::fold(&node.name))
            .collect();
        if self
            .selected
            .is_some_and(|person| network.index_of(person).is_none())
        {
            self.selected = None;
        }
        self.data = Some(network);
    }
}

/// Kleinste Schwelle gemeinsamer Fotos, bei der höchstens vier Fotolinien je Person bleiben.
fn default_threshold(network: &Network) -> usize {
    let mut counts: Vec<usize> = network
        .edges
        .iter()
        .map(|edge| edge.photos)
        .filter(|photos| *photos > 0)
        .collect();
    counts.sort_unstable_by(|a, b| b.cmp(a));
    let allowed = (network.nodes.len() * 4).max(8);
    counts.get(allowed).map_or(1, |photos| photos + 1)
}

struct Palette {
    photo: Color32,
    parent: Color32,
    partner: Color32,
    sibling: Color32,
    other: Color32,
    ring: Color32,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            photo: Color32::from_rgb(120, 148, 176),
            parent: theme::WARM,
            partner: Color32::from_rgb(236, 130, 190),
            sibling: Color32::from_rgb(150, 160, 255),
            other: Color32::from_rgb(170, 180, 150),
            ring: theme::ACCENT,
        }
    } else {
        Palette {
            photo: Color32::from_rgb(88, 106, 128),
            parent: Color32::from_rgb(176, 98, 8),
            partner: Color32::from_rgb(176, 48, 118),
            sibling: Color32::from_rgb(64, 76, 200),
            other: Color32::from_rgb(96, 110, 70),
            ring: Color32::from_rgb(24, 105, 93),
        }
    }
}

fn recorded_label(kind: &Recorded) -> String {
    match kind {
        Recorded::Parent => "Elternteil von".into(),
        Recorded::Child => "Kind von".into(),
        Recorded::Partner => "Partner".into(),
        Recorded::Sibling => "Geschwister".into(),
        Recorded::DerivedSibling { half: false } => "Geschwister (gemeinsame Eltern)".into(),
        Recorded::DerivedSibling { half: true } => {
            "Halbgeschwister (ein gemeinsamer Elternteil)".into()
        }
        Recorded::Other(name, _) => {
            let text = name.replace('_', " ");
            let mut chars = text.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or(text)
        }
    }
}

fn node_fill(person: i64) -> Color32 {
    let hue = (person as f32 * 0.618_034).rem_euclid(1.0);
    egui::ecolor::Hsva::new(hue, 0.55, 0.5, 1.0).into()
}

fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .filter_map(|word| word.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    if letters.is_empty() {
        "?".into()
    } else {
        letters
    }
}

/// Wunschradius eines Knotens mit Porträt in Punkten.
const PORTRAIT_RADIUS: f32 = 14.0;

fn node_radius(photos: usize, max: usize) -> f32 {
    0.16 + 0.2 * (photos as f32 / max.max(1) as f32).sqrt()
}

impl PhotoApp {
    /// Springt aus der Personenliste in den Graph-Reiter und wählt die Person.
    pub(super) fn open_relationship_graph(&mut self, person: i64) {
        self.relationship_graph.selected = Some(person);
        self.relationship_graph.focus_request = Some(person);
        self.face_mode = "graph".into();
        self.face_index = 0;
        self.face_advance = None;
        self.face_row_tops.clear();
        self.refresh_faces();
    }

    fn load_relationship_graph(&mut self) {
        self.relationship_graph.revision += 1;
        self.relationship_graph.stale = false;
        self.relationship_graph.attempted = true;
        let revision = self.relationship_graph.revision;
        let root = self.settings.collection.clone();
        self.launch(
            "Personengraph berechnen",
            false,
            "relationship-graph",
            move |control| {
                let collection = Collection::open(Path::new(&root), false)?;
                collection.cancel_with(&control);
                collection.db.execute_batch("BEGIN")?;
                Ok(Outcome::RelationshipGraph(
                    revision,
                    relationship_graph::network(&collection)?,
                ))
            },
        );
    }

    pub(super) fn relationship_graph_tab(&mut self, ui: &mut egui::Ui) {
        let view = &self.relationship_graph;
        if (view.stale || (view.data.is_none() && !view.attempted))
            && !self.settings.collection.is_empty()
        {
            self.load_relationship_graph();
        }
        let loading = self
            .tasks
            .iter()
            .any(|task| task.kind == "relationship-graph");
        let Some(network) = self.relationship_graph.data.take() else {
            if loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Personengraph wird berechnet …");
                });
            } else {
                ui.label("Der Graph konnte nicht geladen werden.");
                if ui.button("Erneut versuchen").clicked() {
                    self.relationship_graph.attempted = false;
                }
            }
            return;
        };
        let dark = ui.visuals().dark_mode;
        let mut photos = None;
        let mut refresh = false;
        egui::SidePanel::right("graph-side")
            .exact_width(300.0)
            .resizable(false)
            .show_inside(ui, |ui| {
                photos = side_panel(ui, &mut self.relationship_graph, &network, dark);
            });
        if network.nodes.is_empty() {
            ui.label("In dieser Sammlung gibt es noch keine Personen.");
            ui.label("Benenne Gesichter im Reiter „Benennen“, dann erscheinen sie hier.");
        } else {
            refresh = toolbar(ui, &mut self.relationship_graph, &network, loading);
            let source = PortraitSource {
                thumbnails: &mut self.thumbnails,
                root: &self.settings.collection,
                exiftool: &self.settings.exiftool,
            };
            canvas(ui, &mut self.relationship_graph, &network, dark, source);
        }
        self.relationship_graph.data = Some(network);
        if refresh {
            self.relationship_graph.stale = true;
        }
        if let Some(people) = photos {
            self.filters = Search {
                people_all: people,
                ..Search::default()
            };
            self.view = View::Collection;
            self.refresh_collection();
        }
    }
}

fn toolbar(
    ui: &mut egui::Ui,
    view: &mut RelationshipView,
    network: &Network,
    loading: bool,
) -> bool {
    let mut refresh = false;
    let max_shared = network.edges.iter().map(|e| e.photos).max().unwrap_or(0);
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut view.show_recorded, "Eingetragene Beziehungen")
            .on_hover_text("Eltern/Kind, Partner und Geschwister aus der Personenverwaltung");
        ui.checkbox(&mut view.show_photos, "Gemeinsame Fotos")
            .on_hover_text("Linienstärke nach Anzahl gemeinsamer bestätigter Fotos");
        ui.add_enabled_ui(view.show_photos && max_shared > 1, |ui| {
            ui.spacing_mut().slider_width = 110.0;
            ui.add(
                egui::Slider::new(&mut view.min_photos, 1..=max_shared.max(1))
                    .text("ab Fotos")
                    .integer(),
            )
            .on_hover_text(
                "Blendet schwächere Fotoverbindungen aus, damit der Graph lesbar bleibt",
            );
        });
        ui.separator();
        if ui.button("−").on_hover_text("Verkleinern").clicked() {
            view.scale = (view.scale / 1.3).max(MIN_SCALE);
        }
        if ui.button("+").on_hover_text("Vergrößern").clicked() {
            view.scale = (view.scale * 1.3).min(MAX_SCALE);
        }
        if ui.button("Einpassen").clicked() {
            view.fit = true;
        }
        if ui.button("Aktualisieren").clicked() {
            refresh = true;
        }
        if loading {
            ui.spinner();
        }
    });
    let mut info = format!(
        "{} Personen · {} Verbindungen. Mausrad zoomt, Ziehen verschiebt, Doppelklick zentriert.",
        network.nodes.len(),
        network.edges.len()
    );
    if network.hidden_people > 0 || network.hidden_edges > 0 {
        info.push_str(&format!(
            " Dargestellt werden höchstens {} Personen; {} Personen mit den wenigsten Fotos und {} schwächere Verbindungen fehlen.",
            relationship_graph::MAX_NODES,
            network.hidden_people,
            network.hidden_edges
        ));
    }
    ui.label(RichText::new(info).small().color(theme::MUTED));
    refresh
}

const MIN_SCALE: f32 = 1.5;
const MAX_SCALE: f32 = 250.0;

/// Zugriff auf Gesichtsausschnitte für die Knoten des Graphen.
struct PortraitSource<'a> {
    thumbnails: &'a mut Thumbnails,
    root: &'a str,
    exiftool: &'a str,
}

/// UV-Rechteck, das aus einer beliebigen Textur den mittleren quadratischen Teil wählt.
fn square_uv(size: egui::Vec2) -> egui::Rect {
    let (w, h) = (size.x.max(1.0), size.y.max(1.0));
    let (u, v) = if w > h { (h / w, 1.0) } else { (1.0, w / h) };
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(u, v))
}

/// Zeichnet die Textur als Kreis (Dreiecksfächer mit passenden UV-Koordinaten).
fn textured_circle(
    painter: &egui::Painter,
    centre: Pos2,
    radius: f32,
    texture: egui::TextureId,
    uv: egui::Rect,
    tint: Color32,
) {
    const SEGMENTS: u32 = 40;
    let mut mesh = egui::Mesh::with_texture(texture);
    mesh.vertices.push(egui::epaint::Vertex {
        pos: centre,
        uv: uv.center(),
        color: tint,
    });
    for i in 0..=SEGMENTS {
        let angle = std::f32::consts::TAU * i as f32 / SEGMENTS as f32;
        let (sin, cos) = angle.sin_cos();
        mesh.vertices.push(egui::epaint::Vertex {
            pos: centre + egui::vec2(cos, sin) * radius,
            uv: egui::pos2(
                uv.center().x + cos * uv.width() * 0.5,
                uv.center().y + sin * uv.height() * 0.5,
            ),
            color: tint,
        });
    }
    for i in 0..SEGMENTS {
        mesh.indices.extend([0, i + 1, i + 2]);
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn canvas(
    ui: &mut egui::Ui,
    view: &mut RelationshipView,
    network: &Network,
    dark: bool,
    portraits: PortraitSource<'_>,
) {
    let size = egui::vec2(
        ui.available_width(),
        (ui.available_height() - 4.0).max(200.0),
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let colours = palette(dark);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 10, ui.visuals().faint_bg_color);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Other,
            true,
            format!(
                "Personengraph mit {} Personen. Die Liste neben dem Graph wählt Personen mit der Tastatur.",
                network.nodes.len()
            ),
        )
    });

    let positions = &network.positions;
    if view.fit {
        view.fit = false;
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in positions {
            for axis in 0..2 {
                lo[axis] = lo[axis].min(p[axis]);
                hi[axis] = hi[axis].max(p[axis]);
            }
        }
        view.centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let (w, h) = ((hi[0] - lo[0]).max(0.5), (hi[1] - lo[1]).max(0.5));
        view.scale = ((rect.width() - 110.0) / w)
            .min((rect.height() - 110.0) / h)
            .clamp(MIN_SCALE, 70.0);
    }
    if let Some(index) = view
        .focus_request
        .take()
        .and_then(|person| network.index_of(person))
    {
        view.centre = positions[index];
        view.scale = view.scale.clamp(45.0, MAX_SCALE);
    }
    if let Some(pointer) = response.hover_pos() {
        let factor = ui.input(|i| (i.smooth_scroll_delta.y * 0.0025).exp() * i.zoom_delta());
        if (factor - 1.0).abs() > 1e-4 {
            let offset = pointer - rect.center();
            let world = Vec2::new(
                view.centre[0] + offset.x / view.scale,
                view.centre[1] + offset.y / view.scale,
            );
            view.scale = (view.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
            view.centre = [
                world.x - offset.x / view.scale,
                world.y - offset.y / view.scale,
            ];
        }
    }
    if response.dragged() {
        let delta = response.drag_delta();
        view.centre[0] -= delta.x / view.scale;
        view.centre[1] -= delta.y / view.scale;
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let scale = view.scale;
    let centre = view.centre;
    let to_screen = |p: [f32; 2]| {
        Pos2::new(
            rect.center().x + (p[0] - centre[0]) * scale,
            rect.center().y + (p[1] - centre[1]) * scale,
        )
    };
    let max_photos = network.nodes.iter().map(|n| n.photos).max().unwrap_or(1);
    let radius_px: Vec<f32> = network
        .nodes
        .iter()
        .map(|n| {
            let base = (node_radius(n.photos, max_photos) * scale).max(3.0);
            // Ein Porträt braucht Platz, um erkennbar zu sein; der Mindestabstand der
            // Knoten (MIN_DISTANCE) bleibt gewahrt, damit sich nichts überlappt.
            if n.portrait.is_some() {
                base.max(PORTRAIT_RADIUS.min(crate::core::graph_layout::MIN_DISTANCE * 0.5 * scale))
            } else {
                base
            }
        })
        .collect();

    // Sichtbare Kanten gemäß Ebenen.
    let selected = view.selected.and_then(|person| network.index_of(person));
    let visible: Vec<&Edge> = network
        .edges
        .iter()
        .filter(|edge| {
            (view.show_recorded && !edge.recorded.is_empty())
                || (view.show_photos && edge.photos >= view.min_photos)
        })
        .collect();
    let mut related: HashSet<usize> = HashSet::new();
    if let Some(s) = selected {
        related.insert(s);
        for edge in &visible {
            if edge.a == s {
                related.insert(edge.b);
            } else if edge.b == s {
                related.insert(edge.a);
            }
        }
    }
    let dimmed = |edge: &Edge| selected.is_some_and(|s| edge.a != s && edge.b != s);
    let max_shared = visible.iter().map(|e| e.photos).max().unwrap_or(1).max(2) as f32;
    let crowded = visible.len() > network.nodes.len() * 2;

    // Fotolinien unten, eingetragene Beziehungen darüber.
    let clip = rect.expand(40.0);
    for edge in visible
        .iter()
        .filter(|e| view.show_photos && e.photos >= view.min_photos)
    {
        let (a, b) = (to_screen(positions[edge.a]), to_screen(positions[edge.b]));
        if !clip.intersects(Rect::from_two_pos(a, b)) {
            continue;
        }
        let strong = selected.is_some() && !dimmed(edge);
        let width = 0.8 + 3.4 * (edge.photos.max(1) as f32).ln() / max_shared.ln();
        let alpha = if dimmed(edge) {
            0.10
        } else if strong {
            0.95
        } else if crowded {
            0.35
        } else {
            0.6
        };
        painter.line_segment(
            [a, b],
            Stroke::new(
                if strong { width + 0.8 } else { width },
                colours.photo.gamma_multiply(alpha),
            ),
        );
    }
    for edge in visible
        .iter()
        .filter(|e| view.show_recorded && !e.recorded.is_empty())
    {
        let (a, b) = (to_screen(positions[edge.a]), to_screen(positions[edge.b]));
        if !clip.intersects(Rect::from_two_pos(a, b)) {
            continue;
        }
        let alpha = if dimmed(edge) { 0.15 } else { 1.0 };
        for (slot, kind) in edge.recorded.iter().enumerate() {
            // Mehrere Typen auf einer Kante laufen leicht versetzt nebeneinander.
            let offset = line_offset(a, b, slot as f32 * 4.0);
            let (a, b) = (a + offset, b + offset);
            let colour = match kind {
                Recorded::Parent | Recorded::Child => colours.parent,
                Recorded::Partner => colours.partner,
                Recorded::Sibling | Recorded::DerivedSibling { .. } => colours.sibling,
                Recorded::Other(..) => colours.other,
            }
            .gamma_multiply(alpha);
            let stroke = Stroke::new(2.2_f32, colour);
            match kind {
                Recorded::Partner => {
                    painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 7.0, 4.0))
                }
                Recorded::Sibling | Recorded::DerivedSibling { .. } => {
                    painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 2.0, 4.0))
                }
                Recorded::Other(..) => {
                    painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 12.0, 3.0))
                }
                Recorded::Parent => {
                    painter.line_segment([a, b], stroke);
                    arrow_head(&painter, a, b, radius_px[edge.b], colour);
                }
                Recorded::Child => {
                    painter.line_segment([a, b], stroke);
                    arrow_head(&painter, b, a, radius_px[edge.a], colour);
                }
            }
        }
    }

    // Knoten: erst abgeblendete, dann hervorgehobene.
    let hover = response.hover_pos().and_then(|pointer| {
        (0..network.nodes.len())
            .filter_map(|i| {
                let distance = to_screen(positions[i]).distance(pointer);
                (distance <= radius_px[i] + 4.0).then_some((i, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    });
    let node_clip = rect.expand(30.0);
    let is_dim = |i: usize| selected.is_some() && !related.contains(&i);
    let few = network.nodes.len() <= 40;
    for pass in [true, false] {
        for (i, node) in network.nodes.iter().enumerate() {
            if is_dim(i) != pass {
                continue;
            }
            let at = to_screen(positions[i]);
            if !node_clip.contains(at) {
                continue;
            }
            let r = radius_px[i];
            let alpha = if pass { 0.3 } else { 1.0 };
            let fill = node_fill(node.person).gamma_multiply(alpha);
            let ring = if selected == Some(i) {
                Stroke::new(3.5_f32, colours.ring)
            } else if related.contains(&i) && selected.is_some() || hover == Some(i) {
                Stroke::new(2.5_f32, colours.ring)
            } else {
                Stroke::new(1.0_f32, ui.visuals().window_fill)
            };
            painter.circle(at, r, fill, ring);
            let portrait = node.portrait.as_ref().filter(|_| r >= 8.0).and_then(|p| {
                portraits.thumbnails.texture(
                    ui.ctx(),
                    PreviewRequest {
                        root: Some(PathBuf::from(portraits.root)),
                        path: Path::new(portraits.root).join(&p.source),
                        orientation: 1,
                        crop: Some(p.bbox),
                        edge: 96,
                        generation: 0,
                    },
                    portraits.exiftool,
                )
            });
            if let Some((texture, size)) = portrait {
                textured_circle(
                    &painter,
                    at,
                    r - 1.0,
                    texture,
                    square_uv(size),
                    Color32::WHITE.gamma_multiply(alpha),
                );
                painter.circle_stroke(at, r, ring);
            } else if r >= 9.0 {
                painter.text(
                    at,
                    Align2::CENTER_CENTER,
                    initials(&node.name),
                    FontId::proportional((r * 0.85).clamp(9.0, 17.0)),
                    Color32::WHITE.gamma_multiply(alpha.max(0.5)),
                );
            }
        }
    }

    // Beschriftungen mit Vorrang und Kollisionsprüfung.
    let mut order: Vec<usize> = Vec::new();
    order.extend(selected);
    order.extend(hover.filter(|h| Some(*h) != selected));
    order.extend(
        related
            .iter()
            .copied()
            .filter(|i| Some(*i) != selected && Some(*i) != hover),
    );
    let priority = order.len();
    let fixed =
        usize::from(selected.is_some()) + usize::from(hover.is_some_and(|h| Some(h) != selected));
    let chosen: HashSet<usize> = order.iter().copied().collect();
    order.extend(
        (0..network.nodes.len())
            .filter(|i| !chosen.contains(i) && (few || (radius_px[*i] >= 8.0)) && !is_dim(*i)),
    );
    let mut placed: Vec<Rect> = Vec::new();
    for (rank, &i) in order.iter().enumerate() {
        let at = to_screen(positions[i]);
        if !node_clip.contains(at) {
            continue;
        }
        let galley = painter.layout_no_wrap(
            truncate(&network.nodes[i].name),
            FontId::proportional(if rank < priority { 13.5 } else { 12.0 }),
            ui.visuals().text_color(),
        );
        let label = Rect::from_min_size(
            at + egui::vec2(-galley.size().x / 2.0, radius_px[i] + 3.0),
            galley.size(),
        );
        if rank >= fixed
            && placed
                .iter()
                .any(|other| other.expand(2.0).intersects(label))
        {
            continue;
        }
        placed.push(label);
        painter.rect_filled(
            label.expand2(egui::vec2(3.0, 1.0)),
            3,
            ui.visuals().panel_fill.gamma_multiply(0.85),
        );
        painter.galley(label.min, galley, ui.visuals().text_color());
    }

    if hover.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        view.selected = hover.map(|i| network.nodes[i].person);
    }
    if response.double_clicked()
        && let Some(i) = hover
    {
        view.focus_request = Some(network.nodes[i].person);
    }
    if let Some(i) = hover {
        let node = &network.nodes[i];
        response.on_hover_text(format!(
            "{} · #{} · {} Fotos\nKlick wählt aus, Doppelklick zentriert",
            node.name, node.person, node.photos
        ));
    }
}

fn truncate(name: &str) -> String {
    if name.chars().count() > 22 {
        format!("{}…", name.chars().take(21).collect::<String>())
    } else {
        name.to_owned()
    }
}

fn line_offset(a: Pos2, b: Pos2, distance: f32) -> Vec2 {
    let d = b - a;
    let len = d.length().max(1e-3);
    egui::vec2(-d.y / len, d.x / len) * distance
}

/// Pfeilspitze an `to`, zurückgesetzt um den Knotenradius.
fn arrow_head(painter: &egui::Painter, from: Pos2, to: Pos2, radius: f32, colour: Color32) {
    let d = to - from;
    let len = d.length();
    if len < radius * 2.0 + 10.0 {
        return;
    }
    let dir = d / len;
    let tip = to - dir * (radius + 1.0);
    let side = egui::vec2(-dir.y, dir.x);
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            tip - dir * 9.0 + side * 4.5,
            tip - dir * 9.0 - side * 4.5,
        ],
        colour,
        Stroke::NONE,
    ));
}

fn side_panel(
    ui: &mut egui::Ui,
    view: &mut RelationshipView,
    network: &Network,
    dark: bool,
) -> Option<Vec<i64>> {
    let colours = palette(dark);
    let mut photos = None;
    ui.add_space(4.0);
    ui.label(RichText::new("Person finden").strong());
    ui.add(
        egui::TextEdit::singleline(&mut view.filter)
            .hint_text("Name eingeben …")
            .desired_width(f32::INFINITY),
    );
    let needle = files::fold(view.filter.trim());
    let mut matches: Vec<usize> = (0..network.nodes.len())
        .filter(|i| view.names[*i].contains(&needle))
        .collect();
    matches.sort_by(|a, b| view.names[*a].cmp(&view.names[*b]));
    egui::ScrollArea::vertical()
        .id_salt("graph-people")
        .max_height(110.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if matches.is_empty() {
                ui.label("Keine passende Person");
            }
            for &i in matches.iter().take(300) {
                let node = &network.nodes[i];
                let chosen = view.selected == Some(node.person);
                if ui
                    .selectable_label(chosen, format!("{} · {} Fotos", node.name, node.photos))
                    .clicked()
                {
                    view.selected = Some(node.person);
                    view.focus_request = Some(node.person);
                }
            }
            if matches.len() > 300 {
                ui.label(RichText::new("Weitere Treffer: Suchbegriff eingrenzen").small());
            }
        });
    egui::TopBottomPanel::bottom("graph-legend").show_inside(ui, |ui| {
        legend(ui, network, &colours);
    });
    ui.separator();
    let selected = view.selected.and_then(|person| network.index_of(person));
    egui::ScrollArea::vertical()
        .id_salt("graph-detail")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let Some(s) = selected else {
                ui.label("Wähle eine Person im Graph oder in der Liste, um Verbindungen zu sehen.");
                return;
            };
            let node = &network.nodes[s];
            ui.heading(&node.name);
            ui.label(format!("#{} · {} Fotos", node.person, node.photos));
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(node.photos > 0, egui::Button::new("Fotos anzeigen"))
                    .clicked()
                {
                    photos = Some(vec![node.person]);
                }
                if ui.button("Zentrieren").clicked() {
                    view.focus_request = Some(node.person);
                }
                if ui.button("Auswahl aufheben").clicked() {
                    view.selected = None;
                }
            });
            let mut incident: Vec<&Edge> = network
                .edges
                .iter()
                .filter(|e| e.a == s || e.b == s)
                .collect();
            incident
                .sort_by_key(|e| (e.recorded.is_empty(), std::cmp::Reverse(e.photos), e.a, e.b));
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Verbindungen · {}", incident.len())).strong());
            if incident.is_empty() {
                ui.label("Diese Person ist noch mit niemandem verbunden.");
            }
            for edge in incident {
                let (other, mine) = if edge.a == s {
                    (edge.b, edge.recorded.clone())
                } else {
                    (edge.a, edge.recorded.iter().map(|k| k.reversed()).collect())
                };
                let other_node = &network.nodes[other];
                ui.horizontal_wrapped(|ui| {
                    if ui.link(&other_node.name).clicked() {
                        view.selected = Some(other_node.person);
                        view.focus_request = Some(other_node.person);
                    }
                    let mut text: Vec<String> = mine.iter().map(recorded_label).collect();
                    if edge.photos > 0 {
                        text.push(format!("{} gemeinsame Fotos", edge.photos));
                    }
                    ui.label(RichText::new(text.join(" · ")).small());
                    if edge.photos > 0 && ui.small_button("Fotos").clicked() {
                        photos = Some(vec![node.person, other_node.person]);
                    }
                });
            }
        });
    photos
}

fn legend(ui: &mut egui::Ui, network: &Network, colours: &Palette) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.spacing_mut().interact_size.y = 16.0;
    {
        ui.label(RichText::new("Legende · Kreisgröße = Zahl der Fotos").strong());
        legend_row(
            ui,
            colours.photo,
            "Gemeinsame Fotos (dicker = mehr)",
            Line::Thick,
        );
        legend_row(
            ui,
            colours.parent,
            "Elternteil, Pfeil zum Kind",
            Line::Arrow,
        );
        legend_row(ui, colours.partner, "Partner", Line::Dashed);
        legend_row(
            ui,
            colours.sibling,
            "Geschwister (auch aus gemeinsamen Eltern)",
            Line::Dotted,
        );
        // Weitere Beziehungstypen aus der Datenbank erscheinen neutral und mit eigenem Namen.
        let mut others: Vec<String> = network
            .edges
            .iter()
            .flat_map(|e| &e.recorded)
            .filter(|k| matches!(k, Recorded::Other(..)))
            .map(recorded_label)
            .collect();
        others.sort();
        others.dedup();
        for name in others.iter().rev() {
            legend_row(ui, colours.other, name, Line::Long);
        }
    }
}

#[derive(Clone, Copy)]
enum Line {
    Thick,
    Dashed,
    Dotted,
    Long,
    Arrow,
}

fn legend_row(ui: &mut egui::Ui, colour: Color32, text: &str, line: Line) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(34.0, 14.0), Sense::hover());
        let painter = ui.painter();
        let (a, b) = (rect.left_center(), rect.right_center());
        match line {
            Line::Thick => {
                painter.line_segment([a, b], Stroke::new(3.5_f32, colour));
            }
            Line::Dashed => painter.extend(egui::Shape::dashed_line(
                &[a, b],
                Stroke::new(2.2_f32, colour),
                7.0,
                4.0,
            )),
            Line::Dotted => painter.extend(egui::Shape::dashed_line(
                &[a, b],
                Stroke::new(2.2_f32, colour),
                2.0,
                4.0,
            )),
            Line::Long => painter.extend(egui::Shape::dashed_line(
                &[a, b],
                Stroke::new(2.2_f32, colour),
                12.0,
                3.0,
            )),
            Line::Arrow => {
                painter.line_segment([a, b], Stroke::new(2.2_f32, colour));
                arrow_head(painter, a, b + egui::vec2(6.0, 0.0), 0.0, colour);
            }
        }
        ui.label(RichText::new(text).small());
    });
}

#[cfg(test)]
mod portrait_tests {
    use super::*;
    #[test]
    fn portraits_are_cropped_to_the_central_square() {
        let wide = square_uv(egui::vec2(200.0, 100.0));
        assert!((wide.width() - 0.5).abs() < 1e-6 && (wide.height() - 1.0).abs() < 1e-6);
        let tall = square_uv(egui::vec2(100.0, 200.0));
        assert!((tall.width() - 1.0).abs() < 1e-6 && (tall.height() - 0.5).abs() < 1e-6);
        let square = square_uv(egui::vec2(96.0, 96.0));
        assert_eq!(
            square,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        );
        assert_eq!(wide.center(), egui::pos2(0.5, 0.5));
    }
}
