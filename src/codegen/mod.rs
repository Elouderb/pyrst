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
    "__eq__", "__neg__", "__lt__",
    // NOTE: `__bool__` is deliberately NOT here (card 18682938). It has no Rust
    // trait counterpart for truthiness; instead it is emitted as an ordinary
    // inherent method `fn __bool__(&self) -> bool` and CALLED at each bool-context
    // site via `emit_truthy` (if/while/bool()/not/assert/and/or).
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
    ///
    /// (W3-fix / F8) OWNER-KEYED `(owner_module, const_name)` (`None` = root), so a
    /// bare `FOO` in a module that OWNS a function `FOO` is NOT mistaken for a
    /// same-named CONST owned by a co-imported module (fn-vs-const misroute). The
    /// owner at a reference site comes from `bare_owner_for` (bare) / the qualifier
    /// (`X.CONST`) — the SAME resolution the mangled name is built from, so
    /// membership and mangling never disagree.
    const_names: std::collections::HashSet<(Option<String>, String)>,
    /// Names of module-level STRING constants (a subset of `const_names`). A str
    /// const lowers to a Rust `const NAME: &str` (a `String` is not
    /// const-constructible), so a reference to it must additionally append
    /// `.to_string()` to recover pyrst's `str == Rust String` value type.
    /// int/float/bool consts are `Copy` and need no such fix-up.
    ///
    /// (W3-fix / F9) OWNER-KEYED like `const_names`, so a `.to_string()` fix-up
    /// fires per (owner, name): a str const `FOO` in one module and an int const
    /// `FOO` in another no longer cross-contaminate the str-ness decision.
    const_strs: std::collections::HashSet<(Option<String>, String)>,
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
    /// (card 602b1675 / 575bcf3a) Names given a FUNCTION-SCOPE hoist slot
    /// (`let mut x: T = <default>` in the body preamble). Populated by the hoist
    /// loop in `emit_func` / `emit_nested_def`. A divergent shadow of a HOISTED
    /// local inside a nested block must be emitted under a MANGLED name (so it
    /// does not hide the function-scope slot by name and a later reconverging
    /// assign can still reach the slot — see `shadow_map`). A non-hoisted local's
    /// block shadow keeps its own name (a function-scope `let` shadow works). Empty
    /// until the hoist loop runs; saved/restored around a nested `def`.
    hoisted: std::collections::HashSet<String>,
    /// (card 575bcf3a, poison2) ACTIVE divergent shadows of hoisted locals in the
    /// CURRENT scope: pyrst name -> (mangled Rust binding, the hoisted SLOT type).
    /// When a hoisted local is reassigned to a type that conflicts with its slot
    /// inside a block, the shadow is emitted as `let mut <mangled> = ..` and this
    /// records the redirect; `emit_expr`'s Ident arm resolves reads of the name to
    /// `<mangled>` while the shadow is live. A later reassign whose value type
    /// RECONVERGES to the slot type writes the ORIGINAL name (the slot) and clears
    /// the entry — so `xs = gen(3); xs = list(xs)` inside a branch materializes into
    /// the hoisted slot instead of a discarded block-local shadow. Saved/restored
    /// (with locals + declared) around every child block so a shadow never leaks
    /// past its block; empty in the overwhelmingly common shadow-free case.
    shadow_map: std::collections::HashMap<String, (String, Ty)>,
    /// (card 575bcf3a) Monotonic counter making each mangled shadow name unique
    /// and DETERMINISTIC (`__pyrst_shadow_<name>_<n>`). Reset to 0 at the start of
    /// each `emit_func` / `emit_nested_def` body and saved/restored around a nested
    /// def, so emission is byte-stable across runs.
    shadow_counter: usize,
    /// (card c34ac64a fix A) Param-type HINTS for an INLINE-INVOKED lambda
    /// `(lambda a, b: ...)(x, y)`: `emit_callee_with_inline_lambda_types` pins each
    /// bare param's type from the corresponding call ARGUMENT and stashes it here
    /// just before emitting the lambda callee. The Lambda arm `.take()`s these and
    /// applies them to its OWN params — so a bare `Str`-arg param routes `+`
    /// through `format!` — instead of removing the param (its outer-bleed guard).
    /// `None` everywhere except across that single inline-invoke emit; a nested
    /// lambda inside the body sees `None` (the `.take()` consumed it).
    pending_inline_lambda_params: Option<std::collections::HashMap<String, Ty>>,
    /// (W3-2) The dotted module id of the module whose top-level items are
    /// CURRENTLY being emitted (`None` = the ROOT program, whose own top-level
    /// names stay crate-root-unwrapped). Threaded by `emit_program`'s const
    /// prepass + main emit loop from each `Module.module_id`; consulted by
    /// `emit_func` / `emit_const_decl` (the DEFINITION owner) and `bare_owner_for`
    /// (a SAME-MODULE unqualified reference). Like `current_class`, it scopes a
    /// single emission pass and is `None` on every non-`emit_program` path (a
    /// directly built `Codegen` emits a single root unit — root-unwrapped).
    current_module: Option<String>,
    /// (W3-2) The ROOT program's OWN top-level fn/class/const bare names. A bare
    /// reference at the root resolves to the root's own definition FIRST
    /// (root-shadows-imports — matching typeck's flat last-writer merge), so this
    /// guards `bare_owner_for` from mangling a root name that ALSO happens to be
    /// from-imported (a redefinition CPython would last-bind). Empty except during
    /// `emit_program`; irrelevant for import-free programs (every bare name is
    /// root-owned and unwrapped regardless).
    root_defined: std::collections::HashSet<String>,
    /// (W4-a) Names declared `global` in the function CURRENTLY being emitted (set
    /// from the function body in `emit_func`/`emit_nested_def`, saved/restored
    /// around a nested def). A `global`-declared name that is also a promoted
    /// mutable global is REBOUND (`=` / `op=`) to the module static rather than a
    /// function-local; a rebind WITHOUT `global` stays a local shadow (Python's
    /// exact rule). READs and in-place MUTATIONS of a global need no `global`
    /// declaration, so they consult `ctx.mutable_globals` directly, not this set.
    /// Empty for every function with no `global` statement.
    fn_globals: std::collections::HashSet<String>,
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

/// (W3-3) Sanitize a dotted MODULE ID into the identifier fragment embedded in a
/// mangled top-level name (`__pyrst_m_<frag>__<name>` /
/// `__pyrst_const_<frag>__<name>`). Module ids are Python identifiers joined by
/// `.` — each component matches `[A-Za-z_][A-Za-z0-9_]*`, so a component MAY
/// itself contain `_`. The naive `owner.replace('.', "_")` is therefore NOT
/// injective: the dotted id `a.b` and a hypothetical flat module literally named
/// `a_b` both map to `a_b`, so their `basename`s would mangle to the SAME
/// `__pyrst_m_a_b__basename` (a silent def/use cross-wire). We instead escape so
/// the mapping is INJECTIVE: every literal `_` → `_u`, every `.` separator →
/// `_d`. After escaping, a `_` only ever appears as the first char of a `_u`/`_d`
/// digraph, so the encoding is prefix-free / uniquely decodable and distinct
/// module ids always produce distinct fragments (`a.b` → `a_db`, flat `a_b` →
/// `a_ub`; `a_.b` → `a_u_db`, `a._b` → `a_d_ub` — no boundary collision). A
/// single-component id with NO underscore is unchanged (`os` → `os`, `re` →
/// `re`), so every existing single-module mangled name is byte-identical; only
/// dotted ids and underscore-bearing names shift to the escaped form. The result
/// is always a valid Rust identifier fragment (never a keyword, given the
/// `__pyrst_m_`/`__pyrst_const_` prefix).
pub(crate) fn mangle_mod_id(owner: &str) -> String {
    let mut s = String::with_capacity(owner.len() + 2);
    for ch in owner.chars() {
        match ch {
            '_' => s.push_str("_u"),
            '.' => s.push_str("_d"),
            c => s.push(c),
        }
    }
    s
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
///
/// (W3-2) OWNER-QUALIFIED: a ROOT const (`owner = None`) keeps the historical
/// `__pyrst_const_<name>` (so import-free programs are byte-identical), while an
/// IMPORTED module's const gains its owner prefix `__pyrst_const_<owner>__<name>`
/// (the dotted module id sanitized collision-proof via [`mangle_mod_id`]). This
/// closes the previously latent const-vs-const collision (two co-imported modules
/// each defining a same-named const now emit DISTINCT Rust consts). Applied
/// identically at the const definition (`emit_const_decl`, owner = the emitting
/// module) and at every reference (bare `CONST` owner-resolved via
/// `bare_owner_for`; qualified `X.CONST` owner = `X`).
pub fn mangle_const(owner: Option<&str>, name: &str) -> String {
    match owner {
        None => format!("__pyrst_const_{}", name),
        Some(m) => format!("__pyrst_const_{}__{}", mangle_mod_id(m), name),
    }
}

/// (W4-a) Mangle a MODULE-LEVEL MUTABLE GLOBAL's pyrst name into the Rust
/// `thread_local!` static identifier emitted for it. A DISTINCT namespace from
/// `mangle_const` (`__pyrst_g_` vs `__pyrst_const_`) so a promoted global and a
/// hypothetical same-named const never collide, and — like `mangle_const` — the
/// reserved prefix keeps a lowercase global name (`counter`, `argv`) from being
/// captured as a Rust const-pattern. OWNER-QUALIFIED exactly like a const: a ROOT
/// global (`owner = None`) keeps `__pyrst_g_<name>` (import-free output stays
/// self-contained), while an imported module's global gains its collision-proof
/// owner prefix `__pyrst_g_<mangle_mod_id(owner)>__<name>`. Applied identically at
/// the static definition (`emit_program`'s globals prepass) and at every
/// read / rebind / mutation site; a missed site is a def/use mismatch.
pub fn mangle_global(owner: Option<&str>, name: &str) -> String {
    match owner {
        None => format!("__pyrst_g_{}", name),
        Some(m) => format!("__pyrst_g_{}__{}", mangle_mod_id(m), name),
    }
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

/// upstream as the catch-all `true` arm). Each base's FULL transitive closure
/// (base + every subclass, any depth) is written out directly, against the real
/// CPython builtin hierarchy — verified with `issubclass` (card c1fec2bf).
/// `except <base>` OR-expands over this set so it catches every subclass pyrst
/// (or an embedded stdlib module) can raise, exactly like CPython.
fn exc_descendants(base: &str) -> Vec<&'static str> {
    // (dedupe, phase2-fix2) The OSError -> ConnectionError subfamily, written
    // ONCE and shared by both the `OSError` (full transitive closure) and the
    // `ConnectionError` (its own subtree) arms below, so the five connection
    // classes can never drift out of sync between the two tables.
    const CONNECTION_FAMILY: [&str; 5] = [
        "ConnectionError", "BrokenPipeError", "ConnectionAbortedError",
        "ConnectionRefusedError", "ConnectionResetError",
    ];
    match base {
        "ArithmeticError" => vec![
            "ArithmeticError", "ZeroDivisionError", "OverflowError", "FloatingPointError",
        ],
        "LookupError" => vec!["LookupError", "IndexError", "KeyError"],
        "RuntimeError" => vec!["RuntimeError", "RecursionError", "NotImplementedError"],
        "NameError" => vec!["NameError", "UnboundLocalError"],
        // OSError family (CPython `issubclass(X, OSError)` ground truth). The
        // file-system subclasses `FileExistsError` / `NotADirectoryError` /
        // `SameFileError` (the last being `shutil.SameFileError`, a real OSError
        // subclass) were MISSING, so `except OSError:` silently skipped them and
        // the panic propagated — a behavior divergence from CPython on a valid
        // program (card c1fec2bf, found by the fs-trio hardener). The full real
        // builtin OSError subclass set is listed for completeness (connection /
        // process / timeout families included even though the current stdlib
        // surface does not yet raise them), so the table matches CPython rather
        // than only today's raise sites. Every entry is a VERIFIED OSError
        // subclass — nothing here is wrongly widened.
        "OSError" => {
            let mut v = vec![
                "OSError",
                // filesystem
                "FileNotFoundError", "FileExistsError", "IsADirectoryError",
                "NotADirectoryError", "PermissionError", "SameFileError",
                // process / io
                "BlockingIOError", "ChildProcessError", "InterruptedError",
                "ProcessLookupError", "TimeoutError",
            ];
            // connection family (OSError -> ConnectionError -> ...), shared source.
            v.extend_from_slice(&CONNECTION_FAMILY);
            v
        }
        // `ConnectionError` is itself an OSError subclass WITH its own children;
        // `except ConnectionError:` catches those four (CPython-faithful).
        "ConnectionError" => CONNECTION_FAMILY.to_vec(),
        // EOFError: a direct `Exception` child (CPython `issubclass(EOFError,
        // Exception)` is True) with NO subclasses of its own — explicitly
        // registered (rather than left to the leaf/unknown-name default below)
        // so the hierarchy table documents its placement per card df83419e
        // (input() now raises "EOFError\0EOF when reading a line" on true EOF,
        // see codegen/exprs.rs's "input" arm). The singleton OR-expansion this
        // produces (`__exc_type == "EOFError"`) is behaviorally identical to
        // the exact-match fallback a leaf would otherwise get — this entry is
        // documentation/registration, not a behavior change. `except
        // Exception:` catches EOFError independent of this table, via the
        // dedicated wildcard path in emit_try (`h.exc_type == Some("Exception")`
        // short-circuits to `cond = "true"`) — this table is never consulted
        // for the "Exception" base itself.
        "EOFError" => vec!["EOFError"],
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

/// CPython-parity `str`/`repr`/`print` rendering of an `f64`. Emitted once in
/// the preamble (before REPR_PRELUDE, whose `PyRepr for f64` delegates here).
/// See the emission-site comment in `emit_program` for the algorithm.
const FLOAT_FMT_HELPER: &str = r#"fn __py_fmt_float(x: f64) -> String {
    if x.is_nan() { return "nan".to_string(); }
    if x.is_infinite() { return if x < 0.0 { "-inf".to_string() } else { "inf".to_string() }; }
    let neg = x.is_sign_negative();
    let mag = x.abs();
    // Rust's `{:e}` is Ryū SHORTEST, but it breaks an EXACT decimal tie by rounding
    // the magnitude half-UP, whereas CPython's dtoa rounds half-to-EVEN
    // (repr(-887777373534812.25) is "-887777373534812.2", not "...812.3"). Rust's
    // FIXED-precision formatter DOES round half-to-even, so: take the shortest digit
    // COUNT from `{:e}` (the CPython-shortest length), then RE-EMIT the value at that
    // length via `{:.*e}` to recover the even tie-break. Re-parse the fixed output's
    // OWN exponent, since an even-up carry (…95 -> …0 with carry) can shift decpt.
    // The re-emitted length round-trips: for a non-tie it equals the shortest; for a
    // tie both candidates round-trip and half-even picks the even one.
    let short = format!("{:e}", mag);
    let short_mant = short.split_once('e').map(|(m, _)| m).unwrap_or(&short);
    let ndig = short_mant.bytes().filter(|&c| c != b'.').count().max(1);
    let e = format!("{:.*e}", ndig - 1, mag);
    let (mant, exp_s) = e.split_once('e').unwrap();
    let exp: i32 = exp_s.parse().unwrap();
    let digits: String = mant.chars().filter(|&c| c != '.').collect();
    let decpt = exp + 1;
    let ndigits = digits.len() as i32;
    let use_exp = decpt <= -4 || decpt > 16;
    let mut out = String::new();
    if neg { out.push('-'); }
    if use_exp {
        out.push_str(&digits[..1]);
        if ndigits > 1 { out.push('.'); out.push_str(&digits[1..]); }
        let e2 = decpt - 1;
        out.push('e');
        if e2 < 0 { out.push('-'); } else { out.push('+'); }
        let ea = e2.abs();
        if ea < 10 { out.push('0'); }
        out.push_str(&ea.to_string());
    } else if decpt <= 0 {
        out.push_str("0.");
        for _ in 0..(-decpt) { out.push('0'); }
        out.push_str(&digits);
    } else if decpt >= ndigits {
        out.push_str(&digits);
        for _ in 0..(decpt - ndigits) { out.push('0'); }
        out.push_str(".0");
    } else {
        out.push_str(&digits[..decpt as usize]);
        out.push('.');
        out.push_str(&digits[decpt as usize..]);
    }
    out
}
"#;

/// CPython-parity `repr`/`ascii` of a `str`, sharing ONE quote-choice + escape
/// engine (`__py_str_escape`). Quote matrix (the `%r` rule): default single
/// quotes, switch to double iff the string has a `'` and no `"` (so
/// `repr("it's") == "\"it's\""`); always escape backslash + the chosen quote; map
/// `\n`/`\t`/`\r`.
///
/// `repr` (`ascii_only=false`) escapes every char CPython treats as
/// NON-PRINTABLE that this engine covers: ASCII controls (`< 0x20`, `0x7f`), the
/// C1 controls (`U+0080..=U+009F`), and the common format/invisible code points
/// (`U+00AD`; `U+200B..=U+200F`; `U+2028..=U+202E`; `U+FEFF`) — as `\xXX`
/// (`<=0xff`), `\uXXXX` (`<=0xffff`), or `\UXXXXXXXX`. DOCUMENTED GAP: the exotic
/// Cf/Cn categories outside those ranges are still passed through rather than
/// `\u`-escaped (the full Unicode "printable" table is out of scope) — see
/// PYTHON_COMPATIBILITY.md. `ascii` (`ascii_only=true`) additionally escapes
/// EVERY non-ASCII char (`>= 0x80`), which is exactly `ascii()`'s contract.
const STR_REPR_HELPER: &str = r#"fn __py_repr_should_escape(u: u32) -> bool {
    u < 0x20 || u == 0x7f
        || (0x80..=0x9f).contains(&u)
        || u == 0xad
        || (0x200b..=0x200f).contains(&u)
        || (0x2028..=0x202e).contains(&u)
        || u == 0xfeff
}
fn __py_char_escape(u: u32) -> String {
    if u <= 0xff { format!("\\x{:02x}", u) }
    else if u <= 0xffff { format!("\\u{:04x}", u) }
    else { format!("\\U{:08x}", u) }
}
fn __py_str_escape(s: &str, ascii_only: bool) -> String {
    let has_single = s.contains('\'');
    let has_double = s.contains('"');
    let quote = if has_single && !has_double { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        let u = c as u32;
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c == quote => { out.push('\\'); out.push(c); }
            _ if (ascii_only && u >= 0x80) || __py_repr_should_escape(u) => {
                out.push_str(&__py_char_escape(u));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}
fn __py_str_repr(s: &str) -> String { __py_str_escape(s, false) }
fn __py_ascii(s: &str) -> String { __py_str_escape(s, true) }
"#;

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
impl PyRepr for String { fn py_repr(&self) -> String { __py_str_repr(self) } }
impl PyRepr for str { fn py_repr(&self) -> String { __py_str_repr(self) } }
impl<T: PyRepr> PyRepr for Vec<T> {
    fn py_repr(&self) -> String {
        let xs: Vec<String> = self.iter().map(|x| x.py_repr()).collect();
        format!("[{}]", xs.join(", "))
    }
}
impl<T: PyRepr> PyRepr for Option<T> {
    fn py_repr(&self) -> String {
        match self { Some(x) => x.py_repr(), None => "None".to_string() }
    }
}
impl<T: PyRepr> PyRepr for ::std::boxed::Box<T> {
    fn py_repr(&self) -> String { (**self).py_repr() }
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
/// (W1.5, card b671f313) Python `str.title()`-class TITLECASE of one char.
/// Rust std has no `to_titlecase`; Python's first-char mapping differs from
/// `to_uppercase` for (a) the Unicode digraph letters (Dž Lj Nj Dz families —
/// single-char Lt forms), (b) the polytonic-Greek prosgegrammeni letters
/// (U+1F8x/9x/Ax + U+1FB3/C3/F3 map to their Lt forms, NOT the two-char
/// uppercase expansion), and (c) multi-char uppercase expansions (ß -> "Ss",
/// not "SS": first char of the expansion stays upper, the rest lowers).
/// Everything else falls through to `to_uppercase` (single char), matching
/// CPython's title mapping on the tested corpus (python3-diffed).
const TITLECASE_PRELUDE: &str = r#"fn __py_titlecase(c: char) -> String {
    match c {
        '\u{01C4}' | '\u{01C5}' | '\u{01C6}' => "\u{01C5}".to_string(),
        '\u{01C7}' | '\u{01C8}' | '\u{01C9}' => "\u{01C8}".to_string(),
        '\u{01CA}' | '\u{01CB}' | '\u{01CC}' => "\u{01CB}".to_string(),
        '\u{01F1}' | '\u{01F2}' | '\u{01F3}' => "\u{01F2}".to_string(),
        '\u{1F80}'..='\u{1F87}' | '\u{1F90}'..='\u{1F97}' | '\u{1FA0}'..='\u{1FA7}' =>
            char::from_u32(c as u32 + 8).unwrap_or(c).to_string(),
        '\u{1F88}'..='\u{1F8F}' | '\u{1F98}'..='\u{1F9F}' | '\u{1FA8}'..='\u{1FAF}' => c.to_string(),
        '\u{1FB3}' => "\u{1FBC}".to_string(),
        '\u{1FC3}' => "\u{1FCC}".to_string(),
        '\u{1FF3}' => "\u{1FFC}".to_string(),
        '\u{1FBC}' | '\u{1FCC}' | '\u{1FFC}' => c.to_string(),
        _ => {
            let mut out = String::new();
            for (i, u) in c.to_uppercase().enumerate() {
                if i == 0 { out.push(u); } else { out.extend(u.to_lowercase()); }
            }
            out
        }
    }
}
"#;

// (W5-g) The `file` handle's runtime. PyFile is the built-in opaque move-only
// handle (`Ty::Handle("file")`): non-`Clone`, non-`Copy`. The `inner: Option<File>`
// IS the closed flag — `None` means closed. `close()` takes the fd (dropping it
// closes the OS handle NOW, matching Python's eager close); a SECOND close() is an
// IDEMPOTENT no-op and any read/write on a closed file is an honest, catchable
// `ValueError` with CPython's exact message ("I/O operation on closed file."),
// closing the old silent-no-op hole that let a read-after-close silently succeed.
// Drop closes via the `Option<File>`'s own drop, so Drop AFTER an explicit close is
// a no-op (the `with` block's RAII close never double-frees). (W5-g, C8 — LEAD
// DECISION, oracled vs python3 3.12.9) close() is now CPython-faithful IDEMPOTENT
// (a 2nd close is a silent no-op), superseding the design's strict-double-close-
// `ValueError` mandate — see docs/design/w5-bytes-handles.md AS-BUILT note; the
// closed FLAG survives as the read/write-after-close `ValueError` (which still
// matches CPython exactly), so this whole runtime is now dual-run parity-clean.
const FILE_PRELUDE: &str = r#"struct PyFile { inner: Option<std::fs::File> }
impl PyFile {
    fn read(&mut self) -> String {
        use std::io::Read;
        let f = self.inner.as_mut().unwrap_or_else(|| panic!("ValueError\0I/O operation on closed file."));
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap_or_else(|e| panic!("OSError\0read failed: {}", e));
        s
    }
    fn readlines(&mut self) -> Vec<String> {
        self.read().lines().map(|l| l.to_string()).collect()
    }
    fn write(&mut self, s: &str) {
        use std::io::Write;
        let f = self.inner.as_mut().unwrap_or_else(|| panic!("ValueError\0I/O operation on closed file."));
        f.write_all(s.as_bytes()).unwrap_or_else(|e| panic!("OSError\0write failed: {}", e));
    }
    fn close(&mut self) {
        self.inner = None;
    }
}
fn __py_open(path: &str, mode: &str) -> PyFile {
    let f = match mode {
        "w" => std::fs::File::create(path).unwrap_or_else(|e| panic!("OSError\0open failed: {}: {}", path, e)),
        "a" => std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap_or_else(|e| panic!("OSError\0open failed: {}: {}", path, e)),
        _ => std::fs::File::open(path).unwrap_or_else(|e| panic!("OSError\0open failed: {}: {}", path, e)),
    };
    PyFile { inner: Some(f) }
}
"#;

/// (W5-a) The `bytes` runtime. Emitted once per program like FILE_PRELUDE/
/// REPR_PRELUDE (unconditionally, under the crate `#![allow(dead_code)]` when a
/// program uses no bytes — matching the existing prelude policy).
///
/// `__py_bytes_repr` is the SINGLE display engine — `print`/`str`/`repr`/
/// f-string all route here — and its escaping table is python3-oracle-validated
/// byte-for-byte (design §G): default single quotes, switch to double iff the
/// payload contains a `'` and no `"`; escape `\\`, the active quote, and
/// `\t`/`\n`/`\r`; a printable byte 0x20–0x7e is literal; every other byte
/// (0x00–0x1f, 0x7f–0xff) is a lowercase `\xNN`. `__py_bytes_index` is the
/// byte-offset index (negative-normalised, catchable `IndexError`, u8 -> i64).
///
/// `impl PyRepr for Vec<u8>` routes the trait dispatch here too, so a
/// `list[bytes]` / `dict[bytes, _]` reprs its bytes elements as `b'...'` for
/// free. It coexists with the blanket `impl<T: PyRepr> PyRepr for Vec<T>`
/// because rustc proves `u8: PyRepr` is unsatisfiable (no such impl), so the two
/// never overlap (rustc-1.95 coherence-probe-verified).
///
/// (W5-b) The remaining `__py_bytes_*` fns are the method + codec surface, all
/// BYTE-offset and python3-oracle-validated (scratchpad probes): `fromhex`
/// (CPython position/message), strict `decode_utf8` (byte-identical
/// UnicodeDecodeError shape), `find`/`rfind`/`index_of`/`rindex_of`/`count`
/// (subsequence; index_of/rindex_of raise `ValueError: subsection not found`),
/// `replace`/`split`/`split_ws`/`join`/`strip`/`pad`/`zfill`/`hex`/`upper`/
/// `lower`, the `int in bytes` range-checked membership + `bytes in bytes`
/// subsequence, and the ASCII-only predicate helpers. All under the crate
/// `#![allow(dead_code)]`, emitted once like the repr engine.
const BYTES_PRELUDE: &str = r#"fn __py_bytes_repr(b: &[u8]) -> String {
    let has_single = b.contains(&b'\'');
    let has_double = b.contains(&b'"');
    let quote: u8 = if has_single && !has_double { b'"' } else { b'\'' };
    let mut out = String::from("b");
    out.push(quote as char);
    for &c in b {
        if c == quote { out.push('\\'); out.push(c as char); }
        else if c == b'\\' { out.push_str("\\\\"); }
        else if c == b'\t' { out.push_str("\\t"); }
        else if c == b'\n' { out.push_str("\\n"); }
        else if c == b'\r' { out.push_str("\\r"); }
        else if (0x20..=0x7e).contains(&c) { out.push(c as char); }
        else { out.push_str(&format!("\\x{:02x}", c)); }
    }
    out.push(quote as char);
    out
}
fn __py_bytes_index(b: &[u8], i: i64) -> i64 {
    let n = b.len() as i64;
    let j = if i < 0 { i + n } else { i };
    if j < 0 || j >= n { panic!("IndexError\0index out of range"); }
    b[j as usize] as i64
}
impl PyRepr for Vec<u8> { fn py_repr(&self) -> String { __py_bytes_repr(self) } }
fn __py_bytes_fromhex(s: &str) -> Vec<u8> {
    let raw = s.as_bytes();
    let n = raw.len();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    let hexval = |c: u8| -> i32 { match c { b'0'..=b'9' => (c - b'0') as i32, b'a'..=b'f' => (c - b'a' + 10) as i32, b'A'..=b'F' => (c - b'A' + 10) as i32, _ => -1 } };
    while i < n {
        if matches!(raw[i], 9 | 10 | 11 | 12 | 13 | 32) { i += 1; continue; }
        let top = hexval(raw[i]);
        if top < 0 { panic!("ValueError\0non-hexadecimal number found in fromhex() arg at position {}", i); }
        if i + 1 >= n { panic!("ValueError\0non-hexadecimal number found in fromhex() arg at position {}", i + 1); }
        let bot = hexval(raw[i + 1]);
        if bot < 0 { panic!("ValueError\0non-hexadecimal number found in fromhex() arg at position {}", i + 1); }
        out.push(((top << 4) | bot) as u8);
        i += 2;
    }
    out
}
fn __py_bytes_decode_utf8(b: Vec<u8>) -> String {
    match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => {
            let ue = e.utf8_error();
            let p = ue.valid_up_to();
            let raw = e.as_bytes();
            match ue.error_len() {
                None => {
                    if raw.len() - p <= 1 {
                        panic!("UnicodeDecodeError\0'utf-8' codec can't decode byte 0x{:02x} in position {}: unexpected end of data", raw[p], p);
                    } else {
                        panic!("UnicodeDecodeError\0'utf-8' codec can't decode bytes in position {}-{}: unexpected end of data", p, raw.len() - 1);
                    }
                }
                Some(n) => {
                    if n > 1 {
                        // A valid lead byte plus >=1 valid continuation byte was
                        // consumed before an INVALID continuation byte: CPython
                        // reports the RANGE form `bytes in position P-Q` (Q = P+n-1).
                        // For error_len n>1 the reason is ALWAYS "invalid continuation
                        // byte" (an invalid START byte is always error_len==1) — verified
                        // against python3 3.12 + a Rust `Utf8Error::error_len` probe
                        // (b'\xe2\x82\x28'->0-1 n=2, b'\xf0\x90\x8d\x28'->0-2 n=3).
                        panic!("UnicodeDecodeError\0'utf-8' codec can't decode bytes in position {}-{}: invalid continuation byte", p, p + n - 1);
                    } else {
                        let bad = raw[p];
                        let reason = if (0xc2..=0xf4).contains(&bad) { "invalid continuation byte" } else { "invalid start byte" };
                        panic!("UnicodeDecodeError\0'utf-8' codec can't decode byte 0x{:02x} in position {}: {}", bad, p, reason);
                    }
                }
            }
        }
    }
}
fn __py_bytes_find(hay: &[u8], needle: &[u8]) -> i64 {
    if needle.is_empty() { return 0; }
    if needle.len() > hay.len() { return -1; }
    for start in 0..=(hay.len() - needle.len()) { if &hay[start..start + needle.len()] == needle { return start as i64; } }
    -1
}
fn __py_bytes_rfind(hay: &[u8], needle: &[u8]) -> i64 {
    if needle.is_empty() { return hay.len() as i64; }
    if needle.len() > hay.len() { return -1; }
    for start in (0..=(hay.len() - needle.len())).rev() { if &hay[start..start + needle.len()] == needle { return start as i64; } }
    -1
}
fn __py_bytes_index_of(hay: &[u8], needle: &[u8]) -> i64 {
    let r = __py_bytes_find(hay, needle);
    if r < 0 { panic!("ValueError\0subsection not found"); }
    r
}
fn __py_bytes_rindex_of(hay: &[u8], needle: &[u8]) -> i64 {
    let r = __py_bytes_rfind(hay, needle);
    if r < 0 { panic!("ValueError\0subsection not found"); }
    r
}
fn __py_bytes_count(hay: &[u8], needle: &[u8]) -> i64 {
    if needle.is_empty() { return hay.len() as i64 + 1; }
    if needle.len() > hay.len() { return 0; }
    let mut count = 0i64;
    let mut start = 0usize;
    while start + needle.len() <= hay.len() {
        if &hay[start..start + needle.len()] == needle { count += 1; start += needle.len(); } else { start += 1; }
    }
    count
}
fn __py_bytes_replace(hay: &[u8], old: &[u8], new: &[u8]) -> Vec<u8> {
    if old.is_empty() {
        let mut out = Vec::new();
        out.extend_from_slice(new);
        for &b in hay { out.push(b); out.extend_from_slice(new); }
        return out;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < hay.len() {
        if i + old.len() <= hay.len() && &hay[i..i + old.len()] == old { out.extend_from_slice(new); i += old.len(); }
        else { out.push(hay[i]); i += 1; }
    }
    out
}
fn __py_bytes_split(hay: &[u8], sep: &[u8]) -> Vec<Vec<u8>> {
    if sep.is_empty() { panic!("ValueError\0empty separator"); }
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < hay.len() {
        if i + sep.len() <= hay.len() && &hay[i..i + sep.len()] == sep { out.push(std::mem::take(&mut cur)); i += sep.len(); }
        else { cur.push(hay[i]); i += 1; }
    }
    out.push(cur);
    out
}
fn __py_bytes_split_ws(hay: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    for &b in hay {
        if matches!(b, 9 | 10 | 11 | 12 | 13 | 32) { if !cur.is_empty() { out.push(std::mem::take(&mut cur)); } }
        else { cur.push(b); }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}
fn __py_bytes_join(sep: &[u8], parts: &[Vec<u8>]) -> Vec<u8> { parts.join(sep) }
fn __py_bytes_strip(b: &[u8], set: &[u8], left: bool, right: bool) -> Vec<u8> {
    let mut lo = 0usize;
    let mut hi = b.len();
    if left { while lo < hi && set.contains(&b[lo]) { lo += 1; } }
    if right { while hi > lo && set.contains(&b[hi - 1]) { hi -= 1; } }
    b[lo..hi].to_vec()
}
fn __py_int_in_bytes(v: i64, b: &[u8]) -> bool {
    if !(0..=255).contains(&v) { panic!("ValueError\0byte must be in range(0, 256)"); }
    b.contains(&(v as u8))
}
fn __py_bytes_contains(needle: &[u8], hay: &[u8]) -> bool { __py_bytes_find(hay, needle) >= 0 }
fn __py_bytes_pad(b: &[u8], width: i64, fill: &[u8], meth: &str, left_pad: bool, right_pad: bool) -> Vec<u8> {
    if fill.len() != 1 { panic!("TypeError\0{}() argument 2 must be a byte string of length 1, not bytes", meth); }
    let w = if width < 0 { 0usize } else { width as usize };
    if b.len() >= w { return b.to_vec(); }
    let total = w - b.len();
    let lpad = if left_pad && right_pad { total / 2 + (total & w & 1) } else if left_pad { total } else { 0 };
    let rpad = total - lpad;
    let mut out: Vec<u8> = Vec::with_capacity(w);
    out.extend(std::iter::repeat(fill[0]).take(lpad));
    out.extend_from_slice(b);
    out.extend(std::iter::repeat(fill[0]).take(rpad));
    out
}
fn __py_bytes_zfill(b: &[u8], width: i64) -> Vec<u8> {
    let w = if width < 0 { 0usize } else { width as usize };
    if b.len() >= w { return b.to_vec(); }
    let pad = w - b.len();
    let mut out: Vec<u8> = Vec::with_capacity(w);
    let mut body_start = 0usize;
    if !b.is_empty() && (b[0] == b'+' || b[0] == b'-') { out.push(b[0]); body_start = 1; }
    out.extend(std::iter::repeat(b'0').take(pad));
    out.extend_from_slice(&b[body_start..]);
    out
}
fn __py_bytes_hex(b: &[u8]) -> String { b.iter().map(|x| format!("{:02x}", x)).collect() }
fn __py_bytes_upper(b: &[u8]) -> Vec<u8> { b.iter().map(|x| x.to_ascii_uppercase()).collect() }
fn __py_bytes_lower(b: &[u8]) -> Vec<u8> { b.iter().map(|x| x.to_ascii_lowercase()).collect() }
fn __py_bytes_all(b: &[u8], f: fn(u8) -> bool) -> bool { !b.is_empty() && b.iter().all(|&c| f(c)) }
fn __py_byte_is_digit(c: u8) -> bool { c.is_ascii_digit() }
fn __py_byte_is_alpha(c: u8) -> bool { c.is_ascii_alphabetic() }
fn __py_byte_is_alnum(c: u8) -> bool { c.is_ascii_alphanumeric() }
fn __py_byte_is_space(c: u8) -> bool { matches!(c, 9 | 10 | 11 | 12 | 13 | 32) }
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
const GEN_PRELUDE: &str = r#"struct __PyrstYieldNow { done: bool }
impl std::future::Future for __PyrstYieldNow {
    type Output = ();
    fn poll(mut self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        if self.done { std::task::Poll::Ready(()) } else { self.done = true; std::task::Poll::Pending }
    }
}
struct __PyrstCo<T> { slot: std::rc::Rc<std::cell::RefCell<std::option::Option<T>>> }
impl<T> __PyrstCo<T> {
    fn yield_(&self, v: T) -> __PyrstYieldNow {
        *self.slot.borrow_mut() = std::option::Option::Some(v);
        __PyrstYieldNow { done: false }
    }
}
struct __PyrstGen<T> {
    fut: std::pin::Pin<std::boxed::Box<dyn std::future::Future<Output = ()>>>,
    slot: std::rc::Rc<std::cell::RefCell<std::option::Option<T>>>,
    done: bool,
}
impl<T> __PyrstGen<T> {
    fn empty() -> __PyrstGen<T> {
        __PyrstGen {
            fut: std::boxed::Box::pin(async {}),
            slot: std::rc::Rc::new(std::cell::RefCell::new(std::option::Option::None)),
            done: false,
        }
    }
}
impl<T> std::iter::Iterator for __PyrstGen<T> {
    type Item = T;
    fn next(&mut self) -> std::option::Option<T> {
        // FUSED: polling a completed future is a contract violation ("resumed
        // after completion" panic). Python iterates an exhausted generator as
        // empty forever, so next() on a done __PyrstGen returns None forever.
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

    // (W3-2) Record the ROOT program's OWN top-level fn/class/const bare names.
    // A bare reference at the root resolves to the root's own definition FIRST
    // (root-shadows-imports — matching typeck's flat merge, where the root is last
    // and wins the bare namespace), so `bare_owner_for` will not mangle a root name
    // that ALSO happens to be from-imported. The root is the module with no dotted
    // id (`module_id == None`; it is also last in topological order).
    for (m, _src) in modules {
        if m.module_id.is_none() {
            for s in &m.stmts {
                match s {
                    Stmt::Func(f) => { cg.root_defined.insert(f.name.clone()); }
                    Stmt::Class(c) => { cg.root_defined.insert(c.name.clone()); }
                    Stmt::Assign { target, .. } if crate::typeck::is_module_const_decl(s) => {
                        cg.root_defined.insert(target.clone());
                    }
                    _ => {}
                }
            }
        }
    }

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
    // `non_snake_case` (W2 stdlib): CPython-exact public names are kept verbatim
    // (`stat.S_ISREG`, `difflib.IS_LINE_JUNK`, ...) — matching the reference API is
    // the contract, so their non-snake spelling is intentional, not a user mistake.
    // `unused_braces` (sibling of the already-suppressed `unused_parens`): a
    // single-expression `@extern` template body (`plat.uname_release`'s `{ Command
    // ::new(...)...}`) lowers as a braced block return — structurally redundant
    // generated code, never a hand-maintained style choice, same as extra parens.
    cg.line("#![allow(unused_parens, unused_braces, unused_variables, unused_mut, dead_code, unused_imports, non_upper_case_globals, non_camel_case_types, non_snake_case, unreachable_code, unused_assignments)]");
    cg.line("use std::io::Write;");
    cg.line("");
    // CPython-parity float formatting (str==repr for floats in Python 3). Uses
    // the shortest round-tripping digit string + decimal exponent that Rust's
    // `{:e}` already computes (Grisu/Ryū), then applies CPython's
    // `format_float_short('r')` presentation rules: exponential iff
    // `decpt <= -4 || decpt > 16`, a trailing `.0` on integral values, a
    // sign-and-≥2-digit exponent, and `inf`/`-inf`/`nan` specials. This makes
    // `str(1e16)`/`repr(1.0)`/`print(1e-5)` all byte-identical to python3
    // (validated against a broad magnitude battery), replacing the old
    // `{:.1}`-or-`{}` form that never emitted scientific notation.
    cg.line(FLOAT_FMT_HELPER);
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
    // Python FLOAT modulo (`x % y` where either operand is a float). CPython's
    // `float_rem` (Objects/floatobject.c) is fmod-based, NOT the divisor-signed
    // `(((a % b) + b) % b)` reformulation this used to emit — that form
    // DOUBLE-ROUNDS (`0.1 % 1.0` computed `fmod(1.1, 1.0)` and lost the low
    // bits -> 0.10000000000000009 instead of 0.1). Rust's `f64 %` IS fmod, so
    // mirror CPython exactly: take fmod, adjust the sign to the DIVISOR only
    // when the remainder is non-zero (a single add, no second rounding), and in
    // the signed-zero case return a zero carrying the divisor's sign (CPython's
    // `copysign(0.0, b)` rule, so `5.0 % -5.0` is `-0.0` like python3). A zero
    // divisor raises the catchable `ZeroDivisionError\0float modulo` (CPython's
    // exact text) instead of Rust's silent NaN.
    cg.line("fn __py_fmod(a: f64, b: f64) -> f64 {");
    cg.line("    if b == 0.0 { panic!(\"ZeroDivisionError\\0float modulo\"); }");
    cg.line("    let m = a % b;");
    cg.line("    if m != 0.0 { if (b < 0.0) != (m < 0.0) { m + b } else { m } } else { (0.0_f64).copysign(b) }");
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
    // Python str.find / rfind / index / rindex. These return a CHARACTER offset
    // in CPython, but Rust's `str::find`/`rfind` return a UTF-8 BYTE offset —
    // and pyrst's len()/indexing/slicing are all char-based, so emitting the
    // raw byte offset silently corrupted every downstream slice/index on a
    // string with a multibyte char before the match (`"café.txt".rfind(".")`
    // gave 5, python3 gives 4). Convert byte->char once via the matched prefix's
    // `chars().count()` (O(n), correctness over cleverness). find/rfind return
    // -1 when absent; index/rindex raise the catchable
    // `ValueError\0substring not found` (CPython's `ValueError: substring not
    // found`), matching the inline panic the str.index arm already emitted.
    cg.line("fn __py_str_find(__s: &str, __sub: &str) -> i64 {");
    cg.line("    match __s.find(__sub) { Some(__b) => __s[..__b].chars().count() as i64, None => -1i64 }");
    cg.line("}");
    cg.line("fn __py_str_rfind(__s: &str, __sub: &str) -> i64 {");
    cg.line("    match __s.rfind(__sub) { Some(__b) => __s[..__b].chars().count() as i64, None => -1i64 }");
    cg.line("}");
    cg.line("fn __py_str_index(__s: &str, __sub: &str) -> i64 {");
    cg.line("    match __s.find(__sub) { Some(__b) => __s[..__b].chars().count() as i64, None => panic!(\"ValueError\\0substring not found\") }");
    cg.line("}");
    cg.line("fn __py_str_rindex(__s: &str, __sub: &str) -> i64 {");
    cg.line("    match __s.rfind(__sub) { Some(__b) => __s[..__b].chars().count() as i64, None => panic!(\"ValueError\\0substring not found\") }");
    cg.line("}");
    // List-unpacking length check (`a, b = xs` where xs is a list). Panics with
    // CPython 3.12's EXACT ValueError text — "not enough values to unpack
    // (expected N, got G)" when short, "too many values to unpack (expected N)"
    // when long — via the catchable `ValueError\0..` payload the try/except
    // dispatcher matches. Returns the validated Vec so callers index each target.
    cg.line("fn __py_unpack_list<T>(v: Vec<T>, n: usize) -> Vec<T> {");
    cg.line("    let got = v.len();");
    cg.line("    if got < n { panic!(\"ValueError\\0not enough values to unpack (expected {}, got {})\", n, got); }");
    cg.line("    if got > n { panic!(\"ValueError\\0too many values to unpack (expected {})\", n); }");
    cg.line("    v");
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
    // (card 87bd8eb4) Direction-aware `range(start, stop, step)`. CPython's range
    // is DESCENDING when step<0, but Rust's `a..b` is ascending-only and `.step_by`
    // takes a `usize` — a negative step wrapped to a huge usize and the range
    // silently yielded NOTHING (a silent divergence on valid programs). The 3-arg
    // range materializes through this direction-aware builder (used uniformly by
    // for-loops, `list(range(...))`, and comprehensions). A zero step is CPython's
    // catchable `ValueError: range() arg 3 must not be zero`.
    cg.line("fn __py_range_step(__start: i64, __stop: i64, __step: i64) -> Vec<i64> {");
    cg.line("    if __step == 0 { panic!(\"ValueError\\0range() arg 3 must not be zero\"); }");
    cg.line("    let mut __result: Vec<i64> = Vec::new();");
    cg.line("    let mut __i = __start;");
    cg.line("    if __step > 0 { while __i < __stop { __result.push(__i); __i += __step; } }");
    cg.line("    else { while __i > __stop { __result.push(__i); __i += __step; } }");
    cg.line("    __result");
    cg.line("}");
    // (try/except control flow) Signal a try BODY's escaping control flow out of
    // the catch_unwind closure: `Return(R)` carries the enclosing function's
    // return value, `Break`/`Continue` re-target the loop enclosing the try, and
    // `Normal` means the body fell through (so `else` runs). See `emit_try`.
    cg.line("enum __PyrstTryFlow<R> { Normal, Return(R), Break, Continue }");
    cg.line(STR_REPR_HELPER);
    cg.line(REPR_PRELUDE);
    cg.line(TITLECASE_PRELUDE);
    cg.line(FILE_PRELUDE);
    // (W5-a) The `bytes` runtime (repr engine + bounds-checked index + the
    // `PyRepr for Vec<u8>` impl). AFTER REPR_PRELUDE, which declares `PyRepr`.
    cg.line(BYTES_PRELUDE);
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
        // (W3-2) Owner-qualify this module's consts by its dotted id (None=root).
        cg.current_module = m.module_id.clone();
        for s in &m.stmts {
            if crate::typeck::is_module_const_decl(s) {
                // Record const names BEFORE emitting bodies so every reference
                // (bare `CONST` / qualified `X.CONST`) lowers to the MANGLED name,
                // and str consts additionally get `.to_string()`.
                if let Stmt::Assign { target, value, .. } = s {
                    // (W4-a) A scalar-literal binding PROMOTED to a mutable static
                    // (rule (a): `global`+rebind) is a `thread_local!`, NOT a
                    // `const` — the globals prepass below emits it. Skip it here so
                    // it is not double-emitted (and so references resolve to the
                    // static, not a stale const). A never-rebound scalar literal
                    // stays the const path, byte-identical.
                    let owner = m.module_id.clone();
                    if cg.ctx.is_mutable_global(owner.as_deref(), target) {
                        continue;
                    }
                    // (W3-fix / F8,F9) Key by (owner module, name); `m.module_id` is
                    // the owner (`None` = root), matching `bare_owner_for` / the
                    // qualifier at every reference site.
                    cg.const_names.insert((owner.clone(), target.clone()));
                    if matches!(value, Expr::Str(..)) {
                        cg.const_strs.insert((owner, target.clone()));
                    }
                }
                cg.emit_const_decl(s)?;
            }
        }
    }

    // (W4-a) MUTABLE-GLOBAL prepass: emit the `thread_local!` statics + the eager
    // top-down `__pyrst_init_globals()`. Returns whether any global was emitted, so
    // `main()` calls the init fn FIRST (CPython import-time semantics). Emits
    // nothing — and `has_globals` stays false — for a program with no module-level
    // mutable state, keeping the const path byte-identical.
    let has_globals = cg.emit_mutable_globals(modules)?;

    // Emit all modules in order (imports first, root last)
    for (m, _src) in modules {
        // (W3-2) The owner threaded into every top-level DEFINITION emitted below
        // (`emit_func` name, `emit_class` struct/impls, and same-module bare refs
        // via `bare_owner_for`). `None` for the root → crate-root-unwrapped.
        cg.current_module = m.module_id.clone();
        for s in &m.stmts {
            // Skip import statements — they're resolved, not emitted
            if matches!(s, Stmt::Import { .. }) { continue; }
            cg.emit_top_stmt(s)?;
        }
    }
    // (W3-2) Companion enums resolve every class name via the GLOBAL class_owner
    // map (not current_module), but reset to the root for clarity/defensiveness.
    cg.current_module = None;

    // (EPIC-5 C2-2a) Emit the companion-enum machinery (closed-set enum +
    // method-dispatch impl + field-accessor impl) for every polymorphic base,
    // AFTER all value-structs exist. C2-2a emits it as #[allow(dead_code)] and
    // never references it (rust_ty still plain `n`, C1 gate intact), so output is
    // byte-for-byte unchanged; the dead code merely has to compile. C2-2b wires
    // it in.
    cg.emit_companion_enums()?;

    // Synthetic entry point (same as current emit_module logic). (W4-a) When the
    // program has mutable globals, `__pyrst_init_globals()` runs FIRST — before any
    // user code — so every module global is initialized eagerly, top-down, in
    // import order (CPython import-time semantics). Import-free / global-free
    // programs keep the exact `fn main() { user_main(); }` / `fn main() {}` byte
    // sequence (`has_globals` is false → no init call), so their emission is
    // byte-identical.
    let init_call = if has_globals { "__pyrst_init_globals(); " } else { "" };
    if ctx.funcs.contains_key("main") {
        cg.line("");
        cg.line(&format!("fn main() {{ {}user_main(); }}", init_call));
    } else if has_globals {
        cg.line("");
        cg.line("fn main() { __pyrst_init_globals(); }");
    } else {
        cg.line("");
        cg.line("fn main() {}");
    }

    Ok(cg.out)
}

