use super::*;
use super::test_support::*;

    // (EPIC-5 C1-B) `types_compatible` gained a `&TyCtx` param. The existing
    // class-free matrix tests below do not exercise subtyping, so this 2-arg
    // shim forwards to the real function with an empty `TyCtx` (no user classes),
    // keeping those assertions readable and unchanged in meaning. This local item
    // intentionally shadows the glob-imported `super::types_compatible` for the
    // 2-arg call sites in this module; the new subtyping tests call
    // `super::types_compatible(a, b, ctx)` explicitly with a populated ctx.
    fn types_compatible(val_ty: &Ty, declared_ty: &Ty) -> bool {
        super::types_compatible(val_ty, declared_ty, &TyCtx::new())
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
            &Ty::Option(Box::new(Ty::Class("Point".into(), vec![])))
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
            &Ty::Option(Box::new(Ty::Class("Point".into(), vec![])))
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
            &Ty::Class("Point".into(), vec![]),
            &Ty::Option(Box::new(Ty::Class("Point".into(), vec![])))
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
    fn optional_is_none_early_return_narrows_after() {
        // (card c34ac64a, shape 1a) NEGATIVE narrowing: `if x is None: return`
        // (no elif/else, terminating then) narrows x to its payload for the REST
        // of the scope — the early-return guard idiom.
        let ctx = TyCtx::new();
        let mut env = make_env_ret(&ctx, Ty::Int);
        env.locals.insert("x".into(), Ty::Option(Box::new(Ty::Int)));
        let guard = Stmt::If {
            cond: Expr::BinOp {
                op: BinOp::Is,
                lhs: Box::new(ident("x")),
                rhs: Box::new(Expr::None_(Span::DUMMY)),
                span: Span::DUMMY,
            },
            then: vec![Stmt::Return(Some(int_lit(0)), Span::DUMMY)],
            elifs: vec![],
            else_: None,
            span: Span::DUMMY,
        };
        check_stmt(&guard, &mut env).unwrap();
        // x narrows to int after the terminating `is None` guard...
        assert_eq!(env.locals.get("x"), Some(&Ty::Int));
        // ...so `x + 1` now type-checks as int (was an "operator on Optional" error).
        let add = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(ident("x")),
            rhs: Box::new(int_lit(1)),
            span: Span::DUMMY,
        };
        assert_eq!(check_expr(&add, &mut env).unwrap(), Ty::Int);
    }

    #[test]
    fn optional_is_none_guard_with_else_does_not_persist() {
        // The negative narrowing is restricted to the else-less/elif-less shape:
        // an `else` may fall through (and reassign x), so x stays Optional after.
        let ctx = TyCtx::new();
        let mut env = make_env_ret(&ctx, Ty::Int);
        env.locals.insert("x".into(), Ty::Option(Box::new(Ty::Int)));
        let guard = Stmt::If {
            cond: Expr::BinOp {
                op: BinOp::Is,
                lhs: Box::new(ident("x")),
                rhs: Box::new(Expr::None_(Span::DUMMY)),
                span: Span::DUMMY,
            },
            then: vec![Stmt::Return(Some(int_lit(0)), Span::DUMMY)],
            elifs: vec![],
            else_: Some(vec![Stmt::Pass(Span::DUMMY)]),
            span: Span::DUMMY,
        };
        check_stmt(&guard, &mut env).unwrap();
        assert_eq!(env.locals.get("x"), Some(&Ty::Option(Box::new(Ty::Int))));
    }

    #[test]
    fn optional_while_not_none_narrows_body() {
        // (card c34ac64a, shape 1c) WHILE-loop narrowing: `while x is not None:`
        // narrows x to its payload inside the body (so `x + 1` checks), restored
        // to Option after the loop.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("x".into(), Ty::Option(Box::new(Ty::Int)));
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
        let while_stmt = Stmt::While {
            cond: Expr::BinOp {
                op: BinOp::IsNot,
                lhs: Box::new(ident("x")),
                rhs: Box::new(Expr::None_(Span::DUMMY)),
                span: Span::DUMMY,
            },
            body: vec![body_assign],
            span: Span::DUMMY,
        };
        check_stmt(&while_stmt, &mut env).unwrap();
        assert_eq!(env.locals.get("x"), Some(&Ty::Option(Box::new(Ty::Int))));
    }

    #[test]
    fn optional_narrow_dies_after_for_loop() {
        // (card c34ac64a fix B1) A negative narrow born INSIDE a for-loop body
        // (`if v is None: continue`) must NOT leak past the loop — the body runs
        // 0..n times, so `v` is Optional again AFTER the loop.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("v".into(), Ty::Option(Box::new(Ty::Int)));
        let guard = Stmt::If {
            cond: Expr::BinOp {
                op: BinOp::Is,
                lhs: Box::new(ident("v")),
                rhs: Box::new(Expr::None_(Span::DUMMY)),
                span: Span::DUMMY,
            },
            then: vec![Stmt::Continue(Span::DUMMY)],
            elifs: vec![],
            else_: None,
            span: Span::DUMMY,
        };
        let for_stmt = Stmt::For {
            targets: vec!["i".into()],
            iter: call_fn("range", vec![int_lit(3)]),
            body: vec![guard],
            span: Span::DUMMY,
        };
        check_stmt(&for_stmt, &mut env).unwrap();
        // The body-born narrow died at the loop edge: v is Optional again.
        assert_eq!(env.locals.get("v"), Some(&Ty::Option(Box::new(Ty::Int))));
    }

    #[test]
    fn second_none_guard_on_narrowed_var_rejected() {
        // (card c34ac64a fix B3) After `if x is None: return` narrows x to int, a
        // SECOND `if x is None:` is statically always-false — an honest CHECK error
        // (was a leaked rustc `.is_none()`-on-i64).
        let ctx = TyCtx::new();
        let mut env = make_env_ret(&ctx, Ty::Int);
        env.locals.insert("x".into(), Ty::Option(Box::new(Ty::Int)));
        let mk_guard = || Stmt::If {
            cond: Expr::BinOp {
                op: BinOp::Is,
                lhs: Box::new(ident("x")),
                rhs: Box::new(Expr::None_(Span::DUMMY)),
                span: Span::DUMMY,
            },
            then: vec![Stmt::Return(Some(int_lit(-1)), Span::DUMMY)],
            elifs: vec![],
            else_: None,
            span: Span::DUMMY,
        };
        // First guard narrows x -> int.
        check_stmt(&mk_guard(), &mut env).unwrap();
        assert_eq!(env.locals.get("x"), Some(&Ty::Int));
        // Second identical guard is rejected (x can no longer be None).
        assert!(check_stmt(&mk_guard(), &mut env).is_err());
    }

    #[test]
    fn reassign_narrowed_var_to_none_rewidens() {
        // (card c34ac64a fix B2c) A reassignment KILLS the narrow: after
        // `if x is None: return`, `x = None` re-widens x back to Optional
        // (matching codegen's reconverge into the Option slot).
        let ctx = TyCtx::new();
        let mut env = make_env_ret(&ctx, Ty::Int);
        env.locals.insert("x".into(), Ty::Option(Box::new(Ty::Int)));
        let guard = Stmt::If {
            cond: Expr::BinOp {
                op: BinOp::Is,
                lhs: Box::new(ident("x")),
                rhs: Box::new(Expr::None_(Span::DUMMY)),
                span: Span::DUMMY,
            },
            then: vec![Stmt::Return(Some(int_lit(-1)), Span::DUMMY)],
            elifs: vec![],
            else_: None,
            span: Span::DUMMY,
        };
        check_stmt(&guard, &mut env).unwrap();
        assert_eq!(env.locals.get("x"), Some(&Ty::Int));
        // `x = None` reassignment re-widens to the declared Option.
        let reassign = Stmt::Assign {
            target: "x".into(),
            ty: None,
            value: Expr::None_(Span::DUMMY),
            span: Span::DUMMY,
        };
        check_stmt(&reassign, &mut env).unwrap();
        assert_eq!(env.locals.get("x"), Some(&Ty::Option(Box::new(Ty::Int))));
        assert!(!env.narrowed.contains_key("x"));
    }

    #[test]
    fn optional_attribute_access_rejected() {
        // (card c34ac64a, shape 1b) Accessing an attribute on an Optional value is
        // an honest error (bind to a local first) — never a silent rustc leak.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        env.locals.insert("s".into(), Ty::Option(Box::new(Ty::Int)));
        let attr = Expr::Attr {
            obj: Box::new(ident("s")),
            name: "v".into(),
            span: Span::DUMMY,
        };
        assert_type_err(check_expr(&attr, &mut env), "Optional value");
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
            &Ty::Class("Foo".into(), vec![]),
            &Ty::Class("Foo".into(), vec![])
        ));
    }

    #[test]
    fn compat_class_different_false() {
        assert!(!types_compatible(
            &Ty::Class("Foo".into(), vec![]),
            &Ty::Class("Bar".into(), vec![])
        ));
    }

    #[test]
    fn is_subclass_reflexive() {
        let ctx = subtype_ctx();
        assert!(is_subclass("Animal", "Animal", &ctx));
        assert!(is_subclass("Dog", "Dog", &ctx));
        // Reflexive even for a name not in ctx (mirrors the `a == b` fast path).
        assert!(is_subclass("Unknown", "Unknown", &ctx));
    }

    #[test]
    fn is_subclass_direct() {
        let ctx = subtype_ctx();
        assert!(is_subclass("Dog", "Animal", &ctx)); // Dog -> Animal (direct)
    }

    #[test]
    fn is_subclass_transitive() {
        let ctx = subtype_ctx();
        assert!(is_subclass("Cat", "Animal", &ctx)); // Cat -> Dog -> Animal
        assert!(is_subclass("Cat", "Dog", &ctx));
    }

    #[test]
    fn is_subclass_not_reverse() {
        let ctx = subtype_ctx();
        // Directional: a Base is NOT a subclass of its Derived.
        assert!(!is_subclass("Animal", "Dog", &ctx));
        assert!(!is_subclass("Animal", "Cat", &ctx));
    }

    #[test]
    fn is_subclass_unrelated() {
        let ctx = subtype_ctx();
        assert!(!is_subclass("Rock", "Animal", &ctx));
        assert!(!is_subclass("Dog", "Rock", &ctx));
    }

    #[test]
    fn is_subclass_builtin_exception_false() {
        let ctx = subtype_ctx();
        // `Exception` is a builtin not registered in ctx.classes, so even though
        // MyErr lists it as a base, is_subclass cannot reach it -> false. Exception
        // subtyping stays deliberately unimplemented (design §D).
        assert!(!is_subclass("MyErr", "Exception", &ctx));
    }

    #[test]
    fn types_compatible_accepts_derived_for_base() {
        let ctx = subtype_ctx();
        // A Derived value satisfies a Base slot (direct and transitive).
        assert!(super::types_compatible(
            &Ty::Class("Dog".into(), vec![]),
            &Ty::Class("Animal".into(), vec![]),
            &ctx
        ));
        assert!(super::types_compatible(
            &Ty::Class("Cat".into(), vec![]),
            &Ty::Class("Animal".into(), vec![]),
            &ctx
        ));
    }

    #[test]
    fn types_compatible_rejects_base_for_derived() {
        let ctx = subtype_ctx();
        // The reverse (Base value into a Derived slot) is NOT compatible.
        assert!(!super::types_compatible(
            &Ty::Class("Animal".into(), vec![]),
            &Ty::Class("Dog".into(), vec![]),
            &ctx
        ));
    }

    #[test]
    fn types_compatible_rejects_unrelated_classes() {
        let ctx = subtype_ctx();
        assert!(!super::types_compatible(
            &Ty::Class("Rock".into(), vec![]),
            &Ty::Class("Animal".into(), vec![]),
            &ctx
        ));
        // Sibling-ish but unrelated by inheritance.
        assert!(!super::types_compatible(
            &Ty::Class("Animal".into(), vec![]),
            &Ty::Class("Rock".into(), vec![]),
            &ctx
        ));
    }

    #[test]
    fn types_compatible_exception_subclass_stays_incompatible() {
        let ctx = subtype_ctx();
        // MyErr is not is_subclass of the builtin Exception -> incompatible.
        assert!(!super::types_compatible(
            &Ty::Class("MyErr".into(), vec![]),
            &Ty::Class("Exception".into(), vec![]),
            &ctx
        ));
    }

    #[test]
    fn unify_branch_types_two_subtypes_yields_base() {
        let ctx = subtype_ctx();
        // Both orderings unify to the BASE (wider) class, not the first-seen one.
        assert_eq!(
            unify_branch_types(Ty::Class("Dog".into(), vec![]), Ty::Class("Animal".into(), vec![]), &ctx),
            Some(Ty::Class("Animal".into(), vec![]))
        );
        assert_eq!(
            unify_branch_types(Ty::Class("Animal".into(), vec![]), Ty::Class("Dog".into(), vec![]), &ctx),
            Some(Ty::Class("Animal".into(), vec![]))
        );
        // Transitive: Cat & Animal -> Animal.
        assert_eq!(
            unify_branch_types(Ty::Class("Cat".into(), vec![]), Ty::Class("Animal".into(), vec![]), &ctx),
            Some(Ty::Class("Animal".into(), vec![]))
        );
    }

    #[test]
    fn unify_branch_types_unrelated_classes_rejected() {
        let ctx = subtype_ctx();
        // Unrelated classes do not unify (no common slot in C1).
        assert_eq!(
            unify_branch_types(Ty::Class("Rock".into(), vec![]), Ty::Class("Animal".into(), vec![]), &ctx),
            None
        );
    }

    #[test]
    fn nearest_common_ancestor_siblings_and_chain() {
        let ctx = sibling_ctx();
        // (EPIC-5 C2-2b-i) Two sibling subclasses meet at their shared base.
        assert_eq!(nearest_common_ancestor("Dog", "Cat", &ctx), Some("Animal".into()));
        assert_eq!(nearest_common_ancestor("Cat", "Dog", &ctx), Some("Animal".into()));
        // Reflexive / ancestor-descendant cases resolve at the wider class.
        assert_eq!(nearest_common_ancestor("Dog", "Animal", &ctx), Some("Animal".into()));
        assert_eq!(nearest_common_ancestor("Dog", "Dog", &ctx), Some("Dog".into()));
        // No common user-declared ancestor -> None.
        assert_eq!(nearest_common_ancestor("Dog", "Rock", &ctx), None);
    }

    #[test]
    fn unify_branch_types_siblings_yield_common_base() {
        let ctx = sibling_ctx();
        // (EPIC-5 C2-2b-i) `[Dog(), Cat()]` -> the literal's element type is the
        // common base `Animal`, in EITHER element order.
        assert_eq!(
            unify_branch_types(Ty::Class("Dog".into(), vec![]), Ty::Class("Cat".into(), vec![]), &ctx),
            Some(Ty::Class("Animal".into(), vec![]))
        );
        assert_eq!(
            unify_branch_types(Ty::Class("Cat".into(), vec![]), Ty::Class("Dog".into(), vec![]), &ctx),
            Some(Ty::Class("Animal".into(), vec![]))
        );
        // A class with no common ancestor with `Dog` still does NOT unify.
        assert_eq!(
            unify_branch_types(Ty::Class("Dog".into(), vec![]), Ty::Class("Rock".into(), vec![]), &ctx),
            None
        );
    }

    #[test]
    fn unify_branch_types_same_class_unchanged() {
        let ctx = subtype_ctx();
        assert_eq!(
            unify_branch_types(Ty::Class("Dog".into(), vec![]), Ty::Class("Dog".into(), vec![]), &ctx),
            Some(Ty::Class("Dog".into(), vec![]))
        );
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
        // (card 49170944) partition/rpartition now return a 3-TUPLE (str,str,str)
        // — CPython's real shape — so `a, sep, b = s.partition("=")` unpacks.
        assert_eq!(
            builtin_method_ret(&Ty::Str, "partition"),
            Ty::Tuple(vec![Ty::Str, Ty::Str, Ty::Str])
        );
    }

    #[test]
    fn method_ret_str_rpartition() {
        assert_eq!(
            builtin_method_ret(&Ty::Str, "rpartition"),
            Ty::Tuple(vec![Ty::Str, Ty::Str, Ty::Str])
        );
    }

    #[test]
    fn method_ret_str_casefold_and_rsplit() {
        // (card 49170944) casefold -> str; rsplit -> list[str].
        assert_eq!(builtin_method_ret(&Ty::Str, "casefold"), Ty::Str);
        assert_eq!(
            builtin_method_ret(&Ty::Str, "rsplit"),
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
        assert_eq!(builtin_method_ret(&Ty::Handle("file".into()), "read"), Ty::Str);
    }

    #[test]
    fn method_ret_file_readlines() {
        assert_eq!(
            builtin_method_ret(&Ty::Handle("file".into()), "readlines"),
            Ty::List(Box::new(Ty::Str))
        );
    }

    #[test]
    fn method_ret_file_write_is_unit() {
        assert_eq!(builtin_method_ret(&Ty::Handle("file".into()), "write"), Ty::Unit);
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
        let err = Ty::from_type_expr(&t, Span::DUMMY).unwrap_err();
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
        assert!(matches!(Ty::from_type_expr(&t, Span::DUMMY), Err(Error::Type { .. })));
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
            Ty::from_type_expr(&t, Span::DUMMY).unwrap(),
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
    fn infer_nary_scalar_min_max_is_first_arg_type() {
        // (card b557b9c1) The n-ary scalar form min/max(a, b, ...) returns one of
        // its homogeneous positional args, so its result type is the first arg's
        // type. This was formerly the "BUG 3" Unknown fall-through, which starved
        // the print-formatter of the float type (`max(1.0, 2.0, 3.0)` displayed
        // `3` instead of `3.0`). Covers the 2-arg and 3-arg int / float / str forms.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        // 2-arg int -> Int (was Unknown).
        let e_int2 = call_fn("min", vec![int_lit(1), int_lit(2)]);
        assert_eq!(check_expr(&e_int2, &mut env).unwrap(), Ty::Int);
        // 3-arg float -> Float (drives `__py_fmt_float` display).
        let e_flt3 = call_fn("max", vec![float_lit(1.0), float_lit(2.0), float_lit(3.0)]);
        assert_eq!(check_expr(&e_flt3, &mut env).unwrap(), Ty::Float);
        // 3-arg str -> Str.
        let e_str3 = call_fn("min", vec![str_lit("a"), str_lit("b"), str_lit("c")]);
        assert_eq!(check_expr(&e_str3, &mut env).unwrap(), Ty::Str);
    }

    #[test]
    fn nary_scalar_min_max_with_key_is_rejected() {
        // (card b557b9c1) key= is only supported for the single-iterable form;
        // combined with 2+ positional args it has no lowering and must be an honest
        // check-time error (never a silent drop or a leaked rustc failure).
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let mut e = call_fn("max", vec![int_lit(1), int_lit(2), int_lit(3)]);
        if let Expr::Call { kwargs, .. } = &mut e {
            kwargs.push(("key".to_string(), int_lit(0)));
        }
        assert_type_err(check_expr(&e, &mut env), "does not support");
    }

    #[test]
    fn sum_three_args_is_rejected() {
        // (card aabf4ada) `sum` is variadic-exempt from the generic arity check, so a
        // 3rd positional arg was ACCEPTED and silently DROPPED at codegen. CPython
        // raises TypeError; pyrst now rejects it honestly at check.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = call_fn("sum", vec![
            Expr::List(vec![int_lit(1), int_lit(2), int_lit(3)], Span::DUMMY),
            int_lit(10),
            int_lit(20),
        ]);
        assert_type_err(check_expr(&e, &mut env), "at most 2 arguments");
    }

    #[test]
    fn min_max_zero_args_is_rejected() {
        // (card aabf4ada) min()/max() with ZERO args was a codegen `parts[0]`-on-
        // empty-vec ICE (exit 101); CPython raises "expected at least 1 argument".
        // Now an honest check-time arity error for both names.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        assert_type_err(check_expr(&call_fn("min", vec![]), &mut env), "at least 1 argument");
        assert_type_err(check_expr(&call_fn("max", vec![]), &mut env), "at least 1 argument");
    }

    #[test]
    fn int_base_arg_is_rejected() {
        // (card aabf4ada) `int(x, base)` DROPPED the base (int("10",2) -> 10 not 2, a
        // silent miscompile surfaced by the variadic-exempt audit). pyrst's `int(x)`
        // parses base 10 only; a 2nd arg is rejected honestly rather than miscompiled.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = call_fn("int", vec![str_lit("10"), int_lit(2)]);
        assert_type_err(check_expr(&e, &mut env), "base argument is not supported");
    }

    #[test]
    fn dynamic_attr_builtins_are_rejected() {
        // (card aabf4ada) getattr/setattr/hasattr were silently-wrong codegen stubs
        // (returned the NAME string / no-op'd / always-true). pyrst resolves
        // attributes statically, so all three are honest check-time rejections.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        for name in ["getattr", "setattr", "hasattr"] {
            let e = call_fn(name, vec![ident("obj"), str_lit("attr")]);
            assert_type_err(check_expr(&e, &mut env), "dynamic attribute access");
        }
    }

    #[test]
    fn open_extra_positional_arg_is_rejected() {
        // (card aabf4ada) `open` read only args[0]/args[1] (path, mode) — a 3rd+
        // positional arg (CPython's `buffering`) was silently DROPPED. pyrst supports
        // `open(path[, mode])` only; a 3rd positional arg is rejected honestly.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        let e = call_fn("open", vec![str_lit("x.txt"), str_lit("r"), int_lit(1)]);
        assert_type_err(check_expr(&e, &mut env), "at most 2 positional arguments");
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
        // (W5-b) `encode` is now emittable (str.encode -> bytes) and intentionally
        // PRESENT; only isdecimal/format remain unemittable.
        let unemittable = ["isdecimal", "format"];
        for m in &unemittable {
            assert!(
                !STR_METHODS.contains(m),
                "STR_METHODS contains `{m}` but codegen cannot emit it \
                 (card 36f66dd2 drift guard)"
            );
        }
        assert!(
            STR_METHODS.contains(&"encode"),
            "STR_METHODS must contain `encode` (W5-b wired str.encode -> bytes)"
        );
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
        let unemittable = ["isdecimal", "format"];
        for m in &unemittable {
            assert_eq!(
                builtin_method_ret(&Ty::Str, m),
                Ty::Unknown,
                "builtin_method_ret returned a concrete type for removed str method `{m}` \
                 (card 36f66dd2 drift guard)"
            );
        }
        // (W5-b) encode is now a concrete str method -> bytes.
        assert_eq!(builtin_method_ret(&Ty::Str, "encode"), Ty::Bytes);
    }

    /// (W5-b) Every method in BYTES_METHODS must have a concrete (non-Unknown)
    /// return type in `builtin_method_ret`. A table entry without a matching
    /// return-type arm would type as Unknown and silently drift from its codegen
    /// arm — this keeps the two in lockstep (the bytes analogue of the str guard).
    #[test]
    fn bytes_methods_have_concrete_return_types() {
        for m in BYTES_METHODS {
            assert_ne!(
                builtin_method_ret(&Ty::Bytes, m),
                Ty::Unknown,
                "BYTES_METHODS contains `{m}` but builtin_method_ret returns Unknown \
                 for it (table/return-type drift — add its arm)"
            );
        }
        // Spot-check the representative return shapes (byte-offset throughout).
        assert_eq!(builtin_method_ret(&Ty::Bytes, "hex"), Ty::Str);
        assert_eq!(builtin_method_ret(&Ty::Bytes, "decode"), Ty::Str);
        assert_eq!(builtin_method_ret(&Ty::Bytes, "find"), Ty::Int);
        assert_eq!(builtin_method_ret(&Ty::Bytes, "isdigit"), Ty::Bool);
        assert_eq!(builtin_method_ret(&Ty::Bytes, "upper"), Ty::Bytes);
        assert_eq!(
            builtin_method_ret(&Ty::Bytes, "split"),
            Ty::List(Box::new(Ty::Bytes))
        );
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
            Ty::Class("Point".into(), vec![]),
        ];
        for t in cases {
            assert!(!is_copy(&t), "{t:?} must be non-Copy");
            assert!(is_owned(&t), "{t:?} must be owned");
        }
    }

    #[test]
    fn is_copy_conservative_non_copy_variants() {
        // Matches the legacy `is_copy_type`, which excluded these (=> non-Copy).
        for t in [Ty::NoneVal, Ty::Handle("file".into()), Ty::Unknown] {
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
        assert!(!is_copy(&Ty::Option(Box::new(Ty::Class("Point".into(), vec![])))));
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
    fn func_compatibility_arity_args_ret() {
        let ctx = TyCtx::new();
        let int_to_int = Ty::Func(vec![Ty::Int], Box::new(Ty::Int));
        // Exact match.
        assert!(super::types_compatible(&int_to_int, &int_to_int, &ctx));
        // An untyped-lambda value `Callable[[unknown], unknown]` fills a declared
        // `Callable[[int], int]` (Unknown is universally compatible).
        let unknown_fn = Ty::Func(vec![Ty::Unknown], Box::new(Ty::Unknown));
        assert!(super::types_compatible(&unknown_fn, &int_to_int, &ctx));
        // Arity mismatch is rejected.
        let two_arg = Ty::Func(vec![Ty::Int, Ty::Int], Box::new(Ty::Int));
        assert!(!super::types_compatible(&two_arg, &int_to_int, &ctx));
        // Concrete return mismatch is rejected.
        let int_to_str = Ty::Func(vec![Ty::Int], Box::new(Ty::Str));
        assert!(!super::types_compatible(&int_to_str, &int_to_int, &ctx));
        // Concrete arg mismatch is rejected.
        let str_to_int = Ty::Func(vec![Ty::Str], Box::new(Ty::Int));
        assert!(!super::types_compatible(&str_to_int, &int_to_int, &ctx));
    }

    // =========================================================================
    // LAZY-GEN V1-a — the new `Ty::Iterator(T)` variant
    // (docs/design/lazy-generators.md §A.1, §D, §G)
    // =========================================================================

    #[test]
    fn iterator_display_renders_as_iterator_bracket() {
        // Hover / diagnostics render an `Iterator[T]` distinctly from `list[T]`.
        assert_eq!(Ty::Iterator(Box::new(Ty::Int)).to_string(), "Iterator[int]");
        // Nested element types render recursively.
        assert_eq!(
            Ty::Iterator(Box::new(Ty::List(Box::new(Ty::Str)))).to_string(),
            "Iterator[list[str]]"
        );
    }

    #[test]
    fn iterator_type_expr_lowers_to_iterator_variant() {
        // `Iterator[int]` lowers to the DISTINCT `Ty::Iterator(Int)` variant —
        // NOT `Ty::List(Int)` as it did before LAZY-GEN V1-a.
        let te = TypeExpr::Generic(
            "Iterator".to_string(),
            vec![TypeExpr::Named("int".to_string())],
        );
        assert_eq!(
            Ty::from_type_expr(&te, Span::DUMMY).unwrap(),
            Ty::Iterator(Box::new(Ty::Int))
        );
        // It is explicitly a different value from the list of the same element.
        assert_ne!(
            Ty::from_type_expr(&te, Span::DUMMY).unwrap(),
            Ty::List(Box::new(Ty::Int))
        );
    }

    #[test]
    fn unify_typevar_binds_through_iterator() {
        // `def first(xs: Iterator[T]) -> T` must bind `T` from the argument, just
        // like a `list[T]` param does. An `Iterator[int]` actual binds T=int.
        let tps = vec!["T".to_string()];
        let mut subst: HashMap<String, Ty> = HashMap::new();
        super::unify_typevar(
            &Ty::Iterator(Box::new(Ty::TypeVar("T".to_string()))),
            &Ty::Iterator(Box::new(Ty::Int)),
            &tps,
            &mut subst,
        )
        .unwrap();
        assert_eq!(subst.get("T"), Some(&Ty::Int));

        // A `list[str]` actual also binds an `Iterator[T]` param (a list is
        // covariantly assignable to an iterator slot — V1-a).
        let mut subst2: HashMap<String, Ty> = HashMap::new();
        super::unify_typevar(
            &Ty::Iterator(Box::new(Ty::TypeVar("T".to_string()))),
            &Ty::List(Box::new(Ty::Str)),
            &tps,
            &mut subst2,
        )
        .unwrap();
        assert_eq!(subst2.get("T"), Some(&Ty::Str));
    }

    #[test]
    fn v1d_iterator_and_list_are_not_interchangeable_in_types_compatible() {
        // V1-d FLIPS the V1-a behavior-invisible bridge: a generator is not a list.
        // Only `Iterator[T]` fills an `Iterator[T]` slot; both CROSS directions are
        // now rejected (the honest MATERIALIZE error / the deferred V2 list→Gen
        // adapter). `reject_iterator_into_list` supplies the helpful message at the
        // arg/return/assignment sites; here we assert the raw compatibility matrix.
        let it_int = Ty::Iterator(Box::new(Ty::Int));
        let li_int = Ty::List(Box::new(Ty::Int));
        assert!(!types_compatible(&it_int, &li_int)); // Iterator -> list slot: NO (materialize)
        assert!(!types_compatible(&li_int, &it_int)); // list -> Iterator slot: NO (V2 adapter)
        assert!(types_compatible(&it_int, &it_int)); // Iterator -> Iterator slot: yes
        // The cross directions stay false even with an Unknown element (no List↔Iterator bridge).
        assert!(!types_compatible(
            &Ty::List(Box::new(Ty::Unknown)),
            &Ty::Iterator(Box::new(Ty::Int))
        ));
        // Iterator -> Iterator with an Unknown element remains permissive (like list).
        assert!(types_compatible(
            &Ty::Iterator(Box::new(Ty::Unknown)),
            &Ty::Iterator(Box::new(Ty::Int))
        ));
    }

    #[test]
    fn substitute_typevars_descends_through_iterator() {
        // `Iterator[T]` with {T -> int} substitutes to `Iterator[int]`, mirroring
        // how `list[T]` substitutes (V1-a: treat Iterator exactly like List).
        let mut subst: HashMap<String, Ty> = HashMap::new();
        subst.insert("T".to_string(), Ty::Int);
        assert_eq!(
            super::substitute_typevars(
                &Ty::Iterator(Box::new(Ty::TypeVar("T".to_string()))),
                &subst
            ),
            Ty::Iterator(Box::new(Ty::Int))
        );
    }

    // =========================================================================
    // Uniform check-time kwargs gate (card d8a1ed83)
    // =========================================================================

    /// (kwargs v1, card 8a7b7714) A keyword argument on a FLAT free-function
    /// call now MAPS to its parameter slot at check time (it was the honesty-
    /// stopgap rejection before the mapping landed).
    #[test]
    fn kwargs_map_accepts_flat_function_call() {
        let src = "\
def wrap(text: str, width: int) -> str:
    return text

def main() -> None:
    print(wrap(\"hi\", width=10))
";
        assert!(
            check_src(src).is_ok(),
            "a mappable keyword argument on a flat function must pass check"
        );
    }

    /// (kwargs v1) The three mapping errors are check-time and CPython-shaped:
    /// unknown keyword, duplicate binding, missing required parameter.
    #[test]
    fn kwargs_map_rejects_unknown_duplicate_missing() {
        let base = "\
def wrap(text: str, width: int = 70) -> str:
    return text

def main() -> None:
    print(wrap(CALL))
";
        let unknown = base.replace("CALL", "\"hi\", bogus=10");
        let msg = format!("{:?}", check_src(&unknown).expect_err("unknown kwarg"));
        assert!(msg.contains("unexpected keyword argument `bogus`"), "got: {msg}");

        let duplicate = base.replace("CALL", "\"hi\", text=\"bye\"");
        let msg = format!("{:?}", check_src(&duplicate).expect_err("duplicate kwarg"));
        assert!(msg.contains("got multiple values for argument `text`"), "got: {msg}");

        let missing = base.replace("CALL", "width=10");
        let msg = format!("{:?}", check_src(&missing).expect_err("missing required"));
        assert!(msg.contains("missing a required argument: `text`"), "got: {msg}");
    }

    /// (kwargs v1) A keyword argument on a USER METHOD call maps too
    /// (previously rejected by the stopgap gate); an unknown keyword on the
    /// method is still a check-time error.
    #[test]
    fn kwargs_map_accepts_user_method_call() {
        let src = "\
class Greeter:
    name: str
    def __init__(self, name: str) -> None:
        self.name = name
    def greet(self, times: int) -> str:
        return self.name

def main() -> None:
    g: Greeter = Greeter(\"hi\")
    print(g.greet(times=3))
";
        assert!(
            check_src(src).is_ok(),
            "a mappable keyword argument on a user method must pass check"
        );
        let bad = src.replace("times=3", "bogus=3");
        let msg = format!("{:?}", check_src(&bad).expect_err("unknown method kwarg"));
        assert!(msg.contains("unexpected keyword argument `bogus`"), "got: {msg}");
    }

    /// (kwargs v1) Builtin stubs stay positional-only — CPython's builtins
    /// reject keyword arguments (`abs(x=5)` is a TypeError there too).
    #[test]
    fn kwargs_map_keeps_builtins_positional_only() {
        let src = "\
def main() -> None:
    print(abs(x=5))
";
        let msg = format!("{:?}", check_src(src).expect_err("builtin kwarg"));
        assert!(msg.contains("takes no keyword arguments"), "got: {msg}");
    }

    /// An UNKNOWN keyword on a modeled builtin (`list.sort(bogus=1)`) is now a
    /// check-time error — closes the check/build asymmetry from card 5ca2030a.
    #[test]
    fn kwargs_gate_rejects_unknown_kwarg_on_sort() {
        let src = "\
def main() -> None:
    xs: list[int] = [3, 1, 2]
    xs.sort(bogus=1)
    print(xs[0])
";
        let err = check_src(src).expect_err("unknown kwarg on list.sort must be rejected");
        let msg = format!("{:?}", err);
        assert!(
            msg.contains("does not support the keyword argument `bogus`"),
            "expected the unknown-kwarg message, got: {msg}"
        );
    }

    /// The MODELED sites keep passing: a class constructor's field kwargs, and
    /// the builtin key=/reverse= of sorted and list.sort.
    #[test]
    fn kwargs_gate_allows_constructor_and_builtin_kwargs() {
        let ctor = "\
class Point:
    x: int
    y: int
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y

def main() -> None:
    p: Point = Point(x=1, y=2)
    print(p.x)
";
        assert!(check_src(ctor).is_ok(), "constructor field kwargs must pass");

        let builtins = "\
def main() -> None:
    xs: list[int] = [3, 1, 2]
    ys: list[int] = sorted(xs, reverse=True)
    xs.sort(reverse=True)
    m: int = max(xs, key=lambda v: -v)
    print(ys[0] + xs[0] + m)
";
        assert!(
            check_src(builtins).is_ok(),
            "sorted(reverse=)/list.sort(reverse=)/max(key=) must pass"
        );
    }

    /// (W1.5 fix B) Constructor keyword arguments bind the __init__ PARAMETERS,
    /// not the field names. A class whose __init__ params (a, b) differ from its
    /// fields (x, y) accepts `C(a=1, b=2)` and rejects a FIELD-name keyword
    /// (`C(x=1)`) at CHECK time, naming `<Class>.__init__` — closing the old
    /// check-pass / build-fail split.
    #[test]
    fn ctor_kwargs_bind_init_params() {
        let src = "\
class Renamed:
    x: int
    y: int
    def __init__(self, a: int, b: int) -> None:
        self.x = a
        self.y = b

def main() -> None:
    r: Renamed = Renamed(a=1, b=2)
    print(r.x)
";
        assert!(
            check_src(src).is_ok(),
            "constructor kwargs must bind __init__ params (a, b), not fields"
        );
        let bad = src.replace("Renamed(a=1, b=2)", "Renamed(x=1, y=2)");
        let msg = format!("{:?}", check_src(&bad).expect_err("field-name ctor kwarg"));
        assert!(
            msg.contains("`Renamed.__init__` got an unexpected keyword argument `x`"),
            "got: {msg}"
        );
    }

    /// (W1.5 fix B) A class WITHOUT __init__ is a struct-literal whose fields are
    /// matched via a synthesized field-order signature: a duplicate (positional +
    /// keyword) binding, a missing required field, and an unknown keyword are all
    /// honest CHECK-time errors (the positional was previously SILENTLY dropped).
    #[test]
    fn ctor_kwargs_no_init_reject_dup_missing_unknown() {
        let base = "\
class Point:
    x: int
    y: int

def main() -> None:
    p: Point = Point(CALL)
    print(p.x)
";
        let ok = base.replace("CALL", "1, y=2");
        assert!(check_src(&ok).is_ok(), "positional+keyword mix must pass");

        let dup = base.replace("CALL", "1, x=2");
        let msg = format!("{:?}", check_src(&dup).expect_err("dup field binding"));
        assert!(msg.contains("got multiple values for argument `x`"), "got: {msg}");

        let missing = base.replace("CALL", "1");
        let msg = format!("{:?}", check_src(&missing).expect_err("missing field"));
        assert!(msg.contains("missing a required argument: `y`"), "got: {msg}");

        let unknown = base.replace("CALL", "bogus=1");
        let msg = format!("{:?}", check_src(&unknown).expect_err("unknown field kwarg"));
        assert!(msg.contains("got an unexpected keyword argument `bogus`"), "got: {msg}");
    }
