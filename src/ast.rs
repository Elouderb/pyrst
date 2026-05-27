use crate::diag::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),                            // int, str, MyClass, etc.
    Generic(String, Vec<TypeExpr>),           // list[int], dict[str, int]
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
}

#[derive(Debug, Clone)]
pub struct ClassDef {
    pub name: String,
    pub bases: Vec<String>,
    pub fields: Vec<Param>,
    pub methods: Vec<Func>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Assign { target: String, ty: Option<TypeExpr>, value: Expr, span: Span },
    AugAssign { target: String, op: BinOp, value: Expr, span: Span },
    Return(Option<Expr>, Span),
    If { cond: Expr, then: Vec<Stmt>, elifs: Vec<(Expr, Vec<Stmt>)>, else_: Option<Vec<Stmt>>, span: Span },
    While { cond: Expr, body: Vec<Stmt>, span: Span },
    For { target: String, iter: Expr, body: Vec<Stmt>, span: Span },
    Pass(Span),
    Break(Span),
    Continue(Span),
    Func(Func),
    Class(ClassDef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, FloorDiv, Mod, Pow,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg, Not,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    Str(String, Span),
    Bool(bool, Span),
    None_(Span),
    Ident(String, Span),
    Call { callee: Box<Expr>, args: Vec<Expr>, kwargs: Vec<(String, Expr)>, span: Span },
    Attr { obj: Box<Expr>, name: String, span: Span },
    Index { obj: Box<Expr>, idx: Box<Expr>, span: Span },
    BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    UnOp { op: UnOp, expr: Box<Expr>, span: Span },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s) | Expr::Float(_, s) | Expr::Str(_, s)
            | Expr::Bool(_, s) | Expr::None_(s) | Expr::Ident(_, s) => *s,
            Expr::Call { span, .. } | Expr::Attr { span, .. }
            | Expr::Index { span, .. } | Expr::BinOp { span, .. }
            | Expr::UnOp { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Module {
    pub stmts: Vec<Stmt>,
}
