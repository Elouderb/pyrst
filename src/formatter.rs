//! Code formatter for pyrst source code.
//!
//! Provides AST-based formatting with consistent indentation, spacing, and line wrapping.

use crate::ast::*;
use crate::ast::FStrPart;
use crate::diag::{Error, Result};
use std::fmt::Write;

pub struct Formatter {
    output: String,
    indent_level: usize,
    indent_width: usize,  // spaces per indent
}

impl Formatter {
    // `_line_length` (soft wrap limit) is accepted for signature stability but
    // not yet honored by the formatter; the dead field has been removed.
    pub fn new(indent_width: usize, _line_length: usize) -> Self {
        Self {
            output: String::new(),
            indent_level: 0,
            indent_width,
        }
    }

    pub fn format_module(&mut self, m: &Module) -> Result<String> {
        for (i, stmt) in m.stmts.iter().enumerate() {
            self.format_stmt(stmt)?;

            // Add blank line between top-level statements (except imports)
            if i < m.stmts.len() - 1 && !matches!(stmt, Stmt::Import { .. }) {
                self.output.push('\n');
            }
        }

        // Ensure file ends with newline
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }

        Ok(self.output.clone())
    }

    fn format_stmt(&mut self, stmt: &Stmt) -> Result<()> {
        match stmt {
            Stmt::Func(f) => {
                let params = self.format_params(&f.params)?;
                self.writeln(&format!("def {}({}){} -> {}:",
                    f.name,
                    params,
                    "",  // decorators handled separately
                    self.format_type(&f.ret)
                ));
                self.indent_level += 1;
                for s in &f.body {
                    self.format_stmt(s)?;
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
                    let params = self.format_params(&m.params)?;
                    self.writeln(&format!("def {}({}){} -> {}:",
                        m.name,
                        params,
                        "",
                        self.format_type(&m.ret)
                    ));
                    self.indent_level += 1;
                    for s in &m.body {
                        self.format_stmt(s)?;
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
                let value_str = self.format_expr(value)?;
                self.writeln(&format!("{}{} = {}", target, type_str, value_str));
            }
            Stmt::If { cond, then, elifs, else_, .. } => {
                let cond_str = self.format_expr(cond)?;
                self.writeln(&format!("if {}:", cond_str));
                self.indent_level += 1;
                for s in then {
                    self.format_stmt(s)?;
                }
                self.indent_level -= 1;

                for (c, b) in elifs {
                    let c_str = self.format_expr(c)?;
                    self.writeln(&format!("elif {}:", c_str));
                    self.indent_level += 1;
                    for s in b {
                        self.format_stmt(s)?;
                    }
                    self.indent_level -= 1;
                }

                if let Some(b) = else_ {
                    self.writeln("else:");
                    self.indent_level += 1;
                    for s in b {
                        self.format_stmt(s)?;
                    }
                    self.indent_level -= 1;
                }
            }
            Stmt::While { cond, body, .. } => {
                let cond_str = self.format_expr(cond)?;
                self.writeln(&format!("while {}:", cond_str));
                self.indent_level += 1;
                for s in body {
                    self.format_stmt(s)?;
                }
                self.indent_level -= 1;
            }
            Stmt::For { targets, iter, body, .. } => {
                let iter_str = self.format_expr(iter)?;
                self.writeln(&format!("for {} in {}:", targets.join(", "), iter_str));
                self.indent_level += 1;
                for s in body {
                    self.format_stmt(s)?;
                }
                self.indent_level -= 1;
            }
            Stmt::Return(expr, _) => {
                if let Some(e) = expr {
                    let e_str = self.format_expr(e)?;
                    self.writeln(&format!("return {}", e_str));
                } else {
                    self.writeln("return");
                }
            }
            Stmt::Yield(e, _) => {
                let e_str = self.format_expr(e)?;
                self.writeln(&format!("yield {}", e_str));
            }
            Stmt::Pass(_) => self.writeln("pass"),
            Stmt::Break(_) => self.writeln("break"),
            Stmt::Continue(_) => self.writeln("continue"),
            Stmt::Assert { cond, msg, .. } => {
                let cond_str = self.format_expr(cond)?;
                if let Some(m) = msg {
                    let m_str = self.format_expr(m)?;
                    self.writeln(&format!("assert {}, {}", cond_str, m_str));
                } else {
                    self.writeln(&format!("assert {}", cond_str));
                }
            }
            Stmt::Raise { exc, .. } => {
                if let Some(e) = exc {
                    let e_str = self.format_expr(e)?;
                    self.writeln(&format!("raise {}", e_str));
                } else {
                    self.writeln("raise");
                }
            }
            Stmt::Expr(expr) => {
                let expr_str = self.format_expr(expr)?;
                self.writeln(&expr_str);
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                self.writeln("try:");
                self.indent_level += 1;
                for s in body {
                    self.format_stmt(s)?;
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
                        self.format_stmt(s)?;
                    }
                    self.indent_level -= 1;
                }

                if let Some(b) = else_ {
                    self.writeln("else:");
                    self.indent_level += 1;
                    for s in b {
                        self.format_stmt(s)?;
                    }
                    self.indent_level -= 1;
                }

                if let Some(b) = finally_ {
                    self.writeln("finally:");
                    self.indent_level += 1;
                    for s in b {
                        self.format_stmt(s)?;
                    }
                    self.indent_level -= 1;
                }
            }
            Stmt::With { ctx_expr, as_name, body, .. } => {
                let ctx_str = self.format_expr(ctx_expr)?;
                if let Some(n) = as_name {
                    self.writeln(&format!("with {} as {}:", ctx_str, n));
                } else {
                    self.writeln(&format!("with {}:", ctx_str));
                }
                self.indent_level += 1;
                for s in body {
                    self.format_stmt(s)?;
                }
                self.indent_level -= 1;
            }
            Stmt::Del { target, .. } => {
                let target_str = self.format_expr(target)?;
                self.writeln(&format!("del {}", target_str));
            }
            Stmt::Match { subject, arms, .. } => {
                let subject_str = self.format_expr(subject)?;
                self.writeln(&format!("match {}:", subject_str));
                self.indent_level += 1;
                for arm in arms {
                    let pat_str = self.format_pattern(&arm.pattern)?;
                    let guard_str = if let Some(g) = &arm.guard {
                        format!(" if {}", self.format_expr(g)?)
                    } else {
                        String::new()
                    };
                    self.writeln(&format!("case {}{}:", pat_str, guard_str));
                    self.indent_level += 1;
                    for s in &arm.body {
                        self.format_stmt(s)?;
                    }
                    self.indent_level -= 1;
                }
                self.indent_level -= 1;
            }
            Stmt::AttrAssign { obj, attr, value, .. } => {
                let obj_str = self.format_expr(obj)?;
                let value_str = self.format_expr(value)?;
                self.writeln(&format!("{}.{} = {}", obj_str, attr, value_str));
            }
            Stmt::IndexAssign { obj, idx, value, .. } => {
                let obj_str = self.format_expr(obj)?;
                let idx_str = self.format_expr(idx)?;
                let value_str = self.format_expr(value)?;
                self.writeln(&format!("{}[{}] = {}", obj_str, idx_str, value_str));
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
                let value_str = self.format_expr(value)?;
                self.writeln(&format!("{} {} {}", target, op_str, value_str));
            }
            Stmt::Unpack { targets, value, .. } => {
                let value_str = self.format_expr(value)?;
                self.writeln(&format!("({}) = {}", targets.join(", "), value_str));
            }
        }
        Ok(())
    }

    fn format_pattern(&self, pattern: &crate::ast::MatchPattern) -> Result<String> {
        use crate::ast::MatchPattern;
        let s = match pattern {
            MatchPattern::Wildcard => "_".to_string(),
            MatchPattern::Capture(name) => name.clone(),
            MatchPattern::Literal(expr) => self.format_expr(expr)?,
            MatchPattern::Or(patterns) => {
                let mut parts: Vec<String> = Vec::with_capacity(patterns.len());
                for p in patterns {
                    parts.push(self.format_pattern(p)?);
                }
                parts.join(" | ")
            }
        };
        Ok(s)
    }

    fn format_expr(&self, expr: &Expr) -> Result<String> {
        let s = match expr {
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
                            result.push_str(&self.format_expr(inner)?);
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
                let items = self.format_expr_list(elems)?;
                format!("[{}]", items)
            }
            Expr::Tuple(elems, _) => {
                let items = self.format_expr_list(elems)?;
                if elems.len() == 1 {
                    format!("({},)", items)
                } else {
                    format!("({})", items)
                }
            }
            Expr::Set(elems, _) => {
                let items = self.format_expr_list(elems)?;
                format!("{{{}}}", items)
            }
            Expr::Dict(pairs, _) => {
                let mut parts: Vec<String> = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    parts.push(format!("{}: {}", self.format_expr(k)?, self.format_expr(v)?));
                }
                format!("{{{}}}", parts.join(", "))
            }
            Expr::Call { callee, args, .. } => {
                let callee_str = self.format_expr(callee)?;
                let arg_strs = self.format_expr_list(args)?;
                format!("{}({})", callee_str, arg_strs)
            }
            Expr::Attr { obj, name, .. } => {
                format!("{}.{}", self.format_expr(obj)?, name)
            }
            Expr::Index { obj, idx, .. } => {
                format!("{}[{}]", self.format_expr(obj)?, self.format_expr(idx)?)
            }
            Expr::Slice { obj, start, stop, step, .. } => {
                let obj_str = self.format_expr(obj)?;
                let start_str = match start { Some(e) => self.format_expr(e)?, None => String::new() };
                let stop_str = match stop { Some(e) => self.format_expr(e)?, None => String::new() };
                let step_str = match step { Some(e) => self.format_expr(e)?, None => String::new() };
                if step.is_some() {
                    format!("{}[{}:{}:{}]", obj_str, start_str, stop_str, step_str)
                } else {
                    format!("{}[{}:{}]", obj_str, start_str, stop_str)
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
                format!("({} {} {})", self.format_expr(lhs)?, op_str, self.format_expr(rhs)?)
            }
            Expr::UnOp { op, expr, .. } => {
                let op_str = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "not ",
                    UnOp::BitNot => "~",
                };
                format!("{}{}", op_str, self.format_expr(expr)?)
            }
            Expr::ListComp { elt, targets, iter, cond, .. } => {
                let cond_str = match cond {
                    Some(c) => format!(" if {}", self.format_expr(c)?),
                    None => String::new(),
                };
                format!("[{} for {} in {}{}]", self.format_expr(elt)?, targets.join(", "), self.format_expr(iter)?, cond_str)
            }
            Expr::SetComp { elt, targets, iter, cond, .. } => {
                let cond_str = match cond {
                    Some(c) => format!(" if {}", self.format_expr(c)?),
                    None => String::new(),
                };
                format!("{{{} for {} in {}{}}}", self.format_expr(elt)?, targets.join(", "), self.format_expr(iter)?, cond_str)
            }
            Expr::DictComp { key, val, targets, iter, cond, .. } => {
                let cond_str = match cond {
                    Some(c) => format!(" if {}", self.format_expr(c)?),
                    None => String::new(),
                };
                format!("{{{}: {} for {} in {}{}}}", self.format_expr(key)?, self.format_expr(val)?, targets.join(", "), self.format_expr(iter)?, cond_str)
            }
            Expr::Lambda { params, body, .. } => {
                let param_strs = params.iter()
                    .map(|(name, _ty)| name.clone())  // Don't show inferred Any types
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("lambda {}: {}", param_strs, self.format_expr(body)?)
            }
            Expr::IfExp { test, body, orelse, .. } => {
                format!("{} if {} else {}", self.format_expr(body)?, self.format_expr(test)?, self.format_expr(orelse)?)
            }
            // No catch-all placeholder: if a future AST node is not handled
            // above, abort instead of emitting `/* complex expr */` and
            // corrupting the user's source.
            #[allow(unreachable_patterns)]
            other => {
                return Err(Error::Codegen(format!(
                    "pyrst fmt: cannot format expression ({:?}); formatting aborted to avoid corrupting source",
                    std::mem::discriminant(other)
                )));
            }
        };
        Ok(s)
    }

    /// Format a comma-separated list of expressions, aborting on the first
    /// expression that cannot be rendered.
    fn format_expr_list(&self, exprs: &[Expr]) -> Result<String> {
        let mut parts: Vec<String> = Vec::with_capacity(exprs.len());
        for e in exprs {
            parts.push(self.format_expr(e)?);
        }
        Ok(parts.join(", "))
    }

    fn format_params(&self, params: &[Param]) -> Result<String> {
        let mut parts: Vec<String> = Vec::with_capacity(params.len());
        for p in params {
            // Special case: 'self' parameter shouldn't have type annotation
            if p.name == "self" {
                parts.push("self".to_string());
                continue;
            }
            // EPIC-4 V2: a by-reference param had its `Mut[T]` annotation peeled
            // into `ty = T` + the `by_ref` flag at parse time, so re-wrap it here
            // to round-trip the surface syntax (`account: Mut[Account]`).
            let ty_str = if p.by_ref {
                format!("Mut[{}]", self.format_type(&p.ty))
            } else {
                self.format_type(&p.ty)
            };
            if let Some(default) = &p.default {
                parts.push(format!("{}: {} = {}", p.name, ty_str, self.format_expr(default)?));
            } else {
                parts.push(format!("{}: {}", p.name, ty_str));
            }
        }
        Ok(parts.join(", "))
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
            TypeExpr::Func(args, ret) => {
                let arg_strs = args.iter()
                    .map(|a| self.format_type(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Callable[[{}], {}]", arg_strs, self.format_type(ret))
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
///
/// Returns an error (and never a placeholder) for any AST node the formatter
/// cannot faithfully render, so that `pyrst fmt` can abort rather than corrupt
/// the user's source.
pub fn format(m: &Module) -> Result<String> {
    let mut formatter = Formatter::new(4, 100);  // 4-space indent, 100 char line length
    formatter.format_module(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Format `src`, asserting it round-trips: it parses, formats without
    /// error, and the formatted output itself parses. Returns the formatted
    /// string for further assertions.
    fn format_src(src: &str) -> String {
        let module = crate::parser::parse(src).expect("source should parse");
        let formatted = format(&module).expect("formatter should not abort on supported nodes");
        // Round-trip: the formatted output must itself parse.
        crate::parser::parse(&formatted).expect("formatted output should re-parse");
        formatted
    }

    /// Card 8d63b3af acceptance: a set comprehension must round-trip and the
    /// formatter must NEVER emit the old `/* complex expr */` placeholder.
    #[test]
    fn set_comprehension_round_trips_no_placeholder() {
        let src = "def main() -> None:\n    s: set[int] = {x*2 for x in range(5)}\n";
        let formatted = format_src(src);
        assert!(
            !formatted.contains("/* complex expr */"),
            "formatter must never emit a placeholder, got:\n{}",
            formatted
        );
        assert!(
            formatted.contains("{(x * 2) for x in range(5)}"),
            "set comprehension should be rendered as a set comp, got:\n{}",
            formatted
        );
    }

    /// Set comprehension with a condition round-trips.
    #[test]
    fn set_comprehension_with_cond_round_trips() {
        let src = "def main() -> None:\n    s: set[int] = {x for x in range(10) if (x % 2) == 0}\n";
        let formatted = format_src(src);
        assert!(!formatted.contains("/* complex expr */"));
        assert!(
            formatted.contains("{x for x in range(10) if"),
            "got:\n{}",
            formatted
        );
    }

    /// Card 8d63b3af acceptance: a dict comprehension must round-trip and never
    /// produce a placeholder.
    #[test]
    fn dict_comprehension_round_trips_no_placeholder() {
        let src = "def main() -> None:\n    d: dict[int, int] = {x: x*x for x in range(5)}\n";
        let formatted = format_src(src);
        assert!(
            !formatted.contains("/* complex expr */"),
            "formatter must never emit a placeholder, got:\n{}",
            formatted
        );
        assert!(
            formatted.contains("{x: (x * x) for x in range(5)}"),
            "dict comprehension should be rendered as a dict comp, got:\n{}",
            formatted
        );
    }

    /// Dict comprehension with a condition round-trips.
    #[test]
    fn dict_comprehension_with_cond_round_trips() {
        let src = "def main() -> None:\n    d: dict[int, int] = {x: x*2 for x in range(6) if (x % 2) == 0}\n";
        let formatted = format_src(src);
        assert!(!formatted.contains("/* complex expr */"));
        assert!(formatted.contains("{x: (x * 2) for x in range(6) if"), "got:\n{}", formatted);
    }

    /// A list comprehension (pre-existing support) still round-trips, guarding
    /// against regressions from the Result refactor.
    #[test]
    fn list_comprehension_still_round_trips() {
        let src = "def main() -> None:\n    xs: list[int] = [x*2 for x in range(5)]\n";
        let formatted = format_src(src);
        assert!(!formatted.contains("/* complex expr */"));
        assert!(formatted.contains("[(x * 2) for x in range(5)]"), "got:\n{}", formatted);
    }
}
