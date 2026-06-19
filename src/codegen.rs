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

    /// Replace identifier `old_name` with `new_name` in code, respecting word boundaries
    /// to avoid corrupting field names like "price" when replacing "i"
    fn replace_identifier(code: &str, old_name: &str, new_name: &str) -> String {
        // Build regex pattern: \b (word boundary) + old_name + \b (word boundary)
        if old_name.is_empty() {
            return code.to_string();
        }

        // Simple approach: split on word boundaries and reconstruct
        let mut result = String::new();
        let mut chars = code.chars().peekable();
        let old_chars: Vec<char> = old_name.chars().collect();

        while let Some(ch) = chars.next() {
            // Check if we're at the start of an identifier that matches old_name
            if ch.is_alphanumeric() || ch == '_' {
                // Collect the full identifier
                let mut ident = String::from(ch);
                let mut lookahead = vec![ch];

                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_alphanumeric() || next_ch == '_' {
                        lookahead.push(next_ch);
                        ident.push(next_ch);
                        chars.next();
                    } else {
                        break;
                    }
                }

                // Check if this identifier matches old_name
                if ident == old_name {
                    result.push_str(new_name);
                } else {
                    result.push_str(&ident);
                }
            } else {
                result.push(ch);
            }
        }

        result
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
                        // String concatenation for Add
                        if *op == BinOp::Add && (l == Ty::Str || r == Ty::Str) {
                            Ty::Str
                        } else if l == Ty::Float || r == Ty::Float {
                            Ty::Float
                        } else {
                            Ty::Int
                        }
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
                        "sum" => {
                            // sum() returns the type of the iterable elements
                            if let Some(arg) = args.first() {
                                match self.type_of_expr(arg) {
                                    Ty::List(inner) => *inner,
                                    Ty::Set(inner) => *inner,
                                    _ => Ty::Int,  // Default to int for other iterables
                                }
                            } else {
                                Ty::Int
                            }
                        }
                        "int" | "len" | "ord" | "round" | "pow" => Ty::Int,
                        "bool" | "any" | "all" => Ty::Bool,
                        "str" | "chr" | "input" => Ty::Str,
                        "sorted" | "list" | "reversed" => {
                            // These return a list; preserve the element type.
                            if let Some(arg) = args.first() {
                                match self.type_of_expr(arg) {
                                    Ty::List(e) | Ty::Set(e) => Ty::List(e),
                                    Ty::Str => Ty::List(Box::new(Ty::Str)),
                                    _ => Ty::List(Box::new(Ty::Unknown)),
                                }
                            } else {
                                Ty::List(Box::new(Ty::Unknown))
                            }
                        }
                        n => {
                            // Check if it's a class constructor
                            if self.ctx.classes.contains_key(n) {
                                Ty::Class(n.to_string())
                            } else {
                                self.ctx.funcs.get(n).map(|s| s.ret.clone()).unwrap_or(Ty::Unknown)
                            }
                        }
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
            Expr::List(elems, _) => {
                // Infer list element type from the first element if available
                if let Some(first_elem) = elems.first() {
                    let elem_ty = self.type_of_expr(first_elem);
                    Ty::List(Box::new(elem_ty))
                } else {
                    Ty::List(Box::new(Ty::Unknown))
                }
            }
            Expr::Dict(pairs, _) => {
                // Infer dict key/value types from the first pair if available
                if let Some((key_expr, val_expr)) = pairs.first() {
                    let key_ty = self.type_of_expr(key_expr);
                    let val_ty = self.type_of_expr(val_expr);
                    Ty::Dict(Box::new(key_ty), Box::new(val_ty))
                } else {
                    Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown))
                }
            }
            Expr::Set(elems, _) => {
                // Infer set element type from the first element if available
                if let Some(first_elem) = elems.first() {
                    let elem_ty = self.type_of_expr(first_elem);
                    Ty::Set(Box::new(elem_ty))
                } else {
                    Ty::Set(Box::new(Ty::Unknown))
                }
            }
            Expr::ListComp { elt, target, iter, .. } => {
                // Infer element type from the iterable and element expression
                let iter_ty = self.type_of_expr(iter);
                let elem_iter_ty = match &iter_ty {
                    Ty::List(inner) | Ty::Set(inner) => Some(inner.as_ref().clone()),
                    _ => None,
                };

                // Try to infer the element type, accounting for the loop variable type
                if let Some(elem_iter_type) = elem_iter_ty {
                    // Infer from the element expression with knowledge of the loop variable type
                    let inferred = self.infer_comp_elt_type_with_var(elt, &elem_iter_type, target);
                    if inferred != Ty::Unknown {
                        return Ty::List(Box::new(inferred));
                    }
                }

                // Fallback: use the iterable's element type
                match iter_ty {
                    Ty::List(inner) => Ty::List(inner),
                    Ty::Set(inner) => Ty::List(inner),
                    _ => Ty::List(Box::new(Ty::Unknown))
                }
            }
            Expr::SetComp { elt, target, iter, .. } => {
                // Similar to ListComp
                let iter_ty = self.type_of_expr(iter);
                if let Ty::List(ref inner) | Ty::Set(ref inner) = iter_ty {
                    match elt.as_ref() {
                        Expr::Attr { name, .. } => {
                            if let Ty::Class(cls) = inner.as_ref() {
                                if let Some(c) = self.ctx.classes.get(cls.as_str()) {
                                    if let Some(f) = c.fields.iter().find(|f| &f.name == name) {
                                        if let Ok(ty) = Ty::from_type_expr(&f.ty) {
                                            return Ty::Set(Box::new(ty));
                                        }
                                    }
                                }
                            }
                        }
                        Expr::Call { callee, .. } => {
                            if let Expr::Attr { name, .. } = callee.as_ref() {
                                if let Ty::Class(cls) = inner.as_ref() {
                                    if let Some(method_sig) = self.ctx.get_method(cls.as_str(), name) {
                                        return Ty::Set(Box::new(method_sig.ret.clone()));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    Ty::Set(inner.clone())
                } else {
                    Ty::Set(Box::new(Ty::Unknown))
                }
            }
            Expr::DictComp { key, val, target, iter, .. } => {
                // For dict comprehension, infer key and value types
                let iter_ty = self.type_of_expr(iter);
                let key_ty = if let Expr::Attr { name, .. } = key.as_ref() {
                    if let Ty::Class(ref cls) = iter_ty {
                        if let Some(c) = self.ctx.classes.get(cls.as_str()) {
                            if let Some(f) = c.fields.iter().find(|f| &f.name == name) {
                                Ty::from_type_expr(&f.ty).unwrap_or(Ty::Unknown)
                            } else {
                                Ty::Unknown
                            }
                        } else {
                            Ty::Unknown
                        }
                    } else {
                        Ty::Unknown
                    }
                } else {
                    Ty::Unknown
                };

                let val_ty = if let Expr::Attr { name, .. } = val.as_ref() {
                    if let Ty::Class(ref cls) = iter_ty {
                        if let Some(c) = self.ctx.classes.get(cls.as_str()) {
                            if let Some(f) = c.fields.iter().find(|f| &f.name == name) {
                                Ty::from_type_expr(&f.ty).unwrap_or(Ty::Unknown)
                            } else {
                                Ty::Unknown
                            }
                        } else {
                            Ty::Unknown
                        }
                    } else {
                        Ty::Unknown
                    }
                } else {
                    Ty::Unknown
                };

                Ty::Dict(Box::new(key_ty), Box::new(val_ty))
            }
            Expr::Index { obj, .. } => {
                // Type of dict[key] is the value type of the dict
                let obj_ty = self.type_of_expr(obj);
                match obj_ty {
                    Ty::Dict(_, val_ty) => *val_ty,
                    Ty::List(elem_ty) => *elem_ty,
                    _ => Ty::Unknown,
                }
            }
            _ => Ty::Unknown,
        }
    }

    /// Infer the element type of a comprehension element expression
    /// given the type and name of the loop variable
    fn infer_comp_elt_type_with_var(&self, elt: &Expr, loop_var_ty: &Ty, loop_var_name: &str) -> Ty {
        match elt {
            // Case 1: [i.field for i in items] or [i.a.b for i in items]
            Expr::Attr { obj, name, .. } => {
                // First, infer the type of the object being accessed
                let obj_ty = if let Expr::Ident(var_name, _) = obj.as_ref() {
                    if var_name == loop_var_name {
                        // Direct reference to loop variable
                        loop_var_ty.clone()
                    } else {
                        // Some other variable - can't infer
                        Ty::Unknown
                    }
                } else {
                    // Nested attribute access - recursively infer
                    self.infer_comp_elt_type_with_var(obj, loop_var_ty, loop_var_name)
                };

                // Now look up the field on the object type
                if let Ty::Class(cls) = obj_ty {
                    if let Some(c) = self.ctx.classes.get(cls.as_str()) {
                        if let Some(f) = c.fields.iter().find(|f| &f.name == name) {
                            return Ty::from_type_expr(&f.ty).unwrap_or(Ty::Unknown);
                        }
                    }
                }
                Ty::Unknown
            }
            // Case 2: [i.method() for i in items]
            Expr::Call { callee, .. } => {
                if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                    let obj_ty = if let Expr::Ident(var_name, _) = obj.as_ref() {
                        if var_name == loop_var_name {
                            loop_var_ty.clone()
                        } else {
                            Ty::Unknown
                        }
                    } else {
                        self.infer_comp_elt_type_with_var(obj, loop_var_ty, loop_var_name)
                    };

                    if let Ty::Class(cls) = obj_ty {
                        if let Some(method_sig) = self.ctx.get_method(cls.as_str(), name) {
                            return method_sig.ret.clone();
                        }
                    }
                }
                Ty::Unknown
            }
            // Case 3: [i.a + i.b for i in items] - infer from BinOp
            Expr::BinOp { lhs, op, rhs, .. } => {
                let left_ty = self.infer_comp_elt_type_with_var(lhs, loop_var_ty, loop_var_name);
                let right_ty = self.infer_comp_elt_type_with_var(rhs, loop_var_ty, loop_var_name);
                // For arithmetic operations, use type promotion rules
                match (left_ty, right_ty) {
                    (Ty::Float, _) | (_, Ty::Float) => Ty::Float,
                    (Ty::Int, Ty::Int) => {
                        // Division always returns float in Python
                        if *op == BinOp::Div {
                            Ty::Float
                        } else {
                            Ty::Int
                        }
                    }
                    _ => Ty::Unknown,
                }
            }
            _ => Ty::Unknown,
        }
    }

    /// Infer the element type of a comprehension element expression
    /// This helps determine the type of [expr for x in iter] when we can't type-check directly
    fn infer_comp_element_type(&self, elt: &Expr) -> Ty {
        // Fallback method for when we don't have loop variable type
        match elt {
            Expr::Call { callee, .. } => {
                if let Expr::Ident(n, _) = callee.as_ref() {
                    // Built-in function call
                    match n.as_str() {
                        "float" => Ty::Float,
                        "int" => Ty::Int,
                        "str" => Ty::Str,
                        "bool" => Ty::Bool,
                        _ => Ty::Unknown,
                    }
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
            Stmt::Class(c) => {
                let mut c = c.clone();
                crate::typeck::extract_init_fields(&mut c);
                self.emit_class(&c)
            }
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
                // Check for method calls that mutate (like self.items.append())
                Stmt::Expr(Expr::Call { callee, .. }) => {
                    if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                        // Check if this is a method call on self or self.attr that mutates
                        if matches!(name.as_str(), "append" | "extend" | "insert" | "remove" | "pop" | "clear" | "sort" | "reverse" | "update") {
                            // Check if the object is self or self.something
                            if let Expr::Ident(var, _) = obj.as_ref() {
                                if var == "self" {
                                    return true;
                                }
                            } else if let Expr::Attr { obj: inner_obj, .. } = obj.as_ref() {
                                if let Expr::Ident(var, _) = inner_obj.as_ref() {
                                    if var == "self" {
                                        return true;
                                    }
                                }
                            }
                        }
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
            // Value params are bound `mut` so functions may mutate them or their
            // fields in place (Python passes mutable objects by reference);
            // unused-mut is allowed in the generated crate.
            let _ = write!(sig, "mut {}: {}", p.name, rust_ty(&Ty::from_type_expr(&p.ty)?));
        }
        let ret = Ty::from_type_expr(&f.ret)?;
        let ret_s = rust_ty(&ret);
        let _ = write!(sig, ") -> {} {{", ret_s);
        self.line(&sig);
        self.indent += 1;

        // Populate locals from parameters
        // Register self with its class type if this is a method
        if let Some(cls) = method_of {
            self.locals.insert("self".to_string(), Ty::Class(cls.to_string()));
        }

        for p in &f.params {
            if p.name != "self" {
                let ty = Ty::from_type_expr(&p.ty)?;
                self.locals.insert(p.name.clone(), ty);
            }
        }

        // First pass: forward type inference over the whole body (including
        // nested blocks) so un-annotated locals are typed from their value and
        // refined by later uses (e.g. `d = {}` then `d[k] = some_str`, or an
        // `acc = 0` accumulator later assigned a float).
        self.prescan_types(&f.body);

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
            "#[derive(Clone, Debug, PartialEq, Default)]"
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

    /// True for types that are move-only (non-`Copy`) in the generated Rust.
    fn is_owned_ty(t: &Ty) -> bool {
        matches!(t,
            Ty::Str | Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) | Ty::Class(_))
    }

    /// Emit an expression in a position that takes ownership of the value
    /// (function argument, container store). A bare identifier of an owned
    /// (non-`Copy`) type is `.clone()`d so the original binding stays usable.
    /// pyrst follows Python value semantics and performs no move/borrow
    /// analysis, so cloning here is the conservative, always-compiling choice.
    fn emit_owned(&mut self, e: &Expr) -> Result<String> {
        let s = self.emit_expr(e)?;
        if let Expr::Ident(name, _) = e {
            let owned = self.locals.get(name)
                .or_else(|| self.ctx.vars.get(name))
                .map(Self::is_owned_ty)
                .unwrap_or(false);
            if owned {
                return Ok(format!("{}.clone()", s));
            }
        }
        Ok(s)
    }

    /// Combine two inferred types into the more specific / wider one.
    /// `Unknown` yields to anything concrete; `Int` widens to `Float`;
    /// matching collections unify element-wise. Otherwise the first wins.
    fn unify_ty(a: Ty, b: Ty) -> Ty {
        match (a, b) {
            (Ty::Unknown, x) | (x, Ty::Unknown) => x,
            (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => Ty::Float,
            (Ty::Dict(k1, v1), Ty::Dict(k2, v2)) => Ty::Dict(
                Box::new(Self::unify_ty(*k1, *k2)),
                Box::new(Self::unify_ty(*v1, *v2)),
            ),
            (Ty::List(e1), Ty::List(e2)) => Ty::List(Box::new(Self::unify_ty(*e1, *e2))),
            (Ty::Set(e1), Ty::Set(e2)) => Ty::Set(Box::new(Self::unify_ty(*e1, *e2))),
            (a, _) => a,
        }
    }

    /// Whether reassigning a `b`-typed value to an `a`-typed binding is a
    /// genuine type change (Python allows it; single-`let` Rust does not).
    /// `Unknown` and numeric int/float mixes are not conflicts.
    fn types_conflict(a: &Ty, b: &Ty) -> bool {
        use Ty::*;
        if matches!(a, Unknown) || matches!(b, Unknown) {
            return false;
        }
        if matches!((a, b), (Int, Float) | (Float, Int)) {
            return false;
        }
        std::mem::discriminant(a) != std::mem::discriminant(b)
    }

    /// Forward type-inference pre-pass over a statement list (recursing into
    /// nested blocks). Records inferred types for un-annotated locals in
    /// `self.locals` and refines empty-collection element types from later
    /// `obj[k] = v` stores and `obj.append(v)` calls. Runs in source order so
    /// earlier inferences inform later `type_of_expr` lookups.
    fn prescan_types(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Assign { target, ty: Some(te), .. } => {
                    if let Ok(t) = Ty::from_type_expr(te) {
                        self.locals.insert(target.clone(), t);
                    }
                }
                Stmt::Assign { target, ty: None, value, .. } => {
                    let vt = self.type_of_expr(value);
                    let merged = match self.locals.get(target) {
                        Some(existing) => Self::unify_ty(existing.clone(), vt),
                        None => vt,
                    };
                    self.locals.insert(target.clone(), merged);
                }
                Stmt::IndexAssign { obj, value, .. } => {
                    let vt = self.type_of_expr(value);
                    if !matches!(vt, Ty::Unknown) {
                        if let Some(existing) = self.locals.get(obj).cloned() {
                            let refined = match existing {
                                Ty::Dict(k, v) if matches!(*v, Ty::Unknown) => {
                                    Ty::Dict(k, Box::new(vt))
                                }
                                Ty::List(e) if matches!(*e, Ty::Unknown) => {
                                    Ty::List(Box::new(vt))
                                }
                                other => other,
                            };
                            self.locals.insert(obj.clone(), refined);
                        }
                    }
                }
                Stmt::Expr(Expr::Call { callee, args, .. }) => {
                    if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                        if name == "append" {
                            if let Expr::Ident(objn, _) = obj.as_ref() {
                                if let Some(arg) = args.first() {
                                    let at = self.type_of_expr(arg);
                                    if !matches!(at, Ty::Unknown) {
                                        if let Some(Ty::List(e)) = self.locals.get(objn).cloned() {
                                            if matches!(*e, Ty::Unknown) {
                                                self.locals.insert(objn.clone(), Ty::List(Box::new(at)));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Stmt::If { then, elifs, else_, .. } => {
                    self.prescan_types(then);
                    for (_, body) in elifs { self.prescan_types(body); }
                    if let Some(body) = else_ { self.prescan_types(body); }
                }
                Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::With { body, .. } => {
                    self.prescan_types(body);
                }
                Stmt::Try { body, else_, finally_, .. } => {
                    self.prescan_types(body);
                    if let Some(b) = else_ { self.prescan_types(b); }
                    if let Some(b) = finally_ { self.prescan_types(b); }
                }
                _ => {}
            }
        }
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
                            // No message: still use the "<Type> panic: <msg>" payload
                            // format so `except <Type>:` type-matching can parse it.
                            self.line(&format!("panic!(\"{{}} panic: \", \"{}\");", exc_type));
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
                            // Infer type from the value expression, but prefer a
                            // richer type discovered by the forward pre-pass
                            // (e.g. an `acc = 0` later assigned floats).
                            let value_ty = self.type_of_expr(value);
                            let decl_ty = match self.locals.get(target) {
                                Some(pre) => Self::unify_ty(pre.clone(), value_ty.clone()),
                                None => value_ty.clone(),
                            };
                            self.locals.insert(target.clone(), decl_ty.clone());
                            // If the variable is later widened from int to float,
                            // declare it as f64 and cast the integer initializer.
                            if matches!(decl_ty, Ty::Float) && matches!(value_ty, Ty::Int) {
                                self.line(&format!("let mut {}: f64 = {} as f64;", target, v));
                            } else {
                                self.line(&format!("let mut {} = {};", target, v));
                            }
                        }
                    }
                } else {
                    // Python permits rebinding a name to a value of a different
                    // type. When that happens, emit a shadowing `let` (which
                    // always type-checks) instead of a plain reassignment.
                    let value_ty = self.type_of_expr(value);
                    let cur = self.locals.get(target).cloned().unwrap_or(Ty::Unknown);
                    if Self::types_conflict(&cur, &value_ty) {
                        self.locals.insert(target.clone(), value_ty);
                        self.line(&format!("let mut {} = {};", target, v));
                    } else {
                        self.line(&format!("{} = {};", target, v));
                    }
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

                // Register loop variables with their types for type checking
                let iter_ty = self.type_of_expr(iter);
                if targets.len() == 1 {
                    if let Ty::List(inner) | Ty::Set(inner) = iter_ty {
                        let saved = self.locals.get(&targets[0]).cloned();
                        self.locals.insert(targets[0].clone(), *inner);
                        for s in body { self.emit_stmt(s)?; }
                        if let Some(ty) = saved {
                            self.locals.insert(targets[0].clone(), ty);
                        } else {
                            self.locals.remove(targets[0].as_str());
                        }
                    } else {
                        for s in body { self.emit_stmt(s)?; }
                    }
                } else {
                    // Multiple targets (tuple unpacking) - skip type registration for now
                    for s in body { self.emit_stmt(s)?; }
                }

                self.indent -= 1;
                self.line("}");
            }
            Stmt::Import { .. } => {
                // Silently drop imports in v0
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                self.line("{");
                self.indent += 1;

                // Run the try body inside catch_unwind. pyrst's `raise` compiles
                // to a panic whose payload is a formatted string (see Stmt::Raise):
                //   raise Foo("m")  -> "Foo panic: m"
                //   raise Foo       -> "Foo panic: "   (empty message)
                //   raise           -> "explicit raise"
                self.line("let __try_result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {");
                self.indent += 1;
                for s in body { self.emit_stmt(s)?; }
                self.indent -= 1;
                self.line("}));");

                // Whether any handler can catch every exception type.
                let has_catch_all = handlers.iter().any(|h| {
                    h.exc_type.is_none() || h.exc_type.as_deref() == Some("Exception")
                });

                // __reraise holds the original panic payload when no handler
                // matched, so it can be re-raised after the finally block.
                self.line("let __reraise: ::std::option::Option<::std::boxed::Box<dyn ::std::any::Any + ::std::marker::Send>> = match __try_result {");
                self.indent += 1;

                // Success path: run the `else` body (if any), then no re-raise.
                self.line("::std::result::Result::Ok(__ok) => {");
                self.indent += 1;
                self.line("let _ = __ok;");
                if let Some(else_body) = else_ {
                    for s in else_body { self.emit_stmt(s)?; }
                }
                self.line("::std::option::Option::None");
                self.indent -= 1;
                self.line("}");

                // Error path: recover the payload string, parse out the type, and
                // dispatch to the matching handler.
                self.line("::std::result::Result::Err(__payload) => {");
                self.indent += 1;
                self.line("let __exc_str: String = if let Some(s) = __payload.downcast_ref::<&str>() {");
                self.line("    (*s).to_string()");
                self.line("} else if let Some(s) = __payload.downcast_ref::<String>() {");
                self.line("    s.clone()");
                self.line("} else {");
                self.line("    String::from(\"unknown panic\")");
                self.line("};");
                // Split "<Type> panic: <msg>"; otherwise type == msg == whole string.
                self.line("let (__exc_type, __exc_msg): (String, String) = match __exc_str.split_once(\" panic: \") {");
                self.line("    Some((t, m)) => (t.to_string(), m.to_string()),");
                self.line("    None => (__exc_str.clone(), __exc_str.clone()),");
                self.line("};");
                self.line("let _ = &__exc_type; let _ = &__exc_msg;");

                if handlers.is_empty() {
                    // No handlers at all: always re-raise.
                    self.line("::std::option::Option::Some(__payload)");
                } else {
                    let mut first = true;
                    for h in handlers {
                        let is_catch_all =
                            h.exc_type.is_none() || h.exc_type.as_deref() == Some("Exception");
                        let cond = if is_catch_all {
                            "true".to_string()
                        } else {
                            // Build an OR-expansion over the transitive descendant set of
                            // the handler's exception type so that, e.g., `except LookupError`
                            // matches both KeyError and IndexError.  For unknown/user-defined
                            // types exc_descendants returns an empty vec and we fall through to
                            // the plain exact-match path.
                            let exc_name = h.exc_type.as_deref().unwrap();
                            let descendants = exc_descendants(exc_name);
                            if descendants.is_empty() {
                                // Unknown / user-defined type: exact match only (original behaviour).
                                format!("__exc_type == {:?}", exc_name)
                            } else {
                                // OR-expand over base + all transitive subclasses.
                                let clauses: Vec<String> = descendants
                                    .iter()
                                    .map(|d| format!("__exc_type == {:?}", d))
                                    .collect();
                                format!("({})", clauses.join(" || "))
                            }
                        };
                        if first {
                            self.line(&format!("if {} {{", cond));
                            first = false;
                        } else {
                            self.line(&format!("}} else if {} {{", cond));
                        }
                        self.indent += 1;
                        if let Some(name) = &h.exc_name {
                            self.line(&format!("let {} = __exc_msg.clone();", name));
                            self.line(&format!("let _ = &{};", name));
                        }
                        for s in &h.body { self.emit_stmt(s)?; }
                        self.line("::std::option::Option::None");
                        self.indent -= 1;
                    }
                    // Trailing else: if no catch-all handler exists, propagate.
                    if has_catch_all {
                        self.line("} else { ::std::option::Option::None }");
                    } else {
                        self.line("} else { ::std::option::Option::Some(__payload) }");
                    }
                }
                self.indent -= 1;
                self.line("}");

                self.indent -= 1;
                self.line("};");

                // finally: runs on every path, before any re-raise.
                if let Some(fin) = finally_ {
                    for s in fin { self.emit_stmt(s)?; }
                }

                // Re-raise an unmatched exception (after finally).
                self.line("if let ::std::option::Option::Some(__p) = __reraise { ::std::panic::resume_unwind(__p); }");

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
                let v = self.emit_owned(value)?;
                self.line(&format!("{}.{} = {};", obj, attr, v));
            }
            Stmt::IndexAssign { obj, idx, value, .. } => {
                let i = self.emit_expr(idx)?;
                let v = self.emit_owned(value)?;
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
                        crate::ast::FStrPart::Interp(expr, spec) => {
                            match spec {
                                None => {
                                    // No spec: match print()'s Python-style Display
                                    // so bare floats/bools render as `1.0` / `True`.
                                    fmt_str.push_str("{}");
                                    let raw = self.emit_expr(expr)?;
                                    let arg = match self.type_of_expr(expr) {
                                        Ty::Float => format!("__py_fmt_float({})", raw),
                                        Ty::Bool => format!("__py_fmt_bool({})", raw),
                                        _ => raw,
                                    };
                                    args.push(arg);
                                }
                                Some(s) => {
                                    // Explicit spec: emit a Rust format spec and pass the
                                    // raw value (the spec drives formatting, e.g. {:.2}).
                                    let clean = s.trim_end_matches(|c: char| "fdsge%".contains(c));
                                    fmt_str.push_str(&format!("{{:{}}}", clean));
                                    args.push(self.emit_expr(expr)?);
                                }
                            }
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
                for e in elems { parts.push(self.emit_owned(e)?); }
                format!("vec![{}]", parts.join(", "))
            }
            Expr::Tuple(elems, _) => {
                let parts: Result<Vec<_>> = elems.iter().map(|e| self.emit_owned(e)).collect();
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
                } else if matches!(self.type_of_expr(iter), Ty::Str) {
                    // Iterating a str yields 1-character strings (Python semantics)
                    format!("{}.chars().map(|__c| __c.to_string())", iter_s)
                } else {
                    format!("{}.iter().cloned()", iter_s)
                };
                let elt_s = self.emit_expr(elt)?;
                if let Some(cond_expr) = cond {
                    let cond_s = self.emit_expr(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some({}) }} else {{ None }} ).collect::<Vec<_>>()",
                        chain, target, cond_s, elt_s)
                } else {
                    format!("{}.map(|{}| {}).collect::<Vec<_>>()", chain, target, elt_s)
                }
            }
            Expr::SetComp { elt, target, iter, cond, .. } => {
                let iter_s = self.emit_expr(iter)?;
                let is_range = iter_s.contains("..");
                let chain = if is_range {
                    format!("({}).into_iter()", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Str) {
                    format!("{}.chars().map(|__c| __c.to_string())", iter_s)
                } else {
                    format!("{}.iter().cloned()", iter_s)
                };
                let elt_s = self.emit_expr(elt)?;
                if let Some(cond_expr) = cond {
                    let cond_s = self.emit_expr(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some({}) }} else {{ None }} ).collect::<::std::collections::HashSet<_>>()",
                        chain, target, cond_s, elt_s)
                } else {
                    format!("{}.map(|{}| {}).collect::<::std::collections::HashSet<_>>()", chain, target, elt_s)
                }
            }
            Expr::DictComp { key, val, target, iter, cond, .. } => {
                let iter_s = self.emit_expr(iter)?;
                let is_range = iter_s.contains("..");
                let chain = if is_range {
                    format!("({}).into_iter()", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Str) {
                    format!("{}.chars().map(|__c| __c.to_string())", iter_s)
                } else {
                    format!("{}.iter().cloned()", iter_s)
                };
                let key_s = self.emit_expr(key)?;
                let val_s = self.emit_expr(val)?;
                if let Some(cond_expr) = cond {
                    let cond_s = self.emit_expr(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some(({}, {})) }} else {{ None }} ).collect::<::std::collections::HashMap<_,_>>()",
                        chain, target, cond_s, key_s, val_s)
                } else {
                    format!("{}.map(|{}| ({}, {})).collect::<::std::collections::HashMap<_,_>>()", chain, target, key_s, val_s)
                }
            }
            Expr::Set(elems, _) => {
                if elems.is_empty() {
                    return Ok("::std::collections::HashSet::new()".to_string());
                }
                let mut items = Vec::new();
                for e in elems {
                    let es = self.emit_owned(e)?;
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
                    let ks = self.emit_owned(k)?;
                    let vs = self.emit_owned(v)?;
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
                            // Python len() of a str is the CHARACTER count, not the
                            // UTF-8 byte count. Collections keep .len().
                            if matches!(self.type_of_expr(&args[0]), Ty::Str) {
                                return Ok(format!("{}.chars().count() as i64", a));
                            }
                            return Ok(format!("{}.len() as i64", a));
                        }
                        "str" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(format!("format!(\"{{}}\" , {})", a));
                        }
                        "int" => {
                            let a = self.emit_expr(&args[0])?;
                            let arg_type = self.type_of_expr(&args[0]);
                            match arg_type {
                                Ty::Str => {
                                    return Ok(format!("({}.parse::<i64>().unwrap())", a));
                                }
                                _ => return Ok(format!("({} as i64)", a)),
                            }
                        }
                        "float" => {
                            let a = self.emit_expr(&args[0])?;
                            let arg_type = self.type_of_expr(&args[0]);
                            match arg_type {
                                Ty::Str => {
                                    return Ok(format!("({}.parse::<f64>().unwrap())", a));
                                }
                                _ => return Ok(format!("({} as f64)", a)),
                            }
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
                                    // Replace param_name with __x in the body (word-boundary aware)
                                    Self::replace_identifier(&body_s, param_name.as_str(), "__x")
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
                                let elem_ty = match self.type_of_expr(&args[0]) {
                                    Ty::List(inner) => *inner,
                                    _ => Ty::Int,
                                };
                                return Ok(match elem_ty {
                                    Ty::Float => format!("{{ let mut __min = f64::INFINITY; for __x in {}.iter() {{ if __x < &__min {{ __min = *__x; }} }} __min }}", a),
                                    _ => format!("{}.iter().copied().min().unwrap_or(0)", a),
                                });
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
                                    // Replace param_name with __x in the body (word-boundary aware)
                                    Self::replace_identifier(&body_s, param_name.as_str(), "__x")
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
                                let elem_ty = match self.type_of_expr(&args[0]) {
                                    Ty::List(inner) => *inner,
                                    _ => Ty::Int,
                                };
                                return Ok(match elem_ty {
                                    Ty::Float => format!("{{ let mut __max = f64::NEG_INFINITY; for __x in {}.iter() {{ if __x > &__max {{ __max = *__x; }} }} __max }}", a),
                                    _ => format!("{}.iter().copied().max().unwrap_or(0)", a),
                                });
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let b = self.emit_expr(&args[1])?;
                                return Ok(format!("::std::cmp::max({}, {})", a, b));
                            }
                        }
                        "sorted" => {
                            let a = self.emit_expr(&args[0])?;
                            let list_ty = self.type_of_expr(&args[0]);

                            if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                                // sorted with key parameter
                                // Determine the return type of the key expression
                                let key_ret_ty = if let Expr::Lambda { params, body, .. } = key_expr {
                                    // For lambdas, infer from the body expression
                                    // We need to temporarily register the parameter to type-check the body
                                    // But since type_of_expr is &self, we can't do that easily
                                    // So we'll just check common patterns
                                    if let Expr::Attr { name, .. } = body.as_ref() {
                                        // Lambda body is field access - check the field type
                                        if let Ty::List(ref elem_ty) = list_ty {
                                            if let Ty::Class(cls) = elem_ty.as_ref() {
                                                if let Some(c) = self.ctx.classes.get(cls.as_str()) {
                                                    if let Some(f) = c.fields.iter().find(|f| &f.name == name) {
                                                        Ty::from_type_expr(&f.ty).unwrap_or(Ty::Unknown)
                                                    } else {
                                                        Ty::Unknown
                                                    }
                                                } else {
                                                    Ty::Unknown
                                                }
                                            } else {
                                                Ty::Unknown
                                            }
                                        } else {
                                            Ty::Unknown
                                        }
                                    } else if let Expr::Call { callee, .. } = body.as_ref() {
                                        // Lambda body is a method call - check method return type
                                        if let Expr::Attr { name, .. } = callee.as_ref() {
                                            if let Ty::List(ref elem_ty) = list_ty {
                                                if let Ty::Class(cls) = elem_ty.as_ref() {
                                                    if let Some(method_sig) = self.ctx.get_method(cls.as_str(), name) {
                                                        method_sig.ret.clone()
                                                    } else {
                                                        Ty::Unknown
                                                    }
                                                } else {
                                                    Ty::Unknown
                                                }
                                            } else {
                                                Ty::Unknown
                                            }
                                        } else {
                                            Ty::Unknown
                                        }
                                    } else {
                                        Ty::Unknown
                                    }
                                } else {
                                    Ty::Unknown
                                };

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
                                    // Replace param_name with __x in the body (word-boundary aware)
                                    Self::replace_identifier(&body_s, param_name.as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };

                                // Use appropriate sorting method based on key return type
                                return Ok(match key_ret_ty {
                                    Ty::Float => {
                                        format!(
                                            "{{ let mut __sorted = {}.clone(); __sorted.sort_by(|a, b| {{ let ka = {{ let __x = a.clone(); {} }}; let kb = {{ let __x = b.clone(); {} }}; ka.partial_cmp(&kb).unwrap_or(::std::cmp::Ordering::Equal) }}); __sorted }}",
                                            a, key_code, key_code
                                        )
                                    }
                                    _ => {
                                        format!(
                                            "{{ let mut __sorted = {}.clone(); __sorted.sort_by_key(|__x| {}); __sorted }}",
                                            a, key_code
                                        )
                                    }
                                });
                            } else {
                                // Check if this is a float list to handle Ord constraint
                                let is_float_list = matches!(&list_ty, Ty::List(inner) if inner.as_ref() == &Ty::Float);
                                let sort_code = if is_float_list {
                                    ".sort_by(|a, b| a.partial_cmp(b).unwrap_or(::std::cmp::Ordering::Equal))".to_string()
                                } else {
                                    ".sort()".to_string()
                                };

                                if let Some((_, rev_expr)) = kwargs.iter().find(|(n, _)| n == "reverse") {
                                    // sorted with reverse parameter
                                    let rev_s = self.emit_expr(rev_expr)?;
                                    return Ok(format!(
                                        "{{ let mut __sorted = {}.clone(); __sorted{}; if {} {{ __sorted.reverse(); }} __sorted }}",
                                        a, sort_code, rev_s
                                    ));
                                } else {
                                    // Default sorted
                                    return Ok(format!("{{ let mut __sorted = {}.clone(); __sorted{}; __sorted }}", a, sort_code));
                                }
                            }
                        }
                        "sum" => {
                            let a = self.emit_expr(&args[0])?;
                            // Determine the sum type based on the iterable's element type
                            let sum_type = match self.type_of_expr(&args[0]) {
                                Ty::List(inner) | Ty::Set(inner) => match *inner {
                                    Ty::Float => "f64",
                                    _ => "i64",
                                },
                                _ => "i64",
                            };
                            return Ok(format!("{}.iter().sum::<{}>()", a, sum_type));
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
                        "repr" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("repr requires exactly 1 argument".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            let a = self.emit_expr(&args[0])?;
                            let repr_expr = match obj_type {
                                Ty::Str => format!("format!(\"'{{}}'\", {})", a),
                                Ty::Bool => format!("format!(\"{{}}\" , if {} {{ \"True\" }} else {{ \"False\" }})", a),
                                _ => format!("format!(\"{{}}\" , {})", a),
                            };
                            return Ok(repr_expr);
                        }
                        "ascii" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("ascii requires exactly 1 argument".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            let a = self.emit_expr(&args[0])?;
                            let ascii_expr = match obj_type {
                                Ty::Str => {
                                    format!(
                                        "format!(\"'{{}}'\", {}.escape_default())",
                                        a
                                    )
                                }
                                Ty::Bool => {
                                    format!("format!(\"{{}}\" , if {} {{ \"True\" }} else {{ \"False\" }})", a)
                                }
                                _ => format!("format!(\"{{}}\" , {})", a),
                            };
                            return Ok(ascii_expr);
                        }
                        "list" => {
                            if args.is_empty() {
                                return Ok("Vec::<i64>::new()".to_string());
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let arg_type = self.type_of_expr(&args[0]);
                                // If the argument is already a list, just return it. Otherwise collect the iterator.
                                match arg_type {
                                    Ty::List(_) => return Ok(a),
                                    _ => {
                                        // Check if the expression looks like it returns a Vec (contains reverse, sort, etc.)
                                        if a.contains("reverse") || a.contains("sort") || a.contains("clone()") {
                                            return Ok(a);
                                        }
                                        return Ok(format!("{}.collect::<Vec<_>>()", a));
                                    }
                                }
                            }
                        }
                        "dict" => {
                            if args.is_empty() && kwargs.is_empty() {
                                return Ok("std::collections::HashMap::new()".to_string());
                            } else {
                                return Err(crate::diag::Error::Codegen("dict() constructor with arguments not yet supported".into()));
                            }
                        }
                        "tuple" => {
                            if args.is_empty() {
                                return Ok("()".to_string());
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                return Ok(format!("({},)", a));
                            }
                        }
                        "getattr" => {
                            if args.len() < 2 || args.len() > 3 {
                                return Err(crate::diag::Error::Codegen("getattr requires 2 or 3 arguments".into()));
                            }
                            let obj = self.emit_expr(&args[0])?;
                            let attr_name = self.emit_expr(&args[1])?;

                            // For now, just access the field directly (works for simple cases)
                            // This assumes the object is a struct with the matching field name
                            return Ok(format!("{{ let __attr_name = {}; format!(\"{{:?}}\", __attr_name) }}", attr_name));
                        }
                        "setattr" => {
                            if args.len() != 3 {
                                return Err(crate::diag::Error::Codegen("setattr requires exactly 3 arguments".into()));
                            }
                            // Note: In Python, setattr modifies the object. In Rust, we can't modify through a reference.
                            // For now, just return None
                            return Ok("()".to_string());
                        }
                        "hasattr" => {
                            if args.len() != 2 {
                                return Err(crate::diag::Error::Codegen("hasattr requires exactly 2 arguments".into()));
                            }
                            // For now, just return true (conservative assumption)
                            return Ok("true".to_string());
                        }
                        "set" => {
                            if args.is_empty() {
                                return Ok("::std::collections::HashSet::new()".to_string());
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let arg_type = self.type_of_expr(&args[0]);
                                // If the argument is already a set, just return it. Otherwise convert to set.
                                match arg_type {
                                    Ty::Set(_) => return Ok(a),
                                    Ty::List(_) | Ty::Unknown => {
                                        // Check if it looks like a vec literal or variable
                                        if a.starts_with("vec!") {
                                            return Ok(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a));
                                        } else {
                                            return Ok(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a));
                                        }
                                    }
                                    _ => {
                                        // For other iterables, try to convert
                                        return Ok(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a));
                                    }
                                }
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

                    // Special handling for string methods that return &str and need to be converted to String
                    if matches!(name.as_str(), "strip" | "lstrip" | "rstrip") {
                        return Ok(format!("{}.{}().to_string()", obj_s, method));
                    }

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
                        // str length is character count, not UTF-8 byte count.
                        if matches!(self.type_of_expr(obj.as_ref()), Ty::Str) {
                            return Ok(format!("{}.chars().count() as i64", obj_s));
                        }
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
                    if name == "removeprefix" && !parts.is_empty() {
                        return Ok(format!(
                            "{{ let __s = {}.clone(); let __prefix = {}.clone(); \
                            if __s.starts_with(__prefix.as_str()) {{ __s[__prefix.len()..].to_string() }} else {{ __s }} }}",
                            obj_s, parts[0]
                        ));
                    }
                    if name == "removesuffix" && !parts.is_empty() {
                        return Ok(format!(
                            "{{ let __s = {}.clone(); let __suffix = {}.clone(); \
                            if __s.ends_with(__suffix.as_str()) {{ __s[..__s.len() - __suffix.len()].to_string() }} else {{ __s }} }}",
                            obj_s, parts[0]
                        ));
                    }
                    if name == "expandtabs" {
                        let tab_size = if !parts.is_empty() {
                            format!("{} as usize", parts[0])
                        } else {
                            "8usize".to_string()
                        };
                        return Ok(format!(
                            "{{ let __s = {}.clone(); let __tab_size = {}; \
                            __s.replace('\\t', &\" \".repeat(__tab_size)) }}",
                            obj_s, tab_size
                        ));
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
                    if name == "rfind" && !parts.is_empty() {
                        return Ok(format!("{}.rfind({}.as_str()).map(|i| i as i64).unwrap_or(-1i64)", obj_s, parts[0]));
                    }
                    if name == "rindex" && !parts.is_empty() {
                        return Ok(format!(
                            "{{ let __idx = {}.rfind({}.as_str()); match __idx {{ Some(i) => i as i64, None => panic!(\"substring not found\") }} }}",
                            obj_s, parts[0]
                        ));
                    }

                    // String utility methods
                    if name == "isdigit" {
                        return Ok(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_numeric()))", obj_s, obj_s));
                    }
                    if name == "isalpha" {
                        return Ok(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_alphabetic()))", obj_s, obj_s));
                    }
                    if name == "isupper" {
                        return Ok(format!("(!{}.is_empty() && {}.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) && {}.chars().any(|c| c.is_alphabetic()))", obj_s, obj_s, obj_s));
                    }
                    if name == "islower" {
                        return Ok(format!("(!{}.is_empty() && {}.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_lowercase()) && {}.chars().any(|c| c.is_alphabetic()))", obj_s, obj_s, obj_s));
                    }
                    if name == "isspace" {
                        return Ok(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_whitespace()))", obj_s, obj_s));
                    }
                    if name == "isalnum" {
                        return Ok(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_alphanumeric()))", obj_s, obj_s));
                    }
                    if name == "isidentifier" {
                        return Ok(format!(
                            "(!{}.is_empty() && ({}.chars().next().unwrap().is_alphabetic() || {}.chars().next().unwrap() == '_') && {}.chars().all(|c| c.is_alphanumeric() || c == '_'))",
                            obj_s, obj_s, obj_s, obj_s
                        ));
                    }
                    if name == "isnumeric" {
                        return Ok(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_numeric()))", obj_s, obj_s));
                    }
                    if name == "isprintable" {
                        return Ok(format!("({}.chars().all(|c| !c.is_control()))", obj_s));
                    }
                    if name == "istitle" {
                        return Ok(format!(
                            "(!{}.is_empty() && {}.split_whitespace().all(|word| if word.is_empty() {{ true }} else {{ word.chars().next().unwrap().is_uppercase() && word[1..].chars().all(|c| !c.is_alphabetic() || c.is_lowercase()) }}))",
                            obj_s, obj_s
                        ));
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
                            // pop(key) — remove from the receiver and return the value (panic if absent)
                            return Ok(format!("{}.remove(&{}).expect(\"KeyError: key not found\")", obj_s, parts[0]));
                        } else {
                            // pop(key, default) — remove from the receiver; default if absent
                            return Ok(format!("{}.remove(&{}).unwrap_or({})", obj_s, parts[0], parts[1]));
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
                for a in args { parts.push(self.emit_owned(a)?); }

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
                        // String slicing with negative index support and optional step
                        let start_expr = start.as_ref().map(|e| self.emit_expr(e)).transpose()?;
                        let start_val = start_expr.map(|s| {
                            format!("(if {} < 0 {{ (({}.len() as i64) + {}) as usize }} else {{ {} as usize }})", s, o, s, s)
                        }).unwrap_or_else(|| "0usize".to_string());

                        let stop_expr = stop.as_ref().map(|e| self.emit_expr(e)).transpose()?;
                        let stop_val = stop_expr.map(|s| {
                            format!("(if {} < 0 {{ (({}.len() as i64) + {}) as usize }} else {{ {} as usize }})", s, o, s, s)
                        }).unwrap_or_else(|| format!("{}.len()", o));

                        if let Some(step_expr) = step {
                            let step_s = self.emit_expr(step_expr)?;
                            return Ok(format!(
                                "{{ let __s = {}.clone(); let __chars: Vec<char> = __s.chars().collect(); \
                                let __start = {}; let __stop = {}; let __step_val = {}; \
                                if __step_val == 0 {{ panic!(\"slice step cannot be zero\") }} \
                                else if __step_val > 0 {{ \
                                let __step = __step_val as usize; \
                                __chars.iter().enumerate().filter_map(|(i, c)| \
                                if i >= __start && i < __stop && (i as i64 - __start as i64) % __step_val == 0 {{ Some(*c) }} else {{ None }} \
                                ).collect::<String>() }} \
                                else {{ \
                                let mut __result = String::new(); \
                                let mut __i = __stop as i64 - 1; \
                                while __i >= __start as i64 {{ \
                                if __i >= 0 && (__i as usize) < __chars.len() {{ \
                                __result.push(__chars[__i as usize]); \
                                }} \
                                __i += __step_val; \
                                }} \
                                __result }} }}",
                                o, start_val, stop_val, step_s
                            ));
                        } else {
                            return Ok(format!("((&{}[{}..{}]).to_string())", o, start_val, stop_val));
                        }
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

                // Handle division - always returns float in Python
                if *op == BinOp::Div {
                    let l = self.emit_expr(lhs)?;
                    let r = self.emit_expr(rhs)?;
                    // Convert both operands to f64 for division
                    return Ok(format!("(({} as f64) / ({} as f64))", l, r));
                }

                // Handle set operations: union, intersection, difference
                let lt = self.type_of_expr(lhs);
                let rt = self.type_of_expr(rhs);
                if matches!(&lt, Ty::Set(_)) || matches!(&rt, Ty::Set(_)) {
                    let l = self.emit_expr(lhs)?;
                    let r = self.emit_expr(rhs)?;
                    match op {
                        BinOp::BitOr => {
                            // Set union: s1 | s2
                            return Ok(format!("{{ let mut __union = {}.clone(); __union.extend({}.iter().cloned()); __union }}", l, r));
                        }
                        BinOp::BitAnd => {
                            // Set intersection: s1 & s2
                            return Ok(format!("{{ let mut __inter = std::collections::HashSet::new(); for __x in {}.iter() {{ if {}.contains(__x) {{ __inter.insert(__x.clone()); }} }} __inter }}", l, r));
                        }
                        BinOp::BitXor => {
                            // Set symmetric difference: s1 ^ s2
                            return Ok(format!("{{ let mut __diff = {}.clone(); for __x in {}.iter() {{ if __diff.contains(__x) {{ __diff.remove(__x); }} else {{ __diff.insert(__x.clone()); }} }} __diff }}", l, r));
                        }
                        BinOp::Sub => {
                            // Set difference: s1 - s2
                            return Ok(format!("{{ let mut __diff = {}.clone(); for __x in {}.iter() {{ __diff.remove(__x); }} __diff }}", l, r));
                        }
                        _ => {}
                    }
                }

                // Handle string concatenation: str + str needs special handling
                if *op == BinOp::Add {
                    let lt = self.type_of_expr(lhs);
                    let rt = self.type_of_expr(rhs);
                    if lt == Ty::Str || rt == Ty::Str {
                        let l = self.emit_expr(lhs)?;
                        let r = self.emit_expr(rhs)?;
                        // Use format! for robust string concatenation
                        return Ok(format!(r#"format!("{{}}{{}}", {}, {})"#, l, r));
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

                // Get types for numeric type conversion
                let lt = self.type_of_expr(lhs);
                let rt = self.type_of_expr(rhs);

                match op {
                    BinOp::Pow => {
                        // int ** int -> integer power (matches type_of_expr inferring Int);
                        // any float operand -> float power.
                        if matches!(lt, Ty::Int) && matches!(rt, Ty::Int) {
                            return Ok(format!("(({}).pow({} as u32))", l, r));
                        }
                        return Ok(format!("(({} as f64).powf({} as f64))", l, r));
                    }
                    BinOp::In => {
                        // Use contains_key for dicts, contains for lists/sets
                        let contains_method = match rt {
                            Ty::Dict(_, _) => format!("{}.contains_key(&{})", r, l),
                            Ty::List(_) => format!("{}.iter().any(|__x| __x == &{})", r, l),
                            Ty::Set(_) => format!("{}.contains(&{})", r, l),
                            _ => format!("{}.contains(&{})", r, l),
                        };
                        return Ok(contains_method);
                    }
                    BinOp::NotIn => {
                        // Use !contains_key for dicts, !contains for lists/sets
                        let contains_method = match rt {
                            Ty::Dict(_, _) => format!("!{}.contains_key(&{})", r, l),
                            Ty::List(_) => format!("!{}.iter().any(|__x| __x == &{})", r, l),
                            Ty::Set(_) => format!("!{}.contains(&{})", r, l),
                            _ => format!("!{}.contains(&{})", r, l),
                        };
                        return Ok(contains_method);
                    }
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

                        // Handle numeric type promotion: int op float -> convert to float
                        // Also handle cases where type inference failed (Unknown) but we know one side is float
                        let (l_final, r_final) = match (&lt, &rt) {
                            // int op float -> promote both to float
                            (Ty::Int, Ty::Float) => (format!("({} as f64)", l), format!("({})", r)),
                            // float op int -> promote both to float
                            (Ty::Float, Ty::Int) => (format!("({})", l), format!("({} as f64)", r)),
                            // Unknown op float -> try to promote Unknown as int/numeric
                            (Ty::Unknown, Ty::Float) => (format!("({} as f64)", l), format!("({})", r)),
                            // float op Unknown -> try to promote Unknown as int/numeric
                            (Ty::Float, Ty::Unknown) => (format!("({})", l), format!("({} as f64)", r)),
                            // Both same type or non-numeric: no conversion
                            _ => (l, r),
                        };

                        format!("({} {} {})", l_final, op_s, r_final)
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

/// Return the set of builtin exception type names that `base` covers (i.e.
/// `base` itself plus all transitive subclasses in the builtin hierarchy).
/// The caller OR-expands this set into the handler match condition, so
/// `except LookupError` matches a raised `KeyError`/`IndexError`.
///
/// Returns an empty vec for leaves, unknown/user-defined types, and
/// `Exception` — in every one of those cases the caller falls back to an
/// exact-match condition (`Exception` never reaches here: it is handled
/// upstream as the catch-all `true` arm). The builtin hierarchy is only two
/// levels deep, so each base's transitive closure is written out directly.
fn exc_descendants(base: &str) -> Vec<&'static str> {
    match base {
        "ArithmeticError" => vec![
            "ArithmeticError", "ZeroDivisionError", "OverflowError", "FloatingPointError",
        ],
        "LookupError" => vec!["LookupError", "IndexError", "KeyError"],
        "RuntimeError" => vec!["RuntimeError", "RecursionError", "NotImplementedError"],
        "NameError" => vec!["NameError", "UnboundLocalError"],
        "OSError" => vec![
            "OSError", "FileNotFoundError", "PermissionError", "IsADirectoryError",
        ],
        // Leaves and unknown/custom types: caller uses an exact-match condition.
        _ => vec![],
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
                        BinOp::Div => None, // Division always returns float, don't fold
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
