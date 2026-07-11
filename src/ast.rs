// AST span/payload fields are the source-location backbone of the tree: the
// parser populates them for diagnostics and tooling even where no reader
// consumes them yet. Likewise `TypeExpr::Tuple` is matched on by typeck and the
// formatter but is currently produced via the generic-`tuple` path, so it reads
// as never-constructed. Both are intentionally retained; a module-level allow
// keeps the build warning-free without deleting that infrastructure.
#![allow(dead_code)]

use crate::diag::Span;

/// A parsed f-string part. Interpolations carry a fully-parsed [`Expr`]
/// (so every pyrst construct works inside f-strings) plus an optional
/// Python format spec (e.g. ".2f", "08d", ">8").
#[derive(Debug, Clone)]
pub enum FStrPart {
    Lit(String),
    Interp(Expr, Option<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),                            // int, str, MyClass, etc.
    Generic(String, Vec<TypeExpr>),           // list[int], dict[str, int], tuple[int, str]
    Tuple(Vec<TypeExpr>),                     // (int, str) — tuple type
    /// A first-class function type written `Callable[[Arg, ...], Ret]`. The
    /// `Vec` is the (possibly empty) argument-type list; the `Box` is the return
    /// type. Lowered to `Ty::Func` by `from_type_expr` and emitted as
    /// `Rc<dyn Fn(Arg, ...) -> Ret>` by codegen.
    Func(Vec<TypeExpr>, Box<TypeExpr>),
    None_,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub default: Option<Expr>,
    pub span: Span,
    /// EPIC-4 V2: opt-in by-reference param mode. Set true when the annotation
    /// was `Mut[T]` (the parser unwraps `ty` to `T` and raises this flag). It is
    /// meaningful only on a *function/method parameter* — class fields never set
    /// it. Front-end use only for now: typeck reads it to require a place at the
    /// call site and to skip the by-value mutation backstop; codegen still emits
    /// the param by value (the `&mut T` emission is V2-c). Default false.
    pub by_ref: bool,
}

#[derive(Debug, Clone)]
pub struct Func {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Vec<Stmt>,
    pub span: Span,
    pub is_method: bool,
    pub decorators: Vec<String>,
    /// Rust-interop Phase 2: external-crate dependencies declared on this `def`
    /// via the `@crate("name", "version")` decorator, as `(name, version)`
    /// pairs in source order. EMPTY for a function with no `@crate` decorator
    /// (the overwhelming common case). The parser validates each `@crate` takes
    /// exactly two string-literal args and records them here; it carries no body
    /// effect. The driver collects the UNION (deduped by crate name) of these
    /// across every reachable module (root + embedded stdlib) to decide whether a
    /// program needs the Cargo-project build path and, if so, which dependencies
    /// to write into the generated `Cargo.toml`. A bare `@crate` decorator name
    /// still lands in `decorators` (so `validate_decorators` admits it); the
    /// parsed args live here.
    pub crate_deps: Vec<(String, String)>,
    /// Generics v1 (PEP 695): the declared type parameters of a parametric
    /// generic function, e.g. `["T", "U"]` for `def f[T, U](...)`. EMPTY for a
    /// non-generic `def`. A name in this list is a BOUND type variable inside the
    /// function: param/return annotations naming it lower to `Ty::TypeVar(name)`
    /// (scoped lowering), call sites unify it against the actual argument types,
    /// and codegen emits it as a Rust generic parameter `name: Clone`. Only
    /// top-level `def`s may declare type params in v1 (methods stay
    /// non-generic — the parser rejects a clause on a method).
    pub type_params: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExceptHandler {
    pub exc_type: Option<String>,
    pub exc_name: Option<String>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ClassDef {
    pub name: String,
    pub bases: Vec<String>,
    pub fields: Vec<Param>,
    pub methods: Vec<Func>,
    pub is_dataclass: bool,
    /// (card 6f69d4a3) Every decorator name written above the `class` in source
    /// order (e.g. `["dataclass"]`, or `["made_up"]`). EMPTY for an undecorated
    /// class. typeck runs this through `validate_decorators` so an UNRECOGNIZED
    /// class decorator is an honest check error instead of being silently swallowed
    /// (the pre-card behavior: only `dataclass` was ever inspected, every other
    /// name dropped). `is_dataclass` stays the derived convenience flag.
    pub decorators: Vec<String>,
    /// (card 6f69d4a3) True when `@dataclass(...)` was written WITH arguments
    /// (any non-empty parens, e.g. `@dataclass(order=True)`). Only the BARE
    /// `@dataclass` is supported initially; typeck honest-rejects a dataclass whose
    /// flag arguments (order=/frozen=/eq=/repr=/init=/slots=/…) are present, rather
    /// than silently ignoring a requested `order=`/`frozen=` semantics change.
    pub dataclass_has_args: bool,
    pub span: Span,
    /// Generics v2 (PEP 695): the declared type parameters of a parametric
    /// generic CLASS, e.g. `["T"]` for `class Box[T]:` or `["A", "B"]` for
    /// `class Pair[A, B]:`. EMPTY for a non-generic class. A name in this list is
    /// a BOUND type variable SCOPED TO THE WHOLE CLASS: it is visible in every
    /// field annotation and every method param/return annotation, where it lowers
    /// to `Ty::TypeVar(name)` (scoped lowering) instead of `Ty::Class(name)`. At a
    /// constructor call (`Box(5)`) the class's type arguments are inferred by
    /// unifying `__init__`'s param types against the actual arg types, producing a
    /// `Ty::Class("Box", [int])`; method calls and field reads on such an instance
    /// substitute the recorded args into the (type-var-bearing) signature/field
    /// type. Codegen emits the class as `struct Box<T> { .. }` +
    /// `impl<T: Clone + ..> Box<T> { .. }`, with the per-`T` bounds inferred from
    /// the ops the methods perform (reusing the generic-FUNCTION bound machinery).
    pub type_params: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum MatchPattern {
    Literal(Expr),
    Capture(String),
    Wildcard,
    Or(Vec<MatchPattern>),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Assign { target: String, ty: Option<TypeExpr>, value: Expr, span: Span },
    AugAssign { target: String, op: BinOp, value: Expr, span: Span },
    Unpack { targets: Vec<String>, value: Expr, span: Span },
    Return(Option<Expr>, Span),
    /// `yield <expr>` as a STATEMENT. A function whose body contains any
    /// `Stmt::Yield` is a GENERATOR: typeck requires it to be declared
    /// `Iterator[T]` and codegen lowers it LAZILY to an async-coroutine object
    /// (`__PyrstGen<T>`) — each `yield x` becomes `__pyrst_gen_co.yield_(x).await`
    /// inside an `async move` body that runs on demand, one value per `.next()`
    /// poll, not at construction (see `Codegen::emit_func` and the `GEN_PRELUDE`
    /// in `codegen/mod.rs`). This makes an infinite generator (`while True: yield`)
    /// safe to construct — O(1) memory, nothing runs until consumed. `yield` as an
    /// expression, `yield from`, and `send` are intentionally out of scope.
    Yield(Expr, Span),
    If { cond: Expr, then: Vec<Stmt>, elifs: Vec<(Expr, Vec<Stmt>)>, else_: Option<Vec<Stmt>>, span: Span },
    While { cond: Expr, body: Vec<Stmt>, span: Span },
    For { targets: Vec<String>, iter: Expr, body: Vec<Stmt>, span: Span },
    Pass(Span),
    Break(Span),
    Continue(Span),
    Assert { cond: Expr, msg: Option<Expr>, span: Span },
    Raise { exc: Option<Expr>, span: Span },
    Try {
        body: Vec<Stmt>,
        handlers: Vec<ExceptHandler>,
        else_: Option<Vec<Stmt>>,
        finally_: Option<Vec<Stmt>>,
        span: Span,
    },
    With {
        ctx_expr: Expr,
        as_name: Option<String>,
        body: Vec<Stmt>,
        span: Span,
    },
    Del {
        target: Expr,
        span: Span,
    },
    Match {
        subject: Expr,
        arms: Vec<MatchArm>,
        span: Span,
    },
    Func(Func),
    Class(ClassDef),
    /// (W4-a) `global NAME[, NAME...]` — a function-body declaration that the
    /// listed names refer to the enclosing MODULE's bindings, not function locals.
    /// A subsequent rebind of such a name writes the module-level mutable static
    /// (`thread_local!`) instead of creating a function-local (Python's own
    /// explicit-intent marker for module-level mutable state). Parsed anywhere a
    /// statement is; typeck rejects it at MODULE level (there is no enclosing
    /// function to reach out of) and validates each name is a real module binding.
    /// Carries no runtime effect of its own — codegen emits nothing for it.
    Global { names: Vec<String>, span: Span },
    /// (W4-a) `nonlocal NAME[, NAME...]` — honestly DEFERRED. Rebinding an enclosing
    /// function's local from an inner closure needs shared-mutable frame capture,
    /// which EPIC-4's clone-on-capture value semantics disallow; typeck emits an
    /// honest error naming the deferral (use a class field, a returned value, or a
    /// module global via `global`). Parsed so the diagnostic is specific, not a
    /// generic parse failure.
    Nonlocal { names: Vec<String>, span: Span },
    Import { path: Vec<String>, names: Vec<(String, Option<String>)>, span: Span },
    // Assignment targets carry an arbitrary lvalue *base* as a boxed Expr
    // (e.g. `self`, `self.dict`, `rooms[i]`, `a.b`) rather than a bare name, so
    // chained targets like `self.dict[k] = v`, `rooms[i].field = v`, and
    // `a.b.c = v` parse and lower as in-place mutations. `attr`/`idx` are the
    // final field/subscript applied to that base.
    AttrAssign { obj: Box<Expr>, attr: String, value: Expr, span: Span },
    IndexAssign { obj: Box<Expr>, idx: Expr, value: Expr, span: Span },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, FloorDiv, Mod, Pow,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    Is, IsNot, In, NotIn,
    BitAnd, BitOr, BitXor, LShift, RShift,
}

impl BinOp {
    /// (card 333e34a7) The Python arithmetic dunder an ARITHMETIC binary operator
    /// dispatches to when its left operand is a user class — the ONE source of
    /// truth shared by every operator->type layer (the real checker `check_expr`,
    /// the codegen oracles `infer_expr_ty` / `infer_expr_ty_bound`) and codegen's
    /// BinOp / AugAssign emission, so class operator overloading routes identically
    /// everywhere. `None` for the non-arithmetic operators (comparison, logical,
    /// identity, membership, bitwise/shift), which are typed/emitted directly and
    /// never consult a class dunder here.
    pub fn arith_dunder(self) -> Option<&'static str> {
        match self {
            BinOp::Add => Some("__add__"),
            BinOp::Sub => Some("__sub__"),
            BinOp::Mul => Some("__mul__"),
            BinOp::Div => Some("__truediv__"),
            BinOp::FloorDiv => Some("__floordiv__"),
            BinOp::Mod => Some("__mod__"),
            BinOp::Pow => Some("__pow__"),
            _ => None,
        }
    }

    /// (card 4349fe41) The Python comparison dunder a COMPARISON binary operator
    /// dispatches to when its left operand is a user class defining it — the peer
    /// of [`arith_dunder`](Self::arith_dunder) for the rich-comparison protocol,
    /// and the ONE source of truth shared by every operator->type layer (the real
    /// checker `check_expr`, the codegen oracle `infer_expr_ty`) and codegen's
    /// BinOp emission, so a class comparison overload routes identically
    /// everywhere. When it resolves to a defined method, `a <cmp> b` has that
    /// method's declared return type (e.g. a boolean-mask class for `df["x"] > 3`)
    /// and codegen desugars to `a.<dunder>(b)` instead of a native Rust compare.
    ///
    /// SCOPE — returns `Some` for ONLY the four comparison dunders codegen emits as
    /// ORDINARY INHERENT methods (`fn __gt__(&self, other) -> RET`), which can
    /// therefore return any type:
    ///   Gt→`__gt__`  Le→`__le__`  Ge→`__ge__`  Ne→`__ne__`.
    ///
    /// `Lt` (`__lt__`) and `Eq` (`__eq__`) return `None` DELIBERATELY: they are
    /// `DUNDER_TRAIT_NAMES` (src/codegen/mod.rs) lowered to `impl PartialOrd`
    /// (`__lt_impl -> bool`) / `impl PartialEq` (`fn eq -> bool`), whose Rust result
    /// is HARD-LOCKED to `bool` and which also power sorting / dict-keys / min-max /
    /// structural equality. Routing them to a non-bool declared return would be a
    /// check-accept / rustc-E0308 build-fail (and break that machinery), so `<` and
    /// `==` keep their existing bool typing + native emit unchanged — and `==`/`!=`
    /// on builtins, containment (`in`), and dict-key / match equality are never
    /// touched here. `None` for every non-comparison operator too.
    pub fn comparison_dunder(self) -> Option<&'static str> {
        match self {
            BinOp::Gt => Some("__gt__"),
            BinOp::Le => Some("__le__"),
            BinOp::Ge => Some("__ge__"),
            BinOp::Ne => Some("__ne__"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg, Not, BitNot,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    Str(String, Span),
    FStr(Vec<FStrPart>, Span),
    /// A `bytes` literal `b'...'` (W5-a). Carries the decoded raw bytes
    /// (`Ty::Bytes` -> Rust `Vec<u8>`); distinct from `Str` because a `bytes`
    /// holds arbitrary 0x00–0xff values, indexes to `int`, and reprs as `b'...'`.
    Bytes(Vec<u8>, Span),
    Bool(bool, Span),
    None_(Span),
    Ident(String, Span),
    List(Vec<Expr>, Span),
    Tuple(Vec<Expr>, Span),
    // `targets` carries the comprehension loop target(s): a single name for
    // `[x for x in xs]` or several for tuple-unpacking like
    // `[v for k, v in d.items()]` (mirrors `Stmt::For.targets`).
    ListComp { elt: Box<Expr>, targets: Vec<String>, iter: Box<Expr>, cond: Option<Box<Expr>>, span: Span },
    SetComp { elt: Box<Expr>, targets: Vec<String>, iter: Box<Expr>, cond: Option<Box<Expr>>, span: Span },
    DictComp { key: Box<Expr>, val: Box<Expr>, targets: Vec<String>, iter: Box<Expr>, cond: Option<Box<Expr>>, span: Span },
    Dict(Vec<(Expr, Expr)>, Span),
    Set(Vec<Expr>, Span),
    Call { callee: Box<Expr>, args: Vec<Expr>, kwargs: Vec<(String, Expr)>, span: Span },
    Attr { obj: Box<Expr>, name: String, span: Span },
    Index { obj: Box<Expr>, idx: Box<Expr>, span: Span },
    Slice { obj: Box<Expr>, start: Option<Box<Expr>>, stop: Option<Box<Expr>>, step: Option<Box<Expr>>, span: Span },
    BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    UnOp { op: UnOp, expr: Box<Expr>, span: Span },
    Lambda { params: Vec<(String, TypeExpr)>, body: Box<Expr>, span: Span },
    // Conditional expression: `body if test else orelse` (Python's ternary).
    IfExp { test: Box<Expr>, body: Box<Expr>, orelse: Box<Expr>, span: Span },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s) | Expr::Float(_, s) | Expr::Str(_, s) | Expr::FStr(_, s)
            | Expr::Bytes(_, s)
            | Expr::Bool(_, s) | Expr::None_(s) | Expr::Ident(_, s) | Expr::List(_, s) | Expr::Tuple(_, s) | Expr::Dict(_, s) | Expr::Set(_, s) => *s,
            Expr::Call { span, .. } | Expr::Attr { span, .. }
            | Expr::Index { span, .. } | Expr::Slice { span, .. } | Expr::BinOp { span, .. }
            | Expr::UnOp { span, .. } | Expr::ListComp { span, .. } | Expr::SetComp { span, .. } | Expr::DictComp { span, .. } | Expr::Lambda { span, .. } | Expr::IfExp { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Module {
    pub stmts: Vec<Stmt>,
    pub source_path: Option<std::path::PathBuf>,
    /// (W3-1) The canonical DOTTED import path that REACHED this module (`"os"`,
    /// `"os.path"`, `"a.b"`) — the per-module-namespace KEY for owner-first
    /// qualified resolution and (stage 2) owner-qualified emission. Set by the
    /// resolver from the *import path*, NOT the file stem: `lib/os/path.pyrs` has
    /// the ambiguous stem `"path"` but the module id `"os.path"`. `None` for the
    /// ROOT program (the sentinel root, whose own top-level names stay
    /// crate-root-unwrapped) and for the LSP single-file path (which never
    /// resolves imports, so it has no dotted id).
    pub module_id: Option<String>,
}
