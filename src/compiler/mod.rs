use csgrs::Real;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn to_real(value: f64) -> Real {
    Real::try_from(value).ok().unwrap_or_else(Real::zero)
}

pub mod evaluator;
pub mod geometry;
pub mod rendering;
pub mod types;

pub use evaluator::Evaluator;
pub use rendering::render_orthographic_views;
pub use types::{CompilationResult, MeshData, ViewImage};

/// `OpenSCAD` scene loaded by a fresh `SynapsCAD` workspace.
pub const DEFAULT_SCAD_CODE: &str = r#"// Welcome to SynapsCAD!
// Switch $view to render a different model.
$view = "all";

// Snowman
module view_snowman() {
    color("white") sphere(r = 12);
    color("white") translate([0, 0, 16]) sphere(r = 9);
    color("white") translate([0, 0, 27]) sphere(r = 6);
    color("orange")
        translate([0, -4, 27])
            rotate([90, 0, 0])
                cylinder(h = 8, r1 = 1.5, r2 = 0);
}

// Rocket
module view_rocket() {
    color("silver") cylinder(h = 40, r = 8);
    color("red")
        translate([0, 0, 40])
            cylinder(h = 15, r1 = 8, r2 = 0);
    color("darkgray")
        for (a = [0, 120, 240])
            rotate([0, 0, a])
                translate([6, -1, 0])
                    cube([8, 2, 12]);
}

// Castle
module view_castle() {
    color("sandybrown") difference() {
        cube([40, 40, 20], center = true);
        cube([34, 34, 21], center = true);
    }
    color("tan")
        for (x = [-18, 18])
            for (y = [-18, 18])
                translate([x, y, 0]) {
                    cylinder(h = 28, r = 5);
                    color("red") translate([0, 0, 28]) cylinder(h = 12, r1 = 6, r2 = 0);
                }
}

// Exact Hyperreal geometry
module view_exact() {
    // Repeating rational dimensions are retained exactly rather than rounded.
    color("deepskyblue")
        cube([exact("20/3"), exact("25/7"), exact("13/2")]);
    // Symbolic pi remains exact through placement and sphere construction.
    color("gold")
        translate([exact("40/3"), 0, exact("pi")])
            sphere(r = exact("pi"));
}

// Combined scene
module view_all() {
    view_snowman();
    translate([50, 0, 0]) view_rocket();
    translate([0, 60, 0]) view_castle();
    translate([60, 60, 0]) view_exact();
}

// View selector
if ($view == "snowman") view_snowman();
if ($view == "rocket") view_rocket();
if ($view == "castle") view_castle();
if ($view == "exact") view_exact();
if ($view == "all") view_all();
"#;

/// Full compilation pipeline: parse → evaluate → mesh conversion → rendering.
#[must_use]
#[allow(clippy::needless_pass_by_value)]
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
            geometry::Shape::Mesh3D(mesh) => {
                match geometry::conversions::csg_mesh_to_mesh_data(&mesh) {
                    Ok(m) => m,
                    Err(error) => {
                        evaluator
                            .warnings
                            .push(format!("Mesh conversion failed: {error}"));
                        continue;
                    }
                }
            }
            geometry::Shape::Sketch2D(sketch) => {
                // Preview bare 2D shapes as thin solids.
                match geometry::conversions::csg_mesh_to_mesh_data(
                    &sketch.extrude(to_real(0.01), ()),
                ) {
                    Ok(m) => m,
                    Err(error) => {
                        evaluator
                            .warnings
                            .push(format!("2D preview conversion failed: {error}"));
                        continue;
                    }
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

/// Compiles `code` and returns only its rendered orthographic views.
///
/// # Errors
///
/// Returns an error string when parsing fails or compilation is canceled.
/// Recoverable evaluation and mesh warnings remain available only through
/// [`compile_scad_code`].
pub fn compile_views_only(code: &str) -> Result<Vec<ViewImage>, String> {
    match compile_scad_code(code, 0, None) {
        CompilationResult::Success { views, .. } => Ok(views),
        CompilationResult::Canceled => Err("Compilation canceled".into()),
        CompilationResult::Error(e) => Err(e),
    }
}
