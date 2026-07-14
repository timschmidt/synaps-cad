use csgrs::Real;
use openscad_rs::ast::{Argument, Expr, Parameter, SourceFile, Statement};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::geometry::{BoolOp, Shape, TransformKind};

pub mod value;
pub use value::Value;

pub mod booleans;
pub mod builtins;
pub mod primitives;
pub mod transformations;

#[cfg(test)]
mod tests;

/// Stored user-defined module.
#[derive(Clone)]
pub struct UserModule {
    /// Parameter names and optional default expressions in declaration order.
    pub params: Vec<(String, Option<Expr>)>,
    /// Statements evaluated when the module is instantiated.
    pub body: Vec<Statement>,
}

/// Stored user-defined function.
#[derive(Clone)]
pub struct UserFunction {
    /// Parameter names and optional default expressions in declaration order.
    pub params: Vec<(String, Option<Expr>)>,
    /// Expression evaluated when the function is called.
    pub body_expr: Expr,
}

/// Stateful evaluator for one `OpenSCAD` source file.
///
/// The evaluator resolves expressions, user definitions, geometry modules,
/// colors, and exact-number extensions into [`Shape`] values. Construct a
/// fresh evaluator per independent compilation.
pub struct Evaluator {
    /// Current lexical variable bindings, including `OpenSCAD` special variables.
    pub variables: HashMap<String, Value>,
    /// User-defined modules visible in the current source file.
    pub modules: HashMap<String, UserModule>,
    /// User-defined functions visible in the current source file.
    pub functions: HashMap<String, UserFunction>,
    /// Stack of call-site children for `children()` calls inside user modules.
    pub children_stack: Vec<Vec<Statement>>,
    children_cache_stack: Vec<Vec<Option<Vec<Shape>>>>,
    /// Recursion depth counter to prevent stack overflow.
    pub depth: usize,
    /// Stack of active colors from nested `color()` calls.
    pub color_stack: Vec<[f32; 3]>,
    /// Warnings collected during evaluation (shown to the user after compilation).
    pub warnings: Vec<String>,
    /// Optional cancellation signal.
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl Evaluator {
    /// Creates an evaluator with `OpenSCAD`'s default tessellation variables.
    #[must_use]
    pub fn new() -> Self {
        let mut variables = HashMap::new();
        variables.insert("$fn".into(), Value::Number(0.0));
        variables.insert("$fa".into(), Value::Number(12.0));
        variables.insert("$fs".into(), Value::Number(2.0));
        variables.insert("PI".into(), Value::Number(std::f64::consts::PI));
        variables.insert("$preview".into(), Value::Bool(true));
        Self {
            variables,
            modules: HashMap::new(),
            functions: HashMap::new(),
            children_stack: Vec::new(),
            children_cache_stack: Vec::new(),
            depth: 0,
            color_stack: Vec::new(),
            warnings: Vec::new(),
            cancel: None,
        }
    }

    /// Returns whether the optional cancellation flag has been raised.
    #[must_use]
    pub fn is_canceled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
    }

    /// Resolves `$fn`, falling back to `$fa` and `$fs` as `OpenSCAD` does.
    #[must_use]
    pub fn resolve_fn(&self, args: &[(Option<String>, Value)]) -> usize {
        self.resolve_fn_with_radius(args, None)
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::similar_names
    )]
    pub fn resolve_fn_with_radius(
        &self,
        args: &[(Option<String>, Value)],
        r: Option<f64>,
    ) -> usize {
        let fn_val = Self::get_named_arg(args, "$fn")
            .and_then(Value::as_number)
            .or_else(|| self.variables.get("$fn").and_then(Value::as_number))
            .unwrap_or(0.0);

        if fn_val > 0.0 {
            return fn_val as usize;
        }

        let fa = self
            .variables
            .get("$fa")
            .and_then(Value::as_number)
            .unwrap_or(12.0);
        let fs = self
            .variables
            .get("$fs")
            .and_then(Value::as_number)
            .unwrap_or(2.0);

        // Bound both controls away from zero to keep tessellation finite.
        let fa = fa.max(0.01);
        let fs = fs.max(0.01);

        let fragments = r.map_or_else(
            || 360.0 / fa,
            |radius| {
                if radius.abs() < 1e-9 {
                    3.0
                } else {
                    let from_fa = 360.0 / fa;
                    let from_fs = (radius * 2.0 * std::f64::consts::PI) / fs;
                    f64::min(from_fa, from_fs)
                }
            },
        );

        f64::ceil(fragments.max(5.0)) as usize
    }

    pub fn eval_source_file(&mut self, source_file: &SourceFile) -> Vec<(Shape, Option<[f32; 3]>)> {
        // Register definitions before geometry so forward references work.
        self.register_definitions(&source_file.statements);

        let mut shapes = Vec::new();
        for stmt in &source_file.statements {
            if self.is_canceled() {
                break;
            }
            self.eval_statement(stmt, &mut shapes);
        }
        shapes
    }

    /// Recursively scan statements to register module/function definitions.
    pub fn register_definitions(&mut self, stmts: &[Statement]) {
        for stmt in stmts {
            match stmt {
                Statement::ModuleDefinition {
                    name, params, body, ..
                } => {
                    self.register_module(name, params, body);
                    self.register_definitions(body);
                }
                Statement::FunctionDefinition {
                    name, params, body, ..
                } => {
                    self.register_function(name, params, body);
                }
                Statement::ModuleInstantiation { children, .. } => {
                    self.register_definitions(children);
                }
                Statement::IfElse {
                    then_body,
                    else_body,
                    ..
                } => {
                    self.register_definitions(then_body);
                    if let Some(eb) = else_body {
                        self.register_definitions(eb);
                    }
                }
                Statement::Block { body, .. } => {
                    self.register_definitions(body);
                }
                _ => {}
            }
        }
    }

    pub fn eval_statement(
        &mut self,
        stmt: &Statement,
        shapes: &mut Vec<(Shape, Option<[f32; 3]>)>,
    ) {
        if self.is_canceled() {
            return;
        }
        match stmt {
            Statement::ModuleInstantiation {
                name,
                args,
                children,
                ..
            } => {
                // These modules alter evaluation scope rather than geometry.
                match name.as_str() {
                    "for" | "intersection_for" => {
                        shapes.extend(self.eval_for_from_instantiation(args, children));
                    }
                    "let" => {
                        self.eval_let_instantiation(args, children, shapes);
                    }
                    "color" => {
                        let eval_args = self.eval_arguments(args);
                        self.eval_color_into(children, &eval_args, shapes);
                    }
                    "children" => {
                        let eval_args = self.eval_arguments(args);
                        let color = self.color_stack.last().copied();
                        shapes.extend(
                            self.eval_children_instantiation(&eval_args)
                                .into_iter()
                                .map(|shape| (shape, color)),
                        );
                    }
                    "translate" | "rotate" | "scale" | "mirror" => {
                        let eval_args = self.eval_arguments(args);
                        let kind = match name.as_str() {
                            "translate" => TransformKind::Translate,
                            "rotate" => TransformKind::Rotate,
                            "scale" => TransformKind::Scale,
                            _ => TransformKind::Mirror,
                        };
                        self.eval_transform_into(children, &eval_args, kind, shapes);
                    }
                    _ => {
                        // Evaluate user modules directly to preserve per-shape
                        // colors across their bodies.
                        if let Some(user_mod) = self.modules.get(name).cloned() {
                            let eval_args = self.eval_arguments(args);
                            self.eval_user_module_into(&user_mod, &eval_args, children, shapes);
                        } else if let Some(s) =
                            self.eval_module_instantiation_inner(name, args, children)
                        {
                            let color = self.color_stack.last().copied();
                            shapes.push((s, color));
                        }
                    }
                }
            }
            Statement::Assignment { name, expr, .. } => {
                let val = self.eval_expr(expr);
                self.variables.insert(name.clone(), val);
            }
            Statement::IfElse {
                condition,
                then_body,
                else_body,
                ..
            } => {
                shapes.extend(self.eval_if_else(condition, then_body, else_body.as_ref()));
            }
            Statement::Block { body, .. } => {
                for s in body {
                    self.eval_statement(s, shapes);
                }
            }
            _ => {}
        }
    }

    pub fn eval_for_from_instantiation(
        &mut self,
        args: &[Argument],
        children: &[Statement],
    ) -> Vec<(Shape, Option<[f32; 3]>)> {
        // A `for` may bind multiple variables, each with its own iterable.
        let loop_vars: Vec<(String, Value)> = args
            .iter()
            .filter_map(|arg| {
                let name = arg.name.as_ref()?.clone();
                let val = self.eval_expr(&arg.value);
                Some((name, val))
            })
            .collect();

        self.eval_for_nested(&loop_vars, 0, children)
    }

    pub fn eval_for_nested(
        &mut self,
        loop_vars: &[(String, Value)],
        depth: usize,
        children: &[Statement],
    ) -> Vec<(Shape, Option<[f32; 3]>)> {
        if depth >= loop_vars.len() {
            return self.eval_statement_list(children);
        }

        let (name, range_val) = &loop_vars[depth];
        let items = range_val.to_iterable();
        let saved = self.variables.get(name).cloned();

        let mut results = Vec::new();
        for item in items {
            if self.is_canceled() {
                break;
            }
            self.variables.insert(name.clone(), item);
            results.extend(self.eval_for_nested(loop_vars, depth + 1, children));
        }

        match saved {
            Some(v) => {
                self.variables.insert(name.clone(), v);
            }
            None => {
                self.variables.remove(name);
            }
        }
        results
    }

    pub fn eval_let_instantiation(
        &mut self,
        args: &[Argument],
        children: &[Statement],
        shapes: &mut Vec<(Shape, Option<[f32; 3]>)>,
    ) {
        let saved = self.variables.clone();
        for arg in args {
            if let Some(name) = &arg.name {
                let val = self.eval_expr(&arg.value);
                self.variables.insert(name.clone(), val);
            }
        }
        for stmt in children {
            self.eval_statement(stmt, shapes);
        }
        self.variables = saved;
    }

    pub fn eval_statement_list(&mut self, stmts: &[Statement]) -> Vec<(Shape, Option<[f32; 3]>)> {
        let mut shapes = Vec::new();
        for stmt in stmts {
            if self.is_canceled() {
                break;
            }
            self.eval_statement(stmt, &mut shapes);
        }
        shapes
    }

    pub fn eval_if_else(
        &mut self,
        condition: &Expr,
        then_body: &[Statement],
        else_body: Option<&Vec<Statement>>,
    ) -> Vec<(Shape, Option<[f32; 3]>)> {
        let cond_val = self.eval_expr(condition);

        if cond_val.as_bool() {
            self.eval_statement_list(then_body)
        } else if let Some(eb) = else_body {
            self.eval_statement_list(eb)
        } else {
            Vec::new()
        }
    }

    pub fn register_module(&mut self, name: &str, params: &[Parameter], body: &[Statement]) {
        let extracted_params = Self::extract_params(params);
        self.modules.insert(
            name.to_string(),
            UserModule {
                params: extracted_params,
                body: body.to_vec(),
            },
        );
    }

    pub fn register_function(&mut self, name: &str, params: &[Parameter], body: &Expr) {
        let extracted_params = Self::extract_params(params);
        self.functions.insert(
            name.to_string(),
            UserFunction {
                params: extracted_params,
                body_expr: body.clone(),
            },
        );
    }

    fn extract_params(params: &[Parameter]) -> Vec<(String, Option<Expr>)> {
        params
            .iter()
            .map(|p| (p.name.clone(), p.default.clone()))
            .collect()
    }

    #[allow(clippy::cast_precision_loss)]
    pub fn eval_user_module_into(
        &mut self,
        user_mod: &UserModule,
        args: &[(Option<String>, Value)],
        call_site_children: &[Statement],
        shapes: &mut Vec<(Shape, Option<[f32; 3]>)>,
    ) {
        const MAX_DEPTH: usize = 512;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.warnings
                .push(format!("Maximum recursion depth ({MAX_DEPTH}) exceeded"));
            self.depth -= 1;
            return;
        }

        let saved_vars = self.variables.clone();
        self.children_stack.push(call_site_children.to_vec());
        self.children_cache_stack
            .push(vec![None; call_site_children.len()]);
        self.variables.insert(
            "$children".into(),
            Value::Number(call_site_children.len() as f64),
        );

        let mut pos_idx = 0;
        for (param_name, default_expr) in &user_mod.params {
            let named = Self::get_named_arg(args, param_name).cloned();
            let val = named.unwrap_or_else(|| {
                let v = Self::get_positional_arg(args, pos_idx).cloned();
                pos_idx += 1;
                v.or_else(|| default_expr.as_ref().map(|e| self.eval_expr(e)))
                    .unwrap_or(Value::Undef)
            });
            self.variables.insert(param_name.clone(), val);
        }
        for (name, val) in args {
            if let Some(n) = name
                && n.starts_with('$')
            {
                self.variables.insert(n.clone(), val.clone());
            }
        }

        for stmt in &user_mod.body {
            self.eval_statement(stmt, shapes);
        }

        self.variables = saved_vars;
        self.children_cache_stack.pop();
        self.children_stack.pop();
        self.depth -= 1;
    }

    #[allow(clippy::cast_precision_loss, clippy::missing_panics_doc)]
    pub fn eval_user_module(
        &mut self,
        user_mod: &UserModule,
        args: &[(Option<String>, Value)],
        call_site_children: &[Statement],
    ) -> Option<Shape> {
        let saved_vars = self.variables.clone();
        self.children_stack.push(call_site_children.to_vec());
        self.children_cache_stack
            .push(vec![None; call_site_children.len()]);
        self.variables.insert(
            "$children".into(),
            Value::Number(call_site_children.len() as f64),
        );

        let mut pos_idx = 0;
        for (param_name, default_expr) in &user_mod.params {
            let named = Self::get_named_arg(args, param_name).cloned();
            let val = named.unwrap_or_else(|| {
                let v = Self::get_positional_arg(args, pos_idx).cloned();
                pos_idx += 1;
                v.or_else(|| default_expr.as_ref().map(|e| self.eval_expr(e)))
                    .unwrap_or(Value::Undef)
            });
            self.variables.insert(param_name.clone(), val);
        }

        for (name, val) in args {
            if let Some(n) = name
                && n.starts_with('$')
            {
                self.variables.insert(n.clone(), val.clone());
            }
        }

        let mut meshes = Vec::new();
        for stmt in &user_mod.body {
            self.eval_statement(stmt, &mut meshes);
        }

        self.variables = saved_vars;
        self.children_cache_stack.pop();
        self.children_stack.pop();

        if meshes.is_empty() {
            None
        } else {
            let mut iter = meshes.into_iter();
            let (mut result, _) = iter.next().unwrap();
            for (m, _) in iter {
                result = result.union(m);
            }
            Some(result)
        }
    }

    pub fn eval_module_instantiation_inner(
        &mut self,
        name: &str,
        raw_args: &[Argument],
        children: &[Statement],
    ) -> Option<Shape> {
        const MAX_DEPTH: usize = 512;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.warnings.push(format!(
                "Maximum recursion depth ({MAX_DEPTH}) exceeded in {name}()"
            ));
            self.depth -= 1;
            return None;
        }
        let result = self.eval_module_instantiation_dispatch(name, raw_args, children);
        self.depth -= 1;
        result
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn eval_module_instantiation_dispatch(
        &mut self,
        name: &str,
        raw_args: &[Argument],
        children: &[Statement],
    ) -> Option<Shape> {
        let args = self.eval_arguments(raw_args);

        match name {
            // 3D primitives.
            "cube" => self.eval_cube(&args),
            "sphere" => self.eval_sphere(&args),
            "cylinder" => self.eval_cylinder(&args),
            "polyhedron" => self.eval_polyhedron(&args),

            // 2D primitives.
            "circle" => self.eval_circle(&args),
            "square" => self.eval_square(&args),
            "polygon" => self.eval_polygon(&args),
            "text" => self.eval_text(&args),

            // Boolean operations.
            "union" => self.eval_boolean_op(children, BoolOp::Union),
            "difference" => self.eval_boolean_op(children, BoolOp::Difference),
            "intersection" => self.eval_boolean_op(children, BoolOp::Intersection),

            // Transformations.
            "translate" => self.eval_transform(children, &args, TransformKind::Translate),
            "rotate" => self.eval_transform(children, &args, TransformKind::Rotate),
            "scale" => self.eval_transform(children, &args, TransformKind::Scale),
            "mirror" => self.eval_transform(children, &args, TransformKind::Mirror),
            "multmatrix" => {
                self.warnings
                    .push("multmatrix() not yet supported, passing through children".into());
                self.eval_passthrough_children(children)
            }
            "offset" => self.eval_offset(children, &args),
            "resize" | "projection" | "render" | "group" | "import" | "surface" => {
                self.eval_passthrough_children(children)
            }

            // Extrusions.
            "linear_extrude" => self.eval_linear_extrude(children, &args),
            "rotate_extrude" => self.eval_rotate_extrude(children, &args),

            // Other modules.
            "hull" => self.eval_hull(children),
            "minkowski" => {
                let child_shapes = self.eval_children(children);
                if child_shapes.len() >= 2 {
                    let mut iter = child_shapes.into_iter();
                    let base = iter.next().unwrap().into_csg_mesh();
                    let tool = iter.next().unwrap().into_csg_mesh();
                    Some(Shape::from_csg_mesh(base.minkowski_sum(&tool, ())))
                } else {
                    self.eval_passthrough_children(children)
                }
            }
            "echo" => {
                self.eval_echo(&args);
                self.eval_passthrough_children(children)
            }
            "children" => {
                let shapes = self.eval_children_instantiation(&args);
                let mut iter = shapes.into_iter();
                let mut result = iter.next()?;
                for shape in iter {
                    result = result.union(shape);
                }
                Some(result)
            }

            _ => {
                if let Some(user_mod) = self.modules.get(name).cloned() {
                    self.eval_user_module(&user_mod, &args, children)
                } else {
                    self.warnings
                        .push(format!("Unknown module: {name}(), skipping"));
                    None
                }
            }
        }
    }

    pub fn eval_arguments(&mut self, args: &[Argument]) -> Vec<(Option<String>, Value)> {
        args.iter()
            .map(|arg| {
                let val = self.eval_expr(&arg.value);
                (arg.name.clone(), val)
            })
            .collect()
    }

    #[must_use]
    pub fn get_named_arg<'a>(args: &'a [(Option<String>, Value)], name: &str) -> Option<&'a Value> {
        args.iter()
            .find(|(n, _)| n.as_deref() == Some(name))
            .map(|(_, v)| v)
    }

    #[must_use]
    pub fn get_positional_arg(args: &[(Option<String>, Value)], idx: usize) -> Option<&Value> {
        let mut pos = 0;
        for (name, val) in args {
            if name.is_none() {
                if pos == idx {
                    return Some(val);
                }
                pos += 1;
            }
        }
        None
    }

    #[must_use]
    pub fn get_arg<'a>(
        args: &'a [(Option<String>, Value)],
        name: &str,
        pos: usize,
    ) -> Option<&'a Value> {
        Self::get_named_arg(args, name).or_else(|| Self::get_positional_arg(args, pos))
    }

    pub fn get_arg_number(args: &[(Option<String>, Value)], name: &str, pos: usize) -> Option<f64> {
        Self::get_arg(args, name, pos).and_then(Value::as_number)
    }

    pub fn get_arg_real(args: &[(Option<String>, Value)], name: &str, pos: usize) -> Option<Real> {
        Self::get_arg(args, name, pos).and_then(Value::as_real)
    }

    pub fn get_arg_bool(
        args: &[(Option<String>, Value)],
        name: &str,
        pos: usize,
        default: bool,
    ) -> bool {
        Self::get_arg(args, name, pos).map_or(default, Value::as_bool)
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn eval_children(&mut self, children: &[Statement]) -> Vec<Shape> {
        let mut result = Vec::new();
        for stmt in children {
            if self.is_canceled() {
                break;
            }
            let mut shapes = Vec::new();
            self.eval_statement(stmt, &mut shapes);
            if !shapes.is_empty() {
                let mut iter = shapes.into_iter().map(|(s, _)| s);
                let mut merged = iter.next().unwrap();
                for s in iter {
                    merged = merged.union(s);
                }
                result.push(merged);
            }
        }
        result
    }

    fn eval_cached_call_site_child(&mut self, index: usize) -> Vec<Shape> {
        if let Some(cached) = self
            .children_cache_stack
            .last()
            .and_then(|cache| cache.get(index))
            .and_then(Option::as_ref)
        {
            return cached.clone();
        }
        let Some(child) = self
            .children_stack
            .last()
            .and_then(|children| children.get(index))
            .cloned()
        else {
            return Vec::new();
        };
        let mut evaluated = Vec::new();
        self.eval_statement(&child, &mut evaluated);
        let shapes: Vec<Shape> = evaluated.into_iter().map(|(shape, _)| shape).collect();
        if let Some(slot) = self
            .children_cache_stack
            .last_mut()
            .and_then(|cache| cache.get_mut(index))
        {
            *slot = Some(shapes.clone());
        }
        shapes
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn eval_children_instantiation(&mut self, args: &[(Option<String>, Value)]) -> Vec<Shape> {
        let indices = if let Some(index) = Self::get_arg_number(args, "index", 0) {
            if !index.is_finite() || index < 0.0 || index.fract() != 0.0 {
                Vec::new()
            } else {
                vec![index as usize]
            }
        } else {
            (0..self.children_stack.last().map_or(0, Vec::len)).collect()
        };
        indices
            .into_iter()
            .flat_map(|index| self.eval_cached_call_site_child(index))
            .collect()
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn eval_passthrough_children(&mut self, children: &[Statement]) -> Option<Shape> {
        let child_shapes = self.eval_children(children);
        if child_shapes.is_empty() {
            return None;
        }
        let mut iter = child_shapes.into_iter();
        let mut result = iter.next().unwrap();
        for child in iter {
            result = result.union(child);
        }
        Some(result)
    }
}
