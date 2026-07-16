pub mod ai_chat;
pub mod camera;
pub mod code_editor;
pub mod compilation;
pub mod persistence;
pub mod scene;
pub mod ui;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use bevy::render::primitives::Aabb;

/// Returns the world-space AABB using the local center and half-extents.
///
/// The absolute linear transform maps the three local half-extent vectors to
/// their world-axis support radii, avoiding eight transformed corner points.
pub fn transformed_aabb(aabb: &Aabb, transform: &GlobalTransform) -> (Vec3, Vec3) {
    let affine = transform.affine();
    let matrix = affine.matrix3;
    let half_extents = matrix.x_axis.abs() * aabb.half_extents.x
        + matrix.y_axis.abs() * aabb.half_extents.y
        + matrix.z_axis.abs() * aabb.half_extents.z;
    let center = affine.transform_point3a(aabb.center);
    (
        (center - half_extents).into(),
        (center + half_extents).into(),
    )
}

/// Plugin group that registers all `SynapsCAD` plugins.
pub struct SynapScadPlugins;

impl PluginGroup for SynapScadPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(scene::ScenePlugin)
            .add(code_editor::CodeEditorPlugin)
            .add(ui::UiPlugin)
            .add(compilation::CompilationPlugin)
            .add(camera::CameraPlugin)
            .add(ai_chat::AiChatPlugin)
            .add(persistence::PersistencePlugin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transformed_aabb_matches_rotated_scaled_corner_envelope() {
        let aabb = Aabb::from_min_max(Vec3::new(-2.0, -1.0, -3.0), Vec3::new(4.0, 5.0, 1.0));
        let transform = GlobalTransform::from(Transform {
            translation: Vec3::new(7.0, -11.0, 13.0),
            rotation: Quat::from_euler(EulerRot::XYZ, 0.3, -0.7, 1.1),
            scale: Vec3::new(-2.0, 0.5, 3.0),
        });
        let (actual_min, actual_max) = transformed_aabb(&aabb, &transform);

        let local_min = Vec3::from(aabb.center - aabb.half_extents);
        let local_max = Vec3::from(aabb.center + aabb.half_extents);
        let mut expected_min = Vec3::splat(f32::INFINITY);
        let mut expected_max = Vec3::splat(f32::NEG_INFINITY);
        for corner in [
            Vec3::new(local_min.x, local_min.y, local_min.z),
            Vec3::new(local_max.x, local_min.y, local_min.z),
            Vec3::new(local_min.x, local_max.y, local_min.z),
            Vec3::new(local_max.x, local_max.y, local_min.z),
            Vec3::new(local_min.x, local_min.y, local_max.z),
            Vec3::new(local_max.x, local_min.y, local_max.z),
            Vec3::new(local_min.x, local_max.y, local_max.z),
            Vec3::new(local_max.x, local_max.y, local_max.z),
        ] {
            let world = transform.transform_point(corner);
            expected_min = expected_min.min(world);
            expected_max = expected_max.max(world);
        }

        assert!(actual_min.abs_diff_eq(expected_min, 1.0e-5));
        assert!(actual_max.abs_diff_eq(expected_max, 1.0e-5));
    }
}
