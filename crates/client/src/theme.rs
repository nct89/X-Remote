//! Fonts and colours.
//!
//! UltraViewer's own UI is Vietnamese, so the default egui font (Ubuntu-Light)
//! is not enough — it has no precomposed Vietnamese glyphs and they render as
//! replacement boxes. We bundle Be Vietnam Pro instead.

use eframe::egui;
use egui::Color32;

pub const BG: Color32 = Color32::from_rgb(0xF3, 0xF5, 0xF8);
pub const TEXT: Color32 = Color32::from_rgb(0x1A, 0x1F, 0x26);

pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "be-vietnam".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/BeVietnamPro-Regular.ttf")).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "be-vietnam".to_owned());
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    // Text fields draw their background with `extreme_bg_color`; in the light
    // theme it is a light grey, we want pure white like the real client.
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.override_text_color = Some(TEXT);

    // egui's light theme draws text fields with no visible border, so on a white
    // card they disappear. Give them a real edge, and highlight on hover.
    let idle = egui::Stroke::new(1.0_f32, Color32::from_rgb(0xB4, 0xB4, 0xB4));
    let hot = egui::Stroke::new(1.0_f32, Color32::from_rgb(0x2E, 0x7B, 0xC4));
    visuals.widgets.inactive.bg_stroke = idle;
    visuals.widgets.hovered.bg_stroke = hot;
    visuals.widgets.active.bg_stroke = hot;

    // 5px corner radius on every widget (buttons, fields, menus).
    let radius = egui::CornerRadius::same(5);
    visuals.widgets.inactive.corner_radius = radius;
    visuals.widgets.hovered.corner_radius = radius;
    visuals.widgets.active.corner_radius = radius;
    visuals.widgets.open.corner_radius = radius;
    visuals.widgets.noninteractive.corner_radius = radius;

    visuals.menu_corner_radius = radius;
    visuals.window_corner_radius = radius;

    ctx.set_visuals(visuals);
}
