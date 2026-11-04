use egui::{Color32, RichText};
pub const ACCENT: Color32 = Color32::from_rgb(67, 211, 190);
pub const WARM: Color32 = Color32::from_rgb(239, 188, 111);
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
        .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
    ctx.set_style(style);
}
pub fn eyebrow(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).strong().color(ACCENT));
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
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(26, 34, 44))
        .corner_radius(12)
        .inner_margin(16)
}
