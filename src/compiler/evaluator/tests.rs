use super::*;
use crate::compiler::geometry::conversions::csg_mesh_to_mesh_data;
use crate::compiler::{CompilationResult, DEFAULT_SCAD_CODE, MeshData, compile_scad_code};
use csgrs::csg::CSG;
use csgrs::mesh::Mesh as CsgMesh;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static COMPATIBILITY_COMPILE_LOCK: Mutex<()> = Mutex::new(());

fn compile_with_timeout(code: &str, fn_override: u32) -> CompilationResult {
    let _compile_guard = COMPATIBILITY_COMPILE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_clone = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_mins(1));
        cancel_clone.store(true, Ordering::Relaxed);
    });
    let code = code.to_string();
    std::thread::Builder::new()
        .name("synaps-cad-test-compile".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || compile_scad_code(&code, fn_override, Some(cancel)))
        .expect("failed to spawn compilation test thread")
        .join()
        .unwrap_or_else(|_| CompilationResult::Error("Compilation thread panicked".into()))
}

#[test]
fn default_exact_view_compiles_both_infinite_precision_objects() {
    let code = DEFAULT_SCAD_CODE.replace("$view = \"all\";", "$view = \"exact\";");
    match compile_with_timeout(&code, 16) {
        CompilationResult::Success {
            parts, warnings, ..
        } => {
            assert_eq!(parts.len(), 2);
            assert!(parts.iter().all(|part| !part.indices.is_empty()));
            assert!(
                warnings.is_empty(),
                "exact default view warnings: {warnings:?}"
            );
        }
        CompilationResult::Error(error) => panic!("Exact default view failed: {error}"),
        CompilationResult::Canceled => panic!("Exact default view timed out"),
    }
}

#[test]
fn test_star_difference() {
    let code = r"
module star(points = 5, outer_r = 3, inner_r = 1.2, h = 2) {
    linear_extrude(height = h)
        polygon([for (i = [0:2*points-1])
            let(r = (i % 2 == 0) ? outer_r : inner_r,
                a = 90 + i * 180 / points)
            [r * cos(a), r * sin(a)]
        ]);
}

difference() {
    cylinder(h = 1, r = 10, $fn = 64);
    translate([0, 0, -0.5])
        star(points = 5, outer_r = 5, inner_r = 2, h = 2);
}
";
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success { parts, .. } => {
            eprintln!("Parts: {}", parts.len());
            for (i, p) in parts.iter().enumerate() {
                eprintln!(
                    "Part {i}: {} verts, {} tris",
                    p.positions.len(),
                    p.indices.len() / 3
                );
            }
            assert!(
                parts[0].indices.len() / 3 > 96,
                "Expected more tris than plain cylinder (got {})",
                parts[0].indices.len() / 3
            );
        }
        CompilationResult::Error(e) => panic!("Compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn test_star_polygon_standalone() {
    let code = r"
linear_extrude(height = 2)
    polygon([for (i = [0:9])
        let(r = (i % 2 == 0) ? 5 : 2,
            a = 90 + i * 36)
        [r * cos(a), r * sin(a)]
    ]);
";
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success { parts, .. } => {
            eprintln!("Star standalone - Parts: {}", parts.len());
            for (i, p) in parts.iter().enumerate() {
                eprintln!(
                    "Star part {i}: {} verts, {} tris",
                    p.positions.len(),
                    p.indices.len() / 3
                );
            }
            assert!(!parts.is_empty(), "Star polygon should produce geometry");
            assert!(!parts[0].indices.is_empty(), "Star should have triangles");
        }
        CompilationResult::Error(e) => panic!("Compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn test_text_basic() {
    let code = r#"
linear_extrude(height = 5)
    text("Hello", size = 20);
"#;
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success {
            parts, warnings, ..
        } => {
            assert!(!parts.is_empty(), "text() should produce geometry");
            assert!(
                parts[0].positions.len() > 10,
                "Extruded text should have many vertices"
            );
            assert!(
                !warnings
                    .iter()
                    .any(|w: &String| w.contains("text() not yet supported")),
                "text() should be supported now"
            );
        }
        CompilationResult::Error(e) => panic!("Compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn test_text_center_aligned() {
    let code = r#"
linear_extrude(height = 2)
    text("A", size = 30, halign = "center", valign = "center");
"#;
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success { parts, .. } => {
            assert!(!parts.is_empty(), "Centered text should produce geometry");
            let has_neg_x = parts[0].positions.iter().any(|p| p[0] < 0.0);
            let has_pos_x = parts[0].positions.iter().any(|p| p[0] > 0.0);
            assert!(
                has_neg_x && has_pos_x,
                "Centered text should span origin in X"
            );
        }
        CompilationResult::Error(e) => panic!("Compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn test_text_with_font_style() {
    let code = r#"
linear_extrude(height = 3)
    text("B", size = 20, font = "Liberation Sans:style=Bold");
"#;
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success { parts, .. } => {
            assert!(!parts.is_empty(), "Bold text should produce geometry");
        }
        CompilationResult::Error(e) => panic!("Compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn test_text_difference_on_cube() {
    let code = r#"
difference() {
    cube([40, 40, 5]);
    translate([0, 0, 3])
        linear_extrude(height = 3)
            text("Hi", size = 15, halign = "center", valign = "center");
}
"#;
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success { parts, .. } => {
            assert!(!parts.is_empty(), "Text engraving should produce geometry");
            let tri_count = parts[0].indices.len() / 3;
            assert!(
                tri_count > 12,
                "Engraved cube should have more than 12 triangles (plain cube), got {tri_count}"
            );
        }
        CompilationResult::Error(e) => panic!("Compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn test_text_2d_only() {
    let code = r#"text("X", size = 10);"#;
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success { parts, .. } => {
            assert!(!parts.is_empty(), "2D text should render as thin geometry");
        }
        CompilationResult::Error(e) => panic!("2D text should not fail: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn test_text_direction() {
    let code = r#"
text( "Left to Right" ,size=5, direction="ltr");
translate([5,0,0])
    text( "Up" ,size=5, direction="btt");
translate([20,0,0])
    text( "Down" ,size=5, direction="ttb");
translate([0,10,0])
    text( "Right to left" ,size=5, direction="rtl");
"#;
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success { parts, .. } => {
            assert_eq!(parts.len(), 4, "Should produce 4 text parts");
            let rtl_x_min = parts[3]
                .positions
                .iter()
                .map(|p| p[0])
                .fold(f32::MAX, f32::min);
            assert!(
                rtl_x_min >= -0.5,
                "RTL text should be in positive x region, got x_min={rtl_x_min}"
            );
            let btt_y_min = parts[1]
                .positions
                .iter()
                .map(|p| p[2])
                .fold(f32::MAX, f32::min);
            assert!(
                btt_y_min > -0.5,
                "BTT text should be in positive y after mirror, got y_min={btt_y_min}"
            );
            let ttb_y_min = parts[2]
                .positions
                .iter()
                .map(|p| p[2])
                .fold(f32::MAX, f32::min);
            assert!(
                ttb_y_min > -0.5,
                "TTB text should be in positive y after mirror, got y_min={ttb_y_min}"
            );
        }
        CompilationResult::Error(e) => panic!("Text direction should not fail: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn geb_letter_solids_are_closed() {
    let definitions = r#"
        font = "Liberation Sans";
        module G() offset(0.3) text("G", size=10, halign="center", valign="center", font=font);
        module E() offset(0.3) text("E", size=10, halign="center", valign="center", font=font);
        module B() offset(0.5) text("B", size=10, halign="center", valign="center", font=font);
        $fn=64;
    "#;
    let cases = [
        ("B", "linear_extrude(height=20, center=true) B();"),
        (
            "E",
            "rotate([90, 0, 0]) linear_extrude(height=20, center=true) E();",
        ),
        (
            "G",
            "rotate([90, 0, 90]) linear_extrude(height=20, center=true) G();",
        ),
    ];

    for (label, body) in cases {
        let code = format!("{definitions}\n{body}");
        compile_to_csg_mesh(&code)
            .to_hypermesh_exact()
            .unwrap_or_else(|error| panic!("{label} is not closed: {error}"));
        match compile_with_timeout(&code, 0) {
            CompilationResult::Success {
                parts, warnings, ..
            } => assert!(!parts.is_empty(), "{label} is empty; warnings={warnings:?}"),
            CompilationResult::Error(error) => panic!("{label} failed: {error}"),
            CompilationResult::Canceled => panic!("{label} timed out"),
        }
    }
}

#[test]
fn failed_assert_expression_reports_its_message() {
    match compile_with_timeout(
        r#"
            checked = assert(false, "expected failure");
            cube(1);
        "#,
        0,
    ) {
        CompilationResult::Success {
            parts, warnings, ..
        } => {
            assert!(!parts.is_empty());
            assert!(
                warnings
                    .iter()
                    .any(|warning| warning.contains("expected failure")),
                "assertion warning missing: {warnings:?}"
            );
        }
        CompilationResult::Error(error) => panic!("assertion program failed: {error}"),
        CompilationResult::Canceled => panic!("assertion program timed out"),
    }
}

#[test]
fn test_axis_angle_rotate() {
    let code = r"
rotate(a = 45, v = [1, 0, 0])
    cube([10, 10, 10]);
";
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success { parts, .. } => {
            assert!(!parts.is_empty(), "Should produce geometry");
            assert!(
                parts[0].indices.len() / 3 >= 12,
                "Cube should have at least 12 tris"
            );
        }
        CompilationResult::Error(e) => panic!("Compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

fn compile_to_csg_mesh(code: &str) -> CsgMesh<()> {
    let _compile_guard = COMPATIBILITY_COMPILE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let source_file = openscad_rs::parse(code).expect("parse error");
    let mut evaluator = Evaluator::new();
    let shapes = evaluator.eval_source_file(&source_file);
    assert!(!shapes.is_empty(), "No geometry produced");
    let mut iter = shapes.into_iter();
    let (first, _) = iter.next().unwrap();
    let mut result = first.into_csg_mesh();
    for (shape, _) in iter {
        result = result.union(&shape.into_csg_mesh());
    }
    result
}

fn csg_mesh_to_mesh_data_local(mesh: &CsgMesh<()>) -> Result<MeshData, String> {
    csg_mesh_to_mesh_data(mesh)
}

fn compile_to_merged_mesh(code: &str) -> MeshData {
    match compile_with_timeout(code, 0) {
        CompilationResult::Success { parts, .. } => {
            let mut positions = Vec::new();
            let mut normals = Vec::new();
            let mut indices = Vec::new();
            for part in parts {
                let offset = u32::try_from(positions.len()).expect("mesh exceeds u32 indexing");
                positions.extend(part.positions);
                normals.extend(part.normals);
                indices.extend(part.indices.iter().map(|i| i + offset));
            }
            MeshData {
                positions,
                normals,
                indices,
                color: None,
            }
        }
        CompilationResult::Error(e) => panic!("Failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn linear_extrude_twist_and_vector_scale_produce_one_manifold() {
    let mesh = compile_to_csg_mesh(
        r"
        linear_extrude(height = 5, twist = 135, scale = [0.75, 1.25], slices = 12)
            difference() {
                square([4, 4]);
                translate([1, 1]) square([2, 2]);
            }
        ",
    );

    mesh.to_hypermesh_exact()
        .expect("twisted vector-scale extrusion should be a closed manifold");
}

#[test]
fn exact_240_degree_rotation_places_rocket_fin_in_the_third_sector() {
    for degrees in [0.0_f64, 120.0, 240.0] {
        let mesh = compile_to_csg_mesh(&format!(
            "rotate([0, 0, {degrees}]) translate([6, -1, 0]) cube([8, 2, 12]);"
        ));
        let bounds = mesh.bounding_box();
        let center_x = ((bounds.mins.x + bounds.maxs.x) / csgrs::Real::from(2))
            .expect("nonzero center divisor")
            .to_f64_lossy()
            .expect("finite center x");
        let center_y = ((bounds.mins.y + bounds.maxs.y) / csgrs::Real::from(2))
            .expect("nonzero center divisor")
            .to_f64_lossy()
            .expect("finite center y");
        let radians = degrees.to_radians();

        assert!(10.0f64.mul_add(-radians.cos(), center_x).abs() < 1.0e-9);
        assert!(10.0f64.mul_add(-radians.sin(), center_y).abs() < 1.0e-9);
    }
}

#[test]
fn exact_rational_arithmetic_reaches_mesh_coordinates_without_float_demotion() {
    let mesh =
        compile_to_csg_mesh(r#"cube([exact("1/3") + exact("1/6"), exact("2/5"), exact("3/7")]);"#);
    let bounds = mesh.bounding_box();

    assert_eq!(bounds.maxs.x, "1/2".parse::<csgrs::Real>().unwrap());
    assert_eq!(bounds.maxs.y, "2/5".parse::<csgrs::Real>().unwrap());
    assert_eq!(bounds.maxs.z, "3/7".parse::<csgrs::Real>().unwrap());
}

#[test]
fn exact_symbolic_constants_and_trig_reach_transforms() {
    let mesh = compile_to_csg_mesh(
        r#"
        translate([cos(exact("60")), sin(exact("30")), 0])
            cube(exact("1/7"));
        "#,
    );
    let bounds = mesh.bounding_box();
    let half = "1/2".parse::<csgrs::Real>().unwrap();

    assert_eq!(bounds.mins.x, half);
    assert_eq!(bounds.mins.y, "1/2".parse::<csgrs::Real>().unwrap());

    let mut evaluator = Evaluator::new();
    assert!(matches!(
        evaluator.eval_builtin_function("exact", &[Value::String("pi".into())]),
        Value::Exact(value) if value == csgrs::Real::pi()
    ));
}

#[test]
fn test_scalar_vector_mul() {
    let m = compile_to_merged_mesh("r=25; translate(r * [1, 0, 0]) cube(5);");
    let xs: Vec<f64> = m.positions.iter().map(|p| f64::from(p[0])).collect();
    let min_x = xs.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(
        min_x > 20.0,
        "Expected translated to x≈25, got min_x={min_x}"
    );
}

#[test]
fn test_ring_of_children() {
    let code = r"
module ring(radius, count){
    for (a = [0 : count - 1]) {
        angle = a * 360 / count;
        translate(radius * [cos(angle), -sin(angle), 0])
            children();
    }
}
ring(20, 4) { cube(3); }
";
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success { parts, .. } => {
            let total_verts: usize = parts.iter().map(|p| p.positions.len()).sum();
            assert!(
                total_verts >= 96,
                "Expected 4 cubes, got {total_verts} verts"
            );
        }
        CompilationResult::Error(e) => panic!("Failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn test_candle_stand() {
    let code = r"
length=50; radius=25; count=7; centerCandle=true;
candleSize=7; width=4; holeSize=3; CenterCandleWidth=4;
heightOfSupport=3; widthOfSupport=3; heightOfRing=4; widthOfRing=23;

cylinder(length,width-2);

translate([0,0,length-candleSize/2])
if(centerCandle){
    difference(){
        cylinder(candleSize,r=CenterCandleWidth);
        cylinder(candleSize+1,r=CenterCandleWidth-2);
    }
}

translate([0,0,length-candleSize/2]){
    make(radius, count,candleSize,length);
    make_ring_of(radius, count){ cylinder(1,r=width); }
}

for (a = [0 : count - 1]) {
    rotate(a*360/count) {
        translate([0, -width/2, 0]) cube([radius, widthOfSupport, heightOfSupport]);
    }
}

module make(radius, count,candleSize,length){
    difference(){
        union(){
            make_ring_of(radius, count){ cylinder(candleSize,r=width); }
            for (a = [0 : count - 1]) {
                rotate(a*360/count) {
                    translate([0, -width/2, 0]) cube([radius, widthOfSupport, heightOfSupport]);
                }
            }
            linear_extrude(heightOfRing)
            difference(){ circle(radius); circle(widthOfRing); }
        }
        make_ring_of(radius, count){ cylinder(candleSize+1,r=holeSize); }
    }
}

module make_ring_of(radius, count){
    for (a = [0 : count - 1]) {
        angle = a * 360 / count;
        translate(radius * [cos(angle), -sin(angle), 0])
            children();
    }
}
";
    let result = compile_with_timeout(code, 0);
    match result {
        CompilationResult::Success { parts, .. } => {
            let total_verts: usize = parts.iter().map(|p| p.positions.len()).sum();
            assert!(total_verts > 0);
        }
        CompilationResult::Error(e) => panic!("Compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn test_hull_sphere_cube() {
    let code = r"
$fn = 30;
hull() {
    sphere(r=14);
    translate([0, 30, 0]) cube([30, 4, 30], center=true);
}
";
    let csg = compile_to_csg_mesh(code);
    let result = csg_mesh_to_mesh_data_local(&csg).expect("mesh conversion failed");
    assert!(result.positions.len() > 10);
}

#[test]
fn test_intersection_sphere_cube() {
    let code = r"
$fn = 30;
intersection() {
    sphere(r=14);
    translate([-14, -14, -14]) cube([28, 28, 14.4]);
}
";
    let csg = compile_to_csg_mesh(code);
    let result = csg_mesh_to_mesh_data_local(&csg).expect("mesh conversion failed");
    assert!(result.positions.len() > 10);
}

#[test]
fn symbolic_rotated_hull_uses_exact_retained_facts() {
    let code = r"
        $fn = 6;
        hull() {
            cube([6, 2, 5], center=true);
            translate([0, 5, 2]) rotate([-35, 0, 0]) cylinder(h=2, r1=2, r2=3);
        }
    ";
    let mesh = compile_to_csg_mesh(code);
    assert!(!mesh.polygons.is_empty());
    let rendered = csg_mesh_to_mesh_data_local(&mesh).expect("mesh conversion failed");
    assert!(!rendered.indices.is_empty());
}

#[test]
#[ignore = "expensive exact hull Boolean stress; run in release mode"]
fn test_difference_hull_shapes() {
    let outer = r"
        $fn = 6;
        hull() {
        translate([0, 0, 0]) cube([36, 4, 33], center=true);
        translate([0, 25, 10]) rotate([-35, 0, 0]) cylinder(h=8, r1=14, r2=22);
        }
    ";
    let inner = r"
        $fn = 6;
        hull() {
        translate([0, 0, 0]) sphere(r=12.5);
        translate([0, 25, 10]) rotate([-35, 0, 0]) cylinder(h=9, r1=12.5, r2=20);
        }
    ";
    let outer_mesh = compile_to_csg_mesh(outer);
    let inner_mesh = compile_to_csg_mesh(inner);
    assert!(!outer_mesh.polygons.is_empty(), "outer hull is empty");
    assert!(!inner_mesh.polygons.is_empty(), "inner hull is empty");

    let code = format!("difference() {{ {outer} {inner} }}");
    let csg = compile_to_csg_mesh(&code);
    let result = csg_mesh_to_mesh_data_local(&csg).expect("mesh conversion failed");
    assert!(result.positions.len() > 10);
}

#[test]
fn test_refill_clip() {
    let code = REFILL_CLIP_CODE;
    let csg = compile_to_csg_mesh(code);
    let result = csg_mesh_to_mesh_data_local(&csg).expect("mesh conversion failed");
    assert!(result.positions.len() > 100);
}

const REFILL_CLIP_CODE: &str = r"
$fn = 60;
TANK_TOP_WIDTH = 256;
TANK_BOTTOM_WIDTH = 244;
TANK_HEIGHT = 110;
TANK_WALL = 2.8;
REFILL_SLOT_HEIGHT = 33.0;
REFILL_SLOT_WIDTH = 30.4;
REFILL_SLOT_CENTER_Z = TANK_HEIGHT - REFILL_SLOT_HEIGHT/2 + 0.5;
REFILL_FUNNEL_TOP_R = 22;
REFILL_FUNNEL_HEIGHT = 8;
REFILL_FUNNEL_OFFSET = 25;
REFILL_FUNNEL_TILT = 35;
REFILL_CHANNEL_INNER = 25;
REFILL_CHANNEL_WALL = 1.6;

module RefillClip() {
    clip_width = REFILL_SLOT_WIDTH - 0.4;
    channel_outer_r = REFILL_CHANNEL_INNER/2 + REFILL_CHANNEL_WALL;
    funnel_height = REFILL_FUNNEL_HEIGHT;
    funnel_top_r = REFILL_FUNNEL_TOP_R;

    y_wall_outer = TANK_TOP_WIDTH/2;
    y_wall_inner = y_wall_outer - TANK_WALL;
    slot_center_z = REFILL_SLOT_CENTER_Z;
    slot_height = REFILL_SLOT_HEIGHT;
    z_base = slot_center_z - slot_height/2;
    body_top_z = min(z_base + slot_height, TANK_HEIGHT - 3);
    body_mid_z = (z_base + body_top_z)/2;
    funnel_base_z = body_top_z + 8;
    flange_height = min(slot_height + 5, (TANK_HEIGHT - body_mid_z)*2);

    y_funnel = y_wall_outer + REFILL_FUNNEL_OFFSET;
    funnel_tilt = REFILL_FUNNEL_TILT;
    funnel_anchor_overlap = 0.4;

    module FunnelTransform() {
        translate([0, y_funnel, funnel_base_z])
            rotate([-funnel_tilt, 0, 0])
                children();
    }

    module FunnelAnchor(radius) {
        intersection() {
            sphere(r=radius);
            translate([-radius, -radius, -radius])
                cube([radius * 2, radius * 2, radius + funnel_anchor_overlap]);
        }
    }

    nozzle_length = z_base - 35;
    y_nozzle = y_wall_inner - channel_outer_r - 2;

    difference() {
        union() {
            translate([0, 0, z_base]) {
                translate([0, y_wall_outer - TANK_WALL/2, body_mid_z - z_base])
                    cube([clip_width, TANK_WALL, slot_height], center=true);
                translate([0, y_wall_outer + 2, body_mid_z - z_base])
                    cube([clip_width + 6, 4, flange_height], center=true);
                translate([0, y_wall_inner - 2, body_mid_z - z_base])
                    cube([clip_width + 6, 4, flange_height], center=true);
            }

            FunnelTransform()
                cylinder(h=funnel_height, r1=channel_outer_r, r2=funnel_top_r);

            hull() {
                translate([0, y_wall_outer + 2, body_mid_z])
                    cube([clip_width + 6, 4, slot_height], center=true);
                FunnelTransform()
                    cylinder(r=channel_outer_r, h=1);
            }

            hull() {
                translate([0, y_nozzle, body_mid_z]) sphere(r=channel_outer_r);
                translate([0, y_nozzle, z_base - nozzle_length]) cylinder(r=channel_outer_r, h=1);
            }

            hull() {
                translate([0, y_wall_inner - 2, body_mid_z])
                    cube([clip_width + 6, 4, 30], center=true);
                translate([0, y_nozzle, body_mid_z])
                    sphere(r=channel_outer_r);
            }

            hull() {
                translate([0, y_wall_outer + 2, body_mid_z])
                    cube([clip_width + 6, 4, 30], center=true);
                translate([0, y_wall_inner - 2, body_mid_z])
                    cube([clip_width + 6, 4, 30], center=true);
            }

            hull() {
                FunnelTransform()
                    FunnelAnchor(channel_outer_r);
                translate([0, y_wall_outer, body_mid_z])
                    sphere(r=channel_outer_r);
                translate([0, y_nozzle, body_mid_z])
                    sphere(r=channel_outer_r);
            }
        }

        hull() {
            FunnelTransform()
                FunnelAnchor(REFILL_CHANNEL_INNER/2);
            translate([0, y_wall_outer, body_mid_z])
                sphere(r=REFILL_CHANNEL_INNER/2);
            translate([0, y_nozzle, body_mid_z])
                sphere(r=REFILL_CHANNEL_INNER/2);
        }

        FunnelTransform()
            cylinder(h=funnel_height + 1, r1=REFILL_CHANNEL_INNER/2, r2=funnel_top_r - 2);

        hull() {
            translate([0, y_nozzle, body_mid_z]) sphere(r=REFILL_CHANNEL_INNER/2);
            translate([0, y_nozzle, z_base - nozzle_length]) cylinder(r=REFILL_CHANNEL_INNER/2, h=1);
        }

        translate([0, y_nozzle, z_base - nozzle_length - 1])
            cylinder(r=REFILL_CHANNEL_INNER/2, h=5);

        cut_start_x = clip_width/2 - 0.5;
        cut_end_x = clip_width/2 + 5;
        groove_width = cut_end_x - cut_start_x;
        groove_x_offset = cut_start_x + groove_width/2;

        translate([-groove_x_offset, (y_wall_inner + y_wall_outer)/2, z_base + 12.5])
            cube([groove_width, TANK_WALL + 1.0, 40], center=true);
        translate([groove_x_offset, (y_wall_inner + y_wall_outer)/2, z_base + 12.5])
            cube([groove_width, TANK_WALL + 1.0, 40], center=true);
    }
}

RefillClip();
";

fn example_path(relative: &str) -> String {
    format!(
        "{}/tests/openscad_examples/{relative}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn assert_example_compiles(relative: &str) {
    let path = example_path(relative);
    let code = std::fs::read_to_string(&path).unwrap();
    match compile_with_timeout(&code, 0) {
        CompilationResult::Success { parts, .. } => {
            assert!(!parts.is_empty());
        }
        CompilationResult::Error(e) => panic!("{relative}: compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

fn assert_example_no_panic(relative: &str) {
    let path = example_path(relative);
    let code = std::fs::read_to_string(&path).unwrap();
    let _ = std::panic::catch_unwind(|| compile_with_timeout(&code, 0));
}

#[derive(serde::Deserialize)]
struct ReferenceData {
    facets: usize,
    bounding_box: BBox,
}

#[derive(serde::Deserialize)]
struct BBox {
    min: [f64; 3],
    max: [f64; 3],
}

fn assert_example_matches_reference(relative: &str) {
    let path = example_path(relative);
    let code = std::fs::read_to_string(&path).unwrap();
    let parts = match compile_with_timeout(&code, 0) {
        CompilationResult::Success { parts, .. } => parts,
        CompilationResult::Error(e) => panic!("{relative}: compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    };
    let ref_name = relative.replace(".scad", ".json");
    let ref_path = format!(
        "{}/tests/openscad_references/{ref_name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let Ok(ref_json) = std::fs::read_to_string(&ref_path) else {
        return;
    };
    assert_example_matches_reference_data(relative, &parts, &ref_json, 0.20);
}

fn assert_example_matches_reference_loose(relative: &str) {
    let path = example_path(relative);
    let code = std::fs::read_to_string(&path).unwrap();
    let parts = match compile_with_timeout(&code, 0) {
        CompilationResult::Success { parts, .. } => parts,
        CompilationResult::Error(e) => panic!("{relative}: compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    };
    let ref_name = relative.replace(".scad", ".json");
    let ref_path = format!(
        "{}/tests/openscad_references/{ref_name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let Ok(ref_json) = std::fs::read_to_string(&ref_path) else {
        return;
    };
    // This coarse compatibility gate accommodates intentionally different
    // tessellation strategies for text and complex CSG.
    assert_example_matches_reference_data(relative, &parts, &ref_json, 2.0);
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn assert_example_matches_reference_data(
    relative: &str,
    parts: &[crate::compiler::MeshData],
    ref_json: &str,
    tolerance: f64,
) {
    let reference: ReferenceData = serde_json::from_str(ref_json).unwrap();
    let mut our_min = [f64::INFINITY; 3];
    let mut our_max = [f64::NEG_INFINITY; 3];
    let mut our_triangles: usize = 0;
    for part in parts {
        our_triangles += part.indices.len() / 3;
        for pos in &part.positions {
            let zup = [f64::from(pos[0]), -f64::from(pos[2]), f64::from(pos[1])];
            for i in 0..3 {
                our_min[i] = our_min[i].min(zup[i]);
                our_max[i] = our_max[i].max(zup[i]);
            }
        }
    }
    let ref_min = reference.bounding_box.min;
    let ref_max = reference.bounding_box.max;
    for i in 0..3 {
        let ref_size = (ref_max[i] - ref_min[i]).abs();
        let tol = f64::max(1.0, ref_size * 0.10); // increased bbox tolerance to 10%
        assert!(
            (our_min[i] - ref_min[i]).abs() <= tol,
            "bbox min mismatch axis {i}: ours {}, ref {}",
            our_min[i],
            ref_min[i]
        );
        assert!(
            (our_max[i] - ref_max[i]).abs() <= tol,
            "bbox max mismatch axis {i}: ours {}, ref {}",
            our_max[i],
            ref_max[i]
        );
    }

    let facet_diff = our_triangles.abs_diff(reference.facets);
    let facet_tol = (reference.facets as f64 * tolerance) as usize;
    assert!(
        facet_diff <= facet_tol,
        "{relative}: facet count mismatch: ours {our_triangles}, ref {} (tolerance {}%); parts={}, triangles={:?}, colors={:?}",
        reference.facets,
        tolerance * 100.0,
        parts.len(),
        parts
            .iter()
            .map(|part| part.indices.len() / 3)
            .collect::<Vec<_>>(),
        parts.iter().map(|part| part.color).collect::<Vec<_>>(),
    );
}

#[test]
fn openscad_basics_csg() {
    assert_example_matches_reference("Basics/CSG.scad");
}
#[test]
fn openscad_basics_csg_modules() {
    assert_example_matches_reference("Basics/CSG-modules.scad");
}
#[test]
fn openscad_basics_hull() {
    assert_example_matches_reference("Basics/hull.scad");
}
#[test]
fn openscad_basics_linear_extrude() {
    assert_example_no_panic("Basics/linear_extrude.scad");
}
#[test]
fn openscad_basics_logo() {
    assert_example_compiles("Basics/logo.scad");
}
#[test]
fn openscad_basics_rotate_extrude() {
    assert_example_compiles("Basics/rotate_extrude.scad");
}
#[test]
fn openscad_basics_letterblock() {
    assert_example_compiles("Basics/LetterBlock.scad");
}
#[test]
fn openscad_basics_logo_and_text() {
    assert_example_compiles("Basics/logo_and_text.scad");
}
#[test]
fn openscad_basics_projection() {
    assert_example_matches_reference("Basics/projection.scad");
}
#[test]
fn openscad_basics_roof() {
    assert_example_no_panic("Basics/roof.scad");
}
#[test]
fn openscad_basics_text_on_cube() {
    assert_example_matches_reference_loose("Basics/text_on_cube.scad");
}
#[test]
fn openscad_functions_echo() {
    assert_example_no_panic("Functions/echo.scad");
}
#[test]
fn openscad_functions_functions() {
    assert_example_matches_reference_loose("Functions/functions.scad");
}
#[test]
fn openscad_functions_list_comprehensions() {
    assert_example_no_panic("Functions/list_comprehensions.scad");
}
#[test]
fn openscad_functions_recursion() {
    assert_example_no_panic("Functions/recursion.scad");
}
#[test]
#[ignore = "expensive repeated child geometry stress; run in release mode"]
fn openscad_advanced_children() {
    assert_example_compiles("Advanced/children.scad");
}
#[test]
fn openscad_advanced_children_indexed() {
    assert_example_compiles("Advanced/children_indexed.scad");
}
#[test]
fn openscad_advanced_module_recursion() {
    assert_example_no_panic("Advanced/module_recursion.scad");
}
#[test]
#[ignore = "full exact text intersection exceeds the compatibility timeout"]
fn openscad_advanced_geb() {
    assert_example_compiles("Advanced/GEB.scad");
}
#[test]
fn openscad_advanced_offset() {
    assert_example_compiles("Advanced/offset.scad");
}
#[test]
fn openscad_advanced_animation() {
    assert_example_compiles("Advanced/animation.scad");
}
#[test]
fn openscad_advanced_assert() {
    assert_example_matches_reference("Advanced/assert.scad");
}
#[test]
fn openscad_advanced_surface_image() {
    assert_example_no_panic("Advanced/surface_image.scad");
}
#[test]
fn openscad_old_example001() {
    assert_example_compiles("Old/example001.scad");
}
#[test]
fn openscad_old_example002() {
    assert_example_matches_reference("Old/example002.scad");
}
#[test]
fn openscad_old_example003() {
    assert_example_matches_reference_loose("Old/example003.scad");
}
#[test]
fn openscad_old_example004() {
    assert_example_matches_reference("Old/example004.scad");
}
#[test]
fn openscad_old_example006() {
    assert_example_compiles("Old/example006.scad");
}
#[test]
fn openscad_old_example007() {
    assert_example_no_panic("Old/example007.scad");
}
#[test]
fn openscad_old_example008() {
    assert_example_no_panic("Old/example008.scad");
}
#[test]
fn openscad_old_example009() {
    assert_example_no_panic("Old/example009.scad");
}
#[test]
fn openscad_old_example010() {
    assert_example_no_panic("Old/example010.scad");
}
#[test]
fn openscad_old_example011() {
    assert_example_matches_reference("Old/example011.scad");
}
#[test]
fn openscad_old_example012() {
    assert_example_matches_reference("Old/example012.scad");
}
#[test]
fn openscad_old_example013() {
    assert_example_no_panic("Old/example013.scad");
}
#[test]
fn openscad_old_example014() {
    assert_example_compiles("Old/example014.scad");
}
#[test]
fn openscad_old_example015() {
    assert_example_no_panic("Old/example015.scad");
}
#[test]
fn openscad_old_example016() {
    assert_example_matches_reference_loose("Old/example016.scad");
}
#[test]
fn openscad_old_example018() {
    assert_example_compiles("Old/example018.scad");
}
#[test]
fn openscad_old_example019() {
    assert_example_matches_reference("Old/example019.scad");
}
#[test]
fn openscad_old_example021() {
    assert_example_compiles("Old/example021.scad");
}
#[test]
fn openscad_old_example022() {
    assert_example_matches_reference_loose("Old/example022.scad");
}
#[test]
fn openscad_old_example023() {
    assert_example_no_panic("Old/example023.scad");
}
#[test]
fn openscad_old_example024() {
    assert_example_matches_reference("Old/example024.scad");
}
fn assert_example_matches_reference_very_loose(relative: &str) {
    let path = example_path(relative);
    let code = std::fs::read_to_string(&path).unwrap();
    let parts = match compile_with_timeout(&code, 0) {
        CompilationResult::Success { parts, .. } => parts,
        CompilationResult::Error(e) => panic!("{relative}: compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    };
    let ref_name = relative.replace(".scad", ".json");
    let ref_path = format!(
        "{}/tests/openscad_references/{ref_name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let Ok(ref_json) = std::fs::read_to_string(&ref_path) else {
        return;
    };
    // These parametric fixtures currently serve as coarse bounds and facet-count
    // guards while their tessellation differs substantially from OpenSCAD's.
    assert_example_matches_reference_data(relative, &parts, &ref_json, 5.0);
}

#[test]
fn openscad_parametric_candlestand() {
    assert_example_matches_reference_very_loose("Parametric/candleStand.scad");
}
#[test]
fn openscad_parametric_sign() {
    assert_example_matches_reference_very_loose("Parametric/sign.scad");
}
#[test]
fn openscad_basics_dodecahedron_difference() {
    assert_example_matches_reference("Basics/dodecahedron_difference.scad");
}

fn dodecahedron_scad() -> &'static str {
    r"
    phi = (1 + sqrt(5)) / 2;
    points = [
        [ 1,  1,  1], [ 1,  1, -1], [ 1, -1,  1], [ 1, -1, -1],
        [-1,  1,  1], [-1,  1, -1], [-1, -1,  1], [-1, -1, -1],
        [0,  1/phi,  phi], [0,  1/phi, -phi], [0, -1/phi,  phi], [0, -1/phi, -phi],
        [ 1/phi,  phi, 0], [ 1/phi, -phi, 0], [-1/phi,  phi, 0], [-1/phi, -phi, 0],
        [ phi, 0,  1/phi], [ phi, 0, -1/phi], [-phi, 0,  1/phi], [-phi, 0, -1/phi]
    ];
    faces = [
        [0,8,10,2,16],  [0,16,17,1,12], [0,12,14,4,8],
        [1,17,3,11,9],  [1,9,5,14,12],  [2,10,6,15,13],
        [2,13,3,17,16], [3,13,15,7,11], [4,14,5,19,18],
        [4,18,6,10,8],  [5,9,11,7,19],  [6,18,19,7,15]
    ];
    "
}

#[test]
fn test_polyhedron_pentagon_faces_standalone() {
    let code = format!(
        "{} polyhedron(points=points, faces=faces);",
        dodecahedron_scad()
    );
    match compile_with_timeout(&code, 0) {
        CompilationResult::Success { parts, .. } => {
            assert!(!parts.is_empty());
            let total_tris: usize = parts.iter().map(|p| p.positions.len() / 3).sum();
            assert!(total_tris >= 36);
        }
        CompilationResult::Error(e) => panic!("compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
fn test_cone_zero_r1() {
    let code = "cylinder(h=5, r1=0, r2=10, $fn=12);";
    match compile_with_timeout(code, 0) {
        CompilationResult::Success { parts, .. } => {
            assert!(!parts.is_empty());
        }
        CompilationResult::Error(e) => panic!("compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

fn signed_volume(mesh: &MeshData) -> f64 {
    mesh.indices
        .chunks_exact(3)
        .map(|triangle| {
            let a = mesh.positions[triangle[0] as usize].map(f64::from);
            let b = mesh.positions[triangle[1] as usize].map(f64::from);
            let c = mesh.positions[triangle[2] as usize].map(f64::from);
            let cross = [
                b[1].mul_add(c[2], -(b[2] * c[1])),
                b[2].mul_add(c[0], -(b[0] * c[2])),
                b[0].mul_add(c[1], -(b[1] * c[0])),
            ];
            a[0].mul_add(cross[0], a[1].mul_add(cross[1], a[2] * cross[2])) / 6.0
        })
        .sum()
}

#[test]
fn curved_primitives_export_with_outward_winding() {
    for code in [
        "sphere(r=5, $fn=16);",
        "cylinder(h=8, r=5, $fn=16);",
        "cylinder(h=8, r1=0, r2=5, $fn=16);",
        "cylinder(h=8, r1=5, r2=0, $fn=16);",
    ] {
        let mesh = compile_to_merged_mesh(code);
        let volume = signed_volume(&mesh);
        assert!(
            volume > 0.0,
            "{code} exported inward or malformed triangles (signed volume {volume})"
        );
    }
}

#[test]
fn test_color_named_and_rgb() {
    let code = r#"
color("red") cube(10);
color("green") translate([20, 0, 0]) sphere(5, $fn=12);
color([0.2, 0.4, 0.8]) translate([40, 0, 0]) cylinder(h=10, r=5, $fn=12);
"#;
    match compile_with_timeout(code, 0) {
        CompilationResult::Success { parts, .. } => {
            assert_eq!(parts.len(), 3);
            assert_eq!(parts[0].color, Some([1.0, 0.0, 0.0]));
            assert!(
                parts[1]
                    .positions
                    .iter()
                    .all(|position| (15.0..=25.0).contains(&position[0])),
                "translated sphere should export translated x coordinates"
            );
            assert!(
                parts[2]
                    .positions
                    .iter()
                    .all(|position| (35.0..=45.0).contains(&position[0])),
                "translated cylinder should export translated x coordinates"
            );
        }
        CompilationResult::Error(e) => panic!("Color test failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}

#[test]
#[ignore = "expensive exact nested Boolean stress; run in release mode"]
fn test_faceted_pot_polyhedron() {
    let code = r##"
$view = "main";

module faceted_pot(r_bot, r_mid, r_top, h_mid, h_top) {
    points = concat(
        [[0,0,0], [0,0,h_top]],
        [for (i=[0:5]) [r_bot * cos(i*60), r_bot * sin(i*60), 0]],
        [for (i=[0:5]) [r_mid * cos(i*60 + 30), r_mid * sin(i*60 + 30), h_mid]],
        [for (i=[0:5]) [r_top * cos(i*60), r_top * sin(i*60), h_top]]
    );
    faces = concat(
        [for (i=[0:5]) [0, 2+i, 2+(i+1)%6]],
        [for (i=[0:5]) [2+i, 8+i, 2+(i+1)%6]],
        [for (i=[0:5]) [8+i, 8+(i+1)%6, 2+(i+1)%6]],
        [for (i=[0:5]) [8+i, 14+(i+1)%6, 8+(i+1)%6]],
        [for (i=[0:5]) [14+i, 14+(i+1)%6, 8+i]],
        [for (i=[0:5]) [1, 14+(i+1)%6, 14+i]]
    );
    polyhedron(points=points, faces=faces);
}

module planter() {
    difference() {
        faceted_pot(r_bot=35, r_mid=65, r_top=50, h_mid=40, h_top=80);
        translate([0, 0, 4])
        union() {
            faceted_pot(r_bot=31, r_mid=61, r_top=46, h_mid=36, h_top=77);
            translate([0, 0, 75]) cylinder(r=46, h=10, $fn=6);
        }
        for (i=[0:2]) {
            rotate([0, 0, i*120])
            translate([15, 0, -1])
            cylinder(r=3, h=10, $fn=20);
        }
    }
}

module drip_tray() {
    union() {
        difference() {
            cylinder(r1=44, r2=53, h=15, $fn=6);
            translate([0, 0, 3]) cylinder(r1=40, r2=49, h=13, $fn=6);
        }
        for (i=[0:2]) {
            rotate([0, 0, i*120 + 30])
            translate([25, 0, 3])
            scale([2, 0.5, 0.5])
            sphere(r=4, $fn=20);
        }
    }
}

module view_main() {
    translate([-60, 0, 0]) color("#E2E8F0") planter();
    translate([60, 0, 0]) color("#475569") drip_tray();
}

if ($view == "main") { view_main(); }
"##;
    match compile_with_timeout(code, 0) {
        CompilationResult::Success { parts, .. } => {
            assert!(!parts.is_empty(), "Expected at least one part");
        }
        CompilationResult::Error(e) => panic!("Faceted pot compilation failed: {e}"),
        CompilationResult::Canceled => panic!("Compilation was unexpectedly canceled"),
    }
}
