    use super::*;
    use crate::ast::ClassDef;
    use crate::diag::Span;

    /// A minimal `ClassDef` carrying only name + bases — enough for the poly_map
    /// pre-pass, which reads `ctx.classes` / `bases` via `is_subclass`.
    fn class_def(name: &str, bases: &[&str]) -> ClassDef {
        ClassDef {
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

    /// Build a `TyCtx` populated with the given `(name, bases)` classes.
    fn ctx_with(classes: &[(&str, &[&str])]) -> TyCtx {
        let mut ctx = TyCtx::new();
        for (name, bases) in classes {
            ctx.classes.insert(name.to_string(), class_def(name, bases));
        }
        ctx
    }

    #[test]
    fn generic_class_dunder_impl_carries_type_args() {
        // Regression (BLOCKER-1): a DUNDER on a generic class must emit the trait
        // impl for `Box<T>` (with the `<T: ..>` clause), not the bare `Box` —
        // otherwise rustc raises E0107 "missing generics for struct Box". Here a
        // `Box[T]` with `__eq__` must produce `impl<T: ..> ::std::cmp::PartialEq
        // for Box<T>` and a `&Box<T>` `other` param. We assert on the emitted
        // Rust source (the same string `build` feeds rustc).
        let src = "\
class Box[T]:
    value: T
    def __init__(self, v: T) -> None:
        self.value = v
    def __eq__(self, other: Box) -> bool:
        return self.value == other.value

def main() -> None:
    print(Box(5) == Box(5))
";
        let rust = crate::driver::compile_str(src).expect("compile_str must succeed");
        // The PartialEq impl head must name `Box<T>`, never the bare `Box`.
        assert!(
            rust.contains("::std::cmp::PartialEq for Box<T>"),
            "PartialEq impl must be for `Box<T>`, got:\n{}",
            rust
        );
        // It must carry a generic clause with at least Clone + PartialEq.
        assert!(
            rust.contains("impl<T: Clone + PartialEq> ::std::cmp::PartialEq for Box<T>"),
            "PartialEq impl must carry the inferred `<T: Clone + PartialEq>` clause, got:\n{}",
            rust
        );
        // The `other` param must be `&Box<T>`, never `&Box`.
        assert!(
            rust.contains("fn eq(&self, other: &Box<T>)"),
            "eq's `other` param must be `&Box<T>`, got:\n{}",
            rust
        );
        // And the bare-name regression must be ABSENT.
        assert!(
            !rust.contains("PartialEq for Box {") && !rust.contains("other: &Box)"),
            "the bare-name (no <T>) dunder emission must not appear, got:\n{}",
            rust
        );
    }

    #[test]
    fn non_generic_class_dunder_unchanged() {
        // The dunder fix is gated on `!type_params.is_empty()`: a NON-generic
        // class with `__eq__` must still emit the bare `impl ::std::cmp::PartialEq
        // for Point` (no `<T>`), byte-for-byte as before.
        let src = "\
class Point:
    x: int
    def __init__(self, x: int) -> None:
        self.x = x
    def __eq__(self, other: Point) -> bool:
        return self.x == other.x

def main() -> None:
    print(Point(1) == Point(1))
";
        let rust = crate::driver::compile_str(src).expect("compile_str must succeed");
        assert!(
            rust.contains("impl ::std::cmp::PartialEq for Point {"),
            "non-generic PartialEq impl must stay the bare `for Point`, got:\n{}",
            rust
        );
        assert!(
            rust.contains("fn eq(&self, other: &Point)"),
            "non-generic eq's `other` must stay `&Point`, got:\n{}",
            rust
        );
    }

    #[test]
    fn poly_map_direct_siblings() {
        // Dog(Animal) + Cat(Animal) -> poly_map[Animal] == {Cat, Dog} (sorted).
        let ctx = ctx_with(&[
            ("Animal", &[]),
            ("Dog", &["Animal"]),
            ("Cat", &["Animal"]),
        ]);
        let mut cg = Codegen::new(&ctx);
        cg.build_poly_map();
        assert_eq!(
            cg.poly_map.get("Animal"),
            Some(&vec!["Cat".to_string(), "Dog".to_string()])
        );
        assert!(cg.is_polymorphic_base("Animal"));
    }

    #[test]
    fn poly_map_subless_class_not_polymorphic() {
        // A class with no subclasses in the unit is NOT polymorphic and has no
        // poly_map entry. A leaf subclass (Dog) is likewise not a base.
        let ctx = ctx_with(&[
            ("Animal", &[]),
            ("Dog", &["Animal"]),
            ("Rock", &[]), // unrelated, sub-less
        ]);
        let mut cg = Codegen::new(&ctx);
        cg.build_poly_map();
        assert!(!cg.is_polymorphic_base("Rock"));
        assert!(cg.poly_map.get("Rock").is_none());
        assert!(!cg.is_polymorphic_base("Dog")); // leaf: no subclasses
        // Animal IS a base (has Dog under it).
        assert!(cg.is_polymorphic_base("Animal"));
        assert_eq!(cg.poly_map.get("Animal"), Some(&vec!["Dog".to_string()]));
    }

    #[test]
    fn poly_map_transitive_chain() {
        // C(B(A)): poly_map[A] must contain BOTH B and C (direct + transitive),
        // poly_map[B] contains C. is_subclass(C, A) drives the transitivity.
        let ctx = ctx_with(&[
            ("A", &[]),
            ("B", &["A"]),
            ("C", &["B"]),
        ]);
        let mut cg = Codegen::new(&ctx);
        cg.build_poly_map();
        let a_subs = cg.poly_map.get("A").expect("A must be a polymorphic base");
        assert!(a_subs.contains(&"B".to_string()));
        assert!(a_subs.contains(&"C".to_string()));
        assert_eq!(a_subs, &vec!["B".to_string(), "C".to_string()]);
        assert_eq!(cg.poly_map.get("B"), Some(&vec!["C".to_string()]));
        assert!(cg.is_polymorphic_base("A"));
        assert!(cg.is_polymorphic_base("B"));
        assert!(!cg.is_polymorphic_base("C")); // leaf
    }

    #[test]
    fn poly_map_empty_before_prepass() {
        // The field is empty until the pre-pass runs (mirrors mut_self).
        let ctx = ctx_with(&[("Animal", &[]), ("Dog", &["Animal"])]);
        let cg = Codegen::new(&ctx);
        assert!(cg.poly_map.is_empty());
        assert!(!cg.is_polymorphic_base("Animal"));
    }

    // ── Emission helpers ──────────────────────────────────────────────────────
    //
    // `emit_src` compiles a snippet through the full pipeline (parse + typeck +
    // codegen) and returns the Rust source string. Use `.contains(...)` — never
    // byte-equality — because HashMap-backed field ordering is non-deterministic.

    fn emit_src(src: &str) -> String {
        let m = crate::parser::parse(src).expect("test snippet must parse");
        let ctx = TyCtx::new();
        emit_program(&[(m, src.to_string())], &ctx)
            .expect("test snippet must emit successfully")
    }

    // ── Preamble helpers are always present ───────────────────────────────────

    #[test]
    fn preamble_contains_ipow_helper() {
        // The preamble is emitted unconditionally; __py_ipow must always be present.
        let src = "def f() -> None:\n    pass\n";
        let out = emit_src(src);
        assert!(out.contains("__py_ipow"), "preamble must define __py_ipow");
    }

    #[test]
    fn preamble_contains_floordiv_helper() {
        let src = "def f() -> None:\n    pass\n";
        let out = emit_src(src);
        assert!(out.contains("__py_floordiv"), "preamble must define __py_floordiv");
    }

    #[test]
    fn preamble_contains_mod_helper() {
        let src = "def f() -> None:\n    pass\n";
        let out = emit_src(src);
        assert!(out.contains("__py_mod"), "preamble must define __py_mod");
    }

    // ── Operator emission ─────────────────────────────────────────────────────

    #[test]
    fn emit_pow_uses_ipow_helper() {
        // x ** 2 must lower to the __py_ipow helper call in the output.
        let src = "def f(x: int) -> int:\n    y: int = x ** 2\n    return y\n";
        let out = emit_src(src);
        assert!(out.contains("__py_ipow"), "** operator must emit __py_ipow");
    }

    #[test]
    fn emit_floordiv_uses_floordiv_helper() {
        // a // b must lower to the __py_floordiv helper call.
        let src = "def f(a: int, b: int) -> int:\n    c: int = a // b\n    return c\n";
        let out = emit_src(src);
        assert!(out.contains("__py_floordiv"), "// operator must emit __py_floordiv");
    }

    #[test]
    fn emit_mod_uses_mod_helper() {
        // a % b must lower to the __py_mod helper call.
        let src = "def f(a: int, b: int) -> int:\n    c: int = a % b\n    return c\n";
        let out = emit_src(src);
        assert!(out.contains("__py_mod"), "% operator must emit __py_mod");
    }

    #[test]
    fn emit_augassign_pow_uses_ipow_helper() {
        // x **= 2 is an aug-assign; the emitted Rust must still use __py_ipow.
        let src = "def f(x: int) -> int:\n    x **= 2\n    return x\n";
        let out = emit_src(src);
        assert!(out.contains("__py_ipow"), "**= aug-assign must emit __py_ipow");
    }

    // ── F-string emission ─────────────────────────────────────────────────────

    #[test]
    fn emit_fstring_uses_format_macro() {
        // f"hello {name}" must lower to a Rust format! call.
        let src = "def f(name: str) -> str:\n    s: str = f\"hello {name}\"\n    return s\n";
        let out = emit_src(src);
        assert!(out.contains("format!"), "f-string must emit Rust format! macro");
    }

    // ── Type emission ─────────────────────────────────────────────────────────

    #[test]
    fn emit_int_type_becomes_i64() {
        // A function returning int must annotate with i64 in the Rust signature.
        let src = "def f(x: int) -> int:\n    return x\n";
        let out = emit_src(src);
        assert!(out.contains("i64"), "int type must emit as i64");
    }

    #[test]
    fn emit_str_type_becomes_string() {
        // A function returning str must annotate with String.
        let src = "def f(x: str) -> str:\n    return x\n";
        let out = emit_src(src);
        assert!(out.contains("String"), "str type must emit as String");
    }

    #[test]
    fn emit_bool_type_becomes_bool() {
        // A function returning bool must annotate with bool.
        let src = "def f(x: bool) -> bool:\n    return x\n";
        let out = emit_src(src);
        assert!(out.contains("bool"), "bool type must emit as bool");
    }

    // ── List comprehension emission ───────────────────────────────────────────

    #[test]
    fn emit_list_comp_uses_iterator_pattern() {
        // [x * 2 for x in xs] must lower to an iterator chain (.map or .collect).
        let src = "def f(xs: list[int]) -> list[int]:\n    result: list[int] = [x * 2 for x in xs]\n    return result\n";
        let out = emit_src(src);
        assert!(
            out.contains(".map(") || out.contains(".collect()") || out.contains("collect::<"),
            "list comprehension must emit an iterator map/collect pattern"
        );
    }

    #[test]
    fn rust_ty_class_arm_polymorphism_activated() {
        // C2-2b-i acceptance: rust_ty(Class(n)) emits the companion-enum name
        // `n__` for a POLYMORPHIC base (a class with ≥1 subclass), and the plain
        // value-struct name `n` for a leaf / sub-less class. (C2-1 used to return
        // plain `n` for both; the keystone flips the polymorphic branch.)
        let ctx = ctx_with(&[("Animal", &[]), ("Dog", &["Animal"]), ("Rock", &[])]);
        let mut cg = Codegen::new(&ctx);
        cg.build_poly_map();
        assert!(cg.is_polymorphic_base("Animal"));
        // Polymorphic base -> companion enum.
        assert_eq!(cg.rust_ty(&Ty::Class("Animal".into(), vec![])), "Animal__");
        // Sub-less / leaf classes stay their plain value-struct name.
        assert_eq!(cg.rust_ty(&Ty::Class("Rock".into(), vec![])), "Rock");
        assert_eq!(cg.rust_ty(&Ty::Class("Dog".into(), vec![])), "Dog");
        // A list of a polymorphic base is Vec<Animal__> (the element type flips too).
        assert_eq!(
            cg.rust_ty(&Ty::List(Box::new(Ty::Class("Animal".into(), vec![])))),
            "Vec<Animal__>"
        );
    }

    // ── @extern (Rust-FFI binding) emission ───────────────────────────────────

    #[test]
    fn extern_emits_substituted_template_as_tail_expr() {
        // An @extern function emits the signature built from its declared types
        // plus the template string with each `{param}` substituted for the Rust
        // param identifier, as the function's tail expression.
        let src = "\
@extern
def shout(s: str) -> str:
    \"{s}.to_uppercase()\"

@extern
def repeat_str(s: str, n: int) -> str:
    \"{s}.repeat({n} as usize)\"

@extern
def ipow(base: int, exp: int) -> int:
    \"({base}).pow({exp} as u32)\"
";
        let out = emit_src(src);
        // Signature uses the rust_ty mapping (Str -> String, Int -> i64).
        assert!(out.contains("fn shout(mut s: String) -> String {"),
            "extern signature must reuse the normal type mapping; got:\n{}", out);
        // The `{s}` hole is substituted with the emitted param identifier.
        assert!(out.contains("s.to_uppercase()"),
            "template `{{s}}` must be substituted to `s.to_uppercase()`; got:\n{}", out);
        // Multi-hole template: both holes substituted, author glue preserved.
        assert!(out.contains("s.repeat(n as usize)"),
            "multi-hole template must substitute both params; got:\n{}", out);
        assert!(out.contains("(base).pow(exp as u32)"),
            "ipow template must substitute base/exp; got:\n{}", out);
        // The unsubstituted brace form must NOT survive into the emitted Rust.
        assert!(!out.contains("{s}.to_uppercase()"),
            "the literal `{{s}}` hole must not leak into output; got:\n{}", out);
    }

    // ── Qualified module calls — `import X; X.f(args)` (card 81db88e0) ─────────

    /// A `TyCtx` modeling `import os`: the flat `basename` signature is in
    /// `ctx.funcs`, and `module_funcs["os"]` lists it (resolver-equivalent).
    fn ctx_with_os() -> TyCtx {
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("basename".into(), crate::typeck::FuncSig {
            params: vec![("p".into(), Ty::Str)],
            param_defaults: vec![None],
            param_by_ref: vec![],
            ret: Ty::Str,
        });
        ctx.module_funcs.insert("os".into(), vec!["basename".into()]);
        ctx
    }

    #[test]
    fn qualified_module_call_lowers_to_owner_mangled_call() {
        // (W3-2) `os.basename("/a/b.txt")` lowers to the OWNER-QUALIFIED Rust call
        // `__pyrst_m_os__basename("/a/b.txt".to_string())` — the qualifier `os` IS
        // the owner module (threaded as `callee_owner`), so per-module namespacing
        // keeps it distinct from any other module's `basename`. (Pre-W3 this was a
        // flat `basename(..)`; the keystone owner-mangles it.)
        let ctx = ctx_with_os();
        let mut cg = Codegen::new(&ctx);
        let callee: Box<Expr> = Box::new(Expr::Attr {
            obj: Box::new(Expr::Ident("os".into(), Span::DUMMY)),
            name: "basename".into(),
            span: Span::DUMMY,
        });
        let args = vec![Expr::Str("/a/b.txt".into(), Span::DUMMY)];
        let out = cg.emit_method_call_on_attr(&callee, &args, &[])
            .expect("emit must succeed")
            .expect("a tracked module call must be handled by emit_method_call_on_attr");
        assert!(out.starts_with("__pyrst_m_os__basename("),
            "qualified call must owner-mangle by the qualifier module; got: {}", out);
    }

    #[test]
    fn math_qualified_call_lowers_to_owner_mangled_call() {
        // (W3-2) `math.sqrt(16.0)` flows through the general qualified-module path
        // and owner-mangles to `__pyrst_m_math__sqrt((16.0f64))` — the qualifier is
        // the owner, so co-imported same-named functions stay distinct.
        let mut ctx = TyCtx::new();
        ctx.funcs.insert("sqrt".into(), crate::typeck::FuncSig {
            params: vec![("x".into(), Ty::Float)],
            param_defaults: vec![None],
            param_by_ref: vec![],
            ret: Ty::Float,
        });
        ctx.module_funcs.insert("math".into(), vec!["sqrt".into()]);
        let mut cg = Codegen::new(&ctx);
        let callee: Box<Expr> = Box::new(Expr::Attr {
            obj: Box::new(Expr::Ident("math".into(), Span::DUMMY)),
            name: "sqrt".into(),
            span: Span::DUMMY,
        });
        let args = vec![Expr::Float(16.0, Span::DUMMY)];
        let out = cg.emit_method_call_on_attr(&callee, &args, &[])
            .expect("emit must succeed")
            .expect("a tracked module call must be handled by emit_method_call_on_attr");
        assert!(out.starts_with("__pyrst_m_math__sqrt("),
            "qualified call must owner-mangle by the qualifier module; got: {}", out);
    }

    #[test]
    fn module_constant_lowers_to_owner_mangled_const_name() {
        // (W3-2) A qualified module constant `math.pi` lowers to the OWNER-QUALIFIED
        // mangled const `__pyrst_const_math__pi` — the owner prefix keeps a
        // co-imported same-named const in another module distinct (the design-flagged
        // const-vs-const collision). Mangling still prevents const-pattern capture.
        let mut ctx = TyCtx::new();
        ctx.module_consts.insert("math".into(), vec![("pi".into(), Ty::Float)]);
        let mut cg = Codegen::new(&ctx);
        let attr = Expr::Attr {
            obj: Box::new(Expr::Ident("math".into(), Span::DUMMY)),
            name: "pi".into(),
            span: Span::DUMMY,
        };
        let out = cg.emit_expr(&attr).expect("emit must succeed");
        assert_eq!(out, "__pyrst_const_math__pi", "math.pi must lower to the owner-mangled const name; got: {}", out);
    }

    #[test]
    fn module_const_decl_emits_mangled_top_level_const() {
        // A module-level `NAME: T = <literal>` emits a top-level Rust `const` with
        // a MANGLED name (`__pyrst_const_<name>`). int/float/bool are typed Copy
        // consts; a str const is a `&str` const. The bare reference `print(PI)`
        // also uses the mangled name.
        let src = "\
PI: float = 3.14
COUNT: int = 7
GREETING: str = \"hi\"
FLAG: bool = True

def main() -> None:
    print(PI)
";
        let out = emit_src(src);
        assert!(out.contains("const __pyrst_const_PI: f64 = 3.14f64;"), "float const; got:\n{}", out);
        assert!(out.contains("const __pyrst_const_COUNT: i64 = 7;"), "int const; got:\n{}", out);
        assert!(out.contains("const __pyrst_const_GREETING: &str = \"hi\";"), "str const; got:\n{}", out);
        assert!(out.contains("const __pyrst_const_FLAG: bool = true;"), "bool const; got:\n{}", out);
        // The bare reference resolves to the mangled name too (def/use match).
        assert!(out.contains("__pyrst_const_PI"), "bare ref uses mangled name; got:\n{}", out);
    }

    #[test]
    fn lowercase_const_does_not_capture_pattern_var() {
        // Regression: a lowercase module const `i` alongside `for i in range(3)`.
        // The const is emitted MANGLED (so it can't be a const-pattern), the loop
        // var `i` is a FRESH binding inside the loop, and the const read AFTER the
        // loop resolves back to the mangled const (the loop var does not leak).
        let src = "\
i: int = 99

def main() -> None:
    for i in range(3):
        print(i)
    print(i)
";
        let out = emit_src(src);
        // The const is mangled at its definition.
        assert!(out.contains("const __pyrst_const_i: i64 = 99;"),
            "const i emitted mangled; got:\n{}", out);
        // The loop target is the bare `i` (a fresh Rust binding), and the body
        // prints that bare loop var — NOT the mangled const.
        assert!(out.contains("for i in"), "loop target is bare i; got:\n{}", out);
        assert!(out.contains("println!(\"{}\" , i)"),
            "in-loop reference is the loop var (bare i); got:\n{}", out);
        // The post-loop read resolves to the mangled const (loop var out of scope).
        assert!(out.contains("println!(\"{}\" , __pyrst_const_i)"),
            "post-loop reference is the mangled const; got:\n{}", out);
    }

    #[test]
    fn callable_field_class_emits_buildable_rust() {
        // A class holding a `Callable` field lowers to an `Rc<dyn Fn>` struct
        // field, which implements neither Debug nor PartialEq and has no Default.
        // Assert the four codegen pieces that make it BUILD + run:
        //   1. the struct derives only Clone (no Debug / PartialEq),
        //   2. the constructor seeds the field from the param (no Default::default),
        //   3. a Callable-FIELD call lowers to `(self.f)(..)`, not `self.f(..)`,
        //   4. a lambda arg at the constructor call is wrapped `Rc::new(..) as ..`.
        let src = "\
class Maker:
    f: Callable[[], int]
    def __init__(self, g: Callable[[], int]) -> None:
        self.f = g
    def make(self) -> int:
        return self.f()

def main() -> None:
    m: Maker = Maker(lambda: 42)
    print(m.make())
";
        let rust = crate::driver::compile_str(src).expect("compile_str must succeed");
        // 1. The struct must derive only Clone (Rc<dyn Fn> lacks Debug/PartialEq).
        assert!(
            rust.contains("#[derive(Clone)]\nstruct Maker"),
            "Maker struct must derive only Clone (no Debug/PartialEq), got:\n{}", rust
        );
        // 2. Constructor seeds `f` from the param clone, never Default::default().
        assert!(
            rust.contains("Maker { f: g.clone() }"),
            "constructor must seed `f` from the param, got:\n{}", rust
        );
        // 3. The Callable-field call is parenthesised: `(self.f)()`.
        assert!(
            rust.contains("(self.f)()"),
            "Callable-field call must lower to `(self.f)()`, got:\n{}", rust
        );
        // 4. The lambda argument is wrapped into the `Rc<dyn Fn>` slot.
        assert!(
            rust.contains("::std::rc::Rc::new(move ||")
                && rust.contains("as ::std::rc::Rc<dyn Fn() -> i64>"),
            "lambda arg must be wrapped `Rc::new(..) as Rc<dyn Fn() -> i64>`, got:\n{}", rust
        );
    }

    #[test]
    fn generic_class_callable_field_substitutes_ctor_type_arg() {
        // A GENERIC class with a `Callable[[], V]` constructor param: the cast at
        // the call site must use the CONCRETE instance type arg (`i64`), not the
        // bare class type param `V` (which is not in scope at the call site — that
        // would be E0425). `DD(lambda: 0)` infers `V = int` from the factory.
        let src = "\
class DD[K, V]:
    data: dict[K, V]
    default_factory: Callable[[], V]
    def __init__(self, factory: Callable[[], V]) -> None:
        self.data = {}
        self.default_factory = factory
    def get(self, key: K) -> V:
        if key in self.data:
            return self.data[key]
        value: V = self.default_factory()
        self.data[key] = value
        return value

def main() -> None:
    dd: DD[str, int] = DD(lambda: 0)
    print(dd.get(\"x\"))
";
        let rust = crate::driver::compile_str(src).expect("compile_str must succeed");
        // The wrapped lambda cast must name the concrete return type, not `V`.
        assert!(
            rust.contains("as ::std::rc::Rc<dyn Fn() -> i64>"),
            "ctor lambda cast must use the concrete `i64`, got:\n{}", rust
        );
        assert!(
            !rust.contains("as ::std::rc::Rc<dyn Fn() -> V>"),
            "ctor lambda cast must NOT name the bare type param `V`, got:\n{}", rust
        );
    }

    #[test]
    fn codegen_asserts_branch_divergence_as_defence_in_depth() {
        // The V1-d BLOCKER shape (a `list` vs a `set` across sibling `if`
        // branches). typeck rejects it at `check`; this proves the codegen
        // defence-in-depth assertion ALSO rejects it — driving `emit_program`
        // DIRECTLY (bypassing `check_bodies`) so a divergent join can never
        // silently reach the hoist and drop a branch's value. Without the
        // assertion, `emit_program` would succeed and emit a wrong-output hoist.
        let src = "\
def main() -> None:
    flag: bool = False
    if flag:
        xs = [1, 2, 3]
    else:
        xs = {9, 8, 7}
    print(len(xs))
";
        let mut path = std::env::temp_dir();
        path.push(format!("pyrst_branchdiv_{}.pyrs", std::process::id()));
        std::fs::write(&path, src).expect("write temp source");
        // `resolve` parses + builds the ctx but does NOT run typeck, so the
        // divergent program reaches codegen unfiltered.
        let prog = crate::resolver::resolve(&path).expect("resolve must succeed");
        let result = crate::codegen::emit_program(&prog.modules, &prog.ctx);
        let _ = std::fs::remove_file(&path);
        let err = result.expect_err("codegen must reject the divergent branch join");
        let msg = format!("{}", err);
        assert!(
            msg.contains("incompatible types across the branches"),
            "expected a branch-divergence codegen error, got:\n{}",
            msg
        );
    }

    /// Drive `emit_program` on `src` WITHOUT running typeck (`resolve` parses +
    /// builds the ctx but does not typecheck), returning the codegen result. Used
    /// to exercise the branch-divergence defence-in-depth assertion, which typeck
    /// would otherwise reject at `check` before codegen runs.
    fn emit_bypassing_typeck(src: &str, tag: &str) -> Result<String> {
        let mut path = std::env::temp_dir();
        path.push(format!("pyrst_{}_{}.pyrs", tag, std::process::id()));
        std::fs::write(&path, src).expect("write temp source");
        let prog = crate::resolver::resolve(&path).expect("resolve must succeed");
        let result = crate::codegen::emit_program(&prog.modules, &prog.ctx);
        let _ = std::fs::remove_file(&path);
        result
    }

    #[test]
    fn codegen_asserts_try_except_divergence() {
        // The `try` body and its `except` handler are sibling value-paths; a
        // divergent bare local across them is the same silent-drop hazard as
        // if/else. typeck rejects it at `check`; prove the codegen insurance also
        // rejects it (bypassing typeck) rather than emitting a wrong-output hoist.
        let src = "\
def gen(n: int) -> Iterator[int]:
    i: int = 0
    while i < n:
        yield i
        i = i + 1

def main() -> None:
    try:
        xs = [1, 2, 3]
        raise ValueError(\"boom\")
    except ValueError:
        xs = gen(3)
    for x in xs:
        print(x)
";
        let err = emit_bypassing_typeck(src, "trydiv").expect_err("codegen must reject");
        assert!(
            format!("{}", err).contains("incompatible types across"),
            "expected try/except divergence rejection, got:\n{}", err
        );
    }

    #[test]
    fn codegen_asserts_match_arm_divergence() {
        // The arms of a `match` are sibling value-paths; a divergent bare local
        // across them must be caught by the codegen insurance too.
        let src = "\
def gen(n: int) -> Iterator[int]:
    i: int = 0
    while i < n:
        yield i
        i = i + 1

def main() -> None:
    flag: int = 0
    match flag:
        case 0:
            xs = [1, 2, 3]
        case _:
            xs = gen(3)
    for x in xs:
        print(x)
";
        let err = emit_bypassing_typeck(src, "matchdiv").expect_err("codegen must reject");
        assert!(
            format!("{}", err).contains("incompatible types across"),
            "expected match-arm divergence rejection, got:\n{}", err
        );
    }

    #[test]
    fn match_arm_same_type_local_hoists() {
        // MAJOR 2a: a bare local assigned the SAME type in every match arm and read
        // after the match must HOIST to function scope (prescan's Match arm) so it
        // is visible after the `match` block — otherwise a raw rustc E0425 leaked
        // while `check` passed. Assert the emitted Rust hoists `xs` at function
        // scope (a `let mut xs: Vec<i64>` declaration BEFORE the `match`).
        let src = "\
def main() -> None:
    flag: int = 0
    match flag:
        case 0:
            xs = [1, 2, 3]
        case _:
            xs = [4, 5]
    for x in xs:
        print(x)
";
        let rust = crate::driver::compile_str(src).expect("compile_str must succeed");
        assert!(
            rust.contains("let mut xs: Vec<i64>"),
            "match-arm local `xs` must be hoisted to a function-scope Vec, got:\n{}", rust
        );
    }

    #[test]
    fn codegen_asserts_read_after_conflicting_reassign() {
        // (fix-b) An outer local (`xs`, a list) reassigned to a generator inside a
        // single `if` branch and READ after the block is the residual non-sibling
        // silent value-drop. typeck rejects it at `check`; prove the codegen
        // insurance also rejects it (bypassing typeck) rather than emitting a
        // wrong-output hoist that discards the block-scoped shadow at the join.
        let src = "\
def gen(n: int) -> Iterator[int]:
    i: int = 0
    while i < n:
        yield i
        i = i + 1

def main() -> None:
    flag: bool = True
    xs = [1, 2, 3]
    if flag:
        xs = gen(3)
    for x in xs:
        print(x)
";
        let err = emit_bypassing_typeck(src, "readafterdiv").expect_err("codegen must reject");
        assert!(
            format!("{}", err).contains("read after the block"),
            "expected a read-after-conflicting-reassign codegen error, got:\n{}", err
        );
    }

    #[test]
    fn codegen_accepts_conflicting_reassign_read_only_in_block() {
        // The over-rejection guard (the corpus canary shape): an outer local
        // reassigned to a conflicting type inside a block but read ONLY WITHIN that
        // block never crosses a join, so the block-scoped shadow is correct. The
        // codegen insurance must NOT reject it — it is type-free liveness that keeps
        // `xs` out of the block's exit-liveness, exactly as at `check`.
        let src = "\
def main() -> None:
    xs = [1, 2, 3]
    flag: bool = True
    if flag:
        xs = \"hello\"
        print(xs)
    print(\"done\")
";
        emit_bypassing_typeck(src, "readonlyblock")
            .expect("codegen must NOT reject a conflicting reassign read only within its block");
    }

    // ── W3-2: owner-qualified name mangling + owner lookup ─────────────────────

    /// W3-2: `mangle_const` is owner-qualified — a ROOT const keeps the historical
    /// `__pyrst_const_<name>` (byte-identical for import-free programs), a module
    /// const gains its owner prefix, and a dotted module id sanitizes its dots.
    #[test]
    fn w3_mangle_const_owner_qualified() {
        assert_eq!(mangle_const(None, "PI"), "__pyrst_const_PI");
        assert_eq!(mangle_const(Some("sys"), "version"), "__pyrst_const_sys__version");
        // (W3-3) A dotted owner sanitizes collision-proof: `.` → `_d`.
        assert_eq!(mangle_const(Some("os.path"), "sep"), "__pyrst_const_os_dpath__sep");
    }

    /// W3-2: `emit_name` — a ROOT (None) free-function name stays crate-root
    /// (`escape_ident`, a keyword still round-trips), a non-root owner mangles to
    /// the flat `__pyrst_m_<owner>__<name>`, and dotted ids sanitize dots.
    #[test]
    fn w3_emit_name_owner_and_root() {
        let ctx = TyCtx::new();
        let cg = Codegen::new(&ctx);
        assert_eq!(cg.emit_name(None, "escape"), "escape");
        assert_eq!(cg.emit_name(None, "type"), "r#type"); // root keyword still escapes
        assert_eq!(cg.emit_name(Some("re"), "escape"), "__pyrst_m_re__escape");
        // (W3-3) A dotted owner sanitizes collision-proof: `os.path` → `os_dpath`.
        assert_eq!(cg.emit_name(Some("os.path"), "join"), "__pyrst_m_os_dpath__join");
    }

    /// (W3-3) The mangler is COLLISION-PROOF: a dotted module id and a hypothetical
    /// flat module whose bare name contains the sanitized separator must NEVER
    /// produce the same emitted identifier (the naive `.replace('.', "_")` would
    /// map both `a.b` and `a_b` to `a_b`, silently cross-wiring their symbols).
    /// `mangle_mod_id` escapes `_` → `_u` and `.` → `_d`, so the mapping is
    /// injective — distinct module ids give distinct fragments, at every emission
    /// site (`emit_name`, `emit_class_name`, `mangle_const`).
    #[test]
    fn w3_mangler_is_collision_proof() {
        // The exact collision the design flagged: dotted `a.b` vs flat `a_b`.
        assert_eq!(mangle_mod_id("a.b"), "a_db");
        assert_eq!(mangle_mod_id("a_b"), "a_ub");
        assert_ne!(mangle_mod_id("a.b"), mangle_mod_id("a_b"));

        // Boundary cases: a trailing/leading underscore adjacent to the separator
        // must not alias two DIFFERENT dotted ids onto one fragment.
        assert_ne!(mangle_mod_id("a_.b"), mangle_mod_id("a._b"));
        assert_eq!(mangle_mod_id("a_.b"), "a_u_db");
        assert_eq!(mangle_mod_id("a._b"), "a_d_ub");

        // A single-component id with no underscore is unchanged (existing single
        // -module mangled names stay byte-identical).
        assert_eq!(mangle_mod_id("os"), "os");
        assert_eq!(mangle_mod_id("re"), "re");

        // The property propagates through every emission site.
        let mut ctx = TyCtx::new();
        ctx.class_owner.insert("C".to_string(), "a.b".to_string());
        let cg = Codegen::new(&ctx);
        assert_ne!(cg.emit_name(Some("a.b"), "f"), cg.emit_name(Some("a_b"), "f"));
        assert_eq!(cg.emit_name(Some("a.b"), "f"), "__pyrst_m_a_db__f");
        assert_eq!(cg.emit_class_name("C"), "__pyrst_m_a_db__C");
        assert_ne!(mangle_const(Some("a.b"), "K"), mangle_const(Some("a_b"), "K"));
    }

    /// W3-2: `emit_class_name` resolves the owner from the GLOBAL `class_owner`
    /// map (class names are globally unique): a root class stays bare (byte-for-byte
    /// the pre-W3 `n.clone()`), an imported class mangles by its owner.
    #[test]
    fn w3_emit_class_name_via_global_owner() {
        let mut ctx = TyCtx::new();
        ctx.class_owner.insert("Match".to_string(), "re".to_string());
        let cg = Codegen::new(&ctx);
        assert_eq!(cg.emit_class_name("Point"), "Point"); // root class: bare, unchanged
        assert_eq!(cg.emit_class_name("Match"), "__pyrst_m_re__Match");
    }

    /// W3-2: `bare_owner_for` mirrors typeck's flat resolution — the ROOT's own def
    /// shadows a from-import (`root_defined` wins → None → unwrapped); a from-import
    /// binding at the root resolves to its OWNER module; and inside a module a bare
    /// name resolves to that module's own definition. A globally-owned class name
    /// is the final fallback.
    #[test]
    fn w3_bare_owner_for_resolution() {
        use crate::typeck::{FuncSig, ModuleSymbols};
        let mut ctx = TyCtx::new();
        // operator owns `sub`; re owns a class `Match`.
        let mut operator = ModuleSymbols::default();
        operator.funcs.insert(
            "sub".into(),
            FuncSig { params: vec![], ret: Ty::Int, param_defaults: vec![], param_by_ref: vec![] },
        );
        ctx.module_symbols.insert("operator".into(), operator);
        ctx.class_owner.insert("Match".into(), "re".into());
        // root did `from operator import sub`, then defined its OWN `sub`.
        ctx.import_bindings
            .entry(crate::typeck::ROOT_MODULE_ID.into())
            .or_default()
            .insert("sub".into(), ("operator".into(), "sub".into()));

        let mut cg = Codegen::new(&ctx);

        // ROOT scope, `sub` is a from-import binding but NOT root-defined -> owner.
        cg.current_module = None;
        assert_eq!(cg.bare_owner_for("sub").as_deref(), Some("operator"));
        // ROOT scope, a root-defined name shadows the import -> None (unwrapped).
        cg.root_defined.insert("sub".into());
        assert_eq!(cg.bare_owner_for("sub"), None);
        // A globally-owned class name resolves to its owner even at the root.
        assert_eq!(cg.bare_owner_for("Match").as_deref(), Some("re"));
        // A plain unknown/builtin name -> None (unwrapped, import-free unchanged).
        assert_eq!(cg.bare_owner_for("print"), None);

        // MODULE scope `operator`: its OWN `sub` wins (same-module owner).
        cg.root_defined.clear();
        cg.current_module = Some("operator".into());
        assert_eq!(cg.bare_owner_for("sub").as_deref(), Some("operator"));
    }

    /// W3-fix / F1,F14: `with_module_symbols_promoted` makes a module's OWN
    /// function win over the flat last-writer AND drops a same-named FOREIGN class
    /// from that module's checking view — so `time.pyrs`'s internal bare `time()`
    /// resolves to its own fn, not `datetime`'s class `time`. The symmetric view
    /// for `datetime` KEEPS the class (it owns it).
    #[test]
    fn w3fix_promoted_own_func_shadows_foreign_class() {
        use crate::typeck::{FuncSig, ModuleSymbols};
        let fn_sig = FuncSig { params: vec![], ret: Ty::Float, param_defaults: vec![], param_by_ref: vec![] };
        let mut ctx = TyCtx::new();
        // `time` module owns a FUNCTION `time`; `datetime` owns a CLASS `time`.
        let mut timemod = ModuleSymbols::default();
        timemod.funcs.insert("time".into(), fn_sig.clone());
        ctx.module_symbols.insert("time".into(), timemod);
        let mut dtmod = ModuleSymbols::default();
        dtmod.classes.insert("time".into(), class_def("time", &[]));
        ctx.module_symbols.insert("datetime".into(), dtmod);
        // Flat facade (last-merged): the class won `ctx.classes`, the fn `ctx.funcs`.
        ctx.funcs.insert("time".into(), fn_sig);
        ctx.classes.insert("time".into(), class_def("time", &[]));

        // Checking `time`: its own fn is promoted and the foreign class is removed.
        let for_time = ctx.with_module_symbols_promoted(Some("time"));
        assert!(for_time.funcs.contains_key("time"));
        assert!(!for_time.classes.contains_key("time"), "foreign class `time` must be dropped for the fn-owning module");
        // Checking `datetime`: it OWNS the class, so the class stays.
        let for_dt = ctx.with_module_symbols_promoted(Some("datetime"));
        assert!(for_dt.classes.contains_key("time"));
    }

    /// W3-fix / F4: the checking view folds a `from X import f` binding to X's REAL
    /// signature (owner-first), overriding the flat last-MERGED collider — unless
    /// the ROOT defines its own `f` (root-shadows-imports).
    #[test]
    fn w3fix_promoted_folds_from_import_owner_first() {
        use crate::typeck::{FuncSig, ModuleSymbols};
        let one_arg = FuncSig { params: vec![("a".into(), Ty::Int)], ret: Ty::Int, param_defaults: vec![None], param_by_ref: vec![false] };
        let two_arg = FuncSig { params: vec![("a".into(), Ty::Int), ("b".into(), Ty::Int)], ret: Ty::Int, param_defaults: vec![None, None], param_by_ref: vec![false, false] };
        let mut ctx = TyCtx::new();
        // `modb` owns a 1-arg `combine`; the flat table holds a 2-arg collider.
        let mut modb = ModuleSymbols::default();
        modb.funcs.insert("combine".into(), one_arg);
        ctx.module_symbols.insert("modb".into(), modb);
        ctx.funcs.insert("combine".into(), two_arg);
        // root did `from modb import combine`.
        ctx.import_bindings
            .entry(crate::typeck::ROOT_MODULE_ID.into())
            .or_default()
            .insert("combine".into(), ("modb".into(), "combine".into()));

        // Root check: the binding owner's 1-arg sig wins.
        let p = ctx.with_module_symbols_promoted(None);
        assert_eq!(p.funcs["combine"].params.len(), 1, "from-import must resolve to modb's 1-arg sig");
        // But a root that defines its OWN `combine` shadows the import.
        ctx.root_defined.insert("combine".into());
        let p2 = ctx.with_module_symbols_promoted(None);
        assert_eq!(p2.funcs["combine"].params.len(), 2, "root's own def shadows the from-import");
    }

    /// W3-fix / F10,F14: a module's OWN const type is promoted over the flat
    /// last-writer, so a bare const reference inside its body types owner-first.
    #[test]
    fn w3fix_promoted_promotes_own_const() {
        use crate::typeck::ModuleSymbols;
        let mut ctx = TyCtx::new();
        let mut cfg = ModuleSymbols::default();
        cfg.consts.insert("TAG".into(), Ty::Str);
        ctx.module_symbols.insert("cfg".into(), cfg);
        // Flat facade: another module's INT `TAG` merged last.
        ctx.vars.insert("TAG".into(), Ty::Int);
        let p = ctx.with_module_symbols_promoted(Some("cfg"));
        assert_eq!(p.vars.get("TAG"), Some(&Ty::Str), "cfg's own str const type wins in its checking view");
    }
