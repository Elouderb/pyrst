//! Pratt-style parser for the pyrst v0 subset.
//!
//! v0 grammar (informal):
//!   module    := stmt*
//!   stmt      := simple NEWLINE | compound
//!   simple    := pass | break | continue | return [expr]
//!              | ident (":" type)? "=" expr
//!              | ident augop expr
//!              | expr
//!   compound  := if | while | def | class
//!   block     := NEWLINE INDENT stmt+ DEDENT
//!
//! v0 deliberately omits: for, match, decorators, multi-target assignment,
//! unpacking, comprehensions, lambdas, with, try/except, async.

use crate::ast::*;
use crate::diag::{Error, Result, Span};
use crate::lexer::{Tok, Token};

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Self { toks, pos: 0 }
    }

    fn peek(&self) -> &Tok { &self.toks[self.pos].tok }
    fn peek_span(&self) -> Span { self.toks[self.pos].span }
    fn peek2(&self) -> Option<&Tok> { self.toks.get(self.pos + 1).map(|t| &t.tok) }
    fn bump(&mut self) -> Token { let t = self.toks[self.pos].clone(); self.pos += 1; t }

    fn eat(&mut self, want: &Tok) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(want) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, want: &Tok, ctx: &str) -> Result<Token> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(want) {
            Ok(self.bump())
        } else {
            Err(Error::Parse {
                span: self.peek_span(),
                msg: format!("expected {} ({}), found {:?}", tok_name(want), ctx, self.peek()),
            })
        }
    }

    pub fn parse_module(&mut self) -> Result<Module> {
        let mut stmts = Vec::new();
        // skip leading blank newlines
        while matches!(self.peek(), Tok::Newline) { self.bump(); }
        while !matches!(self.peek(), Tok::Eof) {
            stmts.push(self.parse_stmt()?);
            while matches!(self.peek(), Tok::Newline) { self.bump(); }
        }
        Ok(Module { stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        match self.peek() {
            Tok::Def => self.parse_def().map(Stmt::Func),
            Tok::Class => self.parse_class().map(Stmt::Class),
            Tok::If => self.parse_if(),
            Tok::While => self.parse_while(),
            Tok::For => self.parse_for(),
            Tok::Pass => {
                let span = self.peek_span();
                self.bump();
                self.eat_newline()?;
                Ok(Stmt::Pass(span))
            }
            Tok::Break => {
                let span = self.peek_span(); self.bump(); self.eat_newline()?;
                Ok(Stmt::Break(span))
            }
            Tok::Continue => {
                let span = self.peek_span(); self.bump(); self.eat_newline()?;
                Ok(Stmt::Continue(span))
            }
            Tok::Return => {
                let span = self.peek_span();
                self.bump();
                let val = if matches!(self.peek(), Tok::Newline) { None } else { Some(self.parse_expr()?) };
                self.eat_newline()?;
                Ok(Stmt::Return(val, span))
            }
            _ => self.parse_assign_or_expr(),
        }
    }

    fn parse_assign_or_expr(&mut self) -> Result<Stmt> {
        // Try to detect `ident [: type] = expr` or `ident augop expr`.
        let start = self.pos;
        if let Tok::Ident(_) = self.peek() {
            let name_tok = self.bump();
            let name = if let Tok::Ident(s) = &name_tok.tok { s.clone() } else { unreachable!() };

            // Typed binding: ident ":" type "=" expr
            if matches!(self.peek(), Tok::Colon) {
                self.bump();
                let ty = self.parse_type()?;
                self.expect(&Tok::Assign, "typed binding")?;
                let value = self.parse_expr()?;
                self.eat_newline()?;
                return Ok(Stmt::Assign { target: name, ty: Some(ty), value, span: name_tok.span });
            }
            // Plain assignment
            if matches!(self.peek(), Tok::Assign) {
                self.bump();
                let value = self.parse_expr()?;
                self.eat_newline()?;
                return Ok(Stmt::Assign { target: name, ty: None, value, span: name_tok.span });
            }
            // Augmented assignment
            let aug = match self.peek() {
                Tok::PlusAssign => Some(BinOp::Add),
                Tok::MinusAssign => Some(BinOp::Sub),
                Tok::StarAssign => Some(BinOp::Mul),
                Tok::SlashAssign => Some(BinOp::Div),
                _ => None,
            };
            if let Some(op) = aug {
                self.bump();
                let value = self.parse_expr()?;
                self.eat_newline()?;
                return Ok(Stmt::AugAssign { target: name, op, value, span: name_tok.span });
            }
            // Not an assignment — back off and parse as expression.
            self.pos = start;
        }
        let e = self.parse_expr()?;
        self.eat_newline()?;
        Ok(Stmt::Expr(e))
    }

    fn parse_def(&mut self) -> Result<Func> {
        let span = self.peek_span();
        self.expect(&Tok::Def, "def")?;
        let name = self.expect_ident("function name")?;
        self.expect(&Tok::LParen, "def")?;
        let mut params = Vec::new();
        if !matches!(self.peek(), Tok::RParen) {
            loop {
                params.push(self.parse_param()?);
                if !self.eat(&Tok::Comma) { break; }
            }
        }
        self.expect(&Tok::RParen, "def")?;
        let ret = if self.eat(&Tok::Arrow) {
            self.parse_type()?
        } else {
            // v0: require explicit return type. Strict typing.
            return Err(Error::Parse {
                span: self.peek_span(),
                msg: "function must declare a return type with `->`".into(),
            });
        };
        self.expect(&Tok::Colon, "def")?;
        let body = self.parse_block()?;
        Ok(Func { name, params, ret, body, span, is_method: false })
    }

    fn parse_param(&mut self) -> Result<Param> {
        let span = self.peek_span();
        let name = self.expect_ident("parameter name")?;
        // `self` parameter has no type annotation in Python — it is always the receiver.
        if name == "self" {
            return Ok(Param { name, ty: TypeExpr::Named("Self_".to_string()), default: None, span });
        }
        self.expect(&Tok::Colon, "parameter — type annotation required")?;
        let ty = self.parse_type()?;
        let default = if self.eat(&Tok::Assign) { Some(self.parse_expr()?) } else { None };
        Ok(Param { name, ty, default, span })
    }

    fn parse_class(&mut self) -> Result<ClassDef> {
        let span = self.peek_span();
        self.expect(&Tok::Class, "class")?;
        let name = self.expect_ident("class name")?;
        let mut bases = Vec::new();
        if self.eat(&Tok::LParen) {
            if !matches!(self.peek(), Tok::RParen) {
                loop {
                    bases.push(self.expect_ident("base class")?);
                    if !self.eat(&Tok::Comma) { break; }
                }
            }
            self.expect(&Tok::RParen, "class bases")?;
        }
        self.expect(&Tok::Colon, "class")?;
        // class body: zero or more `name: type [= default]` fields, then methods.
        self.expect(&Tok::Newline, "class body")?;
        self.expect(&Tok::Indent, "class body")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
            match self.peek() {
                Tok::Def => {
                    let mut m = self.parse_def()?;
                    m.is_method = true;
                    methods.push(m);
                }
                Tok::Pass => { self.bump(); self.eat_newline()?; }
                Tok::Ident(_) => {
                    // Class field: name ":" type ["=" default] NEWLINE
                    let field_span = self.peek_span();
                    let fname = self.expect_ident("class field")?;
                    self.expect(&Tok::Colon, "class field — type required")?;
                    let ty = self.parse_type()?;
                    let default = if self.eat(&Tok::Assign) { Some(self.parse_expr()?) } else { None };
                    self.eat_newline()?;
                    fields.push(Param { name: fname, ty, default, span: field_span });
                }
                other => return Err(Error::Parse {
                    span: self.peek_span(),
                    msg: format!("unexpected {:?} in class body", other),
                }),
            }
        }
        self.expect(&Tok::Dedent, "class body")?;
        Ok(ClassDef { name, bases, fields, methods, span })
    }

    fn parse_if(&mut self) -> Result<Stmt> {
        let span = self.peek_span();
        self.expect(&Tok::If, "if")?;
        let cond = self.parse_expr()?;
        self.expect(&Tok::Colon, "if")?;
        let then = self.parse_block()?;
        let mut elifs = Vec::new();
        while matches!(self.peek(), Tok::Elif) {
            self.bump();
            let c = self.parse_expr()?;
            self.expect(&Tok::Colon, "elif")?;
            let b = self.parse_block()?;
            elifs.push((c, b));
        }
        let else_ = if self.eat(&Tok::Else) {
            self.expect(&Tok::Colon, "else")?;
            Some(self.parse_block()?)
        } else {
            None
        };
        Ok(Stmt::If { cond, then, elifs, else_, span })
    }

    fn parse_while(&mut self) -> Result<Stmt> {
        let span = self.peek_span();
        self.expect(&Tok::While, "while")?;
        let cond = self.parse_expr()?;
        self.expect(&Tok::Colon, "while")?;
        let body = self.parse_block()?;
        Ok(Stmt::While { cond, body, span })
    }

    fn parse_for(&mut self) -> Result<Stmt> {
        let span = self.peek_span();
        self.expect(&Tok::For, "for")?;
        let target = self.expect_ident("for loop target")?;
        self.expect(&Tok::In, "for loop")?;
        let iter = self.parse_expr()?;
        self.expect(&Tok::Colon, "for loop")?;
        let body = self.parse_block()?;
        Ok(Stmt::For { target, iter, body, span })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>> {
        self.expect(&Tok::Newline, "start of block")?;
        self.expect(&Tok::Indent, "start of block")?;
        let mut stmts = Vec::new();
        while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
            stmts.push(self.parse_stmt()?);
            while matches!(self.peek(), Tok::Newline) { self.bump(); }
        }
        self.expect(&Tok::Dedent, "end of block")?;
        Ok(stmts)
    }

    fn parse_type(&mut self) -> Result<TypeExpr> {
        if matches!(self.peek(), Tok::None_) { self.bump(); return Ok(TypeExpr::None_); }
        let name = self.expect_ident("type name")?;
        if self.eat(&Tok::LBracket) {
            let mut args = Vec::new();
            if !matches!(self.peek(), Tok::RBracket) {
                loop {
                    args.push(self.parse_type()?);
                    if !self.eat(&Tok::Comma) { break; }
                }
            }
            self.expect(&Tok::RBracket, "generic type args")?;
            Ok(TypeExpr::Generic(name, args))
        } else {
            Ok(TypeExpr::Named(name))
        }
    }

    fn eat_newline(&mut self) -> Result<()> {
        // Newline OR an immediate Dedent/Eof are all valid statement terminators.
        if matches!(self.peek(), Tok::Newline) { self.bump(); Ok(()) }
        else if matches!(self.peek(), Tok::Dedent | Tok::Eof | Tok::Semicolon) { Ok(()) }
        else {
            Err(Error::Parse {
                span: self.peek_span(),
                msg: format!("expected end of statement, found {:?}", self.peek()),
            })
        }
    }

    fn expect_ident(&mut self, ctx: &str) -> Result<String> {
        if let Tok::Ident(s) = self.peek().clone() {
            self.bump();
            Ok(s)
        } else {
            Err(Error::Parse {
                span: self.peek_span(),
                msg: format!("expected identifier ({}), found {:?}", ctx, self.peek()),
            })
        }
    }

    // ---- Expressions: Pratt parser ----

    pub fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_and()?;
        while matches!(self.peek(), Tok::Or) {
            let span = self.peek_span(); self.bump();
            let rhs = self.parse_and()?;
            lhs = Expr::BinOp { op: BinOp::Or, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_not()?;
        while matches!(self.peek(), Tok::And) {
            let span = self.peek_span(); self.bump();
            let rhs = self.parse_not()?;
            lhs = Expr::BinOp { op: BinOp::And, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_not(&mut self) -> Result<Expr> {
        if matches!(self.peek(), Tok::Not) {
            let span = self.peek_span(); self.bump();
            let e = self.parse_not()?;
            return Ok(Expr::UnOp { op: UnOp::Not, expr: Box::new(e), span });
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Tok::Eq => BinOp::Eq, Tok::Ne => BinOp::Ne,
                Tok::Lt => BinOp::Lt, Tok::Le => BinOp::Le,
                Tok::Gt => BinOp::Gt, Tok::Ge => BinOp::Ge,
                _ => break,
            };
            let span = self.peek_span(); self.bump();
            let rhs = self.parse_add()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add, Tok::Minus => BinOp::Sub,
                _ => break,
            };
            let span = self.peek_span(); self.bump();
            let rhs = self.parse_mul()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::DoubleSlash => BinOp::FloorDiv,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            let span = self.peek_span(); self.bump();
            let rhs = self.parse_unary()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if matches!(self.peek(), Tok::Minus) {
            let span = self.peek_span(); self.bump();
            let e = self.parse_unary()?;
            return Ok(Expr::UnOp { op: UnOp::Neg, expr: Box::new(e), span });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut e = self.parse_atom()?;
        loop {
            match self.peek() {
                Tok::LParen => {
                    let span = self.peek_span(); self.bump();
                    let mut args: Vec<Expr> = Vec::new();
                    let mut kwargs: Vec<(String, Expr)> = Vec::new();
                    if !matches!(self.peek(), Tok::RParen) {
                        loop {
                            // Detect keyword argument: Ident followed by `=` (not `==`).
                            let is_kw = matches!(self.peek(), Tok::Ident(_))
                                && matches!(self.peek2(), Some(Tok::Assign));
                            if is_kw {
                                let kw_name = self.expect_ident("keyword argument name")?;
                                self.expect(&Tok::Assign, "keyword argument")?;
                                let val = self.parse_expr()?;
                                kwargs.push((kw_name, val));
                            } else {
                                if !kwargs.is_empty() {
                                    return Err(Error::Parse {
                                        span: self.peek_span(),
                                        msg: "positional argument after keyword argument".into(),
                                    });
                                }
                                args.push(self.parse_expr()?);
                            }
                            if !self.eat(&Tok::Comma) { break; }
                        }
                    }
                    self.expect(&Tok::RParen, "call args")?;
                    e = Expr::Call { callee: Box::new(e), args, kwargs, span };
                }
                Tok::Dot => {
                    let span = self.peek_span(); self.bump();
                    let name = self.expect_ident("attribute")?;
                    e = Expr::Attr { obj: Box::new(e), name, span };
                }
                Tok::LBracket => {
                    let span = self.peek_span(); self.bump();
                    let idx = self.parse_expr()?;
                    self.expect(&Tok::RBracket, "index")?;
                    e = Expr::Index { obj: Box::new(e), idx: Box::new(idx), span };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_atom(&mut self) -> Result<Expr> {
        let span = self.peek_span();
        let t = self.bump();
        match t.tok {
            Tok::Int(n) => Ok(Expr::Int(n, span)),
            Tok::Float(f) => Ok(Expr::Float(f, span)),
            Tok::Str(s) => Ok(Expr::Str(s, span)),
            Tok::True => Ok(Expr::Bool(true, span)),
            Tok::False => Ok(Expr::Bool(false, span)),
            Tok::None_ => Ok(Expr::None_(span)),
            Tok::Ident(name) => Ok(Expr::Ident(name, span)),
            Tok::LParen => {
                let e = self.parse_expr()?;
                self.expect(&Tok::RParen, "parenthesized expression")?;
                Ok(e)
            }
            other => Err(Error::Parse {
                span,
                msg: format!("unexpected {:?} in expression", other),
            }),
        }
    }
}

fn tok_name(t: &Tok) -> &'static str {
    match t {
        Tok::LParen => "'('", Tok::RParen => "')'",
        Tok::LBracket => "'['", Tok::RBracket => "']'",
        Tok::Colon => "':'", Tok::Comma => "','",
        Tok::Assign => "'='", Tok::Arrow => "'->'",
        Tok::Newline => "newline", Tok::Indent => "indent", Tok::Dedent => "dedent",
        Tok::Def => "'def'", Tok::Class => "'class'",
        _ => "token",
    }
}

pub fn parse(src: &str) -> Result<Module> {
    let toks = crate::lexer::lex(src)?;
    let mut p = Parser::new(toks);
    p.parse_module()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hello() {
        let m = parse("def main() -> None:\n    print(\"hello\")\n").unwrap();
        assert_eq!(m.stmts.len(), 1);
        assert!(matches!(m.stmts[0], Stmt::Func(_)));
    }

    #[test]
    fn parse_if() {
        let src = "def f(x: int) -> int:\n    if x > 0:\n        return x\n    else:\n        return 0\n";
        let m = parse(src).unwrap();
        assert_eq!(m.stmts.len(), 1);
    }
}
