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
// `MUTATING_METHODS` (collection in-place mutators) now lives in one place —
// `crate::typeck::MUTATING_METHODS` — and is consumed here by
// `method_modifies_self` (to infer `&mut self`) and by the emission site (to
// pick `emit_place` for subscripted receivers). Imported under its original
// name so the local call sites read unchanged.
use crate::typeck::{Ty, TyCtx, MUTATING_METHODS};

/// (EPIC-5) Dunder method names that lower to Rust TRAIT impls (Display /
/// PartialEq / PartialOrd / Add / ...) rather than inherent methods. ONE source
/// of truth (same discipline as `MUTATING_METHODS` / `is_copy`): emit_class, the
/// `__super_` alias pass, and the companion-enum dispatch all key off this list
/// to decide what is NOT an ordinary dispatchable method. Hoisted from three
/// identical local arrays (EPIC-5 C2-2b-i Step 0).
const DUNDER_TRAIT_NAMES: &[&str] = &[
    "__str__", "__repr__", "__add__", "__sub__", "__mul__",
    "__eq__", "__neg__", "__bool__", "__lt__",
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
    /// (EPIC-4 V3) Transitive `&mut self` decision per `(class, method)`.
    /// Computed once by `compute_mut_self` (a pre-pass like `prescan_types`)
    /// before any emission, then consulted by `emit_func` so a method that
    /// mutates `self` only by calling another mutating `self.<m>()` is still
    /// emitted `&mut self`. Empty until the pre-pass runs.
    mut_self: HashMap<(String, String), bool>,
    /// (EPIC-4 V2-c) Names of the CURRENT function's by-reference (`Mut[T]` ->
    /// `&mut T`) param bindings. Populated by `emit_func`'s param loop and
    /// saved/restored across the call. When such a binding is itself forwarded
    /// as a by-reference call argument (e.g. a recursive `fill(visited, ..)`),
    /// `&mut visited` would re-borrow an already-`&mut` binding (rustc E0596);
    /// the call site emits an explicit reborrow `&mut *visited` instead.
    by_ref_locals: std::collections::HashSet<String>,
    /// (EPIC-5 C2-1) Closed-set polymorphism map: `base -> all subclasses in the
    /// compilation unit` (direct AND transitive). Computed once by
    /// `build_poly_map` (a pre-pass like `compute_mut_self`) from `ctx.classes`
    /// before any emission. A base is "polymorphic" iff it has a non-empty entry
    /// here (`is_polymorphic_base`). C2-1 only CONSULTS this map (in `rust_ty`'s
    /// `Class` arm) without changing output; C2-2 flips that hook to emit the
    /// companion-enum name `n__` for polymorphic bases. Empty until the pre-pass
    /// runs.
    poly_map: HashMap<String, Vec<String>>,
    /// (EPIC-5 C2-2b-i) Param names that, in the CURRENT emission context, are
    /// bound to a CONCRETE value-struct even though their pyrst type is a
    /// polymorphic base — namely `other` inside a value-struct dunder impl
    /// (`impl PartialEq/PartialOrd for B`, whose `other: &B` is the struct, not
    /// the `B__` enum). Field reads on such a receiver must NOT lower to the
    /// `__field_*` enum accessor. Populated only around eq/lt/lt_impl bodies;
    /// empty everywhere else (a regular base-typed param IS the enum and DOES
    /// lower). `self` is exempt structurally, so it is never added here.
    concrete_struct_params: std::collections::HashSet<String>,
    /// Names of ALL module-level constants (any type). A reference to a module
    /// const — bare `CONST` (when not shadowed by a local) or qualified
    /// `X.CONST` — must emit the MANGLED Rust name (`mangle_const`), never the
    /// bare pyrst name, so a lowercase const cannot become a Rust const-pattern
    /// in a closure/`for`/`match` position. Populated by `emit_program` from
    /// every module's const declarations before emission; empty on paths that
    /// build a `Codegen` directly (no module constants there).
    const_names: std::collections::HashSet<String>,
    /// Names of module-level STRING constants (a subset of `const_names`). A str
    /// const lowers to a Rust `const NAME: &str` (a `String` is not
    /// const-constructible), so a reference to it must additionally append
    /// `.to_string()` to recover pyrst's `str == Rust String` value type.
    /// int/float/bool consts are `Copy` and need no such fix-up.
    const_strs: std::collections::HashSet<String>,
    /// Generators (EAGER v1): true while emitting a function whose body contains
    /// `yield`. Such a function is lowered to one that declares
    /// `let mut __gen: Vec<T> = Vec::new();` at the top, lowers each `yield x` to
    /// `__gen.push(x);`, and returns `__gen` (both at end-of-body fall-off and at
    /// any bare `return`, which means "stop collecting"). Saved/restored per
    /// function like `current_ret_ty`. EAGER LIMITATION: an INFINITE generator
    /// (`while True: yield ...`) collects forever and hangs / OOMs at runtime —
    /// true lazy iteration (a state-machine / `impl Iterator` transform) is an
    /// explicit follow-up; v1 is faithful for FINITE generators (the common case).
    in_generator: bool,
}

impl<'a> Codegen<'a> {
    pub fn new(ctx: &'a TyCtx) -> Self {
        Self { ctx, out: String::new(), indent: 0, locals: HashMap::new(), declared: Default::default(), current_class: None, current_ret_ty: Ty::Unit, dead_funcs: Default::default(), mut_self: HashMap::new(), by_ref_locals: Default::default(), poly_map: HashMap::new(), concrete_struct_params: Default::default(), const_names: Default::default(), const_strs: Default::default(), in_generator: false }
    }

    pub fn with_dead_funcs(mut self, dead: std::collections::HashSet<String>) -> Self {
        self.dead_funcs = dead;
        self
    }

    /// Thin wrapper over the single shared copy-ness predicate
    /// (`crate::typeck::is_copy`) so the derive/Default decisions read cleanly.
    /// The LOGIC lives in one place; this is only sugar for the `self.` call sites.
    fn is_copy_type(&self, ty: &Ty) -> bool {
        crate::typeck::is_copy(ty)
    }

    /// Returns true when `ty` implements the `Default` trait in the emitted Rust.
    /// Copy classes (all-primitive fields) don't derive Default, so they return false.
    fn type_has_default(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Unit => true,
            Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Option(_) => true,
            Ty::Class(n) => {
                // (EPIC-5 C2-3) A polymorphic base lowers (via `rust_ty`) to its
                // companion enum `n__`, a data-variant enum that CANNOT derive
                // `Default` (emit_companion_enum is `#[derive(Clone, Debug)]`
                // only). So an outer struct holding such a field must NOT include
                // `Default` in its own derive list, and such a local is not
                // hoistable with `Default::default()`.
                if self.is_polymorphic_base(n) {
                    return false;
                }
                // Copy classes don't get #[derive(Default)] (see emit_class).
                let all_copy = self.ctx.get_all_fields(n).iter().all(|f| {
                    Ty::from_type_expr(&f.ty, f.span).map(|t| self.is_copy_type(&t)).unwrap_or(false)
                });
                !all_copy
            }
            _ => false,
        }
    }

    /// (EPIC-5 C2-3) True when the companion enum `base__` carries `impl PartialEq`.
    /// `emit_companion_enum` forwards `PartialEq` to the variant structs ONLY when
    /// EVERY variant defines `__eq__` (its `all_have_eq` predicate); otherwise the
    /// enum has no `PartialEq` at all (cross-variant equality is honestly absent).
    /// A struct holding a polymorphic-base field can therefore derive `PartialEq`
    /// only when this returns true — mirrors `emit_companion_enum`'s `all_have_eq`.
    fn companion_enum_has_partial_eq(&self, base: &str) -> bool {
        if !self.is_polymorphic_base(base) {
            return false;
        }
        let mut variants: Vec<String> = vec![base.to_string()];
        if let Some(subs) = self.poly_map.get(base) {
            variants.extend(subs.iter().cloned());
        }
        variants
            .iter()
            .all(|v| self.resolved_methods(v).iter().any(|m| m.name == "__eq__"))
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
                    Ty::from_type_expr(&f.ty, f.span).map(|t| self.is_copy_type(&t)).unwrap_or(false)
                });
                let struct_init = if all_copy {
                    // Build a struct literal with zeroed primitive fields.
                    let fields: Vec<String> = self.ctx.get_all_fields(n).iter().map(|f| {
                        let inner_ty = Ty::from_type_expr(&f.ty, f.span).unwrap_or(Ty::Int);
                        // (EPIC-6) Escape a keyword field name in the zeroed
                        // struct-literal default (matches the struct field def).
                        format!("{}: {}", escape_ident(&f.name), self.zeroed_default(&inner_ty))
                    }).collect();
                    format!("{} {{ {} }}", n, fields.join(", "))
                } else {
                    "Default::default()".to_string()
                };
                // (EPIC-5 C2-2b-i) A polymorphic-base local is Rust `B__`, so the
                // zeroed initializer must be the base variant carrying the zeroed
                // value struct (`B__::B(B{..})`), not a bare struct literal (the
                // wrong type for the enum slot). Leaf/non-polymorphic classes keep
                // the plain struct init.
                if self.is_polymorphic_base(n) {
                    format!("{}__::{}({})", n, n, struct_init)
                } else {
                    struct_init
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

                // (EPIC-6) A raw identifier `r#kw` is a single token: if we are at
                // a bare `r` immediately followed by `#` and then an identifier
                // char, absorb the `#` so the whole `r#kw` is collected as one
                // token (and can match a `r#`-escaped `old_name`). Without this,
                // `r#type` would split into `r` / `#` / `type` and a replace of
                // `r#type` would corrupt the raw identifier.
                if ch == 'r' && chars.peek() == Some(&'#') {
                    let mut probe = chars.clone();
                    probe.next(); // consume '#'
                    if matches!(probe.peek(), Some(c) if c.is_alphanumeric() || *c == '_') {
                        chars.next(); // consume '#'
                        ident.push('#');
                    }
                }

                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_alphanumeric() || next_ch == '_' {
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

    /// (EPIC-6) Emit a user-defined method call `obj_s.method_name(args)` on a
    /// known class receiver `cls`, threading per-param by-reference (`Mut[T]`)
    /// arguments exactly like the long-standing "Regular method call" tail of
    /// the dispatch block. Factored out so the receiver-type-guarded early
    /// return (which routes a user-class receiver PAST the builtin arms, fixing
    /// the silent miscompile where `instance.get(k)` lowered to a dict
    /// `.get(&k).cloned()`) reuses the SAME by-ref/companion-enum emission
    /// rather than duplicating-and-drifting it.
    ///
    /// `method_name` is the user method's RAW name — not the builtin remap (so a
    /// user method legitimately named `append`/`upper`/`pop` calls the real
    /// `obj.append(..)` inherent/dispatch method, not the remapped `.push(..)`).
    /// For a polymorphic-base receiver `cls` is the base name and the per-param
    /// flags come from `get_method(base, name)` (the base's signature), so the
    /// emitted `obj_s.method_name(..)` resolves to the companion enum `cls__`'s
    /// dispatch method — identical to the pre-existing EPIC-5 lowering.
    fn emit_user_method_call(
        &mut self,
        obj_s: &str,
        cls: &str,
        method_name: &str,
        args: &[Expr],
        parts: &[String],
    ) -> Result<String> {
        let method_by_ref: Vec<bool> = self
            .ctx
            .get_method(cls, method_name)
            .map(|sig| sig.param_by_ref.clone())
            .unwrap_or_default();
        if method_by_ref.iter().any(|&b| b) {
            let mut mparts = Vec::with_capacity(args.len());
            for (i, a) in args.iter().enumerate() {
                if method_by_ref.get(i).copied().unwrap_or(false) {
                    let place = self.emit_place(a)?;
                    mparts.push(self.byref_borrow(a, &place));
                } else {
                    mparts.push(self.emit_consuming(a)?);
                }
            }
            return Ok(format!("{}.{}({})", obj_s, method_name, mparts.join(", ")));
        }
        Ok(format!("{}.{}({})", obj_s, method_name, parts.join(", ")))
    }

    /// (EPIC-5 C1-C) Honest codegen gate for class subtyping.
    ///
    /// Part B made typeck ACCEPT a `Derived` value flowing into a `Base` slot
    /// (`is_subclass(derived, base)`), but codegen cannot yet EMIT it: each pyrst
    /// class is a standalone Rust struct, so a `Dog` value does not fit a slot
    /// typed `Animal` and rustc would reject it with an opaque E0308. Until the
    /// EPIC-5 C2 companion-enum codegen lands, refuse such a flow here with a
    /// clear pyrst error instead of leaking a raw rustc failure.
    ///
    /// Fires ONLY for a strictly-derived class pair (`got != expected` and
    /// `is_subclass(got, expected)` holds). Exact-type flows (`got == expected`),
    /// non-class types, and unrelated classes (which typeck already rejected)
    /// pass through untouched, so no existing exact-typed example is affected.
    /// (EPIC-5 C2-2b-i) True iff `ty` mentions a polymorphic base anywhere — i.e.
    /// a slot of this type lowers (via `rust_ty`) to a companion enum `B__` at
    /// some position, so a raw-struct value flowing in needs WRAPPING. When this
    /// is false the slot is exact-typed and the legacy `emit_consuming` path is
    /// used unchanged (keeps every non-polymorphic example byte-for-byte stable).
    fn ty_has_poly_base(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Class(n) => self.is_polymorphic_base(n),
            Ty::List(e) | Ty::Set(e) | Ty::Option(e) => self.ty_has_poly_base(e),
            Ty::Dict(k, v) => self.ty_has_poly_base(k) || self.ty_has_poly_base(v),
            Ty::Tuple(ts) => ts.iter().any(|t| self.ty_has_poly_base(t)),
            _ => false,
        }
    }

    /// (first-class functions) True iff `ty` is a function type or a collection
    /// whose element / value type is one — i.e. a slot of this type contains an
    /// `Rc<dyn Fn>` position into which a bare function NAME or lambda must be
    /// wrapped (`emit_into_func_slot`). When false the slot has no function
    /// position and the legacy clone-on-use path is used unchanged.
    fn ty_has_func(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Func(..) => true,
            Ty::List(e) | Ty::Set(e) | Ty::Option(e) => self.ty_has_func(e),
            Ty::Dict(k, v) => self.ty_has_func(k) || self.ty_has_func(v),
            Ty::Tuple(ts) => ts.iter().any(|t| self.ty_has_func(t)),
            _ => false,
        }
    }

    /// If `e` is a constructor call `C(...)` for a user class `C`, return `C`.
    /// (Mirrors `infer_expr_ty`'s constructor recognition: a Call whose callee is
    /// a bare Ident registered in `ctx.classes`.) Used to disambiguate a RAW
    /// struct temp (a constructor) from an enum-typed place at a base slot.
    fn constructor_class(&self, e: &Expr) -> Option<String> {
        if let Expr::Call { callee, .. } = e {
            if let Expr::Ident(n, _) = callee.as_ref() {
                if self.ctx.classes.contains_key(n.as_str()) {
                    return Some(n.clone());
                }
            }
        }
        None
    }

    /// (EPIC-5 C2-2b-i, the crux) Emit value expression `value` into a slot whose
    /// declared type `expected` mentions a polymorphic base (caller gated on
    /// `ty_has_poly_base`). Replaces the C1 honest gate: a raw-struct value at a
    /// `B__` slot is WRAPPED in the right enum variant; a value already typed as
    /// the base passes through; a strict-polymorphic-subclass place (multi-level
    /// upcast) is an HONEST Error::Codegen rather than a miscompile.
    fn emit_into_base_slot(&mut self, value: &Expr, expected: &Ty) -> Result<String> {
        match expected {
            // Scalar polymorphic-base slot `B__`.
            Ty::Class(b) if self.is_polymorphic_base(b) => {
                // A constructor `C(...)` is a RAW struct temp -> wrap as variant C.
                if let Some(ctor) = self.constructor_class(value) {
                    let inner = self.emit_consuming(value)?;
                    return Ok(format!("{}__::{}({})", b, ctor, inner));
                }
                let et = self.type_of_expr(value);
                match &et {
                    Ty::Class(c) if self.is_polymorphic_base(c) => {
                        if c == b {
                            // Already a `B__` value (a base-typed place) -> pass through.
                            self.emit_consuming(value)
                        } else if crate::typeck::is_subclass(c, b, self.ctx) {
                            // `et` is a strict POLYMORPHIC subclass: the value is an
                            // `et__` enum, NOT a `B__` variant. A From<et__> for B__
                            // up-conversion is a deferred follow-on — refuse honestly.
                            Err(crate::diag::Error::Codegen(format!(
                                "upcasting an intermediate polymorphic base `{}` to `{}` \
                                 is not yet supported — construct the value at the `{}` \
                                 slot directly (multi-level upcast deferred)",
                                c, b, b
                            )))
                        } else {
                            // Unrelated polymorphic class — typeck already rejected
                            // this flow; pass through defensively.
                            self.emit_consuming(value)
                        }
                    }
                    // A concrete / non-polymorphic value whose type is `B` or a
                    // (leaf) subclass of `B` -> RAW struct -> wrap as variant `et`.
                    Ty::Class(c) => {
                        let inner = self.emit_consuming(value)?;
                        Ok(format!("{}__::{}({})", b, c, inner))
                    }
                    // Non-class value into a base slot — should not occur (typeck);
                    // emit unchanged so any genuine mismatch surfaces as rustc E0308.
                    _ => self.emit_consuming(value),
                }
            }
            // List literal whose element slot mentions a polymorphic base: wrap
            // each element. A non-literal list (already `Vec<B__>`) passes through.
            Ty::List(elem) if self.ty_has_poly_base(elem) => {
                if let Expr::List(elems, _) = value {
                    let mut parts = Vec::with_capacity(elems.len());
                    for el in elems {
                        parts.push(self.emit_into_base_slot(el, elem)?);
                    }
                    Ok(format!("vec![{}]", parts.join(", ")))
                } else {
                    self.emit_consuming(value)
                }
            }
            // Set literal — same element wrapping as the list path.
            Ty::Set(elem) if self.ty_has_poly_base(elem) => {
                if let Expr::Set(elems, _) = value {
                    let mut parts = Vec::with_capacity(elems.len());
                    for el in elems {
                        parts.push(self.emit_into_base_slot(el, elem)?);
                    }
                    Ok(format!(
                        "vec![{}].into_iter().collect::<::std::collections::HashSet<_>>()",
                        parts.join(", ")
                    ))
                } else {
                    self.emit_consuming(value)
                }
            }
            // Tuple literal — wrap element-wise at each polymorphic-base position.
            Ty::Tuple(parts_ty) if self.ty_has_poly_base(expected) => {
                if let Expr::Tuple(elems, _) = value {
                    if elems.len() == parts_ty.len() {
                        let mut parts = Vec::with_capacity(elems.len());
                        for (el, et) in elems.iter().zip(parts_ty.iter()) {
                            if self.ty_has_poly_base(et) {
                                parts.push(self.emit_into_base_slot(el, et)?);
                            } else {
                                parts.push(self.emit_consuming(el)?);
                            }
                        }
                        return Ok(match parts.len() {
                            1 => format!("({},)", parts[0]),
                            _ => format!("({})", parts.join(", ")),
                        });
                    }
                }
                self.emit_consuming(value)
            }
            // Optional polymorphic-base slot: the bare-value case wraps the inner
            // value; the `None` literal and already-Optional values are handled by
            // the caller's `coerce_to_option`, so only a bare value reaches here.
            Ty::Option(inner) if self.ty_has_poly_base(inner) => {
                if matches!(value, Expr::None_(_)) {
                    self.emit_consuming(value)
                } else {
                    self.emit_into_base_slot(value, inner)
                }
            }
            // Dict with a polymorphic-base value/key slot through a literal is not
            // exercised by the corpus; defer element wrapping (honest passthrough —
            // a genuine subtype dict literal would surface as rustc E0308, not a
            // silent miscompile). Documented as a C2-3 gap alongside list+concat.
            _ => self.emit_consuming(value),
        }
    }

    /// (EPIC-5 C2-3) Emit constructor argument `arg` into a slot whose declared
    /// type is `slot` (a `__init__` param type, or a struct field type). When the
    /// slot mentions a polymorphic base, wrap a raw-struct/subclass value into the
    /// companion-enum variant (delegating to `emit_into_base_slot`, the same
    /// wrap-or-passthrough used at the return / annotated-assign / free-fn-arg
    /// sites); otherwise keep the uniform clone-on-use emission. A `None` slot
    /// (untyped / variadic) also keeps clone-on-use. This closes the constructor
    /// arg path, which the keystone's three `ty_has_poly_base` sites did not cover.
    fn emit_arg_into_slot(&mut self, arg: &Expr, slot: Option<&Ty>) -> Result<String> {
        match slot {
            Some(t) if self.ty_has_poly_base(t) => self.emit_into_base_slot(arg, t),
            _ => self.emit_consuming(arg),
        }
    }

    /// (first-class functions) Emit value expression `value` into a slot whose
    /// declared type `expected` is `Ty::Func(arg_tys, ret)` — i.e. a
    /// `Rc<dyn Fn(..) -> ..>` slot. Three shapes:
    ///
    ///  - A bare top-level function NAME used as a value: a Rust `fn` item
    ///    coerces to `dyn Fn`, so emit `Rc::new(<name>) as Rc<dyn Fn(..)->..>`.
    ///    The trailing `as` cast pins the type at the slot so an unannotated
    ///    binding / collection element is still well-typed.
    ///  - A LAMBDA: emit `Rc::new(move |x: A, y: B| body) as Rc<dyn Fn(..)->..>`.
    ///    Capture-by-move closes over any enclosing variable (the `make_adder`
    ///    closure captures `n`); the param TYPES come from the slot's `arg_tys`
    ///    so the closure body type-checks without inference from a call site.
    ///  - Anything else already of `Ty::Func` (a func-valued place, or a call
    ///    that already returns `Rc<dyn Fn>`): clone-on-use, which is a cheap `Rc`
    ///    refcount bump for a place and a pass-through for an owned temp.
    fn emit_into_func_slot(&mut self, value: &Expr, expected: &Ty) -> Result<String> {
        // A collection slot whose element / value type is a function
        // (`list[Callable[..]]`, `dict[K, Callable[..]]`) wraps each element /
        // value into the `Rc<dyn Fn>` slot — only when the source is the matching
        // LITERAL (so the element types are known here); a non-literal collection
        // is already `Rc<dyn Fn>`-typed and passes through via clone-on-use.
        match expected {
            Ty::List(elem) if matches!(**elem, Ty::Func(..)) => {
                if let Expr::List(elems, _) = value {
                    let mut parts = Vec::with_capacity(elems.len());
                    for el in elems {
                        parts.push(self.emit_into_func_slot(el, elem)?);
                    }
                    return Ok(format!("vec![{}]", parts.join(", ")));
                }
                return self.emit_consuming(value);
            }
            Ty::Dict(_k, vv) if self.ty_has_func(vv) => {
                if let Expr::Dict(pairs, _) = value {
                    if pairs.is_empty() {
                        return Ok("::std::collections::HashMap::new()".to_string());
                    }
                    let mut inserts = Vec::with_capacity(pairs.len());
                    for (k, v) in pairs {
                        let ks = self.emit_consuming(k)?;
                        let vs = self.emit_into_func_slot(v, vv)?;
                        inserts.push(format!("({}, {})", ks, vs));
                    }
                    return Ok(format!(
                        "vec![{}].into_iter().collect::<::std::collections::HashMap<_,_>>()",
                        inserts.join(", ")
                    ));
                }
                return self.emit_consuming(value);
            }
            // A tuple slot with one or more function-typed positions
            // (`tuple[Callable[..], int]`). Wrap each element into its own slot:
            // a func position routes through `emit_into_func_slot` (recursively),
            // a non-func position keeps the clone-on-use emission. Mirrors the
            // single-element / multi-element tuple emission in `emit_expr`.
            Ty::Tuple(elem_tys) if self.ty_has_func(expected) => {
                if let Expr::Tuple(elems, _) = value {
                    if elems.len() == elem_tys.len() {
                        let mut parts = Vec::with_capacity(elems.len());
                        for (el, et) in elems.iter().zip(elem_tys.iter()) {
                            if self.ty_has_func(et) {
                                parts.push(self.emit_into_func_slot(el, et)?);
                            } else {
                                parts.push(self.emit_consuming(el)?);
                            }
                        }
                        return Ok(match parts.len() {
                            1 => format!("({},)", parts[0]),
                            _ => format!("({})", parts.join(", ")),
                        });
                    }
                }
                return self.emit_consuming(value);
            }
            // NOTE: there is intentionally NO `Ty::Set(Func)` arm. A pyrst `set`
            // lowers to a Rust `HashSet`, which requires `Eq + Hash` elements;
            // `Rc<dyn Fn>` (and `dyn Fn`) implement neither, so `HashSet<Rc<dyn
            // Fn>>` cannot compile. `set[Callable[..]]` is therefore rejected at
            // typeck (`require_hashable`), the same way `set[float]` is — so this
            // arm is unreachable and a positive emission here would only produce
            // known-uncompilable Rust.
            _ => {}
        }
        let Ty::Func(arg_tys, _ret) = expected else {
            return self.emit_consuming(value);
        };
        let rc_ty = self.rust_ty(expected);
        match value {
            // A function NAME used as a value (must be a known top-level function,
            // not a local that happens to share the name — locals shadow and are
            // already `Rc<dyn Fn>` values handled by the clone-on-use arm below).
            Expr::Ident(n, _)
                if self.ctx.funcs.contains_key(n.as_str())
                    && !self.locals.contains_key(n.as_str()) =>
            {
                Ok(format!("::std::rc::Rc::new({}) as {}", escape_ident(n), rc_ty))
            }
            Expr::Lambda { params, body, .. } => {
                // Annotate each closure param with the slot's argument type so the
                // `move` closure is well-typed at a `dyn Fn` coercion (Rust cannot
                // infer closure param types across the boxed-trait-object cast).
                // When the slot's argument type is `Unknown`, emit the param WITHOUT
                // an annotation (let Rust infer) rather than `x: ()` — `rust_ty`
                // lowers `Unknown` to `()`, and a unit-typed param would be wrong
                // for any non-unit argument. Annotated `Callable` slots always have
                // concrete arg types (from `from_type_expr`), so for Increment 1
                // this is a defensive guard; it becomes load-bearing once a func
                // value can flow from an inferred (Unknown-arg) context.
                let param_strs: Vec<String> = params
                    .iter()
                    .enumerate()
                    .map(|(i, (name, _))| {
                        let name_e = escape_ident(name);
                        match arg_tys.get(i) {
                            Some(pty) if !matches!(pty, Ty::Unknown) => {
                                format!("{}: {}", name_e, self.rust_ty(pty))
                            }
                            _ => name_e,
                        }
                    })
                    .collect();
                let body_s = self.emit_expr(body)?;
                Ok(format!(
                    "::std::rc::Rc::new(move |{}| {}) as {}",
                    param_strs.join(", "),
                    body_s,
                    rc_ty
                ))
            }
            // A conditional `f if cond else g` into a function slot: wrap EACH
            // branch into the same slot so a bare fn name / lambda in either arm
            // becomes `Rc<dyn Fn>` (without this the arms fall to `emit_consuming`
            // and emit bare fn names -> E0308). Both arms are already typed
            // `Ty::Func` by typeck's branch unification, so each is a valid
            // func-slot value.
            Expr::IfExp { test, body, orelse, .. } => {
                let t = self.emit_expr(test)?;
                let b = self.emit_into_func_slot(body, expected)?;
                let o = self.emit_into_func_slot(orelse, expected)?;
                Ok(format!("(if {} {{ {} }} else {{ {} }})", t, b, o))
            }
            // A func-valued place / call temp — Rc clone (value semantics) / passthrough.
            _ => self.emit_consuming(value),
        }
    }

    /// (EPIC-5 C2-3) The declared pyrst `Ty` of field `field_name` on class
    /// `class_def`, looking through inherited base fields (mirrors the constructor
    /// branch's own + inherited field walk). `None` when the field is unknown.
    fn class_field_type(&self, class_def: &ClassDef, field_name: &str) -> Option<Ty> {
        self.ctx
            .get_all_fields(&class_def.name)
            .iter()
            .find(|f| f.name == field_name)
            .and_then(|f| Ty::from_type_expr(&f.ty, f.span).ok())
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
                // Silently accept a bare top-level `main()` call — the Rust
                // `fn main() { user_main(); }` already drives the entry point,
                // so this idiom is a recognised no-op.
                if matches!(
                    other,
                    Stmt::Expr(crate::ast::Expr::Call { callee, args, kwargs, .. })
                        if matches!(callee.as_ref(), crate::ast::Expr::Ident(name, _) if name == "main")
                            && args.is_empty()
                            && kwargs.is_empty()
                ) {
                    return Ok(());
                }
                // A module-level constant (`NAME: T = <literal>`) is already
                // emitted as a top-level Rust `const` by the prepass in
                // `emit_program` (which runs before any function so call sites
                // resolve), so it is a recognised no-op here.
                if crate::typeck::is_module_const_decl(other) {
                    return Ok(());
                }
                // Any other unsupported top-level statement is an honest error.
                // This arm is a backstop; typeck's check_bodies fires the same
                // rejection earlier (at `pyrst check` time).
                Err(crate::diag::Error::Codegen(
                    "top-level statements other than function/class/import \
                     definitions (and module-level constants `NAME: T = <literal>`) \
                     are not supported"
                        .to_string(),
                ))
            }
        }
    }

    /// Emit a MODULE-LEVEL CONSTANT (`NAME: T = <literal>`) as a top-level Rust
    /// `const`. Called by `emit_program`'s prepass for every statement that
    /// [`crate::typeck::is_module_const_decl`] accepts, so the value is always one
    /// of the four primitive literals.
    ///
    /// The Rust identifier is MANGLED via [`mangle_const`] (`__pyrst_const_<name>`)
    /// so a lowercase const name (e.g. `k`/`i`/`e`) cannot be captured as a Rust
    /// CONSTANT PATTERN in any closure/`for`/`match` pattern position in the
    /// generated crate (which would silently miscompile, rustc E0308). The same
    /// mangled name is emitted at every reference site.
    ///
    /// int/float/bool lower to `const <mangled>: <i64|f64|bool> = <value>;` — all
    /// `Copy`, so a reference uses the mangled name directly. A `str` constant
    /// lowers to `const <mangled>: &str = "...";` (a `String` is not
    /// const-constructible), so REFERENCES to a str const append `.to_string()`
    /// to preserve pyrst's `str == Rust String` value semantics.
    fn emit_const_decl(&mut self, s: &Stmt) -> Result<()> {
        let Stmt::Assign { target, value, .. } = s else {
            return Err(crate::diag::Error::Codegen(
                "emit_const_decl called on a non-assignment".to_string(),
            ));
        };
        let name = mangle_const(target);
        let decl = match value {
            Expr::Int(n, _) => format!("const {}: i64 = {};", name, n),
            // Suffix `f64` so a whole-number float literal (`6.0` formats as
            // "6") is still a valid f64 const initializer (`6f64`), and a
            // fractional one (`3.14`) stays `3.14f64`.
            Expr::Float(f, _) => format!("const {}: f64 = {}f64;", name, f),
            Expr::Bool(b, _) => format!("const {}: bool = {};", name, b),
            Expr::Str(st, _) => format!("const {}: &str = {:?};", name, st),
            _ => {
                return Err(crate::diag::Error::Codegen(
                    "module constant value must be an int/float/str/bool literal".to_string(),
                ))
            }
        };
        self.line(&decl);
        Ok(())
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
            // (EPIC-4 V2-c / V3 interaction) A call anywhere in this statement that
            // passes a self-rooted place (`self.field`, `self.list[i]`, ...) into a
            // by-reference (`Mut[T]`) parameter MUTATES self — the callee writes
            // through the `&mut self.field` borrow. The intra-method seed above
            // misses it (it only catches `self`-rooted assignments and mutating
            // method calls), so without this a method that mutates self ONLY by
            // handing `self.field` to a by-ref callee would be emitted `&self` and
            // rustc would reject `&mut self.field` with E0596. Detect it here so
            // the method becomes `&mut self` and propagates through the V3 fixpoint.
            if self.stmt_passes_self_by_ref(stmt) {
                return true;
            }
        }
        false
    }

    /// True when any `Expr::Call` reachable from `stmt` (in any expression
    /// position) passes a SELF-ROOTED place as a by-reference (`Mut[T]`) argument.
    /// Walks the same statement nesting `method_modifies_self` does and scans
    /// every embedded expression (conditions, RHS, return values, call args).
    fn stmt_passes_self_by_ref(&self, stmt: &Stmt) -> bool {
        let mut found = false;
        let mut check = |e: &Expr| { if self.expr_passes_self_by_ref(e) { found = true; } };
        match stmt {
            Stmt::Expr(e) | Stmt::Return(Some(e), _) => check(e),
            Stmt::Assign { value, .. } | Stmt::AugAssign { value, .. } => check(value),
            Stmt::Unpack { value, .. } => check(value),
            Stmt::AttrAssign { obj, value, .. } | Stmt::IndexAssign { obj, value, .. } => {
                check(obj);
                check(value);
            }
            Stmt::If { cond, .. } => check(cond),
            Stmt::While { cond, .. } => check(cond),
            Stmt::For { iter, .. } => check(iter),
            Stmt::With { ctx_expr, .. } => check(ctx_expr),
            _ => {}
        }
        found
    }

    /// Recursively scan `e` for a call that passes a self-rooted place into a
    /// by-reference param. For each `Expr::Call`, resolve the callee's per-param
    /// by-ref flags (free function via `ctx.funcs`; method via `get_method`,
    /// self-exclusive and index-aligned to the args after STEP 0) and report a
    /// self-rooted place sitting in a by-ref slot. Sub-expressions are walked too
    /// so a by-ref call nested in an argument / operand is still caught.
    fn expr_passes_self_by_ref(&self, e: &Expr) -> bool {
        match e {
            Expr::Call { callee, args, kwargs, .. } => {
                let by_ref: Vec<bool> = match callee.as_ref() {
                    Expr::Ident(n, _) => self.ctx.funcs.get(n.as_str())
                        .map(|s| s.param_by_ref.clone()).unwrap_or_default(),
                    Expr::Attr { obj, name, .. } => {
                        if let Ty::Class(cls) = self.type_of_expr(obj.as_ref()) {
                            self.ctx.get_method(&cls, name)
                                .map(|s| s.param_by_ref.clone()).unwrap_or_default()
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                };
                for (i, a) in args.iter().enumerate() {
                    if by_ref.get(i).copied().unwrap_or(false)
                        && Self::expr_roots_at_self(a)
                    {
                        return true;
                    }
                }
                // Walk callee + args + kwargs for nested by-ref-self calls.
                if self.expr_passes_self_by_ref(callee) { return true; }
                if args.iter().any(|a| self.expr_passes_self_by_ref(a)) { return true; }
                if kwargs.iter().any(|(_, v)| self.expr_passes_self_by_ref(v)) { return true; }
                false
            }
            Expr::Attr { obj, .. } => self.expr_passes_self_by_ref(obj),
            Expr::Index { obj, idx, .. } => {
                self.expr_passes_self_by_ref(obj) || self.expr_passes_self_by_ref(idx)
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.expr_passes_self_by_ref(lhs) || self.expr_passes_self_by_ref(rhs)
            }
            Expr::UnOp { expr, .. } => self.expr_passes_self_by_ref(expr),
            Expr::IfExp { test, body, orelse, .. } => {
                self.expr_passes_self_by_ref(test)
                    || self.expr_passes_self_by_ref(body)
                    || self.expr_passes_self_by_ref(orelse)
            }
            _ => false,
        }
    }

    // ───────────────────────── (EPIC-4 V3) transitive &mut self ──────────────
    //
    // `method_modifies_self` above is INTRA-method: it sees `self.x = v` and
    // `self.items.append(x)`, but it does NOT follow a call to another method
    // (`self.advance()`). So a method that mutates `self` only by delegating to
    // a mutating `self.<helper>()` was emitted `&self` → rustc E0596.
    //
    // We close that gap with a call-graph fixpoint, computed once before any
    // emission (`compute_mut_self`, run from `emit_program`) and consulted by
    // `emit_func`:
    //   1. seed `mutates[(C, m)] = method_modifies_self(m.body)` (the precise
    //      intra-method analysis — kept verbatim as the seed),
    //   2. build `self_calls[(C, m)]` = the `self.<name>()` callees in `m`,
    //   3. propagate: `mutates[k] |= any(mutates[resolve(C, c)])` to a fixpoint.
    // Keys are `(emitting_class, method_name)`: `emit_class` emits every
    // RESOLVED method (own + inherited) onto the subclass struct, so an
    // inherited body is keyed under the subclass and its self-calls resolve
    // against the SUBCLASS MRO — an inherited mutating method propagates `&mut`
    // up to a subclass caller.

    /// Collect the set of method names invoked as `self.<name>(...)` anywhere in
    /// `body`, walking the SAME statement nesting `method_modifies_self` does
    /// (if/elif/else, while, for, try body+handlers+else+finally, with) AND the
    /// expression positions a call can hide in (assignment RHS, return value,
    /// conditions, call args, …). Scope is `self.<method>()` chains ONLY: the
    /// receiver must be exactly `self` (`Expr::Attr { obj: Ident("self"), name }`).
    /// `self.child.method()` — a method on a FIELD — is intentionally NOT
    /// collected (that is nested-mutation / V2-d territory, out of scope here).
    fn collect_self_calls(&self, body: &[Stmt], out: &mut std::collections::HashSet<String>) {
        for stmt in body {
            match stmt {
                Stmt::Expr(e) | Stmt::Return(Some(e), _) => Self::collect_self_calls_expr(e, out),
                Stmt::Assign { value, .. } | Stmt::AugAssign { value, .. } => {
                    Self::collect_self_calls_expr(value, out)
                }
                Stmt::Unpack { value, .. } => Self::collect_self_calls_expr(value, out),
                Stmt::AttrAssign { obj, value, .. } => {
                    Self::collect_self_calls_expr(obj, out);
                    Self::collect_self_calls_expr(value, out);
                }
                Stmt::IndexAssign { obj, idx, value, .. } => {
                    Self::collect_self_calls_expr(obj, out);
                    Self::collect_self_calls_expr(idx, out);
                    Self::collect_self_calls_expr(value, out);
                }
                Stmt::If { cond, then, elifs, else_, .. } => {
                    Self::collect_self_calls_expr(cond, out);
                    self.collect_self_calls(then, out);
                    for (c, elif_body) in elifs {
                        Self::collect_self_calls_expr(c, out);
                        self.collect_self_calls(elif_body, out);
                    }
                    if let Some(else_body) = else_ {
                        self.collect_self_calls(else_body, out);
                    }
                }
                Stmt::While { cond, body, .. } => {
                    Self::collect_self_calls_expr(cond, out);
                    self.collect_self_calls(body, out);
                }
                Stmt::For { iter, body, .. } => {
                    Self::collect_self_calls_expr(iter, out);
                    self.collect_self_calls(body, out);
                }
                Stmt::Try { body, handlers, else_, finally_, .. } => {
                    self.collect_self_calls(body, out);
                    for handler in handlers {
                        self.collect_self_calls(&handler.body, out);
                    }
                    if let Some(else_body) = else_ {
                        self.collect_self_calls(else_body, out);
                    }
                    if let Some(finally_body) = finally_ {
                        self.collect_self_calls(finally_body, out);
                    }
                }
                Stmt::With { ctx_expr, body, .. } => {
                    Self::collect_self_calls_expr(ctx_expr, out);
                    self.collect_self_calls(body, out);
                }
                _ => {}
            }
        }
    }

    /// Recurse into an expression collecting `self.<name>(...)` method callees.
    /// Only a call whose callee is `self.<name>` *directly* (receiver is the bare
    /// `self` ident) is recorded; the callee subexpressions are still walked so a
    /// nested `self.a(self.b())` records both `a` and `b`.
    fn collect_self_calls_expr(expr: &Expr, out: &mut std::collections::HashSet<String>) {
        match expr {
            Expr::Call { callee, args, kwargs, .. } => {
                if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                    match obj.as_ref() {
                        // Direct `self.<name>(...)`.
                        Expr::Ident(n, _) if n == "self" => {
                            out.insert(name.clone());
                        }
                        // `super().<name>(...)` lowers to `self.__super_<name>()`
                        // (an alias carrying the immediate parent's body). Record
                        // it under that exact emitted name so the fixpoint can
                        // propagate &mut from a mutating inherited method up to a
                        // delegating-only override (e.g. a `__init__` that does
                        // nothing but `super().__init__()`).
                        Expr::Call { callee: sup, args: sup_args, .. }
                            if sup_args.is_empty()
                                && matches!(sup.as_ref(), Expr::Ident(s, _) if s == "super") =>
                        {
                            out.insert(format!("__super_{}", name));
                        }
                        _ => {}
                    }
                }
                Self::collect_self_calls_expr(callee, out);
                for a in args {
                    Self::collect_self_calls_expr(a, out);
                }
                for (_, v) in kwargs {
                    Self::collect_self_calls_expr(v, out);
                }
            }
            Expr::Attr { obj, .. } => Self::collect_self_calls_expr(obj, out),
            Expr::Index { obj, idx, .. } => {
                Self::collect_self_calls_expr(obj, out);
                Self::collect_self_calls_expr(idx, out);
            }
            Expr::Slice { obj, start, stop, step, .. } => {
                Self::collect_self_calls_expr(obj, out);
                for e in [start, stop, step].into_iter().flatten() {
                    Self::collect_self_calls_expr(e, out);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                Self::collect_self_calls_expr(lhs, out);
                Self::collect_self_calls_expr(rhs, out);
            }
            Expr::UnOp { expr: e, .. } => Self::collect_self_calls_expr(e, out),
            Expr::IfExp { test, body, orelse, .. } => {
                Self::collect_self_calls_expr(test, out);
                Self::collect_self_calls_expr(body, out);
                Self::collect_self_calls_expr(orelse, out);
            }
            Expr::List(elems, _) | Expr::Tuple(elems, _) | Expr::Set(elems, _) => {
                for e in elems {
                    Self::collect_self_calls_expr(e, out);
                }
            }
            Expr::Dict(pairs, _) => {
                for (k, v) in pairs {
                    Self::collect_self_calls_expr(k, out);
                    Self::collect_self_calls_expr(v, out);
                }
            }
            Expr::ListComp { elt, iter, cond, .. } | Expr::SetComp { elt, iter, cond, .. } => {
                Self::collect_self_calls_expr(elt, out);
                Self::collect_self_calls_expr(iter, out);
                if let Some(c) = cond {
                    Self::collect_self_calls_expr(c, out);
                }
            }
            Expr::DictComp { key, val, iter, cond, .. } => {
                Self::collect_self_calls_expr(key, out);
                Self::collect_self_calls_expr(val, out);
                Self::collect_self_calls_expr(iter, out);
                if let Some(c) = cond {
                    Self::collect_self_calls_expr(c, out);
                }
            }
            Expr::Lambda { body, .. } => Self::collect_self_calls_expr(body, out),
            _ => {}
        }
    }

    /// Pre-pass (run once from `emit_program`, before any emission): compute the
    /// transitive `&mut self` decision for every `(class, method)` and store it
    /// in `self.mut_self`. See the block comment above for the algorithm.
    fn compute_mut_self(&mut self) {
        // 1+2: seed `mutates` and build `self_calls`, keyed by (class, method),
        // over the RESOLVED method set of every class (own + inherited).
        let mut mutates: HashMap<(String, String), bool> = HashMap::new();
        let mut self_calls: HashMap<(String, String), std::collections::HashSet<String>> =
            HashMap::new();
        // `resolved[class]` = set of method names visible on the class via MRO,
        // so `resolve(class, name)` can check membership cheaply.
        let mut resolved: HashMap<String, std::collections::HashSet<String>> = HashMap::new();

        // Dunder-trait method names (these become trait impls, not inherent
        // methods, and never get a `__super_` alias — mirrors `emit_class`).
        let dunder_trait_names = DUNDER_TRAIT_NAMES;

        let class_names: Vec<String> = self.ctx.classes.keys().cloned().collect();
        for cls in &class_names {
            let methods = self.resolved_methods(cls);
            let mut names = std::collections::HashSet::new();
            for m in &methods {
                names.insert(m.name.clone());
                let key = (cls.clone(), m.name.clone());
                mutates.insert(key.clone(), self.method_modifies_self(&m.body));
                let mut calls = std::collections::HashSet::new();
                self.collect_self_calls(&m.body, &mut calls);
                self_calls.insert(key, calls);
            }

            // Seed the `__super_<name>` aliases EXACTLY as `emit_class` emits
            // them (codegen.rs ~903): one per OWN method that overrides an
            // immediate-parent method. The alias carries the PARENT's body but is
            // emitted onto THIS class's struct, so its own self-calls resolve
            // against THIS class's MRO. This lets a delegating-only override
            // (`__init__` that just calls `super().__init__()`) inherit `&mut`
            // from the mutating parent method through the fixpoint.
            if let Some(cd) = self.ctx.classes.get(cls) {
                let own_method_names: std::collections::HashSet<&str> =
                    cd.methods.iter().map(|m| m.name.as_str()).collect();
                for base in &cd.bases {
                    if let Some(base_def) = self.ctx.classes.get(base.as_str()) {
                        for m in &base_def.methods {
                            if !dunder_trait_names.contains(&m.name.as_str())
                                && own_method_names.contains(m.name.as_str())
                            {
                                let alias = format!("__super_{}", m.name);
                                names.insert(alias.clone());
                                let key = (cls.clone(), alias);
                                mutates.insert(key.clone(), self.method_modifies_self(&m.body));
                                let mut calls = std::collections::HashSet::new();
                                self.collect_self_calls(&m.body, &mut calls);
                                self_calls.insert(key, calls);
                            }
                        }
                    }
                }
            }

            resolved.insert(cls.clone(), names);
        }

        // 3: fixpoint. `mutates` is monotone (only ever flips false→true) over a
        // finite key set, so it converges; cap iterations at len+1 to defend
        // against mutual-recursion cycles (A↔B) — each pass can newly-true at
        // most one key per chain link, so len passes suffice.
        let max_iters = mutates.len() + 1;
        for _ in 0..max_iters {
            let mut changed = false;
            // Iterate over a stable key snapshot; read `mutates` for callees.
            let keys: Vec<(String, String)> = mutates.keys().cloned().collect();
            for key in &keys {
                if *mutates.get(key).unwrap_or(&false) {
                    continue; // already true — monotone, never reverts
                }
                let (cls, _method) = key;
                let mut now_true = false;
                if let Some(callees) = self_calls.get(key) {
                    for callee in callees {
                        // resolve(cls, callee): the callee is emitted onto THIS
                        // class only if it is visible via the class's MRO; key it
                        // under (cls, callee) so an inherited mutating method
                        // (also seeded under cls) propagates.
                        if resolved.get(cls).map_or(false, |s| s.contains(callee)) {
                            let ckey = (cls.clone(), callee.clone());
                            if *mutates.get(&ckey).unwrap_or(&false) {
                                now_true = true;
                                break;
                            }
                        }
                    }
                }
                if now_true {
                    mutates.insert(key.clone(), true);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        self.mut_self = mutates;
    }

    /// (EPIC-5 C2-1) Pre-pass building the closed-set polymorphism map
    /// `base -> all subclasses in the unit` (direct AND transitive). Run from
    /// `emit_program` right after `compute_mut_self`, BEFORE any emission, so the
    /// map is populated when `rust_ty` consults it. Reads only `ctx.classes`, so
    /// it is independent of module emission order.
    ///
    /// For every ordered pair of registered classes `(sub, base)` with
    /// `is_subclass(sub, base)` and `sub != base`, `sub` is registered under
    /// `base`. Reusing the audited `crate::typeck::is_subclass` (which walks
    /// `bases` edges through `ctx.classes` and terminates at builtins like
    /// `Exception`) gives transitivity for free: in a `C(B(A))` chain,
    /// `is_subclass(C, A)` holds, so `C` lands under `A` as well as under `B`.
    /// Each subclass list is sorted for deterministic, stable codegen.
    fn build_poly_map(&mut self) {
        let class_names: Vec<String> = self.ctx.classes.keys().cloned().collect();
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for sub in &class_names {
            for base in &class_names {
                if sub != base && crate::typeck::is_subclass(sub, base, self.ctx) {
                    map.entry(base.clone()).or_default().push(sub.clone());
                }
            }
        }
        for subs in map.values_mut() {
            subs.sort();
        }
        self.poly_map = map;
    }

    /// (EPIC-5 C2-1) True when `name` is a base class with at least one subclass
    /// in the compilation unit — i.e. it has a non-empty `poly_map` entry. C2-1
    /// only consults this (in `rust_ty`) without changing emitted text; C2-2 will
    /// branch on it to emit the companion-enum name `n__`.
    fn is_polymorphic_base(&self, name: &str) -> bool {
        self.poly_map.get(name).is_some_and(|subs| !subs.is_empty())
    }

    /// The `&mut self` decision for a method, consulted by `emit_func`. Uses the
    /// precomputed transitive result from `compute_mut_self` (the normal path —
    /// the pre-pass map covers every resolved class method, including the
    /// `__super_` aliases). Falls back to the intra-method `method_modifies_self`
    /// seed only for a method absent from the map (a defensive path; the
    /// `__lt_impl` helper is emitted inline and never routed through here).
    fn needs_mut_self(&self, class_name: &str, method_name: &str, body: &[Stmt]) -> bool {
        match self.mut_self.get(&(class_name.to_string(), method_name.to_string())) {
            Some(v) => *v,
            None => self.method_modifies_self(body),
        }
    }

    fn emit_func(&mut self, f: &Func, method_of: Option<&str>) -> Result<()> {
        let is_static = f.decorators.contains(&"staticmethod".to_string());
        let name = if f.name == "main" && method_of.is_none() {
            "user_main".to_string()
        } else if method_of.is_none() {
            // (EPIC-6) Free-function name: escape a keyword name so the `fn` def
            // matches every call site (call sites emit the name through
            // `emit_expr`'s Ident arm, which escapes identically). METHOD names
            // are deliberately NOT escaped here — method-name escaping is the
            // sibling dispatch card's concern, and escaping only the definition
            // would desync it from the (untouched) dispatch call sites.
            escape_ident(&f.name)
        } else {
            f.name.clone()
        };
        let mut sig = format!("fn {}(", name);
        let mut first = true;
        // Static methods don't get self; regular methods take &self or &mut self based on whether they modify self.
        if let Some(cls) = method_of {
            if !is_static && f.params.iter().any(|p| p.name == "self") {
                // (EPIC-4 V3) Use the precomputed TRANSITIVE decision: a method
                // that mutates self only by calling a mutating `self.<helper>()`
                // is now `&mut self` too. Falls back to the intra-method seed for
                // synthesized funcs not in the pre-pass map (`__super_` aliases /
                // `__lt_impl`).
                let needs_mut = self.needs_mut_self(cls, &f.name, &f.body);
                if needs_mut {
                    sig.push_str("&mut self");
                } else {
                    sig.push_str("&self");
                }
                first = false;
            }
        }
        // Always skip `self` from the explicit params list.
        for p in f.params.iter().filter(|p| p.name != "self") {
            if !first { sig.push_str(", "); }
            first = false;
            let pty = self.rust_ty(&Ty::from_type_expr(&p.ty, p.span)?);
            if p.by_ref {
                // (EPIC-4 V2-c) An opt-in by-reference param (`Mut[T]`) becomes
                // `name: &mut T`. The callee's mutations persist to the caller,
                // which threads `&mut <place>` at the call site. No `mut` binding
                // prefix: the binding itself is the reference (already `&mut`);
                // field/method mutation and `place.clone()` reads both work
                // through auto-deref.
                let _ = write!(sig, "{}: &mut {}", escape_ident(&p.name), pty);
            } else {
                // Value params are bound `mut` so functions may mutate them or
                // their fields in place (Python passes mutable objects by
                // reference); unused-mut is allowed in the generated crate.
                // (EPIC-6) Escape a keyword param name; body uses resolve to the
                // same escaped form via emit_expr's Ident arm.
                let _ = write!(sig, "mut {}: {}", escape_ident(&p.name), pty);
            }
        }
        let ret = Ty::from_type_expr(&f.ret, f.span)?;
        let ret_s = self.rust_ty(&ret);
        let _ = write!(sig, ") -> {} {{", ret_s);

        // (FFI Phase 1) An `@extern` function is a Rust-FFI binding: its body is a
        // single string-literal holding a Rust EXPRESSION TEMPLATE, not pyrst
        // statements. We reuse the signature built above (same param + return
        // types), then emit the substituted template as the function's TAIL
        // expression and SKIP all the pyrst-body machinery below (prescan /
        // hoisting / by-ref bookkeeping / body-statement loop). typeck has already
        // verified the body shape and that the signature is fully typed, so the
        // body[0] string extraction below cannot fail for a checked program.
        if f.decorators.iter().any(|d| d == "extern") {
            self.line(&sig);
            self.indent += 1;
            let template = match f.body.first() {
                Some(Stmt::Expr(Expr::Str(s, _))) => s.clone(),
                // Defensive: `build` may be invoked without a prior `check`, so
                // re-assert the body shape here rather than panic on a bad AST.
                _ => {
                    return Err(crate::diag::Error::Codegen(
                        "`@extern` function body must be a single Rust-template \
                         string literal"
                            .to_string(),
                    ))
                }
            };
            // Substitute each `{param}` hole with the param's emitted Rust
            // identifier (`escape_ident` matches how call sites would name it).
            // `self` is skipped (an @extern free function has none). Phase 1 does
            // NOT handle `{{`/`}}` literal-brace escaping — the template is a
            // direct expression and bare braces are uncommon there; documented.
            // A hole naming no param (a typo like `{nane}`) is left literal and
            // surfaces as a rustc error (E0425) at build — intentional for the FFI
            // escape hatch: the template is opaque Rust, validated by rustc.
            let mut emitted = template;
            for p in f.params.iter().filter(|p| p.name != "self") {
                let hole = format!("{{{}}}", p.name);
                emitted = emitted.replace(&hole, &escape_ident(&p.name));
            }
            // Tail expression: no trailing `;`, so the template value is returned.
            self.line(&emitted);
            self.indent -= 1;
            self.line("}");
            self.line("");
            return Ok(());
        }

        self.line(&sig);
        self.indent += 1;
        // (EPIC-5) Track the active return type for Some/None wrapping in `return`.
        let saved_ret_ty = std::mem::replace(&mut self.current_ret_ty, ret.clone());
        // Generators (EAGER v1): a function whose body contains `yield` collects
        // its yielded values into a `Vec<T>` and returns it. typeck has already
        // verified the signature is `Iterator[T]` (which lowered `ret` to
        // `Ty::List(T)` -> a `Vec<T>` Rust return), so detecting the `yield` in
        // the body is enough to switch on the desugar here. The flag is
        // saved/restored so a nested function never inherits it.
        let is_generator =
            crate::typeck::body_contains_yield(&f.body) && matches!(ret, Ty::List(_));
        let saved_in_generator = std::mem::replace(&mut self.in_generator, is_generator);
        if is_generator {
            // The accumulator the lowered `yield`s push into and the function
            // returns. Typed to the Rust return type (`Vec<T>`) so an empty
            // generator and element inference both resolve without annotation
            // churn. See the `Stmt::Yield` arm in `emit_stmt`.
            self.line(&format!("let mut __gen: {} = Vec::new();", ret_s));
        }
        // (EPIC-4 V2-c) Track this function's by-reference (`&mut T`) param
        // bindings so a forwarded-by-reference arg that names one emits an
        // explicit reborrow (`&mut *x`) instead of a double `&mut` (E0596).
        // Save+restore so a nested func/method emission can never leak its set
        // into the enclosing one (mirrors `current_ret_ty`).
        let saved_by_ref = std::mem::take(&mut self.by_ref_locals);
        for p in f.params.iter().filter(|p| p.name != "self") {
            if p.by_ref {
                self.by_ref_locals.insert(p.name.clone());
            }
        }

        // Populate locals from parameters
        // Register self with its class type if this is a method
        if let Some(cls) = method_of {
            self.locals.insert("self".to_string(), Ty::Class(cls.to_string()));
        }

        for p in &f.params {
            if p.name != "self" {
                let ty = Ty::from_type_expr(&p.ty, p.span)?;
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
                // (EPIC-6) Escape the hoisted local's emitted name; the raw name
                // stays the `declared`/`locals` key.
                self.line(&format!("let mut {}: {} = {};", escape_ident(&name), self.rust_ty(&ty), def));
                self.declared.insert(name);
            }
        }

        for s in &f.body {
            self.emit_stmt(s)?;
        }
        // Generators: fall-off-the-end returns the collected Vec. (A bare
        // `return` inside the body also lowers to `return __gen;` via emit_stmt,
        // so collection stops there; this final return covers the normal path
        // where control reaches the end of the body.)
        if is_generator {
            self.line("return __gen;");
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        // Clear locals and declared for next function
        self.locals.clear();
        self.declared.clear();
        self.current_ret_ty = saved_ret_ty;
        self.by_ref_locals = saved_by_ref;
        self.in_generator = saved_in_generator;
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
            Ty::from_type_expr(&f.ty, f.span)
                .map(|ty| self.is_copy_type(&ty))
                .unwrap_or(false)
        });

        // A user-defined __eq__ (own OR inherited) emits a manual `impl PartialEq`,
        // so don't ALSO derive it (that would be a conflicting-impl error, E0119)
        // and don't fall back to a field-wise derived eq that ignores the
        // inherited custom semantics.
        let has_eq = resolved_methods.iter().any(|m| m.name == "__eq__");
        // (EPIC-5 C2-3) A field whose type is a polymorphic base lowers to the
        // companion enum `B__`. A `#[derive(PartialEq)]` on this struct then
        // requires `B__: PartialEq`, which holds ONLY when every variant defines
        // `__eq__`. If any polymorphic-base field's enum lacks PartialEq, derive
        // without it (Python `==` on such a struct is then honestly unavailable
        // — consistent with cross-variant equality being absent on the enum).
        let field_blocks_eq = all_fields.iter().any(|f| {
            matches!(Ty::from_type_expr(&f.ty, f.span), Ok(Ty::Class(ref n))
                if self.is_polymorphic_base(n) && !self.companion_enum_has_partial_eq(n))
        });
        let pe = if has_eq || field_blocks_eq { "" } else { ", PartialEq" };
        // Only derive Default when every field actually implements Default.
        // Copy classes (all-primitive fields) don't derive Default, so an outer
        // struct holding one must NOT include Default in its own derive list.
        let all_fields_default = all_fields.iter().all(|f| {
            Ty::from_type_expr(&f.ty, f.span)
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
            let ty = Ty::from_type_expr(&f.ty, f.span)?;
            // (EPIC-6) Escape a keyword field name in the struct definition; every
            // field read/write/init escapes the same way so they stay in sync.
            self.line(&format!("{}: {},", escape_ident(&f.name), self.rust_ty(&ty)));
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        self.current_class = Some(c.name.clone());

        // Dunder methods that become Rust trait impls instead of regular methods.
        let dunder_trait_names = DUNDER_TRAIT_NAMES;

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
                            let ty = Ty::from_type_expr(&p.ty, p.span)?;
                            // (EPIC-6) new()'s params + their forwarded uses below
                            // escape identically.
                            Ok(format!("{}: {}", escape_ident(&p.name), self.rust_ty(&ty)))
                        })
                        .collect();
                    let param_strs = param_strs?;
                    let param_names: Vec<_> = non_self.iter().map(|p| escape_ident(&p.name)).collect();
                    let defaults: Vec<String> = all_fields.iter().map(|f| {
                        let ty = Ty::from_type_expr(&f.ty, f.span).unwrap_or(Ty::Unknown);
                        // Use zeroed_default which handles Copy classes that don't
                        // implement Default (unlike a plain Default::default() call).
                        let dv = self.zeroed_default(&ty);
                        // (EPIC-6) Escape a keyword field name in the struct-literal
                        // initializer (matches the escaped struct field def).
                        format!("{}: {}", escape_ident(&f.name), dv)
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
                        let ty = Ty::from_type_expr(&f.ty, f.span)?;
                        // (EPIC-6) Dataclass ctor: param name == field name; both
                        // the param binding here and the field-init shorthand below
                        // escape, so `ClassName { r#type, .. }` stays consistent.
                        Ok(format!("{}: {}", escape_ident(&f.name), self.rust_ty(&ty)))
                    })
                    .collect();
                let param_strs = param_strs?;
                let field_inits: Vec<_> = all_fields.iter().map(|f| escape_ident(&f.name)).collect();
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
                        // `other: &c.name` is the CONCRETE struct here — exempt it
                        // from base-field-read lowering (C2-2b-i).
                        self.concrete_struct_params.insert("other".into());
                        for s in &m.body { self.emit_stmt(s)?; }
                        self.concrete_struct_params.remove("other");
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

        // Both __str__ and __repr__ lower to the SAME Rust trait (Display), so a
        // class that defines/inherits BOTH would otherwise emit two `impl Display`
        // → rustc E0119 (conflicting impl). Dedup by the Rust TRAIT, not the Python
        // method name: pick a single Display source, PREFERRING __str__ (Python uses
        // __str__ for str()/print — the user-facing one), else __repr__. The chosen
        // name is matched against the resolved set so the dedup also holds across
        // inheritance (e.g. inherited __str__ + local __repr__).
        let display_source: Option<&str> = if c_methods.iter().any(|m| m.name == "__str__") {
            Some("__str__")
        } else if c_methods.iter().any(|m| m.name == "__repr__") {
            Some("__repr__")
        } else {
            None
        };

        for m in &c_methods {
            match m.name.as_str() {
                "__str__" | "__repr__" if Some(m.name.as_str()) == display_source => {
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
                        .map(|p| Ty::from_type_expr(&p.ty, p.span).unwrap_or(Ty::Class(c.name.clone())))
                        .unwrap_or(Ty::Class(c.name.clone()));
                    let ret_ty = Ty::from_type_expr(&m.ret, m.span).unwrap_or(Ty::Class(c.name.clone()));
                    self.line(&format!("impl ::std::ops::Add<{}> for {} {{", self.rust_ty(&other_ty), c.name));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", self.rust_ty(&ret_ty)));
                    self.line(&format!("fn add(self, other: {}) -> {} {{", self.rust_ty(&other_ty), self.rust_ty(&ret_ty)));
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
                    // `other: &c.name` is the CONCRETE struct here — exempt it
                    // from base-field-read lowering (C2-2b-i).
                    self.concrete_struct_params.insert("other".into());
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.concrete_struct_params.remove("other");
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
                        .map(|p| Ty::from_type_expr(&p.ty, p.span).unwrap_or(Ty::Class(c.name.clone())))
                        .unwrap_or(Ty::Class(c.name.clone()));
                    let ret_ty = Ty::from_type_expr(&m.ret, m.span).unwrap_or(Ty::Class(c.name.clone()));
                    self.line(&format!("impl ::std::ops::Sub<{}> for {} {{", self.rust_ty(&other_ty), c.name));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", self.rust_ty(&ret_ty)));
                    self.line(&format!("fn sub(self, other: {}) -> {} {{", self.rust_ty(&other_ty), self.rust_ty(&ret_ty)));
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
                        .map(|p| Ty::from_type_expr(&p.ty, p.span).unwrap_or(Ty::Class(c.name.clone())))
                        .unwrap_or(Ty::Class(c.name.clone()));
                    let ret_ty = Ty::from_type_expr(&m.ret, m.span).unwrap_or(Ty::Class(c.name.clone()));
                    self.line(&format!("impl ::std::ops::Mul<{}> for {} {{", self.rust_ty(&other_ty), c.name));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", self.rust_ty(&ret_ty)));
                    self.line(&format!("fn mul(self, other: {}) -> {} {{", self.rust_ty(&other_ty), self.rust_ty(&ret_ty)));
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
                    let ret_ty = Ty::from_type_expr(&m.ret, m.span).unwrap_or(Ty::Class(c.name.clone()));
                    self.line(&format!("impl ::std::ops::Neg for {} {{", c.name));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", self.rust_ty(&ret_ty)));
                    self.line(&format!("fn neg(self) -> {} {{", self.rust_ty(&ret_ty)));
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

    /// (EPIC-5 C2-2a) Emit the closed-set companion enum + method-dispatch impl +
    /// field-accessor impl for every polymorphic base, as DEAD CODE that must
    /// compile. C2-2a leaves all of this UNUSED — `rust_ty` still emits plain `n`,
    /// constructors are not wrapped, and the C1 honest gate stays — so output is
    /// byte-for-byte identical and the golden suite staying green proves the
    /// emitted Rust compiles. C2-2b activates it (flips `rust_ty` to `n__`, wraps
    /// constructors, removes the gate, lowers field access to the accessors).
    ///
    /// Run from `emit_program` AFTER the top-level statement loop, so every
    /// variant's value-struct (emitted by `emit_class`) already exists — "after
    /// the structs". Bases are visited in sorted order for deterministic codegen.
    fn emit_companion_enums(&mut self) -> Result<()> {
        let mut bases: Vec<String> = self
            .poly_map
            .iter()
            .filter(|(_, subs)| !subs.is_empty())
            .map(|(b, _)| b.clone())
            .collect();
        bases.sort();
        for base in &bases {
            self.emit_companion_enum(base)?;
        }
        Ok(())
    }

    /// Emit the companion enum + dispatch + field accessors for ONE polymorphic
    /// base `base` (guaranteed to have a non-empty `poly_map` entry). All three
    /// items are `#[allow(dead_code)]` so the 0-warning gate does not trip on the
    /// as-yet-unused machinery.
    fn emit_companion_enum(&mut self, base: &str) -> Result<()> {
        // Variant set: the base itself, then every TRANSITIVE subclass (poly_map
        // is transitive and already sorted). Each variant's payload is the BARE
        // concrete value-struct — NOT `rust_ty` (which would later become `n__`)
        // and NOT a companion-enum name; intermediate bases therefore appear as
        // raw-struct variants too (e.g. `Base__ { Base(Base), Leaf(Leaf),
        // Mid(Mid) }`), so both `Base__` and `Mid__` are independent flat enums.
        let mut variants: Vec<String> = vec![base.to_string()];
        if let Some(subs) = self.poly_map.get(base) {
            variants.extend(subs.iter().cloned());
        }
        let enum_name = format!("{}__", base);

        // 1. The enum. Always exactly `#[derive(Clone, Debug)]`: a data-variant
        // enum cannot derive Default or Copy, and Display/PartialEq for the enum
        // are deferred (C2-2b+). Do NOT reuse emit_class's all_fields_copy derive
        // logic (design §F).
        self.line("#[allow(dead_code)]");
        self.line("#[derive(Clone, Debug)]");
        self.line(&format!("enum {} {{", enum_name));
        self.indent += 1;
        for v in &variants {
            self.line(&format!("{}({}),", v, v));
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        // 2. Method-dispatch impl. Dunder methods become Rust TRAIT impls (Display
        // / PartialEq / PartialOrd / Add / ...), not inherent methods, so a
        // dispatch `match self { _ => x.__str__() }` would not compile — skip them
        // (this is the inherit_dunders crux). Also skip `__init__` (a constructor,
        // not dispatched) and any `@staticmethod` (no `self` receiver). The list
        // mirrors emit_class's `dunder_trait_names` exactly.
        let dunder_trait_names = DUNDER_TRAIT_NAMES;
        let resolved = self.resolved_methods(base);
        let dispatchable: Vec<Func> = resolved
            .into_iter()
            .filter(|m| {
                m.name != "__init__"
                    && !m.decorators.contains(&"staticmethod".to_string())
                    && !dunder_trait_names.contains(&m.name.as_str())
                    && m.params.iter().any(|p| p.name == "self")
            })
            .collect();

        self.line("#[allow(dead_code)]");
        self.line(&format!("impl {} {{", enum_name));
        self.indent += 1;
        for m in &dispatchable {
            self.emit_dispatch_method(base, &enum_name, &variants, m)?;
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        // 3. Field-accessor impl: one `__field_<f>` per base field (every variant
        // inherits the base's fields, so the field exists on every variant
        // struct). Non-Copy fields clone (value semantics); Copy fields read bare.
        // Dedup by field NAME (mirrors emit_class's all_fields walk): get_all_fields
        // can yield the same name more than once across a base chain, which would
        // otherwise emit two identical `__field_<f>` methods (rustc E0592).
        let mut accessor_fields: Vec<Param> = Vec::new();
        for f in self.ctx.get_all_fields(base) {
            if !accessor_fields.iter().any(|ef: &Param| ef.name == f.name) {
                accessor_fields.push(f);
            }
        }
        self.line("#[allow(dead_code)]");
        self.line(&format!("impl {} {{", enum_name));
        self.indent += 1;
        for f in &accessor_fields {
            let fty = Ty::from_type_expr(&f.ty, f.span)?;
            let ret = self.rust_ty(&fty);
            // (EPIC-6) `x.<field>` reads the inner value struct's field, so a
            // keyword field name is escaped to match the (escaped) struct def. The
            // accessor METHOD itself is `__field_<name>` (prefix → never a
            // keyword), so its name is left unescaped at both def and call site.
            let read = if crate::typeck::is_copy(&fty) {
                format!("x.{}", escape_ident(&f.name))
            } else {
                format!("x.{}.clone()", escape_ident(&f.name))
            };
            let arms: Vec<String> = variants
                .iter()
                .map(|v| format!("{}::{}(x) => {}", enum_name, v, read))
                .collect();
            self.line(&format!(
                "fn __field_{}(&self) -> {} {{ match self {{ {} }} }}",
                f.name,
                ret,
                arms.join(", ")
            ));
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        // 4. Dunder-trait FORWARDING impls (EPIC-5 C2-2a2). A polymorphic-base var
        // becomes `B__` after the d5a4ff03 flip, and inherit_dunders shows base
        // vars used with `print(m)` (Display), `==` (PartialEq), and `<`
        // (PartialOrd). The companion enum must carry those traits BEFORE the flip
        // — emitted here as DEAD CODE that must COMPILE (B__ is unused until the
        // keystone). Emit a trait impl for B__ ONLY when EVERY variant struct
        // already has that trait, determined by the SAME predicate emit_class uses
        // to decide whether to emit `impl <Trait> for <variant struct>` (off the
        // variant's RESOLVED method set): Display ⇐ __str__|__repr__
        // (display_source, codegen.rs ~1224), PartialEq ⇐ __eq__ (has_eq, ~1056),
        // PartialOrd ⇐ __lt__ (has_lt, ~1096). If ANY variant lacks the trait, the
        // forward (`write!("{}", x)`, `a == b`, `a.partial_cmp(b)`) would not
        // resolve, so emit NO impl of that trait for B__. Cross-variant comparison
        // is Python-honest: `==` is false, ordering is None.
        let variant_resolved: Vec<Vec<Func>> =
            variants.iter().map(|v| self.resolved_methods(v)).collect();
        let all_variants_have = |pred: &dyn Fn(&Func) -> bool| -> bool {
            variant_resolved.iter().all(|ms| ms.iter().any(|m| pred(m)))
        };
        let all_have_display =
            all_variants_have(&|m| m.name == "__str__" || m.name == "__repr__");
        let all_have_eq = all_variants_have(&|m| m.name == "__eq__");
        let all_have_lt = all_variants_have(&|m| m.name == "__lt__");

        if all_have_display {
            let arms: Vec<String> = variants
                .iter()
                .map(|v| format!("{}::{}(x) => write!(__f, \"{{}}\", x)", enum_name, v))
                .collect();
            self.line("#[allow(dead_code)]");
            self.line(&format!("impl ::std::fmt::Display for {} {{", enum_name));
            self.indent += 1;
            self.line(
                "fn fmt(&self, __f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {",
            );
            self.indent += 1;
            self.line(&format!("match self {{ {} }}", arms.join(", ")));
            self.indent -= 1;
            self.line("}");
            self.indent -= 1;
            self.line("}");
            self.line("");
        }

        if all_have_eq {
            let mut arms: Vec<String> = variants
                .iter()
                .map(|v| format!("({0}::{1}(a), {0}::{1}(b)) => a == b", enum_name, v))
                .collect();
            // Cross-variant: Python `Dog(..) == Cat(..)` is False.
            arms.push("_ => false".to_string());
            self.line("#[allow(dead_code)]");
            self.line(&format!("impl ::std::cmp::PartialEq for {} {{", enum_name));
            self.indent += 1;
            self.line(&format!("fn eq(&self, other: &{}) -> bool {{", enum_name));
            self.indent += 1;
            self.line(&format!("match (self, other) {{ {} }}", arms.join(", ")));
            self.indent -= 1;
            self.line("}");
            self.indent -= 1;
            self.line("}");
            self.line("");
        }

        // Rust's `PartialOrd: PartialEq` supertrait bound: the enum can only impl
        // PartialOrd if it ALSO impls PartialEq. emit_class satisfies this on each
        // variant struct by DERIVING PartialEq when `__eq__` is absent, but the
        // companion enum derives only Clone+Debug and never PartialEq, so require
        // all_have_eq here too (true for inherit_dunders: every variant has both).
        if all_have_lt && all_have_eq {
            let mut arms: Vec<String> = variants
                .iter()
                .map(|v| {
                    format!("({0}::{1}(a), {0}::{1}(b)) => a.partial_cmp(b)", enum_name, v)
                })
                .collect();
            // Cross-variant: two different concrete types are uncomparable → None.
            arms.push("_ => None".to_string());
            self.line("#[allow(dead_code)]");
            self.line(&format!("impl ::std::cmp::PartialOrd for {} {{", enum_name));
            self.indent += 1;
            self.line(&format!(
                "fn partial_cmp(&self, other: &{}) -> Option<::std::cmp::Ordering> {{",
                enum_name
            ));
            self.indent += 1;
            self.line(&format!("match (self, other) {{ {} }}", arms.join(", ")));
            self.indent -= 1;
            self.line("}");
            self.indent -= 1;
            self.line("}");
            self.line("");
        }

        Ok(())
    }

    /// Emit a single dispatch method on the companion enum for resolved base
    /// method `m`. The signature mirrors `m`: each non-`self` param via `rust_ty`
    /// (a `Mut[T]` by-ref param becomes `&mut T`), the return via `rust_ty`. The
    /// receiver is `&mut self` when `m` needs `&mut self` on the base OR on ANY
    /// variant (the per-variant V3 query, design §F): if any variant's concrete
    /// `m` is `&mut self`, binding `x` as `&mut` is required for `x.m()` to
    /// compile. The body forwards each param by name to the variant's inherent
    /// `m` (every variant struct has it — inherited or overridden).
    fn emit_dispatch_method(
        &mut self,
        base: &str,
        enum_name: &str,
        variants: &[String],
        m: &Func,
    ) -> Result<()> {
        // Per-variant `&mut self`: needs_mut_self for the base's `m`, OR for the
        // SAME-named method resolved on any variant (a variant may override `m`
        // as `&mut self` even when the base is `&self`). Bodies come from each
        // variant's resolved method set so the V3 decision is queried against the
        // exact body that variant emits.
        let mut needs_mut = self.needs_mut_self(base, &m.name, &m.body);
        if !needs_mut {
            for v in variants {
                if let Some(vm) = self.resolved_methods(v).into_iter().find(|x| x.name == m.name) {
                    if self.needs_mut_self(v, &m.name, &vm.body) {
                        needs_mut = true;
                        break;
                    }
                }
            }
        }
        let receiver = if needs_mut { "&mut self" } else { "&self" };

        // Non-self params + their forwarded names. A by-ref (`Mut[T]`) param
        // renders `name: &mut T` and forwards the bare binding (already a `&mut`).
        let non_self: Vec<&Param> = m.params.iter().filter(|p| p.name != "self").collect();
        let mut sig_params: Vec<String> = Vec::new();
        let mut fwd: Vec<String> = Vec::new();
        for p in &non_self {
            let pty = self.rust_ty(&Ty::from_type_expr(&p.ty, p.span)?);
            // (EPIC-6) Dispatch-wrapper params + their forwarding both escape so a
            // keyword-named param stays consistent. (The method NAME `m.name` is
            // left unescaped — method-name escaping is the sibling dispatch card.)
            if p.by_ref {
                sig_params.push(format!("{}: &mut {}", escape_ident(&p.name), pty));
            } else {
                sig_params.push(format!("{}: {}", escape_ident(&p.name), pty));
            }
            fwd.push(escape_ident(&p.name));
        }
        let ret = self.rust_ty(&Ty::from_type_expr(&m.ret, m.span)?);
        let fwd_args = fwd.join(", ");
        let arms: Vec<String> = variants
            .iter()
            .map(|v| format!("{}::{}(x) => x.{}({})", enum_name, v, m.name, fwd_args))
            .collect();

        let mut sig = format!("fn {}({}", m.name, receiver);
        for sp in &sig_params {
            sig.push_str(", ");
            sig.push_str(sp);
        }
        let _ = write!(sig, ") -> {} {{ match self {{ {} }} }}", ret, arms.join(", "));
        self.line(&sig);
        Ok(())
    }

    /// True when `obj.name` resolves to a `@property` getter (an owned temp
    /// produced by a method call `obj.name()`), rather than a plain field read.
    /// Shared by the `emit_expr` Attr arm (which appends `()`) and
    /// `emit_consuming` (which must NOT clone a property — its result is already
    /// an owned temporary, not a borrowable place). Mirrors the inheritance-aware
    /// method lookup used elsewhere by resolving through the class's methods.
    fn is_property_access(&self, obj: &Expr, name: &str) -> bool {
        if let Expr::Ident(var, _) = obj {
            self.locals.get(var.as_str()).cloned()
                .and_then(|ty| if let Ty::Class(cn) = ty {
                    self.ctx.classes.get(&cn).map(|cd|
                        cd.methods.iter().any(|m|
                            m.name.as_str() == name
                            && m.decorators.contains(&"property".to_string())
                        )
                    )
                } else { None })
                .unwrap_or(false)
        } else {
            false
        }
    }

    /// The SINGLE ownership-decision point for value semantics (EPIC-4 V1).
    ///
    /// Emit `e` in a position that takes ownership of the value (constructor /
    /// method / function argument, container store, `return`, assignment RHS,
    /// ternary arm, `match` scrutinee). A non-`Copy` *place* — a reusable binding
    /// the program may read again — is deep-`.clone()`d so the original stays
    /// usable, reproducing Python value semantics without aliasing. A deep clone
    /// never changes observable behavior here, so uniform clone-on-use is always
    /// correctness-safe (last-use move-elision is a deferred optimization).
    ///
    /// Decision (the only place this judgement lives):
    /// - `Copy` type (per the shared `crate::typeck::is_copy`) -> bare, no clone.
    /// - `Expr::Ident` -> `<n>.clone()` (a reusable variable place).
    /// - `Expr::Attr` FIELD read -> `<obj>.<field>.clone()` (a reusable field
    ///   place; this is the new capability that fixes `Wrapper(self.items)`
    ///   E0382). A `@property` access is an owned temp (a getter call), so it is
    ///   emitted bare — cloning it would be a redundant temp-clone.
    /// - `Expr::Index` -> `emit_expr(e)` UNCHANGED. Index reads already self-clone
    ///   (tuple `.clone()`, dict `.cloned()`, list `__list[i].clone()`); appending
    ///   another `.clone()` would be a double-clone bug.
    /// - `Expr::IfExp` -> the ARMS are the consuming leaves; recurse so the arm
    ///   *places* clone, not the whole owned if-temp.
    /// - everything else (Call/constructor result, BinOp, literal, comprehension,
    ///   slice, …) -> a fresh owned rvalue temp -> bare, nothing to clone.
    fn emit_consuming(&mut self, e: &Expr) -> Result<String> {
        if self.is_copy_type(&self.type_of_expr(e)) {
            return self.emit_expr(e);
        }
        match e {
            Expr::Ident(..) => Ok(format!("{}.clone()", self.emit_expr(e)?)),
            Expr::Attr { obj, name, .. } => {
                if self.is_property_access(obj, name) {
                    // Owned temp from a getter call — already owned, do not clone.
                    self.emit_expr(e)
                } else {
                    Ok(format!("{}.clone()", self.emit_expr(e)?))
                }
            }
            // Index reads already self-clone — pass through to avoid double-clone.
            Expr::Index { .. } => self.emit_expr(e),
            // Clone the arm PLACES, not the whole owned if-temp.
            Expr::IfExp { test, body, orelse, .. } => {
                let t = self.emit_expr(test)?;
                let b = self.emit_consuming(body)?;
                let o = self.emit_consuming(orelse)?;
                Ok(format!("(if {} {{ {} }} else {{ {} }})", t, b, o))
            }
            // Owned rvalue temp (call/ctor/literal/binop/slice/...) — nothing to clone.
            _ => self.emit_expr(e),
        }
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
            // (EPIC-6) Escape a keyword-named place so it matches its `let`/param
            // definition (which is also escaped).
            Expr::Ident(n, _) => Ok(escape_ident(n)),
            // Field access: the base recursively as a place, then `.field`.
            // (No @property handling: a property getter is not an lvalue.)
            Expr::Attr { obj, name, .. } => {
                let base = self.emit_place(obj)?;
                Ok(format!("{}.{}", base, escape_ident(name)))
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

    /// (EPIC-4 V2-c) Borrow form for an argument flowing into a by-reference
    /// (`Mut[T]` -> `&mut T`) callee parameter. `place` is the already-emitted
    /// `emit_place(a)` text.
    ///
    /// The normal form is `&mut <place>` (a fresh mutable borrow of the caller's
    /// storage). The ONE exception is when `a` is a bare name that is itself a
    /// `&mut T` param binding of the current function (a forwarded-by-reference
    /// arg, e.g. a recursive `fill(visited, ..)` where `visited: &mut HashSet`):
    /// `&mut visited` would re-borrow an already-`&mut` binding and rustc rejects
    /// it (E0596). Emit an explicit reborrow `&mut *visited` instead — valid in
    /// every position including last-use/recursive, so we prefer it over relying
    /// on a bare auto-reborrow.
    ///
    /// Only the BARE-ident case needs this. A field/index of a by-reference param
    /// (`param.field`, `param[k]`) is not an `Expr::Ident`, so `&mut param.field`
    /// stays — that already auto-derefs through the `&mut`. An owned local
    /// (`&mut my_account`) and a self place (`&mut self.field`) are not in
    /// `by_ref_locals`, so they are unchanged.
    fn byref_borrow(&self, a: &Expr, place: &str) -> String {
        if let Expr::Ident(n, _) = a {
            if self.by_ref_locals.contains(n) {
                return format!("&mut *{}", place);
            }
        }
        format!("&mut {}", place)
    }

    /// Emit a list/set element, promoting int-typed elements to `f64` when the
    /// collection's unified element type is `Float` (`widen == true`). Reuses
    /// the same `as f64` cast convention as the assignment int->float coercion
    /// (see `Stmt::Assign` emission) so `[1, 2.0]` becomes a homogeneous
    /// `Vec<f64>` instead of the rustc-rejected `vec![(1i64), (2.0f64)]`
    /// (card 5c2f31d8). Float (and non-int) elements are emitted unchanged.
    fn emit_collection_elem(&mut self, e: &Expr, widen: bool) -> Result<String> {
        let s = self.emit_consuming(e)?;
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

    /// (EPIC-5 C2-2b-i, Step 3) If a list literal's elements share a common
    /// POLYMORPHIC-base class, return that base name (the slot is `Vec<B__>` and
    /// each element must be wrapped). Returns `None` when the elements are not all
    /// classes, or their common ancestor is a leaf / non-polymorphic class (the
    /// ordinary homogeneous-vec path applies). Mirrors typeck's
    /// `nearest_common_ancestor` fold over `unify_branch_types`, so codegen and
    /// typeck agree on the element type of a heterogeneous-subclass literal.
    fn list_poly_base(&self, elems: &[Expr]) -> Option<String> {
        if elems.is_empty() { return None; }
        let mut acc = match self.type_of_expr(&elems[0]) {
            Ty::Class(n) => n,
            _ => return None,
        };
        for e in &elems[1..] {
            let cn = match self.type_of_expr(e) {
                Ty::Class(n) => n,
                _ => return None,
            };
            acc = if crate::typeck::is_subclass(&cn, &acc, self.ctx) {
                acc // acc is already an ancestor of cn
            } else if crate::typeck::is_subclass(&acc, &cn, self.ctx) {
                cn // cn is the wider (base) of the two
            } else {
                crate::typeck::nearest_common_ancestor(&acc, &cn, self.ctx)?
            };
        }
        if self.is_polymorphic_base(&acc) { Some(acc) } else { None }
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
                Stmt::Assign { target, ty: Some(te), span, .. } => {
                    if let Ok(t) = Ty::from_type_expr(te, *span) {
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
                    // SCOPED to the loop body via save/restore: registering it
                    // function-wide would mask a MODULE CONST of the same name at
                    // statements OUTSIDE the loop (the loop var does not leak into
                    // Rust scope anyway), making `print(i)` next to a const `i`
                    // resolve to a (non-existent) local instead of the const.
                    if targets.len() == 1 {
                        let elem = match self.type_of_expr(iter) {
                            Ty::List(inner) | Ty::Set(inner) => *inner,
                            // Iterating a dict yields its KEYS (Python semantics).
                            Ty::Dict(key, _) => *key,
                            Ty::Str => Ty::Str,
                            _ => Ty::Int, // range / unknown iterables yield ints
                        };
                        let saved = self.locals.get(&targets[0]).cloned();
                        self.locals.entry(targets[0].clone()).or_insert(elem);
                        self.prescan_types(body);
                        match saved {
                            Some(ty) => { self.locals.insert(targets[0].clone(), ty); }
                            None => { self.locals.remove(targets[0].as_str()); }
                        }
                    } else {
                        self.prescan_types(body);
                    }
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
                        // SCOPED to the handler body (save/restore): the binding
                        // does not leak past the handler in pyrst, and a function-
                        // wide registration would mask a same-named MODULE CONST at
                        // statements outside the handler (e.g. a const `e` read
                        // before/after an `except ... as e`).
                        let saved = if let Some(name) = &h.exc_name {
                            let prev = self.locals.get(name).cloned();
                            self.locals.insert(name.clone(), Ty::Str);
                            Some((name.clone(), prev))
                        } else {
                            None
                        };
                        self.prescan_types(&h.body);
                        if let Some((name, prev)) = saved {
                            match prev {
                                Some(ty) => { self.locals.insert(name, ty); }
                                None => { self.locals.remove(name.as_str()); }
                            }
                        }
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
                            // Delimit type from message with a NUL byte: it cannot
                            // appear in pyrst user data, so a user message that itself
                            // contains the old " panic: " separator no longer mangles
                            // the type dispatch or the bound `as e` text. See the
                            // try/except dispatcher split for the consuming side.
                            self.line(&format!("panic!(\"{{}}\\0{{}}\", \"{}\", {});", exc_type, msg));
                        } else {
                            // No message: still use the "<Type>\0<msg>" payload format
                            // (empty message) so `except <Type>:` type-matching parses it.
                            self.line(&format!("panic!(\"{{}}\\0\", \"{}\");", exc_type));
                        }
                    }
                    Some(other) => {
                        let e = self.emit_expr(other)?;
                        self.line(&format!("panic!(\"{{}}\", {});", e));
                    }
                }
            }
            Stmt::Return(None, _) => {
                // In a generator a bare `return` stops collection and hands back
                // the values gathered so far. Elsewhere it is a plain `return;`.
                if self.in_generator {
                    self.line("return __gen;");
                } else {
                    self.line("return;");
                }
            }
            Stmt::Yield(e, _) => {
                // EAGER generator desugar: `yield x` -> push x onto the collected
                // Vec. `emit_consuming` deep-clones a non-Copy place so the pushed
                // value is independent of the binding (pyrst value semantics), and
                // a Copy element (int/bool/float) is pushed by value. The
                // accumulator `__gen` and the trailing `return __gen;` are emitted
                // by `emit_func` for any function whose body contains a `yield`.
                let s = self.emit_consuming(e)?;
                self.line(&format!("__gen.push({});", s));
            }
            Stmt::Return(Some(e), _) => {
                // (EPIC-5) In an Option-returning function, wrap the value:
                // `None` -> `return None;`, a bare T -> `return Some(T);`, an
                // already-Optional value -> pass through.
                if matches!(self.current_ret_ty, Ty::Option(_)) {
                    // emit_consuming clones a non-Copy place (e.g. `return self.field`)
                    // before coerce_to_option wraps the result in `Some(..)`.
                    let s = self.emit_consuming(e)?;
                    let wrapped = self.coerce_to_option(s, e, &self.current_ret_ty);
                    self.line(&format!("return {};", wrapped));
                } else if matches!(e, Expr::None_(_)) {
                    self.line("return;");
                } else {
                    // (EPIC-5 C2-2b-i) `return dog` from a `-> Animal` function —
                    // a raw-struct value into a polymorphic-base `Animal__` return
                    // slot is WRAPPED in the right variant (replaces the C1 gate).
                    // (first-class functions) `return lambda x: x + n` /
                    // `return inc` from a `-> Callable[..]` function — wrap the
                    // lambda/name into the `Rc<dyn Fn>` return slot. Non-poly,
                    // non-func returns keep the uniform clone-on-use path: a
                    // non-Copy place (variable, field, index) is deep-cloned so the
                    // returned value is independent of the binding.
                    let s = if matches!(self.current_ret_ty, Ty::Func(..)) {
                        let ret_ty = self.current_ret_ty.clone();
                        self.emit_into_func_slot(e, &ret_ty)?
                    } else if self.ty_has_poly_base(&self.current_ret_ty) {
                        let ret_ty = self.current_ret_ty.clone();
                        self.emit_into_base_slot(e, &ret_ty)?
                    } else {
                        self.emit_consuming(e)?
                    };
                    self.line(&format!("return {};", s));
                }
            }
            Stmt::Expr(e) => {
                let s = self.emit_expr(e)?;
                self.line(&format!("{};", s));
            }
            Stmt::Assign { target, ty, value, span, .. } => {
                // Uniform clone-on-use: assigning from a non-Copy place (`y = x`,
                // `y = self.field`) deep-clones so the two bindings are independent
                // (Python value semantics). Owned temps (call/literal/binop) are bare.
                // (EPIC-5 C2-3 cleanup) `v` is computed lazily per branch: the
                // annotated poly-base path emits via `emit_into_base_slot` directly
                // (which recomputes the clone-on-use emission internally), so the
                // earlier unconditional `emit_consuming(value)` here was redundant
                // work it then discarded. The non-poly annotated path, the inferred
                // path, and the rebind path each compute the clone-on-use `v` once.
                let is_declared = self.declared.contains(target);
                if !is_declared {
                    self.declared.insert(target.clone());
                    match ty {
                        Some(t) => {
                            let ty_obj = Ty::from_type_expr(t, *span)?;
                            // (EPIC-5 C2-2b-i) `a: Animal = Account(...)` — a raw
                            // struct into a polymorphic-base `Animal__` slot is
                            // WRAPPED in the right variant (replaces the C1 gate).
                            // (first-class functions) `g: Callable[..] = inc` /
                            // `= lambda ...` — wrap a function NAME or lambda into
                            // the `Rc<dyn Fn>` slot. Non-poly, non-func slots keep
                            // the clone-on-use emission.
                            let v = if self.ty_has_func(&ty_obj) {
                                self.emit_into_func_slot(value, &ty_obj)?
                            } else if self.ty_has_poly_base(&ty_obj) {
                                self.emit_into_base_slot(value, &ty_obj)?
                            } else {
                                self.emit_consuming(value)?
                            };
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
                            // (EPIC-6) Escape the emitted binding name; the raw
                            // `target` stays the `declared`/`locals` key.
                            let target_e = escape_ident(target);
                            if matches!(ty_obj, Ty::Float)
                                && (matches!(value_ty, Ty::Int) || self.emits_int_pow(value))
                            {
                                self.line(&format!("let mut {}: {} = {} as f64;", target_e, self.rust_ty(&ty_obj), v));
                            } else {
                                self.line(&format!("let mut {}: {} = {};", target_e, self.rust_ty(&ty_obj), v));
                            }
                        }
                        None => {
                            let v = self.emit_consuming(value)?;
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
                            // (EPIC-6) Escape the emitted binding name.
                            let target_e = escape_ident(target);
                            if matches!(decl_ty, Ty::Float)
                                && (matches!(value_ty, Ty::Int) || self.emits_int_pow(value))
                            {
                                self.line(&format!("let mut {}: f64 = {} as f64;", target_e, v));
                            } else {
                                self.line(&format!("let mut {} = {};", target_e, v));
                            }
                        }
                    }
                } else {
                    let cur = self.locals.get(target).cloned().unwrap_or(Ty::Unknown);
                    // (first-class functions) Reassigning a Callable-typed (or
                    // func-containing collection-typed) local: a bare function NAME
                    // / lambda / func-name-bearing literal on the RHS must be
                    // wrapped into the `Rc<dyn Fn>` slot, exactly as in the
                    // declaration branch. Without this, `f = double` would emit
                    // `f = double.clone();` (a fn item has no `.clone() -> Rc<dyn
                    // Fn>`) -> rustc E0308. An `IfExp` RHS (`f = inc if c else
                    // double`) is handled by `emit_into_func_slot` recursing into
                    // its arms via the IfExp case it shares with `emit_consuming`.
                    let v = if self.ty_has_func(&cur) {
                        self.emit_into_func_slot(value, &cur)?
                    } else {
                        self.emit_consuming(value)?
                    };
                    // Python permits rebinding a name to a value of a different
                    // type. When that happens, emit a shadowing `let` (which
                    // always type-checks) instead of a plain reassignment.
                    let value_ty = self.type_of_expr(value);
                    // (EPIC-6) Escape the emitted name (raw `target` stays map key).
                    let target_e = escape_ident(target);
                    if Self::types_conflict(&cur, &value_ty) {
                        self.locals.insert(target.clone(), value_ty);
                        self.line(&format!("let mut {} = {};", target_e, v));
                    } else if matches!(cur, Ty::Float)
                        && (matches!(value_ty, Ty::Int) || self.emits_int_pow(value))
                    {
                        // Reassigning an int into a float-typed (e.g. hoisted) var.
                        self.line(&format!("{} = {} as f64;", target_e, v));
                    } else {
                        self.line(&format!("{} = {};", target_e, v));
                    }
                }
            }
            Stmt::Unpack { targets, value, .. } => {
                let v = self.emit_expr(value)?;
                // (EPIC-6) Escape each unpack target name; body uses resolve to the
                // same escaped form via emit_expr's Ident arm.
                let targets_e: Vec<String> = targets.iter().map(|t| escape_ident(t)).collect();
                self.line(&format!("let ({}) = {};", targets_e.join(", "), v));
            }
            Stmt::AugAssign { target, op, value, .. } => {
                let v = self.emit_expr(value)?;
                let target_ty = self.locals.get(target.as_str()).cloned().unwrap_or(Ty::Unknown);
                // (EPIC-6) `target` names an existing local (emitted escaped by its
                // `let`), so every occurrence here — store target AND read — uses
                // the escaped form.
                let target = escape_ident(target);
                let target = target.as_str();
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
                    BinOp::Pow => {
                        // `x **= y` keeps `x`'s declared type (Python semantics),
                        // unlike binary `**` whose oracle type is Float. Mirror the
                        // operand-driven emission of the binary Pow arm:
                        //   int target  -> __py_ipow (i64, panics on negative exp)
                        //   float target-> f64 powf
                        // so `12 **= 2` stays the int 144 and a float target stays float.
                        if matches!(target_ty, Ty::Float) {
                            self.line(&format!("{} = (({} as f64).powf({} as f64));", target, target, v));
                        } else {
                            self.line(&format!("{} = __py_ipow(({}), ({}));", target, target, v));
                        }
                    }
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div
                    | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor
                    | BinOp::LShift | BinOp::RShift => {
                        // Direct Rust compound-assignment. Bitwise/shift ops are
                        // int-only in pyrst, so `&=`/`|=`/`^=`/`<<=`/`>>=` map 1:1.
                        let op_s = match op {
                            BinOp::Add => "+=", BinOp::Sub => "-=", BinOp::Mul => "*=", BinOp::Div => "/=",
                            BinOp::BitAnd => "&=", BinOp::BitOr => "|=", BinOp::BitXor => "^=",
                            BinOp::LShift => "<<=", BinOp::RShift => ">>=",
                            _ => unreachable!(),
                        };
                        self.line(&format!("{} {} {};", target, op_s, v));
                    }
                    // FloorDiv/Mod/Pow are handled by explicit arms above. No other
                    // BinOp can reach an AugAssign target: comparison, logical,
                    // identity, and membership operators are not augmented-assign
                    // operators, so the parser never produces them here. Make an
                    // unhandled op a hard error rather than silently miscompiling
                    // (the previous `_ => "+="` fallback was a latent miscompile).
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                    | BinOp::And | BinOp::Or
                    | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => {
                        unreachable!("non-augmentable BinOp {:?} reached AugAssign codegen", op);
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
                    // (EPIC-6) `var` names an existing Optional local; both the new
                    // shadow binding and the `.unwrap()` read escape identically.
                    let var_e = escape_ident(var);
                    self.line(&format!("let {} = {}.unwrap();", var_e, var_e));
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
                        // (EPIC-6) Same escape as the THEN-branch narrowing above.
                        let var_e = escape_ident(var);
                        self.line(&format!("let {} = {}.unwrap();", var_e, var_e));
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
                // Check if element type is Copy to use .iter().copied() instead of
                // .iter().cloned(). Copy-ness goes through the single shared
                // predicate (`crate::typeck::is_copy`), so the for-loop lowering
                // can't drift from the rest of codegen — it also picks up `Unit`
                // and recursively-Copy `Tuple`/`Option` elements the old inline
                // `matches!` omitted.
                let is_copy_elem = if let Expr::Ident(name, _) = iter {
                    self.locals.get(name.as_str()).or_else(|| self.ctx.vars.get(name.as_str()))
                        .map(|ty| if let Ty::List(inner) = ty {
                            self.is_copy_type(inner)
                        } else { false })
                        .unwrap_or(false)
                } else {
                    false
                };
                // Resolve the iterable's static type up front so the iteration
                // lowering matches the Python semantics for each container:
                //   dict -> iterate KEYS; str -> iterate characters.
                let for_iter_ty = self.type_of_expr(iter);
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
                } else if matches!(for_iter_ty, Ty::Str) {
                    // Iterating a str yields 1-character strings (Python semantics).
                    // Mirrors the comprehension lowering.
                    format!("{}.chars().map(|__c| __c.to_string())", i)
                } else if matches!(for_iter_ty, Ty::Dict(_, _)) {
                    // Iterating a dict yields its KEYS (Python semantics).
                    // Materialize a sorted Vec of the keys so the iteration order
                    // is deterministic — matching the sort-for-stability convention
                    // used by `PyRepr` for HashMap display.
                    format!(
                        "{{ let mut __keys: Vec<_> = {}.keys().cloned().collect(); __keys.sort(); __keys }}.into_iter()",
                        i
                    )
                } else if is_copy_elem {
                    format!("{}.iter().copied()", i)
                } else {
                    format!("{}.iter().cloned()", i)
                };
                // (EPIC-6) Escape each loop-variable name in the `for` pattern;
                // body uses resolve to the same escaped form (emit_expr Ident).
                let pat = if targets.len() == 1 {
                    escape_ident(&targets[0])
                } else {
                    format!("({})", targets.iter().map(|t| escape_ident(t)).collect::<Vec<_>>().join(", "))
                };
                self.line(&format!("for {} in {} {{", pat, iter_expr));
                self.indent += 1;

                // Register the loop variable's type so the body sees it. Reuse the
                // iterable type resolved above: list/set yield the element type, a
                // dict yields its KEY type, str yields 1-char strings (Str), and a
                // range yields Int. The loop var must be registered as a LOCAL even
                // when its element type is unknown (fallback Unknown), because the
                // for-pattern binding SHADOWS any module const of the same name:
                // the body must reference the loop variable, not mangle the name to
                // the const (`for i in range(3)` with a module const `i`).
                let loop_elem_ty = match &for_iter_ty {
                    Ty::List(inner) | Ty::Set(inner) => (**inner).clone(),
                    Ty::Dict(key, _) => (**key).clone(),
                    Ty::Str => Ty::Str,
                    _ if is_range => Ty::Int,
                    _ => Ty::Unknown,
                };
                if targets.len() == 1 {
                    let saved = self.locals.get(&targets[0]).cloned();
                    self.locals.insert(targets[0].clone(), loop_elem_ty);
                    for s in body { self.emit_stmt(s)?; }
                    if let Some(ty) = saved {
                        self.locals.insert(targets[0].clone(), ty);
                    } else {
                        self.locals.remove(targets[0].as_str());
                    }
                } else {
                    // Multiple targets (tuple unpacking): register each as a local
                    // (Unknown type) for the body's duration so each shadows any
                    // same-named module const, then restore.
                    let saved: Vec<(String, Option<Ty>)> = targets.iter()
                        .map(|t| (t.clone(), self.locals.get(t).cloned()))
                        .collect();
                    for t in targets { self.locals.insert(t.clone(), Ty::Unknown); }
                    for s in body { self.emit_stmt(s)?; }
                    for (t, prev) in saved {
                        match prev {
                            Some(ty) => { self.locals.insert(t, ty); }
                            None => { self.locals.remove(t.as_str()); }
                        }
                    }
                }

                self.indent -= 1;
                self.line("}");
            }
            Stmt::Import { .. } => {
                // Silently drop imports in v0
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                self.emit_try(body, handlers, else_, finally_)?;
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
                    // (EPIC-6) `with ... as <name>:` binds a user local; escape the
                    // emitted name (raw stays the `locals` key).
                    self.line(&format!("let mut {} = {};", escape_ident(name), ctx_s));
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
                // (EPIC-5 C2-2b-i) A field-WRITE through a polymorphic-base var
                // (`a.balance = ...` where `a: Account` and Account has subclasses)
                // would target a `B__` enum, which has no fields. A mutating
                // accessor on the companion enum is a deferred follow-on — refuse
                // honestly rather than miscompile. `self.field = ...` inside a
                // method is EXEMPT: `self` is the concrete struct (the method body
                // runs on a `Account`/`Savings`, not `Account__`), so the write is
                // an ordinary in-place struct-field store.
                if !matches!(obj.as_ref(),
                             Expr::Ident(n, _) if n == "self"
                                 || self.concrete_struct_params.contains(n)) {
                    if let Ty::Class(b) = self.type_of_expr(obj) {
                        if self.is_polymorphic_base(&b) {
                            return Err(crate::diag::Error::Codegen(format!(
                                "writing field `{}` through a polymorphic-base `{}` variable \
                                 is not yet supported — a mutating field accessor on the \
                                 companion enum is a deferred follow-on (read-only base-field \
                                 access is supported)",
                                attr, b
                            )));
                        }
                    }
                }
                let v = self.emit_consuming(value)?;
                // The base must be emitted as a *place* (lvalue), not the
                // clone-based rvalue emit_expr produces for Attr/Index.
                let place = self.emit_place(obj)?;
                // (EPIC-6) Escape a keyword field name in the field-WRITE target so
                // it matches the (escaped) struct field def.
                self.line(&format!("{}.{} = {};", place, escape_ident(attr), v));
            }
            Stmt::IndexAssign { obj, idx, value, .. } => {
                let v = self.emit_consuming(value)?;
                let place = self.emit_place(obj)?;
                // Dispatch on the base's collection kind (dict -> HashMap::insert,
                // list -> indexed store). type_of_expr resolves chained bases
                // (self.dict, grid[r], ...), not just bare locals.
                let is_dict = matches!(self.type_of_expr(obj), Ty::Dict(..));
                if is_dict {
                    // HashMap::insert takes ownership of the key, so emit it owned
                    // (a String key var becomes `k.clone()`; Copy keys are unchanged).
                    let k = self.emit_consuming(idx)?;
                    self.line(&format!("{}.insert({}, {});", place, k, v));
                } else {
                    let i = self.emit_expr(idx)?;
                    self.line(&format!("{}[{} as usize] = {};", place, i, v));
                }
            }
            Stmt::Match { subject, arms, .. } => {
                // Clone (do not move) a non-Copy scrutinee place so it stays usable
                // after the match — uniform clone-on-use.
                let subj = self.emit_consuming(subject)?;
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


    fn emit_try(
        &mut self,
        body: &[Stmt],
        handlers: &[ExceptHandler],
        else_: &Option<Vec<Stmt>>,
        finally_: &Option<Vec<Stmt>>,
    ) -> Result<()> {
                self.line("{");
                self.indent += 1;

                // Run the try body inside catch_unwind. pyrst's `raise` compiles
                // to a panic whose payload is a formatted string (see Stmt::Raise).
                // The exception type and message are separated by a NUL byte (`\0`),
                // a delimiter that cannot occur in pyrst user data:
                //   raise Foo("m")  -> "Foo\0m"
                //   raise Foo       -> "Foo\0"   (empty message)
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
                // Split "<Type>\0<msg>" on the NUL delimiter (which cannot appear in
                // user data); otherwise type == msg == whole string. split_once takes
                // the message verbatim after the delimiter, so a message that contains
                // the old " panic: " text is preserved intact.
                self.line("let (__exc_type, __exc_msg): (String, String) = match __exc_str.split_once('\\0') {");
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
                        // (EPIC-6) `except E as <name>:` binds a user local; escape
                        // it and the suppression read so a keyword name compiles.
                        // Register it as a SCOPED local (Str) for the handler body
                        // so a same-named MODULE CONST is shadowed only INSIDE the
                        // handler — without this scoping, a bare reference to a
                        // const-named exc binding (e.g. `except ... as e` next to a
                        // const `e`) would mangle to the const, and conversely a
                        // const read outside the handler must still resolve to the
                        // const. Save/restore around the body.
                        let exc_saved = if let Some(name) = &h.exc_name {
                            let name_e = escape_ident(name);
                            self.line(&format!("let {} = __exc_msg.clone();", name_e));
                            self.line(&format!("let _ = &{};", name_e));
                            let prev = self.locals.get(name).cloned();
                            self.locals.insert(name.clone(), Ty::Str);
                            Some((name.clone(), prev))
                        } else {
                            None
                        };
                        for s in &h.body { self.emit_stmt(s)?; }
                        if let Some((name, prev)) = exc_saved {
                            match prev {
                                Some(ty) => { self.locals.insert(name, ty); }
                                None => { self.locals.remove(name.as_str()); }
                            }
                        }
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
        Ok(())
    }

    // The body of this helper is moved verbatim from the former `Expr::Call`
    // arm of `emit_expr`, whose match binding typed `callee`/`args`/`kwargs` as
    // `&Box<Expr>` / `&Vec<_>`. Keeping those exact parameter types lets the
    // moved code (`callee.as_ref()`, `args[..]`, `kwargs.iter()`, ...) compile
    // unchanged, so the emitted Rust is byte-for-byte identical.
    #[allow(clippy::borrowed_box)]
    fn emit_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<String> {
                if let Some(__s) = self.emit_builtin_call(callee, args, kwargs)? { return Ok(__s); }

                if let Some(__s) = self.emit_constructor_call(callee, args, kwargs)? { return Ok(__s); }

                if let Some(__s) = self.emit_super_method_call(callee, args)? { return Ok(__s); }

                if let Some(__s) = self.emit_method_call_on_attr(callee, args)? { return Ok(__s); }

                self.emit_plain_func_call(callee, args, kwargs)
    }

    /// Emit a REGULAR function call (not a builtin / constructor / super /
    /// method) — the tail of [`Codegen::emit_call`]. Split out so the qualified
    /// module-call re-dispatch can reach it DIRECTLY: a flat module function
    /// whose name COLLIDES with a builtin (e.g. `math.pow` vs the builtin `pow`)
    /// must call the module function, not the builtin, so it must NOT re-enter
    /// `emit_builtin_call`. This applies the same Optional / by-ref /
    /// default-argument coercion as a bare flat call.
    #[allow(clippy::borrowed_box)]
    fn emit_plain_func_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<String> {
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
                // (EPIC-4 V2-c) Per-arg by-reference (`Mut[T]`) flags for this
                // free-function callee. Parallel to `args` (free functions have no
                // `self`, so `param_by_ref[i]` lines up with `args[i]` directly).
                let param_by_ref: Vec<bool> = if let Expr::Ident(n, _) = callee.as_ref() {
                    self.ctx.funcs.get(n.as_str())
                        .map(|sig| sig.param_by_ref.clone())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let mut parts = Vec::with_capacity(args.len());
                for (i, a) in args.iter().enumerate() {
                    if param_by_ref.get(i).copied().unwrap_or(false) {
                        // By-reference arg: borrow the caller's PLACE so the
                        // callee's mutation persists. typeck already required `a`
                        // to be a place (Ident/Attr/Index), so `emit_place` is
                        // valid and `&mut` of it is a sound mutable borrow. No
                        // clone, no Option coercion — we pass the storage itself.
                        // `byref_borrow` emits an explicit reborrow (`&mut *x`)
                        // when `a` names one of this function's own `&mut T`
                        // params (forwarded-by-reference, e.g. a recursive call),
                        // avoiding the E0596 double-`&mut`.
                        let place = self.emit_place(a)?;
                        parts.push(self.byref_borrow(a, &place));
                        continue;
                    }
                    // (EPIC-5 C2-2b-i) A raw-struct argument into a polymorphic-base
                    // parameter (`feed(dog)` where `feed(a: Animal)`) is WRAPPED in
                    // the right `Animal__` variant (replaces the C1 gate).
                    // (first-class functions) A function NAME / lambda argument into
                    // a `Callable[..]` parameter (`apply_to_all(inc, ..)`) is wrapped
                    // into the `Rc<dyn Fn>` slot. Other params keep clone-on-use.
                    let s = match param_tys.get(i) {
                        Some(pt @ Ty::Func(..)) => self.emit_into_func_slot(a, pt)?,
                        Some(pt) if self.ty_has_poly_base(pt) => self.emit_into_base_slot(a, pt)?,
                        _ => self.emit_consuming(a)?,
                    };
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

                Ok(format!("{}({})", callee_s, parts.join(", ")))
    }

    #[allow(clippy::borrowed_box)]
    fn emit_method_call_on_attr(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
    ) -> Result<Option<String>> {
                // Method call with attribute callee — handle method name remapping
                if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                    // Qualified module call `X.f(args)` for a REAL imported module
                    // (card 81db88e0). When X is a tracked module name and f is one
                    // of its functions, lower the call to the FLAT function `f(args)`
                    // — every imported module's functions are merged into `ctx.funcs`
                    // under their bare name, so the flat call resolves at codegen and
                    // build. We re-dispatch through `emit_call` with a synthesized
                    // `Ident(f)` callee so the regular function-call machinery
                    // (Optional/by-ref/default-argument coercion) applies uniformly,
                    // exactly as if the user had written `from X import f; f(args)`.
                    // `math` is now a REAL embedded module (`lib/math.pyrs`), so
                    // `math.sqrt(x)` flows through here too (its @extern `sqrt`
                    // is merged into `module_funcs`/`ctx.funcs`); the former
                    // hardcoded math call-arm is gone. We re-dispatch through
                    // `emit_plain_func_call` (NOT `emit_call`) so a module
                    // function whose flat name COLLIDES with a builtin — e.g.
                    // `math.pow` vs the builtin `pow` — calls the MODULE function,
                    // not the builtin int-pow. NOTE: flat emission means a
                    // cross-module same-name collision between two modules is
                    // unresolved (stdlib uses distinct names; per-module
                    // namespacing `X__f` is a later refinement).
                    if let Expr::Ident(modname, _) = obj.as_ref() {
                        if self.ctx.module_funcs.get(modname).is_some_and(|fns| fns.iter().any(|n| n == name)) {
                            let span = callee.span();
                            let flat_callee: Box<Expr> = Box::new(Expr::Ident(name.clone(), span));
                            return Ok(Some(self.emit_plain_func_call(&flat_callee, args, &[])?));
                        }
                    }

                    // Check for static method calls: ClassName.method(args)
                    if let Expr::Ident(class_name, _) = obj.as_ref() {
                        if let Some(class_def) = self.ctx.classes.get(class_name.as_str()) {
                            if let Some(method_def) = class_def.methods.iter().find(|m| &m.name == name) {
                                if method_def.decorators.contains(&"staticmethod".to_string()) {
                                    let parts: Result<Vec<_>> = args.iter().map(|a| self.emit_consuming(a)).collect();
                                    let parts = parts?;
                                    return Ok(Some(format!("{}::{}({})", class_name, name, parts.join(", "))));
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
                    let parts: Result<Vec<_>> = args.iter().map(|a| self.emit_consuming(a)).collect();
                    let parts = parts?;

                    // (EPIC-6) Receiver-type-guarded early return. The builtin
                    // method arms below match purely on `name` with NO receiver
                    // guard on most of them (`get`, `keys`, `values`, `items`,
                    // `update`, `pop`, `copy`, `clear`, `append`, `extend`,
                    // `insert`, `remove`, `sort`, ...). So a USER class that
                    // defines a method with one of those names previously had
                    // `instance.get(k)` silently lowered to a dict
                    // `.get(&k).cloned()` (wrong Rust / wrong behavior / a
                    // compile error) — the builtin arm won because it ran BEFORE
                    // the user-method tail. Guard it here: if the receiver's
                    // static type is a user class that HAS an instance method
                    // named `name` (resolved via `get_method`, walking the
                    // inheritance chain — the SAME lookup the user-method tail
                    // uses), dispatch to that user method NOW and return,
                    // bypassing every builtin arm. A builtin receiver
                    // (str/list/dict/set/file) is never `Ty::Class`, so the
                    // guard never fires for it and the builtin arms below run
                    // byte-for-byte unchanged. A polymorphic-base receiver
                    // composes too: `cls` is the base name, `get_method` returns
                    // the base's signature, and `obj_s.name(..)` resolves to the
                    // companion enum `cls__`'s dispatch method — identical to the
                    // pre-existing EPIC-5 lowering.
                    if let Ty::Class(cls) = self.type_of_expr(obj.as_ref()) {
                        if self.ctx.get_method(&cls, name).is_some() {
                            return self.emit_user_method_call(&obj_s, &cls, name, args, &parts).map(Some);
                        }
                    }

                    // Special handling for string methods that return &str and need to be converted to String
                    if matches!(name.as_str(), "strip" | "lstrip" | "rstrip") {
                        return Ok(Some(format!("{}.{}().to_string()", obj_s, method)));
                    }

                    // Special case: split()
                    if name == "split" {
                        return if args.is_empty() {
                            Ok(Some(format!("{}.split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>()", obj_s)))
                        } else {
                            let sep = parts[0].clone();
                            Ok(Some(format!("{}.split({}.as_str()).map(|s| s.to_string()).collect::<Vec<_>>()", obj_s, sep)))
                        };
                    }

                    // Special case: join()
                    if name == "join" {
                        return Ok(Some(format!("{}.join(&{})", parts[0], obj_s)));
                    }

                    // Special case: len() as method
                    if name == "len" {
                        // str length is character count, not UTF-8 byte count.
                        if matches!(self.type_of_expr(obj.as_ref()), Ty::Str) {
                            return Ok(Some(format!("{}.chars().count() as i64", obj_s)));
                        }
                        return Ok(Some(format!("{}.len() as i64", obj_s)));
                    }

                    // Special case: get() for dicts. Arg-count-aware, mirroring
                    // the static typing in `typeck::dict_get_ret`:
                    //   d.get(k)           -> Option<V>  (None when absent), so a
                    //                         caller can narrow it with `is None`.
                    //   d.get(k, default)  -> V          (the supplied fallback).
                    if name == "get" {
                        if parts.len() > 1 {
                            return Ok(Some(format!(
                                "{}.get(&{}).cloned().unwrap_or({})",
                                obj_s, parts[0], parts[1]
                            )));
                        }
                        return Ok(Some(format!("{}.get(&{}).cloned()", obj_s, parts[0])));
                    }

                    // String methods
                    if name == "startswith" && !parts.is_empty() {
                        return Ok(Some(format!("{}.starts_with({}.as_str())", obj_s, parts[0])));
                    }
                    if name == "endswith" && !parts.is_empty() {
                        return Ok(Some(format!("{}.ends_with({}.as_str())", obj_s, parts[0])));
                    }
                    if name == "replace" && parts.len() >= 2 {
                        return Ok(Some(format!("{}.replace({}.as_str(), {}.as_str())", obj_s, parts[0], parts[1])));
                    }
                    if name == "removeprefix" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __prefix = {}; \
                            if __s.starts_with(__prefix.as_str()) {{ __s[__prefix.len()..].to_string() }} else {{ __s }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "removesuffix" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __suffix = {}; \
                            if __s.ends_with(__suffix.as_str()) {{ __s[..__s.len() - __suffix.len()].to_string() }} else {{ __s }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "expandtabs" {
                        let tab_size = if !parts.is_empty() {
                            format!("{} as usize", parts[0])
                        } else {
                            "8usize".to_string()
                        };
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __tab_size = {}; \
                            __s.replace('\\t', &\" \".repeat(__tab_size)) }}",
                            obj_s, tab_size
                        )));
                    }
                    if name == "partition" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __sep = {}; \
                            if let Some(__idx) = __s.find(__sep.as_str()) {{ \
                            vec![__s[..__idx].to_string(), __sep.clone(), __s[__idx + __sep.len()..].to_string()] \
                            }} else {{ vec![__s, String::new(), String::new()] }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "rpartition" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __sep = {}; \
                            if let Some(__idx) = __s.rfind(__sep.as_str()) {{ \
                            vec![__s[..__idx].to_string(), __sep.clone(), __s[__idx + __sep.len()..].to_string()] \
                            }} else {{ vec![String::new(), String::new(), __s] }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "find" && !parts.is_empty() {
                        return Ok(Some(format!("{}.find({}.as_str()).map(|i| i as i64).unwrap_or(-1i64)", obj_s, parts[0])));
                    }
                    if name == "contains" && !parts.is_empty() {
                        return Ok(Some(format!("{}.contains({}.as_str())", obj_s, parts[0])));
                    }
                    if name == "rfind" && !parts.is_empty() {
                        return Ok(Some(format!("{}.rfind({}.as_str()).map(|i| i as i64).unwrap_or(-1i64)", obj_s, parts[0])));
                    }
                    if name == "rindex" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __idx = {}.rfind({}.as_str()); match __idx {{ Some(i) => i as i64, None => panic!(\"substring not found\") }} }}",
                            obj_s, parts[0]
                        )));
                    }

                    // String utility methods
                    if name == "isdigit" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_numeric()))", obj_s, obj_s)));
                    }
                    if name == "isalpha" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_alphabetic()))", obj_s, obj_s)));
                    }
                    if name == "isupper" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) && {}.chars().any(|c| c.is_alphabetic()))", obj_s, obj_s, obj_s)));
                    }
                    if name == "islower" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_lowercase()) && {}.chars().any(|c| c.is_alphabetic()))", obj_s, obj_s, obj_s)));
                    }
                    if name == "isspace" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_whitespace()))", obj_s, obj_s)));
                    }
                    if name == "isalnum" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_alphanumeric()))", obj_s, obj_s)));
                    }
                    if name == "isidentifier" {
                        return Ok(Some(format!(
                            "(!{}.is_empty() && ({}.chars().next().unwrap().is_alphabetic() || {}.chars().next().unwrap() == '_') && {}.chars().all(|c| c.is_alphanumeric() || c == '_'))",
                            obj_s, obj_s, obj_s, obj_s
                        )));
                    }
                    if name == "isnumeric" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_numeric()))", obj_s, obj_s)));
                    }
                    if name == "isprintable" {
                        return Ok(Some(format!("({}.chars().all(|c| !c.is_control()))", obj_s)));
                    }
                    if name == "istitle" {
                        return Ok(Some(format!(
                            "(!{}.is_empty() && {}.split_whitespace().all(|word| if word.is_empty() {{ true }} else {{ word.chars().next().unwrap().is_uppercase() && word[1..].chars().all(|c| !c.is_alphabetic() || c.is_lowercase()) }}))",
                            obj_s, obj_s
                        )));
                    }

                    // Additional string methods
                    if name == "capitalize" {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); if __s.is_empty() {{ __s }} else {{ format!(\"{{}}{{}}\" , __s.chars().next().unwrap().to_uppercase(), &__s[1..].to_lowercase()) }} }}",
                            obj_s
                        )));
                    }
                    if name == "title" {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); __s.split_whitespace().map(|w| if w.is_empty() {{ w.to_string() }} else {{ format!(\"{{}}{{}}\" , w.chars().next().unwrap().to_uppercase(), &w[1..].to_lowercase()) }} ).collect::<Vec<_>>().join(\" \") }}",
                            obj_s
                        )));
                    }
                    if name == "zfill" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ format!(\"{{:0>width$}}\" , __s, width = __width) }} }}",
                            parts[0], obj_s
                        )));
                    }
                    if name == "ljust" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ format!(\"{{:<width$}}\" , __s, width = __width) }} }}",
                            parts[0], obj_s
                        )));
                    }
                    if name == "rjust" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ format!(\"{{:>width$}}\" , __s, width = __width) }} }}",
                            parts[0], obj_s
                        )));
                    }
                    if name == "center" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); if __s.len() >= __width {{ __s }} else {{ let __total = __width - __s.len(); let __left = (__total + 1) / 2; let __right = __total / 2; format!(\"{{}}{{}}{{}}\" , \" \".repeat(__left), __s, \" \".repeat(__right)) }} }}",
                            parts[0], obj_s
                        )));
                    }
                    if name == "swapcase" {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); __s.chars().map(|c| if c.is_uppercase() {{ c.to_lowercase().to_string() }} else {{ c.to_uppercase().to_string() }} ).collect::<String>() }}",
                            obj_s
                        )));
                    }
                    if name == "splitlines" {
                        return Ok(Some(format!(
                            "{}.lines().map(|l| l.to_string()).collect::<Vec<_>>()",
                            obj_s
                        )));
                    }
                    if name == "count" && !parts.is_empty() {
                        let obj_ty = self.type_of_expr(obj);
                        match obj_ty {
                            Ty::Str => {
                                return Ok(Some(format!(
                                    "{{ let __s = {}.clone(); let __sub = {}; let mut __count = 0i64; let mut __start = 0; while let Some(__pos) = __s.as_str()[__start..].find(__sub.as_str()) {{ __count += 1; __start += __pos + __sub.len(); }} __count }}",
                                    obj_s, parts[0]
                                )));
                            }
                            _ => {} // Fall through to list count below
                        }
                    }
                    if name == "index" && !parts.is_empty() {
                        let obj_ty = self.type_of_expr(obj);
                        match obj_ty {
                            Ty::Str => {
                                return Ok(Some(format!(
                                    "{}.find({}.as_str()).map(|i| i as i64).expect(\"substring not found\")",
                                    obj_s, parts[0]
                                )));
                            }
                            _ => {} // Fall through to list index below
                        }
                    }

                    // File methods (PyFile; gated on a File receiver). write takes
                    // &str, so borrow the argument.
                    if let Ty::File = self.type_of_expr(obj) {
                        match name.as_str() {
                            "write" if !parts.is_empty() => return Ok(Some(format!("{}.write(&{})", obj_s, parts[0]))),
                            "write" => return Err(crate::diag::Error::Codegen("file write() requires one argument".into())),
                            "read" | "readlines" | "close" =>
                                return Ok(Some(format!("{}.{}()", obj_s, name))),
                            _ => {}
                        }
                    }

                    // Dict views - materialize into a Vec so they work both in a
                    // for-loop and as a value (e.g. print(d.keys()), len(d.values())),
                    // matching their List(K)/List(V) static type.
                    if name == "keys" {
                        return Ok(Some(format!("{}.keys().cloned().collect::<Vec<_>>()", obj_s)));
                    }
                    if name == "values" {
                        return Ok(Some(format!("{}.values().cloned().collect::<Vec<_>>()", obj_s)));
                    }
                    if name == "items" {
                        // Collect into a Vec<(K, V)> so the for-loop lowering treats it
                        // as a normal collection (it wraps the iterable in .iter().cloned()).
                        return Ok(Some(format!("{}.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>()", obj_s)));
                    }

                    // Set methods (gated on receiver type — many names overlap with
                    // list/dict, so disambiguate by the static type of the receiver).
                    if let Ty::Set(_) = self.type_of_expr(obj) {
                        match name.as_str() {
                            // insert takes ownership, so emit the element owned
                            // (a String var becomes `x.clone()`).
                            "add" if !parts.is_empty() =>
                                return Ok(Some(format!("{{ {}.insert({}); }}", obj_s, self.emit_consuming(&args[0])?))),
                            // NB: unlike Python, neither discard nor remove raises on an
                            // absent element here (Rust's HashSet::remove returns an ignored bool).
                            "discard" | "remove" if !parts.is_empty() =>
                                return Ok(Some(format!("{{ {}.remove(&{}); }}", obj_s, parts[0]))),
                            "update" if !parts.is_empty() =>
                                return Ok(Some(format!("{{ {}.extend({}.iter().cloned()); }}", obj_s, parts[0]))),
                            "union" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.union(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "intersection" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.intersection(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "difference" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.difference(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "symmetric_difference" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.symmetric_difference(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "issubset" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.is_subset(&{})", obj_s, parts[0]))),
                            "issuperset" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.is_superset(&{})", obj_s, parts[0]))),
                            "isdisjoint" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.is_disjoint(&{})", obj_s, parts[0]))),
                            _ => {}
                        }
                    }

                    // dict.update(other) — merge another mapping in place.
                    if name == "update" && !parts.is_empty() {
                        return Ok(Some(format!("{{ {}.extend({}); }}", obj_s, parts[0])));
                    }

                    if name == "pop" {
                        // list.pop(): remove and return the last element (or pop(i) -> remove index).
                        if let Ty::List(_) = self.type_of_expr(obj) {
                            return Ok(Some(if parts.is_empty() {
                                format!("{}.pop().expect(\"pop from empty list\")", obj_s)
                            } else {
                                // Honor Python negative indices: pop(-1) is the last element.
                                format!(
                                    "{{ let __n = {obj}.len() as i64; let __i = {idx}; \
                                     {obj}.remove((if __i < 0 {{ __n + __i }} else {{ __i }}) as usize) }}",
                                    obj = obj_s, idx = parts[0]
                                )
                            }));
                        }
                        // dict.pop(key[, default])
                        if parts.is_empty() {
                            return Err(crate::diag::Error::Codegen("pop requires at least one argument".into()));
                        } else if parts.len() == 1 {
                            // pop(key) — remove from the receiver and return the value (panic if absent)
                            return Ok(Some(format!("{}.remove(&{}).expect(\"KeyError: key not found\")", obj_s, parts[0])));
                        } else {
                            // pop(key, default) — remove from the receiver; default if absent
                            return Ok(Some(format!("{}.remove(&{}).unwrap_or({})", obj_s, parts[0], parts[1])));
                        }
                    }
                    // List methods
                    if name == "extend" && !parts.is_empty() {
                        return Ok(Some(format!("{}.extend({})", obj_s, parts[0])));
                    }
                    if name == "insert" && parts.len() >= 2 {
                        return Ok(Some(format!("{}.insert({} as usize, {})", obj_s, parts[0], parts[1])));
                    }
                    if name == "remove" && !parts.is_empty() {
                        return Ok(Some(format!("{{ let __idx = {}.iter().position(|__x| *__x == {}).expect(\"value not found\"); {}.remove(__idx); }}", obj_s, parts[0], obj_s)));
                    }
                    if name == "index" && !parts.is_empty() {
                        return Ok(Some(format!("{}.iter().position(|__x| *__x == {}).expect(\"value not found\") as i64", obj_s, parts[0])));
                    }
                    if name == "count" && !parts.is_empty() {
                        return Ok(Some(format!("{}.iter().filter(|__x| **__x == {}).count() as i64", obj_s, parts[0])));
                    }
                    if name == "reverse" {
                        return Ok(Some(format!("{}.reverse()", obj_s)));
                    }
                    if name == "sort" {
                        return Ok(Some(format!("{}.sort()", obj_s)));
                    }
                    if name == "clear" {
                        return Ok(Some(format!("{}.clear()", obj_s)));
                    }
                    if name == "copy" {
                        return Ok(Some(format!("{}.clone()", obj_s)));
                    }

                    // Regular method call.
                    // (EPIC-4 V2-c) Thread `&mut <place>` for any by-reference
                    // (`Mut[T]`) method parameter so the callee's mutation persists
                    // to the caller. The method's per-param by-ref flags come from
                    // get_method (self-EXCLUSIVE and index-aligned to `args` after
                    // STEP 0). Only user-defined methods on a known class receiver
                    // can be by-ref; the builtin string/list/dict branches above
                    // all `return`ed earlier, so the by-value `parts` they share is
                    // never reached here. We rebuild `parts` only when the receiver
                    // resolves to a class with a matching method that actually has
                    // a by-ref param; otherwise the original by-value `parts`
                    // (clone-on-use) is used unchanged.
                    let method_by_ref: Vec<bool> =
                        if let Ty::Class(cls) = self.type_of_expr(obj.as_ref()) {
                            self.ctx.get_method(&cls, name)
                                .map(|sig| sig.param_by_ref.clone())
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                    if method_by_ref.iter().any(|&b| b) {
                        let mut mparts = Vec::with_capacity(args.len());
                        for (i, a) in args.iter().enumerate() {
                            if method_by_ref.get(i).copied().unwrap_or(false) {
                                let place = self.emit_place(a)?;
                                mparts.push(self.byref_borrow(a, &place));
                            } else {
                                mparts.push(self.emit_consuming(a)?);
                            }
                        }
                        return Ok(Some(format!("{}.{}({})", obj_s, method, mparts.join(", "))));
                    }
                    return Ok(Some(format!("{}.{}({})", obj_s, method, parts.join(", "))));
                }
        Ok(None)
    }

    #[allow(clippy::borrowed_box)]
    fn emit_super_method_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
    ) -> Result<Option<String>> {
                // Handle super().method(args)
                if let Expr::Attr { obj: super_call_expr, name: method_name, .. } = callee.as_ref() {
                    if let Expr::Call { callee: super_ident, args: super_args, .. } = super_call_expr.as_ref() {
                        if let Expr::Ident(n, _) = super_ident.as_ref() {
                            if n == "super" && super_args.is_empty() {
                                if let Some(_class_name) = self.current_class.clone() {
                                    // Call __super_ alias method which has parent's body
                                    let mut arg_parts = Vec::new();
                                    for a in args { arg_parts.push(self.emit_consuming(a)?); }
                                    return Ok(Some(format!("self.__super_{}({})", method_name, arg_parts.join(", "))));
                                }
                            }
                        }
                    }
                }
        Ok(None)
    }

    #[allow(clippy::borrowed_box)]
    fn emit_constructor_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<Option<String>> {
                // Check if this is a class constructor call.
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if let Some(class_def) = self.ctx.classes.get(name.as_str()).cloned() {
                        let has_init = class_def.methods.iter().any(|m| m.name == "__init__");

                        // Use ::new() constructor whenever __init__ is defined —
                        // including the zero-arg case so that __init__ side-effects
                        // (field assignments, etc.) always run.
                        if has_init {
                            // (EPIC-5 C2-3) The `new()` signature lowers each
                            // `__init__` param via `rust_ty`, so a base-typed param
                            // is `B__`. A raw-struct / subclass argument into such a
                            // slot must be WRAPPED in the right variant (the same
                            // wrap-or-passthrough used at return / assign / free-fn
                            // sites) — otherwise the bare `Dog::new(..)` mismatches
                            // the `Animal__` param (E0308). Non-polymorphic params
                            // keep the clone-on-use emission.
                            let init_params: Vec<(String, Ty)> = class_def.methods.iter()
                                .find(|m| m.name == "__init__")
                                .map(|m| m.params.iter()
                                    .filter(|p| p.name != "self")
                                    .filter_map(|p| Ty::from_type_expr(&p.ty, p.span).ok().map(|t| (p.name.clone(), t)))
                                    .collect())
                                .unwrap_or_default();
                            let mut call_parts = Vec::new();
                            for (i, a) in args.iter().enumerate() {
                                call_parts.push(self.emit_arg_into_slot(a, init_params.get(i).map(|(_, t)| t))?);
                            }
                            for (kw, v) in kwargs {
                                let pt = init_params.iter().find(|(n, _)| n == kw).map(|(_, t)| t);
                                call_parts.push(self.emit_arg_into_slot(v, pt)?);
                            }
                            return Ok(Some(format!("{}::new({})", name, call_parts.join(", "))));
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
                                // (EPIC-5 C2-3) The struct field lowers to `B__` for
                                // a polymorphic-base field, so a raw-struct/subclass
                                // value wraps in its variant (same as the ctor/new
                                // path above).
                                let fty = self.class_field_type(&class_def, field_name);
                                let v = self.emit_arg_into_slot(arg, fty.as_ref())?;
                                // (EPIC-6) Escape a keyword field name in the
                                // positional struct-literal init.
                                parts.push(format!("{}: {}", escape_ident(field_name), v));
                            }
                            return Ok(Some(format!("{} {{ {} }}", name, parts.join(", "))));
                        }

                        // Keyword-args form.
                        if !kwargs.is_empty() {
                            let mut parts = Vec::new();
                            for (kw, val) in kwargs {
                                let fty = self.class_field_type(&class_def, kw);
                                let v = self.emit_arg_into_slot(val, fty.as_ref())?;
                                // (EPIC-6) Escape a keyword field name in the
                                // keyword-arg struct-literal init.
                                parts.push(format!("{}: {}", escape_ident(kw), v));
                            }
                            return Ok(Some(format!("{} {{ {} }}", name, parts.join(", "))));
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
                                let ty = Ty::from_type_expr(&f.ty, f.span)?;
                                self.zeroed_default(&ty)
                            } else {
                                "Default::default()".to_string()
                            };
                            // (EPIC-6) Escape a keyword field name in the no-arg
                            // default struct-literal init.
                            parts.push(format!("{}: {}", escape_ident(fname), default));
                        }
                        return Ok(Some(format!("{} {{ {} }}", name, parts.join(", "))));
                    }
                }
        Ok(None)
    }

    #[allow(clippy::borrowed_box)]
    fn emit_builtin_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<Option<String>> {
                // Multi-arg print with inline format
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "print" {
                        if args.is_empty() {
                            return Ok(Some("println!(\"\")".to_string()));
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
                        return Ok(Some(format!("println!(\"{}\" {})", fmt,
                            if parts.is_empty() { "".to_string() } else { format!(", {}", parts.join(", ")) })));
                    }
                }

                // Inline range() with 1, 2, or 3 args
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "range" {
                        if args.len() == 1 {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("(0..{})", a)));
                        } else if args.len() == 2 {
                            let a = self.emit_expr(&args[0])?;
                            let b = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("({}..{})", a, b)));
                        } else if args.len() == 3 {
                            let a = self.emit_expr(&args[0])?;
                            let b = self.emit_expr(&args[1])?;
                            let step = self.emit_expr(&args[2])?;
                            return Ok(Some(format!("({}..{}).step_by({} as usize)", a, b, step)));
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
                        return Ok(Some(format!("{}.enumerate().map(|(i, v)| (i as i64, v))", iter_chain)));
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
                        return Ok(Some(format!("{}.zip({})", iter_a, iter_b)));
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
                                return Ok(Some(format!("{}.chars().count() as i64", a)));
                            }
                            return Ok(Some(format!("{}.len() as i64", a)));
                        }
                        "str" => {
                            let a = self.emit_expr(&args[0])?;
                            match self.type_of_expr(&args[0]) {
                                // Match print/f-string formatting: a whole float is
                                // "7.0" (Rust's `{}` would drop it to "7"), a bool is
                                // "True"/"False" (not Rust's "true"/"false").
                                Ty::Float => return Ok(Some(format!("__py_fmt_float({})", a))),
                                Ty::Bool => return Ok(Some(format!("__py_fmt_bool({})", a))),
                                Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) =>
                                    return Ok(Some(format!("({}).py_repr()", a))),
                                _ => return Ok(Some(format!("format!(\"{{}}\" , {})", a))),
                            }
                        }
                        "open" => {
                            let path = self.emit_expr(&args[0])?;
                            let mode = if args.len() >= 2 {
                                self.emit_expr(&args[1])?
                            } else {
                                "\"r\".to_string()".to_string()
                            };
                            return Ok(Some(format!("__py_open(&{}, &{})", path, mode)));
                        }
                        "int" => {
                            let a = self.emit_expr(&args[0])?;
                            let arg_type = self.type_of_expr(&args[0]);
                            match arg_type {
                                Ty::Str => {
                                    // Use helper so a bad string panics with "ValueError\0..."
                                    // which the try/except dispatcher can match on ValueError.
                                    return Ok(Some(format!("(__py_int_from_str(&{}))", a)));
                                }
                                _ => return Ok(Some(format!("({} as i64)", a))),
                            }
                        }
                        "float" => {
                            let a = self.emit_expr(&args[0])?;
                            let arg_type = self.type_of_expr(&args[0]);
                            match arg_type {
                                Ty::Str => {
                                    // Use helper so a bad string panics with "ValueError\0..."
                                    // which the try/except dispatcher can match on ValueError.
                                    return Ok(Some(format!("(__py_float_from_str(&{}))", a)));
                                }
                                _ => return Ok(Some(format!("({} as f64)", a))),
                            }
                        }
                        "bool" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("(({}) != 0)", a)));
                        }
                        "abs" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("({}).abs()", a)));
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
                                    // Replace param_name with __x in the body (word-boundary aware).
                                    // (EPIC-6) The body emitted the param through
                                    // emit_expr's Ident arm, which ESCAPES a keyword
                                    // param to `r#<name>`; search for that escaped
                                    // form so a keyword sort-key param is renamed
                                    // correctly (replace_identifier treats `r#kw` as
                                    // one token).
                                    Self::replace_identifier(&body_s, escape_ident(&param_name).as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };
                                return Ok(Some(format!(
                                    "{{ let __list = {}; __list.iter().min_by_key(|__x| {}).map(|__x| __x.clone()).unwrap_or_default() }}",
                                    a, key_code
                                )));
                            } else if args.len() == 1 {
                                let a = self.emit_expr(&args[0])?;
                                let elem_ty = match self.type_of_expr(&args[0]) {
                                    Ty::List(inner) => *inner,
                                    _ => Ty::Int,
                                };
                                return Ok(Some(match elem_ty {
                                    Ty::Float => format!("{{ let mut __min = f64::INFINITY; for __x in {}.iter() {{ if __x < &__min {{ __min = *__x; }} }} __min }}", a),
                                    _ => format!("{}.iter().copied().min().unwrap_or(0)", a),
                                }));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let b = self.emit_expr(&args[1])?;
                                return Ok(Some(format!("::std::cmp::min({}, {})", a, b)));
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
                                    // Replace param_name with __x in the body (word-boundary aware).
                                    // (EPIC-6) The body emitted the param through
                                    // emit_expr's Ident arm, which ESCAPES a keyword
                                    // param to `r#<name>`; search for that escaped
                                    // form so a keyword sort-key param is renamed
                                    // correctly (replace_identifier treats `r#kw` as
                                    // one token).
                                    Self::replace_identifier(&body_s, escape_ident(&param_name).as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };
                                return Ok(Some(format!(
                                    "{{ let __list = {}; __list.iter().max_by_key(|__x| {}).map(|__x| __x.clone()).unwrap_or_default() }}",
                                    a, key_code
                                )));
                            } else if args.len() == 1 {
                                let a = self.emit_expr(&args[0])?;
                                let elem_ty = match self.type_of_expr(&args[0]) {
                                    Ty::List(inner) => *inner,
                                    _ => Ty::Int,
                                };
                                return Ok(Some(match elem_ty {
                                    Ty::Float => format!("{{ let mut __max = f64::NEG_INFINITY; for __x in {}.iter() {{ if __x > &__max {{ __max = *__x; }} }} __max }}", a),
                                    _ => format!("{}.iter().copied().max().unwrap_or(0)", a),
                                }));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let b = self.emit_expr(&args[1])?;
                                return Ok(Some(format!("::std::cmp::max({}, {})", a, b)));
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
                                                        Ty::from_type_expr(&f.ty, f.span).unwrap_or(Ty::Unknown)
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
                                    // Replace param_name with __x in the body (word-boundary aware).
                                    // (EPIC-6) The body emitted the param through
                                    // emit_expr's Ident arm, which ESCAPES a keyword
                                    // param to `r#<name>`; search for that escaped
                                    // form so a keyword sort-key param is renamed
                                    // correctly (replace_identifier treats `r#kw` as
                                    // one token).
                                    Self::replace_identifier(&body_s, escape_ident(&param_name).as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };

                                // Use appropriate sorting method based on key return type
                                return Ok(Some(match key_ret_ty {
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
                                }));
                            } else {
                                // Check if this is a float list to handle Ord constraint
                                let is_float_list = matches!(&list_ty, Ty::List(inner) if inner.as_ref() == &Ty::Float);
                                let sort_code = if is_float_list {
                                    ".sort_by(|a, b| a.partial_cmp(b).unwrap_or(::std::cmp::Ordering::Equal))".to_string()
                                } else {
                                    ".sort()".to_string()
                                };

                                // `sorted` operates on a Vec. A list arg is cloned
                                // directly; a set is materialized from its elements
                                // and a dict from its KEYS (Python semantics — both
                                // HashMap/HashSet lack `.sort()`).
                                let base = match &list_ty {
                                    Ty::Set(_) => format!("{}.iter().cloned().collect::<Vec<_>>()", a),
                                    Ty::Dict(_, _) => format!("{}.keys().cloned().collect::<Vec<_>>()", a),
                                    _ => format!("{}.clone()", a),
                                };

                                if let Some((_, rev_expr)) = kwargs.iter().find(|(n, _)| n == "reverse") {
                                    // sorted with reverse parameter
                                    let rev_s = self.emit_expr(rev_expr)?;
                                    return Ok(Some(format!(
                                        "{{ let mut __sorted = {}; __sorted{}; if {} {{ __sorted.reverse(); }} __sorted }}",
                                        base, sort_code, rev_s
                                    )));
                                } else {
                                    // Default sorted
                                    return Ok(Some(format!("{{ let mut __sorted = {}; __sorted{}; __sorted }}", base, sort_code)));
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
                            return Ok(Some(format!("{}.iter().sum::<{}>()", a, sum_type)));
                        }
                        "input" => {
                            if args.is_empty() {
                                return Ok(Some("{ let mut __s = String::new(); ::std::io::stdin().read_line(&mut __s).unwrap(); __s.trim_end().to_string() }".to_string()));
                            } else {
                                let p = self.emit_expr(&args[0])?;
                                return Ok(Some(format!("{{ print!(\"{{}}\" , {}); ::std::io::stdout().flush().ok(); let mut __s = String::new(); ::std::io::stdin().read_line(&mut __s).unwrap(); __s.trim_end().to_string() }}", p)));
                            }
                        }
                        "any" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("{}.iter().any(|x| *x)", a)));
                        }
                        "all" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("{}.iter().all(|x| *x)", a)));
                        }
                        "round" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("({} as f64).round() as i64", a)));
                        }
                        "pow" => {
                            let base = self.emit_expr(&args[0])?;
                            let exp = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("({} as f64).powi({} as i32) as i64", base, exp)));
                        }
                        "chr" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("(char::from_u32({} as u32).unwrap()).to_string()", a)));
                        }
                        "ord" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("({}.chars().next().unwrap() as i64)", a)));
                        }
                        "reversed" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("{{ let mut __r = {}.clone(); __r.reverse(); __r }}", a)));
                        }
                        "map" => {
                            let f = self.emit_expr(&args[0])?;
                            let it = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("{}.iter().cloned().map({}).collect::<Vec<_>>()", it, f)));
                        }
                        "filter" => {
                            let f = self.emit_expr(&args[0])?;
                            let it = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("{}.iter().cloned().filter(|__x| ({})((__x).clone())).collect::<Vec<_>>()", it, f)));
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
                                        return Ok(Some(format!("true"))); // Placeholder for custom class check
                                    }
                                };
                                return Ok(Some(if matches { "true" } else { "false" }.to_string()));
                            } else {
                                // Dynamic type check (not a literal type name)
                                return Ok(Some("true".to_string())); // Conservative: assume true for dynamic checks
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
                            return Ok(Some(format!("String::from(\"{}\")", type_name)));
                        }
                        "hex" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("format!(\"{{:#x}}\", {})", a)));
                        }
                        "oct" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("format!(\"{{:#o}}\", {})", a)));
                        }
                        "bin" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("format!(\"{{:#b}}\", {})", a)));
                        }
                        "callable" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("callable requires exactly 1 argument".into()));
                            }
                            // Check if the argument is a function name
                            if let Expr::Ident(name, _) = &args[0] {
                                let is_callable = self.ctx.funcs.contains_key(name.as_str()) ||
                                                 self.ctx.classes.contains_key(name.as_str());
                                return Ok(Some(if is_callable { "true" } else { "false" }.to_string()));
                            } else {
                                // For non-identifier expressions, conservatively return false
                                return Ok(Some("false".to_string()));
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
                            return Ok(Some(repr_expr));
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
                            return Ok(Some(ascii_expr));
                        }
                        "list" => {
                            if args.is_empty() {
                                return Ok(Some("Vec::<i64>::new()".to_string()));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let arg_type = self.type_of_expr(&args[0]);
                                // If the argument is already a list, just return it. Otherwise collect the iterator.
                                match arg_type {
                                    Ty::List(_) => return Ok(Some(a)),
                                    // A set/dict is a concrete container, not an
                                    // iterator: take an owned Vec of its elements
                                    // (dict -> its KEYS, Python semantics).
                                    Ty::Set(_) => {
                                        return Ok(Some(format!("{}.iter().cloned().collect::<Vec<_>>()", a)));
                                    }
                                    Ty::Dict(_, _) => {
                                        return Ok(Some(format!("{}.keys().cloned().collect::<Vec<_>>()", a)));
                                    }
                                    _ => {
                                        // Check if the expression looks like it returns a Vec (contains reverse, sort, etc.)
                                        if a.contains("reverse") || a.contains("sort") || a.contains("clone()") {
                                            return Ok(Some(a));
                                        }
                                        return Ok(Some(format!("{}.collect::<Vec<_>>()", a)));
                                    }
                                }
                            }
                        }
                        "dict" => {
                            if args.is_empty() && kwargs.is_empty() {
                                return Ok(Some("std::collections::HashMap::new()".to_string()));
                            } else {
                                return Err(crate::diag::Error::Codegen("dict() constructor with arguments not yet supported".into()));
                            }
                        }
                        "tuple" => {
                            if args.is_empty() {
                                return Ok(Some("()".to_string()));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                return Ok(Some(format!("({},)", a)));
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
                            return Ok(Some(format!("{{ let __attr_name = {}; format!(\"{{:?}}\", __attr_name) }}", attr_name)));
                        }
                        "setattr" => {
                            if args.len() != 3 {
                                return Err(crate::diag::Error::Codegen("setattr requires exactly 3 arguments".into()));
                            }
                            // Note: In Python, setattr modifies the object. In Rust, we can't modify through a reference.
                            // For now, just return None
                            return Ok(Some("()".to_string()));
                        }
                        "hasattr" => {
                            if args.len() != 2 {
                                return Err(crate::diag::Error::Codegen("hasattr requires exactly 2 arguments".into()));
                            }
                            // For now, just return true (conservative assumption)
                            return Ok(Some("true".to_string()));
                        }
                        "set" => {
                            if args.is_empty() {
                                return Ok(Some("::std::collections::HashSet::new()".to_string()));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let arg_type = self.type_of_expr(&args[0]);
                                // If the argument is already a set, just return it. Otherwise convert to set.
                                match arg_type {
                                    Ty::Set(_) => return Ok(Some(a)),
                                    Ty::List(_) | Ty::Unknown => {
                                        // Check if it looks like a vec literal or variable
                                        if a.starts_with("vec!") {
                                            return Ok(Some(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a)));
                                        } else {
                                            return Ok(Some(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a)));
                                        }
                                    }
                                    _ => {
                                        // For other iterables, try to convert
                                        return Ok(Some(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a)));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
        Ok(None)
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
                // (EPIC-5 C2-2b-i, Step 3) A list literal whose elements' common
                // type is a polymorphic base is `Vec<B__>`: each raw-struct/ctor
                // element is wrapped into its enum variant (`[Dog(), Cat()]` ->
                // `vec![Animal__::Dog(..), Animal__::Cat(..)]`). A list of already-
                // `B__` places passes through element-wise. (list+list `+` CONCAT
                // element wrapping stays a documented C2-3 gap — not handled here.)
                if let Some(base) = self.list_poly_base(elems) {
                    let base_ty = Ty::Class(base);
                    let mut parts = Vec::with_capacity(elems.len());
                    for e in elems { parts.push(self.emit_into_base_slot(e, &base_ty)?); }
                    return Ok(format!("vec![{}]", parts.join(", ")));
                }
                // When the literal's unified element type is Float but some
                // elements are int literals (`[1, 2.0]`), cast the int elements
                // to f64 so the vec is a homogeneous `Vec<f64>` (card 5c2f31d8).
                let widen = matches!(self.list_elem_ty(elems), Ty::Float);
                let mut parts = Vec::new();
                for e in elems { parts.push(self.emit_collection_elem(e, widen)?); }
                format!("vec![{}]", parts.join(", "))
            }
            Expr::Tuple(elems, _) => {
                let parts: Result<Vec<_>> = elems.iter().map(|e| self.emit_consuming(e)).collect();
                let parts = parts?;
                match parts.len() {
                    0 => "()".to_string(),
                    1 => format!("({},)", parts[0]),
                    _ => format!("({})", parts.join(", ")),
                }
            }
            Expr::ListComp { elt, targets, iter, cond, .. } => {
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
                // (EPIC-6) Escape each comprehension target in the closure pattern;
                // the elt/cond bodies reference it via emit_expr Ident (same escape).
                // A single target is a bare name; multiple targets (tuple-unpacking,
                // e.g. `[v for k, v in d.items()]`) form a tuple pattern `(k, v)`
                // (mirrors the `Stmt::For` lowering).
                let target = comp_target_pat(targets);
                if let Some(cond_expr) = cond {
                    let cond_s = self.emit_expr(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some({}) }} else {{ None }} ).collect::<Vec<_>>()",
                        chain, target, cond_s, elt_s)
                } else {
                    format!("{}.map(|{}| {}).collect::<Vec<_>>()", chain, target, elt_s)
                }
            }
            Expr::SetComp { elt, targets, iter, cond, .. } => {
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
                // (EPIC-6) Escape the comprehension target(s) (see ListComp above).
                let target = comp_target_pat(targets);
                if let Some(cond_expr) = cond {
                    let cond_s = self.emit_expr(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some({}) }} else {{ None }} ).collect::<::std::collections::HashSet<_>>()",
                        chain, target, cond_s, elt_s)
                } else {
                    format!("{}.map(|{}| {}).collect::<::std::collections::HashSet<_>>()", chain, target, elt_s)
                }
            }
            Expr::DictComp { key, val, targets, iter, cond, .. } => {
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
                // (EPIC-6) Escape the comprehension target(s) (see ListComp above).
                let target = comp_target_pat(targets);
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
                    let ks = self.emit_consuming(k)?;
                    let vs = self.emit_consuming(v)?;
                    inserts.push(format!("({}, {})", ks, vs));
                }
                format!("vec![{}].into_iter().collect::<::std::collections::HashMap<_,_>>()",
                    inserts.join(", "))
            }
            // (EPIC-6) THE central identifier-use emission. Covers a bare
            // variable read AND a free-function call name (a user-fn call falls
            // through to `emit_expr(callee)` here), so escaping once here keeps
            // def and every use in sync. `self` is not a keyword and passes
            // through unchanged (legitimate receiver).
            Expr::Ident(n, _) => {
                // A bare reference to a MODULE CONSTANT emits its MANGLED Rust
                // name (`mangle_const`) — never the bare pyrst name — so the const
                // can't be captured as a Rust const-pattern. A local shadowing the
                // const name keeps the local's value (locals win, matching normal
                // name resolution), so the mangling only applies when `n` is NOT a
                // local. A str const additionally recovers a `String` from its
                // `&str` const.
                if self.const_names.contains(n) && !self.locals.contains_key(n) {
                    if self.const_strs.contains(n) {
                        format!("{}.to_string()", mangle_const(n))
                    } else {
                        mangle_const(n)
                    }
                } else {
                    escape_ident(n)
                }
            }
            Expr::Call { callee, args, kwargs, .. } => {
                self.emit_call(callee, args, kwargs)?
            }
            Expr::Attr { obj, name, .. } => {
                // Qualified MODULE CONSTANT `X.CONST` for a REAL imported module:
                // when X is a tracked module and CONST is one of its module-level
                // constants, lower to the MANGLED Rust `const __pyrst_const_CONST`
                // (the const namespace is flat, mirroring qualified module CALLS;
                // the mangling prevents const-pattern capture). A str const
                // recovers a `String` from its `&str` const. This GENERALIZES the
                // former hardcoded `math.pi`/`math.e`/`math.tau` arm — `math` is
                // now a real embedded module (`lib/math.pyrs`), so its constants
                // flow through here like any other module's.
                if let Expr::Ident(modname, _) = obj.as_ref() {
                    if self
                        .ctx
                        .module_consts
                        .get(modname)
                        .is_some_and(|cs| cs.iter().any(|(c, _)| c == name))
                    {
                        return Ok(if self.const_strs.contains(name) {
                            format!("{}.to_string()", mangle_const(name))
                        } else {
                            mangle_const(name)
                        });
                    }
                }

                let o = self.emit_expr(obj)?;
                // Check if this is a @property access
                let is_property = self.is_property_access(obj, name);
                if is_property {
                    // A @property getter call: the method name (`name`) is a user
                    // method name — escaped so a keyword-named property still
                    // compiles. (Plain field reads below are escaped likewise.)
                    format!("{}.{}()", o, escape_ident(name))
                } else if !matches!(obj.as_ref(),
                                    Expr::Ident(n, _) if n == "self"
                                        || self.concrete_struct_params.contains(n))
                    && matches!(&self.type_of_expr(obj),
                                Ty::Class(b) if self.is_polymorphic_base(b)) {
                    // (EPIC-5 C2-2b-i) FIELD READ through a polymorphic-base var
                    // (a local/param/field whose static type is a polymorphic base).
                    // The receiver is Rust `B__` (an enum with no fields), so a
                    // direct `.{name}` won't compile. Lower to the companion enum's
                    // field-accessor `__field_{name}()` (emitted by
                    // emit_companion_enum for every base field — only base fields
                    // are reachable here; typeck already rejects a derived-only
                    // field on a base var). `self` is EXEMPT: inside a method body
                    // `self` is the concrete struct (`Account`/`Savings`), so
                    // `self.balance` is an ordinary struct-field read. A field-WRITE
                    // through a base var is a deferred honest error (AttrAssign).
                    // The companion-enum accessor is named `__field_<name>` (the
                    // `__field_` prefix makes it a non-keyword), so it is NOT
                    // escaped here — it must match the unescaped accessor emitted
                    // by emit_companion_enum.
                    format!("{}.__field_{}()", o, name)
                } else {
                    // (EPIC-6) Ordinary struct-field read: escape a keyword field
                    // name so it matches the (escaped) struct field definition.
                    format!("{}.{}", o, escape_ident(name))
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
                        // .expect() produces a Rust message without the NUL delimiter;
                        // unwrap_or_else lets us emit a matchable "KeyError\0..." payload.
                        format!("({}.get(&{}).cloned().unwrap_or_else(|| panic!(\"KeyError\\0{{:?}}\", &{})))", o, i, i)
                    }
                    Ty::Str => {
                        // String indexing with negative index support.
                        // Explicit bounds check emits "IndexError\0..." so the
                        // try/except dispatcher can catch it as IndexError.
                        format!(
                            "{{ let __chars: Vec<char> = {}.chars().collect(); let __idx = if {} < 0 {{ ((__chars.len() as i64) + {}) as usize }} else {{ {} as usize }}; if __idx >= __chars.len() {{ panic!(\"IndexError\\0string index out of range\") }}; __chars[__idx].to_string() }}",
                            o, i, i, i
                        )
                    }
                    _ => {
                        // List indexing with negative index support.
                        // Explicit bounds check emits "IndexError\0..." so the
                        // try/except dispatcher can catch it as IndexError.
                        format!(
                            "{{ let __list = {}.clone(); let __idx = if {} < 0 {{ ((__list.len() as i64) + {}) as usize }} else {{ {} as usize }}; if __idx >= __list.len() {{ panic!(\"IndexError\\0list index out of range\") }}; __list[__idx].clone() }}",
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
                    _ => return Err(crate::diag::Error::Codegen(format!("slicing not supported for type {}", obj_ty))),
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
                    // (EPIC-5 C2-3) `list + list` concatenation is a PRE-EXISTING
                    // gap: typeck accepts it, but the generic numeric `+` lowering
                    // below emits `vec![..] + vec![..]`, and Rust's `Vec` has no
                    // `Add` impl — so it leaked a raw rustc E0369 (a miscompile,
                    // for ANY element type, not just subtypes). Refuse honestly
                    // here rather than emit invalid Rust; the documented workaround
                    // is `.extend()` / a comprehension. (Element-wise subtype
                    // wrapping for a base-typed result is the follow-on once concat
                    // itself is implemented.) NOT an EPIC-4 path.
                    if matches!(lt, Ty::List(_)) && matches!(rt, Ty::List(_)) {
                        return Err(crate::diag::Error::Codegen(
                            "list `+` list concatenation is not yet supported — \
                             build the combined list with `.extend()` (e.g. \
                             `xs.extend(ys)`) or a comprehension instead".into(),
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
                // (EPIC-6) Escape each lambda param; the body references it via
                // emit_expr Ident (same escape), so `|r#type| r#type + 1` stays
                // consistent.
                let param_strs: Vec<String> = params.iter()
                    .map(|(name, _ty)| escape_ident(name))
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

    /// Maps a pyrst `Ty` to its emitted Rust type text.
    ///
    /// (EPIC-5 C2-1) This is a `Codegen` METHOD (not a free fn) specifically so
    /// the `Class` arm can consult `self.poly_map` via `is_polymorphic_base` —
    /// the method form avoids threading a `poly_map` parameter through every one
    /// of the call sites (emit_func params/returns, emit_class fields/dunder
    /// impls, emit_stmt hoists). See design §F. C2-1 is BEHAVIOR-PRESERVING: the
    /// `Class` arm still returns plain `n` for every class; the single marked
    /// hook below is what C2-2 flips to `format!("{n}__")` for a polymorphic base.
    fn rust_ty(&self, t: &Ty) -> String {
        match t {
            Ty::Int => "i64".into(),
            Ty::Float => "f64".into(),
            Ty::Bool => "bool".into(),
            Ty::Str => "String".into(),
            Ty::Unit => "()".into(),
            // The `None` literal's type. It never appears as a real binding
            // annotation (annotations come from `from_type_expr`, which yields
            // `Unit`/`Option`, never `NoneVal`); this arm exists for
            // exhaustiveness and mirrors `Unit` (`None` as a bare value is an
            // upstream type error).
            Ty::NoneVal => "()".into(),
            Ty::List(inner) => format!("Vec<{}>", self.rust_ty(inner)),
            Ty::Set(inner) => format!("::std::collections::HashSet<{}>", self.rust_ty(inner)),
            Ty::Dict(k, v) => format!("::std::collections::HashMap<{}, {}>", self.rust_ty(k), self.rust_ty(v)),
            Ty::Tuple(parts) => {
                let inner = parts.iter().map(|p| self.rust_ty(p)).collect::<Vec<_>>().join(", ");
                if parts.len() == 1 {
                    format!("({},)", inner)
                } else {
                    format!("({})", inner)
                }
            }
            Ty::Option(inner) => format!("Option<{}>", self.rust_ty(inner)),
            // A first-class function value lowers to a reference-counted boxed
            // closure `Rc<dyn Fn(A, B) -> R>`. `Rc` is `Clone`, so it round-trips
            // through pyrst's value semantics (clone-on-use = a cheap refcount
            // bump that shares the same callable) and is storable in a list/dict,
            // passable as an argument, and returnable. A `() -> R` return is
            // omitted in Rust only for `()`, but writing `-> ()` is also valid and
            // keeps the formatting uniform.
            Ty::Func(args, ret) => {
                let arg_strs = args.iter().map(|a| self.rust_ty(a)).collect::<Vec<_>>().join(", ");
                format!("::std::rc::Rc<dyn Fn({}) -> {}>", arg_strs, self.rust_ty(ret))
            }
            Ty::Class(n) => {
                // (EPIC-5 C2-2b-i) Polymorphism activation. A class that is a
                // polymorphic base (has ≥1 subclass in this unit) lowers to its
                // companion enum `n__` — emitted by emit_companion_enum with
                // method/field/dunder dispatch — for EVERY param/return/field/
                // var/element position. A leaf or non-subclassed class stays its
                // plain value-struct `n`. The C2-2b-i wrapping (at the 3 former
                // gate sites + list literals) and field-read lowering keep the
                // emitted Rust well-typed against this `n__` slot.
                if self.is_polymorphic_base(n) {
                    format!("{}__", n)
                } else {
                    n.clone()
                }
            }
            Ty::File => "PyFile".into(),
            Ty::Unknown => "()".into(),
        }
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
/// (EPIC-6) Rust keywords that CAN be used as raw identifiers (`r#kw`). A pyrst
/// user name (var / param / field / free-fn) colliding with one of these is
/// escaped so it round-trips through rustc instead of producing a confusing
/// syntax error. The set is intentionally the Rust 2021 keyword set MINUS the
/// four that rustc rejects as raw identifiers (`crate`, `self`, `super`, `Self`
/// — handled by typeck rejection, see `reject_reserved_idents`). The pyrst lexer
/// already reserves the *Python* keywords (`for`, `if`, `class`, `as`, `in`,
/// `with`, `match`, `lambda`, ...), so those can never reach codegen as an
/// identifier; only Rust-only keywords (`type`, `loop`, `fn`, `move`, `let`,
/// `mut`, ...) can. `true`/`false` are NOT pyrst keywords (pyrst spells them
/// `True`/`False`) and rustc *does* accept `r#true`/`r#false`, so they are
/// escaped here rather than rejected.
const RUST_RAW_ESCAPABLE_KEYWORDS: &[&str] = &[
    // strict keywords (2015) that are not pyrst keywords
    "as", "const", "enum", "extern", "fn", "impl", "let", "loop", "mod",
    "move", "mut", "pub", "ref", "static", "struct", "trait", "type", "unsafe",
    "use", "where",
    // `true`/`false` — keywords in Rust, ordinary identifiers in pyrst, and
    // valid as raw identifiers (`r#true`/`r#false`) per rustc 2021.
    "true", "false",
    // 2018+ strict keywords
    "async", "await", "dyn",
    // reserved-for-future keywords (escapable, kept for forward safety)
    "abstract", "become", "box", "do", "final", "macro", "override", "priv",
    "typeof", "unsized", "virtual", "yield", "try",
];

/// (EPIC-6) Escape a USER identifier (var / param / field / free-fn name) so it
/// is a valid Rust identifier. Returns `r#<name>` when `name` is a raw-escapable
/// Rust keyword; the bare name otherwise. This is a NO-OP for every non-keyword
/// identifier (so the 189 existing positive goldens are byte-for-byte
/// unchanged), and must be applied IDENTICALLY at the definition site and at
/// every use of a name (a missed site = def/use mismatch = rustc error).
///
/// `self` is never passed through this for the method receiver (it is emitted
/// verbatim as the Rust receiver); a *user* binding named `self`/`Self`/`super`/
/// `crate` is rejected upstream by typeck, so it never reaches here.
pub fn escape_ident(name: &str) -> String {
    if RUST_RAW_ESCAPABLE_KEYWORDS.contains(&name) {
        format!("r#{}", name)
    } else {
        name.to_string()
    }
}

/// Mangle a MODULE-LEVEL CONSTANT's pyrst name into the Rust identifier emitted
/// for it. Module consts lower to top-level Rust `const` items, and a lowercase
/// const name (e.g. `k`, `i`, `e`) would otherwise be a CONSTANT PATTERN at any
/// pattern position in the generated crate — a closure arg `|(k, v)|`, a
/// `for i in ...` target, a `match` arm binding — silently capturing that name
/// as the const instead of a fresh binding (rustc E0308, a miscompile). The
/// `__pyrst_const_` prefix is reserved (no user/runtime/pattern identifier uses
/// it) so the emitted name can never collide. Applied IDENTICALLY at the const
/// definition AND at every reference (bare `CONST` and qualified `X.CONST`); a
/// missed site is a def/use mismatch. The pyrst-level name is unchanged in
/// typeck and diagnostics — only the emitted Rust identifier is mangled.
pub fn mangle_const(name: &str) -> String {
    format!("__pyrst_const_{}", name)
}

/// Build a comprehension closure-parameter pattern from its loop target(s),
/// escaping each name (EPIC-6). A single target is a bare binding; multiple
/// targets (tuple-unpacking, e.g. `for k, v in d.items()`) form a tuple pattern
/// `(k, v)` — mirroring the `Stmt::For` loop-variable lowering.
fn comp_target_pat(targets: &[String]) -> String {
    if targets.len() == 1 {
        escape_ident(&targets[0])
    } else {
        format!("({})", targets.iter().map(|t| escape_ident(t)).collect::<Vec<_>>().join(", "))
    }
}

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

    // (EPIC-4 V3) Compute the transitive `&mut self` decision for every
    // (class, method) BEFORE any emission, so `emit_func` can consult it. Reads
    // only `ctx.classes` (the resolved MRO), so it is independent of the
    // module-by-module emission order below.
    cg.compute_mut_self();

    // (EPIC-5 C2-1) Build the closed-set polymorphism map (base -> subclasses)
    // BEFORE emission, the same prepass shape as compute_mut_self. C2-1 only
    // CONSULTS it (in `rust_ty`'s Class arm) without changing output; C2-2 flips
    // that hook to emit the companion-enum name for polymorphic bases.
    cg.build_poly_map();

    // Preamble — written once
    cg.line("#![allow(unused_parens, unused_variables, unused_mut, dead_code, unused_imports, non_upper_case_globals)]");
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
    // "ZeroDivisionError\0..." payload matching the try/except dispatcher.
    cg.line("fn __py_mod(a: i64, b: i64) -> i64 {");
    cg.line("    if b == 0 { panic!(\"ZeroDivisionError\\0integer division or modulo by zero\"); }");
    cg.line("    let m = a % b;");
    cg.line("    if m != 0 && ((m < 0) != (b < 0)) { m + b } else { m }");
    cg.line("}");
    // Python integer floor division: floors toward negative infinity.
    // Panics on b==0 with a catchable "ZeroDivisionError\0..." payload.
    // The f64 path previously used here silently returned i64::MAX for x//0.
    cg.line("fn __py_floordiv(a: i64, b: i64) -> i64 {");
    cg.line("    if b == 0 { panic!(\"ZeroDivisionError\\0integer division or modulo by zero\"); }");
    cg.line("    let q = a / b;");
    cg.line("    if (a % b != 0) && ((a % b < 0) != (b < 0)) { q - 1 } else { q }");
    cg.line("}");
    // int() from str: panics with catchable "ValueError\0..." payload
    // instead of Rust's generic unwrap message.
    cg.line("fn __py_int_from_str(s: &str) -> i64 {");
    cg.line("    s.trim().parse::<i64>().unwrap_or_else(|_| panic!(\"ValueError\\0invalid literal for int() with base 10: '{}'\", s.trim()))");
    cg.line("}");
    // float() from str: panics with catchable "ValueError\0..." payload.
    cg.line("fn __py_float_from_str(s: &str) -> f64 {");
    cg.line("    s.trim().parse::<f64>().unwrap_or_else(|_| panic!(\"ValueError\\0could not convert string to float: '{}'\", s.trim()))");
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

    // Module-level CONSTANTS prepass: emit every `NAME: T = <literal>` as a
    // top-level Rust `const` BEFORE any function so a const referenced inside a
    // function (bare `CONST` or qualified `X.CONST`) always resolves, regardless
    // of source order across modules. `emit_top_stmt` treats these assigns as a
    // no-op (they are emitted here), so each const is emitted exactly once.
    for (m, _src) in modules {
        for s in &m.stmts {
            if crate::typeck::is_module_const_decl(s) {
                // Record const names BEFORE emitting bodies so every reference
                // (bare `CONST` / qualified `X.CONST`) lowers to the MANGLED name,
                // and str consts additionally get `.to_string()`.
                if let Stmt::Assign { target, value, .. } = s {
                    cg.const_names.insert(target.clone());
                    if matches!(value, Expr::Str(..)) {
                        cg.const_strs.insert(target.clone());
                    }
                }
                cg.emit_const_decl(s)?;
            }
        }
    }

    // Emit all modules in order (imports first, root last)
    for (m, _src) in modules {
        for s in &m.stmts {
            // Skip import statements — they're resolved, not emitted
            if matches!(s, Stmt::Import { .. }) { continue; }
            cg.emit_top_stmt(s)?;
        }
    }

    // (EPIC-5 C2-2a) Emit the companion-enum machinery (closed-set enum +
    // method-dispatch impl + field-accessor impl) for every polymorphic base,
    // AFTER all value-structs exist. C2-2a emits it as #[allow(dead_code)] and
    // never references it (rust_ty still plain `n`, C1 gate intact), so output is
    // byte-for-byte unchanged; the dead code merely has to compile. C2-2b wires
    // it in.
    cg.emit_companion_enums()?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ClassDef;
    use crate::diag::Span;

    /// A minimal `ClassDef` carrying only name + bases — enough for the poly_map
    /// pre-pass, which reads `ctx.classes` / `bases` via `is_subclass`.
    fn class_def(name: &str, bases: &[&str]) -> ClassDef {
        ClassDef {
            name: name.to_string(),
            bases: bases.iter().map(|s| s.to_string()).collect(),
            fields: vec![],
            methods: vec![],
            is_dataclass: false,
            span: Span::DUMMY,
        }
    }

    /// Build a `TyCtx` populated with the given `(name, bases)` classes.
    fn ctx_with(classes: &[(&str, &[&str])]) -> TyCtx {
        let mut ctx = TyCtx::new();
        for (name, bases) in classes {
            ctx.classes.insert(name.to_string(), class_def(name, bases));
        }
        ctx
    }

    #[test]
    fn poly_map_direct_siblings() {
        // Dog(Animal) + Cat(Animal) -> poly_map[Animal] == {Cat, Dog} (sorted).
        let ctx = ctx_with(&[
            ("Animal", &[]),
            ("Dog", &["Animal"]),
            ("Cat", &["Animal"]),
        ]);
        let mut cg = Codegen::new(&ctx);
        cg.build_poly_map();
        assert_eq!(
            cg.poly_map.get("Animal"),
            Some(&vec!["Cat".to_string(), "Dog".to_string()])
        );
        assert!(cg.is_polymorphic_base("Animal"));
    }

    #[test]
    fn poly_map_subless_class_not_polymorphic() {
        // A class with no subclasses in the unit is NOT polymorphic and has no
        // poly_map entry. A leaf subclass (Dog) is likewise not a base.
        let ctx = ctx_with(&[
            ("Animal", &[]),
            ("Dog", &["Animal"]),
            ("Rock", &[]), // unrelated, sub-less
        ]);
        let mut cg = Codegen::new(&ctx);
        cg.build_poly_map();
        assert!(!cg.is_polymorphic_base("Rock"));
        assert!(cg.poly_map.get("Rock").is_none());
        assert!(!cg.is_polymorphic_base("Dog")); // leaf: no subclasses
        // Animal IS a base (has Dog under it).
        assert!(cg.is_polymorphic_base("Animal"));
        assert_eq!(cg.poly_map.get("Animal"), Some(&vec!["Dog".to_string()]));
    }

    #[test]
    fn poly_map_transitive_chain() {
        // C(B(A)): poly_map[A] must contain BOTH B and C (direct + transitive),
        // poly_map[B] contains C. is_subclass(C, A) drives the transitivity.
        let ctx = ctx_with(&[
            ("A", &[]),
            ("B", &["A"]),
            ("C", &["B"]),
        ]);
        let mut cg = Codegen::new(&ctx);
        cg.build_poly_map();
        let a_subs = cg.poly_map.get("A").expect("A must be a polymorphic base");
        assert!(a_subs.contains(&"B".to_string()));
        assert!(a_subs.contains(&"C".to_string()));
        assert_eq!(a_subs, &vec!["B".to_string(), "C".to_string()]);
        assert_eq!(cg.poly_map.get("B"), Some(&vec!["C".to_string()]));
        assert!(cg.is_polymorphic_base("A"));
        assert!(cg.is_polymorphic_base("B"));
        assert!(!cg.is_polymorphic_base("C")); // leaf
    }

    #[test]
    fn poly_map_empty_before_prepass() {
        // The field is empty until the pre-pass runs (mirrors mut_self).
        let ctx = ctx_with(&[("Animal", &[]), ("Dog", &["Animal"])]);
        let cg = Codegen::new(&ctx);
        assert!(cg.poly_map.is_empty());
        assert!(!cg.is_polymorphic_base("Animal"));
    }

    // ── Emission helpers ──────────────────────────────────────────────────────
    //
    // `emit_src` compiles a snippet through the full pipeline (parse + typeck +
    // codegen) and returns the Rust source string. Use `.contains(...)` — never
    // byte-equality — because HashMap-backed field ordering is non-deterministic.

    fn emit_src(src: &str) -> String {
        let m = crate::parser::parse(src).expect("test snippet must parse");
        let ctx = TyCtx::new();
        emit_program(&[(m, src.to_string())], &ctx)
            .expect("test snippet must emit successfully")
    }

    // ── Preamble helpers are always present ───────────────────────────────────

    #[test]
    fn preamble_contains_ipow_helper() {
        // The preamble is emitted unconditionally; __py_ipow must always be present.
        let src = "def f() -> None:\n    pass\n";
        let out = emit_src(src);
        assert!(out.contains("__py_ipow"), "preamble must define __py_ipow");
    }

    #[test]
    fn preamble_contains_floordiv_helper() {
        let src = "def f() -> None:\n    pass\n";
        let out = emit_src(src);
        assert!(out.contains("__py_floordiv"), "preamble must define __py_floordiv");
    }

    #[test]
    fn preamble_contains_mod_helper() {
        let src = "def f() -> None:\n    pass\n";
        let out = emit_src(src);
        assert!(out.contains("__py_mod"), "preamble must define __py_mod");
    }

    // ── Operator emission ─────────────────────────────────────────────────────

    #[test]
    fn emit_pow_uses_ipow_helper() {
        // x ** 2 must lower to the __py_ipow helper call in the output.
        let src = "def f(x: int) -> int:\n    y: int = x ** 2\n    return y\n";
        let out = emit_src(src);
        assert!(out.contains("__py_ipow"), "** operator must emit __py_ipow");
    }

    #[test]
    fn emit_floordiv_uses_floordiv_helper() {
        // a // b must lower to the __py_floordiv helper call.
        let src = "def f(a: int, b: int) -> int:\n    c: int = a // b\n    return c\n";
        let out = emit_src(src);
        assert!(out.contains("__py_floordiv"), "// operator must emit __py_floordiv");
    }

    #[test]
    fn emit_mod_uses_mod_helper() {
        // a % b must lower to the __py_mod helper call.
        let src = "def f(a: int, b: int) -> int:\n    c: int = a % b\n    return c\n";
        let out = emit_src(src);
        assert!(out.contains("__py_mod"), "% operator must emit __py_mod");
    }

    #[test]
    fn emit_augassign_pow_uses_ipow_helper() {
        // x **= 2 is an aug-assign; the emitted Rust must still use __py_ipow.
        let src = "def f(x: int) -> int:\n    x **= 2\n    return x\n";
        let out = emit_src(src);
        assert!(out.contains("__py_ipow"), "**= aug-assign must emit __py_ipow");
    }

    // ── F-string emission ─────────────────────────────────────────────────────

    #[test]
    fn emit_fstring_uses_format_macro() {
        // f"hello {name}" must lower to a Rust format! call.
        let src = "def f(name: str) -> str:\n    s: str = f\"hello {name}\"\n    return s\n";
        let out = emit_src(src);
        assert!(out.contains("format!"), "f-string must emit Rust format! macro");
    }

    // ── Type emission ─────────────────────────────────────────────────────────

    #[test]
    fn emit_int_type_becomes_i64() {
        // A function returning int must annotate with i64 in the Rust signature.
        let src = "def f(x: int) -> int:\n    return x\n";
        let out = emit_src(src);
        assert!(out.contains("i64"), "int type must emit as i64");
    }

    #[test]
    fn emit_str_type_becomes_string() {
        // A function returning str must annotate with String.
        let src = "def f(x: str) -> str:\n    return x\n";
        let out = emit_src(src);
        assert!(out.contains("String"), "str type must emit as String");
    }

    #[test]
    fn emit_bool_type_becomes_bool() {
        // A function returning bool must annotate with bool.
        let src = "def f(x: bool) -> bool:\n    return x\n";
        let out = emit_src(src);
        assert!(out.contains("bool"), "bool type must emit as bool");
    }

    // ── List comprehension emission ───────────────────────────────────────────

    #[test]
    fn emit_list_comp_uses_iterator_pattern() {
        // [x * 2 for x in xs] must lower to an iterator chain (.map or .collect).
        let src = "def f(xs: list[int]) -> list[int]:\n    result: list[int] = [x * 2 for x in xs]\n    return result\n";
        let out = emit_src(src);
        assert!(
            out.contains(".map(") || out.contains(".collect()") || out.contains("collect::<"),
            "list comprehension must emit an iterator map/collect pattern"
        );
    }

    #[test]
    fn rust_ty_class_arm_polymorphism_activated() {
        // C2-2b-i acceptance: rust_ty(Class(n)) emits the companion-enum name
        // `n__` for a POLYMORPHIC base (a class with ≥1 subclass), and the plain
        // value-struct name `n` for a leaf / sub-less class. (C2-1 used to return
        // plain `n` for both; the keystone flips the polymorphic branch.)
        let ctx = ctx_with(&[("Animal", &[]), ("Dog", &["Animal"]), ("Rock", &[])]);
        let mut cg = Codegen::new(&ctx);
        cg.build_poly_map();
        assert!(cg.is_polymorphic_base("Animal"));
        // Polymorphic base -> companion enum.
        assert_eq!(cg.rust_ty(&Ty::Class("Animal".into())), "Animal__");
        // Sub-less / leaf classes stay their plain value-struct name.
        assert_eq!(cg.rust_ty(&Ty::Class("Rock".into())), "Rock");
        assert_eq!(cg.rust_ty(&Ty::Class("Dog".into())), "Dog");
        // A list of a polymorphic base is Vec<Animal__> (the element type flips too).
        assert_eq!(
            cg.rust_ty(&Ty::List(Box::new(Ty::Class("Animal".into())))),
            "Vec<Animal__>"
        );
    }

    // ── @extern (Rust-FFI binding) emission ───────────────────────────────────

    #[test]
    fn extern_emits_substituted_template_as_tail_expr() {
        // An @extern function emits the signature built from its declared types
        // plus the template string with each `{param}` substituted for the Rust
        // param identifier, as the function's tail expression.
        let src = "\
@extern
def shout(s: str) -> str:
    \"{s}.to_uppercase()\"

@extern
def repeat_str(s: str, n: int) -> str:
    \"{s}.repeat({n} as usize)\"

@extern
def ipow(base: int, exp: int) -> int:
    \"({base}).pow({exp} as u32)\"
";
        let out = emit_src(src);
        // Signature uses the rust_ty mapping (Str -> String, Int -> i64).
        assert!(out.contains("fn shout(mut s: String) -> String {"),
            "extern signature must reuse the normal type mapping; got:\n{}", out);
        // The `{s}` hole is substituted with the emitted param identifier.
        assert!(out.contains("s.to_uppercase()"),
            "template `{{s}}` must be substituted to `s.to_uppercase()`; got:\n{}", out);
        // Multi-hole template: both holes substituted, author glue preserved.
        assert!(out.contains("s.repeat(n as usize)"),
            "multi-hole template must substitute both params; got:\n{}", out);
        assert!(out.contains("(base).pow(exp as u32)"),
            "ipow template must substitute base/exp; got:\n{}", out);
        // The unsubstituted brace form must NOT survive into the emitted Rust.
        assert!(!out.contains("{s}.to_uppercase()"),
            "the literal `{{s}}` hole must not leak into output; got:\n{}", out);
    }

    // ── Qualified module calls — `import X; X.f(args)` (card 81db88e0) ─────────

    /// A `TyCtx` modeling `import os`: the flat `basename` signature is in
    /// `ctx.funcs`, and `module_funcs["os"]` lists it (resolver-equivalent).
    fn ctx_with_os() -> TyCtx {
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("basename".into(), crate::typeck::FuncSig {
            params: vec![("p".into(), Ty::Str)],
            param_defaults: vec![None],
            param_by_ref: vec![],
            ret: Ty::Str,
        });
        ctx.module_funcs.insert("os".into(), vec!["basename".into()]);
        ctx
    }

    #[test]
    fn qualified_module_call_lowers_to_flat_call() {
        // `os.basename("/a/b.txt")` must lower to the FLAT Rust call
        // `basename("/a/b.txt".to_string())` — the module qualifier is dropped
        // (every imported module's functions are merged flat) and the call goes
        // through the regular function-call path (string literal owned via
        // `.to_string()`), exactly as `from os import basename; basename(...)`.
        let ctx = ctx_with_os();
        let mut cg = Codegen::new(&ctx);
        let callee: Box<Expr> = Box::new(Expr::Attr {
            obj: Box::new(Expr::Ident("os".into(), Span::DUMMY)),
            name: "basename".into(),
            span: Span::DUMMY,
        });
        let args = vec![Expr::Str("/a/b.txt".into(), Span::DUMMY)];
        let out = cg.emit_method_call_on_attr(&callee, &args)
            .expect("emit must succeed")
            .expect("a tracked module call must be handled by emit_method_call_on_attr");
        assert!(out.starts_with("basename("),
            "module qualifier must be dropped, emitting a flat call; got: {}", out);
        assert!(!out.contains("os"),
            "the `os` qualifier must not appear in the emitted call; got: {}", out);
    }

    #[test]
    fn math_qualified_call_lowers_to_flat_call() {
        // `math` is now a REAL embedded module: `math.sqrt(x)` flows through the
        // GENERAL qualified-module path and lowers to the FLAT Rust call
        // `sqrt((16.0f64))` (the `@extern` `sqrt` wrapper is merged flat) — the
        // former hardcoded math arm is gone, so the `math` qualifier is dropped
        // exactly like any other module's call.
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("sqrt".into(), crate::typeck::FuncSig {
            params: vec![("x".into(), Ty::Float)],
            param_defaults: vec![None],
            param_by_ref: vec![],
            ret: Ty::Float,
        });
        ctx.module_funcs.insert("math".into(), vec!["sqrt".into()]);
        let mut cg = Codegen::new(&ctx);
        let callee: Box<Expr> = Box::new(Expr::Attr {
            obj: Box::new(Expr::Ident("math".into(), Span::DUMMY)),
            name: "sqrt".into(),
            span: Span::DUMMY,
        });
        let args = vec![Expr::Float(16.0, Span::DUMMY)];
        let out = cg.emit_method_call_on_attr(&callee, &args)
            .expect("emit must succeed")
            .expect("a tracked module call must be handled by emit_method_call_on_attr");
        assert!(out.starts_with("sqrt("),
            "module qualifier must be dropped, emitting a flat call; got: {}", out);
        assert!(!out.contains("math"),
            "the `math` qualifier must not appear in the emitted call; got: {}", out);
    }

    #[test]
    fn module_constant_lowers_to_mangled_const_name() {
        // A qualified module constant `math.pi` (a non-call attribute) lowers to
        // the MANGLED const name `__pyrst_const_pi` (the prepass emits a top-level
        // `const __pyrst_const_pi: f64`). Mangling prevents a lowercase const from
        // being captured as a Rust const-pattern. The former hardcoded
        // `::std::f64::consts::PI` arm is gone.
        let mut ctx = TyCtx::new();
        ctx.module_consts.insert("math".into(), vec![("pi".into(), Ty::Float)]);
        let mut cg = Codegen::new(&ctx);
        let attr = Expr::Attr {
            obj: Box::new(Expr::Ident("math".into(), Span::DUMMY)),
            name: "pi".into(),
            span: Span::DUMMY,
        };
        let out = cg.emit_expr(&attr).expect("emit must succeed");
        assert_eq!(out, "__pyrst_const_pi", "math.pi must lower to the mangled const name; got: {}", out);
    }

    #[test]
    fn module_const_decl_emits_mangled_top_level_const() {
        // A module-level `NAME: T = <literal>` emits a top-level Rust `const` with
        // a MANGLED name (`__pyrst_const_<name>`). int/float/bool are typed Copy
        // consts; a str const is a `&str` const. The bare reference `print(PI)`
        // also uses the mangled name.
        let src = "\
PI: float = 3.14
COUNT: int = 7
GREETING: str = \"hi\"
FLAG: bool = True

def main() -> None:
    print(PI)
";
        let out = emit_src(src);
        assert!(out.contains("const __pyrst_const_PI: f64 = 3.14f64;"), "float const; got:\n{}", out);
        assert!(out.contains("const __pyrst_const_COUNT: i64 = 7;"), "int const; got:\n{}", out);
        assert!(out.contains("const __pyrst_const_GREETING: &str = \"hi\";"), "str const; got:\n{}", out);
        assert!(out.contains("const __pyrst_const_FLAG: bool = true;"), "bool const; got:\n{}", out);
        // The bare reference resolves to the mangled name too (def/use match).
        assert!(out.contains("__pyrst_const_PI"), "bare ref uses mangled name; got:\n{}", out);
    }

    #[test]
    fn lowercase_const_does_not_capture_pattern_var() {
        // Regression: a lowercase module const `i` alongside `for i in range(3)`.
        // The const is emitted MANGLED (so it can't be a const-pattern), the loop
        // var `i` is a FRESH binding inside the loop, and the const read AFTER the
        // loop resolves back to the mangled const (the loop var does not leak).
        let src = "\
i: int = 99

def main() -> None:
    for i in range(3):
        print(i)
    print(i)
";
        let out = emit_src(src);
        // The const is mangled at its definition.
        assert!(out.contains("const __pyrst_const_i: i64 = 99;"),
            "const i emitted mangled; got:\n{}", out);
        // The loop target is the bare `i` (a fresh Rust binding), and the body
        // prints that bare loop var — NOT the mangled const.
        assert!(out.contains("for i in"), "loop target is bare i; got:\n{}", out);
        assert!(out.contains("println!(\"{}\" , i)"),
            "in-loop reference is the loop var (bare i); got:\n{}", out);
        // The post-loop read resolves to the mangled const (loop var out of scope).
        assert!(out.contains("println!(\"{}\" , __pyrst_const_i)"),
            "post-loop reference is the mangled const; got:\n{}", out);
    }
}
