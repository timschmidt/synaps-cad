use crate::plugins::camera::OrbitCamera;
use crate::plugins::code_editor::ScadCode;
use crate::plugins::compilation::PartLabel;
use crate::plugins::scene::{LabelVisibility, MainCamera};
use crate::plugins::ui::resources::{CheatsheetOpen, OccupiedScreenSpace, SplashScreen};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::plugins::ui::systems::SPLASH_FADE_DURATION;

#[allow(clippy::too_many_arguments)]
pub fn viewport_toolbar_system(
    mut contexts: EguiContexts,
    occupied: Res<OccupiedScreenSpace>,
    mut scad_code: ResMut<ScadCode>,
    mut history: ResMut<crate::plugins::code_editor::UndoHistory>,
    mut orbit: ResMut<OrbitCamera>,
    mut ruler: ResMut<crate::plugins::camera::RulerState>,
    mut label_vis: ResMut<LabelVisibility>,
    mut cheatsheet: ResMut<CheatsheetOpen>,
) {
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    let toolbar_x = occupied.left + 8.0;
    let toolbar_y = 8.0;

    egui::Area::new(egui::Id::new("viewport_toolbar"))
        .fixed_pos(egui::pos2(toolbar_x, toolbar_y))
        .interactable(true)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_premultiplied(30, 30, 46, 220))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgb(55, 55, 75),
                ))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);

                        let undo_enabled = history.can_undo();
                        let redo_enabled = history.can_redo();

                        if ui
                            .add_enabled(undo_enabled, egui::Button::new("↩ Undo"))
                            .clicked()
                            && let Some(prev) = history.undo_stack.pop()
                        {
                            let current = scad_code.text.clone();
                            history.redo_stack.push(current);
                            prev.clone_into(&mut history.last_snapshot);
                            scad_code.text = prev;
                            scad_code.dirty = true;
                        }

                        if ui
                            .add_enabled(redo_enabled, egui::Button::new("↪ Redo"))
                            .clicked()
                            && let Some(next) = history.redo_stack.pop()
                        {
                            let current = scad_code.text.clone();
                            history.undo_stack.push(current);
                            next.clone_into(&mut history.last_snapshot);
                            scad_code.text = next;
                            scad_code.dirty = true;
                        }

                        ui.separator();

                        let view_btn = |ui: &mut egui::Ui, label: &str, tooltip: &str| -> bool {
                            ui.small_button(label).on_hover_text(tooltip).clicked()
                        };

                        let pi = std::f32::consts::PI;
                        let half_pi = std::f32::consts::FRAC_PI_2;
                        let quarter_pi = std::f32::consts::FRAC_PI_4;

                        if view_btn(ui, "F", "Front view (1)") {
                            orbit.yaw = 0.0;
                            orbit.pitch = 0.0;
                            orbit.zoom_to_fit = true;
                        }
                        if view_btn(ui, "Bk", "Back view (2)") {
                            orbit.yaw = pi;
                            orbit.pitch = 0.0;
                            orbit.zoom_to_fit = true;
                        }
                        if view_btn(ui, "R", "Right view (3)") {
                            orbit.yaw = half_pi;
                            orbit.pitch = 0.0;
                            orbit.zoom_to_fit = true;
                        }
                        if view_btn(ui, "L", "Left view (4)") {
                            orbit.yaw = -half_pi;
                            orbit.pitch = 0.0;
                            orbit.zoom_to_fit = true;
                        }
                        if view_btn(ui, "T", "Top view (5)") {
                            orbit.yaw = 0.0;
                            orbit.pitch = half_pi - 0.01;
                            orbit.zoom_to_fit = true;
                        }
                        if view_btn(ui, "Bo", "Bottom view (6)") {
                            orbit.yaw = 0.0;
                            orbit.pitch = -(half_pi - 0.01);
                            orbit.zoom_to_fit = true;
                        }
                        if view_btn(ui, "Iso", "Isometric view (7)") {
                            orbit.yaw = quarter_pi;
                            orbit.pitch = quarter_pi;
                            orbit.zoom_to_fit = true;
                        }
                        if view_btn(ui, "⊞", "Zoom to fit") {
                            orbit.zoom_to_fit = true;
                        }

                        ui.separator();

                        let ruler_label = if ruler.active { "📏 ✓" } else { "📏" };
                        if ui.selectable_label(ruler.active, ruler_label).clicked() {
                            ruler.active = !ruler.active;
                            if !ruler.active {
                                ruler.point_a = None;
                                ruler.point_b = None;
                            }
                        }

                        let label_btn = if label_vis.visible { "@" } else { "@ ✗" };
                        if ui.selectable_label(label_vis.visible, label_btn).clicked() {
                            label_vis.visible = !label_vis.visible;
                        }

                        if ui.selectable_label(cheatsheet.0, "⌨").clicked() {
                            cheatsheet.0 = !cheatsheet.0;
                        }
                    });
                });
        });
}

pub fn cheatsheet_system(
    mut contexts: EguiContexts,
    mut cheatsheet: ResMut<CheatsheetOpen>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    if !ctx.wants_keyboard_input() {
        let question_mark = (keyboard.pressed(KeyCode::ShiftLeft)
            || keyboard.pressed(KeyCode::ShiftRight))
            && keyboard.just_pressed(KeyCode::Slash);
        if question_mark || keyboard.just_pressed(KeyCode::KeyK) {
            cheatsheet.0 = !cheatsheet.0;
        }
    }

    if !cheatsheet.0 {
        return;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cheatsheet.0 = false;
        return;
    }

    egui::Window::new("⌨ Keyboard Shortcuts")
        .open(&mut cheatsheet.0)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Grid::new("cheatsheet_grid")
                .num_columns(2)
                .spacing([24.0, 6.0])
                .show(ui, |ui| {
                    let shortcuts: &[(&str, &str)] = &[
                        ("Orbit", "🖱 Middle / 🖱 Right"),
                        ("Pan", "Shift + 🖱 Middle"),
                        ("Zoom", "Scroll / + / −"),
                        ("Move focus", "W A S D / Arrow keys"),
                        ("Front view", "1"),
                        ("Back view", "2"),
                        ("Right view", "3"),
                        ("Left view", "4"),
                        ("Top view", "5"),
                        ("Bottom view", "6"),
                        ("Isometric view", "7"),
                        ("Toggle gizmos", "G"),
                        ("Toggle labels", "L"),
                        ("Keyboard shortcuts", "K / ?"),
                        ("Cancel ruler", "Esc"),
                    ];
                    for (action, key) in shortcuts {
                        ui.label(*action);
                        ui.strong(*key);
                        ui.end_row();
                    }
                });
        });
}

pub fn draw_part_labels(
    mut contexts: EguiContexts,
    part_query: Query<(
        &PartLabel,
        &GlobalTransform,
        &bevy::render::primitives::Aabb,
    )>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    occupied: Res<OccupiedScreenSpace>,
    splash: Res<SplashScreen>,
    label_vis: Res<LabelVisibility>,
) {
    if !label_vis.visible || splash.timer > -SPLASH_FADE_DURATION {
        return;
    }
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera_query.get_single() else {
        return;
    };

    for (part_label, global_transform, aabb) in &part_query {
        let center = global_transform.transform_point(aabb.center.into());
        let Ok(screen_pos) = camera.world_to_viewport(camera_transform, center) else {
            continue;
        };

        // Keep part labels inside the unobstructed viewport.
        let label_pos = egui::pos2(screen_pos.x + occupied.left, screen_pos.y);

        if label_pos.x < occupied.left {
            continue;
        }

        if label_pos.y < 50.0 {
            continue;
        }

        let [r, g, b] = part_label.color;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let part_color = egui::Color32::from_rgb(
            (r * 255.0).clamp(0.0, 255.0) as u8,
            (g * 255.0).clamp(0.0, 255.0) as u8,
            (b * 255.0).clamp(0.0, 255.0) as u8,
        );
        let lum = 0.0722f32.mul_add(b, 0.2126f32.mul_add(r, 0.7152 * g));
        let text_color = if lum > 0.6 {
            egui::Color32::BLACK
        } else {
            egui::Color32::WHITE
        };

        let label_text = &part_label.label;
        let char_width = 8.0;
        #[allow(clippy::cast_precision_loss)]
        let label_w = (label_text.len() as f32).mul_add(char_width, 8.0);
        let label_h = 18.0;

        egui::Area::new(egui::Id::new(format!("part_label_{}", part_label.index)))
            .fixed_pos(egui::pos2(
                screen_pos.x + occupied.left - label_w / 2.0,
                screen_pos.y - label_h / 2.0,
            ))
            .interactable(false)
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(part_color)
                    .corner_radius(egui::CornerRadius::same(3))
                    .inner_margin(egui::Margin::symmetric(4, 2))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(label_text)
                                .color(text_color)
                                .strong()
                                .small(),
                        );
                    });
            });
    }
}

pub fn draw_axis_indicator(
    mut contexts: EguiContexts,
    camera_query: Query<&GlobalTransform, With<MainCamera>>,
    occupied: Res<OccupiedScreenSpace>,
    splash: Res<SplashScreen>,
) {
    if splash.timer > -SPLASH_FADE_DURATION {
        return;
    }
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };
    let Ok(cam_tf) = camera_query.get_single() else {
        return;
    };

    let screen_rect = ctx.screen_rect();
    let center = egui::pos2(occupied.left + 40.0, screen_rect.max.y - 40.0);
    let axis_len = 25.0;
    let cam_rot = cam_tf.compute_transform().rotation;
    let view_rot = cam_rot.inverse();

    let axes = [
        (Vec3::X, egui::Color32::from_rgb(220, 60, 60), "X"),
        (Vec3::Y, egui::Color32::from_rgb(60, 100, 240), "Z"),
        (Vec3::Z, egui::Color32::from_rgb(60, 220, 60), "Y"),
    ];

    let mut sorted_axes: Vec<_> = axes
        .iter()
        .map(|&(dir, color, label)| {
            let view_dir = view_rot * dir;
            (view_dir, color, label)
        })
        .collect();
    sorted_axes
        .sort_by(|(a, _, _), (b, _, _)| a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal));

    egui::Area::new(egui::Id::new("axis_indicator"))
        .fixed_pos(center)
        .interactable(false)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let painter = ui.painter();
            let center = ui.min_rect().min;
            for (view_dir, color, label) in sorted_axes {
                let end = center + egui::vec2(view_dir.x, -view_dir.y) * axis_len;
                painter.line_segment([center, end], egui::Stroke::new(2.5_f32, color));
                painter.text(
                    end + egui::vec2(view_dir.x, -view_dir.y) * 8.0,
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(12.0),
                    color,
                );
            }
            painter.circle_filled(center, 2.0, egui::Color32::WHITE);
        });
}
