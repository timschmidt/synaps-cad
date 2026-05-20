use bevy::prelude::*;
use bevy::render::mesh::{MeshAabb, PrimitiveTopology};

pub struct ScenePlugin;

#[derive(Component)]
pub struct MainCamera;

#[derive(Component)]
pub struct CadModel;

/// Tag for grid + axes entities that can be toggled.
#[derive(Component)]
pub struct ViewportGizmo;

/// Marker for the grid mesh (for despawn on resize).
#[derive(Component)]
pub struct GridEntity;

/// Marker for axis line meshes (for despawn on resize).
#[derive(Component)]
pub struct AxisLineEntity;

/// Tag for the directional light that follows the camera orientation.
#[derive(Component)]
pub struct CameraFollowLight;

/// Tracks the current grid size so we only rebuild when it changes.
#[derive(Resource)]
pub struct CurrentGridSize(pub f32);

impl Default for CurrentGridSize {
    fn default() -> Self {
        Self(50.0)
    }
}

/// Visibility state for viewport gizmos (axes + grid).
#[derive(Resource)]
pub struct GizmoVisibility {
    pub visible: bool,
}

impl Default for GizmoVisibility {
    fn default() -> Self {
        Self { visible: true }
    }
}

/// Visibility state for part labels (@1, @2, ...).
#[derive(Resource)]
pub struct LabelVisibility {
    pub visible: bool,
}

impl Default for LabelVisibility {
    fn default() -> Self {
        Self { visible: true }
    }
}

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GizmoVisibility>()
            .init_resource::<LabelVisibility>()
            .init_resource::<CurrentGridSize>()
            .add_systems(Startup, setup_scene)
            .add_systems(Update, (update_camera_follow_light, update_grid_system));
    }
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(30.0, 30.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
        MainCamera,
    ));

    // CAD-style lighting: strong ambient + soft camera-relative fill light (no harsh shadows).
    // This reveals surface curvature (holes, tubes, fillets) without obscuring detail.
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 800.0,
    });

    // Key light — soft, shadow-free, will be updated to follow camera each frame
    commands.spawn((
        DirectionalLight {
            illuminance: 4_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, 0.5, 0.0)),
        CameraFollowLight,
    ));

    // --- XYZ Axis Lines + Grid ---
    let grid_size = 50.0;
    spawn_axis_lines(&mut commands, &mut meshes, &mut materials, grid_size);
    spawn_grid(&mut commands, &mut meshes, &mut materials, grid_size);
}

fn spawn_axis_lines(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    axis_length: f32,
) {
    // X axis (red)
    spawn_axis_line(
        commands,
        meshes,
        materials,
        Vec3::ZERO,
        Vec3::X * axis_length,
        Color::srgb(0.9, 0.2, 0.2),
    );
    // Y axis (blue) — Bevy Y-up = OpenSCAD Z-up
    spawn_axis_line(
        commands,
        meshes,
        materials,
        Vec3::ZERO,
        Vec3::Y * axis_length,
        Color::srgb(0.2, 0.4, 0.9),
    );
    // Z axis (green) — Bevy Z = OpenSCAD Y
    spawn_axis_line(
        commands,
        meshes,
        materials,
        Vec3::ZERO,
        Vec3::Z * axis_length,
        Color::srgb(0.2, 0.8, 0.2),
    );
}

fn spawn_axis_line(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    from: Vec3,
    to: Vec3,
    color: Color,
) {
    let mut mesh = Mesh::new(PrimitiveTopology::LineList, default());
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[from.x, from.y, from.z], [to.x, to.y, to.z]],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 2]);

    let material = materials.add(StandardMaterial {
        base_color: color,
        unlit: true,
        ..default()
    });

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        ViewportGizmo,
        AxisLineEntity,
        PickingBehavior::IGNORE,
    ));
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn spawn_grid(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    grid_size: f32,
) {
    let grid_step = 10.0_f32;
    let half = grid_size;
    let steps = (grid_size / grid_step) as i32;

    let mut positions: Vec<[f32; 3]> = Vec::new();

    // Lines parallel to X axis (varying Z)
    for i in -steps..=steps {
        let z = i as f32 * grid_step;
        positions.push([-half, 0.0, z]);
        positions.push([half, 0.0, z]);
    }
    // Lines parallel to Z axis (varying X)
    for i in -steps..=steps {
        let x = i as f32 * grid_step;
        positions.push([x, 0.0, -half]);
        positions.push([x, 0.0, half]);
    }

    let vert_count = positions.len();
    let mut mesh = Mesh::new(PrimitiveTopology::LineList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; vert_count]);

    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.35, 0.35, 0.35, 0.4),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        ViewportGizmo,
        GridEntity,
        PickingBehavior::IGNORE,
    ));
}

/// Recompute grid size based on model bounding box and rebuild if changed.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::too_many_arguments
)]
fn update_grid_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut current_size: ResMut<CurrentGridSize>,
    _gizmo_vis: Res<GizmoVisibility>,
    model_q: Query<(&Mesh3d, &GlobalTransform), With<CadModel>>,
    grid_q: Query<Entity, With<GridEntity>>,
    axis_q: Query<Entity, With<AxisLineEntity>>,
) {
    // Compute combined AABB of all CadModel entities
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

    let desired = if found {
        let half_extents = (bb_max - bb_min) * 0.5;
        let max_extent = half_extents.max_element();
        // Round up to nearest 10 for clean grid lines
        let raw = (max_extent * 1.5).max(50.0);
        (raw / 10.0).ceil() * 10.0
    } else {
        50.0
    };

    if (desired - current_size.0).abs() < 0.1 {
        return;
    }

    // Despawn old grid + axis entities
    for entity in &grid_q {
        commands.entity(entity).despawn();
    }
    for entity in &axis_q {
        commands.entity(entity).despawn();
    }

    current_size.0 = desired;

    // Respawn with new size
    spawn_axis_lines(&mut commands, &mut meshes, &mut materials, desired);
    spawn_grid(&mut commands, &mut meshes, &mut materials, desired);
}

/// Keeps the fill light roughly aligned with the camera so geometry is always well-lit.
fn update_camera_follow_light(
    camera_q: Query<&Transform, With<MainCamera>>,
    mut light_q: Query<&mut Transform, (With<CameraFollowLight>, Without<MainCamera>)>,
) {
    let Ok(cam_tf) = camera_q.get_single() else {
        return;
    };
    for mut light_tf in &mut light_q {
        // Point the light in the same direction the camera is looking,
        // offset slightly upward so top surfaces get a bit more light.
        let forward = cam_tf.forward().as_vec3();
        let up = Vec3::Y;
        let dir = (forward + up * 0.3).normalize();
        light_tf.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, dir);
    }
}
