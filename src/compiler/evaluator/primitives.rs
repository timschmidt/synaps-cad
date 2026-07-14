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

fn point_from_real(point: &[Real; 3]) -> Point3 {
    Point3::new(point[0].clone(), point[1].clone(), point[2].clone())
}

impl Evaluator {
    #[allow(clippy::unused_self)]
    pub fn eval_cube(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let size_val = Self::get_arg(args, "size", 0).unwrap_or(&Value::Number(1.0));
        let center = Self::get_arg_bool(args, "center", 1, false);

        let mesh = match size_val {
            Value::Number(_) | Value::Exact(_) => {
                let m = CsgMesh::cube(size_val.as_real()?, ());
                if center { m.center() } else { m }
            }
            Value::List(dims) => {
                let nums: Vec<Real> = dims.iter().filter_map(Value::as_real).collect();
                let (x, y, z) = match nums.len() {
                    1 => (nums[0].clone(), nums[0].clone(), nums[0].clone()),
                    2 => (nums[0].clone(), nums[1].clone(), Real::one()),
                    _ => (
                        nums.first().cloned().unwrap_or_else(Real::one),
                        nums.get(1).cloned().unwrap_or_else(Real::one),
                        nums.get(2).cloned().unwrap_or_else(Real::one),
                    ),
                };
                let m = CsgMesh::cube(Real::one(), ()).scale(x, y, z);
                if center { m.center() } else { m }
            }
            _ => return None,
        };

        Some(Shape::from_csg_mesh(mesh))
    }

    #[must_use]
    pub fn eval_sphere(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let r = Self::get_arg_real(args, "r", 0)
            .or_else(|| Self::get_arg_real(args, "d", 0).and_then(|d| (d / 2.0).ok()))
            .unwrap_or_else(Real::one);

        let slices = self.resolve_fn_with_radius(args, r.to_f64_lossy());
        let stacks = slices / 2;

        Some(Shape::from_csg_mesh(CsgMesh::sphere(r, slices, stacks, ())))
    }

    #[must_use]
    pub fn eval_cylinder(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let h = Self::get_arg_real(args, "h", 0)
            .or_else(|| Self::get_arg_real(args, "height", 0))
            .unwrap_or_else(Real::one);

        // Diameter arguments take precedence over their radius equivalents.
        let half = |d: Real| (d / 2.0).ok();
        let r1 = Self::get_arg_real(args, "r1", 99)
            .or_else(|| Self::get_arg_real(args, "d1", 99).and_then(half))
            .or_else(|| Self::get_arg_real(args, "r", 1))
            .or_else(|| Self::get_arg_real(args, "d", 1).and_then(half))
            .unwrap_or_else(Real::one);
        let r2 = Self::get_arg_real(args, "r2", 99)
            .or_else(|| Self::get_arg_real(args, "d2", 99).and_then(half))
            .unwrap_or_else(|| r1.clone());

        let center = Self::get_arg_bool(args, "center", 99, false);
        // Tessellation follows the larger endpoint radius.
        let max_radius = r1.max(&r2).to_f64_lossy();
        let slices = self.resolve_fn_with_radius(args, max_radius);

        // `frustum` handles zero-radius cone tips without degenerate quads.
        let m = if r1 == r2 {
            CsgMesh::cylinder(r1, h, slices, ())
        } else {
            CsgMesh::frustum(r1, r2, h, slices, ())
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

        let points: Vec<[Real; 3]> = points_val
            .as_list()?
            .iter()
            .filter_map(|v| {
                let nums = v.to_real_list()?;
                if nums.len() >= 3 {
                    Some([nums[0].clone(), nums[1].clone(), nums[2].clone()])
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

        let faces = {
            let mut seen = std::collections::HashSet::new();
            let mut deduped = Vec::with_capacity(faces.len());
            for face in &faces {
                if face.is_empty() {
                    continue;
                }
                // Canonical rotation makes cyclically equivalent faces equal.
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
            let approximate = |point: &[Real; 3]| {
                NalgebraVector3::new(
                    point[0].to_f64_lossy().unwrap_or(0.0),
                    point[1].to_f64_lossy().unwrap_or(0.0),
                    point[2].to_f64_lossy().unwrap_or(0.0),
                )
            };
            let v0 = approximate(pts[0]);
            let v1 = approximate(pts[1]);
            let v2 = approximate(pts[2]);
            let normal =
                nalgebra_vector_to_profile_normal(&(v1 - v0).cross(&(v2 - v0)).normalize());

            if pts.len() == 3 {
                let verts: Vec<_> = pts
                    .iter()
                    .map(|p| Vertex::new(point_from_real(p), normal.clone()))
                    .collect();
                polygons.push(Polygon::new(verts, ()));
            } else {
                // Fan-triangulate faces with more than three vertices.
                let p0 = point_from_real(pts[0]);
                for i in 1..pts.len() - 1 {
                    let p1 = point_from_real(pts[i]);
                    let p2 = point_from_real(pts[i + 1]);
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
        Some(Shape::from_csg_mesh(CsgMesh::from_polygons(polygons)))
    }

    #[must_use]
    pub fn eval_circle(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let r = Self::get_arg_real(args, "r", 0)
            .or_else(|| Self::get_arg_real(args, "d", 0).and_then(|d| (d / 2.0).ok()))
            .unwrap_or_else(Real::one);

        let slices = self.resolve_fn_with_radius(args, r.to_f64_lossy());
        Some(Shape::Sketch2D(Profile::circle(r, slices)))
    }

    #[allow(clippy::unused_self)]
    pub fn eval_square(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        let size_val = Self::get_arg(args, "size", 0).unwrap_or(&Value::Number(1.0));
        let center = Self::get_arg_bool(args, "center", 1, false);

        let sketch = match size_val {
            Value::Number(_) | Value::Exact(_) => Profile::square(size_val.as_real()?),
            Value::List(dims) => {
                let nums: Vec<Real> = dims.iter().filter_map(Value::as_real).collect();
                let w = nums.first().cloned().unwrap_or_else(Real::one);
                let h = nums.get(1).cloned().unwrap_or_else(|| w.clone());
                Profile::rectangle(w, h)
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
        let points: Vec<[Real; 2]> = points_val
            .as_list()?
            .iter()
            .filter_map(|v| {
                let nums = v.to_real_list()?;
                if nums.len() >= 2 {
                    Some([nums[0].clone(), nums[1].clone()])
                } else {
                    None
                }
            })
            .collect();

        if points.len() < 3 {
            return None;
        }
        Some(Shape::Sketch2D(Profile::polygon(&points)))
    }

    #[must_use]
    pub fn eval_text(&self, args: &[(Option<String>, Value)]) -> Option<Shape> {
        // OpenSCAD signature: text, size, font, halign, valign, spacing,
        // direction, language, script, and $fn.
        let text_str = match Self::get_arg(args, "text", 0) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => format!("{n}"),
            Some(Value::Exact(n)) => format!("{n}"),
            _ => return None,
        };

        if text_str.is_empty() {
            return None;
        }

        let size = Self::get_arg_number(args, "size", 1).unwrap_or(10.0);
        let spacing_val = Self::get_arg_number(args, "spacing", 5).unwrap_or(1.0);

        // Prefer the requested system font, then bundled Liberation Sans.
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
