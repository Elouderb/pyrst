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
}
