use super::*;
use super::test_support::*;


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
        let r = Ty::from_type_expr(&me, Span::DUMMY);
        match r {
            Err(Error::Type { msg, .. }) =>
                assert!(msg.contains("Mut[...] is only valid on a parameter"), "got: {}", msg),
            other => panic!("expected Mut rejection, got {:?}", other),
        }
        // Nested inside another generic is rejected the same way.
        let nested = TypeExpr::Generic("list".into(), vec![me]);
        assert!(Ty::from_type_expr(&nested, Span::DUMMY).is_err(), "list[Mut[T]] must be rejected");
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
                        .map(|p| (p.name.clone(), Ty::from_type_expr(&p.ty, p.span).unwrap()))
                        .collect(),
                    param_defaults: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| p.default.clone()).collect(),
                    param_by_ref: f.params.iter().filter(|p| p.name != "self")
                        .map(|p| p.by_ref).collect(),
                    ret: Ty::from_type_expr(&f.ret, f.span).unwrap(),
                });
            }
        }
        assert!(check_bodies(&m, &ctx).is_ok(),
            "a mutated by-ref param must typeck-pass (backstop skipped)");
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

    #[test]
    fn get_method_resolves_param_and_return_types() {
        // find_method (via get_method) resolves the method's param and return
        // types through `from_type_expr`, consistent with check_bodies'
        // error-propagating path. A valid annotation must come back as the
        // concrete Ty (never silently dropped or coerced to Unknown).
        let src = "\
class Box:
    value: int
    def __init__(self, value: int) -> None:
        self.value = value
    def scale(self, factor: int, label: str) -> int:
        return self.value * factor
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let sig = ctx.get_method("Box", "scale").expect("scale must be found");
        assert_eq!(
            sig.params,
            vec![("factor".to_string(), Ty::Int), ("label".to_string(), Ty::Str)],
            "both annotated params must be resolved (none dropped)"
        );
        assert_eq!(sig.ret, Ty::Int, "the return annotation must lower to Int");
    }

    #[test]
    fn generic_class_wrong_arity_is_honest_check_error() {
        // Regression (BLOCKER-2): a generic-class constructor called with the
        // wrong NUMBER of arguments must be an honest typeck error, not a silent
        // check-pass that leaks to a rustc E0061 at build. `Box[T].__init__` takes
        // one arg; `Box(5, 6, 7)` supplies three.
        let src = "\
class Box[T]:
    value: T
    def __init__(self, v: T) -> None:
        self.value = v
    def get(self) -> T:
        return self.value

def main() -> None:
    b: Box[int] = Box(5, 6, 7)
    print(b.get())
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(
            errs.iter().any(|e| format!("{:?}", e).contains("takes 1 argument")),
            "wrong-arity generic constructor must be rejected with an arity error, got: {:?}",
            errs
        );
    }

    #[test]
    fn generic_class_correct_arity_type_checks() {
        // The arity gate must NOT over-reject: the correct argument count
        // type-checks and infers the instance type args from `__init__`.
        let src = "\
class Box[T]:
    value: T
    def __init__(self, v: T) -> None:
        self.value = v
    def get(self) -> T:
        return self.value

def main() -> None:
    b: Box[int] = Box(5)
    print(b.get())
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(errs.is_empty(), "correct-arity generic constructor must type-check, got: {:?}", errs);
    }

    #[test]
    fn generic_class_conflicting_type_args_rejected() {
        // Two constructor args binding the SAME class type var to inconsistent
        // concrete types is an honest conflict error (reuses `unify_typevar`).
        let src = "\
class Same[T]:
    a: T
    b: T
    def __init__(self, x: T, y: T) -> None:
        self.a = x
        self.b = y

def main() -> None:
    s: Same[int] = Same(1, \"two\")
    print(s.a)
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(
            errs.iter().any(|e| format!("{:?}", e).contains("conflicting types for type parameter")),
            "inconsistent type-var bindings must be rejected, got: {:?}",
            errs
        );
    }

    #[test]
    fn get_method_resolves_inherited_method() {
        // A method defined on a base class is resolved for a subclass, with its
        // param/return types lowered the same way.
        let src = "\
class Base:
    def describe(self, n: int) -> str:
        return \"base\"

class Derived(Base):
    pass
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let sig = ctx.get_method("Derived", "describe").expect("inherited method must be found");
        assert_eq!(sig.params, vec![("n".to_string(), Ty::Int)]);
        assert_eq!(sig.ret, Ty::Str);
    }

    // =========================================================================
    // Category E — Ty Display (surface-syntax rendering)
    // =========================================================================

    #[test]
    fn display_primitives() {
        assert_eq!(format!("{}", Ty::Int),   "int");
        assert_eq!(format!("{}", Ty::Float), "float");
        assert_eq!(format!("{}", Ty::Bool),  "bool");
        assert_eq!(format!("{}", Ty::Str),   "str");
        assert_eq!(format!("{}", Ty::Unit),  "None");
        assert_eq!(format!("{}", Ty::NoneVal), "None");
        assert_eq!(format!("{}", Ty::File),    "file");
        assert_eq!(format!("{}", Ty::Unknown), "unknown");
    }

    #[test]
    fn display_list_int() {
        assert_eq!(format!("{}", Ty::List(Box::new(Ty::Int))), "list[int]");
    }

    #[test]
    fn display_dict_str_animal() {
        let ty = Ty::Dict(
            Box::new(Ty::Str),
            Box::new(Ty::Class("Animal".to_string(), vec![])),
        );
        assert_eq!(format!("{}", ty), "dict[str, Animal]");
    }

    #[test]
    fn display_option_int() {
        assert_eq!(format!("{}", Ty::Option(Box::new(Ty::Int))), "int | None");
    }

    #[test]
    fn display_tuple_int_str() {
        let ty = Ty::Tuple(vec![Ty::Int, Ty::Str]);
        assert_eq!(format!("{}", ty), "tuple[int, str]");
    }

    // ---- First-class functions (Increment 1) ------------------------------

    #[test]
    fn display_func_callable() {
        // Ty::Func renders as the source `Callable[[args], ret]` form.
        let ty = Ty::Func(vec![Ty::Int], Box::new(Ty::Int));
        assert_eq!(format!("{}", ty), "Callable[[int], int]");
        let two = Ty::Func(vec![Ty::Int, Ty::Str], Box::new(Ty::Bool));
        assert_eq!(format!("{}", two), "Callable[[int, str], bool]");
        let nullary = Ty::Func(vec![], Box::new(Ty::Unit));
        assert_eq!(format!("{}", nullary), "Callable[[], None]");
    }

    #[test]
    fn from_type_expr_callable() {
        // `Callable[[int], int]` lowers to Ty::Func([Int], Int).
        let te = TypeExpr::Func(
            vec![TypeExpr::Named("int".into())],
            Box::new(TypeExpr::Named("int".into())),
        );
        let ty = Ty::from_type_expr(&te, Span::DUMMY).expect("Callable lowers");
        assert_eq!(ty, Ty::Func(vec![Ty::Int], Box::new(Ty::Int)));
    }

    #[test]
    fn func_name_used_as_value_infers_func_ty() {
        // A bare reference to a top-level function name (used as a VALUE) infers
        // to its first-class Ty::Func type, NOT its return type.
        let src = "\
def inc(x: int) -> int:
    return x + 1

def main() -> None:
    g: Callable[[int], int] = inc
    print(g(2))
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let locals = HashMap::new();
        let ty = infer_expr_ty(&Expr::Ident("inc".into(), Span::DUMMY), &locals, &ctx);
        assert_eq!(ty, Ty::Func(vec![Ty::Int], Box::new(Ty::Int)));
    }

    #[test]
    fn lambda_infers_func_ty() {
        // A lambda value infers to Ty::Func with Unknown-typed params.
        let ctx = TyCtx::new();
        let mut env = make_env(&ctx);
        // lambda x: x  ->  Callable[[unknown], unknown]
        let lam = Expr::Lambda {
            params: vec![("x".into(), TypeExpr::Named("Any".into()))],
            body: Box::new(Expr::Ident("x".into(), Span::DUMMY)),
            span: Span::DUMMY,
        };
        let ty = check_expr(&lam, &mut env).expect("lambda checks");
        assert_eq!(ty, Ty::Func(vec![Ty::Unknown], Box::new(Ty::Unknown)));
    }

    #[test]
    fn higher_order_module_typechecks() {
        // The full Increment-1 acceptance shape must type-check cleanly: a
        // Callable param, a Callable return with a capturing lambda, a
        // dict[str, Callable], and calls of function values.
        let src = "\
def inc(x: int) -> int:
    return x + 1

def apply_to_all(f: Callable[[int], int], xs: list[int]) -> list[int]:
    out: list[int] = []
    for x in xs:
        out.append(f(x))
    return out

def make_adder(n: int) -> Callable[[int], int]:
    return lambda x: x + n

def main() -> None:
    nums: list[int] = [1, 2, 3]
    print(apply_to_all(inc, nums))
    add5: Callable[[int], int] = make_adder(5)
    print(add5(10))
    ops: dict[str, Callable[[int], int]] = {\"inc\": inc}
    print(ops[\"inc\"](7))
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(errs.is_empty(), "first-class fn module must type-check, got: {:?}", errs);
    }

    #[test]
    fn func_value_call_arity_mismatch_rejected() {
        // Calling a Callable-typed value with the wrong argument count is an
        // honest typeck error (not a deferred rustc failure).
        let src = "\
def apply(f: Callable[[int], int]) -> int:
    return f(1, 2)
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(!errs.is_empty(), "wrong-arity call of a function value must error");
    }

    #[test]
    fn call_noncallable_local_rejected() {
        // HIGH-2: calling a value of a KNOWN non-callable type is an honest error.
        let src = "\
def main() -> None:
    x: int = 5
    print(x(3))
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(!errs.is_empty(), "calling an int must be a typeck error");
    }

    #[test]
    fn call_noncallable_index_rejected() {
        // HIGH-2: calling an indexed non-callable (`xs[0](3)`) is an honest error.
        let src = "\
def main() -> None:
    xs: list[int] = [1, 2]
    print(xs[0](3))
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(!errs.is_empty(), "calling an int element must be a typeck error");
    }

    #[test]
    fn method_call_returning_str_not_rejected_as_noncallable() {
        // HIGH-2 guard against over-rejection: a method call whose receiver method
        // returns str/None must NOT be flagged "not callable" (the Attr callee is
        // a method invocation, not a value-call).
        let src = "\
class Animal:
    name: str
    def speak(self) -> str:
        return self.name

def main() -> None:
    a: Animal = Animal()
    print(a.speak())
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(errs.is_empty(), "method call returning str must type-check, got: {:?}", errs);
    }

    #[test]
    fn set_of_callable_rejected() {
        // BLOCKER-3: a function value is not hashable (Rc<dyn Fn> is not Eq/Hash),
        // so `set[Callable[..]]` is an honest typeck error — like `set[float]`.
        let src = "\
def inc(x: int) -> int:
    return x + 1

def main() -> None:
    s: set[Callable[[int], int]] = {inc}
    print(len(s))
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(!errs.is_empty(), "set[Callable] must be rejected (functions are not hashable)");
    }

    #[test]
    fn dict_callable_key_rejected_value_ok() {
        // A Callable dict KEY is rejected (non-hashable); a Callable dict VALUE is fine.
        let bad = "\
def inc(x: int) -> int:
    return x + 1

def main() -> None:
    d: dict[Callable[[int], int], int] = {}
    print(len(d))
";
        let m = crate::parser::parse(bad).expect("parse");
        let ctx = ctx_from_module(&m);
        assert!(!check_all(&m, &ctx).is_empty(), "Callable dict key must be rejected");

        let ok = "\
def inc(x: int) -> int:
    return x + 1

def main() -> None:
    d: dict[str, Callable[[int], int]] = {\"inc\": inc}
    print(d[\"inc\"](2))
";
        let m2 = crate::parser::parse(ok).expect("parse");
        let ctx2 = ctx_from_module(&m2);
        assert!(check_all(&m2, &ctx2).is_empty(), "Callable dict value must type-check");
    }

    #[test]
    fn is_noncallable_ty_classification() {
        // Func/Unknown/Class are permissive (callable or escape-hatch); everything
        // else is definitively non-callable.
        assert!(!is_noncallable_ty(&Ty::Func(vec![], Box::new(Ty::Unit))));
        assert!(!is_noncallable_ty(&Ty::Unknown));
        assert!(!is_noncallable_ty(&Ty::Class("Foo".into(), vec![])));
        for t in [Ty::Int, Ty::Float, Ty::Bool, Ty::Str, Ty::Unit,
                  Ty::List(Box::new(Ty::Int)), Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int))] {
            assert!(is_noncallable_ty(&t), "{} must be non-callable", t);
        }
    }

    #[test]
    fn display_nested_list_dict() {
        // list[dict[str, Animal]]
        let ty = Ty::List(Box::new(Ty::Dict(
            Box::new(Ty::Str),
            Box::new(Ty::Class("Animal".to_string(), vec![])),
        )));
        assert_eq!(format!("{}", ty), "list[dict[str, Animal]]");
    }

    #[test]
    fn display_list_option_int() {
        // list[int | None]
        let ty = Ty::List(Box::new(Ty::Option(Box::new(Ty::Int))));
        assert_eq!(format!("{}", ty), "list[int | None]");
    }

    #[test]
    fn display_class_name() {
        assert_eq!(format!("{}", Ty::Class("Dog".to_string(), vec![])), "Dog");
    }

    // ── check_all: collect-all diagnostics (EPIC-LSP L4) ──────────────────────

    #[test]
    fn check_all_collects_two_function_errors_in_order() {
        // Two top-level functions, each with a distinct type error. `check_all`
        // must collect BOTH (unlike `check_bodies`, which stops at the first),
        // ordered top-to-bottom by source position.
        let src = "\
def f() -> None:
    a: int = \"s\"

def g() -> None:
    b: int = \"t\"
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);

        // check_bodies stays fail-fast: exactly one error.
        assert!(check_bodies(&m, &ctx).is_err(), "check_bodies must still fail-fast");

        let errs = check_all(&m, &ctx);
        assert_eq!(errs.len(), 2, "expected 2 collected errors, got: {:?}", errs);
        // Ordered by span line.
        let l0 = error_span(&errs[0]).line;
        let l1 = error_span(&errs[1]).line;
        assert!(l0 < l1, "errors must be ordered by line, got {l0} then {l1}");
    }

    #[test]
    fn check_all_collects_two_method_errors() {
        let src = "\
class C:
    def m1(self) -> None:
        a: int = \"s\"
    def m2(self) -> None:
        b: int = \"t\"
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert_eq!(errs.len(), 2, "expected 2 collected method errors, got: {:?}", errs);
        let l0 = error_span(&errs[0]).line;
        let l1 = error_span(&errs[1]).line;
        assert!(l0 < l1, "method errors must be ordered by line, got {l0} then {l1}");
    }

    #[test]
    fn check_all_clean_module_is_empty() {
        let src = "\
def f(x: int) -> int:
    return x + 1

def g(y: int) -> int:
    return y * 2
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(errs.is_empty(), "clean module must yield no errors, got: {:?}", errs);
    }

    #[test]
    fn check_all_single_error_yields_one() {
        let src = "\
def f() -> None:
    a: int = \"s\"

def g(y: int) -> int:
    return y * 2
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert_eq!(errs.len(), 1, "expected exactly 1 error, got: {:?}", errs);
    }

    // =========================================================================
    // Category — @extern (Rust-FFI binding) validation
    // =========================================================================

    #[test]
    fn validate_decorators_accepts_extern() {
        // The whitelist must admit `@extern` (the body/typing of an @extern fn
        // are validated separately by validate_extern_func).
        assert!(validate_decorators(&["extern".to_string()], Span::DUMMY).is_ok());
    }

    #[test]
    fn validate_decorators_still_rejects_unknown() {
        // Regression guard: a non-whitelisted decorator is still rejected.
        assert!(validate_decorators(&["bogus".to_string()], Span::DUMMY).is_err());
    }

    #[test]
    fn extern_good_binding_type_checks() {
        // A well-formed @extern (single string-literal body, fully-typed sig)
        // passes typeck, and its declared signature lets a normal call site
        // type-check through the ordinary path (no special-casing).
        let src = "\
@extern
def shout(s: str) -> str:
    \"{s}.to_uppercase()\"

def main() -> None:
    print(shout(\"hi\"))
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(errs.is_empty(), "well-formed @extern + call site must type-check, got: {:?}", errs);
    }

    #[test]
    fn extern_non_string_body_rejected() {
        // An @extern whose body is a normal statement (not a template string)
        // is an honest typeck error.
        let src = "\
@extern
def bad(s: str) -> str:
    return s
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(!errs.is_empty(), "@extern with a non-template body must be rejected");
        assert!(
            errs.iter().any(|e| matches!(e, Error::Type { msg, .. } if msg.contains("string literal"))),
            "error must name the single-template-string requirement, got: {:?}", errs
        );
    }

    /// MEDIUM fix: `@crate` on a function WITHOUT `@extern` is a typeck error
    /// (it would otherwise pull the program onto the Cargo build path while the
    /// crate is never used). A normal pyrst body is present, so this is purely the
    /// decorator-pairing check firing.
    #[test]
    fn crate_without_extern_rejected() {
        let src = "\
@crate(\"regex\", \"1\")
def helper(x: int) -> int:
    return x + 1

def main() -> None:
    print(helper(41))
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(
            errs.iter().any(|e| matches!(e, Error::Type { msg, .. }
                if msg.contains("`@crate` can only be used on `@extern`"))),
            "`@crate` without `@extern` must be rejected, got: {:?}", errs
        );
    }

    /// The legitimate pairing — `@crate` stacked over `@extern` — still
    /// type-checks (the MEDIUM-fix guard must not break the real `re` shape).
    #[test]
    fn crate_with_extern_type_checks() {
        let src = "\
@crate(\"regex\", \"1\")
@extern
def is_match(pattern: str, text: str) -> bool:
    \"regex::Regex::new(&{pattern}).unwrap().is_match(&{text})\"

def main() -> None:
    print(is_match(\"[0-9]+\", \"abc123\"))
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(errs.is_empty(), "@crate + @extern must type-check, got: {:?}", errs);
    }

    #[test]
    fn extern_multi_statement_body_rejected() {
        // The body must be EXACTLY ONE statement; a leading docstring + template
        // (two string-literal statements) is still rejected.
        let src = "\
@extern
def bad(s: str) -> str:
    \"doc\"
    \"{s}.to_uppercase()\"
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(!errs.is_empty(), "@extern with a multi-statement body must be rejected");
    }

    #[test]
    fn extern_method_rejected() {
        // @extern is for top-level functions only; on a method it is rejected.
        let src = "\
class C:
    x: int
    @extern
    def m(self) -> int:
        \"{self}.x\"
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(!errs.is_empty(), "@extern on a method must be rejected");
        assert!(
            errs.iter().any(|e| matches!(e, Error::Type { msg, .. } if msg.contains("not supported on a method"))),
            "error must name the method restriction, got: {:?}", errs
        );
    }

    #[test]
    fn extern_by_ref_param_rejected() {
        // A by-reference (`Mut[T]`) param is out of Phase-1 @extern scope.
        let src = "\
@extern
def bump(n: Mut[int]) -> int:
    \"{n} + 1\"
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(!errs.is_empty(), "@extern with a Mut[T] param must be rejected");
    }

    #[test]
    fn extern_union_param_rejected() {
        // A param whose annotation lowers to Ty::Unknown (a multi-arm Union like
        // `int | str`) must be rejected — @extern requires fully-typed params, since
        // codegen can't infer the Rust-side boundary for an unknown type.
        let src = "\
@extern
def bad(x: int | str) -> str:
    \"{x}.to_string()\"
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(!errs.is_empty(), "@extern with a Union-typed param must be rejected");
    }

    #[test]
    fn qualified_module_call_types_as_function_return() {
        // `os.basename("/x/y/z.txt")` resolves via module_funcs to the flat
        // `basename` signature and types as its return type (str), NOT Unknown.
        let ctx = ctx_with_os_module();
        let mut env = make_env(&ctx);
        let call = method_call(ident("os"), "basename", vec![str_lit("/x/y/z.txt")]);
        assert_eq!(check_expr(&call, &mut env).unwrap(), Ty::Str);
    }

    #[test]
    fn qualified_module_call_inference_oracle_agrees() {
        // The inference oracle (infer_expr_ty) must agree with check_expr: a
        // qualified module call infers the function's declared return type.
        let ctx = ctx_with_os_module();
        let locals = std::collections::HashMap::new();
        let call = method_call(ident("os"), "getenv", vec![str_lit("K"), str_lit("D")]);
        assert_eq!(infer_expr_ty(&call, &locals, &ctx), Ty::Str);
    }

    #[test]
    fn qualified_call_to_unknown_module_function_is_honest_error() {
        // `os.nope(...)` — os IS a tracked module but defines no `nope`. This is a
        // hard typeck error, not a silently-Unknown call deferred to rustc.
        let ctx = ctx_with_os_module();
        let mut env = make_env(&ctx);
        let call = method_call(ident("os"), "nope", vec![str_lit("x")]);
        assert_type_err(check_expr(&call, &mut env), "has no function");
    }

    #[test]
    fn qualified_module_call_checks_arity() {
        // `os.basename()` with no args is rejected (basename takes 1 required arg).
        let ctx = ctx_with_os_module();
        let mut env = make_env(&ctx);
        let call = method_call(ident("os"), "basename", vec![]);
        assert_type_err(check_expr(&call, &mut env), "argument(s)");
    }

    #[test]
    fn qualified_module_call_checks_arg_types() {
        // `os.basename(42)` — basename expects str, gets int → honest error.
        let ctx = ctx_with_os_module();
        let mut env = make_env(&ctx);
        let call = method_call(ident("os"), "basename", vec![int_lit(42)]);
        assert_type_err(check_expr(&call, &mut env), "expected str");
    }

    #[test]
    fn math_qualified_call_resolves_via_module_path() {
        // `math` is now a REAL embedded module (`lib/math.pyrs`): its @extern
        // `sqrt` is merged FLAT into `ctx.funcs` and indexed in
        // `module_funcs["math"]`, so `math.sqrt(x)` types as `sqrt`'s declared
        // return (float) through the GENERAL qualified-module path — no hardcoded
        // math arm. (Models what the resolver produces for the math module.)
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("sqrt".into(), FuncSig {
            params: vec![("x".into(), Ty::Float)],
            param_defaults: vec![None],
            param_by_ref: vec![],
            ret: Ty::Float,
        });
        ctx.module_funcs.insert("math".into(), vec!["sqrt".into()]);
        let locals = std::collections::HashMap::new();
        let call = method_call(ident("math"), "sqrt", vec![float_lit(16.0)]);
        assert_eq!(infer_expr_ty(&call, &locals, &ctx), Ty::Float);
    }

    #[test]
    fn math_constant_resolves_via_module_consts() {
        // `math.pi` (a NON-call attribute) types as float through the general
        // `module_consts` path — the former hardcoded `math.pi` typing is gone.
        let mut ctx = TyCtx::new();
        ctx.module_consts.insert("math".into(), vec![("pi".into(), Ty::Float)]);
        let locals = std::collections::HashMap::new();
        let attr = Expr::Attr {
            obj: Box::new(ident("math")),
            name: "pi".into(),
            span: Span::DUMMY,
        };
        assert_eq!(infer_expr_ty(&attr, &locals, &ctx), Ty::Float);
    }

    #[test]
    fn sum_two_arg_float_start_promotes_result_type() {
        // (card aabf4ada) The 2-arg `sum(iterable, start)` result-type oracle: a FLOAT
        // start promotes an int-element sum to Float (CPython `sum([1,2,3],1.0)` ->
        // 7.0), so the print-formatter DISPLAYS `.0`, agreeing with codegen's own
        // promotion. Int elems + int start stay Int; the 1-arg form is unchanged.
        let ctx = TyCtx::new();
        let locals = std::collections::HashMap::new();
        let ints = || Expr::List(vec![int_lit(1), int_lit(2), int_lit(3)], Span::DUMMY);
        let floats = || Expr::List(vec![float_lit(1.5), float_lit(2.5)], Span::DUMMY);
        // int elems + FLOAT start -> Float (promotion)
        assert_eq!(
            infer_expr_ty(&call_fn("sum", vec![ints(), float_lit(1.0)]), &locals, &ctx),
            Ty::Float
        );
        // float elems + int start -> Float (base is already Float)
        assert_eq!(
            infer_expr_ty(&call_fn("sum", vec![floats(), int_lit(1)]), &locals, &ctx),
            Ty::Float
        );
        // int elems + int start -> Int (no promotion)
        assert_eq!(
            infer_expr_ty(&call_fn("sum", vec![ints(), int_lit(10)]), &locals, &ctx),
            Ty::Int
        );
        // 1-arg no-start form is unchanged -> Int
        assert_eq!(
            infer_expr_ty(&call_fn("sum", vec![ints()]), &locals, &ctx),
            Ty::Int
        );
    }

    /// BLOCKER-1 (honest-errors): a module constant whose NAME duplicates a
    /// function is rejected at `check` (constants and functions share a flat
    /// namespace; otherwise the call would route to the mangled const and
    /// miscompile as rustc E0618). The single check at the const site catches the
    /// pair regardless of source order.
    #[test]
    fn module_const_clashing_with_function_is_rejected() {
        let src = "\
my_fn: float = 3.14

def my_fn() -> float:
    return 2.71
";
        let m = crate::parser::parse(src).expect("parse");
        let ctx = ctx_from_module(&m);
        let errs = check_all(&m, &ctx);
        assert!(
            errs.iter().any(|e| matches!(e, Error::Type { msg, .. } if msg.contains("clashes with a function"))),
            "a const named like a function must be an honest error; got: {:?}", errs
        );
    }

    /// BLOCKER-2 (honest-errors): an UNKNOWN attribute on a KNOWN embedded module
    /// (non-call) is rejected at `check` (`math.inf` — not a pyrst constant —
    /// otherwise emits a bare `math` and fails rustc E0425), while a REAL const
    /// (`math.pi`) still type-checks.
    #[test]
    fn unknown_attr_on_known_module_is_rejected() {
        let mut ctx = TyCtx::new();
        ctx.module_funcs.insert("math".into(), vec!["sqrt".into()]);
        ctx.module_consts.insert("math".into(), vec![("pi".into(), Ty::Float)]);
        // Unknown attribute `inf` on the known `math` module -> honest error.
        let mut env = make_env(&ctx);
        let bad = Expr::Attr { obj: Box::new(ident("math")), name: "inf".into(), span: Span::DUMMY };
        assert_type_err(check_expr(&bad, &mut env), "has no attribute `inf`");
        // A real const still resolves.
        let mut env2 = make_env(&ctx);
        let good = Expr::Attr { obj: Box::new(ident("math")), name: "pi".into(), span: Span::DUMMY };
        assert_eq!(check_expr(&good, &mut env2).expect("math.pi ok"), Ty::Float);
    }

    #[test]
    fn bdr_return_value_returns() {
        assert!(block_definitely_returns(&[ret_val()]));
    }

    #[test]
    fn bdr_bare_return_returns() {
        // A bare `return` still terminates the path (it is a separate honest
        // error in a non-unit fn, but for control-flow it does not fall through).
        assert!(block_definitely_returns(&[Stmt::Return(None, Span::DUMMY)]));
    }

    #[test]
    fn bdr_raise_diverges() {
        assert!(block_definitely_returns(&[raise_stmt()]));
    }

    #[test]
    fn bdr_pass_does_not_return() {
        assert!(!block_definitely_returns(&[Stmt::Pass(Span::DUMMY)]));
    }

    #[test]
    fn bdr_empty_block_does_not_return() {
        assert!(!block_definitely_returns(&[]));
    }

    #[test]
    fn bdr_driven_by_last_statement() {
        // An expr followed by a return -> returns; a return followed by pass ->
        // last stmt is pass -> does not (last-statement-driven, as documented).
        assert!(block_definitely_returns(&[Stmt::Pass(Span::DUMMY), ret_val()]));
        assert!(!block_definitely_returns(&[ret_val(), Stmt::Pass(Span::DUMMY)]));
    }

    #[test]
    fn bdr_if_with_else_all_branches_return() {
        let s = Stmt::If {
            cond: bool_lit(true),
            then: vec![ret_val()],
            elifs: vec![],
            else_: Some(vec![ret_val()]),
            span: Span::DUMMY,
        };
        assert!(block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_if_no_else_does_not_return() {
        let s = Stmt::If {
            cond: bool_lit(true),
            then: vec![ret_val()],
            elifs: vec![],
            else_: None,
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_if_else_one_branch_falls_through() {
        // then returns, else is a `pass` -> not all paths return.
        let s = Stmt::If {
            cond: bool_lit(true),
            then: vec![ret_val()],
            elifs: vec![],
            else_: Some(vec![Stmt::Pass(Span::DUMMY)]),
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_if_elif_else_all_return() {
        let s = Stmt::If {
            cond: bool_lit(true),
            then: vec![ret_val()],
            elifs: vec![(bool_lit(false), vec![ret_val()])],
            else_: Some(vec![ret_val()]),
            span: Span::DUMMY,
        };
        assert!(block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_if_elif_branch_falls_through() {
        // The elif body does NOT return -> the whole if does not.
        let s = Stmt::If {
            cond: bool_lit(true),
            then: vec![ret_val()],
            elifs: vec![(bool_lit(false), vec![Stmt::Pass(Span::DUMMY)])],
            else_: Some(vec![ret_val()]),
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_while_true_no_break_diverges() {
        let s = Stmt::While {
            cond: bool_lit(true),
            body: vec![ret_val()],
            span: Span::DUMMY,
        };
        assert!(block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_while_true_with_break_does_not_diverge() {
        // A reachable `break` means the loop can exit -> not guaranteed to return.
        let s = Stmt::While {
            cond: bool_lit(true),
            body: vec![Stmt::Break(Span::DUMMY)],
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_while_true_break_inside_if_does_not_diverge() {
        // A `break` nested in an `if` still escapes this loop.
        let s = Stmt::While {
            cond: bool_lit(true),
            body: vec![Stmt::If {
                cond: bool_lit(true),
                then: vec![Stmt::Break(Span::DUMMY)],
                elifs: vec![],
                else_: None,
                span: Span::DUMMY,
            }],
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_while_true_break_in_inner_loop_still_diverges() {
        // A `break` in an INNER loop targets that inner loop, not this one, so
        // the outer `while True` is still infinite.
        let s = Stmt::While {
            cond: bool_lit(true),
            body: vec![Stmt::While {
                cond: bool_lit(true),
                body: vec![Stmt::Break(Span::DUMMY)],
                span: Span::DUMMY,
            }],
            span: Span::DUMMY,
        };
        assert!(block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_while_nonliteral_cond_does_not_diverge() {
        // `while <other>:` may run zero times / exit normally.
        let s = Stmt::While {
            cond: ident("c"),
            body: vec![ret_val()],
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_while_false_does_not_diverge() {
        let s = Stmt::While {
            cond: bool_lit(false),
            body: vec![ret_val()],
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_for_does_not_diverge() {
        let s = Stmt::For {
            targets: vec!["x".into()],
            iter: call_fn("range", vec![int_lit(3)]),
            body: vec![ret_val()],
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_match_exhaustive_all_arms_return() {
        let s = Stmt::Match {
            subject: ident("x"),
            arms: vec![
                MatchArm { pattern: MatchPattern::Literal(int_lit(0)), guard: None, body: vec![ret_val()] },
                MatchArm { pattern: MatchPattern::Wildcard, guard: None, body: vec![ret_val()] },
            ],
            span: Span::DUMMY,
        };
        assert!(block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_match_without_wildcard_does_not_return() {
        // No wildcard/capture arm -> exhaustiveness unknown -> conservative false.
        let s = Stmt::Match {
            subject: ident("x"),
            arms: vec![
                MatchArm { pattern: MatchPattern::Literal(int_lit(0)), guard: None, body: vec![ret_val()] },
                MatchArm { pattern: MatchPattern::Literal(int_lit(1)), guard: None, body: vec![ret_val()] },
            ],
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_match_wildcard_arm_falls_through() {
        // Exhaustive but one arm body does not return -> whole match does not.
        let s = Stmt::Match {
            subject: ident("x"),
            arms: vec![
                MatchArm { pattern: MatchPattern::Literal(int_lit(0)), guard: None, body: vec![ret_val()] },
                MatchArm { pattern: MatchPattern::Wildcard, guard: None, body: vec![Stmt::Pass(Span::DUMMY)] },
            ],
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_match_guarded_wildcard_is_not_total() {
        // A wildcard arm with a guard may not match -> not exhaustive.
        let s = Stmt::Match {
            subject: ident("x"),
            arms: vec![
                MatchArm {
                    pattern: MatchPattern::Wildcard,
                    guard: Some(bool_lit(true)),
                    body: vec![ret_val()],
                },
            ],
            span: Span::DUMMY,
        };
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_body_and_handler_return() {
        let s = try_stmt(vec![ret_val()], vec![handler(true)], None, None);
        assert!(block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_handler_does_not_return_falls_through() {
        let s = try_stmt(vec![ret_val()], vec![handler(false)], None, None);
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_body_falls_through_no_else() {
        let s = try_stmt(vec![Stmt::Pass(Span::DUMMY)], vec![handler(true)], None, None);
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_else_returns_covers_normal_path() {
        let s = try_stmt(
            vec![Stmt::Pass(Span::DUMMY)],
            vec![handler(true)],
            Some(vec![ret_val()]),
            None,
        );
        assert!(block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_else_does_not_return_falls_through() {
        let s = try_stmt(
            vec![Stmt::Pass(Span::DUMMY)],
            vec![handler(true)],
            Some(vec![Stmt::Pass(Span::DUMMY)]),
            None,
        );
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_finally_returns_covers_everything() {
        let s = try_stmt(
            vec![Stmt::Pass(Span::DUMMY)],
            vec![handler(false)],
            None,
            Some(vec![ret_val()]),
        );
        assert!(block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_no_handlers_no_finally_does_not_return() {
        let s = try_stmt(vec![Stmt::Pass(Span::DUMMY)], vec![], None, None);
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_no_handlers_returning_finally_returns() {
        let s = try_stmt(vec![Stmt::Pass(Span::DUMMY)], vec![], None, Some(vec![ret_val()]));
        assert!(block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_finally_no_except_returning_body_returns() {
        // `try: return v finally: <non-returning>` (NO except handler): the body's
        // return always runs, finally runs, then the return takes effect. The
        // empty-handler case is vacuously all-return, so this must be accepted.
        let s = try_stmt(
            vec![ret_val()],
            vec![],
            None,
            Some(vec![Stmt::Expr(call_fn("print", vec![str_lit("c")]))]),
        );
        assert!(block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_finally_no_except_nonreturning_body_falls_through() {
        // `try: <falls through> finally: <non-returning>` with no handler still
        // falls off the end -> stays an honest error (soundness boundary).
        let s = try_stmt(
            vec![Stmt::Pass(Span::DUMMY)],
            vec![],
            None,
            Some(vec![Stmt::Expr(call_fn("print", vec![str_lit("c")]))]),
        );
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn bdr_try_multiple_handlers_one_falls_through() {
        let s = try_stmt(
            vec![ret_val()],
            vec![handler(true), handler(false)],
            None,
            None,
        );
        assert!(!block_definitely_returns(&[s]));
    }

    #[test]
    fn gate_rejects_pass_body_nonunit() {
        let r = check_src("def f() -> int:\n    pass\n");
        assert_type_err_unit(r, "may reach the end without returning a value");
    }

    #[test]
    fn gate_rejects_if_no_else_nonunit() {
        let r = check_src("def f(c: bool) -> int:\n    if c:\n        return 1\n");
        assert_type_err_unit(r, "may reach the end without returning a value");
    }

    #[test]
    fn gate_rejects_list_return_pass_body() {
        let r = check_src("def f() -> list[int]:\n    pass\n");
        assert_type_err_unit(r, "may reach the end without returning a value");
    }

    #[test]
    fn gate_accepts_if_else_both_return() {
        let r = check_src("def f(c: bool) -> int:\n    if c:\n        return 1\n    else:\n        return 2\n");
        assert!(r.is_ok(), "if/else both-return must pass: {:?}", r);
    }

    #[test]
    fn gate_accepts_raise_only() {
        let r = check_src("def f() -> int:\n    raise ValueError(\"x\")\n");
        assert!(r.is_ok(), "raise-only must pass: {:?}", r);
    }

    #[test]
    fn gate_accepts_trailing_return_after_if() {
        let r = check_src("def f(c: bool) -> int:\n    if c:\n        return 1\n    return 2\n");
        assert!(r.is_ok(), "trailing return must pass: {:?}", r);
    }

    #[test]
    fn gate_accepts_none_pass() {
        // `-> None` (Unit) functions implicitly return () — exempt.
        let r = check_src("def f() -> None:\n    pass\n");
        assert!(r.is_ok(), "-> None pass must pass: {:?}", r);
    }

    #[test]
    fn gate_accepts_while_true_return() {
        let r = check_src("def f() -> int:\n    while True:\n        return 1\n");
        assert!(r.is_ok(), "while True: return must pass: {:?}", r);
    }

    #[test]
    fn gate_exempts_generator_falling_off_end() {
        // A generator (yield + Iterator[T]) implicitly returns its collected Vec;
        // falling off the end is correct, so the gate must NOT fire.
        let r = check_src("def g(n: int) -> Iterator[int]:\n    i: int = 0\n    while i < n:\n        yield i\n        i = i + 1\n");
        assert!(r.is_ok(), "generator must be exempt from missing-return gate: {:?}", r);
    }

    #[test]
    fn gate_rejects_iterator_return_without_yield() {
        // (LAZY-GEN V1-d) A function declared `-> Iterator[T]` with NO `yield` is
        // now an honest error: since `Iterator[T]` is a DISTINCT type (no longer
        // `≡ list[T]`), a yield-less body claiming to return an iterator is the last
        // vestige of the old conflation. Fix: declare `-> list[T]`, or add a `yield`.
        let r = check_src("def empty() -> Iterator[int]:\n    return []\n");
        assert_type_err_unit(r, "must contain a `yield`");
    }

    #[test]
    fn gate_applies_to_methods() {
        let r = check_src(
            "class C:\n    def __init__(self) -> None:\n        pass\n    def m(self) -> int:\n        pass\n",
        );
        assert_type_err_unit(r, "may reach the end without returning a value");
    }

    #[test]
    fn gate_accepts_method_returning_on_all_paths() {
        let r = check_src(
            "class C:\n    def __init__(self) -> None:\n        pass\n    def m(self, c: bool) -> int:\n        if c:\n            return 1\n        else:\n            return 2\n",
        );
        assert!(r.is_ok(), "method returning on all paths must pass: {:?}", r);
    }

    #[test]
    fn gate_accepts_match_exhaustive_all_return() {
        let r = check_src(
            "def f(x: int) -> int:\n    match x:\n        case 0:\n            return 10\n        case _:\n            return 20\n",
        );
        assert!(r.is_ok(), "exhaustive match returning on all arms must pass: {:?}", r);
    }

    // --- try/except all-paths-return acceptance (card 57274b36, Part 2) ---

    #[test]
    fn gate_accepts_try_body_and_handler_return() {
        let r = check_src(
            "def f(x: int) -> int:\n    try:\n        return 10 // x\n    except ZeroDivisionError:\n        return 0\n",
        );
        assert!(r.is_ok(), "try body+handler both-return must pass: {:?}", r);
    }

    #[test]
    fn gate_accepts_try_handlers_and_else_return() {
        let r = check_src(
            "def f(x: int) -> int:\n    try:\n        y: int = 100 // x\n    except ZeroDivisionError:\n        return -1\n    else:\n        return y\n",
        );
        assert!(r.is_ok(), "try handlers+else return must pass: {:?}", r);
    }

    #[test]
    fn gate_accepts_try_finally_returns() {
        let r = check_src(
            "def f() -> int:\n    try:\n        print(\"x\")\n    finally:\n        return 5\n",
        );
        assert!(r.is_ok(), "try finally-return must pass: {:?}", r);
    }

    #[test]
    fn gate_rejects_try_handler_not_returning() {
        let r = check_src(
            "def f(x: int) -> int:\n    try:\n        return 1\n    except ValueError:\n        print(\"oops\")\n",
        );
        assert_type_err_unit(r, "may reach the end without returning a value");
    }

    #[test]
    fn gate_rejects_try_body_falls_through_no_else() {
        let r = check_src(
            "def f() -> int:\n    try:\n        print(\"x\")\n    except ValueError:\n        return 0\n",
        );
        assert_type_err_unit(r, "may reach the end without returning a value");
    }

    #[test]
    fn transitive_one_hop_propagates_bound() {
        // `use_it` forwards its `T` into `dedup`, which needs PartialEq.
        let src = "def dedup[T](a: T, b: T) -> bool:\n    return a == b\n\ndef use_it[T](a: T, b: T) -> bool:\n    return dedup(a, b)\n";
        let b = bounds_of_named_func(src, "use_it");
        assert!(b["T"].contains(&TypeVarBound::PartialEq),
            "use_it.T must inherit PartialEq from dedup, got {:?}", b["T"]);
    }

    #[test]
    fn transitive_multi_hop_chain_propagates() {
        // top -> mid -> base; base needs PartialOrd -> all three carry it.
        let src = "def base[T](a: T, b: T) -> bool:\n    return a > b\n\ndef mid[T](a: T, b: T) -> bool:\n    return base(a, b)\n\ndef top[T](a: T, b: T) -> bool:\n    return mid(a, b)\n";
        for name in ["base", "mid", "top"] {
            let b = bounds_of_named_func(src, name);
            assert!(b["T"].contains(&TypeVarBound::PartialOrd),
                "{}.T must carry PartialOrd, got {:?}", name, b["T"]);
        }
    }

    #[test]
    fn transitive_nonbounded_callee_adds_no_bound() {
        // `wrap` forwards into the identity `ident`, which needs only Clone.
        let src = "def ident[T](x: T) -> T:\n    return x\n\ndef wrap[T](x: T) -> T:\n    return ident(x)\n";
        let b = bounds_of_named_func(src, "wrap");
        assert_eq!(b["T"].iter().copied().collect::<Vec<_>>(), vec![TypeVarBound::Clone],
            "wrap.T must stay Clone-only, got {:?}", b["T"]);
    }

    #[test]
    fn transitive_self_recursion_terminates() {
        // A generic recursing on itself + a leaf `==` must converge to PartialEq.
        let src = "def rec[T](a: T, b: T, n: int) -> bool:\n    if n <= 0:\n        return a == b\n    return rec(a, b, n - 1)\n";
        let b = bounds_of_named_func(src, "rec");
        assert!(b["T"].contains(&TypeVarBound::PartialEq));
        assert!(b["T"].contains(&TypeVarBound::Clone));
    }

    #[test]
    fn transitive_mutual_recursion_converges() {
        // ping <-> pong cycle; the `==` in ping must propagate to BOTH.
        let src = "def ping[T](a: T, b: T, n: int) -> bool:\n    if n <= 0:\n        return a == b\n    return pong(a, b, n - 1)\n\ndef pong[T](a: T, b: T, n: int) -> bool:\n    return ping(a, b, n - 1)\n";
        assert!(bounds_of_named_func(src, "ping")["T"].contains(&TypeVarBound::PartialEq));
        assert!(bounds_of_named_func(src, "pong")["T"].contains(&TypeVarBound::PartialEq));
    }

    #[test]
    fn transitive_container_element_propagation() {
        // `count_unique` passes list[T] into `to_set` (needs Hash+Eq on element).
        let src = "def to_set[U](xs: list[U]) -> set[U]:\n    out: set[U] = set()\n    for x in xs:\n        out.add(x)\n    return out\n\ndef count_unique[T](xs: list[T]) -> int:\n    return len(to_set(xs))\n";
        let b = bounds_of_named_func(src, "count_unique");
        assert!(b["T"].contains(&TypeVarBound::Hash), "got {:?}", b["T"]);
        assert!(b["T"].contains(&TypeVarBound::Eq), "got {:?}", b["T"]);
    }

    #[test]
    fn transitive_repro_check_and_build_consistent() {
        // The lead's repro: BOTH check AND the inferred clause must now agree
        // (use_it carries PartialEq), so the program is build-sound.
        let src = "def dedup[T](a: T, b: T) -> bool:\n    return a == b\n\ndef use_it[T](a: T, b: T) -> bool:\n    return dedup(a, b)\n\ndef main() -> None:\n    print(use_it(1, 1))\n";
        assert!(check_src(src).is_ok(), "repro must still typecheck");
        let b = bounds_of_named_func(src, "use_it");
        assert!(b["T"].contains(&TypeVarBound::PartialEq));
    }

    #[test]
    fn binop_bound_mapping_supported_ops() {
        // Comparison -> PartialOrd; equality -> PartialEq; +,-,* -> Add/Sub/Mul.
        assert_eq!(binop_typevar_bound(BinOp::Lt), Some(TypeVarBound::PartialOrd));
        assert_eq!(binop_typevar_bound(BinOp::Ge), Some(TypeVarBound::PartialOrd));
        assert_eq!(binop_typevar_bound(BinOp::Eq), Some(TypeVarBound::PartialEq));
        assert_eq!(binop_typevar_bound(BinOp::Ne), Some(TypeVarBound::PartialEq));
        assert_eq!(binop_typevar_bound(BinOp::Add), Some(TypeVarBound::Add));
        assert_eq!(binop_typevar_bound(BinOp::Sub), Some(TypeVarBound::Sub));
        assert_eq!(binop_typevar_bound(BinOp::Mul), Some(TypeVarBound::Mul));
    }

    #[test]
    fn binop_bound_mapping_unsupported_ops_stay_none() {
        // `/ % // **`, membership, boolean, bitwise stay rejected (None).
        for op in [BinOp::Div, BinOp::Mod, BinOp::FloorDiv, BinOp::Pow,
                   BinOp::In, BinOp::NotIn, BinOp::And, BinOp::Or,
                   BinOp::BitAnd, BinOp::BitOr, BinOp::BitXor,
                   BinOp::LShift, BinOp::RShift, BinOp::Is, BinOp::IsNot] {
            assert_eq!(binop_typevar_bound(op), None, "op {:?} must stay unsupported", op);
        }
    }

    #[test]
    fn rust_bound_renders_output_clause_for_arithmetic() {
        assert_eq!(TypeVarBound::Add.rust_bound("T"), "std::ops::Add<Output = T>");
        assert_eq!(TypeVarBound::Sub.rust_bound("U"), "std::ops::Sub<Output = U>");
        assert_eq!(TypeVarBound::PartialOrd.rust_bound("T"), "PartialOrd");
        assert_eq!(TypeVarBound::Display.rust_bound("T"), "std::fmt::Display");
        assert_eq!(TypeVarBound::Hash.rust_bound("T"), "std::hash::Hash");
        assert_eq!(TypeVarBound::Eq.rust_bound("T"), "std::cmp::Eq");
    }

    #[test]
    fn infers_partialord_for_comparison() {
        let b = bounds_of_first_func(
            "def maximum[T](a: T, b: T) -> T:\n    if a > b:\n        return a\n    return b\n",
        );
        let t = &b["T"];
        assert!(t.contains(&TypeVarBound::Clone));
        assert!(t.contains(&TypeVarBound::PartialOrd));
        assert!(!t.contains(&TypeVarBound::Display));
    }

    #[test]
    fn infers_add_for_arithmetic() {
        let b = bounds_of_first_func("def total[T](a: T, b: T) -> T:\n    return a + b\n");
        assert!(b["T"].contains(&TypeVarBound::Add));
    }

    #[test]
    fn infers_display_for_print_and_fstring() {
        let p = bounds_of_first_func("def show[T](x: T) -> None:\n    print(x)\n");
        assert!(p["T"].contains(&TypeVarBound::Display));
        let f = bounds_of_first_func("def label[T](x: T) -> str:\n    return f\"[{x}]\"\n");
        assert!(f["T"].contains(&TypeVarBound::Display));
    }

    #[test]
    fn infers_hash_eq_for_set_literal_and_annotation() {
        let b = bounds_of_first_func(
            "def s[T](a: T, b: T) -> set[T]:\n    x: set[T] = {a, b}\n    return x\n",
        );
        assert!(b["T"].contains(&TypeVarBound::Hash));
        assert!(b["T"].contains(&TypeVarBound::Eq));
    }

    #[test]
    fn bounds_union_across_multiple_ops() {
        // A `T` used by both `>` and `print` collects BOTH bounds.
        let b = bounds_of_first_func(
            "def f[T](a: T, b: T) -> T:\n    print(a)\n    if a > b:\n        return a\n    return b\n",
        );
        let t = &b["T"];
        assert!(t.contains(&TypeVarBound::Clone));
        assert!(t.contains(&TypeVarBound::PartialOrd));
        assert!(t.contains(&TypeVarBound::Display));
    }

    #[test]
    fn nongeneric_func_has_no_bounds() {
        let m = crate::parser::parse("def f(a: int) -> int:\n    return a + 1\n").expect("parse");
        let ctx = ctx_from_module(&m);
        let f = m.stmts.iter().find_map(|s| match s { Stmt::Func(f) => Some(f), _ => None }).unwrap();
        assert!(infer_func_typevar_bounds(f, &ctx).is_empty());
    }

    // -- v2 ACCEPTS the now-supported ops (were v1 rejections) ----------------

    #[test]
    fn v2_accepts_comparison_on_typevar() {
        assert!(check_src(
            "def m[T](a: T, b: T) -> T:\n    if a > b:\n        return a\n    return b\n",
        ).is_ok());
    }

    #[test]
    fn v2_accepts_same_typevar_arithmetic() {
        assert!(check_src("def t[T](a: T, b: T) -> T:\n    return a + b\n").is_ok());
    }

    #[test]
    fn v2_accepts_print_and_set_of_typevar() {
        assert!(check_src("def s[T](x: T) -> None:\n    print(x)\n").is_ok());
        assert!(check_src(
            "def d[T](a: T, b: T) -> set[T]:\n    return {a, b}\n",
        ).is_ok());
    }

    // -- v2 STILL REJECTS the unsupported ops (soundness preservation) --------

    #[test]
    fn v2_rejects_mixed_typevar_concrete_arithmetic() {
        // `T + concrete` has no sound result type / single-trait bound -> reject.
        assert_type_err_unit(
            check_src("def f[T](a: T) -> T:\n    return a + 1\n"),
            "this operation on a type parameter is not supported",
        );
    }

    #[test]
    fn v2_rejects_true_division_on_typevar() {
        // `/` is true float division in pyrst; Rust `Div` truncates ints -> reject.
        assert_type_err_unit(
            check_src("def f[T](a: T, b: T) -> T:\n    return a / b\n"),
            "this operation on a type parameter is not supported",
        );
    }

    #[test]
    fn v2_rejects_membership_on_typevar() {
        assert_type_err_unit(
            check_src("def f[T](a: T, b: T) -> bool:\n    return a in b\n"),
            "this operation on a type parameter is not supported",
        );
    }

    #[test]
    fn v2_accepts_membership_in_known_container() {
        // `k in d` / `k in s` / `x in xs` where the CONTAINER is a known
        // dict/set/list and the element/key is a type variable is a VALID,
        // bound-inferable op (dict/set -> Hash+Eq on the key, list -> PartialEq).
        assert!(check_src(
            "def f[K, V](d: dict[K, V], k: K) -> bool:\n    return k in d\n"
        ).is_ok());
        assert!(check_src(
            "def f[K](s: set[K], k: K) -> bool:\n    return k in s\n"
        ).is_ok());
        assert!(check_src(
            "def f[T](xs: list[T], x: T) -> bool:\n    return x in xs\n"
        ).is_ok());
        // `not in` takes the same path.
        assert!(check_src(
            "def f[K, V](d: dict[K, V], k: K) -> bool:\n    return k not in d\n"
        ).is_ok());
    }

    #[test]
    fn v2_membership_still_rejects_bare_container_typevar() {
        // The container being a BARE type variable (unknown structure) stays an
        // honest rejection — only `dict`/`set`/`list` containers are relaxed.
        assert!(check_src("def f[T](x: int, t: T) -> bool:\n    return x in t\n").is_err());
    }

    #[test]
    fn callable_field_direct_and_indirect_init_accepted() {
        // A Callable field seeded from an __init__ param — directly, or via a
        // chain of local rebindings — type-checks (it has a valid placeholder seed).
        assert!(check_src(
            "class M:\n    f: Callable[[], int]\n    def __init__(self, g: Callable[[], int]) -> None:\n        self.f = g\n    def make(self) -> int:\n        return self.f()\n"
        ).is_ok());
        assert!(check_src(
            "class M:\n    f: Callable[[], int]\n    def __init__(self, g: Callable[[], int]) -> None:\n        tmp: Callable[[], int] = g\n        self.f = tmp\n    def make(self) -> int:\n        return self.f()\n"
        ).is_ok());
        assert!(check_src(
            "class M:\n    f: Callable[[], int]\n    def __init__(self, g: Callable[[], int]) -> None:\n        a: Callable[[], int] = g\n        b: Callable[[], int] = a\n        self.f = b\n    def make(self) -> int:\n        return self.f()\n"
        ).is_ok());
    }

    #[test]
    fn callable_field_non_param_init_rejected() {
        // A Callable field that is NOT seeded from a constructor param has no valid
        // placeholder (Rc<dyn Fn> has no Default) — reject honestly at check, never
        // a silent rustc E0277.
        assert_type_err_unit(
            check_src(
                "def mk() -> int:\n    return 0\n\nclass B:\n    f: Callable[[], int]\n    def __init__(self) -> None:\n        self.f = mk\n    def make(self) -> int:\n        return self.f()\n"
            ),
            "must be initialized from a constructor parameter",
        );
    }

    #[test]
    fn init_field_param_map_follows_local_rebind_chain() {
        // Unit-level check of the shared seed-resolution helper: a direct assign,
        // a one-step rebind, and a two-step chain all resolve the field to the root
        // param; a non-param RHS resolves to nothing.
        use crate::ast::{Expr, Stmt, Func, Param, TypeExpr};
        let sp = crate::diag::Span { start: 0, end: 0, line: 0, col: 0 };
        let id = |n: &str| Expr::Ident(n.to_string(), sp);
        let param = |n: &str| Param {
            name: n.to_string(), ty: TypeExpr::Named("int".to_string()), default: None, span: sp, by_ref: false,
        };
        let self_attr = |attr: &str, val: Expr| Stmt::AttrAssign {
            obj: Box::new(id("self")), attr: attr.to_string(), value: val, span: sp,
        };
        let local = |t: &str, v: Expr| Stmt::Assign {
            target: t.to_string(), ty: None, value: v, span: sp,
        };
        let init = Func {
            name: "__init__".to_string(),
            params: vec![param("self"), param("g")],
            ret: TypeExpr::None_,
            body: vec![
                local("a", id("g")),          // a = g
                local("b", id("a")),          // b = a (chain)
                self_attr("direct", id("g")), // self.direct = g
                self_attr("chained", id("b")),// self.chained = b -> g
                self_attr("nope", Expr::Int(1, sp)), // self.nope = 1 (not a param)
            ],
            span: sp,
            is_method: true,
            decorators: vec![],
            crate_deps: vec![],
            type_params: vec![],
        };
        let map = init_field_param_map(&init);
        assert_eq!(map.get("direct").map(String::as_str), Some("g"));
        assert_eq!(map.get("chained").map(String::as_str), Some("g"));
        assert_eq!(map.get("nope"), None);
    }

    #[test]
    fn v2_still_rejects_index_iterate_attr_on_typevar() {
        // Spot-check the ops that MUST stay rejected in v2.
        assert!(check_src("def f[T](t: T) -> int:\n    return t[0]\n").is_err());
        assert!(check_src("def f[T](t: T) -> int:\n    n: int = 0\n    for x in t:\n        n = n + 1\n    return n\n").is_err());
        assert!(check_src("def f[T](t: T) -> int:\n    return t.x\n").is_err());
        assert!(check_src("def f[T](t: T) -> int:\n    if t:\n        return 1\n    return 0\n").is_err());
        assert!(check_src("def f[T](t: T) -> int:\n    return len(t)\n").is_err());
    }
