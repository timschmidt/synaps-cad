use csgrs::bmesh::BMesh;
use csgrs::csg::CSG;
use csgrs::mesh::Mesh as CsgMesh;
use csgrs::polygon::Polygon;
use csgrs::triangulated::Triangulated3D;
use nalgebra::Point3;
use std::collections::{HashMap, HashSet};

use crate::compiler::types::MeshData;

/// Convert `CsgMesh` to `BMesh`. If the mesh has boundary edges (non-manifold),
/// attempts to fix it by deduplicating vertices and removing degenerate/duplicate triangles.
///
/// # Errors
/// Returns an error string if all repair attempts fail.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn csg_mesh_to_bmesh(mesh: &CsgMesh<()>) -> Result<BMesh<()>, String> {
    use boolmesh::prelude::Manifold;
    const QUANT: f64 = 1e6;

    if mesh.polygons.is_empty() {
        return Ok(BMesh::new());
    }

    // Helper: try Manifold::new with catch_unwind to guard against internal panics
    // in boolmesh's edge topology (e.g. non-manifold meshes with >2 faces per edge).
    let try_manifold = |p: &[f64], i: &[usize]| -> Result<BMesh<()>, String> {
        let p = p.to_vec();
        let i = i.to_vec();
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Manifold::new(&p, &i)))
            .map_err(|_| "Manifold::new panicked (non-manifold mesh)".to_string())?
            .map(|m| BMesh::from_manifold(m, None))
    };

    // Triangulate from polygons directly with proper vertex sharing.
    // CsgMesh::get_vertices_and_indices() creates unshared vertices (each polygon
    // gets its own copies), which breaks boolmesh's manifold requirement.
    let mut vmap: HashMap<[i64; 3], usize> = HashMap::new();
    let mut verts: Vec<f64> = Vec::new();
    let mut tris: Vec<usize> = Vec::new();

    let vert_idx =
        |vmap: &mut HashMap<[i64; 3], usize>, verts: &mut Vec<f64>, p: &Point3<f64>| -> usize {
            let key = [
                (p.x * QUANT).round() as i64,
                (p.y * QUANT).round() as i64,
                (p.z * QUANT).round() as i64,
            ];
            *vmap.entry(key).or_insert_with(|| {
                let idx = verts.len() / 3;
                verts.push(p.x);
                verts.push(p.y);
                verts.push(p.z);
                idx
            })
        };

    for poly in &mesh.polygons {
        let n = poly.vertices.len();
        if n < 3 {
            continue;
        }
        // Fan triangulation from vertex 0
        let i0 = vert_idx(&mut vmap, &mut verts, &poly.vertices[0].position);
        for j in 1..n - 1 {
            let i1 = vert_idx(&mut vmap, &mut verts, &poly.vertices[j].position);
            let i2 = vert_idx(&mut vmap, &mut verts, &poly.vertices[j + 1].position);
            if i0 == i1 || i1 == i2 || i2 == i0 {
                continue;
            }
            tris.push(i0);
            tris.push(i1);
            tris.push(i2);
        }
    }

    if tris.is_empty() {
        return Ok(BMesh::new());
    }

    // Attempt 1: direct with shared vertices
    if let Ok(bmesh) = try_manifold(&verts, &tris) {
        return Ok(bmesh);
    }

    // Attempt 2: flipped winding (hull algorithms sometimes have inconsistent winding)
    let mut flipped = tris.clone();
    for tri in flipped.chunks_mut(3) {
        tri.swap(1, 2);
    }
    if let Ok(bmesh) = try_manifold(&verts, &flipped) {
        return Ok(bmesh);
    }

    // Attempt 3: remove duplicate triangles (can occur from degenerate polygons)
    let mut seen: HashSet<[usize; 3]> = HashSet::new();
    let mut clean_tris: Vec<usize> = Vec::new();
    for tri in tris.chunks(3) {
        let mut key = [tri[0], tri[1], tri[2]];
        key.sort_unstable();
        if seen.insert(key) {
            clean_tris.extend_from_slice(tri);
        }
    }
    if clean_tris.len() != tris.len()
        && let Ok(bmesh) = try_manifold(&verts, &clean_tris)
    {
        return Ok(bmesh);
    }

    eprintln!("[SynapsCAD] Warning: Non-manifold mesh, all repair attempts failed");
    Err("Non-manifold mesh: boolean operation produced geometry that could not be repaired. Please report this bug with the code that caused it.".into())
}

pub fn bmesh_to_csg_mesh(bmesh: &BMesh<()>) -> CsgMesh<()> {
    let mut polygons = Vec::new();
    bmesh.visit_triangles(|[v0, v1, v2]| {
        polygons.push(Polygon::new(vec![v0, v1, v2], None));
    });
    CsgMesh::from_polygons(&polygons, None)
}

/// # Errors
/// Returns an error if the mesh has no vertices.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn bmesh_to_mesh_data(bmesh: &BMesh<()>) -> Result<MeshData, String> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();

    bmesh.visit_triangles(|[v0, v1, v2]| {
        let idx = positions.len() as u32;
        // OpenSCAD Z-up → Bevy Y-up: swap Y and Z
        for v in &[v0, v2, v1] {
            positions.push([
                v.position.x as f32,
                v.position.z as f32,
                -v.position.y as f32,
            ]);
        }
        // Compute face normal in Bevy space from the swapped positions
        let a = positions[idx as usize];
        let b = positions[idx as usize + 1];
        let c = positions[idx as usize + 2];
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1].mul_add(ac[2], -(ab[2] * ac[1])),
            ab[2].mul_add(ac[0], -(ab[0] * ac[2])),
            ab[0].mul_add(ac[1], -(ab[1] * ac[0])),
        ];
        let len = cross[0]
            .mul_add(cross[0], cross[1].mul_add(cross[1], cross[2] * cross[2]))
            .sqrt();
        let n = if len > 1e-6 {
            [cross[0] / len, cross[1] / len, cross[2] / len]
        } else {
            [0.0, 1.0, 0.0]
        };
        normals.push(n);
        normals.push(n);
        normals.push(n);
        indices.push(idx);
        indices.push(idx + 1);
        indices.push(idx + 2);
    });

    if positions.is_empty() {
        return Err("Mesh has no vertices".into());
    }

    Ok(MeshData {
        positions,
        normals,
        indices,
        color: None,
    })
}

/// Direct `CsgMesh` → `MeshData` conversion, bypassing BMesh/Manifold.
/// Used as a fallback when manifold creation fails (e.g. thin extrudes).
///
/// # Errors
/// Returns an error if the mesh has no vertices.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn csg_mesh_to_mesh_data(mesh: &CsgMesh<()>) -> Result<MeshData, String> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();

    for poly in &mesh.polygons {
        let n = poly.vertices.len();
        if n < 3 {
            continue;
        }
        // Fan triangulation from vertex 0
        let p0 = &poly.vertices[0].position;
        for j in 1..n - 1 {
            let p1 = &poly.vertices[j].position;
            let p2 = &poly.vertices[j + 1].position;
            let idx = positions.len() as u32;
            // OpenSCAD Z-up → Bevy Y-up: swap Y and Z
            for p in [p0, p2, p1] {
                positions.push([p.x as f32, p.z as f32, -p.y as f32]);
            }
            let a = positions[idx as usize];
            let b = positions[idx as usize + 1];
            let c = positions[idx as usize + 2];
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let cross = [
                ab[1].mul_add(ac[2], -(ab[2] * ac[1])),
                ab[2].mul_add(ac[0], -(ab[0] * ac[2])),
                ab[0].mul_add(ac[1], -(ab[1] * ac[0])),
            ];
            let len = cross[0]
                .mul_add(cross[0], cross[1].mul_add(cross[1], cross[2] * cross[2]))
                .sqrt();
            let normal = if len > 1e-6 {
                [cross[0] / len, cross[1] / len, cross[2] / len]
            } else {
                [0.0, 1.0, 0.0]
            };
            normals.extend([normal, normal, normal]);
            indices.extend([idx, idx + 1, idx + 2]);
        }
    }

    if positions.is_empty() {
        return Err("Mesh has no vertices".into());
    }

    Ok(MeshData {
        positions,
        normals,
        indices,
        color: None,
    })
}

/// Convert axis-angle rotation (angle in degrees, axis [ax,ay,az]) to Euler angles [rx,ry,rz] in degrees.
/// Uses Rodrigues' rotation matrix → intrinsic ZYX Euler extraction.
#[must_use]
pub fn axis_angle_to_euler(angle_deg: f64, ax: f64, ay: f64, az: f64) -> (f64, f64, f64) {
    let len = ax.mul_add(ax, ay.mul_add(ay, az * az)).sqrt();
    if len < 1e-12 {
        return (0.0, 0.0, 0.0);
    }
    let (ux, uy, uz) = (ax / len, ay / len, az / len);
    let theta = angle_deg.to_radians();
    let c = theta.cos();
    let s = theta.sin();
    let t = 1.0 - c;

    // Rotation matrix from Rodrigues' formula
    let r00 = (t * ux).mul_add(ux, c);
    let r01 = (t * ux).mul_add(uy, -(s * uz));
    let _r02 = (t * ux).mul_add(uz, s * uy);
    let r10 = (t * uy).mul_add(ux, s * uz);
    let r11 = (t * uy).mul_add(uy, c);
    let _r12 = (t * uy).mul_add(uz, -(s * ux));
    let r20 = (t * uz).mul_add(ux, -(s * uy));
    let r21 = (t * uz).mul_add(uy, s * ux);
    let r22 = (t * uz).mul_add(uz, c);

    // Extract intrinsic ZYX Euler angles (matching OpenSCAD's rotate([x,y,z]) convention)
    let ry = (-r20).asin();
    let (rx, rz) = if ry.cos().abs() > 1e-6 {
        (r21.atan2(r22), r10.atan2(r00))
    } else {
        (0.0, r01.atan2(r11))
    };

    (rx.to_degrees(), ry.to_degrees(), rz.to_degrees())
}
