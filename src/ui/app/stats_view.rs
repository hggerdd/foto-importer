//! Ansicht „Statistik“ unter „Personen“: je Person eine Violine der Personenscores
//! (`core::person_stats`). Nur Anzeige; die Berechnung läuft auf einem Worker, das Ergebnis
//! liegt mit Fingerabdruck im Speicher und im lokalen Cache.
use super::*;
use crate::core::person_stats::{self, Distribution, Mode, PersonStat, Stats};
use crate::core::progress::{decimal, group};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};

/// Breite einer Personenspalte.
const COLUMN: f32 = 68.0;
/// Breite der Achsenbeschriftung links.
const GUTTER: f32 = 52.0;
/// Höhe der Beschriftung unter den Violinen (Name, Anzahl, Median).
const LABELS: f32 = 54.0;

#[derive(Default)]
pub(super) struct StatsView {
    pub(super) revision: u64,
    pub(super) data: Option<Stats>,
    /// Fingerabdruck, zu dem `data` gehört (`person_stats::signature`).
    signature: Option<String>,
    /// `data` kommt aus dem lokalen Cache und wird noch geprüft.
    pub(super) from_cache: bool,
    attempted: bool,
    /// Stand der Gesichterseite (`face_page_serial`), zu dem zuletzt geprüft wurde. Jede
    /// Entscheidung liest die Seite neu; danach prüft die Statistik ihren Fingerabdruck.
    checked: Option<u64>,
    pub(super) mode: Mode,
    /// Kennzahlen als Tabelle statt Diagramm.
    pub(super) table: bool,
}

impl StatsView {
    /// Antwort des Prüf- und Rechenauftrags: neue Statistik oder die Bestätigung, dass die
    /// gezeigte noch gilt.
    pub(super) fn refreshed(&mut self, signature: String, stats: Option<Stats>) {
        if let Some(stats) = stats {
            self.data = Some(stats);
        }
        self.signature = Some(signature);
        self.from_cache = false;
    }
    pub(super) fn clear(&mut self) {
        *self = Self {
            revision: self.revision + 1,
            mode: self.mode,
            table: self.table,
            ..Self::default()
        };
    }
}

/// Text der Kennzahlen einer Person in der Sicht (Tooltip und Tabelle).
pub(super) fn describe(stat: &PersonStat, mode: Mode) -> String {
    let median = |d: &Distribution| d.summary.map_or("–".into(), |s| decimal(s.median, 2));
    let quartiles = |d: &Distribution| {
        d.summary.map_or(String::new(), |s| {
            format!(" (Quartile {} bis {})", decimal(s.q1, 2), decimal(s.q3, 2))
        })
    };
    let mut lines = vec![stat.name.clone()];
    match mode {
        Mode::Rated => {
            lines.push(format!(
                "{} bewertete Gesichter, Median {}{}",
                group(stat.rated.n),
                median(&stat.rated),
                quartiles(&stat.rated)
            ));
            if let Some(t) = stat.threshold {
                lines.push(format!(
                    "über t = {} (aus „Nur bestätigte“): {} bewertete",
                    decimal(t.t, 2),
                    group(t.rated_above)
                ));
            }
        }
        Mode::Confirmed => {
            lines.push(format!(
                "{} bestätigt, Median {}{}",
                group(stat.confirmed.n),
                median(&stat.confirmed),
                quartiles(&stat.confirmed)
            ));
            lines.push(format!(
                "{} abgelehnt, Median {}{}",
                group(stat.rejected.n),
                median(&stat.rejected),
                quartiles(&stat.rejected)
            ));
            match stat.threshold {
                Some(t) => lines.push(format!(
                    "über t = {}: 0 Ablehnungen, {} von {} bestätigten",
                    decimal(t.t, 2),
                    group(t.confirmed_above),
                    group(stat.confirmed.n)
                )),
                None => lines.push("Keine Ablehnungen, daher keine Schwelle t".into()),
            }
        }
    }
    lines.join("\n")
}

impl PhotoApp {
    /// Zeigt sofort die zuletzt gespeicherte Statistik (lokaler Cache) und prüft auf einem
    /// Worker per Fingerabdruck, ob sie noch gilt; nur bei Änderungen wird neu gerechnet.
    fn load_person_stats(&mut self) {
        let view = &mut self.person_stats;
        view.revision += 1;
        view.attempted = true;
        view.checked = Some(self.face_page_serial);
        if view.data.is_none()
            && let Some(cached) = person_stats::cached(&self.cache_dir, &self.settings.collection)
        {
            view.data = Some(cached.stats);
            view.signature = Some(cached.signature);
            view.from_cache = true;
        }
        let revision = view.revision;
        let known = view.signature.clone();
        let (root, cache) = (self.settings.collection.clone(), self.cache_dir.clone());
        self.launch(
            "Personenstatistik berechnen",
            false,
            JobKind::PersonStats,
            move |control| {
                let collection = Collection::open(Path::new(&root), false)?;
                collection.cancel_with(&control);
                let (signature, stats) =
                    person_stats::refresh(&collection, known.as_deref(), &control)?;
                if let Some(stats) = &stats {
                    let cached = person_stats::Cached {
                        signature: signature.clone(),
                        stats: stats.clone(),
                    };
                    person_stats::store(&cache, &root, &cached);
                }
                Ok(Outcome::PersonStats(revision, signature, stats))
            },
        );
    }

    pub(super) fn person_stats_tab(&mut self, ui: &mut egui::Ui) {
        let loading = self
            .tasks
            .iter()
            .any(|task| task.kind == JobKind::PersonStats);
        let view = &self.person_stats;
        if !loading
            && !self.settings.collection.is_empty()
            && ((view.data.is_none() && !view.attempted)
                || (view.attempted && view.checked != Some(self.face_page_serial)))
        {
            self.load_person_stats();
        }
        let mut refresh = false;
        let view = &mut self.person_stats;
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                (Mode::Rated, "Alle bewerteten Gesichter"),
                (Mode::Confirmed, "Nur bestätigte"),
            ] {
                if ui.selectable_label(view.mode == mode, label).clicked() {
                    view.mode = mode;
                }
            }
            ui.separator();
            ui.checkbox(&mut view.table, "Als Tabelle");
            if ui
                .add_enabled(!loading, egui::Button::new("Aktualisieren"))
                .on_hover_text("Rechnet die Statistik neu, auch ohne geänderte Daten")
                .clicked()
            {
                refresh = true;
            }
            if loading {
                ui.spinner();
                ui.label(
                    RichText::new(if view.data.is_some() {
                        "wird aktualisiert …"
                    } else {
                        "wird berechnet …"
                    })
                    .weak(),
                );
            }
        });
        ui.label(
            RichText::new(match view.mode {
                Mode::Rated => format!(
                    "Offene Gesichter, die das Modell dieser Person als beste zulässige Person zuordnet (ab {}). Wert: Personenscore, das Mittel der bis zu drei besten Kosinuswerte gegen ihre Referenzen.",
                    decimal(view.data.as_ref().map_or(config::PERSON_CANDIDATE_MIN, |s| s.minimum), 2)
                ),
                Mode::Confirmed => "Grün: bestätigte Gesichter, bewertet ohne sich selbst und ohne Referenzen vom selben Aufnahmetag. Rot: Gesichter mit Ablehnung „nicht X“, mit ihrem Wert für X. t ist der höchste abgelehnte Wert.".into(),
            })
            .weak(),
        );
        ui.label(
            RichText::new("Nur Anzeige: Hier wird nichts übernommen und keine Zuordnung geändert.")
                .weak(),
        );
        if refresh {
            self.person_stats.signature = None;
            self.load_person_stats();
        }
        let Some(stats) = self.person_stats.data.take() else {
            if !loading {
                ui.label("Die Statistik konnte nicht berechnet werden.");
                if ui.button("Erneut versuchen").clicked() {
                    self.person_stats.attempted = false;
                }
            }
            return;
        };
        let mode = self.person_stats.mode;
        let order = stats.order(mode);
        if order.is_empty() {
            ui.label(match mode {
                Mode::Rated => "Noch keine offenen Gesichter mit einer wahrscheinlichen Person.",
                Mode::Confirmed => {
                    "Noch keine bestätigten Gesichter mit Referenzen. Benenne Gesichter unter „Gesichter“."
                }
            });
        } else if self.person_stats.table {
            table(ui, &stats, &order, mode);
        } else {
            plot(ui, &stats, &order, mode);
        }
        self.person_stats.data = Some(stats);
    }
}

/// Kennzahlen als Tabelle (Textalternative zum Diagramm).
fn table(ui: &mut egui::Ui, stats: &Stats, order: &[usize], mode: Mode) {
    let median = |d: &Distribution| d.summary.map_or("–".into(), |s| decimal(s.median, 2));
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("person-stats-table")
                .striped(true)
                .num_columns(6)
                .show(ui, |ui| {
                    let heads: &[&str] = match mode {
                        Mode::Rated => &["Person", "bewertet", "Median", "", "", ""],
                        Mode::Confirmed => &[
                            "Person",
                            "bestätigt",
                            "Median",
                            "abgelehnt",
                            "Median",
                            "über t",
                        ],
                    };
                    for head in heads {
                        ui.label(RichText::new(*head).strong());
                    }
                    ui.end_row();
                    for index in order {
                        let stat = &stats.people[*index];
                        ui.label(&stat.name).on_hover_text(describe(stat, mode));
                        match mode {
                            Mode::Rated => {
                                ui.label(group(stat.rated.n));
                                ui.label(median(&stat.rated));
                                ui.label("");
                                ui.label("");
                                ui.label("");
                            }
                            Mode::Confirmed => {
                                ui.label(group(stat.confirmed.n));
                                ui.label(median(&stat.confirmed));
                                ui.label(group(stat.rejected.n));
                                ui.label(median(&stat.rejected));
                                ui.label(stat.threshold.map_or("–".into(), |t| {
                                    format!(
                                        "t = {}: {} von {}",
                                        decimal(t.t, 2),
                                        group(t.confirmed_above),
                                        group(stat.confirmed.n)
                                    )
                                }));
                            }
                        }
                        ui.end_row();
                    }
                });
        });
}

/// Abbildung eines Werts auf die Höhe im Diagramm.
struct Axis {
    /// Wertebereich des Modells (`Stats::axis`).
    range: (f32, f32),
    top: f32,
    bottom: f32,
}
impl Axis {
    fn y(&self, value: f32) -> f32 {
        let (low, high) = self.range;
        let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
        self.bottom - t * (self.bottom - self.top)
    }
}

/// Violinen nebeneinander, Werteachse senkrecht, waagrecht scrollbar.
fn plot(ui: &mut egui::Ui, stats: &Stats, order: &[usize], mode: Mode) {
    let dark = ui.visuals().dark_mode;
    let text = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let grid_stroke = Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color);
    let height = (ui.available_height() - 8.0).max(240.0);
    let plot_height = height - LABELS - 12.0;
    let top = ui.cursor().top() + 8.0;
    let axis = Axis {
        range: stats.axis,
        top,
        bottom: top + plot_height,
    };
    let lines = [
        (
            stats.review,
            format!("Vorschlag {}", decimal(stats.review, 2)),
        ),
        (stats.sure, format!("sicher {}", decimal(stats.sure, 2))),
    ];
    ui.horizontal_top(|ui| {
        // Achse links, außerhalb des Bildlaufs.
        let (rect, _) = ui.allocate_exact_size(Vec2::new(GUTTER, height), Sense::hover());
        let painter = ui.painter_at(rect);
        let (low, high) = stats.axis;
        let steps = ((high - low) * 10.0).round() as usize;
        for i in 0..=steps {
            let value = low + i as f32 * 0.1;
            painter.text(
                Pos2::new(rect.right() - 6.0, axis.y(value)),
                Align2::RIGHT_CENTER,
                decimal(value, 1),
                FontId::proportional(theme::MIN_TEXT),
                weak,
            );
        }
        egui::ScrollArea::horizontal()
            .id_salt("person-stats-plot")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let width = order.len() as f32 * COLUMN;
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(width.max(COLUMN), height), Sense::hover());
                let painter = ui.painter_at(rect);
                let visible = ui.clip_rect();
                // Gitter und Schwellen.
                let (low, high) = stats.axis;
                let steps = ((high - low) * 10.0).round() as usize;
                for i in 0..=steps {
                    let y = axis.y(low + i as f32 * 0.1);
                    painter.hline(rect.x_range(), y, grid_stroke);
                }
                let threshold_color = theme::accent(dark);
                for (value, label) in &lines {
                    let y = axis.y(*value);
                    painter.add(Shape::dashed_line(
                        &[Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                        Stroke::new(1.5_f32, threshold_color),
                        6.0,
                        4.0,
                    ));
                    painter.text(
                        Pos2::new(visible.left().max(rect.left()) + 4.0, y - 2.0),
                        Align2::LEFT_BOTTOM,
                        label,
                        FontId::proportional(theme::MIN_TEXT),
                        threshold_color,
                    );
                }
                let mut hovered = None;
                for (column, index) in order.iter().enumerate() {
                    let left = rect.left() + column as f32 * COLUMN;
                    let cell =
                        Rect::from_min_size(Pos2::new(left, rect.top()), Vec2::new(COLUMN, height));
                    if !cell.intersects(visible) {
                        continue;
                    }
                    let stat = &stats.people[*index];
                    if response.hover_pos().is_some_and(|pos| cell.contains(pos)) {
                        hovered = Some(*index);
                        painter.rect_filled(cell, 0.0, ui.visuals().faint_bg_color);
                    }
                    let centre = left + COLUMN / 2.0;
                    let half = COLUMN * 0.42;
                    match mode {
                        Mode::Rated => {
                            let color = theme::accent(dark);
                            violin(
                                &painter,
                                &axis,
                                centre,
                                half,
                                &stat.rated,
                                color,
                                Side::Both,
                            );
                        }
                        Mode::Confirmed => {
                            violin(
                                &painter,
                                &axis,
                                centre,
                                half,
                                &stat.confirmed,
                                theme::positive(dark),
                                Side::Left,
                            );
                            violin(
                                &painter,
                                &axis,
                                centre,
                                half,
                                &stat.rejected,
                                theme::negative(dark),
                                Side::Right,
                            );
                            if let Some(t) = stat.threshold {
                                let y = axis.y(t.t);
                                painter.add(Shape::dashed_line(
                                    &[Pos2::new(centre - half, y), Pos2::new(centre + half, y)],
                                    Stroke::new(1.5_f32, text),
                                    3.0,
                                    2.0,
                                ));
                                painter.text(
                                    Pos2::new(centre + half, y),
                                    Align2::RIGHT_BOTTOM,
                                    "t",
                                    FontId::proportional(theme::MIN_TEXT),
                                    text,
                                );
                            }
                        }
                    }
                    // Beschriftung: Name, Anzahl, Median.
                    let base = axis.bottom + 6.0;
                    painter.text(
                        Pos2::new(centre, base),
                        Align2::CENTER_TOP,
                        short_name(&stat.name),
                        FontId::proportional(theme::MIN_TEXT),
                        text,
                    );
                    let small = FontId::proportional(theme::MIN_TEXT);
                    match mode {
                        Mode::Rated => {
                            painter.text(
                                Pos2::new(centre, base + 16.0),
                                Align2::CENTER_TOP,
                                format!("n={}", group(stat.rated.n)),
                                small.clone(),
                                weak,
                            );
                        }
                        Mode::Confirmed => {
                            painter.text(
                                Pos2::new(centre - 2.0, base + 16.0),
                                Align2::RIGHT_TOP,
                                group(stat.confirmed.n),
                                small.clone(),
                                theme::positive(dark),
                            );
                            painter.text(
                                Pos2::new(centre + 2.0, base + 16.0),
                                Align2::LEFT_TOP,
                                group(stat.rejected.n),
                                small.clone(),
                                theme::negative(dark),
                            );
                        }
                    }
                    let shown = match mode {
                        Mode::Rated => &stat.rated,
                        Mode::Confirmed => &stat.confirmed,
                    };
                    if let Some(summary) = shown.summary {
                        painter.text(
                            Pos2::new(centre, base + 32.0),
                            Align2::CENTER_TOP,
                            format!("Md {}", decimal(summary.median, 2)),
                            small,
                            weak,
                        );
                    }
                }
                if let Some(index) = hovered {
                    response.on_hover_text(describe(&stats.people[index], mode));
                }
            });
    });
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Both,
    Left,
    Right,
}

/// Eine Violine (oder halbe Violine) mit Median-Strich; unter
/// `config::PERSON_STATS_MIN_VIOLIN` Werten Punkte.
fn violin(
    painter: &egui::Painter,
    axis: &Axis,
    centre: f32,
    half: f32,
    distribution: &Distribution,
    color: Color32,
    side: Side,
) {
    let (left, right) = match side {
        Side::Both => (true, true),
        Side::Left => (true, false),
        Side::Right => (false, true),
    };
    if !distribution.points.is_empty() {
        let x = match side {
            Side::Both => centre,
            Side::Left => centre - half * 0.4,
            Side::Right => centre + half * 0.4,
        };
        for value in &distribution.points {
            painter.circle_filled(Pos2::new(x, axis.y(*value)), 3.5, color);
        }
        return;
    }
    if distribution.density.is_empty() {
        return;
    }
    let grid = person_stats::grid_on(axis.range);
    let fill = color.gamma_multiply(0.45);
    let mut mesh = egui::Mesh::default();
    let mut outline_left = Vec::new();
    let mut outline_right = Vec::new();
    for (i, (value, density)) in grid.iter().zip(&distribution.density).enumerate() {
        if *density <= 0.0 {
            continue;
        }
        let y = axis.y(*value);
        let width = density * half;
        outline_left.push(Pos2::new(centre - if left { width } else { 0.0 }, y));
        outline_right.push(Pos2::new(centre + if right { width } else { 0.0 }, y));
        if i + 1 < grid.len() && distribution.density[i + 1] > 0.0 {
            let next_y = axis.y(grid[i + 1]);
            let next = distribution.density[i + 1] * half;
            let quad = [
                Pos2::new(centre - if left { width } else { 0.0 }, y),
                Pos2::new(centre + if right { width } else { 0.0 }, y),
                Pos2::new(centre + if right { next } else { 0.0 }, next_y),
                Pos2::new(centre - if left { next } else { 0.0 }, next_y),
            ];
            let base = mesh.vertices.len() as u32;
            for point in quad {
                mesh.colored_vertex(point, fill);
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
    }
    painter.add(Shape::mesh(mesh));
    let stroke = Stroke::new(1.2_f32, color);
    if left {
        painter.add(Shape::line(outline_left, stroke));
    }
    if right {
        painter.add(Shape::line(outline_right, stroke));
    }
    if let Some(summary) = distribution.summary {
        let y = axis.y(summary.median);
        let (from, to) = match side {
            Side::Both => (centre - half * 0.5, centre + half * 0.5),
            Side::Left => (centre - half * 0.5, centre),
            Side::Right => (centre, centre + half * 0.5),
        };
        painter.line_segment(
            [Pos2::new(from, y), Pos2::new(to, y)],
            Stroke::new(2.5_f32, color),
        );
    }
}

/// Name gekürzt auf die Spaltenbreite.
fn short_name(name: &str) -> String {
    const MAX: usize = 9;
    if name.chars().count() <= MAX {
        name.to_string()
    } else {
        let mut short: String = name.chars().take(MAX - 1).collect();
        short.push('…');
        short
    }
}
