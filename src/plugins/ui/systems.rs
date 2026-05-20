use bevy::prelude::*;
use bevy::window::RequestRedraw;
use bevy_egui::{EguiClipboard, EguiContext, EguiContexts, EguiInput, egui};

#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::ai_chat::ChatState;
#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::compilation::LastCompiledParts;
#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::ui::resources::{AppErrors, ExportState, FilePickerState};
use crate::plugins::ui::resources::{PerformanceMonitor, SplashScreen};
pub use crate::plugins::ui::theme::{SPLASH_IMAGE_BYTES, set_window_icon};
#[cfg(not(target_arch = "wasm32"))]
use crate::plugins::ui::utils::{IMAGE_EXTENSIONS, load_image_as_chat_image};

/// Fixes clipboard paste events for cross-platform compatibility.
///
/// `bevy_egui` sends `Event::Text` for clipboard paste, but the correct egui convention
/// (used by the official `egui-winit` integration) is `Event::Paste`. On macOS this is
/// masked because the OS delivers paste content through IME, but on Windows the explicit
/// clipboard handling is the only path — and it can also fail when the logical key mapping
/// doesn't produce `egui::Key::V` (e.g. non-Latin keyboard layouts).
///
/// This system detects Ctrl/Cmd+V via physical key codes (layout-independent), replaces
/// any `Event::Text` from `bevy_egui`'s clipboard handler with `Event::Paste`, and adds
/// `Event::Paste` itself when `bevy_egui`'s handler was skipped entirely.
pub fn fix_clipboard_paste_events(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut egui_inputs: Query<&mut EguiInput, With<EguiContext>>,
    mut egui_clipboard: ResMut<EguiClipboard>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let cmd = keyboard.pressed(KeyCode::SuperLeft) || keyboard.pressed(KeyCode::SuperRight);
    let paste_modifier = if cfg!(target_os = "macos") { cmd } else { ctrl };

    if !paste_modifier || !keyboard.just_pressed(KeyCode::KeyV) {
        return;
    }

    let Some(clipboard_text) = egui_clipboard.get_text() else {
        return;
    };
    if clipboard_text.is_empty() {
        return;
    }

    for mut input in &mut egui_inputs {
        // Remove ALL Event::Text during a paste frame. On Windows, bevy_egui sends
        // individual per-character Text events that don't match the full clipboard
        // string, so an exact-match filter fails. Any Text event in a Ctrl+V frame
        // is an artifact of the key press, not intentional typing.
        input
            .events
            .retain(|event| !matches!(event, egui::Event::Text(_)));

        input
            .events
            .push(egui::Event::Paste(clipboard_text.replace("\r\n", "\n")));
    }
}

pub use crate::plugins::ui::layout::ui_layout_system;
pub use crate::plugins::ui::viewport::{
    cheatsheet_system, draw_axis_indicator, draw_part_labels, viewport_toolbar_system,
};

const SPLASH_DURATION: f32 = 0.75;
pub const SPLASH_FADE_DURATION: f32 = 0.3;

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn splash_screen_system(
    mut contexts: EguiContexts,
    mut splash: ResMut<SplashScreen>,
    time: Res<Time>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut redraw: EventWriter<RequestRedraw>,
) {
    if splash.timer <= -SPLASH_FADE_DURATION {
        return;
    }

    // Keep requesting redraws while splash animation is active
    redraw.send(RequestRedraw);

    if !splash.dismissing
        && (mouse_button.just_pressed(MouseButton::Left) || keyboard.get_just_pressed().len() > 0)
        && splash.timer < SPLASH_DURATION - 0.2
    {
        splash.dismissing = true;
        splash.timer = 0.0;
    }

    splash.timer -= time.delta_secs();
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    if splash.texture.is_none() {
        let image =
            image::load_from_memory(SPLASH_IMAGE_BYTES).expect("Failed to decode splash image");
        let rgba = image.to_rgba8();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        splash.texture =
            Some(ctx.load_texture("splash", color_image, egui::TextureOptions::LINEAR));
    }

    let Some(ref texture) = splash.texture else {
        return;
    };
    let alpha = if splash.timer < 0.0 {
        ((splash.timer + SPLASH_FADE_DURATION) / SPLASH_FADE_DURATION).clamp(0.0, 1.0)
    } else {
        1.0
    };
    if alpha <= 0.0 {
        return;
    }

    let screen_rect = ctx.screen_rect();
    egui::Area::new(egui::Id::new("splash_screen"))
        .fixed_pos(screen_rect.min)
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            ui.painter().rect_filled(
                screen_rect,
                0.0,
                egui::Color32::from_rgba_premultiplied(24, 24, 36, (alpha * 240.0) as u8),
            );
            let tex_size = texture.size_vec2();
            let max_dim = screen_rect.height().min(screen_rect.width()) * 0.5;
            let scale = max_dim / tex_size.x.max(tex_size.y);
            let img_size = tex_size * scale;
            let img_rect = egui::Rect::from_center_size(screen_rect.center(), img_size);
            ui.put(
                img_rect,
                egui::Image::new(egui::load::SizedTexture::new(texture.id(), img_size))
                    .tint(egui::Color32::from_rgba_unmultiplied(
                        255,
                        255,
                        255,
                        (alpha * 255.0) as u8,
                    ))
                    .corner_radius(egui::CornerRadius::same(16)),
            );
            ui.painter().text(
                egui::pos2(screen_rect.center().x, img_rect.max.y + 20.0),
                egui::Align2::CENTER_TOP,
                format!("SynapsCAD v{}", env!("CARGO_PKG_VERSION")),
                egui::FontId::proportional(20.0),
                egui::Color32::from_rgba_unmultiplied(220, 220, 230, (alpha * 255.0) as u8),
            );
        });
}

#[cfg(not(target_arch = "wasm32"))]
pub fn poll_file_picker_system(
    mut file_picker: ResMut<FilePickerState>,
    mut chat_state: ResMut<ChatState>,
    mut redraw: EventWriter<RequestRedraw>,
) {
    if file_picker.receiver.is_some() {
        redraw.send(RequestRedraw);
    }
    let paths = file_picker
        .receiver
        .as_ref()
        .and_then(|rx_mutex| rx_mutex.lock().unwrap().try_recv().ok());

    if let Some(paths) = paths {
        file_picker.receiver = None;
        for path in paths {
            if let Some(img) = load_image_as_chat_image(&path) {
                chat_state.pending_images.push(img);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn poll_export_system(
    mut export_state: ResMut<ExportState>,
    last_parts: Res<LastCompiledParts>,
    mut app_errors: ResMut<AppErrors>,
    mut redraw: EventWriter<RequestRedraw>,
) {
    if export_state.receiver.is_some() {
        redraw.send(RequestRedraw);
    }
    let maybe_path = export_state
        .receiver
        .as_ref()
        .and_then(|rx_mutex| rx_mutex.lock().unwrap().try_recv().ok());

    if let Some(maybe_path) = maybe_path {
        let format = export_state.pending_format.take();
        export_state.receiver = None;
        if let (Some(path), Some(fmt)) = (maybe_path, format) {
            match crate::export::export_parts(&last_parts.parts, &path, fmt) {
                Ok(()) => eprintln!("[SynapsCAD] Exported to {}", path.display()),
                Err(e) => {
                    eprintln!("[SynapsCAD] Export error: {e}");
                    app_errors.push(format!("Export failed: {e}"));
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn file_drop_system(
    mut dnd_events: EventReader<bevy::window::FileDragAndDrop>,
    mut chat_state: ResMut<ChatState>,
) {
    for event in dnd_events.read() {
        if let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = event
            && path_buf
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
            && let Some(img) = load_image_as_chat_image(path_buf)
        {
            chat_state.pending_images.push(img);
        }
    }
}

/// Performance monitoring system that tracks frame times and displays debug overlay.
pub fn performance_monitor_system(
    mut contexts: EguiContexts,
    mut perf_monitor: ResMut<PerformanceMonitor>,
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    // Toggle performance overlay with Ctrl+P
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if ctrl && keyboard.just_pressed(KeyCode::KeyP) {
        perf_monitor.show_overlay = !perf_monitor.show_overlay;
    }

    // Record current frame time
    let frame_time_ms = time.delta_secs() * 1000.0;
    perf_monitor.record_frame_time(frame_time_ms);

    // Show performance overlay if enabled
    if !perf_monitor.show_overlay {
        return;
    }

    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    egui::Window::new("Performance Monitor")
        .default_open(false)
        .resizable(true)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-10.0, 10.0))
        .show(ctx, |ui| {
            ui.label("Press Ctrl+P to toggle this overlay");
            ui.separator();

            let current_fps = perf_monitor.current_fps();
            let avg_frame_time = perf_monitor.average_frame_time();

            ui.label(format!("FPS: {current_fps:.1}"));
            ui.label(format!("Frame Time: {avg_frame_time:.2} ms"));
            ui.label(format!("Est. CPU: {:.1}%", perf_monitor.cpu_usage));

            ui.separator();
            ui.label("Frame Time Graph (last 60 frames):");

            if !perf_monitor.frame_times.is_empty() {
                let min_time = perf_monitor
                    .frame_times
                    .iter()
                    .copied()
                    .fold(f32::INFINITY, f32::min);
                let max_time = perf_monitor
                    .frame_times
                    .iter()
                    .copied()
                    .fold(f32::NEG_INFINITY, f32::max);

                let plot_height = 60.0;
                let plot_width = 200.0;

                let (response, painter) =
                    ui.allocate_painter(egui::vec2(plot_width, plot_height), egui::Sense::hover());

                // Draw background
                painter.rect_filled(response.rect, 2.0, egui::Color32::from_gray(30));

                // Draw frame time line
                if perf_monitor.frame_times.len() > 1 {
                    let points: Vec<egui::Pos2> = perf_monitor
                        .frame_times
                        .iter()
                        .enumerate()
                        .map(|(i, &time)| {
                            #[allow(clippy::cast_precision_loss)]
                            let x = (i as f32 / (perf_monitor.frame_times.len() - 1) as f32)
                                .mul_add(plot_width, response.rect.min.x);
                            let y = if max_time > min_time {
                                ((time - min_time) / (max_time - min_time))
                                    .mul_add(-plot_height, response.rect.max.y)
                            } else {
                                response.rect.center().y
                            };
                            egui::pos2(x, y)
                        })
                        .collect();

                    for window in points.windows(2) {
                        painter.line_segment(
                            [window[0], window[1]],
                            egui::Stroke::new(1.0, egui::Color32::GREEN),
                        );
                    }

                    // Draw 16.67ms line (60 FPS target)
                    if max_time > 16.67 && min_time < 16.67 {
                        let target_y = ((16.67 - min_time) / (max_time - min_time))
                            .mul_add(-plot_height, response.rect.max.y);
                        painter.line_segment(
                            [
                                egui::pos2(response.rect.min.x, target_y),
                                egui::pos2(response.rect.max.x, target_y),
                            ],
                            egui::Stroke::new(1.0, egui::Color32::RED),
                        );
                    }
                }

                ui.label(format!("Range: {min_time:.1} - {max_time:.1} ms"));

                // Color code the performance status
                let status_color = if current_fps < 30.0 {
                    egui::Color32::RED
                } else if current_fps < 55.0 {
                    egui::Color32::YELLOW
                } else {
                    egui::Color32::GREEN
                };

                ui.colored_label(
                    status_color,
                    if current_fps < 30.0 {
                        "Performance: Poor"
                    } else if current_fps < 55.0 {
                        "Performance: Fair"
                    } else {
                        "Performance: Good"
                    },
                );
            }
        });
}
