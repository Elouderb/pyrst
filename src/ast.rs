use crate::diag::Span;
pub use crate::lexer::FStrPart;

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),                            // int, str, MyClass, etc.
    Generic(String, Vec<TypeExpr>),           // list[int], dict[str, int], tuple[int, str]
    Tuple(Vec<TypeExpr>),                     // (int, str) — tuple type
    None_,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub default: Option<Expr>,
    pub span: Span,
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
    pub span: Span,
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
    AttrAssign { obj: String, attr: String, value: Expr, span: Span },
    IndexAssign { obj: String, idx: Expr, value: Expr, span: Span },
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
    ListComp { elt: Box<Expr>, target: String, iter: Box<Expr>, cond: Option<Box<Expr>>, span: Span },
    Dict(Vec<(Expr, Expr)>, Span),
    Set(Vec<Expr>, Span),
    Call { callee: Box<Expr>, args: Vec<Expr>, kwargs: Vec<(String, Expr)>, span: Span },
    Attr { obj: Box<Expr>, name: String, span: Span },
    Index { obj: Box<Expr>, idx: Box<Expr>, span: Span },
    BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    UnOp { op: UnOp, expr: Box<Expr>, span: Span },
    Lambda { params: Vec<(String, TypeExpr)>, body: Box<Expr>, span: Span },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s) | Expr::Float(_, s) | Expr::Str(_, s) | Expr::FStr(_, s)
            | Expr::Bool(_, s) | Expr::None_(s) | Expr::Ident(_, s) | Expr::List(_, s) | Expr::Tuple(_, s) | Expr::Dict(_, s) | Expr::Set(_, s) => *s,
            Expr::Call { span, .. } | Expr::Attr { span, .. }
            | Expr::Index { span, .. } | Expr::BinOp { span, .. }
            | Expr::UnOp { span, .. } | Expr::ListComp { span, .. } | Expr::Lambda { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Module {
    pub stmts: Vec<Stmt>,
    pub source_path: Option<std::path::PathBuf>,
}
