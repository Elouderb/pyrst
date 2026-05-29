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
        Ok(Module { stmts, source_path: None })
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        match self.peek() {
            Tok::Def => self.parse_def().map(Stmt::Func),
            Tok::Class => self.parse_class().map(Stmt::Class),
            Tok::If => self.parse_if(),
            Tok::While => self.parse_while(),
            Tok::For => self.parse_for(),
            Tok::Import => self.parse_import(),
            Tok::From => self.parse_from_import(),
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
            Tok::Assert => {
                let span = self.peek_span();
                self.bump(); // consume 'assert'
                let cond = self.parse_expr()?;
                let msg = if self.eat(&Tok::Comma) { Some(self.parse_expr()?) } else { None };
                self.eat_newline()?;
                Ok(Stmt::Assert { cond, msg, span })
            }
            Tok::Raise => {
                let span = self.peek_span();
                self.bump(); // consume 'raise'
                let exc = if matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::Dedent) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.eat_newline()?;
                Ok(Stmt::Raise { exc, span })
            }
            Tok::Try => self.parse_try(),
            Tok::With => self.parse_with(),
            Tok::Del => {
                let span = self.peek_span();
                self.bump();
                let target = self.parse_expr()?;
                self.eat_newline()?;
                Ok(Stmt::Del { target, span })
            }
            Tok::Return => {
                let span = self.peek_span();
                self.bump();
                let val = if matches!(self.peek(), Tok::Newline) { None } else { Some(self.parse_expr()?) };
                self.eat_newline()?;
                Ok(Stmt::Return(val, span))
            }
            Tok::At => {
                // Collect decorators before def
                let mut decorators = Vec::new();
                while matches!(self.peek(), Tok::At) {
                    self.bump(); // consume '@'
                    let mut deco_name = String::new();
                    loop {
                        if let Tok::Ident(n) = self.peek().clone() {
                            self.bump();
                            deco_name.push_str(&n);
                            if matches!(self.peek(), Tok::Dot) {
                                self.bump();
                                deco_name.push('.');
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    // Skip decorator arguments e.g. @decorator(arg)
                    if matches!(self.peek(), Tok::LParen) {
                        let mut depth = 1i32;
                        self.bump();
                        while depth > 0 && !matches!(self.peek(), Tok::Eof) {
                            match self.peek() {
                                Tok::LParen => { depth += 1; self.bump(); }
                                Tok::RParen => { depth -= 1; self.bump(); }
                                _ => { self.bump(); }
                            }
                        }
                    }
                    while matches!(self.peek(), Tok::Newline) { self.bump(); }
                    if !deco_name.is_empty() { decorators.push(deco_name); }
                }
                while matches!(self.peek(), Tok::Newline) { self.bump(); }
                match self.peek() {
                    Tok::Def => {
                        let mut f = self.parse_def()?;
                        f.decorators = decorators;
                        Ok(Stmt::Func(f))
                    }
                    _ => self.parse_stmt()
                }
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
                Tok::PercentAssign => Some(BinOp::Mod),
                Tok::DoubleSlashAssign => Some(BinOp::FloorDiv),
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
        // General path: parse expr, then check for attr/index assignment or augmented assignment
        let lhs_expr = self.parse_expr()?;

        // Check for augmented assignment on attributes: self.x += 1
        let aug = match self.peek() {
            Tok::PlusAssign => Some(BinOp::Add),
            Tok::MinusAssign => Some(BinOp::Sub),
            Tok::StarAssign => Some(BinOp::Mul),
            Tok::SlashAssign => Some(BinOp::Div),
            Tok::PercentAssign => Some(BinOp::Mod),
            Tok::DoubleSlashAssign => Some(BinOp::FloorDiv),
            _ => None,
        };

        if let Some(op) = aug {
            if let Expr::Attr { obj, name, span } = lhs_expr {
                self.bump(); // consume augop
                let rhs = self.parse_expr()?;
                let obj_name = match *obj {
                    Expr::Ident(n, _) => n,
                    _ => return Err(Error::Parse {
                        span,
                        msg: "only simple `obj.attr += val` assignment is supported".into(),
                    }),
                };
                // Convert x.attr += y to x.attr = x.attr + y
                let value = Expr::BinOp {
                    op,
                    lhs: Box::new(Expr::Attr {
                        obj: Box::new(Expr::Ident(obj_name.clone(), span)),
                        name: name.clone(),
                        span,
                    }),
                    rhs: Box::new(rhs),
                    span,
                };
                self.eat_newline()?;
                return Ok(Stmt::AttrAssign { obj: obj_name, attr: name, value, span });
            }
        }

        if matches!(self.peek(), Tok::Assign) {
            // Check for tuple unpacking upfront
            if let Expr::Tuple(ref elems, span) = lhs_expr {
                let all_idents = elems.iter().all(|e| matches!(e, Expr::Ident(_, _)));
                if all_idents {
                    let targets: Vec<String> = elems.iter().map(|e| {
                        if let Expr::Ident(n, _) = e { n.clone() } else { unreachable!() }
                    }).collect();
                    self.bump(); // consume '='
                    let value = self.parse_expr()?;
                    self.eat_newline()?;
                    return Ok(Stmt::Unpack { targets, value, span });
                }
            }

            match lhs_expr {
                Expr::Attr { obj, name, span } => {
                    self.bump(); // consume '='
                    let value = self.parse_expr()?;
                    self.eat_newline()?;
                    let obj_name = match *obj {
                        Expr::Ident(n, _) => n,
                        _ => return Err(Error::Parse {
                            span,
                            msg: "only simple `obj.attr = val` assignment is supported".into(),
                        }),
                    };
                    return Ok(Stmt::AttrAssign { obj: obj_name, attr: name, value, span });
                }
                Expr::Index { obj, idx, span } => {
                    self.bump(); // consume '='
                    let value = self.parse_expr()?;
                    self.eat_newline()?;
                    let obj_name = match *obj {
                        Expr::Ident(n, _) => n,
                        _ => return Err(Error::Parse {
                            span,
                            msg: "only simple `obj[idx] = val` assignment is supported".into(),
                        }),
                    };
                    return Ok(Stmt::IndexAssign { obj: obj_name, idx: *idx, value, span });
                }
                _ => {}
            }
        }
        self.eat_newline()?;
        Ok(Stmt::Expr(lhs_expr))
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
        Ok(Func { name, params, ret, body, span, is_method: false, decorators: vec![] })
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
                Tok::At => {
                    // Decorator in class body
                    let mut decorators = Vec::new();
                    while matches!(self.peek(), Tok::At) {
                        self.bump();
                        let mut deco_name = String::new();
                        loop {
                            if let Tok::Ident(n) = self.peek().clone() {
                                self.bump();
                                deco_name.push_str(&n);
                                if matches!(self.peek(), Tok::Dot) {
                                    self.bump();
                                    deco_name.push('.');
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        if matches!(self.peek(), Tok::LParen) {
                            let mut depth = 1i32;
                            self.bump();
                            while depth > 0 && !matches!(self.peek(), Tok::Eof) {
                                match self.peek() {
                                    Tok::LParen => { depth += 1; self.bump(); }
                                    Tok::RParen => { depth -= 1; self.bump(); }
                                    _ => { self.bump(); }
                                }
                            }
                        }
                        while matches!(self.peek(), Tok::Newline) { self.bump(); }
                        if !deco_name.is_empty() { decorators.push(deco_name); }
                    }
                    while matches!(self.peek(), Tok::Newline) { self.bump(); }
                    if matches!(self.peek(), Tok::Def) {
                        let mut m = self.parse_def()?;
                        m.is_method = true;
                        m.decorators = decorators;
                        methods.push(m);
                    }
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

    fn parse_try(&mut self) -> Result<Stmt> {
        let span = self.peek_span();
        self.bump(); // consume 'try'
        self.expect(&Tok::Colon, "try block")?;
        let body = self.parse_block()?;

        let mut handlers = Vec::new();
        let mut else_ = None;
        let mut finally_ = None;

        while matches!(self.peek(), Tok::Except) {
            let h_span = self.peek_span();
            self.bump(); // consume 'except'
            let exc_type = if !matches!(self.peek(), Tok::Colon) {
                if let Tok::Ident(name) = self.peek().clone() {
                    self.bump();
                    Some(name)
                } else {
                    None
                }
            } else {
                None
            };
            let exc_name = if matches!(self.peek(), Tok::As) {
                self.bump();
                if let Tok::Ident(name) = self.peek().clone() {
                    self.bump();
                    Some(name)
                } else {
                    None
                }
            } else {
                None
            };
            self.expect(&Tok::Colon, "except clause")?;
            let h_body = self.parse_block()?;
            handlers.push(ExceptHandler { exc_type, exc_name, body: h_body, span: h_span });
        }

        if matches!(self.peek(), Tok::Else) {
            self.bump();
            self.expect(&Tok::Colon, "else clause")?;
            else_ = Some(self.parse_block()?);
        }

        if matches!(self.peek(), Tok::Finally) {
            self.bump();
            self.expect(&Tok::Colon, "finally clause")?;
            finally_ = Some(self.parse_block()?);
        }

        Ok(Stmt::Try { body, handlers, else_, finally_: finally_, span })
    }

    fn parse_with(&mut self) -> Result<Stmt> {
        let span = self.peek_span();
        self.bump(); // consume 'with'
        let ctx_expr = self.parse_expr()?;
        let as_name = if matches!(self.peek(), Tok::As) {
            self.bump();
            if let Tok::Ident(name) = self.peek().clone() {
                self.bump();
                Some(name)
            } else {
                None
            }
        } else {
            None
        };
        self.expect(&Tok::Colon, "with block")?;
        let body = self.parse_block()?;
        Ok(Stmt::With { ctx_expr, as_name, body, span })
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
        let mut targets = vec![self.expect_ident("for loop target")?];
        while self.eat(&Tok::Comma) {
            targets.push(self.expect_ident("for loop target")?);
        }
        self.expect(&Tok::In, "for loop")?;
        let iter = self.parse_expr()?;
        self.expect(&Tok::Colon, "for loop")?;
        let body = self.parse_block()?;
        Ok(Stmt::For { targets, iter, body, span })
    }

    fn parse_import(&mut self) -> Result<Stmt> {
        let span = self.peek_span();
        self.expect(&Tok::Import, "import")?;
        let mut path = vec![self.expect_ident("module name")?];
        while self.eat(&Tok::Dot) {
            path.push(self.expect_ident("module name")?);
        }
        // Optional `as alias` — consume and discard for v0
        if self.eat(&Tok::As) { self.expect_ident("alias")?; }
        self.eat_newline()?;
        Ok(Stmt::Import { path, names: vec![], span })
    }

    fn parse_from_import(&mut self) -> Result<Stmt> {
        let span = self.peek_span();
        self.expect(&Tok::From, "from")?;
        let mut path = vec![self.expect_ident("module name")?];
        while self.eat(&Tok::Dot) { path.push(self.expect_ident("module name")?); }
        self.expect(&Tok::Import, "import")?;
        let mut names = Vec::new();
        if self.eat(&Tok::LParen) {
            // from module import (A, B, C)
            loop {
                let name = self.expect_ident("import name")?;
                let alias = if self.eat(&Tok::As) { Some(self.expect_ident("alias")?) } else { None };
                names.push((name, alias));
                if !self.eat(&Tok::Comma) { break; }
            }
            self.expect(&Tok::RParen, "import list")?;
        } else {
            // from module import A, B, C  (or just A)
            loop {
                let name = self.expect_ident("import name")?;
                let alias = if self.eat(&Tok::As) { Some(self.expect_ident("alias")?) } else { None };
                names.push((name, alias));
                if !self.eat(&Tok::Comma) { break; }
            }
        }
        self.eat_newline()?;
        Ok(Stmt::Import { path, names, span })
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
        let mut ty = if matches!(self.peek(), Tok::None_) {
            self.bump();
            TypeExpr::None_
        } else {
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
                TypeExpr::Generic(name, args)
            } else {
                TypeExpr::Named(name)
            }
        };

        // Handle union syntax: T | U
        while self.eat(&Tok::Pipe) {
            let rhs = if matches!(self.peek(), Tok::None_) {
                self.bump();
                TypeExpr::None_
            } else {
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
                    TypeExpr::Generic(name, args)
                } else {
                    TypeExpr::Named(name)
                }
            };

            // Fold: T | None → Optional(T); None | T → Optional(T)
            ty = match (&ty, &rhs) {
                (_, TypeExpr::None_) => TypeExpr::Generic("Optional".into(), vec![ty]),
                (TypeExpr::None_, _) => TypeExpr::Generic("Optional".into(), vec![rhs]),
                _ => TypeExpr::Generic("Union".into(), vec![ty, rhs]),
            };
        }

        Ok(ty)
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
        let mut lhs = self.parse_bitor()?;
        loop {
            match self.peek() {
                Tok::Is => {
                    let span = self.peek_span(); self.bump();
                    let op = if self.eat(&Tok::Not) { BinOp::IsNot } else { BinOp::Is };
                    let rhs = self.parse_bitor()?;
                    lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
                }
                Tok::In => {
                    let span = self.peek_span(); self.bump();
                    let rhs = self.parse_bitor()?;
                    lhs = Expr::BinOp { op: BinOp::In, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
                }
                Tok::Not => {
                    if matches!(self.peek2(), Some(Tok::In)) {
                        let span = self.peek_span(); self.bump(); self.bump();
                        let rhs = self.parse_bitor()?;
                        lhs = Expr::BinOp { op: BinOp::NotIn, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
                    } else {
                        break;
                    }
                }
                _ => {
                    let op = match self.peek() {
                        Tok::Eq => BinOp::Eq, Tok::Ne => BinOp::Ne,
                        Tok::Lt => BinOp::Lt, Tok::Le => BinOp::Le,
                        Tok::Gt => BinOp::Gt, Tok::Ge => BinOp::Ge,
                        _ => break,
                    };
                    let span = self.peek_span(); self.bump();
                    let rhs = self.parse_bitor()?;
                    lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
                }
            }
        }
        Ok(lhs)
    }

    fn parse_bitor(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_bitxor()?;
        loop {
            let op = match self.peek() {
                Tok::Pipe => BinOp::BitOr,
                _ => break,
            };
            let span = self.peek_span(); self.bump();
            let rhs = self.parse_bitxor()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_bitxor(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_bitand()?;
        loop {
            let op = match self.peek() {
                Tok::Caret => BinOp::BitXor,
                _ => break,
            };
            let span = self.peek_span(); self.bump();
            let rhs = self.parse_bitand()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_bitand(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_shift()?;
        loop {
            let op = match self.peek() {
                Tok::Amp => BinOp::BitAnd,
                _ => break,
            };
            let span = self.peek_span(); self.bump();
            let rhs = self.parse_shift()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Tok::LShift => BinOp::LShift,
                Tok::RShift => BinOp::RShift,
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
        match self.peek() {
            Tok::Minus => {
                let span = self.peek_span(); self.bump();
                let e = self.parse_unary()?;
                return Ok(Expr::UnOp { op: UnOp::Neg, expr: Box::new(e), span });
            }
            Tok::Tilde => {
                let span = self.peek_span(); self.bump();
                let e = self.parse_unary()?;
                return Ok(Expr::UnOp { op: UnOp::BitNot, expr: Box::new(e), span });
            }
            _ => {}
        }
        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_postfix()?;
        if matches!(self.peek(), Tok::DoubleStar) {
            let span = self.peek_span(); self.bump();
            let rhs = self.parse_unary()?;  // Right-associative
            lhs = Expr::BinOp { op: BinOp::Pow, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
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
            Tok::FStr(parts) => Ok(Expr::FStr(parts, span)),
            Tok::True => Ok(Expr::Bool(true, span)),
            Tok::False => Ok(Expr::Bool(false, span)),
            Tok::None_ => Ok(Expr::None_(span)),
            Tok::Ident(name) => Ok(Expr::Ident(name, span)),
            Tok::LParen => {
                if matches!(self.peek(), Tok::RParen) {
                    // Empty tuple ()
                    self.bump();
                    return Ok(Expr::Tuple(vec![], span));
                }
                let first = self.parse_expr()?;
                if self.eat(&Tok::Comma) {
                    // Tuple: (e1, e2, ...)
                    let mut elems = vec![first];
                    while !matches!(self.peek(), Tok::RParen | Tok::Eof) {
                        elems.push(self.parse_expr()?);
                        if !self.eat(&Tok::Comma) { break; }
                    }
                    self.expect(&Tok::RParen, "tuple")?;
                    Ok(Expr::Tuple(elems, span))
                } else {
                    // Grouped expression: (e)
                    self.expect(&Tok::RParen, "grouped expression")?;
                    Ok(first)
                }
            }
            Tok::LBracket => {
                if matches!(self.peek(), Tok::RBracket) {
                    // Empty list []
                    self.bump();
                    return Ok(Expr::List(vec![], span));
                }
                let first = self.parse_expr()?;
                if matches!(self.peek(), Tok::For) {
                    // List comprehension: [elt for target in iter (if cond)?]
                    self.bump(); // consume 'for'
                    let target = self.expect_ident("list comp target")?;
                    self.expect(&Tok::In, "list comp")?;
                    let iter = self.parse_expr()?;
                    let cond = if self.eat(&Tok::If) { Some(Box::new(self.parse_expr()?)) } else { None };
                    self.expect(&Tok::RBracket, "list comp")?;
                    Ok(Expr::ListComp { elt: Box::new(first), target, iter: Box::new(iter), cond, span })
                } else {
                    // Regular list: [e1, e2, ...]
                    let mut elems = vec![first];
                    while self.eat(&Tok::Comma) && !matches!(self.peek(), Tok::RBracket) {
                        elems.push(self.parse_expr()?);
                    }
                    self.expect(&Tok::RBracket, "list literal")?;
                    Ok(Expr::List(elems, span))
                }
            }
            Tok::LBrace => {
                let mut pairs = Vec::new();
                if !matches!(self.peek(), Tok::RBrace) {
                    loop {
                        let key = self.parse_expr()?;
                        self.expect(&Tok::Colon, "dict literal")?;
                        let val = self.parse_expr()?;
                        pairs.push((key, val));
                        if !self.eat(&Tok::Comma) { break; }
                    }
                }
                self.expect(&Tok::RBrace, "dict literal")?;
                Ok(Expr::Dict(pairs, span))
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
