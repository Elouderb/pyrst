use super::*;
use std::collections::HashSet;

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
/// (card e131f8b0) Walk a module and collect every class NAME used as a `dict`
/// KEY or `set` ELEMENT (recursing through list/tuple/Optional/Callable wrappers
/// and into function/method bodies for local annotations). The result seeds
/// `TyCtx::hash_key_classes`, which drives both the codegen `Eq/Hash/Ord` derives
/// and the check-time hashability validation. Candidates are bare `Named` type
/// names in a key/element position that are not primitives — the checker resolves
/// each against `ctx.classes` and validates its eligibility.
pub(crate) fn collect_hash_key_classes(m: &Module, out: &mut std::collections::HashSet<String>) {
    for s in &m.stmts {
        collect_stmt_key_classes(s, out);
    }
}

fn collect_stmt_key_classes(s: &Stmt, out: &mut std::collections::HashSet<String>) {
    match s {
        Stmt::Class(c) => {
            for f in &c.fields {
                collect_te_key_classes(&f.ty, out);
            }
            for meth in &c.methods {
                collect_fn_key_classes(meth, out);
            }
        }
        Stmt::Func(f) => collect_fn_key_classes(f, out),
        Stmt::Assign { ty: Some(te), .. } => collect_te_key_classes(te, out),
        Stmt::If { then, elifs, else_, .. } => {
            then.iter().for_each(|s| collect_stmt_key_classes(s, out));
            for (_, b) in elifs {
                b.iter().for_each(|s| collect_stmt_key_classes(s, out));
            }
            if let Some(b) = else_ {
                b.iter().for_each(|s| collect_stmt_key_classes(s, out));
            }
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::With { body, .. } => {
            body.iter().for_each(|s| collect_stmt_key_classes(s, out));
        }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            body.iter().for_each(|s| collect_stmt_key_classes(s, out));
            for h in handlers {
                h.body.iter().for_each(|s| collect_stmt_key_classes(s, out));
            }
            if let Some(b) = else_ {
                b.iter().for_each(|s| collect_stmt_key_classes(s, out));
            }
            if let Some(b) = finally_ {
                b.iter().for_each(|s| collect_stmt_key_classes(s, out));
            }
        }
        Stmt::Match { arms, .. } => {
            for a in arms {
                a.body.iter().for_each(|s| collect_stmt_key_classes(s, out));
            }
        }
        _ => {}
    }
}

fn collect_fn_key_classes(f: &Func, out: &mut std::collections::HashSet<String>) {
    for p in &f.params {
        collect_te_key_classes(&p.ty, out);
    }
    collect_te_key_classes(&f.ret, out);
    for s in &f.body {
        collect_stmt_key_classes(s, out);
    }
}

fn collect_te_key_classes(te: &TypeExpr, out: &mut std::collections::HashSet<String>) {
    if let TypeExpr::Generic(n, args) = te {
        match (n.as_str(), args.as_slice()) {
            ("dict", [k, _v]) => add_if_class_name(k, out),
            ("set", [e]) => add_if_class_name(e, out),
            _ => {}
        }
        for a in args {
            collect_te_key_classes(a, out);
        }
    } else if let TypeExpr::Tuple(ts) = te {
        ts.iter().for_each(|t| collect_te_key_classes(t, out));
    } else if let TypeExpr::Func(args, ret) = te {
        args.iter().for_each(|t| collect_te_key_classes(t, out));
        collect_te_key_classes(ret, out);
    }
}

fn add_if_class_name(te: &TypeExpr, out: &mut std::collections::HashSet<String>) {
    if let TypeExpr::Named(n) = te {
        // Primitives are valid hashable keys directly (str/int/bool); float is
        // rejected separately by `require_hashable`. Any other bare name is a
        // candidate user class to validate + derive Eq/Hash/Ord for.
        if !matches!(n.as_str(), "int" | "str" | "bool" | "float") {
            out.insert(n.clone());
        }
    }
}

/// (enabler-fix-2 #1a/#1c) FINALIZE `hash_key_classes` over the WHOLE program,
/// after every class is registered and every module AST is available. The
/// per-module annotation scan (`collect_hash_key_classes`) records only classes
/// named DIRECTLY in a `dict`/`set` annotation; two closures it misses caused
/// codegen to emit a struct deriving `Eq/Hash/Ord` whose FIELD did not, or to
/// skip the derive entirely — both leaking rustc E0277/E0599:
///
///   (1a) TRANSITIVE — a hash-key class with a user-class FIELD (directly, or
///        inside a tuple) needs that nested class to derive too. Close the set
///        under "Named/Tuple field class of a member" to a fixed point. Only
///        Named/Tuple fields propagate: a `list`/`dict`/`set`/`Optional`/`Callable`
///        field makes the OWNER ineligible (no derive, so no field requirement).
///
///   (1c) ANNOTATION-LESS LITERAL — a `dict`/`set` literal (or comprehension)
///        keyed by class VALUES with no annotation never reached the annotation
///        scan. Add a key/element that is a CONSTRUCTOR CALL `C(..)` for a known
///        class `C` (the common `{Node(1): ...}` form). A variable/opaque key
///        still needs an annotation (no per-scope inference here — documented in
///        PYTHON_COMPATIBILITY.md). Codegen then adds the derive for the eligible
///        case; `check_class_prelude` still validates `class_hash_eligible` for
///        every member, so an INELIGIBLE class added here stays an honest error.
pub fn finalize_hash_key_classes(modules: &[(Module, String)], ctx: &mut TyCtx) {
    // (1c) constructor-call keys/elements of dict/set literals + comprehensions.
    let classes: std::collections::HashSet<String> = ctx.classes.keys().cloned().collect();
    let mut found = std::collections::HashSet::new();
    for (m, _) in modules {
        for s in &m.stmts {
            hk_scan_stmt(s, &classes, &mut found);
        }
    }
    ctx.hash_key_classes.extend(found);

    // (1a) transitive closure over user-class Named/Tuple field types. Monotone
    // fixed point (a finite class set, insert-only), so it terminates.
    loop {
        let current: Vec<String> = ctx.hash_key_classes.iter().cloned().collect();
        let mut to_add: Vec<String> = Vec::new();
        for cname in &current {
            for f in ctx.get_all_fields(cname) {
                hk_field_class_names(&f.ty, &mut to_add);
            }
        }
        let mut added = false;
        for n in to_add {
            if ctx.classes.contains_key(&n) && ctx.hash_key_classes.insert(n) {
                added = true;
            }
        }
        if !added {
            break;
        }
    }
}

/// Push every class name reachable through a hash-key field TYPE — a bare `Named`
/// or a `Tuple` of such. `list`/`dict`/`set`/`Optional`/`Callable` (`Generic`/
/// `Func`) are intentionally NOT traversed: such a field makes its owner
/// ineligible (`field_hashable` rejects it), so the owner never derives and its
/// element class carries no derive requirement from this position.
fn hk_field_class_names(te: &TypeExpr, out: &mut Vec<String>) {
    match te {
        TypeExpr::Named(n) => out.push(n.clone()),
        TypeExpr::Tuple(ts) => ts.iter().for_each(|t| hk_field_class_names(t, out)),
        _ => {}
    }
}

/// Add class `C` when `key` is a constructor call `C(..)` for a known class.
fn hk_add_ctor_key(key: &Expr, classes: &std::collections::HashSet<String>, out: &mut std::collections::HashSet<String>) {
    if let Expr::Call { callee, .. } = key {
        if let Expr::Ident(c, _) = callee.as_ref() {
            if classes.contains(c) {
                out.insert(c.clone());
            }
        }
    }
}

fn hk_scan_stmt(s: &Stmt, classes: &std::collections::HashSet<String>, out: &mut std::collections::HashSet<String>) {
    match s {
        Stmt::Class(c) => {
            for f in &c.fields {
                if let Some(d) = &f.default { hk_scan_expr(d, classes, out); }
            }
            for meth in &c.methods {
                for st in &meth.body { hk_scan_stmt(st, classes, out); }
            }
        }
        Stmt::Func(f) => { for st in &f.body { hk_scan_stmt(st, classes, out); } }
        Stmt::Assign { value, .. } | Stmt::AugAssign { value, .. }
        | Stmt::Unpack { value, .. } | Stmt::Expr(value)
        | Stmt::Return(Some(value), _) | Stmt::Yield(value, _)
        | Stmt::Del { target: value, .. } => hk_scan_expr(value, classes, out),
        Stmt::AttrAssign { obj, value, .. } => { hk_scan_expr(obj, classes, out); hk_scan_expr(value, classes, out); }
        Stmt::IndexAssign { obj, idx, value, .. } => {
            hk_scan_expr(obj, classes, out); hk_scan_expr(idx, classes, out); hk_scan_expr(value, classes, out);
        }
        Stmt::If { cond, then, elifs, else_, .. } => {
            hk_scan_expr(cond, classes, out);
            then.iter().for_each(|s| hk_scan_stmt(s, classes, out));
            for (c, b) in elifs {
                hk_scan_expr(c, classes, out);
                b.iter().for_each(|s| hk_scan_stmt(s, classes, out));
            }
            if let Some(b) = else_ { b.iter().for_each(|s| hk_scan_stmt(s, classes, out)); }
        }
        Stmt::While { cond, body, .. } => {
            hk_scan_expr(cond, classes, out);
            body.iter().for_each(|s| hk_scan_stmt(s, classes, out));
        }
        Stmt::For { iter, body, .. } => {
            hk_scan_expr(iter, classes, out);
            body.iter().for_each(|s| hk_scan_stmt(s, classes, out));
        }
        Stmt::With { ctx_expr, body, .. } => {
            hk_scan_expr(ctx_expr, classes, out);
            body.iter().for_each(|s| hk_scan_stmt(s, classes, out));
        }
        Stmt::Assert { cond, msg, .. } => {
            hk_scan_expr(cond, classes, out);
            if let Some(msg) = msg { hk_scan_expr(msg, classes, out); }
        }
        Stmt::Raise { exc, .. } => { if let Some(e) = exc { hk_scan_expr(e, classes, out); } }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            body.iter().for_each(|s| hk_scan_stmt(s, classes, out));
            for h in handlers { h.body.iter().for_each(|s| hk_scan_stmt(s, classes, out)); }
            if let Some(b) = else_ { b.iter().for_each(|s| hk_scan_stmt(s, classes, out)); }
            if let Some(b) = finally_ { b.iter().for_each(|s| hk_scan_stmt(s, classes, out)); }
        }
        Stmt::Match { subject, arms, .. } => {
            hk_scan_expr(subject, classes, out);
            for a in arms { a.body.iter().for_each(|s| hk_scan_stmt(s, classes, out)); }
        }
        _ => {}
    }
}

fn hk_scan_expr(e: &Expr, classes: &std::collections::HashSet<String>, out: &mut std::collections::HashSet<String>) {
    match e {
        Expr::Dict(kvs, _) => {
            for (k, v) in kvs {
                hk_add_ctor_key(k, classes, out);
                hk_scan_expr(k, classes, out);
                hk_scan_expr(v, classes, out);
            }
        }
        Expr::Set(xs, _) => {
            for x in xs {
                hk_add_ctor_key(x, classes, out);
                hk_scan_expr(x, classes, out);
            }
        }
        Expr::DictComp { key, val, iter, cond, .. } => {
            hk_add_ctor_key(key, classes, out);
            hk_scan_expr(key, classes, out);
            hk_scan_expr(val, classes, out);
            hk_scan_expr(iter, classes, out);
            if let Some(c) = cond { hk_scan_expr(c, classes, out); }
        }
        Expr::SetComp { elt, iter, cond, .. } => {
            hk_add_ctor_key(elt, classes, out);
            hk_scan_expr(elt, classes, out);
            hk_scan_expr(iter, classes, out);
            if let Some(c) = cond { hk_scan_expr(c, classes, out); }
        }
        Expr::ListComp { elt, iter, cond, .. } => {
            hk_scan_expr(elt, classes, out);
            hk_scan_expr(iter, classes, out);
            if let Some(c) = cond { hk_scan_expr(c, classes, out); }
        }
        Expr::List(xs, _) | Expr::Tuple(xs, _) => xs.iter().for_each(|x| hk_scan_expr(x, classes, out)),
        Expr::Call { callee, args, kwargs, .. } => {
            hk_scan_expr(callee, classes, out);
            args.iter().for_each(|a| hk_scan_expr(a, classes, out));
            kwargs.iter().for_each(|(_, v)| hk_scan_expr(v, classes, out));
        }
        Expr::Attr { obj, .. } => hk_scan_expr(obj, classes, out),
        Expr::Index { obj, idx, .. } => { hk_scan_expr(obj, classes, out); hk_scan_expr(idx, classes, out); }
        Expr::Slice { obj, start, stop, step, .. } => {
            hk_scan_expr(obj, classes, out);
            for o in [start, stop, step].into_iter().flatten() { hk_scan_expr(o, classes, out); }
        }
        Expr::BinOp { lhs, rhs, .. } => { hk_scan_expr(lhs, classes, out); hk_scan_expr(rhs, classes, out); }
        Expr::UnOp { expr, .. } => hk_scan_expr(expr, classes, out),
        Expr::Lambda { body, .. } => hk_scan_expr(body, classes, out),
        Expr::IfExp { test, body, orelse, .. } => {
            hk_scan_expr(test, classes, out);
            hk_scan_expr(body, classes, out);
            hk_scan_expr(orelse, classes, out);
        }
        Expr::FStr(parts, _) => {
            for p in parts {
                if let FStrPart::Interp(x, _) = p { hk_scan_expr(x, classes, out); }
            }
        }
        _ => {}
    }
}

// ─── (enabler-fix-1 #3) Class-constant promotion scan ────────────────────────
// A class-body binding `NAME: T = <literal>` becomes an associated `const` (an enum
// member) ONLY when it is actually USED as `ClassName.NAME` and NEVER written — so a
// mutable "options/record with defaults" (`class Options: verbose: bool = False`
// mutated via `o.verbose = True`, or a `class Pt: x:int=0` constructed `Pt(5)`) stays
// an ordinary instance field. This mirrors the hash-key derive philosophy
// (usage-gated). The scan gathers the whole-program signals below; the decision is
// finalized in `collect_promoted_consts` and stored in `TyCtx::promoted_consts`.

#[derive(Default)]
struct ConstPromotionScan {
    /// (receiver-class, field) from a `ClassName.FIELD` READ (class-name receiver).
    reads: HashSet<(String, String)>,
    /// (owner-class, field) from a `self.FIELD = ..` write inside a method body.
    self_writes: HashSet<(String, String)>,
    /// (receiver-class, field) from a `ClassName.FIELD = ..` write (class receiver).
    class_writes: HashSet<(String, String)>,
    /// field names written through an INSTANCE `<expr>.FIELD = ..` (non-class recv).
    instance_written: HashSet<String>,
    /// class names instantiated via `ClassName(..)`.
    instantiated: HashSet<String>,
}

fn scan_const_stmt(s: &Stmt, owner: Option<&str>, classes: &HashSet<String>, acc: &mut ConstPromotionScan) {
    match s {
        Stmt::Class(c) => {
            for f in &c.fields {
                if let Some(d) = &f.default {
                    scan_const_expr(d, classes, acc);
                }
            }
            // A method body runs with `self` bound to THIS class.
            for meth in &c.methods {
                for st in &meth.body {
                    scan_const_stmt(st, Some(&c.name), classes, acc);
                }
            }
        }
        // A nested function inherits the enclosing owner (a closure inside a method
        // still writes `self.FIELD` of that method's class).
        Stmt::Func(f) => {
            for st in &f.body {
                scan_const_stmt(st, owner, classes, acc);
            }
        }
        Stmt::AttrAssign { obj, attr, value, .. } => {
            match obj.as_ref() {
                Expr::Ident(n, _) if n == "self" && owner.is_some() => {
                    acc.self_writes.insert((owner.unwrap().to_string(), attr.clone()));
                }
                Expr::Ident(n, _) if classes.contains(n) => {
                    acc.class_writes.insert((n.clone(), attr.clone()));
                }
                _ => {
                    acc.instance_written.insert(attr.clone());
                }
            }
            scan_const_expr(obj, classes, acc);
            scan_const_expr(value, classes, acc);
        }
        Stmt::IndexAssign { obj, idx, value, .. } => {
            scan_const_expr(obj, classes, acc);
            scan_const_expr(idx, classes, acc);
            scan_const_expr(value, classes, acc);
        }
        Stmt::Assign { value, .. } => scan_const_expr(value, classes, acc),
        Stmt::AugAssign { value, .. } => scan_const_expr(value, classes, acc),
        Stmt::Unpack { value, .. } => scan_const_expr(value, classes, acc),
        Stmt::Expr(e) => scan_const_expr(e, classes, acc),
        Stmt::Return(Some(e), _) => scan_const_expr(e, classes, acc),
        Stmt::Yield(e, _) => scan_const_expr(e, classes, acc),
        Stmt::If { cond, then, elifs, else_, .. } => {
            scan_const_expr(cond, classes, acc);
            then.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc));
            for (c, b) in elifs {
                scan_const_expr(c, classes, acc);
                b.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc));
            }
            if let Some(b) = else_ {
                b.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc));
            }
        }
        Stmt::While { cond, body, .. } => {
            scan_const_expr(cond, classes, acc);
            body.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc));
        }
        Stmt::For { iter, body, .. } => {
            scan_const_expr(iter, classes, acc);
            body.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc));
        }
        Stmt::Assert { cond, msg, .. } => {
            scan_const_expr(cond, classes, acc);
            if let Some(m) = msg { scan_const_expr(m, classes, acc); }
        }
        Stmt::Raise { exc, .. } => {
            if let Some(e) = exc { scan_const_expr(e, classes, acc); }
        }
        Stmt::Del { target, .. } => scan_const_expr(target, classes, acc),
        Stmt::With { ctx_expr, body, .. } => {
            scan_const_expr(ctx_expr, classes, acc);
            body.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc));
        }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            body.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc));
            for h in handlers {
                h.body.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc));
            }
            if let Some(b) = else_ { b.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc)); }
            if let Some(b) = finally_ { b.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc)); }
        }
        Stmt::Match { subject, arms, .. } => {
            scan_const_expr(subject, classes, acc);
            for a in arms {
                a.body.iter().for_each(|s| scan_const_stmt(s, owner, classes, acc));
            }
        }
        _ => {}
    }
}

fn scan_const_expr(e: &Expr, classes: &HashSet<String>, acc: &mut ConstPromotionScan) {
    match e {
        Expr::Attr { obj, name, .. } => {
            if let Expr::Ident(x, _) = obj.as_ref() {
                if classes.contains(x) {
                    acc.reads.insert((x.clone(), name.clone()));
                }
            }
            scan_const_expr(obj, classes, acc);
        }
        Expr::Call { callee, args, kwargs, .. } => {
            if let Expr::Ident(c, _) = callee.as_ref() {
                if classes.contains(c) {
                    acc.instantiated.insert(c.clone());
                }
            }
            scan_const_expr(callee, classes, acc);
            args.iter().for_each(|a| scan_const_expr(a, classes, acc));
            kwargs.iter().for_each(|(_, v)| scan_const_expr(v, classes, acc));
        }
        Expr::Index { obj, idx, .. } => {
            scan_const_expr(obj, classes, acc);
            scan_const_expr(idx, classes, acc);
        }
        Expr::Slice { obj, start, stop, step, .. } => {
            scan_const_expr(obj, classes, acc);
            for o in [start, stop, step].into_iter().flatten() {
                scan_const_expr(o, classes, acc);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            scan_const_expr(lhs, classes, acc);
            scan_const_expr(rhs, classes, acc);
        }
        Expr::UnOp { expr, .. } => scan_const_expr(expr, classes, acc),
        Expr::List(xs, _) | Expr::Tuple(xs, _) | Expr::Set(xs, _) => {
            xs.iter().for_each(|x| scan_const_expr(x, classes, acc));
        }
        Expr::Dict(kvs, _) => {
            for (k, v) in kvs {
                scan_const_expr(k, classes, acc);
                scan_const_expr(v, classes, acc);
            }
        }
        Expr::ListComp { elt, iter, cond, .. } | Expr::SetComp { elt, iter, cond, .. } => {
            scan_const_expr(elt, classes, acc);
            scan_const_expr(iter, classes, acc);
            if let Some(c) = cond { scan_const_expr(c, classes, acc); }
        }
        Expr::DictComp { key, val, iter, cond, .. } => {
            scan_const_expr(key, classes, acc);
            scan_const_expr(val, classes, acc);
            scan_const_expr(iter, classes, acc);
            if let Some(c) = cond { scan_const_expr(c, classes, acc); }
        }
        Expr::Lambda { body, .. } => scan_const_expr(body, classes, acc),
        Expr::IfExp { test, body, orelse, .. } => {
            scan_const_expr(test, classes, acc);
            scan_const_expr(body, classes, acc);
            scan_const_expr(orelse, classes, acc);
        }
        Expr::FStr(parts, _) => {
            for p in parts {
                if let FStrPart::Interp(x, _) = p {
                    scan_const_expr(x, classes, acc);
                }
            }
        }
        // Int/Float/Str/Bool/None_/Ident: leaves, no class-const usage.
        _ => {}
    }
}

/// (enabler-fix-1 #3) Compute the promoted class constants over the WHOLE program
/// and store them in `ctx.promoted_consts`. A field of class `C` promotes iff it has
/// a literal default, `C` is not a `@dataclass`, it is READ as `C.FIELD` (or via a
/// subclass) somewhere, and it is never written (`self.FIELD=` in `C`/a subclass, a
/// `C.FIELD=` class write, or — for an INSTANTIATED class — an external instance
/// write). This is the single source of truth both typeck and codegen consult.
pub(crate) fn collect_promoted_consts(modules: &[(Module, String)], ctx: &mut TyCtx) {
    let classes: HashSet<String> = ctx.classes.keys().cloned().collect();
    let mut acc = ConstPromotionScan::default();
    for (m, _) in modules {
        for s in &m.stmts {
            scan_const_stmt(s, None, &classes, &mut acc);
        }
    }
    // Resolve each read/write to the class that DECLARES the field (base-chain), so
    // an inherited-const read (`Sub.KIND`) promotes the defining class (`Base`).
    let mut promote_reads: HashSet<(String, String)> = HashSet::new();
    for (x, f) in &acc.reads {
        if let Some(d) = ctx.defining_class(x, f) {
            promote_reads.insert((d, f.clone()));
        }
    }
    let mut denied: HashSet<(String, String)> = HashSet::new();
    for (m, f) in acc.self_writes.iter().chain(acc.class_writes.iter()) {
        if let Some(d) = ctx.defining_class(m, f) {
            denied.insert((d, f.clone()));
        }
    }
    let mut out: std::collections::HashMap<String, HashSet<String>> = std::collections::HashMap::new();
    for (cname, cd) in &ctx.classes {
        if cd.is_dataclass {
            continue;
        }
        for f in &cd.fields {
            let is_lit = matches!(
                f.default,
                Some(Expr::Int(..)) | Some(Expr::Float(..)) | Some(Expr::Str(..)) | Some(Expr::Bool(..))
            );
            if !is_lit {
                continue;
            }
            let key = (cname.clone(), f.name.clone());
            let read_as_const = promote_reads.contains(&key);
            let written = denied.contains(&key)
                || (acc.instantiated.contains(cname) && acc.instance_written.contains(&f.name));
            if read_as_const && !written {
                out.entry(cname.clone()).or_default().insert(f.name.clone());
            }
        }
    }
    ctx.promoted_consts = out;
}

/// (card e131f8b0) Whether class `cname` can soundly derive `Eq + Hash` (and the
/// `Ord` pyrst needs for deterministic sorted-key iteration) so it may be used as
/// a `dict` key / `set` element. Returns `Ok(())` when eligible, or `Err(reason)`
/// naming the blocking field/dunder — the caller turns that into an honest CHECK
/// error, mirroring CPython's `unhashable type` rule.
///
/// A class is eligible iff: it defines no user `__eq__` (a derived `Hash` could
/// then disagree with `==`) and no user `__lt__` (a custom order conflicts with
/// the derived total order); it is not a polymorphic base (its Rust form is a
/// companion enum without a uniform derive); and every field (transitively) is
/// itself hashable-orderable — `int`/`str`/`bool`, a tuple of such, or a nested
/// eligible class. `float`/`list`/`dict`/`set`/`Callable`/`Optional` fields make
/// it ineligible (their Rust forms are not `Eq`/`Hash`).
pub(crate) fn class_hash_eligible(cname: &str, ctx: &TyCtx) -> std::result::Result<(), String> {
    let mut visited = std::collections::HashSet::new();
    class_hash_eligible_rec(cname, ctx, &mut visited)
}

fn class_hash_eligible_rec(
    cname: &str,
    ctx: &TyCtx,
    visited: &mut std::collections::HashSet<String>,
) -> std::result::Result<(), String> {
    if !visited.insert(cname.to_string()) {
        // Already on the current path — a cyclic field graph; treat as eligible
        // for THIS edge (the cycle can only close through an Optional field, which
        // is rejected below on its own, so this never yields a false positive).
        return Ok(());
    }
    if ctx.get_method(cname, "__eq__").is_some() {
        return Err(format!(
            "class `{}` defines `__eq__`, so a derived `Hash` cannot be guaranteed \
             to agree with it (Python's `a == b` must imply `hash(a) == hash(b)`)",
            cname
        ));
    }
    if ctx.get_method(cname, "__lt__").is_some() {
        return Err(format!(
            "class `{}` defines a custom `__lt__`, which conflicts with the derived \
             total order pyrst needs to iterate a class-keyed dict/set in sorted order",
            cname
        ));
    }
    // (enabler-fix-2 #1d) A POLYMORPHIC BASE (a class some other class derives
    // from) lowers to a companion enum `B__`, not a uniform struct, so it carries
    // NO uniform Eq/Hash/Ord derive — using it as a dict key / set element leaked
    // rustc E0599 on the enum. The doc above already CLAIMED this rejection; make
    // it real. Reached both for a directly-keyed base and for a base-typed FIELD of
    // another key class (the recursion). Key on a concrete leaf class instead.
    if ctx.classes.values().any(|cd| cd.bases.iter().any(|b| b == cname)) {
        return Err(format!(
            "class `{}` is a polymorphic base (it has subclasses), so it lowers to a \
             companion enum with no uniform Eq/Hash/Ord derive and cannot be a dict \
             key / set element; key on a concrete leaf subclass instead",
            cname
        ));
    }
    for f in ctx.get_all_fields(cname) {
        let ty = Ty::from_type_expr(&f.ty, f.span)
            .map_err(|_| format!("field `{}` has an unresolved type", f.name))?;
        field_hashable(&f.name, &ty, ctx, visited)?;
    }
    Ok(())
}

fn field_hashable(
    fname: &str,
    ty: &Ty,
    ctx: &TyCtx,
    visited: &mut std::collections::HashSet<String>,
) -> std::result::Result<(), String> {
    match ty {
        Ty::Int | Ty::Str | Ty::Bool => Ok(()),
        Ty::Tuple(ts) => {
            for t in ts {
                field_hashable(fname, t, ctx, visited)?;
            }
            Ok(())
        }
        Ty::Class(n, _) => class_hash_eligible_rec(n, ctx, visited)
            .map_err(|inner| format!("field `{}` (class `{}`) is not hashable: {}", fname, n, inner)),
        Ty::Float => Err(format!(
            "field `{}` is `float` — f64 is not Eq/Hash, so the class cannot be a dict key / set element",
            fname
        )),
        Ty::List(_) => Err(format!("field `{}` is a list (Vec is not Hash/Eq)", fname)),
        Ty::Dict(..) => Err(format!("field `{}` is a dict (HashMap is not Hash/Eq)", fname)),
        Ty::Set(_) => Err(format!("field `{}` is a set (HashSet is not Hash/Eq)", fname)),
        Ty::Func(..) => Err(format!(
            "field `{}` is a Callable (Rc<dyn Fn> is not Hash/Eq)",
            fname
        )),
        Ty::Option(_) => Err(format!(
            "field `{}` is Optional — an Optional field is not supported in a hashable key class",
            fname
        )),
        other => Err(format!(
            "field `{}` has type `{:?}` which is not hashable",
            fname, other
        )),
    }
}

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
        // LAZY-GEN V1-a: a generator result is move-only (a `Vec<T>` in the eager
        // pipeline, a `Gen<T>` later) — non-Copy, exactly like `List`.
        | Ty::Iterator(_)
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

// ── Branch-divergent bare-local detection (LAZY-GEN V1-d BLOCKER) ─────────────

/// Whether two inferred types would DIVERGE if they had to share ONE Rust slot —
/// the exact rule codegen uses to decide a hoisted local's single `let mut`
/// declaration. This is the SINGLE SOURCE OF TRUTH for hoist-slot compatibility,
/// consumed by BOTH modules: typeck (`detect_branch_divergence`, below) and
/// codegen (`Codegen::types_conflict`, which delegates here) — same "one source of
/// truth" discipline as [`is_copy`]/[`MUTATING_METHODS`], so the check-time
/// rejection and the codegen shadow decision can never drift.
///
/// Two types are NON-divergent (can share a slot) when:
///   - either side is `Unknown` (the permissive inference escape hatch), or
///   - they are an `Int`/`Float` mix (codegen widens the slot to `f64`).
/// Otherwise they diverge iff their `Ty` discriminants differ. Note this is a
/// discriminant-level test: `List(Int)` and `List(Str)` do NOT diverge here (both
/// are `Vec`, a genuine mismatch rustc would catch loudly), while `List(_)` vs
/// `Iterator(_)`, `List(_)` vs `Set(_)`, and `List(_)` vs `Str` DO diverge — the
/// exact silent-miscompile shapes of the V1-d BLOCKER.
pub(crate) fn branch_divergent(a: &Ty, b: &Ty) -> bool {
    if matches!(a, Ty::Unknown) || matches!(b, Ty::Unknown) {
        return false;
    }
    if matches!((a, b), (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int)) {
        return false;
    }
    std::mem::discriminant(a) != std::mem::discriminant(b)
}

/// (card c34ac64a fix B2b) Whether reassigning a `value`-typed rvalue into an
/// Option `slot` (a hoisted / narrowed `Option<T>` binding) genuinely CONFLICTS
/// with the slot — a FULL structural comparison that RECURSES into Option
/// payloads, unlike `branch_divergent` (discriminant-only, which wrongly treated
/// `Option<int>` and `Option<str>` as compatible and reconverged them into one
/// slot -> a leaked rustc E0308). Used at codegen's shadow-reconverge site AND by
/// typeck's reassignment re-widen (B2c) so the two layers agree.
///   - `None` (`NoneVal`) is a valid value for ANY Option slot -> NOT a conflict
///     (it reconverges; the slot stays `Option<T>` — pairs with prescan's B2a
///     merge). This is why a bare `x = None` after a narrow keeps the Option slot.
///   - Two Options reconverge only when their PAYLOADS reconcile
///     (`Option<int>` vs `Option<str>` -> conflict).
///   - Everything else falls back to the shared `branch_divergent` rule, so a
///     non-Option slot behaves exactly as before (numeric/Unknown compatible).
pub(crate) fn option_slot_conflict(slot: &Ty, value: &Ty) -> bool {
    match (slot, value) {
        (Ty::Option(_), Ty::NoneVal) => false,
        (Ty::Option(a), Ty::Option(b)) => option_slot_conflict(a, b),
        _ => branch_divergent(slot, value),
    }
}

/// (card c34ac64a fix B1) Re-widen loop-body narrowing at the loop edge. A loop
/// body runs 0..n times, so an Optional that the body (or the loop condition)
/// narrowed to its payload `T` MUST be `Option<T>` again AFTER the loop —
/// assuming the narrow persists is unsound (the leak was a rustc E0369 on a use
/// of the var after the loop). For every name that was `Option<T>` in `pre_loop`
/// and is now viewed as EXACTLY its inner `T` (i.e. the body narrowed it),
/// restore the `Option<T>` type. A body reassignment to a DIFFERENT type is NOT a
/// stale narrow and is left intact; loop targets and function-level narrows
/// established before the loop persist.
pub(crate) fn rewiden_loop_narrows(pre_loop: &HashMap<String, Ty>, env: &mut FuncEnv) {
    for (name, pre_ty) in pre_loop {
        if let Ty::Option(inner) = pre_ty {
            if env.locals.get(name).is_some_and(|cur| cur == inner.as_ref()) {
                env.locals.insert(name.clone(), pre_ty.clone());
            }
        }
    }
}

/// (card c34ac64a fix B3) A `None`-guard (`x is None` / `x is not None`) whose
/// LHS is a CONCRETE, non-Optional local is a STATICALLY-DECIDED test — `x` can
/// never be None, so the guard is a constant. The common source is a SECOND guard
/// on a name an earlier guard already narrowed to `T` (`env.narrowed` names it);
/// a plain `y: int` mis-tested the same way reaches here too. Either way codegen
/// leaked a raw rustc `.is_none()`/`.is_some()`-on-`T` (E0599) — reject it
/// honestly at check instead. `Unknown`, `NoneVal` (the always-TRUE
/// `x = None; if x is None:` shape, left for codegen's reconverge), and a genuine
/// `Option<_>` are all left alone.
fn reject_decided_none_guard(cond: &Expr, env: &FuncEnv) -> Result<()> {
    if let Some((name, is_not_none)) = extract_none_guard(cond) {
        if let Some(ty) = env.locals.get(name.as_str()) {
            if !matches!(ty, Ty::Option(_) | Ty::Unknown | Ty::NoneVal) {
                let sense = if is_not_none { "not " } else { "" };
                let verdict = if is_not_none { "true" } else { "false" };
                let msg = if env.narrowed.contains_key(name.as_str()) {
                    format!(
                        "`{name}` was already narrowed to `{ty}` by an earlier \
                         guard, so `{name} is {sense}None` is always {verdict} \
                         here. Reassign `{name}` before testing it against None again."
                    )
                } else {
                    format!(
                        "`{name}` has type `{ty}` here and can never be None, so \
                         `{name} is {sense}None` is always {verdict}. A None-guard \
                         applies only to an `Optional[...]` value."
                    )
                };
                return Err(Error::Type { span: cond.span(), msg });
            }
        }
    }
    Ok(())
}

/// (LAZY-GEN V1-d BLOCKER) Detect a BARE (un-annotated) local assigned
/// incompatible types across the SIBLING branches of a control-flow join — the
/// silent miscompile the reviewer traced (comment 131): codegen hoists the name to
/// ONE function-scope Rust slot, then the branch whose type diverges from the
/// hoisted type emits a block-scoped shadow that is discarded at the block's end,
/// so the value is silently dropped at the join. pyrst is statically typed with no
/// union type to represent "one type on one path, another on the next", so this is
/// an honest CHECK error.
///
/// `branches` are the sibling value-paths of ONE join and `join_desc` names it for
/// the message. The three join shapes that reach codegen's shared hoist slot are:
///   - `if` — `then` + each `elif` + `else`;
///   - `try`/`except` — the `try` body + each `except` handler body (the `else`/
///     `finally` blocks run SEQUENTIALLY after the body on the no-exception path,
///     not as alternative values, so they are NOT siblings here);
///   - `match` — each arm body.
///
/// SCOPE (over-rejection guards — card AC2):
///   - Only DIRECT (top-level-of-branch) BARE assignments/unpacks participate.
///     Nested joins are covered by their own recursion; ANNOTATED re-declarations
///     are exempt (the user chose the type — `block_scoping`'s `letter: str` in
///     every branch is the canonical legal pattern, and codegen honours it).
///   - Cross-branch only — sequential same-scope retyping (`x = 5` then `x = "s"`)
///     is a within-block sequence, never two sibling branches, so it is untouched.
///   - `branch_divergent` treats Int/Float and Unknown as compatible, so numeric
///     widening across branches and empty-collection branches are NOT rejected.
///   - A name assigned in only ONE branch has no pair to compare, so the
///     hoist-with-default idiom (assign-in-branch, use-after) stays legal.
pub(crate) fn detect_sibling_divergence(
    branches: &[&[Stmt]],
    env: &FuncEnv,
    join_desc: &str,
) -> Result<()> {
    // `seen`: name -> the (type, span) CANDIDATES accumulated from PRIOR branches.
    // A single branch may contribute MORE THAN ONE candidate for a name (a direct
    // assign plus a divergent reassign nested one block deep — card eca0532e), so
    // candidates are a Vec and we compare every CROSS-branch pair. Same-branch
    // candidates are NOT paired against each other here (an intra-branch retype is
    // the read-after-conflicting-reassign check's domain, not a sibling join).
    let mut seen: std::collections::HashMap<String, Vec<(Ty, Span)>> =
        std::collections::HashMap::new();
    for branch in branches {
        let branch_map = branch_direct_bare_assign_types(branch, env);
        // Compare THIS branch's candidates against every PRIOR branch's candidates.
        for (name, cands) in &branch_map {
            if let Some(prev) = seen.get(name) {
                for (pty, _) in prev {
                    for (ty, span) in cands {
                        if branch_divergent(pty, ty) {
                            return Err(branch_divergence_error(name, pty, ty, *span, join_desc));
                        }
                    }
                }
            }
        }
        // Fold this branch's candidates in AFTER comparing (so its own candidates are
        // never paired against each other).
        for (name, cands) in branch_map {
            seen.entry(name).or_default().extend(cands);
        }
    }
    Ok(())
}

/// The `if`-shaped call (`then` + `elifs` + `else`), preserved for the `Stmt::If`
/// arm's call site. Delegates to [`detect_sibling_divergence`].
pub(crate) fn detect_branch_divergence(
    then: &[Stmt],
    elifs: &[(Expr, Vec<Stmt>)],
    else_: &Option<Vec<Stmt>>,
    env: &FuncEnv,
) -> Result<()> {
    let mut branches: Vec<&[Stmt]> = vec![then];
    for (_, b) in elifs { branches.push(b); }
    if let Some(b) = else_ { branches.push(b); }
    detect_sibling_divergence(&branches, env, "the branches of this `if`")
}

/// Type every branch-level BARE binding a branch MAY exit with — a bare
/// `Stmt::Assign` (`xs = ...`) or a `Stmt::Unpack` (`a, b = ...`, always bare) —
/// threading earlier bindings into a throwaway CLONE of `base_env` so a later RHS
/// sees an earlier local. Descends into SINGLE-ALTERNATIVE nested blocks (an `if`
/// with NO `else`, plus `while`/`for` bodies) so a divergent reassign nested one
/// level deep inside a branch (card eca0532e: `else: if cond2: xs = gen(3)`) still
/// participates in the cross-branch divergence comparison; a `with` body always
/// runs, so it is INLINED (unconditional). A sibling-complete `if` (has `else`) is
/// NOT descended into — it runs its own divergence check via the enclosing
/// recursion. Each name maps to a Vec of candidate `(type, span)`:
///   - a DIRECT (unconditional) bare assign REPLACES the name's candidates (a later
///     unconditional store overrides everything before it — this keeps the
///     documented `xs = gen(..); xs = list(xs)` materialize idiom legal), while
///   - a CONDITIONAL nested assign (if-no-else / while / for) UNIONS its candidates
///     in (the name may take the nested type OR keep its prior value at the join).
/// An ANNOTATED direct assignment removes the name (the declared type governs its
/// slot — exempt). Errors from `check_expr` are swallowed: the real `check_body`
/// pass reports them; here an un-typeable RHS is `Unknown`, which is never
/// divergent (so descent never over-rejects on inference gaps).
fn branch_direct_bare_assign_types(
    branch: &[Stmt],
    base_env: &FuncEnv,
) -> std::collections::HashMap<String, Vec<(Ty, Span)>> {
    let mut clone = base_env.clone();
    let mut out: std::collections::HashMap<String, Vec<(Ty, Span)>> =
        std::collections::HashMap::new();
    collect_branch_exit_types(branch, &mut clone, &mut out);
    out
}

/// Walk one statement list computing, per name, the candidate `(type, span)` values
/// it may hold at the list's exit (see [`branch_direct_bare_assign_types`]).
/// `clone` threads forward types for RHS inference and is mutated in place.
fn collect_branch_exit_types(
    branch: &[Stmt],
    clone: &mut FuncEnv,
    out: &mut std::collections::HashMap<String, Vec<(Ty, Span)>>,
) {
    for s in branch {
        match s {
            Stmt::Assign { target, ty, value, span } => {
                let vt = check_expr(value, clone).unwrap_or(Ty::Unknown);
                let tp = clone.type_param_list();
                let declared = match ty {
                    Some(t) => Ty::from_type_expr_scoped(t, *span, &tp).unwrap_or_else(|_| vt.clone()),
                    None => vt.clone(),
                };
                if ty.is_none() {
                    out.insert(target.clone(), vec![(vt, *span)]); // REPLACE (unconditional)
                } else {
                    out.remove(target); // annotated → exempt
                }
                clone.locals.insert(target.clone(), declared);
            }
            // Tuple unpack (`a, b = ...`) is always bare and hoists each name to its
            // own slot, so a divergent component is the same join hazard as a bare
            // `Assign`. Bind each target to its tuple-component type (mirrors the
            // `Stmt::Unpack` arm of `check_stmt`).
            Stmt::Unpack { targets, value, span } => {
                let vt = check_expr(value, clone).unwrap_or(Ty::Unknown);
                let elem_tys = match &vt {
                    Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
                    _ => vec![Ty::Unknown; targets.len()],
                };
                for (t, ety) in targets.iter().zip(elem_tys.iter()) {
                    out.insert(t.clone(), vec![(ety.clone(), *span)]); // REPLACE
                    clone.locals.insert(t.clone(), ety.clone());
                }
            }
            // A `with` body always runs — INLINE it (unconditional; its direct
            // assigns REPLACE like top-level ones). Threads the same `clone`.
            Stmt::With { body, .. } => {
                collect_branch_exit_types(body, clone, out);
            }
            // Single-alternative `if` (NO else): each then/elif body MAY run, so its
            // assignments are CONDITIONAL candidates. A sibling-complete `if` (has
            // `else`) is handled by the enclosing recursion's own check.
            Stmt::If { then, elifs, else_: None, .. } => {
                merge_conditional_exit_types(then, clone, out);
                for (_, b) in elifs {
                    merge_conditional_exit_types(b, clone, out);
                }
            }
            // Loop bodies MAY run zero or more times — CONDITIONAL candidates.
            Stmt::While { body, .. } | Stmt::For { body, .. } => {
                merge_conditional_exit_types(body, clone, out);
            }
            _ => {}
        }
    }
}

/// Collect a CONDITIONAL nested body's exit types against an ISOLATED clone (it may
/// not run, so it must not definitely rebind an outer local's inferred type) and
/// UNION them into `out` — at the join the name may take the nested type OR keep
/// its prior candidates.
fn merge_conditional_exit_types(
    body: &[Stmt],
    clone: &FuncEnv,
    out: &mut std::collections::HashMap<String, Vec<(Ty, Span)>>,
) {
    let mut sub_clone = clone.clone();
    let mut sub: std::collections::HashMap<String, Vec<(Ty, Span)>> =
        std::collections::HashMap::new();
    collect_branch_exit_types(body, &mut sub_clone, &mut sub);
    for (name, cands) in sub {
        out.entry(name).or_default().extend(cands);
    }
}

/// The honest CHECK error for a bare local assigned divergent types across a
/// control-flow join (see [`detect_sibling_divergence`]). `join_desc` names the
/// join ("the branches of this `if`", "the branches of this `try`/`except`", "the
/// arms of this `match`"). States what is wrong (no single static type for the
/// name at the join) and the three fixes: distinct names, a matching annotation on
/// both branches, or — the generator case — materializing with `list(...)`.
pub(crate) fn branch_divergence_error(name: &str, a: &Ty, b: &Ty, span: Span, join_desc: &str) -> Error {
    Error::Type {
        span,
        msg: format!(
            "local `{}` is assigned incompatible types across {} ({} on one path, {} \
             on another). pyrst is statically typed and has no union type to \
             represent a value that is one type on one branch and a different type \
             on the next. Use a distinct name per branch, give both branches the \
             same explicit annotation, or — when mixing a generator with a list — \
             materialize the generator with `list(...)` so both branches produce a \
             `list`.",
            name, join_desc, a, b
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// (fix-b) READ-AFTER-CONFLICTING-REASSIGN — the residual NON-SIBLING silent
// value-drop that the 439ea0ae sibling-divergence fix left open.
//
// THE SHAPE: an outer-scope bare local is reassigned to a type that DIVERGES from
// its outer/slot type inside a SINGLE nested block (an `if` branch, a `while`/`for`
// body, a `try` body/handler/`else`, a `with`, or a `match` arm), then READ after
// that block. codegen emits the block's reassignment as a block-scoped shadow
// `let` (stmts.rs, `types_conflict` → shadow) that is discarded at the block's
// `}`; the hoisted outer slot keeps its stale value, so the read after the block
// silently observes the wrong value. The sibling-divergence check does not see it
// (it is one branch, or every branch diverges from the OUTER type but agrees with
// its siblings).
//
// WHY LIVENESS: the naive "reject any deeper-scope conflicting reassign" guard
// over-rejects the legal Python idiom of reusing a name for a different type
// inside a block that reads it ONLY within the block (the corpus canary
// `student_management.pyrs` reuses `passing` as `list` then `bool` in a `for`
// body, read only there). The sound discriminator is exactly whether the
// reassigned name is READ AFTER the block exits — classic MAY-liveness at the
// block's exit, INCLUDING the loop back-edge (a reassign in a loop body read on
// the next iteration, before it is rebound, is also stale because each iteration
// is a fresh Rust scope).
// ─────────────────────────────────────────────────────────────────────────────

/// Collect the names an expression READS (identifiers used as values), for the
/// liveness analysis below. Comprehension loop targets and lambda parameters are
/// LOCAL to the expression, so reads of them are subtracted (a use of a
/// comprehension/lambda-bound name is not a read of an enclosing local of the same
/// name). Everything else — call arguments, f-string interpolations,
/// comprehension/`for` sources, subscripts, slices, ternary arms — is a read.
pub(crate) fn expr_reads(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) | Expr::None_(..) => {}
        Expr::Ident(n, _) => { out.insert(n.clone()); }
        Expr::FStr(parts, _) => {
            for p in parts {
                if let FStrPart::Interp(e, _) = p { expr_reads(e, out); }
            }
        }
        Expr::List(xs, _) | Expr::Tuple(xs, _) | Expr::Set(xs, _) => {
            for x in xs { expr_reads(x, out); }
        }
        Expr::Dict(pairs, _) => {
            for (k, v) in pairs { expr_reads(k, out); expr_reads(v, out); }
        }
        Expr::ListComp { elt, targets, iter, cond, .. }
        | Expr::SetComp { elt, targets, iter, cond, .. } => {
            expr_reads(iter, out);
            let mut inner = HashSet::new();
            expr_reads(elt, &mut inner);
            if let Some(c) = cond { expr_reads(c, &mut inner); }
            for t in targets { inner.remove(t); }
            out.extend(inner);
        }
        Expr::DictComp { key, val, targets, iter, cond, .. } => {
            expr_reads(iter, out);
            let mut inner = HashSet::new();
            expr_reads(key, &mut inner);
            expr_reads(val, &mut inner);
            if let Some(c) = cond { expr_reads(c, &mut inner); }
            for t in targets { inner.remove(t); }
            out.extend(inner);
        }
        Expr::Call { callee, args, kwargs, .. } => {
            expr_reads(callee, out);
            for a in args { expr_reads(a, out); }
            for (_, v) in kwargs { expr_reads(v, out); }
        }
        Expr::Attr { obj, .. } => expr_reads(obj, out),
        Expr::Index { obj, idx, .. } => { expr_reads(obj, out); expr_reads(idx, out); }
        Expr::Slice { obj, start, stop, step, .. } => {
            expr_reads(obj, out);
            if let Some(e) = start { expr_reads(e, out); }
            if let Some(e) = stop { expr_reads(e, out); }
            if let Some(e) = step { expr_reads(e, out); }
        }
        Expr::BinOp { lhs, rhs, .. } => { expr_reads(lhs, out); expr_reads(rhs, out); }
        Expr::UnOp { expr, .. } => expr_reads(expr, out),
        Expr::Lambda { params, body, .. } => {
            let mut inner = HashSet::new();
            expr_reads(body, &mut inner);
            for (p, _) in params { inner.remove(p); }
            out.extend(inner);
        }
        Expr::IfExp { test, body, orelse, .. } => {
            expr_reads(test, out); expr_reads(body, out); expr_reads(orelse, out);
        }
    }
}

/// Names BOUND (assigned, unpacked, loop target, `with … as`, `except … as`, or a
/// nested `def`/`class` NAME) anywhere in a body, recursing into control-flow
/// blocks but NOT into nested `def`/`class` bodies (their internal bindings are a
/// separate scope; only the def/class NAME is bound at this level).
pub(crate) fn collect_bound_names(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts { collect_bound_names_stmt(s, out); }
}

pub(crate) fn collect_bound_names_stmt(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Assign { target, .. } => { out.insert(target.clone()); }
        Stmt::Unpack { targets, .. } => { for t in targets { out.insert(t.clone()); } }
        Stmt::For { targets, body, .. } => {
            for t in targets { out.insert(t.clone()); }
            collect_bound_names(body, out);
        }
        Stmt::While { body, .. } => collect_bound_names(body, out),
        Stmt::If { then, elifs, else_, .. } => {
            collect_bound_names(then, out);
            for (_, b) in elifs { collect_bound_names(b, out); }
            if let Some(b) = else_ { collect_bound_names(b, out); }
        }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            collect_bound_names(body, out);
            for h in handlers {
                if let Some(nm) = &h.exc_name { out.insert(nm.clone()); }
                collect_bound_names(&h.body, out);
            }
            if let Some(b) = else_ { collect_bound_names(b, out); }
            if let Some(b) = finally_ { collect_bound_names(b, out); }
        }
        Stmt::With { as_name, body, .. } => {
            if let Some(nm) = as_name { out.insert(nm.clone()); }
            collect_bound_names(body, out);
        }
        Stmt::Match { arms, .. } => {
            for arm in arms {
                if let MatchPattern::Capture(nm) = &arm.pattern { out.insert(nm.clone()); }
                collect_bound_names(&arm.body, out);
            }
        }
        Stmt::Func(f) => { out.insert(f.name.clone()); }
        Stmt::Class(c) => { out.insert(c.name.clone()); }
        _ => {}
    }
}

/// All names READ anywhere in a body (recursing into control-flow blocks). A
/// nested `def`'s reads contribute its CAPTURES (free vars) — a captured name is a
/// read at this level too.
fn collect_body_reads(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts { collect_stmt_reads(s, out); }
}

fn collect_stmt_reads(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Expr(e) | Stmt::Yield(e, _) => expr_reads(e, out),
        Stmt::Assign { value, .. } => expr_reads(value, out),
        Stmt::AugAssign { target, value, .. } => { out.insert(target.clone()); expr_reads(value, out); }
        Stmt::Unpack { value, .. } => expr_reads(value, out),
        Stmt::Return(Some(e), _) => expr_reads(e, out),
        Stmt::Raise { exc: Some(e), .. } => expr_reads(e, out),
        Stmt::Assert { cond, msg, .. } => { expr_reads(cond, out); if let Some(m) = msg { expr_reads(m, out); } }
        Stmt::Del { target, .. } => expr_reads(target, out),
        Stmt::AttrAssign { obj, value, .. } => { expr_reads(obj, out); expr_reads(value, out); }
        Stmt::IndexAssign { obj, idx, value, .. } => { expr_reads(obj, out); expr_reads(idx, out); expr_reads(value, out); }
        Stmt::If { cond, then, elifs, else_, .. } => {
            expr_reads(cond, out);
            collect_body_reads(then, out);
            for (c, b) in elifs { expr_reads(c, out); collect_body_reads(b, out); }
            if let Some(b) = else_ { collect_body_reads(b, out); }
        }
        Stmt::While { cond, body, .. } => { expr_reads(cond, out); collect_body_reads(body, out); }
        Stmt::For { iter, body, .. } => { expr_reads(iter, out); collect_body_reads(body, out); }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            collect_body_reads(body, out);
            for h in handlers { collect_body_reads(&h.body, out); }
            if let Some(b) = else_ { collect_body_reads(b, out); }
            if let Some(b) = finally_ { collect_body_reads(b, out); }
        }
        Stmt::With { ctx_expr, body, .. } => { expr_reads(ctx_expr, out); collect_body_reads(body, out); }
        Stmt::Match { subject, arms, .. } => {
            expr_reads(subject, out);
            for arm in arms {
                if let Some(g) = &arm.guard { expr_reads(g, out); }
                collect_body_reads(&arm.body, out);
            }
        }
        Stmt::Func(f) => nested_def_captured_reads(f, out),
        _ => {}
    }
}

/// The names a nested `def` CAPTURES from its enclosing scope — its free
/// variables: every name it reads that it does not itself bind (its params, plus
/// any name assigned anywhere in its body). A captured name is a READ of the
/// enclosing local for liveness: codegen moves/borrows the captured value at the
/// def site, so a divergent reassign whose name is later captured is not safe.
pub(crate) fn nested_def_captured_reads(f: &Func, out: &mut HashSet<String>) {
    let mut reads = HashSet::new();
    collect_body_reads(&f.body, &mut reads);
    let mut bound: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
    collect_bound_names(&f.body, &mut bound);
    for n in reads {
        if !bound.contains(&n) { out.insert(n); }
    }
}

/// Backward MAY-liveness at the ENTRY of a statement list, given `live_out` = the
/// names possibly read (before redefinition) after the list executes.
pub(crate) fn live_in_stmts(stmts: &[Stmt], live_out: &HashSet<String>) -> HashSet<String> {
    let mut live = live_out.clone();
    for s in stmts.iter().rev() {
        live = live_in_stmt(s, &live);
    }
    live
}

/// Liveness at the EXIT of a loop body (what a reassign directly in the body is
/// checked against, and the `live_out` passed when recursing into the body): the
/// least fixed point of `exit_live ∪ reads(cond) ∪ live_in(body)`, so the loop
/// BACK-EDGE is included — a name read at the top of the body on a later iteration,
/// before it is rebound, is live at the body's exit. `for` targets are rebound at
/// the top of each iteration, so they are killed on the back-edge.
pub(crate) fn loop_body_live_out(
    body: &[Stmt],
    exit_live: &HashSet<String>,
    for_targets: Option<&[String]>,
    while_cond: Option<&Expr>,
) -> HashSet<String> {
    let mut bo = exit_live.clone();
    if let Some(c) = while_cond { expr_reads(c, &mut bo); }
    loop {
        let mut inner = live_in_stmts(body, &bo);
        if let Some(ts) = for_targets { for t in ts { inner.remove(t); } }
        let mut next = exit_live.clone();
        if let Some(c) = while_cond { expr_reads(c, &mut next); }
        next.extend(inner);
        if next.is_subset(&bo) { break; }
        for x in next { bo.insert(x); }
    }
    bo
}

/// Liveness at the ENTRY of a single statement, given `live_out` after it. A bare
/// `Assign`/`Unpack` KILLS its target(s) (a redefinition); `AugAssign`, `return`,
/// `for`-source, call arguments, comprehension sources, and a nested `def`'s
/// captures are READS. Loops fold their back-edge via `loop_body_live_out`.
pub(crate) fn live_in_stmt(s: &Stmt, live_out: &HashSet<String>) -> HashSet<String> {
    match s {
        Stmt::Assign { target, value, .. } => {
            let mut live = live_out.clone();
            live.remove(target);
            expr_reads(value, &mut live);
            live
        }
        Stmt::AugAssign { target, value, .. } => {
            let mut live = live_out.clone();
            expr_reads(value, &mut live);
            live.insert(target.clone());
            live
        }
        Stmt::Unpack { targets, value, .. } => {
            let mut live = live_out.clone();
            for t in targets { live.remove(t); }
            expr_reads(value, &mut live);
            live
        }
        Stmt::Expr(e) | Stmt::Yield(e, _) => {
            let mut live = live_out.clone();
            expr_reads(e, &mut live);
            live
        }
        // A `return <e>` diverts control: statements after it in this list are not
        // reached on this path, so nothing in `live_out` survives — only the
        // return expression's own reads are live.
        Stmt::Return(Some(e), _) => {
            let mut live = HashSet::new();
            expr_reads(e, &mut live);
            live
        }
        Stmt::Return(None, _) => HashSet::new(),
        Stmt::Raise { exc, .. } => {
            let mut live = HashSet::new();
            if let Some(e) = exc { expr_reads(e, &mut live); }
            live
        }
        Stmt::Assert { cond, msg, .. } => {
            let mut live = live_out.clone();
            expr_reads(cond, &mut live);
            if let Some(m) = msg { expr_reads(m, &mut live); }
            live
        }
        Stmt::Del { target, .. } => {
            let mut live = live_out.clone();
            expr_reads(target, &mut live);
            live
        }
        Stmt::AttrAssign { obj, value, .. } => {
            let mut live = live_out.clone();
            expr_reads(obj, &mut live);
            expr_reads(value, &mut live);
            live
        }
        Stmt::IndexAssign { obj, idx, value, .. } => {
            let mut live = live_out.clone();
            expr_reads(obj, &mut live);
            expr_reads(idx, &mut live);
            expr_reads(value, &mut live);
            live
        }
        Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::Import { .. } => live_out.clone(),
        Stmt::If { cond, then, elifs, else_, .. } => {
            let mut live = HashSet::new();
            live.extend(live_in_stmts(then, live_out));
            for (_, b) in elifs { live.extend(live_in_stmts(b, live_out)); }
            match else_ {
                Some(b) => live.extend(live_in_stmts(b, live_out)),
                // No `else`: the "no branch taken" path falls through, so `live_out`
                // survives unchanged.
                None => live.extend(live_out.iter().cloned()),
            }
            expr_reads(cond, &mut live);
            for (c, _) in elifs { expr_reads(c, &mut live); }
            live
        }
        Stmt::While { cond, body, .. } => loop_body_live_out(body, live_out, None, Some(cond)),
        Stmt::For { targets, iter, body, .. } => {
            let mut live = loop_body_live_out(body, live_out, Some(targets), None);
            expr_reads(iter, &mut live);
            live
        }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            let after_finally = match finally_ {
                Some(f) => live_in_stmts(f, live_out),
                None => live_out.clone(),
            };
            // No-exception path: body then `else` then finally.
            let else_live = match else_ {
                Some(e) => live_in_stmts(e, &after_finally),
                None => after_finally.clone(),
            };
            let mut live = live_in_stmts(body, &else_live);
            // Exception paths: each handler then finally.
            for h in handlers {
                let mut hlive = live_in_stmts(&h.body, &after_finally);
                if let Some(nm) = &h.exc_name { hlive.remove(nm); }
                live.extend(hlive);
            }
            live
        }
        Stmt::With { ctx_expr, as_name, body, .. } => {
            let mut live = live_in_stmts(body, live_out);
            if let Some(nm) = as_name { live.remove(nm); }
            expr_reads(ctx_expr, &mut live);
            live
        }
        Stmt::Match { subject, arms, .. } => {
            let mut live = HashSet::new();
            for arm in arms {
                let mut al = live_in_stmts(&arm.body, live_out);
                if let Some(g) = &arm.guard { expr_reads(g, &mut al); }
                if let MatchPattern::Capture(nm) = &arm.pattern { al.remove(nm); }
                live.extend(al);
            }
            // A non-exhaustive `match` can fall through, so `live_out` survives.
            live.extend(live_out.iter().cloned());
            expr_reads(subject, &mut live);
            live
        }
        // A nested `def` binds its NAME (kills it) and READS its captures.
        Stmt::Func(f) => {
            let mut live = live_out.clone();
            live.remove(&f.name);
            nested_def_captured_reads(f, &mut live);
            live
        }
        Stmt::Class(c) => {
            let mut live = live_out.clone();
            live.remove(&c.name);
            live
        }
    }
}

/// The honest CHECK error for the read-after-conflicting-reassign shape. Mirrors
/// the sibling-divergence house style: what is wrong (a block-scoped shadow is
/// dropped at the block's end so the read sees the stale outer value) + the fixes
/// (distinct name / same annotation / materialize a generator with `list(...)`).
pub(crate) fn read_after_reassign_error(name: &str, outer: &Ty, inner: &Ty, span: Span) -> Error {
    Error::Type {
        span,
        msg: format!(
            "local `{}` is reassigned to an incompatible type inside this block ({} \
             before the block, {} inside) and is read after the block. pyrst emits \
             the reassignment as a block-scoped shadow that is discarded when the \
             block ends, so the read after the block would see the stale outer value \
             (a silent wrong result). Use a distinct name for the block-local value, \
             give the reassignment the same explicit annotation as the outer \
             binding, or — when mixing a generator with a list — materialize the \
             generator with `list(...)` so the block does not change the type.",
            name, outer, inner
        ),
    }
}

/// The type of a reassignment's RHS for the divergence decision. Thin wrapper
/// over `check_expr` (matching the `Unpack` path's inline `check_expr`).
///
/// (W0-c) This used to carry a set-algebra special-case: `check_expr` blanket-
/// typed `|`/`&`/`^` as `Int`, so a set accumulator `s = s | t` looked like a
/// `set -> int` type change and was falsely flagged as divergent. `check_expr`
/// now types set `&`/`|`/`^`/`-` over two sets as the set type directly, so that
/// correction is dead and has been removed — the wrapper simply returns the
/// checked type.
fn reassign_value_ty(value: &Expr, env: &FuncEnv) -> Ty {
    check_expr(value, &mut env.clone()).unwrap_or(Ty::Unknown)
}

/// (W0-b, honesty hole p09) Enforce Python's ACTUAL scoping rule for a module
/// constant whose name is also assigned inside a function, so pyrst never (a)
/// leaks a raw rustc E0425 nor (b) silently produces wrong output by reading the
/// stale module const where Python would see the function-local.
///
/// **The rule.** In Python, ANY binding of a name anywhere in a function body
/// (plain / augmented assign, tuple unpack, a `for` / `with … as` / `except … as`
/// / `match` capture target, at ANY nesting depth other than a nested `def`/`class`
/// — those are separate scopes) makes that name a function-LOCAL for the WHOLE
/// function. Reading it on a path where the binding may not yet have executed is
/// an `UnboundLocalError`. So: a read of such a shadowed const that is not
/// definitely-bound before it is REJECTED here; a read that a preceding binding
/// dominates is accepted (codegen resolves it to the local — and the match-capture
/// name-resolution bug that used to read the stale const is fixed in codegen).
///
/// This deliberately REPLACES the earlier block-scope compromise, which matched
/// codegen's (non-Python) block scoping and thereby kept three silent
/// wrong-output shapes alive (a `for`/`with`/`match`-target const read after the
/// block, and an `except`-as const read after the handler). Under the new rule
/// those post-block reads are honest check errors (Python raises there too, or the
/// value would be wrong), and an in-block read of the block's own target stays
/// legal. A pure straight-line shadow (`PI = 3.0; print(PI)`) and a read-only use
/// of a const are both untouched.
///
/// `consts` = module-constant names (`ctx.vars` keys); `params` = the function's
/// own parameter names (bound at entry — a param sharing a const name is NOT a
/// shadow, so they seed the in-scope set). Nested `def`s are re-checked as their
/// own scopes (a const shadowed only inside a closure would otherwise still leak
/// E0425 — review probe7).
pub(crate) fn detect_module_const_unbound_local(
    body: &[Stmt],
    consts: &HashSet<String>,
    params: &HashSet<String>,
) -> Result<()> {
    if consts.is_empty() {
        return Ok(());
    }
    // A module const BOUND anywhere in THIS scope (any binding form, any nesting
    // except nested def/class bodies) is a function-local for the whole function.
    let mut local_names = HashSet::new();
    collect_bound_names(body, &mut local_names);
    collect_augassign_targets(body, &mut local_names);
    let shadowed: HashSet<String> = consts
        .iter()
        .filter(|c| local_names.contains(*c) && !params.contains(*c))
        .cloned()
        .collect();
    if !shadowed.is_empty() {
        let mut in_scope: HashSet<String> = params.clone();
        walk_unbound_local(body, &shadowed, &mut in_scope)?;
    }
    // Recurse into every nested `def` as its OWN scope: a const shadowed only
    // inside a closure is invisible to this scope's `local_names`, so without this
    // the E0425 leak reappears there (review comment 208 BLOCKER, probe7).
    check_nested_defs(body, consts)?;
    Ok(())
}

/// Re-run [`detect_module_const_unbound_local`] on every nested `def` found in
/// `body` (recursing control-flow blocks to reach a def defined inside one), each
/// as its OWN function scope seeded with that def's own parameter names. The
/// nested def's body is then itself scanned for deeper nested defs by the
/// recursive `detect_*` call.
fn check_nested_defs(body: &[Stmt], consts: &HashSet<String>) -> Result<()> {
    for s in body {
        match s {
            Stmt::Func(f) => {
                let fparams: HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
                detect_module_const_unbound_local(&f.body, consts, &fparams)?;
            }
            Stmt::If { then, elifs, else_, .. } => {
                check_nested_defs(then, consts)?;
                for (_, b) in elifs { check_nested_defs(b, consts)?; }
                if let Some(b) = else_ { check_nested_defs(b, consts)?; }
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::With { body, .. } => check_nested_defs(body, consts)?,
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                check_nested_defs(body, consts)?;
                for h in handlers { check_nested_defs(&h.body, consts)?; }
                if let Some(b) = else_ { check_nested_defs(b, consts)?; }
                if let Some(b) = finally_ { check_nested_defs(b, consts)?; }
            }
            Stmt::Match { arms, .. } => {
                for arm in arms { check_nested_defs(&arm.body, consts)?; }
            }
            _ => {}
        }
    }
    Ok(())
}

/// `AugAssign` targets anywhere in `stmts` (recursing control-flow blocks but not
/// nested def/class bodies) — the one binding form `collect_bound_names` omits.
fn collect_augassign_targets(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::AugAssign { target, .. } => { out.insert(target.clone()); }
            Stmt::If { then, elifs, else_, .. } => {
                collect_augassign_targets(then, out);
                for (_, b) in elifs { collect_augassign_targets(b, out); }
                if let Some(b) = else_ { collect_augassign_targets(b, out); }
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::With { body, .. } => collect_augassign_targets(body, out),
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                collect_augassign_targets(body, out);
                for h in handlers { collect_augassign_targets(&h.body, out); }
                if let Some(b) = else_ { collect_augassign_targets(b, out); }
                if let Some(b) = finally_ { collect_augassign_targets(b, out); }
            }
            Stmt::Match { arms, .. } => {
                for arm in arms { collect_augassign_targets(&arm.body, out); }
            }
            _ => {}
        }
    }
}

/// The reads in a statement's OWN expressions (its value / condition / iterable /
/// subject / guards) — SHALLOW: it does NOT descend into child block bodies (the
/// caller recurses those) nor into a nested `def`'s captures (a separate scope).
fn shallow_stmt_reads(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Expr(e) | Stmt::Yield(e, _) => expr_reads(e, out),
        Stmt::Assign { value, .. } => expr_reads(value, out),
        Stmt::AugAssign { target, value, .. } => { out.insert(target.clone()); expr_reads(value, out); }
        Stmt::Unpack { value, .. } => expr_reads(value, out),
        Stmt::Return(Some(e), _) => expr_reads(e, out),
        Stmt::Raise { exc: Some(e), .. } => expr_reads(e, out),
        Stmt::Assert { cond, msg, .. } => { expr_reads(cond, out); if let Some(m) = msg { expr_reads(m, out); } }
        Stmt::Del { target, .. } => expr_reads(target, out),
        Stmt::AttrAssign { obj, value, .. } => { expr_reads(obj, out); expr_reads(value, out); }
        Stmt::IndexAssign { obj, idx, value, .. } => { expr_reads(obj, out); expr_reads(idx, out); expr_reads(value, out); }
        Stmt::If { cond, .. } => expr_reads(cond, out),
        Stmt::While { cond, .. } => expr_reads(cond, out),
        Stmt::For { iter, .. } => expr_reads(iter, out),
        Stmt::With { ctx_expr, .. } => expr_reads(ctx_expr, out),
        Stmt::Match { subject, .. } => {
            // Only the subject is read at the whole-statement level; each arm's
            // GUARD is read AFTER that arm's capture binds, so guard reads are
            // checked per-arm inside `walk_unbound_local` (not here) — otherwise a
            // guard reading its own capture (`case M if M > 100`) would be flagged
            // as read-before-bound (review comment 211 Bug B).
            expr_reads(subject, out);
        }
        _ => {}
    }
}

/// In-order worker for [`detect_module_const_unbound_local`]. For each statement,
/// its OWN reads are checked BEFORE its own binds take effect (so `n = n + 1`,
/// whose RHS reads a not-yet-in-scope shadowed const, is flagged). A plain assign
/// at this level records its target in `in_scope`. Control-flow children are
/// walked via [`walk_block_bound`], which returns the names DEFINITELY bound at the
/// block's normal exit; the caller then decides whether those persist:
///   - an `if` promotes only names bound on EVERY branch of an exhaustive set
///     (all of `then` + every elif body + a present `else`) — the definite-bound
///     merge (review comment 211 Bug A);
///   - a `match` promotes names bound in EVERY arm of an exhaustive match (one with
///     an unguarded wildcard / capture catch-all);
///   - a `for`/`while` loop body, a `with`, and a `try`/`except`/`finally` are
///     CONSERVATIVE — their bindings do not persist (the loop may run zero times;
///     an `except`/`finally`/`with` binding may be skipped when the guarded region
///     raises mid-way), matching Python's own may-be-unbound outcome there.
fn walk_unbound_local(
    stmts: &[Stmt],
    shadowed: &HashSet<String>,
    in_scope: &mut HashSet<String>,
) -> Result<()> {
    for s in stmts {
        let mut reads = HashSet::new();
        shallow_stmt_reads(s, &mut reads);
        for r in &reads {
            if shadowed.contains(r) && !in_scope.contains(r) {
                return Err(unbound_local_const_error(r, stmt_span(s)));
            }
        }
        match s {
            Stmt::Assign { target, .. } | Stmt::AugAssign { target, .. } => {
                in_scope.insert(target.clone());
            }
            Stmt::Unpack { targets, .. } => {
                for t in targets { in_scope.insert(t.clone()); }
            }
            Stmt::If { then, elifs, else_, .. } => {
                // Each branch is analyzed from the pre-`if` scope (the primary
                // `cond` was read-checked above; the elif conditions are checked
                // here — they are evaluated only when the earlier conditions were
                // false, still before any branch binds).
                let mut deltas = vec![walk_block_bound(then, shadowed, in_scope, &[])?];
                for (ec, b) in elifs {
                    let mut ereads = HashSet::new();
                    expr_reads(ec, &mut ereads);
                    for r in &ereads {
                        if shadowed.contains(r) && !in_scope.contains(r) {
                            return Err(unbound_local_const_error(r, ec.span()));
                        }
                    }
                    deltas.push(walk_block_bound(b, shadowed, in_scope, &[])?);
                }
                // Definite-bound merge: only with an `else` is the branch set
                // exhaustive, so a name bound in ALL branches is bound on every
                // path and promotes to the outer scope. No `else` → a fall-through
                // path binds nothing, so promote nothing (stays may-be-unbound).
                if let Some(b) = else_ {
                    deltas.push(walk_block_bound(b, shadowed, in_scope, &[])?);
                    if let Some((first, rest)) = deltas.split_first() {
                        let mut common = first.clone();
                        for d in rest { common.retain(|n| d.contains(n)); }
                        in_scope.extend(common);
                    }
                }
            }
            Stmt::While { body, .. } => { walk_block_bound(body, shadowed, in_scope, &[])?; }
            Stmt::For { targets, body, .. } => { walk_block_bound(body, shadowed, in_scope, targets)?; }
            Stmt::With { as_name, body, .. } => {
                let extra: Vec<String> = as_name.iter().cloned().collect();
                walk_block_bound(body, shadowed, in_scope, &extra)?;
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                walk_block_bound(body, shadowed, in_scope, &[])?;
                for h in handlers {
                    let extra: Vec<String> = h.exc_name.iter().cloned().collect();
                    walk_block_bound(&h.body, shadowed, in_scope, &extra)?;
                }
                if let Some(b) = else_ { walk_block_bound(b, shadowed, in_scope, &[])?; }
                if let Some(b) = finally_ { walk_block_bound(b, shadowed, in_scope, &[])?; }
            }
            Stmt::Match { arms, .. } => {
                let mut arm_deltas: Vec<HashSet<String>> = Vec::new();
                let mut exhaustive = false;
                for arm in arms {
                    let cap: Vec<String> = match &arm.pattern {
                        MatchPattern::Capture(nm) => vec![nm.clone()],
                        _ => vec![],
                    };
                    // The capture binds BEFORE the guard runs, so both the guard
                    // and the body see it (Bug B).
                    let mut arm_in = in_scope.clone();
                    for c in &cap { arm_in.insert(c.clone()); }
                    if let Some(g) = &arm.guard {
                        let mut greads = HashSet::new();
                        expr_reads(g, &mut greads);
                        for r in &greads {
                            if shadowed.contains(r) && !arm_in.contains(r) {
                                return Err(unbound_local_const_error(r, g.span()));
                            }
                        }
                    }
                    let mut body_in = arm_in.clone();
                    walk_unbound_local(&arm.body, shadowed, &mut body_in)?;
                    let mut delta: HashSet<String> = body_in.difference(in_scope).cloned().collect();
                    for c in &cap { delta.remove(c); }
                    arm_deltas.push(delta);
                    if arm.guard.is_none()
                        && matches!(&arm.pattern, MatchPattern::Wildcard | MatchPattern::Capture(_))
                    {
                        exhaustive = true;
                    }
                }
                if exhaustive {
                    if let Some((first, rest)) = arm_deltas.split_first() {
                        let mut common = first.clone();
                        for d in rest { common.retain(|n| d.contains(n)); }
                        in_scope.extend(common);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Walk a child BLOCK in its own scope and RETURN the names it DEFINITELY bound at
/// its normal exit (relative to the entry `in_scope`, excluding the block's own
/// `extra` targets — a `for`/`with`/`except`/`match` target is block-scoped and
/// never persists). The caller merges (or discards) that delta per its control-flow
/// semantics. `in_scope` is not mutated.
fn walk_block_bound(
    body: &[Stmt],
    shadowed: &HashSet<String>,
    in_scope: &HashSet<String>,
    extra: &[String],
) -> Result<HashSet<String>> {
    let mut local = in_scope.clone();
    for e in extra { local.insert(e.clone()); }
    walk_unbound_local(body, shadowed, &mut local)?;
    let mut delta: HashSet<String> = local.difference(in_scope).cloned().collect();
    for e in extra { delta.remove(e); }
    Ok(delta)
}

/// The honest error for the Python `UnboundLocalError` hole (p09): a module
/// constant read before the local assignment that shadows it. Points at the same
/// two facts the message needs — the name is module-constant / immutable, and
/// module-level mutable state is not yet supported — without promising the lift.
fn unbound_local_const_error(name: &str, span: Span) -> Error {
    Error::Type {
        span,
        msg: format!(
            "`{0}` is a module constant, but it is assigned inside this function, \
             which (per Python scoping) makes `{0}` a local for the whole function \
             — so reading it here, before that local assignment, is an error \
             (Python raises `UnboundLocalError`). Module constants are immutable \
             and module-level mutable state is not yet supported; use a distinct \
             local name, or pass `{0}` in as a parameter and return the new value.",
            name
        ),
    }
}

/// (fix-b) Entry point: reject a bare outer-scope local reassigned to a divergent
/// type inside a single nested block and READ after that block. `env` is the
/// function-scope environment (params typed); runs AFTER `check_body` so the body
/// already type-checks (RHS typing errors are swallowed here).
pub(crate) fn detect_read_after_conflicting_reassign(body: &[Stmt], env: &FuncEnv) -> Result<()> {
    let params: HashSet<String> = env.params.iter().cloned().collect();
    walk_read_after(body, &HashSet::new(), env, &HashSet::new(), &params)
}

/// Recursive worker. `outer` = names bound in strictly-enclosing scopes
/// (reassigning one HERE emits a block-scoped shadow); `env` = forward types at
/// this body's entry; `body_live_out` = MAY-liveness at this body's exit (the loop
/// fixed point for a loop body); `seed_bound` = names pre-bound at this body's
/// level (params at the top, loop targets / `except`-as / `with`-as / capture in a
/// nested body). A DIRECT bare reassign `n = e` is FLAGGED iff `n ∈ outer`,
/// `n ∈ body_live_out`, and `branch_divergent(outer_slot_ty, ty(e))`. Deeper
/// reassigns are caught by recursion; checking against `body_live_out` (not the
/// per-statement liveness) is what keeps a same-block read AFTER the reassign safe
/// (the block-scoped shadow reaches the rest of the block).
fn walk_read_after(
    body: &[Stmt],
    outer: &HashSet<String>,
    env: &FuncEnv,
    body_live_out: &HashSet<String>,
    seed_bound: &HashSet<String>,
) -> Result<()> {
    let n = body.len();
    // Suffix liveness: live_at[i] = names live just before body[i]; live_at[i+1] is
    // the exit-liveness a non-loop child block at position i is checked against.
    let mut live_at: Vec<HashSet<String>> = vec![HashSet::new(); n + 1];
    live_at[n] = body_live_out.clone();
    for i in (0..n).rev() {
        live_at[i] = live_in_stmt(&body[i], &live_at[i + 1]);
    }

    let mut env = env.clone();
    let mut bound_here: HashSet<String> = seed_bound.clone();

    for i in 0..n {
        let after = &live_at[i + 1];
        let s = &body[i];

        // 1) FLAG a direct bare reassign of an OUTER name to a divergent type that
        //    is read AFTER this block.
        match s {
            Stmt::Assign { target, ty: None, value, span } => {
                if outer.contains(target) && body_live_out.contains(target) {
                    if let Some(outer_ty) = env.locals.get(target).cloned() {
                        let vt = reassign_value_ty(value, &env);
                        if branch_divergent(&outer_ty, &vt) {
                            return Err(read_after_reassign_error(target, &outer_ty, &vt, *span));
                        }
                    }
                }
            }
            Stmt::Unpack { targets, value, span } => {
                let vt = check_expr(value, &mut env.clone()).unwrap_or(Ty::Unknown);
                let elem_tys = match &vt {
                    Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
                    _ => vec![Ty::Unknown; targets.len()],
                };
                for (t, ety) in targets.iter().zip(elem_tys.iter()) {
                    if outer.contains(t) && body_live_out.contains(t) {
                        if let Some(outer_ty) = env.locals.get(t).cloned() {
                            if branch_divergent(&outer_ty, ety) {
                                return Err(read_after_reassign_error(t, &outer_ty, ety, *span));
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // 2) RECURSE into nested blocks. Their DIRECT reassigns are flagged at their
        //    own level; `new_outer` adds everything bound before this block, and each
        //    block body carries the exit-liveness it is checked against.
        let new_outer: HashSet<String> = outer.union(&bound_here).cloned().collect();
        let empty = HashSet::new();
        match s {
            Stmt::If { then, elifs, else_, .. } => {
                walk_read_after(then, &new_outer, &env, after, &empty)?;
                for (_, b) in elifs { walk_read_after(b, &new_outer, &env, after, &empty)?; }
                if let Some(b) = else_ { walk_read_after(b, &new_outer, &env, after, &empty)?; }
            }
            Stmt::While { cond, body: wb, .. } => {
                let loop_out = loop_body_live_out(wb, after, None, Some(cond));
                walk_read_after(wb, &new_outer, &env, &loop_out, &empty)?;
            }
            Stmt::For { targets, body: fb, .. } => {
                let loop_out = loop_body_live_out(fb, after, Some(targets), None);
                let tset: HashSet<String> = targets.iter().cloned().collect();
                let mut benv = env.clone();
                for t in targets { benv.locals.entry(t.clone()).or_insert(Ty::Unknown); }
                walk_read_after(fb, &new_outer, &benv, &loop_out, &tset)?;
            }
            Stmt::Try { body: tb, handlers, else_, finally_, .. } => {
                walk_read_after(tb, &new_outer, &env, after, &empty)?;
                for h in handlers {
                    let mut henv = env.clone();
                    let mut hseed = HashSet::new();
                    if let Some(nm) = &h.exc_name {
                        henv.locals.insert(nm.clone(), Ty::Str);
                        hseed.insert(nm.clone());
                    }
                    walk_read_after(&h.body, &new_outer, &henv, after, &hseed)?;
                }
                if let Some(b) = else_ { walk_read_after(b, &new_outer, &env, after, &empty)?; }
                if let Some(b) = finally_ { walk_read_after(b, &new_outer, &env, after, &empty)?; }
            }
            Stmt::With { as_name, body: wb, .. } => {
                let mut wenv = env.clone();
                let mut wseed = HashSet::new();
                if let Some(nm) = as_name {
                    wenv.locals.entry(nm.clone()).or_insert(Ty::Unknown);
                    wseed.insert(nm.clone());
                }
                walk_read_after(wb, &new_outer, &wenv, after, &wseed)?;
            }
            Stmt::Match { subject, arms, .. } => {
                let subj_ty = check_expr(subject, &mut env.clone()).unwrap_or(Ty::Unknown);
                for arm in arms {
                    let mut aenv = env.clone();
                    let mut aseed = HashSet::new();
                    if let MatchPattern::Capture(nm) = &arm.pattern {
                        aenv.locals.insert(nm.clone(), subj_ty.clone());
                        aseed.insert(nm.clone());
                    }
                    walk_read_after(&arm.body, &new_outer, &aenv, after, &aseed)?;
                }
            }
            _ => {}
        }

        // 3) Advance the forward type env EXACTLY as the checker (so block-locals
        //    hoist and sequential retypes flow into later blocks' baselines), and
        //    grow the set of names bound at this level. Errors are swallowed: the
        //    real `check_body` pass already reported them.
        let _ = check_stmt(s, &mut env);
        collect_bound_names_stmt(s, &mut bound_here);
    }
    Ok(())
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
    block_transfers_flow(stmts, /*count_loopctl=*/ false)
}

/// (dedupe, phase2-fix2) The shared recursion core behind both
/// [`block_definitely_returns`] and [`block_terminates_flow`], which were
/// verbatim copies. Whether the LAST statement of `stmts` transfers control away
/// from the fall-through that follows the enclosing statement. `count_loopctl`
/// selects the variant:
/// - `false` — only `return`/`raise` (and a total `if`/`match`/`try`/`while
///   True`) count. This is the missing-return analysis: `break`/`continue` do
///   NOT return, so they must not satisfy a non-unit function's value path.
/// - `true` — ALSO counts `break`/`continue`. This is the None-guard negative
///   narrowing analysis: they terminate a branch's fall-through even though they
///   do not return.
///
/// Every other rule is identical between the two, which is why keeping the
/// missing-return variant (its negatives prove) byte-identical requires only the
/// `Stmt::Break`/`Stmt::Continue` arm to branch on `count_loopctl`.
fn block_transfers_flow(stmts: &[Stmt], count_loopctl: bool) -> bool {
    match stmts.last() {
        Some(s) => stmt_transfers_flow(s, count_loopctl),
        None => false,
    }
}

fn stmt_transfers_flow(s: &Stmt, count_loopctl: bool) -> bool {
    match s {
        // An explicit `return` (with or without a value) terminates the path — a
        // bare `return` in a non-unit function is a separate honest error (see
        // the `Stmt::Return(None, _)` arm in `check_stmt`) but still does not
        // fall off the end. `raise` diverges — control never continues past it.
        Stmt::Return(..) | Stmt::Raise { .. } => true,
        // `break`/`continue` jump to the loop end/head — they do not fall through
        // to the next sibling statement, but they do NOT return a value. Only the
        // terminates-flow variant (negative narrowing) counts them; the
        // missing-return variant must not (a `break` cannot cover a value path),
        // which is exactly the `false` here for `count_loopctl == false`.
        Stmt::Break(_) | Stmt::Continue(_) => count_loopctl,
        // An `if` only covers all paths when there is an `else` and EVERY branch
        // (then, each elif, else) transfers. No `else` -> the implicit empty else
        // falls through, so the `if` cannot guarantee it.
        Stmt::If { then, elifs, else_: Some(else_block), .. } => {
            block_transfers_flow(then, count_loopctl)
                && elifs.iter().all(|(_, b)| block_transfers_flow(b, count_loopctl))
                && block_transfers_flow(else_block, count_loopctl)
        }
        Stmt::If { else_: None, .. } => false,
        // `while True:` with no reachable `break` is an infinite loop (codegen
        // lowers it to Rust `loop`, which diverges). Any other while/for may be
        // skipped or exit, so it cannot guarantee the transfer.
        Stmt::While { cond, body, .. } => {
            matches!(cond, Expr::Bool(true, _)) && !body_has_reachable_break(body)
        }
        // A `match` covers all paths only when it is exhaustive (a wildcard or
        // bare-capture arm makes it total) AND every arm body transfers. When
        // exhaustiveness is uncertain, treat as falling through.
        Stmt::Match { arms, .. } => {
            arms.iter().any(|arm| {
                matches!(arm.pattern, MatchPattern::Wildcard | MatchPattern::Capture(_))
                    && arm.guard.is_none()
            }) && arms.iter().all(|arm| block_transfers_flow(&arm.body, count_loopctl))
        }
        // A `try` transfers on every path iff:
        //   (a) there IS a `finally` that transfers (it runs on every exit and
        //       itself diverges, so nothing after the try is reachable), OR
        //   (b) every `except` handler transfers AND the value path is covered:
        //       the try BODY transfers, OR there is an `else` that transfers (the
        //       `else` runs exactly when the body completed normally).
        // SOUND for the missing-return variant because the exception codegen
        // threads a try-body `return`/`break`/`continue` out of the catch_unwind
        // closure (see `Codegen::emit_try`), so no implicit `()` falls off the
        // end. EMPTY handlers make `handlers.all(..)` VACUOUSLY true, reducing the
        // rule to `body || else_` — exactly right for a handler-less `try/finally`.
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            if finally_.as_ref().is_some_and(|f| block_transfers_flow(f, count_loopctl)) {
                true
            } else {
                handlers.iter().all(|h| block_transfers_flow(&h.body, count_loopctl))
                    && (block_transfers_flow(body, count_loopctl)
                        || else_.as_ref().is_some_and(|e| block_transfers_flow(e, count_loopctl)))
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

/// (card c34ac64a, shape 1a) Whether `stmts` definitely transfers control AWAY
/// from the fall-through that follows the enclosing statement — a `return`,
/// `raise`, `break`, or `continue`, or a total `if`/`match`/`try`/`while True`
/// whose every path does so. This is [`block_definitely_returns`] WIDENED with
/// `break`/`continue` — the `count_loopctl` variant of the shared
/// [`block_transfers_flow`] core: those do not *return* (so they must not count
/// toward the missing-return analysis) but they DO terminate the branch's
/// fall-through, which is exactly what None-guard negative narrowing needs. For
/// `if x is None: <terminates>` the code AFTER the `if` is reached only when the
/// guard was false (`x is not None`), so `x` narrows to its inner payload there —
/// the early-return guard idiom.
pub(crate) fn block_terminates_flow(stmts: &[Stmt]) -> bool {
    block_transfers_flow(stmts, /*count_loopctl=*/ true)
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
            // (LAZY-GEN V1-d) Returning a generator where a `list[T]` is declared:
            // honest MATERIALIZE error (`list(g)`) before the bare mismatch.
            reject_iterator_into_list(&ty, &env.ret_ty, *span)?;
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
            // which lowers to `Ty::Iterator(T)` (LAZY-GEN V1-a). The yielded value
            // must match `T`.
            if let Ty::Iterator(elem) = &env.ret_ty {
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
                // (LAZY-GEN V1-d) A generator assigned into a `list[T]` slot:
                // honest MATERIALIZE error (`list(g)`) before the bare mismatch.
                reject_iterator_into_list(&val_ty, &explicit, *span)?;
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
            // (card c34ac64a fix B2c) A reassignment KILLS an active persistent
            // narrow on `target`. The post-assignment type must match what
            // codegen's reconverge emits: if the value reconverges into the
            // declared `Option<T>` (e.g. `x = None`, `cur = cur.next`) the var is
            // `Option<T>` again; otherwise it is a genuine type-changing rebind to
            // the value's type (e.g. narrowed `x = <Option[str]>`). Without this,
            // a read/guard after the reassignment saw the stale narrowed `T`.
            if let Some(opt) = env.narrowed.remove(target.as_str()) {
                let post = if option_slot_conflict(&opt, &val_ty) { declared } else { opt };
                env.locals.insert(target.clone(), post);
            } else {
                env.locals.insert(target.clone(), declared);
            }
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
            // (LAZY-GEN V1-d) A generator has no in-place augmented-assignment
            // form. `xs: list[int] = [..]; xs += gen(3)` otherwise slips past
            // `check` and leaks a raw rustc E0368 at `build` (Vec has no
            // `AddAssign<__PyrstGen>`). Give it the same honest MATERIALIZE
            // treatment as `Return`/annotated `Assign`: a generator RHS into a
            // list target (the reviewer's repro), and — symmetrically — a
            // generator TARGET (a generator cannot be the LHS of `+=`/`*=`/…).
            if let Some(target_ty) = env.locals.get(target.as_str()).cloned() {
                reject_iterator_into_list(&val_ty, &target_ty, *span)?;
                if matches!(target_ty, Ty::Iterator(_)) {
                    return Err(iterator_materialize_error(
                        "cannot be the target of an augmented assignment (`+=`, `*=`, …)",
                        "materialize it into a `list` local first",
                        *span,
                    ));
                }
            }
            Ok(())
        }
        Stmt::Unpack { targets, value, span } => {
            let val_ty = check_expr(value, env)?;
            // Generics v1: destructuring a bare type variable (`a, b = t` where
            // `t: T`) needs the value to have a known tuple SHAPE — a `T` is
            // opaque, so this is an honest error (it would otherwise emit a
            // tuple-pattern bind against an opaque `T` and fail rustc).
            reject_typevar_op(&val_ty, "unpack", *span)?;
            // (enabler-fix-2 #2) A statically-known tuple RHS whose ARITY differs
            // from the target count is a CHECK error — CPython raises a ValueError
            // at runtime, but pyrst knows the arity at compile time, so it leaked to
            // rustc (E0308) before. Names expected/got in CPython's own wording. An
            // EMPTY `Ty::Tuple` is the unknown-shape placeholder (e.g. `tuple(xs)`),
            // not a real 0-tuple, so it is exempt; a `list[T]` RHS is length-checked
            // at RUNTIME by codegen (`__py_unpack_list`), never here.
            if let Ty::Tuple(tys) = &val_ty {
                if !tys.is_empty() && tys.len() != targets.len() {
                    let detail = if tys.len() > targets.len() {
                        format!("too many values to unpack (expected {})", targets.len())
                    } else {
                        format!("not enough values to unpack (expected {}, got {})", targets.len(), tys.len())
                    };
                    return Err(Error::Type {
                        span: *span,
                        msg: format!(
                            "cannot unpack a {}-element tuple into {} name{}: {} \
                             (the tuple arity is statically known)",
                            tys.len(), targets.len(),
                            if targets.len() == 1 { "" } else { "s" }, detail
                        ),
                    });
                }
            }
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
            // (card c34ac64a fix B3) A None-guard on a name already narrowed to a
            // concrete `T` (or any concrete non-Optional local) is statically
            // decided — honest error instead of a leaked `.is_none()`-on-`T`.
            reject_decided_none_guard(cond, env)?;
            // (LAZY-GEN V1-d BLOCKER) Reject a bare local assigned incompatible
            // types across the sibling branches of this `if` — a silent miscompile
            // otherwise (codegen hoists one Rust slot and the divergent branch's
            // value is dropped at the join). Runs on the PRE-branch env (the helper
            // clones it), so it does not disturb the branch checks below.
            detect_branch_divergence(then, elifs, else_, env)?;
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
            // (card c34ac64a, shape 1a) NEGATIVE narrowing — the early-return guard
            // idiom. For `if x is None: <terminates>` with NO elif and NO else, the
            // only way control reaches the statements AFTER this `if` is the guard
            // being false, i.e. `x is not None`. So persistently narrow `x` to its
            // inner payload for the REST of the scope (unlike the branch narrowing
            // above, this does NOT restore). Restricted to the else-less, elif-less
            // shape: an `else`/`elif` that falls through (possibly reassigning `x`)
            // would make the post-`if` type depend on that path, which is unsound to
            // assume non-None. `is not None` + terminating-then leaves `x` as None
            // afterward — no useful narrowing — so only the `is None` sense applies.
            if let Some((name, is_not_none, inner)) = &guard {
                if !*is_not_none
                    && elifs.is_empty()
                    && else_.is_none()
                    && block_terminates_flow(then)
                {
                    // Record the DECLARED Option type so a later reassignment can
                    // re-widen (B2c) and a second guard on the still-narrowed name
                    // is caught (B3). `env.locals` holds the narrowed inner meanwhile.
                    let declared = env.locals.get(name.as_str()).cloned()
                        .unwrap_or_else(|| Ty::Option(Box::new(inner.clone())));
                    env.narrowed.insert(name.clone(), declared);
                    env.locals.insert(name.clone(), inner.clone());
                }
            }
            Ok(())
        }
        Stmt::While { cond, body, .. } => {
            let cond_ty = check_expr(cond, env)?;
            // Generics v1: a bare type variable as a loop condition (`while t:`)
            // needs truthiness — rejected (see the `if` arm).
            reject_typevar_op(&cond_ty, "use as a condition", cond.span())?;
            // (card c34ac64a fix B3) A None-guard header on a name already narrowed
            // to a concrete `T` (or any concrete non-Optional local) is statically
            // decided — honest error instead of a leaked `.is_none()`-on-`T`.
            reject_decided_none_guard(cond, env)?;
            // (card c34ac64a fix B1) Snapshot the pre-loop type view. A loop body
            // runs 0..n times, so NO narrowing the condition OR a body-nested
            // `if v is None: continue` establishes may be assumed AFTER the loop
            // (a leak was a rustc E0369 on a use of the var after the loop).
            // Function-level narrows established before the loop are in this
            // snapshot and thus preserved; `rewiden_loop_narrows` restores any
            // body-narrowed Optional at the loop edge.
            let pre_loop = env.locals.clone();
            // (card c34ac64a, shape 1c) WHILE-loop narrowing — the linked-list
            // traversal idiom `while cur is not None: ...; cur = cur.next`. The loop
            // body runs only when the guard is true, so narrow `cur` to its inner
            // payload (`T`) for the body. A loop-carried reassignment `cur = cur.next`
            // (value type `Option<T>`) restores the Optional in `env.locals` via the
            // normal `Stmt::Assign` path, so a read of `cur` AFTER the reassignment is
            // correctly Optional again. Only the `is not None` sense narrows (a
            // `while cur is None:` body would not deref `cur` as `T`).
            if let Some((name, true)) = extract_none_guard(cond) {
                if let Some(Ty::Option(inner)) = env.locals.get(name.as_str()) {
                    let inner = (**inner).clone();
                    env.locals.insert(name, inner);
                }
            }
            check_body(body, env)?;
            rewiden_loop_narrows(&pre_loop, env);
            Ok(())
        }
        Stmt::For { targets, iter, body, span } => {
            let iter_ty = check_expr(iter, env)?;
            // Generics v1: iterating a bare type variable (`for it in xs` where
            // `xs: T`) needs an `IntoIterator` bound — `T` is opaque, with no
            // `.iter()`. Reject it honestly (E0599 otherwise). Iterating a
            // `list[T]`/`dict[K, V]` whose ELEMENT is a type var is fine and
            // yields the element/key type below.
            reject_typevar_op(&iter_ty, "iterate over", *span)?;
            // (enabler-fix-2 #2) Iterating a fixed-shape TUPLE is an honest CHECK
            // error. A pyrst tuple lowers to a Rust tuple, which is NOT iterable
            // (the old `.into_iter()`/`.iter()` emission was a rustc E0599). DESIGN
            // CALL: tuples (esp. the `(str,str,str)` partition family) are meant to
            // be DESTRUCTURED, not iterated — direct them there rather than silently
            // building an array temp. An empty `Ty::Tuple` is the unknown-shape
            // placeholder, left permissive.
            if matches!(&iter_ty, Ty::Tuple(tys) if !tys.is_empty()) {
                return Err(Error::Type {
                    span: *span,
                    msg: "cannot iterate a tuple directly — destructure it \
                          (`a, b, c = t`) or convert with `list(t)` first"
                        .to_string(),
                });
            }
            // Determine element type from iterator type
            let elem_ty = match &iter_ty {
                // LAZY-GEN V1-a: a generator result (`Ty::Iterator`) yields the same
                // element type as a `list[T]` — treated identically for now.
                Ty::List(inner) | Ty::Iterator(inner) => *inner.clone(),
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
            // (card c34ac64a fix B1) Snapshot AFTER binding targets (so the loop
            // targets persist, matching Python) but BEFORE the body: a body-nested
            // `if v is None: continue` negative narrow must NOT leak past the loop
            // (the leak was a rustc E0369 on `v + 1` after the loop). The body runs
            // 0..n times, so `rewiden_loop_narrows` re-widens any Optional the body
            // narrowed down to its payload back to `Option<T>` at the loop edge.
            let pre_loop = env.locals.clone();
            check_body(body, env)?;
            rewiden_loop_narrows(&pre_loop, env);
            Ok(())
        }
        Stmt::Import { .. } => Ok(()), // Ignored in v0
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            // (LAZY-GEN V1-d BLOCKER) The `try` body and each `except` handler body
            // are SIBLING value-paths that codegen merges into one hoisted slot —
            // the same silent-drop hazard as `if`/`else`. A bare local assigned
            // divergent types across body-vs-handler is an honest CHECK error.
            // (`else`/`finally` run sequentially after the body, not as alternative
            // values, so they are excluded here — see `detect_sibling_divergence`.)
            let mut branches: Vec<&[Stmt]> = vec![body.as_slice()];
            for h in handlers { branches.push(&h.body); }
            detect_sibling_divergence(&branches, env, "the branches of this `try`/`except`")?;
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
            // (card ff3b4fa8) The context-manager protocol is NOT wired for user
            // classes: `with Guard(...) as g:` emits a plain `let mut g = Guard::new(...)`
            // and silently skips BOTH __enter__ and __exit__ (the body runs, the hooks
            // never fire) — a silent divergence from Python. Only a file handle from
            // `open(...)` (`Ty::File`, closed via RAII Drop) is a real context manager
            // in pyrst today. Full protocol support is blocked on real exception objects:
            // pyrst `raise` panics with a string-encoded type and no exception
            // value/traceback, so `__exit__(self, exc_type, exc_value, traceback)` cannot
            // receive Python-correct arguments on the raise path, nor honor suppression.
            // Reject honestly instead of miscompiling; see the (a) follow-up card.
            if !matches!(ctx_ty, Ty::File) {
                return Err(Error::Type {
                    span: ctx_expr.span(),
                    msg: "context-manager protocol (__enter__/__exit__) is not yet \
                           supported; only `with open(...) as f:` is a context manager \
                           in pyrst. Call the methods explicitly instead (e.g. \
                           `g = Guard(...)`, run the body, then `g.__exit__(...)`)"
                        .to_string(),
                });
            }
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
            // (LAZY-GEN V1-d) A generator cannot be the scrutinee of a `match`:
            // match codegen clones the subject (`let __match_val = g.clone();`),
            // and `__PyrstGen<T>` has no `Clone` — so `match g:` check-passes but
            // build-fails with a raw rustc E0599. Reject it honestly at `check`.
            if matches!(subject_ty, Ty::Iterator(_)) {
                return Err(Error::Type {
                    span: *span,
                    msg: "a generator cannot be matched (`match` clones and \
                          re-inspects its subject, which a lazy generator cannot \
                          do); materialize it with `list(...)` and match that, or \
                          iterate it with a `for` loop"
                        .to_string(),
                });
            }
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
            // (LAZY-GEN V1-d BLOCKER) The arm bodies are SIBLING value-paths merged
            // into one hoisted slot — the same divergent-join hazard as `if`/`else`.
            // A bare local assigned divergent types across arms is an honest CHECK
            // error (a divergent case otherwise leaks a raw rustc E0425 at build).
            let arm_branches: Vec<&[Stmt]> = arms.iter().map(|a| a.body.as_slice()).collect();
            detect_sibling_divergence(&arm_branches, env, "the arms of this `match`")?;
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

    // CAPTURE-A-GENERATOR gate (card 56e46767). A generator (`Ty::Iterator`) is
    // move-only AND NOT `Clone` (its Rust `Gen<T>` holds a coroutine future), and
    // iterating it MUTATES it. Captured by a `move` closure it becomes `FnMut`, so
    // the `Rc<dyn Fn>` cast codegen emits fails with a raw rustc E0525; a clone-on-
    // capture (how other non-Copy captures get their value snapshot) is impossible
    // here and would be semantically WRONG anyway — Python SHARES generator state,
    // it does not snapshot it. Reject honestly at check: materialize first with
    // `list(...)` and capture the list. Only ENCLOSING locals are policed (a nested
    // param of the same name SHADOWS the capture and is bound, so it is excluded by
    // `nested_def_captured_reads`); builtins / top-level fns are not `Iterator`
    // locals here.
    {
        let mut captured: std::collections::HashSet<String> = std::collections::HashSet::new();
        nested_def_captured_reads(f, &mut captured);
        for name in &captured {
            if matches!(env.locals.get(name), Some(Ty::Iterator(_))) {
                return Err(iterator_materialize_error(
                    &format!("cannot be captured by a nested function (`{}`)", name),
                    &format!("bind `{name}_items = list({name})` before the nested function and capture that"),
                    f.span,
                ));
            }
        }

        // CAPTURE-A-BY-REF-PARAM gate (card 56e46767, review comment 182). A
        // `Mut[T]` by-reference parameter lowers to a Rust `&mut T`. Unlike an
        // ordinary non-Copy capture (which codegen SNAPSHOTS by clone), an `&mut`
        // must NOT be cloned on capture — that would silently snapshot a
        // by-REFERENCE parameter, dropping the aliasing that is its entire purpose —
        // so codegen deliberately excludes `by_ref_locals` from clone-on-capture and
        // MOVES the `&mut` into the closure instead. That leaves any later use of the
        // param in the enclosing scope (a further read, an in-place mutation, or
        // passing it on) a raw rustc E0382 ("borrow of moved value"), and a captured
        // `&mut` also cannot outlive the frame if the closure escapes — neither of
        // which is a clean pyrst diagnostic. Reject the capture honestly at check
        // (mirroring the generator gate above), so `check` and `build` agree. The
        // fix is to snapshot the referent into a value LOCAL first and capture that
        // (verified: `local = ds` clones the `&mut`'s target into an owned value the
        // closure then clone-captures, leaving the `&mut` param usable afterward).
        for name in &captured {
            if env.by_ref_params.contains(name) {
                return Err(Error::Type {
                    span: f.span,
                    msg: format!(
                        "the `Mut[T]` by-reference parameter `{name}` cannot be \
                         captured by a nested function (a by-reference binding cannot \
                         be moved into a closure); copy it into a local first \
                         (`{name}_local = {name}`) and capture that",
                    ),
                });
            }
        }
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
    // (fix-b) Snapshot before check_body mutates locals (see check_one_func).
    let entry_env = nested_env.clone();
    check_body(&f.body, &mut nested_env)?;
    check_all_paths_return(&f.body, &nested_env, &f.name, f.span)?;
    detect_read_after_conflicting_reassign(&f.body, &entry_env)?;
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
    // (card 49170944) casefold/rsplit/translate are now emittable (codegen wired).
    // casefold is SIMPLE-casefold (Unicode `to_lowercase`): it matches CPython's
    // str.casefold for ASCII / İ / Σ but NOT for full-fold chars (ß stays ß, not
    // "ss"; ﬁ stays ﬁ, not "fi") — see PYTHON_COMPATIBILITY.md.
    "casefold", "rsplit", "translate",
    // NOTE: encode/isdecimal/format stay removed — codegen cannot emit them
    // (card 36f66dd2).
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
            | "expandtabs" | "join"
            // (card 49170944) casefold -> str (simple-casefold, see STR_METHODS
            // note); translate -> str (applies an int->int code-point map).
            | "casefold" | "translate" => Ty::Str,
            // NOTE: encode/format removed from str arms — codegen cannot emit
            // them (card 36f66dd2 stopgap).
            // (card 49170944) rsplit joins split/splitlines as list[str]; but
            // partition/rpartition return a 3-TUPLE (str, str, str) — CPython's
            // real shape — so `head, sep, tail = s.partition("=")` unpacks (and
            // repr matches). This diverged from CPython as a `list` before.
            "split" | "splitlines" | "rsplit" => Ty::List(Box::new(Ty::Str)),
            "partition" | "rpartition" => {
                Ty::Tuple(vec![Ty::Str, Ty::Str, Ty::Str])
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

