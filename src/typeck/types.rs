use super::*;

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
    /// A user class instance type. The `String` is the class NAME; the `Vec<Ty>`
    /// is its TYPE ARGUMENTS, which is EMPTY for a non-generic class (or a bare,
    /// not-yet-instantiated class name) and carries the inferred/declared args for
    /// a generic-class instance — e.g. `Box(5)` infers `Class("Box", [Int])` and
    /// `b: Pair[int, str]` declares `Class("Pair", [Int, Str])`. The args are the
    /// substitution for the class's `type_params`, in declaration order: method
    /// calls and field reads on the instance substitute them into the (type-var-
    /// bearing) method signature / field type. For a non-generic class the Vec is
    /// always empty, so `Class("Point", [])` is byte-for-byte the old `Class(n)`
    /// behaviour (equality, hashing, Display, rust_ty all collapse to the name).
    Class(String, Vec<Ty>),
    /// A first-class function value: `Func(arg_types, ret_type)`. Spelled
    /// `Callable[[Arg, ...], Ret]` in source and lowered to
    /// `Rc<dyn Fn(Arg, ...) -> Ret>` by codegen. Covers both named function
    /// references used as values and (capturing) lambdas.
    Func(Vec<Ty>, Box<Ty>),
    File,            // an open file handle (open() / `with open(...) as f`)
    /// Generics v1: a BOUND type variable inside a parametric generic function,
    /// e.g. the `T` in `def f[T](x: T) -> T`. It is produced ONLY by the scoped
    /// annotation lowering (`from_type_expr_scoped`) when a name matches one of
    /// the enclosing function's declared `type_params`; a type name that is not a
    /// declared param stays a `Ty::Class`/builtin exactly as before. A `TypeVar`
    /// is OPAQUE in a function body (only move/clone/assign/return/pass/container
    /// index+store are allowed — see `reject_typevar_ops`); at a CALL site it is
    /// unified against the actual argument type and SUBSTITUTED away, so it never
    /// reaches a concrete call's result type. Codegen emits it as the Rust
    /// generic parameter name (`rust_ty` -> the bare name) and `is_copy` is false
    /// (clone-on-use, matching the `T: Clone` bound emitted on the `fn`).
    TypeVar(String),
    Unknown,
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Int     => write!(f, "int"),
            Ty::Float   => write!(f, "float"),
            Ty::Bool    => write!(f, "bool"),
            Ty::Str     => write!(f, "str"),
            Ty::Unit    => write!(f, "None"),
            Ty::NoneVal => write!(f, "None"),
            Ty::List(t) => write!(f, "list[{}]", t),
            Ty::Set(t)  => write!(f, "set[{}]", t),
            Ty::Dict(k, v) => write!(f, "dict[{}, {}]", k, v),
            Ty::Tuple(ts) => {
                write!(f, "tuple[")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", t)?;
                }
                write!(f, "]")
            }
            Ty::Option(t) => write!(f, "{} | None", t),
            Ty::Class(n, args) => {
                write!(f, "{}", n)?;
                if !args.is_empty() {
                    write!(f, "[")?;
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", a)?;
                    }
                    write!(f, "]")?;
                }
                Ok(())
            }
            Ty::Func(args, ret) => {
                write!(f, "Callable[[")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", a)?;
                }
                write!(f, "], {}]", ret)
            }
            Ty::File      => write!(f, "file"),
            Ty::TypeVar(n) => write!(f, "{}", n),
            Ty::Unknown   => write!(f, "unknown"),
        }
    }
}

impl Ty {
    /// Lower a `TypeExpr` annotation to a `Ty`, rejecting illegal annotations
    /// (non-hashable `set`/`dict` keys, misplaced `Mut[...]`, unknown generics).
    ///
    /// `span` is the source location of the *whole annotation* and is attached
    /// to every diagnostic this produces (EPIC-8): callers pass the most precise
    /// real span they have (a param's `.span`, an annotated assignment's `.span`,
    /// the enclosing function's `.span` for a return type, etc.) so the rendered
    /// error points at a real `line:col` with a caret instead of `0:0`. Recursive
    /// calls for nested element types reuse the same `span` — a nested
    /// `set[float]` error pointing at the full annotation is correct and far
    /// better than a dummy span. Callers that only consult this for inference and
    /// discard any error (codegen, type-inference fallbacks) may pass
    /// `Span::DUMMY` since the error never reaches the user.
    pub fn from_type_expr(t: &TypeExpr, span: Span) -> Result<Ty> {
        Ty::from_type_expr_scoped(t, span, &[])
    }

    /// Generics v1: like [`from_type_expr`], but a bare type NAME that matches one
    /// of `type_params` lowers to `Ty::TypeVar(name)` instead of `Ty::Class(name, _)`.
    /// `type_params` is the enclosing generic function's declared type-variable
    /// set (empty for every non-generic context, where this is identical to
    /// `from_type_expr`). The scope threads through every nested element type, so
    /// `list[T]`, `tuple[A, B]`, `dict[K, V]`, `Optional[T]`, and
    /// `Callable[[T], T]` all resolve their type-var components. A name NOT in
    /// `type_params` is unaffected (stays a builtin / `Ty::Class`).
    pub fn from_type_expr_scoped(t: &TypeExpr, span: Span, type_params: &[String]) -> Result<Ty> {
        Ok(match t {
            TypeExpr::None_ => Ty::Unit,
            TypeExpr::Named(n) => {
                let stripped = n.trim_matches('\'').trim_matches('"');
                if type_params.iter().any(|tp| tp == stripped) {
                    Ty::TypeVar(stripped.to_string())
                } else {
                    match stripped {
                        "int" => Ty::Int,
                        "float" => Ty::Float,
                        "bool" => Ty::Bool,
                        "str" => Ty::Str,
                        other => Ty::Class(other.to_string(), vec![]),
                    }
                }
            }
            TypeExpr::Generic(n, args) => match (n.as_str(), args.as_slice()) {
                ("list", [t]) => Ty::List(Box::new(Ty::from_type_expr_scoped(t, span, type_params)?)),
                ("set", [t]) => {
                    // A declared `set[float]` resolves to Set(Float), which
                    // codegen would emit as the uncompilable `HashSet<f64>`.
                    // Reject it at the resolver so vars, params, and returns are
                    // covered uniformly — even when initialized with `set()`.
                    let elem = Ty::from_type_expr_scoped(t, span, type_params)?;
                    require_hashable(&elem, span, "set element")?;
                    Ty::Set(Box::new(elem))
                }
                ("dict", [k, v]) => {
                    // A declared `dict[float, _]` resolves to Dict(Float, _) ->
                    // uncompilable `HashMap<f64, _>`. Reject the KEY only; float
                    // values are fine.
                    let key = Ty::from_type_expr_scoped(k, span, type_params)?;
                    require_hashable(&key, span, "dict key")?;
                    Ty::Dict(Box::new(key), Box::new(Ty::from_type_expr_scoped(v, span, type_params)?))
                }
                ("tuple", args) => Ty::Tuple(args.iter().map(|a| Ty::from_type_expr_scoped(a, span, type_params)).collect::<Result<Vec<_>>>()?),
                // `Iterator[T]` is the declared return type of a GENERATOR (a
                // function whose body contains `yield`). In pyrst's EAGER v1 a
                // generator runs to completion collecting its yielded values into
                // a `Vec<T>` and returns it, so `Iterator[T]` lowers to the same
                // internal type as `list[T]` — every existing list machinery
                // (for-loop / comprehension element typing, `Vec<T>` codegen)
                // applies unchanged. Generator-ness is tracked separately (by the
                // presence of `Stmt::Yield`) and drives the honest-error checks +
                // the Vec-collect desugar; the element typing is just `list[T]`.
                ("Iterator", [t]) => Ty::List(Box::new(Ty::from_type_expr_scoped(t, span, type_params)?)),
                ("Optional", [t]) => Ty::Option(Box::new(Ty::from_type_expr_scoped(t, span, type_params)?)),
                ("Union", args) => {
                    let non_none: Vec<_> = args.iter()
                        .filter(|a| !matches!(a, TypeExpr::None_))
                        .collect();
                    if non_none.len() == 1 {
                        Ty::Option(Box::new(Ty::from_type_expr_scoped(non_none[0], span, type_params)?))
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
                    span,
                    msg: "Mut[...] is only valid on a parameter".to_string(),
                }),
                // Generics v2 (generic CLASSES): a `Name[Arg, ...]` whose head is
                // not a builtin generic is read as a parametrized USER-CLASS
                // instance type `Ty::Class("Box", [Int])`. This mirrors the bare
                // `Named` arm, which already lowers an unknown name to
                // `Ty::Class(name, [])` permissively (a name that is not really a
                // class fails later at field/method resolution, never here) — the
                // resolver has no `ctx` to consult, so validity is checked at the
                // use site, not at lowering. The arg types lower recursively with
                // the same `type_params` scope, so `Box[T]` inside a generic
                // function and `Pair[int, str]` both resolve their components.
                (other, generic_args) => Ty::Class(
                    other.to_string(),
                    generic_args
                        .iter()
                        .map(|a| Ty::from_type_expr_scoped(a, span, type_params))
                        .collect::<Result<Vec<_>>>()?,
                ),
            },
            TypeExpr::Tuple(parts) => {
                let tys = parts.iter().map(|p| Ty::from_type_expr_scoped(p, span, type_params)).collect::<Result<Vec<_>>>()?;
                Ty::Tuple(tys)
            }
            // `Callable[[Arg, ...], Ret]` -> `Ty::Func`. Each argument type and
            // the return type lower recursively (so a `Callable` nested inside a
            // `Callable` argument/return also resolves).
            TypeExpr::Func(args, ret) => {
                let arg_tys = args.iter().map(|a| Ty::from_type_expr_scoped(a, span, type_params)).collect::<Result<Vec<_>>>()?;
                Ty::Func(arg_tys, Box::new(Ty::from_type_expr_scoped(ret, span, type_params)?))
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
    /// Qualified-module-call support: an IMPORTED file/stdlib module's NAME (its
    /// source-file stem, e.g. `"os"`) → the names of the top-level functions that
    /// module defines. Populated by `merge_ctx_from_module` for NON-ROOT modules
    /// only (the root program's own functions are not a qualifiable module). It
    /// lets `import X; X.f(args)` resolve: a `Call` whose callee is
    /// `Attr{Ident(X), f}` with `X` a key here and `f` in its list is typed and
    /// lowered exactly like a flat call to `f` (whose signature lives in `funcs`).
    ///
    /// `funcs` stays FLAT (every module's functions merged under their bare name)
    /// — codegen emits the flat name. CONSEQUENCE: a cross-module same-name
    /// collision (two imported modules each defining `f`) is unresolved here;
    /// stdlib modules use distinct names, and per-module namespacing (`X__f`) is a
    /// later refinement. `math` is deliberately absent: it is skip-listed by the
    /// resolver and never becomes a real module, so `math.sqrt(x)` keeps using its
    /// dedicated hardcoded handling in codegen/typeck and never reaches this path.
    ///
    /// Empty on the LSP single-file path (`analysis.rs` merges with
    /// `is_root = true`), so qualified calls don't resolve in the editor — the
    /// same gap the rest of the stdlib already has there; the call simply stays
    /// `Unknown` and does not crash.
    pub module_funcs: HashMap<String, Vec<String>>,
    /// Module-level CONSTANTS support (mirror of `module_funcs`): an IMPORTED
    /// file/stdlib module's NAME → its top-level annotated-literal constants as
    /// `(const-name, type)` pairs. Populated by `merge_ctx_from_module` for
    /// NON-ROOT modules only, for an annotated assign `NAME: T = <literal>`
    /// (int/float/str/bool). It lets `import X; X.CONST` resolve: an
    /// `Attr{Ident(X), CONST}` (a non-call) where `X` is a key here and `CONST`
    /// is in its list is typed as the const's `T` and lowered to the FLAT Rust
    /// `const CONST`. A BARE `CONST` inside the defining module resolves through
    /// `vars` (the same annotated-assign arm also registers it there).
    ///
    /// Like `module_funcs`, the const NAMESPACE is FLAT: codegen emits one
    /// top-level `const CONST` per module-level constant, so a cross-module
    /// same-name collision is unresolved (stdlib uses distinct names). Empty on
    /// the LSP single-file path for the same reason as `module_funcs`.
    pub module_consts: HashMap<String, Vec<(String, Ty)>>,
    /// Generics v1: function NAME → its declared PEP 695 type-parameter list
    /// (`def f[T, U](...)` -> `["T", "U"]`). ONLY generic functions appear here;
    /// a plain `def` is absent. Populated by the resolver alongside `funcs`. The
    /// matching `FuncSig` in `funcs` already has its `params`/`ret` lowered with
    /// these names as `Ty::TypeVar` (scoped lowering), so call-site unification
    /// reads the structure from the sig and consults THIS map for the full
    /// declared set — needed to detect a type param that no argument can infer
    /// (the "cannot infer type parameter `T`" error) and to keep the unification
    /// fast-path off the non-generic hot path (`funcs` lookups stay unchanged for
    /// the 99% non-generic case).
    pub generic_funcs: HashMap<String, Vec<String>>,
    /// Generics v2 (transitive bound propagation): generic function NAME → its
    /// full `Func` AST body. Populated by the resolver alongside `generic_funcs`
    /// (ONLY generic top-level functions; a plain `def` and methods are absent).
    /// `infer_func_typevar_bounds` needs a callee's BODY — not just its signature
    /// — to recompute the trait bounds the callee requires, so that when a
    /// generic `f` passes one of its type vars into a generic `g`, `g`'s inferred
    /// bounds for that position FOLD INTO `f`'s own bound set (the fixed point
    /// over the generic call graph). Without the body here, `f`'s clause would be
    /// missing `g`'s bound and the generated crate would fail rustc (the
    /// silent-build-fail this field closes). `TyCtx` is built once per program
    /// (never cheap-cloned in a hot path), so storing the bodies is acceptable.
    pub generic_func_bodies: HashMap<String, Func>,
    /// Generics v2 (generic CLASSES): class NAME → its declared PEP 695
    /// type-parameter list (`class Box[T]:` -> `["T"]`, `class Pair[A, B]:` ->
    /// `["A", "B"]`). ONLY generic classes appear here; a plain `class` is
    /// absent. Populated by the resolver alongside `classes`. The names index
    /// the `Vec<Ty>` carried by a `Ty::Class(name, args)` instance — position `i`
    /// of `args` is the binding for `type_params[i]`. The substitution helpers
    /// (`class_type_subst`) zip this list against an instance's args to turn a
    /// method signature's / field's `Ty::TypeVar(T)` into the instance's concrete
    /// arg. Empty for the LSP single-file path until the resolver runs, exactly
    /// like `generic_funcs`; method/field resolution then degrades to the
    /// non-substituted (type-var-bearing) signature, never a crash.
    pub generic_classes: HashMap<String, Vec<String>>,
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

        Self { funcs, classes: HashMap::new(), vars, module_funcs: HashMap::new(), module_consts: HashMap::new(), generic_funcs: HashMap::new(), generic_func_bodies: HashMap::new(), generic_classes: HashMap::new() }
    }

    pub fn get_all_fields(&self, class_name: &str) -> Vec<crate::ast::Param> {
        let mut fields = Vec::new();
        let mut visited = std::collections::HashSet::new();
        self.collect_fields(class_name, &mut fields, &mut visited);
        fields
    }

    pub(crate)     fn collect_fields(&self, class_name: &str, fields: &mut Vec<crate::ast::Param>, visited: &mut std::collections::HashSet<String>) {
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

    pub(crate)     fn find_method(&self, class_name: &str, method_name: &str, visited: &mut std::collections::HashSet<String>) -> Option<FuncSig> {
        if visited.contains(class_name) {
            return None;
        }
        visited.insert(class_name.to_string());

        if let Some(class_def) = self.classes.get(class_name) {
            // Check this class's methods
            if let Some(method) = class_def.methods.iter().find(|m| &m.name == method_name) {
                // Resolve param/return types consistently with the error-propagating
                // path in `check_bodies` (commit 8023fbc): `from_type_expr` is taken
                // as the single source of truth. Any method reaching here already
                // passed `check_bodies` (the driver runs it before any inference /
                // codegen site that calls `get_method`), so every annotation lowered
                // successfully and these resolutions cannot fail in practice. Should
                // one ever fail, we surface it as method-not-found (`None`) rather
                // than silently DROPPING a param (the old `.filter_map(...ok())`) or
                // fabricating `Ty::Unknown` for the return — never a corrupt FuncSig.
                // Generics v2: lower the method signature with the CLASS's type
                // parameters in scope, so a class type var `T` in a param/return
                // becomes `Ty::TypeVar("T")` (matching the resolver's scoped
                // registration). A non-generic class has empty `type_params`, so
                // this is identical to the old unscoped lowering. Call sites on a
                // concrete `Ty::Class(name, args)` substitute the args afterwards.
                let params: Vec<(String, Ty)> = match method.params.iter()
                    .filter(|p| p.name != "self")
                    .map(|p| Ty::from_type_expr_scoped(&p.ty, p.span, &class_def.type_params).map(|ty| (p.name.clone(), ty)))
                    .collect::<Result<Vec<_>>>()
                {
                    Ok(ps) => ps,
                    Err(_) => return None,
                };
                let ret = match Ty::from_type_expr_scoped(&method.ret, method.span, &class_def.type_params) {
                    Ok(ty) => ty,
                    Err(_) => return None,
                };
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

/// (EPIC-5 C1-A) Is `child` the same class as, or a subclass of, `ancestor`?
///
/// Walks `child`'s single-inheritance `bases` chain (via `ctx.classes` /
/// `ClassDef.bases`) until it reaches `ancestor` or runs out of bases. The
/// relation is REFLEXIVE (`is_subclass(X, X, _) == true`) so that the exact-type
/// behaviour of `types_compatible` is preserved when both sides name the same
/// class — the `(Class(d), Class(b)) if is_subclass(d, b, ctx)` arm subsumes the
/// `a == b` arm for class pairs without changing its result.
///
/// Only user-declared classes live in `ctx.classes`. Builtins such as
/// `Exception` are NOT registered there, so `is_subclass(MyErr, "Exception", _)`
/// returns `false` — exception subtyping deliberately stays unimplemented (see
/// design §D). A `visited` set guards against a malformed cyclic base chain so
/// this can never loop (single inheritance is already enforced at the resolver,
/// but the guard keeps this total regardless of input).
pub fn is_subclass(child: &str, ancestor: &str, ctx: &TyCtx) -> bool {
    // Reflexive: a class is a subclass of itself (mirrors the `a == b` fast path
    // in `types_compatible`). This holds for any name, registered or not.
    if child == ancestor {
        return true;
    }
    // For the strict-ancestor case the relationship is only recognized when the
    // walk stays inside USER-declared classes: we follow `bases` edges through
    // `ctx.classes` and report success only on reaching `ancestor` AS A
    // REGISTERED CLASS. A base naming a BUILTIN (e.g. `Exception`) is not in
    // `ctx.classes`, so it is never followed and never matched — exception
    // subtyping therefore stays unimplemented (design §D), which is correct.
    let mut current = child;
    let mut visited = std::collections::HashSet::new();
    loop {
        let def = match ctx.classes.get(current) {
            Some(d) => d,
            None => return false, // current is not a user class -> chain ends
        };
        if !visited.insert(current.to_string()) {
            return false; // cycle guard — defensive; single inheritance is enforced
        }
        // Single inheritance is enforced elsewhere (>1 base rejected upstream).
        // Follow the first base that is itself a registered class; a base that
        // is a builtin (not in ctx.classes) terminates the walk. We compare
        // against `ancestor` only AFTER confirming the base is a real class node,
        // so an unregistered base name can never satisfy the query.
        let next = def.bases.iter().find(|b| ctx.classes.contains_key(b.as_str()));
        match next {
            Some(base) if base == ancestor => return true,
            Some(base) => current = base,
            None => return false,
        }
    }
}

/// (EPIC-5 C2-2b-i) The nearest common ancestor of two user classes, or `None`
/// when they share no user-declared ancestor. Single inheritance makes each
/// class's ancestor chain linear, so we walk `a`'s chain (a, a's base, ...) and
/// return the first entry that is also an ancestor of `b` (`is_subclass(b, x)`).
/// Used to unify two SIBLING subclasses (`Dog`, `Cat`) flowing into one slot —
/// e.g. a `list[Animal] = [Dog(...), Cat(...)]` literal — to their common base
/// `Animal`. `is_subclass` is reflexive, so an ancestor/descendant pair already
/// resolves at the first step (covered by the explicit arms in
/// `unify_branch_types`); this only adds the sibling case.
pub fn nearest_common_ancestor(a: &str, b: &str, ctx: &TyCtx) -> Option<String> {
    let mut current = a;
    let mut visited = std::collections::HashSet::new();
    loop {
        if !visited.insert(current.to_string()) {
            return None; // cycle guard (defensive; single inheritance enforced)
        }
        // `current` is an ancestor of `b` (reflexively for `current == b`).
        if is_subclass(b, current, ctx) {
            return Some(current.to_string());
        }
        let def = ctx.classes.get(current)?;
        match def.bases.iter().find(|x| ctx.classes.contains_key(x.as_str())) {
            Some(base) => current = base,
            None => return None, // chain ended at a builtin/no base — no common ancestor
        }
    }
}

/// Map each `self.<field>` assigned (directly or via a chain of simple local
/// rebindings) from an `__init__` PARAMETER to that parameter's name. Shared by
/// codegen — to seed a generic / `Callable` field's struct-literal placeholder
/// with `<param>.clone()` instead of the unavailable `Default::default()` — and
/// by `check_class_prelude` — to reject a `Callable` field that is NOT seeded
/// from a constructor param (which would otherwise silently build-fail, since
/// `Rc<dyn Fn>` has no `Default`).
///
/// Resolution: a forward pass folds `tmp = g` / `tmp2 = tmp` ident aliases down
/// to the underlying param, so an INDIRECT `self.f = tmp` still resolves to `g`.
/// Only pure `local = <ident>` aliasing is tracked; any other RHS (a call, an
/// expression) clears the local, so `self.f = make_default()` resolves to
/// nothing and is reported as an honest error by the caller.
pub fn init_field_param_map(init_fn: &Func) -> std::collections::HashMap<String, String> {
    let param_names: std::collections::HashSet<&str> = init_fn.params.iter()
        .filter(|p| p.name != "self")
        .map(|p| p.name.as_str())
        .collect();
    let mut local_to_param: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut map = std::collections::HashMap::new();
    for stmt in &init_fn.body {
        match stmt {
            Stmt::Assign { target, value, .. } => {
                let resolved = match value {
                    Expr::Ident(src, _) if param_names.contains(src.as_str()) => Some(src.clone()),
                    Expr::Ident(src, _) => local_to_param.get(src).cloned(),
                    _ => None,
                };
                match resolved {
                    Some(p) => { local_to_param.insert(target.clone(), p); }
                    None => { local_to_param.remove(target); }
                }
            }
            Stmt::AttrAssign { obj, attr, value, .. } => {
                let is_self = matches!(obj.as_ref(), Expr::Ident(n, _) if n == "self");
                if is_self {
                    if let Expr::Ident(p, _) = value {
                        if param_names.contains(p.as_str()) {
                            map.insert(attr.clone(), p.clone());
                        } else if let Some(root) = local_to_param.get(p) {
                            map.insert(attr.clone(), root.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    map
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
pub(crate) fn infer_list_element_type(class_def: &ClassDef, field_name: &str) -> Option<String> {
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
pub(crate) fn guess_field_type(expr: &Expr, params: &std::collections::HashMap<String, TypeExpr>) -> TypeExpr {
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

