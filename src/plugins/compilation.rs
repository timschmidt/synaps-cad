use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};

use super::ai_chat::{ChatMessage, VerificationState};
use super::camera::OrbitCamera;
use super::code_editor::ScadCode;
use super::scene::CadModel;
use crate::compiler;

const PART_PALETTE: &[[f32; 3]] = &[
    [0.40, 0.70, 1.00], // sky blue
    [1.00, 0.60, 0.40], // coral
    [0.50, 0.85, 0.50], // soft green
    [0.95, 0.75, 0.30], // amber
    [0.70, 0.50, 0.90], // lavender
    [0.30, 0.85, 0.85], // teal
    [0.95, 0.45, 0.60], // rose
    [0.60, 0.80, 0.30], // lime
    [0.85, 0.55, 0.80], // orchid
    [0.45, 0.65, 0.85], // steel blue
    [0.90, 0.65, 0.55], // salmon
    [0.55, 0.75, 0.65], // sage
];

/// Human-readable color names matching `PART_PALETTE` (for AI context).
#[allow(dead_code)]
pub const PART_COLOR_NAMES: &[&str] = &[
    "sky blue",
    "coral",
    "soft green",
    "amber",
    "lavender",
    "teal",
    "rose",
    "lime",
    "orchid",
    "steel blue",
    "salmon",
    "sage",
];

pub struct CompilationPlugin;

/// Label attached to each compiled submesh for AI context and viewport display.
#[derive(Component)]
pub struct PartLabel {
    /// 1-based part index.
    pub index: usize,
    /// Display string, e.g. `@1`.
    pub label: String,
    /// RGB color matching the part material.
    pub color: [f32; 3],
}

/// Toggle visibility of part labels in the viewport.
#[derive(Resource)]
#[allow(dead_code)]
pub struct PartLabelVisibility {
    pub visible: bool,
}

impl Default for PartLabelVisibility {
    fn default() -> Self {
        Self { visible: true }
    }
}

#[derive(Clone)]
pub struct StlMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Optional color from `color()` in the code.
    pub color: Option<[f32; 3]>,
}

pub type ModelView = (String, String);
pub type AlternateModelView = (String, Vec<ModelView>);

pub enum CompilationResult {
    Success {
        /// Hash of the source and render settings that produced this result.
        source_hash: [u8; 32],
        parts: Vec<StlMeshData>,
        views: Vec<ModelView>, // (label, base64_png)
        /// Views for non-active `$view` branches.
        other_views: Vec<AlternateModelView>,
        warnings: Vec<String>,
    },
    Error(String),
    Canceled,
}

#[derive(Resource)]
pub struct CompilationState {
    pub is_compiling: bool,
    pub result_receiver: Option<Mutex<mpsc::Receiver<CompilationResult>>>,
    /// Whether the next successful compilation should trigger a zoom-to-fit.
    /// This is set when the user explicitly requests a fresh render (Clear, Load)
    /// but NOT on iterative edits/updates.
    pub should_zoom: bool,
    /// Cancellation signal for the running compilation.
    pub cancel_signal: Option<Arc<AtomicBool>>,
}

impl Default for CompilationState {
    fn default() -> Self {
        Self {
            is_compiling: false,
            result_receiver: None,
            should_zoom: true, // Default to true so first load/compile zooms
            cancel_signal: None,
        }
    }
}

/// Rendered orthographic views of the model (for AI context).
#[derive(Resource, Default)]
pub struct ModelViews {
    /// Hash of the source and render settings that produced these images.
    pub source_hash: Option<[u8; 32]>,
    pub views: Vec<ModelView>, // (label, base64_png) for active view
    /// Views rendered for non-active `$view` branches (smaller resolution).
    /// Each entry is (`view_name`, Vec<(label, `base64_png`)>).
    pub other_views: Vec<AlternateModelView>,
}

impl ModelViews {
    /// Remove rendered AI context as soon as its compilation inputs change.
    pub fn invalidate(&mut self) {
        self.source_hash = None;
        self.views.clear();
        self.other_views.clear();
    }

    /// Return whether these views were rendered from the current compilation inputs.
    pub fn matches_source(&self, code: &str, fn_value: u32) -> bool {
        self.source_hash == Some(model_source_hash(code, fn_value))
    }
}

/// Hash every input that affects rendered model views.
pub fn model_source_hash(code: &str, fn_value: u32) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"synaps-cad-model-views-v1\0");
    hasher.update(&fn_value.to_le_bytes());
    hasher.update(code.as_bytes());
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod model_view_cache_tests {
    use super::*;

    fn populated_views(code: &str, fn_value: u32) -> ModelViews {
        ModelViews {
            source_hash: Some(model_source_hash(code, fn_value)),
            views: vec![("Iso".into(), "image-data".into())],
            other_views: vec![(
                "detail".into(),
                vec![("Front".into(), "other-image-data".into())],
            )],
        }
    }

    #[test]
    fn model_views_match_only_their_exact_compilation_inputs() {
        let views = populated_views("cube(10);", 16);

        assert!(views.matches_source("cube(10);", 16));
        assert!(!views.matches_source("cube(11);", 16));
        assert!(!views.matches_source("cube(10);", 32));
    }

    #[test]
    fn invalidation_clears_hash_and_all_images() {
        let mut views = populated_views("cube(10);", 16);

        views.invalidate();

        assert_eq!(views.source_hash, None);
        assert!(views.views.is_empty());
        assert!(views.other_views.is_empty());
    }
}

/// Cached copy of last compiled mesh data for re-rendering views with markers.
#[derive(Resource, Default)]
pub struct LastCompiledParts {
    pub parts: Vec<StlMeshData>,
}

/// Timer that limits how often the main loop polls for compilation results.
#[derive(Resource)]
pub struct CompilationPollingTimer {
    timer: Timer,
}

impl Default for CompilationPollingTimer {
    fn default() -> Self {
        Self {
            // Ten polls per second keep idle redraw work bounded.
            timer: Timer::from_seconds(0.1, TimerMode::Repeating),
        }
    }
}

#[derive(Resource, Default)]
pub struct LoadedModel {
    pub entity: Option<Entity>,
}

impl Plugin for CompilationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CompilationState>()
            .init_resource::<LoadedModel>()
            .init_resource::<ModelViews>()
            .init_resource::<LastCompiledParts>()
            .init_resource::<CompilationPollingTimer>()
            .init_resource::<PartLabelVisibility>()
            .add_systems(
                Update,
                (trigger_compilation_system, poll_compilation_system)
                    .chain()
                    .in_set(CompilationSystemSet),
            )
            // Ensure UI/editor systems can mark `ScadCode::dirty` before we decide
            // whether to trigger compilation in this frame.
            .configure_sets(
                Update,
                CompilationSystemSet.after(crate::plugins::ui::layout::ui_layout_system),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct CompilationSystemSet;

fn trigger_compilation_system(
    mut scad_code: ResMut<ScadCode>,
    mut compilation_state: ResMut<CompilationState>,
    mut model_views: ResMut<ModelViews>,
) {
    if !scad_code.dirty {
        return;
    }

    // Never expose views rendered from previous source while compilation is pending.
    model_views.invalidate();

    // Supersede a running compilation so the latest source can start immediately.
    if compilation_state.is_compiling {
        if let Some(cancel) = &compilation_state.cancel_signal {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        // Dropping the receiver makes the canceled thread's eventual send harmless.
        compilation_state.result_receiver = None;
        compilation_state.cancel_signal = None;
        compilation_state.is_compiling = false;
    }

    scad_code.dirty = false;
    compilation_state.is_compiling = true;

    let code = scad_code.text.clone();
    let fn_value = scad_code.fn_value;
    let (tx, rx) = mpsc::channel();
    compilation_state.result_receiver = Some(Mutex::new(rx));

    #[cfg(not(target_arch = "wasm32"))]
    {
        let cancel = Arc::new(AtomicBool::new(false));
        compilation_state.cancel_signal = Some(cancel.clone());

        // Use a larger stack (64 MB) for the compilation thread because complex
        // OpenSCAD models with deep nesting (hull, difference, nested modules)
        // can overflow the default 8 MB stack.
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                let result = compile_openscad(&code, fn_value, Some(cancel));
                let _ = tx.send(result);
            })
            .expect("Failed to spawn compilation thread");
    }

    #[cfg(target_arch = "wasm32")]
    {
        compilation_state.cancel_signal = None;
        let result = compile_openscad(&code, fn_value, None);
        let _ = tx.send(result);
    }
}

fn compile_openscad(
    code: &str,
    fn_value: u32,
    cancel: Option<Arc<AtomicBool>>,
) -> CompilationResult {
    use super::code_editor::{detect_views, set_active_view};

    // An empty document intentionally clears the viewport.
    if code.trim().is_empty() {
        return CompilationResult::Success {
            source_hash: model_source_hash(code, fn_value),
            parts: Vec::new(),
            views: Vec::new(),
            other_views: Vec::new(),
            warnings: Vec::new(),
        };
    }

    let t0 = web_time::Instant::now();
    eprintln!("[SynapsCAD] Compiling (fn={fn_value})...");

    // Convert dependency panics into user-visible compilation failures.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        compiler::compile_scad_code(code, fn_value, cancel)
    }));

    let result = match result {
        Ok(r) => r,
        Err(panic_info) => {
            #[allow(clippy::option_if_let_else)]
            let msg = if let Some(s) = panic_info.downcast_ref::<&'static str>() {
                (*s).to_string()
            } else if let Some(s) = panic_info.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic during compilation".to_string()
            };
            eprintln!("[SynapsCAD] Compilation panic caught: {msg}");
            return CompilationResult::Error(format!("Internal error: {msg}"));
        }
    };

    match result {
        compiler::CompilationResult::Success {
            parts,
            views,
            warnings,
        } => {
            let total_verts: usize = parts.iter().map(|p| p.positions.len()).sum();
            let total_tris: usize = parts.iter().map(|p| p.indices.len()).sum::<usize>() / 3;
            eprintln!(
                "[SynapsCAD] Compiled in {:?}: {} parts, {} verts, {} tris, {} views",
                t0.elapsed(),
                parts.len(),
                total_verts,
                total_tris,
                views.len(),
            );
            if !warnings.is_empty() {
                for w in &warnings {
                    eprintln!("[SynapsCAD] Warning: {w}");
                }
            }

            // Render inactive `$view` branches at the smaller default resolution.
            let (active_view, all_views) = detect_views(code);
            let mut other_views = Vec::new();
            if all_views.len() > 1 {
                let active = active_view.unwrap_or_default();
                for view_name in &all_views {
                    if *view_name == active {
                        continue;
                    }
                    let mut alt_code = code.to_string();
                    if set_active_view(&mut alt_code, view_name)
                        && let Ok(alt_views) = compiler::compile_views_only(&alt_code)
                        && !alt_views.is_empty()
                    {
                        other_views.push((
                            view_name.clone(),
                            alt_views
                                .into_iter()
                                .map(|v| (v.label, v.base64_png))
                                .collect(),
                        ));
                    }
                }
                if !other_views.is_empty() {
                    eprintln!(
                        "[SynapsCAD] Rendered {} other view(s) in {:?}",
                        other_views.len(),
                        t0.elapsed()
                    );
                }
            }

            CompilationResult::Success {
                source_hash: model_source_hash(code, fn_value),
                parts: parts
                    .into_iter()
                    .map(|m| StlMeshData {
                        positions: m.positions,
                        normals: m.normals,
                        indices: m.indices,
                        color: m.color,
                    })
                    .collect(),
                views: views.into_iter().map(|v| (v.label, v.base64_png)).collect(),
                other_views,
                warnings,
            }
        }
        compiler::CompilationResult::Canceled => {
            eprintln!("[SynapsCAD] Compilation canceled");
            CompilationResult::Canceled
        }
        compiler::CompilationResult::Error(e) => {
            eprintln!("[SynapsCAD] Compilation error: {e}");
            CompilationResult::Error(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn poll_compilation_system(
    mut commands: Commands,
    mut compilation_state: ResMut<CompilationState>,
    mut loaded_model: ResMut<LoadedModel>,
    mut model_views: ResMut<ModelViews>,
    mut last_compiled: ResMut<LastCompiledParts>,
    mut chat_state: ResMut<super::ai_chat::ChatState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut orbit: ResMut<OrbitCamera>,
    model_query: Query<Entity, With<CadModel>>,
    mut polling_timer: ResMut<CompilationPollingTimer>,
    time: Res<Time>,
    mut redraw: EventWriter<bevy::window::RequestRedraw>,
) {
    // Polling requires redraws even when the rest of the interface is idle.
    if compilation_state.result_receiver.is_some() {
        redraw.send(bevy::window::RequestRedraw);
    }

    // Match the polling rate configured by `CompilationPollingTimer`.
    if !polling_timer.timer.tick(time.delta()).just_finished() {
        return;
    }

    let result = {
        let Some(ref rx_mutex) = compilation_state.result_receiver else {
            return;
        };
        let rx = rx_mutex.lock().unwrap();
        match rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                eprintln!("[SynapsCAD] Compilation worker disconnected without a result");
                drop(rx);
                compilation_state.is_compiling = false;
                compilation_state.result_receiver = None;
                return;
            }
        }
    };

    compilation_state.is_compiling = false;
    compilation_state.result_receiver = None;
    compilation_state.cancel_signal = None;

    match result {
        CompilationResult::Success {
            source_hash,
            parts,
            views,
            other_views,
            warnings,
        } => {
            for entity in model_query.iter() {
                commands.entity(entity).despawn();
            }

            // Surface recoverable compiler diagnostics in the chat panel.
            if !warnings.is_empty() {
                let warning_text = format!(
                    "⚠️ Compilation warnings:\n{}\n\nPlease report issues with the code that caused them.",
                    warnings
                        .iter()
                        .map(|w| format!("• {w}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                chat_state.messages.push(ChatMessage {
                    role: "assistant".into(),
                    content: warning_text,
                    thinking: None,
                    images: Vec::new(),
                    auto_generated: true,
                    is_error: true,
                });
            }

            model_views.source_hash = Some(source_hash);
            model_views.views = views;
            model_views.other_views = other_views;
            last_compiled.parts.clone_from(&parts);

            for (i, stl_data) in parts.into_iter().enumerate() {
                let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, stl_data.positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, stl_data.normals);
                mesh.insert_indices(Indices::U32(stl_data.indices));

                let part_color = stl_data
                    .color
                    .unwrap_or(PART_PALETTE[i % PART_PALETTE.len()]);
                let [r, g, b] = part_color;
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgb(r, g, b),
                    perceptual_roughness: 0.9,
                    metallic: 0.0,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                });

                let part_index = i + 1; // 1-based
                let entity = commands
                    .spawn((
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(material),
                        Transform::default(),
                        CadModel,
                        PartLabel {
                            index: part_index,
                            label: format!("@{part_index}"),
                            color: part_color,
                        },
                        PickingBehavior::default(),
                    ))
                    .id();
                loaded_model.entity = Some(entity);
            }
            orbit.zoom_to_fit = compilation_state.should_zoom;
            compilation_state.should_zoom = false; // Reset flag after use
        }
        CompilationResult::Error(err) => {
            // AI-generated failures enter the verification recovery path.
            let is_ai_error = chat_state.verification == VerificationState::WaitingForCompilation;

            let user_msg = if err.contains("Internal error")
                || err.contains("Non-manifold")
                || err.contains("panicked")
            {
                format!(
                    "⚠️ {err}\n\nThis is likely a bug. Please report it with the code that caused it so we can fix it."
                )
            } else {
                format!("Compilation error: {err}")
            };
            chat_state.messages.push(ChatMessage {
                role: "assistant".into(),
                content: user_msg,
                thinking: None,
                images: Vec::new(),
                auto_generated: true,
                is_error: true,
            });

            // Feed broken AI output back to the active verification loop.
            if is_ai_error {
                chat_state.verification = VerificationState::ErrorRecovery(err);
            }
        }
        CompilationResult::Canceled => {
            // Manual compilation failures stop any stale verification state.
            if chat_state.verification == VerificationState::WaitingForCompilation {
                chat_state.verification = VerificationState::Idle;
            }
        }
    }
}
