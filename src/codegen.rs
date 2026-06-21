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

/// Canonical list of collection methods that mutate the receiver in-place.
/// Consulted by both `method_modifies_self` (to infer `&mut self` on the
/// enclosing method) and the emission site (to pick `emit_place` for
/// subscripted receivers so the mutation lands on the real element).
const MUTATING_METHODS: &[&str] = &[
    "append", "extend", "insert", "remove", "pop", "clear",
    "sort", "reverse", "update", "add", "discard",
    "setdefault", "popitem",
];

pub struct Codegen<'a> {
    pub ctx: &'a TyCtx,
    out: String,
    indent: usize,
    locals: HashMap<String, Ty>,
    declared: std::collections::HashSet<String>,
    current_class: Option<String>,
    /// (EPIC-5) Declared return type of the function currently being emitted.
    /// Lets `return` decide whether to wrap a bare value in `Some(..)` / emit
    /// `None` (when the function returns `Option<T>`) vs. a bare `return;` (Unit).
    current_ret_ty: Ty,
    dead_funcs: std::collections::HashSet<String>,  // Functions that are never called
}

impl<'a> Codegen<'a> {
    pub fn new(ctx: &'a TyCtx) -> Self {
        Self { ctx, out: String::new(), indent: 0, locals: HashMap::new(), declared: Default::default(), current_class: None, current_ret_ty: Ty::Unit, dead_funcs: Default::default() }
    }

    pub fn with_dead_funcs(mut self, dead: std::collections::HashSet<String>) -> Self {
        self.dead_funcs = dead;
        self
    }

    fn is_copy_type(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Int | Ty::Float | Ty::Bool | Ty::Unit)
    }

    /// Returns true when `ty` implements the `Default` trait in the emitted Rust.
    /// Copy classes (all-primitive fields) don't derive Default, so they return false.
    fn type_has_default(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Unit => true,
            Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Option(_) => true,
            Ty::Class(n) => {
                // Copy classes don't get #[derive(Default)] (see emit_class).
                let all_copy = self.ctx.get_all_fields(n).iter().all(|f| {
                    Ty::from_type_expr(&f.ty).map(|t| self.is_copy_type(&t)).unwrap_or(false)
                });
                !all_copy
            }
            _ => false,
        }
    }

    /// Returns a zero-value Rust initializer for any type, including Copy classes
    /// whose primitive fields are zeroed recursively.  Used in `new()` bodies
    /// where `Default::default()` is unavailable for Copy-only structs.
    fn zeroed_default(&self, ty: &Ty) -> String {
        match ty {
            Ty::Int => "0i64".to_string(),
            Ty::Float => "0.0f64".to_string(),
            Ty::Bool => "false".to_string(),
            Ty::Str => "String::new()".to_string(),
            Ty::Class(n) => {
                let all_copy = self.ctx.get_all_fields(n).iter().all(|f| {
                    Ty::from_type_expr(&f.ty).map(|t| self.is_copy_type(&t)).unwrap_or(false)
                });
                if all_copy {
                    // Build a struct literal with zeroed primitive fields.
                    let fields: Vec<String> = self.ctx.get_all_fields(n).iter().map(|f| {
                        let inner_ty = Ty::from_type_expr(&f.ty).unwrap_or(Ty::Int);
                        format!("{}: {}", f.name, self.zeroed_default(&inner_ty))
                    }).collect();
                    format!("{} {{ {} }}", n, fields.join(", "))
                } else {
                    "Default::default()".to_string()
                }
            }
            _ => "Default::default()".to_string(),
        }
    }

    /// True if a type has no `Unknown` anywhere — only then is it safe to hoist
    /// (an `Unknown` element would render as `()` and mismatch a real value).
    fn fully_concrete(ty: &Ty) -> bool {
        match ty {
            Ty::Unknown => false,
            Ty::List(e) | Ty::Set(e) | Ty::Option(e) => Self::fully_concrete(e),
            Ty::Dict(k, v) => Self::fully_concrete(k) && Self::fully_concrete(v),
            Ty::Tuple(ts) => ts.iter().all(Self::fully_concrete),
            _ => true,
        }
    }

    /// A safe Rust default initializer for hoisting a local, or None for types
    /// with no usable default (Copy class without `Default`, Tuple, Unit,
    /// Unknown, File) — those names are not hoisted and keep their in-place let.
    fn default_val(&self, ty: &Ty) -> Option<String> {
        if !Self::fully_concrete(ty) { return None; }
        Some(match ty {
            Ty::Int => "0i64".to_string(),
            Ty::Float => "0.0f64".to_string(),
            Ty::Bool => "false".to_string(),
            Ty::Str => "String::new()".to_string(),
            Ty::List(_) => "Vec::new()".to_string(),
            Ty::Set(_) => "::std::collections::HashSet::new()".to_string(),
            Ty::Dict(_, _) => "::std::collections::HashMap::new()".to_string(),
            Ty::Option(_) => "None".to_string(),
            Ty::Class(n) => {
                // Only derive Default when all fields support it (mirrors emit_class).
                if self.type_has_default(&Ty::Class(n.clone())) {
                    "Default::default()".to_string()
                } else {
                    return None;  // Not hoistable — no Default impl available.
                }
            }
            _ => return None,
        })
    }

    /// Collect names first-assigned inside a nested block (depth > 0) and all
    /// unpack targets (never hoistable). Recurses through every block but not
    /// into nested function/class definitions (those have their own scope).
    fn collect_hoistable(
        stmts: &[Stmt],
        depth: usize,
        block_assigned: &mut std::collections::HashSet<String>,
        unpack: &mut std::collections::HashSet<String>,
    ) {
        for s in stmts {
            match s {
                Stmt::Assign { target, .. } => { if depth > 0 { block_assigned.insert(target.clone()); } }
                Stmt::Unpack { targets, .. } => { for t in targets { unpack.insert(t.clone()); } }
                Stmt::If { then, elifs, else_, .. } => {
                    Self::collect_hoistable(then, depth + 1, block_assigned, unpack);
                    for (_, b) in elifs { Self::collect_hoistable(b, depth + 1, block_assigned, unpack); }
                    if let Some(b) = else_ { Self::collect_hoistable(b, depth + 1, block_assigned, unpack); }
                }
                Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::With { body, .. } => {
                    Self::collect_hoistable(body, depth + 1, block_assigned, unpack);
                }
                Stmt::Try { body, handlers, else_, finally_, .. } => {
                    Self::collect_hoistable(body, depth + 1, block_assigned, unpack);
                    for h in handlers { Self::collect_hoistable(&h.body, depth + 1, block_assigned, unpack); }
                    if let Some(b) = else_ { Self::collect_hoistable(b, depth + 1, block_assigned, unpack); }
                    if let Some(b) = finally_ { Self::collect_hoistable(b, depth + 1, block_assigned, unpack); }
                }
                Stmt::Match { arms, .. } => {
                    for a in arms { Self::collect_hoistable(&a.body, depth + 1, block_assigned, unpack); }
                }
                _ => {}
            }
        }
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
        crate::typeck::infer_expr_ty(e, &self.locals, self.ctx)
    }

    /// (EPIC-5) Coerce an already-emitted expression `s` (for source expr `e`)
    /// into the Rust representation expected by `target` when `target` is
    /// `Option<T>`:
    ///   - a `None` literal  -> `None`
    ///   - a value already typed `Option<_>` (e.g. an Optional var, or a call
    ///     returning Optional) -> passed through unchanged
    ///   - any other bare value -> `Some(s)`  (the auto-Some that mirrors
    ///     typeck's `T ~ Optional[T]` compatibility arm)
    /// When `target` is not an Option, `s` is returned unchanged. This is the
    /// single wrapping point shared by assignment, return, and argument sites so
    /// the three never drift.
    fn coerce_to_option(&self, s: String, e: &Expr, target: &Ty) -> String {
        if !matches!(target, Ty::Option(_)) {
            return s;
        }
        if matches!(e, Expr::None_(_)) {
            return "None".to_string();
        }
        if matches!(self.type_of_expr(e), Ty::Option(_)) {
            return s;
        }
        format!("Some({})", s)
    }

    /// True when `e` emits an integer-valued (`i64`) Rust expression whose
    /// *logical* type (per the inference oracle) is nonetheless `Float`.
    ///
    /// The only such case is integer exponentiation: D5 (Python semantics) makes
    /// the oracle type `int ** int` as `Float`, but emission is operand-driven —
    /// `int ** int` is lowered to the `i64`-returning `__py_ipow` (or a folded
    /// `i64` literal), matching the Pow arm in `emit_expr`. A `float`-typed
    /// binding receiving such a value therefore still needs an `as f64` cast,
    /// which the plain `type_of_expr(value) == Int` coercion check no longer
    /// detects now that the oracle reports `Float`. This predicate restores that
    /// signal so the keystone oracle composes with the pow-into-float emission.
    fn emits_int_pow(&self, e: &Expr) -> bool {
        match e {
            // `-(int ** int)` is still an integer value.
            Expr::UnOp { op: UnOp::Neg, expr, .. } => self.emits_int_pow(expr),
            Expr::BinOp { lhs, op: BinOp::Pow, rhs, .. } => {
                // Mirror the emit_expr Pow rule: int**int -> i64 (__py_ipow);
                // any float operand -> f64 (powf).
                matches!(self.type_of_expr(lhs), Ty::Int)
                    && matches!(self.type_of_expr(rhs), Ty::Int)
            }
            _ => false,
        }
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
                // extract_init_fields is already called by resolver.rs:132 when
                // building TyCtx, so ctx.classes already holds the populated
                // ClassDef.  emit_class reads fields via ctx.get_all_fields, not
                // from c.fields directly, so no clone+mutate is needed here.
                self.emit_class(c)
            }
            other => {
                // Top-level non-decl statements are not yet supported (would need
                // collecting them into the synthetic main). v0 punts.
                self.line(&format!("// TODO: top-level stmt {:?}", std::mem::discriminant(other)));
                Ok(())
            }
        }
    }

    /// Whether an lvalue / receiver chain bottoms out at the `self` receiver —
    /// i.e. walking through `Attr`/`Index` bases reaches `Expr::Ident("self")`.
    /// Used to decide a method needs `&mut self` when it mutates anything rooted
    /// at self (`self.x`, `self.dict[k]`, `self.rooms[i].field`, ...).
    fn expr_roots_at_self(e: &Expr) -> bool {
        match e {
            Expr::Ident(n, _) => n == "self",
            Expr::Attr { obj, .. } | Expr::Index { obj, .. } => Self::expr_roots_at_self(obj),
            _ => false,
        }
    }

    fn method_modifies_self(&self, body: &[Stmt]) -> bool {
        for stmt in body {
            match stmt {
                // Any assignment whose target base chain roots at `self` mutates
                // it: `self.x = v`, `self.dict[k] = v`, `self.a.b = v`,
                // `self.rooms[i].field = v`.
                Stmt::AttrAssign { obj, .. } | Stmt::IndexAssign { obj, .. } => {
                    if Self::expr_roots_at_self(obj) {
                        return true;
                    }
                }
                // Check for method calls that mutate (like self.items.append()
                // or self.rooms[i].append()) — any mutating call whose receiver
                // chain roots at `self`.
                Stmt::Expr(Expr::Call { callee, .. }) => {
                    if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                        if MUTATING_METHODS.contains(&name.as_str()) {
                            if Self::expr_roots_at_self(obj) {
                                return true;
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
        // (EPIC-5) Track the active return type for Some/None wrapping in `return`.
        let saved_ret_ty = std::mem::replace(&mut self.current_ret_ty, ret.clone());

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

        // Python has function scope, but pyrst emits if/for/while/with/try bodies
        // as Rust `{}` blocks. Hoist locals first-assigned inside a block to
        // function scope (with a safe default) so they stay visible after the
        // block. Params, unpack targets, and names whose type has no usable
        // default keep their in-place `let` (and the prior block-scope limit).
        let mut block_assigned = std::collections::HashSet::new();
        let mut unpack_targets = std::collections::HashSet::new();
        Self::collect_hoistable(&f.body, 0, &mut block_assigned, &mut unpack_targets);
        let params: std::collections::HashSet<&str> = f.params.iter().map(|p| p.name.as_str()).collect();
        let mut hoist: Vec<String> = block_assigned.into_iter()
            .filter(|n| !unpack_targets.contains(n) && !params.contains(n.as_str()) && !self.declared.contains(n))
            .collect();
        hoist.sort();
        for name in hoist {
            let ty = self.locals.get(&name).cloned().unwrap_or(Ty::Unknown);
            if let Some(def) = self.default_val(&ty) {
                self.line(&format!("let mut {}: {} = {};", name, rust_ty(&ty), def));
                self.declared.insert(name);
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
        self.current_ret_ty = saved_ret_ty;
        Ok(())
    }

    /// Resolve the full set of methods visible on `class_name` across its entire
    /// base chain (transitive), with the nearest-defining class winning — the
    /// same MRO walk TyCtx::get_method / get_all_fields use, but returning the
    /// actual `Func` (with body) so an inherited method or dunder can be emitted
    /// onto the subclass's own value-struct.
    ///
    /// Ordering: own methods first (in source order), then each inherited name
    /// the first time it is reached walking bases depth-first. A name already
    /// resolved (by the class itself or a nearer ancestor) is never overwritten,
    /// so overrides shadow inherited definitions exactly like Python's MRO.
    fn resolved_methods(&self, class_name: &str) -> Vec<Func> {
        let mut out: Vec<Func> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        self.collect_resolved_methods(class_name, &mut out, &mut seen, &mut visited);
        out
    }

    fn collect_resolved_methods(
        &self,
        class_name: &str,
        out: &mut Vec<Func>,
        seen: &mut std::collections::HashSet<String>,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if visited.contains(class_name) {
            return;
        }
        visited.insert(class_name.to_string());
        if let Some(cd) = self.ctx.classes.get(class_name) {
            // This class's own methods take precedence over anything inherited.
            for m in &cd.methods {
                if seen.insert(m.name.clone()) {
                    out.push(m.clone());
                }
            }
            // Then inherited methods, nearer bases before farther ones.
            for base in &cd.bases {
                self.collect_resolved_methods(base, out, seen, visited);
            }
        }
    }

    fn emit_class(&mut self, c: &ClassDef) -> Result<()> {
        // Full transitive field set (ancestors first, then own; deduped by name),
        // sourced from typeck's get_all_fields so the struct layout, Copy/Default
        // derivation, and default_val agree even for multi-level inheritance
        // (a manual one-level walk dropped grandparent fields).
        let mut all_fields: Vec<Param> = Vec::new();
        for f in self.ctx.get_all_fields(&c.name) {
            if !all_fields.iter().any(|ef: &Param| ef.name == f.name) {
                all_fields.push(f);
            }
        }

        // Full transitive method set (own methods win, then inherited via MRO),
        // so a subclass that INHERITS a dunder or a grandparent method emits it
        // onto its own value-struct. Dunder trait impls, the `__lt_impl` helper,
        // and the inherited-method emission all key off this set rather than
        // `c.methods` (which is the defining class only).
        let resolved_methods = self.resolved_methods(&c.name);

        let all_fields_copy = all_fields.iter().all(|f| {
            Ty::from_type_expr(&f.ty)
                .map(|ty| self.is_copy_type(&ty))
                .unwrap_or(false)
        });

        // A user-defined __eq__ (own OR inherited) emits a manual `impl PartialEq`,
        // so don't ALSO derive it (that would be a conflicting-impl error, E0119)
        // and don't fall back to a field-wise derived eq that ignores the
        // inherited custom semantics.
        let has_eq = resolved_methods.iter().any(|m| m.name == "__eq__");
        let pe = if has_eq { "" } else { ", PartialEq" };
        // Only derive Default when every field actually implements Default.
        // Copy classes (all-primitive fields) don't derive Default, so an outer
        // struct holding one must NOT include Default in its own derive list.
        let all_fields_default = all_fields.iter().all(|f| {
            Ty::from_type_expr(&f.ty)
                .map(|ty| self.type_has_default(&ty))
                .unwrap_or(false)
        });
        let derives = if all_fields_copy {
            format!("#[derive(Copy, Clone, Debug{})]", pe)
        } else if all_fields_default {
            format!("#[derive(Clone, Debug{}, Default)]", pe)
        } else {
            format!("#[derive(Clone, Debug{})]", pe)
        };
        self.line(&derives);
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

        // Constructor emission keys off the OWN __init__ (the existing model:
        // every subclass in scope defines its own __init__). The inherent-impl
        // block, however, must also open when the class only INHERITS methods
        // (a regular method or __lt__ pulled from an ancestor), so those flags
        // read from the resolved (transitive) set.
        let has_init = c.methods.iter().any(|m| m.name == "__init__");
        let has_lt = resolved_methods.iter().any(|m| m.name == "__lt__");
        let is_dataclass = c.is_dataclass;
        let has_regular_methods = resolved_methods.iter().any(|m|
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
                        // Use zeroed_default which handles Copy classes that don't
                        // implement Default (unlike a plain Default::default() call).
                        let dv = self.zeroed_default(&ty);
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

            // Emit all methods except dunder-trait ones, drawn from the resolved
            // (transitive) set so inherited and grandparent methods land on this
            // class's value-struct. `resolved_methods` already applies MRO (own
            // wins, then nearer bases), so each name appears once.
            let class_name = c.name.clone();

            // Own method names identify overrides (those still get a __super_
            // alias so `super().m()` can reach the immediate parent body).
            let own_method_names: std::collections::HashSet<String> = c.methods.iter()
                .map(|m| m.name.clone())
                .collect();

            // Emit __super_ aliases for OWN methods that override an immediate
            // parent method (one-level, unchanged: super() targets the parent).
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

            // Emit every resolved regular method (own + transitively inherited),
            // including __init__ itself (the constructor's new() calls
            // self.__init__(...), so it must exist as an inherent method).
            // Dunder-trait methods become trait impls below, except __lt__ which
            // also needs a __lt_impl helper that PartialOrd::partial_cmp calls.
            for m in &resolved_methods {
                if dunder_trait_names.contains(&m.name.as_str()) {
                    // Special handling for __lt__: emit as __lt_impl (own or inherited).
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
            }

            self.indent -= 1;
            self.line("}");
            self.line("");
        }

        // Emit trait implementations for dunder methods, drawn from the resolved
        // (transitive) set so an inherited __str__/__eq__/__lt__/etc. produces the
        // matching Display/PartialEq/PartialOrd impl on THIS class's value-struct
        // (the impl body is the ancestor's, but every reference is to c.name and
        // to fields that get_all_fields guarantees exist on the subclass).
        let c_methods = resolved_methods.clone();
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

    /// Emit an lvalue *place* expression (one assignable / mutable in Rust) for
    /// an assignment-target base — as opposed to `emit_expr`, which produces a
    /// clone-based *rvalue* for `Index`/`Attr` (e.g. `{ let __list = x.clone();
    /// __list[i].clone() }`) that cannot be stored into. Used by AttrAssign /
    /// IndexAssign so chained targets (`self.dict[k]=v`, `rooms[i].field=v`,
    /// `a.b.c=v`) lower to in-place mutation. Struct fields are bare
    /// HashMap/Vec/struct values (no Rc/RefCell), so `place.field`,
    /// `place[i as usize]`, and `*place.get_mut(&k)` are valid places.
    fn emit_place(&mut self, e: &Expr) -> Result<String> {
        match e {
            // A bare name (`self`, a local, a `mut` param) is already a place.
            Expr::Ident(n, _) => Ok(n.clone()),
            // Field access: the base recursively as a place, then `.field`.
            // (No @property handling: a property getter is not an lvalue.)
            Expr::Attr { obj, name, .. } => {
                let base = self.emit_place(obj)?;
                Ok(format!("{}.{}", base, name))
            }
            // Subscript in a *base* position (we are descending further into the
            // target, e.g. `grid[r][c]` or `self.dict[k].field`): produce a place
            // that mutably borrows the element. Dict -> deref of get_mut; list ->
            // direct index. Negative indices in nested lvalue position are not
            // supported yet (a place cannot contain a `let`); the common
            // non-negative cases lower directly.
            Expr::Index { obj, idx, .. } => {
                let base = self.emit_place(obj)?;
                if matches!(self.type_of_expr(obj), Ty::Dict(..)) {
                    let k = self.emit_expr(idx)?;
                    Ok(format!("(*{}.get_mut(&{}).expect(\"key not found\"))", base, k))
                } else {
                    let i = self.emit_expr(idx)?;
                    Ok(format!("{}[{} as usize]", base, i))
                }
            }
            // Any other base (a parenthesized/computed expr) — fall back to the
            // normal emission; in practice the parser only yields Ident/Attr/Index
            // chains as assignment-target bases.
            _ => self.emit_expr(e),
        }
    }

    /// Emit a list/set element, promoting int-typed elements to `f64` when the
    /// collection's unified element type is `Float` (`widen == true`). Reuses
    /// the same `as f64` cast convention as the assignment int->float coercion
    /// (see `Stmt::Assign` emission) so `[1, 2.0]` becomes a homogeneous
    /// `Vec<f64>` instead of the rustc-rejected `vec![(1i64), (2.0f64)]`
    /// (card 5c2f31d8). Float (and non-int) elements are emitted unchanged.
    fn emit_collection_elem(&mut self, e: &Expr, widen: bool) -> Result<String> {
        let s = self.emit_owned(e)?;
        if widen && matches!(self.type_of_expr(e), Ty::Int) {
            Ok(format!("({}) as f64", s))
        } else {
            Ok(s)
        }
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

    /// Unified element type of a list/set literal's elements. Folds every
    /// element's type with `unify_ty` (rather than first-element-wins) so a
    /// mixed numeric literal like `[1, 2.0]` is typed `Float` — matching
    /// typeck's `unify_elem_types`. An empty literal is `Unknown`.
    fn list_elem_ty(&self, elems: &[Expr]) -> Ty {
        let mut iter = elems.iter();
        match iter.next() {
            None => Ty::Unknown,
            Some(first) => iter.fold(self.type_of_expr(first), |acc, e| {
                Self::unify_ty(acc, self.type_of_expr(e))
            }),
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
                    // Refine an empty-collection element type from `name[k] = v`
                    // — only for a bare local base (`Expr::Ident`); chained bases
                    // (self.dict[k], grid[r][c]) refine via their declared types.
                    if let Expr::Ident(name, _) = obj.as_ref() {
                        let vt = self.type_of_expr(value);
                        if !matches!(vt, Ty::Unknown) {
                            if let Some(existing) = self.locals.get(name).cloned() {
                                let refined = match existing {
                                    Ty::Dict(k, v) if matches!(*v, Ty::Unknown) => {
                                        Ty::Dict(k, Box::new(vt))
                                    }
                                    Ty::List(e) if matches!(*e, Ty::Unknown) => {
                                        Ty::List(Box::new(vt))
                                    }
                                    other => other,
                                };
                                self.locals.insert(name.clone(), refined);
                            }
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
                Stmt::For { targets, iter, body, .. } => {
                    // Register the loop variable's type so the body sees it (e.g.
                    // a `Temp` element's float fields aren't mistyped as Int).
                    if targets.len() == 1 {
                        let elem = match self.type_of_expr(iter) {
                            Ty::List(inner) | Ty::Set(inner) => *inner,
                            Ty::Str => Ty::Str,
                            _ => Ty::Int, // range / unknown iterables yield ints
                        };
                        self.locals.entry(targets[0].clone()).or_insert(elem);
                    }
                    self.prescan_types(body);
                }
                Stmt::While { body, .. } | Stmt::With { body, .. } => {
                    self.prescan_types(body);
                }
                Stmt::Try { body, handlers, else_, finally_, .. } => {
                    self.prescan_types(body);
                    for h in handlers {
                        // Bind exc_name as Str before scanning the handler body so
                        // that `len(e)` on a handler-bound exception yields char-count
                        // (Ty::Str path) rather than byte-count (Ty::Unknown path).
                        // Mirrors typeck.rs line 748-750.
                        if let Some(name) = &h.exc_name {
                            self.locals.insert(name.clone(), Ty::Str);
                        }
                        self.prescan_types(&h.body);
                    }
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
                // (EPIC-5) In an Option-returning function, wrap the value:
                // `None` -> `return None;`, a bare T -> `return Some(T);`, an
                // already-Optional value -> pass through.
                if matches!(self.current_ret_ty, Ty::Option(_)) {
                    let s = self.emit_expr(e)?;
                    let wrapped = self.coerce_to_option(s, e, &self.current_ret_ty);
                    self.line(&format!("return {};", wrapped));
                } else if matches!(e, Expr::None_(_)) {
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
                            // (EPIC-5) An Optional-annotated binding wraps a bare
                            // value in `Some(..)` (or emits `None` for the None
                            // literal); an already-Optional initializer passes
                            // through. Shared with return/argument sites.
                            let v = self.coerce_to_option(v, value, &ty_obj);
                            // A float-annotated binding may receive an integer-typed
                            // value (e.g. `x: float = 2 ** 3`, where `**` constant-folds
                            // to an int and int**int otherwise emits i64). Cast to f64 so
                            // the declared type matches the initializer (avoids E0308).
                            // `emits_int_pow` covers the case the oracle now types as
                            // Float (D5) but emission still lowers to i64.
                            let value_ty = self.type_of_expr(value);
                            if matches!(ty_obj, Ty::Float)
                                && (matches!(value_ty, Ty::Int) || self.emits_int_pow(value))
                            {
                                self.line(&format!("let mut {}: {} = {} as f64;", target, rust_ty(&ty_obj), v));
                            } else {
                                self.line(&format!("let mut {}: {} = {};", target, rust_ty(&ty_obj), v));
                            }
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
                            if matches!(decl_ty, Ty::Float)
                                && (matches!(value_ty, Ty::Int) || self.emits_int_pow(value))
                            {
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
                    } else if matches!(cur, Ty::Float)
                        && (matches!(value_ty, Ty::Int) || self.emits_int_pow(value))
                    {
                        // Reassigning an int into a float-typed (e.g. hoisted) var.
                        self.line(&format!("{} = {} as f64;", target, v));
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
                let target_ty = self.locals.get(target.as_str()).cloned().unwrap_or(Ty::Unknown);
                match op {
                    BinOp::FloorDiv => {
                        // Python's //= floors toward negative infinity; Rust's /= truncates toward zero.
                        // For float targets keep the f64 floor path.
                        // For int targets route through __py_floordiv which also panics on /0
                        // with a catchable ZeroDivisionError payload.
                        if matches!(target_ty, Ty::Float) {
                            self.line(&format!("{} = ({} as f64 / {} as f64).floor();", target, target, v));
                        } else {
                            self.line(&format!("{} = __py_floordiv(({}), ({}));", target, target, v));
                        }
                    }
                    BinOp::Mod => {
                        // Python's %= takes the sign of the divisor; Rust's %= takes the
                        // sign of the dividend. Mirror the BinOp lowering.
                        if matches!(target_ty, Ty::Float) {
                            self.line(&format!(
                                "{{ let __b = ({} as f64); {} = ((({} as f64) % __b) + __b) % __b; }}",
                                v, target, target
                            ));
                        } else {
                            self.line(&format!("{} = __py_mod(({}), ({}));", target, target, v));
                        }
                    }
                    _ => {
                        let op_s = match op {
                            BinOp::Add => "+=", BinOp::Sub => "-=", BinOp::Mul => "*=", BinOp::Div => "/=",
                            _ => "+=", // fallback for other ops
                        };
                        self.line(&format!("{} {} {};", target, op_s, v));
                    }
                }
            }
            Stmt::If { cond, then, elifs, else_, .. } => {
                // (EPIC-5) None-guard narrowing must agree with typeck
                // (check_stmt's If arm): for `x is not None` the THEN branch sees
                // the unwrapped payload; for `x is None` the ELSE branch (when
                // there are no elifs) sees it. The unwrap shadows the Option
                // binding inside the block, so it never leaks past the `if`. Only
                // a local actually typed `Option<_>` is narrowed.
                // `narrowed` = the Option local and its inner type when the
                // condition is a None-guard on a local typed `Option<_>`.
                let narrowed: Option<(String, bool, Ty)> = extract_narrowing(cond)
                    .and_then(|(var, is_not_none)| match self.locals.get(var.as_str()) {
                        Some(Ty::Option(inner)) => Some((var, is_not_none, (**inner).clone())),
                        _ => None,
                    });
                let c = self.emit_expr(cond)?;
                self.line(&format!("if {} {{", c));
                self.indent += 1;
                // THEN branch is the non-None case for `x is not None`. Emit the
                // unwrap and retype the local so type-dispatched emission inside
                // the block (e.g. `str(x)`) sees the inner type; restore after.
                let then_narrow = narrowed.as_ref().filter(|(_, is_not_none, _)| *is_not_none);
                let then_saved = then_narrow.map(|(var, _, inner)| {
                    self.line(&format!("let {} = {}.unwrap();", var, var));
                    let prev = self.locals.insert(var.clone(), inner.clone());
                    (var.clone(), prev)
                });
                for s in then { self.emit_stmt(s)?; }
                if let Some((var, prev)) = then_saved {
                    match prev { Some(t) => { self.locals.insert(var, t); } None => { self.locals.remove(var.as_str()); } }
                }
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
                    // ELSE is the non-None case only for `x is None` with no elifs.
                    let else_narrow = narrowed.as_ref()
                        .filter(|(_, is_not_none, _)| !*is_not_none && elifs.is_empty());
                    let else_saved = else_narrow.map(|(var, _, inner)| {
                        self.line(&format!("let {} = {}.unwrap();", var, var));
                        let prev = self.locals.insert(var.clone(), inner.clone());
                        (var.clone(), prev)
                    });
                    for s in b { self.emit_stmt(s)?; }
                    if let Some((var, prev)) = else_saved {
                        match prev { Some(t) => { self.locals.insert(var, t); } None => { self.locals.remove(var.as_str()); } }
                    }
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
                                 i.contains(".collect::<Vec<_>>()");
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
                //
                // Suppress the default panic hook while the try body runs so that a
                // *caught* exception produces no stderr noise.  The hook is saved and
                // restored immediately after catch_unwind so that an *uncaught*
                // exception (re-raised via resume_unwind below) still goes through the
                // caller's hook and the Rust runtime prints a useful message + aborts
                // with a non-zero exit code.
                self.line("let __prev_hook = ::std::panic::take_hook();");
                self.line("::std::panic::set_hook(::std::boxed::Box::new(|_| {}));");
                self.line("let __try_result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {");
                self.indent += 1;
                for s in body { self.emit_stmt(s)?; }
                self.indent -= 1;
                self.line("}));");
                self.line("::std::panic::set_hook(__prev_hook); // restore before any re-raise");

                // Whether any handler can catch every exception type.
                let has_catch_all = handlers.iter().any(|h| {
                    h.exc_type.is_none() || h.exc_type.as_deref() == Some("Exception")
                });

                // Accumulate the panic message string in case we need to print it to
                // stderr on an unmatched re-raise (the payload Box is moved into
                // resume_unwind, so we must capture the string before that). It is
                // only reassigned on a re-raise path; a catch-all try never re-raises,
                // so emit a non-`mut` binding there to avoid an unused-mut warning.
                let reraise_possible = handlers.is_empty() || !has_catch_all;
                let reraise_binding = if reraise_possible { "let mut" } else { "let" };
                self.line(&format!(
                    "{} __reraise_msg: ::std::option::Option<String> = ::std::option::Option::None;",
                    reraise_binding
                ));

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
                    self.line("__reraise_msg = ::std::option::Option::Some(__exc_str.clone());");
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
                        self.line("} else { __reraise_msg = ::std::option::Option::Some(__exc_str.clone()); ::std::option::Option::Some(__payload) }");
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
                // Print the exception message to stderr first so the user sees a
                // useful error; resume_unwind then aborts with a non-zero exit code.
                self.line("if let ::std::option::Option::Some(__p) = __reraise { if let ::std::option::Option::Some(ref __msg) = __reraise_msg { eprintln!(\"{}\", __msg); } ::std::panic::resume_unwind(__p); }");

                self.indent -= 1;
                self.line("}");
            }
            Stmt::With { ctx_expr, as_name, body, .. } => {
                let ctx_s = self.emit_expr(ctx_expr)?;
                self.line("{");
                self.indent += 1;
                // The bound name is block-scoped in the generated Rust, so save and
                // restore the outer locals entry around the body (mirrors for-loop).
                let saved = if let Some(name) = as_name {
                    // Register the bound type (e.g. open() -> File) so method calls
                    // on it (f.write/read) resolve to the right emission.
                    let prev = self.locals.get(name).cloned();
                    self.locals.insert(name.clone(), self.type_of_expr(ctx_expr));
                    self.line(&format!("let mut {} = {};", name, ctx_s));
                    Some((name.clone(), prev))
                } else {
                    self.line(&format!("let _ = {};", ctx_s));
                    None
                };
                for s in body { self.emit_stmt(s)?; }
                if let Some((name, prev)) = saved {
                    match prev {
                        Some(ty) => { self.locals.insert(name, ty); }
                        None => { self.locals.remove(name.as_str()); }
                    }
                }
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Del { target, .. } => {
                let t = self.emit_expr(target)?;
                self.line(&format!("drop({});", t));
            }
            Stmt::AttrAssign { obj, attr, value, .. } => {
                let v = self.emit_owned(value)?;
                // The base must be emitted as a *place* (lvalue), not the
                // clone-based rvalue emit_expr produces for Attr/Index.
                let place = self.emit_place(obj)?;
                self.line(&format!("{}.{} = {};", place, attr, v));
            }
            Stmt::IndexAssign { obj, idx, value, .. } => {
                let v = self.emit_owned(value)?;
                let place = self.emit_place(obj)?;
                // Dispatch on the base's collection kind (dict -> HashMap::insert,
                // list -> indexed store). type_of_expr resolves chained bases
                // (self.dict, grid[r], ...), not just bare locals.
                let is_dict = matches!(self.type_of_expr(obj), Ty::Dict(..));
                if is_dict {
                    // HashMap::insert takes ownership of the key, so emit it owned
                    // (a String key var becomes `k.clone()`; Copy keys are unchanged).
                    let k = self.emit_owned(idx)?;
                    self.line(&format!("{}.insert({}, {});", place, k, v));
                } else {
                    let i = self.emit_expr(idx)?;
                    self.line(&format!("{}[{} as usize] = {};", place, i, v));
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
                                        Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) =>
                                            format!("({}).py_repr()", raw),
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
                // When the literal's unified element type is Float but some
                // elements are int literals (`[1, 2.0]`), cast the int elements
                // to f64 so the vec is a homogeneous `Vec<f64>` (card 5c2f31d8).
                let widen = matches!(self.list_elem_ty(elems), Ty::Float);
                let mut parts = Vec::new();
                for e in elems { parts.push(self.emit_collection_elem(e, widen)?); }
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
                // Mirror the list case: cast int elements to f64 when the set's
                // unified element type is Float. NOTE: a Float-element set
                // (`HashSet<f64>`) does not compile (f64 is not Eq/Hash) and is
                // unsupported in pyrst today — this widening only keeps the
                // emission consistent with the list path; it does not make a
                // numeric set literal compilable (card 5c2f31d8).
                let widen = matches!(self.list_elem_ty(elems), Ty::Float);
                let mut items = Vec::new();
                for e in elems {
                    items.push(self.emit_collection_elem(e, widen)?);
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
                                Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) =>
                                    format!("({}).py_repr()", raw),
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
                            match self.type_of_expr(&args[0]) {
                                Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) =>
                                    return Ok(format!("({}).py_repr()", a)),
                                _ => return Ok(format!("format!(\"{{}}\" , {})", a)),
                            }
                        }
                        "open" => {
                            let path = self.emit_expr(&args[0])?;
                            let mode = if args.len() >= 2 {
                                self.emit_expr(&args[1])?
                            } else {
                                "\"r\".to_string()".to_string()
                            };
                            return Ok(format!("__py_open(&{}, &{})", path, mode));
                        }
                        "int" => {
                            let a = self.emit_expr(&args[0])?;
                            let arg_type = self.type_of_expr(&args[0]);
                            match arg_type {
                                Ty::Str => {
                                    // Use helper so a bad string panics with "ValueError panic: ..."
                                    // which the try/except dispatcher can match on ValueError.
                                    return Ok(format!("(__py_int_from_str(&{}))", a));
                                }
                                _ => return Ok(format!("({} as i64)", a)),
                            }
                        }
                        "float" => {
                            let a = self.emit_expr(&args[0])?;
                            let arg_type = self.type_of_expr(&args[0]);
                            match arg_type {
                                Ty::Str => {
                                    // Use helper so a bad string panics with "ValueError panic: ..."
                                    // which the try/except dispatcher can match on ValueError.
                                    return Ok(format!("(__py_float_from_str(&{}))", a));
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
                                let key_ret_ty = if let Expr::Lambda { params: _, body, .. } = key_expr {
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
                                // Both the `None` literal (NoneVal) and a void
                                // result (Unit) report Python's NoneType, matching
                                // the pre-NoneVal behavior of `type(None)`.
                                Ty::Unit | Ty::NoneVal => "<class 'NoneType'>",
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
                            let _obj = self.emit_expr(&args[0])?;
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

                        // Use ::new() constructor whenever __init__ is defined —
                        // including the zero-arg case so that __init__ side-effects
                        // (field assignments, etc.) always run.
                        if has_init {
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
                                self.zeroed_default(&ty)
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

                    // Mutating list/set/dict methods need an lvalue receiver. For
                    // a *subscripted* receiver (`self.rows[i].append(x)`,
                    // `grid[r].sort()`) emit_expr would produce a clone-based
                    // rvalue, so the mutation would hit (and drop) a temporary.
                    // Use emit_place for those so the in-place mutation lands on
                    // the real element. Bare-name and `self.field` receivers are
                    // already place expressions under emit_expr.
                    // MUTATING_METHODS is the module-level const above.
                    let obj_s = if matches!(obj.as_ref(), Expr::Index { .. })
                        && MUTATING_METHODS.contains(&name.as_str())
                    {
                        self.emit_place(obj)?
                    } else {
                        self.emit_expr(obj)?
                    };
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

                    // File methods (PyFile; gated on a File receiver). write takes
                    // &str, so borrow the argument.
                    if let Ty::File = self.type_of_expr(obj) {
                        match name.as_str() {
                            "write" if !parts.is_empty() => return Ok(format!("{}.write(&{})", obj_s, parts[0])),
                            "write" => return Err(crate::diag::Error::Codegen("file write() requires one argument".into())),
                            "read" | "readlines" | "close" =>
                                return Ok(format!("{}.{}()", obj_s, name)),
                            _ => {}
                        }
                    }

                    // Dict views - materialize into a Vec so they work both in a
                    // for-loop and as a value (e.g. print(d.keys()), len(d.values())),
                    // matching their List(K)/List(V) static type.
                    if name == "keys" {
                        return Ok(format!("{}.keys().cloned().collect::<Vec<_>>()", obj_s));
                    }
                    if name == "values" {
                        return Ok(format!("{}.values().cloned().collect::<Vec<_>>()", obj_s));
                    }
                    if name == "items" {
                        // Collect into a Vec<(K, V)> so the for-loop lowering treats it
                        // as a normal collection (it wraps the iterable in .iter().cloned()).
                        return Ok(format!("{}.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>()", obj_s));
                    }

                    // Set methods (gated on receiver type — many names overlap with
                    // list/dict, so disambiguate by the static type of the receiver).
                    if let Ty::Set(_) = self.type_of_expr(obj) {
                        match name.as_str() {
                            // insert takes ownership, so emit the element owned
                            // (a String var becomes `x.clone()`).
                            "add" if !parts.is_empty() =>
                                return Ok(format!("{{ {}.insert({}); }}", obj_s, self.emit_owned(&args[0])?)),
                            // NB: unlike Python, neither discard nor remove raises on an
                            // absent element here (Rust's HashSet::remove returns an ignored bool).
                            "discard" | "remove" if !parts.is_empty() =>
                                return Ok(format!("{{ {}.remove(&{}); }}", obj_s, parts[0])),
                            "update" if !parts.is_empty() =>
                                return Ok(format!("{{ {}.extend({}.iter().cloned()); }}", obj_s, parts[0])),
                            "union" if !parts.is_empty() =>
                                return Ok(format!("{}.union(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0])),
                            "intersection" if !parts.is_empty() =>
                                return Ok(format!("{}.intersection(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0])),
                            "difference" if !parts.is_empty() =>
                                return Ok(format!("{}.difference(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0])),
                            "symmetric_difference" if !parts.is_empty() =>
                                return Ok(format!("{}.symmetric_difference(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0])),
                            "issubset" if !parts.is_empty() =>
                                return Ok(format!("{}.is_subset(&{})", obj_s, parts[0])),
                            "issuperset" if !parts.is_empty() =>
                                return Ok(format!("{}.is_superset(&{})", obj_s, parts[0])),
                            "isdisjoint" if !parts.is_empty() =>
                                return Ok(format!("{}.is_disjoint(&{})", obj_s, parts[0])),
                            _ => {}
                        }
                    }

                    // dict.update(other) — merge another mapping in place.
                    if name == "update" && !parts.is_empty() {
                        return Ok(format!("{{ {}.extend({}.clone()); }}", obj_s, parts[0]));
                    }

                    if name == "pop" {
                        // list.pop(): remove and return the last element (or pop(i) -> remove index).
                        if let Ty::List(_) = self.type_of_expr(obj) {
                            return Ok(if parts.is_empty() {
                                format!("{}.pop().expect(\"pop from empty list\")", obj_s)
                            } else {
                                // Honor Python negative indices: pop(-1) is the last element.
                                format!(
                                    "{{ let __n = {obj}.len() as i64; let __i = {idx}; \
                                     {obj}.remove((if __i < 0 {{ __n + __i }} else {{ __i }}) as usize) }}",
                                    obj = obj_s, idx = parts[0]
                                )
                            });
                        }
                        // dict.pop(key[, default])
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
                // (EPIC-5) Look up the callee signature so an argument flowing
                // into an `Optional[T]` parameter is wrapped (`Some(..)` for a
                // bare value, `None` for the None literal, pass-through for an
                // already-Optional value) — the same coercion as assignment and
                // return. Methods / unknown callees keep the bare emission.
                let param_tys: Vec<Ty> = if let Expr::Ident(n, _) = callee.as_ref() {
                    self.ctx.funcs.get(n.as_str())
                        .map(|sig| sig.params.iter().map(|(_, t)| t.clone()).collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let mut parts = Vec::with_capacity(args.len());
                for (i, a) in args.iter().enumerate() {
                    let s = self.emit_owned(a)?;
                    let s = match param_tys.get(i) {
                        Some(pt) => self.coerce_to_option(s, a, pt),
                        None => s,
                    };
                    parts.push(s);
                }

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
                // type_of_expr (not just an Ident lookup) so nested/chained
                // receivers resolve — e.g. grid["row"]["x"] sees the inner Dict.
                let obj_ty = self.type_of_expr(obj);
                let o = self.emit_expr(obj)?;
                // Tuple subscript with a literal index -> Rust field access (t.N),
                // cloned so the element can be used without moving out of the tuple.
                if let Ty::Tuple(_) = obj_ty {
                    if let Expr::Int(n, _) = idx.as_ref() {
                        return Ok(format!("({}).{}.clone()", o, n));
                    }
                }
                let i = self.emit_expr(idx)?;
                match &obj_ty {
                    Ty::Dict(..) => {
                        // .expect() produces a Rust message without " panic: " delimiter;
                        // unwrap_or_else lets us emit a matchable "KeyError panic: ..." payload.
                        format!("({}.get(&{}).cloned().unwrap_or_else(|| panic!(\"KeyError panic: {{:?}}\", &{})))", o, i, i)
                    }
                    Ty::Str => {
                        // String indexing with negative index support.
                        // Explicit bounds check emits "IndexError panic: ..." so the
                        // try/except dispatcher can catch it as IndexError.
                        format!(
                            "{{ let __chars: Vec<char> = {}.chars().collect(); let __idx = if {} < 0 {{ ((__chars.len() as i64) + {}) as usize }} else {{ {} as usize }}; if __idx >= __chars.len() {{ panic!(\"IndexError panic: string index out of range\") }}; __chars[__idx].to_string() }}",
                            o, i, i, i
                        )
                    }
                    _ => {
                        // List indexing with negative index support.
                        // Explicit bounds check emits "IndexError panic: ..." so the
                        // try/except dispatcher can catch it as IndexError.
                        format!(
                            "{{ let __list = {}.clone(); let __idx = if {} < 0 {{ ((__list.len() as i64) + {}) as usize }} else {{ {} as usize }}; if __idx >= __list.len() {{ panic!(\"IndexError panic: list index out of range\") }}; __list[__idx].clone() }}",
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
                        // any float operand -> float power. Use the __py_ipow helper for
                        // the integer case so a negative exponent panics with a clear
                        // message instead of silently wrapping `as u32` to a huge value.
                        if matches!(lt, Ty::Int) && matches!(rt, Ty::Int) {
                            return Ok(format!("__py_ipow(({}), ({}))", l, r));
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
                    BinOp::FloorDiv => {
                        // Python `//` floors toward negative infinity; Rust integer `/`
                        // truncates toward zero and Rust float `/` does not floor at all.
                        // For integer operands use __py_floordiv which also panics on /0
                        // with a catchable ZeroDivisionError payload.
                        // For float operands keep the f64 path (float //0.0 -> INF in
                        // Python is also a ZeroDivisionError but lower-priority; noted as
                        // a known gap).
                        let is_float = matches!(lt, Ty::Float) || matches!(rt, Ty::Float);
                        if is_float {
                            return Ok(format!("((({} as f64) / ({} as f64)).floor())", l, r));
                        }
                        return Ok(format!("__py_floordiv(({}), ({}))", l, r));
                    }
                    BinOp::Mod => {
                        // Python `%` returns a result with the sign of the divisor; Rust
                        // `%` returns the sign of the dividend. Use the divisor-signed
                        // helper for ints (single evaluation), rem_euclid-style for floats.
                        let is_float = matches!(lt, Ty::Float) || matches!(rt, Ty::Float);
                        if is_float {
                            return Ok(format!(
                                "{{ let __a = ({} as f64); let __b = ({} as f64); (((__a % __b) + __b) % __b) }}",
                                l, r
                            ));
                        }
                        return Ok(format!("__py_mod(({}), ({}))", l, r));
                    }
                    _ => {
                        let op_s = match op {
                            BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*",
                            BinOp::Div => "/",
                            BinOp::Eq => "==", BinOp::Ne => "!=",
                            BinOp::Lt => "<", BinOp::Le => "<=",
                            BinOp::Gt => ">", BinOp::Ge => ">=",
                            BinOp::And => "&&", BinOp::Or => "||",
                            BinOp::Is => "==", BinOp::IsNot => "!=",
                            BinOp::BitAnd => "&", BinOp::BitOr => "|", BinOp::BitXor => "^",
                            BinOp::LShift => "<<", BinOp::RShift => ">>",
                            BinOp::In | BinOp::NotIn => unreachable!(), // handled above
                            BinOp::Pow => unreachable!(), // handled above
                            BinOp::FloorDiv | BinOp::Mod => unreachable!(), // handled above
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
                // Emit closure params WITHOUT a type annotation and let Rust infer
                // each param's type from the use site: the call-site argument for
                // an inline-invoked lambda `(lambda x: ...)(5)`, or the iterator
                // element type for a lambda passed to map()/filter(). Hardcoding
                // `: i64` was only correct for int iterables and broke e.g.
                // `map(lambda w: len(w), words)` over a list[str].
                let param_strs: Vec<String> = params.iter()
                    .map(|(name, _ty)| name.clone())
                    .collect();
                let body_s = self.emit_expr(body)?;
                format!("|{}| {}", param_strs.join(", "), body_s)
            }
            Expr::IfExp { test, body, orelse, .. } => {
                // Python's `body if test else orelse` -> Rust's if-expression.
                let t = self.emit_expr(test)?;
                let b = self.emit_expr(body)?;
                let o = self.emit_expr(orelse)?;
                format!("(if {} {{ {} }} else {{ {} }})", t, b, o)
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
                        // Python `//` floors toward negative infinity. Rust `/` truncates
                        // toward zero, so adjust the quotient down by one when the
                        // truncated remainder is non-zero and its sign differs from the
                        // divisor's (do NOT use div_euclid — wrong for negative divisors).
                        BinOp::FloorDiv if *b != 0 => {
                            let q = a / b;
                            let r = a % b;
                            if r != 0 && ((r < 0) != (*b < 0)) { Some(q - 1) } else { Some(q) }
                        }
                        // Python `%` takes the sign of the divisor. Adjust Rust's
                        // dividend-signed remainder into the divisor's sign.
                        BinOp::Mod if *b != 0 => {
                            let m = a % b;
                            if m != 0 && ((m < 0) != (*b < 0)) { Some(m + b) } else { Some(m) }
                        }
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
        // The `None` literal's type. It never appears as a real binding
        // annotation (annotations come from `from_type_expr`, which yields
        // `Unit`/`Option`, never `NoneVal`); this arm exists for exhaustiveness
        // and mirrors `Unit` (`None` as a bare value is an upstream type error).
        Ty::NoneVal => "()".into(),
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
        Ty::File => "PyFile".into(),
        Ty::Unknown => "()".into(),
    }
}

/// Prelude implementing CPython-style `repr` for collections, used by
/// print()/str()/f-strings so `print([1, 2, 3])` yields `[1, 2, 3]` — str
/// elements quoted, bools as True/False, floats via __py_fmt_float, nested
/// collections recursing. Set/dict entries are emitted in a stable
/// sorted-by-repr order (HashSet/HashMap have no insertion order), which may
/// differ from Python's order. Depends on __py_fmt_float/__py_fmt_bool being
/// declared earlier in the prelude.
const REPR_PRELUDE: &str = r#"trait PyRepr { fn py_repr(&self) -> String; }
impl PyRepr for i64 { fn py_repr(&self) -> String { format!("{}", self) } }
impl PyRepr for f64 { fn py_repr(&self) -> String { __py_fmt_float(*self) } }
impl PyRepr for bool { fn py_repr(&self) -> String { __py_fmt_bool(*self) } }
impl PyRepr for String {
    fn py_repr(&self) -> String {
        let mut s = String::from("'");
        for c in self.chars() {
            match c {
                '\\' => s.push_str("\\\\"),
                '\'' => s.push_str("\\'"),
                '\n' => s.push_str("\\n"),
                '\t' => s.push_str("\\t"),
                '\r' => s.push_str("\\r"),
                _ => s.push(c),
            }
        }
        s.push('\'');
        s
    }
}
impl<T: PyRepr> PyRepr for Vec<T> {
    fn py_repr(&self) -> String {
        let xs: Vec<String> = self.iter().map(|x| x.py_repr()).collect();
        format!("[{}]", xs.join(", "))
    }
}
impl<T: PyRepr> PyRepr for std::collections::HashSet<T> {
    fn py_repr(&self) -> String {
        if self.is_empty() { return "set()".to_string(); }
        let mut xs: Vec<String> = self.iter().map(|x| x.py_repr()).collect();
        xs.sort();
        format!("{{{}}}", xs.join(", "))
    }
}
impl<K: PyRepr, V: PyRepr> PyRepr for std::collections::HashMap<K, V> {
    fn py_repr(&self) -> String {
        let mut xs: Vec<String> = self.iter().map(|(k, v)| format!("{}: {}", k.py_repr(), v.py_repr())).collect();
        xs.sort();
        format!("{{{}}}", xs.join(", "))
    }
}
impl<A: PyRepr> PyRepr for (A,) { fn py_repr(&self) -> String { format!("({},)", self.0.py_repr()) } }
impl<A: PyRepr, B: PyRepr> PyRepr for (A, B) { fn py_repr(&self) -> String { format!("({}, {})", self.0.py_repr(), self.1.py_repr()) } }
impl<A: PyRepr, B: PyRepr, C: PyRepr> PyRepr for (A, B, C) { fn py_repr(&self) -> String { format!("({}, {}, {})", self.0.py_repr(), self.1.py_repr(), self.2.py_repr()) } }
impl<A: PyRepr, B: PyRepr, C: PyRepr, D: PyRepr> PyRepr for (A, B, C, D) { fn py_repr(&self) -> String { format!("({}, {}, {}, {})", self.0.py_repr(), self.1.py_repr(), self.2.py_repr(), self.3.py_repr()) } }
impl<A: PyRepr, B: PyRepr, C: PyRepr, D: PyRepr, E: PyRepr> PyRepr for (A, B, C, D, E) { fn py_repr(&self) -> String { format!("({}, {}, {}, {}, {})", self.0.py_repr(), self.1.py_repr(), self.2.py_repr(), self.3.py_repr(), self.4.py_repr()) } }
impl<A: PyRepr, B: PyRepr, C: PyRepr, D: PyRepr, E: PyRepr, F: PyRepr> PyRepr for (A, B, C, D, E, F) { fn py_repr(&self) -> String { format!("({}, {}, {}, {}, {}, {})", self.0.py_repr(), self.1.py_repr(), self.2.py_repr(), self.3.py_repr(), self.4.py_repr(), self.5.py_repr()) } }
"#;

/// Prelude implementing the minimal file-object model: `open()` -> PyFile, with
/// read/readlines/write/close. The PyFile owns a std::fs::File, so a `with
/// open(...) as f:` block closes it via RAII when the Rust scope ends. I/O
/// errors panic (matching pyrst's raise->panic model). readlines() strips line
/// endings (a documented deviation from CPython, which keeps them).
const FILE_PRELUDE: &str = r#"struct PyFile { inner: std::fs::File }
impl PyFile {
    fn read(&mut self) -> String {
        use std::io::Read;
        let mut s = String::new();
        self.inner.read_to_string(&mut s).expect("read failed");
        s
    }
    fn readlines(&mut self) -> Vec<String> {
        self.read().lines().map(|l| l.to_string()).collect()
    }
    fn write(&mut self, s: &str) {
        use std::io::Write;
        self.inner.write_all(s.as_bytes()).expect("write failed");
    }
    fn close(&mut self) {}
}
fn __py_open(path: &str, mode: &str) -> PyFile {
    let f = match mode {
        "w" => std::fs::File::create(path).expect("open failed"),
        "a" => std::fs::OpenOptions::new().create(true).append(true).open(path).expect("open failed"),
        _ => std::fs::File::open(path).expect("open failed"),
    };
    PyFile { inner: f }
}
"#;

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
    // Python integer modulo: the result takes the sign of the divisor (Rust's
    // `%` takes the sign of the dividend). Panics on b==0 with a catchable
    // "ZeroDivisionError panic: ..." payload matching the try/except dispatcher.
    cg.line("fn __py_mod(a: i64, b: i64) -> i64 {");
    cg.line("    if b == 0 { panic!(\"ZeroDivisionError panic: integer division or modulo by zero\"); }");
    cg.line("    let m = a % b;");
    cg.line("    if m != 0 && ((m < 0) != (b < 0)) { m + b } else { m }");
    cg.line("}");
    // Python integer floor division: floors toward negative infinity.
    // Panics on b==0 with a catchable "ZeroDivisionError panic: ..." payload.
    // The f64 path previously used here silently returned i64::MAX for x//0.
    cg.line("fn __py_floordiv(a: i64, b: i64) -> i64 {");
    cg.line("    if b == 0 { panic!(\"ZeroDivisionError panic: integer division or modulo by zero\"); }");
    cg.line("    let q = a / b;");
    cg.line("    if (a % b != 0) && ((a % b < 0) != (b < 0)) { q - 1 } else { q }");
    cg.line("}");
    // int() from str: panics with catchable "ValueError panic: ..." payload
    // instead of Rust's generic unwrap message.
    cg.line("fn __py_int_from_str(s: &str) -> i64 {");
    cg.line("    s.trim().parse::<i64>().unwrap_or_else(|_| panic!(\"ValueError panic: invalid literal for int() with base 10: '{}'\", s.trim()))");
    cg.line("}");
    // float() from str: panics with catchable "ValueError panic: ..." payload.
    cg.line("fn __py_float_from_str(s: &str) -> f64 {");
    cg.line("    s.trim().parse::<f64>().unwrap_or_else(|_| panic!(\"ValueError panic: could not convert string to float: '{}'\", s.trim()))");
    cg.line("}");
    // Python integer exponentiation. A negative exponent yields a float in
    // Python, which cannot be represented in an i64 result, so panic with a
    // clear message rather than silently wrapping the `as u32` cast.
    cg.line("fn __py_ipow(base: i64, exp: i64) -> i64 {");
    cg.line("    if exp < 0 { panic!(\"negative exponent for integer ** integer\"); }");
    cg.line("    base.pow(exp as u32)");
    cg.line("}");
    cg.line(REPR_PRELUDE);
    cg.line(FILE_PRELUDE);
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
