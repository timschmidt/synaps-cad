use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub mod evaluator;
pub mod geometry;
pub mod rendering;
pub mod types;

pub use evaluator::Evaluator;
pub use rendering::render_orthographic_views;
pub use types::{CompilationResult, MeshData, ViewImage};

/// Full compilation pipeline: parse → evaluate → mesh conversion → rendering.
#[must_use]
pub fn compile_scad_code(
    code: &str,
    fn_override: u32,
    cancel: Option<Arc<AtomicBool>>,
) -> CompilationResult {
    let source_file = match openscad_rs::parse(code) {
        Ok(f) => f,
        Err(e) => return CompilationResult::Error(format!("Parse error: {e}")),
    };

    let mut evaluator = Evaluator::new();
    evaluator.cancel.clone_from(&cancel);

    if fn_override > 0 {
        evaluator.variables.insert(
            "$fn".into(),
            evaluator::value::Value::Number(f64::from(fn_override)),
        );
    }
    let shapes = evaluator.eval_source_file(&source_file);

    if evaluator.is_canceled() {
        return CompilationResult::Canceled;
    }

    let mut parts = Vec::new();
    for (shape, color) in shapes {
        if cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
            return CompilationResult::Canceled;
        }
        let mut mesh_data = match shape {
            geometry::Shape::Mesh3D(bmesh) => {
                match geometry::conversions::bmesh_to_mesh_data(&bmesh) {
                    Ok(m) => m,
                    Err(_) => continue,
                }
            }
            geometry::Shape::Sketch2D(sketch) => {
                // 2D shapes that weren't extruded are rendered as thin 3D meshes
                match geometry::conversions::csg_mesh_to_mesh_data(&sketch.extrude(0.01)) {
                    Ok(m) => m,
                    Err(_) => continue,
                }
            }
            geometry::Shape::FallbackMesh(csg) => {
                match geometry::conversions::csg_mesh_to_mesh_data(&csg) {
                    Ok(m) => m,
                    Err(_) => continue,
                }
            }
            geometry::Shape::Failed(e) => {
                evaluator.warnings.push(format!("Geometry failed: {e}"));
                continue;
            }
        };
        mesh_data.color = color;
        parts.push(mesh_data);
    }

    let views = render_orthographic_views(&parts);

    CompilationResult::Success {
        parts,
        views,
        warnings: evaluator.warnings,
    }
}

/// Lightweight compilation that only produces orthographic views (skips full mesh generation if possible).
///
/// # Errors
/// Returns an error string if compilation fails.
pub fn compile_views_only(code: &str) -> Result<Vec<ViewImage>, String> {
    match compile_scad_code(code, 0, None) {
        CompilationResult::Success { views, .. } => Ok(views),
        CompilationResult::Canceled => Err("Compilation canceled".into()),
        CompilationResult::Error(e) => Err(e),
    }
}
