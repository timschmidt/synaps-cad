use csgrs::Real;
use csgrs::mesh::Mesh as CsgMesh;
use openscad_rs::ast::Statement;

use super::{Evaluator, Value};
use crate::compiler::geometry::{BoolOp, Shape};

impl Evaluator {
    #[allow(clippy::missing_panics_doc)]
    pub fn eval_boolean_op(&mut self, children: &[Statement], op: BoolOp) -> Option<Shape> {
        let child_shapes = self.eval_children(children);
        if child_shapes.is_empty() {
            return None;
        }

        let mut iter = child_shapes.into_iter();
        let first = iter.next().unwrap();

        match op {
            BoolOp::Union => {
                let rest: Vec<Shape> = iter.collect();
                if rest.is_empty() {
                    return Some(first);
                }
                let mut result = first;
                for child in rest {
                    result = result.union(child);
                }
                Some(result)
            }
            BoolOp::Difference => {
                let rest: Vec<Shape> = iter.collect();
                if rest.is_empty() {
                    return Some(first);
                }
                let mut tool_iter = rest.into_iter();
                let mut tool = tool_iter.next().unwrap();
                for t in tool_iter {
                    tool = tool.union(t);
                }
                Some(first.difference(tool))
            }
            BoolOp::Intersection => {
                let mut result = first;
                for child in iter {
                    result = result.intersection(child);
                }
                Some(result)
            }
        }
    }

    pub fn eval_offset(
        &mut self,
        children: &[Statement],
        args: &[(Option<String>, Value)],
    ) -> Option<Shape> {
        let r = Self::get_arg_real(args, "r", 99);
        let delta = Self::get_arg_real(args, "delta", 99);

        let child_shapes = self.eval_children(children);
        if child_shapes.is_empty() {
            return None;
        }
        let sketch = self.shapes_to_sketch(&child_shapes)?;

        if let Some(r_val) = r {
            if r_val == Real::zero() {
                Some(Shape::Sketch2D(sketch))
            } else {
                let distance = r_val;
                let offset = sketch.offset_rounded(distance.clone());
                Some(Shape::Sketch2D(
                    if offset.is_empty() && !sketch.is_empty() {
                        sketch.offset_rounded_finite_output(distance)
                    } else {
                        offset
                    },
                ))
            }
        } else if let Some(d_val) = delta {
            if d_val == Real::zero() {
                Some(Shape::Sketch2D(sketch))
            } else {
                Some(Shape::Sketch2D(sketch.offset(d_val)))
            }
        } else {
            let d = Self::get_arg_real(args, "", 0).unwrap_or_else(Real::zero);
            if d == Real::zero() {
                Some(Shape::Sketch2D(sketch))
            } else {
                let distance = d;
                let offset = sketch.offset_rounded(distance.clone());
                Some(Shape::Sketch2D(
                    if offset.is_empty() && !sketch.is_empty() {
                        sketch.offset_rounded_finite_output(distance)
                    } else {
                        offset
                    },
                ))
            }
        }
    }

    pub fn eval_hull(&mut self, children: &[Statement]) -> Option<Shape> {
        let child_shapes = self.eval_children(children);
        if child_shapes.is_empty() {
            return None;
        }
        let mut all_polygons = Vec::new();
        for shape in child_shapes {
            let mesh = shape.into_csg_mesh();
            all_polygons.extend(mesh.polygons);
        }
        let combined = CsgMesh::from_polygons(all_polygons);
        Some(Shape::from_csg_mesh(combined.convex_hull(())))
    }

    pub fn eval_color_into(
        &mut self,
        children: &[Statement],
        args: &[(Option<String>, Value)],
        shapes: &mut Vec<(Shape, Option<[f32; 3]>)>,
    ) {
        let rgb = Self::parse_color_args(args);
        if let Some(c) = rgb {
            self.color_stack.push(c);
        }
        for stmt in children {
            self.eval_statement(stmt, shapes);
        }
        if rgb.is_some() {
            self.color_stack.pop();
        }
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn parse_color_args(args: &[(Option<String>, Value)]) -> Option<[f32; 3]> {
        let first = args.first().map(|(_, v)| v)?;
        match first {
            Value::String(name) => parse_hex_color(name)
                .or_else(|| crate::compiler::rendering::colors::named_color(name)),
            Value::List(items) => {
                if items.len() >= 3 {
                    let r = items[0].as_number()? as f32;
                    let g = items[1].as_number()? as f32;
                    let b = items[2].as_number()? as f32;
                    Some([r, g, b])
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Parse a hex color string like "#D4A76A", "#fff", or "D4A76A" into [r, g, b] in 0.0–1.0 range.
fn parse_hex_color(s: &str) -> Option<[f32; 3]> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some([
                f32::from(r) / 255.0,
                f32::from(g) / 255.0,
                f32::from(b) / 255.0,
            ])
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            Some([
                f32::from(r) / 15.0,
                f32::from(g) / 15.0,
                f32::from(b) / 15.0,
            ])
        }
        8 => {
            // OpenSCAD geometry colors ignore the `#RRGGBBAA` alpha component.
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some([
                f32::from(r) / 255.0,
                f32::from(g) / 255.0,
                f32::from(b) / 255.0,
            ])
        }
        _ => None,
    }
}
