//! v0 codegen — emits Rust source from a pyrst AST.
//!
//! Ownership strategy for v0: aggressive clone()-on-use. Strings become
//! `String`, lists become `Vec<T>`. Numbers are `Copy` so they pass through.
//! This is the "Pythonic default" that the user signed off on — explicit
//! borrowing comes later.

use std::collections::HashMap;
use std::fmt::Write;

use crate::ast::*;
use crate::diag::Result;
use crate::typeck::{Ty, TyCtx};

pub struct Codegen<'a> {
    pub ctx: &'a TyCtx,
    out: String,
    indent: usize,
    locals: HashMap<String, Ty>,
    declared: std::collections::HashSet<String>,
    current_class: Option<String>,
    dead_funcs: std::collections::HashSet<String>,  // Functions that are never called
}

impl<'a> Codegen<'a> {
    pub fn new(ctx: &'a TyCtx) -> Self {
        Self { ctx, out: String::new(), indent: 0, locals: HashMap::new(), declared: Default::default(), current_class: None, dead_funcs: Default::default() }
    }

    pub fn with_dead_funcs(mut self, dead: std::collections::HashSet<String>) -> Self {
        self.dead_funcs = dead;
        self
    }

    fn is_copy_type(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Int | Ty::Float | Ty::Bool | Ty::Unit)
    }

    fn type_of_expr(&self, e: &Expr) -> Ty {
        match e {
            Expr::Float(..) => Ty::Float,
            Expr::Int(..) => Ty::Int,
            Expr::Bool(..) => Ty::Bool,
            Expr::Str(..) | Expr::FStr(..) => Ty::Str,
            Expr::None_(_) => Ty::Unit,
            Expr::Ident(n, _) => self.locals.get(n.as_str()).cloned().unwrap_or(Ty::Unknown),
            Expr::UnOp { op: UnOp::Neg, expr, .. } => self.type_of_expr(expr),
            Expr::UnOp { op: UnOp::Not, .. } => Ty::Bool,
            Expr::UnOp { op: UnOp::BitNot, .. } => Ty::Int,
            Expr::BinOp { lhs, op, rhs, .. } => {
                let l = self.type_of_expr(lhs);
                let r = self.type_of_expr(rhs);
                match op {
                    BinOp::Div => Ty::Float,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod | BinOp::FloorDiv | BinOp::Pow => {
                        if l == Ty::Float || r == Ty::Float { Ty::Float } else { Ty::Int }
                    }
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                    | BinOp::And | BinOp::Or | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => Ty::Bool,
                    _ => Ty::Unknown,
                }
            }
            Expr::Attr { obj, name, .. } => {
                if let Ty::Class(cls) = self.type_of_expr(obj) {
                    if let Some(c) = self.ctx.classes.get(cls.as_str()) {
                        if let Some(f) = c.fields.iter().find(|f| &f.name == name) {
                            return Ty::from_type_expr(&f.ty).unwrap_or(Ty::Unknown);
                        }
                    }
                }
                Ty::Unknown
            }
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(n, _) = callee.as_ref() {
                    match n.as_str() {
                        "float" => Ty::Float,
                        "abs" => {
                            // abs returns the same type as its argument
                            if let Some(arg) = args.first() {
                                self.type_of_expr(arg)
                            } else {
                                Ty::Unknown
                            }
                        }
                        "int" | "len" | "ord" | "round" | "pow" | "sum" => Ty::Int,
                        "bool" | "any" | "all" => Ty::Bool,
                        "str" | "chr" | "input" => Ty::Str,
                        n => self.ctx.funcs.get(n).map(|s| s.ret.clone()).unwrap_or(Ty::Unknown),
                    }
                } else if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                    // Math module return types
                    if let Expr::Ident(modname, _) = obj.as_ref() {
                        if modname == "math" {
                            return match name.as_str() {
                                "isnan" | "isinf" | "isfinite" => Ty::Bool,
                                _ => Ty::Float,
                            };
                        }
                    }
                    // Class method dispatch
                    if let Ty::Class(cls) = self.type_of_expr(obj) {
                        self.ctx.get_method(&cls, name).map(|s| s.ret.clone()).unwrap_or(Ty::Unknown)
                    } else { Ty::Unknown }
                } else {
                    Ty::Unknown
                }
            }
            _ => Ty::Unknown,
        }
    }

    pub fn emit_module(mut self, m: &Module) -> Result<String> {
        // Preamble — pyrst stdlib shims live here.
        self.line("#![allow(unused_parens, unused_variables, unused_mut, dead_code)]");
        self.line("use std::io::Write;");
        self.line("");
        self.line("fn __py_fmt_float(x: f64) -> String {");
        self.line("    if x.fract() == 0.0 { format!(\"{:.1}\", x) } else { format!(\"{}\", x) }");
        self.line("}");
        self.line("fn __py_fmt_bool(x: bool) -> String {");
        self.line("    if x { \"True\".to_string() } else { \"False\".to_string() }");
        self.line("}");
        self.line("");
        self.line("// ----- user code -----");

        for s in &m.stmts {
            self.emit_top_stmt(s)?;
        }

        // Synthetic entry point: dispatch to user's `main()` if present.
        if self.ctx.funcs.contains_key("main") {
            self.line("");
            self.line("fn main() { user_main(); }");
        } else {
            self.line("");
            self.line("fn main() {}");
        }

        Ok(self.out)
    }

    fn emit_top_stmt(&mut self, s: &Stmt) -> Result<()> {
        match s {
            Stmt::Func(f) => {
                // Skip dead functions (not called anywhere) unless it's main
                if f.name != "main" && self.dead_funcs.contains(&f.name) {
                    self.line(&format!("// Dead function removed: {}", f.name));
                    return Ok(());
                }
                self.emit_func(f, /*method_of=*/ None)
            }
            Stmt::Class(c) => self.emit_class(c),
            other => {
                // Top-level non-decl statements are not yet supported (would need
                // collecting them into the synthetic main). v0 punts.
                self.line(&format!("// TODO: top-level stmt {:?}", std::mem::discriminant(other)));
                Ok(())
            }
        }
    }

    fn method_modifies_self(&self, body: &[Stmt]) -> bool {
        for stmt in body {
            match stmt {
                Stmt::AttrAssign { obj, .. } => {
                    if obj == "self" {
                        return true;
                    }
                }
                Stmt::If { then, elifs, else_, .. } => {
                    if self.method_modifies_self(then) {
                        return true;
                    }
                    for (_, elif_body) in elifs {
                        if self.method_modifies_self(elif_body) {
                            return true;
                        }
                    }
                    if let Some(else_body) = else_ {
                        if self.method_modifies_self(else_body) {
                            return true;
                        }
                    }
                }
                Stmt::While { body, .. } | Stmt::For { body, .. } => {
                    if self.method_modifies_self(body) {
                        return true;
                    }
                }
                Stmt::Try { body, handlers, else_, finally_, .. } => {
                    if self.method_modifies_self(body) {
                        return true;
                    }
                    for handler in handlers {
                        if self.method_modifies_self(&handler.body) {
                            return true;
                        }
                    }
                    if let Some(else_body) = else_ {
                        if self.method_modifies_self(else_body) {
                            return true;
                        }
                    }
                    if let Some(finally_body) = finally_ {
                        if self.method_modifies_self(finally_body) {
                            return true;
                        }
                    }
                }
                Stmt::With { body, .. } => {
                    if self.method_modifies_self(body) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn emit_func(&mut self, f: &Func, method_of: Option<&str>) -> Result<()> {
        let is_static = f.decorators.contains(&"staticmethod".to_string());
        let name = if f.name == "main" && method_of.is_none() {
            "user_main".to_string()
        } else {
            f.name.clone()
        };
        let mut sig = format!("fn {}(", name);
        let mut first = true;
        // Static methods don't get self; regular methods take &self or &mut self based on whether they modify self.
        if method_of.is_some() && !is_static && f.params.iter().any(|p| p.name == "self") {
            let needs_mut = self.method_modifies_self(&f.body);
            if needs_mut {
                sig.push_str("&mut self");
            } else {
                sig.push_str("&self");
            }
            first = false;
        }
        // Always skip `self` from the explicit params list.
        for p in f.params.iter().filter(|p| p.name != "self") {
            if !first { sig.push_str(", "); }
            first = false;
            let _ = write!(sig, "{}: {}", p.name, rust_ty(&Ty::from_type_expr(&p.ty)?));
        }
        let ret = Ty::from_type_expr(&f.ret)?;
        let ret_s = rust_ty(&ret);
        let _ = write!(sig, ") -> {} {{", ret_s);
        self.line(&sig);
        self.indent += 1;

        // Populate locals from parameters
        for p in &f.params {
            if p.name != "self" {
                let ty = Ty::from_type_expr(&p.ty)?;
                self.locals.insert(p.name.clone(), ty);
            }
        }

        // First pass: collect assignments to populate locals
        for s in &f.body {
            if let Stmt::Assign { target, ty: Some(type_expr), .. } = s {
                if let Ok(ty) = Ty::from_type_expr(type_expr) {
                    self.locals.insert(target.clone(), ty);
                }
            }
        }

        for s in &f.body {
            self.emit_stmt(s)?;
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        // Clear locals and declared for next function
        self.locals.clear();
        self.declared.clear();
        Ok(())
    }

    fn emit_class(&mut self, c: &ClassDef) -> Result<()> {
        // Collect fields: inherited first, then own fields (no duplicates).
        let mut all_fields: Vec<Param> = Vec::new();
        for base in &c.bases {
            if let Some(base_def) = self.ctx.classes.get(base.as_str()).cloned() {
                for f in &base_def.fields {
                    if !all_fields.iter().any(|ef: &Param| ef.name == f.name) {
                        all_fields.push(f.clone());
                    }
                }
            }
        }
        for f in &c.fields {
            if !all_fields.iter().any(|ef: &Param| ef.name == f.name) {
                all_fields.push(f.clone());
            }
        }

        let all_fields_copy = all_fields.iter().all(|f| {
            Ty::from_type_expr(&f.ty)
                .map(|ty| self.is_copy_type(&ty))
                .unwrap_or(false)
        });

        let derives = if all_fields_copy {
            "#[derive(Copy, Clone, Debug, PartialEq)]"
        } else {
            "#[derive(Clone, Debug, PartialEq)]"
        };
        self.line(derives);
        self.line(&format!("struct {} {{", c.name));
        self.indent += 1;
        for f in &all_fields {
            let ty = Ty::from_type_expr(&f.ty)?;
            self.line(&format!("{}: {},", f.name, rust_ty(&ty)));
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        self.current_class = Some(c.name.clone());

        // Dunder methods that become Rust trait impls instead of regular methods.
        let dunder_trait_names = ["__str__", "__repr__", "__add__", "__sub__", "__mul__",
                                   "__eq__", "__neg__", "__bool__", "__lt__"];

        let has_init = c.methods.iter().any(|m| m.name == "__init__");
        let has_lt = c.methods.iter().any(|m| m.name == "__lt__");
        let is_dataclass = c.is_dataclass;
        let has_regular_methods = c.methods.iter().any(|m|
            m.name != "__init__" && !dunder_trait_names.contains(&m.name.as_str())) || has_lt;

        if has_init || has_regular_methods || (is_dataclass && !has_init) {
            self.line(&format!("impl {} {{", c.name));
            self.indent += 1;

            // Emit new() constructor when __init__ is defined.
            if has_init {
                if let Some(init_fn) = c.methods.iter().find(|m| m.name == "__init__").cloned() {
                    let non_self: Vec<_> = init_fn.params.iter().filter(|p| p.name != "self").collect();
                    let param_strs: Result<Vec<_>> = non_self.iter()
                        .map(|p| {
                            let ty = Ty::from_type_expr(&p.ty)?;
                            Ok(format!("{}: {}", p.name, rust_ty(&ty)))
                        })
                        .collect();
                    let param_strs = param_strs?;
                    let param_names: Vec<_> = non_self.iter().map(|p| p.name.clone()).collect();
                    let defaults: Vec<String> = all_fields.iter().map(|f| {
                        let ty = Ty::from_type_expr(&f.ty).unwrap_or(Ty::Unknown);
                        let dv = match &ty {
                            Ty::Int => "0i64".to_string(),
                            Ty::Float => "0.0f64".to_string(),
                            Ty::Bool => "false".to_string(),
                            Ty::Str => "String::new()".to_string(),
                            _ => "Default::default()".to_string(),
                        };
                        format!("{}: {}", f.name, dv)
                    }).collect();
                    self.line(&format!("fn new({}) -> Self {{", param_strs.join(", ")));
                    self.indent += 1;
                    self.line(&format!("let mut __inst = {} {{ {} }};", c.name, defaults.join(", ")));
                    self.line(&format!("__inst.__init__({});", param_names.join(", ")));
                    self.line("__inst");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
            }

            // Auto-generate constructor for @dataclass without __init__
            if is_dataclass && !has_init {
                let param_strs: Result<Vec<_>> = all_fields.iter()
                    .map(|f| {
                        let ty = Ty::from_type_expr(&f.ty)?;
                        Ok(format!("{}: {}", f.name, rust_ty(&ty)))
                    })
                    .collect();
                let param_strs = param_strs?;
                let field_inits: Vec<_> = all_fields.iter().map(|f| f.name.clone()).collect();
                self.line(&format!("fn new({}) -> Self {{", param_strs.join(", ")));
                self.indent += 1;
                self.line(&format!("{} {{ {} }}", c.name, field_inits.join(", ")));
                self.indent -= 1;
                self.line("}");
                self.line("");
            }

            // Emit all methods except dunder-trait ones (including inherited methods).
            let class_name = c.name.clone();
            let mut emitted_methods = std::collections::HashSet::new();

            // Collect own method names first to identify overrides
            let own_method_names: std::collections::HashSet<String> = c.methods.iter()
                .map(|m| m.name.clone())
                .collect();

            // First, collect inherited methods from parent classes (skip if overridden)
            for base in &c.bases {
                if let Some(base_def) = self.ctx.classes.get(base.as_str()).cloned() {
                    for m in &base_def.methods {
                        if !dunder_trait_names.contains(&m.name.as_str())
                            && !emitted_methods.contains(&m.name)
                            && !own_method_names.contains(&m.name) {
                            self.emit_func(m, Some(&class_name))?;
                            emitted_methods.insert(m.name.clone());
                        }
                    }
                }
            }

            // Emit __super_ aliases for methods that are overridden
            for base in &c.bases {
                if let Some(base_def) = self.ctx.classes.get(base.as_str()).cloned() {
                    for m in &base_def.methods {
                        if !dunder_trait_names.contains(&m.name.as_str()) && own_method_names.contains(&m.name) {
                            // Child overrides this parent method — emit __super_ alias
                            let mut super_m = m.clone();
                            super_m.name = format!("__super_{}", m.name);
                            self.emit_func(&super_m, Some(&class_name))?;
                        }
                    }
                }
            }

            // Then emit own methods (these override inherited ones)
            for m in &c.methods {
                if dunder_trait_names.contains(&m.name.as_str()) {
                    // Special handling for __lt__: emit as __lt_impl
                    if m.name == "__lt__" {
                        self.line(&format!("fn __lt_impl(&self, other: &{}) -> bool {{", c.name));
                        self.indent += 1;
                        self.locals.insert("self".into(), Ty::Class(c.name.clone()));
                        self.locals.insert("other".into(), Ty::Class(c.name.clone()));
                        for s in &m.body { self.emit_stmt(s)?; }
                        self.locals.remove("self");
                        self.locals.remove("other");
                        self.indent -= 1;
                        self.line("}");
                        self.line("");
                    }
                    continue;
                }
                self.emit_func(m, Some(&class_name))?;
                emitted_methods.insert(m.name.clone());
            }

            self.indent -= 1;
            self.line("}");
            self.line("");
        }

        // Emit trait implementations for dunder methods.
        let c_methods = c.methods.clone();
        for m in &c_methods {
            match m.name.as_str() {
                "__str__" | "__repr__" => {
                    self.line(&format!("impl ::std::fmt::Display for {} {{", c.name));
                    self.indent += 1;
                    self.line("fn fmt(&self, __f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {");
                    self.indent += 1;
                    self.locals.insert("self".into(), Ty::Class(c.name.clone()));
                    let body = &m.body;
                    let split_at = if body.is_empty() { 0 } else { body.len() - 1 };
                    for s in &body[..split_at] { self.emit_stmt(s)?; }
                    if let Some(Stmt::Return(Some(e), _)) = body.last() {
                        let s = self.emit_expr(e)?;
                        self.line(&format!("write!(__f, \"{{}}\", {})", s));
                    } else {
                        if let Some(s) = body.last() { self.emit_stmt(s)?; }
                        self.line("Ok(())");
                    }
                    self.locals.remove("self");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__add__" => {
                    let other_param = m.params.iter().find(|p| p.name == "other");
                    let other_ty = other_param
                        .map(|p| Ty::from_type_expr(&p.ty).unwrap_or(Ty::Class(c.name.clone())))
                        .unwrap_or(Ty::Class(c.name.clone()));
                    let ret_ty = Ty::from_type_expr(&m.ret).unwrap_or(Ty::Class(c.name.clone()));
                    self.line(&format!("impl ::std::ops::Add<{}> for {} {{", rust_ty(&other_ty), c.name));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", rust_ty(&ret_ty)));
                    self.line(&format!("fn add(self, other: {}) -> {} {{", rust_ty(&other_ty), rust_ty(&ret_ty)));
                    self.indent += 1;
                    self.locals.insert("self".into(), Ty::Class(c.name.clone()));
                    self.locals.insert("other".into(), other_ty);
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.locals.remove("self");
                    self.locals.remove("other");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__eq__" => {
                    self.line(&format!("impl ::std::cmp::PartialEq for {} {{", c.name));
                    self.indent += 1;
                    self.line(&format!("fn eq(&self, other: &{}) -> bool {{", c.name));
                    self.indent += 1;
                    self.locals.insert("self".into(), Ty::Class(c.name.clone()));
                    self.locals.insert("other".into(), Ty::Class(c.name.clone()));
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.locals.remove("self");
                    self.locals.remove("other");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__sub__" => {
                    let other_param = m.params.iter().find(|p| p.name == "other");
                    let other_ty = other_param
                        .map(|p| Ty::from_type_expr(&p.ty).unwrap_or(Ty::Class(c.name.clone())))
                        .unwrap_or(Ty::Class(c.name.clone()));
                    let ret_ty = Ty::from_type_expr(&m.ret).unwrap_or(Ty::Class(c.name.clone()));
                    self.line(&format!("impl ::std::ops::Sub<{}> for {} {{", rust_ty(&other_ty), c.name));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", rust_ty(&ret_ty)));
                    self.line(&format!("fn sub(self, other: {}) -> {} {{", rust_ty(&other_ty), rust_ty(&ret_ty)));
                    self.indent += 1;
                    self.locals.insert("self".into(), Ty::Class(c.name.clone()));
                    self.locals.insert("other".into(), other_ty);
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.locals.remove("self");
                    self.locals.remove("other");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__mul__" => {
                    let other_param = m.params.iter().find(|p| p.name == "other");
                    let other_ty = other_param
                        .map(|p| Ty::from_type_expr(&p.ty).unwrap_or(Ty::Class(c.name.clone())))
                        .unwrap_or(Ty::Class(c.name.clone()));
                    let ret_ty = Ty::from_type_expr(&m.ret).unwrap_or(Ty::Class(c.name.clone()));
                    self.line(&format!("impl ::std::ops::Mul<{}> for {} {{", rust_ty(&other_ty), c.name));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", rust_ty(&ret_ty)));
                    self.line(&format!("fn mul(self, other: {}) -> {} {{", rust_ty(&other_ty), rust_ty(&ret_ty)));
                    self.indent += 1;
                    self.locals.insert("self".into(), Ty::Class(c.name.clone()));
                    self.locals.insert("other".into(), other_ty);
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.locals.remove("self");
                    self.locals.remove("other");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__neg__" => {
                    let ret_ty = Ty::from_type_expr(&m.ret).unwrap_or(Ty::Class(c.name.clone()));
                    self.line(&format!("impl ::std::ops::Neg for {} {{", c.name));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", rust_ty(&ret_ty)));
                    self.line(&format!("fn neg(self) -> {} {{", rust_ty(&ret_ty)));
                    self.indent += 1;
                    self.locals.insert("self".into(), Ty::Class(c.name.clone()));
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.locals.remove("self");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__lt__" => {
                    self.line(&format!("impl ::std::cmp::PartialOrd for {} {{", c.name));
                    self.indent += 1;
                    self.line(&format!("fn partial_cmp(&self, other: &{}) -> Option<::std::cmp::Ordering> {{", c.name));
                    self.indent += 1;
                    self.line("if self.__lt_impl(other) { Some(::std::cmp::Ordering::Less) }");
                    self.line("else if other.__lt_impl(self) { Some(::std::cmp::Ordering::Greater) }");
                    self.line("else { Some(::std::cmp::Ordering::Equal) }");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                _ => {}
            }
        }

        self.current_class = None;
        Ok(())
    }

    fn emit_stmt(&mut self, s: &Stmt) -> Result<()> {
        match s {
            Stmt::Pass(_) => self.line("// pass"),
            Stmt::Break(_) => self.line("break;"),
            Stmt::Continue(_) => self.line("continue;"),
            Stmt::Assert { cond, msg, .. } => {
                let c = self.emit_expr(cond)?;
                match msg {
                    Some(m) => {
                        let m_s = self.emit_expr(m)?;
                        self.line(&format!("assert!({}, \"{{}}\", {});", c, m_s));
                    }
                    None => {
                        self.line(&format!("assert!({});", c));
                    }
                }
            }
            Stmt::Raise { exc, .. } => {
                match exc {
                    None => self.line("panic!(\"explicit raise\");"),
                    Some(Expr::Call { callee, args, .. }) => {
                        let exc_type = if let Expr::Ident(n, _) = callee.as_ref() {
                            n.clone()
                        } else {
                            "Exception".into()
                        };
                        if let Some(first_arg) = args.first() {
                            let msg = self.emit_expr(first_arg)?;
                            self.line(&format!("panic!(\"{{}} panic: {{}}\", \"{}\", {});", exc_type, msg));
                        } else {
                            self.line(&format!("panic!(\"raised {}\");", exc_type));
                        }
                    }
                    Some(other) => {
                        let e = self.emit_expr(other)?;
                        self.line(&format!("panic!(\"{{}}\", {});", e));
                    }
                }
            }
            Stmt::Return(None, _) => self.line("return;"),
            Stmt::Return(Some(e), _) => {
                if matches!(e, Expr::None_(_)) {
                    self.line("return;");
                } else {
                    let s = self.emit_expr(e)?;
                    // Auto-clone non-Copy types when returning from methods that take &self
                    let should_clone = match e {
                        Expr::Attr { obj, name, .. } => {
                            if let Expr::Ident(obj_name, _) = obj.as_ref() {
                                if obj_name == "self" && self.current_class.is_some() {
                                    // Check if the attribute type is non-Copy
                                    if let Some(class_name) = &self.current_class {
                                        let all_fields = self.ctx.get_all_fields(class_name.as_str());
                                        all_fields.iter().any(|f| &f.name == name)
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };
                    if should_clone {
                        self.line(&format!("return {}.clone();", s));
                    } else {
                        self.line(&format!("return {};", s));
                    }
                }
            }
            Stmt::Expr(e) => {
                let s = self.emit_expr(e)?;
                self.line(&format!("{};", s));
            }
            Stmt::Assign { target, ty, value, .. } => {
                let v = self.emit_expr(value)?;
                let is_declared = self.declared.contains(target);
                if !is_declared {
                    self.declared.insert(target.clone());
                    match ty {
                        Some(t) => {
                            let ty_obj = Ty::from_type_expr(t)?;
                            self.locals.insert(target.clone(), ty_obj.clone());
                            self.line(&format!("let mut {}: {} = {};", target, rust_ty(&ty_obj), v));
                        }
                        None => {
                            self.line(&format!("let mut {} = {};", target, v));
                        }
                    }
                } else {
                    self.line(&format!("{} = {};", target, v));
                }
            }
            Stmt::Unpack { targets, value, .. } => {
                let v = self.emit_expr(value)?;
                self.line(&format!("let ({}) = {};", targets.join(", "), v));
            }
            Stmt::AugAssign { target, op, value, .. } => {
                let v = self.emit_expr(value)?;
                match op {
                    BinOp::FloorDiv => {
                        // Python's //= floors toward negative infinity; Rust's /= truncates toward zero
                        // Use explicit float division + floor cast for correctness
                        self.line(&format!("{} = ({} as f64 / {} as f64).floor() as i64;", target, target, v));
                    }
                    _ => {
                        let op_s = match op {
                            BinOp::Add => "+=", BinOp::Sub => "-=", BinOp::Mul => "*=", BinOp::Div => "/=",
                            BinOp::Mod => "%=",
                            _ => "+=", // fallback for other ops
                        };
                        self.line(&format!("{} {} {};", target, op_s, v));
                    }
                }
            }
            Stmt::If { cond, then, elifs, else_, .. } => {
                let narrowed = extract_narrowing(cond);
                let c = self.emit_expr(cond)?;
                self.line(&format!("if {} {{", c));
                self.indent += 1;
                if let Some((var, is_some)) = &narrowed {
                    if *is_some {
                        self.line(&format!("let {} = {}.unwrap();", var, var));
                    }
                }
                for s in then { self.emit_stmt(s)?; }
                self.indent -= 1;
                for (c, b) in elifs {
                    let cs = self.emit_expr(c)?;
                    self.line(&format!("}} else if {} {{", cs));
                    self.indent += 1;
                    for s in b { self.emit_stmt(s)?; }
                    self.indent -= 1;
                }
                if let Some(b) = else_ {
                    self.line("} else {");
                    self.indent += 1;
                    for s in b { self.emit_stmt(s)?; }
                    self.indent -= 1;
                }
                self.line("}");
            }
            Stmt::While { cond, body, .. } => {
                let c = self.emit_expr(cond)?;
                self.line(&format!("while {} {{", c));
                self.indent += 1;
                for s in body { self.emit_stmt(s)?; }
                self.indent -= 1;
                self.line("}");
            }
            Stmt::For { targets, iter, body, .. } => {
                // Check if element type is Copy to use .iter().copied() instead of .iter().cloned()
                let is_copy_elem = if let Expr::Ident(name, _) = iter {
                    self.locals.get(name.as_str()).or_else(|| self.ctx.vars.get(name.as_str()))
                        .map(|ty| if let Ty::List(inner) = ty {
                            matches!(inner.as_ref(), Ty::Int | Ty::Float | Ty::Bool)
                        } else { false })
                        .unwrap_or(false)
                } else {
                    false
                };
                let i = self.emit_expr(iter)?;
                let is_range = i.contains("..");
                let is_iterator = i.contains(".enumerate()") || i.contains(".zip(") ||
                                 i.contains(".cloned()") || i.contains(".copied()") ||
                                 i.contains(".keys()") || i.contains(".values()") ||
                                 i.contains(".items()");
                // For ranges, use into_iter(); for collections, use iter().cloned() or iter().copied().
                // If it's already an iterator (enumerate/zip), use directly.
                let iter_expr = if is_iterator {
                    i
                } else if is_range {
                    format!("({}).into_iter()", i)
                } else if is_copy_elem {
                    format!("{}.iter().copied()", i)
                } else {
                    format!("{}.iter().cloned()", i)
                };
                let pat = if targets.len() == 1 {
                    targets[0].clone()
                } else {
                    format!("({})", targets.join(", "))
                };
                self.line(&format!("for {} in {} {{", pat, iter_expr));
                self.indent += 1;
                for s in body { self.emit_stmt(s)?; }
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Import { .. } => {
                // Silently drop imports in v0
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                self.line("{");
                self.indent += 1;
                self.line("let __try_result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {");
                self.indent += 1;
                for s in body { self.emit_stmt(s)?; }
                self.indent -= 1;
                self.line("}));");

                if !handlers.is_empty() || else_.is_some() {
                    self.line("if let Err(_) = __try_result {");
                    self.indent += 1;
                    for h in handlers {
                        for s in &h.body { self.emit_stmt(s)?; }
                    }
                    self.indent -= 1;
                    self.line("}");

                    if let Some(else_body) = else_ {
                        self.line("if let Ok(_) = __try_result {");
                        self.indent += 1;
                        for s in else_body { self.emit_stmt(s)?; }
                        self.indent -= 1;
                        self.line("}");
                    }
                }

                if let Some(fin) = finally_ {
                    for s in fin { self.emit_stmt(s)?; }
                }
                self.indent -= 1;
                self.line("}");
            }
            Stmt::With { ctx_expr, as_name, body, .. } => {
                let ctx_s = self.emit_expr(ctx_expr)?;
                self.line("{");
                self.indent += 1;
                if let Some(name) = as_name {
                    self.line(&format!("let mut {} = {};", name, ctx_s));
                } else {
                    self.line(&format!("let _ = {};", ctx_s));
                }
                for s in body { self.emit_stmt(s)?; }
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Del { target, .. } => {
                let t = self.emit_expr(target)?;
                self.line(&format!("drop({});", t));
            }
            Stmt::AttrAssign { obj, attr, value, .. } => {
                let v = self.emit_expr(value)?;
                self.line(&format!("{}.{} = {};", obj, attr, v));
            }
            Stmt::IndexAssign { obj, idx, value, .. } => {
                let i = self.emit_expr(idx)?;
                let v = self.emit_expr(value)?;
                // Check if obj is a dict or list based on type info
                let is_dict = self.locals.get(obj)
                    .or_else(|| self.ctx.vars.get(obj))
                    .map(|t| matches!(t, Ty::Dict(..)))
                    .unwrap_or(false);
                if is_dict {
                    self.line(&format!("{}.insert({}, {});", obj, i, v));
                } else {
                    self.line(&format!("{}[{} as usize] = {};", obj, i, v));
                }
            }
            Stmt::Match { subject, arms, .. } => {
                let subj = self.emit_expr(subject)?;
                let temp_var = "__match_val".to_string();
                self.line(&format!("let {} = {};", temp_var, subj));

                // Emit as if/else chain
                let mut first = true;
                for (idx, arm) in arms.iter().enumerate() {
                    let is_last = idx == arms.len() - 1;
                    let is_catchall = matches!(&arm.pattern, crate::ast::MatchPattern::Wildcard | crate::ast::MatchPattern::Capture(_));

                    let cond = self.emit_pattern_cond(&temp_var, &arm.pattern)?;
                    let guard_str = if let Some(guard) = &arm.guard {
                        let g = self.emit_expr(guard)?;
                        format!(" && {}", g)
                    } else {
                        String::new()
                    };

                    if first {
                        self.line(&format!("if {}{} {{", cond, guard_str));
                        first = false;
                    } else if is_last && is_catchall && guard_str.is_empty() {
                        // Last arm is catchall without guard — emit as plain else
                        self.line("} else {");
                    } else {
                        self.line(&format!("}} else if {}{} {{", cond, guard_str));
                    }
                    self.indent += 1;
                    for s in &arm.body {
                        self.emit_stmt(s)?;
                    }
                    self.indent -= 1;
                }
                if !first {
                    self.line("}");
                }
            }
            Stmt::Func(_) | Stmt::Class(_) => {
                // Nested functions/classes — punt.
                self.line("// TODO: nested function/class");
            }
        }
        Ok(())
    }

    fn emit_expr(&mut self, e: &Expr) -> Result<String> {
        Ok(match e {
            Expr::Int(n, _) => format!("({}i64)", n),
            Expr::Float(f, _) => format!("({}f64)", f),
            Expr::Bool(b, _) => b.to_string(),
            Expr::None_(_) => "None".to_string(),
            Expr::Str(s, _) => format!("String::from({:?})", s),
            Expr::FStr(parts, _) => {
                let mut fmt_str = String::new();
                let mut args = Vec::new();
                for part in parts {
                    match part {
                        crate::ast::FStrPart::Lit(s) => {
                            // Escape { and } in the format string
                            fmt_str.push_str(&s.replace('{', "{{").replace('}', "}}"));
                        }
                        crate::ast::FStrPart::Interp(src, spec) => {
                            let fmt_placeholder = match spec {
                                None => "{}".to_string(),
                                Some(s) => {
                                    // Strip Python type suffix (f, d, s, g, e, %) from spec
                                    let clean = s.trim_end_matches(|c: char| "fdsge%".contains(c));
                                    format!("{{:{}}}", clean)
                                }
                            };
                            fmt_str.push_str(&fmt_placeholder);
                            args.push(src.clone());
                        }
                    }
                }
                if args.is_empty() {
                    format!("String::from(\"{}\")", fmt_str)
                } else {
                    format!("format!(\"{}\", {})", fmt_str, args.join(", "))
                }
            }
            Expr::List(elems, _) => {
                let mut parts = Vec::new();
                for e in elems { parts.push(self.emit_expr(e)?); }
                format!("vec![{}]", parts.join(", "))
            }
            Expr::Tuple(elems, _) => {
                let parts: Result<Vec<_>> = elems.iter().map(|e| self.emit_expr(e)).collect();
                let parts = parts?;
                match parts.len() {
                    0 => "()".to_string(),
                    1 => format!("({},)", parts[0]),
                    _ => format!("({})", parts.join(", ")),
                }
            }
            Expr::ListComp { elt, target, iter, cond, .. } => {
                let iter_s = self.emit_expr(iter)?;
                let is_range = iter_s.contains("..");
                let chain = if is_range {
                    format!("({}).into_iter()", iter_s)
                } else {
                    format!("{}.iter().cloned()", iter_s)
                };
                let elt_s = self.emit_expr(elt)?;
                if let Some(cond_expr) = cond {
                    let cond_s = self.emit_expr(cond_expr)?;
                    format!("{}.filter(|{}| {}).map(|{}| {}).collect::<Vec<_>>()",
                        chain, target, cond_s, target, elt_s)
                } else {
                    format!("{}.map(|{}| {}).collect::<Vec<_>>()", chain, target, elt_s)
                }
            }
            Expr::Set(elems, _) => {
                if elems.is_empty() {
                    return Ok("::std::collections::HashSet::new()".to_string());
                }
                let mut items = Vec::new();
                for e in elems {
                    let es = self.emit_expr(e)?;
                    items.push(es);
                }
                format!("vec![{}].into_iter().collect::<::std::collections::HashSet<_>>()",
                    items.join(", "))
            }
            Expr::Dict(pairs, _) => {
                if pairs.is_empty() {
                    return Ok("::std::collections::HashMap::new()".to_string());
                }
                let mut inserts = Vec::new();
                for (k, v) in pairs {
                    let ks = self.emit_expr(k)?;
                    let vs = self.emit_expr(v)?;
                    inserts.push(format!("({}, {})", ks, vs));
                }
                format!("vec![{}].into_iter().collect::<::std::collections::HashMap<_,_>>()",
                    inserts.join(", "))
            }
            Expr::Ident(n, _) => n.clone(),
            Expr::Call { callee, args, kwargs, .. } => {
                // Multi-arg print with inline format
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "print" {
                        if args.is_empty() {
                            return Ok("println!(\"\")".to_string());
                        }
                        let mut parts: Vec<String> = Vec::new();
                        for arg in args {
                            let raw = self.emit_expr(arg)?;
                            let formatted = match self.type_of_expr(arg) {
                                Ty::Float => format!("__py_fmt_float({})", raw),
                                Ty::Bool => format!("__py_fmt_bool({})", raw),
                                _ => raw,
                            };
                            parts.push(formatted);
                        }
                        // Use {} (Display format) for most types; {:?} breaks strings by adding quotes
                        let fmt = (0..parts.len()).map(|_| "{}").collect::<Vec<_>>().join(" ");
                        return Ok(format!("println!(\"{}\" {})", fmt,
                            if parts.is_empty() { "".to_string() } else { format!(", {}", parts.join(", ")) }));
                    }
                }

                // Inline range() with 1, 2, or 3 args
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "range" {
                        if args.len() == 1 {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("(0..{})", a));
                        } else if args.len() == 2 {
                            let a = self.emit_expr(&args[0])?;
                            let b = self.emit_expr(&args[1])?;
                            return Ok(format!("({}..{})", a, b));
                        } else if args.len() == 3 {
                            let a = self.emit_expr(&args[0])?;
                            let b = self.emit_expr(&args[1])?;
                            let step = self.emit_expr(&args[2])?;
                            return Ok(format!("({}..{}).step_by({} as usize)", a, b, step));
                        }
                    }
                }

                // Inline enumerate(iter) — emits iterator chain without collecting
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "enumerate" && args.len() == 1 {
                        let a = self.emit_expr(&args[0])?;
                        let is_range = a.contains("..");
                        let iter_chain = if is_range {
                            format!("({}).into_iter()", a)
                        } else {
                            format!("{}.iter().cloned()", a)
                        };
                        return Ok(format!("{}.enumerate().map(|(i, v)| (i as i64, v))", iter_chain));
                    }
                }

                // Inline zip(a, b) — emits iterator chain without collecting
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "zip" && args.len() == 2 {
                        let a = self.emit_expr(&args[0])?;
                        let b = self.emit_expr(&args[1])?;
                        let is_range_a = a.contains("..");
                        let is_range_b = b.contains("..");
                        let iter_a = if is_range_a { format!("({}).into_iter()", a) } else { format!("{}.iter().cloned()", a) };
                        let iter_b = if is_range_b { format!("({}).into_iter()", b) } else { format!("{}.iter().cloned()", b) };
                        return Ok(format!("{}.zip({})", iter_a, iter_b));
                    }
                }

                // Builtin function dispatch
                if let Expr::Ident(n, _) = callee.as_ref() {
                    match n.as_str() {
                        "len" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("{}.len() as i64", a));
                        }
                        "str" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("format!(\"{{}}\" , {})", a));
                        }
                        "int" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("({} as i64)", a));
                        }
                        "float" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("({} as f64)", a));
                        }
                        "bool" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("(({}) != 0)", a));
                        }
                        "abs" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("({}).abs()", a));
                        }
                        "min" => {
                            if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                                // min with key parameter
                                let a = self.emit_expr(&args[0])?;
                                // Check if key_expr is a Lambda to handle it specially
                                let key_code = if let Expr::Lambda { params, body, .. } = key_expr {
                                    // Lambda: extract parameter name and body, rename param to __x
                                    let param_name = params.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "__x".to_string());
                                    let saved_local = self.locals.get(&param_name).cloned();
                                    self.locals.insert(param_name.clone(), Ty::Unknown);
                                    let body_s = self.emit_expr(body)?;
                                    if let Some(ty) = saved_local {
                                        self.locals.insert(param_name.clone(), ty);
                                    } else {
                                        self.locals.remove(param_name.as_str());
                                    }
                                    // Replace param_name with __x in the body
                                    body_s.replace(param_name.as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };
                                return Ok(format!(
                                    "{{ let __list = {}; __list.iter().min_by_key(|__x| {}).map(|__x| __x.clone()).unwrap_or_default() }}",
                                    a, key_code
                                ));
                            } else if args.len() == 1 {
                                let a = self.emit_expr(&args[0])?;
                                return Ok(format!("{}.iter().copied().min().unwrap_or(0)", a));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let b = self.emit_expr(&args[1])?;
                                return Ok(format!("::std::cmp::min({}, {})", a, b));
                            }
                        }
                        "max" => {
                            if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                                // max with key parameter
                                let a = self.emit_expr(&args[0])?;
                                // Check if key_expr is a Lambda to handle it specially
                                let key_code = if let Expr::Lambda { params, body, .. } = key_expr {
                                    // Lambda: extract parameter name and body, rename param to __x
                                    let param_name = params.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "__x".to_string());
                                    let saved_local = self.locals.get(&param_name).cloned();
                                    self.locals.insert(param_name.clone(), Ty::Unknown);
                                    let body_s = self.emit_expr(body)?;
                                    if let Some(ty) = saved_local {
                                        self.locals.insert(param_name.clone(), ty);
                                    } else {
                                        self.locals.remove(param_name.as_str());
                                    }
                                    // Replace param_name with __x in the body
                                    body_s.replace(param_name.as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };
                                return Ok(format!(
                                    "{{ let __list = {}; __list.iter().max_by_key(|__x| {}).map(|__x| __x.clone()).unwrap_or_default() }}",
                                    a, key_code
                                ));
                            } else if args.len() == 1 {
                                let a = self.emit_expr(&args[0])?;
                                return Ok(format!("{}.iter().copied().max().unwrap_or(0)", a));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let b = self.emit_expr(&args[1])?;
                                return Ok(format!("::std::cmp::max({}, {})", a, b));
                            }
                        }
                        "sorted" => {
                            let a = self.emit_expr(&args[0])?;
                            if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                                // sorted with key parameter
                                // Check if key_expr is a Lambda to handle it specially
                                let key_code = if let Expr::Lambda { params, body, .. } = key_expr {
                                    // Lambda: extract parameter name and body, rename param to __x
                                    let param_name = params.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "__x".to_string());
                                    let saved_local = self.locals.get(&param_name).cloned();
                                    self.locals.insert(param_name.clone(), Ty::Unknown);
                                    let body_s = self.emit_expr(body)?;
                                    if let Some(ty) = saved_local {
                                        self.locals.insert(param_name.clone(), ty);
                                    } else {
                                        self.locals.remove(param_name.as_str());
                                    }
                                    // Replace param_name with __x in the body
                                    body_s.replace(param_name.as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };
                                return Ok(format!(
                                    "{{ let mut __sorted = {}.clone(); __sorted.sort_by_key(|__x| {}); __sorted }}",
                                    a, key_code
                                ));
                            } else if let Some((_, rev_expr)) = kwargs.iter().find(|(n, _)| n == "reverse") {
                                // sorted with reverse parameter
                                let rev_s = self.emit_expr(rev_expr)?;
                                return Ok(format!(
                                    "{{ let mut __sorted = {}.clone(); __sorted.sort(); if {} {{ __sorted.reverse(); }} __sorted }}",
                                    a, rev_s
                                ));
                            } else {
                                // Default sorted
                                return Ok(format!("{{ let mut __sorted = {}.clone(); __sorted.sort(); __sorted }}", a));
                            }
                        }
                        "sum" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("{}.iter().sum::<i64>()", a));
                        }
                        "input" => {
                            if args.is_empty() {
                                return Ok("{ let mut __s = String::new(); ::std::io::stdin().read_line(&mut __s).unwrap(); __s.trim_end().to_string() }".to_string());
                            } else {
                                let p = self.emit_expr(&args[0])?;
                                return Ok(format!("{{ print!(\"{{}}\" , {}); ::std::io::stdout().flush().ok(); let mut __s = String::new(); ::std::io::stdin().read_line(&mut __s).unwrap(); __s.trim_end().to_string() }}", p));
                            }
                        }
                        "any" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("{}.iter().any(|x| *x)", a));
                        }
                        "all" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("{}.iter().all(|x| *x)", a));
                        }
                        "round" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("({} as f64).round() as i64", a));
                        }
                        "pow" => {
                            let base = self.emit_expr(&args[0])?;
                            let exp = self.emit_expr(&args[1])?;
                            return Ok(format!("({} as f64).powi({} as i32) as i64", base, exp));
                        }
                        "chr" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("(char::from_u32({} as u32).unwrap()).to_string()", a));
                        }
                        "ord" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("({}.chars().next().unwrap() as i64)", a));
                        }
                        "reversed" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("{{ let mut __r = {}.clone(); __r.reverse(); __r }}", a));
                        }
                        "map" => {
                            let f = self.emit_expr(&args[0])?;
                            let it = self.emit_expr(&args[1])?;
                            return Ok(format!("{}.iter().cloned().map({}).collect::<Vec<_>>()", it, f));
                        }
                        "filter" => {
                            let f = self.emit_expr(&args[0])?;
                            let it = self.emit_expr(&args[1])?;
                            return Ok(format!("{}.iter().cloned().filter(|__x| ({})((__x).clone())).collect::<Vec<_>>()", it, f));
                        }
                        "isinstance" => {
                            if args.len() != 2 {
                                return Err(crate::diag::Error::Codegen("isinstance requires exactly 2 arguments".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            // Check if args[1] is a builtin type identifier
                            if let Expr::Ident(type_name, _) = &args[1] {
                                let matches = match type_name.as_str() {
                                    "int" => matches!(&obj_type, Ty::Int),
                                    "str" => matches!(&obj_type, Ty::Str),
                                    "float" => matches!(&obj_type, Ty::Float),
                                    "bool" => matches!(&obj_type, Ty::Bool),
                                    "list" => matches!(&obj_type, Ty::List(_)),
                                    "dict" => matches!(&obj_type, Ty::Dict(_, _)),
                                    "set" => matches!(&obj_type, Ty::Set(_)),
                                    _ => {
                                        // For custom classes, emit runtime check
                                        let _obj = self.emit_expr(&args[0])?;
                                        return Ok(format!("true")); // Placeholder for custom class check
                                    }
                                };
                                return Ok(if matches { "true" } else { "false" }.to_string());
                            } else {
                                // Dynamic type check (not a literal type name)
                                return Ok("true".to_string()); // Conservative: assume true for dynamic checks
                            }
                        }
                        "type" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("type requires exactly 1 argument".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            let type_name = match obj_type {
                                Ty::Int => "<class 'int'>",
                                Ty::Str => "<class 'str'>",
                                Ty::Float => "<class 'float'>",
                                Ty::Bool => "<class 'bool'>",
                                Ty::List(_) => "<class 'list'>",
                                Ty::Dict(_, _) => "<class 'dict'>",
                                Ty::Set(_) => "<class 'set'>",
                                Ty::Unit => "<class 'NoneType'>",
                                _ => "<class 'object'>",
                            };
                            return Ok(format!("String::from(\"{}\")", type_name));
                        }
                        "hex" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("format!(\"{{:#x}}\", {})", a));
                        }
                        "oct" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("format!(\"{{:#o}}\", {})", a));
                        }
                        "bin" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("format!(\"{{:#b}}\", {})", a));
                        }
                        "callable" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("callable requires exactly 1 argument".into()));
                            }
                            // Check if the argument is a function name
                            if let Expr::Ident(name, _) = &args[0] {
                                let is_callable = self.ctx.funcs.contains_key(name.as_str()) ||
                                                 self.ctx.classes.contains_key(name.as_str());
                                return Ok(if is_callable { "true" } else { "false" }.to_string());
                            } else {
                                // For non-identifier expressions, conservatively return false
                                return Ok("false".to_string());
                            }
                        }
                        _ => {}
                    }
                }

                // Check if this is a class constructor call.
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if let Some(class_def) = self.ctx.classes.get(name.as_str()).cloned() {
                        let has_init = class_def.methods.iter().any(|m| m.name == "__init__");

                        // Use ::new() constructor when __init__ is defined and args provided.
                        if has_init && (!args.is_empty() || !kwargs.is_empty()) {
                            let mut call_parts = Vec::new();
                            for a in args { call_parts.push(self.emit_expr(a)?); }
                            for (_, v) in kwargs { call_parts.push(self.emit_expr(v)?); }
                            return Ok(format!("{}::new({})", name, call_parts.join(", ")));
                        }

                        // Class constructor: emit a Rust struct literal.
                        // Use inherited + own fields for positional.
                        let mut all_field_names: Vec<String> = Vec::new();
                        for base in &class_def.bases {
                            if let Some(bd) = self.ctx.classes.get(base.as_str()).cloned() {
                                for f in &bd.fields {
                                    if !all_field_names.contains(&f.name) {
                                        all_field_names.push(f.name.clone());
                                    }
                                }
                            }
                        }
                        for f in &class_def.fields {
                            if !all_field_names.contains(&f.name) {
                                all_field_names.push(f.name.clone());
                            }
                        }

                        if !args.is_empty() && kwargs.is_empty() {
                            // Positional args to a class constructor.
                            if args.len() != all_field_names.len() {
                                return Err(crate::diag::Error::Codegen(format!(
                                    "class `{}` has {} fields but {} positional arguments given",
                                    name, all_field_names.len(), args.len()
                                )));
                            }
                            let mut parts = Vec::new();
                            for (field_name, arg) in all_field_names.iter().zip(args.iter()) {
                                let v = self.emit_expr(arg)?;
                                parts.push(format!("{}: {}", field_name, v));
                            }
                            return Ok(format!("{} {{ {} }}", name, parts.join(", ")));
                        }

                        // Keyword-args form.
                        if !kwargs.is_empty() {
                            let mut parts = Vec::new();
                            for (kw, val) in kwargs {
                                let v = self.emit_expr(val)?;
                                parts.push(format!("{}: {}", kw, v));
                            }
                            return Ok(format!("{} {{ {} }}", name, parts.join(", ")));
                        }

                        // No args at all: emit default struct literal.
                        let mut parts = Vec::new();
                        for fname in &all_field_names {
                            let field = class_def.fields.iter().find(|f| &f.name == fname)
                                .or_else(|| {
                                    class_def.bases.iter().find_map(|b| {
                                        self.ctx.classes.get(b.as_str())
                                            .and_then(|bd| bd.fields.iter().find(|f| &f.name == fname))
                                    })
                                });
                            let default = if let Some(f) = field {
                                let ty = Ty::from_type_expr(&f.ty)?;
                                match ty {
                                    Ty::Int => "0i64".to_string(),
                                    Ty::Float => "0.0f64".to_string(),
                                    Ty::Bool => "false".to_string(),
                                    _ => "Default::default()".to_string(),
                                }
                            } else {
                                "Default::default()".to_string()
                            };
                            parts.push(format!("{}: {}", fname, default));
                        }
                        return Ok(format!("{} {{ {} }}", name, parts.join(", ")));
                    }
                }

                // Handle super().method(args)
                if let Expr::Attr { obj: super_call_expr, name: method_name, .. } = callee.as_ref() {
                    if let Expr::Call { callee: super_ident, args: super_args, .. } = super_call_expr.as_ref() {
                        if let Expr::Ident(n, _) = super_ident.as_ref() {
                            if n == "super" && super_args.is_empty() {
                                if let Some(_class_name) = self.current_class.clone() {
                                    // Call __super_ alias method which has parent's body
                                    let mut arg_parts = Vec::new();
                                    for a in args { arg_parts.push(self.emit_expr(a)?); }
                                    return Ok(format!("self.__super_{}({})", method_name, arg_parts.join(", ")));
                                }
                            }
                        }
                    }
                }

                // Method call with attribute callee — handle method name remapping
                if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                    // Math module functions
                    if let Expr::Ident(modname, _) = obj.as_ref() {
                        if modname == "math" {
                            let parts: Result<Vec<_>> = args.iter().map(|a| self.emit_expr(a)).collect();
                            let parts = parts?;
                            let a0 = parts.get(0).map(|s| s.as_str()).unwrap_or("0.0");
                            let a1 = parts.get(1).map(|s| s.as_str()).unwrap_or("0.0");
                            return Ok(match name.as_str() {
                                "sqrt"     => format!("({} as f64).sqrt()", a0),
                                "floor"    => format!("({} as f64).floor()", a0),
                                "ceil"     => format!("({} as f64).ceil()", a0),
                                "trunc"    => format!("({} as f64).trunc()", a0),
                                "fabs" | "abs" => format!("({} as f64).abs()", a0),
                                "exp"      => format!("({} as f64).exp()", a0),
                                "log"      => if parts.len() == 2 { format!("({} as f64).log({} as f64)", a0, a1) } else { format!("({} as f64).ln()", a0) },
                                "log2"     => format!("({} as f64).log2()", a0),
                                "log10"    => format!("({} as f64).log10()", a0),
                                "sin"      => format!("({} as f64).sin()", a0),
                                "cos"      => format!("({} as f64).cos()", a0),
                                "tan"      => format!("({} as f64).tan()", a0),
                                "asin"     => format!("({} as f64).asin()", a0),
                                "acos"     => format!("({} as f64).acos()", a0),
                                "atan"     => format!("({} as f64).atan()", a0),
                                "atan2"    => format!("({} as f64).atan2({} as f64)", a0, a1),
                                "pow"      => format!("({} as f64).powf({} as f64)", a0, a1),
                                "hypot"    => format!("({} as f64).hypot({} as f64)", a0, a1),
                                "degrees"  => format!("({} as f64).to_degrees()", a0),
                                "radians"  => format!("({} as f64).to_radians()", a0),
                                "isnan"    => format!("({} as f64).is_nan()", a0),
                                "isinf"    => format!("({} as f64).is_infinite()", a0),
                                "isfinite" => format!("({} as f64).is_finite()", a0),
                                _ => return Err(crate::diag::Error::Codegen(format!("unknown math function: math.{}", name))),
                            });
                        }
                    }

                    // Check for static method calls: ClassName.method(args)
                    if let Expr::Ident(class_name, _) = obj.as_ref() {
                        if let Some(class_def) = self.ctx.classes.get(class_name.as_str()) {
                            if let Some(method_def) = class_def.methods.iter().find(|m| &m.name == name) {
                                if method_def.decorators.contains(&"staticmethod".to_string()) {
                                    let parts: Result<Vec<_>> = args.iter().map(|a| self.emit_expr(a)).collect();
                                    let parts = parts?;
                                    return Ok(format!("{}::{}({})", class_name, name, parts.join(", ")));
                                }
                            }
                        }
                    }

                    let obj_s = self.emit_expr(obj)?;
                    let method = match name.as_str() {
                        // String methods
                        "upper"      => "to_uppercase",
                        "lower"      => "to_lowercase",
                        "strip"      => "trim",
                        "lstrip"     => "trim_start",
                        "rstrip"     => "trim_end",
                        // List methods
                        "append"     => "push",
                        "pop"        => "pop().unwrap",
                        // passthrough
                        other        => other,
                    };
                    let parts: Result<Vec<_>> = args.iter().map(|a| self.emit_expr(a)).collect();
                    let parts = parts?;

                    // Special case: split()
                    if name == "split" {
                        return if args.is_empty() {
                            Ok(format!("{}.split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>()", obj_s))
                        } else {
                            let sep = parts[0].clone();
                            Ok(format!("{}.split({}.as_str()).map(|s| s.to_string()).collect::<Vec<_>>()", obj_s, sep))
                        };
                    }

                    // Special case: join()
                    if name == "join" {
                        return Ok(format!("{}.join(&{})", parts[0], obj_s));
                    }

                    // Special case: len() as method
                    if name == "len" {
                        return Ok(format!("{}.len() as i64", obj_s));
                    }

                    // Special case: get() for dicts
                    if name == "get" {
                        let default = if parts.len() > 1 {
                            parts[1].clone()
                        } else {
                            "Default::default()".to_string()
                        };
                        return Ok(format!("{}.get(&{}).cloned().unwrap_or({})", obj_s, parts[0], default));
                    }

                    // String methods
                    if name == "startswith" && !parts.is_empty() {
                        return Ok(format!("{}.starts_with({}.as_str())", obj_s, parts[0]));
                    }
                    if name == "endswith" && !parts.is_empty() {
                        return Ok(format!("{}.ends_with({}.as_str())", obj_s, parts[0]));
                    }
                    if name == "replace" && parts.len() >= 2 {
                        return Ok(format!("{}.replace({}.as_str(), {}.as_str())", obj_s, parts[0], parts[1]));
                    }
                    if name == "partition" && !parts.is_empty() {
                        return Ok(format!(
                            "{{ let __s = {}.clone(); let __sep = {}.clone(); \
                            if let Some(__idx) = __s.find(__sep.as_str()) {{ \
                            vec![__s[..__idx].to_string(), __sep.clone(), __s[__idx + __sep.len()..].to_string()] \
                            }} else {{ vec![__s, String::new(), String::new()] }} }}",
                            obj_s, parts[0]
                        ));
                    }
                    if name == "rpartition" && !parts.is_empty() {
                        return Ok(format!(
                            "{{ let __s = {}.clone(); let __sep = {}.clone(); \
                            if let Some(__idx) = __s.rfind(__sep.as_str()) {{ \
                            vec![__s[..__idx].to_string(), __sep.clone(), __s[__idx + __sep.len()..].to_string()] \
                            }} else {{ vec![String::new(), String::new(), __s] }} }}",
                            obj_s, parts[0]
                        ));
                    }
                    if name == "find" && !parts.is_empty() {
                        return Ok(format!("{}.find({}.as_str()).map(|i| i as i64).unwrap_or(-1i64)", obj_s, parts[0]));
                    }
                    if name == "contains" && !parts.is_empty() {
                        return Ok(format!("{}.contains({}.as_str())", obj_s, parts[0]));
                    }

                    // String utility methods
                    if name == "isdigit" {
                        return Ok(format!("(if {}.chars().all(|c| c.is_numeric()) {{ \"True\" }} else {{ \"False\" }}).to_string()", obj_s));
                    }
                    if name == "isalpha" {
                        return Ok(format!("(if {}.chars().all(|c| c.is_alphabetic()) {{ \"True\" }} else {{ \"False\" }}).to_string()", obj_s));
                    }
                    if name == "isupper" {
                        return Ok(format!("(if {}.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) && {}.chars().any(|c| c.is_alphabetic()) {{ \"True\" }} else {{ \"False\" }}).to_string()", obj_s, obj_s));
                    }
                    if name == "islower" {
                        return Ok(format!("(if {}.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_lowercase()) && {}.chars().any(|c| c.is_alphabetic()) {{ \"True\" }} else {{ \"False\" }}).to_string()", obj_s, obj_s));
                    }
                    if name == "isspace" {
                        return Ok(format!("(if {}.chars().all(|c| c.is_whitespace()) {{ \"True\" }} else {{ \"False\" }}).to_string()", obj_s));
                    }
                    if name == "isalnum" {
                        return Ok(format!("(if {}.chars().all(|c| c.is_alphanumeric()) {{ \"True\" }} else {{ \"False\" }}).to_string()", obj_s));
                    }

                    // Additional string methods
                    if name == "capitalize" {
                        return Ok(format!(
                            "{{ let __s = {}.clone(); if __s.is_empty() {{ __s }} else {{ format!(\"{{}}{{}}\" , __s.chars().next().unwrap().to_uppercase(), &__s[1..].to_lowercase()) }} }}",
                            obj_s
                        ));
                    }
                    if name == "title" {
                        return Ok(format!(
                            "{{ let __s = {}.clone(); __s.split_whitespace().map(|w| if w.is_empty() {{ w.to_string() }} else {{ format!(\"{{}}{{}}\" , w.chars().next().unwrap().to_uppercase(), &w[1..].to_lowercase()) }} ).collect::<Vec<_>>().join(\" \") }}",
                            obj_s
                        ));
                    }
                    if name == "zfill" && !parts.is_empty() {
                        return Ok(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ format!(\"{{:0>width$}}\" , __s, width = __width) }} }}",
                            parts[0], obj_s
                        ));
                    }
                    if name == "ljust" && !parts.is_empty() {
                        return Ok(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ format!(\"{{:<width$}}\" , __s, width = __width) }} }}",
                            parts[0], obj_s
                        ));
                    }
                    if name == "rjust" && !parts.is_empty() {
                        return Ok(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ format!(\"{{:>width$}}\" , __s, width = __width) }} }}",
                            parts[0], obj_s
                        ));
                    }
                    if name == "center" && !parts.is_empty() {
                        return Ok(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ let __total = __width - __s.len(); let __left = (__total + 1) / 2; let __right = __total / 2; format!(\"{{}}{{}}{{}}\" , \" \".repeat(__left), __s, \" \".repeat(__right)) }} }}",
                            parts[0], obj_s
                        ));
                    }
                    if name == "swapcase" {
                        return Ok(format!(
                            "{{ let __s = {}.clone(); __s.chars().map(|c| if c.is_uppercase() {{ c.to_lowercase().to_string() }} else {{ c.to_uppercase().to_string() }} ).collect::<String>() }}",
                            obj_s
                        ));
                    }
                    if name == "splitlines" {
                        return Ok(format!(
                            "{}.lines().map(|l| l.to_string()).collect::<Vec<_>>()",
                            obj_s
                        ));
                    }
                    if name == "count" && !parts.is_empty() {
                        let obj_ty = self.type_of_expr(obj);
                        match obj_ty {
                            Ty::Str => {
                                return Ok(format!(
                                    "{{ let __s = {}.clone(); let __sub = {}.clone(); let mut __count = 0i64; let mut __start = 0; while let Some(__pos) = __s.as_str()[__start..].find(__sub.as_str()) {{ __count += 1; __start += __pos + __sub.len(); }} __count }}",
                                    obj_s, parts[0]
                                ));
                            }
                            _ => {} // Fall through to list count below
                        }
                    }
                    if name == "index" && !parts.is_empty() {
                        let obj_ty = self.type_of_expr(obj);
                        match obj_ty {
                            Ty::Str => {
                                return Ok(format!(
                                    "{}.find({}.as_str()).map(|i| i as i64).expect(\"substring not found\")",
                                    obj_s, parts[0]
                                ));
                            }
                            _ => {} // Fall through to list index below
                        }
                    }

                    // Dict methods - return iterators directly (will be wrapped by for loop)
                    if name == "keys" {
                        return Ok(format!("{}.keys().cloned()", obj_s));
                    }
                    if name == "values" {
                        return Ok(format!("{}.values().cloned()", obj_s));
                    }
                    if name == "items" {
                        return Ok(format!("{}.iter().map(|(k, v)| (k.clone(), v.clone()))", obj_s));
                    }
                    if name == "pop" {
                        if parts.is_empty() {
                            return Err(crate::diag::Error::Codegen("pop requires at least one argument".into()));
                        } else if parts.len() == 1 {
                            // pop(key) — panic if not found
                            return Ok(format!("{{ let mut __d = {}.clone(); __d.remove(&{}).expect(\"KeyError: key not found\") }}", obj_s, parts[0]));
                        } else {
                            // pop(key, default) — return default if not found
                            return Ok(format!("{{ let mut __d = {}.clone(); __d.remove(&{}).unwrap_or({}) }}", obj_s, parts[0], parts[1]));
                        }
                    }
                    // List methods
                    if name == "extend" && !parts.is_empty() {
                        return Ok(format!("{}.extend({}.clone())", obj_s, parts[0]));
                    }
                    if name == "insert" && parts.len() >= 2 {
                        return Ok(format!("{}.insert({} as usize, {})", obj_s, parts[0], parts[1]));
                    }
                    if name == "remove" && !parts.is_empty() {
                        return Ok(format!("{{ let __idx = {}.iter().position(|__x| *__x == {}).expect(\"value not found\"); {}.remove(__idx); }}", obj_s, parts[0], obj_s));
                    }
                    if name == "index" && !parts.is_empty() {
                        return Ok(format!("{}.iter().position(|__x| *__x == {}).expect(\"value not found\") as i64", obj_s, parts[0]));
                    }
                    if name == "count" && !parts.is_empty() {
                        return Ok(format!("{}.iter().filter(|__x| **__x == {}).count() as i64", obj_s, parts[0]));
                    }
                    if name == "reverse" {
                        return Ok(format!("{}.reverse()", obj_s));
                    }
                    if name == "sort" {
                        return Ok(format!("{}.sort()", obj_s));
                    }
                    if name == "clear" {
                        return Ok(format!("{}.clear()", obj_s));
                    }
                    if name == "copy" {
                        return Ok(format!("{}.clone()", obj_s));
                    }

                    // Regular method call
                    return Ok(format!("{}.{}({})", obj_s, method, parts.join(", ")));
                }

                // Regular function call (not a class).
                let mut parts = Vec::with_capacity(args.len());
                for a in args { parts.push(self.emit_expr(a)?); }

                // Inject default arguments for named functions
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if let Some(sig) = self.ctx.funcs.get(n.as_str()).cloned() {
                        let expected = sig.params.len();
                        if parts.len() < expected && !sig.param_defaults.is_empty() {
                            let defaults_needed = expected - parts.len();
                            let defaults_start = sig.param_defaults.len().saturating_sub(defaults_needed);
                            for def_expr in &sig.param_defaults[defaults_start..] {
                                match def_expr {
                                    Some(e) => parts.push(self.emit_expr(e)?),
                                    None => return Err(crate::diag::Error::Codegen("missing required argument".into())),
                                }
                            }
                        }
                    }
                }

                let callee_s = self.emit_expr(callee)?;
                // Parenthesize lambda expressions when used as callees
                let callee_s = if matches!(callee.as_ref(), Expr::Lambda { .. }) {
                    format!("({})", callee_s)
                } else {
                    callee_s
                };

                // kwargs on a non-class call site are an error in v0.
                if !kwargs.is_empty() {
                    return Err(crate::diag::Error::Codegen(
                        "keyword arguments are only supported for class constructors in v0".into()
                    ));
                }

                format!("{}({})", callee_s, parts.join(", "))
            }
            Expr::Attr { obj, name, .. } => {
                // Math module constants
                if let Expr::Ident(modname, _) = obj.as_ref() {
                    if modname == "math" {
                        return Ok(match name.as_str() {
                            "pi"  => "::std::f64::consts::PI".to_string(),
                            "e"   => "::std::f64::consts::E".to_string(),
                            "tau" => "::std::f64::consts::TAU".to_string(),
                            "inf" => "f64::INFINITY".to_string(),
                            "nan" => "f64::NAN".to_string(),
                            _ => return Err(crate::diag::Error::Codegen(format!("unknown math constant: {}", name))),
                        });
                    }
                }

                let o = self.emit_expr(obj)?;
                // Check if this is a @property access
                let is_property = if let Expr::Ident(var, _) = obj.as_ref() {
                    self.locals.get(var.as_str()).cloned()
                        .and_then(|ty| if let Ty::Class(cn) = ty {
                            self.ctx.classes.get(&cn).map(|cd|
                                cd.methods.iter().any(|m|
                                    m.name.as_str() == name.as_str()
                                    && m.decorators.contains(&"property".to_string())
                                )
                            )
                        } else { None })
                        .unwrap_or(false)
                } else {
                    false
                };
                if is_property {
                    format!("{}.{}()", o, name)
                } else {
                    format!("{}.{}", o, name)
                }
            }
            Expr::Index { obj, idx, .. } => {
                let obj_ty = if let Expr::Ident(name, _) = obj.as_ref() {
                    self.locals.get(name.as_str()).or_else(|| self.ctx.vars.get(name.as_str())).cloned()
                } else {
                    None
                };
                let o = self.emit_expr(obj)?;
                let i = self.emit_expr(idx)?;
                match obj_ty.as_ref() {
                    Some(Ty::Dict(..)) => format!("{}.get(&{}).cloned().expect(\"key not found\")", o, i),
                    Some(Ty::Str) => {
                        // String indexing with negative index support
                        format!(
                            "{{ let __chars: Vec<char> = {}.chars().collect(); let __idx = if {} < 0 {{ ((__chars.len() as i64) + {}) as usize }} else {{ {} as usize }}; __chars[__idx].to_string() }}",
                            o, i, i, i
                        )
                    }
                    _ => {
                        // List indexing with negative index support
                        format!(
                            "{{ let __list = {}.clone(); let __idx = if {} < 0 {{ ((__list.len() as i64) + {}) as usize }} else {{ {} as usize }}; __list[__idx].clone() }}",
                            o, i, i, i
                        )
                    }
                }
            }
            Expr::Slice { obj, start, stop, step, .. } => {
                let obj_ty = self.type_of_expr(obj);
                let o = self.emit_expr(obj)?;

                match obj_ty {
                    Ty::Str => {
                        // String slicing with negative index support
                        if step.is_some() {
                            return Err(crate::diag::Error::Codegen("string slicing with step not supported".into()));
                        }
                        let start_expr = start.as_ref().map(|e| self.emit_expr(e)).transpose()?;
                        let start_val = start_expr.map(|s| {
                            format!("(if {} < 0 {{ (({}.len() as i64) + {}) as usize }} else {{ {} as usize }})", s, o, s, s)
                        }).unwrap_or_else(|| "0usize".to_string());

                        let stop_expr = stop.as_ref().map(|e| self.emit_expr(e)).transpose()?;
                        let stop_val = stop_expr.map(|s| {
                            format!("(if {} < 0 {{ (({}.len() as i64) + {}) as usize }} else {{ {} as usize }})", s, o, s, s)
                        }).unwrap_or_else(|| format!("{}.len()", o));

                        format!("((&{}[{}..{}]).to_string())", o, start_val, stop_val)
                    }
                    Ty::List(_) => {
                        // List slicing with step support and negative index handling
                        match (start, stop, step) {
                            (Some(s), Some(e), None) => {
                                // Simple: x[start:stop]
                                let start_s = self.emit_expr(s)?;
                                let stop_s = self.emit_expr(e)?;
                                format!(
                                    "{{ let __list = {}.clone(); let __len = __list.len() as i64; let __start = if {} < 0 {{ ((__len + {}) as usize).min(__list.len()) }} else {{ ({} as usize).min(__list.len()) }}; let __stop = if {} < 0 {{ ((__len + {}) as usize).min(__list.len()) }} else {{ ({} as usize).min(__list.len()) }}; __list[__start..(__start + (__stop - __start))].to_vec() }}",
                                    o, start_s, start_s, start_s, stop_s, stop_s, stop_s
                                )
                            }
                            _ => {
                                // General with step and negative index handling
                                let start_val = start.as_ref().map(|e| self.emit_expr(e)).transpose()?.unwrap_or_else(|| "0i64".to_string());
                                let stop_val = stop.as_ref().map(|e| self.emit_expr(e)).transpose()?.unwrap_or_else(|| format!("{}.len() as i64", o));
                                let step_val = step.as_ref().map(|e| self.emit_expr(e)).transpose()?.unwrap_or_else(|| "1i64".to_string());

                                // Wrap values in parens for safety in comparisons
                                let start_expr = format!("({})", start_val);
                                let stop_expr = format!("({})", stop_val);

                                format!(
                                    "{{ let __list = {}.clone(); let mut __result = Vec::new(); let __len = __list.len() as i64; let __start = (if {} < 0 {{ (__len + {}) as usize }} else {{ {} as usize }}).min(__list.len()); let __stop = (if {} < 0 {{ (__len + {}) as usize }} else {{ {} as usize }}).min(__list.len()); let __step = {}; if __step > 0 {{ let mut __i = __start as i64; while __i < __stop as i64 {{ __result.push(__list[__i as usize].clone()); __i += __step; }} }} else if __step < 0 {{ let mut __i = (__stop as i64) - 1; while __i >= __start as i64 {{ __result.push(__list[__i as usize].clone()); __i += __step; }} }} __result }}",
                                    o, start_expr, start_val, start_val, stop_expr, stop_val, stop_val, step_val
                                )
                            }
                        }
                    }
                    _ => return Err(crate::diag::Error::Codegen(format!("slicing not supported for type {:?}", obj_ty))),
                }
            }
            Expr::BinOp { op, lhs, rhs, span } => {
                // Try constant folding first
                if let Some(folded) = try_fold_const(&Expr::BinOp {
                    op: *op,
                    lhs: lhs.clone(),
                    rhs: rhs.clone(),
                    span: *span,
                }) {
                    return self.emit_expr(&folded);
                }

                // Handle sequence repetition: "abc" * 3 and [0] * 5
                if *op == BinOp::Mul {
                    let lt = self.type_of_expr(lhs);
                    let rt = self.type_of_expr(rhs);
                    if lt == Ty::Str || rt == Ty::Str {
                        let (str_e, num_e) = if lt == Ty::Str { (lhs, rhs) } else { (rhs, lhs) };
                        let s = self.emit_expr(str_e)?;
                        let n = self.emit_expr(num_e)?;
                        return Ok(format!("{}.repeat({} as usize)", s, n));
                    }
                    if matches!(&lt, Ty::List(_)) || matches!(&rt, Ty::List(_)) {
                        let (lst_e, num_e) = if matches!(&lt, Ty::List(_)) { (lhs, rhs) } else { (rhs, lhs) };
                        let v = self.emit_expr(lst_e)?;
                        let n = self.emit_expr(num_e)?;
                        return Ok(format!(
                            "{{ let mut __rep: Vec<_> = Vec::new(); for _ in 0..({} as usize) {{ __rep.extend({}.clone().into_iter()); }} __rep }}",
                            n, v
                        ));
                    }
                }

                // Handle `x is None` / `x is not None` → .is_none() / .is_some()
                if matches!(op, BinOp::Is | BinOp::IsNot) && matches!(rhs.as_ref(), Expr::None_(_)) {
                    let l = self.emit_expr(lhs)?;
                    return Ok(match op {
                        BinOp::Is => format!("{}.is_none()", l),
                        BinOp::IsNot => format!("{}.is_some()", l),
                        _ => unreachable!(),
                    });
                }
                let l = self.emit_expr(lhs)?;
                let r = self.emit_expr(rhs)?;
                match op {
                    BinOp::Pow => return Ok(format!("(({} as f64).powf({} as f64))", l, r)),
                    BinOp::In => return Ok(format!("{}.contains(&{})", r, l)),
                    BinOp::NotIn => return Ok(format!("!{}.contains(&{})", r, l)),
                    _ => {
                        let op_s = match op {
                            BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*",
                            BinOp::Div => "/", BinOp::FloorDiv => "/", BinOp::Mod => "%",
                            BinOp::Eq => "==", BinOp::Ne => "!=",
                            BinOp::Lt => "<", BinOp::Le => "<=",
                            BinOp::Gt => ">", BinOp::Ge => ">=",
                            BinOp::And => "&&", BinOp::Or => "||",
                            BinOp::Is => "==", BinOp::IsNot => "!=",
                            BinOp::BitAnd => "&", BinOp::BitOr => "|", BinOp::BitXor => "^",
                            BinOp::LShift => "<<", BinOp::RShift => ">>",
                            BinOp::In | BinOp::NotIn => unreachable!(), // handled above
                            BinOp::Pow => unreachable!(), // handled above
                        };
                        format!("({} {} {})", l, op_s, r)
                    }
                }
            }
            Expr::UnOp { op, expr, span } => {
                // Try constant folding first
                if let Some(folded) = try_fold_const(&Expr::UnOp {
                    op: *op,
                    expr: expr.clone(),
                    span: *span,
                }) {
                    return self.emit_expr(&folded);
                }

                let e = self.emit_expr(expr)?;
                match op {
                    UnOp::Neg => format!("(-{})", e),
                    UnOp::Not => format!("(!{})", e),
                    UnOp::BitNot => format!("(!({}))", e),
                }
            }
            Expr::Lambda { params, body, .. } => {
                let param_strs: Vec<String> = params.iter().map(|(name, _ty)| {
                    // For now, default to i64 for all lambda parameters since we don't have full type info
                    format!("{}: i64", name)
                }).collect();
                let body_s = self.emit_expr(body)?;
                format!("|{}| {}", param_strs.join(", "), body_s)
            }
        })
    }

    fn emit_pattern_cond(&self, var: &str, pattern: &crate::ast::MatchPattern) -> Result<String> {
        use crate::ast::MatchPattern;
        match pattern {
            MatchPattern::Wildcard => Ok("true".to_string()),
            MatchPattern::Capture(_) => Ok("true".to_string()),
            MatchPattern::Literal(Expr::Int(n, _)) => {
                Ok(format!("{} == {}i64", var, n))
            }
            MatchPattern::Literal(Expr::Bool(b, _)) => {
                Ok(format!("{} == {}", var, b))
            }
            MatchPattern::Literal(Expr::Str(s, _)) => {
                Ok(format!("{} == {:?}", var, s))
            }
            MatchPattern::Literal(Expr::None_(_)) => {
                Ok(format!("{} == None", var))
            }
            MatchPattern::Literal(_) => {
                Ok("true".to_string())
            }
            MatchPattern::Or(patterns) => {
                let conds: Result<Vec<String>> = patterns.iter()
                    .map(|p| self.emit_pattern_cond(var, p))
                    .collect();
                let conds = conds?;
                Ok(format!("({})", conds.join(" || ")))
            }
        }
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.indent { self.out.push_str("    "); }
        self.out.push_str(s);
        self.out.push('\n');
    }
}

fn extract_narrowing(cond: &Expr) -> Option<(String, bool)> {
    if let Expr::BinOp { op, lhs, rhs, .. } = cond {
        if matches!(rhs.as_ref(), Expr::None_(_)) {
            if let Expr::Ident(name, _) = lhs.as_ref() {
                return Some((name.clone(), *op == BinOp::IsNot));
            }
        }
    }
    None
}

/// Attempt to evaluate constant expressions at compile time.
/// Returns the folded expression, or None if constant folding isn't possible.
fn try_fold_const(expr: &Expr) -> Option<Expr> {
    match expr {
        Expr::BinOp { op, lhs, rhs, span } => {
            // Try to fold both sides first (recursive)
            let lhs = try_fold_const(lhs).unwrap_or(*lhs.clone());
            let rhs = try_fold_const(rhs).unwrap_or(*rhs.clone());

            // Arithmetic folding
            match (&lhs, &rhs) {
                (Expr::Int(a, _), Expr::Int(b, _)) => {
                    let result = match op {
                        BinOp::Add => Some(a + b),
                        BinOp::Sub => Some(a - b),
                        BinOp::Mul => Some(a * b),
                        BinOp::Div if *b != 0 => Some(a / b),
                        BinOp::FloorDiv if *b != 0 => Some(a / b),
                        BinOp::Mod if *b != 0 => Some(a % b),
                        BinOp::Pow if *b >= 0 && *b < 64 => Some(a.pow(*b as u32)),
                        _ => None,
                    };
                    return result.map(|v| Expr::Int(v, *span));
                }
                (Expr::Bool(a, _), Expr::Bool(b, _)) => {
                    let result = match op {
                        BinOp::And => Some(*a && *b),
                        BinOp::Or => Some(*a || *b),
                        BinOp::Eq => Some(a == b),
                        BinOp::Ne => Some(a != b),
                        _ => None,
                    };
                    return result.map(|v| Expr::Bool(v, *span));
                }
                _ => {}
            }
        }
        Expr::UnOp { op, expr: inner, span } => {
            if let Some(folded) = try_fold_const(inner) {
                match (&folded, op) {
                    (Expr::Int(n, _), UnOp::Neg) => return Some(Expr::Int(-n, *span)),
                    (Expr::Bool(b, _), UnOp::Not) => return Some(Expr::Bool(!b, *span)),
                    (Expr::Int(n, _), UnOp::BitNot) => return Some(Expr::Int(!n, *span)),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    None
}

fn rust_ty(t: &Ty) -> String {
    match t {
        Ty::Int => "i64".into(),
        Ty::Float => "f64".into(),
        Ty::Bool => "bool".into(),
        Ty::Str => "String".into(),
        Ty::Unit => "()".into(),
        Ty::List(inner) => format!("Vec<{}>", rust_ty(inner)),
        Ty::Set(inner) => format!("::std::collections::HashSet<{}>", rust_ty(inner)),
        Ty::Dict(k, v) => format!("::std::collections::HashMap<{}, {}>", rust_ty(k), rust_ty(v)),
        Ty::Tuple(parts) => {
            let inner = parts.iter().map(rust_ty).collect::<Vec<_>>().join(", ");
            if parts.len() == 1 {
                format!("({},)", inner)
            } else {
                format!("({})", inner)
            }
        }
        Ty::Option(inner) => format!("Option<{}>", rust_ty(inner)),
        Ty::Class(n) => n.clone(),
        Ty::Unknown => "()".into(),
    }
}

pub fn emit(m: &Module, ctx: &TyCtx) -> Result<String> {
    Codegen::new(ctx).emit_module(m)
}

/// Emit Rust code from multiple modules in dependency order.
/// Used for multi-file compilation.
pub fn emit_program(modules: &[(Module, String)], ctx: &TyCtx) -> Result<String> {
    // Analyze which functions are actually called
    let mut called_funcs = std::collections::HashSet::new();
    for (m, _src) in modules {
        let calls = crate::typeck::analyze_called_functions(m);
        called_funcs.extend(calls);
    }

    // Identify dead functions (defined but not called)
    let mut dead_funcs = std::collections::HashSet::new();
    for func_name in ctx.funcs.keys() {
        if func_name != "main" && !called_funcs.contains(func_name.as_str()) {
            dead_funcs.insert(func_name.clone());
        }
    }

    let mut cg = Codegen::new(ctx).with_dead_funcs(dead_funcs);

    // Preamble — written once
    cg.line("#![allow(unused_parens, unused_variables, unused_mut, dead_code)]");
    cg.line("use std::io::Write;");
    cg.line("");
    cg.line("fn __py_fmt_float(x: f64) -> String {");
    cg.line("    if x.fract() == 0.0 { format!(\"{:.1}\", x) } else { format!(\"{}\", x) }");
    cg.line("}");
    cg.line("fn __py_fmt_bool(x: bool) -> String {");
    cg.line("    if x { \"True\".to_string() } else { \"False\".to_string() }");
    cg.line("}");
    cg.line("");
    cg.line("// ----- user code -----");

    // Emit all modules in order (imports first, root last)
    for (m, _src) in modules {
        for s in &m.stmts {
            // Skip import statements — they're resolved, not emitted
            if matches!(s, Stmt::Import { .. }) { continue; }
            cg.emit_top_stmt(s)?;
        }
    }

    // Synthetic entry point (same as current emit_module logic)
    if ctx.funcs.contains_key("main") {
        cg.line("");
        cg.line("fn main() { user_main(); }");
    } else {
        cg.line("");
        cg.line("fn main() {}");
    }

    Ok(cg.out)
}
