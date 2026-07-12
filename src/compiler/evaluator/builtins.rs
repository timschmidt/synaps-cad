use super::{Evaluator, UserFunction, Value};
use openscad_rs::ast::{BinaryOp, Expr, ExprKind, UnaryOp};

impl Evaluator {
    pub fn eval_expr(&mut self, expr: &Expr) -> Value {
        match &expr.kind {
            ExprKind::Number(n) => Value::Number(*n),
            ExprKind::String(s) => Value::String(s.clone()),
            ExprKind::BoolTrue => Value::Bool(true),
            ExprKind::BoolFalse => Value::Bool(false),
            ExprKind::Identifier(name) => self.variables.get(name).cloned().unwrap_or(Value::Undef),
            ExprKind::Vector(items) => {
                let mut vals: Vec<Value> = Vec::new();
                for item in items {
                    let is_lc = matches!(
                        &item.kind,
                        ExprKind::LcFor { .. }
                            | ExprKind::LcForC { .. }
                            | ExprKind::LcIf { .. }
                            | ExprKind::LcEach { .. }
                            | ExprKind::LcLet { .. }
                    );
                    let val = self.eval_expr(item);
                    if is_lc {
                        if let Value::List(inner) = val {
                            vals.extend(inner);
                        } else if !matches!(val, Value::Undef) {
                            vals.push(val);
                        }
                    } else {
                        vals.push(val);
                    }
                }
                Value::List(vals)
            }
            ExprKind::Range { start, step, end } => {
                let from = self.eval_expr(start).as_number().unwrap_or(0.0);
                let to = self.eval_expr(end).as_number().unwrap_or(0.0);
                let s = step.as_ref().map_or_else(
                    || if to >= from { 1.0 } else { -1.0 },
                    |step_expr| self.eval_expr(step_expr).as_number().unwrap_or(1.0),
                );
                Value::Range(from, to, s)
            }
            ExprKind::UnaryOp { op, operand } => {
                let inner = self.eval_expr(operand);
                match op {
                    UnaryOp::Negate => match inner {
                        Value::Number(n) => Value::Number(-n),
                        _ => Value::Undef,
                    },
                    UnaryOp::Not => Value::Bool(!inner.as_bool()),
                    UnaryOp::Plus => inner,
                    UnaryOp::BinaryNot => Value::Undef,
                }
            }
            ExprKind::BinaryOp { op, left, right } => self.eval_binary_op(*op, left, right),
            ExprKind::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                let cond = self.eval_expr(condition);
                if cond.as_bool() {
                    self.eval_expr(then_expr)
                } else {
                    self.eval_expr(else_expr)
                }
            }
            ExprKind::FunctionCall { callee, args } => {
                let name = match &callee.kind {
                    ExprKind::Identifier(n) => n.clone(),
                    _ => return Value::Undef,
                };
                let call_args: Vec<(Option<String>, Value)> = args
                    .iter()
                    .map(|arg| {
                        let val = self.eval_expr(&arg.value);
                        (arg.name.clone(), val)
                    })
                    .collect();

                if let Some(user_fn) = self.functions.get(&name).cloned() {
                    return self.eval_user_function(&user_fn, &call_args);
                }

                let args_vals: Vec<Value> = call_args.into_iter().map(|(_, v)| v).collect();
                self.eval_builtin_function(&name, &args_vals)
            }
            ExprKind::Index { object, index } => {
                let base = self.eval_expr(object);
                let idx = self.eval_expr(index);
                match (&base, &idx) {
                    (Value::List(l), Value::Number(i)) => {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let idx = *i as usize;
                        l.get(idx).cloned().unwrap_or(Value::Undef)
                    }
                    _ => Value::Undef,
                }
            }
            ExprKind::MemberAccess { object, member } => {
                let base = self.eval_expr(object);
                match (&base, member.as_str()) {
                    (Value::List(l), "x") => l.first().cloned().unwrap_or(Value::Undef),
                    (Value::List(l), "y") => l.get(1).cloned().unwrap_or(Value::Undef),
                    (Value::List(l), "z") => l.get(2).cloned().unwrap_or(Value::Undef),
                    _ => Value::Undef,
                }
            }
            ExprKind::Let { assignments, body } | ExprKind::LcLet { assignments, body } => {
                let saved = self.variables.clone();
                for arg in assignments {
                    if let Some(name) = &arg.name {
                        let val = self.eval_expr(&arg.value);
                        self.variables.insert(name.clone(), val);
                    }
                }
                let result = self.eval_expr(body);
                self.variables = saved;
                result
            }
            ExprKind::LcFor { assignments, body } => {
                let loop_vars: Vec<(String, Value)> = assignments
                    .iter()
                    .filter_map(|arg| {
                        let name = arg.name.as_ref()?.clone();
                        let val = self.eval_expr(&arg.value);
                        Some((name, val))
                    })
                    .collect();
                let mut results = Vec::new();
                self.eval_lc_for_nested(&loop_vars, 0, body, &mut results);
                Value::List(results)
            }
            ExprKind::LcIf {
                condition,
                then_expr,
                else_expr,
            } => {
                let cond = self.eval_expr(condition);
                if cond.as_bool() {
                    self.eval_expr(then_expr)
                } else if let Some(ee) = else_expr {
                    self.eval_expr(ee)
                } else {
                    Value::Undef
                }
            }
            ExprKind::LcEach { body } => self.eval_expr(body),
            ExprKind::Echo { args, body } => {
                let echo_args: Vec<(Option<String>, Value)> = args
                    .iter()
                    .map(|a| (a.name.clone(), self.eval_expr(&a.value)))
                    .collect();
                self.eval_echo(&echo_args);
                body.as_ref().map_or(Value::Undef, |b| self.eval_expr(b))
            }
            ExprKind::Assert { args, body } => {
                let values = args
                    .iter()
                    .map(|argument| (argument.name.clone(), self.eval_expr(&argument.value)))
                    .collect::<Vec<_>>();
                if values.first().is_some_and(|(_, value)| value.as_bool()) {
                    body.as_ref()
                        .map_or(Value::Undef, |body| self.eval_expr(body))
                } else {
                    let message = values.get(1).map_or_else(
                        || "assertion failed".into(),
                        |(_, value)| format_value(value),
                    );
                    self.warnings.push(format!("Assertion failed: {message}"));
                    Value::Undef
                }
            }
            _ => Value::Undef,
        }
    }

    pub fn eval_lc_for_nested(
        &mut self,
        loop_vars: &[(String, Value)],
        depth: usize,
        body: &Expr,
        results: &mut Vec<Value>,
    ) {
        if depth >= loop_vars.len() {
            let val = self.eval_expr(body);
            match val {
                Value::Undef => {}
                _ => results.push(val),
            }
            return;
        }
        let (name, range_val) = &loop_vars[depth];
        let items = range_val.to_iterable();
        let saved = self.variables.get(name).cloned();
        for item in items {
            self.variables.insert(name.clone(), item);
            self.eval_lc_for_nested(loop_vars, depth + 1, body, results);
        }
        match saved {
            Some(v) => {
                self.variables.insert(name.clone(), v);
            }
            None => {
                self.variables.remove(name);
            }
        }
    }

    pub fn eval_user_function(
        &mut self,
        user_fn: &UserFunction,
        args: &[(Option<String>, Value)],
    ) -> Value {
        let saved = self.variables.clone();

        let mut pos_idx = 0;
        for (param_name, default_expr) in &user_fn.params {
            let val = Self::get_named_arg(args, param_name)
                .cloned()
                .or_else(|| {
                    let v = Self::get_positional_arg(args, pos_idx).cloned();
                    pos_idx += 1;
                    v
                })
                .or_else(|| default_expr.as_ref().map(|e| self.eval_expr(e)))
                .unwrap_or(Value::Undef);
            self.variables.insert(param_name.clone(), val);
        }

        let result = self.eval_expr(&user_fn.body_expr);
        self.variables = saved;
        result
    }

    pub fn eval_binary_op(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> Value {
        let lhs = self.eval_expr(left);
        let rhs = self.eval_expr(right);
        match (lhs, rhs) {
            (Value::Number(a), Value::Number(b)) => match op {
                BinaryOp::Add => Value::Number(a + b),
                BinaryOp::Subtract => Value::Number(a - b),
                BinaryOp::Multiply => Value::Number(a * b),
                BinaryOp::Divide => Value::Number(if b == 0.0 { f64::NAN } else { a / b }),
                BinaryOp::Modulo => Value::Number(a % b),
                BinaryOp::Exponent => Value::Number(a.powf(b)),
                BinaryOp::Less => Value::Bool(a < b),
                BinaryOp::Greater => Value::Bool(a > b),
                BinaryOp::LessEqual => Value::Bool(a <= b),
                BinaryOp::GreaterEqual => Value::Bool(a >= b),
                BinaryOp::Equal => Value::Bool((a - b).abs() < f64::EPSILON),
                BinaryOp::NotEqual => Value::Bool((a - b).abs() >= f64::EPSILON),
                BinaryOp::LogicalAnd => Value::Bool(a != 0.0 && b != 0.0),
                BinaryOp::LogicalOr => Value::Bool(a != 0.0 || b != 0.0),
                _ => Value::Undef,
            },
            (Value::Bool(a), Value::Bool(b)) => match op {
                BinaryOp::LogicalAnd => Value::Bool(a && b),
                BinaryOp::LogicalOr => Value::Bool(a || b),
                BinaryOp::Equal => Value::Bool(a == b),
                BinaryOp::NotEqual => Value::Bool(a != b),
                _ => Value::Undef,
            },
            (Value::String(a), Value::String(b)) => match op {
                BinaryOp::Equal => Value::Bool(a == b),
                BinaryOp::NotEqual => Value::Bool(a != b),
                _ => Value::Undef,
            },
            (Value::Number(s), Value::List(l)) | (Value::List(l), Value::Number(s))
                if matches!(op, BinaryOp::Multiply) =>
            {
                fn scale_list(l: &[Value], s: f64) -> Vec<Value> {
                    l.iter()
                        .map(|v| match v {
                            Value::Number(n) => Value::Number(n * s),
                            Value::List(inner) => Value::List(scale_list(inner, s)),
                            other => other.clone(),
                        })
                        .collect()
                }
                Value::List(scale_list(&l, s))
            }
            (Value::List(l), Value::Number(s)) if matches!(op, BinaryOp::Divide) => {
                fn div_list(l: &[Value], s: f64) -> Vec<Value> {
                    l.iter()
                        .map(|v| match v {
                            Value::Number(n) => {
                                Value::Number(if s == 0.0 { f64::NAN } else { n / s })
                            }
                            Value::List(inner) => Value::List(div_list(inner, s)),
                            other => other.clone(),
                        })
                        .collect()
                }
                Value::List(div_list(&l, s))
            }
            (Value::List(a), Value::List(b))
                if matches!(op, BinaryOp::Add | BinaryOp::Subtract) =>
            {
                let len = a.len().max(b.len());
                let result: Vec<Value> = (0..len)
                    .map(|i| {
                        let va = a.get(i).and_then(Value::as_number).unwrap_or(0.0);
                        let vb = b.get(i).and_then(Value::as_number).unwrap_or(0.0);
                        Value::Number(match op {
                            BinaryOp::Add => va + vb,
                            _ => va - vb,
                        })
                    })
                    .collect();
                Value::List(result)
            }
            _ => Value::Undef,
        }
    }

    #[allow(clippy::unused_self)]
    pub fn eval_echo(&self, args: &[(Option<String>, Value)]) {
        let parts: Vec<String> = args
            .iter()
            .map(|(name, val)| {
                let v = match val {
                    Value::Number(n) => format!("{n}"),
                    Value::Bool(b) => format!("{b}"),
                    Value::String(s) => format!("\"{s}\""),
                    Value::List(l) => format!("{l:?}"),
                    Value::Range(a, b, c) => format!("[{a}:{c}:{b}]"),
                    Value::Undef => "undef".into(),
                };
                match name {
                    Some(n) => format!("{n} = {v}"),
                    None => v,
                }
            })
            .collect();
        eprintln!("ECHO: {}", parts.join(", "));
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::missing_panics_doc,
        clippy::cognitive_complexity
    )]
    pub fn eval_builtin_function(&mut self, name: &str, args: &[Value]) -> Value {
        match name {
            // Trigonometric (OpenSCAD uses degrees)
            "sin" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.to_radians().sin())),
            "cos" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.to_radians().cos())),
            "tan" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.to_radians().tan())),
            "asin" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.asin().to_degrees())),
            "acos" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.acos().to_degrees())),
            "atan" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.atan().to_degrees())),
            "atan2" => {
                if args.len() >= 2 {
                    match (args[0].as_number(), args[1].as_number()) {
                        (Some(y), Some(x)) => Value::Number(y.atan2(x).to_degrees()),
                        _ => Value::Undef,
                    }
                } else {
                    Value::Undef
                }
            }

            // Math
            "abs" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.abs())),
            "sqrt" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.sqrt())),
            "exp" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.exp())),
            "ln" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.ln())),
            "log" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.log10())),
            "sign" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.signum())),
            "pow" => {
                if args.len() >= 2 {
                    match (args[0].as_number(), args[1].as_number()) {
                        (Some(a), Some(b)) => Value::Number(a.powf(b)),
                        _ => Value::Undef,
                    }
                } else {
                    Value::Undef
                }
            }

            // Rounding
            "round" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.round())),
            "ceil" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.ceil())),
            "floor" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| Value::Number(n.floor())),

            // Min/max
            "min" => args
                .iter()
                .filter_map(Value::as_number)
                .reduce(f64::min)
                .map_or(Value::Undef, Value::Number),
            "max" => args
                .iter()
                .filter_map(Value::as_number)
                .reduce(f64::max)
                .map_or(Value::Undef, Value::Number),

            // List/string operations
            "len" => match args.first() {
                Some(Value::List(l)) => Value::Number(l.len() as f64),
                Some(Value::String(s)) => Value::Number(s.len() as f64),
                _ => Value::Undef,
            },
            "concat" => {
                let mut result = Vec::new();
                for arg in args {
                    match arg {
                        Value::List(l) => result.extend(l.iter().cloned()),
                        other => result.push(other.clone()),
                    }
                }
                Value::List(result)
            }

            // Vector operations
            "norm" => {
                if let Some(Value::List(l)) = args.first() {
                    let sum_sq: f64 = l.iter().filter_map(Value::as_number).map(|n| n * n).sum();
                    Value::Number(sum_sq.sqrt())
                } else {
                    Value::Undef
                }
            }
            "cross" => {
                if args.len() >= 2 {
                    let a = args[0].to_number_list();
                    let b = args[1].to_number_list();
                    match (a, b) {
                        (Some(a), Some(b)) if a.len() >= 3 && b.len() >= 3 => Value::List(vec![
                            Value::Number(a[1].mul_add(b[2], -(a[2] * b[1]))),
                            Value::Number(a[2].mul_add(b[0], -(a[0] * b[2]))),
                            Value::Number(a[0].mul_add(b[1], -(a[1] * b[0]))),
                        ]),
                        _ => Value::Undef,
                    }
                } else {
                    Value::Undef
                }
            }

            // Type checking
            "is_undef" => Value::Bool(matches!(args.first(), Some(Value::Undef) | None)),
            "is_list" => Value::Bool(matches!(args.first(), Some(Value::List(_)))),
            "is_num" => Value::Bool(matches!(args.first(), Some(Value::Number(_)))),
            "is_string" => Value::Bool(matches!(args.first(), Some(Value::String(_)))),
            "is_bool" => Value::Bool(matches!(args.first(), Some(Value::Bool(_)))),

            // String operations
            "str" => {
                let s: String = args
                    .iter()
                    .map(|v| match v {
                        Value::Number(n) => format!("{n}"),
                        Value::Bool(b) => format!("{b}"),
                        Value::String(s) => s.clone(),
                        Value::Undef => "undef".into(),
                        Value::List(l) => format!("{l:?}"),
                        Value::Range(a, b, c) => format!("[{a}:{c}:{b}]"),
                    })
                    .collect::<String>();
                Value::String(s)
            }
            "chr" => args
                .first()
                .and_then(Value::as_number)
                .map_or(Value::Undef, |n| {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    char::from_u32(n as u32).map_or(Value::Undef, |c| Value::String(c.to_string()))
                }),
            "ord" => {
                if let Some(Value::String(s)) = args.first() {
                    s.chars()
                        .next()
                        .map_or(Value::Undef, |c| Value::Number(f64::from(c as u32)))
                } else {
                    Value::Undef
                }
            }

            // Random
            "rands" => {
                if args.len() >= 3 {
                    match (
                        args[0].as_number(),
                        args[1].as_number(),
                        args[2].as_number(),
                    ) {
                        (Some(min), Some(max), Some(count)) => {
                            let n = count as usize;
                            let seed = args.get(3).and_then(Value::as_number).unwrap_or(0.0) as u64;
                            let mut rng =
                                seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                            let vals: Vec<Value> = (0..n)
                                .map(|_| {
                                    rng =
                                        rng.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                                    let t = (rng >> 33) as f64 / (1u64 << 31) as f64;
                                    Value::Number(t.mul_add(max - min, min))
                                })
                                .collect();
                            Value::List(vals)
                        }
                        _ => Value::Undef,
                    }
                } else {
                    Value::Undef
                }
            }

            // Lookup
            "lookup" => {
                if args.len() >= 2 {
                    if let (Some(key), Some(Value::List(table))) =
                        (args[0].as_number(), args.get(1))
                    {
                        let pairs: Vec<(f64, f64)> = table
                            .iter()
                            .filter_map(|row| {
                                let nums = row.to_number_list()?;
                                if nums.len() >= 2 {
                                    Some((nums[0], nums[1]))
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if pairs.is_empty() {
                            return Value::Undef;
                        }
                        if key <= pairs[0].0 {
                            return Value::Number(pairs[0].1);
                        }
                        if key >= pairs.last().unwrap().0 {
                            return Value::Number(pairs.last().unwrap().1);
                        }
                        for w in pairs.windows(2) {
                            if key >= w[0].0 && key <= w[1].0 {
                                let t = (key - w[0].0) / (w[1].0 - w[0].0);
                                return Value::Number(t.mul_add(w[1].1 - w[0].1, w[0].1));
                            }
                        }
                        Value::Number(pairs.last().unwrap().1)
                    } else {
                        Value::Undef
                    }
                } else {
                    Value::Undef
                }
            }
            _ => {
                self.warnings.push(format!("Unknown function: {name}()"));
                Value::Undef
            }
        }
    }
}

fn format_value(value: &Value) -> String {
    match value {
        Value::Number(number) => number.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::List(values) => format!("{values:?}"),
        Value::String(value) => value.clone(),
        Value::Range(from, to, step) => format!("[{from}:{step}:{to}]"),
        Value::Undef => "undef".into(),
    }
}
