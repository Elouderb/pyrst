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
    Unit,            // a void function's `-> None` return; maps to Rust ()
    NoneVal,         // the type of the `None` LITERAL only (distinct from Unit)
    List(Box<Ty>),
    Set(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Tuple(Vec<Ty>),
    Option(Box<Ty>),
    Class(String),
    File,            // an open file handle (open() / `with open(...) as f`)
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
                ("set", [t]) => {
                    // A declared `set[float]` resolves to Set(Float), which
                    // codegen would emit as the uncompilable `HashSet<f64>`.
                    // Reject it at the resolver so vars, params, and returns are
                    // covered uniformly — even when initialized with `set()`.
                    let elem = Ty::from_type_expr(t)?;
                    require_hashable(&elem, Span::DUMMY, "set element")?;
                    Ty::Set(Box::new(elem))
                }
                ("dict", [k, v]) => {
                    // A declared `dict[float, _]` resolves to Dict(Float, _) ->
                    // uncompilable `HashMap<f64, _>`. Reject the KEY only; float
                    // values are fine.
                    let key = Ty::from_type_expr(k)?;
                    require_hashable(&key, Span::DUMMY, "dict key")?;
                    Ty::Dict(Box::new(key), Box::new(Ty::from_type_expr(v)?))
                }
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
                // EPIC-4 V2: `Mut[T]` is a by-reference PARAMETER mode marker, not
                // a type. The parser peels a top-level `Mut[T]` off a parameter
                // annotation (raising `Param.by_ref`), so a `Mut[...]` that reaches
                // here is in an illegal position — a return type, a field/variable
                // annotation, or nested inside another type (e.g. `list[Mut[T]]`).
                ("Mut", _) => return Err(Error::Type {
                    span: Span::DUMMY,
                    msg: "Mut[...] is only valid on a parameter".to_string(),
                }),
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
    /// EPIC-4 V2: per-param by-reference (`Mut[T]`) flag, PARALLEL to `params`
    /// (self filtered out, like `param_defaults`). A kept-as-`Vec<bool>` sidecar
    /// rather than widening the `params` tuple so the many `sig.params` readers
    /// stay untouched. Built-ins and synthetic sigs leave it empty (`vec![]`):
    /// the call-site place-check treats a missing entry as "by value", which is
    /// always correct because no built-in takes a by-ref param.
    pub param_by_ref: Vec<bool>,
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
            param_by_ref: vec![],
        });
        // range(n) yields ints; model as a list of ints so loop vars/usages are
        // typed. NOTE: codegen emits range as a Rust range expr (0..n), not a
        // Vec, so `r: list[int] = range(5)` type-checks here but is rejected by
        // rustc at build. No example relies on that; the model is a pragmatic
        // approximation that buys correct element typing for `for i in range(..)`.
        funcs.insert("range".into(), FuncSig {
            params: vec![("n".into(), Ty::Int)],
            ret: Ty::List(Box::new(Ty::Int)),
            param_defaults: vec![],
            param_by_ref: vec![],
        });
        // Core builtins
        funcs.insert("len".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Int,
            param_defaults: vec![],
            param_by_ref: vec![],
        });
        funcs.insert("str".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Str,
            param_defaults: vec![],
            param_by_ref: vec![],
        });
        funcs.insert("int".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Int,
            param_defaults: vec![],
            param_by_ref: vec![],
        });
        funcs.insert("float".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Float,
            param_defaults: vec![],
            param_by_ref: vec![],
        });
        funcs.insert("bool".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Bool,
            param_defaults: vec![],
            param_by_ref: vec![],
        });
        funcs.insert("enumerate".into(), FuncSig {
            params: vec![("x".into(), Ty::Unknown)],
            ret: Ty::Unknown,
            param_defaults: vec![],
            param_by_ref: vec![],
        });
        funcs.insert("zip".into(), FuncSig {
            params: vec![("a".into(), Ty::Unknown), ("b".into(), Ty::Unknown)],
            ret: Ty::Unknown,
            param_defaults: vec![],
            param_by_ref: vec![],
        });
        funcs.insert("abs".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("min".into(), FuncSig { params: vec![("a".into(), Ty::Unknown), ("b".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("max".into(), FuncSig { params: vec![("a".into(), Ty::Unknown), ("b".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("sorted".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::List(Box::new(Ty::Unknown)), param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("sum".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("input".into(), FuncSig { params: vec![("prompt".into(), Ty::Unknown)], ret: Ty::Str, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("any".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Bool, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("all".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Bool, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("round".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Int, param_defaults: vec![], param_by_ref: vec![] });
        // open(path) / open(path, mode) -> file handle. Arity is not checked
        // (added to the variadic skip list) so the optional mode arg works.
        funcs.insert("open".into(), FuncSig { params: vec![("path".into(), Ty::Str), ("mode".into(), Ty::Str)], ret: Ty::File, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("pow".into(), FuncSig { params: vec![("base".into(), Ty::Unknown), ("exp".into(), Ty::Unknown)], ret: Ty::Int, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("chr".into(), FuncSig { params: vec![("x".into(), Ty::Int)], ret: Ty::Str, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("ord".into(), FuncSig { params: vec![("x".into(), Ty::Str)], ret: Ty::Int, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("reversed".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::List(Box::new(Ty::Unknown)), param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("map".into(), FuncSig { params: vec![("f".into(), Ty::Unknown), ("x".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("filter".into(), FuncSig { params: vec![("f".into(), Ty::Unknown), ("x".into(), Ty::Unknown)], ret: Ty::Unknown, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("isinstance".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown), ("type_".into(), Ty::Unknown)], ret: Ty::Bool, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("type".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown)], ret: Ty::Str, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("hex".into(), FuncSig { params: vec![("x".into(), Ty::Int)], ret: Ty::Str, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("oct".into(), FuncSig { params: vec![("x".into(), Ty::Int)], ret: Ty::Str, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("bin".into(), FuncSig { params: vec![("x".into(), Ty::Int)], ret: Ty::Str, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("callable".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown)], ret: Ty::Bool, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("repr".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown)], ret: Ty::Str, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("ascii".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown)], ret: Ty::Str, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("list".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::List(Box::new(Ty::Unknown)), param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("dict".into(), FuncSig { params: vec![], ret: Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown)), param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("tuple".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Tuple(vec![]), param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("getattr".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown), ("name".into(), Ty::Str)], ret: Ty::Unknown, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("setattr".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown), ("name".into(), Ty::Str), ("value".into(), Ty::Unknown)], ret: Ty::Unit, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("hasattr".into(), FuncSig { params: vec![("obj".into(), Ty::Unknown), ("name".into(), Ty::Str)], ret: Ty::Bool, param_defaults: vec![], param_by_ref: vec![] });
        funcs.insert("set".into(), FuncSig { params: vec![("x".into(), Ty::Unknown)], ret: Ty::Set(Box::new(Ty::Unknown)), param_defaults: vec![], param_by_ref: vec![] });

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
                let param_by_ref = method.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| p.by_ref)
                    .collect();
                return Some(FuncSig { params, ret, param_defaults, param_by_ref });
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

// Extract field assignments from __init__ method
pub fn extract_init_fields(class_def: &mut ClassDef) {
    let mut discovered_fields: std::collections::HashMap<String, TypeExpr> = std::collections::HashMap::new();

    // Find the __init__ method
    for method in &class_def.methods {
        if method.name == "__init__" {
            // Build a map of parameters for type lookups
            let param_types: std::collections::HashMap<String, TypeExpr> = method.params.iter()
                .map(|p| (p.name.clone(), p.ty.clone()))
                .collect();

            // Scan the body for self.attr assignments
            for stmt in &method.body {
                match stmt {
                    Stmt::AttrAssign { obj, attr, value, .. } => {
                        // Only a direct `self.<attr> = ...` declares a field.
                        let is_self = matches!(obj.as_ref(), Expr::Ident(n, _) if n == "self");
                        if is_self && !class_def.fields.iter().any(|f| &f.name == attr) {
                            let inferred_ty = guess_field_type(value, &param_types);
                            discovered_fields.insert(attr.clone(), inferred_ty);
                        }
                    }
                    _ => {}
                }
            }
            break;  // Only process __init__
        }
    }

    // For fields with generic/collection types, try to infer element types from method usage
    let mut updated_fields = std::collections::HashMap::new();
    for (field_name, field_type) in &discovered_fields {
        let updated_type = if let TypeExpr::Generic(coll_type, generic_args) = field_type {
            if coll_type == "list" && generic_args.len() > 0 && generic_args[0] == TypeExpr::Named("int".to_string()) {
                // This was a default inference for an empty list
                // Try to find what type is actually used
                if let Some(inferred) = infer_list_element_type(class_def, field_name) {
                    TypeExpr::Generic("list".to_string(), vec![TypeExpr::Named(inferred)])
                } else {
                    field_type.clone()
                }
            } else {
                field_type.clone()
            }
        } else {
            field_type.clone()
        };
        updated_fields.insert(field_name.clone(), updated_type);
    }

    // Add all discovered fields to the class
    for (attr_name, attr_type) in updated_fields {
        class_def.fields.push(Param {
            name: attr_name,
            ty: attr_type,
            default: None,
            span: Span::DUMMY,
            by_ref: false,
        });
    }
}

// Try to infer the element type of a list field from method calls
fn infer_list_element_type(class_def: &ClassDef, field_name: &str) -> Option<String> {
    // Build a map of parameter names to their types for all methods
    let mut param_types: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for method in &class_def.methods {
        for param in &method.params {
            if param.name != "self" {
                // Extract the type name from the TypeExpr
                if let TypeExpr::Named(name) = &param.ty {
                    param_types.insert(param.name.clone(), name.clone());
                }
            }
        }
    }

    // Look for method calls that push/append to this field
    for method in &class_def.methods {
        if method.name == "__init__" {
            continue;
        }
        for stmt in &method.body {
            if let Stmt::Expr(expr) = stmt {
                // Look for self.field.method(arg) patterns
                if let Expr::Call { callee, args, .. } = expr {
                    if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                        if let Expr::Attr { obj: obj2, name: field, .. } = obj.as_ref() {
                            if let Expr::Ident(self_name, _) = obj2.as_ref() {
                                if self_name == "self" && field == field_name {
                                    // This is self.field.method(args)
                                    if (name == "append" || name == "push") && !args.is_empty() {
                                        // Look at the type of the argument
                                        if let Expr::Ident(arg_name, _) = &args[0] {
                                            if let Some(arg_type) = param_types.get(arg_name) {
                                                return Some(arg_type.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Structural guess for the TypeExpr of an untyped `__init__` field assignment.
/// Returns hardcoded TypeExpr approximations based on the literal or parameter
/// kind of `expr`. This is NOT the inference oracle — it does not resolve
/// bindings, call sites, or return types. The real inference oracle is
/// `infer_expr_ty`. Use this only inside `extract_init_fields` where we need a
/// quick TypeExpr (not a Ty) for field-discovery purposes.
fn guess_field_type(expr: &Expr, params: &std::collections::HashMap<String, TypeExpr>) -> TypeExpr {
    match expr {
        Expr::Int(..) => TypeExpr::Named("int".to_string()),
        Expr::Float(..) => TypeExpr::Named("float".to_string()),
        Expr::Bool(..) => TypeExpr::Named("bool".to_string()),
        Expr::Str(..) => TypeExpr::Named("str".to_string()),
        Expr::Ident(name, _) => {
            // Look up parameter type if available
            params.get(name).cloned().unwrap_or_else(|| TypeExpr::Named("Unknown".to_string()))
        }
        // For collections without more info, use generic with flexible types
        // Empty lists default to list[int] as a reasonable default
        Expr::List(..) => TypeExpr::Generic("list".to_string(), vec![TypeExpr::Named("int".to_string())]),
        Expr::Dict(..) => TypeExpr::Generic("dict".to_string(), vec![
            TypeExpr::Named("str".to_string()),
            TypeExpr::Named("int".to_string()),
        ]),
        Expr::Set(..) => TypeExpr::Generic("set".to_string(), vec![TypeExpr::Named("int".to_string())]),
        Expr::Tuple(..) => TypeExpr::Named("tuple".to_string()),
        Expr::None_(..) => TypeExpr::None_,
        Expr::Call { callee, .. } => {
            if let Expr::Ident(name, _) = callee.as_ref() {
                TypeExpr::Named(name.clone())
            } else {
                TypeExpr::Named("Unknown".to_string())
            }
        }
        _ => TypeExpr::Named("Unknown".to_string()),
    }
}

// Local scope during function body type checking.
struct FuncEnv<'a> {
    ctx: &'a TyCtx,
    locals: HashMap<String, Ty>,
    ret_ty: Ty,
    used_vars: std::collections::HashSet<String>,  // Track variable usage for dead code detection
    /// Names that were original function/method parameters (never changes after construction).
    params: std::collections::HashSet<String>,
    /// Subset of `params` that have been unconditionally reassigned via `Stmt::Assign`.
    /// A param in this set is no longer the original by-value binding.
    reassigned_params: std::collections::HashSet<String>,
    /// Subset of `params` whose name appears (as an Ident) in at least one `return` expression
    /// anywhere in the function body. Mutating and returning a by-value param is the valid
    /// functional pattern — the callee works on its own copy and returns the result; the caller
    /// captures the new value. We suppress the by-value-param-mutation error for these params.
    returned_params: std::collections::HashSet<String>,
    /// EPIC-4 V2: subset of `params` declared `Mut[T]` (by-reference). A by-ref
    /// param's mutation IS visible to the caller, so the by-value mutation
    /// backstop (AttrAssign / IndexAssign / mutating method-call) must NOT fire
    /// for these names.
    by_ref_params: std::collections::HashSet<String>,
}

impl<'a> FuncEnv<'a> {
    /// Build a function-checking environment. `by_ref_names` is the set of
    /// parameter names declared `Mut[T]` (empty for lambdas, test helpers, and
    /// any function with no by-reference params).
    fn with_by_ref(ctx: &'a TyCtx, params: &[(String, Ty)], by_ref_names: &[String], ret_ty: Ty) -> Self {
        let mut locals = HashMap::new();
        let mut used_vars = std::collections::HashSet::new();
        let mut param_set = std::collections::HashSet::new();
        for (name, ty) in params {
            locals.insert(name.clone(), ty.clone());
            used_vars.insert(name.clone());  // Parameters are always considered "used"
            param_set.insert(name.clone());
        }
        let by_ref_params = by_ref_names.iter().cloned().collect();
        FuncEnv { ctx, locals, ret_ty, used_vars, params: param_set, reassigned_params: std::collections::HashSet::new(), returned_params: std::collections::HashSet::new(), by_ref_params }
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

/// Validate that every decorator name in `decorators` is in the supported whitelist.
/// Returns an error pointing at `span` for the first unsupported decorator found.
fn validate_decorators(decorators: &[String], span: Span) -> Result<()> {
    for dec in decorators {
        match dec.as_str() {
            "staticmethod" | "property" | "dataclass" => {}
            _ => {
                return Err(Error::Type {
                    span,
                    msg: format!("decorator `@{}` is not supported", dec),
                });
            }
        }
    }
    Ok(())
}

/// Type-check function/class bodies against a pre-built context.
/// Used for multi-file compilation where the context is merged from all modules.
pub fn check_bodies(m: &Module, ctx: &TyCtx) -> Result<()> {
    // Second pass: type-check function bodies.
    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
                // Reject unsupported decorators on top-level functions.
                validate_decorators(&f.decorators, f.span)?;

                let params: Vec<(String, Ty)> = f.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| Ty::from_type_expr(&p.ty).map(|ty| (p.name.clone(), ty)))
                    .collect::<Result<Vec<_>>>()?;
                let by_ref_names: Vec<String> = f.params.iter()
                    .filter(|p| p.name != "self" && p.by_ref)
                    .map(|p| p.name.clone())
                    .collect();
                let ret = Ty::from_type_expr(&f.ret)?;
                let mut env = FuncEnv::with_by_ref(ctx, &params, &by_ref_names, ret);
                collect_returned_param_idents(&f.body, &env.params, &mut env.returned_params);
                check_body(&f.body, &mut env)?;
            }
            Stmt::Class(c) => {
                // Reject multiple inheritance.
                if c.bases.len() > 1 {
                    return Err(Error::Type {
                        span: c.span,
                        msg: "multiple inheritance is not supported".to_string(),
                    });
                }

                // (EPIC-4 V2-c) Validate explicit class-FIELD annotations at
                // `check` time. Field types are otherwise only lowered lazily at
                // codegen (`build`), so a `Mut[T]` field annotation would slip past
                // `pyrst check`. Running each field through `from_type_expr` here
                // makes the existing `("Mut", _)` rejection arm fire at check time,
                // so a class-field `Mut[T]` is an honest error in BOTH `check` and
                // `build` (mode markers belong only on parameters).
                for field in &c.fields {
                    Ty::from_type_expr(&field.ty)?;
                }

                for method in &c.methods {
                    // Reject unsupported decorators on class methods.
                    validate_decorators(&method.decorators, method.span)?;

                    // `__bool__` is listed among the dunder-trait names in codegen
                    // (so it is skipped by the inherent-methods loop) but has no
                    // trait-impl arm, which would silently DROP a user-defined
                    // `__bool__`. pyrst also has no working object-truthiness
                    // lowering today: `bool(obj)` lowers numerically and an
                    // `if obj:` / `while obj:` condition is not constrained to
                    // `bool`, so a class instance in a truthiness position emits
                    // invalid Rust regardless. Rather than mislead the user with a
                    // silently-ignored method, reject `__bool__` honestly here (it
                    // is then caught by both `check` and `build`). Lowering object
                    // truthiness is a separate, larger feature.
                    if method.name == "__bool__" {
                        return Err(Error::Type {
                            span: method.span,
                            msg: "__bool__ is not yet supported (object truthiness is not lowered); \
                                  define an explicit predicate method instead".to_string(),
                        });
                    }

                    // (EPIC-4 V2-c) `Mut[T]` is unsupported on a CONSTRUCTOR
                    // parameter. The generated `new()` wrapper passes owned values
                    // into `self.__init__(...)`, which would mismatch a `&mut T`
                    // `__init__` signature — and a fresh `__inst` has no
                    // caller-visible storage for a by-ref param to alias anyway.
                    // Reject here so both `check` and `build` catch it cleanly
                    // rather than silently mis-emitting.
                    if method.name == "__init__" {
                        if let Some(p) = method.params.iter().find(|p| p.by_ref) {
                            return Err(Error::Type {
                                span: method.span,
                                msg: format!(
                                    "Mut[T] is not supported on a constructor (`__init__`) parameter `{}`",
                                    p.name
                                ),
                            });
                        }
                    }

                    let mut params: Vec<(String, Ty)> = method.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| Ty::from_type_expr(&p.ty).map(|ty| (p.name.clone(), ty)))
                        .collect::<Result<Vec<_>>>()?;
                    params.insert(0, ("self".into(), Ty::Class(c.name.clone())));
                    let by_ref_names: Vec<String> = method.params.iter()
                        .filter(|p| p.name != "self" && p.by_ref)
                        .map(|p| p.name.clone())
                        .collect();
                    let ret = Ty::from_type_expr(&method.ret)?;
                    let mut env = FuncEnv::with_by_ref(ctx, &params, &by_ref_names, ret);
                    collect_returned_param_idents(&method.body, &env.params, &mut env.returned_params);
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
        Stmt::AttrAssign { obj, value, .. } => {
            collect_calls_from_expr(obj, called);
            collect_calls_from_expr(value, called);
        }
        Stmt::IndexAssign { obj, idx, value, .. } => {
            collect_calls_from_expr(obj, called);
            collect_calls_from_expr(idx, called);
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
        Expr::SetComp { elt, iter, cond, .. } => {
            collect_calls_from_expr(elt, called);
            collect_calls_from_expr(iter, called);
            if let Some(c) = cond { collect_calls_from_expr(c, called); }
        }
        Expr::DictComp { key, val, iter, cond, .. } => {
            collect_calls_from_expr(key, called);
            collect_calls_from_expr(val, called);
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
                if let crate::ast::FStrPart::Interp(inner, _) = part {
                    collect_calls_from_expr(inner, called);
                }
            }
        }
        Expr::Lambda { body, .. } => {
            collect_calls_from_expr(body, called);
        }
        Expr::IfExp { test, body, orelse, .. } => {
            collect_calls_from_expr(test, called);
            collect_calls_from_expr(body, called);
            collect_calls_from_expr(orelse, called);
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

/// Check if two types are compatible for assignment.
/// Collections with Unknown element types are considered compatible with any collection of the same kind.
fn types_compatible(val_ty: &Ty, declared_ty: &Ty) -> bool {
    match (val_ty, declared_ty) {
        // Exact match
        (a, b) if a == b => true,
        // Unknown is compatible with anything
        (Ty::Unknown, _) | (_, Ty::Unknown) => true,
        // List with Unknown elements compatible with any List
        (Ty::List(inner), Ty::List(_)) if **inner == Ty::Unknown => true,
        (Ty::List(_), Ty::List(inner)) if **inner == Ty::Unknown => true,
        // Set with Unknown elements compatible with any Set
        (Ty::Set(inner), Ty::Set(_)) if **inner == Ty::Unknown => true,
        (Ty::Set(_), Ty::Set(inner)) if **inner == Ty::Unknown => true,
        // Dict with Unknown key/value compatible with any Dict
        (Ty::Dict(k, v), Ty::Dict(_, _)) if **k == Ty::Unknown && **v == Ty::Unknown => true,
        (Ty::Dict(_, _), Ty::Dict(k, v)) if **k == Ty::Unknown && **v == Ty::Unknown => true,
        // ── Optional / None ──────────────────────────────────────────────────
        // (EPIC-5) `types_compatible(val_ty, declared_ty)` is directional: it asks
        // whether a value of `val_ty` may flow into a slot of `declared_ty`. The
        // Option arms below are written so a value may FILL an Optional slot, but
        // an Optional value may NOT silently fill a bare slot — using an
        // `Optional[T]` as a bare `T` without narrowing stays an honest error.
        //
        // The `None` LITERAL has its own type `Ty::NoneVal`, kept strictly
        // separate from `Ty::Unit` (a *void function's* `-> None` return). This
        // separation is load-bearing: were they the same, a void call result
        // (`Ty::Unit`) would wrongly satisfy an Optional slot and codegen would
        // emit `Some(void_call())` -> `Option<()>` — a silent miscompile. So a
        // void result is NOT compatible with Optional; only the literal `None` is.
        //
        // 1a. The `None` literal fills any Optional slot regardless of inner type
        //     (`None` is a valid `Optional[Class]`). Placed before the bare-value
        //     arm so it never recurses into the (incompatible) inner type.
        (Ty::NoneVal, Ty::Option(_)) => true,
        // 1b. The `None` literal also satisfies a `-> None` (void) return — this
        //     is what makes `return None` typecheck in a void function (the
        //     Return path compares the value type against the declared Unit ret).
        (Ty::NoneVal, Ty::Unit) => true,
        // 1c. Two `None` literals are mutually compatible (e.g. branch unification
        //     of `None`/`None`, or `x = None` re-checked against itself).
        (Ty::NoneVal, Ty::NoneVal) => true,
        // 2. Optional[A] fills Optional[B] when the inner types are compatible
        //    (covers Optional[Unknown] permissively, and Optional[T]~Optional[T]).
        (Ty::Option(a), Ty::Option(b)) => types_compatible(a, b),
        // 3. A bare value of type A fills Optional[B] when A fits B (auto-Some).
        //    Checked AFTER the Option/Option arm so an Optional value never takes
        //    this path. `NoneVal` is excluded (it is handled by 1a above, never by
        //    recursing into the inner type). Codegen wraps the bare value in
        //    `Some(...)` at the site.
        (a, Ty::Option(b)) if !matches!(a, Ty::Option(_) | Ty::NoneVal) => types_compatible(a, b),
        // Otherwise not compatible
        _ => false,
    }
}

/// (EPIC-5) Recognize a None-guard condition of the form `x is None` /
/// `x is not None` on a plain local name. Returns `(name, is_not_none)` where
/// `is_not_none` is true for `is not None` (the branch in which `x` is the
/// non-None payload). Mirrors codegen's `extract_narrowing` so the two layers
/// agree on which guards narrow.
fn extract_none_guard(cond: &Expr) -> Option<(String, bool)> {
    if let Expr::BinOp { op, lhs, rhs, .. } = cond {
        if matches!(op, BinOp::Is | BinOp::IsNot) && matches!(rhs.as_ref(), Expr::None_(_)) {
            if let Expr::Ident(name, _) = lhs.as_ref() {
                return Some((name.clone(), *op == BinOp::IsNot));
            }
        }
    }
    None
}

/// Unify the two branch types of a conditional expression. Returns the more
/// concrete type when the branches are compatible (an `Unknown`, or a
/// collection with `Unknown` elements, absorbs the concrete side), or `None`
/// when they are genuinely incompatible.
fn unify_branch_types(a: Ty, b: Ty) -> Option<Ty> {
    if !types_compatible(&a, &b) {
        return None;
    }
    Some(match (&a, &b) {
        (Ty::Unknown, _) => b,
        (Ty::List(i), Ty::List(_)) if **i == Ty::Unknown => b,
        (Ty::Set(i), Ty::Set(_)) if **i == Ty::Unknown => b,
        (Ty::Dict(k, v), Ty::Dict(_, _)) if **k == Ty::Unknown && **v == Ty::Unknown => b,
        // `a` is the concrete side (or both equal) -> keep it.
        _ => a,
    })
}

/// Unify the element types of a homogeneous collection literal.
///
/// Returns the unified element type when the two types can coexist in one Rust
/// collection, or `None` when they are genuinely heterogeneous and the literal
/// should be rejected. Stays permissive on `Unknown` (and collections with an
/// `Unknown` inner) via the shared `unify_branch_types` arms; only both-concrete,
/// non-`Unknown`, incompatible pairs (e.g. Int/Str) return `None`.
///
/// `widen_numeric` controls Int/Float promotion, which is only SOUND where the
/// element type may be `Float`. A `list[float]` (`Vec<f64>`) is representable, so
/// LIST literals pass `true` and `[1, 2.0]` widens to `List(Float)` (codegen
/// casts the int elements to f64 — see `Codegen::emit_collection_elem`). It is
/// UNSOUND in hashable positions: a `set[float]` (`HashSet<f64>`) does not
/// compile (f64 is not `Eq`/`Hash`), so SET literals pass `false` and `{1, 2.0}`
/// is rejected. (Dict keys are hashable -> `false`; dict values -> `true`.)
/// The broader `set[float]` gap is tracked separately.
fn unify_elem_types(a: Ty, b: Ty, widen_numeric: bool) -> Option<Ty> {
    match (&a, &b) {
        // Numeric promotion to Float — only where a Float element is representable.
        (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) if widen_numeric => Some(Ty::Float),
        _ => unify_branch_types(a, b),
    }
}

/// Reject a `Float` type in a hashable position (set element, dict key).
///
/// `HashSet<f64>` / `HashMap<f64, _>` do not compile because `f64` is not
/// `Eq`/`Hash`; codegen's `rust_ty` would emit exactly those forms. To keep
/// typeck and codegen in agreement (the soundness rule), forbid a concretely
/// `Float` element/key here — whether it arises from a literal, a comprehension,
/// or a declared `set[float]` / `dict[float, _]` annotation.
///
/// Stays permissive on `Unknown` (e.g. `set()` / `{}` with no concrete inner):
/// only a concrete `Ty::Float` is rejected, never `Unknown`.
fn require_hashable(ty: &Ty, span: Span, position: &str) -> Result<()> {
    if matches!(ty, Ty::Float) {
        return Err(Error::Type {
            span,
            msg: format!(
                "{} type must be hashable; float is not supported here \
                 (f64 is not Eq/Hash, so HashSet<f64>/HashMap<f64, _> won't compile)",
                position
            ),
        });
    }
    Ok(())
}

// ── By-value parameter mutation detection helpers ─────────────────────────────

/// Walk `Attr { obj }` and `Index { obj }` chains to find the innermost `Ident`.
/// Returns the identifier name if the expression is rooted at a plain name.
fn root_ident(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(name, _) => Some(name.as_str()),
        Expr::Attr { obj, .. } => root_ident(obj),
        Expr::Index { obj, .. } => root_ident(obj),
        _ => None,
    }
}

/// EPIC-4 V2: is `e` a *place* (an addressable lvalue) we could borrow `&mut`?
/// A by-reference (`Mut[T]`) argument must be one of these — a variable, a field
/// access, or an index — never a temporary (call/constructor/literal/binop/etc.),
/// which has no caller-visible storage to mutate.
fn is_place_expr(e: &Expr) -> bool {
    matches!(e, Expr::Ident(..) | Expr::Attr { .. } | Expr::Index { .. })
}

/// The single source of truth for copy-ness, consumed by both `typeck` and
/// `codegen` (via `crate::typeck::is_copy` / `is_owned`). A type is `Copy` when
/// its emitted Rust representation implements the `Copy` trait, so a by-value
/// use neither moves the original binding nor needs a `.clone()`.
///
/// Rule (defined recursively for the aggregate variants):
/// - `Int`/`Float`/`Bool`/`Unit` are `Copy`.
/// - `Tuple(elems)` is `Copy` iff **every** element is `Copy` (Rust tuples of
///   `Copy` elements are `Copy`).
/// - `Option(inner)` is `Copy` iff `inner` is `Copy` (Rust `Option<T: Copy>` is
///   `Copy`).
/// - Everything else is non-`Copy`: `Str`, `List`, `Set`, `Dict`, `Class`, and
///   the conservative `NoneVal`/`File`/`Unknown` cases (excluded here exactly as
///   the legacy `is_copy_type` excluded them).
pub fn is_copy(ty: &Ty) -> bool {
    match ty {
        Ty::Int | Ty::Float | Ty::Bool | Ty::Unit => true,
        Ty::Tuple(elems) => elems.iter().all(is_copy),
        Ty::Option(inner) => is_copy(inner),
        Ty::Str
        | Ty::List(_)
        | Ty::Set(_)
        | Ty::Dict(_, _)
        | Ty::Class(_)
        | Ty::NoneVal
        | Ty::File
        | Ty::Unknown => false,
    }
}

/// Complement of [`is_copy`]: `true` for move-only (non-`Copy`) types, i.e. ones
/// that need clone-on-use because a by-value pass would otherwise consume the
/// original binding (and, for params, hand the callee a clone whose mutations
/// cannot propagate back to the caller).
pub fn is_owned(ty: &Ty) -> bool {
    !is_copy(ty)
}

/// The single source of truth for collection methods that mutate their receiver
/// in place (List/Set/Dict mutators). Consumed by BOTH modules — same "one
/// source of truth" discipline as [`is_copy`]:
/// - `typeck`'s by-value-param backstop: calling any of these on a by-value
///   non-Copy param is a bug (the mutation is lost on the caller's copy).
/// - `codegen`'s `method_modifies_self` (to infer `&mut self` on the enclosing
///   method) and the emission site (to pick `emit_place` for subscripted
///   receivers so the mutation lands on the real element).
///
/// Previously duplicated as `codegen::MUTATING_METHODS` and
/// `typeck::PARAM_MUTATING_METHODS` (content-identical 13-name lists, differing
/// only in ordering); merged here so the two analyses can never drift.
pub const MUTATING_METHODS: &[&str] = &[
    // List mutators
    "append", "extend", "insert", "remove", "sort", "reverse", "clear",
    // Set mutators
    "add", "discard",
    // Dict mutators
    "update", "pop", "setdefault", "popitem",
];

/// Shared body of the by-value-parameter-mutation backstop error. EPIC-4 V2 adds
/// the `Mut[T]` on-ramp to the remedy clause: the user can now opt into a real
/// by-reference param instead of only the return-the-value idiom. All three
/// backstop sites (AttrAssign / IndexAssign / mutating method-call) use this so
/// the message can never drift between them.
fn by_value_mutation_error(param: &str) -> String {
    format!(
        "mutation of by-value parameter `{}` is not visible to the caller; \
         mutate via a method on it or return the updated value; \
         or declare the parameter `Mut[T]` to mutate it in place",
        param
    )
}

// ─────────────────────────────────────────────────────────────────────────────

/// Pre-scan a function body and collect the names of parameters that appear as
/// identifiers in any `return <expr>` statement (including nested blocks).
///
/// A param that is mutated then returned is the valid functional pattern:
///   `xs.append(99); return xs`
/// The callee operates on its own copy and returns the updated value; the
/// caller captures it.  We suppress the by-value-param-mutation error for
/// any param that flows to at least one return — conservative (favour avoiding
/// false positives over false negatives).
fn collect_returned_param_idents(
    stmts: &[Stmt],
    params: &std::collections::HashSet<String>,
    out: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        collect_returned_param_idents_stmt(s, params, out);
    }
}

fn collect_returned_param_idents_stmt(
    s: &Stmt,
    params: &std::collections::HashSet<String>,
    out: &mut std::collections::HashSet<String>,
) {
    match s {
        Stmt::Return(Some(e), _) => {
            collect_returned_param_idents_expr(e, params, out);
        }
        // Recurse into all nested statement blocks.
        Stmt::If { then, elifs, else_, .. } => {
            collect_returned_param_idents(then, params, out);
            for (_, b) in elifs { collect_returned_param_idents(b, params, out); }
            if let Some(b) = else_ { collect_returned_param_idents(b, params, out); }
        }
        Stmt::While { body, .. } => collect_returned_param_idents(body, params, out),
        Stmt::For { body, .. } => collect_returned_param_idents(body, params, out),
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            collect_returned_param_idents(body, params, out);
            for h in handlers { collect_returned_param_idents(&h.body, params, out); }
            if let Some(b) = else_ { collect_returned_param_idents(b, params, out); }
            if let Some(b) = finally_ { collect_returned_param_idents(b, params, out); }
        }
        Stmt::With { body, .. } => collect_returned_param_idents(body, params, out),
        // Match arms
        Stmt::Match { arms, .. } => {
            for arm in arms { collect_returned_param_idents(&arm.body, params, out); }
        }
        // Nested defs / classes — do NOT descend; their returns belong to a
        // different function scope.
        Stmt::Func(_) | Stmt::Class(_) => {}
        _ => {}
    }
}

/// Walk an expression and collect any top-level Ident that is a known param.
/// We stay shallow (just check the expression itself and direct sub-expressions
/// of Tuple/IfExp) to avoid spurious suppression from `return [xs]` or similar.
fn collect_returned_param_idents_expr(
    e: &Expr,
    params: &std::collections::HashSet<String>,
    out: &mut std::collections::HashSet<String>,
) {
    match e {
        Expr::Ident(name, _) => {
            if params.contains(name.as_str()) {
                out.insert(name.clone());
            }
        }
        // `return (a, b)` — both parts count.
        Expr::Tuple(elems, _) => {
            for elem in elems {
                collect_returned_param_idents_expr(elem, params, out);
            }
        }
        // `return x if cond else y` — both branches count.
        Expr::IfExp { body, orelse, .. } => {
            collect_returned_param_idents_expr(body, params, out);
            collect_returned_param_idents_expr(orelse, params, out);
        }
        // Any other expression shape — do not descend. Being conservative here
        // is deliberate: we only suppress the error when the param flows
        // *directly* to the return, not via an arbitrary computation.
        _ => {}
    }
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
            // The raised value names an exception type (e.g. `ValueError("msg")`
            // or bare `ValueError`). Exception types are not user-defined
            // functions/classes, so don't validate the type name as a callee —
            // only type-check the message arguments.
            match exc {
                Some(Expr::Call { callee, args, .. }) if matches!(callee.as_ref(), Expr::Ident(..)) => {
                    for a in args { check_expr(a, env)?; }
                    Ok(())
                }
                Some(Expr::Ident(..)) => Ok(()),
                Some(e) => { check_expr(e, env)?; Ok(()) }
                None => Ok(()),
            }
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
            if !types_compatible(&ty, &env.ret_ty) {
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
                if !types_compatible(&val_ty, &explicit) {
                    return Err(Error::Type {
                        span: *span,
                        msg: format!("type mismatch in assignment: declared {:?}, got {:?}", explicit, val_ty),
                    });
                }
            }
            // NOTE: bare reassignment to a different concrete type is intentionally
            // allowed — codegen emits a shadowing `let`, so pyrst supports Python's
            // type-changing rebind (e.g. an int accumulator later assigned a float,
            // or a name reused for a different value). Rejecting it here would
            // contradict that feature.
            // Track when an original parameter is rebound so that subsequent mutations
            // on the new local value are NOT flagged as by-value param mutations.
            if env.params.contains(target.as_str()) {
                env.reassigned_params.insert(target.clone());
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
            // (EPIC-5) None-guard narrowing. For `if x is not None:` the THEN
            // branch sees `x: T` (the non-None payload); for `if x is None:` the
            // ELSE branch sees `x: T`. `x` must be a local typed `Option(T)`.
            // We narrow only the directly-guarded branch and save/restore the
            // local's type so the narrowing never leaks past the `if`.
            let guard = extract_none_guard(cond)
                .and_then(|(name, is_not_none)| match env.locals.get(name.as_str()) {
                    Some(Ty::Option(inner)) => Some((name, is_not_none, (**inner).clone())),
                    _ => None,
                });
            // THEN branch: narrowed iff the guard is `is not None`.
            {
                let restore = guard.as_ref().filter(|(_, is_not_none, _)| *is_not_none)
                    .map(|(name, _, inner)| {
                        let prev = env.locals.insert(name.clone(), inner.clone());
                        (name.clone(), prev)
                    });
                check_body(then, env)?;
                if let Some((name, prev)) = restore {
                    match prev { Some(t) => { env.locals.insert(name, t); } None => { env.locals.remove(name.as_str()); } }
                }
            }
            for (c, b) in elifs {
                check_expr(c, env)?;
                check_body(b, env)?;
            }
            // ELSE branch: narrowed iff the guard is `is None` (so the else is the
            // non-None case). Skipped when there are elifs, since the else then
            // belongs to a different condition.
            if let Some(b) = else_ {
                let restore = guard.as_ref()
                    .filter(|(_, is_not_none, _)| !*is_not_none && elifs.is_empty())
                    .map(|(name, _, inner)| {
                        let prev = env.locals.insert(name.clone(), inner.clone());
                        (name.clone(), prev)
                    });
                check_body(b, env)?;
                if let Some((name, prev)) = restore {
                    match prev { Some(t) => { env.locals.insert(name, t); } None => { env.locals.remove(name.as_str()); } }
                }
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
                Ty::Set(inner) => *inner.clone(),
                // Iterating a dict yields its KEYS (Python semantics).
                Ty::Dict(key, _) => *key.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Unknown,
            };
            // Bind all targets
            if targets.len() == 1 {
                // Single target gets the full element type
                env.locals.insert(targets[0].clone(), elem_ty.clone());
            } else {
                // Multiple targets: if the element type is a tuple of matching
                // arity (e.g. iterating dict.items() -> List[Tuple[K, V]]), bind
                // each target to its component type. Otherwise fall back to
                // Unknown (mirrors the Stmt::Unpack destructuring above).
                let elem_tys = match &elem_ty {
                    Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
                    _ => vec![Ty::Unknown; targets.len()],
                };
                for (i, target) in targets.iter().enumerate() {
                    let ty = elem_tys.get(i).cloned().unwrap_or(Ty::Unknown);
                    env.locals.insert(target.clone(), ty);
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
                    // The bound exception value is the panic message string.
                    env.locals.insert(name.clone(), Ty::Str);
                }
                check_body(&h.body, env)?;
            }
            if let Some(b) = else_ { check_body(b, env)?; }
            if let Some(b) = finally_ { check_body(b, env)?; }
            Ok(())
        }
        Stmt::With { ctx_expr, as_name, body, .. } => {
            let ctx_ty = check_expr(ctx_expr, env)?;
            // Bound name is block-scoped in codegen; save/restore so a stale type
            // does not leak past the block (mirrors the for-loop handling).
            let saved = as_name.as_ref().map(|n| (n.clone(), env.locals.get(n).cloned()));
            if let Some(name) = as_name {
                env.locals.insert(name.clone(), ctx_ty);
            }
            check_body(body, env)?;
            if let Some((name, prev)) = saved {
                match prev {
                    Some(ty) => { env.locals.insert(name, ty); }
                    None => { env.locals.remove(name.as_str()); }
                }
            }
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
        Stmt::AttrAssign { obj, attr, value, span } => {
            // Validate the target base chain (the base expr must type-check;
            // unknown names / bad nested attributes are rejected by check_expr).
            let obj_ty = check_expr(obj, env)?;
            check_expr(value, env)?;
            // Detect mutation of a by-value non-Copy parameter.
            // `param.field = v` where `param` is still the original binding is a
            // silent wrong-output bug — the caller's value is never updated.
            // Exception: if the param is returned by the function, the mutation
            // is the caller's own copy that gets handed back — a valid pattern.
            if let Some(root) = root_ident(obj) {
                if root != "self"
                    && env.params.contains(root)
                    && !env.reassigned_params.contains(root)
                    && !env.returned_params.contains(root)
                    && !env.by_ref_params.contains(root)
                    && is_owned(&obj_ty)
                {
                    return Err(Error::Type {
                        span: *span,
                        msg: by_value_mutation_error(root),
                    });
                }
            }
            // If the base is a known user class, the assigned field must exist on
            // it (including inherited fields) — `a.b.c = v` with no field `c` is a
            // type error, not a deferred-to-rustc one.
            if let Ty::Class(class_name) = &obj_ty {
                if env.ctx.classes.contains_key(class_name.as_str()) {
                    let has_field = env.ctx.get_all_fields(class_name.as_str())
                        .iter().any(|f| &f.name == attr);
                    if !has_field {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("class `{}` has no attribute `{}`", class_name, attr),
                        });
                    }
                }
            }
            Ok(())
        }
        Stmt::IndexAssign { obj, idx, value, span } => {
            // Validate the target base chain, the subscript, and the value.
            let obj_ty = check_expr(obj, env)?;
            check_expr(idx, env)?;
            check_expr(value, env)?;
            // Detect mutation of a by-value non-Copy parameter via index assignment.
            // Exception: if the param is returned by the function, the mutation is valid.
            if let Some(root) = root_ident(obj) {
                if root != "self"
                    && env.params.contains(root)
                    && !env.reassigned_params.contains(root)
                    && !env.returned_params.contains(root)
                    && !env.by_ref_params.contains(root)
                    && is_owned(&obj_ty)
                {
                    return Err(Error::Type {
                        span: *span,
                        msg: by_value_mutation_error(root),
                    });
                }
            }
            Ok(())
        }
        Stmt::Func(_) | Stmt::Class(_) => Ok(()), // Nested — punt in v0.
    }
}

// --- Builtin method tables (S4 soundness check) ---
// Superset of every method codegen handles (special-cased or valid Rust
// passthrough) and every method the example suite calls on a concrete receiver.
const STR_METHODS: &[&str] = &[
    "upper", "lower", "strip", "lstrip", "rstrip", "split",
    "splitlines", "join", "startswith", "endswith", "replace", "removeprefix",
    "removesuffix", "expandtabs", "partition", "rpartition", "find", "rfind",
    "index", "rindex", "count", "contains", "isdigit", "isalpha", "isupper",
    "islower", "isspace", "isalnum", "isidentifier", "isnumeric", "isprintable",
    "istitle", "capitalize", "title", "zfill", "ljust", "rjust",
    "center", "swapcase", "len",
    // NOTE: casefold/encode/isdecimal/rsplit/format removed — codegen cannot
    // emit them and they are absent from the example corpus (card 36f66dd2).
];
const LIST_METHODS: &[&str] = &[
    "append", "extend", "insert", "remove", "pop", "index", "count",
    "reverse", "sort", "clear", "copy", "len", "contains",
];
const SET_METHODS: &[&str] = &[
    "add", "discard", "remove", "clear", "copy", "pop", "len", "union",
    "intersection", "difference", "symmetric_difference", "issubset",
    "issuperset", "isdisjoint", "update", "contains",
];
const DICT_METHODS: &[&str] = &[
    "get", "keys", "values", "items", "pop", "clear", "copy", "update",
    "len", "contains",
    // NOTE: setdefault/popitem removed — codegen cannot emit them and they are
    // absent from the example corpus (card 36f66dd2).
];
const FILE_METHODS: &[&str] = &["read", "readlines", "write", "close"];

/// Returns (type-name, method-table) for a concrete builtin receiver, or None
/// for Unknown/Class/numeric receivers (the check must not run on those).
fn builtin_method_table(ty: &Ty) -> Option<(&'static str, &'static [&'static str])> {
    match ty {
        Ty::Str => Some(("str", STR_METHODS)),
        Ty::List(_) => Some(("list", LIST_METHODS)),
        Ty::Set(_) => Some(("set", SET_METHODS)),
        Ty::Dict(_, _) => Some(("dict", DICT_METHODS)),
        Ty::File => Some(("file", FILE_METHODS)),
        _ => None,
    }
}

/// Mutators whose single argument must be assignable to the receiver's element
/// type. Restricted to set mutators (list `.append` excluded: empty-list field
/// inference defaults to list[int] and would risk false rejections). Returns the
/// element type to check the argument against.
fn elem_arg_check_ty(recv: &Ty, method: &str) -> Option<Ty> {
    match recv {
        Ty::Set(elem) if matches!(method, "add" | "discard" | "remove") => Some((**elem).clone()),
        _ => None,
    }
}

/// Concrete return type of a builtin method call on a known builtin receiver
/// (str/list/set/dict); unrecognized methods or receivers return Unknown.
/// This is the single source of truth — codegen's type_of_expr delegates here.
/// Note: pyrst models str.partition/rpartition as list[str] (not a tuple),
/// matching codegen and the example fixtures.
pub fn builtin_method_ret(recv: &Ty, method: &str) -> Ty {
    match recv {
        Ty::Str => match method {
            "upper" | "lower" | "strip" | "lstrip" | "rstrip" | "replace"
            | "capitalize" | "title" | "swapcase" | "zfill"
            | "ljust" | "rjust" | "center" | "removeprefix" | "removesuffix"
            | "expandtabs" | "join" => Ty::Str,
            // NOTE: casefold/encode/format/rsplit removed from str arms —
            // codegen cannot emit them (card 36f66dd2 stopgap).
            "split" | "splitlines" | "partition" | "rpartition" => {
                Ty::List(Box::new(Ty::Str))
            }
            "find" | "rfind" | "index" | "rindex" | "count" => Ty::Int,
            "startswith" | "endswith" | "isdigit" | "isalpha" | "isupper" | "islower"
            | "isspace" | "isalnum" | "isidentifier" | "isnumeric" | "isprintable"
            | "istitle" => Ty::Bool,
            // NOTE: isdecimal removed — codegen cannot emit it (card 36f66dd2).
            _ => Ty::Unknown,
        },
        // Concrete element/collection returns, plus in-place mutators typed as
        // Unit (card 2b3bf7f5; audited: no example assigns/chains a mutator
        // result). Deliberately still Unknown: dict.get / dict.setdefault, which
        // need arg-count-aware typing (the 2-arg `get(k, default)` returns V,
        // not Optional[V]).
        Ty::List(elem) => match method {
            "pop" => (**elem).clone(),
            "copy" => Ty::List(elem.clone()),
            "index" | "count" => Ty::Int,
            // In-place mutators return None (audited: no example assigns/chains them).
            "append" | "extend" | "insert" | "remove" | "sort" | "reverse" | "clear" => Ty::Unit,
            _ => Ty::Unknown,
        },
        Ty::Set(elem) => match method {
            "union" | "intersection" | "difference" | "symmetric_difference" | "copy" => {
                Ty::Set(elem.clone())
            }
            "pop" => (**elem).clone(),
            "issubset" | "issuperset" | "isdisjoint" => Ty::Bool,
            // In-place mutators return None.
            "add" | "discard" | "remove" | "update" | "intersection_update"
            | "difference_update" | "symmetric_difference_update" | "clear" => Ty::Unit,
            _ => Ty::Unknown,
        },
        Ty::Dict(key, val) => match method {
            "keys" => Ty::List(key.clone()),
            "values" => Ty::List(val.clone()),
            "copy" => Ty::Dict(key.clone(), val.clone()),
            "items" => Ty::List(Box::new(Ty::Tuple(vec![(**key).clone(), (**val).clone()]))),
            "pop" => (**val).clone(),
            // In-place mutators return None. (get/setdefault return V/Optional and
            // are deliberately left Unknown — they need arg-count-aware typing.)
            "update" | "clear" => Ty::Unit,
            _ => Ty::Unknown,
        },
        Ty::File => match method {
            "read" => Ty::Str,
            "readlines" => Ty::List(Box::new(Ty::Str)),
            "write" | "close" => Ty::Unit,
            _ => Ty::Unknown,
        },
        _ => Ty::Unknown,
    }
}

/// Arg-count-aware return type for `dict.get`, which `builtin_method_ret` cannot
/// express (it has no view of the call's arguments). Python's `d.get(k)` returns
/// `Optional[V]` (None when absent), while `d.get(k, default)` returns `V`. Both
/// the inference oracle (`infer_expr_ty`) and the error-producing checker
/// (`check_expr`) route dict.get through here so the two never drift. For any
/// non-dict receiver (or a non-`get` method) it returns None, leaving the caller
/// to fall back to `builtin_method_ret`.
pub fn dict_get_ret(recv: &Ty, method: &str, argc: usize) -> Option<Ty> {
    if method != "get" {
        return None;
    }
    if let Ty::Dict(_key, val) = recv {
        // 1-arg get -> Optional[V]; 2-arg get(k, default) -> V.
        if argc <= 1 {
            Some(Ty::Option(val.clone()))
        } else {
            Some((**val).clone())
        }
    } else {
        None
    }
}

/// Pure inference oracle — the single source of truth for expression types.
///
/// A side-effect-free port of codegen's `type_of_expr` (codegen.rs:264-548) with
/// the SAME contract: it never errors and never mutates; on any ambiguity it
/// falls to `Ty::Unknown` (preserving the `types_compatible` `(Unknown, _) => true`
/// escape hatch). Inputs are exactly what both call sites already hold — typeck's
/// `env.locals`/`env.ctx` and codegen's `self.locals`/`self.ctx` are identical
/// types — so E.2 can route both through here.
///
/// It bakes in the CORRECT side of every documented divergence
/// (docs/design/inference-oracle.md §A.4): D1 str-index → Str; D3 abs(x) → arg
/// type; D4 sum(xs) → element type; D5 `**` → Float; D6 dict literal folds ALL
/// pairs; D7 attribute access is inheritance-aware (`get_all_fields`).
pub fn infer_expr_ty(expr: &Expr, locals: &HashMap<String, Ty>, ctx: &TyCtx) -> Ty {
    match expr {
        Expr::Float(..) => Ty::Float,
        Expr::Int(..) => Ty::Int,
        Expr::Bool(..) => Ty::Bool,
        Expr::Str(..) | Expr::FStr(..) => Ty::Str,
        Expr::None_(_) => Ty::NoneVal,
        Expr::IfExp { body, orelse, .. } => {
            // Both branches unify in typeck; prefer the concrete one.
            let b = infer_expr_ty(body, locals, ctx);
            if b == Ty::Unknown {
                infer_expr_ty(orelse, locals, ctx)
            } else {
                b
            }
        }
        Expr::Ident(n, _) => locals
            .get(n.as_str())
            .or_else(|| ctx.vars.get(n.as_str()))
            .cloned()
            .unwrap_or(Ty::Unknown),
        Expr::UnOp { op: UnOp::Neg, expr, .. } => infer_expr_ty(expr, locals, ctx),
        Expr::UnOp { op: UnOp::Not, .. } => Ty::Bool,
        Expr::UnOp { op: UnOp::BitNot, .. } => Ty::Int,
        Expr::BinOp { lhs, op, rhs, .. } => {
            let l = infer_expr_ty(lhs, locals, ctx);
            let r = infer_expr_ty(rhs, locals, ctx);
            match op {
                // D5: Python `**` always yields a float (split out of the
                // int-biased arithmetic arm below — codegen's bug).
                BinOp::Pow => Ty::Float,
                BinOp::Div => Ty::Float,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod | BinOp::FloorDiv => {
                    // Operator overloading: a class lhs uses its dunder return type.
                    if let Ty::Class(cls) = &l {
                        let dunder = match op {
                            BinOp::Add => Some("__add__"),
                            BinOp::Sub => Some("__sub__"),
                            BinOp::Mul => Some("__mul__"),
                            _ => None,
                        };
                        if let Some(ret) =
                            dunder.and_then(|d| ctx.get_method(cls, d)).map(|s| s.ret.clone())
                        {
                            return ret;
                        }
                    }
                    // String concatenation for Add.
                    if *op == BinOp::Add && (l == Ty::Str || r == Ty::Str) {
                        Ty::Str
                    } else if l == Ty::Float || r == Ty::Float {
                        Ty::Float
                    } else {
                        Ty::Int
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                | BinOp::And | BinOp::Or | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => {
                    Ty::Bool
                }
                _ => Ty::Unknown,
            }
        }
        Expr::Attr { obj, name, .. } => {
            // D7: resolve the field inheritance-aware via `get_all_fields`
            // (codegen reads `c.fields` directly and misses inherited fields).
            if let Ty::Class(cls) = infer_expr_ty(obj, locals, ctx) {
                let all_fields = ctx.get_all_fields(cls.as_str());
                if let Some(f) = all_fields.iter().find(|f| f.name == *name) {
                    return Ty::from_type_expr(&f.ty).unwrap_or(Ty::Unknown);
                }
            }
            Ty::Unknown
        }
        Expr::Call { callee, args, .. } => {
            if let Expr::Ident(n, _) = callee.as_ref() {
                match n.as_str() {
                    "float" => Ty::Float,
                    "abs" => {
                        // D3: abs returns the same type as its argument.
                        if let Some(arg) = args.first() {
                            infer_expr_ty(arg, locals, ctx)
                        } else {
                            Ty::Unknown
                        }
                    }
                    "sum" => {
                        // D4: sum() returns the type of the iterable's elements.
                        if let Some(arg) = args.first() {
                            match infer_expr_ty(arg, locals, ctx) {
                                Ty::List(inner) => *inner,
                                Ty::Set(inner) => *inner,
                                _ => Ty::Int, // Default to int for other iterables.
                            }
                        } else {
                            Ty::Int
                        }
                    }
                    "int" | "len" | "ord" | "round" | "pow" => Ty::Int,
                    "bool" | "any" | "all" => Ty::Bool,
                    "str" | "chr" | "input" => Ty::Str,
                    "map" if args.len() == 2 => {
                        // map(f, iterable) -> List(applied return type of f).
                        // Only a List iterable yields a concrete List result;
                        // Set/Str/unknown stay Unknown (permissive).
                        match infer_expr_ty(&args[1], locals, ctx) {
                            Ty::List(e) => {
                                let body_ty = lambda_applied_ty(&args[0], &e, locals, ctx);
                                Ty::List(Box::new(body_ty))
                            }
                            _ => Ty::Unknown,
                        }
                    }
                    "filter" if args.len() == 2 => {
                        // filter(pred, iterable) -> the iterable's list type
                        // unchanged. Only List yields a concrete type.
                        match infer_expr_ty(&args[1], locals, ctx) {
                            Ty::List(e) => Ty::List(e),
                            _ => Ty::Unknown,
                        }
                    }
                    "sorted" | "list" | "reversed" => {
                        // These return a list; preserve the element type.
                        // Over a dict they operate on its KEYS (Python semantics),
                        // so the result element type is the dict's key type.
                        if let Some(arg) = args.first() {
                            match infer_expr_ty(arg, locals, ctx) {
                                Ty::List(e) | Ty::Set(e) => Ty::List(e),
                                Ty::Dict(k, _) => Ty::List(k),
                                Ty::Str => Ty::List(Box::new(Ty::Str)),
                                _ => Ty::List(Box::new(Ty::Unknown)),
                            }
                        } else {
                            Ty::List(Box::new(Ty::Unknown))
                        }
                    }
                    n => {
                        // A class constructor yields an instance; otherwise look
                        // up the user function's declared return type.
                        if ctx.classes.contains_key(n) {
                            Ty::Class(n.to_string())
                        } else {
                            ctx.funcs.get(n).map(|s| s.ret.clone()).unwrap_or(Ty::Unknown)
                        }
                    }
                }
            } else if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                // Math module return types.
                if let Expr::Ident(modname, _) = obj.as_ref() {
                    if modname == "math" {
                        return match name.as_str() {
                            "isnan" | "isinf" | "isfinite" => Ty::Bool,
                            _ => Ty::Float,
                        };
                    }
                }
                // Class methods use their declared return; builtin receivers
                // (str/list/set/dict/file) delegate to the shared
                // `builtin_method_ret` so the two never drift and chained calls
                // resolve.
                let recv = infer_expr_ty(obj, locals, ctx);
                if let Ty::Class(cls) = &recv {
                    ctx.get_method(cls, name).map(|s| s.ret.clone()).unwrap_or(Ty::Unknown)
                } else if let Some(t) = dict_get_ret(&recv, name, args.len()) {
                    // dict.get is arg-count-aware: get(k) -> Optional[V],
                    // get(k, default) -> V (see dict_get_ret).
                    t
                } else {
                    builtin_method_ret(&recv, name)
                }
            } else {
                Ty::Unknown
            }
        }
        Expr::List(elems, _) => {
            // Unify all element types (not first-element-wins) so a mixed numeric
            // literal like `[1, 2.0]` is typed `List(Float)`.
            Ty::List(Box::new(infer_list_elem_ty(elems, locals, ctx)))
        }
        Expr::Dict(pairs, _) => {
            // D6: fold ALL pairs, unifying key types and value types
            // independently (codegen uses the first pair only). On a both-concrete
            // conflict, degrade THAT position to Unknown — never error (the pure
            // contract; check_expr rejects, this oracle stays permissive).
            if pairs.is_empty() {
                Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown))
            } else {
                let mut k_ty = infer_expr_ty(&pairs[0].0, locals, ctx);
                let mut v_ty = infer_expr_ty(&pairs[0].1, locals, ctx);
                for (k, v) in &pairs[1..] {
                    let kt = infer_expr_ty(k, locals, ctx);
                    let vt = infer_expr_ty(v, locals, ctx);
                    // widen_numeric=false: float dict keys are non-hashable and
                    // dict values have no codegen cast, matching check_expr.
                    k_ty = unify_elem_types(k_ty.clone(), kt, false).unwrap_or(Ty::Unknown);
                    v_ty = unify_elem_types(v_ty.clone(), vt, false).unwrap_or(Ty::Unknown);
                }
                Ty::Dict(Box::new(k_ty), Box::new(v_ty))
            }
        }
        Expr::Set(elems, _) => {
            // Unify all element types (mirrors the list case).
            Ty::Set(Box::new(infer_list_elem_ty(elems, locals, ctx)))
        }
        Expr::ListComp { elt, target, iter, .. } => {
            // Infer element type from the iterable and element expression.
            let iter_ty = infer_expr_ty(iter, locals, ctx);
            let elem_iter_ty = match &iter_ty {
                Ty::List(inner) | Ty::Set(inner) => Some(inner.as_ref().clone()),
                _ => None,
            };
            if let Some(elem_iter_type) = elem_iter_ty {
                let inferred =
                    infer_comp_elt_type_with_var(elt, &elem_iter_type, target, ctx);
                if inferred != Ty::Unknown {
                    return Ty::List(Box::new(inferred));
                }
            }
            // Fallback: use the iterable's element type.
            match iter_ty {
                Ty::List(inner) => Ty::List(inner),
                Ty::Set(inner) => Ty::List(inner),
                _ => Ty::List(Box::new(Ty::Unknown)),
            }
        }
        Expr::SetComp { elt, target: _, iter, .. } => {
            let iter_ty = infer_expr_ty(iter, locals, ctx);
            if let Ty::List(ref inner) | Ty::Set(ref inner) = iter_ty {
                match elt.as_ref() {
                    Expr::Attr { name, .. } => {
                        if let Ty::Class(cls) = inner.as_ref() {
                            if let Some(c) = ctx.classes.get(cls.as_str()) {
                                if let Some(f) = c.fields.iter().find(|f| f.name == *name) {
                                    if let Ok(ty) = Ty::from_type_expr(&f.ty) {
                                        return Ty::Set(Box::new(ty));
                                    }
                                }
                            }
                        }
                    }
                    Expr::Call { callee, .. } => {
                        if let Expr::Attr { name, .. } = callee.as_ref() {
                            if let Ty::Class(cls) = inner.as_ref() {
                                if let Some(method_sig) = ctx.get_method(cls.as_str(), name) {
                                    return Ty::Set(Box::new(method_sig.ret.clone()));
                                }
                            }
                        }
                    }
                    _ => {}
                }
                Ty::Set(inner.clone())
            } else {
                Ty::Set(Box::new(Ty::Unknown))
            }
        }
        Expr::DictComp { key, val, target: _, iter, .. } => {
            let iter_ty = infer_expr_ty(iter, locals, ctx);
            let field_ty = |e: &Expr| -> Ty {
                if let Expr::Attr { name, .. } = e {
                    if let Ty::Class(ref cls) = iter_ty {
                        if let Some(c) = ctx.classes.get(cls.as_str()) {
                            if let Some(f) = c.fields.iter().find(|f| f.name == *name) {
                                return Ty::from_type_expr(&f.ty).unwrap_or(Ty::Unknown);
                            }
                        }
                    }
                }
                Ty::Unknown
            };
            Ty::Dict(Box::new(field_ty(key)), Box::new(field_ty(val)))
        }
        Expr::Index { obj, .. } => {
            // D1: a Str receiver yields Str (codegen lacks this arm). Dict[k] is
            // the value type; List[i] is the element type.
            match infer_expr_ty(obj, locals, ctx) {
                Ty::Dict(_, val_ty) => *val_ty,
                Ty::List(elem_ty) => *elem_ty,
                Ty::Str => Ty::Str,
                _ => Ty::Unknown,
            }
        }
        _ => Ty::Unknown,
    }
}

/// Unified element type of a list/set literal's elements, for `infer_expr_ty`.
/// Folds every element's type with `unify_oracle_ty` (not first-element-wins) so
/// a mixed numeric literal like `[1, 2.0]` is typed `Float`. Empty -> `Unknown`.
/// Pure port of codegen's `list_elem_ty`/`unify_ty`.
fn infer_list_elem_ty(elems: &[Expr], locals: &HashMap<String, Ty>, ctx: &TyCtx) -> Ty {
    let mut iter = elems.iter();
    match iter.next() {
        None => Ty::Unknown,
        Some(first) => iter.fold(infer_expr_ty(first, locals, ctx), |acc, e| {
            unify_oracle_ty(acc, infer_expr_ty(e, locals, ctx))
        }),
    }
}

/// Structural element-type unification for collection literals (pure port of
/// codegen's `unify_ty`). Int/Float widen to Float; nested collections recurse;
/// `Unknown` is absorbed; otherwise the left (concrete) side wins.
fn unify_oracle_ty(a: Ty, b: Ty) -> Ty {
    match (a, b) {
        (Ty::Unknown, x) | (x, Ty::Unknown) => x,
        (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => Ty::Float,
        (Ty::Dict(k1, v1), Ty::Dict(k2, v2)) => Ty::Dict(
            Box::new(unify_oracle_ty(*k1, *k2)),
            Box::new(unify_oracle_ty(*v1, *v2)),
        ),
        (Ty::List(e1), Ty::List(e2)) => Ty::List(Box::new(unify_oracle_ty(*e1, *e2))),
        (Ty::Set(e1), Ty::Set(e2)) => Ty::Set(Box::new(unify_oracle_ty(*e1, *e2))),
        (a, _) => a,
    }
}

/// Infer the applied return type of a `map`'s callable over an element of type
/// `elem`, for `infer_expr_ty`'s `map` arm. Pure port of codegen's
/// `lambda_applied_ty` -> `type_of_expr_bound`.
fn lambda_applied_ty(callable: &Expr, elem: &Ty, locals: &HashMap<String, Ty>, ctx: &TyCtx) -> Ty {
    if let Expr::Lambda { params, body, .. } = callable {
        if let Some((param, _)) = params.first() {
            return infer_expr_ty_bound(body, param, elem, locals, ctx);
        }
    }
    Ty::Unknown
}

/// Like `infer_expr_ty`, but the single identifier `param` resolves to `elem`
/// (the bound lambda parameter). Recurses through the compound forms that appear
/// in map lambda bodies; for everything else it delegates to `infer_expr_ty`.
/// Pure port of codegen's `type_of_expr_bound`.
fn infer_expr_ty_bound(
    e: &Expr,
    param: &str,
    elem: &Ty,
    locals: &HashMap<String, Ty>,
    ctx: &TyCtx,
) -> Ty {
    match e {
        Expr::Ident(n, _) if n == param => elem.clone(),
        Expr::UnOp { op: UnOp::Neg, expr, .. } => {
            infer_expr_ty_bound(expr, param, elem, locals, ctx)
        }
        Expr::IfExp { body, orelse, .. } => {
            let b = infer_expr_ty_bound(body, param, elem, locals, ctx);
            if b == Ty::Unknown {
                infer_expr_ty_bound(orelse, param, elem, locals, ctx)
            } else {
                b
            }
        }
        Expr::BinOp { lhs, op, rhs, .. } => {
            let l = infer_expr_ty_bound(lhs, param, elem, locals, ctx);
            let r = infer_expr_ty_bound(rhs, param, elem, locals, ctx);
            match op {
                BinOp::Div | BinOp::Pow => Ty::Float,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod | BinOp::FloorDiv => {
                    if *op == BinOp::Add && (l == Ty::Str || r == Ty::Str) {
                        Ty::Str
                    } else if l == Ty::Float || r == Ty::Float {
                        Ty::Float
                    } else if l == Ty::Int || r == Ty::Int {
                        Ty::Int
                    } else {
                        Ty::Unknown
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                | BinOp::And | BinOp::Or | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => {
                    Ty::Bool
                }
                _ => Ty::Unknown,
            }
        }
        // Other forms do not depend on `param` for their result type — delegate.
        _ => infer_expr_ty(e, locals, ctx),
    }
}

/// Infer a comprehension element expression's type given the loop variable's
/// type and name, for `infer_expr_ty`'s comprehension arms. Pure port of
/// codegen's `infer_comp_elt_type_with_var`.
fn infer_comp_elt_type_with_var(
    elt: &Expr,
    loop_var_ty: &Ty,
    loop_var_name: &str,
    ctx: &TyCtx,
) -> Ty {
    match elt {
        // [i.field for i in items] or [i.a.b for i in items]
        Expr::Attr { obj, name, .. } => {
            let obj_ty = if let Expr::Ident(var_name, _) = obj.as_ref() {
                if var_name == loop_var_name {
                    loop_var_ty.clone()
                } else {
                    Ty::Unknown
                }
            } else {
                infer_comp_elt_type_with_var(obj, loop_var_ty, loop_var_name, ctx)
            };
            if let Ty::Class(cls) = obj_ty {
                if let Some(c) = ctx.classes.get(cls.as_str()) {
                    if let Some(f) = c.fields.iter().find(|f| f.name == *name) {
                        return Ty::from_type_expr(&f.ty).unwrap_or(Ty::Unknown);
                    }
                }
            }
            Ty::Unknown
        }
        // [i.method() for i in items]
        Expr::Call { callee, .. } => {
            if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                let obj_ty = if let Expr::Ident(var_name, _) = obj.as_ref() {
                    if var_name == loop_var_name {
                        loop_var_ty.clone()
                    } else {
                        Ty::Unknown
                    }
                } else {
                    infer_comp_elt_type_with_var(obj, loop_var_ty, loop_var_name, ctx)
                };
                if let Ty::Class(cls) = obj_ty {
                    if let Some(method_sig) = ctx.get_method(cls.as_str(), name) {
                        return method_sig.ret.clone();
                    }
                }
            }
            Ty::Unknown
        }
        // [i.a + i.b for i in items] - infer from BinOp.
        Expr::BinOp { lhs, op, rhs, .. } => {
            let left_ty = infer_comp_elt_type_with_var(lhs, loop_var_ty, loop_var_name, ctx);
            let right_ty = infer_comp_elt_type_with_var(rhs, loop_var_ty, loop_var_name, ctx);
            match (left_ty, right_ty) {
                (Ty::Float, _) | (_, Ty::Float) => Ty::Float,
                (Ty::Int, Ty::Int) => {
                    if *op == BinOp::Div || *op == BinOp::Pow {
                        Ty::Float
                    } else {
                        Ty::Int
                    }
                }
                _ => Ty::Unknown,
            }
        }
        _ => Ty::Unknown,
    }
}

/// Resolve a lambda parameter's annotation to a `Ty`. Lambda params are
/// untyped in the surface syntax; the parser records the placeholder
/// `TypeExpr::Named("Any")`. That sentinel must resolve to `Ty::Unknown` (not
/// the bogus `Ty::Class("Any")` the generic resolver would produce), so a
/// param-dependent lambda body stays permissive instead of spuriously typing as
/// a nonexistent class.
fn lambda_param_ty(param_ty: &TypeExpr) -> Ty {
    if let TypeExpr::Named(n) = param_ty {
        if n == "Any" {
            return Ty::Unknown;
        }
    }
    Ty::from_type_expr(param_ty).unwrap_or(Ty::Unknown)
}

/// Infer the return type of a callable applied to a single element of type
/// `elem`, for the `map`/`filter` special cases.
///
/// When `callable` is an inline `lambda` with at least one parameter, its first
/// param is bound to `elem` (or `Unknown` when the iterable element type is
/// unknown) in a temporary env, the body is type-checked, and its inferred type
/// is returned as `Some(body_ty)`. For any other callable (a named function,
/// `def`-bound variable, etc.) or a parameterless lambda, the expression is
/// still type-checked for its own errors and `None` is returned so the caller
/// stays permissive (yields `Ty::Unknown`). This never narrows
/// `types_compatible`; it only widens positive inference.
fn lambda_ret_with_elem(
    callable: &Expr,
    elem: Option<&Ty>,
    env: &mut FuncEnv,
) -> Result<Option<Ty>> {
    if let Expr::Lambda { params, body, .. } = callable {
        if !params.is_empty() {
            let mut lambda_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: Ty::Unknown,
                used_vars: env.used_vars.clone(),
                params: std::collections::HashSet::new(),
                reassigned_params: std::collections::HashSet::new(),
                returned_params: std::collections::HashSet::new(),
                by_ref_params: std::collections::HashSet::new(),
            };
            // Bind every param: the first to the iterable element type, the
            // rest to their declared type or Unknown (map/filter pass a single
            // element, so only the first param is meaningfully constrained).
            for (i, (param_name, param_ty)) in params.iter().enumerate() {
                let ty = if i == 0 {
                    elem.cloned().unwrap_or(Ty::Unknown)
                } else {
                    lambda_param_ty(param_ty)
                };
                lambda_env.locals.insert(param_name.clone(), ty);
            }
            let body_ty = check_expr(body, &mut lambda_env)?;
            return Ok(Some(body_ty));
        }
    }
    // Non-lambda callable (or zero-param lambda): still check it for its own
    // errors, but we cannot infer an applied return type here.
    check_expr(callable, env)?;
    Ok(None)
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
        Expr::IfExp { test, body, orelse, span } => {
            check_expr(test, env)?;
            let bt = check_expr(body, env)?;
            let ot = check_expr(orelse, env)?;
            // Both arms must agree; the more concrete side wins so a branch like
            // `[]` (List(Unknown)) unifies with `[1, 2, 3]` (List(Int)).
            unify_branch_types(bt.clone(), ot.clone()).ok_or_else(|| Error::Type {
                span: *span,
                msg: format!(
                    "conditional expression branches have incompatible types: `{:?}` vs `{:?}`",
                    bt, ot
                ),
            })?
        }
        Expr::ListComp { elt, target, iter, cond, .. } => {
            let iter_ty = check_expr(iter, env)?;
            let elem_ty = match &iter_ty {
                Ty::List(inner) => *inner.clone(),
                Ty::Set(inner) => *inner.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Int, // ranges and unknown iterables -> Int
            };
            // Create a new scope with the loop variable bound
            let mut inner_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: env.ret_ty.clone(),
                used_vars: env.used_vars.clone(),
                params: env.params.clone(),
                reassigned_params: env.reassigned_params.clone(),
                returned_params: env.returned_params.clone(),
                by_ref_params: env.by_ref_params.clone(),
            };
            inner_env.locals.insert(target.clone(), elem_ty);
            if let Some(c) = cond { check_expr(c, &mut inner_env)?; }
            let elt_ty = check_expr(elt, &mut inner_env)?;
            Ty::List(Box::new(elt_ty))
        }
        Expr::SetComp { elt, target, iter, cond, span } => {
            let iter_ty = check_expr(iter, env)?;
            let elem_ty = match &iter_ty {
                Ty::List(inner) => *inner.clone(),
                Ty::Set(inner) => *inner.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Int,
            };
            let mut inner_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: env.ret_ty.clone(),
                used_vars: env.used_vars.clone(),
                params: env.params.clone(),
                reassigned_params: env.reassigned_params.clone(),
                returned_params: env.returned_params.clone(),
                by_ref_params: env.by_ref_params.clone(),
            };
            inner_env.locals.insert(target.clone(), elem_ty);
            if let Some(c) = cond { check_expr(c, &mut inner_env)?; }
            let elt_ty = check_expr(elt, &mut inner_env)?;
            // Same hashability rule as set literals: a Float element produces
            // the uncompilable `HashSet<f64>`, so reject it here too.
            require_hashable(&elt_ty, *span, "set element")?;
            Ty::Set(Box::new(elt_ty))
        }
        Expr::DictComp { key, val, target, iter, cond, span } => {
            let iter_ty = check_expr(iter, env)?;
            let elem_ty = match &iter_ty {
                Ty::List(inner) => *inner.clone(),
                Ty::Set(inner) => *inner.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Int,
            };
            let mut inner_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: env.ret_ty.clone(),
                used_vars: env.used_vars.clone(),
                params: env.params.clone(),
                reassigned_params: env.reassigned_params.clone(),
                returned_params: env.returned_params.clone(),
                by_ref_params: env.by_ref_params.clone(),
            };
            inner_env.locals.insert(target.clone(), elem_ty);
            if let Some(c) = cond { check_expr(c, &mut inner_env)?; }
            let key_ty = check_expr(key, &mut inner_env)?;
            let val_ty = check_expr(val, &mut inner_env)?;
            // Same hashability rule as dict literals: a Float KEY produces the
            // uncompilable `HashMap<f64, _>`. Values may be Float.
            require_hashable(&key_ty, *span, "dict key")?;
            Ty::Dict(Box::new(key_ty), Box::new(val_ty))
        }
        Expr::None_(_) => Ty::NoneVal,
        Expr::List(elems, span) => {
            let elem_ty = if elems.is_empty() {
                Ty::Unknown
            } else {
                // Unify all element types: every element is checked (for its own
                // errors), and their types are folded together. A genuinely
                // heterogeneous literal (two both-concrete, non-Unknown,
                // non-numeric-mixable types) is rejected here instead of being
                // silently typed as `List(first-element-type)` and deferred to
                // rustc. Int/Float mixing and Unknown elements stay permissive.
                let mut acc = check_expr(&elems[0], env)?;
                for e in &elems[1..] {
                    let next = check_expr(e, env)?;
                    // Lists may hold floats, so int/float elements widen to Float.
                    acc = unify_elem_types(acc.clone(), next.clone(), true).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "list elements have incompatible types: {:?} vs {:?}",
                            acc, next
                        ),
                    })?;
                }
                acc
            };
            Ty::List(Box::new(elem_ty))
        }
        Expr::Set(elems, span) => {
            let elem_ty = if elems.is_empty() {
                Ty::Unknown
            } else {
                // Same element-type unification as list literals above, but
                // WITHOUT Int/Float widening: a set's element type must be
                // hashable and `set[float]` (`HashSet<f64>`) is not representable
                // in Rust, so `{1, 2.0}` is rejected rather than typed Set(Float).
                let mut acc = check_expr(&elems[0], env)?;
                for e in &elems[1..] {
                    let next = check_expr(e, env)?;
                    acc = unify_elem_types(acc.clone(), next.clone(), false).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "set elements have incompatible types: {:?} vs {:?}",
                            acc, next
                        ),
                    })?;
                }
                acc
            };
            // A pure-float set literal (`{1.0, 2.0}`) folds to Set(Float), which
            // codegen would emit as the uncompilable `HashSet<f64>`. Reject it
            // here so typeck and codegen agree. (`{1, 2.0}` is already rejected
            // by the widen_numeric=false fold above; this closes the all-float
            // case.) Unknown element types (`set()`) stay permissive.
            require_hashable(&elem_ty, *span, "set element")?;
            Ty::Set(Box::new(elem_ty))
        }
        Expr::Dict(pairs, span) => {
            if pairs.is_empty() {
                Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown))
            } else {
                // Unify all key types and all value types independently via a
                // left-to-right fold. Genuinely heterogeneous dicts (two
                // both-concrete incompatible key or value types) are rejected
                // here instead of silently using first-pair types and deferring
                // the error to rustc. widen_numeric=false for both: float dict
                // keys are non-hashable (HashMap<f64,_> doesn't compile), and
                // there is no codegen value-cast for dict values, so mixed
                // Int/Float values would also fail at rustc.
                let mut k_ty = check_expr(&pairs[0].0, env)?;
                let mut v_ty = check_expr(&pairs[0].1, env)?;
                for (k, v) in &pairs[1..] {
                    let kt = check_expr(k, env)?;
                    let vt = check_expr(v, env)?;
                    k_ty = unify_elem_types(k_ty.clone(), kt.clone(), false).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "dict keys have incompatible types: {:?} vs {:?}",
                            k_ty, kt
                        ),
                    })?;
                    v_ty = unify_elem_types(v_ty.clone(), vt.clone(), false).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "dict values have incompatible types: {:?} vs {:?}",
                            v_ty, vt
                        ),
                    })?;
                }
                // A float-keyed dict literal (`{1.0: "a"}`) folds to Dict(Float, _),
                // which codegen would emit as the uncompilable `HashMap<f64, _>`.
                // Reject the KEY only — float VALUES are fine (`HashMap<_, f64>`
                // compiles), so v_ty is left untouched.
                require_hashable(&k_ty, *span, "dict key")?;
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
                    } else if (name == "min" || name == "max") && args.len() == 1 {
                        // Single-iterable min/max: the result is the element type
                        // of the list/set argument. A `key=`/other kwarg may also
                        // be present (e.g. `min(words, key=len)`) — the lone
                        // positional arg is still the iterable. The 2-arg form
                        // `min(a, b)` falls through to the generic path below and
                        // stays Unknown (Rust's std::cmp::min already resolves it).
                        let arg_ty = check_expr(&args[0], env)?;
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        match arg_ty {
                            Ty::List(elem) | Ty::Set(elem) => *elem,
                            _ => Ty::Unknown,
                        }
                    } else if name == "enumerate" && !args.is_empty() {
                        // enumerate(iterable[, start]) -> List(Tuple(Int, elem))
                        // Check all args/kwargs for their own errors first.
                        let arg0_ty = check_expr(&args[0], env)?;
                        for a in &args[1..] {
                            check_expr(a, env)?;
                        }
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        let elem = match arg0_ty {
                            Ty::List(inner) | Ty::Set(inner) => *inner,
                            Ty::Str => Ty::Str,
                            _ => Ty::Unknown,
                        };
                        if matches!(elem, Ty::Unknown) {
                            Ty::Unknown
                        } else {
                            Ty::List(Box::new(Ty::Tuple(vec![Ty::Int, elem])))
                        }
                    } else if name == "zip" {
                        // zip(a, b, ...) -> List(Tuple(elem_a, elem_b, ...))
                        // Check all args/kwargs for their own errors first.
                        let mut elem_tys: Vec<Ty> = Vec::new();
                        let mut any_unknown = false;
                        for a in args {
                            let ty = check_expr(a, env)?;
                            match ty {
                                Ty::List(inner) | Ty::Set(inner) => elem_tys.push(*inner),
                                Ty::Str => elem_tys.push(Ty::Str),
                                _ => any_unknown = true,
                            }
                        }
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        if any_unknown || elem_tys.is_empty() {
                            Ty::Unknown
                        } else {
                            Ty::List(Box::new(Ty::Tuple(elem_tys)))
                        }
                    } else if name == "map" && args.len() == 2 {
                        // map(f, iterable) -> List(return type of f applied to the
                        // iterable's element type). Only a List iterable yields a
                        // concrete result: codegen's `.iter().cloned().map(..)`
                        // compiles for a Vec, but a String has no `.iter()` and
                        // map-over-set is unverified, so Set/Str/unknown stay
                        // permissive (Unknown), matching the filter arm below. The
                        // lambda body is still checked for its own errors, and we
                        // never narrow types_compatible.
                        let iter_ty = check_expr(&args[1], env)?;
                        let elem = match &iter_ty {
                            Ty::List(inner) => Some((**inner).clone()),
                            _ => None,
                        };
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        let body_ty = lambda_ret_with_elem(&args[0], elem.as_ref(), env)?;
                        match (&iter_ty, body_ty) {
                            (Ty::List(_), Some(t)) if !matches!(t, Ty::Unknown) => {
                                Ty::List(Box::new(t))
                            }
                            _ => Ty::Unknown,
                        }
                    } else if name == "filter" && args.len() == 2 {
                        // filter(pred, iterable) -> the iterable's list type
                        // unchanged (filter preserves elements). The predicate body
                        // is still checked (binding its first param to the element
                        // type) so a malformed predicate is caught; its return type
                        // is irrelevant to the result element type.
                        let iter_ty = check_expr(&args[1], env)?;
                        let elem = match &iter_ty {
                            Ty::List(inner) | Ty::Set(inner) => Some((**inner).clone()),
                            Ty::Str => Some(Ty::Str),
                            _ => None,
                        };
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        let _ = lambda_ret_with_elem(&args[0], elem.as_ref(), env)?;
                        match iter_ty {
                            Ty::List(_) => iter_ty,
                            _ => Ty::Unknown,
                        }
                    } else if let Some(sig) = env.ctx.funcs.get(name.as_str()) {
                        // Regular function call: check arity (positional only in v0).
                        let expected = sig.params.len();
                        let got = args.len() + kwargs.len();
                        // Variadic builtins: skip arity check.
                        let variadic = matches!(name.as_str(),
                            "print" | "range" | "len" | "str" | "int" | "float" | "bool" | "enumerate" | "zip"
                            | "abs" | "min" | "max" | "sorted" | "sum" | "input" | "list" | "dict" | "tuple" | "set"
                            | "getattr" | "setattr" | "hasattr" | "open");
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
                        let sig_params = sig.params.clone();
                        let sig_by_ref = sig.param_by_ref.clone();
                        let sig_ret = sig.ret.clone();
                        for (i, a) in args.iter().enumerate() {
                            // EPIC-4 V2: an argument bound to a by-reference
                            // (`Mut[T]`) param must be a PLACE — an lvalue we can
                            // take `&mut` of (variable / field / index). A
                            // temporary (call/constructor/literal/binop result)
                            // has no caller-visible storage to borrow, so it is an
                            // honest typeck error here rather than a later rustc
                            // borrow failure. The arg's TYPE is still checked
                            // against the inner `T` by the compatibility check
                            // below (the param type was unwrapped from `Mut[T]`).
                            if sig_by_ref.get(i).copied().unwrap_or(false)
                                && !is_place_expr(a)
                            {
                                let pname = sig_params.get(i)
                                    .map(|(n, _)| n.as_str())
                                    .unwrap_or("<arg>");
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!(
                                        "by-reference parameter `{}` requires a variable, not a temporary",
                                        pname
                                    ),
                                });
                            }
                            let arg_ty = check_expr(a, env)?;
                            // Concrete-only positional arg-type check (skip variadic builtins).
                            // Only fires when BOTH param and arg types are concrete and
                            // incompatible. Int->Float is explicitly allowed (Python coercion).
                            if !variadic {
                                if let Some((_, param_ty)) = sig_params.get(i) {
                                    let int_to_float =
                                        matches!(arg_ty, Ty::Int) && matches!(param_ty, Ty::Float);
                                    if !int_to_float
                                        && !matches!(arg_ty, Ty::Unknown)
                                        && !matches!(param_ty, Ty::Unknown)
                                        && !types_compatible(&arg_ty, param_ty)
                                    {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "argument {} to `{}`: expected {:?}, found {:?}",
                                                i + 1, name, param_ty, arg_ty
                                            ),
                                        });
                                    }
                                }
                            }
                        }
                        sig_ret
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
                            if let Some(sig) = env.ctx.funcs.get(&key).cloned() {
                                // (EPIC-4 V2-c) Enforce the by-reference (`Mut[T]`)
                                // place-requirement at METHOD call sites too (it was
                                // already enforced for free functions in V2-ab). An
                                // arg bound to a by-ref method param must be a PLACE
                                // (Ident/Attr/Index) — a temporary has no
                                // caller-visible storage to borrow `&mut`. We look
                                // up the by-ref flags via get_method, whose vectors
                                // are self-EXCLUSIVE and index-aligned to `args`
                                // (mirrors the resolver alignment fixed in STEP 0).
                                let method_sig = env.ctx.get_method(class_name, name);
                                if let Some(msig) = &method_sig {
                                    for (i, a) in args.iter().enumerate() {
                                        if msig.param_by_ref.get(i).copied().unwrap_or(false)
                                            && !is_place_expr(a)
                                        {
                                            let pname = msig.params.get(i)
                                                .map(|(n, _)| n.as_str())
                                                .unwrap_or("<arg>");
                                            return Err(Error::Type {
                                                span: *span,
                                                msg: format!(
                                                    "by-reference parameter `{}` requires a variable, not a temporary",
                                                    pname
                                                ),
                                            });
                                        }
                                    }
                                }
                                for a in args { check_expr(a, env)?; }
                                return Ok(sig.ret.clone());
                            }
                        }
                        // (a) Builtin method existence — only on concrete Str/List/Set/Dict.
                        // Skipped for Unknown (unprovable) and Class (handled above).
                        if let Some((type_name, table)) = builtin_method_table(&obj_ty) {
                            if !table.contains(&name.as_str()) {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!("type `{}` has no method `{}`", type_name, name),
                                });
                            }
                            // (b) Detect in-place mutating method calls on a by-value param.
                            // e.g. `visited.add(node)` where `visited` is a Set parameter,
                            // OR `param.field.append(x)` / `param[0].add(x)` — a mutator on
                            // any PLACE rooted at the param (the mutation is lost on the
                            // caller's clone either way). EPIC-4 V2-d closes the former
                            // nested-mutation gap: we now root the receiver via `root_ident`
                            // (like the AttrAssign / IndexAssign backstops already do)
                            // instead of requiring the receiver to be the bare param ident.
                            // `obj_ty` is the RECEIVER's type (the collection being mutated),
                            // which is always owned inside this builtin-method-table arm, so
                            // the `is_owned(&obj_ty)` guard still holds for the field/index
                            // case. The suppressions are preserved verbatim: self-exclusion,
                            // reassigned, returned, and — critically — by_ref (`Mut[T]`)
                            // params, whose nested mutation IS caller-visible and must NOT
                            // fire.
                            if MUTATING_METHODS.contains(&name.as_str()) {
                                if let Some(param_name) = root_ident(obj) {
                                    if param_name != "self"
                                        && env.params.contains(param_name)
                                        && !env.reassigned_params.contains(param_name)
                                        && !env.returned_params.contains(param_name)
                                        && !env.by_ref_params.contains(param_name)
                                        && is_owned(&obj_ty)
                                    {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: by_value_mutation_error(param_name),
                                        });
                                    }
                                }
                            }
                            // (c) Element-type argument check for set mutators only.
                            if let Some(elem_ty) = elem_arg_check_ty(&obj_ty, name) {
                                if let Some(arg0) = args.first() {
                                    let arg_ty = check_expr(arg0, env)?;
                                    let int_to_float =
                                        matches!(arg_ty, Ty::Int) && matches!(elem_ty, Ty::Float);
                                    if !int_to_float
                                        && !matches!(arg_ty, Ty::Unknown)
                                        && !matches!(elem_ty, Ty::Unknown)
                                        && !types_compatible(&arg_ty, &elem_ty)
                                    {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "argument to `{}.{}`: expected element type {:?}, found {:?}",
                                                type_name, name, elem_ty, arg_ty
                                            ),
                                        });
                                    }
                                }
                            }
                            for a in args { check_expr(a, env)?; }
                            // dict.get is arg-count-aware: get(k) -> Optional[V],
                            // get(k, default) -> V. Route through the shared helper
                            // so the checker and the inference oracle agree; fall
                            // back to builtin_method_ret for every other method.
                            if let Some(t) = dict_get_ret(&obj_ty, name.as_str(), args.len()) {
                                return Ok(t);
                            }
                            return Ok(builtin_method_ret(&obj_ty, name.as_str()));
                        }
                    }
                    // Inline lambda call `(lambda x: body)(args)`: the call's
                    // value type is the lambda body type. The Lambda arm now
                    // returns that body type (instead of Unknown), so for an
                    // immediately-invoked lambda we surface it to the caller
                    // rather than degrading to Unknown. Other callees (e.g. a
                    // variable holding a value) remain Unknown — this only widens
                    // inference and never narrows types_compatible.
                    let callee_ty = check_expr(callee, env)?;
                    for a in args { check_expr(a, env)?; }
                    if matches!(callee.as_ref(), Expr::Lambda { .. }) {
                        callee_ty
                    } else {
                        Ty::Unknown
                    }
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
        Expr::BinOp { op, lhs, rhs, span } => {
            let l = check_expr(lhs, env)?;
            let r = check_expr(rhs, env)?;
            // (EPIC-5) Reject using a raw `Optional[T]` operand without narrowing.
            // An Option only supports identity/equality testing against `None`
            // (`is` / `is not` / `==` / `!=`); any other operator (arithmetic,
            // ordering, membership, boolean) on an un-narrowed Optional is an
            // honest error — the value must be narrowed via `is None` /
            // `is not None` first (see PYTHON_COMPATIBILITY.md, Optional section).
            // Without this, `x + 1` on an `Optional[int]` would infer `Unknown`
            // and silently slip through, then miscompile.
            let nullary_ok = matches!(op, BinOp::Is | BinOp::IsNot | BinOp::Eq | BinOp::Ne);
            if !nullary_ok && (matches!(l, Ty::Option(_)) || matches!(r, Ty::Option(_))) {
                return Err(Error::Type {
                    span: *span,
                    msg: format!(
                        "operator on an Optional value requires narrowing first: \
                         use `if x is not None:` to obtain the inner value before applying `{:?}`",
                        op
                    ),
                });
            }
            match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le
                | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or
                | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => Ty::Bool,
                BinOp::Pow | BinOp::Div => Ty::Float,  // Division always returns float in Python
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::LShift | BinOp::RShift => Ty::Int,
                _ => {
                    // Arithmetic: apply numeric type promotion rules
                    match (&l, &r) {
                        // Operator overloading: a class lhs dispatches to the
                        // declared return type of its dunder (__add__/__sub__/__mul__).
                        (Ty::Class(cls), _) => {
                            let dunder = match op {
                                BinOp::Add => Some("__add__"),
                                BinOp::Sub => Some("__sub__"),
                                BinOp::Mul => Some("__mul__"),
                                _ => None,
                            };
                            dunder.and_then(|d| env.ctx.get_method(cls, d))
                                .map(|s| s.ret.clone())
                                .unwrap_or_else(|| l.clone())
                        }
                        // Same type: return that type
                        (a, b) if a == b => l,
                        // Mixed numeric types: promote to float
                        (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => Ty::Float,
                        // String + String = String (for concatenation)
                        (Ty::Str, Ty::Str) => Ty::Str,
                        // List + List = List (for concatenation)
                        (Ty::List(inner_l), Ty::List(inner_r)) if inner_l == inner_r => Ty::List(inner_l.clone()),
                        // Otherwise unknown
                        _ => Ty::Unknown,
                    }
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
                params: std::collections::HashSet::new(),
                reassigned_params: std::collections::HashSet::new(),
                returned_params: std::collections::HashSet::new(),
                by_ref_params: std::collections::HashSet::new(),
            };
            for (param_name, param_ty) in params {
                let ty = lambda_param_ty(param_ty);
                lambda_env.locals.insert(param_name.clone(), ty);
            }
            // The lambda's value type is its body type. For an inline call
            // `(lambda x: x + 1)(5)` this lets the caller see a concrete type
            // instead of Unknown. Lambda params are untyped here (no annotation
            // syntax), so a param-dependent body still yields Unknown — which is
            // sound and never narrows types_compatible.
            check_expr(body, &mut lambda_env)?
        }
    })
}

// =============================================================================
// UNIT TESTS
// Architecture: one in-file #[cfg(test)] block at the bottom of the module so
// private items (types_compatible, check_expr, check_stmt, FuncEnv) are
// accessible without pub-widening any production code.
//
// Four categories:
//   A. types_compatible matrix         (~19 cases)
//   B. builtin_method_ret              (~20 cases)
//   C. inference via check_expr/stmt   (~24 cases)
//   D. error-firing                    (~13 cases)
//
// CHARACTERIZATION philosophy: each test asserts the code's ACTUAL current
// behaviour. Where behaviour is a known limitation or design choice, a comment
// marks it (BUG 1, BUG 2, BUG 3).
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BinOp, Expr, Stmt, TypeExpr, UnOp};
    use crate::diag::Span;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Build a minimal FuncEnv backed by a fresh TyCtx, returning Unit.
    fn make_env(ctx: &TyCtx) -> FuncEnv<'_> {
        FuncEnv::with_by_ref(ctx, &[], &[], Ty::Unit)
    }

    /// Build a FuncEnv with a declared return type.
    fn make_env_ret(ctx: &TyCtx, ret: Ty) -> FuncEnv<'_> {
        FuncEnv::with_by_ref(ctx, &[], &[], ret)
    }

    /// Construct a Call expr: callee is an Ident, no kwargs.
    fn call_fn(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            callee: Box::new(Expr::Ident(name.to_string(), Span::DUMMY)),
            args,
            kwargs: vec![],
            span: Span::DUMMY,
        }
    }

    /// Construct a method-call expr: obj.method(args).
    fn method_call(obj: Expr, method: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            callee: Box::new(Expr::Attr {
                obj: Box::new(obj),
                name: method.to_string(),
                span: Span::DUMMY,
            }),
            args,
            kwargs: vec![],
            span: Span::DUMMY,
        }
    }

    /// Ident shorthand.
    fn ident(name: &str) -> Expr {
        Expr::Ident(name.to_string(), Span::DUMMY)
    }

    /// Int literal shorthand.
    fn int_lit(v: i64) -> Expr { Expr::Int(v, Span::DUMMY) }

    /// Float literal shorthand.
    fn float_lit(v: f64) -> Expr { Expr::Float(v, Span::DUMMY) }

    /// Str literal shorthand.
    fn str_lit(s: &str) -> Expr { Expr::Str(s.to_string(), Span::DUMMY) }

    /// Bool literal shorthand.
    fn bool_lit(v: bool) -> Expr { Expr::Bool(v, Span::DUMMY) }

    /// Assert that a Result<Ty> is a Type error whose message contains `fragment`.
    fn assert_type_err(r: Result<Ty>, fragment: &str) {
        match r {
            Err(Error::Type { msg, .. }) => {
                assert!(
                    msg.contains(fragment),
                    "expected error containing {:?}, got msg: {:?}",
                    fragment, msg
                );
            }
            Err(other) => panic!("expected Type error, got {:?}", other),
            Ok(ty) => panic!("expected Type error, got Ok({:?})", ty),
        }
    }

    /// Same but for Result<()> (check_stmt).
    fn assert_stmt_type_err(r: Result<()>, fragment: &str) {
        match r {
            Err(Error::Type { msg, .. }) => {
                assert!(
                    msg.contains(fragment),
                    "expected error containing {:?}, got msg: {:?}",
                    fragment, msg
                );
            }
            Err(other) => panic!("expected Type error, got {:?}", other),
            Ok(()) => panic!("expected Type error, got Ok(())"),
        }
    }

    // =========================================================================
    // Category A — types_compatible matrix
    // =========================================================================

    #[test]
    fn compat_exact_int() {
        assert!(types_compatible(&Ty::Int, &Ty::Int));
    }

    #[test]
    fn compat_exact_str() {
        assert!(types_compatible(&Ty::Str, &Ty::Str));
    }

    #[test]
    fn compat_exact_list_int() {
        assert!(types_compatible(
            &Ty::List(Box::new(Ty::Int)),
            &Ty::List(Box::new(Ty::Int))
        ));
    }

    #[test]
    fn compat_int_vs_str_false() {
        assert!(!types_compatible(&Ty::Int, &Ty::Str));
    }

    #[test]
    fn compat_int_vs_float_false() {
        // No implicit widening in types_compatible itself; caller handles Int→Float.
        assert!(!types_compatible(&Ty::Int, &Ty::Float));
    }

    #[test]
    fn compat_unknown_lhs() {
        assert!(types_compatible(&Ty::Unknown, &Ty::Int));
    }

    #[test]
    fn compat_unknown_rhs() {
        assert!(types_compatible(&Ty::Int, &Ty::Unknown));
    }

    #[test]
    fn compat_both_unknown() {
        assert!(types_compatible(&Ty::Unknown, &Ty::Unknown));
    }

    #[test]
    fn compat_list_unknown_elem_lhs() {
        // List(Unknown) is compatible with List(Int): wildcard-from-left arm.
        assert!(types_compatible(
            &Ty::List(Box::new(Ty::Unknown)),
            &Ty::List(Box::new(Ty::Int))
        ));
    }

    #[test]
    fn compat_list_unknown_elem_rhs() {
        // List(Int) compatible with List(Unknown): wildcard-from-right arm.
        assert!(types_compatible(
            &Ty::List(Box::new(Ty::Int)),
            &Ty::List(Box::new(Ty::Unknown))
        ));
    }

    #[test]
    fn compat_list_concrete_mismatch() {
        // List(Int) vs List(Str): neither side has Unknown inner → false.
        assert!(!types_compatible(
            &Ty::List(Box::new(Ty::Int)),
            &Ty::List(Box::new(Ty::Str))
        ));
    }

    // ── EPIC-5: Optional / None compatibility ─────────────────────────────────

    #[test]
    fn compat_none_fills_option() {
        // The `None` literal (typed `NoneVal`) fills any Optional slot, including
        // `Optional[Class]` (inner type need not be compatible with NoneVal).
        assert!(types_compatible(&Ty::NoneVal, &Ty::Option(Box::new(Ty::Int))));
        assert!(types_compatible(
            &Ty::NoneVal,
            &Ty::Option(Box::new(Ty::Class("Point".into())))
        ));
    }

    #[test]
    fn compat_void_does_not_fill_option() {
        // SOUNDNESS BACKSTOP (EPIC-5 review blocker): a *void* result (`Ty::Unit`,
        // the `-> None` return of e.g. `print(...)` or any `def f() -> None`) is
        // NOT compatible with an Optional slot. Only the `None` literal (NoneVal)
        // is. Were this true, codegen would emit `Some(void_call())` -> `Option<()>`
        // — a silent miscompile caught only by rustc. This must stay FALSE.
        assert!(!types_compatible(&Ty::Unit, &Ty::Option(Box::new(Ty::Int))));
        assert!(!types_compatible(
            &Ty::Unit,
            &Ty::Option(Box::new(Ty::Class("Point".into())))
        ));
    }

    #[test]
    fn compat_none_literal_satisfies_void_return() {
        // `return None` in a `-> None` (void) function must still typecheck: the
        // Return path compares NoneVal against the declared Unit return type.
        assert!(types_compatible(&Ty::NoneVal, &Ty::Unit));
    }

    #[test]
    fn compat_bare_t_fills_option() {
        // A bare T auto-wraps into Optional[T].
        assert!(types_compatible(&Ty::Int, &Ty::Option(Box::new(Ty::Int))));
        assert!(types_compatible(
            &Ty::Class("Point".into()),
            &Ty::Option(Box::new(Ty::Class("Point".into())))
        ));
    }

    #[test]
    fn compat_option_fills_option_inner() {
        // Optional[T] ~ Optional[T], and Optional[Unknown] is permissive.
        assert!(types_compatible(
            &Ty::Option(Box::new(Ty::Int)),
            &Ty::Option(Box::new(Ty::Int))
        ));
        assert!(types_compatible(
            &Ty::Option(Box::new(Ty::Unknown)),
            &Ty::Option(Box::new(Ty::Int))
        ));
    }

    #[test]
    fn compat_bare_t_fills_option_inner_mismatch_false() {
        // A bare Str does NOT fit Optional[int].
        assert!(!types_compatible(&Ty::Str, &Ty::Option(Box::new(Ty::Int))));
    }

    #[test]
    fn compat_option_does_not_fill_bare_slot() {
        // The directional guard: an Optional value may NOT silently fill a bare
        // slot. Using Optional[int] where int is required is rejected — the
        // honest-rejection backstop that keeps `x + 1` on an un-narrowed Optional
        // an error rather than a silent miscompile.
        assert!(!types_compatible(&Ty::Option(Box::new(Ty::Int)), &Ty::Int));
    }

    #[test]
    fn optional_arithmetic_without_narrowing_rejected() {
        // `x + 1` where x: Optional[int] is an honest error — narrow first.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("x".into(), Ty::Option(Box::new(Ty::Int)));
        let add = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(ident("x")),
            rhs: Box::new(int_lit(1)),
            span: Span::DUMMY,
        };
        assert_type_err(check_expr(&add, &mut env), "requires narrowing");
    }

    #[test]
    fn optional_is_none_comparison_allowed() {
        // `x is None` / `x is not None` are the sanctioned tests on a raw Optional.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("x".into(), Ty::Option(Box::new(Ty::Int)));
        for op in [BinOp::Is, BinOp::IsNot] {
            let cmp = Expr::BinOp {
                op,
                lhs: Box::new(ident("x")),
                rhs: Box::new(Expr::None_(Span::DUMMY)),
                span: Span::DUMMY,
            };
            assert_eq!(check_expr(&cmp, &mut env).unwrap(), Ty::Bool);
        }
    }

    #[test]
    fn optional_not_none_narrows_then_branch() {
        // `if x is not None: y = x + 1` type-checks because x narrows to int in
        // the then branch; the local is restored to Option afterwards.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("x".into(), Ty::Option(Box::new(Ty::Int)));
        let cond = Expr::BinOp {
            op: BinOp::IsNot,
            lhs: Box::new(ident("x")),
            rhs: Box::new(Expr::None_(Span::DUMMY)),
            span: Span::DUMMY,
        };
        let body_assign = Stmt::Assign {
            target: "y".into(),
            ty: None,
            value: Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(ident("x")),
                rhs: Box::new(int_lit(1)),
                span: Span::DUMMY,
            },
            span: Span::DUMMY,
        };
        let if_stmt = Stmt::If {
            cond,
            then: vec![body_assign],
            elifs: vec![],
            else_: None,
            span: Span::DUMMY,
        };
        check_stmt(&if_stmt, &mut env).unwrap();
        // The narrowing must not leak: x is Option again after the if.
        assert_eq!(env.locals.get("x"), Some(&Ty::Option(Box::new(Ty::Int))));
    }

    #[test]
    fn return_none_in_optional_fn_typechecks() {
        // `return None` and `return <bare int>` both satisfy an Optional[int] ret.
        let ctx = TyCtx::new();
        let mut env = make_env_ret(&ctx, Ty::Option(Box::new(Ty::Int)));
        let ret_none = Stmt::Return(Some(Expr::None_(Span::DUMMY)), Span::DUMMY);
        check_stmt(&ret_none, &mut env).unwrap();
        let ret_int = Stmt::Return(Some(int_lit(7)), Span::DUMMY);
        check_stmt(&ret_int, &mut env).unwrap();
    }

    #[test]
    fn compat_set_unknown_elem_lhs() {
        assert!(types_compatible(
            &Ty::Set(Box::new(Ty::Unknown)),
            &Ty::Set(Box::new(Ty::Bool))
        ));
    }

    #[test]
    fn compat_set_unknown_elem_rhs() {
        assert!(types_compatible(
            &Ty::Set(Box::new(Ty::Bool)),
            &Ty::Set(Box::new(Ty::Unknown))
        ));
    }

    #[test]
    fn compat_dict_both_unknown_lhs() {
        // Dict(Unknown,Unknown) vs Dict(Str,Int) → true.
        assert!(types_compatible(
            &Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown)),
            &Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int))
        ));
    }

    #[test]
    fn compat_dict_both_unknown_rhs() {
        // Dict(Str,Int) vs Dict(Unknown,Unknown) → true.
        assert!(types_compatible(
            &Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int)),
            &Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown))
        ));
    }

    #[test]
    fn compat_dict_partial_unknown_false() {
        // BUG 2 (design choice): Dict wildcard requires BOTH k AND v = Unknown.
        // Dict(Unknown, Int) vs Dict(Str, Int) → false because only k is Unknown.
        assert!(!types_compatible(
            &Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Int)),
            &Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int))
        ));
    }

    #[test]
    fn compat_dict_concrete_mismatch() {
        assert!(!types_compatible(
            &Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int)),
            &Ty::Dict(Box::new(Ty::Int), Box::new(Ty::Str))
        ));
    }

    #[test]
    fn compat_class_same() {
        assert!(types_compatible(
            &Ty::Class("Foo".into()),
            &Ty::Class("Foo".into())
        ));
    }

    #[test]
    fn compat_class_different_false() {
        assert!(!types_compatible(
            &Ty::Class("Foo".into()),
            &Ty::Class("Bar".into())
        ));
    }

    // =========================================================================
    // Category B — builtin_method_ret
    // =========================================================================

    #[test]
    fn method_ret_str_upper() {
        assert_eq!(builtin_method_ret(&Ty::Str, "upper"), Ty::Str);
    }

    #[test]
    fn method_ret_str_lower() {
        assert_eq!(builtin_method_ret(&Ty::Str, "lower"), Ty::Str);
    }

    #[test]
    fn method_ret_str_join() {
        assert_eq!(builtin_method_ret(&Ty::Str, "join"), Ty::Str);
    }

    #[test]
    fn method_ret_str_split() {
        assert_eq!(
            builtin_method_ret(&Ty::Str, "split"),
            Ty::List(Box::new(Ty::Str))
        );
    }

    #[test]
    fn method_ret_str_partition() {
        // partition is modelled as list[str] (not a tuple), per the source comment.
        assert_eq!(
            builtin_method_ret(&Ty::Str, "partition"),
            Ty::List(Box::new(Ty::Str))
        );
    }

    #[test]
    fn method_ret_str_rpartition() {
        assert_eq!(
            builtin_method_ret(&Ty::Str, "rpartition"),
            Ty::List(Box::new(Ty::Str))
        );
    }

    #[test]
    fn method_ret_str_find() {
        assert_eq!(builtin_method_ret(&Ty::Str, "find"), Ty::Int);
    }

    #[test]
    fn method_ret_str_count() {
        assert_eq!(builtin_method_ret(&Ty::Str, "count"), Ty::Int);
    }

    #[test]
    fn method_ret_str_startswith() {
        assert_eq!(builtin_method_ret(&Ty::Str, "startswith"), Ty::Bool);
    }

    #[test]
    fn method_ret_str_isdigit() {
        assert_eq!(builtin_method_ret(&Ty::Str, "isdigit"), Ty::Bool);
    }

    #[test]
    fn method_ret_str_unknown_method() {
        assert_eq!(builtin_method_ret(&Ty::Str, "no_such_method"), Ty::Unknown);
    }

    #[test]
    fn method_ret_list_pop() {
        let list_int = Ty::List(Box::new(Ty::Int));
        assert_eq!(builtin_method_ret(&list_int, "pop"), Ty::Int);
    }

    #[test]
    fn method_ret_list_copy() {
        let list_str = Ty::List(Box::new(Ty::Str));
        assert_eq!(
            builtin_method_ret(&list_str, "copy"),
            Ty::List(Box::new(Ty::Str))
        );
    }

    #[test]
    fn method_ret_list_append_is_unit() {
        let list_int = Ty::List(Box::new(Ty::Int));
        assert_eq!(builtin_method_ret(&list_int, "append"), Ty::Unit);
    }

    #[test]
    fn method_ret_list_index() {
        let list_int = Ty::List(Box::new(Ty::Int));
        assert_eq!(builtin_method_ret(&list_int, "index"), Ty::Int);
    }

    #[test]
    fn method_ret_set_pop() {
        let set_str = Ty::Set(Box::new(Ty::Str));
        assert_eq!(builtin_method_ret(&set_str, "pop"), Ty::Str);
    }

    #[test]
    fn method_ret_set_union() {
        let set_int = Ty::Set(Box::new(Ty::Int));
        assert_eq!(
            builtin_method_ret(&set_int, "union"),
            Ty::Set(Box::new(Ty::Int))
        );
    }

    #[test]
    fn method_ret_set_issubset() {
        let set_int = Ty::Set(Box::new(Ty::Int));
        assert_eq!(builtin_method_ret(&set_int, "issubset"), Ty::Bool);
    }

    #[test]
    fn method_ret_dict_keys() {
        let dict = Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int));
        assert_eq!(
            builtin_method_ret(&dict, "keys"),
            Ty::List(Box::new(Ty::Str))
        );
    }

    #[test]
    fn method_ret_dict_values() {
        let dict = Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int));
        assert_eq!(
            builtin_method_ret(&dict, "values"),
            Ty::List(Box::new(Ty::Int))
        );
    }

    #[test]
    fn method_ret_dict_items() {
        let dict = Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int));
        assert_eq!(
            builtin_method_ret(&dict, "items"),
            Ty::List(Box::new(Ty::Tuple(vec![Ty::Str, Ty::Int])))
        );
    }

    #[test]
    fn method_ret_dict_pop() {
        let dict = Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Bool));
        assert_eq!(builtin_method_ret(&dict, "pop"), Ty::Bool);
    }

    #[test]
    fn method_ret_dict_copy() {
        let dict = Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int));
        assert_eq!(
            builtin_method_ret(&dict, "copy"),
            Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int))
        );
    }

    #[test]
    fn method_ret_file_read() {
        assert_eq!(builtin_method_ret(&Ty::File, "read"), Ty::Str);
    }

    #[test]
    fn method_ret_file_readlines() {
        assert_eq!(
            builtin_method_ret(&Ty::File, "readlines"),
            Ty::List(Box::new(Ty::Str))
        );
    }

    #[test]
    fn method_ret_file_write_is_unit() {
        assert_eq!(builtin_method_ret(&Ty::File, "write"), Ty::Unit);
    }

    // =========================================================================
    // Category C — inference via check_expr / check_stmt
    // =========================================================================

    #[test]
    fn infer_int_literal() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        assert_eq!(check_expr(&int_lit(42), &mut env).unwrap(), Ty::Int);
    }

    #[test]
    fn infer_float_literal() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        assert_eq!(check_expr(&float_lit(3.14), &mut env).unwrap(), Ty::Float);
    }

    #[test]
    fn infer_str_literal() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        assert_eq!(check_expr(&str_lit("hi"), &mut env).unwrap(), Ty::Str);
    }

    #[test]
    fn infer_bool_literal() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        assert_eq!(check_expr(&bool_lit(true), &mut env).unwrap(), Ty::Bool);
    }

    #[test]
    fn infer_none_literal() {
        // The `None` literal types as `NoneVal` (distinct from a void function's
        // `Unit` return) so that void results never satisfy an Optional slot.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        assert_eq!(check_expr(&Expr::None_(Span::DUMMY), &mut env).unwrap(), Ty::NoneVal);
    }

    #[test]
    fn infer_list_of_ints() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::List(vec![int_lit(1), int_lit(2), int_lit(3)], Span::DUMMY);
        assert_eq!(
            check_expr(&e, &mut env).unwrap(),
            Ty::List(Box::new(Ty::Int))
        );
    }

    #[test]
    fn infer_empty_list_is_unknown() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::List(vec![], Span::DUMMY);
        assert_eq!(
            check_expr(&e, &mut env).unwrap(),
            Ty::List(Box::new(Ty::Unknown))
        );
    }

    #[test]
    fn error_heterogeneous_list_rejected() {
        // A list mixing two genuinely-incompatible concrete types (Int vs Str)
        // is rejected at the type checker rather than silently typed as the
        // first element's type and deferred to rustc.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::List(vec![int_lit(1), str_lit("oops")], Span::DUMMY);
        let err = check_expr(&e, &mut env).unwrap_err();
        match err {
            Error::Type { msg, .. } => {
                assert!(
                    msg.contains("incompatible types"),
                    "expected incompatible-types message, got: {msg}"
                );
            }
            other => panic!("expected Error::Type, got {other:?}"),
        }
    }

    #[test]
    fn infer_list_int_float_unifies_to_float() {
        // `[1, 2.0]` is accepted and widens to `List(Float)`: typeck unifies the
        // numeric elements and codegen casts the int elements to f64 so the
        // emitted `Vec<f64>` is homogeneous and compiles (card 5c2f31d8).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::List(vec![int_lit(1), float_lit(2.0)], Span::DUMMY);
        assert_eq!(
            check_expr(&e, &mut env).unwrap(),
            Ty::List(Box::new(Ty::Float))
        );
        // Order-independent: Float first then Int also unifies to Float.
        let e2 = Expr::List(vec![float_lit(1.5), int_lit(2)], Span::DUMMY);
        assert_eq!(
            check_expr(&e2, &mut env).unwrap(),
            Ty::List(Box::new(Ty::Float))
        );
        // Three elements with a trailing int still widen to Float.
        let e3 = Expr::List(vec![int_lit(1), float_lit(2.0), int_lit(3)], Span::DUMMY);
        assert_eq!(
            check_expr(&e3, &mut env).unwrap(),
            Ty::List(Box::new(Ty::Float))
        );
    }

    #[test]
    fn error_set_int_float_rejected() {
        // Numeric widening is list-only: a set's element type must be hashable,
        // but `set[float]` (`HashSet<f64>`) is not representable in Rust, so
        // `{1, 2.0}` is rejected rather than typed Set(Float) (card 5c2f31d8).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Set(vec![int_lit(1), float_lit(2.0)], Span::DUMMY);
        assert!(matches!(check_expr(&e, &mut env), Err(Error::Type { .. })));
    }

    #[test]
    fn error_pure_float_set_rejected() {
        // A pure-float set literal `{1.0, 2.0}` folds to Set(Float), which
        // codegen would emit as the uncompilable `HashSet<f64>` (f64 is not
        // Eq/Hash). Reject it at typeck so typeck and codegen agree (card
        // 3c0243de). Distinct from the int/float mix above: every element is
        // Float, so the fold succeeds but the resulting element type is not
        // hashable.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Set(vec![float_lit(1.0), float_lit(2.0)], Span::DUMMY);
        let err = check_expr(&e, &mut env).unwrap_err();
        match err {
            Error::Type { msg, .. } => assert!(
                msg.contains("hashable"),
                "expected hashability message, got: {msg}"
            ),
            other => panic!("expected Error::Type, got {other:?}"),
        }
    }

    #[test]
    fn error_float_keyed_dict_rejected() {
        // `{1.0: "a"}` folds to Dict(Float, _) -> uncompilable `HashMap<f64, _>`.
        // Reject the float KEY at typeck (card 3c0243de).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Dict(vec![(float_lit(1.0), str_lit("a"))], Span::DUMMY);
        let err = check_expr(&e, &mut env).unwrap_err();
        match err {
            Error::Type { msg, .. } => assert!(
                msg.contains("hashable"),
                "expected hashability message, got: {msg}"
            ),
            other => panic!("expected Error::Type, got {other:?}"),
        }
    }

    #[test]
    fn ok_float_valued_dict_accepted() {
        // A float VALUE is fine: `{"a": 1.0}` -> Dict(Str, Float) ->
        // `HashMap<String, f64>` compiles. Only float KEYS are rejected.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Dict(vec![(str_lit("a"), float_lit(1.0))], Span::DUMMY);
        assert_eq!(
            check_expr(&e, &mut env).unwrap(),
            Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Float))
        );
    }

    #[test]
    fn error_declared_set_float_rejected() {
        // A declared `set[float]` annotation resolves to Set(Float), rejected at
        // the TypeExpr->Ty resolver so vars, params, and returns are covered
        // uniformly — even with an empty/`set()` initializer (card 3c0243de).
        let t = TypeExpr::Generic(
            "set".to_string(),
            vec![TypeExpr::Named("float".to_string())],
        );
        let err = Ty::from_type_expr(&t).unwrap_err();
        match err {
            Error::Type { msg, .. } => assert!(
                msg.contains("hashable"),
                "expected hashability message, got: {msg}"
            ),
            other => panic!("expected Error::Type, got {other:?}"),
        }
    }

    #[test]
    fn error_declared_dict_float_key_rejected() {
        // A declared `dict[float, str]` resolves to Dict(Float, Str), rejected
        // for the float KEY (card 3c0243de).
        let t = TypeExpr::Generic(
            "dict".to_string(),
            vec![
                TypeExpr::Named("float".to_string()),
                TypeExpr::Named("str".to_string()),
            ],
        );
        assert!(matches!(Ty::from_type_expr(&t), Err(Error::Type { .. })));
    }

    #[test]
    fn ok_declared_dict_float_value_accepted() {
        // `dict[str, float]` -> Dict(Str, Float) is fine (float VALUE).
        let t = TypeExpr::Generic(
            "dict".to_string(),
            vec![
                TypeExpr::Named("str".to_string()),
                TypeExpr::Named("float".to_string()),
            ],
        );
        assert_eq!(
            Ty::from_type_expr(&t).unwrap(),
            Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Float))
        );
    }

    #[test]
    fn infer_empty_dict_is_unknown_unknown() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Dict(vec![], Span::DUMMY);
        assert_eq!(
            check_expr(&e, &mut env).unwrap(),
            Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown))
        );
    }

    #[test]
    fn infer_dict_from_first_pair() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Dict(vec![(str_lit("k"), int_lit(1))], Span::DUMMY);
        assert_eq!(
            check_expr(&e, &mut env).unwrap(),
            Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int))
        );
    }

    #[test]
    fn error_dict_hetero_values() {
        // {"a": 1, "b": "x"} — values Int vs Str — must be rejected.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Dict(
            vec![
                (str_lit("a"), int_lit(1)),
                (str_lit("b"), str_lit("x")),
            ],
            Span::DUMMY,
        );
        assert!(matches!(check_expr(&e, &mut env), Err(Error::Type { .. })));
    }

    #[test]
    fn error_dict_hetero_keys() {
        // {1: "a", "two": "a"} — keys Int vs Str — must be rejected.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Dict(
            vec![
                (int_lit(1), str_lit("a")),
                (str_lit("two"), str_lit("a")),
            ],
            Span::DUMMY,
        );
        assert!(matches!(check_expr(&e, &mut env), Err(Error::Type { .. })));
    }

    #[test]
    fn infer_dict_homogeneous() {
        // {"a": 1, "b": 2, "c": 3} — 3-pair homogeneous dict — must fold to Dict(Str, Int).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Dict(
            vec![
                (str_lit("a"), int_lit(1)),
                (str_lit("b"), int_lit(2)),
                (str_lit("c"), int_lit(3)),
            ],
            Span::DUMMY,
        );
        assert_eq!(
            check_expr(&e, &mut env).unwrap(),
            Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int))
        );
    }

    #[test]
    fn infer_tuple_types_all_elems() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Tuple(vec![int_lit(1), str_lit("a"), bool_lit(true)], Span::DUMMY);
        assert_eq!(
            check_expr(&e, &mut env).unwrap(),
            Ty::Tuple(vec![Ty::Int, Ty::Str, Ty::Bool])
        );
    }

    #[test]
    fn infer_binop_add_int_int() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(int_lit(1)),
            rhs: Box::new(int_lit(2)),
            span: Span::DUMMY,
        };
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Int);
    }

    #[test]
    fn infer_binop_div_always_float() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::BinOp {
            op: BinOp::Div,
            lhs: Box::new(int_lit(4)),
            rhs: Box::new(int_lit(2)),
            span: Span::DUMMY,
        };
        // Division always returns Float in Python.
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Float);
    }

    #[test]
    fn infer_binop_eq_returns_bool() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::BinOp {
            op: BinOp::Eq,
            lhs: Box::new(int_lit(1)),
            rhs: Box::new(int_lit(1)),
            span: Span::DUMMY,
        };
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Bool);
    }

    #[test]
    fn infer_unop_not_returns_bool() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::UnOp {
            op: UnOp::Not,
            expr: Box::new(bool_lit(false)),
            span: Span::DUMMY,
        };
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Bool);
    }

    #[test]
    fn infer_unop_neg_preserves_type() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::UnOp {
            op: UnOp::Neg,
            expr: Box::new(int_lit(5)),
            span: Span::DUMMY,
        };
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Int);
    }

    #[test]
    fn infer_range_returns_list_int() {
        // range is registered in TyCtx::new() with ret = List(Int).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = call_fn("range", vec![int_lit(10)]);
        assert_eq!(
            check_expr(&e, &mut env).unwrap(),
            Ty::List(Box::new(Ty::Int))
        );
    }

    #[test]
    fn infer_min_one_arg_list_int() {
        // min([...]) with 1 arg → element type of the list.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let list_expr = Expr::List(vec![int_lit(3), int_lit(1)], Span::DUMMY);
        let e = call_fn("min", vec![list_expr]);
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Int);
    }

    #[test]
    fn infer_max_one_arg_set_str() {
        // max(set[str]) with 1 arg → Str (element type of the set).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let set_expr = Expr::Set(vec![str_lit("a"), str_lit("b")], Span::DUMMY);
        let e = call_fn("max", vec![set_expr]);
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Str);
    }

    #[test]
    fn infer_min_two_args_is_unknown_bug3() {
        // BUG 3 (design choice): 2-arg min/max falls through to the generic path.
        // ctx.funcs["min"] has ret=Unknown, so the result is Unknown.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = call_fn("min", vec![int_lit(1), int_lit(2)]);
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Unknown);
    }

    #[test]
    fn infer_ident_after_assign_stmt() {
        // After `x = 5` the env knows x: Int.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let stmt = Stmt::Assign {
            target: "x".into(),
            ty: None,
            value: int_lit(5),
            span: Span::DUMMY,
        };
        check_stmt(&stmt, &mut env).unwrap();
        assert_eq!(
            check_expr(&ident("x"), &mut env).unwrap(),
            Ty::Int
        );
    }

    #[test]
    fn infer_for_loop_binds_elem_type() {
        // for x in [1,2]: env["x"] = Int.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let iter = Expr::List(vec![int_lit(1), int_lit(2)], Span::DUMMY);
        let stmt = Stmt::For {
            targets: vec!["x".into()],
            iter,
            body: vec![],
            span: Span::DUMMY,
        };
        check_stmt(&stmt, &mut env).unwrap();
        assert_eq!(env.locals.get("x").cloned(), Some(Ty::Int));
    }

    #[test]
    fn infer_for_loop_over_str_yields_str() {
        // for c in "hello": env["c"] = Str.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let stmt = Stmt::For {
            targets: vec!["c".into()],
            iter: str_lit("hello"),
            body: vec![],
            span: Span::DUMMY,
        };
        check_stmt(&stmt, &mut env).unwrap();
        assert_eq!(env.locals.get("c").cloned(), Some(Ty::Str));
    }

    #[test]
    fn infer_unpack_tuple() {
        // a, b = (1, "hello") → a: Int, b: Str.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let value = Expr::Tuple(vec![int_lit(1), str_lit("hello")], Span::DUMMY);
        let stmt = Stmt::Unpack {
            targets: vec!["a".into(), "b".into()],
            value,
            span: Span::DUMMY,
        };
        check_stmt(&stmt, &mut env).unwrap();
        assert_eq!(env.locals.get("a").cloned(), Some(Ty::Int));
        assert_eq!(env.locals.get("b").cloned(), Some(Ty::Str));
    }

    #[test]
    fn infer_index_list() {
        // xs[0] where xs: list[int] → Int.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Int)));
        let e = Expr::Index {
            obj: Box::new(ident("xs")),
            idx: Box::new(int_lit(0)),
            span: Span::DUMMY,
        };
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Int);
    }

    #[test]
    fn infer_index_dict_returns_val_type() {
        // d["k"] where d: dict[str,bool] → Bool.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert(
            "d".into(),
            Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Bool)),
        );
        let e = Expr::Index {
            obj: Box::new(ident("d")),
            idx: Box::new(str_lit("k")),
            span: Span::DUMMY,
        };
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Bool);
    }

    #[test]
    fn infer_str_method_call_upper() {
        // "hi".upper() → Str.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = method_call(str_lit("hi"), "upper", vec![]);
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Str);
    }

    #[test]
    fn infer_list_method_pop() {
        // xs.pop() where xs: list[float] → Float.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Float)));
        let e = method_call(ident("xs"), "pop", vec![]);
        assert_eq!(check_expr(&e, &mut env).unwrap(), Ty::Float);
    }

    #[test]
    fn infer_return_unit_in_unit_fn() {
        // bare return in unit-returning fn → ok.
        let ctx = TyCtx::new();
        let mut env = make_env_ret(&ctx, Ty::Unit);
        let stmt = Stmt::Return(None, Span::DUMMY);
        assert!(check_stmt(&stmt, &mut env).is_ok());
    }

    #[test]
    fn infer_return_int_in_int_fn() {
        // return 42 in Int-returning fn → ok.
        let ctx = TyCtx::new();
        let mut env = make_env_ret(&ctx, Ty::Int);
        let stmt = Stmt::Return(Some(int_lit(42)), Span::DUMMY);
        assert!(check_stmt(&stmt, &mut env).is_ok());
    }

    #[test]
    fn infer_assign_typed_ok() {
        // x: int = 5 → ok, x: Int in locals.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let stmt = Stmt::Assign {
            target: "x".into(),
            ty: Some(TypeExpr::Named("int".into())),
            value: int_lit(5),
            span: Span::DUMMY,
        };
        assert!(check_stmt(&stmt, &mut env).is_ok());
        assert_eq!(env.locals.get("x").cloned(), Some(Ty::Int));
    }

    #[test]
    fn infer_empty_set_is_unknown() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = Expr::Set(vec![], Span::DUMMY);
        assert_eq!(
            check_expr(&e, &mut env).unwrap(),
            Ty::Set(Box::new(Ty::Unknown))
        );
    }

    // =========================================================================
    // Category D — error-firing
    // =========================================================================

    #[test]
    fn error_undefined_name() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let r = check_expr(&ident("no_such_var"), &mut env);
        assert_type_err(r, "undefined name");
    }

    #[test]
    fn error_undefined_function() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let r = check_expr(&call_fn("no_such_fn", vec![]), &mut env);
        assert_type_err(r, "undefined function");
    }

    #[test]
    fn error_return_type_mismatch() {
        let ctx = TyCtx::new();
        let mut env = make_env_ret(&ctx, Ty::Int);
        // Returning a Str from an Int-returning function.
        let stmt = Stmt::Return(Some(str_lit("oops")), Span::DUMMY);
        assert_stmt_type_err(check_stmt(&stmt, &mut env), "return type mismatch");
    }

    #[test]
    fn error_bare_return_in_typed_fn() {
        let ctx = TyCtx::new();
        let mut env = make_env_ret(&ctx, Ty::Int);
        let stmt = Stmt::Return(None, Span::DUMMY);
        assert_stmt_type_err(check_stmt(&stmt, &mut env), "bare return");
    }

    #[test]
    fn error_assign_type_mismatch() {
        // x: int = "wrong" → type mismatch in assignment.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let stmt = Stmt::Assign {
            target: "x".into(),
            ty: Some(TypeExpr::Named("int".into())),
            value: str_lit("wrong"),
            span: Span::DUMMY,
        };
        assert_stmt_type_err(check_stmt(&stmt, &mut env), "type mismatch");
    }

    #[test]
    fn error_augassign_undefined_var() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let stmt = Stmt::AugAssign {
            target: "missing".into(),
            op: BinOp::Add,
            value: int_lit(1),
            span: Span::DUMMY,
        };
        assert_stmt_type_err(check_stmt(&stmt, &mut env), "undefined variable");
    }

    #[test]
    fn no_error_augassign_when_var_exists() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("x".into(), Ty::Int);
        let stmt = Stmt::AugAssign {
            target: "x".into(),
            op: BinOp::Add,
            value: int_lit(1),
            span: Span::DUMMY,
        };
        assert!(check_stmt(&stmt, &mut env).is_ok());
    }

    #[test]
    fn error_unknown_method_on_str() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = method_call(str_lit("hello"), "no_such_method", vec![]);
        assert_type_err(check_expr(&e, &mut env), "has no method");
    }

    #[test]
    fn error_unknown_method_on_list() {
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Int)));
        let e = method_call(ident("xs"), "nonexistent", vec![]);
        assert_type_err(check_expr(&e, &mut env), "has no method");
    }

    #[test]
    fn error_arity_mismatch_too_many() {
        // Register a 1-param function, call it with 2 args.
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("myfn".into(), FuncSig {
            params: vec![("x".into(), Ty::Int)],
            param_defaults: vec![None],
            param_by_ref: vec![],
            ret: Ty::Int,
        });
        let mut env = make_env(&ctx);
        let r = check_expr(&call_fn("myfn", vec![int_lit(1), int_lit(2)]), &mut env);
        assert_type_err(r, "argument(s)");
    }

    #[test]
    fn error_arity_mismatch_too_few() {
        // Register a 2-param function (both required), call it with 0 args.
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("twoarg".into(), FuncSig {
            params: vec![("a".into(), Ty::Int), ("b".into(), Ty::Str)],
            param_defaults: vec![None, None],
            param_by_ref: vec![],
            ret: Ty::Bool,
        });
        let mut env = make_env(&ctx);
        let r = check_expr(&call_fn("twoarg", vec![]), &mut env);
        assert_type_err(r, "argument(s)");
    }

    #[test]
    fn error_arg_type_mismatch() {
        // Register a function taking Int; pass Str.
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("takes_int".into(), FuncSig {
            params: vec![("n".into(), Ty::Int)],
            param_defaults: vec![None],
            param_by_ref: vec![],
            ret: Ty::Unit,
        });
        let mut env = make_env(&ctx);
        let r = check_expr(&call_fn("takes_int", vec![str_lit("oops")]), &mut env);
        assert_type_err(r, "argument 1 to");
    }

    #[test]
    fn error_set_add_wrong_elem_type() {
        // s.add("x") where s: set[int] → element type mismatch.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("s".into(), Ty::Set(Box::new(Ty::Int)));
        let e = method_call(ident("s"), "add", vec![str_lit("oops")]);
        assert_type_err(check_expr(&e, &mut env), "expected element type");
    }

    #[test]
    fn no_error_int_to_float_param() {
        // Int passed to a Float param → allowed (Python numeric coercion).
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("takes_float".into(), FuncSig {
            params: vec![("f".into(), Ty::Float)],
            param_defaults: vec![None],
            param_by_ref: vec![],
            ret: Ty::Unit,
        });
        let mut env = make_env(&ctx);
        let r = check_expr(&call_fn("takes_float", vec![int_lit(3)]), &mut env);
        assert!(r.is_ok(), "Int→Float coercion should be allowed, got {:?}", r);
    }

    // =========================================================================
    // Category C — enumerate/zip inference (card 7ccffd5a)
    // =========================================================================

    #[test]
    fn infer_enumerate_list_str() {
        // enumerate(xs: list[str]) -> List(Tuple(Int, Str))
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Str)));
        let call = call_fn("enumerate", vec![ident("xs")]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(
            ty,
            Ty::List(Box::new(Ty::Tuple(vec![Ty::Int, Ty::Str])))
        );
    }

    #[test]
    fn infer_enumerate_list_int() {
        // enumerate(ys: list[int]) -> List(Tuple(Int, Int))
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("ys".into(), Ty::List(Box::new(Ty::Int)));
        let call = call_fn("enumerate", vec![ident("ys")]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(
            ty,
            Ty::List(Box::new(Ty::Tuple(vec![Ty::Int, Ty::Int])))
        );
    }

    #[test]
    fn infer_enumerate_str_iterable() {
        // enumerate("hello") -> List(Tuple(Int, Str))
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let call = call_fn("enumerate", vec![str_lit("hello")]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(
            ty,
            Ty::List(Box::new(Ty::Tuple(vec![Ty::Int, Ty::Str])))
        );
    }

    #[test]
    fn infer_enumerate_unknown_arg_stays_unknown() {
        // enumerate(42) — non-iterable arg → Unknown (stay permissive).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let call = call_fn("enumerate", vec![int_lit(42)]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::Unknown);
    }

    #[test]
    fn infer_zip_two_lists() {
        // zip(xs: list[str], ys: list[int]) -> List(Tuple(Str, Int))
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Str)));
        env.locals.insert("ys".into(), Ty::List(Box::new(Ty::Int)));
        let call = call_fn("zip", vec![ident("xs"), ident("ys")]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(
            ty,
            Ty::List(Box::new(Ty::Tuple(vec![Ty::Str, Ty::Int])))
        );
    }

    #[test]
    fn infer_zip_unknown_arg_stays_unknown() {
        // zip(xs: list[str], 42) — non-iterable arg → Unknown.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Str)));
        let call = call_fn("zip", vec![ident("xs"), int_lit(42)]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::Unknown);
    }

    #[test]
    fn infer_for_enumerate_binds_int_and_elem() {
        // for i, x in enumerate(xs: list[str]): → i: Int, x: Str
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Str)));
        let iter = call_fn("enumerate", vec![ident("xs")]);
        let stmt = Stmt::For {
            targets: vec!["i".into(), "x".into()],
            iter,
            body: vec![],
            span: Span::DUMMY,
        };
        check_stmt(&stmt, &mut env).unwrap();
        assert_eq!(env.locals.get("i").cloned(), Some(Ty::Int));
        assert_eq!(env.locals.get("x").cloned(), Some(Ty::Str));
    }

    // =========================================================================
    // Category C2 — lambda / map / filter return-type inference (card 21424502)
    // =========================================================================

    /// Single-param lambda `lambda <param>: <body>` (param is untyped, as the
    /// parser emits — `TypeExpr::Named("Any")`).
    fn lambda1(param: &str, body: Expr) -> Expr {
        Expr::Lambda {
            params: vec![(param.to_string(), TypeExpr::Named("Any".into()))],
            body: Box::new(body),
            span: Span::DUMMY,
        }
    }

    /// `lhs <op> rhs` binary op.
    fn binop(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span: Span::DUMMY }
    }

    #[test]
    fn infer_lambda_body_return_type_identity() {
        // (lambda x: x)(5) — the Lambda arm now returns the body type; with x
        // bound to the call arg's path it would be Int. Here we check the inline
        // call: the param is untyped (Unknown) so an identity lambda yields the
        // body's resolved type, which for a bare untyped param is Unknown — but
        // a literal body resolves concretely.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        // (lambda x: 5)(99) — body is a literal Int, independent of the param.
        let lam = lambda1("x", int_lit(5));
        let call = Expr::Call {
            callee: Box::new(lam),
            args: vec![int_lit(99)],
            kwargs: vec![],
            span: Span::DUMMY,
        };
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::Int);
    }

    #[test]
    fn infer_lambda_body_str_literal() {
        // (lambda x: "hi")(0) -> Str (body type propagates instead of Unknown).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let lam = lambda1("x", str_lit("hi"));
        let call = Expr::Call {
            callee: Box::new(lam),
            args: vec![int_lit(0)],
            kwargs: vec![],
            span: Span::DUMMY,
        };
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::Str);
    }

    #[test]
    fn infer_map_over_list_int_returns_list_int() {
        // map(lambda x: x + 1, xs: list[int]) -> List(Int)
        // The element type Int is bound to the lambda param, so `x + 1` resolves
        // to Int and the result is List(Int).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Int)));
        let lam = lambda1("x", binop(BinOp::Add, ident("x"), int_lit(1)));
        let call = call_fn("map", vec![lam, ident("xs")]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::List(Box::new(Ty::Int)));
    }

    #[test]
    fn infer_map_over_str_is_unknown() {
        // map over a non-list iterable (here a str) stays Unknown: codegen can't
        // compile `.iter()` over a String, so typeck must not assert a concrete
        // List type. Scoped to List iterables only, matching the filter arm.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let lam = lambda1("c", ident("c"));
        let call = call_fn("map", vec![lam, str_lit("hello")]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::Unknown);
    }

    #[test]
    fn infer_map_str_body_returns_list_str() {
        // map(lambda x: str(x), xs: list[int]) -> List(Str) — the body type
        // (str()'s return) drives the result element type, not the input.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Int)));
        let lam = lambda1("x", call_fn("str", vec![ident("x")]));
        let call = call_fn("map", vec![lam, ident("xs")]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::List(Box::new(Ty::Str)));
    }

    #[test]
    fn infer_filter_over_list_int_returns_list_int() {
        // filter(lambda x: x % 2 == 0, xs: list[int]) -> List(Int)
        // filter preserves the element type regardless of the predicate body.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Int)));
        let pred = lambda1(
            "x",
            binop(BinOp::Eq, binop(BinOp::Mod, ident("x"), int_lit(2)), int_lit(0)),
        );
        let call = call_fn("filter", vec![pred, ident("xs")]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::List(Box::new(Ty::Int)));
    }

    #[test]
    fn infer_filter_over_list_str_returns_list_str() {
        // filter(pred, xs: list[str]) -> List(Str) (element type preserved).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Str)));
        let pred = lambda1("x", bool_lit(true));
        let call = call_fn("filter", vec![pred, ident("xs")]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::List(Box::new(Ty::Str)));
    }

    #[test]
    fn infer_map_unknown_iterable_stays_unknown() {
        // map(lambda x: x + 1, 42) — non-list iterable → Unknown (permissive),
        // never narrowing types_compatible.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let lam = lambda1("x", binop(BinOp::Add, ident("x"), int_lit(1)));
        let call = call_fn("map", vec![lam, int_lit(42)]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::Unknown);
    }

    #[test]
    fn infer_filter_unknown_iterable_stays_unknown() {
        // filter(pred, 42) — non-list iterable → Unknown.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let pred = lambda1("x", bool_lit(true));
        let call = call_fn("filter", vec![pred, int_lit(42)]);
        let ty = check_expr(&call, &mut env).unwrap();
        assert_eq!(ty, Ty::Unknown);
    }

    #[test]
    fn error_map_wrong_declared_type() {
        // result: list[int] = map(lambda x: str(x), xs: list[int])
        // map yields List(Str); the list[int] annotation must be rejected.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Int)));
        let lam = lambda1("x", call_fn("str", vec![ident("x")]));
        let call = call_fn("map", vec![lam, ident("xs")]);
        let stmt = Stmt::Assign {
            target: "result".into(),
            ty: Some(TypeExpr::Generic("list".into(), vec![TypeExpr::Named("int".into())])),
            value: call,
            span: Span::DUMMY,
        };
        assert_stmt_type_err(check_stmt(&stmt, &mut env), "type mismatch");
    }

    // =========================================================================
    // Category D — enumerate/zip error cases (card 7ccffd5a)
    // =========================================================================

    #[test]
    fn error_enumerate_index_passed_as_str() {
        // fn takes_str(s: str) -> None; for i, x in enumerate(xs: list[str]): takes_str(i)
        // i is Int; passing it to takes_str should be a type error.
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("takes_str".into(), FuncSig {
            params: vec![("s".into(), Ty::Str)],
            param_defaults: vec![None],
            param_by_ref: vec![],
            ret: Ty::Unit,
        });
        let mut env = make_env(&ctx);
        env.locals.insert("xs".into(), Ty::List(Box::new(Ty::Str)));

        // First bind i:Int, x:Str via the for loop.
        let iter = call_fn("enumerate", vec![ident("xs")]);
        let for_stmt = Stmt::For {
            targets: vec!["i".into(), "x".into()],
            iter,
            body: vec![],
            span: Span::DUMMY,
        };
        check_stmt(&for_stmt, &mut env).unwrap();
        assert_eq!(env.locals.get("i").cloned(), Some(Ty::Int));

        // Now call takes_str(i) — i is Int, param expects Str → error.
        let call = call_fn("takes_str", vec![ident("i")]);
        let r = check_expr(&call, &mut env);
        assert_type_err(r, "expected");
    }

    // -------------------------------------------------------------------------
    // E. Drift guard — removed unemittable methods must stay absent (card 36f66dd2)
    // -------------------------------------------------------------------------

    /// Ensure that the str/dict methods codegen cannot emit are permanently
    /// absent from STR_METHODS / DICT_METHODS.  If a future implementer adds
    /// them back here without wiring codegen they will hit this test first.
    #[test]
    fn removed_unemittable_methods_absent_from_str_table() {
        let unemittable = ["casefold", "encode", "isdecimal", "rsplit", "format"];
        for m in &unemittable {
            assert!(
                !STR_METHODS.contains(m),
                "STR_METHODS contains `{m}` but codegen cannot emit it \
                 (card 36f66dd2 drift guard)"
            );
        }
    }

    #[test]
    fn removed_unemittable_methods_absent_from_dict_table() {
        let unemittable = ["setdefault", "popitem"];
        for m in &unemittable {
            assert!(
                !DICT_METHODS.contains(m),
                "DICT_METHODS contains `{m}` but codegen cannot emit it \
                 (card 36f66dd2 drift guard)"
            );
        }
    }

    /// Confirm that `builtin_method_ret` returns Unknown (not a concrete type)
    /// for every method removed from the acceptance tables — the method-existence
    /// check runs before builtin_method_ret, so Unknown is the right sentinel.
    #[test]
    fn removed_str_methods_return_unknown_from_builtin_method_ret() {
        let unemittable = ["casefold", "encode", "isdecimal", "rsplit", "format"];
        for m in &unemittable {
            assert_eq!(
                builtin_method_ret(&Ty::Str, m),
                Ty::Unknown,
                "builtin_method_ret returned a concrete type for removed str method `{m}` \
                 (card 36f66dd2 drift guard)"
            );
        }
    }

    #[test]
    fn removed_dict_methods_return_unknown_from_builtin_method_ret() {
        let unemittable = ["setdefault", "popitem"];
        for m in &unemittable {
            assert_eq!(
                builtin_method_ret(&Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int)), m),
                Ty::Unknown,
                "builtin_method_ret returned a concrete type for removed dict method `{m}` \
                 (card 36f66dd2 drift guard)"
            );
        }
    }

    // -------------------------------------------------------------------------
    // EPIC-4 V1-a: the single shared copy-ness predicate (`is_copy`/`is_owned`).
    // Pins the defined rule, including the intentional Tuple/Option refinement
    // and the conservative non-Copy treatment of NoneVal/File/Unknown.
    // -------------------------------------------------------------------------

    #[test]
    fn is_copy_scalars_are_copy() {
        for t in [Ty::Int, Ty::Float, Ty::Bool, Ty::Unit] {
            assert!(is_copy(&t), "{t:?} must be Copy");
            assert!(!is_owned(&t), "{t:?} must not be owned");
        }
    }

    #[test]
    fn is_copy_collections_and_class_are_non_copy() {
        let cases = [
            Ty::Str,
            Ty::List(Box::new(Ty::Int)),
            Ty::Set(Box::new(Ty::Int)),
            Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int)),
            Ty::Class("Point".into()),
        ];
        for t in cases {
            assert!(!is_copy(&t), "{t:?} must be non-Copy");
            assert!(is_owned(&t), "{t:?} must be owned");
        }
    }

    #[test]
    fn is_copy_conservative_non_copy_variants() {
        // Matches the legacy `is_copy_type`, which excluded these (=> non-Copy).
        for t in [Ty::NoneVal, Ty::File, Ty::Unknown] {
            assert!(!is_copy(&t), "{t:?} must be conservatively non-Copy");
        }
    }

    #[test]
    fn is_copy_tuple_is_elementwise() {
        // All-Copy elements => Copy (the V1-a refinement: tuple-of-ints no longer cloned).
        assert!(is_copy(&Ty::Tuple(vec![Ty::Int, Ty::Int])));
        assert!(is_copy(&Ty::Tuple(vec![Ty::Int, Ty::Float, Ty::Bool])));
        // The empty tuple () is trivially Copy.
        assert!(is_copy(&Ty::Tuple(vec![])));
        // Any non-Copy element makes the whole tuple non-Copy.
        assert!(!is_copy(&Ty::Tuple(vec![Ty::Int, Ty::Str])));
        assert!(!is_copy(&Ty::Tuple(vec![Ty::List(Box::new(Ty::Int))])));
        // Nested all-Copy tuple stays Copy.
        assert!(is_copy(&Ty::Tuple(vec![Ty::Tuple(vec![Ty::Int, Ty::Int]), Ty::Bool])));
    }

    #[test]
    fn is_copy_option_follows_inner() {
        // Option<Copy> is Copy (the V1-a refinement: Optional[int] no longer cloned).
        assert!(is_copy(&Ty::Option(Box::new(Ty::Int))));
        assert!(is_copy(&Ty::Option(Box::new(Ty::Tuple(vec![Ty::Int, Ty::Bool])))));
        // Option<non-Copy> is non-Copy.
        assert!(!is_copy(&Ty::Option(Box::new(Ty::Str))));
        assert!(!is_copy(&Ty::Option(Box::new(Ty::Class("Point".into())))));
    }

    // =========================================================================
    // Category — EPIC-4 V2: Mut[T] by-reference param mode (front-end)
    // =========================================================================

    /// Register a single-param function whose one param is by-reference.
    fn ctx_with_byref_fn(name: &str, param: &str, ty: Ty) -> TyCtx {
        let mut ctx = TyCtx::new();
        ctx.funcs.insert(name.into(), FuncSig {
            params: vec![(param.into(), ty)],
            param_defaults: vec![None],
            param_by_ref: vec![true],
            ret: Ty::Unit,
        });
        ctx
    }

    #[test]
    fn byref_arg_temporary_is_rejected() {
        // A by-ref param given a TEMPORARY (here an int literal) is an honest
        // typeck error — you cannot borrow `&mut` of a value with no storage.
        let ctx = ctx_with_byref_fn("touch", "slot", Ty::Int);
        let mut env = make_env(&ctx);
        let r = check_expr(&call_fn("touch", vec![int_lit(7)]), &mut env);
        assert_type_err(r, "by-reference parameter `slot` requires a variable, not a temporary");
    }

    #[test]
    fn byref_arg_constructor_temporary_is_rejected() {
        // A constructor/call result is equally a temporary, not a place.
        let ctx = ctx_with_byref_fn("touch", "slot", Ty::Int);
        let mut env = make_env(&ctx);
        // `helper()` returns Unknown; the place-check fires BEFORE arg-type
        // compatibility, so the diagnostic is the by-reference one.
        let temp = call_fn("helper", vec![]);
        // Register `helper` so the inner call resolves (it returns Unknown).
        let mut ctx2 = ctx;
        ctx2.funcs.insert("helper".into(), FuncSig {
            params: vec![], param_defaults: vec![], param_by_ref: vec![], ret: Ty::Int,
        });
        let mut env2 = make_env(&ctx2);
        let r = check_expr(&call_fn("touch", vec![temp]), &mut env2);
        assert_type_err(r, "requires a variable, not a temporary");
    }

    #[test]
    fn byref_arg_place_ident_is_accepted() {
        // A bound variable (a place) satisfies the by-ref requirement.
        let ctx = ctx_with_byref_fn("touch", "slot", Ty::Int);
        let mut env = make_env(&ctx);
        env.locals.insert("n".into(), Ty::Int);
        let r = check_expr(&call_fn("touch", vec![ident("n")]), &mut env);
        assert!(r.is_ok(), "a variable place should satisfy a by-ref param, got {:?}", r);
    }

    #[test]
    fn mut_type_rejected_in_non_param_position() {
        // `Mut[T]` is never a real type: from_type_expr (the lowering boundary for
        // every NON-param annotation — return types, field/variable annotations,
        // and nested forms) rejects it with the directed message.
        let me = TypeExpr::Generic("Mut".into(), vec![TypeExpr::Named("int".into())]);
        let r = Ty::from_type_expr(&me);
        match r {
            Err(Error::Type { msg, .. }) =>
                assert!(msg.contains("Mut[...] is only valid on a parameter"), "got: {}", msg),
            other => panic!("expected Mut rejection, got {:?}", other),
        }
        // Nested inside another generic is rejected the same way.
        let nested = TypeExpr::Generic("list".into(), vec![me]);
        assert!(Ty::from_type_expr(&nested).is_err(), "list[Mut[T]] must be rejected");
    }

    #[test]
    fn backstop_message_mentions_mut_remedy() {
        // The by-value-param-mutation backstop now offers the `Mut[T]` on-ramp in
        // addition to the existing return-the-value idiom.
        let msg = by_value_mutation_error("acc");
        assert!(msg.contains("mutate via a method on it or return the updated value"),
            "must keep the original remedy: {}", msg);
        assert!(msg.contains("declare the parameter `Mut[T]` to mutate it in place"),
            "must add the Mut[T] on-ramp: {}", msg);
    }

    #[test]
    fn nested_index_mutate_on_by_value_param_fires() {
        // EPIC-4 V2-d: a mutating method on an INDEX of a by-value non-Copy param
        // — `rows[0].append(x)` where `rows: list[list[int]]` — now fires the
        // backstop. Before V2-d the receiver `rows[0]` (an `Expr::Index`, not the
        // bare param ident) escaped silently. The fix roots the receiver via
        // `root_ident`, recovering `rows` as the mutated by-value param.
        let ctx = TyCtx::new();
        let mut env = FuncEnv::with_by_ref(
            &ctx,
            &[("rows".into(), Ty::List(Box::new(Ty::List(Box::new(Ty::Int)))))],
            &[],
            Ty::Unit,
        );
        // rows[0].append(7)
        let receiver = Expr::Index {
            obj: Box::new(ident("rows")),
            idx: Box::new(int_lit(0)),
            span: Span::DUMMY,
        };
        let call = method_call(receiver, "append", vec![int_lit(7)]);
        let r = check_expr(&call, &mut env);
        assert_type_err(r, "mutation of by-value parameter `rows` is not visible");
        // And it points the user at the Mut[T] remedy.
        if let Err(Error::Type { msg, .. }) =
            check_expr(&{
                let receiver = Expr::Index {
                    obj: Box::new(ident("rows")),
                    idx: Box::new(int_lit(0)),
                    span: Span::DUMMY,
                };
                method_call(receiver, "append", vec![int_lit(7)])
            }, &mut env)
        {
            assert!(msg.contains("declare the parameter `Mut[T]`"),
                "nested-mutation error must offer the Mut[T] remedy: {}", msg);
        }
    }

    #[test]
    fn nested_index_mutate_on_by_ref_param_is_suppressed() {
        // EPIC-4 V2-d suppression: when the SAME nested shape roots at a `Mut[T]`
        // (by-reference) param, the mutation IS visible to the caller, so the
        // backstop must NOT fire. Closing the gap must not weaken this.
        let ctx = TyCtx::new();
        let mut env = FuncEnv::with_by_ref(
            &ctx,
            &[("rows".into(), Ty::List(Box::new(Ty::List(Box::new(Ty::Int)))))],
            &["rows".into()], // declared Mut[list[list[int]]]
            Ty::Unit,
        );
        let receiver = Expr::Index {
            obj: Box::new(ident("rows")),
            idx: Box::new(int_lit(0)),
            span: Span::DUMMY,
        };
        let call = method_call(receiver, "append", vec![int_lit(7)]);
        let r = check_expr(&call, &mut env);
        assert!(r.is_ok(),
            "a Mut[T] param's nested mutation must NOT fire the backstop, got {:?}", r);
    }

    #[test]
    fn nested_index_mutate_on_local_does_not_fire() {
        // Guard against over-firing: the same shape on a LOCAL (non-param) place
        // must never fire — only by-value PARAMS are caller-invisible.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert(
            "rows".into(),
            Ty::List(Box::new(Ty::List(Box::new(Ty::Int)))),
        );
        let receiver = Expr::Index {
            obj: Box::new(ident("rows")),
            idx: Box::new(int_lit(0)),
            span: Span::DUMMY,
        };
        let call = method_call(receiver, "append", vec![int_lit(7)]);
        let r = check_expr(&call, &mut env);
        assert!(r.is_ok(),
            "mutating a local (not a param) must not fire the backstop, got {:?}", r);
    }

    #[test]
    fn parser_unwraps_mut_param_to_by_ref_flag() {
        // `Mut[Account]` on a parameter raises by_ref and the param's annotation
        // is unwrapped to the inner `Account` (Mut never survives as a type).
        let src = "def f(a: Mut[int], b: str) -> None:\n    pass\n";
        let m = crate::parser::parse(src).expect("parse");
        let func = m.stmts.iter().find_map(|s| match s {
            Stmt::Func(f) => Some(f),
            _ => None,
        }).expect("func");
        let a = &func.params[0];
        assert!(a.by_ref, "Mut[int] param must set by_ref");
        assert_eq!(a.ty, TypeExpr::Named("int".into()), "Mut must be unwrapped to inner");
        let b = &func.params[1];
        assert!(!b.by_ref, "a plain param must not be by_ref");
        assert_eq!(b.ty, TypeExpr::Named("str".into()));
    }

    #[test]
    fn byref_param_mutation_typechecks() {
        // End-to-end (parse + check_bodies): a by-ref param that IS mutated must
        // typeck-PASS — the backstop is skipped because the mutation is now
        // legitimately caller-visible. NOTE this is a CHECK-only assertion; it is
        // deliberately NOT a build/run golden because codegen does not yet emit
        // `&mut` (V2-c), so a built binary would silently drop the mutation.
        let src = "\
class Account:
    balance: int
    def __init__(self, balance: int) -> None:
        self.balance = balance

def deposit(account: Mut[Account], amt: int) -> None:
    account.balance = account.balance + amt
";
        let m = crate::parser::parse(src).expect("parse");
        // Build a context the way the resolver does for a single module.
        let mut ctx = TyCtx::new();
        for s in &m.stmts {
            if let Stmt::Class(c) = s {
                let mut c = c.clone();
                extract_init_fields(&mut c);
                ctx.classes.insert(c.name.clone(), c);
            }
        }
        for s in &m.stmts {
            if let Stmt::Func(f) = s {
                ctx.funcs.insert(f.name.clone(), FuncSig {
                    params: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| (p.name.clone(), Ty::from_type_expr(&p.ty).unwrap()))
                        .collect(),
                    param_defaults: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| p.default.clone()).collect(),
                    param_by_ref: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| p.by_ref).collect(),
                    ret: Ty::from_type_expr(&f.ret).unwrap(),
                });
            }
        }
        assert!(check_bodies(&m, &ctx).is_ok(),
            "a mutated by-ref param must typeck-pass (backstop skipped)");
    }

    /// Build a TyCtx from a module exactly as the single-module resolver path
    /// does (classes extracted, free funcs + methods registered self-exclusive).
    /// Used by the V2-c end-to-end check_bodies tests below.
    fn ctx_from_module(m: &Module) -> TyCtx {
        let mut ctx = TyCtx::new();
        for s in &m.stmts {
            if let Stmt::Class(c) = s {
                let mut c = c.clone();
                extract_init_fields(&mut c);
                ctx.classes.insert(c.name.clone(), c.clone());
                for mf in &c.methods {
                    let key = format!("{}.{}", c.name, mf.name);
                    ctx.funcs.insert(key, FuncSig {
                        params: mf.params.iter().filter(|p| p.name != "self")
                            .map(|p| (p.name.clone(), Ty::from_type_expr(&p.ty).unwrap_or(Ty::Unknown)))
                            .collect(),
                        param_defaults: mf.params.iter().filter(|p| p.name != "self")
                            .map(|p| p.default.clone()).collect(),
                        param_by_ref: mf.params.iter().filter(|p| p.name != "self")
                            .map(|p| p.by_ref).collect(),
                        ret: Ty::from_type_expr(&mf.ret).unwrap_or(Ty::Unknown),
                    });
                }
            }
        }
        for s in &m.stmts {
            if let Stmt::Func(f) = s {
                ctx.funcs.insert(f.name.clone(), FuncSig {
                    params: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| (p.name.clone(), Ty::from_type_expr(&p.ty).unwrap_or(Ty::Unknown)))
                        .collect(),
                    param_defaults: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| p.default.clone()).collect(),
                    param_by_ref: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| p.by_ref).collect(),
                    ret: Ty::from_type_expr(&f.ret).unwrap_or(Ty::Unknown),
                });
            }
        }
        ctx
    }

    #[test]
    fn mut_on_constructor_param_is_rejected() {
        // EPIC-4 V2-c: `Mut[T]` on an __init__ parameter is unsupported (the
        // generated new() wrapper passes owned values into __init__). Rejected at
        // check time so both `check` and `build` catch it.
        let src = "\
class Account:
    balance: int
    def __init__(self, balance: int) -> None:
        self.balance = balance

class Vault:
    held: Account
    def __init__(self, acct: Mut[Account]) -> None:
        self.held = acct
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let r = check_bodies(&m, &ctx);
        assert!(r.is_err(), "Mut[T] on a constructor param must be rejected");
        let msg = format!("{:?}", r.unwrap_err());
        assert!(msg.contains("constructor") && msg.contains("__init__"),
            "error must name the constructor: {msg}");
    }

    #[test]
    fn mut_on_class_field_is_rejected_at_check() {
        // EPIC-4 V2-c: a class-FIELD annotated `Mut[T]` is rejected at CHECK time
        // (fields are now from_type_expr'd in check_bodies), not deferred to build.
        let src = "\
class Holder:
    value: Mut[int]
    def __init__(self, value: int) -> None:
        self.value = value
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let r = check_bodies(&m, &ctx);
        assert!(r.is_err(), "Mut[T] class field must be rejected at check");
        let msg = format!("{:?}", r.unwrap_err());
        assert!(msg.contains("Mut[...] is only valid on a parameter"),
            "field-Mut error must be the parameter-only message: {msg}");
    }

    #[test]
    fn method_byref_arg_temporary_is_rejected() {
        // EPIC-4 V2-c: the by-ref place-requirement is now enforced at METHOD call
        // sites too. Passing a temporary (a constructor result) to a by-ref method
        // param is an honest typeck error.
        let src = "\
class Account:
    balance: int
    def __init__(self, balance: int) -> None:
        self.balance = balance

class Bank:
    name: str
    def __init__(self, name: str) -> None:
        self.name = name
    def pay_into(self, acct: Mut[Account], amt: int) -> None:
        acct.balance = acct.balance + amt

def main() -> None:
    b: Bank = Bank(\"ACME\")
    b.pay_into(Account(5), 25)
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let r = check_bodies(&m, &ctx);
        assert!(r.is_err(), "a temporary passed to a by-ref method param must be rejected");
        let msg = format!("{:?}", r.unwrap_err());
        assert!(msg.contains("by-reference parameter `acct` requires a variable"),
            "method by-ref place error expected: {msg}");
    }

    #[test]
    fn method_byref_arg_place_is_accepted() {
        // The companion positive: a variable place passed to a by-ref method param
        // typeck-passes.
        let src = "\
class Account:
    balance: int
    def __init__(self, balance: int) -> None:
        self.balance = balance

class Bank:
    name: str
    def __init__(self, name: str) -> None:
        self.name = name
    def pay_into(self, acct: Mut[Account], amt: int) -> None:
        acct.balance = acct.balance + amt

def main() -> None:
    b: Bank = Bank(\"ACME\")
    a: Account = Account(100)
    b.pay_into(a, 25)
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        assert!(check_bodies(&m, &ctx).is_ok(),
            "a place passed to a by-ref method param must typeck-pass");
    }
}
