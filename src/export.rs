use crate::plugins::compilation::StlMeshData;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

/// Weld vertices that share the same position, producing a properly indexed
/// mesh with shared vertices (manifold-compatible). The rendering pipeline
/// duplicates vertices for flat shading; this reverses that for export.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn weld_vertices(positions: &[[f32; 3]], indices: &[u32]) -> (Vec<[f32; 3]>, Vec<u32>) {
    const QUANT: f64 = 1e6;
    let mut vmap: HashMap<[i64; 3], u32> = HashMap::new();
    let mut new_positions: Vec<[f32; 3]> = Vec::new();
    let mut new_indices: Vec<u32> = Vec::with_capacity(indices.len());

    for &idx in indices {
        let pos = positions[idx as usize];
        let key = [
            (f64::from(pos[0]) * QUANT).round() as i64,
            (f64::from(pos[1]) * QUANT).round() as i64,
            (f64::from(pos[2]) * QUANT).round() as i64,
        ];
        let new_idx = *vmap.entry(key).or_insert_with(|| {
            let i = new_positions.len() as u32;
            new_positions.push(pos);
            i
        });
        new_indices.push(new_idx);
    }

    (new_positions, new_indices)
}

/// Supported export formats.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Stl,
    Obj,
    ThreeMf,
}

impl ExportFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Stl => "stl",
            Self::Obj => "obj",
            Self::ThreeMf => "3mf",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Stl => "STL",
            Self::Obj => "OBJ (with colors)",
            Self::ThreeMf => "3MF (with colors)",
        }
    }
}

pub const ALL_FORMATS: &[ExportFormat] =
    &[ExportFormat::ThreeMf, ExportFormat::Obj, ExportFormat::Stl];

/// Export parts to the given path in the specified format.
pub fn export_parts(
    parts: &[StlMeshData],
    path: &Path,
    format: ExportFormat,
) -> Result<(), String> {
    match format {
        ExportFormat::Stl => export_stl(parts, path),
        ExportFormat::Obj => export_obj(parts, path),
        ExportFormat::ThreeMf => export_3mf(parts, path),
    }
}

/// Binary STL export (no colors).
#[allow(clippy::cast_possible_truncation, clippy::tuple_array_conversions)]
fn export_stl(parts: &[StlMeshData], path: &Path) -> Result<(), String> {
    let mut file =
        std::fs::File::create(path).map_err(|e| format!("Failed to create file: {e}"))?;

    let total_triangles: u32 = parts.iter().map(|p| (p.indices.len() / 3) as u32).sum();

    // Binary STL begins with an arbitrary 80-byte header and triangle count.
    let mut header = [0u8; 80];
    let label = b"SynapsCAD STL Export";
    header[..label.len()].copy_from_slice(label);
    file.write_all(&header).map_err(|e| e.to_string())?;
    file.write_all(&total_triangles.to_le_bytes())
        .map_err(|e| e.to_string())?;

    for part in parts {
        for tri in part.indices.chunks(3) {
            // Swap v1/v2 to produce CCW winding (outward normals);
            // internal mesh uses CW order.
            let (v0, v1, v2) = (
                part.positions[tri[0] as usize],
                part.positions[tri[2] as usize],
                part.positions[tri[1] as usize],
            );
            let u = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
            let v = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
            let n = [
                u[1].mul_add(v[2], -(u[2] * v[1])),
                u[2].mul_add(v[0], -(u[0] * v[2])),
                u[0].mul_add(v[1], -(u[1] * v[0])),
            ];
            let len = n[2].mul_add(n[2], n[0].mul_add(n[0], n[1] * n[1])).sqrt();
            let normal = if len > 0.0 {
                [n[0] / len, n[1] / len, n[2] / len]
            } else {
                [0.0, 0.0, 1.0]
            };

            for &c in &normal {
                file.write_all(&c.to_le_bytes())
                    .map_err(|e| e.to_string())?;
            }
            for vtx in [v0, v1, v2] {
                for c in vtx {
                    file.write_all(&c.to_le_bytes())
                        .map_err(|e| e.to_string())?;
                }
            }
            file.write_all(&0u16.to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

/// OBJ + MTL export (with per-part colors).
fn export_obj(parts: &[StlMeshData], path: &Path) -> Result<(), String> {
    let mtl_path = path.with_extension("mtl");
    let mtl_filename = mtl_path.file_name().unwrap_or_default().to_string_lossy();

    let mut obj =
        std::fs::File::create(path).map_err(|e| format!("Failed to create OBJ file: {e}"))?;
    let mut mtl =
        std::fs::File::create(&mtl_path).map_err(|e| format!("Failed to create MTL file: {e}"))?;

    writeln!(obj, "# SynapsCAD OBJ Export").map_err(|e| e.to_string())?;
    writeln!(obj, "mtllib {mtl_filename}").map_err(|e| e.to_string())?;
    writeln!(mtl, "# SynapsCAD MTL Export").map_err(|e| e.to_string())?;

    let mut vertex_offset = 1usize;
    let mut normal_global_offset = 0usize;

    for (i, part) in parts.iter().enumerate() {
        let mat_name = format!("part_{}", i + 1);
        let color = part.color.unwrap_or([0.7, 0.7, 0.7]);

        writeln!(mtl, "newmtl {mat_name}").map_err(|e| e.to_string())?;
        writeln!(mtl, "Kd {} {} {}", color[0], color[1], color[2]).map_err(|e| e.to_string())?;
        writeln!(mtl, "Ka 0.1 0.1 0.1").map_err(|e| e.to_string())?;
        writeln!(mtl, "Ks 0.3 0.3 0.3").map_err(|e| e.to_string())?;
        writeln!(mtl, "Ns 100.0").map_err(|e| e.to_string())?;
        writeln!(mtl, "d 1.0").map_err(|e| e.to_string())?;

        writeln!(obj, "o part_{}", i + 1).map_err(|e| e.to_string())?;
        writeln!(obj, "usemtl {mat_name}").map_err(|e| e.to_string())?;

        let (welded_pos, welded_idx) = weld_vertices(&part.positions, &part.indices);

        for pos in &welded_pos {
            writeln!(obj, "v {} {} {}", pos[0], pos[1], pos[2]).map_err(|e| e.to_string())?;
        }

        for tri in welded_idx.chunks(3) {
            let verts = [
                welded_pos[tri[0] as usize],
                welded_pos[tri[2] as usize],
                welded_pos[tri[1] as usize],
            ];
            let edge1 = [
                verts[1][0] - verts[0][0],
                verts[1][1] - verts[0][1],
                verts[1][2] - verts[0][2],
            ];
            let edge2 = [
                verts[2][0] - verts[0][0],
                verts[2][1] - verts[0][1],
                verts[2][2] - verts[0][2],
            ];
            let cross = [
                edge1[1].mul_add(edge2[2], -(edge1[2] * edge2[1])),
                edge1[2].mul_add(edge2[0], -(edge1[0] * edge2[2])),
                edge1[0].mul_add(edge2[1], -(edge1[1] * edge2[0])),
            ];
            let len = cross[2]
                .mul_add(cross[2], cross[0].mul_add(cross[0], cross[1] * cross[1]))
                .sqrt();
            let normal = if len > 0.0 {
                [cross[0] / len, cross[1] / len, cross[2] / len]
            } else {
                [0.0, 0.0, 1.0]
            };
            writeln!(obj, "vn {} {} {}", normal[0], normal[1], normal[2])
                .map_err(|e| e.to_string())?;
        }

        // OBJ indices are one-based; swap the last two vertices for CCW winding.
        let normal_offset = normal_global_offset;
        for (fi, tri) in welded_idx.chunks(3).enumerate() {
            let (a, b, c) = (
                tri[0] as usize + vertex_offset,
                tri[2] as usize + vertex_offset,
                tri[1] as usize + vertex_offset,
            );
            let ni = normal_offset + fi + 1;
            writeln!(obj, "f {a}//{ni} {b}//{ni} {c}//{ni}").map_err(|e| e.to_string())?;
        }

        vertex_offset += welded_pos.len();
        normal_global_offset += welded_idx.len() / 3;
    }

    Ok(())
}

/// 3MF export (with per-part colors via `ColorGroup`).
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn export_3mf(parts: &[StlMeshData], path: &Path) -> Result<(), String> {
    use lib3mf::{BuildItem, Mesh, Model, Object, Triangle, Vertex};

    let mut model = Model::new();
    let color_group_id = 1usize;

    // Deduplicate colors into one 3MF ColorGroup.
    let mut colors: Vec<(u8, u8, u8, u8)> = Vec::new();
    let mut part_color_indices: Vec<Option<usize>> = Vec::new();

    for part in parts {
        if let Some(c) = part.color {
            let rgba = (
                (c[0] * 255.0).clamp(0.0, 255.0) as u8,
                (c[1] * 255.0).clamp(0.0, 255.0) as u8,
                (c[2] * 255.0).clamp(0.0, 255.0) as u8,
                255u8,
            );
            let idx = colors
                .iter()
                .position(|&existing| existing == rgba)
                .unwrap_or_else(|| {
                    colors.push(rgba);
                    colors.len() - 1
                });
            part_color_indices.push(Some(idx));
        } else {
            part_color_indices.push(None);
        }
    }

    let has_colors = !colors.is_empty();
    if has_colors {
        model.required_extensions.push(lib3mf::Extension::Material);
        let cg = lib3mf::ColorGroup {
            id: color_group_id,
            colors,
            parse_order: 0,
        };
        model.resources.color_groups.push(cg);
    }

    // Each part is a sub-object so it can retain an independent color.
    let first_part_id = if has_colors { 2 } else { 1 };
    for (i, part) in parts.iter().enumerate() {
        let object_id = first_part_id + i;
        let mut mesh = Mesh::new();

        let (welded_pos, welded_idx) = weld_vertices(&part.positions, &part.indices);

        for pos in &welded_pos {
            mesh.vertices.push(Vertex::new(
                f64::from(pos[0]),
                f64::from(pos[1]),
                f64::from(pos[2]),
            ));
        }

        for tri_indices in welded_idx.chunks(3) {
            // The 3MF specification requires CCW winding; internal meshes use CW.
            let mut tri = Triangle::new(
                tri_indices[0] as usize,
                tri_indices[2] as usize,
                tri_indices[1] as usize,
            );
            if let Some(Some(color_idx)) = part_color_indices.get(i) {
                tri.pid = Some(color_group_id);
                tri.pindex = Some(*color_idx);
            }
            mesh.triangles.push(tri);
        }

        let mut object = Object::new(object_id);
        object.name = Some(format!("Part {}", i + 1));
        object.mesh = Some(mesh);
        model.resources.objects.push(object);
    }

    // An assembly makes multiple colored sub-objects one slicer-visible model.
    if parts.len() > 1 {
        let assembly_id = first_part_id + parts.len();
        let mut assembly = Object::new(assembly_id);
        assembly.name = Some("SynapsCAD Model".to_string());
        for i in 0..parts.len() {
            assembly
                .components
                .push(lib3mf::Component::new(first_part_id + i));
        }
        model.resources.objects.push(assembly);
        model.build.items.push(BuildItem::new(assembly_id));
    } else if !parts.is_empty() {
        model.build.items.push(BuildItem::new(first_part_id));
    }

    model
        .write_to_file(path)
        .map_err(|e| format!("Failed to write 3MF: {e}"))?;

    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn export_3mf(_parts: &[StlMeshData], _path: &Path) -> Result<(), String> {
    Err("3MF export is not available in the web build.".into())
}
