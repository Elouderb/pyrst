//! v0 codegen — emits Rust source from a pyrst AST.
//!
//! Ownership strategy for v0: aggressive clone()-on-use. Strings become
//! `String`, lists become `Vec<T>`. Numbers are `Copy` so they pass through.
//! This is the "Pythonic default" that the user signed off on — explicit
//! borrowing comes later.

use std::fmt::Write;

use crate::ast::*;
use crate::diag::Result;
use crate::typeck::{Ty, TyCtx};

pub struct Codegen<'a> {
    pub ctx: &'a TyCtx,
    out: String,
    indent: usize,
}

impl<'a> Codegen<'a> {
    pub fn new(ctx: &'a TyCtx) -> Self {
        Self { ctx, out: String::new(), indent: 0 }
    }

    pub fn emit_module(mut self, m: &Module) -> Result<String> {
        // Preamble — pyrst stdlib shims live here.
        self.line("#![allow(unused_parens, unused_variables, unused_mut, dead_code)]");
        self.line("");
        self.line("// ----- pyrst runtime shims -----");
        self.line("fn __pyrst_print<T: ::std::fmt::Display>(x: T) { println!(\"{}\", x); }");
        self.line("fn __pyrst_range(n: i64) -> ::std::ops::Range<i64> { 0..n }");
        self.line("fn __pyrst_range2(a: i64, b: i64) -> ::std::ops::Range<i64> { a..b }");
        self.line("");
        self.line("// ----- user code -----");

        for s in &m.stmts {
            self.emit_top_stmt(s)?;
        }

        // Synthetic entry point: dispatch to user's `main()` if present.
        if self.ctx.funcs.contains_key("main") {
            self.line("");
            self.line("fn main() { user_main(); }");
        } else {
            self.line("");
            self.line("fn main() {}");
        }

        Ok(self.out)
    }

    fn emit_top_stmt(&mut self, s: &Stmt) -> Result<()> {
        match s {
            Stmt::Func(f) => self.emit_func(f, /*method_of=*/ None),
            Stmt::Class(c) => self.emit_class(c),
            other => {
                // Top-level non-decl statements are not yet supported (would need
                // collecting them into the synthetic main). v0 punts.
                self.line(&format!("// TODO: top-level stmt {:?}", std::mem::discriminant(other)));
                Ok(())
            }
        }
    }

    fn emit_func(&mut self, f: &Func, method_of: Option<&str>) -> Result<()> {
        let name = if f.name == "main" && method_of.is_none() {
            "user_main".to_string()
        } else {
            f.name.clone()
        };
        let mut sig = format!("fn {}(", name);
        let mut first = true;
        // Skip `self` if this is a method — handled separately.
        let params_iter = f.params.iter().filter(|p| !(method_of.is_some() && p.name == "self"));
        if method_of.is_some() && f.params.iter().any(|p| p.name == "self") {
            sig.push_str("&mut self");
            first = false;
        }
        for p in params_iter {
            if !first { sig.push_str(", "); }
            first = false;
            let _ = write!(sig, "{}: {}", p.name, rust_ty(&Ty::from_type_expr(&p.ty)?));
        }
        let ret = Ty::from_type_expr(&f.ret)?;
        let ret_s = rust_ty(&ret);
        let _ = write!(sig, ") -> {} {{", ret_s);
        self.line(&sig);
        self.indent += 1;
        for s in &f.body {
            self.emit_stmt(s)?;
        }
        self.indent -= 1;
        self.line("}");
        self.line("");
        Ok(())
    }

    fn emit_class(&mut self, c: &ClassDef) -> Result<()> {
        // v0 class strategy:
        //   struct Foo { ...fields... }
        //   impl Foo { ...methods... }
        // Inheritance via base classes is recorded in the AST but lowered later;
        // for v0 we emit a TODO and ignore base methods.
        if !c.bases.is_empty() {
            self.line(&format!("// TODO: class `{}` inherits from {:?} (inheritance lowering pending)", c.name, c.bases));
        }
        self.line(&format!("#[derive(Clone, Debug)]"));
        self.line(&format!("struct {} {{", c.name));
        self.indent += 1;
        for f in &c.fields {
            let ty = Ty::from_type_expr(&f.ty)?;
            self.line(&format!("{}: {},", f.name, rust_ty(&ty)));
        }
        self.indent -= 1;
        self.line("}");
        self.line("");
        if !c.methods.is_empty() {
            self.line(&format!("impl {} {{", c.name));
            self.indent += 1;
            for m in &c.methods {
                self.emit_func(m, Some(&c.name))?;
            }
            self.indent -= 1;
            self.line("}");
            self.line("");
        }
        Ok(())
    }

    fn emit_stmt(&mut self, s: &Stmt) -> Result<()> {
        match s {
            Stmt::Pass(_) => self.line("// pass"),
            Stmt::Break(_) => self.line("break;"),
            Stmt::Continue(_) => self.line("continue;"),
            Stmt::Return(None, _) => self.line("return;"),
            Stmt::Return(Some(e), _) => {
                let s = self.emit_expr(e)?;
                self.line(&format!("return {};", s));
            }
            Stmt::Expr(e) => {
                let s = self.emit_expr(e)?;
                self.line(&format!("{};", s));
            }
            Stmt::Assign { target, ty, value, .. } => {
                let v = self.emit_expr(value)?;
                match ty {
                    Some(t) => {
                        let ty = Ty::from_type_expr(t)?;
                        self.line(&format!("let mut {}: {} = {};", target, rust_ty(&ty), v));
                    }
                    None => self.line(&format!("let mut {} = {};", target, v)),
                }
            }
            Stmt::AugAssign { target, op, value, .. } => {
                let v = self.emit_expr(value)?;
                let op_s = match op {
                    BinOp::Add => "+=", BinOp::Sub => "-=", BinOp::Mul => "*=", BinOp::Div => "/=",
                    _ => "+=", // unreachable in current grammar
                };
                self.line(&format!("{} {} {};", target, op_s, v));
            }
            Stmt::If { cond, then, elifs, else_, .. } => {
                let c = self.emit_expr(cond)?;
                self.line(&format!("if {} {{", c));
                self.indent += 1;
                for s in then { self.emit_stmt(s)?; }
                self.indent -= 1;
                for (c, b) in elifs {
                    let cs = self.emit_expr(c)?;
                    self.line(&format!("}} else if {} {{", cs));
                    self.indent += 1;
                    for s in b { self.emit_stmt(s)?; }
                    self.indent -= 1;
                }
                if let Some(b) = else_ {
                    self.line("} else {");
                    self.indent += 1;
                    for s in b { self.emit_stmt(s)?; }
                    self.indent -= 1;
                }
                self.line("}");
            }
            Stmt::While { cond, body, .. } => {
                let c = self.emit_expr(cond)?;
                self.line(&format!("while {} {{", c));
                self.indent += 1;
                for s in body { self.emit_stmt(s)?; }
                self.indent -= 1;
                self.line("}");
            }
            Stmt::For { target, iter, body, .. } => {
                let i = self.emit_expr(iter)?;
                self.line(&format!("for {} in {} {{", target, i));
                self.indent += 1;
                for s in body { self.emit_stmt(s)?; }
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Func(_) | Stmt::Class(_) => {
                // Nested functions/classes — punt.
                self.line("// TODO: nested function/class");
            }
        }
        Ok(())
    }

    fn emit_expr(&mut self, e: &Expr) -> Result<String> {
        Ok(match e {
            Expr::Int(n, _) => format!("({}i64)", n),
            Expr::Float(f, _) => format!("({}f64)", f),
            Expr::Bool(b, _) => b.to_string(),
            Expr::None_(_) => "()".to_string(),
            Expr::Str(s, _) => format!("String::from({:?})", s),
            Expr::Ident(n, _) => n.clone(),
            Expr::Call { callee, args, kwargs, .. } => {
                // Check if this is a class constructor call.
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if let Some(class_def) = self.ctx.classes.get(name.as_str()) {
                        // Class constructor: emit a Rust struct literal.
                        // Clone field names/types to avoid borrow conflict.
                        let field_names: Vec<String> = class_def.fields.iter()
                            .map(|f| f.name.clone()).collect();

                        if !args.is_empty() && kwargs.is_empty() {
                            // Positional args to a class constructor.
                            if args.len() != field_names.len() {
                                return Err(crate::diag::Error::Codegen(format!(
                                    "class `{}` has {} fields but {} positional arguments given",
                                    name, field_names.len(), args.len()
                                )));
                            }
                            let mut parts = Vec::new();
                            for (field_name, arg) in field_names.iter().zip(args.iter()) {
                                let v = self.emit_expr(arg)?;
                                parts.push(format!("{}: {}", field_name, v));
                            }
                            return Ok(format!("{} {{ {} }}", name, parts.join(", ")));
                        }

                        // Keyword-args form.
                        if !kwargs.is_empty() {
                            let mut parts = Vec::new();
                            for (kw, val) in kwargs {
                                let v = self.emit_expr(val)?;
                                parts.push(format!("{}: {}", kw, v));
                            }
                            return Ok(format!("{} {{ {} }}", name, parts.join(", ")));
                        }

                        // No args at all: emit default struct literal.
                        let mut parts = Vec::new();
                        for f in &class_def.fields {
                            let ty = Ty::from_type_expr(&f.ty)?;
                            let default = match ty {
                                Ty::Int => "0i64",
                                Ty::Float => "0.0f64",
                                Ty::Bool => "false",
                                _ => "Default::default()",
                            };
                            parts.push(format!("{}: {}", f.name, default));
                        }
                        return Ok(format!("{} {{ {} }}", name, parts.join(", ")));
                    }
                }

                // Regular function call (not a class).
                let callee_s = match callee.as_ref() {
                    Expr::Ident(n, _) if n == "print" => "__pyrst_print".to_string(),
                    Expr::Ident(n, _) if n == "range" => "__pyrst_range".to_string(),
                    _ => self.emit_expr(callee)?,
                };
                let mut parts = Vec::with_capacity(args.len());
                for a in args { parts.push(self.emit_expr(a)?); }

                // kwargs on a non-class call site are an error in v0.
                if !kwargs.is_empty() {
                    return Err(crate::diag::Error::Codegen(
                        "keyword arguments are only supported for class constructors in v0".into()
                    ));
                }

                format!("{}({})", callee_s, parts.join(", "))
            }
            Expr::Attr { obj, name, .. } => {
                let o = self.emit_expr(obj)?;
                format!("{}.{}", o, name)
            }
            Expr::Index { obj, idx, .. } => {
                let o = self.emit_expr(obj)?;
                let i = self.emit_expr(idx)?;
                format!("{}[{} as usize]", o, i)
            }
            Expr::BinOp { op, lhs, rhs, .. } => {
                let l = self.emit_expr(lhs)?;
                let r = self.emit_expr(rhs)?;
                let op_s = match op {
                    BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*",
                    BinOp::Div => "/", BinOp::FloorDiv => "/", BinOp::Mod => "%",
                    BinOp::Pow => return Ok(format!("(({} as f64).powf({} as f64))", l, r)),
                    BinOp::Eq => "==", BinOp::Ne => "!=",
                    BinOp::Lt => "<", BinOp::Le => "<=",
                    BinOp::Gt => ">", BinOp::Ge => ">=",
                    BinOp::And => "&&", BinOp::Or => "||",
                };
                format!("({} {} {})", l, op_s, r)
            }
            Expr::UnOp { op, expr, .. } => {
                let e = self.emit_expr(expr)?;
                match op {
                    UnOp::Neg => format!("(-{})", e),
                    UnOp::Not => format!("(!{})", e),
                }
            }
        })
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.indent { self.out.push_str("    "); }
        self.out.push_str(s);
        self.out.push('\n');
    }
}

fn rust_ty(t: &Ty) -> String {
    match t {
        Ty::Int => "i64".into(),
        Ty::Float => "f64".into(),
        Ty::Bool => "bool".into(),
        Ty::Str => "String".into(),
        Ty::Unit => "()".into(),
        Ty::List(inner) => format!("Vec<{}>", rust_ty(inner)),
        Ty::Dict(k, v) => format!("::std::collections::HashMap<{}, {}>", rust_ty(k), rust_ty(v)),
        Ty::Class(n) => n.clone(),
        Ty::Unknown => "()".into(),
    }
}

pub fn emit(m: &Module, ctx: &TyCtx) -> Result<String> {
    Codegen::new(ctx).emit_module(m)
}
