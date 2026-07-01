use super::*;

impl<'a> Codegen<'a> {
    pub(crate) fn emit_func(&mut self, f: &Func, method_of: Option<&str>) -> Result<()> {
        let is_static = f.decorators.contains(&"staticmethod".to_string());
        let name = if f.name == "main" && method_of.is_none() {
            "user_main".to_string()
        } else if method_of.is_none() {
            // (EPIC-6) Free-function name: escape a keyword name so the `fn` def
            // matches every call site (call sites emit the name through
            // `emit_expr`'s Ident arm, which escapes identically). METHOD names
            // are deliberately NOT escaped here — method-name escaping is the
            // sibling dispatch card's concern, and escaping only the definition
            // would desync it from the (untouched) dispatch call sites.
            escape_ident(&f.name)
        } else {
            f.name.clone()
        };
        // Generics v2 (PEP 695): a parametric generic function emits a Rust
        // generic-parameter clause `<T: Clone + PartialOrd, U: Clone + Display>`
        // right after the name. The bound set per type variable is the UNION of:
        //   - `Clone` (always — pyrst value-semantics clone-on-use; a type-var
        //     value is non-Copy, `is_copy(TypeVar)` is false, so it is cloned
        //     wherever a Copy value would be used directly), plus
        //   - every bound INFERRED from the body by `infer_func_typevar_bounds`
        //     (comparison -> PartialOrd, equality -> PartialEq, arithmetic ->
        //     Add/Sub/Mul/Div/Rem<Output=T>, Display contexts -> Display, set
        //     element / dict key -> Hash + Eq).
        // The same inference drives the typeck op-sites that now ACCEPT these ops
        // on a bare `T`, so the emitted bounds always cover exactly the ops the
        // body performs. A non-generic function has an empty `type_params` and
        // emits no clause. Methods are never generic in v1 (parser-rejected), so
        // this only matters for free functions, but the code is uniform.
        let generics = if f.type_params.is_empty() {
            String::new()
        } else {
            let inferred = crate::typeck::infer_func_typevar_bounds(f, self.ctx);
            let bounds = f.type_params.iter()
                .map(|t| {
                    // Preserve the declared type-param ORDER for the clause; within
                    // each var the bounds emit in `TypeVarBound`'s canonical order
                    // (Clone first) via the BTreeSet iteration.
                    let mut set = inferred.get(t).cloned().unwrap_or_default();
                    set.insert(crate::typeck::TypeVarBound::Clone);
                    let parts = set.iter()
                        .map(|b| b.rust_bound(t))
                        .collect::<Vec<_>>()
                        .join(" + ");
                    format!("{}: {}", t, parts)
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("<{}>", bounds)
        };
        // Generics v2 (generic CLASSES): the type-var names IN SCOPE for lowering
        // this function's param/return annotations = its own `type_params` PLUS,
        // when it is a method of a generic class, the class's type params (which
        // the enclosing `impl<T, ..> Box<T>` block declares). A method has empty
        // `f.type_params`, so for a generic class's method this is exactly the
        // class params; for a free function or a non-generic class's method it is
        // just `f.type_params` (the class list is empty), so the legacy path is
        // unchanged. The per-method generic CLAUSE above still uses only
        // `f.type_params`, so the class params are NOT re-declared on the method.
        let mut scope: Vec<String> = f.type_params.clone();
        if method_of.is_some() {
            for tp in &self.current_class_type_params {
                if !scope.contains(tp) {
                    scope.push(tp.clone());
                }
            }
        }
        // Make the in-scope type vars visible to the BODY-emission paths
        // (`prescan_types`, the annotated-assign declaration) so a local `acc: T`
        // lowers to `Ty::TypeVar("T")` — matching the param/return lowering and
        // the call-result oracle. Saved/restored so a nested/sibling fn never
        // inherits this set.
        let saved_fn_type_params = std::mem::replace(&mut self.current_fn_type_params, scope.clone());

        let mut sig = format!("fn {}{}(", name, generics);
        let mut first = true;
        // Static methods don't get self; regular methods take &self or &mut self based on whether they modify self.
        if let Some(cls) = method_of {
            if !is_static && f.params.iter().any(|p| p.name == "self") {
                // (EPIC-4 V3) Use the precomputed TRANSITIVE decision: a method
                // that mutates self only by calling a mutating `self.<helper>()`
                // is now `&mut self` too. Falls back to the intra-method seed for
                // synthesized funcs not in the pre-pass map (`__super_` aliases /
                // `__lt_impl`).
                let needs_mut = self.needs_mut_self(cls, &f.name, &f.body);
                if needs_mut {
                    sig.push_str("&mut self");
                } else {
                    sig.push_str("&self");
                }
                first = false;
            }
        }
        // Always skip `self` from the explicit params list.
        for p in f.params.iter().filter(|p| p.name != "self") {
            if !first { sig.push_str(", "); }
            first = false;
            let pty = self.rust_ty(&Ty::from_type_expr_scoped(&p.ty, p.span, &scope)?);
            if p.by_ref {
                // (EPIC-4 V2-c) An opt-in by-reference param (`Mut[T]`) becomes
                // `name: &mut T`. The callee's mutations persist to the caller,
                // which threads `&mut <place>` at the call site. No `mut` binding
                // prefix: the binding itself is the reference (already `&mut`);
                // field/method mutation and `place.clone()` reads both work
                // through auto-deref.
                let _ = write!(sig, "{}: &mut {}", escape_ident(&p.name), pty);
            } else {
                // Value params are bound `mut` so functions may mutate them or
                // their fields in place (Python passes mutable objects by
                // reference); unused-mut is allowed in the generated crate.
                // (EPIC-6) Escape a keyword param name; body uses resolve to the
                // same escaped form via emit_expr's Ident arm.
                let _ = write!(sig, "mut {}: {}", escape_ident(&p.name), pty);
            }
        }
        let ret = Ty::from_type_expr_scoped(&f.ret, f.span, &scope)?;
        let ret_s = self.rust_ty(&ret);
        let _ = write!(sig, ") -> {} {{", ret_s);

        // (FFI Phase 1) An `@extern` function is a Rust-FFI binding: its body is a
        // single string-literal holding a Rust EXPRESSION TEMPLATE, not pyrst
        // statements. We reuse the signature built above (same param + return
        // types), then emit the substituted template as the function's TAIL
        // expression and SKIP all the pyrst-body machinery below (prescan /
        // hoisting / by-ref bookkeeping / body-statement loop). typeck has already
        // verified the body shape and that the signature is fully typed, so the
        // body[0] string extraction below cannot fail for a checked program.
        if f.decorators.iter().any(|d| d == "extern") {
            self.line(&sig);
            self.indent += 1;
            let template = match f.body.first() {
                Some(Stmt::Expr(Expr::Str(s, _))) => s.clone(),
                // Defensive: `build` may be invoked without a prior `check`, so
                // re-assert the body shape here rather than panic on a bad AST.
                _ => {
                    return Err(crate::diag::Error::Codegen(
                        "`@extern` function body must be a single Rust-template \
                         string literal"
                            .to_string(),
                    ))
                }
            };
            // Substitute each `{param}` hole with the param's emitted Rust
            // identifier (`escape_ident` matches how call sites would name it).
            // `self` is skipped (an @extern free function has none). Phase 1 does
            // NOT handle `{{`/`}}` literal-brace escaping — the template is a
            // direct expression and bare braces are uncommon there; documented.
            // A hole naming no param (a typo like `{nane}`) is left literal and
            // surfaces as a rustc error (E0425) at build — intentional for the FFI
            // escape hatch: the template is opaque Rust, validated by rustc.
            let mut emitted = template;
            for p in f.params.iter().filter(|p| p.name != "self") {
                let hole = format!("{{{}}}", p.name);
                emitted = emitted.replace(&hole, &escape_ident(&p.name));
            }
            // Tail expression: no trailing `;`, so the template value is returned.
            self.line(&emitted);
            self.indent -= 1;
            self.line("}");
            self.line("");
            return Ok(());
        }

        self.line(&sig);
        self.indent += 1;
        // (EPIC-5) Track the active return type for Some/None wrapping in `return`.
        let saved_ret_ty = std::mem::replace(&mut self.current_ret_ty, ret.clone());
        // Generators (EAGER v1): a function whose body contains `yield` collects
        // its yielded values into a `Vec<T>` and returns it. typeck has already
        // verified the signature is `Iterator[T]` (which lowered `ret` to
        // `Ty::List(T)` -> a `Vec<T>` Rust return), so detecting the `yield` in
        // the body is enough to switch on the desugar here. The flag is
        // saved/restored so a nested function never inherits it.
        let is_generator =
            crate::typeck::body_contains_yield(&f.body) && matches!(ret, Ty::List(_));
        let saved_in_generator = std::mem::replace(&mut self.in_generator, is_generator);
        // (try/except control flow) A nested `def` owns its own control flow:
        // a `return`/`break`/`continue` inside it is local to that function (or
        // its own loops), NOT an escape from an enclosing `try:` body. Suspend
        // both try-escape flags for the duration of this function's emission so
        // a nested def emitted while the parent was lowering a try body lowers
        // its `return` as a plain `return`, not `__PyrstTryFlow::Return`. Saved
        // and restored like `current_ret_ty` / `in_generator`.
        let saved_try_return_escape = std::mem::replace(&mut self.try_return_escape, false);
        let saved_try_loopctl_escape = std::mem::replace(&mut self.try_loopctl_escape, false);
        if is_generator {
            // The accumulator the lowered `yield`s push into and the function
            // returns. Typed to the Rust return type (`Vec<T>`) so an empty
            // generator and element inference both resolve without annotation
            // churn. The `__pyrst_` prefix is reserved (typeck rejects user
            // identifiers under it), so this name can never collide with a user
            // local. See the `Stmt::Yield` arm in `emit_stmt`.
            self.line(&format!("let mut __pyrst_gen_acc: {} = Vec::new();", ret_s));
        }
        // (EPIC-4 V2-c) Track this function's by-reference (`&mut T`) param
        // bindings so a forwarded-by-reference arg that names one emits an
        // explicit reborrow (`&mut *x`) instead of a double `&mut` (E0596).
        // Save+restore so a nested func/method emission can never leak its set
        // into the enclosing one (mirrors `current_ret_ty`).
        let saved_by_ref = std::mem::take(&mut self.by_ref_locals);
        for p in f.params.iter().filter(|p| p.name != "self") {
            if p.by_ref {
                self.by_ref_locals.insert(p.name.clone());
            }
        }

        // Populate locals from parameters
        // Register self with its class type if this is a method. Generics v2: for
        // a generic class, type `self` as `Ty::Class(cls, [TypeVar(T), ..])` so a
        // field read `self.value` resolves through the type-var-bearing field/
        // method machinery and types as the class's `T` (which `rust_ty` lowers to
        // the impl's generic `T`). A non-generic class has empty
        // `current_class_type_params`, so `self` is the plain `Ty::Class(cls, [])`.
        if let Some(cls) = method_of {
            let self_args: Vec<Ty> = self.current_class_type_params.iter()
                .map(|tp| Ty::TypeVar(tp.clone()))
                .collect();
            self.locals.insert("self".to_string(), Ty::Class(cls.to_string(), self_args));
        }

        for p in &f.params {
            if p.name != "self" {
                // Generics v2: scope param-local types with the in-scope type vars
                // (own + enclosing generic class) so a `v: T` local is `TypeVar(T)`.
                let ty = Ty::from_type_expr_scoped(&p.ty, p.span, &scope)?;
                self.locals.insert(p.name.clone(), ty);
                // (param-mutation fix) A value parameter is ALREADY a `let mut`
                // binding (emitted `mut <name>: T` above) at function scope. Seed
                // its name into `declared` so a later reassignment — at the body
                // top level OR nested inside a while/for/if/try block — lowers to a
                // MUTATION (`x = ...;`) of that binding, not a fresh shadowing
                // `let mut x = ...;` scoped to the inner block. Without this, a loop
                // whose condition reads the param never sees the update (the inner
                // shadow dies at the block end) -> infinite loop / wrong result.
                // BY-REFERENCE params (`Mut[T]` -> `&mut T`) are deliberately NOT
                // seeded: their reassignment semantics are a separate concern and
                // seeding them would turn the existing shadowing `let` into an
                // ill-typed reference rebind. Value params are pyrst's common case
                // and are function-scoped, so a reassignment anywhere is a mutation.
                if !p.by_ref {
                    self.declared.insert(p.name.clone());
                }
            }
        }

        // First pass: forward type inference over the whole body (including
        // nested blocks) so un-annotated locals are typed from their value and
        // refined by later uses (e.g. `d = {}` then `d[k] = some_str`, or an
        // `acc = 0` accumulator later assigned a float).
        self.prescan_types(&f.body);

        // Python has function scope, but pyrst emits if/for/while/with/try bodies
        // as Rust `{}` blocks. Hoist locals first-assigned inside a block to
        // function scope (with a safe default) so they stay visible after the
        // block. Params, unpack targets, and names whose type has no usable
        // default keep their in-place `let` (and the prior block-scope limit).
        let mut block_assigned = std::collections::HashSet::new();
        let mut unpack_targets = std::collections::HashSet::new();
        Self::collect_hoistable(&f.body, 0, &mut block_assigned, &mut unpack_targets);
        let params: std::collections::HashSet<&str> = f.params.iter().map(|p| p.name.as_str()).collect();
        let mut hoist: Vec<String> = block_assigned.into_iter()
            .filter(|n| !unpack_targets.contains(n) && !params.contains(n.as_str()) && !self.declared.contains(n))
            .collect();
        hoist.sort();
        for name in hoist {
            let ty = self.locals.get(&name).cloned().unwrap_or(Ty::Unknown);
            if let Some(def) = self.default_val(&ty) {
                // (EPIC-6) Escape the hoisted local's emitted name; the raw name
                // stays the `declared`/`locals` key.
                self.line(&format!("let mut {}: {} = {};", escape_ident(&name), self.rust_ty(&ty), def));
                self.declared.insert(name);
            }
        }

        for s in &f.body {
            self.emit_stmt(s)?;
        }
        // Generators: fall-off-the-end returns the collected Vec. (A bare
        // `return` inside the body also lowers to `return __pyrst_gen_acc;` via
        // emit_stmt, so collection stops there; this final return covers the
        // normal path where control reaches the end of the body.)
        if is_generator {
            self.line("return __pyrst_gen_acc;");
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        // Clear locals and declared for next function
        self.locals.clear();
        self.declared.clear();
        self.current_ret_ty = saved_ret_ty;
        self.by_ref_locals = saved_by_ref;
        self.in_generator = saved_in_generator;
        self.try_return_escape = saved_try_return_escape;
        self.try_loopctl_escape = saved_try_loopctl_escape;
        self.current_fn_type_params = saved_fn_type_params;
        Ok(())
    }

    /// Resolve the full set of methods visible on `class_name` across its entire
    /// base chain (transitive), with the nearest-defining class winning — the
    /// same MRO walk TyCtx::get_method / get_all_fields use, but returning the
    /// actual `Func` (with body) so an inherited method or dunder can be emitted
    /// onto the subclass's own value-struct.
    ///
    /// Ordering: own methods first (in source order), then each inherited name
    /// the first time it is reached walking bases depth-first. A name already
    /// resolved (by the class itself or a nearer ancestor) is never overwritten,
    /// so overrides shadow inherited definitions exactly like Python's MRO.
    pub(crate) fn resolved_methods(&self, class_name: &str) -> Vec<Func> {
        let mut out: Vec<Func> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        self.collect_resolved_methods(class_name, &mut out, &mut seen, &mut visited);
        out
    }

    pub(crate) fn collect_resolved_methods(
        &self,
        class_name: &str,
        out: &mut Vec<Func>,
        seen: &mut std::collections::HashSet<String>,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if visited.contains(class_name) {
            return;
        }
        visited.insert(class_name.to_string());
        if let Some(cd) = self.ctx.classes.get(class_name) {
            // This class's own methods take precedence over anything inherited.
            for m in &cd.methods {
                if seen.insert(m.name.clone()) {
                    out.push(m.clone());
                }
            }
            // Then inherited methods, nearer bases before farther ones.
            for base in &cd.bases {
                self.collect_resolved_methods(base, out, seen, visited);
            }
        }
    }

    pub(crate) fn emit_class(&mut self, c: &ClassDef) -> Result<()> {
        // Full transitive field set (ancestors first, then own; deduped by name),
        // sourced from typeck's get_all_fields so the struct layout, Copy/Default
        // derivation, and default_val agree even for multi-level inheritance
        // (a manual one-level walk dropped grandparent fields).
        let mut all_fields: Vec<Param> = Vec::new();
        for f in self.ctx.get_all_fields(&c.name) {
            if !all_fields.iter().any(|ef: &Param| ef.name == f.name) {
                all_fields.push(f);
            }
        }

        // Full transitive method set (own methods win, then inherited via MRO),
        // so a subclass that INHERITS a dunder or a grandparent method emits it
        // onto its own value-struct. Dunder trait impls, the `__lt_impl` helper,
        // and the inherited-method emission all key off this set rather than
        // `c.methods` (which is the defining class only).
        let resolved_methods = self.resolved_methods(&c.name);

        let all_fields_copy = all_fields.iter().all(|f| {
            Ty::from_type_expr(&f.ty, f.span)
                .map(|ty| self.is_copy_type(&ty))
                .unwrap_or(false)
        });

        // A user-defined __eq__ (own OR inherited) emits a manual `impl PartialEq`,
        // so don't ALSO derive it (that would be a conflicting-impl error, E0119)
        // and don't fall back to a field-wise derived eq that ignores the
        // inherited custom semantics.
        let has_eq = resolved_methods.iter().any(|m| m.name == "__eq__");
        // (EPIC-5 C2-3) A field whose type is a polymorphic base lowers to the
        // companion enum `B__`. A `#[derive(PartialEq)]` on this struct then
        // requires `B__: PartialEq`, which holds ONLY when every variant defines
        // `__eq__`. If any polymorphic-base field's enum lacks PartialEq, derive
        // without it (Python `==` on such a struct is then honestly unavailable
        // — consistent with cross-variant equality being absent on the enum).
        let field_blocks_eq = all_fields.iter().any(|f| {
            matches!(Ty::from_type_expr(&f.ty, f.span), Ok(Ty::Class(ref n, _))
                if self.is_polymorphic_base(n) && !self.companion_enum_has_partial_eq(n))
        });
        // A `Callable` field lowers to `Rc<dyn Fn(..) -> ..>`, which implements
        // neither `Debug` nor `PartialEq` (and has no `Default`). A struct holding
        // one therefore cannot derive `Debug`/`PartialEq` — Python's `repr()` and
        // `==` on such an object are honestly unavailable, matching the trait
        // object's own lack of those impls. (The field is scoped with the class
        // type params so a `Callable[[], V]` field on a generic class is seen.)
        let has_func_field = all_fields.iter().any(|f| {
            Ty::from_type_expr_scoped(&f.ty, f.span, &c.type_params)
                .map(|ty| self.ty_has_func(&ty))
                .unwrap_or(false)
        });
        let pe = if has_eq || field_blocks_eq || has_func_field { "" } else { ", PartialEq" };
        let dbg = if has_func_field { "" } else { ", Debug" };
        // Only derive Default when every field actually implements Default.
        // Copy classes (all-primitive fields) don't derive Default, so an outer
        // struct holding one must NOT include Default in its own derive list.
        let all_fields_default = all_fields.iter().all(|f| {
            Ty::from_type_expr(&f.ty, f.span)
                .map(|ty| self.type_has_default(&ty))
                .unwrap_or(false)
        });
        let derives = if all_fields_copy {
            format!("#[derive(Copy, Clone{}{})]", dbg, pe)
        } else if all_fields_default {
            format!("#[derive(Clone{}{}, Default)]", dbg, pe)
        } else {
            format!("#[derive(Clone{}{})]", dbg, pe)
        };
        // Generics v2 (generic CLASSES): the type-parameter clauses threaded
        // through this class's struct + impl + method emission.
        //   - `struct_generics` = `<T, U>` on the STRUCT declaration (no bounds:
        //     `struct Box<T> { value: T }`).
        //   - `impl_generics`   = `<T: Clone + ..>` on the IMPL header, carrying
        //     the bounds inferred from the ops the methods perform on `T`
        //     (reusing the generic-FUNCTION bound machinery via
        //     `infer_class_typevar_bounds`).
        //   - `ty_args`         = `<T, U>` after the type name in the impl head
        //     and in trait-impl heads (`impl<T> Box<T>`).
        // All three are empty for a non-generic class, so its emission is
        // byte-for-byte unchanged. While the impl block is open we set
        // `current_class_type_params` so method sigs and field lowering see the
        // class type vars in scope (a `v: T` lowers to the Rust `T`).
        let (struct_generics, impl_generics, ty_args) = self.class_generic_clauses(c);

        self.line(&derives);
        self.line(&format!("struct {}{} {{", c.name, struct_generics));
        self.indent += 1;
        for f in &all_fields {
            // Generics v2: scope the field annotation with the class type params
            // so a `value: T` field lowers to the Rust generic `T` (not a `T`
            // struct). Empty for a non-generic class.
            let ty = Ty::from_type_expr_scoped(&f.ty, f.span, &c.type_params)?;
            // (EPIC-6) Escape a keyword field name in the struct definition; every
            // field read/write/init escapes the same way so they stay in sync.
            self.line(&format!("{}: {},", escape_ident(&f.name), self.rust_ty(&ty)));
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        self.current_class = Some(c.name.clone());
        // In scope for every method/field lowering until the end of emit_class.
        let saved_class_tps = std::mem::replace(&mut self.current_class_type_params, c.type_params.clone());

        // Generics v2: the RECEIVER TYPE for `self` / `other` inside this class's
        // methods and dunder impls. For a generic class it is `Box<T>` (so field
        // reads resolve the type-var-bearing field types and a `&Box` param spells
        // `&Box<T>` — without the `<T>` rustc raises E0107 "missing generics");
        // for a non-generic class it is the bare struct name `Point`, so every
        // existing dunder impl is byte-for-byte unchanged.
        let recv_ty_str = format!("{}{}", c.name, ty_args);
        // The `Ty` typing for a `self`/`other` local — carries the class's type
        // params as type vars for a generic class (drives field-read lowering),
        // empty args for a non-generic class (the legacy `Ty::Class(name, [])`).
        let self_class_ty = Ty::Class(
            c.name.clone(),
            c.type_params.iter().map(|tp| Ty::TypeVar(tp.clone())).collect(),
        );

        // Dunder methods that become Rust trait impls instead of regular methods.
        let dunder_trait_names = DUNDER_TRAIT_NAMES;

        // Constructor emission keys off the OWN __init__ (the existing model:
        // every subclass in scope defines its own __init__). The inherent-impl
        // block, however, must also open when the class only INHERITS methods
        // (a regular method or __lt__ pulled from an ancestor), so those flags
        // read from the resolved (transitive) set.
        let has_init = c.methods.iter().any(|m| m.name == "__init__");
        let has_lt = resolved_methods.iter().any(|m| m.name == "__lt__");
        let is_dataclass = c.is_dataclass;
        let has_regular_methods = resolved_methods.iter().any(|m|
            m.name != "__init__" && !dunder_trait_names.contains(&m.name.as_str())) || has_lt;

        if has_init || has_regular_methods || (is_dataclass && !has_init) {
            // Generics v2: `impl<T: Clone + ..> Box<T> { .. }`. Bounds clause +
            // type-args; both empty for a non-generic class (`impl Point { .. }`).
            self.line(&format!("impl{} {}{} {{", impl_generics, c.name, ty_args));
            self.indent += 1;

            // Emit new() constructor when __init__ is defined.
            if has_init {
                if let Some(init_fn) = c.methods.iter().find(|m| m.name == "__init__").cloned() {
                    let non_self: Vec<_> = init_fn.params.iter().filter(|p| p.name != "self").collect();
                    let param_strs: Result<Vec<_>> = non_self.iter()
                        .map(|p| {
                            // Generics v2: scope `__init__`'s params so `v: T`
                            // becomes the impl's generic `T`.
                            let ty = Ty::from_type_expr_scoped(&p.ty, p.span, &c.type_params)?;
                            // (EPIC-6) new()'s params + their forwarded uses below
                            // escape identically.
                            Ok(format!("{}: {}", escape_ident(&p.name), self.rust_ty(&ty)))
                        })
                        .collect();
                    let param_strs = param_strs?;
                    let param_names: Vec<_> = non_self.iter().map(|p| escape_ident(&p.name)).collect();
                    // Generics v2: a field of type-var type (`value: T`) has NO Rust
                    // `Default` (we don't bound `T: Default` — that would reject a
                    // `Box[NonDefault]`), so the zero-then-`__init__` placeholder
                    // `Default::default()` won't compile. For a GENERIC class we seed
                    // each placeholder from the `__init__` param the field is directly
                    // assigned (`self.value = v` -> `value: v.clone()`); `T: Clone` is
                    // always bounded, and `__init__` then runs as usual (it just
                    // re-assigns). A field NOT directly param-assigned (computed in
                    // `__init__`) keeps `zeroed_default` — for Box/Pair every field is
                    // param-assigned, so this is exact. The non-generic path is
                    // unaffected (`type_params` empty => the map is empty => the old
                    // `zeroed_default` branch is taken for every field, byte-for-byte).
                    // The field<-param map is also needed for a `Callable` field: a
                    // `Rc<dyn Fn>` field has no `Default`, so its zero-then-`__init__`
                    // placeholder must clone the directly-assigned `__init__` param
                    // (which is already the `Rc<dyn Fn>` value) rather than
                    // `Default::default()`. Build the map when the class is generic OR
                    // holds a func field; a class that is neither keeps the empty map
                    // and the legacy `zeroed_default` path byte-for-byte.
                    let init_field_params = if c.type_params.is_empty() && !has_func_field {
                        std::collections::HashMap::new()
                    } else {
                        Self::init_field_param_map(&init_fn)
                    };
                    let defaults: Vec<String> = all_fields.iter().map(|f| {
                        let ty = Ty::from_type_expr_scoped(&f.ty, f.span, &c.type_params).unwrap_or(Ty::Unknown);
                        // A type-var-bearing OR func-typed field with a known init
                        // param: clone that param as the placeholder (`T: Clone` is
                        // always bounded; an `Rc<dyn Fn>` clones cheaply). Otherwise
                        // the legacy zeroed default.
                        let dv = if crate::typeck::ty_contains_typevar(&ty) || self.ty_has_func(&ty) {
                            if let Some(param) = init_field_params.get(&f.name) {
                                format!("{}.clone()", escape_ident(param))
                            } else {
                                self.zeroed_default(&ty)
                            }
                        } else {
                            // Use zeroed_default which handles Copy classes that don't
                            // implement Default (unlike a plain Default::default() call).
                            self.zeroed_default(&ty)
                        };
                        // (EPIC-6) Escape a keyword field name in the struct-literal
                        // initializer (matches the escaped struct field def).
                        format!("{}: {}", escape_ident(&f.name), dv)
                    }).collect();
                    // The throwaway struct literal: a NON-generic class names the
                    // struct directly (`Vector { .. }`) — byte-for-byte the legacy
                    // emission. A GENERIC class uses `Self { .. }` so the literal
                    // resolves to `Box<T>` WITHOUT writing explicit type args (Rust
                    // infers `T` once `__init__` fills the `T`-typed field). Keeping
                    // the non-generic case on `c.name` preserves byte-identity for
                    // every existing class.
                    let inst_ty = if c.type_params.is_empty() { c.name.clone() } else { "Self".to_string() };
                    self.line(&format!("fn new({}) -> Self {{", param_strs.join(", ")));
                    self.indent += 1;
                    self.line(&format!("let mut __inst = {} {{ {} }};", inst_ty, defaults.join(", ")));
                    self.line(&format!("__inst.__init__({});", param_names.join(", ")));
                    self.line("__inst");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
            }

            // Auto-generate constructor for @dataclass without __init__
            if is_dataclass && !has_init {
                let param_strs: Result<Vec<_>> = all_fields.iter()
                    .map(|f| {
                        let ty = Ty::from_type_expr(&f.ty, f.span)?;
                        // (EPIC-6) Dataclass ctor: param name == field name; both
                        // the param binding here and the field-init shorthand below
                        // escape, so `ClassName { r#type, .. }` stays consistent.
                        Ok(format!("{}: {}", escape_ident(&f.name), self.rust_ty(&ty)))
                    })
                    .collect();
                let param_strs = param_strs?;
                let field_inits: Vec<_> = all_fields.iter().map(|f| escape_ident(&f.name)).collect();
                self.line(&format!("fn new({}) -> Self {{", param_strs.join(", ")));
                self.indent += 1;
                self.line(&format!("{} {{ {} }}", c.name, field_inits.join(", ")));
                self.indent -= 1;
                self.line("}");
                self.line("");
            }

            // Emit all methods except dunder-trait ones, drawn from the resolved
            // (transitive) set so inherited and grandparent methods land on this
            // class's value-struct. `resolved_methods` already applies MRO (own
            // wins, then nearer bases), so each name appears once.
            let class_name = c.name.clone();

            // Own method names identify overrides (those still get a __super_
            // alias so `super().m()` can reach the immediate parent body).
            let own_method_names: std::collections::HashSet<String> = c.methods.iter()
                .map(|m| m.name.clone())
                .collect();

            // Emit __super_ aliases for OWN methods that override an immediate
            // parent method (one-level, unchanged: super() targets the parent).
            for base in &c.bases {
                if let Some(base_def) = self.ctx.classes.get(base.as_str()).cloned() {
                    for m in &base_def.methods {
                        if !dunder_trait_names.contains(&m.name.as_str()) && own_method_names.contains(&m.name) {
                            // Child overrides this parent method — emit __super_ alias
                            let mut super_m = m.clone();
                            super_m.name = format!("__super_{}", m.name);
                            self.emit_func(&super_m, Some(&class_name))?;
                        }
                    }
                }
            }

            // Emit every resolved regular method (own + transitively inherited),
            // including __init__ itself (the constructor's new() calls
            // self.__init__(...), so it must exist as an inherent method).
            // Dunder-trait methods become trait impls below, except __lt__ which
            // also needs a __lt_impl helper that PartialOrd::partial_cmp calls.
            for m in &resolved_methods {
                if dunder_trait_names.contains(&m.name.as_str()) {
                    // Special handling for __lt__: emit as __lt_impl (own or inherited).
                    if m.name == "__lt__" {
                        // Inside the inherent `impl<..> Box<T>` block: the receiver
                        // type is `Box<T>` (`recv_ty_str`); the impl header already
                        // declares `<T>`, so the helper takes no clause of its own.
                        self.line(&format!("fn __lt_impl(&self, other: &{}) -> bool {{", recv_ty_str));
                        self.indent += 1;
                        self.locals.insert("self".into(), self_class_ty.clone());
                        self.locals.insert("other".into(), self_class_ty.clone());
                        // `other: &c.name` is the CONCRETE struct here — exempt it
                        // from base-field-read lowering (C2-2b-i).
                        self.concrete_struct_params.insert("other".into());
                        for s in &m.body { self.emit_stmt(s)?; }
                        self.concrete_struct_params.remove("other");
                        self.locals.remove("self");
                        self.locals.remove("other");
                        self.indent -= 1;
                        self.line("}");
                        self.line("");
                    }
                    continue;
                }
                self.emit_func(m, Some(&class_name))?;
            }

            self.indent -= 1;
            self.line("}");
            self.line("");
        }

        // Emit trait implementations for dunder methods, drawn from the resolved
        // (transitive) set so an inherited __str__/__eq__/__lt__/etc. produces the
        // matching Display/PartialEq/PartialOrd impl on THIS class's value-struct
        // (the impl body is the ancestor's, but every reference is to c.name and
        // to fields that get_all_fields guarantees exist on the subclass).
        let c_methods = resolved_methods.clone();

        // Both __str__ and __repr__ lower to the SAME Rust trait (Display), so a
        // class that defines/inherits BOTH would otherwise emit two `impl Display`
        // → rustc E0119 (conflicting impl). Dedup by the Rust TRAIT, not the Python
        // method name: pick a single Display source, PREFERRING __str__ (Python uses
        // __str__ for str()/print — the user-facing one), else __repr__. The chosen
        // name is matched against the resolved set so the dedup also holds across
        // inheritance (e.g. inherited __str__ + local __repr__).
        let display_source: Option<&str> = if c_methods.iter().any(|m| m.name == "__str__") {
            Some("__str__")
        } else if c_methods.iter().any(|m| m.name == "__repr__") {
            Some("__repr__")
        } else {
            None
        };

        for m in &c_methods {
            match m.name.as_str() {
                "__str__" | "__repr__" if Some(m.name.as_str()) == display_source => {
                    self.line(&format!("impl{} ::std::fmt::Display for {} {{", impl_generics, recv_ty_str));
                    self.indent += 1;
                    self.line("fn fmt(&self, __f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {");
                    self.indent += 1;
                    self.locals.insert("self".into(), self_class_ty.clone());
                    let body = &m.body;
                    let split_at = if body.is_empty() { 0 } else { body.len() - 1 };
                    for s in &body[..split_at] { self.emit_stmt(s)?; }
                    if let Some(Stmt::Return(Some(e), _)) = body.last() {
                        let s = self.emit_expr(e)?;
                        self.line(&format!("write!(__f, \"{{}}\", {})", s));
                    } else {
                        if let Some(s) = body.last() { self.emit_stmt(s)?; }
                        self.line("Ok(())");
                    }
                    self.locals.remove("self");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__add__" => {
                    let other_param = m.params.iter().find(|p| p.name == "other");
                    let other_ty = other_param
                        .map(|p| Ty::from_type_expr_scoped(&p.ty, p.span, &c.type_params).unwrap_or(Ty::Class(c.name.clone(), vec![])))
                        .unwrap_or(Ty::Class(c.name.clone(), vec![]));
                    let ret_ty = Ty::from_type_expr_scoped(&m.ret, m.span, &c.type_params).unwrap_or(Ty::Class(c.name.clone(), vec![]));
                    let other_s = self.dunder_operand_rust_ty(&other_ty, &c.name, &self_class_ty);
                    let ret_s = self.dunder_operand_rust_ty(&ret_ty, &c.name, &self_class_ty);
                    self.line(&format!("impl{} ::std::ops::Add<{}> for {} {{", impl_generics, other_s, recv_ty_str));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", ret_s));
                    self.line(&format!("fn add(self, other: {}) -> {} {{", other_s, ret_s));
                    self.indent += 1;
                    self.locals.insert("self".into(), self_class_ty.clone());
                    self.locals.insert("other".into(), other_ty);
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.locals.remove("self");
                    self.locals.remove("other");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__eq__" => {
                    self.line(&format!("impl{} ::std::cmp::PartialEq for {} {{", impl_generics, recv_ty_str));
                    self.indent += 1;
                    self.line(&format!("fn eq(&self, other: &{}) -> bool {{", recv_ty_str));
                    self.indent += 1;
                    self.locals.insert("self".into(), self_class_ty.clone());
                    self.locals.insert("other".into(), self_class_ty.clone());
                    // `other: &c.name` is the CONCRETE struct here — exempt it
                    // from base-field-read lowering (C2-2b-i).
                    self.concrete_struct_params.insert("other".into());
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.concrete_struct_params.remove("other");
                    self.locals.remove("self");
                    self.locals.remove("other");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__sub__" => {
                    let other_param = m.params.iter().find(|p| p.name == "other");
                    let other_ty = other_param
                        .map(|p| Ty::from_type_expr_scoped(&p.ty, p.span, &c.type_params).unwrap_or(Ty::Class(c.name.clone(), vec![])))
                        .unwrap_or(Ty::Class(c.name.clone(), vec![]));
                    let ret_ty = Ty::from_type_expr_scoped(&m.ret, m.span, &c.type_params).unwrap_or(Ty::Class(c.name.clone(), vec![]));
                    let other_s = self.dunder_operand_rust_ty(&other_ty, &c.name, &self_class_ty);
                    let ret_s = self.dunder_operand_rust_ty(&ret_ty, &c.name, &self_class_ty);
                    self.line(&format!("impl{} ::std::ops::Sub<{}> for {} {{", impl_generics, other_s, recv_ty_str));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", ret_s));
                    self.line(&format!("fn sub(self, other: {}) -> {} {{", other_s, ret_s));
                    self.indent += 1;
                    self.locals.insert("self".into(), self_class_ty.clone());
                    self.locals.insert("other".into(), other_ty);
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.locals.remove("self");
                    self.locals.remove("other");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__mul__" => {
                    let other_param = m.params.iter().find(|p| p.name == "other");
                    let other_ty = other_param
                        .map(|p| Ty::from_type_expr_scoped(&p.ty, p.span, &c.type_params).unwrap_or(Ty::Class(c.name.clone(), vec![])))
                        .unwrap_or(Ty::Class(c.name.clone(), vec![]));
                    let ret_ty = Ty::from_type_expr_scoped(&m.ret, m.span, &c.type_params).unwrap_or(Ty::Class(c.name.clone(), vec![]));
                    let other_s = self.dunder_operand_rust_ty(&other_ty, &c.name, &self_class_ty);
                    let ret_s = self.dunder_operand_rust_ty(&ret_ty, &c.name, &self_class_ty);
                    self.line(&format!("impl{} ::std::ops::Mul<{}> for {} {{", impl_generics, other_s, recv_ty_str));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", ret_s));
                    self.line(&format!("fn mul(self, other: {}) -> {} {{", other_s, ret_s));
                    self.indent += 1;
                    self.locals.insert("self".into(), self_class_ty.clone());
                    self.locals.insert("other".into(), other_ty);
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.locals.remove("self");
                    self.locals.remove("other");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__neg__" => {
                    let ret_ty = Ty::from_type_expr_scoped(&m.ret, m.span, &c.type_params).unwrap_or(Ty::Class(c.name.clone(), vec![]));
                    let ret_s = self.dunder_operand_rust_ty(&ret_ty, &c.name, &self_class_ty);
                    self.line(&format!("impl{} ::std::ops::Neg for {} {{", impl_generics, recv_ty_str));
                    self.indent += 1;
                    self.line(&format!("type Output = {};", ret_s));
                    self.line(&format!("fn neg(self) -> {} {{", ret_s));
                    self.indent += 1;
                    self.locals.insert("self".into(), self_class_ty.clone());
                    for s in &m.body { self.emit_stmt(s)?; }
                    self.locals.remove("self");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                "__lt__" => {
                    self.line(&format!("impl{} ::std::cmp::PartialOrd for {} {{", impl_generics, recv_ty_str));
                    self.indent += 1;
                    self.line(&format!("fn partial_cmp(&self, other: &{}) -> Option<::std::cmp::Ordering> {{", recv_ty_str));
                    self.indent += 1;
                    self.line("if self.__lt_impl(other) { Some(::std::cmp::Ordering::Less) }");
                    self.line("else if other.__lt_impl(self) { Some(::std::cmp::Ordering::Greater) }");
                    self.line("else { Some(::std::cmp::Ordering::Equal) }");
                    self.indent -= 1;
                    self.line("}");
                    self.indent -= 1;
                    self.line("}");
                    self.line("");
                }
                _ => {}
            }
        }

        self.current_class = None;
        self.current_class_type_params = saved_class_tps;
        Ok(())
    }

    /// Generics v2 (generic CLASSES): build the three type-parameter clause
    /// strings threaded through a class's emission, for the class `c`:
    ///   - `.0` STRUCT generics `<T, U>` (no bounds) for `struct Box<T> { .. }`;
    ///   - `.1` IMPL bounds clause `<T: Clone + PartialOrd, ..>` for the impl
    ///     header, the per-`T` bounds inferred from the ops the methods perform
    ///     (reusing `infer_class_typevar_bounds`, the class analogue of the
    ///     generic-function bound inference); and
    ///   - `.2` the type-args `<T, U>` that follow the type name in the impl head
    ///     (`impl<..> Box<T>`).
    /// All three are the EMPTY string for a non-generic class, so its struct/impl
    /// emit byte-for-byte as before. The bound emission order matches `emit_func`:
    /// declared type-param order across vars, `TypeVarBound`'s canonical order
    /// (Clone first) within each var.
    pub(crate) fn class_generic_clauses(&self, c: &ClassDef) -> (String, String, String) {
        if c.type_params.is_empty() {
            return (String::new(), String::new(), String::new());
        }
        let struct_generics = format!("<{}>", c.type_params.join(", "));
        let ty_args = format!("<{}>", c.type_params.join(", "));
        let inferred = crate::typeck::infer_class_typevar_bounds(c, self.ctx);
        let bounds = c.type_params.iter()
            .map(|t| {
                let mut set = inferred.get(t).cloned().unwrap_or_default();
                set.insert(crate::typeck::TypeVarBound::Clone);
                let parts = set.iter()
                    .map(|b| b.rust_bound(t))
                    .collect::<Vec<_>>()
                    .join(" + ");
                format!("{}: {}", t, parts)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let impl_generics = format!("<{}>", bounds);
        (struct_generics, impl_generics, ty_args)
    }

    /// Generics v2 (generic CLASSES): the Rust type string for a DUNDER operand /
    /// return type. A dunder like `__add__(self, other: Box) -> Box` annotates the
    /// operand and result with the BARE class name, which lowers to
    /// `Ty::Class("Box", [])` and would `rust_ty` to `Box` — but inside a generic
    /// class's `impl<T> .. for Box<T>` that bare `Box` is E0107 ("missing
    /// generics"). When `ty` is exactly THIS class with no args, substitute the
    /// class's own type args (`Box<T>`); otherwise (a concrete other-type operand,
    /// or a non-generic class) defer to the normal `rust_ty`. `self_ty` is the
    /// already-built `Ty::Class(name, [TypeVar..])` for the class.
    pub(crate) fn dunder_operand_rust_ty(&self, ty: &Ty, class_name: &str, self_ty: &Ty) -> String {
        if let Ty::Class(n, args) = ty {
            if n == class_name && args.is_empty() {
                return self.rust_ty(self_ty);
            }
        }
        self.rust_ty(ty)
    }

    /// Map each field assigned (directly OR via a chain of local rebindings) from
    /// an `__init__` PARAMETER to that parameter NAME, so a generic / `Callable`
    /// field's struct-literal placeholder is seeded with `<param>.clone()` instead
    /// of the unavailable `Default::default()`. Thin wrapper over the shared
    /// `typeck::init_field_param_map` (single source of truth — see its doc).
    pub(crate) fn init_field_param_map(init_fn: &Func) -> std::collections::HashMap<String, String> {
        // Single source of truth in typeck: the same field<-param resolution drives
        // BOTH the constructor placeholder seed (here) and the honest-error check
        // (`check_class_prelude`), so the two can never drift on which fields are
        // considered param-seeded. Follows one-step+chained local rebinding.
        crate::typeck::init_field_param_map(init_fn)
    }

    /// (EPIC-5 C2-2a) Emit the closed-set companion enum + method-dispatch impl +
    /// field-accessor impl for every polymorphic base, as DEAD CODE that must
    /// compile. C2-2a leaves all of this UNUSED — `rust_ty` still emits plain `n`,
    /// constructors are not wrapped, and the C1 honest gate stays — so output is
    /// byte-for-byte identical and the golden suite staying green proves the
    /// emitted Rust compiles. C2-2b activates it (flips `rust_ty` to `n__`, wraps
    /// constructors, removes the gate, lowers field access to the accessors).
    ///
    /// Run from `emit_program` AFTER the top-level statement loop, so every
    /// variant's value-struct (emitted by `emit_class`) already exists — "after
    /// the structs". Bases are visited in sorted order for deterministic codegen.
    pub(crate) fn emit_companion_enums(&mut self) -> Result<()> {
        let mut bases: Vec<String> = self
            .poly_map
            .iter()
            .filter(|(_, subs)| !subs.is_empty())
            .map(|(b, _)| b.clone())
            .collect();
        bases.sort();
        for base in &bases {
            self.emit_companion_enum(base)?;
        }
        Ok(())
    }

    /// Emit the companion enum + dispatch + field accessors for ONE polymorphic
    /// base `base` (guaranteed to have a non-empty `poly_map` entry). All three
    /// items are `#[allow(dead_code)]` so the 0-warning gate does not trip on the
    /// as-yet-unused machinery.
    pub(crate) fn emit_companion_enum(&mut self, base: &str) -> Result<()> {
        // Variant set: the base itself, then every TRANSITIVE subclass (poly_map
        // is transitive and already sorted). Each variant's payload is the BARE
        // concrete value-struct — NOT `rust_ty` (which would later become `n__`)
        // and NOT a companion-enum name; intermediate bases therefore appear as
        // raw-struct variants too (e.g. `Base__ { Base(Base), Leaf(Leaf),
        // Mid(Mid) }`), so both `Base__` and `Mid__` are independent flat enums.
        let mut variants: Vec<String> = vec![base.to_string()];
        if let Some(subs) = self.poly_map.get(base) {
            variants.extend(subs.iter().cloned());
        }
        let enum_name = format!("{}__", base);

        // 1. The enum. Always exactly `#[derive(Clone, Debug)]`: a data-variant
        // enum cannot derive Default or Copy, and Display/PartialEq for the enum
        // are deferred (C2-2b+). Do NOT reuse emit_class's all_fields_copy derive
        // logic (design §F).
        self.line("#[allow(dead_code)]");
        self.line("#[derive(Clone, Debug)]");
        self.line(&format!("enum {} {{", enum_name));
        self.indent += 1;
        for v in &variants {
            self.line(&format!("{}({}),", v, v));
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        // 2. Method-dispatch impl. Dunder methods become Rust TRAIT impls (Display
        // / PartialEq / PartialOrd / Add / ...), not inherent methods, so a
        // dispatch `match self { _ => x.__str__() }` would not compile — skip them
        // (this is the inherit_dunders crux). Also skip `__init__` (a constructor,
        // not dispatched) and any `@staticmethod` (no `self` receiver). The list
        // mirrors emit_class's `dunder_trait_names` exactly.
        let dunder_trait_names = DUNDER_TRAIT_NAMES;
        let resolved = self.resolved_methods(base);
        let dispatchable: Vec<Func> = resolved
            .into_iter()
            .filter(|m| {
                m.name != "__init__"
                    && !m.decorators.contains(&"staticmethod".to_string())
                    && !dunder_trait_names.contains(&m.name.as_str())
                    && m.params.iter().any(|p| p.name == "self")
            })
            .collect();

        self.line("#[allow(dead_code)]");
        self.line(&format!("impl {} {{", enum_name));
        self.indent += 1;
        for m in &dispatchable {
            self.emit_dispatch_method(base, &enum_name, &variants, m)?;
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        // 3. Field-accessor impl: one `__field_<f>` per base field (every variant
        // inherits the base's fields, so the field exists on every variant
        // struct). Non-Copy fields clone (value semantics); Copy fields read bare.
        // Dedup by field NAME (mirrors emit_class's all_fields walk): get_all_fields
        // can yield the same name more than once across a base chain, which would
        // otherwise emit two identical `__field_<f>` methods (rustc E0592).
        let mut accessor_fields: Vec<Param> = Vec::new();
        for f in self.ctx.get_all_fields(base) {
            if !accessor_fields.iter().any(|ef: &Param| ef.name == f.name) {
                accessor_fields.push(f);
            }
        }
        self.line("#[allow(dead_code)]");
        self.line(&format!("impl {} {{", enum_name));
        self.indent += 1;
        for f in &accessor_fields {
            let fty = Ty::from_type_expr(&f.ty, f.span)?;
            let ret = self.rust_ty(&fty);
            // (EPIC-6) `x.<field>` reads the inner value struct's field, so a
            // keyword field name is escaped to match the (escaped) struct def. The
            // accessor METHOD itself is `__field_<name>` (prefix → never a
            // keyword), so its name is left unescaped at both def and call site.
            let read = if crate::typeck::is_copy(&fty) {
                format!("x.{}", escape_ident(&f.name))
            } else {
                format!("x.{}.clone()", escape_ident(&f.name))
            };
            let arms: Vec<String> = variants
                .iter()
                .map(|v| format!("{}::{}(x) => {}", enum_name, v, read))
                .collect();
            self.line(&format!(
                "fn __field_{}(&self) -> {} {{ match self {{ {} }} }}",
                f.name,
                ret,
                arms.join(", ")
            ));
        }
        self.indent -= 1;
        self.line("}");
        self.line("");

        // 4. Dunder-trait FORWARDING impls (EPIC-5 C2-2a2). A polymorphic-base var
        // becomes `B__` after the d5a4ff03 flip, and inherit_dunders shows base
        // vars used with `print(m)` (Display), `==` (PartialEq), and `<`
        // (PartialOrd). The companion enum must carry those traits BEFORE the flip
        // — emitted here as DEAD CODE that must COMPILE (B__ is unused until the
        // keystone). Emit a trait impl for B__ ONLY when EVERY variant struct
        // already has that trait, determined by the SAME predicate emit_class uses
        // to decide whether to emit `impl <Trait> for <variant struct>` (off the
        // variant's RESOLVED method set): Display ⇐ __str__|__repr__
        // (display_source, codegen.rs ~1224), PartialEq ⇐ __eq__ (has_eq, ~1056),
        // PartialOrd ⇐ __lt__ (has_lt, ~1096). If ANY variant lacks the trait, the
        // forward (`write!("{}", x)`, `a == b`, `a.partial_cmp(b)`) would not
        // resolve, so emit NO impl of that trait for B__. Cross-variant comparison
        // is Python-honest: `==` is false, ordering is None.
        let variant_resolved: Vec<Vec<Func>> =
            variants.iter().map(|v| self.resolved_methods(v)).collect();
        let all_variants_have = |pred: &dyn Fn(&Func) -> bool| -> bool {
            variant_resolved.iter().all(|ms| ms.iter().any(|m| pred(m)))
        };
        let all_have_display =
            all_variants_have(&|m| m.name == "__str__" || m.name == "__repr__");
        let all_have_eq = all_variants_have(&|m| m.name == "__eq__");
        let all_have_lt = all_variants_have(&|m| m.name == "__lt__");

        if all_have_display {
            let arms: Vec<String> = variants
                .iter()
                .map(|v| format!("{}::{}(x) => write!(__f, \"{{}}\", x)", enum_name, v))
                .collect();
            self.line("#[allow(dead_code)]");
            self.line(&format!("impl ::std::fmt::Display for {} {{", enum_name));
            self.indent += 1;
            self.line(
                "fn fmt(&self, __f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {",
            );
            self.indent += 1;
            self.line(&format!("match self {{ {} }}", arms.join(", ")));
            self.indent -= 1;
            self.line("}");
            self.indent -= 1;
            self.line("}");
            self.line("");
        }

        if all_have_eq {
            let mut arms: Vec<String> = variants
                .iter()
                .map(|v| format!("({0}::{1}(a), {0}::{1}(b)) => a == b", enum_name, v))
                .collect();
            // Cross-variant: Python `Dog(..) == Cat(..)` is False.
            arms.push("_ => false".to_string());
            self.line("#[allow(dead_code)]");
            self.line(&format!("impl ::std::cmp::PartialEq for {} {{", enum_name));
            self.indent += 1;
            self.line(&format!("fn eq(&self, other: &{}) -> bool {{", enum_name));
            self.indent += 1;
            self.line(&format!("match (self, other) {{ {} }}", arms.join(", ")));
            self.indent -= 1;
            self.line("}");
            self.indent -= 1;
            self.line("}");
            self.line("");
        }

        // Rust's `PartialOrd: PartialEq` supertrait bound: the enum can only impl
        // PartialOrd if it ALSO impls PartialEq. emit_class satisfies this on each
        // variant struct by DERIVING PartialEq when `__eq__` is absent, but the
        // companion enum derives only Clone+Debug and never PartialEq, so require
        // all_have_eq here too (true for inherit_dunders: every variant has both).
        if all_have_lt && all_have_eq {
            let mut arms: Vec<String> = variants
                .iter()
                .map(|v| {
                    format!("({0}::{1}(a), {0}::{1}(b)) => a.partial_cmp(b)", enum_name, v)
                })
                .collect();
            // Cross-variant: two different concrete types are uncomparable → None.
            arms.push("_ => None".to_string());
            self.line("#[allow(dead_code)]");
            self.line(&format!("impl ::std::cmp::PartialOrd for {} {{", enum_name));
            self.indent += 1;
            self.line(&format!(
                "fn partial_cmp(&self, other: &{}) -> Option<::std::cmp::Ordering> {{",
                enum_name
            ));
            self.indent += 1;
            self.line(&format!("match (self, other) {{ {} }}", arms.join(", ")));
            self.indent -= 1;
            self.line("}");
            self.indent -= 1;
            self.line("}");
            self.line("");
        }

        Ok(())
    }

    /// Emit a single dispatch method on the companion enum for resolved base
    /// method `m`. The signature mirrors `m`: each non-`self` param via `rust_ty`
    /// (a `Mut[T]` by-ref param becomes `&mut T`), the return via `rust_ty`. The
    /// receiver is `&mut self` when `m` needs `&mut self` on the base OR on ANY
    /// variant (the per-variant V3 query, design §F): if any variant's concrete
    /// `m` is `&mut self`, binding `x` as `&mut` is required for `x.m()` to
    /// compile. The body forwards each param by name to the variant's inherent
    /// `m` (every variant struct has it — inherited or overridden).
    pub(crate) fn emit_dispatch_method(
        &mut self,
        base: &str,
        enum_name: &str,
        variants: &[String],
        m: &Func,
    ) -> Result<()> {
        // Per-variant `&mut self`: needs_mut_self for the base's `m`, OR for the
        // SAME-named method resolved on any variant (a variant may override `m`
        // as `&mut self` even when the base is `&self`). Bodies come from each
        // variant's resolved method set so the V3 decision is queried against the
        // exact body that variant emits.
        let mut needs_mut = self.needs_mut_self(base, &m.name, &m.body);
        if !needs_mut {
            for v in variants {
                if let Some(vm) = self.resolved_methods(v).into_iter().find(|x| x.name == m.name) {
                    if self.needs_mut_self(v, &m.name, &vm.body) {
                        needs_mut = true;
                        break;
                    }
                }
            }
        }
        let receiver = if needs_mut { "&mut self" } else { "&self" };

        // Non-self params + their forwarded names. A by-ref (`Mut[T]`) param
        // renders `name: &mut T` and forwards the bare binding (already a `&mut`).
        let non_self: Vec<&Param> = m.params.iter().filter(|p| p.name != "self").collect();
        let mut sig_params: Vec<String> = Vec::new();
        let mut fwd: Vec<String> = Vec::new();
        for p in &non_self {
            let pty = self.rust_ty(&Ty::from_type_expr(&p.ty, p.span)?);
            // (EPIC-6) Dispatch-wrapper params + their forwarding both escape so a
            // keyword-named param stays consistent. (The method NAME `m.name` is
            // left unescaped — method-name escaping is the sibling dispatch card.)
            if p.by_ref {
                sig_params.push(format!("{}: &mut {}", escape_ident(&p.name), pty));
            } else {
                sig_params.push(format!("{}: {}", escape_ident(&p.name), pty));
            }
            fwd.push(escape_ident(&p.name));
        }
        let ret = self.rust_ty(&Ty::from_type_expr(&m.ret, m.span)?);
        let fwd_args = fwd.join(", ");
        let arms: Vec<String> = variants
            .iter()
            .map(|v| format!("{}::{}(x) => x.{}({})", enum_name, v, m.name, fwd_args))
            .collect();

        let mut sig = format!("fn {}({}", m.name, receiver);
        for sp in &sig_params {
            sig.push_str(", ");
            sig.push_str(sp);
        }
        let _ = write!(sig, ") -> {} {{ match self {{ {} }} }}", ret, arms.join(", "));
        self.line(&sig);
        Ok(())
    }

    /// True when `obj.name` resolves to a `@property` getter (an owned temp
    /// produced by a method call `obj.name()`), rather than a plain field read.
    /// Shared by the `emit_expr` Attr arm (which appends `()`) and
    /// `emit_consuming` (which must NOT clone a property — its result is already
    /// an owned temporary, not a borrowable place). Mirrors the inheritance-aware
    /// method lookup used elsewhere by resolving through the class's methods.
    pub(crate) fn is_property_access(&self, obj: &Expr, name: &str) -> bool {
        if let Expr::Ident(var, _) = obj {
            self.locals.get(var.as_str()).cloned()
                .and_then(|ty| if let Ty::Class(cn, _) = ty {
                    self.ctx.classes.get(&cn).map(|cd|
                        cd.methods.iter().any(|m|
                            m.name.as_str() == name
                            && m.decorators.contains(&"property".to_string())
                        )
                    )
                } else { None })
                .unwrap_or(false)
        } else {
            false
        }
    }

    /// (PERF) True when `e` lowers to a Rust *place* we can borrow (`&e`) cheaply:
    /// a bare variable, or an ordinary struct-field chain rooted at one. This is
    /// the "borrow the base" precondition for list index/slice reads — when it
    /// holds we clone only the element, not the whole container. The Attr rules
    /// mirror the `emit_expr` Attr arm's plain-field branch so `emit_expr(obj)`
    /// yields a real place: a `@property` is a getter call (owned temp), and a
    /// polymorphic-base field read lowers to a `__field_x()` accessor call — both
    /// are temporaries, not borrowable places. A module CONST may lower to a
    /// `&'static` slice or a `.to_string()` temp, so it also stays on the clone
    /// path. Anything else (a call result, slice, literal, nested subscript) is a
    /// temporary rvalue and returns false.
    pub(crate) fn is_borrowable_place(&self, e: &Expr) -> bool {
        match e {
            Expr::Ident(n, _) => !(self.const_names.contains(n) && !self.locals.contains_key(n)),
            Expr::Attr { obj, name, .. } => {
                if self.is_property_access(obj, name) {
                    return false;
                }
                let poly_field = !matches!(obj.as_ref(),
                        Expr::Ident(n, _) if n == "self" || self.concrete_struct_params.contains(n))
                    && matches!(&self.type_of_expr(obj),
                                Ty::Class(b, _) if self.is_polymorphic_base(b));
                !poly_field && self.is_borrowable_place(obj)
            }
            _ => false,
        }
    }

    /// (PERF) True when evaluating the index/slice sub-expression `e` cannot
    /// mutate or move the base container we hold a shared borrow of — it performs
    /// only reads, arithmetic, and the read-only `len()` builtin. This is the
    /// second precondition (with `is_borrowable_place`) for the borrow fast path:
    /// because every accepted form only SHARED-borrows the base, the emitted
    /// `__py_list_get(&base, <idx>)` (base shared + idx shared) always compiles.
    /// Any other call (a mutating method like `.pop()`, or an unknown user call)
    /// forces the conservative clone fallback — honest correctness over a silent
    /// miscompile.
    pub(crate) fn is_borrow_safe_index(&self, e: &Expr) -> bool {
        match e {
            Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Str(..)
            | Expr::None_(..) | Expr::Ident(..) => true,
            Expr::BinOp { lhs, rhs, .. } =>
                self.is_borrow_safe_index(lhs) && self.is_borrow_safe_index(rhs)
                    && !self.operand_moves_by_dunder(lhs)
                    && !self.operand_moves_by_dunder(rhs),
            Expr::UnOp { expr, .. } =>
                self.is_borrow_safe_index(expr) && !self.operand_moves_by_dunder(expr),
            Expr::IfExp { test, body, orelse, .. } =>
                self.is_borrow_safe_index(test)
                    && self.is_borrow_safe_index(body)
                    && self.is_borrow_safe_index(orelse),
            Expr::Index { obj, idx, .. } =>
                self.is_borrow_safe_index(obj) && self.is_borrow_safe_index(idx),
            Expr::Attr { obj, name, .. } =>
                !self.is_property_access(obj, name) && self.is_borrow_safe_index(obj),
            Expr::Call { callee, args, .. } =>
                matches!(callee.as_ref(), Expr::Ident(n, _) if n == "len")
                    && args.iter().all(|a| self.is_borrow_safe_index(a)),
            _ => false,
        }
    }

    /// (PERF) True when using `e` as an arithmetic/unary OPERAND may MOVE it:
    /// a class-typed operand invokes a dunder trait impl that takes its
    /// operands BY VALUE (`std::ops` convention — `__add__` is
    /// `Add::add(self, rhs)`), so `xs[h + h2]` would move `h` while the fast
    /// path holds `&h.items` alive (E0505). A `TypeVar` operand may
    /// monomorphize to such a class, and `Unknown` gets the same conservative
    /// treatment: force the clone fallback rather than emit a borrow that a
    /// by-value dunder call can invalidate.
    pub(crate) fn operand_moves_by_dunder(&self, e: &Expr) -> bool {
        matches!(
            self.type_of_expr(e),
            Ty::Class(..) | Ty::TypeVar(_) | Ty::Unknown
        )
    }

    /// The SINGLE ownership-decision point for value semantics (EPIC-4 V1).
    ///
    /// Emit `e` in a position that takes ownership of the value (constructor /
    /// method / function argument, container store, `return`, assignment RHS,
    /// ternary arm, `match` scrutinee). A non-`Copy` *place* — a reusable binding
    /// the program may read again — is deep-`.clone()`d so the original stays
    /// usable, reproducing Python value semantics without aliasing. A deep clone
    /// never changes observable behavior here, so uniform clone-on-use is always
    /// correctness-safe (last-use move-elision is a deferred optimization).
    ///
    /// Decision (the only place this judgement lives):
    /// - `Copy` type (per the shared `crate::typeck::is_copy`) -> bare, no clone.
    /// - `Expr::Ident` -> `<n>.clone()` (a reusable variable place).
    /// - `Expr::Attr` FIELD read -> `<obj>.<field>.clone()` (a reusable field
    ///   place; this is the new capability that fixes `Wrapper(self.items)`
    ///   E0382). A `@property` access is an owned temp (a getter call), so it is
    ///   emitted bare — cloning it would be a redundant temp-clone.
    /// - `Expr::Index` -> `emit_expr(e)` UNCHANGED. Index reads already self-clone
    ///   (tuple `.clone()`, dict `.cloned()`, list `__list[i].clone()`); appending
    ///   another `.clone()` would be a double-clone bug.
    /// - `Expr::IfExp` -> the ARMS are the consuming leaves; recurse so the arm
    ///   *places* clone, not the whole owned if-temp.
    /// - everything else (Call/constructor result, BinOp, literal, comprehension,
    ///   slice, …) -> a fresh owned rvalue temp -> bare, nothing to clone.
    pub(crate) fn emit_consuming(&mut self, e: &Expr) -> Result<String> {
        if self.is_copy_type(&self.type_of_expr(e)) {
            return self.emit_expr(e);
        }
        match e {
            Expr::Ident(..) => Ok(format!("{}.clone()", self.emit_expr(e)?)),
            Expr::Attr { obj, name, .. } => {
                if self.is_property_access(obj, name) {
                    // Owned temp from a getter call — already owned, do not clone.
                    self.emit_expr(e)
                } else {
                    Ok(format!("{}.clone()", self.emit_expr(e)?))
                }
            }
            // Index reads already self-clone — pass through to avoid double-clone.
            Expr::Index { .. } => self.emit_expr(e),
            // Clone the arm PLACES, not the whole owned if-temp.
            Expr::IfExp { test, body, orelse, .. } => {
                let t = self.emit_expr(test)?;
                let b = self.emit_consuming(body)?;
                let o = self.emit_consuming(orelse)?;
                Ok(format!("(if {} {{ {} }} else {{ {} }})", t, b, o))
            }
            // Owned rvalue temp (call/ctor/literal/binop/slice/...) — nothing to clone.
            _ => self.emit_expr(e),
        }
    }

    /// Emit an lvalue *place* expression (one assignable / mutable in Rust) for
    /// an assignment-target base — as opposed to `emit_expr`, which produces a
    /// clone-based *rvalue* for `Index`/`Attr` (e.g. `{ let __list = x.clone();
    /// __list[i].clone() }`) that cannot be stored into. Used by AttrAssign /
    /// IndexAssign so chained targets (`self.dict[k]=v`, `rooms[i].field=v`,
    /// `a.b.c=v`) lower to in-place mutation. Struct fields are bare
    /// HashMap/Vec/struct values (no Rc/RefCell), so `place.field`,
    /// `place[i as usize]`, and `*place.get_mut(&k)` are valid places.
    pub(crate) fn emit_place(&mut self, e: &Expr) -> Result<String> {
        match e {
            // A bare name (`self`, a local, a `mut` param) is already a place.
            // (EPIC-6) Escape a keyword-named place so it matches its `let`/param
            // definition (which is also escaped).
            Expr::Ident(n, _) => Ok(escape_ident(n)),
            // Field access: the base recursively as a place, then `.field`.
            // (No @property handling: a property getter is not an lvalue.)
            Expr::Attr { obj, name, .. } => {
                let base = self.emit_place(obj)?;
                Ok(format!("{}.{}", base, escape_ident(name)))
            }
            // Subscript in a *base* position (we are descending further into the
            // target, e.g. `grid[r][c]` or `self.dict[k].field`): produce a place
            // that mutably borrows the element. Dict -> deref of get_mut; list ->
            // direct index. Negative indices in nested lvalue position are not
            // supported yet (a place cannot contain a `let`); the common
            // non-negative cases lower directly.
            Expr::Index { obj, idx, .. } => {
                let base = self.emit_place(obj)?;
                if matches!(self.type_of_expr(obj), Ty::Dict(..)) {
                    let k = self.emit_expr(idx)?;
                    Ok(format!("(*{}.get_mut(&{}).expect(\"key not found\"))", base, k))
                } else {
                    // Parenthesize the index before `as usize`: a nested list
                    // subscript lowers `idx` to a *block* expression, and bare
                    // `base[{ block } as usize]` is a Rust parse error ("expected
                    // expression, found `as`"). The parens make it `base[(block)
                    // as usize]`, valid for block / call / arith / ident alike.
                    let i = self.emit_expr(idx)?;
                    Ok(format!("{}[({}) as usize]", base, i))
                }
            }
            // Any other base (a parenthesized/computed expr) — fall back to the
            // normal emission; in practice the parser only yields Ident/Attr/Index
            // chains as assignment-target bases.
            _ => self.emit_expr(e),
        }
    }

    /// (EPIC-4 V2-c) Borrow form for an argument flowing into a by-reference
    /// (`Mut[T]` -> `&mut T`) callee parameter. `place` is the already-emitted
    /// `emit_place(a)` text.
    ///
    /// The normal form is `&mut <place>` (a fresh mutable borrow of the caller's
    /// storage). The ONE exception is when `a` is a bare name that is itself a
    /// `&mut T` param binding of the current function (a forwarded-by-reference
    /// arg, e.g. a recursive `fill(visited, ..)` where `visited: &mut HashSet`):
    /// `&mut visited` would re-borrow an already-`&mut` binding and rustc rejects
    /// it (E0596). Emit an explicit reborrow `&mut *visited` instead — valid in
    /// every position including last-use/recursive, so we prefer it over relying
    /// on a bare auto-reborrow.
    ///
    /// Only the BARE-ident case needs this. A field/index of a by-reference param
    /// (`param.field`, `param[k]`) is not an `Expr::Ident`, so `&mut param.field`
    /// stays — that already auto-derefs through the `&mut`. An owned local
    /// (`&mut my_account`) and a self place (`&mut self.field`) are not in
    /// `by_ref_locals`, so they are unchanged.
    pub(crate) fn byref_borrow(&self, a: &Expr, place: &str) -> String {
        if let Expr::Ident(n, _) = a {
            if self.by_ref_locals.contains(n) {
                return format!("&mut *{}", place);
            }
        }
        format!("&mut {}", place)
    }

    /// Emit a list/set element, promoting int-typed elements to `f64` when the
    /// collection's unified element type is `Float` (`widen == true`). Reuses
    /// the same `as f64` cast convention as the assignment int->float coercion
    /// (see `Stmt::Assign` emission) so `[1, 2.0]` becomes a homogeneous
    /// `Vec<f64>` instead of the rustc-rejected `vec![(1i64), (2.0f64)]`
    /// (card 5c2f31d8). Float (and non-int) elements are emitted unchanged.
    pub(crate) fn emit_collection_elem(&mut self, e: &Expr, widen: bool) -> Result<String> {
        let s = self.emit_consuming(e)?;
        if widen && matches!(self.type_of_expr(e), Ty::Int) {
            Ok(format!("({}) as f64", s))
        } else {
            Ok(s)
        }
    }

    /// Emit a comprehension element/key/value expression, casting an integer-valued
    /// power to `f64` when its INFERRED type is `Float` (D5: `**` always types as
    /// Float, but `int ** int` still EMITS i64 via `__py_ipow`). Without this, a
    /// `[x ** 2 for x in [1, 2, 3]]` assigned to `list[float]` emits a `Vec<i64>`
    /// and mismatches `Vec<f64>` (E0308). Mirrors the `Stmt::Assign` float-coercion
    /// rule (`matches!(ty, Float) && emits_int_pow(value)`). A genuine float element
    /// passes through unchanged. This matters now that the comprehension loop
    /// variable carries its element type (so `type_of_expr` is accurate inside the
    /// closure body) — previously the untyped loop var made `**` fall to the float
    /// emission path by accident.
    pub(crate) fn emit_comp_value(&mut self, e: &Expr) -> Result<String> {
        let s = self.emit_expr(e)?;
        if matches!(self.type_of_expr(e), Ty::Float) && self.emits_int_pow(e) {
            Ok(format!("(({}) as f64)", s))
        } else {
            Ok(s)
        }
    }

    /// Combine two inferred types into the more specific / wider one.
    /// `Unknown` yields to anything concrete; `Int` widens to `Float`;
    /// matching collections unify element-wise. Otherwise the first wins.
    pub(crate) fn unify_ty(a: Ty, b: Ty) -> Ty {
        match (a, b) {
            (Ty::Unknown, x) | (x, Ty::Unknown) => x,
            (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => Ty::Float,
            (Ty::Dict(k1, v1), Ty::Dict(k2, v2)) => Ty::Dict(
                Box::new(Self::unify_ty(*k1, *k2)),
                Box::new(Self::unify_ty(*v1, *v2)),
            ),
            (Ty::List(e1), Ty::List(e2)) => Ty::List(Box::new(Self::unify_ty(*e1, *e2))),
            (Ty::Set(e1), Ty::Set(e2)) => Ty::Set(Box::new(Self::unify_ty(*e1, *e2))),
            (a, _) => a,
        }
    }

    /// (EPIC-5 C2-2b-i, Step 3) If a list literal's elements share a common
    /// POLYMORPHIC-base class, return that base name (the slot is `Vec<B__>` and
    /// each element must be wrapped). Returns `None` when the elements are not all
    /// classes, or their common ancestor is a leaf / non-polymorphic class (the
    /// ordinary homogeneous-vec path applies). Mirrors typeck's
    /// `nearest_common_ancestor` fold over `unify_branch_types`, so codegen and
    /// typeck agree on the element type of a heterogeneous-subclass literal.
    pub(crate) fn list_poly_base(&self, elems: &[Expr]) -> Option<String> {
        if elems.is_empty() { return None; }
        let mut acc = match self.type_of_expr(&elems[0]) {
            Ty::Class(n, _) => n,
            _ => return None,
        };
        for e in &elems[1..] {
            let cn = match self.type_of_expr(e) {
                Ty::Class(n, _) => n,
                _ => return None,
            };
            acc = if crate::typeck::is_subclass(&cn, &acc, self.ctx) {
                acc // acc is already an ancestor of cn
            } else if crate::typeck::is_subclass(&acc, &cn, self.ctx) {
                cn // cn is the wider (base) of the two
            } else {
                crate::typeck::nearest_common_ancestor(&acc, &cn, self.ctx)?
            };
        }
        if self.is_polymorphic_base(&acc) { Some(acc) } else { None }
    }

    /// Unified element type of a list/set literal's elements. Folds every
    /// element's type with `unify_ty` (rather than first-element-wins) so a
    /// mixed numeric literal like `[1, 2.0]` is typed `Float` — matching
    /// typeck's `unify_elem_types`. An empty literal is `Unknown`.
    pub(crate) fn list_elem_ty(&self, elems: &[Expr]) -> Ty {
        let mut iter = elems.iter();
        match iter.next() {
            None => Ty::Unknown,
            Some(first) => iter.fold(self.type_of_expr(first), |acc, e| {
                Self::unify_ty(acc, self.type_of_expr(e))
            }),
        }
    }

    /// Whether reassigning a `b`-typed value to an `a`-typed binding is a
    /// genuine type change (Python allows it; single-`let` Rust does not).
    /// `Unknown` and numeric int/float mixes are not conflicts.
    pub(crate) fn types_conflict(a: &Ty, b: &Ty) -> bool {
        use Ty::*;
        if matches!(a, Unknown) || matches!(b, Unknown) {
            return false;
        }
        if matches!((a, b), (Int, Float) | (Float, Int)) {
            return false;
        }
        std::mem::discriminant(a) != std::mem::discriminant(b)
    }

    /// Forward type-inference pre-pass over a statement list (recursing into
    /// nested blocks). Records inferred types for un-annotated locals in
    /// `self.locals` and refines empty-collection element types from later
    /// `obj[k] = v` stores and `obj.append(v)` calls. Runs in source order so
    /// earlier inferences inform later `type_of_expr` lookups.
    pub(crate) fn prescan_types(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Assign { target, ty: Some(te), span, .. } => {
                    // Scope with the enclosing generic function's type vars so a
                    // local `acc: T` is `TypeVar("T")` (not `Class("T")`) — matching
                    // the call-result oracle, so a later `acc = f(...)` mutates
                    // rather than shadows.
                    if let Ok(t) = Ty::from_type_expr_scoped(te, *span, &self.current_fn_type_params) {
                        self.locals.insert(target.clone(), t);
                    }
                }
                Stmt::Assign { target, ty: None, value, .. } => {
                    let vt = self.type_of_expr(value);
                    let merged = match self.locals.get(target) {
                        Some(existing) => Self::unify_ty(existing.clone(), vt),
                        None => vt,
                    };
                    self.locals.insert(target.clone(), merged);
                }
                Stmt::IndexAssign { obj, value, .. } => {
                    // Refine an empty-collection element type from `name[k] = v`
                    // — only for a bare local base (`Expr::Ident`); chained bases
                    // (self.dict[k], grid[r][c]) refine via their declared types.
                    if let Expr::Ident(name, _) = obj.as_ref() {
                        let vt = self.type_of_expr(value);
                        if !matches!(vt, Ty::Unknown) {
                            if let Some(existing) = self.locals.get(name).cloned() {
                                let refined = match existing {
                                    Ty::Dict(k, v) if matches!(*v, Ty::Unknown) => {
                                        Ty::Dict(k, Box::new(vt))
                                    }
                                    Ty::List(e) if matches!(*e, Ty::Unknown) => {
                                        Ty::List(Box::new(vt))
                                    }
                                    other => other,
                                };
                                self.locals.insert(name.clone(), refined);
                            }
                        }
                    }
                }
                Stmt::Expr(Expr::Call { callee, args, .. }) => {
                    if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                        if name == "append" {
                            if let Expr::Ident(objn, _) = obj.as_ref() {
                                if let Some(arg) = args.first() {
                                    let at = self.type_of_expr(arg);
                                    if !matches!(at, Ty::Unknown) {
                                        if let Some(Ty::List(e)) = self.locals.get(objn).cloned() {
                                            if matches!(*e, Ty::Unknown) {
                                                self.locals.insert(objn.clone(), Ty::List(Box::new(at)));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Stmt::If { then, elifs, else_, .. } => {
                    self.prescan_types(then);
                    for (_, body) in elifs { self.prescan_types(body); }
                    if let Some(body) = else_ { self.prescan_types(body); }
                }
                Stmt::For { targets, iter, body, .. } => {
                    // Register the loop variable's type so the body sees it (e.g.
                    // a `Temp` element's float fields aren't mistyped as Int).
                    // SCOPED to the loop body via save/restore: registering it
                    // function-wide would mask a MODULE CONST of the same name at
                    // statements OUTSIDE the loop (the loop var does not leak into
                    // Rust scope anyway), making `print(i)` next to a const `i`
                    // resolve to a (non-existent) local instead of the const.
                    if targets.len() == 1 {
                        let elem = match self.type_of_expr(iter) {
                            Ty::List(inner) | Ty::Set(inner) => *inner,
                            // Iterating a dict yields its KEYS (Python semantics).
                            Ty::Dict(key, _) => *key,
                            Ty::Str => Ty::Str,
                            _ => Ty::Int, // range / unknown iterables yield ints
                        };
                        let saved = self.locals.get(&targets[0]).cloned();
                        self.locals.entry(targets[0].clone()).or_insert(elem);
                        self.prescan_types(body);
                        match saved {
                            Some(ty) => { self.locals.insert(targets[0].clone(), ty); }
                            None => { self.locals.remove(targets[0].as_str()); }
                        }
                    } else {
                        self.prescan_types(body);
                    }
                }
                Stmt::While { body, .. } | Stmt::With { body, .. } => {
                    self.prescan_types(body);
                }
                Stmt::Try { body, handlers, else_, finally_, .. } => {
                    self.prescan_types(body);
                    for h in handlers {
                        // Bind exc_name as Str before scanning the handler body so
                        // that `len(e)` on a handler-bound exception yields char-count
                        // (Ty::Str path) rather than byte-count (Ty::Unknown path).
                        // SCOPED to the handler body (save/restore): the binding
                        // does not leak past the handler in pyrst, and a function-
                        // wide registration would mask a same-named MODULE CONST at
                        // statements outside the handler (e.g. a const `e` read
                        // before/after an `except ... as e`).
                        let saved = if let Some(name) = &h.exc_name {
                            let prev = self.locals.get(name).cloned();
                            self.locals.insert(name.clone(), Ty::Str);
                            Some((name.clone(), prev))
                        } else {
                            None
                        };
                        self.prescan_types(&h.body);
                        if let Some((name, prev)) = saved {
                            match prev {
                                Some(ty) => { self.locals.insert(name, ty); }
                                None => { self.locals.remove(name.as_str()); }
                            }
                        }
                    }
                    if let Some(b) = else_ { self.prescan_types(b); }
                    if let Some(b) = finally_ { self.prescan_types(b); }
                }
                _ => {}
            }
        }
    }

}
