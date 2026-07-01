use super::*;

/// Generics v1: whether a `match` arm pattern DISCRIMINATES — i.e. it compares the
/// subject against a value and therefore needs `PartialEq` on the subject's type.
/// A `Literal` pattern (and an `Or` containing one) discriminates; a `Wildcard` or
/// a `Capture` (bare binding) does not. Used to decide whether matching a bare
/// type variable is an honest error (a wildcard/capture-only match on a `T` needs
/// no comparison and stays legal).
pub(crate) fn pattern_discriminates(p: &MatchPattern) -> bool {
    match p {
        MatchPattern::Literal(_) => true,
        MatchPattern::Wildcard | MatchPattern::Capture(_) => false,
        MatchPattern::Or(alts) => alts.iter().any(pattern_discriminates),
    }
}

/// (EPIC-5) Recognize a None-guard condition of the form `x is None` /
/// `x is not None` on a plain local name. Returns `(name, is_not_none)` where
/// `is_not_none` is true for `is not None` (the branch in which `x` is the
/// non-None payload). Mirrors codegen's `extract_narrowing` so the two layers
/// agree on which guards narrow.
pub(crate) fn extract_none_guard(cond: &Expr) -> Option<(String, bool)> {
    if let Expr::BinOp { op, lhs, rhs, .. } = cond {
        if matches!(op, BinOp::Is | BinOp::IsNot) && matches!(rhs.as_ref(), Expr::None_(_)) {
            if let Expr::Ident(name, _) = lhs.as_ref() {
                return Some((name.clone(), *op == BinOp::IsNot));
            }
        }
    }
    None
}

/// Unify the two branch types of a conditional expression. Returns the more
/// concrete type when the branches are compatible (an `Unknown`, or a
/// collection with `Unknown` elements, absorbs the concrete side), or `None`
/// when they are genuinely incompatible.
pub(crate) fn unify_branch_types(a: Ty, b: Ty, ctx: &TyCtx) -> Option<Ty> {
    // (EPIC-5 C1-B) Unification is SYMMETRIC ("can these two coexist in one
    // slot?"), whereas `types_compatible` is DIRECTIONAL (value→slot). For two
    // classes related by subtyping in EITHER order the answer is yes (they meet
    // at the base), so probe both directions before bailing — otherwise a branch
    // that yields `Base` then `Derived` (the order in which `types_compatible`
    // is false) would be wrongly rejected. Non-class pairs are unaffected: the
    // class-pair arm only fires for `(Class, Class)`, and for unrelated classes
    // both `is_subclass` checks are false, so the original directional gate is
    // the deciding test exactly as before.
    // (EPIC-5 C2-2b-i) Two classes are "related" for unification when one derives
    // from the other OR they share a common user-declared ancestor (sibling
    // subclasses unify at that ancestor — `Dog` & `Cat` meet at `Animal`).
    let class_related = matches!((&a, &b), (Ty::Class(x, _), Ty::Class(y, _))
        if is_subclass(x, y, ctx) || is_subclass(y, x, ctx)
            || nearest_common_ancestor(x, y, ctx).is_some());
    if !class_related && !types_compatible(&a, &b, ctx) {
        return None;
    }
    Some(match (&a, &b) {
        (Ty::Unknown, _) => b,
        (Ty::List(i), Ty::List(_)) if **i == Ty::Unknown => b,
        (Ty::Set(i), Ty::Set(_)) if **i == Ty::Unknown => b,
        (Ty::Dict(k, v), Ty::Dict(_, _)) if **k == Ty::Unknown && **v == Ty::Unknown => b,
        // (EPIC-5 C1-B) Two subtype-related classes unify to the BASE (wider)
        // type, not the first-seen one — a `Derived` and its `Base` share a
        // common slot only at the `Base`. `types_compatible` above already
        // verified the pair is related (in EITHER direction, since it is checked
        // both ways below). For unrelated classes neither `is_subclass` holds and
        // the equal-name case fell through to the default `=> a` arm unchanged.
        (Ty::Class(da, _), Ty::Class(db, _)) if da != db && is_subclass(da, db, ctx) => b, // a derives from b -> b is base
        (Ty::Class(da, _), Ty::Class(db, _)) if da != db && is_subclass(db, da, ctx) => a, // b derives from a -> a is base
        // (EPIC-5 C2-2b-i) Two SIBLING subclasses unify to their nearest common
        // ancestor (`Dog` & `Cat` -> `Animal`). Reached only when neither is a
        // subclass of the other but a common ancestor exists (the `class_related`
        // guard above admitted the pair).
        (Ty::Class(da, _), Ty::Class(db, _)) if da != db => {
            match nearest_common_ancestor(da, db, ctx) {
                Some(anc) => Ty::Class(anc, vec![]),
                None => a, // defensive: guard already ensured one exists
            }
        }
        // `a` is the concrete side (or both equal) -> keep it.
        _ => a,
    })
}

/// Unify the element types of a homogeneous collection literal.
///
/// Returns the unified element type when the two types can coexist in one Rust
/// collection, or `None` when they are genuinely heterogeneous and the literal
/// should be rejected. Stays permissive on `Unknown` (and collections with an
/// `Unknown` inner) via the shared `unify_branch_types` arms; only both-concrete,
/// non-`Unknown`, incompatible pairs (e.g. Int/Str) return `None`.
///
/// `widen_numeric` controls Int/Float promotion, which is only SOUND where the
/// element type may be `Float`. A `list[float]` (`Vec<f64>`) is representable, so
/// LIST literals pass `true` and `[1, 2.0]` widens to `List(Float)` (codegen
/// casts the int elements to f64 — see `Codegen::emit_collection_elem`). It is
/// UNSOUND in hashable positions: a `set[float]` (`HashSet<f64>`) does not
/// compile (f64 is not `Eq`/`Hash`), so SET literals pass `false` and `{1, 2.0}`
/// is rejected. (Dict keys are hashable -> `false`; dict values -> `true`.)
/// The broader `set[float]` gap is tracked separately.
pub(crate) fn unify_elem_types(a: Ty, b: Ty, widen_numeric: bool, ctx: &TyCtx) -> Option<Ty> {
    match (&a, &b) {
        // Numeric promotion to Float — only where a Float element is representable.
        (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) if widen_numeric => Some(Ty::Float),
        _ => unify_branch_types(a, b, ctx),
    }
}

/// Reject a `Float` type in a hashable position (set element, dict key).
///
/// `HashSet<f64>` / `HashMap<f64, _>` do not compile because `f64` is not
/// `Eq`/`Hash`; codegen's `rust_ty` would emit exactly those forms. To keep
/// typeck and codegen in agreement (the soundness rule), forbid a concretely
/// `Float` element/key here — whether it arises from a literal, a comprehension,
/// or a declared `set[float]` / `dict[float, _]` annotation.
///
/// Stays permissive on `Unknown` (e.g. `set()` / `{}` with no concrete inner):
/// only a concrete `Ty::Float` is rejected, never `Unknown`.
pub(crate) fn require_hashable(ty: &Ty, span: Span, position: &str) -> Result<()> {
    if matches!(ty, Ty::Float) {
        return Err(Error::Type {
            span,
            msg: format!(
                "{} type must be hashable; float is not supported here \
                 (f64 is not Eq/Hash, so HashSet<f64>/HashMap<f64, _> won't compile)",
                position
            ),
        });
    }
    // (first-class functions) A function value is NOT a valid hashable element:
    // it lowers to `Rc<dyn Fn(..) -> ..>`, and `dyn Fn` implements neither `Eq`
    // nor `Hash`, so `HashSet<Rc<dyn Fn>>` / `HashMap<Rc<dyn Fn>, _>` cannot
    // compile. Reject `set[Callable[..]]` and a Callable dict KEY here — the same
    // honest typeck error as `set[float]` — rather than deferring an opaque rustc
    // E0277. (A Callable dict VALUE is fine and is not routed through this check.)
    if matches!(ty, Ty::Func(..)) {
        return Err(Error::Type {
            span,
            msg: format!(
                "{} type must be hashable; a function value (Callable) is not \
                 supported here (Rc<dyn Fn> is not Eq/Hash, so HashSet/HashMap-key \
                 of functions won't compile)",
                position
            ),
        });
    }
    // Generics v2: a bare type variable in a hashable position (`set[T]` /
    // `dict[T, _]` element or key, a `{a, b}` set literal of type-var values, or
    // a `{k: v}` dict whose KEY is a type var) is now LEGAL — it INFERS a
    // `Hash + Eq` bound on `T` (collected by `infer_func_typevar_bounds`,
    // emitted in the generic clause), so the generated `HashSet<T>` /
    // `HashMap<T, _>` is instantiable. No rejection here; the bound inference
    // covers all six hashable-element sites (set/dict literals, set/dict
    // annotations, set/dict comprehensions).
    Ok(())
}

/// (honest errors) True for a type that is KNOWN to be non-callable, so calling
/// a value of this type is a genuine type error rather than a deferred rustc
/// E0618. `Ty::Func` is callable; `Ty::Unknown` is the permissive escape hatch
/// (an untyped value / `super()` / stdlib stand-in may be callable) and
/// `Ty::Class` is left permissive too (a class instance may gain a `__call__` in
/// a later increment). Everything else — primitives, collections, Option, File,
/// the unit/None types — is definitively not callable.
pub(crate) fn is_noncallable_ty(ty: &Ty) -> bool {
    !matches!(ty, Ty::Func(..) | Ty::Unknown | Ty::Class(_, _))
}

// ── By-value parameter mutation detection helpers ─────────────────────────────

/// Walk `Attr { obj }` and `Index { obj }` chains to find the innermost `Ident`.
/// Returns the identifier name if the expression is rooted at a plain name.
pub(crate) fn root_ident(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(name, _) => Some(name.as_str()),
        Expr::Attr { obj, .. } => root_ident(obj),
        Expr::Index { obj, .. } => root_ident(obj),
        _ => None,
    }
}

/// EPIC-4 V2: is `e` a *place* (an addressable lvalue) we could borrow `&mut`?
/// A by-reference (`Mut[T]`) argument must be one of these — a variable, a field
/// access, or an index — never a temporary (call/constructor/literal/binop/etc.),
/// which has no caller-visible storage to mutate.
pub(crate) fn is_place_expr(e: &Expr) -> bool {
    matches!(e, Expr::Ident(..) | Expr::Attr { .. } | Expr::Index { .. })
}

/// The single source of truth for copy-ness, consumed by both `typeck` and
/// `codegen` (via `crate::typeck::is_copy` / `is_owned`). A type is `Copy` when
/// its emitted Rust representation implements the `Copy` trait, so a by-value
/// use neither moves the original binding nor needs a `.clone()`.
///
/// Rule (defined recursively for the aggregate variants):
/// - `Int`/`Float`/`Bool`/`Unit` are `Copy`.
/// - `Tuple(elems)` is `Copy` iff **every** element is `Copy` (Rust tuples of
///   `Copy` elements are `Copy`).
/// - `Option(inner)` is `Copy` iff `inner` is `Copy` (Rust `Option<T: Copy>` is
///   `Copy`).
/// - Everything else is non-`Copy`: `Str`, `List`, `Set`, `Dict`, `Class`, and
///   the conservative `NoneVal`/`File`/`Unknown` cases (excluded here exactly as
///   the legacy `is_copy_type` excluded them).
pub fn is_copy(ty: &Ty) -> bool {
    match ty {
        Ty::Int | Ty::Float | Ty::Bool | Ty::Unit => true,
        Ty::Tuple(elems) => elems.iter().all(is_copy),
        Ty::Option(inner) => is_copy(inner),
        Ty::Str
        | Ty::List(_)
        | Ty::Set(_)
        | Ty::Dict(_, _)
        | Ty::Class(_, _)
        | Ty::Func(_, _)
        | Ty::NoneVal
        | Ty::File
        // A bound type variable is non-Copy: codegen emits a `T: Clone` bound and
        // clones on use, so a type-var value behaves like any owned value.
        | Ty::TypeVar(_)
        | Ty::Unknown => false,
    }
}

/// Complement of [`is_copy`]: `true` for move-only (non-`Copy`) types, i.e. ones
/// that need clone-on-use because a by-value pass would otherwise consume the
/// original binding (and, for params, hand the callee a clone whose mutations
/// cannot propagate back to the caller).
pub fn is_owned(ty: &Ty) -> bool {
    !is_copy(ty)
}

/// The single source of truth for collection methods that mutate their receiver
/// in place (List/Set/Dict mutators). Consumed by BOTH modules — same "one
/// source of truth" discipline as [`is_copy`]:
/// - `typeck`'s by-value-param backstop: calling any of these on a by-value
///   non-Copy param is a bug (the mutation is lost on the caller's copy).
/// - `codegen`'s `method_modifies_self` (to infer `&mut self` on the enclosing
///   method) and the emission site (to pick `emit_place` for subscripted
///   receivers so the mutation lands on the real element).
///
/// Previously duplicated as `codegen::MUTATING_METHODS` and
/// `typeck::PARAM_MUTATING_METHODS` (content-identical 13-name lists, differing
/// only in ordering); merged here so the two analyses can never drift.
pub const MUTATING_METHODS: &[&str] = &[
    // List mutators
    "append", "extend", "insert", "remove", "sort", "reverse", "clear",
    // Set mutators
    "add", "discard",
    // Dict mutators
    "update", "pop", "setdefault", "popitem",
];

/// Shared body of the by-value-parameter-mutation backstop error. EPIC-4 V2 adds
/// the `Mut[T]` on-ramp to the remedy clause: the user can now opt into a real
/// by-reference param instead of only the return-the-value idiom. All three
/// backstop sites (AttrAssign / IndexAssign / mutating method-call) use this so
/// the message can never drift between them.
pub(crate) fn by_value_mutation_error(param: &str) -> String {
    format!(
        "mutation of by-value parameter `{}` is not visible to the caller; \
         mutate via a method on it or return the updated value; \
         or declare the parameter `Mut[T]` to mutate it in place",
        param
    )
}

// ─────────────────────────────────────────────────────────────────────────────

/// Pre-scan a function body and collect the names of parameters that appear as
/// identifiers in any `return <expr>` statement (including nested blocks).
///
/// A param that is mutated then returned is the valid functional pattern:
///   `xs.append(99); return xs`
/// The callee operates on its own copy and returns the updated value; the
/// caller captures it.  We suppress the by-value-param-mutation error for
/// any param that flows to at least one return — conservative (favour avoiding
/// false positives over false negatives).
pub(crate) fn collect_returned_param_idents(
    stmts: &[Stmt],
    params: &std::collections::HashSet<String>,
    out: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        collect_returned_param_idents_stmt(s, params, out);
    }
}

pub(crate) fn collect_returned_param_idents_stmt(
    s: &Stmt,
    params: &std::collections::HashSet<String>,
    out: &mut std::collections::HashSet<String>,
) {
    match s {
        Stmt::Return(Some(e), _) => {
            collect_returned_param_idents_expr(e, params, out);
        }
        // Recurse into all nested statement blocks.
        Stmt::If { then, elifs, else_, .. } => {
            collect_returned_param_idents(then, params, out);
            for (_, b) in elifs { collect_returned_param_idents(b, params, out); }
            if let Some(b) = else_ { collect_returned_param_idents(b, params, out); }
        }
        Stmt::While { body, .. } => collect_returned_param_idents(body, params, out),
        Stmt::For { body, .. } => collect_returned_param_idents(body, params, out),
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            collect_returned_param_idents(body, params, out);
            for h in handlers { collect_returned_param_idents(&h.body, params, out); }
            if let Some(b) = else_ { collect_returned_param_idents(b, params, out); }
            if let Some(b) = finally_ { collect_returned_param_idents(b, params, out); }
        }
        Stmt::With { body, .. } => collect_returned_param_idents(body, params, out),
        // Match arms
        Stmt::Match { arms, .. } => {
            for arm in arms { collect_returned_param_idents(&arm.body, params, out); }
        }
        // Nested defs / classes — do NOT descend; their returns belong to a
        // different function scope.
        Stmt::Func(_) | Stmt::Class(_) => {}
        _ => {}
    }
}

/// Whether a function body (a flat `[Stmt]`) contains a `yield` ANYWHERE in its
/// own control flow — directly or nested inside if/while/for/try/with/match
/// blocks — making the enclosing function a GENERATOR. Nested `def`/`class`
/// bodies are NOT descended: a `yield` inside an inner function makes THAT inner
/// function the generator, not the outer one (mirrors `collect_returned_param_idents`).
pub fn body_contains_yield(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_contains_yield)
}

pub(crate) fn stmt_contains_yield(s: &Stmt) -> bool {
    match s {
        Stmt::Yield(..) => true,
        Stmt::If { then, elifs, else_, .. } => {
            body_contains_yield(then)
                || elifs.iter().any(|(_, b)| body_contains_yield(b))
                || else_.as_ref().is_some_and(|b| body_contains_yield(b))
        }
        Stmt::While { body, .. } => body_contains_yield(body),
        Stmt::For { body, .. } => body_contains_yield(body),
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            body_contains_yield(body)
                || handlers.iter().any(|h| body_contains_yield(&h.body))
                || else_.as_ref().is_some_and(|b| body_contains_yield(b))
                || finally_.as_ref().is_some_and(|b| body_contains_yield(b))
        }
        Stmt::With { body, .. } => body_contains_yield(body),
        Stmt::Match { arms, .. } => arms.iter().any(|arm| body_contains_yield(&arm.body)),
        // A nested function/class owns its own yields.
        Stmt::Func(_) | Stmt::Class(_) => false,
        _ => false,
    }
}

/// Whether a block (a flat `[Stmt]`) DEFINITELY returns a value or diverges on
/// every control-flow path — i.e. control can never "fall off the end" of the
/// block. Used by the missing-return gate ([`check_one_func`] /
/// [`check_one_method`]) so a non-unit, non-generator function that can reach
/// the end of its body without a `return <value>` is an honest type error
/// rather than a silent rustc E0308 miscompile.
///
/// The analysis is driven by the block's LAST statement: an unconditional
/// earlier `return`/`raise` makes the rest dead code, but in practice such code
/// is itself terminated by that statement, so a last-statement rule covers the
/// real cases without a full liveness pass. This is intentionally CONSERVATIVE
/// — when unsure (e.g. a possibly-non-exhaustive `match`, or any `for` / bounded
/// `while`), it returns `false`, which can only ever ask the user to add an
/// explicit `return`; it never accepts a body that might fall through.
///
/// Per-statement (on the last statement):
/// - `return <value>` or bare `return` -> definitely returns.
/// - `raise ...` -> diverges (counts as definitely-returns).
/// - `if`/`elif`/`else` -> only when there IS an `else` AND every branch (the
///   `then` block, every `elif` block, and the `else` block) definitely returns.
///   No `else` -> `false` (the implicit empty else falls through).
/// - `while True:` (the LITERAL `True` condition) whose body has no reachable
///   `break` -> diverges (matches codegen lowering `while True` to Rust `loop`).
///   Any other `while`, and every `for`, -> `false` (the loop may run zero times
///   or exit normally).
/// - `match` -> only when it is exhaustive (a `_`/capture arm makes it total)
///   AND every arm body definitely returns; otherwise `false`.
/// - anything else -> `false`.
pub fn block_definitely_returns(stmts: &[Stmt]) -> bool {
    match stmts.last() {
        Some(s) => stmt_definitely_returns(s),
        None => false,
    }
}

pub(crate) fn stmt_definitely_returns(s: &Stmt) -> bool {
    match s {
        // An explicit `return` (with or without a value) terminates the path.
        // A bare `return` in a non-unit function is itself a separate honest
        // error (see the `Stmt::Return(None, _)` arm in `check_stmt`), but for
        // control-flow purposes it still does not fall off the end.
        Stmt::Return(..) => true,
        // `raise` diverges — control never continues past it.
        Stmt::Raise { .. } => true,
        // An `if` only covers all paths when there is an `else` and EVERY branch
        // (then, each elif, else) definitely returns. No `else` -> the implicit
        // empty else falls through, so the `if` cannot guarantee a return.
        Stmt::If { then, elifs, else_: Some(else_block), .. } => {
            block_definitely_returns(then)
                && elifs.iter().all(|(_, b)| block_definitely_returns(b))
                && block_definitely_returns(else_block)
        }
        Stmt::If { else_: None, .. } => false,
        // `while True:` with no reachable `break` is an infinite loop (codegen
        // lowers it to Rust `loop`, which diverges). Any other while/for may be
        // skipped or exit, so it cannot guarantee a return.
        Stmt::While { cond, body, .. } => {
            matches!(cond, Expr::Bool(true, _)) && !body_has_reachable_break(body)
        }
        // A `match` covers all paths only when it is exhaustive (a wildcard or
        // bare-capture arm makes it total) AND every arm body definitely returns.
        // When exhaustiveness is uncertain, treat as falling through.
        Stmt::Match { arms, .. } => {
            arms.iter().any(|arm| {
                matches!(arm.pattern, MatchPattern::Wildcard | MatchPattern::Capture(_))
                    && arm.guard.is_none()
            }) && arms.iter().all(|arm| block_definitely_returns(&arm.body))
        }
        // A `try` definitely returns on every path iff:
        //   (a) there IS a `finally` that definitely returns (it runs on every
        //       exit and itself diverges, so nothing after the try is reachable),
        //   OR
        //   (b) every `except` handler definitely returns AND the value path is
        //       covered: the try BODY definitely returns, OR there is an `else`
        //       that definitely returns (the `else` runs exactly when the body
        //       completed normally, so a returning `else` covers the no-exception
        //       path while the returning handlers cover the exception paths).
        // This is now SOUND because the exception codegen threads a try-body
        // `return`/`break`/`continue` out of the catch_unwind closure (see
        // `Codegen::emit_try`): a returning try body really returns from the
        // function, so no implicit `()` falls off the end (no rustc E0317/E0308).
        //
        // EMPTY handlers (a `try/finally` with no `except`): `handlers.all(..)`
        // is VACUOUSLY true, so the rule reduces to `body_returns || else_returns`
        // — which is exactly right. A `try: return v finally: ...` always runs the
        // body's `return` (an exception in a handler-less body re-raises and
        // diverges, never falling through), so it definitely returns; a
        // `try: <falls through> finally: <no return>` (no handler, no returning
        // finally, body does not return) still evaluates to `false` and stays an
        // honest error.
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            if finally_.as_ref().is_some_and(|f| block_definitely_returns(f)) {
                true
            } else {
                handlers.iter().all(|h| block_definitely_returns(&h.body))
                    && (block_definitely_returns(body)
                        || else_.as_ref().is_some_and(|e| block_definitely_returns(e)))
            }
        }
        _ => false,
    }
}

/// Whether `stmts` contains a `break` that would break out of the loop whose
/// body these statements are — i.e. a `break` reachable at this loop level. A
/// `break` nested inside an INNER `while`/`for` targets that inner loop, not
/// this one, so inner loops are not descended for breaks. Nested `def`/`class`
/// bodies are likewise not descended. `if`/`match`/`with`/`try` blocks ARE
/// descended because a `break` inside them still escapes this loop.
pub(crate) fn body_has_reachable_break(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_has_reachable_break)
}

pub(crate) fn stmt_has_reachable_break(s: &Stmt) -> bool {
    match s {
        Stmt::Break(_) => true,
        Stmt::If { then, elifs, else_, .. } => {
            body_has_reachable_break(then)
                || elifs.iter().any(|(_, b)| body_has_reachable_break(b))
                || else_.as_ref().is_some_and(|b| body_has_reachable_break(b))
        }
        Stmt::Match { arms, .. } => arms.iter().any(|arm| body_has_reachable_break(&arm.body)),
        Stmt::With { body, .. } => body_has_reachable_break(body),
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            body_has_reachable_break(body)
                || handlers.iter().any(|h| body_has_reachable_break(&h.body))
                || else_.as_ref().is_some_and(|b| body_has_reachable_break(b))
                || finally_.as_ref().is_some_and(|b| body_has_reachable_break(b))
        }
        // An inner loop captures its own `break`; do not descend into it.
        Stmt::While { .. } | Stmt::For { .. } => false,
        // A nested function/class owns its own control flow.
        Stmt::Func(_) | Stmt::Class(_) => false,
        _ => false,
    }
}

/// Walk an expression and collect any top-level Ident that is a known param.
/// We stay shallow (just check the expression itself and direct sub-expressions
/// of Tuple/IfExp) to avoid spurious suppression from `return [xs]` or similar.
pub(crate) fn collect_returned_param_idents_expr(
    e: &Expr,
    params: &std::collections::HashSet<String>,
    out: &mut std::collections::HashSet<String>,
) {
    match e {
        Expr::Ident(name, _) => {
            if params.contains(name.as_str()) {
                out.insert(name.clone());
            }
        }
        // `return (a, b)` — both parts count.
        Expr::Tuple(elems, _) => {
            for elem in elems {
                collect_returned_param_idents_expr(elem, params, out);
            }
        }
        // `return x if cond else y` — both branches count.
        Expr::IfExp { body, orelse, .. } => {
            collect_returned_param_idents_expr(body, params, out);
            collect_returned_param_idents_expr(orelse, params, out);
        }
        // Any other expression shape — do not descend. Being conservative here
        // is deliberate: we only suppress the error when the param flows
        // *directly* to the return, not via an arbitrary computation.
        _ => {}
    }
}

pub(crate) fn check_stmt(s: &Stmt, env: &mut FuncEnv) -> Result<()> {
    match s {
        Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => Ok(()),
        Stmt::Assert { cond, msg, .. } => {
            let cond_ty = check_expr(cond, env)?;
            // Generics v1: `assert t` puts a bare type variable in a boolean
            // context (needs truthiness) — rejected like `if t:`.
            reject_typevar_op(&cond_ty, "use as a condition", cond.span())?;
            if let Some(m) = msg { check_expr(m, env)?; }
            Ok(())
        }
        Stmt::Raise { exc, .. } => {
            // The raised value names an exception type (e.g. `ValueError("msg")`
            // or bare `ValueError`). Exception types are not user-defined
            // functions/classes, so don't validate the type name as a callee —
            // only type-check the message arguments.
            match exc {
                Some(Expr::Call { callee, args, .. }) if matches!(callee.as_ref(), Expr::Ident(..)) => {
                    for a in args { check_expr(a, env)?; }
                    Ok(())
                }
                Some(Expr::Ident(..)) => Ok(()),
                Some(e) => { check_expr(e, env)?; Ok(()) }
                None => Ok(()),
            }
        }
        Stmt::Return(None, span) => {
            // In a GENERATOR a bare `return` ends value collection early — it is
            // always allowed regardless of the declared `Iterator[T]` return.
            if !env.is_generator && env.ret_ty != Ty::Unit {
                return Err(Error::Type {
                    span: *span,
                    msg: format!("bare return in function declared to return {}", env.ret_ty),
                });
            }
            Ok(())
        }
        Stmt::Return(Some(e), span) => {
            // A generator yields values; it does NOT return one. `return <value>`
            // inside a generator is an honest error (use `yield`, or a bare
            // `return` to stop early).
            if env.is_generator {
                // Still type-check the expression so its own errors surface.
                let _ = check_expr(e, env)?;
                return Err(Error::Type {
                    span: *span,
                    msg: "a generator cannot `return` a value (it `yield`s values); \
                          use a bare `return` to stop early"
                        .to_string(),
                });
            }
            let ty = check_expr(e, env)?;
            if !types_compatible(&ty, &env.ret_ty, env.ctx) {
                return Err(Error::Type {
                    span: *span,
                    msg: format!("return type mismatch: expected {}, found {}", env.ret_ty, ty),
                });
            }
            Ok(())
        }
        Stmt::Yield(e, span) => {
            // `yield` is only meaningful inside a generator. `check_one_func` /
            // `check_one_method` set `env.is_generator` from the body + a valid
            // `Iterator[T]` return, so a `yield` that reaches here in a
            // non-generator env means the enclosing function is NOT typed as an
            // iterator (the signature check already errored) — but a defensive
            // honest error here covers any path that builds a `FuncEnv` directly.
            let yielded = check_expr(e, env)?;
            if !env.is_generator {
                return Err(Error::Type {
                    span: *span,
                    msg: "`yield` is only valid inside a generator function \
                          declared to return `Iterator[T]`"
                        .to_string(),
                });
            }
            // The element type is the inner `T` of the `Iterator[T]` return,
            // which lowered to `Ty::List(T)`. The yielded value must match `T`.
            if let Ty::List(elem) = &env.ret_ty {
                if !types_compatible(&yielded, elem, env.ctx) {
                    return Err(Error::Type {
                        span: *span,
                        msg: format!(
                            "yield type mismatch: generator yields {}, found {}",
                            elem, yielded
                        ),
                    });
                }
            }
            Ok(())
        }
        Stmt::Expr(e) => {
            check_expr(e, env)?;
            Ok(())
        }
        Stmt::Assign { target, ty, value, span } => {
            let val_ty = check_expr(value, env)?;
            // Generics v1: a local annotation `y: T` inside a generic function
            // resolves `T` to the same `Ty::TypeVar` the params/return use, so an
            // assignment of a type-var value to a type-var-annotated local
            // type-checks (move/clone/assign-to-T-var is allowed). The scope is
            // the enclosing function's type params (empty everywhere else).
            let tp = env.type_param_list();
            let declared = match ty {
                Some(t) => Ty::from_type_expr_scoped(t, *span, &tp)?,
                None => val_ty.clone(),
            };
            if let Some(t) = ty {
                let explicit = Ty::from_type_expr_scoped(t, *span, &tp)?;
                if !types_compatible(&val_ty, &explicit, env.ctx) {
                    return Err(Error::Type {
                        span: *span,
                        msg: format!("type mismatch in assignment: declared {}, got {}", explicit, val_ty),
                    });
                }
            }
            // NOTE: bare reassignment to a different concrete type is intentionally
            // allowed — codegen emits a shadowing `let`, so pyrst supports Python's
            // type-changing rebind (e.g. an int accumulator later assigned a float,
            // or a name reused for a different value). Rejecting it here would
            // contradict that feature.
            // Track when an original parameter is rebound so that subsequent mutations
            // on the new local value are NOT flagged as by-value param mutations.
            if env.params.contains(target.as_str()) {
                env.reassigned_params.insert(target.clone());
            }
            env.locals.insert(target.clone(), declared);
            Ok(())
        }
        Stmt::AugAssign { target, value, span, .. } => {
            if env.locals.get(target.as_str()).is_none() && !env.ctx.funcs.contains_key(target.as_str()) {
                return Err(Error::Type {
                    span: *span,
                    msg: format!("undefined variable `{}`", target),
                });
            }
            // Generics v1: `x += y` desugars to `x = x <op> y`, so an augmented
            // assignment whose TARGET (or RHS) is a bare type variable applies an
            // operator to a generic value — needs a bound (E0368 otherwise).
            // Reject it honestly here, mirroring the `Expr::BinOp` op-on-`T` gate.
            if let Some(target_ty) = env.locals.get(target.as_str()).cloned() {
                reject_typevar_op(&target_ty, "apply an operator to", *span)?;
            }
            let val_ty = check_expr(value, env)?;
            reject_typevar_op(&val_ty, "apply an operator to", *span)?;
            Ok(())
        }
        Stmt::Unpack { targets, value, span } => {
            let val_ty = check_expr(value, env)?;
            // Generics v1: destructuring a bare type variable (`a, b = t` where
            // `t: T`) needs the value to have a known tuple SHAPE — a `T` is
            // opaque, so this is an honest error (it would otherwise emit a
            // tuple-pattern bind against an opaque `T` and fail rustc).
            reject_typevar_op(&val_ty, "unpack", *span)?;
            let elem_tys = match &val_ty {
                Ty::Tuple(tys) => tys.clone(),
                _ => vec![Ty::Unknown; targets.len()],
            };
            for (i, t) in targets.iter().enumerate() {
                let ty = elem_tys.get(i).cloned().unwrap_or(Ty::Unknown);
                env.locals.insert(t.clone(), ty);
            }
            Ok(())
        }
        Stmt::If { cond, then, elifs, else_, .. } => {
            let cond_ty = check_expr(cond, env)?;
            // Generics v1: a bare type variable in a boolean context (`if t:`)
            // needs truthiness, which a generic value lacks (no Bool coercion in
            // v1). A narrowing guard (`if x is not None:`) is a `BinOp` typed
            // Bool, so it is never a bare `TypeVar` and is unaffected.
            reject_typevar_op(&cond_ty, "use as a condition", cond.span())?;
            // (EPIC-5) None-guard narrowing. For `if x is not None:` the THEN
            // branch sees `x: T` (the non-None payload); for `if x is None:` the
            // ELSE branch sees `x: T`. `x` must be a local typed `Option(T)`.
            // We narrow only the directly-guarded branch and save/restore the
            // local's type so the narrowing never leaks past the `if`.
            let guard = extract_none_guard(cond)
                .and_then(|(name, is_not_none)| match env.locals.get(name.as_str()) {
                    Some(Ty::Option(inner)) => Some((name, is_not_none, (**inner).clone())),
                    _ => None,
                });
            // THEN branch: narrowed iff the guard is `is not None`.
            {
                let restore = guard.as_ref().filter(|(_, is_not_none, _)| *is_not_none)
                    .map(|(name, _, inner)| {
                        let prev = env.locals.insert(name.clone(), inner.clone());
                        (name.clone(), prev)
                    });
                check_body(then, env)?;
                if let Some((name, prev)) = restore {
                    match prev { Some(t) => { env.locals.insert(name, t); } None => { env.locals.remove(name.as_str()); } }
                }
            }
            for (c, b) in elifs {
                let c_ty = check_expr(c, env)?;
                reject_typevar_op(&c_ty, "use as a condition", c.span())?;
                check_body(b, env)?;
            }
            // ELSE branch: narrowed iff the guard is `is None` (so the else is the
            // non-None case). Skipped when there are elifs, since the else then
            // belongs to a different condition.
            if let Some(b) = else_ {
                let restore = guard.as_ref()
                    .filter(|(_, is_not_none, _)| !*is_not_none && elifs.is_empty())
                    .map(|(name, _, inner)| {
                        let prev = env.locals.insert(name.clone(), inner.clone());
                        (name.clone(), prev)
                    });
                check_body(b, env)?;
                if let Some((name, prev)) = restore {
                    match prev { Some(t) => { env.locals.insert(name, t); } None => { env.locals.remove(name.as_str()); } }
                }
            }
            Ok(())
        }
        Stmt::While { cond, body, .. } => {
            let cond_ty = check_expr(cond, env)?;
            // Generics v1: a bare type variable as a loop condition (`while t:`)
            // needs truthiness — rejected (see the `if` arm).
            reject_typevar_op(&cond_ty, "use as a condition", cond.span())?;
            check_body(body, env)
        }
        Stmt::For { targets, iter, body, span } => {
            let iter_ty = check_expr(iter, env)?;
            // Generics v1: iterating a bare type variable (`for it in xs` where
            // `xs: T`) needs an `IntoIterator` bound — `T` is opaque, with no
            // `.iter()`. Reject it honestly (E0599 otherwise). Iterating a
            // `list[T]`/`dict[K, V]` whose ELEMENT is a type var is fine and
            // yields the element/key type below.
            reject_typevar_op(&iter_ty, "iterate over", *span)?;
            // Determine element type from iterator type
            let elem_ty = match &iter_ty {
                Ty::List(inner) => *inner.clone(),
                Ty::Set(inner) => *inner.clone(),
                // Iterating a dict yields its KEYS (Python semantics).
                Ty::Dict(key, _) => *key.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Unknown,
            };
            // Bind all targets
            if targets.len() == 1 {
                // Single target gets the full element type
                env.locals.insert(targets[0].clone(), elem_ty.clone());
            } else {
                // Multiple targets: if the element type is a tuple of matching
                // arity (e.g. iterating dict.items() -> List[Tuple[K, V]]), bind
                // each target to its component type. Otherwise fall back to
                // Unknown (mirrors the Stmt::Unpack destructuring above).
                let elem_tys = match &elem_ty {
                    Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
                    _ => vec![Ty::Unknown; targets.len()],
                };
                for (i, target) in targets.iter().enumerate() {
                    let ty = elem_tys.get(i).cloned().unwrap_or(Ty::Unknown);
                    env.locals.insert(target.clone(), ty);
                }
            }
            check_body(body, env)?;
            Ok(())
        }
        Stmt::Import { .. } => Ok(()), // Ignored in v0
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            check_body(body, env)?;
            for h in handlers {
                if let Some(name) = &h.exc_name {
                    // The bound exception value is the panic message string.
                    env.locals.insert(name.clone(), Ty::Str);
                }
                check_body(&h.body, env)?;
            }
            if let Some(b) = else_ { check_body(b, env)?; }
            if let Some(b) = finally_ { check_body(b, env)?; }
            Ok(())
        }
        Stmt::With { ctx_expr, as_name, body, .. } => {
            let ctx_ty = check_expr(ctx_expr, env)?;
            // Generics v1: a `with t as r:` context manager needs the
            // enter/exit protocol (in pyrst, a concrete `file` handle). A bare
            // type variable is opaque — reject it honestly (it would otherwise
            // emit context-manager glue against an opaque `T` and fail rustc).
            reject_typevar_op(&ctx_ty, "use as a context manager", ctx_expr.span())?;
            // Bound name is block-scoped in codegen; save/restore so a stale type
            // does not leak past the block (mirrors the for-loop handling).
            let saved = as_name.as_ref().map(|n| (n.clone(), env.locals.get(n).cloned()));
            if let Some(name) = as_name {
                env.locals.insert(name.clone(), ctx_ty);
            }
            check_body(body, env)?;
            if let Some((name, prev)) = saved {
                match prev {
                    Some(ty) => { env.locals.insert(name, ty); }
                    None => { env.locals.remove(name.as_str()); }
                }
            }
            Ok(())
        }
        Stmt::Del { target, .. } => {
            check_expr(target, env)?;
            Ok(())
        }
        Stmt::Match { subject, arms, span } => {
            let subject_ty = check_expr(subject, env)?;
            // Generics v1: matching a bare type variable against a LITERAL pattern
            // (`case 0:` / `case "x":`) lowers to a Rust literal match, which needs
            // `PartialEq` on the subject (E0369 otherwise). A match whose arms are
            // ALL wildcard/capture patterns needs no comparison and is fine. Reject
            // only when the subject is a type var AND at least one arm discriminates
            // on a literal — an honest error instead of a deferred rustc failure.
            if matches!(subject_ty, Ty::TypeVar(_))
                && arms.iter().any(|arm| pattern_discriminates(&arm.pattern))
            {
                reject_typevar_op(&subject_ty, "match on a literal pattern against", *span)?;
            }
            for arm in arms {
                // A `case <name>:` (capture) pattern BINDS `<name>` to the subject's
                // value for the duration of this arm — its GUARD and its body (so
                // both `case y if y > 10:` and `return y + 1` type-check). Insert the
                // binding BEFORE checking the guard, scope it to the arm, then restore
                // the prior binding (or remove it) so the capture name never leaks to
                // a sibling arm or past the match. `_` (Wildcard) and literal patterns
                // introduce no binding.
                let saved_capture = match &arm.pattern {
                    MatchPattern::Capture(name) => {
                        let prev = env.locals.get(name).cloned();
                        env.locals.insert(name.clone(), subject_ty.clone());
                        Some((name.clone(), prev))
                    }
                    _ => None,
                };
                // Check guard if present (may reference the capture binding).
                if let Some(guard) = &arm.guard {
                    check_expr(guard, env)?;
                }
                for s in &arm.body {
                    check_stmt(s, env)?;
                }
                if let Some((name, prev)) = saved_capture {
                    match prev {
                        Some(ty) => { env.locals.insert(name, ty); }
                        None => { env.locals.remove(name.as_str()); }
                    }
                }
            }
            Ok(())
        }
        Stmt::AttrAssign { obj, attr, value, span } => {
            // Validate the target base chain (the base expr must type-check;
            // unknown names / bad nested attributes are rejected by check_expr).
            let obj_ty = check_expr(obj, env)?;
            check_expr(value, env)?;
            // Detect mutation of a by-value non-Copy parameter.
            // `param.field = v` where `param` is still the original binding is a
            // silent wrong-output bug — the caller's value is never updated.
            // Exception: if the param is returned by the function, the mutation
            // is the caller's own copy that gets handed back — a valid pattern.
            if let Some(root) = root_ident(obj) {
                if root != "self"
                    && env.params.contains(root)
                    && !env.reassigned_params.contains(root)
                    && !env.returned_params.contains(root)
                    && !env.by_ref_params.contains(root)
                    && is_owned(&obj_ty)
                {
                    return Err(Error::Type {
                        span: *span,
                        msg: by_value_mutation_error(root),
                    });
                }
            }
            // If the base is a known user class, the assigned field must exist on
            // it (including inherited fields) — `a.b.c = v` with no field `c` is a
            // type error, not a deferred-to-rustc one.
            if let Ty::Class(class_name, _) = &obj_ty {
                if env.ctx.classes.contains_key(class_name.as_str()) {
                    let has_field = env.ctx.get_all_fields(class_name.as_str())
                        .iter().any(|f| &f.name == attr);
                    if !has_field {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("class `{}` has no attribute `{}`", class_name, attr),
                        });
                    }
                }
            }
            Ok(())
        }
        Stmt::IndexAssign { obj, idx, value, span } => {
            // Validate the target base chain, the subscript, and the value.
            let obj_ty = check_expr(obj, env)?;
            check_expr(idx, env)?;
            check_expr(value, env)?;
            // Detect mutation of a by-value non-Copy parameter via index assignment.
            // Exception: if the param is returned by the function, the mutation is valid.
            if let Some(root) = root_ident(obj) {
                if root != "self"
                    && env.params.contains(root)
                    && !env.reassigned_params.contains(root)
                    && !env.returned_params.contains(root)
                    && !env.by_ref_params.contains(root)
                    && is_owned(&obj_ty)
                {
                    return Err(Error::Type {
                        span: *span,
                        msg: by_value_mutation_error(root),
                    });
                }
            }
            Ok(())
        }
        // (first-class functions, Increment 2) A NESTED `def` lowers to a NAMED
        // local closure. Register it as a `Ty::Func` local in the ENCLOSING scope
        // (so it is callable / returnable / passable like any Callable value) and
        // type-check its body with the enclosing locals + params VISIBLE (lexical
        // capture) plus its own params. Define-before-use: it is in scope from
        // here onward, exactly like a local assignment. A nested `class` is still
        // out of scope (punted).
        Stmt::Func(f) => check_nested_def(f, env),
        Stmt::Class(_) => Ok(()), // Nested class — punt in v0.
    }
}

/// (first-class functions, Increment 2) Type-check a NESTED `def` and register it
/// as a named `Ty::Func` LOCAL in the enclosing function environment `env`.
///
/// A nested def lowers (in codegen) to a `move` closure `Rc<dyn Fn(..) -> Ret>`
/// bound to a `let <name>`; here we establish the matching type discipline:
///  - the nested def's signature becomes a `Ty::Func(param_tys, ret)` local so
///    `<name>(args)` type-checks and the value can be returned / passed / stored;
///  - the body is checked in a FRESH `FuncEnv` whose locals start as the
///    enclosing locals (LEXICAL CAPTURE) plus the nested params, with the nested
///    def's own return type and the same generic type-parameter scope;
///  - the all-paths-return / honest-missing-return gate applies to the body too.
///
/// SOUNDNESS GATES (Increment 2 scope), each an honest error rather than emitting
/// broken Rust:
///  - SELF-RECURSION is rejected: a Rust closure cannot name itself in its own
///    initializer, so a nested def that calls its own name cannot be lowered.
///  - MUTATING A CAPTURED enclosing variable is rejected: capture is by value
///    (`move` + clone), so an assignment to a captured (non-param, non-local)
///    name would silently fail to propagate to the enclosing scope.
///  - NESTED GENERICS and NESTED GENERATORS (a `yield` in the nested body) are
///    rejected: a closure has no place for Rust generic params, and the eager
///    generator desugar targets a `fn` return slot, not a closure.
///  - Decorators on a nested def are not supported.
pub(crate) fn check_nested_def(f: &Func, env: &mut FuncEnv) -> Result<()> {
    if !f.decorators.is_empty() {
        return Err(Error::Type {
            span: f.span,
            msg: "decorators on a nested function are not supported".to_string(),
        });
    }
    if !f.type_params.is_empty() {
        return Err(Error::Type {
            span: f.span,
            msg: "a nested function cannot declare type parameters (generics are \
                  only supported on top-level functions)"
                .to_string(),
        });
    }
    if body_contains_yield(&f.body) {
        return Err(Error::Type {
            span: f.span,
            msg: "a nested function cannot be a generator (`yield` is only \
                  supported in a top-level function or method)"
                .to_string(),
        });
    }

    // SELF-RECURSION: a Rust closure cannot reference itself in its own scope, so
    // a nested def that calls its own name cannot be lowered. Reject it honestly.
    let mut called = std::collections::HashSet::new();
    for s in &f.body {
        collect_calls_from_stmt(s, &mut called);
    }
    if called.contains(&f.name) {
        return Err(Error::Type {
            span: f.span,
            msg: format!(
                "recursive nested function `{}` is not supported \
                 (a nested closure cannot call itself — use a top-level function)",
                f.name
            ),
        });
    }

    // Lower the nested signature (scoped to the ENCLOSING function's type params,
    // so a nested def inside a generic function may still name them in annotations
    // — they are opaque type variables there, never bound by the nested def).
    let tp = env.type_param_list();
    let params: Vec<(String, Ty)> = f.params.iter()
        .map(|p| Ty::from_type_expr_scoped(&p.ty, p.span, &tp).map(|ty| (p.name.clone(), ty)))
        .collect::<Result<Vec<_>>>()?;
    let ret = Ty::from_type_expr_scoped(&f.ret, f.span, &tp)?;

    // The nested def's PARAM names: assignments to these inside the body are the
    // closure's own bindings (fine), NOT captured-variable mutations.
    let nested_param_names: std::collections::HashSet<&str> =
        f.params.iter().map(|p| p.name.as_str()).collect();

    // MUTATE-CAPTURED gate: capture is by value (`move` + clone), so an assignment
    // to a name that is VISIBLE in the enclosing scope but is neither a nested
    // param nor a nested-local would not propagate to the enclosing scope. Reject
    // it honestly. A nested-local (a name first BOUND inside the body) is allowed;
    // we seed `nested_locals` with the params and grow it as we scan assignments,
    // so an assignment to a fresh name (a new nested local) is never flagged.
    {
        let mut nested_locals: std::collections::HashSet<String> =
            nested_param_names.iter().map(|s| s.to_string()).collect();
        reject_captured_mutation(&f.body, env, &mut nested_locals)?;
    }

    // Register the nested def as a callable local in the ENCLOSING scope BEFORE
    // checking the body, so a LATER nested def (or a recursive-looking forward
    // reference, already rejected above) sees it, and so the enclosing body can
    // call/return/pass it from this point onward (define-before-use).
    env.locals.insert(f.name.clone(), Ty::Func(
        params.iter().map(|(_, t)| t.clone()).collect(),
        Box::new(ret.clone()),
    ));

    // Check the nested body in a FRESH environment that CAPTURES the enclosing
    // locals (every enclosing param/local/earlier-nested-def is visible) and adds
    // the nested params on top (nested params SHADOW an enclosing name of the
    // same identifier). The nested def's return type drives its own `return`
    // checks and missing-return gate.
    let mut nested_env = FuncEnv::with_by_ref(env.ctx, &params, &[], ret);
    // Lexical capture: start from the enclosing locals, then overlay the nested
    // params (so a param shadows a captured name).
    for (k, v) in &env.locals {
        nested_env.locals.entry(k.clone()).or_insert_with(|| v.clone());
    }
    // The nested params must keep their own (possibly shadowing) types.
    for (name, ty) in &params {
        nested_env.locals.insert(name.clone(), ty.clone());
    }
    // Carry the enclosing generic type-parameter scope so an op on a captured
    // type-var value is still rejected by the same gate inside the nested body.
    nested_env.type_params = env.type_params.clone();
    collect_returned_param_idents(&f.body, &nested_env.params, &mut nested_env.returned_params);
    check_body(&f.body, &mut nested_env)?;
    check_all_paths_return(&f.body, &nested_env, &f.name, f.span)?;
    Ok(())
}

/// (first-class functions, Increment 2) Walk a nested def's body and reject any
/// assignment to a CAPTURED enclosing variable — a name that is visible in the
/// enclosing scope `env` but is not one of the nested def's own bindings
/// (`nested_locals`, seeded with its params and grown as new locals are bound).
/// Capture is by value (`move` + clone), so such a mutation would not propagate
/// to the enclosing scope; rejecting it keeps the by-value capture honest.
///
/// A bare `Stmt::Assign`/`Unpack` to a FRESH name introduces a new nested local
/// (recorded in `nested_locals`), so it is never flagged. An assignment to a name
/// already in `nested_locals` is the closure mutating its OWN binding — allowed.
/// In-place mutations (`AttrAssign`/`IndexAssign`/`AugAssign`) whose ROOT names a
/// captured variable are also rejected (they mutate the captured value's interior).
pub(crate) fn reject_captured_mutation(
    stmts: &[Stmt],
    env: &FuncEnv,
    nested_locals: &mut std::collections::HashSet<String>,
) -> Result<()> {
    // True when `name` is a captured enclosing variable (visible in `env` but not
    // a nested-local binding). Top-level functions / classes resolved via the
    // enclosing env's `lookup` are NOT plain locals (they are global callables),
    // so reassigning such a name is a separate concern — we only police names that
    // are enclosing LOCALS (params/locals of the outer function).
    let is_captured = |name: &str, locals: &std::collections::HashSet<String>| {
        !locals.contains(name) && env.locals.contains_key(name)
    };
    for s in stmts {
        match s {
            Stmt::Assign { target, span, .. } => {
                if is_captured(target, nested_locals) {
                    return Err(captured_mutation_err(target, *span));
                }
                // A fresh assignment binds a new nested local (shadows capture).
                nested_locals.insert(target.clone());
            }
            Stmt::Unpack { targets, span, .. } => {
                for t in targets {
                    if is_captured(t, nested_locals) {
                        return Err(captured_mutation_err(t, *span));
                    }
                }
                for t in targets { nested_locals.insert(t.clone()); }
            }
            Stmt::AugAssign { target, span, .. } => {
                if is_captured(target, nested_locals) {
                    return Err(captured_mutation_err(target, *span));
                }
            }
            Stmt::AttrAssign { obj, span, .. } => {
                if let Some(root) = root_ident(obj) {
                    if is_captured(root, nested_locals) {
                        return Err(captured_mutation_err(root, *span));
                    }
                }
            }
            Stmt::IndexAssign { obj, span, .. } => {
                if let Some(root) = root_ident(obj) {
                    if is_captured(root, nested_locals) {
                        return Err(captured_mutation_err(root, *span));
                    }
                }
            }
            // Recurse into nested control-flow blocks. A name first bound inside a
            // block is conservatively treated as a nested local from then on
            // (pyrst hoists block-locals to function scope), which is sound for
            // the gate: it only ever makes the check MORE permissive for the
            // closure's own names, never admitting a captured-variable mutation.
            Stmt::If { then, elifs, else_, .. } => {
                reject_captured_mutation(then, env, nested_locals)?;
                for (_, b) in elifs { reject_captured_mutation(b, env, nested_locals)?; }
                if let Some(b) = else_ { reject_captured_mutation(b, env, nested_locals)?; }
            }
            Stmt::While { body, .. } | Stmt::With { body, .. } => {
                reject_captured_mutation(body, env, nested_locals)?;
            }
            Stmt::For { targets, body, .. } => {
                for t in targets { nested_locals.insert(t.clone()); }
                reject_captured_mutation(body, env, nested_locals)?;
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                reject_captured_mutation(body, env, nested_locals)?;
                for h in handlers {
                    if let Some(n) = &h.exc_name { nested_locals.insert(n.clone()); }
                    reject_captured_mutation(&h.body, env, nested_locals)?;
                }
                if let Some(b) = else_ { reject_captured_mutation(b, env, nested_locals)?; }
                if let Some(b) = finally_ { reject_captured_mutation(b, env, nested_locals)?; }
            }
            Stmt::Match { arms, .. } => {
                for arm in arms { reject_captured_mutation(&arm.body, env, nested_locals)?; }
            }
            // A doubly-nested def owns its OWN capture analysis (checked when its
            // enclosing nested def is checked); don't descend here.
            Stmt::Func(_) | Stmt::Class(_) => {}
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn captured_mutation_err(name: &str, span: Span) -> Error {
    Error::Type {
        span,
        msg: format!(
            "nested function cannot mutate the captured variable `{}` \
             (closures capture by value; assign to a local inside the nested \
             function, or return the new value instead)",
            name
        ),
    }
}

// --- Builtin method tables (S4 soundness check) ---
// Superset of every method codegen handles (special-cased or valid Rust
// passthrough) and every method the example suite calls on a concrete receiver.
pub(crate) const STR_METHODS: &[&str] = &[
    "upper", "lower", "strip", "lstrip", "rstrip", "split",
    "splitlines", "join", "startswith", "endswith", "replace", "removeprefix",
    "removesuffix", "expandtabs", "partition", "rpartition", "find", "rfind",
    "index", "rindex", "count", "contains", "isdigit", "isalpha", "isupper",
    "islower", "isspace", "isalnum", "isidentifier", "isnumeric", "isprintable",
    "istitle", "capitalize", "title", "zfill", "ljust", "rjust",
    "center", "swapcase", "len",
    // NOTE: casefold/encode/isdecimal/rsplit/format removed — codegen cannot
    // emit them and they are absent from the example corpus (card 36f66dd2).
];
pub(crate) const LIST_METHODS: &[&str] = &[
    "append", "extend", "insert", "remove", "pop", "index", "count",
    "reverse", "sort", "clear", "copy", "len", "contains",
];
pub(crate) const SET_METHODS: &[&str] = &[
    "add", "discard", "remove", "clear", "copy", "pop", "len", "union",
    "intersection", "difference", "symmetric_difference", "issubset",
    "issuperset", "isdisjoint", "update", "contains",
];
pub(crate) const DICT_METHODS: &[&str] = &[
    "get", "keys", "values", "items", "pop", "clear", "copy", "update",
    "len", "contains",
    // NOTE: setdefault/popitem removed — codegen cannot emit them and they are
    // absent from the example corpus (card 36f66dd2).
];
pub(crate) const FILE_METHODS: &[&str] = &["read", "readlines", "write", "close"];

/// Returns (type-name, method-table) for a concrete builtin receiver, or None
/// for Unknown/Class/numeric receivers (the check must not run on those).
pub(crate) fn builtin_method_table(ty: &Ty) -> Option<(&'static str, &'static [&'static str])> {
    match ty {
        Ty::Str => Some(("str", STR_METHODS)),
        Ty::List(_) => Some(("list", LIST_METHODS)),
        Ty::Set(_) => Some(("set", SET_METHODS)),
        Ty::Dict(_, _) => Some(("dict", DICT_METHODS)),
        Ty::File => Some(("file", FILE_METHODS)),
        _ => None,
    }
}

/// Mutators whose single argument must be assignable to the receiver's element
/// type. Restricted to set mutators (list `.append` excluded: empty-list field
/// inference defaults to list[int] and would risk false rejections). Returns the
/// element type to check the argument against.
pub(crate) fn elem_arg_check_ty(recv: &Ty, method: &str) -> Option<Ty> {
    match recv {
        Ty::Set(elem) if matches!(method, "add" | "discard" | "remove") => Some((**elem).clone()),
        _ => None,
    }
}

/// Concrete return type of a builtin method call on a known builtin receiver
/// (str/list/set/dict); unrecognized methods or receivers return Unknown.
/// This is the single source of truth — codegen's type_of_expr delegates here.
/// Note: pyrst models str.partition/rpartition as list[str] (not a tuple),
/// matching codegen and the example fixtures.
pub fn builtin_method_ret(recv: &Ty, method: &str) -> Ty {
    match recv {
        Ty::Str => match method {
            "upper" | "lower" | "strip" | "lstrip" | "rstrip" | "replace"
            | "capitalize" | "title" | "swapcase" | "zfill"
            | "ljust" | "rjust" | "center" | "removeprefix" | "removesuffix"
            | "expandtabs" | "join" => Ty::Str,
            // NOTE: casefold/encode/format/rsplit removed from str arms —
            // codegen cannot emit them (card 36f66dd2 stopgap).
            "split" | "splitlines" | "partition" | "rpartition" => {
                Ty::List(Box::new(Ty::Str))
            }
            "find" | "rfind" | "index" | "rindex" | "count" => Ty::Int,
            "startswith" | "endswith" | "isdigit" | "isalpha" | "isupper" | "islower"
            | "isspace" | "isalnum" | "isidentifier" | "isnumeric" | "isprintable"
            | "istitle" => Ty::Bool,
            // NOTE: isdecimal removed — codegen cannot emit it (card 36f66dd2).
            _ => Ty::Unknown,
        },
        // Concrete element/collection returns, plus in-place mutators typed as
        // Unit (card 2b3bf7f5; audited: no example assigns/chains a mutator
        // result). Deliberately still Unknown: dict.get / dict.setdefault, which
        // need arg-count-aware typing (the 2-arg `get(k, default)` returns V,
        // not Optional[V]).
        Ty::List(elem) => match method {
            "pop" => (**elem).clone(),
            "copy" => Ty::List(elem.clone()),
            "index" | "count" => Ty::Int,
            // In-place mutators return None (audited: no example assigns/chains them).
            "append" | "extend" | "insert" | "remove" | "sort" | "reverse" | "clear" => Ty::Unit,
            _ => Ty::Unknown,
        },
        Ty::Set(elem) => match method {
            "union" | "intersection" | "difference" | "symmetric_difference" | "copy" => {
                Ty::Set(elem.clone())
            }
            "pop" => (**elem).clone(),
            "issubset" | "issuperset" | "isdisjoint" => Ty::Bool,
            // In-place mutators return None.
            "add" | "discard" | "remove" | "update" | "intersection_update"
            | "difference_update" | "symmetric_difference_update" | "clear" => Ty::Unit,
            _ => Ty::Unknown,
        },
        Ty::Dict(key, val) => match method {
            "keys" => Ty::List(key.clone()),
            "values" => Ty::List(val.clone()),
            "copy" => Ty::Dict(key.clone(), val.clone()),
            "items" => Ty::List(Box::new(Ty::Tuple(vec![(**key).clone(), (**val).clone()]))),
            "pop" => (**val).clone(),
            // In-place mutators return None. (get/setdefault return V/Optional and
            // are deliberately left Unknown — they need arg-count-aware typing.)
            "update" | "clear" => Ty::Unit,
            _ => Ty::Unknown,
        },
        Ty::File => match method {
            "read" => Ty::Str,
            "readlines" => Ty::List(Box::new(Ty::Str)),
            "write" | "close" => Ty::Unit,
            _ => Ty::Unknown,
        },
        _ => Ty::Unknown,
    }
}

/// Arg-count-aware return type for `dict.get`, which `builtin_method_ret` cannot
/// express (it has no view of the call's arguments). Python's `d.get(k)` returns
/// `Optional[V]` (None when absent), while `d.get(k, default)` returns `V`. Both
/// the inference oracle (`infer_expr_ty`) and the error-producing checker
/// (`check_expr`) route dict.get through here so the two never drift. For any
/// non-dict receiver (or a non-`get` method) it returns None, leaving the caller
/// to fall back to `builtin_method_ret`.
pub fn dict_get_ret(recv: &Ty, method: &str, argc: usize) -> Option<Ty> {
    if method != "get" {
        return None;
    }
    if let Ty::Dict(_key, val) = recv {
        // 1-arg get -> Optional[V]; 2-arg get(k, default) -> V.
        if argc <= 1 {
            Some(Ty::Option(val.clone()))
        } else {
            Some((**val).clone())
        }
    } else {
        None
    }
}

