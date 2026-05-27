//! v0 type checker with full function body type checking, name resolution, and arity checking.

use std::collections::HashMap;

use crate::ast::*;
use crate::diag::{Error, Result, Span};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Unit,            // maps to Rust ()
    List(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Class(String),
    Unknown,
}

impl Ty {
    pub fn from_type_expr(t: &TypeExpr) -> Result<Ty> {
        Ok(match t {
            TypeExpr::None_ => Ty::Unit,
            TypeExpr::Named(n) => match n.as_str() {
                "int" => Ty::Int,
                "float" => Ty::Float,
                "bool" => Ty::Bool,
                "str" => Ty::Str,
                other => Ty::Class(other.to_string()),
            },
            TypeExpr::Generic(n, args) => match (n.as_str(), args.as_slice()) {
                ("list", [t]) => Ty::List(Box::new(Ty::from_type_expr(t)?)),
                ("dict", [k, v]) => Ty::Dict(Box::new(Ty::from_type_expr(k)?), Box::new(Ty::from_type_expr(v)?)),
                (other, _) => return Err(Error::Type {
                    span: Span::DUMMY,
                    msg: format!("unknown generic type `{}`", other),
                }),
            },
        })
    }
}

#[derive(Debug, Clone)]
pub struct FuncSig {
    pub params: Vec<(String, Ty)>,
    pub ret: Ty,
}

pub struct TyCtx {
    // global symbol table — function name → signature (params + return type)
    pub funcs: HashMap<String, FuncSig>,
    pub classes: HashMap<String, ClassDef>,
}

impl TyCtx {
    pub fn new() -> Self {
        let mut funcs = HashMap::new();
        // print is variadic in Python; use Unknown for param type
        funcs.insert("print".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Unit,
        });
        // range(n) returns an iterable; use Unknown for return type since we don't have an iterator type
        funcs.insert("range".into(), FuncSig {
            params: vec![("n".into(), Ty::Int)],
            ret: Ty::Unknown,
        });
        Self { funcs, classes: HashMap::new() }
    }
}

// Local scope during function body type checking.
struct FuncEnv<'a> {
    ctx: &'a TyCtx,
    locals: HashMap<String, Ty>,
    ret_ty: Ty,
}

impl<'a> FuncEnv<'a> {
    fn new(ctx: &'a TyCtx, params: &[(String, Ty)], ret_ty: Ty) -> Self {
        let mut locals = HashMap::new();
        for (name, ty) in params {
            locals.insert(name.clone(), ty.clone());
        }
        FuncEnv { ctx, locals, ret_ty }
    }

    fn lookup(&self, name: &str) -> Option<Ty> {
        self.locals.get(name).cloned()
            .or_else(|| self.ctx.funcs.get(name).map(|sig| sig.ret.clone()))
            .or_else(|| {
                if self.ctx.classes.contains_key(name) {
                    Some(Ty::Class(name.to_string()))
                } else {
                    None
                }
            })
    }
}

pub fn check_module(m: &Module) -> Result<TyCtx> {
    let mut ctx = TyCtx::new();

    // First pass: collect function signatures and class definitions.
    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
                let params: Result<Vec<(String, Ty)>> = f.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| Ok((p.name.clone(), Ty::from_type_expr(&p.ty)?)))
                    .collect();
                ctx.funcs.insert(f.name.clone(), FuncSig {
                    params: params?,
                    ret: Ty::from_type_expr(&f.ret)?,
                });
            }
            Stmt::Class(c) => {
                ctx.classes.insert(c.name.clone(), c.clone());
                // Register each method as a function under "ClassName.method".
                for m_fn in &c.methods {
                    let params: Result<Vec<(String, Ty)>> = m_fn.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| Ok((p.name.clone(), Ty::from_type_expr(&p.ty)?)))
                        .collect();
                    ctx.funcs.insert(
                        format!("{}.{}", c.name, m_fn.name),
                        FuncSig { params: params?, ret: Ty::from_type_expr(&m_fn.ret)? },
                    );
                }
            }
            _ => {}
        }
    }

    // Second pass: type-check function bodies.
    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
                let params: Vec<(String, Ty)> = f.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| (p.name.clone(), Ty::from_type_expr(&p.ty).unwrap_or(Ty::Unknown)))
                    .collect();
                let ret = Ty::from_type_expr(&f.ret)?;
                let mut env = FuncEnv::new(&ctx, &params, ret);
                check_body(&f.body, &mut env)?;
            }
            Stmt::Class(c) => {
                for method in &c.methods {
                    let mut params: Vec<(String, Ty)> = method.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| (p.name.clone(), Ty::from_type_expr(&p.ty).unwrap_or(Ty::Unknown)))
                        .collect();
                    // Add `self` as the class type.
                    params.insert(0, ("self".into(), Ty::Class(c.name.clone())));
                    let ret = Ty::from_type_expr(&method.ret)?;
                    let mut env = FuncEnv::new(&ctx, &params, ret);
                    check_body(&method.body, &mut env)?;
                }
            }
            _ => {}
        }
    }

    Ok(ctx)
}

fn check_body(stmts: &[Stmt], env: &mut FuncEnv) -> Result<()> {
    for s in stmts {
        check_stmt(s, env)?;
    }
    Ok(())
}

fn check_stmt(s: &Stmt, env: &mut FuncEnv) -> Result<()> {
    match s {
        Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => Ok(()),
        Stmt::Return(None, span) => {
            if env.ret_ty != Ty::Unit {
                return Err(Error::Type {
                    span: *span,
                    msg: format!("bare return in function declared to return {:?}", env.ret_ty),
                });
            }
            Ok(())
        }
        Stmt::Return(Some(e), span) => {
            let ty = check_expr(e, env)?;
            // Lenient check: Unknown is compatible with any type, and vice versa.
            if ty != env.ret_ty && ty != Ty::Unknown && env.ret_ty != Ty::Unknown {
                return Err(Error::Type {
                    span: *span,
                    msg: format!("return type mismatch: expected {:?}, found {:?}", env.ret_ty, ty),
                });
            }
            Ok(())
        }
        Stmt::Expr(e) => {
            check_expr(e, env)?;
            Ok(())
        }
        Stmt::Assign { target, ty, value, span } => {
            let val_ty = check_expr(value, env)?;
            let declared = match ty {
                Some(t) => Ty::from_type_expr(t)?,
                None => val_ty.clone(),
            };
            if let Some(t) = ty {
                let explicit = Ty::from_type_expr(t)?;
                if val_ty != explicit && val_ty != Ty::Unknown && explicit != Ty::Unknown {
                    return Err(Error::Type {
                        span: *span,
                        msg: format!("type mismatch in assignment: declared {:?}, got {:?}", explicit, val_ty),
                    });
                }
            }
            env.locals.insert(target.clone(), declared);
            Ok(())
        }
        Stmt::AugAssign { target, value, span, .. } => {
            if env.locals.get(target.as_str()).is_none() && !env.ctx.funcs.contains_key(target.as_str()) {
                return Err(Error::Type {
                    span: *span,
                    msg: format!("undefined variable `{}`", target),
                });
            }
            check_expr(value, env)?;
            Ok(())
        }
        Stmt::If { cond, then, elifs, else_, .. } => {
            check_expr(cond, env)?;
            check_body(then, env)?;
            for (c, b) in elifs {
                check_expr(c, env)?;
                check_body(b, env)?;
            }
            if let Some(b) = else_ {
                check_body(b, env)?;
            }
            Ok(())
        }
        Stmt::While { cond, body, .. } => {
            check_expr(cond, env)?;
            check_body(body, env)
        }
        Stmt::For { target, iter, body, .. } => {
            check_expr(iter, env)?;
            // Bind the loop variable as Unknown for v0.
            env.locals.insert(target.clone(), Ty::Unknown);
            check_body(body, env)?;
            Ok(())
        }
        Stmt::Func(_) | Stmt::Class(_) => Ok(()), // Nested — punt in v0.
    }
}

fn check_expr(e: &Expr, env: &mut FuncEnv) -> Result<Ty> {
    Ok(match e {
        Expr::Int(_, _) => Ty::Int,
        Expr::Float(_, _) => Ty::Float,
        Expr::Str(_, _) => Ty::Str,
        Expr::Bool(_, _) => Ty::Bool,
        Expr::None_(_) => Ty::Unit,
        Expr::Ident(name, span) => {
            env.lookup(name).ok_or_else(|| Error::Type {
                span: *span,
                msg: format!("undefined name `{}`", name),
            })?
        }
        Expr::Call { callee, args, kwargs, span } => {
            // Check if this is a class constructor or function call.
            match callee.as_ref() {
                Expr::Ident(name, _) => {
                    if let Some(class_def) = env.ctx.classes.get(name.as_str()) {
                        // Constructor call: check that kwarg field names are valid.
                        for (kw, val) in kwargs {
                            if !class_def.fields.iter().any(|f| &f.name == kw) {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!("class `{}` has no field `{}`", name, kw),
                                });
                            }
                            check_expr(val, env)?;
                        }
                        for a in args {
                            check_expr(a, env)?;
                        }
                        Ty::Class(name.clone())
                    } else if let Some(sig) = env.ctx.funcs.get(name.as_str()) {
                        // Regular function call: check arity (positional only in v0).
                        let expected = sig.params.len();
                        let got = args.len() + kwargs.len();
                        // print and range are variadic — skip arity check for them.
                        if name != "print" && name != "range" && got != expected {
                            return Err(Error::Type {
                                span: *span,
                                msg: format!(
                                    "function `{}` takes {} argument(s), {} given",
                                    name, expected, got
                                ),
                            });
                        }
                        for a in args {
                            check_expr(a, env)?;
                        }
                        sig.ret.clone()
                    } else {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("undefined function `{}`", name),
                        });
                    }
                }
                // Method call: e.g., p.magnitude() — callee is Attr
                _ => {
                    check_expr(callee, env)?;
                    for a in args {
                        check_expr(a, env)?;
                    }
                    Ty::Unknown // v0: method return types inferred as Unknown
                }
            }
        }
        Expr::Attr { obj, name, span } => {
            let obj_ty = check_expr(obj, env)?;
            if let Ty::Class(class_name) = &obj_ty {
                if let Some(class_def) = env.ctx.classes.get(class_name.as_str()) {
                    // Check field access.
                    if let Some(field) = class_def.fields.iter().find(|f| &f.name == name) {
                        return Ty::from_type_expr(&field.ty);
                    }
                    // Check method access.
                    if class_def.methods.iter().any(|m| &m.name == name) {
                        return Ok(Ty::Unknown);
                    }
                    return Err(Error::Type {
                        span: *span,
                        msg: format!("class `{}` has no attribute `{}`", class_name, name),
                    });
                }
            }
            Ty::Unknown
        }
        Expr::Index { obj, idx, .. } => {
            check_expr(obj, env)?;
            check_expr(idx, env)?;
            Ty::Unknown
        }
        Expr::BinOp { op, lhs, rhs, .. } => {
            let l = check_expr(lhs, env)?;
            let r = check_expr(rhs, env)?;
            match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le
                | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or => Ty::Bool,
                BinOp::Pow => Ty::Float,
                _ => {
                    // Arithmetic: if both sides match, return that type; else Unknown.
                    if l == r { l } else { Ty::Unknown }
                }
            }
        }
        Expr::UnOp { op, expr, .. } => {
            let t = check_expr(expr, env)?;
            match op {
                UnOp::Not => Ty::Bool,
                UnOp::Neg => t,
            }
        }
    })
}
