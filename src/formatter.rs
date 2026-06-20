//! Code formatter for pyrst source code.
//!
//! Provides AST-based formatting with consistent indentation, spacing, and line wrapping.

use crate::ast::*;
use crate::ast::FStrPart;
use std::fmt::Write;

pub struct Formatter {
    output: String,
    indent_level: usize,
    indent_width: usize,  // spaces per indent
    line_length: usize,   // target line length (soft limit)
}

impl Formatter {
    pub fn new(indent_width: usize, line_length: usize) -> Self {
        Self {
            output: String::new(),
            indent_level: 0,
            indent_width,
            line_length,
        }
    }

    pub fn format_module(&mut self, m: &Module) -> String {
        for (i, stmt) in m.stmts.iter().enumerate() {
            self.format_stmt(stmt);

            // Add blank line between top-level statements (except imports)
            if i < m.stmts.len() - 1 && !matches!(stmt, Stmt::Import { .. }) {
                self.output.push('\n');
            }
        }

        // Ensure file ends with newline
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }

        self.output.clone()
    }

    fn format_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Func(f) => {
                self.writeln(&format!("def {}({}){} -> {}:",
                    f.name,
                    self.format_params(&f.params),
                    "",  // decorators handled separately
                    self.format_type(&f.ret)
                ));
                self.indent_level += 1;
                for s in &f.body {
                    self.format_stmt(s);
                }
                self.indent_level -= 1;
                self.output.push('\n');
            }
            Stmt::Class(c) => {
                let bases = if c.bases.is_empty() {
                    String::new()
                } else {
                    format!("({})", c.bases.join(", "))
                };
                self.writeln(&format!("class {}{}:", c.name, bases));
                self.indent_level += 1;

                // Format fields
                for field in &c.fields {
                    self.writeln(&format!("{}: {}", field.name, self.format_type(&field.ty)));
                }

                if !c.fields.is_empty() && !c.methods.is_empty() {
                    self.output.push('\n');
                }

                // Format methods
                for m in &c.methods {
                    self.writeln(&format!("def {}({}){} -> {}:",
                        m.name,
                        self.format_params(&m.params),
                        "",
                        self.format_type(&m.ret)
                    ));
                    self.indent_level += 1;
                    for s in &m.body {
                        self.format_stmt(s);
                    }
                    self.indent_level -= 1;
                    self.output.push('\n');
                }

                self.indent_level -= 1;
            }
            Stmt::Import { path, names, .. } => {
                if names.is_empty() {
                    self.writeln(&format!("import {}", path.join(".")));
                } else {
                    let import_list = names.iter()
                        .map(|(name, alias)| {
                            if let Some(a) = alias {
                                format!("{} as {}", name, a)
                            } else {
                                name.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.writeln(&format!("from {} import {}", path.join("."), import_list));
                }
            }
            Stmt::Assign { target, ty, value, .. } => {
                let type_str = if let Some(t) = ty {
                    format!(": {} ", self.format_type(t))
                } else {
                    String::new()
                };
                self.writeln(&format!("{}{} = {}", target, type_str, self.format_expr(value)));
            }
            Stmt::If { cond, then, elifs, else_, .. } => {
                self.writeln(&format!("if {}:", self.format_expr(cond)));
                self.indent_level += 1;
                for s in then {
                    self.format_stmt(s);
                }
                self.indent_level -= 1;

                for (c, b) in elifs {
                    self.writeln(&format!("elif {}:", self.format_expr(c)));
                    self.indent_level += 1;
                    for s in b {
                        self.format_stmt(s);
                    }
                    self.indent_level -= 1;
                }

                if let Some(b) = else_ {
                    self.writeln("else:");
                    self.indent_level += 1;
                    for s in b {
                        self.format_stmt(s);
                    }
                    self.indent_level -= 1;
                }
            }
            Stmt::While { cond, body, .. } => {
                self.writeln(&format!("while {}:", self.format_expr(cond)));
                self.indent_level += 1;
                for s in body {
                    self.format_stmt(s);
                }
                self.indent_level -= 1;
            }
            Stmt::For { targets, iter, body, .. } => {
                self.writeln(&format!("for {} in {}:", targets.join(", "), self.format_expr(iter)));
                self.indent_level += 1;
                for s in body {
                    self.format_stmt(s);
                }
                self.indent_level -= 1;
            }
            Stmt::Return(expr, _) => {
                if let Some(e) = expr {
                    self.writeln(&format!("return {}", self.format_expr(e)));
                } else {
                    self.writeln("return");
                }
            }
            Stmt::Pass(_) => self.writeln("pass"),
            Stmt::Break(_) => self.writeln("break"),
            Stmt::Continue(_) => self.writeln("continue"),
            Stmt::Assert { cond, msg, .. } => {
                if let Some(m) = msg {
                    self.writeln(&format!("assert {}, {}", self.format_expr(cond), self.format_expr(m)));
                } else {
                    self.writeln(&format!("assert {}", self.format_expr(cond)));
                }
            }
            Stmt::Raise { exc, .. } => {
                if let Some(e) = exc {
                    self.writeln(&format!("raise {}", self.format_expr(e)));
                } else {
                    self.writeln("raise");
                }
            }
            Stmt::Expr(expr) => {
                self.writeln(&self.format_expr(expr));
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                self.writeln("try:");
                self.indent_level += 1;
                for s in body {
                    self.format_stmt(s);
                }
                self.indent_level -= 1;

                for h in handlers {
                    if let Some(exc_type) = &h.exc_type {
                        if let Some(name) = &h.exc_name {
                            self.writeln(&format!("except {} as {}:", exc_type, name));
                        } else {
                            self.writeln(&format!("except {}:", exc_type));
                        }
                    } else {
                        self.writeln("except:");
                    }
                    self.indent_level += 1;
                    for s in &h.body {
                        self.format_stmt(s);
                    }
                    self.indent_level -= 1;
                }

                if let Some(b) = else_ {
                    self.writeln("else:");
                    self.indent_level += 1;
                    for s in b {
                        self.format_stmt(s);
                    }
                    self.indent_level -= 1;
                }

                if let Some(b) = finally_ {
                    self.writeln("finally:");
                    self.indent_level += 1;
                    for s in b {
                        self.format_stmt(s);
                    }
                    self.indent_level -= 1;
                }
            }
            Stmt::With { ctx_expr, as_name, body, .. } => {
                if let Some(n) = as_name {
                    self.writeln(&format!("with {} as {}:", self.format_expr(ctx_expr), n));
                } else {
                    self.writeln(&format!("with {}:", self.format_expr(ctx_expr)));
                }
                self.indent_level += 1;
                for s in body {
                    self.format_stmt(s);
                }
                self.indent_level -= 1;
            }
            Stmt::Del { target, .. } => {
                self.writeln(&format!("del {}", self.format_expr(target)));
            }
            Stmt::Match { subject, arms, .. } => {
                self.writeln(&format!("match {}:", self.format_expr(subject)));
                self.indent_level += 1;
                for arm in arms {
                    let pat_str = self.format_pattern(&arm.pattern);
                    let guard_str = if let Some(g) = &arm.guard {
                        format!(" if {}", self.format_expr(g))
                    } else {
                        String::new()
                    };
                    self.writeln(&format!("case {}{}:", pat_str, guard_str));
                    self.indent_level += 1;
                    for s in &arm.body {
                        self.format_stmt(s);
                    }
                    self.indent_level -= 1;
                }
                self.indent_level -= 1;
            }
            Stmt::AttrAssign { obj, attr, value, .. } => {
                self.writeln(&format!("{}.{} = {}", obj, attr, self.format_expr(value)));
            }
            Stmt::IndexAssign { obj, idx, value, .. } => {
                self.writeln(&format!("{}[{}] = {}", obj, self.format_expr(idx), self.format_expr(value)));
            }
            Stmt::AugAssign { target, op, value, .. } => {
                let op_str = match op {
                    BinOp::Add => "+=",
                    BinOp::Sub => "-=",
                    BinOp::Mul => "*=",
                    BinOp::Div => "/=",
                    BinOp::Mod => "%=",
                    BinOp::FloorDiv => "//=",
                    _ => "+=",
                };
                self.writeln(&format!("{} {} {}", target, op_str, self.format_expr(value)));
            }
            Stmt::Unpack { targets, value, .. } => {
                self.writeln(&format!("({}) = {}", targets.join(", "), self.format_expr(value)));
            }
        }
    }

    fn format_pattern(&self, pattern: &crate::ast::MatchPattern) -> String {
        use crate::ast::MatchPattern;
        match pattern {
            MatchPattern::Wildcard => "_".to_string(),
            MatchPattern::Capture(name) => name.clone(),
            MatchPattern::Literal(expr) => self.format_expr(expr),
            MatchPattern::Or(patterns) => {
                let parts: Vec<String> = patterns.iter().map(|p| self.format_pattern(p)).collect();
                parts.join(" | ")
            }
        }
    }

    fn format_expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Int(n, _) => n.to_string(),
            Expr::Float(f, _) => f.to_string(),
            Expr::Bool(b, _) => b.to_string(),
            Expr::None_(_) => "None".to_string(),
            Expr::Str(s, _) => format!("\"{}\"", s.replace('\"', "\\\"")),
            Expr::FStr(parts, _) => {
                let mut result = String::from("f\"");
                for part in parts {
                    match part {
                        FStrPart::Lit(s) => result.push_str(&s.replace('\"', "\\\"")),
                        FStrPart::Interp(inner, spec) => {
                            result.push('{');
                            result.push_str(&self.format_expr(inner));
                            if let Some(s) = spec {
                                result.push(':');
                                result.push_str(s);
                            }
                            result.push('}');
                        }
                    }
                }
                result.push('"');
                result
            }
            Expr::Ident(n, _) => n.clone(),
            Expr::List(elems, _) => {
                let items = elems.iter()
                    .map(|e| self.format_expr(e))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{}]", items)
            }
            Expr::Tuple(elems, _) => {
                let items = elems.iter()
                    .map(|e| self.format_expr(e))
                    .collect::<Vec<_>>()
                    .join(", ");
                if elems.len() == 1 {
                    format!("({},)", items)
                } else {
                    format!("({})", items)
                }
            }
            Expr::Set(elems, _) => {
                let items = elems.iter()
                    .map(|e| self.format_expr(e))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{}}}", items)
            }
            Expr::Dict(pairs, _) => {
                let items = pairs.iter()
                    .map(|(k, v)| format!("{}: {}", self.format_expr(k), self.format_expr(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{}}}", items)
            }
            Expr::Call { callee, args, .. } => {
                let arg_strs = args.iter()
                    .map(|a| self.format_expr(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", self.format_expr(callee), arg_strs)
            }
            Expr::Attr { obj, name, .. } => {
                format!("{}.{}", self.format_expr(obj), name)
            }
            Expr::Index { obj, idx, .. } => {
                format!("{}[{}]", self.format_expr(obj), self.format_expr(idx))
            }
            Expr::Slice { obj, start, stop, step, .. } => {
                let start_str = start.as_ref().map(|e| self.format_expr(e)).unwrap_or_default();
                let stop_str = stop.as_ref().map(|e| self.format_expr(e)).unwrap_or_default();
                let step_str = step.as_ref().map(|e| self.format_expr(e)).unwrap_or_default();
                if step.is_some() {
                    format!("{}[{}:{}:{}]", self.format_expr(obj), start_str, stop_str, step_str)
                } else {
                    format!("{}[{}:{}]", self.format_expr(obj), start_str, stop_str)
                }
            }
            Expr::BinOp { op, lhs, rhs, .. } => {
                let op_str = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::FloorDiv => "//",
                    BinOp::Mod => "%",
                    BinOp::Pow => "**",
                    BinOp::Eq => "==",
                    BinOp::Ne => "!=",
                    BinOp::Lt => "<",
                    BinOp::Le => "<=",
                    BinOp::Gt => ">",
                    BinOp::Ge => ">=",
                    BinOp::And => "and",
                    BinOp::Or => "or",
                    BinOp::Is => "is",
                    BinOp::IsNot => "is not",
                    BinOp::In => "in",
                    BinOp::NotIn => "not in",
                    BinOp::BitAnd => "&",
                    BinOp::BitOr => "|",
                    BinOp::BitXor => "^",
                    BinOp::LShift => "<<",
                    BinOp::RShift => ">>",
                };
                format!("({} {} {})", self.format_expr(lhs), op_str, self.format_expr(rhs))
            }
            Expr::UnOp { op, expr, .. } => {
                let op_str = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "not ",
                    UnOp::BitNot => "~",
                };
                format!("{}{}", op_str, self.format_expr(expr))
            }
            Expr::ListComp { elt, target, iter, cond, .. } => {
                let cond_str = if let Some(c) = cond {
                    format!(" if {}", self.format_expr(c))
                } else {
                    String::new()
                };
                format!("[{} for {} in {}{}]", self.format_expr(elt), target, self.format_expr(iter), cond_str)
            }
            Expr::Lambda { params, body, .. } => {
                let param_strs = params.iter()
                    .map(|(name, _ty)| name.clone())  // Don't show inferred Any types
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("lambda {}: {}", param_strs, self.format_expr(body))
            }
            Expr::IfExp { test, body, orelse, .. } => {
                format!("{} if {} else {}", self.format_expr(body), self.format_expr(test), self.format_expr(orelse))
            }
            _ => "/* complex expr */".to_string(),
        }
    }

    fn format_params(&self, params: &[Param]) -> String {
        params.iter()
            .map(|p| {
                // Special case: 'self' parameter shouldn't have type annotation
                if p.name == "self" {
                    return "self".to_string();
                }
                if let Some(default) = &p.default {
                    format!("{}: {} = {}", p.name, self.format_type(&p.ty), self.format_expr(default))
                } else {
                    format!("{}: {}", p.name, self.format_type(&p.ty))
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn format_type(&self, ty: &TypeExpr) -> String {
        match ty {
            TypeExpr::Named(n) => n.clone(),
            TypeExpr::Generic(name, args) => {
                let arg_strs = args.iter()
                    .map(|a| self.format_type(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}[{}]", name, arg_strs)
            }
            TypeExpr::Tuple(types) => {
                let type_strs = types.iter()
                    .map(|t| self.format_type(t))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({})", type_strs)
            }
            TypeExpr::None_ => "None".to_string(),
        }
    }

    fn writeln(&mut self, line: &str) {
        for _ in 0..self.indent_level {
            let _ = write!(self.output, "{}", " ".repeat(self.indent_width));
        }
        let _ = writeln!(self.output, "{}", line);
    }
}

/// Format a pyrst module with consistent style.
pub fn format(m: &Module) -> String {
    let mut formatter = Formatter::new(4, 100);  // 4-space indent, 100 char line length
    formatter.format_module(m)
}
