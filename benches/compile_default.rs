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

fn time_phases(code: &str) -> PhaseTimes {
    let start = Instant::now();
    let source = openscad_rs::parse(code).expect("default scene must parse");
    let parse = start.elapsed();

    let start = Instant::now();
    let mut evaluator = Evaluator::new();
    evaluator.variables.insert(
        "$fn".into(),
        synaps_cad::compiler::evaluator::value::Value::Number(16.0),
    );
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

fn compile_once(code: &str) -> usize {
    match compile_scad_code(black_box(code), 16, None) {
        CompilationResult::Success {
            parts,
            views,
            warnings,
        } => {
            assert!(warnings.is_empty(), "default scene warnings: {warnings:?}");
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
    let code = DEFAULT_SCAD_CODE.replace("$view = \"all\";", &format!("$view = \"{scene}\";"));
    let iterations = std::env::var("SYNAPS_BENCH_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(10);

    let warm_checksum = compile_once(&code);
    let start = Instant::now();
    let mut checksum = 0;
    for _ in 0..iterations {
        checksum ^= black_box(compile_once(&code));
    }
    let elapsed = start.elapsed();
    eprintln!(
        "[SynapsCAD] compile_default: scene={scene} iterations={iterations} elapsed={elapsed:?} per_iteration={:?} checksum={} warm_checksum={warm_checksum}",
        elapsed / iterations,
        black_box(checksum),
    );
    let phases = time_phases(&code);
    eprintln!(
        "[SynapsCAD] phase_times: parse={:?} evaluate={:?} convert={:?} render={:?}",
        phases.parse, phases.evaluate, phases.convert, phases.render
    );

    #[cfg(feature = "dispatch-trace")]
    {
        hyperreal::dispatch_trace::reset();
        let checksum = hyperreal::dispatch_trace::with_recording(|| compile_once(&code));
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
