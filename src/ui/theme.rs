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
