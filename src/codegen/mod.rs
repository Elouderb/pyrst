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
    /// Generators (LAZY-GEN V1-b): true while emitting a function whose body
    /// contains `yield`. Such a function lowers to the lazy `Gen<T>` coroutine
    /// (docs/design/lazy-generators.md §C): its body is wrapped in a
    /// `Box::pin(async move { .. })` future driven by the GEN_PRELUDE, `yield x`
    /// becomes `__pyrst_gen_co.yield_(x).await`, and a bare `return` completes the
    /// future (end of iteration). The coroutine locals live under the reserved
    /// `__pyrst_` prefix (typeck's `reject_if_reserved` blocks user identifiers
    /// there) so they can never be shadowed by a user local. Saved/restored per
    /// function like `current_ret_ty` so a nested `def` never inherits it. Unlike
    /// the retired eager desugar (which collected into a `Vec<T>` and hung on an
    /// infinite generator), nothing runs until the first `next()` and an infinite
    /// `while True: yield ...` consumed with `break` is O(1) and terminates.
    in_generator: bool,
    /// (try/except control flow) True while emitting the BODY of a `try:` —
    /// the statements that run inside the `catch_unwind` closure. Because that
    /// body is a Rust closure, a plain `return` would return from the CLOSURE
    /// (silently dropping the value) rather than the enclosing function. When
    /// this flag is set, a `return <v>` is lowered to
    /// `return __PyrstTryFlow::Return(<v>);` so the value is threaded OUT of the
    /// closure and re-issued as a real function `return` after the try lowering
    /// (and after any `finally`). It STAYS set through a nested loop body (a
    /// `return` inside an inner loop still escapes the function), and is
    /// suspended only inside a nested `def` (saved/restored by `emit_func`),
    /// whose `return` is local to that function. Saved/restored around the try
    /// body in `emit_try`.
    try_return_escape: bool,
    /// (try/except control flow) True while emitting the BODY of a `try:` AT THE
    /// TRY-BODY LOOP LEVEL — i.e. a `break`/`continue` here targets the loop
    /// ENCLOSING the try, so it must thread out of the catch_unwind closure as
    /// `return __PyrstTryFlow::Break;` / `::Continue;`. Unlike `try_return_escape`
    /// this is SUSPENDED inside a nested `while`/`for` body (where a
    /// break/continue targets that inner loop and is a real Rust break/continue)
    /// as well as inside a nested `def`. Saved/restored around the try body, the
    /// nested-loop bodies, and (via `emit_func`) nested functions.
    try_loopctl_escape: bool,
    /// Generics v2 (generic CLASSES): the type parameters of the class whose
    /// `impl<T, ..> Box<T> { .. }` block is CURRENTLY being emitted (`["T"]` for
    /// `Box`). A method emitted inside that block references `T` from the IMPL
    /// header, so its own param/return annotations must lower with these names in
    /// scope (a `v: T` becomes the Rust `T`) but must NOT re-declare them as a
    /// per-method generic clause. `emit_func` threads this set into
    /// `from_type_expr_scoped` and suppresses the per-method clause when the
    /// method has no type params of its own. EMPTY everywhere except inside a
    /// generic class's impl block (saved/restored around it), so non-generic
    /// classes and free functions are byte-for-byte unchanged.
    current_class_type_params: Vec<String>,
    /// Generics: the type parameters in scope for the FREE FUNCTION (or method)
    /// whose body is CURRENTLY being emitted — `["T"]` inside `apply_twice[T]`,
    /// `["A", "B"]` inside `swap[A, B]`. A local annotation in the body (`acc: T`)
    /// must lower with these names in scope so `T` becomes `Ty::TypeVar("T")`
    /// (matching the param/return lowering and the call-result oracle), NOT a
    /// `Ty::Class("T")`. Without this, the reassignment `acc = f(...)` sees a type
    /// CHANGE (`Class("T")` vs the call result `TypeVar("T")`) and wrongly emits a
    /// shadowing `let` instead of a mutation — silently breaking a running fold.
    /// Saved/restored around each `emit_func` body (so a nested def / sibling fn
    /// never inherits it); EMPTY for non-generic functions.
    current_fn_type_params: Vec<String>,
}


mod analysis;
mod items;
mod stmts;
mod exprs;
#[cfg(test)]
mod tests;

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

/// Whether `stmts` contains a `return` that, inside a `try:` body, must escape
/// the catch_unwind closure as a function return. Unlike loop-control, a
/// `return` inside a NESTED `while`/`for` STILL escapes the function (loops do
/// not capture returns), so those ARE descended; only a nested `def`/`class`
/// (which owns its own returns) is not. Used by `emit_try` to decide whether to
/// declare the flow holder and emit the `Return(v) => return v` arm.
fn body_has_try_level_return(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_has_try_level_return)
}

fn stmt_has_try_level_return(s: &Stmt) -> bool {
    match s {
        Stmt::Return(..) => true,
        Stmt::If { then, elifs, else_, .. } => {
            body_has_try_level_return(then)
                || elifs.iter().any(|(_, b)| body_has_try_level_return(b))
                || else_.as_ref().is_some_and(|b| body_has_try_level_return(b))
        }
        Stmt::Match { arms, .. } => arms.iter().any(|arm| body_has_try_level_return(&arm.body)),
        Stmt::With { body, .. } => body_has_try_level_return(body),
        // A `return` inside an inner loop still leaves the function.
        Stmt::While { body, .. } | Stmt::For { body, .. } => body_has_try_level_return(body),
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            body_has_try_level_return(body)
                || handlers.iter().any(|h| body_has_try_level_return(&h.body))
                || else_.as_ref().is_some_and(|b| body_has_try_level_return(b))
                || finally_.as_ref().is_some_and(|b| body_has_try_level_return(b))
        }
        // A nested function/class owns its own returns.
        Stmt::Func(_) | Stmt::Class(_) => false,
        _ => false,
    }
}

/// Whether `stmts` contains a `break` (when `want_break`) / `continue` (when
/// `!want_break`) that, at the TRY-BODY level, would target the loop ENCLOSING
/// the `try:` — i.e. an escaping loop-control statement that `emit_try` must
/// re-issue after the try lowering. The descent rule mirrors codegen's
/// don't-descend handling and typeck's `body_has_reachable_break`: `if` / `match`
/// / `with` / nested `try` blocks ARE descended (a break/continue inside them
/// still escapes the try body), but an inner `while`/`for` is NOT descended (its
/// break/continue targets that inner loop) and a nested `def`/`class` owns its
/// own control flow. Used to decide whether `emit_try`'s post-lowering flow
/// `match` emits a real `break`/`continue` arm — emitting one only when the body
/// can actually produce that signal keeps a `break` out of a try that is not
/// inside a loop (which would be an honest rustc E0268, preserved) from turning
/// every loop-free try into a spurious build failure.
fn try_body_has_loopctl(stmts: &[Stmt], want_break: bool) -> bool {
    stmts.iter().any(|s| stmt_has_loopctl(s, want_break))
}

fn stmt_has_loopctl(s: &Stmt, want_break: bool) -> bool {
    match s {
        Stmt::Break(_) => want_break,
        Stmt::Continue(_) => !want_break,
        Stmt::If { then, elifs, else_, .. } => {
            try_body_has_loopctl(then, want_break)
                || elifs.iter().any(|(_, b)| try_body_has_loopctl(b, want_break))
                || else_.as_ref().is_some_and(|b| try_body_has_loopctl(b, want_break))
        }
        Stmt::Match { arms, .. } => {
            arms.iter().any(|arm| try_body_has_loopctl(&arm.body, want_break))
        }
        Stmt::With { body, .. } => try_body_has_loopctl(body, want_break),
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            try_body_has_loopctl(body, want_break)
                || handlers.iter().any(|h| try_body_has_loopctl(&h.body, want_break))
                || else_.as_ref().is_some_and(|b| try_body_has_loopctl(b, want_break))
                || finally_.as_ref().is_some_and(|b| try_body_has_loopctl(b, want_break))
        }
        // An inner loop captures its own break/continue; do not descend.
        Stmt::While { .. } | Stmt::For { .. } => false,
        // A nested function/class owns its own control flow.
        Stmt::Func(_) | Stmt::Class(_) => false,
        _ => false,
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
/// errors panic with the `OSError\0<msg>` payload convention, so `except
/// OSError:` catches them (exact-name match; OSError is not in the builtin
/// descendant hierarchy). readlines() strips line endings (a documented
/// deviation from CPython, which keeps them).
const FILE_PRELUDE: &str = r#"struct PyFile { inner: std::fs::File }
impl PyFile {
    fn read(&mut self) -> String {
        use std::io::Read;
        let mut s = String::new();
        self.inner.read_to_string(&mut s).unwrap_or_else(|e| panic!("OSError\0read failed: {}", e));
        s
    }
    fn readlines(&mut self) -> Vec<String> {
        self.read().lines().map(|l| l.to_string()).collect()
    }
    fn write(&mut self, s: &str) {
        use std::io::Write;
        self.inner.write_all(s.as_bytes()).unwrap_or_else(|e| panic!("OSError\0write failed: {}", e));
    }
    fn close(&mut self) {}
}
fn __py_open(path: &str, mode: &str) -> PyFile {
    let f = match mode {
        "w" => std::fs::File::create(path).unwrap_or_else(|e| panic!("OSError\0open failed: {}: {}", path, e)),
        "a" => std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap_or_else(|e| panic!("OSError\0open failed: {}: {}", path, e)),
        _ => std::fs::File::open(path).unwrap_or_else(|e| panic!("OSError\0open failed: {}: {}", path, e)),
    };
    PyFile { inner: f }
}
"#;

/// (LAZY-GEN V1-b) The lazy-generator runtime. A pyrst generator — a `def` whose
/// body `yield`s, declared `-> Iterator[T]` — lowers to a `Gen<T>`: its body
/// becomes an `async move { .. }` coroutine, `yield x` becomes
/// `__pyrst_gen_co.yield_(x).await`, and `Gen`'s `Iterator` impl drives that
/// coroutine one yield at a time with a no-op waker. Nothing runs until the first
/// `next()` (Python-exact laziness), an infinite generator is O(1) and terminates
/// when its consumer stops, and `T` need not be `Send` (single-threaded `Rc`,
/// matching pyrst's `Rc`-holding values). `Gen<T>` is a concrete, nameable struct
/// (a boxed `dyn Future` inside) — NOT `impl Iterator` — so a generator can be
/// stored, passed, and returned like any other value (`rust_ty` emits `Gen<T>`
/// uniformly). Emitted once per program, like REPR_PRELUDE/FILE_PRELUDE, and
/// covered by the crate-level `#![allow(dead_code, ...)]` when a program defines
/// no generator. Prototype-validated: docs/design/lazy-generators.md §C.1. Every
/// `std::` path (incl. `Box`, `Option`, `Some`, `None`, and the `Iterator` impl)
/// is spelled out in full rather than `use`-imported, matching the other preludes'
/// namespace-safe convention: a pyrst `class Box`/`Option`/`Iterator` emits a
/// same-named `struct`/type that would otherwise shadow the std name here (the
/// corpus has `class Box`). The logic is byte-identical to the validated
/// prototype. `Waker::noop()` is stable since rustc 1.85.
const GEN_PRELUDE: &str = r#"struct YieldNow { done: bool }
impl std::future::Future for YieldNow {
    type Output = ();
    fn poll(mut self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        if self.done { std::task::Poll::Ready(()) } else { self.done = true; std::task::Poll::Pending }
    }
}
struct Co<T> { slot: std::rc::Rc<std::cell::RefCell<std::option::Option<T>>> }
impl<T> Co<T> {
    fn yield_(&self, v: T) -> YieldNow {
        *self.slot.borrow_mut() = std::option::Option::Some(v);
        YieldNow { done: false }
    }
}
struct Gen<T> {
    fut: std::pin::Pin<std::boxed::Box<dyn std::future::Future<Output = ()>>>,
    slot: std::rc::Rc<std::cell::RefCell<std::option::Option<T>>>,
    done: bool,
}
impl<T> Gen<T> {
    fn empty() -> Gen<T> {
        Gen {
            fut: std::boxed::Box::pin(async {}),
            slot: std::rc::Rc::new(std::cell::RefCell::new(std::option::Option::None)),
            done: false,
        }
    }
}
impl<T> std::iter::Iterator for Gen<T> {
    type Item = T;
    fn next(&mut self) -> std::option::Option<T> {
        // FUSED: polling a completed future is a contract violation ("resumed
        // after completion" panic). Python iterates an exhausted generator as
        // empty forever, so next() on a done Gen returns None forever.
        if self.done {
            return std::option::Option::None;
        }
        let __waker = std::task::Waker::noop();
        let mut __cx = std::task::Context::from_waker(__waker);
        match self.fut.as_mut().poll(&mut __cx) {
            std::task::Poll::Ready(()) => {
                self.done = true;
                std::option::Option::None
            }
            std::task::Poll::Pending => self.slot.borrow_mut().take(),
        }
    }
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
    // `unreachable_code` is allowed because the try/except control-flow lowering
    // emits a `__PyrstTryFlow::Normal` sentinel as the catch_unwind closure's
    // tail; when the try body always returns/raises (e.g. `try: return 7`), that
    // sentinel is legitimately unreachable. It is a structural artifact of the
    // generated wrapper, not a user mistake — the same reason `dead_code` /
    // `unused_variables` are already suppressed for generated code.
    // `unused_assignments` (card adc0d1c4): when 2+ locals are hoisted, the sorted
    // decl preamble separates all but the last-sorted var's default decl from its
    // first reassignment, so `try_fold_hoisted_init`'s adjacency fold cannot fire
    // and those keep `let mut x: T = <default>; x = <init>;`. The dead default is a
    // generated-code artifact (never a user bug), suppressed here uniformly for all
    // hoist orderings — see the note on `try_fold_hoisted_init`.
    cg.line("#![allow(unused_parens, unused_variables, unused_mut, dead_code, unused_imports, non_upper_case_globals, non_camel_case_types, unreachable_code, unused_assignments)]");
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
    cg.line("    if exp < 0 { panic!(\"ValueError\\0negative exponent for integer ** integer\"); }");
    cg.line("    base.pow(exp as u32)");
    cg.line("}");
    // List index/slice reads. The BASE is passed by shared reference (`&[T]`) so
    // only the returned ELEMENT is cloned — an indexed loop is O(n) instead of
    // the O(n^2) that cloning the whole container per access produced. Value
    // semantics are unchanged (the element is still deep-`.clone()`d). Callers
    // borrow the base (`__py_list_get(&xs, i)`) only when it is a genuine place
    // and the index cannot mutate it; otherwise emit_expr falls back to an inline
    // snapshot-clone form (see the Index/Slice arms). Bodies mirror that inline
    // form character-for-character, incl. the byte-identical IndexError payload.
    cg.line("fn __py_list_get<T: Clone>(__list: &[T], __i_idx: i64) -> T {");
    cg.line("    let __idx = if __i_idx < 0 { ((__list.len() as i64) + __i_idx) as usize } else { __i_idx as usize };");
    cg.line("    if __idx >= __list.len() { panic!(\"IndexError\\0list index out of range\") }");
    cg.line("    __list[__idx].clone()");
    cg.line("}");
    // Simple (step-1) list slice. Resolves a negative bound by +len then clamps
    // to [0, len] (CPython semantics for step>0), and yields the empty slice when
    // the clamped stop is not past the clamped start — so out-of-range bounds
    // (`xs[-100:2]`, `xs[10:2]`) clamp instead of panicking on a usize underflow.
    cg.line("fn __py_list_slice<T: Clone>(__list: &[T], __start_in: i64, __stop_in: i64) -> Vec<T> {");
    cg.line("    let __len = __list.len() as i64;");
    cg.line("    let mut __start = if __start_in < 0 { __start_in + __len } else { __start_in };");
    cg.line("    if __start < 0 { __start = 0; } else if __start > __len { __start = __len; }");
    cg.line("    let mut __stop = if __stop_in < 0 { __stop_in + __len } else { __stop_in };");
    cg.line("    if __stop < 0 { __stop = 0; } else if __stop > __len { __stop = __len; }");
    cg.line("    if __stop > __start { __list[(__start as usize)..(__stop as usize)].to_vec() } else { Vec::new() }");
    cg.line("}");
    // Stepped list slice, faithful to CPython PySlice_AdjustIndices for BOTH step
    // signs. Absent bounds arrive as `None` so the step-sign-dependent default is
    // applied at RUNTIME (step may be a runtime expr): start None -> len-1 (step<0)
    // / 0 (step>0); stop None -> -1 (step<0) / len (step>0). A present bound is
    // resolved by +len then clamped to [lower,upper] = [-1,len-1] for step<0 /
    // [0,len] for step>0. Iteration is direction-aware: i=start while i<stop
    // (step>0) or i>stop (step<0). The fallback (rvalue-base) call sites clone the
    // base into a local and call this same fn, so borrow and fallback agree.
    cg.line("fn __py_list_slice_step<T: Clone>(__list: &[T], __start_in: Option<i64>, __stop_in: Option<i64>, __step: i64) -> Vec<T> {");
    cg.line("    if __step == 0 { panic!(\"ValueError\\0slice step cannot be zero\"); }");
    cg.line("    let __len = __list.len() as i64;");
    cg.line("    let __lower = if __step < 0 { -1i64 } else { 0i64 };");
    cg.line("    let __upper = if __step < 0 { __len - 1 } else { __len };");
    cg.line("    let __start = match __start_in { None => if __step < 0 { __len - 1 } else { 0 }, Some(__v) => { let __v = if __v < 0 { __v + __len } else { __v }; if __v < __lower { __lower } else if __v > __upper { __upper } else { __v } } };");
    cg.line("    let __stop = match __stop_in { None => if __step < 0 { -1 } else { __len }, Some(__v) => { let __v = if __v < 0 { __v + __len } else { __v }; if __v < __lower { __lower } else if __v > __upper { __upper } else { __v } } };");
    cg.line("    let mut __result: Vec<T> = Vec::new();");
    cg.line("    let mut __i = __start;");
    cg.line("    if __step > 0 { while __i < __stop { __result.push(__list[__i as usize].clone()); __i += __step; } }");
    cg.line("    else { while __i > __stop { __result.push(__list[__i as usize].clone()); __i += __step; } }");
    cg.line("    __result");
    cg.line("}");
    // Stepped STRING slice — the char-based twin of __py_list_slice_step (Python
    // slices strings by code point, not UTF-8 byte). Same PySlice_AdjustIndices
    // logic and Option-encoded absent bounds. Also serves the simple (step-1)
    // string slice, so every string slice is char-correct and out-of-range-safe.
    cg.line("fn __py_str_slice_step(__s: &str, __start_in: Option<i64>, __stop_in: Option<i64>, __step: i64) -> String {");
    cg.line("    if __step == 0 { panic!(\"ValueError\\0slice step cannot be zero\"); }");
    cg.line("    let __chars: Vec<char> = __s.chars().collect();");
    cg.line("    let __len = __chars.len() as i64;");
    cg.line("    let __lower = if __step < 0 { -1i64 } else { 0i64 };");
    cg.line("    let __upper = if __step < 0 { __len - 1 } else { __len };");
    cg.line("    let __start = match __start_in { None => if __step < 0 { __len - 1 } else { 0 }, Some(__v) => { let __v = if __v < 0 { __v + __len } else { __v }; if __v < __lower { __lower } else if __v > __upper { __upper } else { __v } } };");
    cg.line("    let __stop = match __stop_in { None => if __step < 0 { -1 } else { __len }, Some(__v) => { let __v = if __v < 0 { __v + __len } else { __v }; if __v < __lower { __lower } else if __v > __upper { __upper } else { __v } } };");
    cg.line("    let mut __result = String::new();");
    cg.line("    let mut __i = __start;");
    cg.line("    if __step > 0 { while __i < __stop { __result.push(__chars[__i as usize]); __i += __step; } }");
    cg.line("    else { while __i > __stop { __result.push(__chars[__i as usize]); __i += __step; } }");
    cg.line("    __result");
    cg.line("}");
    // (try/except control flow) Signal a try BODY's escaping control flow out of
    // the catch_unwind closure: `Return(R)` carries the enclosing function's
    // return value, `Break`/`Continue` re-target the loop enclosing the try, and
    // `Normal` means the body fell through (so `else` runs). See `emit_try`.
    cg.line("enum __PyrstTryFlow<R> { Normal, Return(R), Break, Continue }");
    cg.line(REPR_PRELUDE);
    cg.line(FILE_PRELUDE);
    // (LAZY-GEN V1-b) The lazy-generator runtime (Gen<T>/Co<T>/YieldNow).
    // Emitted unconditionally like the preludes above; dead when a program has
    // no generator (covered by the crate-level `#![allow(dead_code)]`).
    cg.line(GEN_PRELUDE);
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

