use egui::{Color32, RichText};
pub const ACCENT: Color32 = Color32::from_rgb(67, 211, 190);
pub const WARM: Color32 = Color32::from_rgb(239, 188, 111);
/// Gedämpfter Text: Kontrast mindestens 6,5:1 auf Panel- und Kartenfläche.
pub const MUTED: Color32 = Color32::from_rgb(156, 168, 182);
/// Kleinste Schriftgröße der Oberfläche.
pub const MIN_TEXT: f32 = 12.0;
pub fn apply(ctx: &egui::Context, dark: bool) {
    let mut style = egui::Style {
        visuals: if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        },
        ..Default::default()
    };
    if dark {
        style.visuals.panel_fill = Color32::from_rgb(19, 24, 31);
        style.visuals.window_fill = Color32::from_rgb(25, 32, 41);
        style.visuals.extreme_bg_color = Color32::from_rgb(14, 19, 25);
        style.visuals.faint_bg_color = Color32::from_rgb(29, 38, 48);
        style.visuals.override_text_color = Some(Color32::from_rgb(222, 230, 239));
        style.visuals.weak_text_color = Some(MUTED);
    }
    if dark {
        // Sekundäre Schaltflächen: ruhige Fläche mit feiner Kante, Hover mit Akzent.
        let border = egui::Stroke::new(1.0_f32, Color32::from_rgb(55, 69, 85));
        let widgets = &mut style.visuals.widgets;
        widgets.inactive.weak_bg_fill = Color32::from_rgb(33, 43, 55);
        widgets.inactive.bg_fill = Color32::from_rgb(33, 43, 55);
        widgets.inactive.bg_stroke = border;
        widgets.hovered.weak_bg_fill = Color32::from_rgb(42, 56, 71);
        widgets.hovered.bg_fill = Color32::from_rgb(42, 56, 71);
        for state in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
        ] {
            state.corner_radius = egui::CornerRadius::same(8);
        }
    }
    style.visuals.selection.bg_fill = Color32::from_rgb(24, 92, 85);
    style.visuals.selection.stroke = egui::Stroke::new(1.5_f32, ACCENT);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(30, 111, 101);
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(14.0, 9.0);
    style.spacing.interact_size.y = 36.0;
    style.spacing.window_margin = egui::Margin::same(20);
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(28.0));
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(13.0));
    ctx.set_style(style);
}
pub fn eyebrow(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(MIN_TEXT).strong().color(ACCENT));
}
pub fn dark_panel(ui: &mut egui::Ui) {
    ui.style_mut().visuals = egui::Visuals::dark();
    ui.visuals_mut().selection.bg_fill = Color32::from_rgb(24, 92, 85);
}
pub fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(
            RichText::new(text)
                .color(Color32::from_rgb(9, 35, 34))
                .strong(),
        )
        .fill(ACCENT)
        .min_size(egui::vec2(150.0, 40.0)),
    )
    .clicked()
}
/// Eintrag der Seitenleiste: flach, der gewählte Bereich mit Akzentbalken.
pub fn nav_item(ui: &mut egui::Ui, text: &str, selected: bool, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, text)
    });
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            Color32::from_rgb(24, 92, 85)
        } else if response.hovered() || response.has_focus() {
            Color32::from_rgb(30, 41, 53)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 8, fill);
        if selected {
            let bar = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
            ui.painter().rect_filled(bar, 2, ACCENT);
        }
        ui.painter().text(
            egui::pos2(rect.left() + 14.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(15.0),
            if selected {
                Color32::WHITE
            } else {
                Color32::from_rgb(200, 210, 222)
            },
        );
    }
    response
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PagerStep {
    First,
    Previous,
    Next,
    Last,
}
/// Zielseite (ab 0) eines Pager-Schritts, immer innerhalb von `0..pages`.
pub fn pager_target(page: usize, pages: usize, step: PagerStep) -> usize {
    let last = pages.max(1) - 1;
    match step {
        PagerStep::First => 0,
        PagerStep::Previous => page.min(last).saturating_sub(1),
        PagerStep::Next => (page + 1).min(last),
        PagerStep::Last => last,
    }
}
fn pager_button(ui: &mut egui::Ui, step: PagerStep, enabled: bool, label: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(36.0, 36.0),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let widget = if enabled && response.hovered() {
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
        let colour = if enabled {
            visuals.text_color()
        } else {
            visuals.weak_text_color().gamma_multiply(0.45)
        };
        let (cx, cy) = (rect.center().x, rect.center().y);
        let forward = matches!(step, PagerStep::Next | PagerStep::Last);
        let sign = if forward { 1.0_f32 } else { -1.0_f32 };
        let double = matches!(step, PagerStep::First | PagerStep::Last);
        // Dreieck; bei erste/letzte Seite zusätzlich ein Balken am Rand.
        let shift = if double { 2.0 * sign } else { 0.0 };
        let tip = cx + shift + 5.0 * sign;
        let base = cx + shift - 4.0 * sign;
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(tip, cy),
                egui::pos2(base, cy - 6.0),
                egui::pos2(base, cy + 6.0),
            ],
            colour,
            egui::Stroke::NONE,
        ));
        if double {
            let bar_x = cx + 7.0 * sign;
            ui.painter().rect_filled(
                egui::Rect::from_center_size(egui::pos2(bar_x, cy), egui::vec2(2.5, 12.0)),
                1,
                colour,
            );
        }
    }
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let clicked = enabled && response.clicked();
    response.on_hover_text(label);
    clicked
}
/// Übliche Seitennavigation: erste, zurück, Seite X von Y, weiter, letzte.
/// Gibt die neue Seite (ab 0) zurück, wenn gewählt wurde.
pub fn pager(ui: &mut egui::Ui, page: usize, pages: usize, enabled: bool) -> Option<usize> {
    let pages = pages.max(1);
    let page = page.min(pages - 1);
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (step, label, available) in [
            (PagerStep::First, "Erste Seite", page > 0),
            (PagerStep::Previous, "Vorherige Seite", page > 0),
        ] {
            if pager_button(ui, step, enabled && available, label) {
                chosen = Some(pager_target(page, pages, step));
            }
        }
        ui.label(RichText::new(format!("Seite {} von {pages}", page + 1)).strong());
        for (step, label, available) in [
            (PagerStep::Next, "Nächste Seite", page + 1 < pages),
            (PagerStep::Last, "Letzte Seite", page + 1 < pages),
        ] {
            if pager_button(ui, step, enabled && available, label) {
                chosen = Some(pager_target(page, pages, step));
            }
        }
    });
    chosen
}
pub fn card(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(if ui.visuals().dark_mode {
            Color32::from_rgb(26, 34, 44)
        } else {
            Color32::WHITE
        })
        .corner_radius(12)
        .inner_margin(16)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(c: Color32) -> f32 {
        let channel = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }
    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
    #[test]
    fn pager_steps_stay_inside_the_page_range() {
        use PagerStep::*;
        assert_eq!(pager_target(4, 10, First), 0);
        assert_eq!(pager_target(4, 10, Previous), 3);
        assert_eq!(pager_target(4, 10, Next), 5);
        assert_eq!(pager_target(4, 10, Last), 9);
        assert_eq!(pager_target(0, 10, Previous), 0);
        assert_eq!(pager_target(9, 10, Next), 9);
        assert_eq!(pager_target(0, 0, Last), 0, "leere Liste hat eine Seite");
        assert_eq!(
            pager_target(50, 10, Previous),
            8,
            "veraltete Seite wird begrenzt"
        );
    }
    #[test]
    fn muted_and_accent_text_stay_readable_on_every_dark_surface() {
        let surfaces = [
            Color32::from_rgb(19, 24, 31),
            Color32::from_rgb(14, 19, 25),
            Color32::from_rgb(26, 34, 44),
            Color32::from_rgb(25, 32, 41),
        ];
        for surface in surfaces {
            assert!(contrast(MUTED, surface) >= 6.0, "{surface:?}");
            assert!(contrast(ACCENT, surface) >= 7.0, "{surface:?}");
            assert!(contrast(Color32::from_rgb(222, 230, 239), surface) >= 10.0);
        }
    }
}
