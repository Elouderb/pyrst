use super::*;

// Local scope during function body type checking.
pub(crate) struct FuncEnv<'a> {
    pub(crate) ctx: &'a TyCtx,
    pub(crate) locals: HashMap<String, Ty>,
    pub(crate) ret_ty: Ty,
    pub(crate) used_vars: std::collections::HashSet<String>,  // Track variable usage for dead code detection
    /// Names that were original function/method parameters (never changes after construction).
    pub(crate) params: std::collections::HashSet<String>,
    /// Subset of `params` that have been unconditionally reassigned via `Stmt::Assign`.
    /// A param in this set is no longer the original by-value binding.
    pub(crate) reassigned_params: std::collections::HashSet<String>,
    /// Subset of `params` whose name appears (as an Ident) in at least one `return` expression
    /// anywhere in the function body. Mutating and returning a by-value param is the valid
    /// functional pattern — the callee works on its own copy and returns the result; the caller
    /// captures the new value. We suppress the by-value-param-mutation error for these params.
    pub(crate) returned_params: std::collections::HashSet<String>,
    /// EPIC-4 V2: subset of `params` declared `Mut[T]` (by-reference). A by-ref
    /// param's mutation IS visible to the caller, so the by-value mutation
    /// backstop (AttrAssign / IndexAssign / mutating method-call) must NOT fire
    /// for these names.
    pub(crate) by_ref_params: std::collections::HashSet<String>,
    /// Generators: true when the function being checked has a `yield` in its
    /// body. A generator MUST be declared `Iterator[T]` (so `ret_ty` is the
    /// `Ty::Iterator(T)` that `Iterator[T]` lowers to — LAZY-GEN V1-a). When set, a `yield x` checks
    /// `x` against the element type `T`, a bare `return` is allowed even though
    /// `ret_ty` is not `Unit` (it ends collection early), and a `return <value>`
    /// is rejected (generators yield values, they do not return one).
    pub(crate) is_generator: bool,
    /// Generics v1: the enclosing function's declared type-parameter names. A
    /// param bound to a `Ty::TypeVar` in this set is OPAQUE inside the body —
    /// `reject_typevar_op` turns any operation on it that needs a trait bound
    /// (arithmetic, comparison, `print`, calling a method, ...) into an honest
    /// error. Empty for every non-generic function/method/lambda.
    pub(crate) type_params: std::collections::HashSet<String>,
}

impl<'a> FuncEnv<'a> {
    /// Build a function-checking environment. `by_ref_names` is the set of
    /// parameter names declared `Mut[T]` (empty for lambdas, test helpers, and
    /// any function with no by-reference params).
    pub(crate)     fn with_by_ref(ctx: &'a TyCtx, params: &[(String, Ty)], by_ref_names: &[String], ret_ty: Ty) -> Self {
        let mut locals = HashMap::new();
        let mut used_vars = std::collections::HashSet::new();
        let mut param_set = std::collections::HashSet::new();
        for (name, ty) in params {
            locals.insert(name.clone(), ty.clone());
            used_vars.insert(name.clone());  // Parameters are always considered "used"
            param_set.insert(name.clone());
        }
        let by_ref_params = by_ref_names.iter().cloned().collect();
        FuncEnv { ctx, locals, ret_ty, used_vars, params: param_set, reassigned_params: std::collections::HashSet::new(), returned_params: std::collections::HashSet::new(), by_ref_params, is_generator: false, type_params: std::collections::HashSet::new() }
    }

    /// The enclosing generic function's declared type-parameter names as a
    /// `Vec<String>` for `from_type_expr_scoped`. Empty for non-generic
    /// functions (so scoped lowering there is identical to the plain path).
    pub(crate)     fn type_param_list(&self) -> Vec<String> {
        self.type_params.iter().cloned().collect()
    }

    pub(crate)     fn lookup(&self, name: &str) -> Option<Ty> {
        self.locals.get(name).cloned()
            .or_else(|| self.ctx.vars.get(name).cloned())
            // A bare reference to a top-level function NAME (used as a value, not
            // a call) resolves to its first-class function type `Ty::Func`. The
            // CALL paths look the signature up directly (Call arm / emit_call)
            // and never reach here for a name they recognize, so this only fires
            // when the name appears in a value position (`g = inc`, `apply(inc)`,
            // `{"k": inc}`). Builtins with a synthetic sig (print/len/...) are
            // included; that is harmless — they are never used as values in the
            // corpus, and a call still routes through the dedicated builtin arms.
            .or_else(|| self.ctx.funcs.get(name).map(func_sig_to_ty))
            .or_else(|| {
                if self.ctx.classes.contains_key(name) {
                    Some(Ty::Class(name.to_string(), vec![]))
                } else {
                    None
                }
            })
    }
}

/// Build the first-class function type `Ty::Func(arg_types, ret)` for a resolved
/// function signature — the type of the function NAME when used as a value.
pub(crate) fn func_sig_to_ty(sig: &FuncSig) -> Ty {
    Ty::Func(
        sig.params.iter().map(|(_, t)| t.clone()).collect(),
        Box::new(sig.ret.clone()),
    )
}

/// Validate that every decorator name in `decorators` is in the supported whitelist.
/// Returns an error pointing at `span` for the first unsupported decorator found.
pub(crate) fn validate_decorators(decorators: &[String], span: Span) -> Result<()> {
    for dec in decorators {
        match dec.as_str() {
            // `extern` declares a Rust-FFI binding (a bare `@extern` decorator
            // over a `def` whose body is a single Rust-expression-template string
            // literal). The body/typing of an `@extern` function are validated
            // separately by `validate_extern_func`; here we only admit the name.
            //
            // `crate` (Rust interop Phase 2) declares an external-crate dependency
            // via `@crate("name", "version")`. It is pure build metadata with no
            // body effect — the parser has already validated its two string-literal
            // args and recorded them in `Func::crate_deps`; here we only admit the
            // name so it is not rejected as unknown.
            "staticmethod" | "property" | "dataclass" | "extern" | "crate" => {}
            _ => {
                return Err(Error::Type {
                    span,
                    msg: format!("decorator `@{}` is not supported", dec),
                });
            }
        }
    }
    Ok(())
}

/// Validate a function carrying the `@extern` decorator (a Rust-FFI binding).
///
/// Phase 1 (std-only) contract — the binding AUTHOR declares the full boundary,
/// because codegen cannot infer the Rust-side glue:
///   (a) the body is EXACTLY ONE statement and it is a string literal — the Rust
///       expression TEMPLATE with `{param}` substitution holes;
///   (b) every (non-`self`) parameter AND the return type lower to a concrete,
///       fully-known `Ty` (not `Ty::Unknown`); and
///   (c) no parameter uses the by-reference `Mut[T]` mode (out of Phase-1 scope —
///       template substitution emits params by value).
///
/// The TEMPLATE CONTENTS are deliberately NOT type-checked here: the string is
/// opaque Rust (the FFI escape hatch), so a malformed template surfaces as a
/// rustc error at `build` time, not a pyrst typeck error. The function's declared
/// signature still registers in the ctx like any `def`, so CALL sites type-check
/// through the normal path with no special-casing.
pub(crate) fn validate_extern_func(f: &Func, ctx: &TyCtx) -> Result<()> {
    // (a) body must be a single string-literal statement (the template).
    let single_str = matches!(f.body.as_slice(), [Stmt::Expr(Expr::Str(_, _))]);
    if !single_str {
        return Err(Error::Type {
            span: f.span,
            msg: "`@extern` function body must be a single PLAIN string literal — \
                  the Rust expression template with `{param}` holes (not an f-string; \
                  use a regular string and pyrst fills the `{param}` holes)"
                .to_string(),
        });
    }

    // (c) by-reference (`Mut[T]`) params are out of Phase-1 @extern scope.
    if let Some(p) = f.params.iter().find(|p| p.by_ref) {
        return Err(Error::Type {
            span: f.span,
            msg: format!(
                "`@extern` does not support the by-reference parameter `{}` \
                 (`Mut[T]`); declare it by value",
                p.name
            ),
        });
    }

    // (b) every non-self param + the return type must be fully typed (the parser
    // already forces an annotation on each, so the only residual gap is a user
    // annotation that lowers to `Ty::Unknown`, e.g. a multi-arm `Union`).
    for p in f.params.iter().filter(|p| p.name != "self") {
        let ty = Ty::from_type_expr(&p.ty, p.span)?;
        if matches!(ty, Ty::Unknown) {
            return Err(Error::Type {
                span: f.span,
                msg: format!(
                    "`@extern` requires fully-typed params and return: parameter \
                     `{}` has an unresolved type",
                    p.name
                ),
            });
        }
    }
    let ret = Ty::from_type_expr(&f.ret, f.span)?;
    if matches!(ret, Ty::Unknown) {
        return Err(Error::Type {
            span: f.span,
            msg: "`@extern` requires fully-typed params and return: the return \
                  type is unresolved"
                .to_string(),
        });
    }

    // `ctx` is accepted for symmetry with the other per-function checks and to
    // keep the door open for future cross-checks; Phase 1 needs no ctx lookups.
    let _ = ctx;
    Ok(())
}

/// Return a best-effort `Span` for a statement, used for error reporting.
pub(crate) fn stmt_span(s: &Stmt) -> Span {
    match s {
        Stmt::Expr(e) => e.span(),
        Stmt::Assign { span, .. }
        | Stmt::AugAssign { span, .. }
        | Stmt::Unpack { span, .. }
        | Stmt::If { span, .. }
        | Stmt::While { span, .. }
        | Stmt::For { span, .. }
        | Stmt::Assert { span, .. }
        | Stmt::Raise { span, .. }
        | Stmt::Try { span, .. }
        | Stmt::With { span, .. }
        | Stmt::Del { span, .. }
        | Stmt::Match { span, .. }
        | Stmt::AttrAssign { span, .. }
        | Stmt::IndexAssign { span, .. }
        | Stmt::Import { span, .. } => *span,
        Stmt::Return(_, span) | Stmt::Yield(_, span) | Stmt::Pass(span) | Stmt::Break(span) | Stmt::Continue(span) => *span,
        Stmt::Func(f) => f.span,
        Stmt::Class(c) => c.span,
    }
}

/// Return true if `s` is a bare top-level call to `main()` with no arguments —
/// the conventional pyrst entry-point idiom.  The Rust `fn main()` emitted by
/// `emit_program` already calls `user_main()`, so this call is a recognised
/// no-op that must be silently accepted to keep existing positive examples green.
pub(crate) fn is_bare_main_call(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::Expr(Expr::Call { callee, args, kwargs, .. })
            if matches!(callee.as_ref(), Expr::Ident(name, _) if name == "main")
                && args.is_empty()
                && kwargs.is_empty()
    )
}

/// Type-check function/class bodies against a pre-built context.
/// Used for multi-file compilation where the context is merged from all modules.
/// (EPIC-6) Rust keywords that CANNOT be raw identifiers — `r#crate` / `r#self`
/// / `r#super` / `r#Self` are rejected by rustc (verified against rustc 2021).
/// A pyrst USER identifier (var / param / field / free-fn / comprehension or
/// lambda target / except-as / with-as binding) colliding with one of these
/// would have to be mangled to compile, so we reject it HONESTLY at typeck (an
/// honest pyrst diagnostic beats a confusing rustc error or a silent mangle).
/// All OTHER Rust keywords are escapable (`r#type`, `r#loop`, ...) and are
/// handled transparently by codegen's `escape_ident`. NOTE: `self` here is a
/// *user* binding named `self` — the legitimate method receiver `self` (the
/// first parameter of a method) is recognized and exempted below.
pub(crate) const RUST_NON_RAW_KEYWORDS: &[&str] = &["crate", "self", "super", "Self"];

/// Reserved codegen identifier prefixes. The compiler lowers several internal
/// constructs to Rust identifiers under the `__pyrst_` namespace: module-level
/// constants become `const __pyrst_const_<name>` (see codegen's `mangle_const`),
/// and a generator's coroutine locals are `__pyrst_gen_slot`/`_co`/`_fut` (see
/// codegen's `emit_func`). The always-emitted runtime prelude additionally
/// defines helpers under the `__py_` namespace (`__py_mod`, `__py_floordiv`,
/// `__py_list_get`, …) and the lazy-generator runtime TYPES under the `__Pyrst`
/// namespace (`__PyrstGen`/`__PyrstCo`/`__PyrstYieldNow`, see the GEN_PRELUDE).
/// A USER identifier sharing any of these prefixes could collide with a
/// generated name — a `__pyrst_` clash can silently miscompile (e.g. a
/// generator local named `__pyrst_gen_slot`), a `__py_` clash duplicates a
/// prelude `fn`, and a `__Pyrst` clash (e.g. a user `class __PyrstGen`)
/// duplicates a prelude `struct` (all rustc E0428). The `__Pyrst` case-variant
/// is listed SEPARATELY because pyrst class/type names are conventionally
/// capitalized, so a colliding user type would not match the lowercase
/// `__pyrst_` prefix. All three prefixes are reserved for compiler-generated
/// names and rejected honestly at typeck rather than risking a clash. (No real
/// program uses these prefixes; they exist only to make the lowering
/// collision-proof and to keep future internals safe by construction.)
pub(crate) const RESERVED_CODEGEN_PREFIXES: &[&str] = &["__pyrst_", "__py_", "__Pyrst"];

pub(crate) fn reject_if_reserved(name: &str, span: Span, role: &str) -> Result<()> {
    if RUST_NON_RAW_KEYWORDS.contains(&name) {
        return Err(Error::Type {
            span,
            msg: format!(
                "`{}` cannot be used as a {} name: it is a Rust keyword that has no \
                 raw-identifier form (`r#{}` is rejected by rustc), so pyrst cannot \
                 lower it. Rename it (other Rust keywords like `type`/`loop` are \
                 escaped automatically and need no change).",
                name, role, name
            ),
        });
    }
    for prefix in RESERVED_CODEGEN_PREFIXES {
        if name.starts_with(prefix) {
            return Err(Error::Type {
                span,
                msg: format!(
                    "`{}` cannot be used as a {} name: the `{}` prefix is reserved for \
                     compiler-generated identifiers (e.g. module-constant lowering, \
                     generator coroutine locals, runtime helpers like `__py_list_get`, \
                     and the lazy-generator runtime types `__PyrstGen`/`__PyrstCo`/\
                     `__PyrstYieldNow`). Rename it.",
                    name, role, prefix
                ),
            });
        }
    }
    Ok(())
}

/// Walk a statement body and reject any local binding whose name is a non-raw
/// Rust keyword (the same honest rejection applied to params/fields/fns at the
/// top level). Covers `=` / `:` assignment targets, tuple-unpack targets, for
/// loop variables, `with ... as`, `except ... as`, and the binding targets of
/// comprehensions / lambdas reachable through expressions.
pub(crate) fn reject_reserved_in_body(stmts: &[Stmt]) -> Result<()> {
    for s in stmts {
        match s {
            Stmt::Assign { target, value, span, .. }
            | Stmt::AugAssign { target, value, span, .. } => {
                reject_if_reserved(target, *span, "variable")?;
                reject_reserved_in_expr(value)?;
            }
            Stmt::Unpack { targets, value, span } => {
                for t in targets { reject_if_reserved(t, *span, "variable")?; }
                reject_reserved_in_expr(value)?;
            }
            Stmt::For { targets, iter, body, span } => {
                for t in targets { reject_if_reserved(t, *span, "loop variable")?; }
                reject_reserved_in_expr(iter)?;
                reject_reserved_in_body(body)?;
            }
            Stmt::While { cond, body, .. } => {
                reject_reserved_in_expr(cond)?;
                reject_reserved_in_body(body)?;
            }
            Stmt::If { cond, then, elifs, else_, .. } => {
                reject_reserved_in_expr(cond)?;
                reject_reserved_in_body(then)?;
                for (c, b) in elifs {
                    reject_reserved_in_expr(c)?;
                    reject_reserved_in_body(b)?;
                }
                if let Some(b) = else_ { reject_reserved_in_body(b)?; }
            }
            Stmt::With { ctx_expr, as_name, body, span } => {
                reject_reserved_in_expr(ctx_expr)?;
                if let Some(n) = as_name { reject_if_reserved(n, *span, "variable")?; }
                reject_reserved_in_body(body)?;
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                reject_reserved_in_body(body)?;
                for h in handlers {
                    if let Some(n) = &h.exc_name {
                        reject_if_reserved(n, h.span, "variable")?;
                    }
                    reject_reserved_in_body(&h.body)?;
                }
                if let Some(b) = else_ { reject_reserved_in_body(b)?; }
                if let Some(b) = finally_ { reject_reserved_in_body(b)?; }
            }
            Stmt::Match { subject, arms, .. } => {
                reject_reserved_in_expr(subject)?;
                for arm in arms {
                    if let Some(g) = &arm.guard { reject_reserved_in_expr(g)?; }
                    reject_reserved_in_body(&arm.body)?;
                }
            }
            Stmt::Return(Some(e), _) | Stmt::Expr(e) | Stmt::Del { target: e, .. } => {
                reject_reserved_in_expr(e)?;
            }
            Stmt::Assert { cond, msg, .. } => {
                reject_reserved_in_expr(cond)?;
                if let Some(m) = msg { reject_reserved_in_expr(m)?; }
            }
            Stmt::Raise { exc, .. } => {
                if let Some(e) = exc { reject_reserved_in_expr(e)?; }
            }
            Stmt::AttrAssign { obj, value, .. } => {
                reject_reserved_in_expr(obj)?;
                reject_reserved_in_expr(value)?;
            }
            Stmt::IndexAssign { obj, idx, value, .. } => {
                reject_reserved_in_expr(obj)?;
                reject_reserved_in_expr(idx)?;
                reject_reserved_in_expr(value)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Reject a comprehension / lambda binding target inside an expression. Only the
/// BINDING positions matter (a non-raw keyword used as a plain `Expr::Ident`
/// READ never resolves to a real var — name resolution already rejects an
/// undefined name — so we only police the introducing positions here).
pub(crate) fn reject_reserved_in_expr(e: &Expr) -> Result<()> {
    match e {
        Expr::ListComp { elt, targets, iter, cond, span }
        | Expr::SetComp { elt, targets, iter, cond, span } => {
            for target in targets { reject_if_reserved(target, *span, "comprehension variable")?; }
            reject_reserved_in_expr(elt)?;
            reject_reserved_in_expr(iter)?;
            if let Some(c) = cond { reject_reserved_in_expr(c)?; }
        }
        Expr::DictComp { key, val, targets, iter, cond, span } => {
            for target in targets { reject_if_reserved(target, *span, "comprehension variable")?; }
            reject_reserved_in_expr(key)?;
            reject_reserved_in_expr(val)?;
            reject_reserved_in_expr(iter)?;
            if let Some(c) = cond { reject_reserved_in_expr(c)?; }
        }
        Expr::Lambda { params, body, span } => {
            for (n, _) in params { reject_if_reserved(n, *span, "lambda parameter")?; }
            reject_reserved_in_expr(body)?;
        }
        Expr::Call { callee, args, kwargs, .. } => {
            reject_reserved_in_expr(callee)?;
            for a in args { reject_reserved_in_expr(a)?; }
            for (_, v) in kwargs { reject_reserved_in_expr(v)?; }
        }
        Expr::Attr { obj, .. } => reject_reserved_in_expr(obj)?,
        Expr::Index { obj, idx, .. } => {
            reject_reserved_in_expr(obj)?;
            reject_reserved_in_expr(idx)?;
        }
        Expr::Slice { obj, start, stop, step, .. } => {
            reject_reserved_in_expr(obj)?;
            if let Some(x) = start { reject_reserved_in_expr(x)?; }
            if let Some(x) = stop { reject_reserved_in_expr(x)?; }
            if let Some(x) = step { reject_reserved_in_expr(x)?; }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            reject_reserved_in_expr(lhs)?;
            reject_reserved_in_expr(rhs)?;
        }
        Expr::UnOp { expr, .. } => reject_reserved_in_expr(expr)?,
        Expr::IfExp { test, body, orelse, .. } => {
            reject_reserved_in_expr(test)?;
            reject_reserved_in_expr(body)?;
            reject_reserved_in_expr(orelse)?;
        }
        Expr::List(items, _) | Expr::Tuple(items, _) | Expr::Set(items, _) => {
            for it in items { reject_reserved_in_expr(it)?; }
        }
        Expr::Dict(pairs, _) => {
            for (k, v) in pairs {
                reject_reserved_in_expr(k)?;
                reject_reserved_in_expr(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// (EPIC-6) Reject every USER identifier whose name is a non-raw Rust keyword
/// (`crate`/`self`/`super`/`Self`) BEFORE body type-checking, so both `check`
/// and `build` fail honestly. The method receiver `self` is exempt (it is the
/// conventional receiver, emitted verbatim as the Rust `&self`).
pub(crate) fn reject_reserved_idents(m: &Module) -> Result<()> {
    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
                reject_if_reserved(&f.name, f.span, "function")?;
                for p in &f.params {
                    reject_if_reserved(&p.name, p.span, "parameter")?;
                }
                reject_reserved_in_body(&f.body)?;
            }
            Stmt::Class(c) => {
                // The class NAME lowers to a Rust `struct`/`enum` of the same name,
                // so it must not collide with a reserved compiler-generated type
                // (notably the `__Pyrst`-prefixed lazy-generator runtime structs).
                reject_if_reserved(&c.name, c.span, "class")?;
                for field in &c.fields {
                    reject_if_reserved(&field.name, field.span, "field")?;
                }
                for method in &c.methods {
                    // A method's first param `self` is the legitimate receiver and
                    // is exempt; every other param/binding is policed.
                    for (i, p) in method.params.iter().enumerate() {
                        let is_receiver = i == 0 && p.name == "self";
                        if !is_receiver {
                            reject_if_reserved(&p.name, p.span, "parameter")?;
                        }
                    }
                    reject_reserved_in_body(&method.body)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn check_bodies(m: &Module, ctx: &TyCtx) -> Result<()> {
    // (EPIC-6) Reject non-raw-keyword user identifiers up front (honest in both
    // `check` and `build`). Escapable Rust keywords (`type`, `loop`, ...) are
    // accepted here and lowered via codegen's `escape_ident`.
    reject_reserved_idents(m)?;

    // Second pass: type-check each top-level item's body, fail-fast (first
    // error stops the pass). The per-item work lives in `check_one_stmt` so the
    // collecting entry point `check_all` can reuse it without changing this
    // function's observable first-error-and-stop behavior (the CLI exit codes,
    // EPIC-8 multi-file sourcing, and the 64 negative fixtures depend on it).
    for s in &m.stmts {
        check_one_stmt(s, ctx)?;
    }
    Ok(())
}

/// Collect EVERY top-level-item type error in `m` instead of stopping at the
/// first (EPIC-LSP L4). Used by the LSP layer so the language server can surface
/// one squiggle per failing top-level function / method rather than a single
/// diagnostic per edit.
///
/// Semantics, contrasted with [`check_bodies`]:
/// - Runs the SAME `reject_reserved_idents` module-wide pre-pass first. That
///   pass is fail-fast by nature (a single reserved-identifier error for the
///   whole module); if it fires, this returns exactly that one error and does
///   not attempt per-item checks.
/// - Otherwise checks each top-level item, pushing each failing item's error
///   into the result `Vec` and CONTINUING to the next item (instead of
///   `?`-bailing). The item GRANULARITY is one top-level function OR one method:
///   a class with type errors in two different methods produces two errors. A
///   per-class prelude failure (multiple inheritance, a bad field annotation)
///   is one error and skips that class's methods, since those checks establish
///   class-level invariants the method checks rely on.
/// - Per-EXPRESSION recovery WITHIN a single function/method is not attempted —
///   each item is still checked fail-fast (first error in that item), matching
///   `check_bodies`' own per-item semantics. So at most one error is produced
///   per function/method.
/// - Errors are sorted by source position (span line, then col) so the caller
///   can render diagnostics top-to-bottom.
///
/// Returns an empty `Vec` for a clean module.
pub fn check_all(m: &Module, ctx: &TyCtx) -> Vec<Error> {
    // Module-wide pre-pass: fail-fast, identical to `check_bodies`. A reserved
    // identifier anywhere makes per-item checking meaningless, so surface that
    // single error alone.
    if let Err(e) = reject_reserved_idents(m) {
        return vec![e];
    }

    let mut errors: Vec<Error> = Vec::new();
    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
                if let Err(e) = check_one_func(f, ctx) {
                    errors.push(e);
                }
            }
            Stmt::Class(c) => {
                // The per-class prelude (multiple inheritance, field annotations)
                // establishes invariants the method checks rely on; if it fails,
                // record that one error and skip this class's methods.
                if let Err(e) = check_class_prelude(c, ctx) {
                    errors.push(e);
                    continue;
                }
                // Collect one error per failing method (the L4 method-level
                // granularity), continuing past a failing method to the next.
                for method in &c.methods {
                    if let Err(e) = check_one_method(c, method, ctx) {
                        errors.push(e);
                    }
                }
            }
            // Import statements have no body to check (resolved by the resolver).
            Stmt::Import { .. } => {}
            _ => {
                if let Err(e) = check_top_level_other(s, ctx) {
                    errors.push(e);
                }
            }
        }
    }

    // Order top-to-bottom by the error's source span (line, then col) so
    // squiggles appear in reading order regardless of statement-iteration order.
    errors.sort_by_key(|e| {
        let span = error_span(e);
        (span.line, span.col, span.start)
    });
    errors
}

/// Type-check a SINGLE top-level statement's body, fail-fast. Used by
/// [`check_bodies`], which `?`-propagates the first error. Composes the same
/// per-item helpers [`check_all`] uses, so the two entry points apply
/// byte-identical per-item checks — only their continue-vs-stop policy differs.
pub(crate) fn check_one_stmt(s: &Stmt, ctx: &TyCtx) -> Result<()> {
    match s {
        Stmt::Func(f) => check_one_func(f, ctx)?,
        Stmt::Class(c) => {
            check_class_prelude(c, ctx)?;
            for method in &c.methods {
                check_one_method(c, method, ctx)?;
            }
        }
        // Import statements are resolved by the resolver and are
        // intentionally not type-checked here (no body to check).
        Stmt::Import { .. } => {}
        _ => check_top_level_other(s, ctx)?,
    }
    Ok(())
}

/// Type-check ONE top-level function (decorators + signature + body), fail-fast.
pub(crate) fn check_one_func(f: &Func, ctx: &TyCtx) -> Result<()> {
    // Reject unsupported decorators on top-level functions.
    validate_decorators(&f.decorators, f.span)?;

    // `@crate(...)` (a declared external-crate dependency) is only meaningful on
    // an `@extern` binding — it tells the driver which crate the binding's Rust
    // template needs. Without `@extern` it would still pull the program onto the
    // Cargo build path while emitting a normal pyrst body that never uses the
    // crate, surfacing as a confusing cargo error. Reject it honestly here.
    if !f.crate_deps.is_empty() && !f.decorators.iter().any(|d| d == "extern") {
        return Err(Error::Type {
            span: f.span,
            msg: "`@crate` can only be used on `@extern` functions (it declares the \
                  crate an `@extern` binding's Rust template depends on)"
                .to_string(),
        });
    }

    // An `@extern` function is a Rust-FFI binding: its body is an opaque Rust
    // template string, not pyrst statements. Validate the binding shape (single
    // string-literal body + fully-typed signature) and STOP — there is no pyrst
    // body to type-check, and the template is validated by rustc at build.
    if f.decorators.iter().any(|d| d == "extern") {
        return validate_extern_func(f, ctx);
    }

    // Generics v1: param/return annotations naming a declared type parameter
    // lower to `Ty::TypeVar` (scoped lowering). Empty `type_params` => identical
    // to the non-generic path.
    let params: Vec<(String, Ty)> = f.params.iter()
        .filter(|p| p.name != "self")
        .map(|p| Ty::from_type_expr_scoped(&p.ty, p.span, &f.type_params).map(|ty| (p.name.clone(), ty)))
        .collect::<Result<Vec<_>>>()?;
    let by_ref_names: Vec<String> = f.params.iter()
        .filter(|p| p.name != "self" && p.by_ref)
        .map(|p| p.name.clone())
        .collect();
    // (LAZY-GEN V1-d) An `Iterator[T]` parameter is a V2 feature — reject at the
    // def site (honest error, not an accidental codegen success for fresh-call args).
    reject_iterator_params(&f.params)?;
    let ret = Ty::from_type_expr_scoped(&f.ret, f.span, &f.type_params)?;
    let mut env = FuncEnv::with_by_ref(ctx, &params, &by_ref_names, ret);
    env.type_params = f.type_params.iter().cloned().collect();
    env.is_generator = check_generator_signature(&f.body, &f.ret, f.span)?;
    // (LAZY-GEN V1-d) `yield` inside `try` cannot be lowered (E0728, §C.4) — reject.
    reject_yield_in_try(&f.body)?;
    collect_returned_param_idents(&f.body, &env.params, &mut env.returned_params);
    check_body(&f.body, &mut env)?;
    check_all_paths_return(&f.body, &env, &f.name, f.span)?;
    Ok(())
}

/// MISSING-RETURN GATE: a function whose declared return type is NON-UNIT (not
/// `None`/`Unit`) and that is NOT a generator must return a value (or diverge)
/// on EVERY control-flow path. Otherwise control can fall off the end of the
/// body and codegen emits an implicit `()` tail, which rustc rejects (E0308) —
/// a silent miscompile that breaches the honest-errors invariant. Catching it
/// here turns that into a clean `pyrst check` error.
///
/// Exemptions:
/// - `-> None`/Unit functions implicitly return `()`; nothing to enforce.
/// - Generators (`Iterator[T]` + a `yield` in the body) implicitly return their
///   eagerly-collected `Vec<T>` (codegen appends `return __pyrst_gen_acc;`), so
///   falling off the end is correct for them.
pub(crate) fn check_all_paths_return(body: &[Stmt], env: &FuncEnv, name: &str, span: Span) -> Result<()> {
    if env.is_generator || env.ret_ty == Ty::Unit {
        return Ok(());
    }
    if !block_definitely_returns(body) {
        return Err(Error::Type {
            span,
            msg: format!(
                "function `{}` declared to return `{}` may reach the end without returning a value",
                name, env.ret_ty
            ),
        });
    }
    Ok(())
}

/// Whether the function/method whose body is `body` and declared return type is
/// `ret` is a GENERATOR, validating its signature in the process. A body
/// containing `yield` MUST be declared `Iterator[T]` (honest error otherwise — a
/// generator that is not typed as an iterator). A body WITHOUT `yield` is never a
/// generator, even if declared `Iterator[T]` (such a function just returns a
/// `list[T]`, which is fine — no `yield`, no special handling). Returns
/// `Ok(true)` iff the function is a (well-formed) generator.
pub(crate) fn check_generator_signature(body: &[Stmt], ret: &TypeExpr, span: Span) -> Result<bool> {
    if !body_contains_yield(body) {
        // (LAZY-GEN V1-d) Require `yield` for an `Iterator[T]` return. Since V1-a
        // made `Iterator[T]` a DISTINCT type (no longer `≡ list[T]`), a `yield`-less
        // function declaring `-> Iterator[T]` is the last vestige of the old
        // list/iterator conflation — it promises a lazy generator but produces none.
        // Honest error (docs/design/lazy-generators.md §F): return `list[T]`, or add
        // a `yield`.
        if is_iterator_type_expr(ret) {
            return Err(Error::Type {
                span,
                msg: "a function declared to return `Iterator[T]` must contain a \
                      `yield` (it is a generator). To return a materialized sequence \
                      instead, declare `-> list[T]`; or add a `yield` to make it a \
                      generator."
                    .to_string(),
            });
        }
        return Ok(false);
    }
    if !is_iterator_type_expr(ret) {
        return Err(Error::Type {
            span,
            msg: "a generator (a function whose body uses `yield`) must be \
                  declared to return `Iterator[T]`"
                .to_string(),
        });
    }
    Ok(true)
}

/// (LAZY-GEN V1-d) The honest error for a lazy generator (`Ty::Iterator`) used in
/// a position that cannot be lazy — one needing a length, random access, a second
/// pass, a string form, or any binary-operator result. Every such site suggests
/// the same fix: materialize with `list(...)`. `problem` completes "a generator is
/// lazy and …"; `fix` shows the materialized form (e.g. `len(list(g))`). Codegen
/// would otherwise leak a raw rustc failure on the internal `__PyrstGen<T>` type;
/// this keeps the diagnostic honest and at `check` time.
pub(crate) fn iterator_materialize_error(problem: &str, fix: &str, span: Span) -> Error {
    Error::Type {
        span,
        msg: format!(
            "a generator is lazy and {}; materialize it first with `list(...)`: {}",
            problem, fix
        ),
    }
}

/// (LAZY-GEN V1-d) Reject a generator (`Ty::Iterator`) value flowing into a
/// concrete `list[T]` slot — a function argument, a `return`, or an annotated
/// assignment. `types_compatible` already returns `false` for this pair (the V1-a
/// interchangeability was flipped in V1-d); calling this FIRST replaces the
/// generic "expected list[..], found Iterator[..]" with the honest materialize
/// suggestion. The reverse direction (a `list` into an `Iterator[T]` slot) is a
/// V2 adapter feature and stays the generic type-mismatch error.
pub(crate) fn reject_iterator_into_list(val_ty: &Ty, slot_ty: &Ty, span: Span) -> Result<()> {
    if matches!(val_ty, Ty::Iterator(_)) && matches!(slot_ty, Ty::List(_)) {
        return Err(Error::Type {
            span,
            msg: format!(
                "a generator is lazy and cannot be used where `{}` is required; \
                 materialize it first with `list(...)`",
                slot_ty
            ),
        });
    }
    Ok(())
}

/// (LAZY-GEN V1-d) Reject `yield` inside a `try:` (or its `except`/`else`/
/// `finally` blocks). A `yield` lowers to `.await` on the coroutine, but the `try`
/// body runs inside a synchronous `catch_unwind` closure where `await` is illegal
/// (rustc E0728 — disproof in docs/design/lazy-generators.md §C.4). This is a V1
/// honest error and a V3 feature (needs a non-`catch_unwind` try lowering for
/// generator bodies). Walks the whole body; does NOT descend into nested
/// `def`/`class` bodies (they own their own yields).
pub(crate) fn reject_yield_in_try(body: &[Stmt]) -> Result<()> {
    for s in body {
        match s {
            Stmt::Try { body, handlers, else_, finally_, span } => {
                let has_yield = body_contains_yield(body)
                    || handlers.iter().any(|h| body_contains_yield(&h.body))
                    || else_.as_ref().is_some_and(|b| body_contains_yield(b))
                    || finally_.as_ref().is_some_and(|b| body_contains_yield(b));
                if has_yield {
                    return Err(Error::Type {
                        span: *span,
                        msg: "yield inside `try` is not yet supported; move the \
                              `yield` out of the `try` block (a generator's `try` \
                              body runs in a synchronous `catch_unwind` where `await` \
                              — the lowering of `yield` — is illegal)"
                            .to_string(),
                    });
                }
                // A `yield` may also sit in a `try` NESTED inside these blocks.
                reject_yield_in_try(body)?;
                for h in handlers { reject_yield_in_try(&h.body)?; }
                if let Some(b) = else_ { reject_yield_in_try(b)?; }
                if let Some(b) = finally_ { reject_yield_in_try(b)?; }
            }
            Stmt::If { then, elifs, else_, .. } => {
                reject_yield_in_try(then)?;
                for (_, b) in elifs { reject_yield_in_try(b)?; }
                if let Some(b) = else_ { reject_yield_in_try(b)?; }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::With { body, .. } => {
                reject_yield_in_try(body)?;
            }
            Stmt::Match { arms, .. } => {
                for arm in arms { reject_yield_in_try(&arm.body)?; }
            }
            // Nested defs/classes own their own yields.
            Stmt::Func(_) | Stmt::Class(_) => {}
            _ => {}
        }
    }
    Ok(())
}

/// (LAZY-GEN V1-d) Reject a parameter annotated `Iterator[T]`. An `Iterator[T]`
/// PARAMETER is a V2 feature — it needs a call-site `list → __PyrstGen` adapter
/// (a `Vec<T>` argument does not fit a `__PyrstGen<T>` slot). Until then a
/// generator parameter silently type-checks and only works by accident for a
/// fresh-call argument (review c5/c6), so reject it honestly at the def site
/// (docs/design/lazy-generators.md §F). Applies to free functions and methods;
/// the receiver `self` carries no annotation and is skipped by the caller.
pub(crate) fn reject_iterator_params(params: &[Param]) -> Result<()> {
    for p in params {
        if p.name == "self" {
            continue;
        }
        if is_iterator_type_expr(&p.ty) {
            return Err(Error::Type {
                span: p.span,
                msg: format!(
                    "`Iterator[T]` parameters arrive in V2: parameter `{}` cannot be \
                     a generator yet. Take a `list[T]` and pass `list(g)` at the call \
                     site.",
                    p.name
                ),
            });
        }
    }
    Ok(())
}

/// Whether a declared return annotation is `Iterator[T]` (the generator return
/// form). Spelled as a single-argument `Generic("Iterator", [T])` by the parser.
pub(crate) fn is_iterator_type_expr(t: &TypeExpr) -> bool {
    matches!(t, TypeExpr::Generic(name, args) if name == "Iterator" && args.len() == 1)
}

/// Per-CLASS checks that run before (and gate) the method checks: multiple
/// inheritance and explicit field-annotation validation. Fail-fast.
pub(crate) fn check_class_prelude(c: &ClassDef, ctx: &TyCtx) -> Result<()> {
    // Reject multiple inheritance.
    if c.bases.len() > 1 {
        return Err(Error::Type {
            span: c.span,
            msg: "multiple inheritance is not supported".to_string(),
        });
    }

    // Generics v2 (DEFERRED): a generic class participating in INHERITANCE is not
    // yet supported. The companion-enum dispatch codegen for a polymorphic base
    // (`B__::B(x) => x.get()`) does not thread the base's type parameters, so a
    // generic base/derived pair type-checks but emits Rust referencing an
    // undefined `T` (a silent check-pass / build-fail). Reject it honestly at
    // `check` — covering both directions: a generic class that DECLARES a base,
    // and a (generic or not) class whose base is a generic class. The core
    // single-class generics (Box / Pair) have no bases and are unaffected.
    if !c.bases.is_empty() {
        let base_is_generic = c.bases.iter().any(|b| {
            ctx.generic_classes.get(b).is_some_and(|tps| !tps.is_empty())
        });
        if !c.type_params.is_empty() || base_is_generic {
            return Err(Error::Type {
                span: c.span,
                msg: "generic classes with inheritance are not yet supported \
                      (a generic class may not declare a base, and a class may not \
                      inherit from a generic class)"
                    .to_string(),
            });
        }
    }

    // (EPIC-4 V2-c) Validate explicit class-FIELD annotations at `check` time.
    // Field types are otherwise only lowered lazily at codegen (`build`), so a
    // `Mut[T]` field annotation would slip past `pyrst check`. Running each
    // field through `from_type_expr` here makes the existing `("Mut", _)`
    // rejection arm fire at check time, so a class-field `Mut[T]` is an honest
    // error in BOTH `check` and `build` (mode markers belong only on params).
    // Generics v2: lower field annotations with the class's type parameters in
    // scope, so a generic field `value: T` lowers to `Ty::TypeVar("T")` (a valid
    // field type for a generic class) rather than the bogus `Ty::Class("T", [])`.
    // A non-generic class has empty `type_params`, identical to the legacy path.
    for field in &c.fields {
        Ty::from_type_expr_scoped(&field.ty, field.span, &c.type_params)?;
    }

    // A `Callable` field lowers to `Rc<dyn Fn(..) -> ..>`, which has no `Default`,
    // so the zero-then-`__init__` constructor placeholder cannot synthesize one —
    // the field MUST be seeded from an `__init__` parameter (directly, or through
    // a chain of local rebindings; see `init_field_param_map`). A `Callable` field
    // assigned from a non-param expression (`self.f = make_default()`) or with no
    // `__init__` at all has no valid placeholder and would SILENTLY build-fail with
    // rustc E0277 (`dyn Fn: Default`). Reject it honestly here so `pyrst check`
    // catches it. (The direct/indirect param-seeded cases — the common shape —
    // pass, so existing Callable-field classes are unaffected.)
    let init_fn = c.methods.iter().find(|m| m.name == "__init__");
    let seeded = init_fn.map(init_field_param_map).unwrap_or_default();
    for field in &c.fields {
        let ty = Ty::from_type_expr_scoped(&field.ty, field.span, &c.type_params)?;
        if matches!(ty, Ty::Func(..)) && !seeded.contains_key(&field.name) {
            return Err(Error::Type {
                span: field.span,
                msg: format!(
                    "a Callable field (`{}`) must be initialized from a constructor \
                     parameter (`self.{} = <__init__ param>`); a Callable value has no \
                     default, so it cannot be synthesized any other way",
                    field.name, field.name
                ),
            });
        }
    }
    Ok(())
}

/// Type-check ONE method of class `c` (decorators + dunder restrictions +
/// signature + body), fail-fast. The receiver type is `c`'s class type.
pub(crate) fn check_one_method(c: &ClassDef, method: &Func, ctx: &TyCtx) -> Result<()> {
    // Reject unsupported decorators on class methods.
    validate_decorators(&method.decorators, method.span)?;

    // `@crate` is tied to `@extern`, and `@extern` is not supported on methods
    // (rejected below), so a `@crate` on a method can never be valid — reject it
    // with the same message as the free-function path for a consistent error.
    if !method.crate_deps.is_empty() {
        return Err(Error::Type {
            span: method.span,
            msg: "`@crate` can only be used on `@extern` functions (it declares the \
                  crate an `@extern` binding's Rust template depends on)"
                .to_string(),
        });
    }

    // `@extern` is a Phase-1 binding for TOP-LEVEL std functions only. On a
    // method it would interact with the `self` receiver and by-reference mode
    // decisions, which are out of scope; reject it honestly here so it is caught
    // at both `check` and `build` rather than silently mis-emitted.
    if method.decorators.iter().any(|d| d == "extern") {
        return Err(Error::Type {
            span: method.span,
            msg: "`@extern` is not supported on a method (it is for top-level \
                  functions only); declare it as a free function"
                .to_string(),
        });
    }

    // `__bool__` is listed among the dunder-trait names in codegen (so it is
    // skipped by the inherent-methods loop) but has no trait-impl arm, which
    // would silently DROP a user-defined `__bool__`. pyrst also has no working
    // object-truthiness lowering today: `bool(obj)` lowers numerically and an
    // `if obj:` / `while obj:` condition is not constrained to `bool`, so a
    // class instance in a truthiness position emits invalid Rust regardless.
    // Rather than mislead the user with a silently-ignored method, reject
    // `__bool__` honestly here (it is then caught by both `check` and `build`).
    // Lowering object truthiness is a separate, larger feature.
    if method.name == "__bool__" {
        return Err(Error::Type {
            span: method.span,
            msg: "__bool__ is not yet supported (object truthiness is not lowered); \
                  define an explicit predicate method instead".to_string(),
        });
    }

    // (EPIC-4 V2-c) `Mut[T]` is unsupported on a CONSTRUCTOR parameter. The
    // generated `new()` wrapper passes owned values into `self.__init__(...)`,
    // which would mismatch a `&mut T` `__init__` signature — and a fresh
    // `__inst` has no caller-visible storage for a by-ref param to alias anyway.
    // Reject here so both `check` and `build` catch it cleanly rather than
    // silently mis-emitting.
    if method.name == "__init__" {
        if let Some(p) = method.params.iter().find(|p| p.by_ref) {
            return Err(Error::Type {
                span: method.span,
                msg: format!(
                    "Mut[T] is not supported on a constructor (`__init__`) parameter `{}`",
                    p.name
                ),
            });
        }
    }

    // Generics v2: the class's type parameters are SCOPED TO THE METHOD BODY —
    // a param/return naming one (`v: T`, `-> T`) lowers to `Ty::TypeVar(T)`
    // (scoped lowering), and `self` is typed `Ty::Class(name, [TypeVar(T), ..])`
    // so a field read `self.value: T` substitutes the identity `{T -> T}` and
    // stays `T`. The class type vars also go into `env.type_params`, so an
    // UNSUPPORTED op on a bare `T` is rejected here exactly like in a generic
    // function (and a supported op infers its bound for codegen). A non-generic
    // class has empty `type_params` => identical to the legacy unscoped path.
    let mut params: Vec<(String, Ty)> = method.params.iter()
        .filter(|p| p.name != "self")
        .map(|p| Ty::from_type_expr_scoped(&p.ty, p.span, &c.type_params).map(|ty| (p.name.clone(), ty)))
        .collect::<Result<Vec<_>>>()?;
    let self_args: Vec<Ty> = c.type_params.iter().map(|tp| Ty::TypeVar(tp.clone())).collect();
    params.insert(0, ("self".into(), Ty::Class(c.name.clone(), self_args)));
    let by_ref_names: Vec<String> = method.params.iter()
        .filter(|p| p.name != "self" && p.by_ref)
        .map(|p| p.name.clone())
        .collect();
    // (LAZY-GEN V1-d) An `Iterator[T]` parameter is a V2 feature — reject it on a
    // method too (`self` carries no annotation and is skipped).
    reject_iterator_params(&method.params)?;
    let ret = Ty::from_type_expr_scoped(&method.ret, method.span, &c.type_params)?;
    let mut env = FuncEnv::with_by_ref(ctx, &params, &by_ref_names, ret);
    env.type_params = c.type_params.iter().cloned().collect();
    env.is_generator = check_generator_signature(&method.body, &method.ret, method.span)?;
    // (LAZY-GEN V1-d) Generator METHODS are a V2 feature (V2-b): the returned
    // `__PyrstGen<T>` outlives the `&self` borrow, so the body must capture the
    // needed `self` fields by clone into `async move` — not wired yet. A generator
    // method currently type-checks and mis-lowers; reject it honestly at the def
    // site (docs/design/lazy-generators.md §F).
    if env.is_generator {
        return Err(Error::Type {
            span: method.span,
            msg: format!(
                "generator methods arrive in V2: method `{}` uses `yield`, which is \
                 not yet supported inside a class. Move the generator to a free \
                 function (`def {}(...) -> Iterator[T]:`) and call it from the method.",
                method.name, method.name
            ),
        });
    }
    // (LAZY-GEN V1-d) `yield` inside `try` cannot be lowered (E0728, §C.4) — reject.
    reject_yield_in_try(&method.body)?;
    collect_returned_param_idents(&method.body, &env.params, &mut env.returned_params);
    check_body(&method.body, &mut env)?;
    check_all_paths_return(&method.body, &env, &method.name, method.span)?;
    Ok(())
}

/// Whether `e` is a CONST LITERAL eligible for a module-level constant: a bare
/// int / float / str / bool literal. Negative numbers parse as `UnOp{Neg, ...}`
/// and const EXPRESSIONS (`2 * pi`) are out of scope for v1 — only the four
/// primitive literal forms qualify. Shared by typeck (relaxed top-level check),
/// the resolver (`module_consts` population), and codegen (`const` emission) so
/// the three never drift on what "a module constant" means.
pub(crate) fn is_const_literal(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..)
    )
}

/// Whether `s` is a legal MODULE-LEVEL CONSTANT declaration: a top-level
/// ANNOTATED assignment `NAME: T = <literal>` whose value is a const literal
/// (see [`is_const_literal`]). This is the SOLE top-level statement form (beyond
/// function/class/import) that the EPIC-6 relaxation legalizes — an UNANNOTATED
/// `x = 5`, a call, a print, or an annotated assign to a NON-literal value all
/// stay rejected.
pub(crate) fn is_module_const_decl(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::Assign { ty: Some(_), value, .. } if is_const_literal(value)
    )
}

/// The static [`Ty`] of a const LITERAL (the four forms [`is_const_literal`]
/// admits). Returns `None` for any other expression.
pub(crate) fn const_literal_ty(e: &Expr) -> Option<Ty> {
    match e {
        Expr::Int(..) => Some(Ty::Int),
        Expr::Float(..) => Some(Ty::Float),
        Expr::Str(..) => Some(Ty::Str),
        Expr::Bool(..) => Some(Ty::Bool),
        _ => None,
    }
}

/// Handle a top-level statement that is neither a function, class, nor import.
/// Silently accepts a bare top-level `main()` call (the conventional pyrst
/// entry-point idiom) AND a module-level annotated-literal constant declaration
/// (`NAME: T = <literal>`, the EPIC-6-A relaxation that lets a module hold
/// constants like `math.pi`); rejects any other stray top-level statement.
/// Fail-fast.
pub(crate) fn check_top_level_other(s: &Stmt, ctx: &TyCtx) -> Result<()> {
    // A bare top-level `main()` call is the conventional pyrst entry-point idiom
    // and is already driven by the synthetic Rust `fn main() { user_main(); }`.
    if is_bare_main_call(s) {
        return Ok(());
    }
    // `yield` outside any function is an honest error (there is no generator to
    // collect into). Caught here with a specific message rather than the generic
    // "top-level statements ... are not supported" fall-through below.
    if let Stmt::Yield(_, span) = s {
        return Err(Error::Type {
            span: *span,
            msg: "`yield` outside a function is not allowed (it is only valid \
                  inside a generator function declared `Iterator[T]`)"
                .to_string(),
        });
    }
    // A module-level constant (`NAME: T = <literal>`) is the narrow EPIC-6-A
    // relaxation: it is the ONLY assignment form accepted at top level — an
    // unannotated assign, an annotated assign to a non-literal value, a call, a
    // print, or any other stray statement is still an honest error. The declared
    // type must be valid AND match the literal (so `x: int = "s"` is rejected,
    // and an invalid annotation like `set[float]` is rejected by `from_type_expr`).
    if let Stmt::Assign { target, ty: Some(t), value, span } = s {
        if is_const_literal(value) {
            // The const NAME must not be a Rust non-raw keyword nor use the
            // reserved compiler-generated prefix (the mangled-const namespace).
            reject_if_reserved(target, *span, "module constant")?;
            // (Honest-errors) The module-const namespace is FLAT — codegen emits
            // one top-level Rust `const __pyrst_const_<name>` and rewrites bare /
            // qualified references to it. A const whose name DUPLICATES a function
            // (built-in OR user/stdlib, any module — `ctx.funcs` is the merged flat
            // table) or a class is ambiguous: a call `name()` would route to the
            // const and miscompile (E0618). Reject it honestly at `check` time
            // rather than deferring the clash to rustc at `build`. This single
            // check at the const site catches the symmetric pair regardless of
            // source order (ctx is fully merged before checking) and the
            // cross-module case (flat table).
            if ctx.funcs.contains_key(target) {
                return Err(Error::Type {
                    span: *span,
                    msg: format!(
                        "module constant `{}` clashes with a function of the same name; \
                         rename one (constants and functions share a flat namespace)",
                        target
                    ),
                });
            }
            if ctx.classes.contains_key(target) {
                return Err(Error::Type {
                    span: *span,
                    msg: format!(
                        "module constant `{}` clashes with a class of the same name; \
                         rename one (constants and classes share a flat namespace)",
                        target
                    ),
                });
            }
            let declared = Ty::from_type_expr(t, *span)?;
            let lit_ty = const_literal_ty(value).unwrap_or(Ty::Unknown);
            if !types_compatible(&lit_ty, &declared, ctx) {
                return Err(Error::Type {
                    span: *span,
                    msg: format!(
                        "type mismatch in module constant: declared {}, got {}",
                        declared, lit_ty
                    ),
                });
            }
            return Ok(());
        }
    }
    let span = stmt_span(s);
    Err(Error::Type {
        span,
        msg: "top-level statements other than function/class/import \
              definitions (and module-level constants `NAME: T = <literal>`) \
              are not supported"
            .to_string(),
    })
}

/// Innermost source [`Span`] of an [`Error`], unwrapping the EPIC-8 `Sourced`
/// wrapper. Used by [`check_all`] to order collected errors top-to-bottom.
/// Span-less variants (`Io`, `Codegen`, `Rustc`) fall back to `Span::DUMMY`,
/// which sorts to the front (line/col/start all zero).
pub(crate) fn error_span(e: &Error) -> Span {
    match e {
        Error::Lex { span, .. }
        | Error::Parse { span, .. }
        | Error::Type { span, .. }
        | Error::ImportNotFound { span, .. }
        | Error::CircularImport { span, .. } => *span,
        Error::Sourced { inner, .. } => error_span(inner),
        Error::Io(_) | Error::Codegen(_) | Error::Rustc(_) => Span::DUMMY,
    }
}

/// Analyze which functions are actually called in a module.
/// Returns a set of function names that are referenced.
pub fn analyze_called_functions(module: &Module) -> std::collections::HashSet<String> {
    let mut called = std::collections::HashSet::new();

    for stmt in &module.stmts {
        collect_calls_from_stmt(stmt, &mut called);
    }

    called
}

pub(crate) fn collect_calls_from_stmt(stmt: &Stmt, called: &mut std::collections::HashSet<String>) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e), _) => collect_calls_from_expr(e, called),
        Stmt::Assign { value, .. } | Stmt::AugAssign { value, .. } => collect_calls_from_expr(value, called),
        Stmt::Unpack { value, .. } => collect_calls_from_expr(value, called),
        Stmt::If { cond, then, elifs, else_, .. } => {
            collect_calls_from_expr(cond, called);
            for s in then { collect_calls_from_stmt(s, called); }
            for (c, b) in elifs {
                collect_calls_from_expr(c, called);
                for s in b { collect_calls_from_stmt(s, called); }
            }
            if let Some(b) = else_ {
                for s in b { collect_calls_from_stmt(s, called); }
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_calls_from_expr(cond, called);
            for s in body { collect_calls_from_stmt(s, called); }
        }
        Stmt::For { iter, body, .. } => {
            collect_calls_from_expr(iter, called);
            for s in body { collect_calls_from_stmt(s, called); }
        }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            for s in body { collect_calls_from_stmt(s, called); }
            for h in handlers {
                for s in &h.body { collect_calls_from_stmt(s, called); }
            }
            if let Some(b) = else_ {
                for s in b { collect_calls_from_stmt(s, called); }
            }
            if let Some(b) = finally_ {
                for s in b { collect_calls_from_stmt(s, called); }
            }
        }
        Stmt::With { ctx_expr, body, .. } => {
            collect_calls_from_expr(ctx_expr, called);
            for s in body { collect_calls_from_stmt(s, called); }
        }
        Stmt::Func(f) => {
            for s in &f.body { collect_calls_from_stmt(s, called); }
        }
        Stmt::Class(c) => {
            for m in &c.methods {
                for s in &m.body { collect_calls_from_stmt(s, called); }
            }
        }
        Stmt::AttrAssign { obj, value, .. } => {
            collect_calls_from_expr(obj, called);
            collect_calls_from_expr(value, called);
        }
        Stmt::IndexAssign { obj, idx, value, .. } => {
            collect_calls_from_expr(obj, called);
            collect_calls_from_expr(idx, called);
            collect_calls_from_expr(value, called);
        }
        // A function called ONLY inside a `raise`/`yield` expression must still be
        // kept alive through dead-code elimination, or codegen emits a call to a
        // pruned function -> rustc "cannot find function" (a check-passes/build-fails).
        Stmt::Raise { exc: Some(e), .. } => collect_calls_from_expr(e, called),
        Stmt::Yield(e, _) => collect_calls_from_expr(e, called),
        _ => {}
    }
}

pub(crate) fn collect_calls_from_expr(expr: &Expr, called: &mut std::collections::HashSet<String>) {
    match expr {
        Expr::Call { callee, args, .. } => {
            if let Expr::Ident(name, _) = callee.as_ref() {
                called.insert(name.clone());
            } else if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                // A qualified module call `X.f(...)` lowers to a flat `f(...)`, so
                // register `f` to keep the module function alive through dead-code
                // elimination (otherwise it is pruned and codegen emits a call to a
                // function that was never output -> rustc "cannot find function f").
                // Harmless for a true method call (only over-keeps a same-named
                // top-level function).
                called.insert(name.clone());
                collect_calls_from_expr(obj, called);
            } else {
                // A non-name callee (`ops["f"](x)`, `(make_adder(5))(10)`) may
                // itself reference functions — traverse it so they stay alive.
                collect_calls_from_expr(callee, called);
            }
            for arg in args { collect_calls_from_expr(arg, called); }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_calls_from_expr(lhs, called);
            collect_calls_from_expr(rhs, called);
        }
        Expr::UnOp { expr: e, .. } => collect_calls_from_expr(e, called),
        Expr::List(elems, _) => {
            for e in elems { collect_calls_from_expr(e, called); }
        }
        Expr::Tuple(elems, _) => {
            for e in elems { collect_calls_from_expr(e, called); }
        }
        Expr::Set(elems, _) => {
            for e in elems {
                collect_calls_from_expr(e, called);
            }
        }
        Expr::Dict(pairs, _) => {
            for (k, v) in pairs {
                collect_calls_from_expr(k, called);
                collect_calls_from_expr(v, called);
            }
        }
        Expr::ListComp { elt, iter, cond, .. } => {
            collect_calls_from_expr(elt, called);
            collect_calls_from_expr(iter, called);
            if let Some(c) = cond { collect_calls_from_expr(c, called); }
        }
        Expr::SetComp { elt, iter, cond, .. } => {
            collect_calls_from_expr(elt, called);
            collect_calls_from_expr(iter, called);
            if let Some(c) = cond { collect_calls_from_expr(c, called); }
        }
        Expr::DictComp { key, val, iter, cond, .. } => {
            collect_calls_from_expr(key, called);
            collect_calls_from_expr(val, called);
            collect_calls_from_expr(iter, called);
            if let Some(c) = cond { collect_calls_from_expr(c, called); }
        }
        Expr::Attr { obj, .. } => collect_calls_from_expr(obj, called),
        Expr::Index { obj, idx, .. } => {
            collect_calls_from_expr(obj, called);
            collect_calls_from_expr(idx, called);
        }
        Expr::Slice { obj, start, stop, step, .. } => {
            collect_calls_from_expr(obj, called);
            if let Some(e) = start { collect_calls_from_expr(e, called); }
            if let Some(e) = stop { collect_calls_from_expr(e, called); }
            if let Some(e) = step { collect_calls_from_expr(e, called); }
        }
        Expr::FStr(parts, _) => {
            for part in parts {
                if let crate::ast::FStrPart::Interp(inner, _) = part {
                    collect_calls_from_expr(inner, called);
                }
            }
        }
        Expr::Lambda { body, .. } => {
            collect_calls_from_expr(body, called);
        }
        Expr::IfExp { test, body, orelse, .. } => {
            collect_calls_from_expr(test, called);
            collect_calls_from_expr(body, called);
            collect_calls_from_expr(orelse, called);
        }
        // (first-class functions) A bare name in a VALUE position keeps the
        // function it refers to alive for dead-code elimination. `inc`/`double`
        // passed to `apply_to_all` or stored in a dict are never the callee of a
        // `Call`, so without this they would be pruned as "uncalled" and their
        // `Rc::new(inc)` reference would dangle. Inserting non-function local
        // names too is harmless: `dead_funcs` is built from `ctx.funcs` keys only.
        Expr::Ident(name, _) => {
            called.insert(name.clone());
        }
        _ => {}
    }
}

