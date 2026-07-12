use csgrs::Profile;
use csgrs::Real;
use csgrs::csg::CSG;
use csgrs::mesh::Mesh as CsgMesh;
use csgrs::mesh::Polygon;
use csgrs::vertex::Vertex;
use hyperlattice::{Point3, Vector3};
use nalgebra::Vector3 as NalgebraVector3;

use super::{Evaluator, Value};
use crate::compiler::geometry::Shape;
use crate::compiler::rendering::fonts::{
    apply_text_alignment, render_text_with_direction, resolve_font_data,
};

fn to_real(value: f64) -> Real {
    Real::try_from(value).ok().unwrap_or_else(Real::zero)
}

fn nalgebra_vector_to_profile_normal(vector: &NalgebraVector3<f64>) -> Vector3 {
    Vector3::new([to_real(vector[0]), to_real(vector[1]), to_real(vector[2])])
}

fn point_from_f64(point: &[f64; 3]) -> Point3 {
    Point3::new(to_real(point[0]), to_real(point[1]), to_real(point[2]))
}

impl Evaluator {
    // =======================================================================
    // 3D Primitives
    // =======================================================================

    #[allow(clippy::unused_self)]
    pub fn eval_cube(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let size_val = Self::get_arg(args, "size", 0).unwrap_or(&Value::Number(1.0));
        let center = Self::get_arg_bool(args, "center", 1, false);

        let mesh = match size_val {
            Value::Number(s) => {
                let m = CsgMesh::cube(to_real(*s), ());
                if center { m.center() } else { m }
            }
            Value::List(dims) => {
                let nums: Vec<f64> = dims.iter().filter_map(Value::as_number).collect();
                let (x, y, z) = match nums.len() {
                    1 => (nums[0], nums[0], nums[0]),
                    2 => (nums[0], nums[1], 1.0),
                    _ => (
                        nums.first().copied().unwrap_or(1.0),
                        nums.get(1).copied().unwrap_or(1.0),
                        nums.get(2).copied().unwrap_or(1.0),
                    ),
                };
                let m = CsgMesh::cube(to_real(1.0), ()).scale(to_real(x), to_real(y), to_real(z));
                if center { m.center() } else { m }
            }
            _ => return None,
        };

        Some(Shape::from_csg_mesh(mesh))
    }

    #[must_use]
    pub fn eval_sphere(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let r = Self::get_arg_number(args, "r", 0)
            .or_else(|| Self::get_arg_number(args, "d", 0).map(|d| d / 2.0))
            .unwrap_or(1.0);

        let slices = self.resolve_fn_with_radius(args, Some(r));
        let stacks = slices / 2;

        Some(Shape::from_csg_mesh(CsgMesh::sphere(
            to_real(r),
            slices,
            stacks,
            (),
        )))
    }

    #[must_use]
    pub fn eval_cylinder(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let h = Self::get_arg_number(args, "h", 0)
            .or_else(|| Self::get_arg_number(args, "height", 0))
            .unwrap_or(1.0);

        // Handle d/d1/d2 (diameter) as well as r/r1/r2
        let r1 = Self::get_arg_number(args, "r1", 99)
            .or_else(|| Self::get_arg_number(args, "d1", 99).map(|d| d / 2.0))
            .or_else(|| Self::get_arg_number(args, "r", 1))
            .or_else(|| Self::get_arg_number(args, "d", 1).map(|d| d / 2.0))
            .unwrap_or(1.0);
        let r2 = Self::get_arg_number(args, "r2", 99)
            .or_else(|| Self::get_arg_number(args, "d2", 99).map(|d| d / 2.0))
            .unwrap_or(r1);

        let center = Self::get_arg_bool(args, "center", 99, false);
        // Use max radius to determine segments
        let slices = self.resolve_fn_with_radius(args, Some(r1.max(r2)));

        // For cones (r1 != r2): use CsgMesh::frustum which correctly
        // handles zero-radius (emits triangles, not degenerate quads).
        let m = if (r1 - r2).abs() < 1e-12 {
            CsgMesh::cylinder(to_real(r1), to_real(h), slices, ())
        } else {
            CsgMesh::frustum(to_real(r1), to_real(r2), to_real(h), slices, ())
        };
        let m = if center { m.center() } else { m };

        Some(Shape::from_csg_mesh(m))
    }

    #[allow(
        clippy::unused_self,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::missing_panics_doc
    )]
    #[must_use]
    pub fn eval_polyhedron(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let points_val = Self::get_arg(args, "points", 0)?;
        let faces_val =
            Self::get_arg(args, "faces", 1).or_else(|| Self::get_arg(args, "triangles", 1));

        let points: Vec<[f64; 3]> = points_val
            .as_list()?
            .iter()
            .filter_map(|v| {
                let nums = v.to_number_list()?;
                if nums.len() >= 3 {
                    Some([nums[0], nums[1], nums[2]])
                } else {
                    None
                }
            })
            .collect();

        let faces: Vec<Vec<usize>> = faces_val?
            .as_list()?
            .iter()
            .filter_map(|v| {
                let nums = v.to_number_list()?;
                Some(nums.iter().map(|n| *n as usize).collect())
            })
            .collect();

        // Deduplicate faces
        let faces = {
            let mut seen = std::collections::HashSet::new();
            let mut deduped = Vec::with_capacity(faces.len());
            for face in &faces {
                if face.is_empty() {
                    continue;
                }
                // Rotate so the minimum index comes first (canonical form)
                let min_pos = face
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| *v)
                    .map(|(i, _)| i)
                    .unwrap();
                let mut canonical: Vec<usize> = face[min_pos..].to_vec();
                canonical.extend_from_slice(&face[..min_pos]);
                if seen.insert(canonical) {
                    deduped.push(face.clone());
                }
            }
            deduped
        };

        let mut polygons = Vec::new();
        for face in &faces {
            if face.len() < 3 {
                continue;
            }
            let pts: Vec<_> = face.iter().filter_map(|&idx| points.get(idx)).collect();
            if pts.len() < 3 {
                continue;
            }
            // Compute face normal
            let v0 = NalgebraVector3::new(pts[0][0], pts[0][1], pts[0][2]);
            let v1 = NalgebraVector3::new(pts[1][0], pts[1][1], pts[1][2]);
            let v2 = NalgebraVector3::new(pts[2][0], pts[2][1], pts[2][2]);
            let normal =
                nalgebra_vector_to_profile_normal(&(v1 - v0).cross(&(v2 - v0)).normalize());

            if pts.len() == 3 {
                let verts: Vec<_> = pts
                    .iter()
                    .map(|p| Vertex::new(point_from_f64(p), normal.clone()))
                    .collect();
                polygons.push(Polygon::new(verts, ()));
            } else {
                // Fan-triangulate N-gons (N>3)
                let p0 = point_from_f64(pts[0]);
                for i in 1..pts.len() - 1 {
                    let p1 = point_from_f64(pts[i]);
                    let p2 = point_from_f64(pts[i + 1]);
                    let verts = vec![
                        Vertex::new(p0.clone(), normal.clone()),
                        Vertex::new(p1, normal.clone()),
                        Vertex::new(p2, normal.clone()),
                    ];
                    polygons.push(Polygon::new(verts, ()));
                }
            }
        }

        if polygons.is_empty() {
            return None;
        }
        Some(Shape::from_csg_mesh(CsgMesh::from_polygons(&polygons)))
    }

    // =======================================================================
    // 2D Primitives
    // =======================================================================

    #[must_use]
    pub fn eval_circle(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let r = Self::get_arg_number(args, "r", 0)
            .or_else(|| Self::get_arg_number(args, "d", 0).map(|d| d / 2.0))
            .unwrap_or(1.0);

        let slices = self.resolve_fn_with_radius(args, Some(r));
        Some(Shape::Sketch2D(Profile::circle(to_real(r), slices)))
    }

    #[allow(clippy::unused_self)]
    pub fn eval_square(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let size_val = Self::get_arg(args, "size", 0).unwrap_or(&Value::Number(1.0));
        let center = Self::get_arg_bool(args, "center", 1, false);

        let sketch = match size_val {
            Value::Number(s) => Profile::square(to_real(*s)),
            Value::List(dims) => {
                let nums: Vec<f64> = dims.iter().filter_map(Value::as_number).collect();
                let w = nums.first().copied().unwrap_or(1.0);
                let h = nums.get(1).copied().unwrap_or(w);
                Profile::rectangle(to_real(w), to_real(h))
            }
            _ => return None,
        };

        let sketch = if center { sketch.center() } else { sketch };
        Some(Shape::Sketch2D(sketch))
    }

    #[allow(clippy::unused_self)]
    #[must_use]
    pub fn eval_polygon(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let points_val = Self::get_arg(args, "points", 0)?;
        let points: Vec<[f64; 2]> = points_val
            .as_list()?
            .iter()
            .filter_map(|v| {
                let nums = v.to_number_list()?;
                if nums.len() >= 2 {
                    Some([nums[0], nums[1]])
                } else {
                    None
                }
            })
            .collect();

        if points.len() < 3 {
            return None;
        }
        let points = points
            .iter()
            .map(|p| [to_real(p[0]), to_real(p[1])])
            .collect::<Vec<_>>();
        Some(Shape::Sketch2D(Profile::polygon(&points)))
    }

    #[must_use]
    pub fn eval_text(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        // text(t, size, font, halign, valign, spacing, direction, language, script, $fn)
        let text_str = match Self::get_arg(args, "text", 0) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => format!("{n}"),
            _ => return None,
        };

        if text_str.is_empty() {
            return None;
        }

        let size = Self::get_arg_number(args, "size", 1).unwrap_or(10.0);
        let spacing_val = Self::get_arg_number(args, "spacing", 5).unwrap_or(1.0);

        // Resolve font data: try system font, fall back to bundled Liberation Sans
        let font_param = match Self::get_arg(args, "font", 2) {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        let font_data = resolve_font_data(font_param.as_deref());

        let direction = match Self::get_arg(args, "direction", 6) {
            Some(Value::String(s)) => s.to_lowercase(),
            _ => "ltr".to_string(),
        };

        let sketch =
            render_text_with_direction(&text_str, &font_data, size, spacing_val, &direction);

        // Apply horizontal alignment (default "left" = no offset)
        let halign = match Self::get_arg(args, "halign", 3) {
            Some(Value::String(s)) => s.clone(),
            _ => "left".to_string(),
        };
        let valign = match Self::get_arg(args, "valign", 4) {
            Some(Value::String(s)) => s.clone(),
            _ => "baseline".to_string(),
        };

        let sketch = apply_text_alignment(sketch, &halign, &valign);

        Some(Shape::Sketch2D(sketch))
    }
}
