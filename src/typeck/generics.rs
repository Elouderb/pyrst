use super::*;

pub(crate) fn check_body(stmts: &[Stmt], env: &mut FuncEnv) -> Result<()> {
    for s in stmts {
        check_stmt(s, env)?;
    }
    Ok(())
}

/// Check if two types are compatible for assignment.
/// Collections with Unknown element types are considered compatible with any collection of the same kind.
pub(crate) fn types_compatible(val_ty: &Ty, declared_ty: &Ty, ctx: &TyCtx) -> bool {
    match (val_ty, declared_ty) {
        // Exact match
        (a, b) if a == b => true,
        // (EPIC-5 C1-B) A `Derived` value satisfies a `Base` slot. `is_subclass`
        // is reflexive, but the `a == b` arm above already handled the equal-name
        // case, so this arm only adds the strictly-derived direction. It is
        // DIRECTIONAL: a Derived flows into a Base slot, never the reverse
        // (`is_subclass(Base, Derived)` is false), matching the value-flow meaning
        // of `types_compatible(val_ty, declared_ty)`. Builtins (e.g. Exception)
        // are not in `ctx.classes`, so exception subtyping stays an honest error.
        // NOTE: typeck ACCEPTS this here; codegen still rejects it via the
        // honest gate (EPIC-5 C1-C) until the C2 companion-enum codegen lands.
        (Ty::Class(d, _), Ty::Class(b, _)) if is_subclass(d, b, ctx) => true,
        // Unknown is compatible with anything
        (Ty::Unknown, _) | (_, Ty::Unknown) => true,
        // List with Unknown elements compatible with any List
        (Ty::List(inner), Ty::List(_)) if **inner == Ty::Unknown => true,
        (Ty::List(_), Ty::List(inner)) if **inner == Ty::Unknown => true,
        // Set with Unknown elements compatible with any Set
        (Ty::Set(inner), Ty::Set(_)) if **inner == Ty::Unknown => true,
        (Ty::Set(_), Ty::Set(inner)) if **inner == Ty::Unknown => true,
        // Dict with Unknown key/value compatible with any Dict
        (Ty::Dict(k, v), Ty::Dict(_, _)) if **k == Ty::Unknown && **v == Ty::Unknown => true,
        (Ty::Dict(_, _), Ty::Dict(k, v)) if **k == Ty::Unknown && **v == Ty::Unknown => true,
        // ── LAZY-GEN V1-d: an Iterator is NOT interchangeable with a List ─────
        // V1-a made `Iterator[T]` and `list[T]` mutually assignable here (a
        // behavior-invisible bridge while the variant split landed). V1-d FLIPS
        // that: a generator is not a list. Only an `Iterator[T]` fills an
        // `Iterator[T]` slot (recursing on the element preserves equal/Unknown
        // rules); both CROSS directions now fall through to `_ => false`:
        //   • Iterator → list slot: honest MATERIALIZE error (`list(g)`), produced
        //     with a helpful message by `reject_iterator_into_list` at the arg /
        //     return / assignment sites before this check reports the bare mismatch.
        //   • list → Iterator slot: the `list → __PyrstGen` adapter is a V2 feature;
        //     it stays a plain type-mismatch error until then.
        // (docs/design/lazy-generators.md §D.2/§F.)
        (Ty::Iterator(v), Ty::Iterator(d)) => types_compatible(v, d, ctx),
        // ── Optional / None ──────────────────────────────────────────────────
        // (EPIC-5) `types_compatible(val_ty, declared_ty)` is directional: it asks
        // whether a value of `val_ty` may flow into a slot of `declared_ty`. The
        // Option arms below are written so a value may FILL an Optional slot, but
        // an Optional value may NOT silently fill a bare slot — using an
        // `Optional[T]` as a bare `T` without narrowing stays an honest error.
        //
        // The `None` LITERAL has its own type `Ty::NoneVal`, kept strictly
        // separate from `Ty::Unit` (a *void function's* `-> None` return). This
        // separation is load-bearing: were they the same, a void call result
        // (`Ty::Unit`) would wrongly satisfy an Optional slot and codegen would
        // emit `Some(void_call())` -> `Option<()>` — a silent miscompile. So a
        // void result is NOT compatible with Optional; only the literal `None` is.
        //
        // 1a. The `None` literal fills any Optional slot regardless of inner type
        //     (`None` is a valid `Optional[Class]`). Placed before the bare-value
        //     arm so it never recurses into the (incompatible) inner type.
        (Ty::NoneVal, Ty::Option(_)) => true,
        // 1b. The `None` literal also satisfies a `-> None` (void) return — this
        //     is what makes `return None` typecheck in a void function (the
        //     Return path compares the value type against the declared Unit ret).
        (Ty::NoneVal, Ty::Unit) => true,
        // 1c. Two `None` literals are mutually compatible (e.g. branch unification
        //     of `None`/`None`, or `x = None` re-checked against itself).
        (Ty::NoneVal, Ty::NoneVal) => true,
        // 2. Optional[A] fills Optional[B] when the inner types are compatible
        //    (covers Optional[Unknown] permissively, and Optional[T]~Optional[T]).
        (Ty::Option(a), Ty::Option(b)) => types_compatible(a, b, ctx),
        // 3. A bare value of type A fills Optional[B] when A fits B (auto-Some).
        //    Checked AFTER the Option/Option arm so an Optional value never takes
        //    this path. `NoneVal` is excluded (it is handled by 1a above, never by
        //    recursing into the inner type). Codegen wraps the bare value in
        //    `Some(...)` at the site.
        (a, Ty::Option(b)) if !matches!(a, Ty::Option(_) | Ty::NoneVal) => types_compatible(a, b, ctx),
        // ── Function values ──────────────────────────────────────────────────
        // A `Ty::Func` value fits a `Ty::Func` slot when the arities match and
        // each argument type and the return type are compatible. Argument
        // positions are CONTRAVARIANT in theory, but pyrst's function values are
        // monomorphic (`Rc<dyn Fn(A) -> R>`) and the only inexact case in
        // Increment 1 is an `Unknown` from an untyped lambda parameter / body,
        // which `types_compatible` already treats as universally compatible in
        // either direction. So a direction-agnostic per-position check is both
        // sound for the supported cases and permissive for the Unknown ones
        // (e.g. a lambda inferred `Callable[[unknown], unknown]` fills a declared
        // `Callable[[int], int]`).
        (Ty::Func(va, vr), Ty::Func(da, dr)) => {
            va.len() == da.len()
                && va.iter().zip(da.iter()).all(|(v, d)| types_compatible(v, d, ctx))
                && types_compatible(vr, dr, ctx)
        }
        // Otherwise not compatible
        _ => false,
    }
}

// ── Generics v1: call-site unification + substitution ────────────────────────
//
// When a parametric generic function `def f[T, U](..)` is called, each declared
// parameter type (which may CONTAIN `Ty::TypeVar`s) is structurally unified
// against the corresponding actual argument type, accumulating a substitution
// `{T -> concrete}`. A type variable that appears in more than one position must
// bind CONSISTENTLY; substituting the result into the declared return type gives
// the call's concrete result type. The same machinery is consumed by BOTH the
// error-checking `check_expr` Call arm and the codegen-facing `infer_expr_ty`
// oracle (via `infer_generic_call_result`), so the two never drift on what a
// generic call returns.

/// Structurally UNIFY a declared parameter type `declared` (which may contain
/// `Ty::TypeVar`s drawn from `type_params`) against the actual argument type
/// `actual`, recording each variable's binding in `subst`. Returns `Err(msg)` on
/// a CONFLICTING binding for some `T` (e.g. `int` then `str`); the message names
/// the variable and the two conflicting types.
///
/// Soundness notes:
/// - A `TypeVar` binds to the FIRST concrete `actual` seen, then every later
///   occurrence must AGREE. `Ty::Unknown` on the actual side is permissive (it
///   neither binds nor conflicts) so untyped values never spuriously fail.
/// - Recursion descends ONLY through matching structure (`List`/`List`,
///   `Tuple`/`Tuple` of equal arity, `Dict`/`Dict`, `Option`/`Option`,
///   `Func`/`Func` of equal arity). A structural MISMATCH where the declared
///   side contains no type variable is NOT an error here — it is left to the
///   caller's existing `types_compatible` check, which already produces the
///   canonical "argument N: expected X, found Y" diagnostic. A mismatch where the
///   declared side IS (or contains) a bare type variable simply binds the whole
///   actual to that variable (e.g. `T` against `list[int]` binds `T=list[int]`).
pub(crate) fn unify_typevar(
    declared: &Ty,
    actual: &Ty,
    type_params: &[String],
    subst: &mut HashMap<String, Ty>,
) -> std::result::Result<(), String> {
    match declared {
        Ty::TypeVar(name) => {
            // Only a name that is actually in scope as a type parameter binds.
            // (Defensive: every TypeVar reaching here is in `type_params` by
            // construction, but this keeps the function total either way.)
            if !type_params.iter().any(|tp| tp == name) {
                return Ok(());
            }
            // An `Unknown` actual carries no information — do not bind to it
            // (binding `T=Unknown` would poison later consistency checks).
            if matches!(actual, Ty::Unknown) {
                return Ok(());
            }
            match subst.get(name) {
                None => {
                    subst.insert(name.clone(), actual.clone());
                    Ok(())
                }
                Some(existing) if existing == actual => Ok(()),
                // A previously-bound variable seen with an `Unknown` later is
                // fine (keep the concrete binding); only two CONCRETE, differing
                // types conflict.
                Some(_) if matches!(actual, Ty::Unknown) => Ok(()),
                Some(existing) => Err(format!(
                    "conflicting types for type parameter `{}`: {} vs {}",
                    name, existing, actual
                )),
            }
        }
        // Descend through matching container structure so nested type vars bind
        // (`list[T]` vs `list[int]` -> T=int; `dict[K, V]` vs `dict[str, int]`
        // -> K=str, V=int; `tuple[A, B]` vs `tuple[int, str]` -> A=int, B=str).
        Ty::List(d) => match actual {
            Ty::List(a) => unify_typevar(d, a, type_params, subst),
            _ => Ok(()),
        },
        // LAZY-GEN V1-a: a DELIBERATE change to a silent-fail site (the outer
        // `_ => Ok(())` binds nothing). An `Iterator[T]` declared param binds `T`
        // from an `Iterator` actual OR a `list` actual (a list is covariantly
        // assignable to an iterator slot — `types_compatible` above). Mirrors the
        // `Ty::List` arm so `def first(xs: Iterator[T]) -> T` infers `T`.
        Ty::Iterator(d) => match actual {
            Ty::Iterator(a) | Ty::List(a) => unify_typevar(d, a, type_params, subst),
            _ => Ok(()),
        },
        Ty::Set(d) => match actual {
            Ty::Set(a) => unify_typevar(d, a, type_params, subst),
            _ => Ok(()),
        },
        Ty::Dict(dk, dv) => match actual {
            Ty::Dict(ak, av) => {
                unify_typevar(dk, ak, type_params, subst)?;
                unify_typevar(dv, av, type_params, subst)
            }
            _ => Ok(()),
        },
        // (W1.5, card 6e6b33ab) An `Optional[T]` param participates in
        // unification: an already-Optional actual unifies structurally; the
        // `None` literal carries no information; and — the previously-skipped
        // case — a BARE actual (which the call-site coerces into the Option
        // slot with `Some(..)`) unifies its type against the inner `T`, so
        // `pick(1, "s")` against `pick[T](x: T, y: Optional[T] = None)` is a
        // check-time "conflicting types for type parameter `T`" instead of a
        // leaked rustc E0308 at build.
        Ty::Option(d) => match actual {
            Ty::Option(a) => unify_typevar(d, a, type_params, subst),
            Ty::NoneVal => Ok(()),
            other => unify_typevar(d, other, type_params, subst),
        },
        Ty::Tuple(ds) => match actual {
            Ty::Tuple(as_) if ds.len() == as_.len() => {
                for (d, a) in ds.iter().zip(as_.iter()) {
                    unify_typevar(d, a, type_params, subst)?;
                }
                Ok(())
            }
            _ => Ok(()),
        },
        Ty::Func(dargs, dret) => match actual {
            Ty::Func(aargs, aret) if dargs.len() == aargs.len() => {
                for (d, a) in dargs.iter().zip(aargs.iter()) {
                    unify_typevar(d, a, type_params, subst)?;
                }
                unify_typevar(dret, aret, type_params, subst)
            }
            _ => Ok(()),
        },
        // A concrete declared type contributes no binding; compatibility of
        // concrete positions is the caller's `types_compatible` concern.
        _ => Ok(()),
    }
}

/// Apply a `{TypeVar -> Ty}` substitution to `ty`, replacing every bound type
/// variable with its concrete type and recursing through containers. An UNBOUND
/// type variable (one absent from `subst`) is left as-is — the caller decides
/// whether that is an "uninferable" error.
pub(crate) fn substitute_typevars(ty: &Ty, subst: &HashMap<String, Ty>) -> Ty {
    match ty {
        Ty::TypeVar(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Ty::List(inner) => Ty::List(Box::new(substitute_typevars(inner, subst))),
        // LAZY-GEN V1-a: substitute through an `Iterator[T]` exactly like `List[T]`.
        Ty::Iterator(inner) => Ty::Iterator(Box::new(substitute_typevars(inner, subst))),
        Ty::Set(inner) => Ty::Set(Box::new(substitute_typevars(inner, subst))),
        Ty::Dict(k, v) => Ty::Dict(
            Box::new(substitute_typevars(k, subst)),
            Box::new(substitute_typevars(v, subst)),
        ),
        Ty::Option(inner) => Ty::Option(Box::new(substitute_typevars(inner, subst))),
        Ty::Tuple(parts) => Ty::Tuple(parts.iter().map(|p| substitute_typevars(p, subst)).collect()),
        Ty::Func(args, ret) => Ty::Func(
            args.iter().map(|a| substitute_typevars(a, subst)).collect(),
            Box::new(substitute_typevars(ret, subst)),
        ),
        _ => ty.clone(),
    }
}

/// Substitute class type-parameter NAMES with concrete types, matching a name in
/// EITHER form it can take in a lowered type: a `Ty::TypeVar(name)` (scoped
/// lowering) or a bare `Ty::Class(name, [])` (UNSCOPED lowering, which renders a
/// type-param annotation as a class of that name). Used by codegen at a generic
/// constructor call to turn a `Callable[[], V]` param type (`Rc<dyn Fn() -> V>`,
/// where `V` is not in scope at the call site) into the concrete instance type
/// (`Rc<dyn Fn() -> i64>`). Recurses through every container and `Ty::Class` args.
pub fn substitute_class_typarams(ty: &Ty, subst: &HashMap<String, Ty>) -> Ty {
    match ty {
        Ty::TypeVar(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        // A bare `Ty::Class(name, [])` whose name is a substituted type param IS
        // that type param (unscoped lowering). A real class with args recurses.
        Ty::Class(name, args) if args.is_empty() && subst.contains_key(name) => {
            subst.get(name).cloned().unwrap()
        }
        Ty::Class(name, args) => Ty::Class(
            name.clone(),
            args.iter().map(|a| substitute_class_typarams(a, subst)).collect(),
        ),
        Ty::List(inner) => Ty::List(Box::new(substitute_class_typarams(inner, subst))),
        // LAZY-GEN V1-a: substitute class type-params through an `Iterator[T]`
        // member exactly like a `list[T]` one (structural twin of the
        // `substitute_typevars` arm; keeps generic-class substitution uniform).
        Ty::Iterator(inner) => Ty::Iterator(Box::new(substitute_class_typarams(inner, subst))),
        Ty::Set(inner) => Ty::Set(Box::new(substitute_class_typarams(inner, subst))),
        Ty::Dict(k, v) => Ty::Dict(
            Box::new(substitute_class_typarams(k, subst)),
            Box::new(substitute_class_typarams(v, subst)),
        ),
        Ty::Option(inner) => Ty::Option(Box::new(substitute_class_typarams(inner, subst))),
        Ty::Tuple(parts) => Ty::Tuple(parts.iter().map(|p| substitute_class_typarams(p, subst)).collect()),
        Ty::Func(args, ret) => Ty::Func(
            args.iter().map(|a| substitute_class_typarams(a, subst)).collect(),
            Box::new(substitute_class_typarams(ret, subst)),
        ),
        _ => ty.clone(),
    }
}

/// True if `ty` mentions any `Ty::TypeVar` (used to decide whether a return type
/// still has unsubstituted variables after unification). Recurses through every
/// container AND a `Ty::Class`'s type args, so a `Box[T]`-typed field is detected
/// too. Public so codegen can ask "does this field need a non-Default placeholder"
/// (a generic-class field of type-var type has no Rust `Default`).
pub fn ty_contains_typevar(ty: &Ty) -> bool {
    match ty {
        Ty::TypeVar(_) => true,
        Ty::List(inner) | Ty::Iterator(inner) | Ty::Set(inner) | Ty::Option(inner) => ty_contains_typevar(inner),
        Ty::Dict(k, v) => ty_contains_typevar(k) || ty_contains_typevar(v),
        Ty::Tuple(parts) => parts.iter().any(ty_contains_typevar),
        Ty::Func(args, ret) => args.iter().any(ty_contains_typevar) || ty_contains_typevar(ret),
        Ty::Class(_, args) => args.iter().any(ty_contains_typevar),
        _ => false,
    }
}

/// Internal alias kept for the existing call sites (generic-function return-type
/// inference) — identical behaviour to [`ty_contains_typevar`].
pub(crate) fn contains_typevar(ty: &Ty) -> bool {
    ty_contains_typevar(ty)
}

/// The result of unifying a generic call's declared param types against its
/// actual argument types: the SUBSTITUTED return type plus the accumulated
/// substitution. Shared by the checking path (which surfaces the errors) and the
/// inference oracle (which only needs the substituted return type).
pub(crate) struct GenericCallResolution {
    /// Declared return type with every inferred type variable substituted away.
    ret: Ty,
    /// Names of declared type parameters that NO argument position could bind.
    uninferable: Vec<String>,
    /// First conflicting-binding error message, if any.
    conflict: Option<String>,
}

/// Run unification for a generic function call. `params`/`ret` are the declared
/// signature types (containing `Ty::TypeVar`), `type_params` the declared
/// type-variable set, and `arg_tys` the actual argument types (positional,
/// already type-checked). Pure: surfaces conflicts and uninferable params for the
/// caller to report, and returns the substituted return type (still containing
/// any uninferable variables, which the caller treats as an error).
pub(crate) fn resolve_generic_call(
    params: &[(String, Ty)],
    ret: &Ty,
    type_params: &[String],
    arg_tys: &[Ty],
) -> GenericCallResolution {
    let mut subst: HashMap<String, Ty> = HashMap::new();
    let mut conflict: Option<String> = None;
    for ((_, decl), actual) in params.iter().zip(arg_tys.iter()) {
        if let Err(msg) = unify_typevar(decl, actual, type_params, &mut subst) {
            conflict = Some(msg);
            break;
        }
    }
    let uninferable: Vec<String> = type_params
        .iter()
        .filter(|tp| !subst.contains_key(*tp))
        .cloned()
        .collect();
    GenericCallResolution {
        ret: substitute_typevars(ret, &subst),
        uninferable,
        conflict,
    }
}

/// Compute the concrete RESULT TYPE of a call to the generic function `name`
/// given its already-resolved argument types `arg_tys`. Returns:
/// - `Ok(Some(ty))` — the substituted return type for a successful unification;
/// - `Ok(None)` — `name` is not a generic function (caller uses its plain path);
/// - `Err(..)` — a conflicting binding or an uninferable type parameter, with an
///   honest diagnostic pointing at `span`.
///
/// This is the SINGLE entry point both `check_expr` and `infer_expr_ty` use, so
/// the error-checking and codegen-inference views of a generic call agree.
pub(crate) fn infer_generic_call_result(
    name: &str,
    arg_tys: &[Ty],
    ctx: &TyCtx,
    span: Span,
) -> Result<Option<Ty>> {
    infer_generic_call_result_with_sig(name, None, arg_tys, ctx, span)
}

/// (W3-2) As [`infer_generic_call_result`], but with an OWNER-FIRST signature
/// override. A QUALIFIED call to a co-imported generic (`copy.copy(xs)` where
/// `shutil` ALSO defines a non-generic `copy`) must unify against COPY's generic
/// params — but the flat `ctx.funcs["copy"]` holds whichever collider merged last
/// (shutil's, with no type var), which would spuriously fail as "cannot infer
/// `T`". The qualified check path resolves the sig owner-first and threads it here
/// via `sig_override`; a bare call passes `None` and re-fetches from the (already
/// owner-promoted, per `check_bodies`) flat table. The type-parameter LIST is
/// still keyed by the bare `name` in `generic_funcs` (a same-named GENERIC in two
/// modules is not among the co-import pairs; that remains a v2 edge).
pub(crate) fn infer_generic_call_result_with_sig(
    name: &str,
    sig_override: Option<&FuncSig>,
    arg_tys: &[Ty],
    ctx: &TyCtx,
    span: Span,
) -> Result<Option<Ty>> {
    let type_params = match ctx.generic_funcs.get(name) {
        Some(tps) if !tps.is_empty() => tps,
        _ => return Ok(None),
    };
    let sig = match sig_override.or_else(|| ctx.funcs.get(name)) {
        Some(s) => s,
        None => return Ok(None),
    };
    // (W3-2) `generic_funcs` is bare-keyed, so a co-imported NON-generic function
    // sharing a generic function's name (`shutil.copy` vs the generic `copy.copy`)
    // would spuriously match the generic markers here. When an OWNER-FIRST sig was
    // supplied and it mentions NO type variable, the OWNER's function is genuinely
    // non-generic — use its declared return (a real generic always has a
    // type-var-bearing param or return).
    if sig_override.is_some()
        && !sig.params.iter().any(|(_, t)| contains_typevar(t))
        && !contains_typevar(&sig.ret)
    {
        return Ok(None);
    }
    let res = resolve_generic_call(&sig.params, &sig.ret, type_params, arg_tys);
    if let Some(msg) = res.conflict {
        return Err(Error::Type { span, msg });
    }
    if let Some(missing) = res.uninferable.first() {
        return Err(Error::Type {
            span,
            msg: format!(
                "cannot infer type parameter `{}` of generic function `{}` from its arguments \
                 (explicit type arguments are not supported)",
                missing, name
            ),
        });
    }
    Ok(Some(res.ret))
}

/// CODEGEN: the type-argument substitution `{T -> concrete}` inferred for a
/// generic function call to `name` from its actual argument types `arg_tys`.
/// Returns `None` when `name` is not a generic function. Used by codegen at the
/// call site to SUBSTITUTE the callee's declared param types before emitting an
/// argument into a typed slot — most importantly a `Callable[[T], T]` param,
/// whose `Rc<dyn Fn(T) -> T>` cast and lambda parameter types must be the
/// MONOMORPHIZED concrete types (`Rc<dyn Fn(i64) -> i64>`, `move |n: i64|`), not
/// the unsubstituted `T` (which would leak into the caller and fail rustc
/// E0425). Value params (`x: T`) need no such substitution — Rust infers their
/// monomorphization directly from the concrete argument — but substituting them
/// too is harmless and keeps the slot types uniform.
///
/// Pure: a conflicting/uninferable binding (already rejected by the checking
/// path) just yields a partial map; codegen never errors on it.
pub fn generic_call_param_subst(
    name: &str,
    sig_override: Option<&FuncSig>,
    arg_tys: &[Ty],
    ctx: &TyCtx,
) -> Option<HashMap<String, Ty>> {
    let type_params = match ctx.generic_funcs.get(name) {
        Some(tps) if !tps.is_empty() => tps,
        _ => return None,
    };
    // (W3-fix / F12) Resolve the callee's signature OWNER-FIRST (via `sig_override`,
    // threaded from the call site's `resolve_callee_sig`), falling back to the flat
    // table for a root fn / synthetic ctx. The flat `ctx.funcs[name]` holds only
    // the last-merged collider, so a `copy.copy[T]` call co-imported with a
    // non-generic `shutil.copy` would otherwise monomorphize against the WRONG sig.
    let sig = sig_override.or_else(|| ctx.funcs.get(name))?;
    // `generic_funcs` is bare-keyed, so a co-imported NON-generic callee sharing a
    // generic's name spuriously matches the markers above. When an OWNER-FIRST sig
    // was supplied and it mentions NO type variable, the owner's function is
    // genuinely non-generic — there is nothing to monomorphize.
    if sig_override.is_some()
        && !sig.params.iter().any(|(_, t)| contains_typevar(t))
        && !contains_typevar(&sig.ret)
    {
        return None;
    }
    let mut subst: HashMap<String, Ty> = HashMap::new();
    for ((_, decl), actual) in sig.params.iter().zip(arg_tys.iter()) {
        // Ignore a conflict here — the checking path already rejected it; we
        // only need a best-effort map to monomorphize the emitted slots.
        let _ = unify_typevar(decl, actual, type_params, &mut subst);
    }
    Some(subst)
}

/// CODEGEN: apply a type-argument substitution to a declared type, exposing the
/// internal `substitute_typevars` so codegen can monomorphize a generic call's
/// param-type slots. (Thin `pub` wrapper — same semantics.)
pub fn apply_typevar_subst(ty: &Ty, subst: &HashMap<String, Ty>) -> Ty {
    substitute_typevars(ty, subst)
}

/// PURE codegen-oracle result type for a (possibly generic) call to `name` whose
/// signature is `sig` and whose argument types are `arg_tys`. Mirrors the
/// CHECKING path's substitution but never errors: on a non-generic callee, or a
/// conflict/uninferable case (which the checking path already rejects), it falls
/// back to the declared return. Shared by the FLAT and QUALIFIED oracle arms so
/// codegen sees the same concrete result type for `swap(5,"x")` and
/// `heapq.heappop(h)` regardless of call form.
pub(crate) fn oracle_generic_call_ret(name: &str, sig: &FuncSig, arg_tys: &[Ty], ctx: &TyCtx) -> Ty {
    match ctx.generic_funcs.get(name) {
        Some(tps) if !tps.is_empty() => {
            resolve_generic_call(&sig.params, &sig.ret, tps, arg_tys).ret
        }
        _ => sig.ret.clone(),
    }
}

/// Per-argument TYPE compatibility + RESULT-type resolution for a resolved
/// function signature, shared by the FLAT (`f(args)`) and QUALIFIED (`X.f(args)`)
/// call paths so both treat a GENERIC callee identically (card: qualified generic
/// calls). `arg_tys` are the already-checked positional argument types;
/// `sig` is the callee's flat signature (whose `params`/`ret` carry `Ty::TypeVar`
/// when the callee is generic); `lookup_name` is the BARE function name used to
/// consult `ctx.generic_funcs`/`ctx.funcs` for generic unification; `diag_label`
/// is how the function is named in diagnostics (`"heappush"` for a flat call,
/// `"heapq.heappush"` for a qualified one).
///
/// Behaviour:
/// - A CONCRETE param (no type variable) is checked with `types_compatible`
///   (int→float coercion allowed, `Unknown` permissive) — an incompatible
///   argument is an honest "argument N to `f`: expected X, found Y".
/// - A param that IS or CONTAINS a type variable is SKIPPED here; structural
///   unification validates it instead.
/// - When the callee is GENERIC, `infer_generic_call_result` unifies the
///   type-var-bearing params against `arg_tys` (consistency-checked) and returns
///   the SUBSTITUTED concrete return type; a conflicting binding or an
///   uninferable type parameter is surfaced as an honest error. Otherwise the
///   declared return type is returned unchanged.
///
/// NOTE: this does NOT do arity, by-reference place checks, or shape-consuming
/// builtin checks — those are call-path-specific and stay at the call sites. It
/// covers exactly the generic-vs-concrete arg typing and the result type, which
/// is the logic that must NOT differ between the flat and qualified forms.
pub(crate) fn check_call_arg_types_and_result(
    lookup_name: &str,
    diag_label: &str,
    sig: &FuncSig,
    arg_tys: &[Ty],
    ctx: &TyCtx,
    span: Span,
) -> Result<Ty> {
    for (i, arg_ty) in arg_tys.iter().enumerate() {
        if let Some((_, param_ty)) = sig.params.get(i) {
            let int_to_float = matches!(arg_ty, Ty::Int) && matches!(param_ty, Ty::Float);
            if !int_to_float
                && !matches!(arg_ty, Ty::Unknown)
                && !matches!(param_ty, Ty::Unknown)
                && !contains_typevar(param_ty)
                && !types_compatible(arg_ty, param_ty, ctx)
            {
                return Err(Error::Type {
                    span,
                    msg: format!(
                        "argument {} to `{}`: expected {}, found {}",
                        i + 1, diag_label, param_ty, arg_ty
                    ),
                });
            }
        }
    }
    // GENERIC callee: unify + substitute the return type (and surface
    // conflicting / uninferable type parameters). Non-generic: declared return.
    // (W3-2) Pass the OWNER-FIRST `sig` (the caller resolved it via
    // `resolve_module_func`) so a co-imported same-named collider in the flat
    // table cannot supply the wrong param list to the unifier.
    Ok(infer_generic_call_result_with_sig(lookup_name, Some(sig), arg_tys, ctx, span)?
        .unwrap_or_else(|| sig.ret.clone()))
}

// ── Generics v2: generic-CLASS instantiation + member substitution ───────────
//
// A generic class `class Box[T]:` carries its type parameters as `ClassDef.
// type_params` (registered in `ctx.generic_classes`). An INSTANCE is typed
// `Ty::Class("Box", [arg, ...])`, the args positionally bound to the class's
// type params. Two operations make that work end to end:
//   - INSTANTIATION: at `Box(5)` the class args are INFERRED by unifying the
//     scoped `__init__` parameter types (which contain the class type vars)
//     against the constructor argument types — the SAME `unify_typevar` /
//     `substitute_typevars` machinery the generic functions use.
//   - MEMBER ACCESS: `b.get()` / `b.value` on a `Ty::Class("Box", [int])`
//     SUBSTITUTES `{T -> int}` into the (type-var-bearing) method-return / field
//     type, so the member access is concrete.

/// Build the `{type_param -> arg}` substitution for a generic-class INSTANCE
/// `Ty::Class(name, args)`. Returns an empty map for a non-generic class, an
/// arg-less bare class name, or when the class is not registered in
/// `ctx.generic_classes` — in every one of those cases member access falls back
/// to the unsubstituted signature, which is exactly the legacy behaviour. The
/// args are zipped positionally against the declared type-param names; a length
/// mismatch (an under/over-applied annotation) binds only the common prefix,
/// leaving any surplus type var unsubstituted (it then surfaces as a `TypeVar`
/// the caller treats as unresolved — never a panic).
pub(crate) fn class_type_subst(ty: &Ty, ctx: &TyCtx) -> HashMap<String, Ty> {
    let mut subst = HashMap::new();
    if let Ty::Class(name, args) = ty {
        if !args.is_empty() {
            if let Some(params) = ctx.generic_classes.get(name) {
                for (p, a) in params.iter().zip(args.iter()) {
                    subst.insert(p.clone(), a.clone());
                }
            }
        }
    }
    subst
}

/// Substitute a generic-class instance's type args into a member type `member`
/// (a method return / param type or a field type that may contain the class's
/// `Ty::TypeVar`s). `instance` is the receiver's `Ty::Class(name, args)`; for a
/// non-generic / arg-less receiver the substitution is empty and `member` is
/// returned unchanged (the universal non-generic path). Reuses the same
/// `substitute_typevars` used by generic functions, so the two never drift.
pub(crate) fn subst_class_member(member: &Ty, instance: &Ty, ctx: &TyCtx) -> Ty {
    let subst = class_type_subst(instance, ctx);
    if subst.is_empty() {
        member.clone()
    } else {
        substitute_typevars(member, &subst)
    }
}

/// Generics v2: INFER a generic class's type arguments at a constructor call.
/// Given the class `name`, its constructor argument types `arg_tys`, and the
/// `ctx`, returns the instance type `Ty::Class(name, [arg_for_T, ...])` with the
/// class's type params resolved by unifying the scoped `__init__` parameter
/// types against `arg_tys`.
///
/// - For a NON-generic class (absent from `ctx.generic_classes`) returns the
///   plain `Ty::Class(name, [])` — the legacy result, so every existing
///   constructor call is byte-for-byte unchanged.
/// - A type param that NO `__init__` position can bind stays unresolved; it is
///   filled with `Ty::Unknown` so the instance is still usable (permissive — the
///   pure inference oracle never errors). The checking path enforces consistency
///   separately via the same unification surfacing a conflict.
pub(crate) fn infer_class_instantiation(name: &str, arg_tys: &[Ty], ctx: &TyCtx) -> Ty {
    let type_params = match ctx.generic_classes.get(name) {
        Some(tps) if !tps.is_empty() => tps,
        _ => return Ty::Class(name.to_string(), vec![]),
    };
    // The scoped `__init__` parameter types (containing the class type vars).
    let init_key = format!("{}.__init__", name);
    let init_params: Vec<Ty> = ctx
        .funcs
        .get(&init_key)
        .map(|sig| sig.params.iter().map(|(_, t)| t.clone()).collect())
        .unwrap_or_default();
    let mut subst: HashMap<String, Ty> = HashMap::new();
    for (decl, actual) in init_params.iter().zip(arg_tys.iter()) {
        // Ignore a conflict here (the checking path reports it); this oracle is
        // permissive and only needs the best-effort binding.
        let _ = unify_typevar(decl, actual, type_params, &mut subst);
    }
    let args: Vec<Ty> = type_params
        .iter()
        .map(|tp| subst.get(tp).cloned().unwrap_or(Ty::Unknown))
        .collect();
    Ty::Class(name.to_string(), args)
}

/// Generics v2: the checking-path counterpart of [`infer_class_instantiation`].
/// Performs the same `__init__` unification but SURFACES two honest typeck errors
/// at `span`: (1) an ARITY mismatch — the constructor argument count is outside
/// `__init__`'s `[required, total]` range (required = the leading run of
/// non-defaulted params), which the `.zip()` unification would otherwise drop and
/// leak to a rustc E0061; and (2) a CONFLICTING binding for a class type
/// variable (the same `T = int then str` inconsistency `resolve_generic_call`
/// reports for functions). Returns the inferred instance type on success. A
/// non-generic class takes the early return and is unaffected.
pub(crate) fn check_class_instantiation(
    name: &str,
    arg_tys: &[Ty],
    ctx: &TyCtx,
    span: Span,
) -> Result<Ty> {
    let type_params = match ctx.generic_classes.get(name) {
        Some(tps) if !tps.is_empty() => tps,
        _ => return Ok(Ty::Class(name.to_string(), vec![])),
    };
    let init_key = format!("{}.__init__", name);
    if let Some(sig) = ctx.funcs.get(&init_key) {
        let init_params: Vec<Ty> = sig.params.iter().map(|(_, t)| t.clone()).collect();
        // ARITY: the `.zip()` below stops at the shorter of params/args, so a
        // wrong COUNT would otherwise be silently accepted and leak to a rustc
        // E0061 at build. `__init__`'s `params`/`param_defaults` are self-EXCLUSIVE
        // and index-aligned (resolver STEP 0). A trailing run of defaulted params
        // is optional, so the accepted count is `[required, expected]` — the same
        // rule the free-function call-arity check uses.
        let expected = init_params.len();
        let required = sig.param_defaults.iter().take_while(|d| d.is_none()).count();
        let got = arg_tys.len();
        if got < required || got > expected {
            let arg_desc = if required == expected {
                format!("{}", expected)
            } else {
                format!("{} to {}", required, expected)
            };
            return Err(Error::Type {
                span,
                msg: format!(
                    "`{}.__init__` takes {} argument(s) but {} {} given",
                    name, arg_desc, got, if got == 1 { "was" } else { "were" }
                ),
            });
        }
        let mut subst: HashMap<String, Ty> = HashMap::new();
        for (decl, actual) in init_params.iter().zip(arg_tys.iter()) {
            if let Err(msg) = unify_typevar(decl, actual, type_params, &mut subst) {
                return Err(Error::Type { span, msg });
            }
        }
        let args: Vec<Ty> = type_params
            .iter()
            .map(|tp| subst.get(tp).cloned().unwrap_or(Ty::Unknown))
            .collect();
        Ok(Ty::Class(name.to_string(), args))
    } else {
        Ok(Ty::Class(name.to_string(), vec![]))
    }
}

/// Generics v1 — the OPS-ON-`T` restriction. A bound type variable is PARAMETRIC
/// (opaque): inside a generic function a value of type `Ty::TypeVar` may be
/// moved, cloned, assigned, returned, passed to another generic, and stored
/// in / read from a container — but it may NOT be OPERATED ON, because any
/// operation (arithmetic, comparison, indexing, calling a method, `print`, ...)
/// would require a trait bound that v1 does not support. This turns such an
/// operation into an HONEST typeck error instead of a confusing rustc error
/// (e.g. "cannot add `T` to `T`") on the generated crate.
///
/// `ty` is the operand's type, `op_desc` names the operation for the diagnostic
/// (e.g. "apply `+` to", "compare", "index", "call a method on", "print").
/// A non-`TypeVar` `ty` is always Ok (the operation proceeds normally).
pub(crate) fn reject_typevar_op(ty: &Ty, op_desc: &str, span: Span) -> Result<()> {
    if let Ty::TypeVar(name) = ty {
        return Err(Error::Type {
            span,
            msg: format!(
                "cannot {} a value of generic type `{}` \
                 (this operation on a type parameter is not supported — \
                 generics v2 infers bounds only for comparison, equality, \
                 arithmetic, Display, and Hash)",
                op_desc, name
            ),
        });
    }
    Ok(())
}

/// (card 2b37b965, Z4) Reject a bare `Optional[T]` used for its TRUTHINESS —
/// `if m:` / `while m:` / `not m` / `x if m else y` / `[.. for .. if m]` /
/// `assert m`. CPython tests such a value's truthiness (an `Optional` is falsy
/// only when `None`), but pyrst does not yet lower Optional truthiness, so
/// codegen would emit Rust `if <Option>` and leak a raw `rustc` E0308 at BUILD —
/// a check-passes / build-fails DISAGREEMENT. Rejecting it here (at `check`)
/// keeps check and build in agreement and points at the sound idiom. Real
/// Optional truthiness (`if m:` == `m is not None` for an always-truthy payload,
/// plus the general falsy-payload semantics) is a tracked follow-on (card
/// 6a554b41); do NOT implement it here. `and`/`or` operands are already rejected
/// by the EPIC-5 BinOp-Optional gate, so this covers the remaining sites.
///
/// A narrowing guard (`m is not None`) is a `BinOp` typed `Bool`, never an
/// `Option`, so it is unaffected — only a RAW Optional value trips this.
pub(crate) fn reject_optional_truthiness(ty: &Ty, span: Span) -> Result<()> {
    if let Ty::Option(_) = ty {
        return Err(Error::Type {
            span,
            msg: "bare truthiness on an Optional value is not supported: narrow it \
                  first with `if x is not None:` (or `x is None`) to obtain the inner \
                  value. CPython's `if x:` truthiness over an Optional is a tracked \
                  follow-on (card 6a554b41); pyrst rejects it here so `check` and \
                  `build` agree instead of leaking a rustc type error."
                .to_string(),
        });
    }
    Ok(())
}

/// Generics v2: a Rust trait bound INFERRED from an operation performed on a
/// bare type variable inside a generic function body. The SUPPORTED subset of
/// ops on a `T` no longer rejects (v1) but instead records the trait the
/// generated Rust needs, which codegen emits in the generic clause
/// (`fn f<T: Clone + PartialOrd>(..)`). The set is the union of every bound
/// inferred for that `T` across the whole body, plus an always-present `Clone`
/// (pyrst value semantics clone-on-use). `Ord`-style variants emit `<Output =
/// T>` where the arithmetic trait requires it.
///
/// The variant ORDER is the canonical emission order (derive of `Ord` is
/// intentional) so the generated clause is deterministic and `Clone` leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TypeVarBound {
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Add,
    Sub,
    Mul,
    Display,
    /// `repr(x)` on a bare `T` lowers to `x.py_repr()` (the CPython-parity
    /// `PyRepr` trait), NOT `format!("{}", x)`. This is what makes a generic
    /// function quote a `str` element the way CPython's `%r` does (e.g.
    /// `deque.remove`'s "'x' is not in deque" message) instead of the unquoted
    /// Display form. Distinct from `Display` (str/print/f-string keep Display).
    Repr,
}

impl TypeVarBound {
    /// The Rust trait-bound text for this inferred bound, given the type-var
    /// name `t` (needed for the `<Output = T>` on arithmetic traits so the
    /// result of `T + T` is `T`, matching pyrst's same-type arithmetic rule).
    pub fn rust_bound(self, t: &str) -> String {
        match self {
            TypeVarBound::Clone => "Clone".to_string(),
            TypeVarBound::PartialEq => "PartialEq".to_string(),
            TypeVarBound::Eq => "std::cmp::Eq".to_string(),
            TypeVarBound::Hash => "std::hash::Hash".to_string(),
            TypeVarBound::PartialOrd => "PartialOrd".to_string(),
            TypeVarBound::Add => format!("std::ops::Add<Output = {}>", t),
            TypeVarBound::Sub => format!("std::ops::Sub<Output = {}>", t),
            TypeVarBound::Mul => format!("std::ops::Mul<Output = {}>", t),
            TypeVarBound::Display => "std::fmt::Display".to_string(),
            TypeVarBound::Repr => "PyRepr".to_string(),
        }
    }
}

/// Generics v2: the SINGLE SOURCE OF TRUTH mapping a binary operator on two
/// values of the SAME type variable (`T op T`) to the Rust trait bound it
/// requires — or `None` when the op on a bare `T` is NOT supported in v2 and
/// must stay an honest `reject_typevar_op` rejection.
///
/// Supported (op -> bound, result type computed by the BinOp arm):
///   - `< > <= >=`     -> `PartialOrd`  (result `bool`)
///   - `== !=`         -> `PartialEq`   (result `bool`)
///   - `+ - * / %`     -> `Add`/`Sub`/`Mul`/`Div`/`Rem` (`<Output = T>`, result `T`)
/// STILL REJECTED on a bare `T` (return `None`): `in`/`not in` (membership),
/// `is`/`is not`, boolean `and`/`or`, bitwise/shift, `**` (Pow), `//` (FloorDiv)
/// — no clean single-trait mapping with a known result type for an opaque `T`.
///
/// BOTH typeck (to decide allow-vs-reject and type the result) and codegen (to
/// build the clause) consult this function, so the "what's supported" decision
/// can never drift between the two layers.
pub fn binop_typevar_bound(op: BinOp) -> Option<TypeVarBound> {
    match op {
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Some(TypeVarBound::PartialOrd),
        BinOp::Eq | BinOp::Ne => Some(TypeVarBound::PartialEq),
        // Arithmetic `+ - *` map FAITHFULLY: Rust's `Add`/`Sub`/`Mul` on the
        // numeric types pyrst supports compute the same result as Python, and
        // `<Output = T>` makes `T op T -> T` exactly the same-type rule. The
        // result is `T`.
        BinOp::Add => Some(TypeVarBound::Add),
        BinOp::Sub => Some(TypeVarBound::Sub),
        BinOp::Mul => Some(TypeVarBound::Mul),
        // INTENTIONALLY NOT MAPPED — these stay rejected on a bare `T` because no
        // single Rust trait reproduces pyrst's Python semantics for an opaque `T`:
        //   - `/`  : pyrst `/` is TRUE division (always Float, e.g. 5/2 == 2.5);
        //            Rust `Div` on an integer `T` truncates (5/2 == 2). A
        //            `Div<Output = T>` bound would silently miscompile integer
        //            division, so `/` on a bare `T` is NOT supported in v2.
        //   - `%`  : pyrst `%` is DIVISOR-signed (Python), Rust `Rem` is
        //            dividend-signed — they disagree for negative operands, so
        //            `Rem` is not a faithful lowering of a bare `T % T`.
        //   - `//` / `**` : lowered via int-specific helpers (`__py_floordiv` /
        //            `__py_ipow`) with no clean single-trait generic form.
        // Mixed `T op concrete` (e.g. `x + 1`) also stays rejected — only the
        // same-`T` shape is admitted.
        BinOp::Div | BinOp::Mod | BinOp::FloorDiv | BinOp::Pow => None,
        // Everything else on a bare `T` stays rejected in v2.
        _ => None,
    }
}

/// A per-type-variable bound map: `TypeVar name -> {bounds}`.
pub(crate) type BoundMap = std::collections::BTreeMap<String, std::collections::BTreeSet<TypeVarBound>>;

/// A transitive-propagation EDGE captured at a generic CALL inside a generic
/// function: `(caller_tv, callee_name, callee_tv)` means "this function's type
/// variable `caller_tv` flows into generic function `callee_name`'s type
/// parameter `callee_tv`", so whatever bounds `callee_name` requires on
/// `callee_tv` must ALSO be required on `caller_tv`. Folded by the fixed point in
/// `infer_func_typevar_bounds`.
pub(crate) type PropEdge = (String, String, String);

/// Generics v2: infer the per-TYPE-VARIABLE Rust trait-bound set for one generic
/// function, INCLUDING bounds propagated transitively from generic functions it
/// calls. The returned map is `TypeVar -> {bounds}`; every declared type
/// parameter is present with at least `Clone` (pyrst value semantics). Codegen
/// reads this map to emit the generic clause `fn f<T: Clone + PartialOrd, ..>`.
///
/// Two layers:
///  1. DIRECT bounds — `direct_func_typevar_bounds` walks the body/signature and
///     records the bound each SUPPORTED op on a bare `T` requires (comparison ->
///     PartialOrd, `+ - *` -> Add/Sub/Mul, Display contexts -> Display, set/dict
///     element/key -> Hash + Eq), mirroring exactly the typeck op-sites.
///  2. TRANSITIVE propagation — when `f` passes one of its type vars `T` into a
///     generic callee `g`'s parameter `U` (e.g. `dedup(a, b)` where `a, b: T`
///     bind `g`'s `U`), `g`'s required bounds on `U` FOLD INTO `T`. This is the
///     fixed point over the whole generic call graph: repeatedly union callee
///     bounds into callers along the captured edges until nothing changes.
///
/// CYCLES (a generic calling itself, or mutual generic recursion) are handled by
/// the fixed point itself — a self-edge `T -> (f, T)` unions `f`'s own current
/// `T` bounds into `T` (a no-op once stable), and the loop terminates because the
/// bound lattice is finite and monotonically growing (each pass only ADDS bounds;
/// it stops the first pass that adds none). Closing this gap turns the former
/// silent check-passes/build-fails transitive call into a correct clause.
///
/// `ctx.generic_func_bodies` supplies every generic callee's body, so a callee's
/// direct bounds can be recomputed here. A non-generic `f` (empty `type_params`)
/// returns an empty map and costs one early return — the hot path is unaffected.
pub fn infer_func_typevar_bounds(f: &Func, ctx: &TyCtx) -> BoundMap {
    if f.type_params.is_empty() {
        return BoundMap::new();
    }
    // Build the working set: `f` plus every generic function reachable via
    // `ctx.generic_func_bodies` (the call graph is small; we just take them all,
    // since propagation only flows along edges that actually exist). `f` itself
    // may or may not be registered in `ctx` (tests build a func without a ctx
    // entry), so insert it explicitly under its own name.
    let mut direct: std::collections::HashMap<String, BoundMap> = std::collections::HashMap::new();
    let mut edges: std::collections::HashMap<String, Vec<PropEdge>> = std::collections::HashMap::new();
    direct.insert(f.name.clone(), direct_func_typevar_bounds(f, ctx));
    edges.insert(f.name.clone(), collect_prop_edges(f, ctx));
    for (name, body) in &ctx.generic_func_bodies {
        direct.entry(name.clone()).or_insert_with(|| direct_func_typevar_bounds(body, ctx));
        edges.entry(name.clone()).or_insert_with(|| collect_prop_edges(body, ctx));
    }

    // Fixed point: start from the direct bounds, then fold callee bounds into
    // callers along every edge until a full pass adds nothing. Monotone + finite
    // lattice => terminates; cycles are absorbed (a repeated union is idempotent).
    let mut result = direct.clone();
    loop {
        let mut changed = false;
        // Iterate caller functions in a stable order for determinism.
        let callers: Vec<String> = {
            let mut v: Vec<String> = edges.keys().cloned().collect();
            v.sort();
            v
        };
        for caller in &callers {
            let caller_edges = edges.get(caller).cloned().unwrap_or_default();
            for (caller_tv, callee, callee_tv) in &caller_edges {
                // The bounds the callee currently requires on the bound position.
                let inherited: Vec<TypeVarBound> = result
                    .get(callee)
                    .and_then(|m| m.get(callee_tv))
                    .map(|s| s.iter().copied().collect())
                    .unwrap_or_default();
                if inherited.is_empty() {
                    continue;
                }
                let entry = result
                    .entry(caller.clone())
                    .or_default()
                    .entry(caller_tv.clone())
                    .or_default();
                for b in inherited {
                    if entry.insert(b) {
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Defensive: ensure every declared type param of `f` is present with `Clone`
    // even if it was never used by any op (so codegen always emits a clause).
    let mut out = result.remove(&f.name).unwrap_or_default();
    for tp in &f.type_params {
        out.entry(tp.clone()).or_default().insert(TypeVarBound::Clone);
    }
    out
}

/// (enabler-fix-2 #1b) Honest CHECK error when a user CLASS is bound to a type
/// variable that generic function `name` uses in a HASHABLE position — a `dict`
/// key / `set` element, surfaced as an inferred `Hash` bound — UNLESS that class
/// already derives `Eq`/`Hash` (it is in `ctx.hash_key_classes`).
///
/// pyrst does NOT monomorphize: it emits ONE generic Rust fn with a `T: Hash + Eq`
/// bound, so a class that never keys a CONCRETE `dict`/`set` never gains those
/// derives and the call leaked rustc E0277/E0599 ("Node: Hash not satisfied"). The
/// derive cannot be threaded through a type parameter after the fact, so this is a
/// documented limitation (PYTHON_COMPATIBILITY.md): key the class CONCRETELY
/// (`dict[C, _]` / `set[C]`) somewhere to opt it in. A class that DOES so passes
/// through unaffected — this only rejects the otherwise-silent-build-fail case, so
/// it never regresses a program that already compiled.
pub fn reject_class_key_through_generic(
    name: &str,
    arg_tys: &[Ty],
    ctx: &TyCtx,
    span: Span,
) -> Result<()> {
    let body = match ctx.generic_func_bodies.get(name) {
        Some(b) => b,
        None => return Ok(()),
    };
    let bounds = infer_func_typevar_bounds(body, ctx);
    // Cheap exit: no type var needs Hash, so no class-key hazard exists.
    if !bounds.values().any(|s| s.contains(&TypeVarBound::Hash)) {
        return Ok(());
    }
    // (W3-fix / F12) Bare-name hazard check run under the OWNER-FIRST checking ctx
    // (`with_module_symbols_promoted`), so the flat table is already owner-correct
    // here — no explicit sig override needed.
    let subst = match generic_call_param_subst(name, None, arg_tys, ctx) {
        Some(s) => s,
        None => return Ok(()),
    };
    for (tv, tv_bounds) in &bounds {
        if !tv_bounds.contains(&TypeVarBound::Hash) {
            continue;
        }
        if let Some(Ty::Class(cn, _)) = subst.get(tv) {
            if !ctx.hash_key_classes.contains(cn) {
                return Err(Error::Type {
                    span,
                    msg: format!(
                        "user class `{}` reaches a dict-key / set-element position inside \
                         generic `{}` (via type parameter `{}`), but pyrst cannot thread the \
                         required Eq/Hash derive through a type parameter (it emits one generic \
                         function, not a monomorphized copy). Key `{}` CONCRETELY somewhere \
                         (`dict[{}, _]` or `set[{}]`) to opt it into the derive, or use a \
                         concrete container instead of the generic",
                        cn, name, tv, cn, cn, cn
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Generics v2: infer the per-TYPE-VARIABLE Rust trait-bound set for one generic
/// CLASS, by walking the bodies and signatures of ALL its methods. Reuses the
/// SAME `infer_bounds_body` / `record_hashable_typevars` machinery as the
/// generic-function path, so the "what op needs what bound" decision can never
/// drift between functions, classes, and the typeck op-sites. The returned map
/// is `class type var -> {bounds}`; every declared class type param is present
/// with at least `Clone` (pyrst value semantics), so codegen always emits a
/// well-formed `impl<T: Clone + ..>` clause.
///
/// Each method is seeded exactly as it is type-checked: `self` is typed
/// `Ty::Class(name, [TypeVar(T), ..])` and each non-self param is scope-lowered
/// with the class type params (a `v: T` param is `Ty::TypeVar("T")`), so an op
/// on a field/param/return of type `T` records its bound. Field annotations are
/// scanned too: a `set[T]` / `dict[T, _]` field needs `Hash + Eq` on `T`. Method
/// transitive propagation into generic FREE functions is intentionally not
/// modelled here (a method calling a generic free function with a class `T` is a
/// rare stretch case — see the deferred notes); the direct ops cover Box/Pair
/// and the comparison/arith/Display/Hash subset the spec requires.
///
/// A NON-generic class (empty `type_params`) returns an empty map and costs one
/// early return — the non-generic emission path is untouched.
pub fn infer_class_typevar_bounds(c: &ClassDef, ctx: &TyCtx) -> BoundMap {
    let mut bounds = BoundMap::new();
    if c.type_params.is_empty() {
        return bounds;
    }
    // Every class type parameter carries at least `Clone` (clone-on-use).
    for tp in &c.type_params {
        bounds.entry(tp.clone()).or_default().insert(TypeVarBound::Clone);
    }
    // A `set[T]` / `dict[T, _]` FIELD makes the struct hold a `HashSet<T>` /
    // `HashMap<T, _>`, which requires `Hash + Eq` on `T`.
    for field in &c.fields {
        if let Ok(ty) = Ty::from_type_expr_scoped(&field.ty, field.span, &c.type_params) {
            record_hashable_typevars(&ty, &mut bounds);
        }
    }
    // Walk every method body with the same locals seeding typeck uses.
    let self_args: Vec<Ty> = c.type_params.iter().map(|tp| Ty::TypeVar(tp.clone())).collect();
    for m in &c.methods {
        let mut locals: HashMap<String, Ty> = HashMap::new();
        locals.insert("self".to_string(), Ty::Class(c.name.clone(), self_args.clone()));
        for p in m.params.iter().filter(|p| p.name != "self") {
            if let Ok(ty) = Ty::from_type_expr_scoped(&p.ty, p.span, &c.type_params) {
                record_hashable_typevars(&ty, &mut bounds);
                locals.insert(p.name.clone(), ty);
            }
        }
        if let Ok(ret) = Ty::from_type_expr_scoped(&m.ret, m.span, &c.type_params) {
            record_hashable_typevars(&ret, &mut bounds);
        }
        infer_bounds_body(&m.body, &mut locals, ctx, &mut bounds);
    }
    bounds
}

/// The DIRECT (non-propagated) bound map for one generic function: a self-
/// contained walk of its body and signature that records the bound each
/// SUPPORTED op on a bare `T` requires. It seeds `locals: name -> Ty` from the
/// params (scoped-lowered, so a `T` param is `Ty::TypeVar("T")`) and uses the
/// shared `infer_expr_ty` — the same inference codegen's `type_of_expr` uses — so
/// typeck, codegen, and this pass agree on which operands are type variables.
/// Transitive bounds from generic calls are added separately by the fixed point.
pub(crate) fn direct_func_typevar_bounds(f: &Func, ctx: &TyCtx) -> BoundMap {
    let mut bounds = BoundMap::new();
    if f.type_params.is_empty() {
        return bounds;
    }
    // Every declared type parameter carries at least `Clone` (clone-on-use).
    for tp in &f.type_params {
        bounds.entry(tp.clone()).or_default().insert(TypeVarBound::Clone);
    }
    // Seed locals from the (scoped-lowered) param types. A param annotation that
    // FAILS to lower (it cannot for a checked program — typeck already lowered
    // it) is skipped defensively rather than panicking.
    let mut locals: HashMap<String, Ty> = HashMap::new();
    for p in f.params.iter().filter(|p| p.name != "self") {
        if let Ok(ty) = Ty::from_type_expr_scoped(&p.ty, p.span, &f.type_params) {
            // A `set[T]` / `dict[T, _]` param annotation needs `Hash + Eq` on `T`
            // (the container is `HashSet<T>` / `HashMap<T, _>`).
            record_hashable_typevars(&ty, &mut bounds);
            locals.insert(p.name.clone(), ty);
        }
    }
    // A `set[T]` / `dict[T, _]` RETURN annotation needs `Hash + Eq` on `T` too
    // (e.g. the dedup-into-`set[T]` case).
    if let Ok(ret) = Ty::from_type_expr_scoped(&f.ret, f.span, &f.type_params) {
        record_hashable_typevars(&ret, &mut bounds);
    }
    infer_bounds_body(&f.body, &mut locals, ctx, &mut bounds);
    bounds
}

/// Collect the transitive-propagation EDGES for one generic function `f`: for
/// every CALL to a generic callee `g` inside `f`'s body where an argument's type
/// is one of `f`'s own type variables `T` and that argument position binds `g`'s
/// type parameter `U`, emit `(T, g, U)`. The fixed point in
/// `infer_func_typevar_bounds` then folds `g`'s bounds on `U` into `T`.
///
/// Argument→callee-param mapping reuses the SAME shape as the call-site
/// unification (`unify_typevar`): a scalar `T` flows into a scalar `U`, and a
/// container `list[T]` / `set[T]` / `dict[T, _]` / `tuple[T, ..]` flows its
/// element/key var into the matching position of the callee's declared param.
/// Mixed/positional-only and `Unknown` args contribute no edge (no type var to
/// propagate).
pub(crate) fn collect_prop_edges(f: &Func, ctx: &TyCtx) -> Vec<PropEdge> {
    if f.type_params.is_empty() {
        return Vec::new();
    }
    let mut locals: HashMap<String, Ty> = HashMap::new();
    for p in f.params.iter().filter(|p| p.name != "self") {
        if let Ok(ty) = Ty::from_type_expr_scoped(&p.ty, p.span, &f.type_params) {
            locals.insert(p.name.clone(), ty);
        }
    }
    let mut edges: Vec<PropEdge> = Vec::new();
    collect_prop_edges_body(&f.body, &mut locals, ctx, &mut edges);
    // De-duplicate (a callee called twice yields the same edge).
    edges.sort();
    edges.dedup();
    edges
}

pub(crate) fn collect_prop_edges_body(
    body: &[Stmt],
    locals: &mut HashMap<String, Ty>,
    ctx: &TyCtx,
    edges: &mut Vec<PropEdge>,
) {
    for s in body {
        collect_prop_edges_stmt(s, locals, ctx, edges);
    }
}

pub(crate) fn collect_prop_edges_stmt(
    s: &Stmt,
    locals: &mut HashMap<String, Ty>,
    ctx: &TyCtx,
    edges: &mut Vec<PropEdge>,
) {
    // Reuse the bounds walk's local-tracking shape so `infer_expr_ty` stays
    // accurate; we only care about Call expressions, found by recursing on every
    // sub-expression below.
    match s {
        Stmt::Expr(e) | Stmt::Return(Some(e), _) | Stmt::Yield(e, _) => {
            collect_prop_edges_expr(e, locals, ctx, edges);
        }
        Stmt::Assign { target, value, .. } => {
            collect_prop_edges_expr(value, locals, ctx, edges);
            let t = infer_expr_ty(value, locals, ctx);
            locals.insert(target.clone(), t);
        }
        Stmt::AugAssign { value, .. } => collect_prop_edges_expr(value, locals, ctx, edges),
        Stmt::Unpack { targets, value, .. } => {
            collect_prop_edges_expr(value, locals, ctx, edges);
            let vt = infer_expr_ty(value, locals, ctx);
            if let Ty::Tuple(elems) = &vt {
                for (i, t) in targets.iter().enumerate() {
                    locals.insert(t.clone(), elems.get(i).cloned().unwrap_or(Ty::Unknown));
                }
            } else {
                for t in targets {
                    locals.insert(t.clone(), Ty::Unknown);
                }
            }
        }
        Stmt::Return(None, _) | Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_)
        | Stmt::Import { .. } => {}
        Stmt::If { cond, then, elifs, else_, .. } => {
            collect_prop_edges_expr(cond, locals, ctx, edges);
            collect_prop_edges_body(then, locals, ctx, edges);
            for (c, b) in elifs {
                collect_prop_edges_expr(c, locals, ctx, edges);
                collect_prop_edges_body(b, locals, ctx, edges);
            }
            if let Some(b) = else_ {
                collect_prop_edges_body(b, locals, ctx, edges);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_prop_edges_expr(cond, locals, ctx, edges);
            collect_prop_edges_body(body, locals, ctx, edges);
        }
        Stmt::For { targets, iter, body, .. } => {
            collect_prop_edges_expr(iter, locals, ctx, edges);
            let elem = match infer_expr_ty(iter, locals, ctx) {
                // LAZY-GEN V1-a: a generator source yields elements like a list.
                Ty::List(inner) | Ty::Iterator(inner) | Ty::Set(inner) => *inner,
                Ty::Str => Ty::Str,
                _ => Ty::Unknown,
            };
            if targets.len() == 1 {
                locals.insert(targets[0].clone(), elem);
            } else if let Ty::Tuple(elems) = &elem {
                for (i, t) in targets.iter().enumerate() {
                    locals.insert(t.clone(), elems.get(i).cloned().unwrap_or(Ty::Unknown));
                }
            } else {
                for t in targets {
                    locals.insert(t.clone(), Ty::Unknown);
                }
            }
            collect_prop_edges_body(body, locals, ctx, edges);
        }
        Stmt::Assert { cond, msg, .. } => {
            collect_prop_edges_expr(cond, locals, ctx, edges);
            if let Some(m) = msg {
                collect_prop_edges_expr(m, locals, ctx, edges);
            }
        }
        Stmt::Raise { exc, .. } => {
            if let Some(e) = exc {
                collect_prop_edges_expr(e, locals, ctx, edges);
            }
        }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            collect_prop_edges_body(body, locals, ctx, edges);
            for h in handlers {
                collect_prop_edges_body(&h.body, locals, ctx, edges);
            }
            if let Some(b) = else_ {
                collect_prop_edges_body(b, locals, ctx, edges);
            }
            if let Some(b) = finally_ {
                collect_prop_edges_body(b, locals, ctx, edges);
            }
        }
        Stmt::With { ctx_expr, body, .. } => {
            collect_prop_edges_expr(ctx_expr, locals, ctx, edges);
            collect_prop_edges_body(body, locals, ctx, edges);
        }
        Stmt::Del { target, .. } => collect_prop_edges_expr(target, locals, ctx, edges),
        Stmt::Match { subject, arms, .. } => {
            collect_prop_edges_expr(subject, locals, ctx, edges);
            for a in arms {
                if let Some(g) = &a.guard {
                    collect_prop_edges_expr(g, locals, ctx, edges);
                }
                collect_prop_edges_body(&a.body, locals, ctx, edges);
            }
        }
        Stmt::AttrAssign { obj, value, .. } => {
            collect_prop_edges_expr(obj, locals, ctx, edges);
            collect_prop_edges_expr(value, locals, ctx, edges);
        }
        Stmt::IndexAssign { obj, idx, value, .. } => {
            collect_prop_edges_expr(obj, locals, ctx, edges);
            collect_prop_edges_expr(idx, locals, ctx, edges);
            collect_prop_edges_expr(value, locals, ctx, edges);
        }
        Stmt::Func(_) | Stmt::Class(_) | Stmt::Global { .. } | Stmt::Nonlocal { .. } => {}
    }
}

pub(crate) fn collect_prop_edges_expr(
    e: &Expr,
    locals: &HashMap<String, Ty>,
    ctx: &TyCtx,
    edges: &mut Vec<PropEdge>,
) {
    if let Expr::Call { callee, args, .. } = e {
        if let Expr::Ident(callee_name, _) = callee.as_ref() {
            // Only a GENERIC callee can carry bounds to propagate.
            if let Some(callee_tps) = ctx.generic_funcs.get(callee_name) {
                if let Some(sig) = ctx.funcs.get(callee_name) {
                    for (i, arg) in args.iter().enumerate() {
                        let arg_ty = infer_expr_ty(arg, locals, ctx);
                        if let Some((_, decl)) = sig.params.get(i) {
                            // Map the caller's type var(s) inside `arg_ty` to the
                            // callee's type param(s) at the matching structural
                            // position of the declared param `decl`.
                            map_typevar_edges(&arg_ty, decl, callee_name, callee_tps, edges);
                        }
                    }
                }
            }
        }
    }
    // Recurse into every sub-expression so a call nested anywhere is found.
    for sub in expr_children(e) {
        collect_prop_edges_expr(sub, locals, ctx, edges);
    }
}

/// Structurally align a caller argument type `arg` (which may be / contain
/// `Ty::TypeVar(caller_tv)`) against the callee's declared param type `decl`
/// (which may be / contain `Ty::TypeVar(callee_tv)`), emitting an edge
/// `(caller_tv, callee, callee_tv)` for each position where a caller type var
/// lines up with a callee type param. Mirrors `unify_typevar`'s shape so the
/// propagation graph matches the actual call-site binding.
pub(crate) fn map_typevar_edges(
    arg: &Ty,
    decl: &Ty,
    callee: &str,
    callee_tps: &[String],
    edges: &mut Vec<PropEdge>,
) {
    match (arg, decl) {
        (Ty::TypeVar(caller_tv), Ty::TypeVar(callee_tv)) if callee_tps.iter().any(|t| t == callee_tv) => {
            edges.push((caller_tv.clone(), callee.to_string(), callee_tv.clone()));
        }
        // LAZY-GEN V1-a: propagate type-var edges through an `Iterator[T]` param
        // exactly like a `list[T]` one.
        (Ty::List(a), Ty::List(d)) | (Ty::Iterator(a), Ty::Iterator(d)) | (Ty::Set(a), Ty::Set(d)) | (Ty::Option(a), Ty::Option(d)) => {
            map_typevar_edges(a, d, callee, callee_tps, edges);
        }
        (Ty::Dict(ak, av), Ty::Dict(dk, dv)) => {
            map_typevar_edges(ak, dk, callee, callee_tps, edges);
            map_typevar_edges(av, dv, callee, callee_tps, edges);
        }
        (Ty::Tuple(aa), Ty::Tuple(dd)) if aa.len() == dd.len() => {
            for (a, d) in aa.iter().zip(dd.iter()) {
                map_typevar_edges(a, d, callee, callee_tps, edges);
            }
        }
        _ => {}
    }
}

/// The immediate child expressions of `e` (for the generic-call edge walk). Kept
/// local and total so `collect_prop_edges_expr` recurses without missing a nest.
pub(crate) fn expr_children(e: &Expr) -> Vec<&Expr> {
    match e {
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bytes(..) | Expr::Bool(..)
        | Expr::None_(_) | Expr::Ident(..) => vec![],
        Expr::FStr(parts, _) => parts.iter().filter_map(|p| match p {
            FStrPart::Interp(e, _) => Some(e),
            FStrPart::Lit(_) => None,
        }).collect(),
        Expr::List(es, _) | Expr::Tuple(es, _) | Expr::Set(es, _) => es.iter().collect(),
        Expr::Dict(pairs, _) => pairs.iter().flat_map(|(k, v)| [k, v]).collect(),
        Expr::ListComp { elt, iter, cond, .. } | Expr::SetComp { elt, iter, cond, .. } => {
            let mut v: Vec<&Expr> = vec![elt.as_ref(), iter.as_ref()];
            if let Some(c) = cond { v.push(c.as_ref()); }
            v
        }
        Expr::DictComp { key, val, iter, cond, .. } => {
            let mut v: Vec<&Expr> = vec![key.as_ref(), val.as_ref(), iter.as_ref()];
            if let Some(c) = cond { v.push(c.as_ref()); }
            v
        }
        Expr::Call { callee, args, kwargs, .. } => {
            let mut v: Vec<&Expr> = vec![callee.as_ref()];
            v.extend(args.iter());
            v.extend(kwargs.iter().map(|(_, e)| e));
            v
        }
        Expr::Attr { obj, .. } => vec![obj.as_ref()],
        Expr::Index { obj, idx, .. } => vec![obj.as_ref(), idx.as_ref()],
        Expr::Slice { obj, start, stop, step, .. } => {
            let mut v = vec![obj.as_ref()];
            for o in [start, stop, step].into_iter().flatten() { v.push(o.as_ref()); }
            v
        }
        Expr::BinOp { lhs, rhs, .. } => vec![lhs.as_ref(), rhs.as_ref()],
        Expr::UnOp { expr, .. } => vec![expr.as_ref()],
        Expr::Lambda { body, .. } => vec![body.as_ref()],
        Expr::IfExp { test, body, orelse, .. } => vec![test.as_ref(), body.as_ref(), orelse.as_ref()],
    }
}

/// Record `Hash + Eq` for every type variable that appears as a SET ELEMENT or
/// DICT KEY anywhere inside `ty` (the only hashable positions). A `dict` VALUE is
/// not hashable, so only its key is scanned; nested containers recurse.
pub(crate) fn record_hashable_typevars(
    ty: &Ty,
    bounds: &mut std::collections::BTreeMap<String, std::collections::BTreeSet<TypeVarBound>>,
) {
    match ty {
        Ty::Set(elem) => {
            if let Ty::TypeVar(n) = elem.as_ref() {
                add_bound(bounds, n, TypeVarBound::Hash);
                add_bound(bounds, n, TypeVarBound::Eq);
            }
            record_hashable_typevars(elem, bounds);
        }
        Ty::Dict(k, v) => {
            if let Ty::TypeVar(n) = k.as_ref() {
                add_bound(bounds, n, TypeVarBound::Hash);
                add_bound(bounds, n, TypeVarBound::Eq);
            }
            record_hashable_typevars(k, bounds);
            record_hashable_typevars(v, bounds);
        }
        Ty::List(inner) | Ty::Iterator(inner) | Ty::Option(inner) => record_hashable_typevars(inner, bounds),
        Ty::Tuple(elems) => elems.iter().for_each(|e| record_hashable_typevars(e, bounds)),
        _ => {}
    }
}

/// Add `bound` to `name`'s set (always also keeping `Clone`, which the seed
/// already inserted). Helper to keep the call sites terse.
pub(crate) fn add_bound(
    bounds: &mut std::collections::BTreeMap<String, std::collections::BTreeSet<TypeVarBound>>,
    name: &str,
    bound: TypeVarBound,
) {
    let e = bounds.entry(name.to_string()).or_default();
    e.insert(TypeVarBound::Clone);
    e.insert(bound);
}

/// Walk a statement block, updating `locals` (so `infer_expr_ty` stays accurate
/// for later statements) and collecting type-var bounds from every expression.
pub(crate) fn infer_bounds_body(
    body: &[Stmt],
    locals: &mut HashMap<String, Ty>,
    ctx: &TyCtx,
    bounds: &mut std::collections::BTreeMap<String, std::collections::BTreeSet<TypeVarBound>>,
) {
    for s in body {
        infer_bounds_stmt(s, locals, ctx, bounds);
    }
}

pub(crate) fn infer_bounds_stmt(
    s: &Stmt,
    locals: &mut HashMap<String, Ty>,
    ctx: &TyCtx,
    bounds: &mut std::collections::BTreeMap<String, std::collections::BTreeSet<TypeVarBound>>,
) {
    match s {
        Stmt::Expr(e) | Stmt::Return(Some(e), _) | Stmt::Yield(e, _) => {
            infer_bounds_expr(e, locals, ctx, bounds);
        }
        Stmt::Assign { target, value, .. } => {
            infer_bounds_expr(value, locals, ctx, bounds);
            let t = infer_expr_ty(value, locals, ctx);
            locals.insert(target.clone(), t);
        }
        Stmt::AugAssign { value, .. } => {
            // `x += y` on a bare `T` is STILL REJECTED by typeck (aug-assign is
            // not in the v2 supported set), so an aug-assign never contributes a
            // type-var bound; only the RHS sub-expressions are scanned for nested
            // supported ops.
            infer_bounds_expr(value, locals, ctx, bounds);
        }
        Stmt::Unpack { targets, value, .. } => {
            infer_bounds_expr(value, locals, ctx, bounds);
            let vt = infer_expr_ty(value, locals, ctx);
            if let Ty::Tuple(elems) = &vt {
                for (i, t) in targets.iter().enumerate() {
                    locals.insert(t.clone(), elems.get(i).cloned().unwrap_or(Ty::Unknown));
                }
            } else {
                for t in targets {
                    locals.insert(t.clone(), Ty::Unknown);
                }
            }
        }
        Stmt::Return(None, _) | Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_)
        | Stmt::Import { .. } => {}
        Stmt::If { cond, then, elifs, else_, .. } => {
            infer_bounds_expr(cond, locals, ctx, bounds);
            infer_bounds_body(then, locals, ctx, bounds);
            for (c, b) in elifs {
                infer_bounds_expr(c, locals, ctx, bounds);
                infer_bounds_body(b, locals, ctx, bounds);
            }
            if let Some(b) = else_ {
                infer_bounds_body(b, locals, ctx, bounds);
            }
        }
        Stmt::While { cond, body, .. } => {
            infer_bounds_expr(cond, locals, ctx, bounds);
            infer_bounds_body(body, locals, ctx, bounds);
        }
        Stmt::For { targets, iter, body, .. } => {
            infer_bounds_expr(iter, locals, ctx, bounds);
            // Bind loop targets to the element type so a `print(item)` of a
            // type-var element infers Display. Iterating a bare `T` is rejected
            // by typeck, so the iterable is always a concrete container here.
            let elem = match infer_expr_ty(iter, locals, ctx) {
                // LAZY-GEN V1-a: a generator source yields elements like a list.
                Ty::List(inner) | Ty::Iterator(inner) | Ty::Set(inner) => *inner,
                Ty::Str => Ty::Str,
                _ => Ty::Unknown,
            };
            if targets.len() == 1 {
                locals.insert(targets[0].clone(), elem);
            } else if let Ty::Tuple(elems) = &elem {
                for (i, t) in targets.iter().enumerate() {
                    locals.insert(t.clone(), elems.get(i).cloned().unwrap_or(Ty::Unknown));
                }
            } else {
                for t in targets {
                    locals.insert(t.clone(), Ty::Unknown);
                }
            }
            infer_bounds_body(body, locals, ctx, bounds);
        }
        Stmt::Assert { cond, msg, .. } => {
            infer_bounds_expr(cond, locals, ctx, bounds);
            if let Some(m) = msg {
                infer_bounds_expr(m, locals, ctx, bounds);
            }
        }
        Stmt::Raise { exc, .. } => {
            if let Some(e) = exc {
                infer_bounds_expr(e, locals, ctx, bounds);
            }
        }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            infer_bounds_body(body, locals, ctx, bounds);
            for h in handlers {
                infer_bounds_body(&h.body, locals, ctx, bounds);
            }
            if let Some(b) = else_ {
                infer_bounds_body(b, locals, ctx, bounds);
            }
            if let Some(b) = finally_ {
                infer_bounds_body(b, locals, ctx, bounds);
            }
        }
        Stmt::With { ctx_expr, body, .. } => {
            infer_bounds_expr(ctx_expr, locals, ctx, bounds);
            infer_bounds_body(body, locals, ctx, bounds);
        }
        Stmt::Del { target, .. } => infer_bounds_expr(target, locals, ctx, bounds),
        Stmt::Match { subject, arms, .. } => {
            infer_bounds_expr(subject, locals, ctx, bounds);
            for a in arms {
                if let Some(g) = &a.guard {
                    infer_bounds_expr(g, locals, ctx, bounds);
                }
                infer_bounds_body(&a.body, locals, ctx, bounds);
            }
        }
        Stmt::AttrAssign { obj, value, .. } => {
            infer_bounds_expr(obj, locals, ctx, bounds);
            infer_bounds_expr(value, locals, ctx, bounds);
        }
        Stmt::IndexAssign { obj, idx, value, .. } => {
            infer_bounds_expr(obj, locals, ctx, bounds);
            infer_bounds_expr(idx, locals, ctx, bounds);
            infer_bounds_expr(value, locals, ctx, bounds);
        }
        // A nested `def`/`class` is its own generic scope (nested generic defs are
        // parser-rejected, and a nested non-generic def cannot reference the
        // outer `T` as a bound op since typeck scopes type params per function);
        // no outer-`T` bound flows out of it. `global`/`nonlocal` carry no
        // expression to infer bounds from.
        Stmt::Func(_) | Stmt::Class(_) | Stmt::Global { .. } | Stmt::Nonlocal { .. } => {}
    }
}

/// Collect type-var bounds from one expression. Each arm mirrors a typeck op-site
/// that NOW SUPPORTS a bare `T`: BinOp (`binop_typevar_bound`), Display contexts
/// (`print`/`str`/`repr`/`ascii` + f-strings), and hashable positions (set/dict
/// literals + comprehensions). Sub-expressions always recurse so a supported op
/// nested anywhere (e.g. `print(a + b)`) is found.
pub(crate) fn infer_bounds_expr(
    e: &Expr,
    locals: &HashMap<String, Ty>,
    ctx: &TyCtx,
    bounds: &mut std::collections::BTreeMap<String, std::collections::BTreeSet<TypeVarBound>>,
) {
    match e {
        Expr::BinOp { op, lhs, rhs, .. } => {
            // `T op T` (same variable) with a mapped bound -> record it. Anything
            // else with a type-var operand is rejected by typeck and never reaches
            // a successful build, so recording nothing for it is correct.
            let lt = infer_expr_ty(lhs, locals, ctx);
            let rt = infer_expr_ty(rhs, locals, ctx);
            if let (Ty::TypeVar(a), Ty::TypeVar(b)) = (&lt, &rt) {
                if a == b {
                    if let Some(bound) = binop_typevar_bound(*op) {
                        add_bound(bounds, a, bound);
                    }
                }
            }
            // Membership of a type-var element/key into a known container:
            // `k in dict`/`k in set` needs `K: Hash + Eq`; `x in list` needs
            // `T: PartialEq`. Mirrors the typeck accept-site (`container_membership`)
            // so the inferred trait clause matches the now-legal op exactly.
            if matches!(op, BinOp::In | BinOp::NotIn) {
                if let Ty::TypeVar(n) = &lt {
                    match &rt {
                        Ty::Dict(..) | Ty::Set(_) => {
                            add_bound(bounds, n, TypeVarBound::Hash);
                            add_bound(bounds, n, TypeVarBound::Eq);
                        }
                        Ty::List(_) => {
                            add_bound(bounds, n, TypeVarBound::PartialEq);
                        }
                        _ => {}
                    }
                }
            }
            infer_bounds_expr(lhs, locals, ctx, bounds);
            infer_bounds_expr(rhs, locals, ctx, bounds);
        }
        Expr::FStr(parts, _) => {
            for part in parts {
                if let FStrPart::Interp(expr, _) = part {
                    if let Ty::TypeVar(n) = infer_expr_ty(expr, locals, ctx) {
                        add_bound(bounds, &n, TypeVarBound::Display);
                    }
                    infer_bounds_expr(expr, locals, ctx, bounds);
                }
            }
        }
        Expr::Call { callee, args, kwargs, .. } => {
            // `print`/`str`/`ascii` of a bare `T` -> Display (Display formatting).
            // `repr` of a bare `T` -> Repr: it lowers to `x.py_repr()`, so the
            // type var needs the `PyRepr` bound, not Display — this is what quotes
            // str elements in a generic context (deque.remove's `%r` message).
            if let Expr::Ident(n, _) = callee.as_ref() {
                if matches!(n.as_str(), "print" | "str" | "ascii") {
                    for a in args {
                        if let Ty::TypeVar(tn) = infer_expr_ty(a, locals, ctx) {
                            add_bound(bounds, &tn, TypeVarBound::Display);
                        }
                    }
                } else if n == "repr" {
                    for a in args {
                        if let Ty::TypeVar(tn) = infer_expr_ty(a, locals, ctx) {
                            add_bound(bounds, &tn, TypeVarBound::Repr);
                        }
                    }
                }
            }
            infer_bounds_expr(callee, locals, ctx, bounds);
            for a in args {
                infer_bounds_expr(a, locals, ctx, bounds);
            }
            for (_, v) in kwargs {
                infer_bounds_expr(v, locals, ctx, bounds);
            }
        }
        Expr::Set(elems, _) => {
            for el in elems {
                if let Ty::TypeVar(n) = infer_expr_ty(el, locals, ctx) {
                    add_bound(bounds, &n, TypeVarBound::Hash);
                    add_bound(bounds, &n, TypeVarBound::Eq);
                }
                infer_bounds_expr(el, locals, ctx, bounds);
            }
        }
        Expr::Dict(pairs, _) => {
            for (k, v) in pairs {
                if let Ty::TypeVar(n) = infer_expr_ty(k, locals, ctx) {
                    add_bound(bounds, &n, TypeVarBound::Hash);
                    add_bound(bounds, &n, TypeVarBound::Eq);
                }
                infer_bounds_expr(k, locals, ctx, bounds);
                infer_bounds_expr(v, locals, ctx, bounds);
            }
        }
        Expr::SetComp { elt, targets, iter, cond, .. } => {
            // Bind comprehension targets to the iterable element type so an `elt`
            // referencing a type-var element is detected; then the produced
            // element (if a type var) needs Hash + Eq for the `HashSet<T>`.
            let mut inner = locals.clone();
            bind_comp_targets_for_bounds(targets, iter, &mut inner, ctx);
            if let Ty::TypeVar(n) = infer_expr_ty(elt, &inner, ctx) {
                add_bound(bounds, &n, TypeVarBound::Hash);
                add_bound(bounds, &n, TypeVarBound::Eq);
            }
            infer_bounds_expr(iter, locals, ctx, bounds);
            infer_bounds_expr(elt, &inner, ctx, bounds);
            if let Some(c) = cond {
                infer_bounds_expr(c, &inner, ctx, bounds);
            }
        }
        Expr::DictComp { key, val, targets, iter, cond, .. } => {
            let mut inner = locals.clone();
            bind_comp_targets_for_bounds(targets, iter, &mut inner, ctx);
            if let Ty::TypeVar(n) = infer_expr_ty(key, &inner, ctx) {
                add_bound(bounds, &n, TypeVarBound::Hash);
                add_bound(bounds, &n, TypeVarBound::Eq);
            }
            infer_bounds_expr(iter, locals, ctx, bounds);
            infer_bounds_expr(key, &inner, ctx, bounds);
            infer_bounds_expr(val, &inner, ctx, bounds);
            if let Some(c) = cond {
                infer_bounds_expr(c, &inner, ctx, bounds);
            }
        }
        Expr::ListComp { elt, targets, iter, cond, .. } => {
            let mut inner = locals.clone();
            bind_comp_targets_for_bounds(targets, iter, &mut inner, ctx);
            infer_bounds_expr(iter, locals, ctx, bounds);
            infer_bounds_expr(elt, &inner, ctx, bounds);
            if let Some(c) = cond {
                infer_bounds_expr(c, &inner, ctx, bounds);
            }
        }
        Expr::UnOp { expr, .. } => infer_bounds_expr(expr, locals, ctx, bounds),
        Expr::List(elems, _) | Expr::Tuple(elems, _) => {
            elems.iter().for_each(|e| infer_bounds_expr(e, locals, ctx, bounds));
        }
        Expr::Attr { obj, .. } => infer_bounds_expr(obj, locals, ctx, bounds),
        Expr::Index { obj, idx, .. } => {
            infer_bounds_expr(obj, locals, ctx, bounds);
            infer_bounds_expr(idx, locals, ctx, bounds);
        }
        Expr::Slice { obj, start, stop, step, .. } => {
            infer_bounds_expr(obj, locals, ctx, bounds);
            for o in [start, stop, step].into_iter().flatten() {
                infer_bounds_expr(o, locals, ctx, bounds);
            }
        }
        Expr::IfExp { test, body, orelse, .. } => {
            infer_bounds_expr(test, locals, ctx, bounds);
            infer_bounds_expr(body, locals, ctx, bounds);
            infer_bounds_expr(orelse, locals, ctx, bounds);
        }
        Expr::Lambda { body, .. } => infer_bounds_expr(body, locals, ctx, bounds),
        // Leaves carry no nested op.
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bytes(..) | Expr::Bool(..)
        | Expr::None_(_) | Expr::Ident(..) => {}
    }
}

/// Bind comprehension loop targets to the iterable's element type for the
/// bound-inference walk (mirror of the typeck `bind_comp_targets`, kept local so
/// the bounds pass stays self-contained).
pub(crate) fn bind_comp_targets_for_bounds(
    targets: &[String],
    iter: &Expr,
    locals: &mut HashMap<String, Ty>,
    ctx: &TyCtx,
) {
    let elem = match infer_expr_ty(iter, locals, ctx) {
        // LAZY-GEN V1-a: a generator source yields elements like a list.
        Ty::List(inner) | Ty::Iterator(inner) | Ty::Set(inner) => *inner,
        Ty::Str => Ty::Str,
        _ => Ty::Unknown,
    };
    if targets.len() == 1 {
        locals.insert(targets[0].clone(), elem);
    } else if let Ty::Tuple(elems) = &elem {
        for (i, t) in targets.iter().enumerate() {
            locals.insert(t.clone(), elems.get(i).cloned().unwrap_or(Ty::Unknown));
        }
    } else {
        for t in targets {
            locals.insert(t.clone(), Ty::Unknown);
        }
    }
}

