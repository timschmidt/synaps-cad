use csgrs::Profile;
use csgrs::Real;
use csgrs::csg::CSG;
use openscad_rs::ast::Statement;

use super::{Evaluator, Value};
use crate::compiler::geometry::conversions::axis_angle_to_euler;
use crate::compiler::geometry::{Shape, TransformKind};

fn to_real(value: f64) -> Real {
    Real::try_from(value).ok().unwrap_or_else(Real::zero)
}

impl Evaluator {
    pub fn eval_transform(
        &mut self,
        children: &[Statement],
        args: &[(Option<String>, Value)],
        kind: TransformKind,
    ) -> Option<Shape> {
        let child = self.eval_passthrough_children(children)?;
        Some(Self::apply_transform(child, &kind, args))
    }

    /// Evaluate children preserving per-shape colors, then apply a transform to each.
    pub fn eval_transform_into(
        &mut self,
        children: &[Statement],
        args: &[(Option<String>, Value)],
        kind: TransformKind,
        shapes: &mut Vec<(Shape, Option<[f32; 3]>)>,
    ) {
        let before = shapes.len();
        for stmt in children {
            self.eval_statement(stmt, shapes);
        }
        // Apply the transform to every newly-added shape
        let new_shapes: Vec<_> = shapes.drain(before..).collect();
        for (s, color) in new_shapes {
            shapes.push((Self::apply_transform(s, &kind, args), color));
        }
    }

    /// Apply a single transform to a shape.
    pub fn apply_transform(
        shape: Shape,
        kind: &TransformKind,
        args: &[(Option<String>, Value)],
    ) -> Shape {
        match kind {
            TransformKind::Translate => {
                let v = Self::get_positional_arg(args, 0)
                    .or_else(|| Self::get_named_arg(args, "v"))
                    .and_then(Value::to_number_list)
                    .unwrap_or_default();
                let (x, y, z) = (
                    v.first().copied().unwrap_or(0.0),
                    v.get(1).copied().unwrap_or(0.0),
                    v.get(2).copied().unwrap_or(0.0),
                );
                shape.translate(x, y, z)
            }
            TransformKind::Rotate => {
                let axis_vec = Self::get_named_arg(args, "v").and_then(Value::to_number_list);
                let a_val =
                    Self::get_positional_arg(args, 0).or_else(|| Self::get_named_arg(args, "a"));

                if let (Some(angle), Some(ax)) =
                    (a_val.as_ref().and_then(|v| v.as_number()), &axis_vec)
                {
                    let (ex, ey, ez) = axis_angle_to_euler(
                        angle,
                        ax.first().copied().unwrap_or(0.0),
                        ax.get(1).copied().unwrap_or(0.0),
                        ax.get(2).copied().unwrap_or(1.0),
                    );
                    shape.rotate(ex, ey, ez)
                } else if let Some(v) = a_val.and_then(Value::to_number_list) {
                    let (x, y, z) = (
                        v.first().copied().unwrap_or(0.0),
                        v.get(1).copied().unwrap_or(0.0),
                        v.get(2).copied().unwrap_or(0.0),
                    );
                    shape.rotate(x, y, z)
                } else {
                    let angle = Self::get_positional_arg(args, 0)
                        .and_then(Value::as_number)
                        .unwrap_or(0.0);
                    shape.rotate(0.0, 0.0, angle)
                }
            }
            TransformKind::Scale => {
                let val =
                    Self::get_positional_arg(args, 0).or_else(|| Self::get_named_arg(args, "v"));
                match val {
                    Some(Value::List(_)) => {
                        let v = val.and_then(Value::to_number_list).unwrap_or_default();
                        let (x, y, z) = (
                            v.first().copied().unwrap_or(1.0),
                            v.get(1).copied().unwrap_or(1.0),
                            v.get(2).copied().unwrap_or(1.0),
                        );
                        shape.scale(x, y, z)
                    }
                    Some(Value::Number(s)) => shape.scale(*s, *s, *s),
                    _ => shape,
                }
            }
            TransformKind::Mirror => {
                let v = Self::get_positional_arg(args, 0)
                    .or_else(|| Self::get_named_arg(args, "v"))
                    .and_then(Value::to_number_list)
                    .unwrap_or_else(|| vec![1.0, 0.0, 0.0]);
                let (nx, ny, nz) = (
                    v.first().copied().unwrap_or(1.0),
                    v.get(1).copied().unwrap_or(0.0),
                    v.get(2).copied().unwrap_or(0.0),
                );
                shape.mirror(nx, ny, nz)
            }
        }
    }

    pub fn eval_linear_extrude(
        &mut self,
        children: &[Statement],
        args: &[(Option<String>, Value)],
    ) -> Option<Shape> {
        let height = Self::get_arg_number(args, "height", 0).unwrap_or(1.0);
        let twist = Self::get_arg_number(args, "twist", 99).unwrap_or(0.0);
        let scale = Self::get_named_arg(args, "scale")
            .map(|value| match value {
                Value::Number(scale) => [*scale, *scale],
                Value::List(_) => {
                    let values = value.to_number_list().unwrap_or_default();
                    [
                        values.first().copied().unwrap_or(1.0),
                        values.get(1).copied().unwrap_or(1.0),
                    ]
                }
                _ => [1.0, 1.0],
            })
            .unwrap_or([1.0, 1.0]);
        let center = Self::get_arg_bool(args, "center", 99, false);
        let slices = Self::get_arg_number(args, "slices", 99)
            .filter(|value| value.is_finite() && *value >= 1.0)
            .map(|value| value.round() as usize)
            .unwrap_or_else(|| self.resolve_fn(args));

        // Collect 2D children
        let child_shapes = self.eval_children(children);
        if child_shapes.is_empty() {
            return None;
        }

        // Merge all children into a single sketch (if possible)
        let sketch = self.shapes_to_sketch(&child_shapes)?;

        let mesh = if twist != 0.0 || scale != [1.0, 1.0] {
            match sketch.extrude_twisted(
                to_real(height),
                to_real(twist),
                [to_real(scale[0]), to_real(scale[1])],
                slices.max(1),
                (),
            ) {
                Ok(mesh) => mesh,
                Err(error) => {
                    self.warnings
                        .push(format!("linear_extrude() error: {error:?}"));
                    return None;
                }
            }
        } else {
            sketch.extrude(to_real(height), ())
        };

        let mesh = if center { mesh.center() } else { mesh };
        Some(Shape::from_csg_mesh(mesh))
    }

    pub fn eval_rotate_extrude(
        &mut self,
        children: &[Statement],
        args: &[(Option<String>, Value)],
    ) -> Option<Shape> {
        let angle = Self::get_arg_number(args, "angle", 0).unwrap_or(360.0);
        let slices = self.resolve_fn(args);

        let child_shapes = self.eval_children(children);
        if child_shapes.is_empty() {
            return None;
        }

        let sketch = self.shapes_to_sketch(&child_shapes)?;
        let mesh = match sketch.revolve(to_real(angle), slices, ()) {
            Ok(m) => m,
            Err(e) => {
                self.warnings.push(format!("rotate_extrude() error: {e:?}"));
                return None;
            }
        };
        Some(Shape::from_csg_mesh(mesh))
    }

    /// Convert shapes to a single Sketch. 3D meshes are dropped with a warning.
    pub fn shapes_to_sketch(&mut self, shapes: &[Shape]) -> Option<Profile> {
        let mut result: Option<Profile> = None;
        for shape in shapes {
            match shape {
                Shape::Sketch2D(s) => {
                    result = Some(result.map_or_else(|| s.clone(), |r| r.union(s)));
                }
                Shape::Mesh3D(_) => {
                    self.warnings
                        .push("3D mesh child inside extrude, skipping".into());
                }
                Shape::Failed(e) => {
                    self.warnings
                        .push(format!("Failed child inside extrude: {e}"));
                }
            }
        }
        result
    }
}
