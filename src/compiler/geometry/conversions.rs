use csgrs::mesh::Mesh as CsgMesh;

use crate::compiler::types::MeshData;

/// Direct `CsgMesh` → `MeshData` conversion.
///
/// # Errors
/// Returns an error if the mesh has no vertices.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn csg_mesh_to_mesh_data(mesh: &CsgMesh<()>) -> Result<MeshData, String> {
    let vertex_capacity = mesh
        .polygons
        .iter()
        .map(|polygon| polygon.vertices().len().saturating_sub(2) * 3)
        .sum();
    let mut positions = Vec::with_capacity(vertex_capacity);
    let mut normals = Vec::with_capacity(vertex_capacity);
    let mut indices = Vec::with_capacity(vertex_capacity);

    for poly in &mesh.polygons {
        for triangle in poly.triangulate_indices_finite_output() {
            let idx = positions.len() as u32;
            let triangle_positions = triangle.map(|vertex_index| {
                let vertex = &poly.vertices()[vertex_index];
                let p = &vertex.position;
                let x = p.x.to_f64_lossy().unwrap_or(0.0) as f32;
                let y = p.y.to_f64_lossy().unwrap_or(0.0) as f32;
                let z = p.z.to_f64_lossy().unwrap_or(0.0) as f32;
                // OpenSCAD Z-up -> Bevy Y-up (a proper rotation).
                [x, z, -y]
            });
            let [a, b, c] = triangle_positions;
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
            positions.extend([a, b, c]);
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

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
/// Convert axis-angle rotation to Euler angles.
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
    let _r01 = (t * ux).mul_add(uy, -(s * uz));
    let _r02 = (t * ux).mul_add(uz, s * uy);
    let r10 = (t * uy).mul_add(ux, s * uz);
    let _r11 = (t * uy).mul_add(uy, c);
    let _r12 = (t * uy).mul_add(uz, -(s * ux));
    let r20 = (t * uz).mul_add(ux, -(s * uy));
    let r21 = (t * uz).mul_add(uy, s * ux);
    let r22 = (t * uz).mul_add(uz, c);

    // Convert to intrinsic ZYX Euler angles.
    let pitch = if r20.abs() < 1.0 - 1e-12 {
        (-r20).asin()
    } else if r20 < 0.0 {
        std::f64::consts::FRAC_PI_2
    } else {
        -std::f64::consts::FRAC_PI_2
    };
    let is_not_singular = pitch.cos().abs() > 1e-12_f64;
    let yaw = if is_not_singular { r21.atan2(r22) } else { 0.0 };
    let roll = if is_not_singular { r10.atan2(r00) } else { 0.0 };

    (yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees())
}

#[cfg(test)]
mod tests {
    use super::csg_mesh_to_mesh_data;
    use csgrs::mesh::{Mesh, Polygon};
    use csgrs::vertex::Vertex;
    use hyperlattice::{Point3, Real, Vector3};

    #[test]
    fn concave_faces_are_ear_clipped_instead_of_fanned() {
        let point = |x: i32, y: i32| {
            Vertex::new(
                Point3::new(Real::from(x), Real::from(y), Real::zero()),
                Vector3::z(),
            )
        };
        let polygon = Polygon::new(
            vec![
                point(0, 0),
                point(2, 0),
                point(1, 1),
                point(2, 2),
                point(0, 2),
            ],
            (),
        );
        let mesh = Mesh::from_polygons(vec![polygon]);

        let rendered = csg_mesh_to_mesh_data(&mesh).unwrap();

        assert_eq!(rendered.positions.len(), 9);
        for triangle in rendered.positions.chunks_exact(3) {
            let [a, b, c] = triangle else { unreachable!() };
            let area = (b[2] - a[2]).mul_add(-(c[0] - a[0]), (b[0] - a[0]) * (c[2] - a[2]));
            assert!(area.abs() > f32::EPSILON);
        }
    }
}
