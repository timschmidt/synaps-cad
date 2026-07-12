use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, egui};

pub const SPLASH_IMAGE_BYTES: &[u8] = include_bytes!("../../../assets/splash@2x.png");

pub fn setup_egui_theme(mut contexts: EguiContexts) {
    let ctx = contexts.ctx_mut();

    let mut visuals = egui::Visuals::dark();

    // Panel & window backgrounds
    let bg = egui::Color32::from_rgb(24, 24, 36);
    let panel_bg = egui::Color32::from_rgb(30, 30, 46);
    let widget_bg = egui::Color32::from_rgb(40, 40, 58);
    let accent = egui::Color32::from_rgb(100, 160, 255);
    let text_color = egui::Color32::from_rgb(220, 220, 230);
    let dim_text = egui::Color32::from_rgb(140, 140, 160);
    let separator = egui::Color32::from_rgb(55, 55, 75);

    visuals.panel_fill = panel_bg;
    visuals.window_fill = panel_bg;
    visuals.extreme_bg_color = bg;
    visuals.faint_bg_color = widget_bg;

    // Widget styling
    let rounding = egui::CornerRadius::same(6);
    let small_rounding = egui::CornerRadius::same(4);

    visuals.widgets.noninteractive.bg_fill = widget_bg;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, dim_text);
    visuals.widgets.noninteractive.corner_radius = small_rounding;

    visuals.widgets.inactive.bg_fill = widget_bg;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, text_color);
    visuals.widgets.inactive.corner_radius = rounding;

    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(55, 55, 80);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
    visuals.widgets.hovered.corner_radius = rounding;

    visuals.widgets.active.bg_fill = accent;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
    visuals.widgets.active.corner_radius = rounding;

    visuals.widgets.open.bg_fill = egui::Color32::from_rgb(50, 50, 72);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, text_color);
    visuals.widgets.open.corner_radius = rounding;

    visuals.selection.bg_fill = accent.linear_multiply(0.3);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, accent);

    visuals.window_corner_radius = egui::CornerRadius::same(8);
    visuals.window_stroke = egui::Stroke::new(1.0_f32, separator);

    // Separator
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, separator);

    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);

    ctx.set_visuals(visuals);

    // Spacing and performance optimizations
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.window_margin = egui::Margin::same(12);

    // Reduce tessellation complexity for better performance
    style.visuals.clip_rect_margin = 3.0; // Reduce clipping margin
    style.animation_time = 0.1; // Reduce animation time to reduce redraws
    style.explanation_tooltips = false; // Disable tooltips to reduce hover redraws

    ctx.set_style(style);

    // Set tessellation options for better performance
    ctx.tessellation_options_mut(|options| {
        options.feathering_size_in_pixels = 1.0; // Reduce feathering for better performance
        options.round_text_to_pixels = true; // Align text to pixels for better caching
    });
}

pub fn set_window_icon(
    mut contexts: EguiContexts,
    primary: Query<Entity, With<PrimaryWindow>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Ok(entity) = primary.get_single() else {
        return;
    };
    let Some(ctx) = contexts.try_ctx_for_entity_mut(entity) else {
        return;
    };
    let image = image::load_from_memory(SPLASH_IMAGE_BYTES).expect("Failed to decode icon image");
    let icon_img = image.resize(256, 256, image::imageops::FilterType::Lanczos3);
    let rgba = icon_img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let icon_data = egui::IconData {
        rgba: rgba.into_raw(),
        width: w,
        height: h,
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(std::sync::Arc::new(
        icon_data,
    ))));
    *done = true;
}
