use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, RayCastSettings};
use bevy::prelude::*;
use bevy::render::mesh::MeshAabb;
use bevy_egui::{EguiContexts, egui};

use super::scene::{
    CadModel, CurrentGridSize, GizmoVisibility, LabelVisibility, MainCamera, ViewportGizmo,
};
use super::ui::OccupiedScreenSpace;

pub struct CameraPlugin;

/// Orbit camera state (Blender-style: MMB orbit, Shift+MMB pan, scroll zoom).
#[derive(Resource)]
pub struct OrbitCamera {
    pub focus: Vec3,
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
    /// Set to true to trigger zoom-to-fit on next frame.
    pub zoom_to_fit: bool,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            radius: 50.0,
            yaw: std::f32::consts::FRAC_PI_4,
            pitch: std::f32::consts::FRAC_PI_4,
            zoom_to_fit: false,
        }
    }
}

/// Ruler measurement state — two-point distance tool.
#[derive(Resource, Default)]
pub struct RulerState {
    /// Whether ruler mode is active (waiting for clicks).
    pub active: bool,
    /// First measurement point (set on first click).
    pub point_a: Option<Vec3>,
    /// Second measurement point (set on second click).
    pub point_b: Option<Vec3>,
}

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrbitCamera>()
            .init_resource::<RulerState>()
            .add_systems(
                Update,
                (
                    orbit_camera_system,
                    zoom_to_fit_system,
                    adjust_camera_viewport,
                    toggle_gizmos_system,
                    ruler_click_system,
                    ruler_gizmo_system,
                ),
            );
    }
}

#[allow(clippy::too_many_arguments)]
fn orbit_camera_system(
    mut orbit: ResMut<OrbitCamera>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut mouse_motion: EventReader<MouseMotion>,
    mut scroll_events: EventReader<MouseWheel>,
    mut camera_q: Query<&mut Transform, With<MainCamera>>,
    occupied: Res<OccupiedScreenSpace>,
    windows: Query<&Window>,
    mut contexts: EguiContexts,
) {
    // Don't process camera input if egui wants the pointer
    let Some(egui_ctx) = contexts.try_ctx_mut() else {
        return;
    };
    let egui_wants_pointer = egui_ctx.wants_pointer_input();

    let Ok(window) = windows.get_single() else {
        return;
    };
    let Ok(mut transform) = camera_q.get_single_mut() else {
        return;
    };

    // Check if cursor is over the 3D viewport (not the side panel)
    let cursor_in_viewport = if let Some(pos) = window.cursor_position() {
        pos.x > occupied.left
    } else {
        false
    };

    let can_interact = cursor_in_viewport && !egui_wants_pointer;

    // --- Scroll to zoom ---
    for ev in scroll_events.read() {
        if !can_interact {
            continue;
        }
        let scroll = match ev.unit {
            MouseScrollUnit::Line => ev.y * 5.0,
            MouseScrollUnit::Pixel => ev.y * 0.15,
        };
        orbit.radius *= scroll.mul_add(-0.02, 1.0).clamp(0.5, 2.0);
        orbit.radius = orbit.radius.clamp(0.1, 5000.0);
    }

    // --- Middle mouse button: orbit / pan ---
    let mmb = mouse_button.pressed(MouseButton::Middle);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

    // Also support right mouse button for orbit (common alternative)
    let rmb = mouse_button.pressed(MouseButton::Right);

    if (mmb || rmb) && can_interact {
        let mut delta = Vec2::ZERO;
        for ev in mouse_motion.read() {
            delta += ev.delta;
        }

        if shift && (mmb || rmb) {
            // Pan
            let sensitivity = orbit.radius * 0.002;
            let right = transform.rotation * Vec3::X;
            let up = transform.rotation * Vec3::Y;
            orbit.focus -= right * delta.x * sensitivity;
            orbit.focus += up * delta.y * sensitivity;
        } else {
            // Orbit
            let sensitivity = 0.005;
            orbit.yaw = delta.x.mul_add(-sensitivity, orbit.yaw);
            orbit.pitch = delta.y.mul_add(-sensitivity, orbit.pitch);
            orbit.pitch = orbit.pitch.clamp(
                -std::f32::consts::FRAC_PI_2 + 0.01,
                std::f32::consts::FRAC_PI_2 - 0.01,
            );
        }
    } else {
        // Drain unread motion events
        mouse_motion.read().for_each(|_| {});
    }

    // --- Keyboard controls (skip if egui has keyboard focus, e.g. code editor) ---
    if !egui_ctx.wants_keyboard_input() {
        let speed = orbit.radius * 0.02;
        if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::ArrowUp) {
            let forward = (orbit.focus - transform.translation).normalize();
            orbit.focus += forward * speed;
        }
        if keyboard.pressed(KeyCode::KeyS) || keyboard.pressed(KeyCode::ArrowDown) {
            let forward = (orbit.focus - transform.translation).normalize();
            orbit.focus -= forward * speed;
        }
        if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::ArrowLeft) {
            let right = transform.rotation * Vec3::X;
            orbit.focus -= right * speed;
        }
        if keyboard.pressed(KeyCode::KeyD) || keyboard.pressed(KeyCode::ArrowRight) {
            let right = transform.rotation * Vec3::X;
            orbit.focus += right * speed;
        }
        // Numpad/key zoom
        if keyboard.pressed(KeyCode::Equal) || keyboard.pressed(KeyCode::NumpadAdd) {
            orbit.radius = (orbit.radius * 0.97).max(0.1);
        }
        if keyboard.pressed(KeyCode::Minus) || keyboard.pressed(KeyCode::NumpadSubtract) {
            orbit.radius = (orbit.radius * 1.03).min(5000.0);
        }
        // Numpad views (also regular digit keys for keyboards without numpad)
        // Front: looking along -Z
        if keyboard.just_pressed(KeyCode::Numpad1) || keyboard.just_pressed(KeyCode::Digit1) {
            orbit.yaw = 0.0;
            orbit.pitch = 0.0;
            orbit.zoom_to_fit = true;
        }
        // Back: looking along +Z
        if keyboard.just_pressed(KeyCode::Numpad2) || keyboard.just_pressed(KeyCode::Digit2) {
            orbit.yaw = std::f32::consts::PI;
            orbit.pitch = 0.0;
            orbit.zoom_to_fit = true;
        }
        // Right: looking along -X
        if keyboard.just_pressed(KeyCode::Numpad3) || keyboard.just_pressed(KeyCode::Digit3) {
            orbit.yaw = std::f32::consts::FRAC_PI_2;
            orbit.pitch = 0.0;
            orbit.zoom_to_fit = true;
        }
        // Left: looking along +X
        if keyboard.just_pressed(KeyCode::Numpad4) || keyboard.just_pressed(KeyCode::Digit4) {
            orbit.yaw = -std::f32::consts::FRAC_PI_2;
            orbit.pitch = 0.0;
            orbit.zoom_to_fit = true;
        }
        // Top: looking down from above (+Y)
        if keyboard.just_pressed(KeyCode::Numpad5) || keyboard.just_pressed(KeyCode::Digit5) {
            orbit.yaw = 0.0;
            orbit.pitch = std::f32::consts::FRAC_PI_2 - 0.01;
            orbit.zoom_to_fit = true;
        }
        // Bottom: looking up from below (-Y)
        if keyboard.just_pressed(KeyCode::Numpad6) || keyboard.just_pressed(KeyCode::Digit6) {
            orbit.yaw = 0.0;
            orbit.pitch = -(std::f32::consts::FRAC_PI_2 - 0.01);
            orbit.zoom_to_fit = true;
        }
        // Isometric: default 45° view
        if keyboard.just_pressed(KeyCode::Numpad7) || keyboard.just_pressed(KeyCode::Digit7) {
            orbit.yaw = std::f32::consts::FRAC_PI_4;
            orbit.pitch = std::f32::consts::FRAC_PI_4;
            orbit.zoom_to_fit = true;
        }
    } // end keyboard guard

    // --- Apply orbit transform ---
    let rot = Quat::from_euler(EulerRot::YXZ, orbit.yaw, -orbit.pitch, 0.0);
    let offset = rot * Vec3::new(0.0, 0.0, orbit.radius);
    transform.translation = orbit.focus + offset;
    transform.look_at(orbit.focus, Vec3::Y);
}

/// After a successful compilation, compute the model's bounding box and zoom to fit.
fn zoom_to_fit_system(
    mut orbit: ResMut<OrbitCamera>,
    model_q: Query<(&Mesh3d, &GlobalTransform), With<CadModel>>,
    meshes: Res<Assets<Mesh>>,
) {
    if !orbit.zoom_to_fit {
        return;
    }

    // Compute combined bounding box over ALL parts
    let mut bb_min = Vec3::splat(f32::INFINITY);
    let mut bb_max = Vec3::splat(f32::NEG_INFINITY);
    let mut found = false;

    for (mesh3d, global_tf) in &model_q {
        let Some(mesh) = meshes.get(&mesh3d.0) else {
            continue;
        };
        let Some(aabb) = mesh.compute_aabb() else {
            continue;
        };
        // Transform AABB corners to world space
        let local_min = Vec3::from(aabb.center) - Vec3::from(aabb.half_extents);
        let local_max = Vec3::from(aabb.center) + Vec3::from(aabb.half_extents);
        for corner in [
            Vec3::new(local_min.x, local_min.y, local_min.z),
            Vec3::new(local_max.x, local_min.y, local_min.z),
            Vec3::new(local_min.x, local_max.y, local_min.z),
            Vec3::new(local_min.x, local_min.y, local_max.z),
            Vec3::new(local_max.x, local_max.y, local_min.z),
            Vec3::new(local_max.x, local_min.y, local_max.z),
            Vec3::new(local_min.x, local_max.y, local_max.z),
            Vec3::new(local_max.x, local_max.y, local_max.z),
        ] {
            let world = global_tf.transform_point(corner);
            bb_min = bb_min.min(world);
            bb_max = bb_max.max(world);
        }
        found = true;
    }

    if !found {
        return;
    }

    orbit.zoom_to_fit = false;

    let center = (bb_min + bb_max) * 0.5;
    let half_extents = (bb_max - bb_min) * 0.5;
    let max_extent = half_extents.max_element();

    orbit.focus = center;
    // Place camera far enough to see the full object (FOV ≈ 45°)
    orbit.radius = (max_extent * 2.5).max(2.0);
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn adjust_camera_viewport(
    occupied: Res<OccupiedScreenSpace>,
    windows: Query<&Window>,
    mut camera: Query<&mut Camera, With<MainCamera>>,
) {
    let Ok(window) = windows.get_single() else {
        return;
    };
    let Ok(mut cam) = camera.get_single_mut() else {
        return;
    };

    let width = window.physical_width() as f32;
    let height = window.physical_height() as f32;
    let scale = window.scale_factor();
    let left_pixels = (occupied.left * scale).round();

    let vp_width = (width - left_pixels).max(1.0) as u32;
    let vp_x = (left_pixels as u32).min(window.physical_width().saturating_sub(1));
    // Clamp so viewport never exceeds render target
    let vp_width = vp_width.min(window.physical_width().saturating_sub(vp_x));
    let vp_height = (height as u32).max(1);

    if vp_width > 0 && vp_height > 0 {
        cam.viewport = Some(bevy::render::camera::Viewport {
            physical_position: UVec2::new(vp_x, 0),
            physical_size: UVec2::new(vp_width, vp_height),
            ..default()
        });
    }
}

fn toggle_gizmos_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut gizmo_vis: ResMut<GizmoVisibility>,
    mut label_vis: ResMut<LabelVisibility>,
    mut gizmos: Query<&mut Visibility, With<ViewportGizmo>>,
    mut contexts: EguiContexts,
    grid_size: Res<CurrentGridSize>,
) {
    // When the grid was rebuilt, sync visibility on new entities
    if grid_size.is_changed() && !gizmo_vis.visible {
        for mut v in &mut gizmos {
            *v = Visibility::Hidden;
        }
    }

    // Don't toggle if egui wants keyboard input (e.g. typing in text field)
    let Some(egui_ctx) = contexts.try_ctx_mut() else {
        return;
    };
    if egui_ctx.wants_keyboard_input() {
        return;
    }

    if keyboard.just_pressed(KeyCode::KeyG) {
        gizmo_vis.visible = !gizmo_vis.visible;
        let vis = if gizmo_vis.visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for mut v in &mut gizmos {
            *v = vis;
        }
    }

    if keyboard.just_pressed(KeyCode::KeyL) {
        label_vis.visible = !label_vis.visible;
    }
}

/// Handle clicks in ruler mode — cast ray to find surface point.
#[allow(clippy::too_many_arguments)]
fn ruler_click_system(
    mut ruler: ResMut<RulerState>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut ray_cast: MeshRayCast,
    mut contexts: EguiContexts,
    occupied: Res<OccupiedScreenSpace>,
) {
    // Escape cancels ruler mode
    if keyboard.just_pressed(KeyCode::Escape) && ruler.active {
        ruler.active = false;
        ruler.point_a = None;
        ruler.point_b = None;
        return;
    }

    if !ruler.active || !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }

    // Don't raycast if egui is using the pointer (toolbar click, etc.)
    let Some(egui_ctx) = contexts.try_ctx_mut() else {
        return;
    };
    if egui_ctx.is_pointer_over_area() {
        return;
    }

    let Ok(window) = windows.get_single() else {
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera_q.get_single() else {
        return;
    };

    // Convert window cursor to viewport-local coordinates
    let viewport_cursor = Vec2::new(cursor_pos.x - occupied.left, cursor_pos.y);
    if viewport_cursor.x < 0.0 {
        return;
    }

    let Ok(ray) = camera.viewport_to_world(camera_transform, viewport_cursor) else {
        return;
    };

    let settings = RayCastSettings::default();
    let hits = ray_cast.cast_ray(ray, &settings);

    if let Some((_entity, hit)) = hits.first() {
        if ruler.point_a.is_none() || ruler.point_b.is_some() {
            // Start new measurement (or restart after completed)
            ruler.point_a = Some(hit.point);
            ruler.point_b = None;
        } else {
            // Complete measurement
            ruler.point_b = Some(hit.point);
        }
    }
}

/// Draw ruler gizmo line and distance label.
fn ruler_gizmo_system(
    ruler: Res<RulerState>,
    mut gizmos: Gizmos,
    camera_q: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut contexts: EguiContexts,
) {
    if !ruler.active {
        return;
    }

    let Some(a) = ruler.point_a else { return };

    // Draw point A marker
    gizmos.sphere(
        Isometry3d::from_translation(a),
        0.5,
        Color::srgb(1.0, 0.3, 0.3),
    );

    let Some(b) = ruler.point_b else { return };

    // Draw point B marker and line
    gizmos.sphere(
        Isometry3d::from_translation(b),
        0.5,
        Color::srgb(0.3, 0.3, 1.0),
    );
    gizmos.line(a, b, Color::srgb(1.0, 1.0, 0.3));

    // Draw distance label as egui overlay at midpoint
    let midpoint = (a + b) * 0.5;
    let distance = a.distance(b);
    let Ok((camera, camera_transform)) = camera_q.get_single() else {
        return;
    };
    let Some(screen_pos) = camera.world_to_viewport(camera_transform, midpoint).ok() else {
        return;
    };
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    egui::Area::new(egui::Id::new("ruler_label"))
        .fixed_pos(egui::pos2(screen_pos.x, screen_pos.y))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_premultiplied(20, 20, 30, 230))
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(6, 3))
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgb(255, 255, 100),
                ))
                .show(ui, |ui: &mut egui::Ui| {
                    ui.label(
                        egui::RichText::new(format!("{distance:.2}"))
                            .color(egui::Color32::from_rgb(255, 255, 100))
                            .strong(),
                    );
                });
        });
}
