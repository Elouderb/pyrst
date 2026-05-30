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
    Set(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Tuple(Vec<Ty>),
    Option(Box<Ty>),
    Class(String),
    Unknown,
}

impl Ty {
    pub fn from_type_expr(t: &TypeExpr) -> Result<Ty> {
        Ok(match t {
            TypeExpr::None_ => Ty::Unit,
            TypeExpr::Named(n) => {
                let stripped = n.trim_matches('\'').trim_matches('"');
                match stripped {
                    "int" => Ty::Int,
                    "float" => Ty::Float,
                    "bool" => Ty::Bool,
                    "str" => Ty::Str,
                    other => Ty::Class(other.to_string()),
                }
            }
            TypeExpr::Generic(n, args) => match (n.as_str(), args.as_slice()) {
                ("list", [t]) => Ty::List(Box::new(Ty::from_type_expr(t)?)),
                ("set", [t]) => Ty::Set(Box::new(Ty::from_type_expr(t)?)),
                ("dict", [k, v]) => Ty::Dict(Box::new(Ty::from_type_expr(k)?), Box::new(Ty::from_type_expr(v)?)),
                ("tuple", args) => Ty::Tuple(args.iter().map(Ty::from_type_expr).collect::<Result<Vec<_>>>()?),
                ("Optional", [t]) => Ty::Option(Box::new(Ty::from_type_expr(t)?)),
                ("Union", args) => {
                    let non_none: Vec<_> = args.iter()
                        .filter(|a| !matches!(a, TypeExpr::None_))
                        .collect();
                    if non_none.len() == 1 {
                        Ty::Option(Box::new(Ty::from_type_expr(non_none[0])?))
                    } else {
                        Ty::Unknown
                    }
                }
                (other, _) => return Err(Error::Type {
                    span: Span::DUMMY,
                    msg: format!("unknown generic type `{}`", other),
                }),
            },
            TypeExpr::Tuple(parts) => {
                let tys = parts.iter().map(Ty::from_type_expr).collect::<Result<Vec<_>>>()?;
                Ty::Tuple(tys)
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct FuncSig {
    pub params: Vec<(String, Ty)>,
    pub ret: Ty,
    pub param_defaults: Vec<Option<Expr>>,  // None = required, Some = has default
}

pub struct TyCtx {
    // global symbol table — function name → signature (params + return type)
    pub funcs: HashMap<String, FuncSig>,
    pub classes: HashMap<String, ClassDef>,
    pub vars: HashMap<String, Ty>,
}

impl TyCtx {
    pub fn new() -> Self {
        let mut funcs = HashMap::new();
        // print is variadic in Python; use Unknown for param type
        funcs.insert("print".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Unit,
            param_defaults: vec![],
        });
        // range(n) returns an iterable; use Unknown for return type since we don't have an iterator type
        funcs.insert("range".into(), FuncSig {
            params: vec![("n".into(), Ty::Int)],
            ret: Ty::Unknown,
            param_defaults: vec![],
        });
        // Core builtins
        funcs.insert("len".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Int,
            param_defaults: vec![],
        });
        funcs.insert("str".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Str,
            param_defaults: vec![],
        });
        funcs.insert("int".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Int,
            param_defaults: vec![],
        });
        funcs.insert("float".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Float,
            param_defaults: vec![],
        });
        funcs.insert("bool".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Bool,
            param_defaults: vec![],
        });
        funcs.insert("enumerate".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Unknown,
            param_defaults: vec![],
        });
        funcs.insert("zip".into(), FuncSig {
            params: vec![("a".into(), Ty::Unknown), ("b".into(), Ty::Unknown)],
            ret: Ty::Unknown,
            param_defaults: vec![],
        });
        funcs.insert("abs".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Int, param_defaults: vec![] });
        funcs.insert("min".into(), FuncSig { params: vec![("a".into(), Ty::Unknown), ("b".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![] });
        funcs.insert("max".into(), FuncSig { params: vec![("a".into(), Ty::Unknown), ("b".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![] });
        funcs.insert("sorted".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![] });
        funcs.insert("sum".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Int, param_defaults: vec![] });
        funcs.insert("input".into(), FuncSig { params: vec![("prompt".into(), Ty::Unknown)], ret: Ty::Str, param_defaults: vec![] });
        funcs.insert("any".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Bool, param_defaults: vec![] });
        funcs.insert("all".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Bool, param_defaults: vec![] });
        funcs.insert("round".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Int, param_defaults: vec![] });
        funcs.insert("pow".into(), FuncSig { params: vec![("base".into(), Ty::Unknown), ("exp".into(), Ty::Unknown)], ret: Ty::Int, param_defaults: vec![] });
        funcs.insert("chr".into(), FuncSig { params: vec![("x".into(), Ty::Int)], ret: Ty::Str, param_defaults: vec![] });
        funcs.insert("ord".into(), FuncSig { params: vec![("x".into(), Ty::Str)], ret: Ty::Int, param_defaults: vec![] });
        funcs.insert("reversed".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![] });
        funcs.insert("map".into(), FuncSig { params: vec![("f".into(), Ty::Unknown), ("x".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![] });
        funcs.insert("filter".into(), FuncSig { params: vec![("f".into(), Ty::Unknown), ("x".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![] });
        funcs.insert("isinstance".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown), ("type_".into(), Ty::Unknown)], ret: Ty::Bool, param_defaults: vec![] });
        funcs.insert("type".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown)], ret: Ty::Str, param_defaults: vec![] });
        funcs.insert("hex".into(), FuncSig { params: vec![("x".into(), Ty::Int)], ret: Ty::Str, param_defaults: vec![] });
        funcs.insert("oct".into(), FuncSig { params: vec![("x".into(), Ty::Int)], ret: Ty::Str, param_defaults: vec![] });
        funcs.insert("bin".into(), FuncSig { params: vec![("x".into(), Ty::Int)], ret: Ty::Str, param_defaults: vec![] });
        funcs.insert("callable".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown)], ret: Ty::Bool, param_defaults: vec![] });

        // Builtin type names for isinstance checks
        let mut vars = HashMap::new();
        vars.insert("int".into(), Ty::Int);
        vars.insert("str".into(), Ty::Str);
        vars.insert("float".into(), Ty::Float);
        vars.insert("bool".into(), Ty::Bool);
        vars.insert("list".into(), Ty::List(Box::new(Ty::Unknown)));
        vars.insert("dict".into(), Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Unknown)));
        vars.insert("set".into(), Ty::Set(Box::new(Ty::Unknown)));

        Self { funcs, classes: HashMap::new(), vars }
    }

    pub fn get_all_fields(&self, class_name: &str) -> Vec<crate::ast::Param> {
        let mut fields = Vec::new();
        let mut visited = std::collections::HashSet::new();
        self.collect_fields(class_name, &mut fields, &mut visited);
        fields
    }

    fn collect_fields(&self, class_name: &str, fields: &mut Vec<crate::ast::Param>, visited: &mut std::collections::HashSet<String>) {
        if visited.contains(class_name) {
            return;
        }
        visited.insert(class_name.to_string());

        if let Some(class_def) = self.classes.get(class_name) {
            // First collect from parent classes
            for base in &class_def.bases {
                self.collect_fields(base, fields, visited);
            }
            // Then add this class's fields
            for field in &class_def.fields {
                fields.push(field.clone());
            }
        }
    }

    pub fn get_method(&self, class_name: &str, method_name: &str) -> Option<FuncSig> {
        let mut visited = std::collections::HashSet::new();
        self.find_method(class_name, method_name, &mut visited)
    }

    fn find_method(&self, class_name: &str, method_name: &str, visited: &mut std::collections::HashSet<String>) -> Option<FuncSig> {
        if visited.contains(class_name) {
            return None;
        }
        visited.insert(class_name.to_string());

        if let Some(class_def) = self.classes.get(class_name) {
            // Check this class's methods
            if let Some(method) = class_def.methods.iter().find(|m| &m.name == method_name) {
                let params: Vec<(String, Ty)> = method.params.iter()
                    .filter(|p| p.name != "self")
                    .filter_map(|p| Ty::from_type_expr(&p.ty).ok().map(|ty| (p.name.clone(), ty)))
                    .collect();
                let ret = Ty::from_type_expr(&method.ret).unwrap_or(Ty::Unknown);
                let param_defaults = method.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| p.default.clone())
                    .collect();
                return Some(FuncSig { params, ret, param_defaults });
            }
            // Check parent classes
            for base in &class_def.bases {
                if let Some(sig) = self.find_method(base, method_name, visited) {
                    return Some(sig);
                }
            }
        }
        None
    }
}

// Local scope during function body type checking.
struct FuncEnv<'a> {
    ctx: &'a TyCtx,
    locals: HashMap<String, Ty>,
    ret_ty: Ty,
    used_vars: std::collections::HashSet<String>,  // Track variable usage for dead code detection
}

impl<'a> FuncEnv<'a> {
    fn new(ctx: &'a TyCtx, params: &[(String, Ty)], ret_ty: Ty) -> Self {
        let mut locals = HashMap::new();
        let mut used_vars = std::collections::HashSet::new();
        for (name, ty) in params {
            locals.insert(name.clone(), ty.clone());
            used_vars.insert(name.clone());  // Parameters are always considered "used"
        }
        FuncEnv { ctx, locals, ret_ty, used_vars }
    }

    fn lookup(&self, name: &str) -> Option<Ty> {
        self.locals.get(name).cloned()
            .or_else(|| self.ctx.vars.get(name).cloned())
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

    // First pass: collect function signatures, class definitions, and module-level variables.
    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
                let params: Result<Vec<(String, Ty)>> = f.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| Ok((p.name.clone(), Ty::from_type_expr(&p.ty)?)))
                    .collect();
                let param_defaults: Vec<Option<Expr>> = f.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| p.default.clone())
                    .collect();
                ctx.funcs.insert(f.name.clone(), FuncSig {
                    params: params?,
                    ret: Ty::from_type_expr(&f.ret)?,
                    param_defaults,
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
                    let param_defaults: Vec<Option<Expr>> = m_fn.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| p.default.clone())
                        .collect();
                    ctx.funcs.insert(
                        format!("{}.{}", c.name, m_fn.name),
                        FuncSig { params: params?, ret: Ty::from_type_expr(&m_fn.ret)?, param_defaults },
                    );
                }
            }
            Stmt::Assign { target, ty, .. } => {
                if let Some(t) = ty {
                    if let Ok(resolved) = Ty::from_type_expr(t) {
                        ctx.vars.insert(target.clone(), resolved);
                    }
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

/// Type-check function/class bodies against a pre-built context.
/// Used for multi-file compilation where the context is merged from all modules.
pub fn check_bodies(m: &Module, ctx: &TyCtx) -> Result<()> {
    // Second pass: type-check function bodies.
    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
                let params: Vec<(String, Ty)> = f.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| (p.name.clone(), Ty::from_type_expr(&p.ty).unwrap_or(Ty::Unknown)))
                    .collect();
                let ret = Ty::from_type_expr(&f.ret)?;
                let mut env = FuncEnv::new(ctx, &params, ret);
                check_body(&f.body, &mut env)?;
            }
            Stmt::Class(c) => {
                for method in &c.methods {
                    let mut params: Vec<(String, Ty)> = method.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| (p.name.clone(), Ty::from_type_expr(&p.ty).unwrap_or(Ty::Unknown)))
                        .collect();
                    params.insert(0, ("self".into(), Ty::Class(c.name.clone())));
                    let ret = Ty::from_type_expr(&method.ret)?;
                    let mut env = FuncEnv::new(ctx, &params, ret);
                    check_body(&method.body, &mut env)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Analyze which functions are actually called in a module.
/// Returns a set of function names that are referenced.
pub fn analyze_called_functions(module: &Module) -> std::collections::HashSet<String> {
    let mut called = std::collections::HashSet::new();

    for stmt in &module.stmts {
        collect_calls_from_stmt(stmt, &mut called);
    }

    called
}

fn collect_calls_from_stmt(stmt: &Stmt, called: &mut std::collections::HashSet<String>) {
    match stmt {
        Stmt::Expr(e) | Stmt::Return(Some(e), _) => collect_calls_from_expr(e, called),
        Stmt::Assign { value, .. } | Stmt::AugAssign { value, .. } => collect_calls_from_expr(value, called),
        Stmt::Unpack { value, .. } => collect_calls_from_expr(value, called),
        Stmt::If { cond, then, elifs, else_, .. } => {
            collect_calls_from_expr(cond, called);
            for s in then { collect_calls_from_stmt(s, called); }
            for (c, b) in elifs {
                collect_calls_from_expr(c, called);
                for s in b { collect_calls_from_stmt(s, called); }
            }
            if let Some(b) = else_ {
                for s in b { collect_calls_from_stmt(s, called); }
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_calls_from_expr(cond, called);
            for s in body { collect_calls_from_stmt(s, called); }
        }
        Stmt::For { iter, body, .. } => {
            collect_calls_from_expr(iter, called);
            for s in body { collect_calls_from_stmt(s, called); }
        }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            for s in body { collect_calls_from_stmt(s, called); }
            for h in handlers {
                for s in &h.body { collect_calls_from_stmt(s, called); }
            }
            if let Some(b) = else_ {
                for s in b { collect_calls_from_stmt(s, called); }
            }
            if let Some(b) = finally_ {
                for s in b { collect_calls_from_stmt(s, called); }
            }
        }
        Stmt::With { ctx_expr, body, .. } => {
            collect_calls_from_expr(ctx_expr, called);
            for s in body { collect_calls_from_stmt(s, called); }
        }
        Stmt::Func(f) => {
            for s in &f.body { collect_calls_from_stmt(s, called); }
        }
        Stmt::Class(c) => {
            for m in &c.methods {
                for s in &m.body { collect_calls_from_stmt(s, called); }
            }
        }
        Stmt::AttrAssign { value, .. } | Stmt::IndexAssign { value, .. } => {
            collect_calls_from_expr(value, called);
        }
        _ => {}
    }
}

fn collect_calls_from_expr(expr: &Expr, called: &mut std::collections::HashSet<String>) {
    match expr {
        Expr::Call { callee, args, .. } => {
            if let Expr::Ident(name, _) = callee.as_ref() {
                called.insert(name.clone());
            }
            for arg in args { collect_calls_from_expr(arg, called); }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_calls_from_expr(lhs, called);
            collect_calls_from_expr(rhs, called);
        }
        Expr::UnOp { expr: e, .. } => collect_calls_from_expr(e, called),
        Expr::List(elems, _) => {
            for e in elems { collect_calls_from_expr(e, called); }
        }
        Expr::Tuple(elems, _) => {
            for e in elems { collect_calls_from_expr(e, called); }
        }
        Expr::Set(elems, _) => {
            for e in elems {
                collect_calls_from_expr(e, called);
            }
        }
        Expr::Dict(pairs, _) => {
            for (k, v) in pairs {
                collect_calls_from_expr(k, called);
                collect_calls_from_expr(v, called);
            }
        }
        Expr::ListComp { elt, iter, cond, .. } => {
            collect_calls_from_expr(elt, called);
            collect_calls_from_expr(iter, called);
            if let Some(c) = cond { collect_calls_from_expr(c, called); }
        }
        Expr::Attr { obj, .. } => collect_calls_from_expr(obj, called),
        Expr::Index { obj, idx, .. } => {
            collect_calls_from_expr(obj, called);
            collect_calls_from_expr(idx, called);
        }
        Expr::Slice { obj, start, stop, step, .. } => {
            collect_calls_from_expr(obj, called);
            if let Some(e) = start { collect_calls_from_expr(e, called); }
            if let Some(e) = stop { collect_calls_from_expr(e, called); }
            if let Some(e) = step { collect_calls_from_expr(e, called); }
        }
        Expr::FStr(parts, _) => {
            for part in parts {
                if let crate::ast::FStrPart::Interp(src, _) = part {
                    // FStr interpolations are stored as strings, not expressions
                    // Would need to parse them again to analyze - skip for now
                }
            }
        }
        Expr::Lambda { body, .. } => {
            collect_calls_from_expr(body, called);
        }
        _ => {}
    }
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
        Stmt::Assert { cond, msg, .. } => {
            check_expr(cond, env)?;
            if let Some(m) = msg { check_expr(m, env)?; }
            Ok(())
        }
        Stmt::Raise { exc, .. } => {
            if let Some(e) = exc { check_expr(e, env)?; }
            Ok(())
        }
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
        Stmt::Unpack { targets, value, .. } => {
            let val_ty = check_expr(value, env)?;
            let elem_tys = match &val_ty {
                Ty::Tuple(tys) => tys.clone(),
                _ => vec![Ty::Unknown; targets.len()],
            };
            for (i, t) in targets.iter().enumerate() {
                let ty = elem_tys.get(i).cloned().unwrap_or(Ty::Unknown);
                env.locals.insert(t.clone(), ty);
            }
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
        Stmt::For { targets, iter, body, .. } => {
            let iter_ty = check_expr(iter, env)?;
            // Determine element type from iterator type
            let elem_ty = match &iter_ty {
                Ty::List(inner) => *inner.clone(),
                _ => Ty::Unknown,
            };
            // Bind all targets
            for target in targets {
                if targets.len() == 1 {
                    // Single target gets the full element type
                    env.locals.insert(target.clone(), elem_ty.clone());
                } else {
                    // Multiple targets: if iter is List<Tuple<T1, T2, ...>>, bind accordingly
                    // For v0, just bind all to Unknown
                    env.locals.insert(target.clone(), Ty::Unknown);
                }
            }
            check_body(body, env)?;
            Ok(())
        }
        Stmt::Import { .. } => Ok(()), // Ignored in v0
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            check_body(body, env)?;
            for h in handlers {
                if let Some(name) = &h.exc_name {
                    env.locals.insert(name.clone(), Ty::Unknown);
                }
                check_body(&h.body, env)?;
            }
            if let Some(b) = else_ { check_body(b, env)?; }
            if let Some(b) = finally_ { check_body(b, env)?; }
            Ok(())
        }
        Stmt::With { ctx_expr, as_name, body, .. } => {
            let ctx_ty = check_expr(ctx_expr, env)?;
            if let Some(name) = as_name {
                env.locals.insert(name.clone(), ctx_ty);
            }
            check_body(body, env)?;
            Ok(())
        }
        Stmt::Del { target, .. } => {
            check_expr(target, env)?;
            Ok(())
        }
        Stmt::Match { subject, arms, .. } => {
            check_expr(subject, env)?;
            for arm in arms {
                // Check guard if present
                if let Some(guard) = &arm.guard {
                    check_expr(guard, env)?;
                }
                // Check body (with capture bindings noted but not applied in our simple impl)
                for s in &arm.body {
                    check_stmt(s, env)?;
                }
            }
            Ok(())
        }
        Stmt::AttrAssign { obj, value, span, .. } => {
            check_expr(value, env)?;
            if env.lookup(obj).is_none() {
                return Err(Error::Type { span: *span, msg: format!("undefined name `{}`", obj) });
            }
            Ok(())
        }
        Stmt::IndexAssign { obj, idx, value, span } => {
            check_expr(idx, env)?;
            check_expr(value, env)?;
            if env.lookup(obj).is_none() {
                return Err(Error::Type { span: *span, msg: format!("undefined name `{}`", obj) });
            }
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
        Expr::FStr(_, _) => Ty::Str,
        Expr::Bool(_, _) => Ty::Bool,
        Expr::Tuple(elems, _) => {
            let tys = elems.iter().map(|e| check_expr(e, env)).collect::<Result<Vec<_>>>()?;
            Ty::Tuple(tys)
        }
        Expr::ListComp { elt, target, iter, cond, .. } => {
            let iter_ty = check_expr(iter, env)?;
            let elem_ty = match &iter_ty {
                Ty::List(inner) => *inner.clone(),
                _ => Ty::Int, // ranges and unknown iterables -> Int
            };
            // Create a new scope with the loop variable bound
            let mut inner_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: env.ret_ty.clone(),
                used_vars: env.used_vars.clone(),
            };
            inner_env.locals.insert(target.clone(), elem_ty);
            if let Some(c) = cond { check_expr(c, &mut inner_env)?; }
            let elt_ty = check_expr(elt, &mut inner_env)?;
            Ty::List(Box::new(elt_ty))
        }
        Expr::None_(_) => Ty::Unit,
        Expr::List(elems, _) => {
            let elem_ty = if elems.is_empty() {
                Ty::Unknown
            } else {
                let first = check_expr(&elems[0], env)?;
                for e in &elems[1..] { check_expr(e, env)?; }
                first
            };
            Ty::List(Box::new(elem_ty))
        }
        Expr::Set(elems, _) => {
            if elems.is_empty() {
                Ty::Set(Box::new(Ty::Unknown))
            } else {
                let elem_ty = check_expr(&elems[0], env)?;
                for e in &elems[1..] { check_expr(e, env)?; }
                Ty::Set(Box::new(elem_ty))
            }
        }
        Expr::Dict(pairs, _) => {
            if pairs.is_empty() {
                Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown))
            } else {
                let k_ty = check_expr(&pairs[0].0, env)?;
                let v_ty = check_expr(&pairs[0].1, env)?;
                for (k, v) in &pairs[1..] { check_expr(k, env)?; check_expr(v, env)?; }
                Ty::Dict(Box::new(k_ty), Box::new(v_ty))
            }
        }
        Expr::Ident(name, span) => {
            // Track variable usage for dead code detection
            if env.locals.contains_key(name.as_str()) {
                env.used_vars.insert(name.clone());
            }
            // Allow standard library modules (math, dataclasses, etc.) to be Ty::Unknown
            if matches!(name.as_str(), "math" | "dataclasses" | "sys" | "os" | "json" | "re" | "collections" | "itertools") {
                Ty::Unknown
            } else {
                env.lookup(name).ok_or_else(|| Error::Type {
                    span: *span,
                    msg: format!("undefined name `{}`", name),
                })?
            }
        }
        Expr::Call { callee, args, kwargs, span } => {
            // Check if this is a class constructor or function call.
            match callee.as_ref() {
                Expr::Ident(name, _) => {
                    if let Some(_class_def) = env.ctx.classes.get(name.as_str()) {
                        // Constructor call: check that kwarg field names are valid (including inherited fields).
                        let all_fields = env.ctx.get_all_fields(name.as_str());
                        for (kw, val) in kwargs {
                            if !all_fields.iter().any(|f| &f.name == kw) {
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
                        // Variadic builtins: skip arity check.
                        let variadic = matches!(name.as_str(),
                            "print" | "range" | "len" | "str" | "int" | "float" | "bool" | "enumerate" | "zip"
                            | "abs" | "min" | "max" | "sorted" | "sum" | "input");
                        // Count required parameters (those without defaults)
                        let required = sig.param_defaults.iter().take_while(|d| d.is_none()).count();
                        if !variadic && (got < required || got > expected) {
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
                    } else if name == "super" && args.is_empty() && kwargs.is_empty() {
                        // super() returns Unknown type — the codegen handles super().method() specially
                        Ty::Unknown
                    } else if let Some(_local_ty) = env.lookup(name) {
                        // Variable call: could be a lambda or any callable
                        // Check arguments but return Unknown for the result type
                        for a in args {
                            check_expr(a, env)?;
                        }
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        Ty::Unknown
                    } else {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("undefined function `{}`", name),
                        });
                    }
                }
                // Method call: e.g., p.magnitude() — callee is Attr
                _ => {
                    if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                        let obj_ty = check_expr(obj, env)?;
                        if let Ty::Class(class_name) = &obj_ty {
                            let key = format!("{}.{}", class_name, name);
                            if let Some(sig) = env.ctx.funcs.get(&key) {
                                for a in args { check_expr(a, env)?; }
                                return Ok(sig.ret.clone());
                            }
                        }
                    }
                    check_expr(callee, env)?;
                    for a in args { check_expr(a, env)?; }
                    Ty::Unknown
                }
            }
        }
        Expr::Attr { obj, name, span } => {
            let obj_ty = check_expr(obj, env)?;
            if let Ty::Class(class_name) = &obj_ty {
                if let Some(_class_def) = env.ctx.classes.get(class_name.as_str()) {
                    // Check field access (including inherited fields).
                    let all_fields = env.ctx.get_all_fields(class_name.as_str());
                    if let Some(field) = all_fields.iter().find(|f| &f.name == name) {
                        return Ty::from_type_expr(&field.ty);
                    }
                    // Check method access (including inherited methods).
                    if let Some(method) = env.ctx.get_method(class_name.as_str(), name) {
                        return Ok(method.ret.clone());
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
            let obj_ty = check_expr(obj, env)?;
            check_expr(idx, env)?;
            match obj_ty {
                Ty::List(inner) => *inner,
                Ty::Dict(_, v) => *v,
                Ty::Str => Ty::Str,
                _ => Ty::Unknown,
            }
        }
        Expr::Slice { obj, start, stop, step, .. } => {
            let obj_ty = check_expr(obj, env)?;
            // Validate slice indices are integers
            for e in &[start.as_ref(), stop.as_ref(), step.as_ref()] {
                if let Some(e) = e {
                    let ty = check_expr(e, env)?;
                    if !matches!(ty, Ty::Int | Ty::Unknown) {
                        return Err(Error::Type {
                            span: e.span(),
                            msg: "slice indices must be integers".into(),
                        });
                    }
                }
            }
            // Slicing a list/string returns the same type
            match obj_ty {
                Ty::List(inner) => Ty::List(inner),
                Ty::Str => Ty::Str,
                _ => Ty::Unknown,
            }
        }
        Expr::BinOp { op, lhs, rhs, .. } => {
            let l = check_expr(lhs, env)?;
            let r = check_expr(rhs, env)?;
            match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le
                | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or
                | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => Ty::Bool,
                BinOp::Pow => Ty::Float,
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::LShift | BinOp::RShift => Ty::Int,
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
                UnOp::BitNot => Ty::Int,
            }
        }
        Expr::Lambda { params, body, .. } => {
            let mut lambda_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: Ty::Unknown,
                used_vars: env.used_vars.clone(),
            };
            for (param_name, param_ty) in params {
                let ty = Ty::from_type_expr(param_ty).unwrap_or(Ty::Unknown);
                lambda_env.locals.insert(param_name.clone(), ty);
            }
            check_expr(body, &mut lambda_env)?;
            Ty::Unknown
        }
    })
}
