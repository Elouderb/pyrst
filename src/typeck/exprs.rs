use super::*;

/// Pure inference oracle — the single source of truth for expression types.
///
/// A side-effect-free port of codegen's `type_of_expr` (codegen.rs:264-548) with
/// the SAME contract: it never errors and never mutates; on any ambiguity it
/// falls to `Ty::Unknown` (preserving the `types_compatible` `(Unknown, _) => true`
/// escape hatch). Inputs are exactly what both call sites already hold — typeck's
/// `env.locals`/`env.ctx` and codegen's `self.locals`/`self.ctx` are identical
/// types — so E.2 can route both through here.
///
/// It bakes in the CORRECT side of every documented divergence
/// (docs/design/inference-oracle.md §A.4): D1 str-index → Str; D3 abs(x) → arg
/// type; D4 sum(xs) → element type; D5 `**` → Float; D6 dict literal folds ALL
/// pairs; D7 attribute access is inheritance-aware (`get_all_fields`).
pub fn infer_expr_ty(expr: &Expr, locals: &HashMap<String, Ty>, ctx: &TyCtx) -> Ty {
    match expr {
        Expr::Float(..) => Ty::Float,
        Expr::Int(..) => Ty::Int,
        Expr::Bool(..) => Ty::Bool,
        Expr::Str(..) | Expr::FStr(..) => Ty::Str,
        Expr::None_(_) => Ty::NoneVal,
        Expr::IfExp { body, orelse, .. } => {
            // Both branches unify in typeck; prefer the concrete one.
            let b = infer_expr_ty(body, locals, ctx);
            if b == Ty::Unknown {
                infer_expr_ty(orelse, locals, ctx)
            } else {
                b
            }
        }
        Expr::Ident(n, _) => locals
            .get(n.as_str())
            .or_else(|| ctx.vars.get(n.as_str()))
            .cloned()
            // A bare top-level function name in a value position infers to its
            // first-class `Ty::Func` type (`g = inc` -> g: Callable[[int],int]).
            // Locals/vars shadow it (checked first). Call sites never reach this
            // arm for the callee — `Expr::Call` resolves the name itself.
            .or_else(|| ctx.funcs.get(n.as_str()).map(func_sig_to_ty))
            .unwrap_or(Ty::Unknown),
        Expr::UnOp { op: UnOp::Neg, expr, .. } => infer_expr_ty(expr, locals, ctx),
        Expr::UnOp { op: UnOp::Not, .. } => Ty::Bool,
        Expr::UnOp { op: UnOp::BitNot, .. } => Ty::Int,
        Expr::BinOp { lhs, op, rhs, .. } => {
            let l = infer_expr_ty(lhs, locals, ctx);
            let r = infer_expr_ty(rhs, locals, ctx);
            match op {
                // D5: Python `**` always yields a float (split out of the
                // int-biased arithmetic arm below — codegen's bug).
                BinOp::Pow => Ty::Float,
                BinOp::Div => Ty::Float,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod | BinOp::FloorDiv => {
                    // Operator overloading: a class lhs uses its dunder return type.
                    if let Ty::Class(cls, _) = &l {
                        let dunder = match op {
                            BinOp::Add => Some("__add__"),
                            BinOp::Sub => Some("__sub__"),
                            BinOp::Mul => Some("__mul__"),
                            _ => None,
                        };
                        if let Some(ret) =
                            dunder.and_then(|d| ctx.get_method(cls, d)).map(|s| s.ret.clone())
                        {
                            return ret;
                        }
                    }
                    // String concatenation for Add.
                    if *op == BinOp::Add && (l == Ty::Str || r == Ty::Str) {
                        Ty::Str
                    } else if l == Ty::Float || r == Ty::Float {
                        Ty::Float
                    } else {
                        Ty::Int
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                | BinOp::And | BinOp::Or | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => {
                    Ty::Bool
                }
                _ => Ty::Unknown,
            }
        }
        Expr::Attr { obj, name, .. } => {
            // Qualified MODULE CONSTANT `X.CONST` for a REAL imported module:
            // when X is a tracked module and CONST is one of its module-level
            // constants, the access has the const's declared type. GENERALIZES
            // the former hardcoded `math.pi` typing — `math` is now a real
            // embedded module whose consts are tracked here.
            if let Expr::Ident(modname, _) = obj.as_ref() {
                if let Some(consts) = ctx.module_consts.get(modname) {
                    if let Some((_, ty)) = consts.iter().find(|(c, _)| c == name) {
                        return ty.clone();
                    }
                }
            }
            // D7: resolve the field inheritance-aware via `get_all_fields`
            // (codegen reads `c.fields` directly and misses inherited fields).
            let recv = infer_expr_ty(obj, locals, ctx);
            if let Ty::Class(cls, _) = &recv {
                let all_fields = ctx.get_all_fields(cls.as_str());
                if let Some(f) = all_fields.iter().find(|f| f.name == *name) {
                    // Generics v2: scope the field annotation with the class's
                    // type params (`value: T` -> `TypeVar(T)`) and substitute the
                    // receiver instance's args (`Box[int]` -> `{T -> int}`) so the
                    // oracle types `b.value` concretely (drives codegen var typing
                    // and print formatting). Non-generic class => empty subst =>
                    // identical to the old unscoped result.
                    let tps = ctx.classes.get(cls.as_str()).map(|c| c.type_params.as_slice()).unwrap_or(&[]);
                    let field_ty = Ty::from_type_expr_scoped(&f.ty, f.span, tps).unwrap_or(Ty::Unknown);
                    return subst_class_member(&field_ty, &recv, ctx);
                }
            }
            Ty::Unknown
        }
        Expr::Call { callee, args, .. } => {
            if let Expr::Ident(n, _) = callee.as_ref() {
                match n.as_str() {
                    "float" => Ty::Float,
                    "abs" => {
                        // D3: abs returns the same type as its argument.
                        if let Some(arg) = args.first() {
                            infer_expr_ty(arg, locals, ctx)
                        } else {
                            Ty::Unknown
                        }
                    }
                    "sum" => {
                        // D4: sum() returns the type of the iterable's elements.
                        if let Some(arg) = args.first() {
                            match infer_expr_ty(arg, locals, ctx) {
                                Ty::List(inner) => *inner,
                                Ty::Set(inner) => *inner,
                                // (LAZY-GEN V1-c) A generator source sums to its
                                // element type, same as a list/set.
                                Ty::Iterator(inner) => *inner,
                                _ => Ty::Int, // Default to int for other iterables.
                            }
                        } else {
                            Ty::Int
                        }
                    }
                    "int" | "len" | "ord" | "round" | "pow" => Ty::Int,
                    "bool" | "any" | "all" => Ty::Bool,
                    "str" | "chr" | "input" => Ty::Str,
                    "map" if args.len() == 2 => {
                        // map(f, iterable) -> List(applied return type of f).
                        // Only a List iterable yields a concrete List result;
                        // Set/Str/unknown stay Unknown (permissive).
                        match infer_expr_ty(&args[1], locals, ctx) {
                            Ty::List(e) => {
                                let body_ty = lambda_applied_ty(&args[0], &e, locals, ctx);
                                Ty::List(Box::new(body_ty))
                            }
                            _ => Ty::Unknown,
                        }
                    }
                    "filter" if args.len() == 2 => {
                        // filter(pred, iterable) -> the iterable's list type
                        // unchanged. Only List yields a concrete type.
                        match infer_expr_ty(&args[1], locals, ctx) {
                            Ty::List(e) => Ty::List(e),
                            _ => Ty::Unknown,
                        }
                    }
                    "sorted" | "list" | "reversed" => {
                        // These return a list; preserve the element type.
                        // Over a dict they operate on its KEYS (Python semantics),
                        // so the result element type is the dict's key type.
                        // (LAZY-GEN V1-c) `sorted(gen)`/`list(gen)` materialize a
                        // generator into `list[T]`, same element type as a
                        // list/set source (`reversed(gen)` is a V1-d MATERIALIZE
                        // error at the codegen/typeck-error layer; this arm is
                        // the pure, non-erroring inference oracle and just
                        // reports the type it WOULD be).
                        if let Some(arg) = args.first() {
                            match infer_expr_ty(arg, locals, ctx) {
                                Ty::List(e) | Ty::Set(e) | Ty::Iterator(e) => Ty::List(e),
                                Ty::Dict(k, _) => Ty::List(k),
                                Ty::Str => Ty::List(Box::new(Ty::Str)),
                                _ => Ty::List(Box::new(Ty::Unknown)),
                            }
                        } else {
                            Ty::List(Box::new(Ty::Unknown))
                        }
                    }
                    n => {
                        // A class constructor yields an instance; a named user
                        // function yields its declared return type; a func-VALUED
                        // local/param/var (`f: Callable[[int],int]`) called as
                        // `f(x)` yields the function value's return type.
                        if ctx.classes.contains_key(n) {
                            // Generics v2: for a generic class, INFER its type args
                            // from the constructor argument types (`Box(5)` ->
                            // `Box[int]`), matching the checking path so codegen
                            // sees the same concrete instance type. A non-generic
                            // class yields the legacy `Ty::Class(n, [])`.
                            let arg_tys: Vec<Ty> = args.iter()
                                .map(|a| infer_expr_ty(a, locals, ctx))
                                .collect();
                            infer_class_instantiation(n, &arg_tys, ctx)
                        } else if let Some(sig) = ctx.funcs.get(n) {
                            // Generics v1: for a generic call, infer the concrete
                            // result by unifying the declared param types against
                            // the argument types — so `first([10, 20])` infers
                            // `int` (not `T`) and `swap(5, "x")` infers
                            // `tuple[str, int]`. This is what lets codegen pick the
                            // right print-formatting and variable types for a
                            // generic call's RESULT (the result is always concrete
                            // after substitution). Shared with the qualified arm via
                            // `oracle_generic_call_ret`, which never errors.
                            let arg_tys: Vec<Ty> = args.iter()
                                .map(|a| infer_expr_ty(a, locals, ctx))
                                .collect();
                            oracle_generic_call_ret(n, sig, &arg_tys, ctx)
                        } else if let Some(Ty::Func(_, ret)) =
                            locals.get(n).or_else(|| ctx.vars.get(n))
                        {
                            (**ret).clone()
                        } else {
                            Ty::Unknown
                        }
                    }
                }
            } else if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                // Qualified module call `X.f(args)` for a REAL imported module
                // (card 81db88e0): when X is a tracked module and f is one of its
                // functions, the call's type is f's declared return type — exactly
                // as if `f(args)` were called by its flat name. `math` is now a
                // real embedded module (`lib/math.pyrs`), so `math.sqrt(x)` flows
                // through here (its @extern `sqrt` lives in `module_funcs`); the
                // former hardcoded math return-typing arm is gone.
                if let Expr::Ident(modname, _) = obj.as_ref() {
                    if ctx.module_funcs.get(modname).is_some_and(|fns| fns.iter().any(|n| n == name)) {
                        // Generics v1: a QUALIFIED generic stdlib call
                        // (`heapq.heappop(h)`) substitutes its inferred type args so
                        // codegen sees a CONCRETE result type — the same handling as
                        // the flat form, via the shared `oracle_generic_call_ret`. A
                        // non-generic module fn returns its declared type unchanged.
                        return match ctx.funcs.get(name) {
                            Some(sig) => {
                                let arg_tys: Vec<Ty> = args.iter()
                                    .map(|a| infer_expr_ty(a, locals, ctx))
                                    .collect();
                                oracle_generic_call_ret(name, sig, &arg_tys, ctx)
                            }
                            None => Ty::Unknown,
                        };
                    }
                }
                // Class methods use their declared return; builtin receivers
                // (str/list/set/dict/file) delegate to the shared
                // `builtin_method_ret` so the two never drift and chained calls
                // resolve.
                let recv = infer_expr_ty(obj, locals, ctx);
                if let Ty::Class(cls, _) = &recv {
                    // Generics v2: substitute the receiver instance's type args
                    // into the method's (type-var-bearing) return, so a generic
                    // method call types concretely for codegen (`b.get(): int`).
                    ctx.get_method(cls, name)
                        .map(|s| subst_class_member(&s.ret, &recv, ctx))
                        .unwrap_or(Ty::Unknown)
                } else if let Some(t) = dict_get_ret(&recv, name, args.len()) {
                    // dict.get is arg-count-aware: get(k) -> Optional[V],
                    // get(k, default) -> V (see dict_get_ret).
                    t
                } else {
                    builtin_method_ret(&recv, name)
                }
            } else {
                // Calling a function VALUE whose callee is an arbitrary expression
                // (a lambda, an indexed slot `ops["double"]`, an attr, ...). Infer
                // the callee's type and, if it is a `Ty::Func`, surface its return
                // type so `ops["double"](7)` and `(make_adder(5))(10)` are typed.
                match infer_expr_ty(callee, locals, ctx) {
                    Ty::Func(_, ret) => *ret,
                    _ => Ty::Unknown,
                }
            }
        }
        Expr::List(elems, _) => {
            // Unify all element types (not first-element-wins) so a mixed numeric
            // literal like `[1, 2.0]` is typed `List(Float)`.
            Ty::List(Box::new(infer_list_elem_ty(elems, locals, ctx)))
        }
        Expr::Dict(pairs, _) => {
            // D6: fold ALL pairs, unifying key types and value types
            // independently (codegen uses the first pair only). On a both-concrete
            // conflict, degrade THAT position to Unknown — never error (the pure
            // contract; check_expr rejects, this oracle stays permissive).
            if pairs.is_empty() {
                Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown))
            } else {
                let mut k_ty = infer_expr_ty(&pairs[0].0, locals, ctx);
                let mut v_ty = infer_expr_ty(&pairs[0].1, locals, ctx);
                for (k, v) in &pairs[1..] {
                    let kt = infer_expr_ty(k, locals, ctx);
                    let vt = infer_expr_ty(v, locals, ctx);
                    // widen_numeric=false: float dict keys are non-hashable and
                    // dict values have no codegen cast, matching check_expr.
                    k_ty = unify_elem_types(k_ty.clone(), kt, false, ctx).unwrap_or(Ty::Unknown);
                    v_ty = unify_elem_types(v_ty.clone(), vt, false, ctx).unwrap_or(Ty::Unknown);
                }
                Ty::Dict(Box::new(k_ty), Box::new(v_ty))
            }
        }
        Expr::Set(elems, _) => {
            // Unify all element types (mirrors the list case).
            Ty::Set(Box::new(infer_list_elem_ty(elems, locals, ctx)))
        }
        Expr::ListComp { elt, targets, iter, .. } => {
            // Infer element type from the iterable and element expression.
            let iter_ty = infer_expr_ty(iter, locals, ctx);
            let elem_iter_ty = match &iter_ty {
                // LAZY-GEN V1-a: a generator source == a list source, element-wise.
                Ty::List(inner) | Ty::Iterator(inner) | Ty::Set(inner) => Some(inner.as_ref().clone()),
                _ => None,
            };
            // The single-variable oracle only applies to single-target comps;
            // for tuple-unpacking targets we fall through to the iterable-elem
            // fallback (the authoritative element type comes from `check_expr`).
            if let (Some(elem_iter_type), [target]) = (&elem_iter_ty, targets.as_slice()) {
                let inferred =
                    infer_comp_elt_type_with_var(elt, elem_iter_type, target, ctx);
                if inferred != Ty::Unknown {
                    return Ty::List(Box::new(inferred));
                }
            }
            // Fallback: use the iterable's element type.
            match iter_ty {
                // LAZY-GEN V1-a: a comprehension over a generator yields a list of
                // its element type, exactly like a comprehension over a list.
                Ty::List(inner) | Ty::Iterator(inner) => Ty::List(inner),
                Ty::Set(inner) => Ty::List(inner),
                _ => Ty::List(Box::new(Ty::Unknown)),
            }
        }
        Expr::SetComp { elt, targets: _, iter, .. } => {
            let iter_ty = infer_expr_ty(iter, locals, ctx);
            if let Ty::List(ref inner) | Ty::Iterator(ref inner) | Ty::Set(ref inner) = iter_ty {
                match elt.as_ref() {
                    Expr::Attr { name, .. } => {
                        if let Ty::Class(cls, _) = inner.as_ref() {
                            if let Some(c) = ctx.classes.get(cls.as_str()) {
                                if let Some(f) = c.fields.iter().find(|f| f.name == *name) {
                                    // Generics v2: scope the field with the class's
                                    // type params (`value: T` -> `TypeVar(T)`) then
                                    // substitute the element instance's args
                                    // (`Box[int]` -> `{T -> int}`), so a comp over a
                                    // generic-class element infers the concrete
                                    // field type. Non-generic class => no-op.
                                    if let Ok(ty) = Ty::from_type_expr_scoped(&f.ty, f.span, &c.type_params) {
                                        return Ty::Set(Box::new(subst_class_member(&ty, inner, ctx)));
                                    }
                                }
                            }
                        }
                    }
                    Expr::Call { callee, .. } => {
                        if let Expr::Attr { name, .. } = callee.as_ref() {
                            if let Ty::Class(cls, _) = inner.as_ref() {
                                if let Some(method_sig) = ctx.get_method(cls.as_str(), name) {
                                    // Substitute the element instance's type args
                                    // into the (scoped) method return.
                                    return Ty::Set(Box::new(subst_class_member(&method_sig.ret, inner, ctx)));
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
        Expr::DictComp { key, val, targets: _, iter, .. } => {
            let iter_ty = infer_expr_ty(iter, locals, ctx);
            let field_ty = |e: &Expr| -> Ty {
                if let Expr::Attr { name, .. } = e {
                    if let Ty::Class(ref cls, _) = iter_ty {
                        if let Some(c) = ctx.classes.get(cls.as_str()) {
                            if let Some(f) = c.fields.iter().find(|f| f.name == *name) {
                                // Generics v2: scope + substitute the field type
                                // against the generic-class instance (mirrors the
                                // non-comprehension field-access path).
                                let ty = Ty::from_type_expr_scoped(&f.ty, f.span, &c.type_params).unwrap_or(Ty::Unknown);
                                return subst_class_member(&ty, &iter_ty, ctx);
                            }
                        }
                    }
                }
                Ty::Unknown
            };
            Ty::Dict(Box::new(field_ty(key)), Box::new(field_ty(val)))
        }
        Expr::Index { obj, .. } => {
            // D1: a Str receiver yields Str (codegen lacks this arm). Dict[k] is
            // the value type; List[i] is the element type.
            match infer_expr_ty(obj, locals, ctx) {
                Ty::Dict(_, val_ty) => *val_ty,
                Ty::List(elem_ty) => *elem_ty,
                Ty::Str => Ty::Str,
                _ => Ty::Unknown,
            }
        }
        Expr::Slice { obj, .. } => {
            // A slice yields the SAME container kind: str -> str (substring),
            // list[T] -> list[T] (sublist). Without this arm a slice fell through
            // to Unknown, so an inline `int(s[a:b])` / `float(s[a:b])` took the
            // numeric-cast path and miscompiled (`String as i64`) — the oracle had
            // an Index arm but no Slice arm.
            match infer_expr_ty(obj, locals, ctx) {
                Ty::Str => Ty::Str,
                list_ty @ Ty::List(_) => list_ty,
                _ => Ty::Unknown,
            }
        }
        // A lambda is a first-class function value. Its parameters carry no
        // annotation in pyrst, so each argument type is `Unknown`; the return
        // type is the body's type with the parameter names bound to `Unknown`.
        // The result `Callable[[unknown, ...], body_ty]` is permissive — it fills
        // any `Callable` slot of matching arity (see `types_compatible`).
        Expr::Lambda { params, body, .. } => {
            let mut inner = locals.clone();
            for (name, _) in params {
                inner.insert(name.clone(), Ty::Unknown);
            }
            let ret = infer_expr_ty(body, &inner, ctx);
            Ty::Func(vec![Ty::Unknown; params.len()], Box::new(ret))
        }
        _ => Ty::Unknown,
    }
}

/// Unified element type of a list/set literal's elements, for `infer_expr_ty`.
/// Folds every element's type with `unify_oracle_ty` (not first-element-wins) so
/// a mixed numeric literal like `[1, 2.0]` is typed `Float`. Empty -> `Unknown`.
/// Pure port of codegen's `list_elem_ty`/`unify_ty`.
pub(crate) fn infer_list_elem_ty(elems: &[Expr], locals: &HashMap<String, Ty>, ctx: &TyCtx) -> Ty {
    let mut iter = elems.iter();
    match iter.next() {
        None => Ty::Unknown,
        Some(first) => iter.fold(infer_expr_ty(first, locals, ctx), |acc, e| {
            unify_oracle_ty(acc, infer_expr_ty(e, locals, ctx))
        }),
    }
}

/// Structural element-type unification for collection literals (pure port of
/// codegen's `unify_ty`). Int/Float widen to Float; nested collections recurse;
/// `Unknown` is absorbed; otherwise the left (concrete) side wins.
pub(crate) fn unify_oracle_ty(a: Ty, b: Ty) -> Ty {
    match (a, b) {
        (Ty::Unknown, x) | (x, Ty::Unknown) => x,
        (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => Ty::Float,
        (Ty::Dict(k1, v1), Ty::Dict(k2, v2)) => Ty::Dict(
            Box::new(unify_oracle_ty(*k1, *k2)),
            Box::new(unify_oracle_ty(*v1, *v2)),
        ),
        (Ty::List(e1), Ty::List(e2)) => Ty::List(Box::new(unify_oracle_ty(*e1, *e2))),
        (Ty::Set(e1), Ty::Set(e2)) => Ty::Set(Box::new(unify_oracle_ty(*e1, *e2))),
        (a, _) => a,
    }
}

/// Infer the applied return type of a `map`'s callable over an element of type
/// `elem`, for `infer_expr_ty`'s `map` arm. Pure port of codegen's
/// `lambda_applied_ty` -> `type_of_expr_bound`.
pub(crate) fn lambda_applied_ty(callable: &Expr, elem: &Ty, locals: &HashMap<String, Ty>, ctx: &TyCtx) -> Ty {
    if let Expr::Lambda { params, body, .. } = callable {
        if let Some((param, _)) = params.first() {
            return infer_expr_ty_bound(body, param, elem, locals, ctx);
        }
    }
    Ty::Unknown
}

/// Like `infer_expr_ty`, but the single identifier `param` resolves to `elem`
/// (the bound lambda parameter). Recurses through the compound forms that appear
/// in map lambda bodies; for everything else it delegates to `infer_expr_ty`.
/// Pure port of codegen's `type_of_expr_bound`.
pub(crate) fn infer_expr_ty_bound(
    e: &Expr,
    param: &str,
    elem: &Ty,
    locals: &HashMap<String, Ty>,
    ctx: &TyCtx,
) -> Ty {
    match e {
        Expr::Ident(n, _) if n == param => elem.clone(),
        Expr::UnOp { op: UnOp::Neg, expr, .. } => {
            infer_expr_ty_bound(expr, param, elem, locals, ctx)
        }
        Expr::IfExp { body, orelse, .. } => {
            let b = infer_expr_ty_bound(body, param, elem, locals, ctx);
            if b == Ty::Unknown {
                infer_expr_ty_bound(orelse, param, elem, locals, ctx)
            } else {
                b
            }
        }
        Expr::BinOp { lhs, op, rhs, .. } => {
            let l = infer_expr_ty_bound(lhs, param, elem, locals, ctx);
            let r = infer_expr_ty_bound(rhs, param, elem, locals, ctx);
            match op {
                BinOp::Div | BinOp::Pow => Ty::Float,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod | BinOp::FloorDiv => {
                    if *op == BinOp::Add && (l == Ty::Str || r == Ty::Str) {
                        Ty::Str
                    } else if l == Ty::Float || r == Ty::Float {
                        Ty::Float
                    } else if l == Ty::Int || r == Ty::Int {
                        Ty::Int
                    } else {
                        Ty::Unknown
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                | BinOp::And | BinOp::Or | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => {
                    Ty::Bool
                }
                _ => Ty::Unknown,
            }
        }
        // Other forms do not depend on `param` for their result type — delegate.
        _ => infer_expr_ty(e, locals, ctx),
    }
}

/// Bind a comprehension's loop target(s) into `locals` from the iterable's
/// element type. A single target gets the full element type; multiple targets
/// (tuple-unpacking, e.g. `for k, v in d.items()`) destructure a matching-arity
/// `Ty::Tuple` into each, falling back to `Unknown`. Mirrors the `Stmt::For`
/// binding in `check_stmt`.
pub(crate) fn bind_comp_targets(targets: &[String], elem_ty: Ty, locals: &mut HashMap<String, Ty>) {
    if targets.len() == 1 {
        locals.insert(targets[0].clone(), elem_ty);
    } else {
        let elem_tys = match &elem_ty {
            Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
            _ => vec![Ty::Unknown; targets.len()],
        };
        for (i, target) in targets.iter().enumerate() {
            locals.insert(target.clone(), elem_tys.get(i).cloned().unwrap_or(Ty::Unknown));
        }
    }
}

/// Infer a comprehension element expression's type given the loop variable's
/// type and name, for `infer_expr_ty`'s comprehension arms. Pure port of
/// codegen's `infer_comp_elt_type_with_var`.
pub(crate) fn infer_comp_elt_type_with_var(
    elt: &Expr,
    loop_var_ty: &Ty,
    loop_var_name: &str,
    ctx: &TyCtx,
) -> Ty {
    match elt {
        // [i.field for i in items] or [i.a.b for i in items]
        Expr::Attr { obj, name, .. } => {
            let obj_ty = if let Expr::Ident(var_name, _) = obj.as_ref() {
                if var_name == loop_var_name {
                    loop_var_ty.clone()
                } else {
                    Ty::Unknown
                }
            } else {
                infer_comp_elt_type_with_var(obj, loop_var_ty, loop_var_name, ctx)
            };
            if let Ty::Class(cls, _) = &obj_ty {
                if let Some(c) = ctx.classes.get(cls.as_str()) {
                    if let Some(f) = c.fields.iter().find(|f| f.name == *name) {
                        // Generics v2: scope the field with the class's type params
                        // and substitute the loop-var instance's args, so
                        // `[item.value for item in boxes]` over `list[Box[int]]`
                        // infers `int` (not the bare `T`). Non-generic class: no-op.
                        let ty = Ty::from_type_expr_scoped(&f.ty, f.span, &c.type_params).unwrap_or(Ty::Unknown);
                        return subst_class_member(&ty, &obj_ty, ctx);
                    }
                }
            }
            Ty::Unknown
        }
        // [i.method() for i in items]
        Expr::Call { callee, .. } => {
            if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                let obj_ty = if let Expr::Ident(var_name, _) = obj.as_ref() {
                    if var_name == loop_var_name {
                        loop_var_ty.clone()
                    } else {
                        Ty::Unknown
                    }
                } else {
                    infer_comp_elt_type_with_var(obj, loop_var_ty, loop_var_name, ctx)
                };
                if let Ty::Class(cls, _) = &obj_ty {
                    if let Some(method_sig) = ctx.get_method(cls.as_str(), name) {
                        // Substitute the loop-var instance's type args into the
                        // (scoped) method return.
                        return subst_class_member(&method_sig.ret, &obj_ty, ctx);
                    }
                }
            }
            Ty::Unknown
        }
        // [i.a + i.b for i in items] - infer from BinOp.
        Expr::BinOp { lhs, op, rhs, .. } => {
            let left_ty = infer_comp_elt_type_with_var(lhs, loop_var_ty, loop_var_name, ctx);
            let right_ty = infer_comp_elt_type_with_var(rhs, loop_var_ty, loop_var_name, ctx);
            match (left_ty, right_ty) {
                (Ty::Float, _) | (_, Ty::Float) => Ty::Float,
                (Ty::Int, Ty::Int) => {
                    if *op == BinOp::Div || *op == BinOp::Pow {
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

/// Resolve a lambda parameter's annotation to a `Ty`. Lambda params are
/// untyped in the surface syntax; the parser records the placeholder
/// `TypeExpr::Named("Any")`. That sentinel must resolve to `Ty::Unknown` (not
/// the bogus `Ty::Class("Any", vec![])` the generic resolver would produce), so a
/// param-dependent lambda body stays permissive instead of spuriously typing as
/// a nonexistent class.
pub(crate) fn lambda_param_ty(param_ty: &TypeExpr) -> Ty {
    if let TypeExpr::Named(n) = param_ty {
        if n == "Any" {
            return Ty::Unknown;
        }
    }
    // Inference-only fallback: a lambda param annotation has no carried span and
    // any error is swallowed to `Unknown`, so a dummy span never reaches a user.
    Ty::from_type_expr(param_ty, Span::DUMMY).unwrap_or(Ty::Unknown)
}

/// Infer the return type of a callable applied to a single element of type
/// `elem`, for the `map`/`filter` special cases.
///
/// When `callable` is an inline `lambda` with at least one parameter, its first
/// param is bound to `elem` (or `Unknown` when the iterable element type is
/// unknown) in a temporary env, the body is type-checked, and its inferred type
/// is returned as `Some(body_ty)`. For any other callable (a named function,
/// `def`-bound variable, etc.) or a parameterless lambda, the expression is
/// still type-checked for its own errors and `None` is returned so the caller
/// stays permissive (yields `Ty::Unknown`). This never narrows
/// `types_compatible`; it only widens positive inference.
pub(crate) fn lambda_ret_with_elem(
    callable: &Expr,
    elem: Option<&Ty>,
    env: &mut FuncEnv,
) -> Result<Option<Ty>> {
    if let Expr::Lambda { params, body, .. } = callable {
        if !params.is_empty() {
            let mut lambda_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: Ty::Unknown,
                used_vars: env.used_vars.clone(),
                params: std::collections::HashSet::new(),
                reassigned_params: std::collections::HashSet::new(),
                returned_params: std::collections::HashSet::new(),
                by_ref_params: std::collections::HashSet::new(),
                // A lambda body is a single expression and can never contain a
                // `yield` statement, so it is never a generator.
                is_generator: false,
                // A lambda introduces its own (untyped) parameters; the enclosing
                // function's type variables are not in scope for the lambda's own
                // params, so this stays empty.
                type_params: std::collections::HashSet::new(),
            };
            // Bind every param: the first to the iterable element type, the
            // rest to their declared type or Unknown (map/filter pass a single
            // element, so only the first param is meaningfully constrained).
            for (i, (param_name, param_ty)) in params.iter().enumerate() {
                let ty = if i == 0 {
                    elem.cloned().unwrap_or(Ty::Unknown)
                } else {
                    lambda_param_ty(param_ty)
                };
                lambda_env.locals.insert(param_name.clone(), ty);
            }
            let body_ty = check_expr(body, &mut lambda_env)?;
            return Ok(Some(body_ty));
        }
    }
    // Non-lambda callable (or zero-param lambda): still check it for its own
    // errors, but we cannot infer an applied return type here.
    check_expr(callable, env)?;
    Ok(None)
}

pub(crate) fn check_expr(e: &Expr, env: &mut FuncEnv) -> Result<Ty> {
    Ok(match e {
        Expr::Int(_, _) => Ty::Int,
        Expr::Float(_, _) => Ty::Float,
        Expr::Str(_, _) => Ty::Str,
        Expr::FStr(parts, _span) => {
            // Visit each interpolation: an f-string FORMATS each `{expr}` via the
            // value's `Display`. Generics v2: a bare type variable (`f"{x}"` where
            // `x: T`) is now LEGAL — it infers a `Display` bound on `T` (collected
            // by `infer_func_typevar_bounds`, emitted in the generic clause), so
            // the generated `format!("{}", x)` is well-typed. Checking the
            // sub-exprs still surfaces any of THEIR own errors.
            for part in parts {
                if let FStrPart::Interp(expr, _) = part {
                    check_expr(expr, env)?;
                }
            }
            Ty::Str
        }
        Expr::Bool(_, _) => Ty::Bool,
        Expr::Tuple(elems, _) => {
            let tys = elems.iter().map(|e| check_expr(e, env)).collect::<Result<Vec<_>>>()?;
            Ty::Tuple(tys)
        }
        Expr::IfExp { test, body, orelse, span } => {
            check_expr(test, env)?;
            let bt = check_expr(body, env)?;
            let ot = check_expr(orelse, env)?;
            // Both arms must agree; the more concrete side wins so a branch like
            // `[]` (List(Unknown)) unifies with `[1, 2, 3]` (List(Int)).
            unify_branch_types(bt.clone(), ot.clone(), env.ctx).ok_or_else(|| Error::Type {
                span: *span,
                msg: format!(
                    "conditional expression branches have incompatible types: `{}` vs `{}`",
                    bt, ot
                ),
            })?
        }
        Expr::ListComp { elt, targets, iter, cond, .. } => {
            let iter_ty = check_expr(iter, env)?;
            // Generics v1: a comprehension iterates its source, so iterating a bare
            // type variable needs an `IntoIterator` bound (E0599 otherwise).
            // (The element-type match below falls through to a concrete type for
            // an opaque iterable, hiding the gap from `check` — so reject here,
            // mirroring the `Stmt::For` gate.)
            reject_typevar_op(&iter_ty, "iterate over", iter.span())?;
            let elem_ty = match &iter_ty {
                // LAZY-GEN V1-a: a generator source (`Ty::Iterator`) yields the
                // same element type as a `list[T]` — treated identically.
                Ty::List(inner) | Ty::Iterator(inner) => *inner.clone(),
                Ty::Set(inner) => *inner.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Int, // ranges and unknown iterables -> Int
            };
            // Create a new scope with the loop variable(s) bound
            let mut inner_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: env.ret_ty.clone(),
                used_vars: env.used_vars.clone(),
                params: env.params.clone(),
                reassigned_params: env.reassigned_params.clone(),
                returned_params: env.returned_params.clone(),
                by_ref_params: env.by_ref_params.clone(),
                // A comprehension lives inside the current function; inherit its
                // generator status so the bare-return / yield rules stay coherent
                // (a `yield` cannot appear inside a comprehension expression, but
                // propagating keeps the env honest).
                is_generator: env.is_generator,
                // Inherit the enclosing generic function's type parameters so a
                // comprehension body inside a generic function keeps the
                // ops-on-`T` restriction.
                type_params: env.type_params.clone(),
            };
            bind_comp_targets(targets, elem_ty, &mut inner_env.locals);
            if let Some(c) = cond { check_expr(c, &mut inner_env)?; }
            let elt_ty = check_expr(elt, &mut inner_env)?;
            Ty::List(Box::new(elt_ty))
        }
        Expr::SetComp { elt, targets, iter, cond, span } => {
            let iter_ty = check_expr(iter, env)?;
            // Generics v1: a comprehension iterates its source, so iterating a bare
            // type variable needs an `IntoIterator` bound (E0599 otherwise).
            // (The element-type match below falls through to a concrete type for
            // an opaque iterable, hiding the gap from `check` — so reject here,
            // mirroring the `Stmt::For` gate.)
            reject_typevar_op(&iter_ty, "iterate over", iter.span())?;
            let elem_ty = match &iter_ty {
                // LAZY-GEN V1-a: a generator source (`Ty::Iterator`) yields the
                // same element type as a `list[T]` — treated identically.
                Ty::List(inner) | Ty::Iterator(inner) => *inner.clone(),
                Ty::Set(inner) => *inner.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Int,
            };
            let mut inner_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: env.ret_ty.clone(),
                used_vars: env.used_vars.clone(),
                params: env.params.clone(),
                reassigned_params: env.reassigned_params.clone(),
                returned_params: env.returned_params.clone(),
                by_ref_params: env.by_ref_params.clone(),
                // A comprehension lives inside the current function; inherit its
                // generator status so the bare-return / yield rules stay coherent
                // (a `yield` cannot appear inside a comprehension expression, but
                // propagating keeps the env honest).
                is_generator: env.is_generator,
                // Inherit the enclosing generic function's type parameters so a
                // comprehension body inside a generic function keeps the
                // ops-on-`T` restriction.
                type_params: env.type_params.clone(),
            };
            bind_comp_targets(targets, elem_ty, &mut inner_env.locals);
            if let Some(c) = cond { check_expr(c, &mut inner_env)?; }
            let elt_ty = check_expr(elt, &mut inner_env)?;
            // Same hashability rule as set literals: a Float element produces
            // the uncompilable `HashSet<f64>`, so reject it here too.
            require_hashable(&elt_ty, *span, "set element")?;
            Ty::Set(Box::new(elt_ty))
        }
        Expr::DictComp { key, val, targets, iter, cond, span } => {
            let iter_ty = check_expr(iter, env)?;
            // Generics v1: a comprehension iterates its source, so iterating a bare
            // type variable needs an `IntoIterator` bound (E0599 otherwise).
            // (The element-type match below falls through to a concrete type for
            // an opaque iterable, hiding the gap from `check` — so reject here,
            // mirroring the `Stmt::For` gate.)
            reject_typevar_op(&iter_ty, "iterate over", iter.span())?;
            let elem_ty = match &iter_ty {
                // LAZY-GEN V1-a: a generator source (`Ty::Iterator`) yields the
                // same element type as a `list[T]` — treated identically.
                Ty::List(inner) | Ty::Iterator(inner) => *inner.clone(),
                Ty::Set(inner) => *inner.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Int,
            };
            let mut inner_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: env.ret_ty.clone(),
                used_vars: env.used_vars.clone(),
                params: env.params.clone(),
                reassigned_params: env.reassigned_params.clone(),
                returned_params: env.returned_params.clone(),
                by_ref_params: env.by_ref_params.clone(),
                // A comprehension lives inside the current function; inherit its
                // generator status so the bare-return / yield rules stay coherent
                // (a `yield` cannot appear inside a comprehension expression, but
                // propagating keeps the env honest).
                is_generator: env.is_generator,
                // Inherit the enclosing generic function's type parameters so a
                // comprehension body inside a generic function keeps the
                // ops-on-`T` restriction.
                type_params: env.type_params.clone(),
            };
            bind_comp_targets(targets, elem_ty, &mut inner_env.locals);
            if let Some(c) = cond { check_expr(c, &mut inner_env)?; }
            let key_ty = check_expr(key, &mut inner_env)?;
            let val_ty = check_expr(val, &mut inner_env)?;
            // Same hashability rule as dict literals: a Float KEY produces the
            // uncompilable `HashMap<f64, _>`. Values may be Float.
            require_hashable(&key_ty, *span, "dict key")?;
            Ty::Dict(Box::new(key_ty), Box::new(val_ty))
        }
        Expr::None_(_) => Ty::NoneVal,
        Expr::List(elems, span) => {
            let elem_ty = if elems.is_empty() {
                Ty::Unknown
            } else {
                // Unify all element types: every element is checked (for its own
                // errors), and their types are folded together. A genuinely
                // heterogeneous literal (two both-concrete, non-Unknown,
                // non-numeric-mixable types) is rejected here instead of being
                // silently typed as `List(first-element-type)` and deferred to
                // rustc. Int/Float mixing and Unknown elements stay permissive.
                let mut acc = check_expr(&elems[0], env)?;
                for e in &elems[1..] {
                    let next = check_expr(e, env)?;
                    // Lists may hold floats, so int/float elements widen to Float.
                    acc = unify_elem_types(acc.clone(), next.clone(), true, env.ctx).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "list elements have incompatible types: {} vs {}",
                            acc, next
                        ),
                    })?;
                }
                acc
            };
            Ty::List(Box::new(elem_ty))
        }
        Expr::Set(elems, span) => {
            let elem_ty = if elems.is_empty() {
                Ty::Unknown
            } else {
                // Same element-type unification as list literals above, but
                // WITHOUT Int/Float widening: a set's element type must be
                // hashable and `set[float]` (`HashSet<f64>`) is not representable
                // in Rust, so `{1, 2.0}` is rejected rather than typed Set(Float).
                let mut acc = check_expr(&elems[0], env)?;
                for e in &elems[1..] {
                    let next = check_expr(e, env)?;
                    acc = unify_elem_types(acc.clone(), next.clone(), false, env.ctx).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "set elements have incompatible types: {} vs {}",
                            acc, next
                        ),
                    })?;
                }
                acc
            };
            // A pure-float set literal (`{1.0, 2.0}`) folds to Set(Float), which
            // codegen would emit as the uncompilable `HashSet<f64>`. Reject it
            // here so typeck and codegen agree. (`{1, 2.0}` is already rejected
            // by the widen_numeric=false fold above; this closes the all-float
            // case.) Unknown element types (`set()`) stay permissive.
            require_hashable(&elem_ty, *span, "set element")?;
            Ty::Set(Box::new(elem_ty))
        }
        Expr::Dict(pairs, span) => {
            if pairs.is_empty() {
                Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown))
            } else {
                // Unify all key types and all value types independently via a
                // left-to-right fold. Genuinely heterogeneous dicts (two
                // both-concrete incompatible key or value types) are rejected
                // here instead of silently using first-pair types and deferring
                // the error to rustc. widen_numeric=false for both: float dict
                // keys are non-hashable (HashMap<f64,_> doesn't compile), and
                // there is no codegen value-cast for dict values, so mixed
                // Int/Float values would also fail at rustc.
                let mut k_ty = check_expr(&pairs[0].0, env)?;
                let mut v_ty = check_expr(&pairs[0].1, env)?;
                for (k, v) in &pairs[1..] {
                    let kt = check_expr(k, env)?;
                    let vt = check_expr(v, env)?;
                    k_ty = unify_elem_types(k_ty.clone(), kt.clone(), false, env.ctx).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "dict keys have incompatible types: {} vs {}",
                            k_ty, kt
                        ),
                    })?;
                    v_ty = unify_elem_types(v_ty.clone(), vt.clone(), false, env.ctx).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "dict values have incompatible types: {} vs {}",
                            v_ty, vt
                        ),
                    })?;
                }
                // A float-keyed dict literal (`{1.0: "a"}`) folds to Dict(Float, _),
                // which codegen would emit as the uncompilable `HashMap<f64, _>`.
                // Reject the KEY only — float VALUES are fine (`HashMap<_, f64>`
                // compiles), so v_ty is left untouched.
                require_hashable(&k_ty, *span, "dict key")?;
                Ty::Dict(Box::new(k_ty), Box::new(v_ty))
            }
        }
        Expr::Ident(name, span) => {
            // Track variable usage for dead code detection
            if env.locals.contains_key(name.as_str()) {
                env.used_vars.insert(name.clone());
            }
            // Allow standard library modules (math, dataclasses, etc.) to be Ty::Unknown
            if matches!(name.as_str(), "math" | "dataclasses" | "sys" | "os" | "json" | "re" | "collections" | "itertools") {
                Ty::Unknown
            } else {
                env.lookup(name).ok_or_else(|| Error::Type {
                    span: *span,
                    msg: format!("undefined name `{}`", name),
                })?
            }
        }
        Expr::Call { callee, args, kwargs, span } => {
            // Generics v2 (generic CLASSES): an EXPLICIT type-argument constructor
            // `Box[int](5)` parses as a CALL whose callee is `Box[int]` — an
            // `Index` of the class name. pyrst infers a generic class's type args
            // from `__init__` (`Box(5)` -> `Box[int]`), and the `Box[int]` callee
            // would otherwise be (mis)read as a list-index expression that
            // type-checks but emits broken Rust. Reject it honestly here, pointing
            // at the supported inferred form. (A genuine index-then-call like
            // `ops["double"](7)` has a non-class base and is unaffected.)
            if let Expr::Index { obj, .. } = callee.as_ref() {
                if let Expr::Ident(cls, _) = obj.as_ref() {
                    if env.ctx.classes.contains_key(cls.as_str()) {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!(
                                "explicit type arguments on a constructor are not supported: \
                                 write `{}(...)` and let the type arguments be inferred from the \
                                 constructor arguments",
                                cls
                            ),
                        });
                    }
                }
            }
            // Check if this is a class constructor or function call.
            match callee.as_ref() {
                Expr::Ident(name, _) => {
                    if let Some(_class_def) = env.ctx.classes.get(name.as_str()) {
                        // Constructor call: check that kwarg field names are valid (including inherited fields).
                        let all_fields = env.ctx.get_all_fields(name.as_str());
                        for (kw, val) in kwargs {
                            if !all_fields.iter().any(|f| &f.name == kw) {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!("class `{}` has no field `{}`", name, kw),
                                });
                            }
                            check_expr(val, env)?;
                        }
                        // Generics v2: collect the positional argument types and,
                        // for a GENERIC class, infer its type arguments by unifying
                        // `__init__`'s scoped param types against them
                        // (`Box(5)` -> `Box[int]`). A conflicting binding
                        // (`Pair(1, 1)` against `Pair[A, B]` is fine; an
                        // inconsistent same-var binding is reported) surfaces as an
                        // honest error at `span`. A non-generic class takes the
                        // early return inside the helper and yields the legacy
                        // `Ty::Class(name, [])` — unchanged behaviour.
                        let mut arg_tys = Vec::with_capacity(args.len());
                        for a in args {
                            arg_tys.push(check_expr(a, env)?);
                        }
                        check_class_instantiation(name, &arg_tys, env.ctx, *span)?
                    } else if (name == "min" || name == "max") && args.len() == 1 {
                        // Single-iterable min/max: the result is the element type
                        // of the list/set argument. A `key=`/other kwarg may also
                        // be present (e.g. `min(words, key=len)`) — the lone
                        // positional arg is still the iterable. The 2-arg form
                        // `min(a, b)` falls through to the generic path below and
                        // stays Unknown (Rust's std::cmp::min already resolves it).
                        let arg_ty = check_expr(&args[0], env)?;
                        // Generics v1: `min`/`max` iterate the argument (and order
                        // its elements) — a bare type variable has neither
                        // IntoIterator nor Ord, so reject it honestly here.
                        reject_typevar_op(&arg_ty, "consume the contents of", *span)?;
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        match arg_ty {
                            // (LAZY-GEN V1-c) A generator argument's min/max is
                            // its element type, same as a list/set.
                            Ty::List(elem) | Ty::Set(elem) | Ty::Iterator(elem) => *elem,
                            _ => Ty::Unknown,
                        }
                    } else if name == "enumerate" && !args.is_empty() {
                        // enumerate(iterable[, start]) -> List(Tuple(Int, elem))
                        // Check all args/kwargs for their own errors first.
                        let arg0_ty = check_expr(&args[0], env)?;
                        // Generics v1: enumerate iterates its argument — a bare
                        // type variable has no IntoIterator bound.
                        reject_typevar_op(&arg0_ty, "consume the contents of", *span)?;
                        for a in &args[1..] {
                            check_expr(a, env)?;
                        }
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        let elem = match arg0_ty {
                            // (LAZY-GEN V1-c) A generator argument enumerates by
                            // its element type, same as a list/set.
                            Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => *inner,
                            Ty::Str => Ty::Str,
                            _ => Ty::Unknown,
                        };
                        if matches!(elem, Ty::Unknown) {
                            Ty::Unknown
                        } else {
                            Ty::List(Box::new(Ty::Tuple(vec![Ty::Int, elem])))
                        }
                    } else if name == "zip" {
                        // zip(a, b, ...) -> List(Tuple(elem_a, elem_b, ...))
                        // Check all args/kwargs for their own errors first.
                        let mut elem_tys: Vec<Ty> = Vec::new();
                        let mut any_unknown = false;
                        for a in args {
                            let ty = check_expr(a, env)?;
                            // Generics v1: zip iterates each argument — a bare type
                            // variable has no IntoIterator bound.
                            reject_typevar_op(&ty, "consume the contents of", *span)?;
                            match ty {
                                // (LAZY-GEN V1-c) `zip` accepts a mix of sources
                                // per argument — a generator arg contributes its
                                // element type, same as a list/set.
                                Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => elem_tys.push(*inner),
                                Ty::Str => elem_tys.push(Ty::Str),
                                _ => any_unknown = true,
                            }
                        }
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        if any_unknown || elem_tys.is_empty() {
                            Ty::Unknown
                        } else {
                            Ty::List(Box::new(Ty::Tuple(elem_tys)))
                        }
                    } else if name == "map" && args.len() == 2 {
                        // map(f, iterable) -> List(return type of f applied to the
                        // iterable's element type). Only a List iterable yields a
                        // concrete result: codegen's `.iter().cloned().map(..)`
                        // compiles for a Vec, but a String has no `.iter()` and
                        // map-over-set is unverified, so Set/Str/unknown stay
                        // permissive (Unknown), matching the filter arm below. The
                        // lambda body is still checked for its own errors, and we
                        // never narrow types_compatible.
                        let iter_ty = check_expr(&args[1], env)?;
                        let elem = match &iter_ty {
                            Ty::List(inner) => Some((**inner).clone()),
                            _ => None,
                        };
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        let body_ty = lambda_ret_with_elem(&args[0], elem.as_ref(), env)?;
                        match (&iter_ty, body_ty) {
                            (Ty::List(_), Some(t)) if !matches!(t, Ty::Unknown) => {
                                Ty::List(Box::new(t))
                            }
                            _ => Ty::Unknown,
                        }
                    } else if name == "filter" && args.len() == 2 {
                        // filter(pred, iterable) -> the iterable's list type
                        // unchanged (filter preserves elements). The predicate body
                        // is still checked (binding its first param to the element
                        // type) so a malformed predicate is caught; its return type
                        // is irrelevant to the result element type.
                        let iter_ty = check_expr(&args[1], env)?;
                        let elem = match &iter_ty {
                            Ty::List(inner) | Ty::Set(inner) => Some((**inner).clone()),
                            Ty::Str => Some(Ty::Str),
                            _ => None,
                        };
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        let _ = lambda_ret_with_elem(&args[0], elem.as_ref(), env)?;
                        match iter_ty {
                            Ty::List(_) => iter_ty,
                            _ => Ty::Unknown,
                        }
                    } else if let Some(sig) = env.ctx.funcs.get(name.as_str()) {
                        // Regular function call: check arity (positional only in v0).
                        let expected = sig.params.len();
                        let got = args.len() + kwargs.len();
                        // Variadic builtins: skip arity check.
                        let variadic = matches!(name.as_str(),
                            "print" | "range" | "len" | "str" | "int" | "float" | "bool" | "enumerate" | "zip"
                            | "abs" | "min" | "max" | "sorted" | "sum" | "input" | "list" | "dict" | "tuple" | "set"
                            | "getattr" | "setattr" | "hasattr" | "open");
                        // Count required parameters (those without defaults)
                        let required = sig.param_defaults.iter().take_while(|d| d.is_none()).count();
                        if !variadic && (got < required || got > expected) {
                            return Err(Error::Type {
                                span: *span,
                                msg: format!(
                                    "function `{}` takes {} argument(s), {} given",
                                    name, expected, got
                                ),
                            });
                        }
                        let sig_params = sig.params.clone();
                        let sig_by_ref = sig.param_by_ref.clone();
                        let sig_ret = sig.ret.clone();
                        // Generics v1: is this a parametric generic function? Its
                        // type-var-bearing params are validated by call-site
                        // UNIFICATION (below), not the concrete `types_compatible`
                        // check, so a `T` param accepts any argument type while a
                        // CONCRETE param of a generic function is still checked.
                        let is_generic = env.ctx.generic_funcs
                            .get(name.as_str())
                            .is_some_and(|tps| !tps.is_empty());
                        let mut arg_tys: Vec<Ty> = Vec::with_capacity(args.len());
                        for (i, a) in args.iter().enumerate() {
                            // EPIC-4 V2: an argument bound to a by-reference
                            // (`Mut[T]`) param must be a PLACE — an lvalue we can
                            // take `&mut` of (variable / field / index). A
                            // temporary (call/constructor/literal/binop result)
                            // has no caller-visible storage to borrow, so it is an
                            // honest typeck error here rather than a later rustc
                            // borrow failure. The arg's TYPE is still checked
                            // against the inner `T` by the compatibility check
                            // below (the param type was unwrapped from `Mut[T]`).
                            if sig_by_ref.get(i).copied().unwrap_or(false)
                                && !is_place_expr(a)
                            {
                                let pname = sig_params.get(i)
                                    .map(|(n, _)| n.as_str())
                                    .unwrap_or("<arg>");
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!(
                                        "by-reference parameter `{}` requires a variable, not a temporary",
                                        pname
                                    ),
                                });
                            }
                            let arg_ty = check_expr(a, env)?;
                            // A builtin that uses the SHAPE of its argument cannot
                            // accept a bare type variable from the `T: Clone` bound
                            // alone. Two families differ in v2:
                            //  - FORMAT (`print`/`str`/`repr`/`ascii`): generics v2
                            //    INFERS a `Display` bound on `T` (collected by
                            //    `infer_func_typevar_bounds`), so a bare `T` is now
                            //    LEGAL here — no rejection.
                            //  - SHAPE-CONSUMING (`len`/`sum`/`sorted`/`reversed`/
                            //    `any`/`all`/`list`/`tuple`/`set`/`dict`/
                            //    `enumerate`/`zip`) iterate/index/sum the argument
                            //    (IntoIterator / Add / etc.) — beyond v2, so a bare
                            //    `T` STAYS an honest rejection.
                            // (`first([...])` etc. are fine: their RESULT is
                            // concrete after unification; only a BARE `T` value
                            // reaches here as `Ty::TypeVar`.)
                            if matches!(name.as_str(),
                                "len" | "sum" | "sorted" | "reversed" | "any" | "all"
                                | "list" | "tuple" | "set" | "dict" | "enumerate" | "zip")
                            {
                                reject_typevar_op(&arg_ty, "consume the contents of", *span)?;
                            }
                            // Concrete-only positional arg-type check (skip variadic builtins).
                            // Only fires when BOTH param and arg types are concrete and
                            // incompatible. Int->Float is explicitly allowed (Python coercion).
                            // A param that IS (or contains) a type variable is skipped
                            // here — unification validates it structurally afterwards.
                            if !variadic {
                                if let Some((_, param_ty)) = sig_params.get(i) {
                                    let int_to_float =
                                        matches!(arg_ty, Ty::Int) && matches!(param_ty, Ty::Float);
                                    if !int_to_float
                                        && !matches!(arg_ty, Ty::Unknown)
                                        && !matches!(param_ty, Ty::Unknown)
                                        && !contains_typevar(param_ty)
                                        && !types_compatible(&arg_ty, param_ty, env.ctx)
                                    {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "argument {} to `{}`: expected {}, found {}",
                                                i + 1, name, param_ty, arg_ty
                                            ),
                                        });
                                    }
                                }
                            }
                            arg_tys.push(arg_ty);
                        }
                        if is_generic {
                            // Unify the declared (type-var-bearing) params against
                            // the actual argument types: surfaces a conflicting
                            // binding ("conflicting types for type parameter `T`")
                            // or an uninferable type parameter, and yields the
                            // SUBSTITUTED concrete return type for this call.
                            infer_generic_call_result(name, &arg_tys, env.ctx, *span)?
                                .unwrap_or(sig_ret)
                        } else {
                            sig_ret
                        }
                    } else if name == "super" && args.is_empty() && kwargs.is_empty() {
                        // super() returns Unknown type — the codegen handles super().method() specially
                        Ty::Unknown
                    } else if let Some(local_ty) = env.lookup(name) {
                        // Calling a function-VALUED local/param by bare name
                        // (`f(x)` where `f: Callable[[int], int]`). Check the
                        // arguments first (for their own errors), then — if the
                        // value's type is a `Ty::Func` — enforce arity and per-arg
                        // compatibility and yield its return type. A non-Func
                        // callable value (untyped lambda binding, Unknown) stays
                        // permissive (Unknown), exactly as before.
                        let arg_tys = args.iter()
                            .map(|a| check_expr(a, env))
                            .collect::<Result<Vec<_>>>()?;
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        if let Ty::Func(param_tys, ret) = &local_ty {
                            if args.len() != param_tys.len() {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!(
                                        "function value `{}` takes {} argument(s), {} given",
                                        name, param_tys.len(), args.len()
                                    ),
                                });
                            }
                            for (i, (arg_ty, param_ty)) in arg_tys.iter().zip(param_tys.iter()).enumerate() {
                                let int_to_float =
                                    matches!(arg_ty, Ty::Int) && matches!(param_ty, Ty::Float);
                                if !int_to_float
                                    && !matches!(arg_ty, Ty::Unknown)
                                    && !matches!(param_ty, Ty::Unknown)
                                    && !types_compatible(arg_ty, param_ty, env.ctx)
                                {
                                    return Err(Error::Type {
                                        span: *span,
                                        msg: format!(
                                            "argument {} to `{}`: expected {}, found {}",
                                            i + 1, name, param_ty, arg_ty
                                        ),
                                    });
                                }
                            }
                            (**ret).clone()
                        } else if is_noncallable_ty(&local_ty) {
                            // (honest errors) Calling a value of a KNOWN
                            // non-callable type (`x: int = 5; x(3)`) is a type
                            // error, not a deferred rustc E0618. `Unknown` and
                            // `Class` stay permissive (escape hatch: a class
                            // instance may be callable in a later increment).
                            return Err(Error::Type {
                                span: *span,
                                msg: format!("`{}` of type {} is not callable", name, local_ty),
                            });
                        } else {
                            Ty::Unknown
                        }
                    } else {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("undefined function `{}`", name),
                        });
                    }
                }
                // Method call: e.g., p.magnitude() — callee is Attr
                _ => {
                    // Qualified module call `X.f(args)` for a REAL imported module
                    // (card 81db88e0). When the callee is `Attr{Ident(X), f}` and X
                    // is a tracked module name, this is NOT a method call: it is a
                    // call to module X's function `f`, whose signature lives FLAT in
                    // `ctx.funcs` under the bare name. We type it exactly like a flat
                    // call to `f` (arity + per-arg compatibility + return). `math`
                    // is now a real embedded module, so `math.sqrt(x)` resolves
                    // through here like any other module's function. A qualified
                    // call to a name the module does NOT define is an honest error
                    // here (see the unknown-qualified-call rejection below), not a
                    // silently-Unknown call.
                    if let Expr::Attr { obj, name, span: attr_span } = callee.as_ref() {
                        if let Expr::Ident(modname, _) = obj.as_ref() {
                            if let Some(mod_fns) = env.ctx.module_funcs.get(modname) {
                                if mod_fns.iter().any(|n| n == name) {
                                    // f is defined by module X — resolve its flat sig.
                                    let sig = env.ctx.funcs.get(name).cloned().ok_or_else(|| Error::Type {
                                        span: *attr_span,
                                        msg: format!("module `{}` function `{}` has no signature", modname, name),
                                    })?;
                                    // Arity (positional only; module @extern fns are
                                    // not variadic and take no kwargs).
                                    let expected = sig.params.len();
                                    let got = args.len() + kwargs.len();
                                    let required = sig.param_defaults.iter().take_while(|d| d.is_none()).count();
                                    if got < required || got > expected {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "function `{}.{}` takes {} argument(s), {} given",
                                                modname, name, expected, got
                                            ),
                                        });
                                    }
                                    // Per-arg type-check + result resolution via the
                                    // SHARED helper, so a qualified call to a GENERIC
                                    // imported function (`heapq.heappush(h, 5)`) runs
                                    // the SAME call-site unification as the flat form
                                    // (`heappush(h, 5)`): a `list[T]` param accepts a
                                    // `list[int]` arg (T=int), the return type is
                                    // substituted, and conflicting/uninferable type
                                    // parameters are honest errors here too. A
                                    // non-generic qualified call (`string.capwords`)
                                    // is unchanged — concrete params are still checked
                                    // and the declared return is returned.
                                    let arg_tys = args.iter()
                                        .map(|a| check_expr(a, env))
                                        .collect::<Result<Vec<_>>>()?;
                                    let diag_label = format!("{}.{}", modname, name);
                                    let result = check_call_arg_types_and_result(
                                        name, &diag_label, &sig, &arg_tys, env.ctx, *span,
                                    )?;
                                    for (_, v) in kwargs { check_expr(v, env)?; }
                                    return Ok(result);
                                } else {
                                    // X IS a tracked module but defines no such `f`.
                                    return Err(Error::Type {
                                        span: *attr_span,
                                        msg: format!("module `{}` has no function `{}`", modname, name),
                                    });
                                }
                            }
                        }
                    }
                    if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                        let obj_ty = check_expr(obj, env)?;
                        // Generics v1: calling a method on a bare type variable
                        // (`t.foo()` where `t: T`) needs a trait bound and is
                        // rejected — `T` is opaque, with no known methods.
                        reject_typevar_op(&obj_ty, "call a method on", *span)?;
                        if let Ty::Class(class_name, _) = &obj_ty {
                            let key = format!("{}.{}", class_name, name);
                            if let Some(sig) = env.ctx.funcs.get(&key).cloned() {
                                // (EPIC-4 V2-c) Enforce the by-reference (`Mut[T]`)
                                // place-requirement at METHOD call sites too (it was
                                // already enforced for free functions in V2-ab). An
                                // arg bound to a by-ref method param must be a PLACE
                                // (Ident/Attr/Index) — a temporary has no
                                // caller-visible storage to borrow `&mut`. We look
                                // up the by-ref flags via get_method, whose vectors
                                // are self-EXCLUSIVE and index-aligned to `args`
                                // (mirrors the resolver alignment fixed in STEP 0).
                                let method_sig = env.ctx.get_method(class_name, name);
                                if let Some(msig) = &method_sig {
                                    for (i, a) in args.iter().enumerate() {
                                        if msig.param_by_ref.get(i).copied().unwrap_or(false)
                                            && !is_place_expr(a)
                                        {
                                            let pname = msig.params.get(i)
                                                .map(|(n, _)| n.as_str())
                                                .unwrap_or("<arg>");
                                            return Err(Error::Type {
                                                span: *span,
                                                msg: format!(
                                                    "by-reference parameter `{}` requires a variable, not a temporary",
                                                    pname
                                                ),
                                            });
                                        }
                                    }
                                }
                                for a in args { check_expr(a, env)?; }
                                // Generics v2: the registered sig's return may
                                // contain the class's type vars (`get(self) -> T`).
                                // Substitute the RECEIVER instance's type args
                                // (`b: Box[int]` -> `{T -> int}`) so the call types
                                // concretely (`b.get(): int`). A non-generic / arg-
                                // less receiver yields an empty subst and returns
                                // the ret unchanged.
                                return Ok(subst_class_member(&sig.ret, &obj_ty, env.ctx));
                            }
                        }
                        // (a) Builtin method existence — only on concrete Str/List/Set/Dict.
                        // Skipped for Unknown (unprovable) and Class (handled above).
                        if let Some((type_name, table)) = builtin_method_table(&obj_ty) {
                            if !table.contains(&name.as_str()) {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!("type `{}` has no method `{}`", type_name, name),
                                });
                            }
                            // (b) Detect in-place mutating method calls on a by-value param.
                            // e.g. `visited.add(node)` where `visited` is a Set parameter,
                            // OR `param.field.append(x)` / `param[0].add(x)` — a mutator on
                            // any PLACE rooted at the param (the mutation is lost on the
                            // caller's clone either way). EPIC-4 V2-d closes the former
                            // nested-mutation gap: we now root the receiver via `root_ident`
                            // (like the AttrAssign / IndexAssign backstops already do)
                            // instead of requiring the receiver to be the bare param ident.
                            // `obj_ty` is the RECEIVER's type (the collection being mutated),
                            // which is always owned inside this builtin-method-table arm, so
                            // the `is_owned(&obj_ty)` guard still holds for the field/index
                            // case. The suppressions are preserved verbatim: self-exclusion,
                            // reassigned, returned, and — critically — by_ref (`Mut[T]`)
                            // params, whose nested mutation IS caller-visible and must NOT
                            // fire.
                            if MUTATING_METHODS.contains(&name.as_str()) {
                                if let Some(param_name) = root_ident(obj) {
                                    if param_name != "self"
                                        && env.params.contains(param_name)
                                        && !env.reassigned_params.contains(param_name)
                                        && !env.returned_params.contains(param_name)
                                        && !env.by_ref_params.contains(param_name)
                                        && is_owned(&obj_ty)
                                    {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: by_value_mutation_error(param_name),
                                        });
                                    }
                                }
                            }
                            // (c) Element-type argument check for set mutators only.
                            if let Some(elem_ty) = elem_arg_check_ty(&obj_ty, name) {
                                if let Some(arg0) = args.first() {
                                    let arg_ty = check_expr(arg0, env)?;
                                    let int_to_float =
                                        matches!(arg_ty, Ty::Int) && matches!(elem_ty, Ty::Float);
                                    if !int_to_float
                                        && !matches!(arg_ty, Ty::Unknown)
                                        && !matches!(elem_ty, Ty::Unknown)
                                        && !types_compatible(&arg_ty, &elem_ty, env.ctx)
                                    {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "argument to `{}.{}`: expected element type {}, found {}",
                                                type_name, name, elem_ty, arg_ty
                                            ),
                                        });
                                    }
                                }
                            }
                            for a in args { check_expr(a, env)?; }
                            // dict.get is arg-count-aware: get(k) -> Optional[V],
                            // get(k, default) -> V. Route through the shared helper
                            // so the checker and the inference oracle agree; fall
                            // back to builtin_method_ret for every other method.
                            if let Some(t) = dict_get_ret(&obj_ty, name.as_str(), args.len()) {
                                return Ok(t);
                            }
                            return Ok(builtin_method_ret(&obj_ty, name.as_str()));
                        }
                    }
                    // Calling a function VALUE whose callee is an arbitrary
                    // expression (not a bare name or method). Two cases:
                    //  - An inline lambda `(lambda x: body)(args)`: the call's
                    //    value type is the lambda BODY type (computed directly so
                    //    it is unaffected by the Lambda arm now yielding Ty::Func).
                    //  - Any other func-valued callee (`ops["double"](7)`,
                    //    `(make_adder(5))(10)`): the result is the function value's
                    //    return type, surfaced from its `Ty::Func`.
                    let result = if let Expr::Lambda { params, body, .. } = callee.as_ref() {
                        lambda_body_ty(params, body, env)?
                    } else {
                        let callee_ty = check_expr(callee, env)?;
                        // (honest errors) Calling the result of an expression whose
                        // type is a KNOWN non-callable (`xs[0](3)` where `xs:
                        // list[int]`) is a type error, not a deferred rustc E0618.
                        // `Unknown`/`Class` stay permissive. CRUCIAL EXCLUSION: an
                        // `Expr::Attr` callee here is an UNRESOLVED method call
                        // (`m.kind()`, `self.bump()`) that the method-dispatch block
                        // above did not match and let fall through — `check_expr`
                        // returns the method's RETURN type, not the callee's own
                        // type, so the non-callable test would misfire on a method
                        // that returns str/None/etc. Method calls are never the
                        // value-call form this gate targets, so skip them.
                        let is_method_callee = matches!(callee.as_ref(), Expr::Attr { .. });
                        match callee_ty {
                            Ty::Func(_, ret) => *ret,
                            ref t if !is_method_callee && is_noncallable_ty(t) => {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!("value of type {} is not callable", callee_ty),
                                });
                            }
                            _ => Ty::Unknown,
                        }
                    };
                    for a in args { check_expr(a, env)?; }
                    result
                }
            }
        }
        Expr::Attr { obj, name, span } => {
            // Qualified MODULE CONSTANT `X.CONST` for a REAL imported module:
            // when X is a tracked module and CONST is one of its module-level
            // constants, the access type-checks as the const's declared type.
            // GENERALIZES the former hardcoded `math.pi` handling (where `math`
            // was a Ty::Unknown placeholder and `math.pi` silently stayed
            // Unknown); `math` is now a real embedded module whose consts are
            // tracked in `module_consts`.
            if let Expr::Ident(modname, _) = obj.as_ref() {
                if let Some(consts) = env.ctx.module_consts.get(modname) {
                    if let Some((_, ty)) = consts.iter().find(|(c, _)| c == name) {
                        return Ok(ty.clone());
                    }
                }
                // (Honest-errors) `X.attr` (non-call) where X is a KNOWN imported
                // module (it has tracked functions or constants) but `attr` is
                // neither a constant nor a function of X is an UNKNOWN ATTRIBUTE.
                // Reject it honestly at `check` rather than letting it fall to
                // Ty::Unknown and miscompile at `build` (e.g. `math.inf` — inf/nan
                // are not pyrst constants — would emit a bare `math` and fail rustc
                // E0425). Mirrors the unknown-qualified-FUNCTION rejection on the
                // call path. A known constant returned above; a function name
                // (used as a value) is a separate, deferred feature and is left to
                // fall through unchanged.
                let is_known_module = env.ctx.module_funcs.contains_key(modname)
                    || env.ctx.module_consts.contains_key(modname);
                let is_module_func = env
                    .ctx
                    .module_funcs
                    .get(modname)
                    .is_some_and(|fns| fns.iter().any(|f| f == name));
                if is_known_module && !is_module_func {
                    return Err(Error::Type {
                        span: *span,
                        msg: format!("module `{}` has no attribute `{}`", modname, name),
                    });
                }
            }
            let obj_ty = check_expr(obj, env)?;
            // Generics v1: accessing an attribute of a bare type variable
            // (`t.x` where `t: T`) is rejected — `T` is opaque, with no known
            // fields/attributes (E0609 otherwise). A method CALL on a type var is
            // rejected separately in the Call arm.
            reject_typevar_op(&obj_ty, "access an attribute of", *span)?;
            if let Ty::Class(class_name, _) = &obj_ty {
                if let Some(class_def) = env.ctx.classes.get(class_name.as_str()) {
                    // Check field access (including inherited fields).
                    let all_fields = env.ctx.get_all_fields(class_name.as_str());
                    if let Some(field) = all_fields.iter().find(|f| &f.name == name) {
                        // Generics v2: lower the field annotation with the class's
                        // type params in scope (`value: T` -> `Ty::TypeVar(T)`),
                        // then substitute the RECEIVER instance's type args
                        // (`b: Box[int]` -> `{T -> int}`) so `b.value: int`. A
                        // non-generic class scopes/substitutes with an empty set,
                        // identical to the legacy `from_type_expr` result.
                        let field_ty = Ty::from_type_expr_scoped(&field.ty, *span, &class_def.type_params)?;
                        return Ok(subst_class_member(&field_ty, &obj_ty, env.ctx));
                    }
                    // Check method access (including inherited methods). A bare
                    // method reference's return type substitutes the receiver's
                    // type args too (parity with the method-CALL arm).
                    if let Some(method) = env.ctx.get_method(class_name.as_str(), name) {
                        return Ok(subst_class_member(&method.ret, &obj_ty, env.ctx));
                    }
                    return Err(Error::Type {
                        span: *span,
                        msg: format!("class `{}` has no attribute `{}`", class_name, name),
                    });
                }
            }
            Ty::Unknown
        }
        Expr::Index { obj, idx, span } => {
            let obj_ty = check_expr(obj, env)?;
            check_expr(idx, env)?;
            // Generics v1: a bare type variable is OPAQUE — it is not known to be
            // a container, so indexing it (`t[i]`) needs a bound and is rejected.
            // (Indexing a `list[T]`/`dict[K, V]` whose ELEMENT is a type var is
            // fine — that yields the element type below.)
            reject_typevar_op(&obj_ty, "index", *span)?;
            match obj_ty {
                Ty::List(inner) => *inner,
                Ty::Dict(_, v) => *v,
                Ty::Str => Ty::Str,
                _ => Ty::Unknown,
            }
        }
        Expr::Slice { obj, start, stop, step, span } => {
            let obj_ty = check_expr(obj, env)?;
            // Generics v1: a bare type variable is OPAQUE — slicing it (`t[a:b]`)
            // needs a slice/Index bound and is rejected (mirrors the Index arm).
            reject_typevar_op(&obj_ty, "slice", *span)?;
            // Validate slice indices are integers
            for e in &[start.as_ref(), stop.as_ref(), step.as_ref()] {
                if let Some(e) = e {
                    let ty = check_expr(e, env)?;
                    if !matches!(ty, Ty::Int | Ty::Unknown) {
                        return Err(Error::Type {
                            span: e.span(),
                            msg: "slice indices must be integers".into(),
                        });
                    }
                }
            }
            // Slicing a list/string returns the same type
            match obj_ty {
                Ty::List(inner) => Ty::List(inner),
                Ty::Str => Ty::Str,
                _ => Ty::Unknown,
            }
        }
        Expr::BinOp { op, lhs, rhs, span } => {
            let l = check_expr(lhs, env)?;
            let r = check_expr(rhs, env)?;
            // Generics v2: a SUPPORTED binary operator on two values of the SAME
            // type variable (`T op T`) is now LEGAL — codegen emits the inferred
            // trait bound (`PartialOrd` / `PartialEq` / `Add<Output=T>` / ...) in
            // the generic clause. An UNSUPPORTED op on a bare `T` (membership,
            // boolean, bitwise, `**`, `//`), or a MIXED `T op concrete` /
            // `T op differentU`, stays an honest rejection. The `op_desc`
            // distinguishes comparison from arithmetic so the message reads
            // naturally.
            let op_desc = match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => "compare",
                BinOp::In | BinOp::NotIn => "test membership of",
                _ => "apply an operator to",
            };
            // The single supported shape is `T op T` of the SAME variable with a
            // mapped bound. Recognise it first; anything else with a TypeVar
            // operand falls through to the v1 rejection.
            let same_typevar = matches!((&l, &r), (Ty::TypeVar(a), Ty::TypeVar(b)) if a == b);
            let supported_typevar_op = same_typevar && binop_typevar_bound(*op).is_some();
            // Generics v2: membership where the CONTAINER (rhs) is a known
            // `dict`/`set`/`list` and the ELEMENT/key (lhs) is a TypeVar is a
            // VALID, bound-inferable op — `k in d` infers `K: Hash + Eq`
            // (dict/set) or `K: PartialEq` (list), mirroring `infer_bounds_expr`.
            // Only `x in t` where `t` itself is a BARE TypeVar (an unknown
            // container) stays rejected by the bare-T sweep below.
            let container_membership = matches!(op, BinOp::In | BinOp::NotIn)
                && matches!(r, Ty::Dict(..) | Ty::Set(_) | Ty::List(_));
            if !supported_typevar_op && !container_membership {
                reject_typevar_op(&l, op_desc, *span)?;
                reject_typevar_op(&r, op_desc, *span)?;
            }
            // (EPIC-5) Reject using a raw `Optional[T]` operand without narrowing.
            // An Option only supports identity/equality testing against `None`
            // (`is` / `is not` / `==` / `!=`); any other operator (arithmetic,
            // ordering, membership, boolean) on an un-narrowed Optional is an
            // honest error — the value must be narrowed via `is None` /
            // `is not None` first (see PYTHON_COMPATIBILITY.md, Optional section).
            // Without this, `x + 1` on an `Optional[int]` would infer `Unknown`
            // and silently slip through, then miscompile.
            let nullary_ok = matches!(op, BinOp::Is | BinOp::IsNot | BinOp::Eq | BinOp::Ne);
            if !nullary_ok && (matches!(l, Ty::Option(_)) || matches!(r, Ty::Option(_))) {
                return Err(Error::Type {
                    span: *span,
                    msg: format!(
                        "operator on an Optional value requires narrowing first: \
                         use `if x is not None:` to obtain the inner value before applying `{:?}`",
                        op
                    ),
                });
            }
            // Generics v2: type the result of a SUPPORTED `T op T`. Comparison /
            // equality yield `bool`; the supported arithmetic ops (`+ - *`) yield
            // `T` (the same-type rule, matching the emitted `Add`/`Sub`/`Mul<Output
            // = T>` bound). This explicit redirect fires ONLY for the recognised
            // same-`T` shape; concrete operands keep the Python rules below.
            if supported_typevar_op {
                return Ok(match op {
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Ty::Bool,
                    // Add/Sub/Mul on `T op T` -> `T`.
                    _ => l,
                });
            }
            match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le
                | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or
                | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => Ty::Bool,
                BinOp::Pow | BinOp::Div => Ty::Float,  // Division always returns float in Python
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::LShift | BinOp::RShift => Ty::Int,
                _ => {
                    // Arithmetic: apply numeric type promotion rules
                    match (&l, &r) {
                        // Operator overloading: a class lhs dispatches to the
                        // declared return type of its dunder (__add__/__sub__/__mul__).
                        (Ty::Class(cls, _), _) => {
                            let dunder = match op {
                                BinOp::Add => Some("__add__"),
                                BinOp::Sub => Some("__sub__"),
                                BinOp::Mul => Some("__mul__"),
                                _ => None,
                            };
                            dunder.and_then(|d| env.ctx.get_method(cls, d))
                                .map(|s| s.ret.clone())
                                .unwrap_or_else(|| l.clone())
                        }
                        // Same type: return that type
                        (a, b) if a == b => l,
                        // Mixed numeric types: promote to float
                        (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => Ty::Float,
                        // String + String = String (for concatenation)
                        (Ty::Str, Ty::Str) => Ty::Str,
                        // List + List = List (for concatenation)
                        (Ty::List(inner_l), Ty::List(inner_r)) if inner_l == inner_r => Ty::List(inner_l.clone()),
                        // Otherwise unknown
                        _ => Ty::Unknown,
                    }
                }
            }
        }
        Expr::UnOp { op, expr, span } => {
            let t = check_expr(expr, env)?;
            // Generics v1: a unary operator on a bare type variable is rejected
            // (needs `Neg`/`Not` bounds, out of v1 scope).
            reject_typevar_op(&t, "apply a unary operator to", *span)?;
            match op {
                UnOp::Not => Ty::Bool,
                UnOp::Neg => t,
                UnOp::BitNot => Ty::Int,
            }
        }
        Expr::Lambda { params, body, .. } => {
            // The lambda's value type is its first-class function type
            // `Callable[[unknown, ...], body_ty]`. Checking the body in a child
            // env (params bound to Unknown) both validates the body for its own
            // errors and yields the return type. Returning a `Ty::Func` (rather
            // than the bare body type) is what lets a lambda flow into a declared
            // `Callable` slot — assignment, argument, return, and dict/list value.
            // The two inline-call paths (the Ident-callee Lambda branch and the
            // `_`-callee branch in the Call arm) compute the body type DIRECTLY,
            // so they are unaffected by this change.
            let body_ty = lambda_body_ty(params, body, env)?;
            Ty::Func(vec![Ty::Unknown; params.len()], Box::new(body_ty))
        }
    })
}

/// Type-check a lambda body in a child environment with each parameter bound to
/// `Unknown` (pyrst lambda params are unannotated), returning the body's type.
/// Shared by the `Expr::Lambda` value arm (which wraps it in `Ty::Func`) and the
/// inline-invocation call paths (which surface the body type as the call result).
pub(crate) fn lambda_body_ty(
    params: &[(String, TypeExpr)],
    body: &Expr,
    env: &mut FuncEnv,
) -> Result<Ty> {
    let mut lambda_env = FuncEnv {
        ctx: env.ctx,
        locals: env.locals.clone(),
        ret_ty: Ty::Unknown,
        used_vars: env.used_vars.clone(),
        params: std::collections::HashSet::new(),
        reassigned_params: std::collections::HashSet::new(),
        returned_params: std::collections::HashSet::new(),
        by_ref_params: std::collections::HashSet::new(),
        // A lambda body is a single expression — never a generator.
        is_generator: false,
        // A lambda's params are its own; enclosing type variables don't apply.
        type_params: std::collections::HashSet::new(),
    };
    for (param_name, param_ty) in params {
        let ty = lambda_param_ty(param_ty);
        lambda_env.locals.insert(param_name.clone(), ty);
    }
    check_expr(body, &mut lambda_env)
}

// =============================================================================
// UNIT TESTS
