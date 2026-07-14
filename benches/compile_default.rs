use std::hint::black_box;
use std::time::{Duration, Instant};

use csgrs::Real;
use synaps_cad::compiler::geometry::Shape;
use synaps_cad::compiler::{
    CompilationResult, DEFAULT_SCAD_CODE, Evaluator, compile_scad_code, render_orthographic_views,
};

struct PhaseTimes {
    parse: Duration,
    evaluate: Duration,
    convert: Duration,
    render: Duration,
}

fn time_phases(code: &str, fn_override: u32) -> PhaseTimes {
    let start = Instant::now();
    let source = openscad_rs::parse(code).expect("default scene must parse");
    let parse = start.elapsed();

    let start = Instant::now();
    let mut evaluator = Evaluator::new();
    if fn_override > 0 {
        evaluator.variables.insert(
            "$fn".into(),
            synaps_cad::compiler::evaluator::value::Value::Number(f64::from(fn_override)),
        );
    }
    let shapes = evaluator.eval_source_file(&source);
    let evaluate = start.elapsed();

    let start = Instant::now();
    let parts = shapes
        .into_iter()
        .filter_map(|(shape, _)| match shape {
            Shape::Mesh3D(mesh) => {
                synaps_cad::compiler::geometry::conversions::csg_mesh_to_mesh_data(&mesh).ok()
            }
            Shape::Sketch2D(sketch) => {
                synaps_cad::compiler::geometry::conversions::csg_mesh_to_mesh_data(&sketch.extrude(
                    Real::try_from(0.01).expect("finite benchmark thickness"),
                    (),
                ))
                .ok()
            }
            Shape::Failed(_) => None,
        })
        .collect::<Vec<_>>();
    let convert = start.elapsed();

    let start = Instant::now();
    black_box(render_orthographic_views(&parts));
    let render = start.elapsed();

    PhaseTimes {
        parse,
        evaluate,
        convert,
        render,
    }
}

fn compile_once(code: &str, fn_override: u32, require_no_warnings: bool) -> usize {
    match compile_scad_code(black_box(code), fn_override, None) {
        CompilationResult::Success {
            parts,
            views,
            warnings,
        } => {
            if require_no_warnings {
                assert!(warnings.is_empty(), "default scene warnings: {warnings:?}");
            }
            parts.iter().map(|part| part.indices.len()).sum::<usize>()
                + views
                    .iter()
                    .map(|view| view.base64_png.len())
                    .sum::<usize>()
        }
        CompilationResult::Canceled => panic!("default scene compilation was canceled"),
        CompilationResult::Error(error) => panic!("default scene compilation failed: {error}"),
    }
}

fn main() {
    let scene = std::env::var("SYNAPS_BENCH_SCENE").unwrap_or_else(|_| "all".into());
    let (label, code) = std::env::var("SYNAPS_BENCH_FILE").map_or_else(
        |_| {
            (
                format!("default:{scene}"),
                DEFAULT_SCAD_CODE.replace("$view = \"all\";", &format!("$view = \"{scene}\";")),
            )
        },
        |path| {
            let code = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read benchmark fixture {path}: {error}"));
            (path, code)
        },
    );
    let fn_override = std::env::var("SYNAPS_BENCH_FN_OVERRIDE")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or_else(|| if label.starts_with("default:") { 16 } else { 0 });
    let require_no_warnings = label.starts_with("default:");
    let iterations = std::env::var("SYNAPS_BENCH_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(10);
    if std::env::var_os("SYNAPS_BENCH_ONCE").is_some() {
        let start = Instant::now();
        let checksum = compile_once(&code, fn_override, require_no_warnings);
        eprintln!(
            "[SynapsCAD] compile_once: workload={label} fn_override={fn_override} elapsed={:?} checksum={checksum}",
            start.elapsed()
        );
        return;
    }

    if iterations > 0 {
        let warm_checksum = compile_once(&code, fn_override, require_no_warnings);
        let start = Instant::now();
        let mut checksum = 0;
        for _ in 0..iterations {
            checksum ^= black_box(compile_once(&code, fn_override, require_no_warnings));
        }
        let elapsed = start.elapsed();
        eprintln!(
            "[SynapsCAD] compile_default: workload={label} fn_override={fn_override} iterations={iterations} elapsed={elapsed:?} per_iteration={:?} checksum={} warm_checksum={warm_checksum}",
            elapsed / iterations,
            black_box(checksum),
        );
        let phases = time_phases(&code, fn_override);
        eprintln!(
            "[SynapsCAD] phase_times: parse={:?} evaluate={:?} convert={:?} render={:?}",
            phases.parse, phases.evaluate, phases.convert, phases.render
        );
    }

    #[cfg(feature = "dispatch-trace")]
    {
        hyperreal::dispatch_trace::reset();
        let checksum = hyperreal::dispatch_trace::with_recording(|| {
            compile_once(&code, fn_override, require_no_warnings)
        });
        let trace = hyperreal::dispatch_trace::take_trace();
        eprintln!(
            "[SynapsCAD] dispatch_trace: checksum={checksum} correlation={:?}",
            trace.correlation_summary()
        );
        for summary in &trace.dispatch {
            eprintln!(
                "[SynapsCAD] {}/{}/{}: {}",
                summary.layer, summary.operation, summary.path, summary.count
            );
        }
    }
}
