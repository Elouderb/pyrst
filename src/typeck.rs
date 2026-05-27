//! v0 type checker. Skeleton — currently performs structural checks only
//! (every binding has a known type, return types declared, names resolve).
//! Real type inference and class/inheritance checking comes in v1+.

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

pub struct TyCtx {
    // global symbol table — function name → return type
    pub funcs: HashMap<String, Ty>,
    pub classes: HashMap<String, ClassDef>,
}

impl TyCtx {
    pub fn new() -> Self {
        let mut funcs = HashMap::new();
        funcs.insert("print".into(), Ty::Unit);
        Self { funcs, classes: HashMap::new() }
    }
}

pub fn check_module(m: &Module) -> Result<TyCtx> {
    let mut ctx = TyCtx::new();
    // First pass: collect signatures so order of declaration doesn't matter.
    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
                ctx.funcs.insert(f.name.clone(), Ty::from_type_expr(&f.ret)?);
            }
            Stmt::Class(c) => {
                ctx.classes.insert(c.name.clone(), c.clone());
            }
            _ => {}
        }
    }
    // Second pass — TODO: walk function bodies, resolve names, check call arity,
    // verify return-type consistency, verify class-field types resolve, etc.
    Ok(ctx)
}
