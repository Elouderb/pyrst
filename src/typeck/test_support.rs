use super::*;


    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Build a minimal FuncEnv backed by a fresh TyCtx, returning Unit.
    pub(crate) fn make_env(ctx: &TyCtx) -> FuncEnv<'_> {
        FuncEnv::with_by_ref(ctx, &[], &[], Ty::Unit)
    }

    /// Build a FuncEnv with a declared return type.
    pub(crate) fn make_env_ret(ctx: &TyCtx, ret: Ty) -> FuncEnv<'_> {
        FuncEnv::with_by_ref(ctx, &[], &[], ret)
    }

    /// Construct a Call expr: callee is an Ident, no kwargs.
    pub(crate) fn call_fn(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call {
            callee: Box::new(Expr::Ident(name.to_string(), Span::DUMMY)),
            args,
            kwargs: vec![],
            span: Span::DUMMY,
        }
    }

    /// Construct a method-call expr: obj.method(args).
    pub(crate) fn method_call(obj: Expr, method: &str, args: Vec<Expr>) -> Expr {
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
    pub(crate) fn ident(name: &str) -> Expr {
        Expr::Ident(name.to_string(), Span::DUMMY)
    }

    /// Int literal shorthand.
    pub(crate) fn int_lit(v: i64) -> Expr { Expr::Int(v, Span::DUMMY) }

    /// Float literal shorthand.
    pub(crate) fn float_lit(v: f64) -> Expr { Expr::Float(v, Span::DUMMY) }

    /// Str literal shorthand.
    pub(crate) fn str_lit(s: &str) -> Expr { Expr::Str(s.to_string(), Span::DUMMY) }

    /// Bool literal shorthand.
    pub(crate) fn bool_lit(v: bool) -> Expr { Expr::Bool(v, Span::DUMMY) }

    /// Assert that a Result<Ty> is a Type error whose message contains `fragment`.
    pub(crate) fn assert_type_err(r: Result<Ty>, fragment: &str) {
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
    pub(crate) fn assert_stmt_type_err(r: Result<()>, fragment: &str) {
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
    // Category A' — (EPIC-5 C1) class subtyping: is_subclass + types_compatible
    // =========================================================================

    /// Build a `ClassDef` with the given name and direct bases (no fields/methods).
    pub(crate) fn class_def(name: &str, bases: &[&str]) -> crate::ast::ClassDef {
        crate::ast::ClassDef {
            name: name.to_string(),
            bases: bases.iter().map(|s| s.to_string()).collect(),
            fields: vec![],
            methods: vec![],
            is_dataclass: false,
            decorators: vec![],
            dataclass_has_args: false,
            span: Span::DUMMY,
            type_params: vec![],
        }
    }

    /// A `TyCtx` with a single-inheritance chain Cat <- Dog <- Animal, plus an
    /// unrelated class Rock and an Exception-subclass MyErr(Exception). Note
    /// `Exception` itself is intentionally NOT registered (it is a builtin), so
    /// `is_subclass(MyErr, "Exception")` must be false.
    pub(crate) fn subtype_ctx() -> TyCtx {
        let mut ctx = TyCtx::new();
        ctx.classes.insert("Animal".into(), class_def("Animal", &[]));
        ctx.classes.insert("Dog".into(), class_def("Dog", &["Animal"]));
        ctx.classes.insert("Cat".into(), class_def("Cat", &["Dog"])); // transitive
        ctx.classes.insert("Rock".into(), class_def("Rock", &[]));
        ctx.classes.insert("MyErr".into(), class_def("MyErr", &["Exception"]));
        ctx
    }

    /// Sibling subclasses both directly under one base (`Dog`, `Cat` : `Animal`).
    pub(crate) fn sibling_ctx() -> TyCtx {
        let mut ctx = TyCtx::new();
        ctx.classes.insert("Animal".into(), class_def("Animal", &[]));
        ctx.classes.insert("Dog".into(), class_def("Dog", &["Animal"]));
        ctx.classes.insert("Cat".into(), class_def("Cat", &["Animal"]));
        ctx.classes.insert("Rock".into(), class_def("Rock", &[]));
        ctx
    }

    // =========================================================================
    // Category C2 — lambda / map / filter return-type inference (card 21424502)
    // =========================================================================

    /// Single-param lambda `lambda <param>: <body>` (param is untyped, as the
    /// parser emits — `TypeExpr::Named("Any")`).
    pub(crate) fn lambda1(param: &str, body: Expr) -> Expr {
        Expr::Lambda {
            params: vec![(param.to_string(), TypeExpr::Named("Any".into()))],
            body: Box::new(body),
            span: Span::DUMMY,
        }
    }

    /// `lhs <op> rhs` binary op.
    pub(crate) fn binop(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span: Span::DUMMY }
    }

    // =========================================================================
    // Category — EPIC-4 V2: Mut[T] by-reference param mode (front-end)
    // =========================================================================

    /// Register a single-param function whose one param is by-reference.
    pub(crate) fn ctx_with_byref_fn(name: &str, param: &str, ty: Ty) -> TyCtx {
        let mut ctx = TyCtx::new();
        ctx.funcs.insert(name.into(), FuncSig {
            params: vec![(param.into(), ty)],
            param_defaults: vec![None],
            param_by_ref: vec![true],
            ret: Ty::Unit,
        });
        ctx
    }

    /// Build a TyCtx from a module exactly as the single-module resolver path
    /// does (classes extracted, free funcs + methods registered self-exclusive).
    /// Used by the V2-c end-to-end check_bodies tests below.
    pub(crate) fn ctx_from_module(m: &Module) -> TyCtx {
        let mut ctx = TyCtx::new();
        for s in &m.stmts {
            if let Stmt::Class(c) = s {
                let mut c = c.clone();
                extract_init_fields(&mut c);
                ctx.classes.insert(c.name.clone(), c.clone());
                // Generics v2: register a generic class's type params + scope its
                // method sigs with them, mirroring the real resolver so
                // generic-class tests exercise the production code path.
                if !c.type_params.is_empty() {
                    ctx.generic_classes.insert(c.name.clone(), c.type_params.clone());
                }
                for mf in &c.methods {
                    let key = format!("{}.{}", c.name, mf.name);
                    ctx.funcs.insert(key, FuncSig {
                        params: mf.params.iter().filter(|p| p.name != "self")
                            .map(|p| (p.name.clone(), Ty::from_type_expr_scoped(&p.ty, p.span, &c.type_params).unwrap_or(Ty::Unknown)))
                            .collect(),
                        param_defaults: mf.params.iter().filter(|p| p.name != "self")
                            .map(|p| p.default.clone()).collect(),
                        param_by_ref: mf.params.iter().filter(|p| p.name != "self")
                            .map(|p| p.by_ref).collect(),
                        ret: Ty::from_type_expr_scoped(&mf.ret, mf.span, &c.type_params).unwrap_or(Ty::Unknown),
                    });
                }
            }
        }
        for s in &m.stmts {
            if let Stmt::Func(f) = s {
                // Lower param/return with the function's own type params in scope
                // so a generic `f`'s signature carries `Ty::TypeVar` (mirroring the
                // resolver's scoped lowering — `from_type_expr` alone would treat
                // `T` as an unknown class and break generic unification in tests).
                ctx.funcs.insert(f.name.clone(), FuncSig {
                    params: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| (p.name.clone(), Ty::from_type_expr_scoped(&p.ty, p.span, &f.type_params).unwrap_or(Ty::Unknown)))
                        .collect(),
                    param_defaults: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| p.default.clone()).collect(),
                    param_by_ref: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| p.by_ref).collect(),
                    ret: Ty::from_type_expr_scoped(&f.ret, f.span, &f.type_params).unwrap_or(Ty::Unknown),
                });
                // Generics: register the type-param list and (v2) the body so the
                // transitive-bound fixed point can recurse through generic calls.
                if !f.type_params.is_empty() {
                    ctx.generic_funcs.insert(f.name.clone(), f.type_params.clone());
                    ctx.generic_func_bodies.insert(f.name.clone(), f.clone());
                }
            }
        }
        ctx
    }

    // =========================================================================
    // Qualified module calls — `import X; X.f(args)` (card 81db88e0)
    // =========================================================================

    /// Build a TyCtx that models `import os` having merged the embedded `os`
    /// module: its functions live FLAT in `ctx.funcs` (under bare names) and the
    /// module→funcs index `module_funcs["os"]` lists them. Mirrors what the
    /// resolver produces for a non-root module.
    pub(crate) fn ctx_with_os_module() -> TyCtx {
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("basename".into(), FuncSig {
            params: vec![("p".into(), Ty::Str)],
            param_defaults: vec![None],
            param_by_ref: vec![],
            ret: Ty::Str,
        });
        ctx.funcs.insert("getenv".into(), FuncSig {
            params: vec![("key".into(), Ty::Str), ("default".into(), Ty::Str)],
            param_defaults: vec![None, None],
            param_by_ref: vec![],
            ret: Ty::Str,
        });
        ctx.module_funcs.insert("os".into(), vec!["basename".into(), "getenv".into()]);
        ctx
    }

    // -------------------------------------------------------------------------
    // Missing-return gate (card adcbe706): block_definitely_returns + the
    // all-paths-return check applied to non-unit, non-generator functions.
    // -------------------------------------------------------------------------

    // --- block_definitely_returns: direct rule coverage ---

    pub(crate) fn ret_val() -> Stmt { Stmt::Return(Some(int_lit(1)), Span::DUMMY) }
    pub(crate) fn raise_stmt() -> Stmt { Stmt::Raise { exc: Some(call_fn("ValueError", vec![str_lit("x")])), span: Span::DUMMY } }

    // --- block_definitely_returns: Stmt::Try arm (card 57274b36) ---

    pub(crate) fn handler(returns: bool) -> ExceptHandler {
        ExceptHandler {
            exc_type: Some("ValueError".into()),
            exc_name: None,
            body: if returns { vec![ret_val()] } else { vec![Stmt::Pass(Span::DUMMY)] },
            span: Span::DUMMY,
        }
    }

    pub(crate) fn try_stmt(
        body: Vec<Stmt>,
        handlers: Vec<ExceptHandler>,
        else_: Option<Vec<Stmt>>,
        finally_: Option<Vec<Stmt>>,
    ) -> Stmt {
        Stmt::Try { body, handlers, else_, finally_, span: Span::DUMMY }
    }

    // --- End-to-end gate via check_bodies (the real `check` path) ---

    pub(crate) fn check_src(src: &str) -> Result<()> {
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        check_bodies(&m, &ctx)
    }

    // -------------------------------------------------------------------------
    // Generics v2: bounded generics — op -> bound inference + still-rejected ops
    // -------------------------------------------------------------------------

    /// Infer the bound set for the FIRST generic function in `src`.
    pub(crate) fn bounds_of_first_func(src: &str) -> std::collections::BTreeMap<
        String,
        std::collections::BTreeSet<TypeVarBound>,
    > {
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let f = m.stmts.iter().find_map(|s| match s {
            Stmt::Func(f) if !f.type_params.is_empty() => Some(f),
            _ => None,
        }).expect("generic func");
        infer_func_typevar_bounds(f, &ctx)
    }

    /// Infer the (propagated) bound set for the generic function NAMED `name`.
    pub(crate) fn bounds_of_named_func(src: &str, name: &str) -> std::collections::BTreeMap<
        String,
        std::collections::BTreeSet<TypeVarBound>,
    > {
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let f = m.stmts.iter().find_map(|s| match s {
            Stmt::Func(f) if f.name == name => Some(f),
            _ => None,
        }).expect("named func");
        infer_func_typevar_bounds(f, &ctx)
    }

    /// Assert a `Result<()>` is a Type error whose message contains `fragment`.
    pub(crate) fn assert_type_err_unit(r: Result<()>, fragment: &str) {
        match r {
            Err(Error::Type { msg, .. }) => assert!(
                msg.contains(fragment),
                "expected error containing {:?}, got msg: {:?}", fragment, msg
            ),
            Err(other) => panic!("expected Type error, got {:?}", other),
            Ok(()) => panic!("expected Type error, got Ok(())"),
        }
    }
