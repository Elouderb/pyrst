use super::*;

impl<'a> Codegen<'a> {
    /// (W0-b, honesty hole p12b) Whether a `Vec<elem>` must be sorted with the
    /// `partial_cmp` comparator instead of `.sort()`. Rust's `.sort()` needs
    /// `Ord`, which `f64` lacks (only `PartialOrd`) and which a user class with a
    /// `__lt__` also lacks — `__lt__` lowers to `impl PartialOrd` only (no `Ord`;
    /// see codegen/items.rs). Both cases therefore need
    /// `.sort_by(|a, b| a.partial_cmp(b)...)`. Mirrors the pre-existing float path
    /// and extends it to comparable user classes, closing the `sorted(list_of_obj)`
    /// -> rustc E0277 leak.
    pub(crate) fn elem_needs_partial_cmp(&self, elem: &Ty) -> bool {
        match elem {
            Ty::Float => true,
            // A class is `Ord`-sortable only via a `__lt__` dunder, which emits
            // `PartialOrd` (not `Ord`); so it always needs the partial_cmp path.
            Ty::Class(cls, _) => self.ctx.get_method(cls, "__lt__").is_some(),
            _ => false,
        }
    }

    /// The `.sort*()` call suffix for a `Vec<elem>`: the `partial_cmp` comparator
    /// for `f64` / a `__lt__`-comparable class (see [`Self::elem_needs_partial_cmp`]),
    /// else the plain `.sort()` (which requires `Ord`).
    pub(crate) fn sort_suffix_for_elem(&self, elem: &Ty) -> String {
        if self.elem_needs_partial_cmp(elem) {
            ".sort_by(|a, b| a.partial_cmp(b).unwrap_or(::std::cmp::Ordering::Equal))".to_string()
        } else {
            ".sort()".to_string()
        }
    }

    /// (Bug C) The RETURN type of a `sort`/`sorted` `key=` lambda over a container
    /// of type `src_ty` — a `Float` key needs the `partial_cmp` comparator (`f64`
    /// is not `Ord`). Ports the `sorted(...)` key-return inference so the in-place
    /// `list.sort(key=...)` picks the same comparator; `Unknown` (the common
    /// `Ord`-key case) drives `sort_by_key`.
    pub(crate) fn sort_key_ret_ty(&self, key_expr: &Expr, src_ty: &Ty) -> Ty {
        if let Expr::Lambda { body, .. } = key_expr {
            match body.as_ref() {
                Expr::Attr { name, .. } => {
                    if let Ty::List(elem) | Ty::Iterator(elem) = src_ty {
                        if let Ty::Class(cls, _) = elem.as_ref() {
                            if let Some(c) = self.ctx.classes.get(cls.as_str()) {
                                if let Some(f) = c.fields.iter().find(|f| &f.name == name) {
                                    return Ty::from_type_expr(&f.ty, f.span).unwrap_or(Ty::Unknown);
                                }
                            }
                        }
                    }
                }
                Expr::Call { callee, .. } => {
                    if let Expr::Attr { name, .. } = callee.as_ref() {
                        if let Ty::List(elem) | Ty::Iterator(elem) = src_ty {
                            if let Ty::Class(cls, _) = elem.as_ref() {
                                if let Some(sig) = self.ctx.get_method(cls.as_str(), name) {
                                    return sig.ret.clone();
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ty::Unknown
    }

    /// (Bug C) Emit a `sort`/`sorted` `key=` expression's body as a Rust snippet
    /// that reads the element as `__x` — the lambda parameter is bound to the
    /// container's element type (list/set/generator elem, or dict KEY) so a
    /// tuple/field/method body lowers correctly, then renamed to `__x`. A non-lambda
    /// key (a function value) is emitted directly. Shared with the in-place
    /// `list.sort(key=...)` path (mirrors the `sorted(...)` key-code extraction).
    pub(crate) fn emit_sort_key_code(&mut self, key_expr: &Expr, src_ty: &Ty) -> Result<String> {
        if let Expr::Lambda { params, body, .. } = key_expr {
            let param_name = params.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "__x".to_string());
            let key_param_ty = match src_ty {
                Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => (**inner).clone(),
                Ty::Dict(k, _) => (**k).clone(),
                _ => Ty::Unknown,
            };
            let saved = self.locals.get(&param_name).cloned();
            self.locals.insert(param_name.clone(), key_param_ty);
            let body_s = self.emit_expr(body)?;
            match saved {
                Some(ty) => { self.locals.insert(param_name.clone(), ty); }
                None => { self.locals.remove(param_name.as_str()); }
            }
            Ok(Self::replace_identifier(&body_s, escape_ident(&param_name).as_str(), "__x"))
        } else {
            self.emit_expr(key_expr)
        }
    }

    /// (card cc7ae370, item 1) Thin wrapper: collects any subscript-index temps
    /// hoisted for a MUTATING-method receiver (`grid[len(grid) - 1].append(x)`)
    /// and wraps the whole emitted call in a `{ let __idxN = ..; <call> }` block so
    /// the index temp runs before the receiver's `&mut` borrow (E0502). Wrapping
    /// the ENTIRE call — with the bare receiver place still INSIDE — rather than
    /// just the receiver keeps two-phase borrows valid for the method's own
    /// arguments (`grid[i].append(grid[j])`). With no subscript receiver the
    /// prelude is empty and the call is returned unchanged (byte-identical).
    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_method_call_on_attr(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<Option<String>> {
        let mut recv_prelude = Vec::new();
        let result = self.emit_method_call_on_attr_inner(callee, args, kwargs, &mut recv_prelude)?;
        Ok(result.map(|s| Self::hoist_wrap(&recv_prelude, s)))
    }

    /// (W1.5 fix E) Emit the `let __fill = ..;` binding for `str.ljust`/`rjust`/
    /// `center`. When a 2nd argument (fillchar) is present it is bound and
    /// runtime-checked to be exactly one character — CPython raises
    /// `TypeError: The fill character must be exactly one character long`
    /// otherwise. Absent, the pad character is a single space (no check needed).
    /// The caller pads with `__fill.repeat(n)` in place of the old hardcoded space.
    fn justify_fillchar(parts: &[String]) -> String {
        if parts.len() >= 2 {
            format!(
                "let __fill = {}; if __fill.chars().count() != 1 {{ panic!(\"TypeError\\0The fill character must be exactly one character long\"); }}",
                parts[1]
            )
        } else {
            "let __fill = \" \";".to_string()
        }
    }

    /// (enabler-fix-2 #3) STR-mode lowering of an `Option<T>` value at a
    /// print()/str()/f-string site. CPython holds the payload OR None: it prints
    /// the payload via `str()` (a `str` shows UNQUOTED — `Some("x")` -> `x`) when
    /// present, else the literal `None`. This mirrors the PyRepr `Option` impl but
    /// with STR (Display) semantics for the inner value — `repr(opt)` still routes
    /// through PyRepr (quoted). Without this an un-narrowed `Option<T>` reached
    /// `println!("{}", opt)` and leaked rustc E0277 (`Option<T>: !Display`).
    /// `depth` uniquely names the match binding so a nested `Optional[Optional[T]]`
    /// does not shadow. The inner formatting reuses the exact per-type str rules
    /// the three call sites apply to a bare value (float/bool/container/…).
    fn emit_str_option(&self, raw: &str, inner: &Ty, depth: usize) -> String {
        let v = format!("__optv{}", depth);
        let inner_form = match inner {
            Ty::Float => format!("__py_fmt_float(*{})", v),
            Ty::Bool => format!("__py_fmt_bool(*{})", v),
            Ty::Bytes | Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) => format!("({}).py_repr()", v),
            Ty::Option(i2) => self.emit_str_option(&v, i2, depth + 1),
            _ => format!("format!(\"{{}}\", {})", v),
        };
        format!(
            "(match &({}) {{ Some({}) => {}, None => \"None\".to_string() }})",
            raw, v, inner_form
        )
    }

    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_method_call_on_attr_inner(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
        recv_prelude: &mut Vec<String>,
    ) -> Result<Option<String>> {
                // Method call with attribute callee — handle method name remapping
                if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                    // (W4-a) In-place MUTATING method on a module global container
                    // (`items.append(x)` on a `thread_local!` RefCell<Vec>). Needs no
                    // `global` declaration (Python: a mutation, not a rebind). The
                    // whole mutation runs inside one `.with()` so the `borrow_mut()`
                    // is scoped to the call and never aliases a read. A NON-mutating
                    // method (`items.count(x)`) is a read and falls through — its
                    // receiver is the value-semantics clone snapshot (correct). This
                    // only handles a BARE global receiver; a subscripted-global chain
                    // (`grid[i].append(x)`) is a later increment.
                    if MUTATING_METHODS.contains(&name.as_str()) {
                        if let Expr::Ident(g, _) = obj.as_ref() {
                            if !self.locals.contains_key(g) {
                                if let Some((owner, _)) = self.resolve_bare_global(g) {
                                    let gname = crate::codegen::mangle_global(owner.as_deref(), g);
                                    // Clean single-call mutators (borrow_mut().M(args)):
                                    // append→push, plus the direct-mapping ones. The
                                    // richer mutators (sort/pop/remove/insert/…) need
                                    // the full method machinery on a place and are a
                                    // W4-b follow-on — honest codegen error, not a
                                    // silent mutate-a-clone.
                                    let mapped = match name.as_str() {
                                        "append" => Some("push"),
                                        "extend" => Some("extend"),
                                        "clear" => Some("clear"),
                                        "reverse" => Some("reverse"),
                                        "add" => Some("insert"),
                                        _ => None,
                                    };
                                    match mapped {
                                        Some(m) => {
                                            // (W4-a, F3) Hoist every argument into a
                                            // block-local temp BEFORE entering
                                            // `.with(borrow_mut())`. An argument that
                                            // READS the same global (`g.append(g[0])`,
                                            // `g.append(len(g))`) then evaluates its own
                                            // `.with(borrow())` FIRST — the read borrow is
                                            // released before the mutating borrow — so the
                                            // RefCell is never re-entrantly borrowed (the
                                            // old inline emission panicked, exit 101).
                                            // Mirrors the block-temp discipline of every
                                            // other W4-a write path.
                                            let base = self.shadow_counter;
                                            self.shadow_counter += 1;
                                            let mut prelude = String::new();
                                            let mut temp_names: Vec<String> = Vec::with_capacity(args.len());
                                            for (i, a) in args.iter().enumerate() {
                                                let av = self.emit_consuming(a)?;
                                                let tn = format!("__pyrst_gm_{}_{}", base, i);
                                                prelude.push_str(&format!("let {} = {}; ", tn, av));
                                                temp_names.push(tn);
                                            }
                                            let call_args = temp_names.join(", ");
                                            // Mirror the general per-type lowering so a
                                            // global receiver behaves IDENTICALLY to a
                                            // local one: `HashSet::insert` (add) returns a
                                            // `bool`, so wrap it in a braces+semicolon unit
                                            // discard exactly like the general set-path's
                                            // `{ s.insert(x); }`; push/extend/clear/reverse
                                            // already return `()`.
                                            let mutation = if m == "insert" {
                                                format!(
                                                    "{}.with(|__gc| {{ __gc.borrow_mut().insert({}); }})",
                                                    gname, call_args
                                                )
                                            } else {
                                                format!(
                                                    "{}.with(|__gc| __gc.borrow_mut().{}({}))",
                                                    gname, m, call_args
                                                )
                                            };
                                            return Ok(Some(format!("{{ {}{} }}", prelude, mutation)));
                                        }
                                        None => {
                                            return Err(crate::diag::Error::Codegen(format!(
                                                "in-place `{}.{}(...)` on a module global \
                                                 is not yet supported (W4-b); for now use \
                                                 `append`/`extend`/`clear`/`reverse`/`add`, \
                                                 or rebind the whole global under `global {}`",
                                                g, name, g
                                            )));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // (card 49170944) `str.maketrans(x, y)` -> a `HashMap<i64, i64>`
                    // code-point map (zip the from/to chars as ords). Feeds
                    // `s.translate(table)`. 2-arg equal-length form is the honest
                    // subset (the 3-arg delete form is out of scope).
                    if name == "maketrans"
                        && matches!(obj.as_ref(), Expr::Ident(sn, _) if sn == "str")
                        && args.len() == 2
                    {
                        let from = self.emit_expr(&args[0])?;
                        let to = self.emit_expr(&args[1])?;
                        // (enabler-fix-2 #5) CPython raises `ValueError: the first two
                        // maketrans arguments must have equal length` on unequal
                        // lengths; the old code SILENTLY zip-truncated to the shorter
                        // (a silent miscompile). Emit the exact, CATCHABLE ValueError
                        // (NUL-delimited "Type\0msg" convention every runtime error
                        // uses). Compare CODE POINTS (chars, not UTF-8 bytes), matching
                        // CPython's per-character length.
                        return Ok(Some(format!(
                            "{{ let __mt_from: Vec<char> = ({}).chars().collect(); let __mt_to: Vec<char> = ({}).chars().collect(); if __mt_from.len() != __mt_to.len() {{ panic!(\"ValueError\\0the first two maketrans arguments must have equal length\"); }} __mt_from.into_iter().zip(__mt_to).map(|(__a, __b)| (__a as i64, __b as i64)).collect::<::std::collections::HashMap<i64, i64>>() }}",
                            from, to
                        )));
                    }
                    // (W5-b) `bytes.fromhex(s)` STATIC constructor -> Vec<u8>. Matched
                    // structurally (like str.maketrans) because the receiver `bytes` is
                    // a type name, not a bytes value, so it never reaches the bytes-
                    // receiver dispatch. Whitespace between pairs is ignored; an odd
                    // length / bad digit is a catchable ValueError with CPython's exact
                    // position/message (validated in the prelude helper).
                    if name == "fromhex"
                        && matches!(obj.as_ref(), Expr::Ident(bn, _) if bn == "bytes")
                        && args.len() == 1
                    {
                        let s = self.emit_expr(&args[0])?;
                        return Ok(Some(format!("__py_bytes_fromhex(&({}))", s)));
                    }
                    // Qualified module call `X.f(args)` for a REAL imported module
                    // (card 81db88e0). When X is a tracked module name and f is one
                    // of its functions, lower the call to the FLAT function `f(args)`
                    // — every imported module's functions are merged into `ctx.funcs`
                    // under their bare name, so the flat call resolves at codegen and
                    // build. We re-dispatch through `emit_call` with a synthesized
                    // `Ident(f)` callee so the regular function-call machinery
                    // (Optional/by-ref/default-argument coercion) applies uniformly,
                    // exactly as if the user had written `from X import f; f(args)`.
                    // `math` is now a REAL embedded module (`lib/math.pyrs`), so
                    // `math.sqrt(x)` flows through here too (its @extern `sqrt`
                    // is merged into `module_funcs`/`ctx.funcs`); the former
                    // hardcoded math call-arm is gone. We re-dispatch through
                    // `emit_plain_func_call` (NOT `emit_call`) so a module
                    // function whose flat name COLLIDES with a builtin — e.g.
                    // `math.pow` vs the builtin `pow` — calls the MODULE function,
                    // not the builtin int-pow. NOTE: flat emission means a
                    // cross-module same-name collision between two modules is
                    // unresolved (stdlib uses distinct names; per-module
                    // namespacing `X__f` is a later refinement).
                    // (W3-2/W3-3) The qualifier is the owner module id: a
                    // single-component `X.f()` (`obj = Ident("X")` → owner "X") or a
                    // TWO-component dotted `a.b.f()` (`obj = Attr{Ident("a"), "b"}` →
                    // owner "a.b"). Any other receiver shape is not a module call.
                    let qual_owner: Option<String> = match obj.as_ref() {
                        Expr::Ident(m, _) => Some(m.clone()),
                        Expr::Attr { obj: inner, name: b, .. } => match inner.as_ref() {
                            Expr::Ident(a, _) => Some(format!("{}.{}", a, b)),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(modname) = qual_owner {
                        if self.ctx.module_funcs.get(&modname).is_some_and(|fns| fns.iter().any(|n| n == name)) {
                            let span = callee.span();
                            let flat_callee: Box<Expr> = Box::new(Expr::Ident(name.clone(), span));
                            // (card d8a1ed83, kwargs v1) Forward `kwargs` (NOT
                            // `&[]`) so a kwarg on a qualified module call flows
                            // into emit_plain_func_call's keyword→positional
                            // mapping (`textwrap.fill(text, width=10)` lowers
                            // exactly like the flat `fill(text, width=10)`).
                            // (W3-2/W3-3) The qualifier `modname` IS the owner module
                            // (single or dotted id): thread it as `callee_owner` so
                            // the callee emits the owner-mangled
                            // `__pyrst_m_<mangle_mod_id(modname)>__<name>` (e.g.
                            // `os.path.basename` → `__pyrst_m_os_dpath__basename`)
                            // while the synthesized BARE `Ident(name)` still drives
                            // the sig / kwarg / coercion lookups (which key by bare
                            // name).
                            return Ok(Some(self.emit_plain_func_call(&flat_callee, args, kwargs, Some(modname))?));
                        }
                    }

                    // Check for static method calls: ClassName.method(args)
                    if let Expr::Ident(class_name, _) = obj.as_ref() {
                        if let Some(class_def) = self.ctx.classes.get(class_name.as_str()) {
                            if let Some(method_def) = class_def.methods.iter().find(|m| &m.name == name) {
                                if method_def.decorators.contains(&"staticmethod".to_string()) {
                                    // (enabler-fix-1 #6) Coerce every positional arg AND
                                    // every filled default into its parameter slot
                                    // (Some-wrap an `Optional[T]`, cast a `Callable`,
                                    // wrap a poly-base). The old path emitted positionals
                                    // via a bare emit_consuming and defaults via a bare
                                    // emit_expr, so a value into an `Optional[T]`
                                    // static-method slot leaked rustc E0308.
                                    let sig = self.ctx.get_method(class_name, name);
                                    let param_tys: Vec<Ty> = sig
                                        .as_ref()
                                        .map(|s| s.params.iter().map(|(_, t)| t.clone()).collect())
                                        .unwrap_or_default();
                                    let mut parts: Vec<String> = Vec::with_capacity(args.len());
                                    for (i, a) in args.iter().enumerate() {
                                        parts.push(self.emit_arg_into_slot(a, param_tys.get(i))?);
                                    }
                                    if let Some(sig) = &sig {
                                        if parts.len() < sig.params.len() {
                                            let start = sig.param_defaults.len()
                                                .saturating_sub(sig.params.len() - parts.len());
                                            // (W3-fix / F6) A static method's default
                                            // expression lives in its CLASS's module —
                                            // emit it under that owner so a bare
                                            // helper/const inside resolves owner-first
                                            // there, not the caller's scope.
                                            let def_owner = self.ctx.class_owner.get(class_name).cloned();
                                            for (off, def_expr) in sig.param_defaults[start..].iter().enumerate() {
                                                if let Some(e) = def_expr {
                                                    let e = e.clone();
                                                    let v = self.with_default_scope(def_owner.clone(), |cg| {
                                                        cg.emit_call_arg_value(&e, &param_tys, start + off, /*coerced=*/ true)
                                                    })?;
                                                    parts.push(v);
                                                }
                                            }
                                        }
                                    }
                                    return Ok(Some(format!("{}::{}({})", self.emit_class_name(class_name), name, parts.join(", "))));
                                }
                            }
                        }
                    }

                    // Mutating list/set/dict methods need an lvalue receiver. For
                    // a *subscripted* receiver (`self.rows[i].append(x)`,
                    // `grid[r].sort()`) emit_expr would produce a clone-based
                    // rvalue, so the mutation would hit (and drop) a temporary.
                    // Use emit_place for those so the in-place mutation lands on
                    // the real element. Bare-name and `self.field` receivers are
                    // already place expressions under emit_expr.
                    // MUTATING_METHODS is the module-level const above.
                    // (card cc7ae370, item 1) Hoist the subscript receiver's index
                    // temps into `recv_prelude`; the wrapper blocks the whole call
                    // so the temp runs before the `&mut` receiver borrow (E0502).
                    // The receiver stays a bare place here, preserving two-phase
                    // borrows for the method's arguments.
                    let obj_s = if matches!(obj.as_ref(), Expr::Index { .. })
                        && MUTATING_METHODS.contains(&name.as_str())
                    {
                        self.emit_place_hoisted(obj, recv_prelude)?
                    } else {
                        self.emit_expr(obj)?
                    };
                    let method = match name.as_str() {
                        // String methods
                        "upper"      => "to_uppercase",
                        "lower"      => "to_lowercase",
                        "strip"      => "trim",
                        "lstrip"     => "trim_start",
                        "rstrip"     => "trim_end",
                        // List methods
                        "append"     => "push",
                        "pop"        => "pop().unwrap",
                        // passthrough
                        other        => other,
                    };
                    let parts: Result<Vec<_>> = args.iter().map(|a| self.emit_consuming(a)).collect();
                    let parts = parts?;

                    // (EPIC-6) Receiver-type-guarded early return. The builtin
                    // method arms below match purely on `name` with NO receiver
                    // guard on most of them (`get`, `keys`, `values`, `items`,
                    // `update`, `pop`, `copy`, `clear`, `append`, `extend`,
                    // `insert`, `remove`, `sort`, ...). So a USER class that
                    // defines a method with one of those names previously had
                    // `instance.get(k)` silently lowered to a dict
                    // `.get(&k).cloned()` (wrong Rust / wrong behavior / a
                    // compile error) — the builtin arm won because it ran BEFORE
                    // the user-method tail. Guard it here: if the receiver's
                    // static type is a user class that HAS an instance method
                    // named `name` (resolved via `get_method`, walking the
                    // inheritance chain — the SAME lookup the user-method tail
                    // uses), dispatch to that user method NOW and return,
                    // bypassing every builtin arm. A builtin receiver
                    // (str/list/dict/set/file) is never `Ty::Class`, so the
                    // guard never fires for it and the builtin arms below run
                    // byte-for-byte unchanged. A polymorphic-base receiver
                    // composes too: `cls` is the base name, `get_method` returns
                    // the base's signature, and `obj_s.name(..)` resolves to the
                    // companion enum `cls__`'s dispatch method — identical to the
                    // pre-existing EPIC-5 lowering.
                    if let Ty::Class(cls, cls_args) = self.type_of_expr(obj.as_ref()) {
                        // `x.__str__()` / `x.__repr__()` are Python's stringify
                        // dunders. pyrst lowers __str__/__repr__ to the Display impl
                        // and the `PyRepr` trait — NOT inherent methods (they are
                        // skipped in the inherent impl block) — so a direct method
                        // call must route to those, not to a non-existent
                        // `self.__str__()`/`self.__repr__()`. This makes the common
                        // `def __repr__(self): return self.__str__()` delegation
                        // (e.g. time.struct_time) compile. Display already resolves
                        // __str__-or-__repr__ per CPython's str() fallback.
                        if name == "__str__" && args.is_empty() && kwargs.is_empty() {
                            return Ok(Some(format!("format!(\"{{}}\", {})", obj_s)));
                        }
                        if name == "__repr__" && args.is_empty() && kwargs.is_empty() {
                            return Ok(Some(format!("({}).py_repr()", obj_s)));
                        }
                        if self.ctx.get_method(&cls, name).is_some() {
                            // (card e10df981) Reconstruct the receiver instance type
                            // (with its type args) so the method-call path can
                            // substitute the class's type vars into the sig's param
                            // types. Empty args (non-generic receiver) => no-op subst.
                            let recv_ty = Ty::Class(cls.clone(), cls_args.clone());
                            return self
                                .emit_user_method_call(&obj_s, &cls, name, args, kwargs, &parts, callee.span(), &recv_ty)
                                .map(Some);
                        }
                        // Not a method — a `Callable` FIELD invoked as `obj.f(args)`.
                        // The field holds an `Rc<dyn Fn(..) -> ..>`, so Rust needs the
                        // field-access parenthesised before the call: `(obj.f)(args)`.
                        // (`obj.f(args)` would be parsed as a method named `f`, which
                        // does not exist — E0599.) Resolved via the same field-type
                        // lookup used elsewhere so it walks the inheritance chain.
                        if let Some(field_ty) = self
                            .ctx
                            .classes
                            .get(cls.as_str())
                            .and_then(|cd| self.class_field_type(cd, name))
                        {
                            if matches!(field_ty, Ty::Func(..)) {
                                return Ok(Some(format!(
                                    "({}.{})({})",
                                    obj_s,
                                    escape_ident(name),
                                    parts.join(", ")
                                )));
                            }
                        }
                    }

                    // (W5-b) bytes methods — gated on a bytes receiver and dispatched
                    // BEFORE every str/list arm below. Those arms match on `name`
                    // ALONE (no receiver guard) — `find`/`split`/`replace`/`count`/
                    // `index`/`strip`/`upper`/`lower`/`join`/`startswith` all overlap
                    // bytes' surface — so a bytes receiver reaching them would emit
                    // `.as_str()`/`.chars()`/`__py_str_find` (a rustc leak or a
                    // silent char-offset miscompile). This wins the overlap, mirroring
                    // the set-method receiver-gating below. typeck already proved the
                    // method exists and its args/arity are valid (check_bytes_method_call).
                    if matches!(self.type_of_expr(obj.as_ref()), Ty::Bytes) {
                        return self.emit_bytes_method_call(&obj_s, name, &parts).map(Some);
                    }
                    // (W5-b) `str.encode()` / `encode('utf-8')` -> bytes. A String's
                    // bytes ARE UTF-8, so this is a borrow-and-copy (never consumes the
                    // receiver). utf-8 only; the encoding/errors args were validated at
                    // check (`check_str_encode_call`).
                    if name == "encode" && matches!(self.type_of_expr(obj.as_ref()), Ty::Str) {
                        return Ok(Some(format!("{}.as_bytes().to_vec()", obj_s)));
                    }

                    // Special handling for string methods that return &str and need to be converted to String
                    if matches!(name.as_str(), "strip" | "lstrip" | "rstrip") {
                        return Ok(Some(format!("{}.{}().to_string()", obj_s, method)));
                    }

                    // Special case: split()
                    if name == "split" {
                        return if args.is_empty() {
                            Ok(Some(format!("{}.split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>()", obj_s)))
                        } else {
                            let sep = parts[0].clone();
                            Ok(Some(format!("{}.split({}.as_str()).map(|s| s.to_string()).collect::<Vec<_>>()", obj_s, sep)))
                        };
                    }

                    // Special case: join()
                    if name == "join" {
                        return Ok(Some(format!("{}.join(&{})", parts[0], obj_s)));
                    }

                    // Special case: len() as method
                    if name == "len" {
                        // str length is character count, not UTF-8 byte count.
                        // (W1.5) PARENTHESIZED: a bare `x.len() as i64 < n`
                        // makes rustc parse `i64<` as generic arguments.
                        if matches!(self.type_of_expr(obj.as_ref()), Ty::Str) {
                            return Ok(Some(format!("({}.chars().count() as i64)", obj_s)));
                        }
                        return Ok(Some(format!("({}.len() as i64)", obj_s)));
                    }

                    // Special case: get() for dicts. Arg-count-aware, mirroring
                    // the static typing in `typeck::dict_get_ret`:
                    //   d.get(k)           -> Option<V>  (None when absent), so a
                    //                         caller can narrow it with `is None`.
                    //   d.get(k, default)  -> V          (the supplied fallback).
                    if name == "get" {
                        // A user-class receiver with a `get` method has already been
                        // dispatched above; reaching here means a dict `.get()`. It
                        // requires at least the key argument — a no-arg `.get()` is an
                        // honest error (NEVER an index-out-of-bounds panic on parts[0]).
                        if parts.is_empty() {
                            return Err(crate::diag::Error::Codegen(
                                "`.get()` requires a key argument (dict.get(k) or dict.get(k, default))".into(),
                            ));
                        }
                        if parts.len() > 1 {
                            return Ok(Some(format!(
                                "{}.get(&{}).cloned().unwrap_or({})",
                                obj_s, parts[0], parts[1]
                            )));
                        }
                        return Ok(Some(format!("{}.get(&{}).cloned()", obj_s, parts[0])));
                    }

                    // String methods
                    if name == "startswith" && !parts.is_empty() {
                        return Ok(Some(format!("{}.starts_with({}.as_str())", obj_s, parts[0])));
                    }
                    if name == "endswith" && !parts.is_empty() {
                        return Ok(Some(format!("{}.ends_with({}.as_str())", obj_s, parts[0])));
                    }
                    if name == "replace" && parts.len() >= 2 {
                        return Ok(Some(format!("{}.replace({}.as_str(), {}.as_str())", obj_s, parts[0], parts[1])));
                    }
                    if name == "removeprefix" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __prefix = {}; \
                            if __s.starts_with(__prefix.as_str()) {{ __s[__prefix.len()..].to_string() }} else {{ __s }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "removesuffix" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __suffix = {}; \
                            if __s.ends_with(__suffix.as_str()) {{ __s[..__s.len() - __suffix.len()].to_string() }} else {{ __s }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "expandtabs" {
                        // (W1.5, textwrap pipeline) REAL CPython tab-stop
                        // semantics: each '\t' pads to the next multiple of
                        // `tabsize` from the current COLUMN, which resets on
                        // '\n'/'\r'; tabsize <= 0 deletes tabs. The previous
                        // lowering (`replace('\t', " ".repeat(n))`) padded a
                        // fixed width regardless of column — python3-diffed
                        // wrong for any text with a non-empty prefix before
                        // the tab ("a\tb".expandtabs(8) is "a       b", 7
                        // pad spaces, not 8).
                        let tab_size = if !parts.is_empty() {
                            format!("{}", parts[0])
                        } else {
                            "8i64".to_string()
                        };
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __t: i64 = {}; \
                            let __tab = if __t < 1 {{ 0usize }} else {{ __t as usize }}; \
                            let mut __out = String::new(); let mut __col = 0usize; \
                            for __ch in __s.chars() {{ \
                            if __ch == '\\t' {{ if __tab > 0 {{ let __pad = __tab - (__col % __tab); \
                            for _ in 0..__pad {{ __out.push(' '); }} __col += __pad; }} }} \
                            else if __ch == '\\n' || __ch == '\\r' {{ __out.push(__ch); __col = 0; }} \
                            else {{ __out.push(__ch); __col += 1; }} }} \
                            __out }}",
                            obj_s, tab_size
                        )));
                    }
                    if name == "partition" && !parts.is_empty() {
                        // (card 49170944) Return a 3-TUPLE (String, String, String)
                        // — CPython's real shape — so `head, sep, tail =
                        // s.partition("=")` unpacks (typeck types this as
                        // Tuple(Str,Str,Str)). Was a `vec![..]` (list) before, which
                        // diverged from CPython and blocked the idiomatic unpack.
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __sep = {}; \
                            if let Some(__idx) = __s.find(__sep.as_str()) {{ \
                            (__s[..__idx].to_string(), __sep.clone(), __s[__idx + __sep.len()..].to_string()) \
                            }} else {{ (__s, String::new(), String::new()) }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    if name == "rpartition" && !parts.is_empty() {
                        // (card 49170944) 3-TUPLE like partition; the no-match case
                        // puts the whole string in the LAST slot (CPython semantics).
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); let __sep = {}; \
                            if let Some(__idx) = __s.rfind(__sep.as_str()) {{ \
                            (__s[..__idx].to_string(), __sep.clone(), __s[__idx + __sep.len()..].to_string()) \
                            }} else {{ (String::new(), String::new(), __s) }} }}",
                            obj_s, parts[0]
                        )));
                    }
                    // (card 49170944) casefold(): SIMPLE-casefold.
                    // (enabler-fix-2 #7) CONTEXT-FREE: fold each char INDEPENDENTLY
                    // via `char::to_lowercase` — NOT `str::to_lowercase`, which applies
                    // the Unicode SpecialCasing final-sigma rule (a word-final Σ ->
                    // ς / U+03C2) and so diverged from CPython, whose casefold is
                    // context-free (every Σ -> σ / U+03C3). Per-char folding matches
                    // CPython for ASCII / İ / word-final Σ and all 1:1 mappings.
                    // STILL simple-fold only: ß stays ß and ﬁ stays ﬁ (CPython
                    // full-folds them to "ss" / "fi") — the full-fold table is out of
                    // scope; documented precisely in PYTHON_COMPATIBILITY.md.
                    if name == "casefold" {
                        return Ok(Some(format!(
                            "{}.chars().flat_map(|__c| __c.to_lowercase()).collect::<String>()",
                            obj_s
                        )));
                    }
                    // (card 49170944) rsplit(sep[, maxsplit]) — python3-exact. Rust's
                    // `rsplitn` yields pieces RIGHT-to-LEFT, so collect + reverse to
                    // restore CPython's left-to-right list. A negative maxsplit (or
                    // absent) means unlimited == plain `split`. The 1-arg form is
                    // exactly `split` (no limit). The no-sep whitespace form is out of
                    // scope here (honest-rejected: rsplit requires a separator).
                    if name == "rsplit" {
                        if parts.is_empty() {
                            return Err(crate::diag::Error::Codegen(
                                "`rsplit()` without a separator is not supported — pass a \
                                 separator (rsplit(sep) or rsplit(sep, maxsplit))".into(),
                            ));
                        }
                        if parts.len() >= 2 {
                            return Ok(Some(format!(
                                "{{ let __s = {}.clone(); let __sep = {}; let __n: i64 = {}; \
                                if __n < 0 {{ __s.split(__sep.as_str()).map(|p| p.to_string()).collect::<Vec<String>>() }} \
                                else {{ let mut __v: Vec<String> = __s.rsplitn((__n as usize) + 1, __sep.as_str()).map(|p| p.to_string()).collect(); __v.reverse(); __v }} }}",
                                obj_s, parts[0], parts[1]
                            )));
                        }
                        return Ok(Some(format!(
                            "{}.split({}.as_str()).map(|p| p.to_string()).collect::<Vec<String>>()",
                            obj_s, parts[0]
                        )));
                    }
                    // (card 49170944) translate(table): apply an int->int code-point
                    // map (`dict[int, int]`, e.g. from str.maketrans). Each char whose
                    // code point is a key is replaced by chr(value); others pass
                    // through unchanged. The delete form (None values / 3-arg
                    // maketrans) needs `dict[int, Optional[int]]` and is out of scope
                    // (honest subset — documented).
                    if name == "translate" && !parts.is_empty() {
                        return Ok(Some(format!(
                            "{{ let __t = &{}; {}.chars().map(|__c| \
                            match __t.get(&(__c as i64)) {{ \
                            Some(&__r) => ::std::char::from_u32(__r as u32).unwrap_or(__c), \
                            None => __c }}).collect::<String>() }}",
                            parts[0], obj_s
                        )));
                    }
                    if name == "find" && !parts.is_empty() {
                        // (card dfd89c12) CHARACTER offset like CPython — the old
                        // `str::find(...).map(|i| i as i64)` leaked Rust's BYTE offset,
                        // silently corrupting slice/index math on any multibyte string
                        // (`"café.txt".find(".")` gave 5, python3 gives 4). __py_str_find
                        // converts byte->char and binds the receiver once (no double eval).
                        return Ok(Some(format!("__py_str_find(&({}), &({}))", obj_s, parts[0])));
                    }
                    if name == "contains" && !parts.is_empty() {
                        return Ok(Some(format!("{}.contains({}.as_str())", obj_s, parts[0])));
                    }
                    if name == "rfind" && !parts.is_empty() {
                        // (card dfd89c12) CHARACTER offset like CPython (see find above).
                        return Ok(Some(format!("__py_str_rfind(&({}), &({}))", obj_s, parts[0])));
                    }
                    if name == "rindex" && !parts.is_empty() {
                        // (card dfd89c12) CHARACTER offset; raises the catchable
                        // ValueError on a miss (CPython `substring not found`).
                        return Ok(Some(format!("__py_str_rindex(&({}), &({}))", obj_s, parts[0])));
                    }

                    // String utility methods
                    if name == "isdigit" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_numeric()))", obj_s, obj_s)));
                    }
                    if name == "isalpha" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_alphabetic()))", obj_s, obj_s)));
                    }
                    if name == "isupper" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) && {}.chars().any(|c| c.is_alphabetic()))", obj_s, obj_s, obj_s)));
                    }
                    if name == "islower" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_lowercase()) && {}.chars().any(|c| c.is_alphabetic()))", obj_s, obj_s, obj_s)));
                    }
                    if name == "isspace" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_whitespace()))", obj_s, obj_s)));
                    }
                    if name == "isalnum" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_alphanumeric()))", obj_s, obj_s)));
                    }
                    if name == "isidentifier" {
                        return Ok(Some(format!(
                            "(!{}.is_empty() && ({}.chars().next().unwrap().is_alphabetic() || {}.chars().next().unwrap() == '_') && {}.chars().all(|c| c.is_alphanumeric() || c == '_'))",
                            obj_s, obj_s, obj_s, obj_s
                        )));
                    }
                    if name == "isnumeric" {
                        return Ok(Some(format!("(!{}.is_empty() && {}.chars().all(|c| c.is_numeric()))", obj_s, obj_s)));
                    }
                    if name == "isprintable" {
                        return Ok(Some(format!("({}.chars().all(|c| !c.is_control()))", obj_s)));
                    }
                    if name == "istitle" {
                        // (W1.5 fix E) CPython's exact single-pass cased-run rule
                        // (unicodeobject.c unicode_istitle_impl), replacing the old
                        // whitespace-word predicate that wrongly answered True on
                        // uncased-separated words (`"A1a".istitle()` -> True, python3
                        // False; `"A1A"` -> False, python3 True). An UPPER/TITLECASE
                        // char must NOT follow a cased char; a LOWERCASE char MUST
                        // follow a cased char; any UNCASED char (digit/space/punct)
                        // resets the run; the whole string must contain >=1 cased
                        // char. `__is_upper` also admits titlecase (Lt) digraphs —
                        // neither is_uppercase nor is_lowercase, but with distinct
                        // upper AND lower mappings — matching Py_UNICODE_ISTITLE.
                        return Ok(Some(format!(
                            "{{ let mut __cased = false; let mut __prev_cased = false; let mut __ok = true; for __c in {}.chars() {{ let __is_upper = __c.is_uppercase() || (!__c.is_lowercase() && __c.to_uppercase().next() != Some(__c) && __c.to_lowercase().next() != Some(__c)); if __is_upper {{ if __prev_cased {{ __ok = false; break; }} __prev_cased = true; __cased = true; }} else if __c.is_lowercase() {{ if !__prev_cased {{ __ok = false; break; }} __prev_cased = true; __cased = true; }} else {{ __prev_cased = false; }} }} (__ok && __cased) }}",
                            obj_s
                        )));
                    }

                    // Additional string methods
                    if name == "capitalize" {
                        // (W1.5, card b671f313) CHAR-based, CPython-exact: the
                        // old `&__s[1..]` BYTE slice panicked on a multibyte
                        // first char (capwords("héllo") crash). Now: first
                        // char TITLECASED (__py_titlecase — ß -> "Ss", ǆ -> ǅ,
                        // like python3), rest = the FULL string's
                        // to_lowercase() minus the first char's lowered bytes,
                        // so the Final_Sigma context is preserved
                        // ("ΑΣ".capitalize() == "Ας", python3-diffed).
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); match __s.chars().next() {{ None => __s, Some(__c0) => {{ let __low = __s.to_lowercase(); let __skip: usize = __c0.to_lowercase().map(|c| c.len_utf8()).sum(); format!(\"{{}}{{}}\", __py_titlecase(__c0), &__low[__skip..]) }} }} }}",
                            obj_s
                        )));
                    }
                    if name == "title" {
                        // (W1.5, card b671f313) Same byte-slice crash fixed
                        // (multibyte-initial words), same titlecase+context-
                        // lower treatment per word. KNOWN DIVERGENCE (kept,
                        // documented): word boundaries are WHITESPACE here
                        // (split_whitespace + single-space join); CPython
                        // titles after ANY non-cased char ("don't".title() is
                        // "Don'T" there, "Don't" here) and preserves the
                        // original whitespace.
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); __s.split_whitespace().map(|w| match w.chars().next() {{ None => w.to_string(), Some(__c0) => {{ let __low = w.to_lowercase(); let __skip: usize = __c0.to_lowercase().map(|c| c.len_utf8()).sum(); format!(\"{{}}{{}}\", __py_titlecase(__c0), &__low[__skip..]) }} }} ).collect::<Vec<_>>().join(\" \") }}",
                            obj_s
                        )));
                    }
                    if name == "zfill" && !parts.is_empty() {
                        // (W1.5, card b671f313 audit) Width was compared in
                        // BYTES (multibyte strings silently under/over-padded)
                        // and a leading sign was zero-padded on the WRONG side
                        // ("-42".zfill(5) gave "00-42", CPython gives
                        // "-0042"). Now: char-count width, sign kept first.
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); let __n = __s.chars().count(); if __n >= __width {{ __s }} else {{ let (__sign, __body) = if __s.starts_with('+') || __s.starts_with('-') {{ __s.split_at(1) }} else {{ (\"\", __s.as_str()) }}; format!(\"{{}}{{}}{{}}\", __sign, \"0\".repeat(__width - __n), __body) }} }}",
                            parts[0], obj_s
                        )));
                    }
                    if name == "ljust" && !parts.is_empty() {
                        // (W1.5, card b671f313 audit) BYTE-length width bug
                        // fixed: pad by CHAR count like CPython.
                        // (W1.5 fix E) The optional 2nd arg (fillchar) is now
                        // HONORED — it used to be silently ignored (always padded
                        // with a space). CPython requires exactly one char, else
                        // TypeError.
                        let fill = Self::justify_fillchar(&parts);
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); {} let __n = __s.chars().count(); if __n >= __width {{ __s }} else {{ format!(\"{{}}{{}}\", __s, __fill.repeat(__width - __n)) }} }}",
                            parts[0], obj_s, fill
                        )));
                    }
                    if name == "rjust" && !parts.is_empty() {
                        // (W1.5, card b671f313 audit) BYTE-length width bug
                        // fixed: pad by CHAR count like CPython.
                        // (W1.5 fix E) fillchar now honored (see ljust).
                        let fill = Self::justify_fillchar(&parts);
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); {} let __n = __s.chars().count(); if __n >= __width {{ __s }} else {{ format!(\"{{}}{{}}\", __fill.repeat(__width - __n), __s) }} }}",
                            parts[0], obj_s, fill
                        )));
                    }
                    if name == "center" && !parts.is_empty() {
                        // (W1.5, card b671f313 audit) BYTE-length width bug
                        // fixed + CPython's exact left-margin rule:
                        // left = marg/2 + (marg & width & 1) — the old
                        // (total+1)/2 flipped which side gets the odd space
                        // for even widths ("abc".center(6) is " abc  " in
                        // CPython, not "  abc ").
                        // (W1.5 fix E) fillchar now honored (see ljust) — the
                        // byte-width rewrite still hardcoded a space fill.
                        let fill = Self::justify_fillchar(&parts);
                        return Ok(Some(format!(
                            "{{ let __width = {} as usize; let __s = {}.clone(); {} let __n = __s.chars().count(); if __n >= __width {{ __s }} else {{ let __marg = __width - __n; let __left = __marg / 2 + (__marg & __width & 1); format!(\"{{}}{{}}{{}}\", __fill.repeat(__left), __s, __fill.repeat(__marg - __left)) }} }}",
                            parts[0], obj_s, fill
                        )));
                    }
                    if name == "swapcase" {
                        return Ok(Some(format!(
                            "{{ let __s = {}.clone(); __s.chars().map(|c| if c.is_uppercase() {{ c.to_lowercase().to_string() }} else {{ c.to_uppercase().to_string() }} ).collect::<String>() }}",
                            obj_s
                        )));
                    }
                    if name == "splitlines" {
                        return Ok(Some(format!(
                            "{}.lines().map(|l| l.to_string()).collect::<Vec<_>>()",
                            obj_s
                        )));
                    }
                    if name == "count" && !parts.is_empty() {
                        let obj_ty = self.type_of_expr(obj);
                        match obj_ty {
                            Ty::Str => {
                                return Ok(Some(format!(
                                    "{{ let __s = {}.clone(); let __sub = {}; let mut __count = 0i64; let mut __start = 0; while let Some(__pos) = __s.as_str()[__start..].find(__sub.as_str()) {{ __count += 1; __start += __pos + __sub.len(); }} __count }}",
                                    obj_s, parts[0]
                                )));
                            }
                            _ => {} // Fall through to list count below
                        }
                    }
                    if name == "index" && !parts.is_empty() {
                        let obj_ty = self.type_of_expr(obj);
                        match obj_ty {
                            Ty::Str => {
                                // (card dfd89c12) CHARACTER offset like CPython; raises the
                                // catchable ValueError on a miss (see find/__py_str_index).
                                return Ok(Some(format!("__py_str_index(&({}), &({}))", obj_s, parts[0])));
                            }
                            _ => {} // Fall through to list index below
                        }
                    }

                    // File methods (PyFile; gated on a `file` handle receiver).
                    // write takes &str, so borrow the argument. (W5-g: the receiver
                    // is now `Ty::Handle("file")`; a method call BORROWS `&mut self`,
                    // it does not consume — the receiver is not a move site.)
                    if matches!(self.type_of_expr(obj), Ty::Handle(ref n) if n == "file") {
                        match name.as_str() {
                            "write" if !parts.is_empty() => return Ok(Some(format!("{}.write(&{})", obj_s, parts[0]))),
                            "write" => return Err(crate::diag::Error::Codegen("file write() requires one argument".into())),
                            "read" | "readlines" | "close" =>
                                return Ok(Some(format!("{}.{}()", obj_s, name))),
                            _ => {}
                        }
                    }

                    // Dict views - materialize into a Vec so they work both in a
                    // for-loop and as a value (e.g. print(d.keys()), len(d.values())),
                    // matching their List(K)/List(V) static type.
                    if name == "keys" {
                        return Ok(Some(format!("{}.keys().cloned().collect::<Vec<_>>()", obj_s)));
                    }
                    if name == "values" {
                        return Ok(Some(format!("{}.values().cloned().collect::<Vec<_>>()", obj_s)));
                    }
                    if name == "items" {
                        // Collect into a Vec<(K, V)> so the for-loop lowering treats it
                        // as a normal collection (it wraps the iterable in .iter().cloned()).
                        return Ok(Some(format!("{}.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>()", obj_s)));
                    }

                    // Set methods (gated on receiver type — many names overlap with
                    // list/dict, so disambiguate by the static type of the receiver).
                    if let Ty::Set(_) = self.type_of_expr(obj) {
                        match name.as_str() {
                            // insert takes ownership, so emit the element owned
                            // (a String var becomes `x.clone()`).
                            "add" if !parts.is_empty() =>
                                return Ok(Some(format!("{{ {}.insert({}); }}", obj_s, self.emit_consuming(&args[0])?))),
                            // NB: unlike Python, neither discard nor remove raises on an
                            // absent element here (Rust's HashSet::remove returns an ignored bool).
                            "discard" | "remove" if !parts.is_empty() =>
                                return Ok(Some(format!("{{ {}.remove(&{}); }}", obj_s, parts[0]))),
                            "update" if !parts.is_empty() =>
                                return Ok(Some(format!("{{ {}.extend({}.iter().cloned()); }}", obj_s, parts[0]))),
                            "union" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.union(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "intersection" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.intersection(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "difference" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.difference(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "symmetric_difference" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.symmetric_difference(&{}).cloned().collect::<std::collections::HashSet<_>>()", obj_s, parts[0]))),
                            "issubset" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.is_subset(&{})", obj_s, parts[0]))),
                            "issuperset" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.is_superset(&{})", obj_s, parts[0]))),
                            "isdisjoint" if !parts.is_empty() =>
                                return Ok(Some(format!("{}.is_disjoint(&{})", obj_s, parts[0]))),
                            _ => {}
                        }
                    }

                    // dict.update(other) — merge another mapping in place.
                    if name == "update" && !parts.is_empty() {
                        return Ok(Some(format!("{{ {}.extend({}); }}", obj_s, parts[0])));
                    }

                    if name == "pop" {
                        // list.pop(): remove and return the last element (or pop(i) -> remove index).
                        if let Ty::List(_) = self.type_of_expr(obj) {
                            return Ok(Some(if parts.is_empty() {
                                format!("{}.pop().unwrap_or_else(|| panic!(\"IndexError\\0pop from empty list\"))", obj_s)
                            } else {
                                // Honor Python negative indices: pop(-1) is the last element.
                                format!(
                                    "{{ let __n = {obj}.len() as i64; let __i = {idx}; \
                                     {obj}.remove((if __i < 0 {{ __n + __i }} else {{ __i }}) as usize) }}",
                                    obj = obj_s, idx = parts[0]
                                )
                            }));
                        }
                        // dict.pop(key[, default])
                        if parts.is_empty() {
                            return Err(crate::diag::Error::Codegen("pop requires at least one argument".into()));
                        } else if parts.len() == 1 {
                            // pop(key) — remove from the receiver and return the value (panic if absent)
                            return Ok(Some(format!("{}.remove(&{}).unwrap_or_else(|| panic!(\"KeyError\\0<key>\"))", obj_s, parts[0])));
                        } else {
                            // pop(key, default) — remove from the receiver; default if absent
                            return Ok(Some(format!("{}.remove(&{}).unwrap_or({})", obj_s, parts[0], parts[1])));
                        }
                    }
                    // List methods
                    if name == "extend" && !parts.is_empty() {
                        return Ok(Some(format!("{}.extend({})", obj_s, parts[0])));
                    }
                    if name == "insert" && parts.len() >= 2 {
                        return Ok(Some(format!("{}.insert({} as usize, {})", obj_s, parts[0], parts[1])));
                    }
                    if name == "remove" && !parts.is_empty() {
                        return Ok(Some(format!("{{ let __idx = {}.iter().position(|__x| *__x == {}).unwrap_or_else(|| panic!(\"ValueError\\0value not found\")); {}.remove(__idx); }}", obj_s, parts[0], obj_s)));
                    }
                    if name == "index" && !parts.is_empty() {
                        // (W1.5 fix D) Parenthesize the `as i64` cast — an
                        // unparenthesized `EXPR as i64` as the LEFTMOST operand of
                        // a comparison (`xs.index(v) < n`) is misparsed by rustc as
                        // generic arguments (`i64 < .. >`), the same E0747-class
                        // ambiguity fixed for len() this epic.
                        return Ok(Some(format!("({}.iter().position(|__x| *__x == {}).unwrap_or_else(|| panic!(\"ValueError\\0value not found\")) as i64)", obj_s, parts[0])));
                    }
                    if name == "count" && !parts.is_empty() {
                        // (W1.5 fix D) Parenthesize the `as i64` cast — see index above
                        // (`xs.count(1) < len(xs)` died at rustc without the parens).
                        return Ok(Some(format!("({}.iter().filter(|__x| **__x == {}).count() as i64)", obj_s, parts[0])));
                    }
                    if name == "reverse" {
                        return Ok(Some(format!("{}.reverse()", obj_s)));
                    }
                    if name == "sort" {
                        // Only `key` / `reverse` are valid `list.sort` kwargs — an
                        // unknown one (e.g. `.sort(bogus=1)`) is a TypeError in
                        // Python and would otherwise be silently ignored here.
                        if let Some((k, _)) = kwargs.iter().find(|(n, _)| n != "key" && n != "reverse") {
                            return Err(crate::diag::Error::Codegen(format!(
                                "list.sort() has no keyword argument `{}` (only `key` and `reverse`)",
                                k
                            )));
                        }
                        let list_ty = self.type_of_expr(obj.as_ref());
                        let elem_ty = match &list_ty {
                            Ty::List(inner) => (**inner).clone(),
                            _ => Ty::Unknown,
                        };
                        let rev_expr = kwargs.iter().find(|(n, _)| n == "reverse").map(|(_, e)| e);
                        // (Bug C) `list.sort(key=...)` — mirror the `sorted(..., key=)`
                        // comparator (the kwarg used to be silently dropped, sorting
                        // as if no key were given). Supports `key` alone and
                        // `key`+`reverse` (a REVERSED-COMPARATOR stable sort).
                        if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                            let float_key = matches!(self.sort_key_ret_ty(key_expr, &list_ty), Ty::Float);
                            let key_code = self.emit_sort_key_code(key_expr, &list_ty)?;
                            let key_cmp = if float_key {
                                "ka.partial_cmp(&kb).unwrap_or(::std::cmp::Ordering::Equal)"
                            } else {
                                "ka.cmp(&kb)"
                            };
                            if let Some(re) = rev_expr {
                                let rev = self.emit_expr(re)?;
                                return Ok(Some(format!(
                                    "{}.sort_by(|a, b| {{ let ka = {{ let __x = a.clone(); {} }}; let kb = {{ let __x = b.clone(); {} }}; let __ord = {}; if {} {{ __ord.reverse() }} else {{ __ord }} }})",
                                    obj_s, key_code, key_code, key_cmp, rev
                                )));
                            }
                            if float_key {
                                return Ok(Some(format!(
                                    "{}.sort_by(|a, b| {{ let ka = {{ let __x = a.clone(); {} }}; let kb = {{ let __x = b.clone(); {} }}; {} }})",
                                    obj_s, key_code, key_code, key_cmp
                                )));
                            }
                            return Ok(Some(format!("{}.sort_by_key(|__x| {})", obj_s, key_code)));
                        }
                        // (W0 follow-up) `list.sort(reverse=True)` used to silently
                        // drop `reverse`. Emit a REVERSED-COMPARATOR stable sort —
                        // equal elements keep input order (Python's stable reverse),
                        // not `.sort();.reverse()` which would flip them. No kwargs =
                        // the plain element-typed sort (Ord / partial_cmp).
                        if let Some(re) = rev_expr {
                            let rev = self.emit_expr(re)?;
                            let cmp = if self.elem_needs_partial_cmp(&elem_ty) {
                                "a.partial_cmp(b).unwrap_or(::std::cmp::Ordering::Equal)"
                            } else {
                                "a.cmp(b)"
                            };
                            return Ok(Some(format!(
                                "{}.sort_by(|a, b| {{ let __ord = {}; if {} {{ __ord.reverse() }} else {{ __ord }} }})",
                                obj_s, cmp, rev
                            )));
                        }
                        return Ok(Some(format!("{}{}", obj_s, self.sort_suffix_for_elem(&elem_ty))));
                    }
                    if name == "clear" {
                        return Ok(Some(format!("{}.clear()", obj_s)));
                    }
                    if name == "copy" {
                        return Ok(Some(format!("{}.clone()", obj_s)));
                    }

                    // Regular method call.
                    // (EPIC-4 V2-c) Thread `&mut <place>` for any by-reference
                    // (`Mut[T]`) method parameter so the callee's mutation persists
                    // to the caller. The method's per-param by-ref flags come from
                    // get_method (self-EXCLUSIVE and index-aligned to `args` after
                    // STEP 0). Only user-defined methods on a known class receiver
                    // can be by-ref; the builtin string/list/dict branches above
                    // all `return`ed earlier, so the by-value `parts` they share is
                    // never reached here. We rebuild `parts` only when the receiver
                    // resolves to a class with a matching method that actually has
                    // a by-ref param; otherwise the original by-value `parts`
                    // (clone-on-use) is used unchanged.
                    let method_by_ref: Vec<bool> =
                        if let Ty::Class(cls, _) = self.type_of_expr(obj.as_ref()) {
                            self.ctx.get_method(&cls, name)
                                .map(|sig| sig.param_by_ref.clone())
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                    if method_by_ref.iter().any(|&b| b) {
                        let mut mparts = Vec::with_capacity(args.len());
                        for (i, a) in args.iter().enumerate() {
                            if method_by_ref.get(i).copied().unwrap_or(false) {
                                // (card cc7ae370, item 1) Hoist + block-wrap a
                                // subscripted by-ref arg place (see the free-func
                                // by-ref path) so its index temp runs before `&mut`.
                                let mut aprelude = Vec::new();
                                let place = self.emit_place_hoisted(a, &mut aprelude)?;
                                let borrow = self.byref_borrow(a, &place);
                                mparts.push(Self::hoist_wrap(&aprelude, borrow));
                            } else {
                                mparts.push(self.emit_consuming(a)?);
                            }
                        }
                        return Ok(Some(format!("{}.{}({})", obj_s, method, mparts.join(", "))));
                    }
                    return Ok(Some(format!("{}.{}({})", obj_s, method, parts.join(", "))));
                }
        Ok(None)
    }

    /// (W5-b) Lower a `bytes` method call to the BYTE-offset `__py_bytes_*` prelude
    /// helper (mod.rs BYTES_PRELUDE), all python3-oracle-validated. `obj_s` is the
    /// receiver value expression; `parts` are the emit_consuming'd arguments (a
    /// `bytes` arg is a `Vec<u8>` value, an int arg an `i64`). typeck
    /// (`check_bytes_method_call`) already validated the method exists and its
    /// arity/arg-types, so every arm here is reachable only with valid inputs.
    pub(crate) fn emit_bytes_method_call(
        &mut self,
        obj_s: &str,
        name: &str,
        parts: &[String],
    ) -> Result<String> {
        // `&(recv)` / `&(arg)` are `&[u8]` views (a `&Vec<u8>` derefs to `&[u8]`).
        let recv = format!("&({})", obj_s);
        let a0 = || format!("&({})", parts[0]);
        match name {
            "hex" => Ok(format!("__py_bytes_hex({})", recv)),
            // decode consumes its Vec<u8>; clone so the receiver survives (Python
            // `b.decode()` does not consume `b`).
            "decode" => Ok(format!("__py_bytes_decode_utf8(({}).clone())", obj_s)),
            "find" => Ok(format!("__py_bytes_find({}, {})", recv, a0())),
            "rfind" => Ok(format!("__py_bytes_rfind({}, {})", recv, a0())),
            "index" => Ok(format!("__py_bytes_index_of({}, {})", recv, a0())),
            "rindex" => Ok(format!("__py_bytes_rindex_of({}, {})", recv, a0())),
            "count" => Ok(format!("__py_bytes_count({}, {})", recv, a0())),
            // starts_with/ends_with are slice methods; `&Vec<u8>` derefs to `&[u8]`.
            "startswith" => Ok(format!("({}).starts_with(&({}))", obj_s, parts[0])),
            "endswith" => Ok(format!("({}).ends_with(&({}))", obj_s, parts[0])),
            "replace" => Ok(format!(
                "__py_bytes_replace({}, {}, &({}))",
                recv, a0(), parts[1]
            )),
            // No maxsplit supported, so rsplit == split (order preserved) — both the
            // separator form and the no-arg whitespace form.
            "split" | "rsplit" => {
                if parts.is_empty() {
                    Ok(format!("__py_bytes_split_ws({})", recv))
                } else {
                    Ok(format!("__py_bytes_split({}, {})", recv, a0()))
                }
            }
            // sep.join(list[bytes]); parts[0] is a `Vec<Vec<u8>>` (-> `&[Vec<u8>]`).
            "join" => Ok(format!("__py_bytes_join({}, &({}))", recv, parts[0])),
            "strip" => Ok(self.emit_bytes_strip(obj_s, parts, true, true)),
            "lstrip" => Ok(self.emit_bytes_strip(obj_s, parts, true, false)),
            "rstrip" => Ok(self.emit_bytes_strip(obj_s, parts, false, true)),
            "upper" => Ok(format!("__py_bytes_upper({})", recv)),
            "lower" => Ok(format!("__py_bytes_lower({})", recv)),
            "ljust" => Ok(self.emit_bytes_pad(obj_s, parts, "ljust", false, true)),
            "rjust" => Ok(self.emit_bytes_pad(obj_s, parts, "rjust", true, false)),
            "center" => Ok(self.emit_bytes_pad(obj_s, parts, "center", true, true)),
            "zfill" => Ok(format!("__py_bytes_zfill({}, {})", recv, parts[0])),
            "isdigit" => Ok(format!("__py_bytes_all({}, __py_byte_is_digit)", recv)),
            "isalpha" => Ok(format!("__py_bytes_all({}, __py_byte_is_alpha)", recv)),
            "isalnum" => Ok(format!("__py_bytes_all({}, __py_byte_is_alnum)", recv)),
            "isspace" => Ok(format!("__py_bytes_all({}, __py_byte_is_space)", recv)),
            other => Err(crate::diag::Error::Codegen(format!(
                "bytes method `{}` is not supported (W5-b)",
                other
            ))),
        }
    }

    /// (W5-b) `b.strip()/lstrip()/rstrip()` — default strips the ASCII whitespace
    /// SET [9,10,11,12,13,32] (0x0b included, unlike Rust's `is_ascii_whitespace`);
    /// with an argument the arg is a SET of bytes to strip (CPython semantics).
    fn emit_bytes_strip(&self, obj_s: &str, parts: &[String], left: bool, right: bool) -> String {
        let set = if parts.is_empty() {
            "&[9u8, 10, 11, 12, 13, 32]".to_string()
        } else {
            format!("&({})", parts[0])
        };
        format!("__py_bytes_strip(&({}), {}, {}, {})", obj_s, set, left, right)
    }

    /// (W5-b) `b.ljust/rjust/center(width[, fillbyte])` — default fill is a space
    /// (0x20); a fillbyte that is not exactly one byte is a catchable runtime
    /// TypeError (CPython). center's left margin is `marg/2 + (marg & width & 1)`.
    fn emit_bytes_pad(&self, obj_s: &str, parts: &[String], meth: &str, left_pad: bool, right_pad: bool) -> String {
        let fill = if parts.len() >= 2 {
            format!("&({})", parts[1])
        } else {
            "b\" \"".to_string()
        };
        format!(
            "__py_bytes_pad(&({}), {}, {}, \"{}\", {}, {})",
            obj_s, parts[0], fill, meth, left_pad, right_pad
        )
    }

    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_super_method_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
    ) -> Result<Option<String>> {
                // Handle super().method(args)
                if let Expr::Attr { obj: super_call_expr, name: method_name, .. } = callee.as_ref() {
                    if let Expr::Call { callee: super_ident, args: super_args, .. } = super_call_expr.as_ref() {
                        if let Expr::Ident(n, _) = super_ident.as_ref() {
                            if n == "super" && super_args.is_empty() {
                                if let Some(_class_name) = self.current_class.clone() {
                                    // Call __super_ alias method which has parent's body
                                    let mut arg_parts = Vec::new();
                                    for a in args { arg_parts.push(self.emit_consuming(a)?); }
                                    return Ok(Some(format!("self.__super_{}({})", method_name, arg_parts.join(", "))));
                                }
                            }
                        }
                    }
                }
        Ok(None)
    }

    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_constructor_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<Option<String>> {
                // Check if this is a class constructor call.
                if let Expr::Ident(name, _) = callee.as_ref() {
                    // (W3-fix / F1) Owner-first: if the CURRENT emit scope resolves
                    // `name` to its OWN top-level FUNCTION, a same-named FOREIGN class
                    // must not hijack the bare call as a constructor — datetime's
                    // class `time` must not capture time.pyrs's internal `time()`.
                    // Non-root: the emit module owns a `fn name`. Root: the root
                    // defines `name` while an IMPORT owns the class of that name
                    // (root-vs-import class collision is an honest stopgap error, so a
                    // root def coexisting with an imported class `name` is a fn/const,
                    // never a class → it shadows). Byte-identical for single-owner
                    // programs (no imports → `class_owner` empty → never fires).
                    let own_func_shadows = match self.current_module.as_deref() {
                        Some(cur) => self
                            .ctx
                            .module_symbols
                            .get(cur)
                            .is_some_and(|ms| ms.funcs.contains_key(name.as_str())),
                        None => self.root_defined.contains(name.as_str())
                            && self.ctx.class_owner.contains_key(name.as_str()),
                    };
                    if let Some(class_def) = if own_func_shadows {
                        None
                    } else {
                        self.ctx.classes.get(name.as_str()).cloned()
                    } {
                        // (W3-2) The owner-qualified Rust struct name emitted for the
                        // `::new(..)` / struct-literal construction. `name` stays bare
                        // for every ctx LOOKUP (classes.get, the `<name>.__init__`
                        // funcs key) — only the EMITTED constructor identifier mangles.
                        let cname = self.emit_class_name(name);
                        let has_init = class_def.methods.iter().any(|m| m.name == "__init__");

                        // Use ::new() constructor whenever __init__ is defined —
                        // including the zero-arg case so that __init__ side-effects
                        // (field assignments, etc.) always run.
                        if has_init {
                            // (EPIC-5 C2-3) The `new()` signature lowers each
                            // `__init__` param via `rust_ty`, so a base-typed param
                            // is `B__`. A raw-struct / subclass argument into such a
                            // slot must be WRAPPED in the right variant (the same
                            // wrap-or-passthrough used at return / assign / free-fn
                            // sites) — otherwise the bare `Dog::new(..)` mismatches
                            // the `Animal__` param (E0308). Non-polymorphic params
                            // keep the clone-on-use emission.
                            let mut init_params: Vec<(String, Ty)> = class_def.methods.iter()
                                .find(|m| m.name == "__init__")
                                .map(|m| m.params.iter()
                                    .filter(|p| p.name != "self")
                                    .filter_map(|p| Ty::from_type_expr(&p.ty, p.span).ok().map(|t| (p.name.clone(), t)))
                                    .collect())
                                .unwrap_or_default();
                            // Generics v2 (generic class with a `Callable[.., V]`
                            // CONSTRUCTOR PARAM): the param type lowers with the bare
                            // class type-param name (`Rc<dyn Fn() -> V>`), but `V` is
                            // NOT in scope at this CALL site — a lambda arg would be
                            // cast `as Rc<dyn Fn() -> V>` and rustc raises E0425. So
                            // substitute the class's type params with the concrete
                            // instance type args inferred for THIS constructor call
                            // (`DD(lambda: 0)` infers `V = int` from the factory's
                            // return), yielding a concrete cast (`Rc<dyn Fn() -> i64>`).
                            // A non-generic class has empty `type_params`, so the map
                            // is empty and every param type is unchanged.
                            if !class_def.type_params.is_empty() {
                                if let Ty::Class(_, inst_args) =
                                    self.type_of_expr(&Expr::Call {
                                        callee: callee.clone(),
                                        args: args.to_vec(),
                                        kwargs: kwargs.to_vec(),
                                        span: callee.span(),
                                    })
                                {
                                    let subst: std::collections::HashMap<String, Ty> = class_def
                                        .type_params
                                        .iter()
                                        .cloned()
                                        .zip(inst_args.into_iter())
                                        .filter(|(_, t)| !matches!(t, Ty::Unknown))
                                        .collect();
                                    if !subst.is_empty() {
                                        for (_, t) in init_params.iter_mut() {
                                            *t = crate::typeck::substitute_class_typarams(t, &subst);
                                        }
                                    }
                                }
                            }
                            // (kwargs v1 + card f21369d7) Bind every argument
                            // to its `__init__` PARAM SLOT via the shared
                            // keyword→positional mapping: positional
                            // left-to-right, keywords by name, declared
                            // defaults into the holes. The OLD code appended
                            // keyword values in SOURCE order — an
                            // out-of-order keyword ctor call (`C(b=2, a=1)`)
                            // silently passed `(2, 1)` positionally, a
                            // miscompile whenever the swapped params shared a
                            // type. Out-of-slot-order keyword values are
                            // hoisted into source-ordered temps so side
                            // effects still run left-to-right (CPython call
                            // order), mirroring emit_plain_func_call.
                            let init_key = format!("{}.__init__", name);
                            if let Some(mut sig) = self.ctx.funcs.get(&init_key).cloned() {
                                // (card 63870682) A keyword-bearing OR UNDER-APPLIED
                                // (trailing-default-taking) constructor call routes through
                                // the shared slot mapper so declared defaults fill the
                                // `::new` params — matching the free-fn and method call
                                // sites. The default-fill was previously gated behind
                                // kwargs-nonempty, so a purely-positional under-application
                                // (`Point(5)`, `Fraction(0)`, zero-arg `ConfigParser()`)
                                // emitted `::new` with too few args and leaked rustc E0061.
                                // A FULLY-applied positional call (args.len() ==
                                // sig.params.len(), no kwargs) still takes the
                                // byte-identical plain positional loop below.
                                if !kwargs.is_empty() || args.len() < sig.params.len() {
                                    let slots = crate::typeck::map_kwargs_to_slots(
                                        &init_key, &sig, args.len(), kwargs, callee.span(),
                                    )?;
                                    // (item A) Shared slot mapper: source-order
                                    // hoist (positionals AND keyword values) +
                                    // declared-default fill, mirroring the free-fn
                                    // and method call sites. `coerced = false`
                                    // keeps the constructor's `emit_arg_into_slot`
                                    // emission (poly-base wrap / `Callable` cast /
                                    // clone-on-use) against the `__init__` param
                                    // (already substituted for a generic class).
                                    // The constructor path emits every param by
                                    // VALUE (like the positional path below), so
                                    // clear `param_by_ref` — a `Mut[T]` ctor param
                                    // is not a modeled `::new` shape today.
                                    sig.param_by_ref = Vec::new();
                                    let param_tys: Vec<Ty> =
                                        init_params.iter().map(|(_, t)| t.clone()).collect();
                                    // (W3-fix / F5) `__init__` defaults belong to the
                                    // class's module — fill them under that owner.
                                    let def_owner = self.ctx.class_owner.get(name.as_str()).cloned();
                                    let (prelude, call_parts) = self.emit_slotted_args(
                                        &slots, args, kwargs, &sig, &param_tys, /*coerced=*/ false,
                                        def_owner.as_deref(),
                                    )?;
                                    return Ok(Some(Self::hoist_wrap(
                                        &prelude,
                                        format!("{}::new({})", cname, call_parts.join(", ")),
                                    )));
                                }
                            }
                            let mut call_parts = Vec::new();
                            for (i, a) in args.iter().enumerate() {
                                call_parts.push(self.emit_arg_into_slot(a, init_params.get(i).map(|(_, t)| t))?);
                            }
                            return Ok(Some(format!("{}::new({})", cname, call_parts.join(", "))));
                        }

                        // Class constructor: emit a Rust struct literal.
                        // Use inherited + own fields for positional.
                        // (enabler-fix-1 #3a) EXCLUDE promoted class constants — they
                        // are associated `const`s, not struct fields (items.rs excludes
                        // them from the struct too), so counting one as a required ctor
                        // arg gave the wrong arity / a bogus struct-field init.
                        // (card 63870682) Build the field set from the FULL transitive
                        // ancestry (`get_all_fields`: ancestors-first, then own, deduped
                        // by name, class-constants excluded) — the SAME walk emit_class
                        // uses for the struct layout and the synthesized `new()`. The old
                        // inline walk descended only ONE level into `class_def.bases`
                        // (each direct base's OWN `fields`), so a 3-level dataclass chain
                        // (Base→Mid→Leaf) dropped the grandparent's fields: a positional
                        // `Leaf(a,b,c,d)` then mis-bound against `[c, d]` (wrong-arity /
                        // E0062), while the keyword form — which emits each field by name —
                        // still worked. Sourcing from get_all_fields keeps the call-site
                        // field order byte-identical to the struct definition.
                        let mut all_field_names: Vec<String> = Vec::new();
                        for f in self.ctx.get_all_fields(name.as_str()) {
                            if self.is_class_const_field(name, &f.name) {
                                continue;
                            }
                            if !all_field_names.contains(&f.name) {
                                all_field_names.push(f.name.clone());
                            }
                        }

                        if !args.is_empty() && kwargs.is_empty() {
                            // Positional args to a class constructor.
                            if args.len() > all_field_names.len() {
                                return Err(crate::diag::Error::Codegen(format!(
                                    "class `{}` has {} fields but {} positional arguments given",
                                    name, all_field_names.len(), args.len()
                                )));
                            }
                            // (card 6f69d4a3) Fewer positionals than fields is valid
                            // when the TRAILING fields have DEFAULTS (a @dataclass
                            // `port: int = 8080`): fill each omitted field with its
                            // default value. typeck already rejected an omitted field
                            // that has NO default (map_kwargs_to_slots), so a missing
                            // default here is a defensive codegen error only.
                            // (dedupe, phase2-fix2) Each field emits via the shared
                            // struct-literal helpers: a provided positional through
                            // `emit_struct_field_value` (class_field_type coercion —
                            // incl. the EPIC-5 C2-3 polymorphic-base variant wrap — +
                            // enabler-fix-1 #4a self-referential boxing), an omitted
                            // TRAILING field through `emit_omitted_field_default`
                            // (default-fill, or the "missing required argument" error).
                            let all_params = self.ctx.get_all_fields(name.as_str());
                            let mut parts = Vec::new();
                            for (i, field_name) in all_field_names.iter().enumerate() {
                                let v = if i < args.len() {
                                    self.emit_struct_field_value(name, &class_def, field_name, &args[i])?
                                } else {
                                    self.emit_omitted_field_default(name, &class_def, &all_params, field_name)?
                                };
                                // (EPIC-6) Escape a keyword field name in the
                                // positional struct-literal init.
                                parts.push(format!("{}: {}", escape_ident(field_name), v));
                            }
                            return Ok(Some(format!("{} {{ {} }}", cname, parts.join(", "))));
                        }

                        // Keyword-args form (possibly MIXED with positional args).
                        if !kwargs.is_empty() {
                            let mut parts = Vec::new();
                            // (W1.5 fix B) Positional args bind to the LEADING
                            // fields (declaration order) — they used to be SILENTLY
                            // DROPPED whenever a call mixed positionals with
                            // keywords (`Point(1, y=2)` emitted `Point { y: 2 }`,
                            // losing the `1`). Emitting the positionals first, then
                            // the keywords in written order, also preserves CPython
                            // left-to-right call-site evaluation: a Rust struct
                            // literal evaluates its field initializers in the order
                            // written, which is exactly positionals-then-keywords
                            // source order. typeck (map_kwargs_to_slots against the
                            // synthesized field signature) has already rejected any
                            // duplicate / unknown / missing / too-many argument.
                            // (dedupe, phase2-fix2) Leading positionals then keywords,
                            // each through the shared `emit_struct_field_value`
                            // (class_field_type coercion + enabler-fix-1 #4a boxing).
                            for (field_name, arg) in all_field_names.iter().zip(args.iter()) {
                                let v = self.emit_struct_field_value(name, &class_def, field_name, arg)?;
                                parts.push(format!("{}: {}", escape_ident(field_name), v));
                            }
                            for (kw, val) in kwargs {
                                let v = self.emit_struct_field_value(name, &class_def, kw, val)?;
                                // (EPIC-6) Escape a keyword field name in the
                                // keyword-arg struct-literal init.
                                parts.push(format!("{}: {}", escape_ident(kw), v));
                            }
                            // (card 63870682) Fill any field left UNCOVERED by both
                            // the positionals (which bind the leading `args.len()`
                            // fields) and the keywords with its declared default — the
                            // same middle/trailing default-fill the positional-only and
                            // `__init__` (`::new`) paths perform, making the dataclass
                            // struct-literal keyword form uniform with them. typeck
                            // (map_kwargs_to_slots over the synthesized field signature)
                            // already rejected an omitted field with NO default, so a
                            // missing default here is a defensive codegen error only. A
                            // Rust struct literal is order-insensitive, so appending the
                            // filled defaults after the provided fields is well-formed.
                            // Without this, a mixed/keyword call that omitted a defaulted
                            // field (`Config("k", debug=True)` leaving `port` to default)
                            // emitted an incomplete struct literal (rustc E0063).
                            let provided_by_kw: std::collections::HashSet<&str> =
                                kwargs.iter().map(|(k, _)| k.as_str()).collect();
                            let all_params = self.ctx.get_all_fields(name.as_str());
                            for (i, field_name) in all_field_names.iter().enumerate() {
                                if i < args.len() || provided_by_kw.contains(field_name.as_str()) {
                                    continue;
                                }
                                // (dedupe, phase2-fix2) Fill an uncovered field with
                                // its declared default via the shared helper (same
                                // default-fill + missing-argument error as the
                                // positional-only path).
                                let v = self.emit_omitted_field_default(name, &class_def, &all_params, field_name)?;
                                parts.push(format!("{}: {}", escape_ident(field_name), v));
                            }
                            return Ok(Some(format!("{} {{ {} }}", cname, parts.join(", "))));
                        }

                        // No args at all: emit default struct literal.
                        let mut parts = Vec::new();
                        for fname in &all_field_names {
                            let field = class_def.fields.iter().find(|f| &f.name == fname)
                                .or_else(|| {
                                    class_def.bases.iter().find_map(|b| {
                                        self.ctx.classes.get(b.as_str())
                                            .and_then(|bd| bd.fields.iter().find(|f| &f.name == fname))
                                    })
                                });
                            let default = if let Some(f) = field {
                                // (enabler-fix-1 #3) Honor a field's DECLARED default
                                // in a zero-arg construction (`Options()` with `level:
                                // int = 1`). Before the usage-gated promotion, such a
                                // literal-defaulted field was always a const, so this
                                // path only ever saw undefaulted fields and zeroed them
                                // — an unpromoted defaulted instance field would have
                                // been silently zeroed (`level` -> 0 instead of 1).
                                match &f.default {
                                    // (dedupe, phase2-fix2) Honor a declared default via
                                    // the shared struct-literal emit core.
                                    Some(d) => self.emit_struct_field_value(name, &class_def, fname, d)?,
                                    None => {
                                        let ty = Ty::from_type_expr(&f.ty, f.span)?;
                                        self.zeroed_default(&ty)
                                    }
                                }
                            } else {
                                "Default::default()".to_string()
                            };
                            // (EPIC-6) Escape a keyword field name in the no-arg
                            // default struct-literal init.
                            parts.push(format!("{}: {}", escape_ident(fname), default));
                        }
                        return Ok(Some(format!("{} {{ {} }}", cname, parts.join(", "))));
                    }
                }
        Ok(None)
    }

    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_builtin_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<Option<String>> {
                // Multi-arg print with inline format
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "print" {
                        if args.is_empty() {
                            return Ok(Some("println!(\"\")".to_string()));
                        }
                        let mut parts: Vec<String> = Vec::new();
                        for arg in args {
                            let raw = self.emit_expr(arg)?;
                            let formatted = match self.type_of_expr(arg) {
                                Ty::Float => format!("__py_fmt_float({})", raw),
                                Ty::Bool => format!("__py_fmt_bool({})", raw),
                                Ty::Bytes | Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) =>
                                    format!("({}).py_repr()", raw),
                                // (enabler-fix-2 #3) print(opt) -> payload-or-None.
                                Ty::Option(inner) => self.emit_str_option(&raw, inner.as_ref(), 0),
                                _ => raw,
                            };
                            parts.push(formatted);
                        }
                        // Use {} (Display format) for most types; {:?} breaks strings by adding quotes
                        let fmt = (0..parts.len()).map(|_| "{}").collect::<Vec<_>>().join(" ");
                        return Ok(Some(format!("println!(\"{}\" {})", fmt,
                            if parts.is_empty() { "".to_string() } else { format!(", {}", parts.join(", ")) })));
                    }
                }

                // Inline range() with 1, 2, or 3 args
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "range" {
                        if args.len() == 1 {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("(0..{})", a)));
                        } else if args.len() == 2 {
                            let a = self.emit_expr(&args[0])?;
                            let b = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("({}..{})", a, b)));
                        } else if args.len() == 3 {
                            let a = self.emit_expr(&args[0])?;
                            let b = self.emit_expr(&args[1])?;
                            let step = self.emit_expr(&args[2])?;
                            // (card 87bd8eb4) Direction-aware: a negative step
                            // descends (CPython), a zero step is a catchable
                            // ValueError. `.step_by(neg as usize)` silently yielded
                            // nothing. Materializes a `Vec<i64>` (range types as
                            // list[int] already), consumed uniformly by for-loops /
                            // list() / comprehensions (they see a list, not a `..`).
                            return Ok(Some(format!("__py_range_step({}, {}, {})", a, b, step)));
                        }
                    }
                }

                // Inline enumerate(iter[, start]) — emits iterator chain without collecting
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "enumerate" && (args.len() == 1 || args.len() == 2) {
                        let a = self.emit_expr(&args[0])?;
                        let is_range = a.contains("..");
                        let iter_chain = if is_range {
                            format!("({}).into_iter()", a)
                        } else if matches!(self.type_of_expr(&args[0]), Ty::Str) {
                            // (CARD 0c4bb6be) Iterating a str yields
                            // 1-character strings (Python semantics) — mirrors
                            // the Str arm already used by the comprehension
                            // chain (ListComp/SetComp/DictComp) and the
                            // `Stmt::For` lowering. Previously missing here,
                            // so `enumerate("hi")` typechecked (the oracle
                            // already knew the element type) but failed at
                            // BUILD: `String` has no `.iter()`, so the
                            // fallback `.iter().cloned()` below was a raw
                            // rustc E0599.
                            format!("{}.chars().map(|__c| __c.to_string())", a)
                        } else if matches!(self.type_of_expr(&args[0]), Ty::Iterator(_)) {
                            // (LAZY-GEN V1-c) A generator source (`Gen<T>`) is
                            // itself an `Iterator` yielding owned `T` — consume
                            // it directly (no `.iter()`, `Gen` has none).
                            Self::iter_arg_source(&args[0], &a)
                        } else {
                            format!("{}.iter().cloned()", a)
                        };
                        // (card 00fb0e6d) CPython `enumerate(it, start)` seeds the
                        // counter at `start` (default 0).
                        // (card c34ac64a fix E) `start` must be evaluated exactly
                        // ONCE (CPython), not once per element. Splicing it into the
                        // per-element `.map()` closure re-ran a side-effecting /
                        // non-idempotent `start` on every step. HOIST it to a
                        // pre-call temp via a block expression and `move`-capture it
                        // (an int is Copy). The 1-arg form is byte-identical to before.
                        if args.len() == 2 {
                            let s = self.emit_expr(&args[1])?;
                            return Ok(Some(format!(
                                "{{ let __enum_start: i64 = ({}); {}.enumerate().map(move |(i, v)| (i as i64 + __enum_start, v)) }}",
                                s, iter_chain
                            )));
                        }
                        return Ok(Some(format!("{}.enumerate().map(|(i, v)| (i as i64, v))", iter_chain)));
                    }
                }

                // Inline zip(a, b, ...) — emits iterator chain without collecting.
                // (CARD 0c4bb6be) CPython's `zip` is variadic and yields FLAT
                // N-tuples; Rust's `Iterator::zip` is binary and NESTS when
                // chained (`a.zip(b).zip(c)` -> `((a,b),c)`, not `(a,b,c)`).
                // The 2-arg case needs no flattening (Rust's pair already
                // matches Python's 2-tuple) and stays the exact byte-identical
                // fast path it always was. For 3-4 args, fold a nested
                // `.zip()` chain then `.map()` it into a flat tuple — the
                // typeck arm (typeck/exprs.rs `check_expr`'s "zip" case)
                // rejects 5+ args honestly at check/build time, so codegen
                // never needs to handle more than 4 here.
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if n == "zip" && args.len() >= 2 && args.len() <= 4 {
                        let mut iters: Vec<String> = Vec::with_capacity(args.len());
                        for arg in args {
                            let a = self.emit_expr(arg)?;
                            let is_range = a.contains("..");
                            // (LAZY-GEN V1-c) Each side is classified on its
                            // OWN type — a mixed generator+list `zip` is valid
                            // Python.
                            let iter_s = if is_range {
                                format!("({}).into_iter()", a)
                            } else if matches!(self.type_of_expr(arg), Ty::Str) {
                                // (CARD 0c4bb6be review, comment 180) A str arg
                                // yields 1-character strings (Python semantics) —
                                // mirror the Str arm the SAME wave added to the
                                // `enumerate` lowering a few lines above. Without it
                                // the fallback `.iter().cloned()` emitted `.iter()`
                                // on a `String` (no such method) — a raw rustc E0599
                                // that passed `check`. Applies to every arg position
                                // independently (a mixed generator+list+str zip is
                                // valid Python), and to both the 2-arg fast path and
                                // the 3/4-arg fold below.
                                format!("{}.chars().map(|__c| __c.to_string())", a)
                            } else if matches!(self.type_of_expr(arg), Ty::Iterator(_)) {
                                Self::iter_arg_source(arg, &a)
                            } else {
                                format!("{}.iter().cloned()", a)
                            };
                            iters.push(iter_s);
                        }
                        if args.len() == 2 {
                            return Ok(Some(format!("{}.zip({})", iters[0], iters[1])));
                        }
                        // 3/4 args: fold the nested `.zip()` chain, then
                        // flatten via `.map()`. E.g. for 3 args:
                        // `a.zip(b).zip(c).map(|((x,y),z)| (x,y,z))`.
                        let mut chain = iters[0].clone();
                        for it in &iters[1..] {
                            chain = format!("{}.zip({})", chain, it);
                        }
                        let vars: Vec<String> = (0..iters.len()).map(|i| format!("__z{}", i)).collect();
                        let mut pat = vars[0].clone();
                        for v in &vars[1..] {
                            pat = format!("({}, {})", pat, v);
                        }
                        let flat = format!("({})", vars.join(", "));
                        return Ok(Some(format!("{}.map(|{}| {})", chain, pat, flat)));
                    }
                }

                // Builtin function dispatch
                if let Expr::Ident(n, _) = callee.as_ref() {
                    match n.as_str() {
                        "len" => {
                            let arg_ty = self.type_of_expr(&args[0]);
                            // (enabler-fix-2 #2) len() of a statically-shaped tuple is
                            // its CONST arity — a Rust tuple has no `.len()` (the old
                            // `.len()` emission was a rustc E0599). Still EVALUATE the
                            // argument (Python evaluates it before taking len), so any
                            // side effect is preserved; then yield the arity. Only a
                            // KNOWN, NON-EMPTY arity qualifies — an empty `Ty::Tuple`
                            // is the unknown-shape placeholder (e.g. `tuple(xs)`), left
                            // to the fall-through rather than mis-reported as 0.
                            if let Ty::Tuple(tys) = &arg_ty {
                                if !tys.is_empty() {
                                    let a = self.emit_expr(&args[0])?;
                                    return Ok(Some(format!("{{ let _ = &({}); {}i64 }}", a, tys.len())));
                                }
                            }
                            let a = self.emit_expr(&args[0])?;
                            // Python len() of a str is the CHARACTER count, not the
                            // UTF-8 byte count. Collections keep .len().
                            // (W1.5) PARENTHESIZED: a bare `x.len() as i64 < n`
                            // (len leftmost in a `<`/`<=` comparison) makes rustc
                            // parse `i64<` as generic arguments (E0658-style
                            // parse error); the parens keep every context valid.
                            if matches!(arg_ty, Ty::Str) {
                                return Ok(Some(format!("({}.chars().count() as i64)", a)));
                            }
                            return Ok(Some(format!("({}.len() as i64)", a)));
                        }
                        "str" => {
                            // (card 00fb0e6d) CPython `str()` with no argument is the
                            // EMPTY string (not an error). Handle before touching
                            // `args[0]` (an empty-vec index was the exit-101 ICE).
                            if args.is_empty() {
                                return Ok(Some("String::new()".into()));
                            }
                            let a = self.emit_expr(&args[0])?;
                            match self.type_of_expr(&args[0]) {
                                // Match print/f-string formatting: a whole float is
                                // "7.0" (Rust's `{}` would drop it to "7"), a bool is
                                // "True"/"False" (not Rust's "true"/"false").
                                Ty::Float => return Ok(Some(format!("__py_fmt_float({})", a))),
                                Ty::Bool => return Ok(Some(format!("__py_fmt_bool({})", a))),
                                Ty::Bytes | Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) =>
                                    return Ok(Some(format!("({}).py_repr()", a))),
                                // (enabler-fix-2 #3) str(opt) -> payload-or-None.
                                Ty::Option(inner) => return Ok(Some(self.emit_str_option(&a, inner.as_ref(), 0))),
                                _ => return Ok(Some(format!("format!(\"{{}}\" , {})", a))),
                            }
                        }
                        "bytes" => {
                            // (W5-a) The `bytes(...)` constructor, dispatched on the
                            // (check-validated) argument type:
                            //   bytes()          -> empty `Vec<u8>`
                            //   bytes(n: int)    -> n zero bytes (negative -> ValueError)
                            //   bytes(list[int]) -> each in range(0, 256) (else ValueError)
                            //   bytes(b: bytes)  -> a copy
                            if args.is_empty() {
                                return Ok(Some("Vec::<u8>::new()".to_string()));
                            }
                            match self.type_of_expr(&args[0]) {
                                Ty::Int => {
                                    let n = self.emit_expr(&args[0])?;
                                    return Ok(Some(format!(
                                        "{{ let __n: i64 = {}; if __n < 0 {{ panic!(\"ValueError\\0negative count\") }}; vec![0u8; __n as usize] }}",
                                        n
                                    )));
                                }
                                // A copy of the source bytes (value-semantics clone).
                                Ty::Bytes => return Ok(Some(self.emit_consuming(&args[0])?)),
                                // list[int]: build a byte vector, range-checking each
                                // element to 0..=255 (CPython ValueError otherwise).
                                _ => {
                                    // Value-semantic: `emit_consuming` clones a place
                                    // (leaving the caller's list usable) and passes an
                                    // rvalue bare. Annotate `__src: Vec<i64>` (list[int]
                                    // is always Vec<i64>) so an EMPTY-list arg
                                    // (`bytes([])` -> an untyped `vec![]`) still infers
                                    // instead of leaking a rustc E0282.
                                    let src = self.emit_consuming(&args[0])?;
                                    return Ok(Some(format!(
                                        "{{ let __src: Vec<i64> = {}; __src.iter().map(|&__x| {{ if __x < 0 || __x > 255 {{ panic!(\"ValueError\\0bytes must be in range(0, 256)\") }} __x as u8 }}).collect::<Vec<u8>>() }}",
                                        src
                                    )));
                                }
                            }
                        }
                        "open" => {
                            let path = self.emit_expr(&args[0])?;
                            let mode = if args.len() >= 2 {
                                self.emit_expr(&args[1])?
                            } else {
                                "\"r\".to_string()".to_string()
                            };
                            return Ok(Some(format!("__py_open(&{}, &{})", path, mode)));
                        }
                        "int" => {
                            // (card 00fb0e6d) CPython `int()` with no argument is 0.
                            if args.is_empty() {
                                return Ok(Some("0i64".into()));
                            }
                            let a = self.emit_expr(&args[0])?;
                            let arg_type = self.type_of_expr(&args[0]);
                            match arg_type {
                                Ty::Str => {
                                    // Use helper so a bad string panics with "ValueError\0..."
                                    // which the try/except dispatcher can match on ValueError.
                                    return Ok(Some(format!("(__py_int_from_str(&{}))", a)));
                                }
                                _ => return Ok(Some(format!("({} as i64)", a))),
                            }
                        }
                        "float" => {
                            // (card 00fb0e6d) CPython `float()` with no argument is 0.0.
                            if args.is_empty() {
                                return Ok(Some("0.0f64".into()));
                            }
                            let a = self.emit_expr(&args[0])?;
                            let arg_type = self.type_of_expr(&args[0]);
                            match arg_type {
                                Ty::Str => {
                                    // Use helper so a bad string panics with "ValueError\0..."
                                    // which the try/except dispatcher can match on ValueError.
                                    return Ok(Some(format!("(__py_float_from_str(&{}))", a)));
                                }
                                _ => return Ok(Some(format!("({} as f64)", a))),
                            }
                        }
                        "bool" => {
                            // (card 00fb0e6d) CPython `bool()` with no argument is False.
                            if args.is_empty() {
                                return Ok(Some("false".into()));
                            }
                            // (card 18682938) `bool(obj)` for a class-with-`__bool__`
                            // lowers to `obj.__bool__()`; numeric/other args keep the
                            // `!= 0` truthiness lowering.
                            if let Ty::Class(cls, _) = self.type_of_expr(&args[0]) {
                                if self.ctx.get_method(&cls, "__bool__").is_some() {
                                    return Ok(Some(self.emit_truthy(&args[0])?));
                                }
                            }
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("(({}) != 0)", a)));
                        }
                        "abs" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("({}).abs()", a)));
                        }
                        "min" => {
                            if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                                // min with key parameter
                                // (CARD bd2bd472) `emit_expr` on a variable source
                                // emits it BARE, so `let __list = {a};` below MOVED
                                // it — reusing the source after `min(xs, key=..)`
                                // hit a genuine E0382, unlike `sorted`'s key= arm
                                // (which explicitly `.clone()`s its list source).
                                // Route through the shared ownership-decision point
                                // (`emit_consuming`: `.clone()` a reusable place,
                                // bare for an owned temp) so the source stays
                                // usable afterward, matching Python value semantics.
                                let a = self.emit_consuming(&args[0])?;
                                // Check if key_expr is a Lambda to handle it specially
                                let key_code = if let Expr::Lambda { params, body, .. } = key_expr {
                                    // Lambda: extract parameter name and body, rename param to __x
                                    let param_name = params.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "__x".to_string());
                                    // (CARD 55343eaa) See the `sorted` key= arm
                                    // below for the full explanation: bind the
                                    // param to the SOURCE's element type (not
                                    // `Ty::Unknown`) so a tuple-indexing body
                                    // (`t[0]`) lowers through tuple field access
                                    // instead of list indexing.
                                    let src_ty = self.type_of_expr(&args[0]);
                                    let key_param_ty = match &src_ty {
                                        Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => (**inner).clone(),
                                        Ty::Dict(k, _) => (**k).clone(),
                                        _ => Ty::Unknown,
                                    };
                                    let saved_local = self.locals.get(&param_name).cloned();
                                    self.locals.insert(param_name.clone(), key_param_ty);
                                    let body_s = self.emit_expr(body)?;
                                    if let Some(ty) = saved_local {
                                        self.locals.insert(param_name.clone(), ty);
                                    } else {
                                        self.locals.remove(param_name.as_str());
                                    }
                                    // Replace param_name with __x in the body (word-boundary aware).
                                    // (EPIC-6) The body emitted the param through
                                    // emit_expr's Ident arm, which ESCAPES a keyword
                                    // param to `r#<name>`; search for that escaped
                                    // form so a keyword sort-key param is renamed
                                    // correctly (replace_identifier treats `r#kw` as
                                    // one token).
                                    Self::replace_identifier(&body_s, escape_ident(&param_name).as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };
                                // (REVIEW FOLLOW-UP on 577b04f, items 1+3) The
                                // former `__list.iter().min_by_key(|__x| ..)` had
                                // two bugs, both fixed by this manual fold:
                                //  (1) `__x` inside `min_by_key`'s closure is
                                //  `&Self::Item` = `&&Elem` (double reference,
                                //  since `.iter()`'s Item is already `&Elem`) —
                                //  method calls auto-deref through this fine, but
                                //  a plain HELPER FUNCTION call in the key body
                                //  (`helper(__x)`) does not get that coercion and
                                //  fails E0308 (unlike `sorted`'s key= arm, whose
                                //  closure binds `__x` to an OWNED clone). Binding
                                //  `__x` via `let __x = __ref.clone();` here
                                //  matches `sorted`'s shape exactly, fixing it.
                                //  (2) Tie-breaking: `min_by_key` already returns
                                //  the FIRST minimal element on a tie, matching
                                //  Python's `min()` — an explicit `<` (not `<=`)
                                //  fold preserves that (keep the earlier winner
                                //  unless a STRICTLY smaller key is found).
                                // A side benefit: `<`/`>` need only `PartialOrd`
                                // (not the `Ord` `min_by_key`/`max_by_key`
                                // require), so a FLOAT-valued key now compiles too.
                                // (REVIEW FOLLOW-UP on 88e91b4) `min([])`/an empty
                                // key= source must be a CATCHABLE `ValueError`
                                // ("min() iterable argument is empty", matching
                                // CPython verbatim), not a silent type default —
                                // `unwrap_or_default()` returned `""`/`0` for an
                                // empty source instead. `unwrap_or_else(|| panic!(
                                // "ValueError\0..."))` matches the NUL-delimited
                                // "Type\0msg" convention the try/except dispatcher
                                // and every other builtin runtime error already use
                                // (see `__py_int_from_str` etc.).
                                return Ok(Some(format!(
                                    "{{ let __list = {}; let mut __best: Option<(_, _)> = None; for __ref in __list.iter() {{ let __x = __ref.clone(); let __k = {}; let __take = match &__best {{ None => true, Some((__bk, _)) => __k < *__bk }}; if __take {{ __best = Some((__k, __x)); }} }} __best.map(|(_, __v)| __v).unwrap_or_else(|| panic!(\"ValueError\\0min() iterable argument is empty\")) }}",
                                    a, key_code
                                )));
                            } else if args.len() == 1 {
                                let a = self.emit_expr(&args[0])?;
                                let arg_ty = self.type_of_expr(&args[0]);
                                let elem_ty = match &arg_ty {
                                    Ty::List(inner) | Ty::Iterator(inner) => (**inner).clone(),
                                    _ => Ty::Int,
                                };
                                // (LAZY-GEN V1-c) A generator source (`Gen<T>`)
                                // yields OWNED elements directly — no `.iter()`
                                // (`Gen` has none) and no `&`/deref on `__x`.
                                // (REVIEW FOLLOW-UP on 88e91b4) Both the Float loop
                                // and the non-Float `.min()` must raise the same
                                // catchable ValueError on an empty source instead
                                // of silently returning `f64::INFINITY`/`0` — see
                                // the note on the key= fold above.
                                if matches!(arg_ty, Ty::Iterator(_)) {
                                    let src = Self::iter_arg_source(&args[0], &a);
                                    return Ok(Some(match elem_ty {
                                        Ty::Float => format!("{{ let mut __min = f64::INFINITY; let mut __seen = false; for __x in {} {{ __seen = true; if __x < __min {{ __min = __x; }} }} if !__seen {{ panic!(\"ValueError\\0min() iterable argument is empty\"); }} __min }}", src),
                                        _ => format!("{}.min().unwrap_or_else(|| panic!(\"ValueError\\0min() iterable argument is empty\"))", src),
                                    }));
                                }
                                // (REVIEW FOLLOW-UP on 577b04f, item 2) `.copied()`
                                // requires `Copy`, which a non-Copy element type
                                // (e.g. `str`) does not implement — `.cloned()`
                                // requires only `Clone` and covers both Copy and
                                // non-Copy element types uniformly.
                                // (W2 card ebd703d9) A user class with only `__lt__`
                                // is `PartialOrd`, not `Ord`, so `.min()` (which needs
                                // `Ord`) is an E0277 build wall — the same honesty
                                // hole `sorted()` already closed. Route it through a
                                // FIRST-WINS `<` fold (replace only on a STRICTLY
                                // smaller element) so ties keep the earliest element,
                                // exactly like CPython's `min()`. `elem_needs_partial_cmp`
                                // is the same predicate `sorted()` uses.
                                let is_cmp_class = matches!(&elem_ty, Ty::Class(..))
                                    && self.elem_needs_partial_cmp(&elem_ty);
                                return Ok(Some(if matches!(elem_ty, Ty::Float) {
                                    format!("{{ let mut __min = f64::INFINITY; let mut __seen = false; for __x in {}.iter() {{ __seen = true; if __x < &__min {{ __min = *__x; }} }} if !__seen {{ panic!(\"ValueError\\0min() iterable argument is empty\"); }} __min }}", a)
                                } else if is_cmp_class {
                                    // (enabler-fix-2 #8a) Compare by REFERENCE and
                                    // clone ONLY the winner (was: clone EVERY element
                                    // into `__x` each iteration). `&Elem: PartialOrd`
                                    // delegates to `Elem`, so `__ref < __b` compares
                                    // in place; `__best` holds a borrow into `__src`
                                    // (bound first, temporary-lifetime-extended so it
                                    // outlives the fold) and is `.cloned()` once at the
                                    // end. First-wins ties preserved -> byte-identical.
                                    format!("{{ let __src = &({}); let mut __best: Option<&_> = None; for __ref in __src.iter() {{ let __take = match __best {{ None => true, Some(__b) => __ref < __b }}; if __take {{ __best = Some(__ref); }} }} __best.cloned().unwrap_or_else(|| panic!(\"ValueError\\0min() iterable argument is empty\")) }}", a)
                                } else {
                                    format!("{}.iter().cloned().min().unwrap_or_else(|| panic!(\"ValueError\\0min() iterable argument is empty\"))", a)
                                }));
                            } else {
                                // (card b557b9c1) n-ary min: fold ALL positional
                                // args left-to-right, FIRST-WINS on ties (replace
                                // only on a STRICTLY smaller candidate), exactly
                                // like CPython's `min(a, b, c, ...)`. The prior arm
                                // consumed ONLY args[0]/args[1] and silently dropped
                                // args[2..] — a P0 silent miscompile (`min(3,1,2)`
                                // evaluated as `min(3,1)`). A uniform `<` fold needs
                                // only `PartialOrd` (not the `Ord` that
                                // `::std::cmp::min` required), so ONE shape covers
                                // int / str AND f64 / a `__lt__`-only user class
                                // (the `__lt__` typeck gate above already rejects a
                                // class without `__lt__`); it also subsumes the old
                                // 2-arg partial-cmp special case byte-for-byte. Each
                                // arg is bound once (`__c`) so a side-effecting or
                                // expensive arg evaluates exactly once, and is routed
                                // through `emit_consuming` so a reusable place is
                                // cloned, not moved (E0382). The RESULT type is the
                                // (homogeneous) first arg's type — see the min/max
                                // n-ary arm in `typeck::infer_expr_ty`, which restores
                                // correct float DISPLAY (`min(1.0,2.0,3.0)` -> 1.0).
                                let mut parts = Vec::with_capacity(args.len());
                                for arg in args {
                                    parts.push(self.emit_consuming(arg)?);
                                }
                                let mut fold = format!("let mut __best = {};", parts[0]);
                                for p in &parts[1..] {
                                    fold.push_str(&format!(
                                        " {{ let __c = {}; if __c < __best {{ __best = __c; }} }}",
                                        p
                                    ));
                                }
                                return Ok(Some(format!("{{ {} __best }}", fold)));
                            }
                        }
                        "max" => {
                            if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                                // max with key parameter
                                // (CARD bd2bd472) See the `min` key= arm above: route
                                // through `emit_consuming` so a reusable source
                                // variable is cloned (not moved) into `__list`,
                                // matching `sorted`'s clone convention and Python
                                // value semantics (reuse-after works).
                                let a = self.emit_consuming(&args[0])?;
                                // Check if key_expr is a Lambda to handle it specially
                                let key_code = if let Expr::Lambda { params, body, .. } = key_expr {
                                    // Lambda: extract parameter name and body, rename param to __x
                                    let param_name = params.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "__x".to_string());
                                    // (CARD 55343eaa) See the `sorted` key= arm
                                    // below for the full explanation: bind the
                                    // param to the SOURCE's element type (not
                                    // `Ty::Unknown`) so a tuple-indexing body
                                    // (`t[0]`) lowers through tuple field access
                                    // instead of list indexing.
                                    let src_ty = self.type_of_expr(&args[0]);
                                    let key_param_ty = match &src_ty {
                                        Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => (**inner).clone(),
                                        Ty::Dict(k, _) => (**k).clone(),
                                        _ => Ty::Unknown,
                                    };
                                    let saved_local = self.locals.get(&param_name).cloned();
                                    self.locals.insert(param_name.clone(), key_param_ty);
                                    let body_s = self.emit_expr(body)?;
                                    if let Some(ty) = saved_local {
                                        self.locals.insert(param_name.clone(), ty);
                                    } else {
                                        self.locals.remove(param_name.as_str());
                                    }
                                    // Replace param_name with __x in the body (word-boundary aware).
                                    // (EPIC-6) The body emitted the param through
                                    // emit_expr's Ident arm, which ESCAPES a keyword
                                    // param to `r#<name>`; search for that escaped
                                    // form so a keyword sort-key param is renamed
                                    // correctly (replace_identifier treats `r#kw` as
                                    // one token).
                                    Self::replace_identifier(&body_s, escape_ident(&param_name).as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };
                                // (REVIEW FOLLOW-UP on 577b04f, items 1+3) See the
                                // `min` key= arm above for the full explanation.
                                // Unlike `min_by_key` (already first-wins, same as
                                // Python), Rust's `max_by_key` returns the LAST
                                // maximal element on a tie — Python's `max()`
                                // returns the FIRST. This manual fold uses `>`
                                // (STRICTLY greater) so an equal-or-lesser
                                // candidate never replaces the earlier winner,
                                // reproducing Python's first-wins tie rule; it
                                // also binds `__x` to an OWNED clone (not the
                                // double-referenced closure item), fixing a
                                // helper-function-in-key-body E0308, and needs
                                // only `PartialOrd` (not `Ord`), so a float key
                                // now compiles too.
                                // (REVIEW FOLLOW-UP on 88e91b4) See the `min` key=
                                // arm above: an empty source must raise a catchable
                                // ValueError ("max() iterable argument is empty",
                                // matching CPython), not silently default.
                                return Ok(Some(format!(
                                    "{{ let __list = {}; let mut __best: Option<(_, _)> = None; for __ref in __list.iter() {{ let __x = __ref.clone(); let __k = {}; let __take = match &__best {{ None => true, Some((__bk, _)) => __k > *__bk }}; if __take {{ __best = Some((__k, __x)); }} }} __best.map(|(_, __v)| __v).unwrap_or_else(|| panic!(\"ValueError\\0max() iterable argument is empty\")) }}",
                                    a, key_code
                                )));
                            } else if args.len() == 1 {
                                let a = self.emit_expr(&args[0])?;
                                let arg_ty = self.type_of_expr(&args[0]);
                                let elem_ty = match &arg_ty {
                                    Ty::List(inner) | Ty::Iterator(inner) => (**inner).clone(),
                                    _ => Ty::Int,
                                };
                                // (LAZY-GEN V1-c) See the `min` arm above: a
                                // generator source yields OWNED elements — no
                                // `.iter()`/deref.
                                // (REVIEW FOLLOW-UP on 88e91b4) See the `min` arm
                                // above: raise a catchable ValueError on an empty
                                // source instead of silently returning
                                // `f64::NEG_INFINITY`/`0`.
                                if matches!(arg_ty, Ty::Iterator(_)) {
                                    let src = Self::iter_arg_source(&args[0], &a);
                                    return Ok(Some(match elem_ty {
                                        Ty::Float => format!("{{ let mut __max = f64::NEG_INFINITY; let mut __seen = false; for __x in {} {{ __seen = true; if __x > __max {{ __max = __x; }} }} if !__seen {{ panic!(\"ValueError\\0max() iterable argument is empty\"); }} __max }}", src),
                                        _ => format!("{}.max().unwrap_or_else(|| panic!(\"ValueError\\0max() iterable argument is empty\"))", src),
                                    }));
                                }
                                // (REVIEW FOLLOW-UP on 577b04f, item 2) See the
                                // `min` arm above: `.cloned()` works for both Copy
                                // and non-Copy element types.
                                // (W2 card ebd703d9) A `__lt__`-only class is
                                // `PartialOrd`, not `Ord`: route through a FIRST-WINS
                                // `>` fold (replace only on a STRICTLY greater
                                // element) so a tie keeps the EARLIEST element, exactly
                                // like CPython's `max()` (Rust's `.max()` would keep
                                // the last, and also needs `Ord`).
                                let is_cmp_class = matches!(&elem_ty, Ty::Class(..))
                                    && self.elem_needs_partial_cmp(&elem_ty);
                                return Ok(Some(if matches!(elem_ty, Ty::Float) {
                                    format!("{{ let mut __max = f64::NEG_INFINITY; let mut __seen = false; for __x in {}.iter() {{ __seen = true; if __x > &__max {{ __max = *__x; }} }} if !__seen {{ panic!(\"ValueError\\0max() iterable argument is empty\"); }} __max }}", a)
                                } else if is_cmp_class {
                                    // (enabler-fix-2 #8a) Compare by REFERENCE, clone
                                    // only the winner — see the mirror note in `min`.
                                    format!("{{ let __src = &({}); let mut __best: Option<&_> = None; for __ref in __src.iter() {{ let __take = match __best {{ None => true, Some(__b) => __ref > __b }}; if __take {{ __best = Some(__ref); }} }} __best.cloned().unwrap_or_else(|| panic!(\"ValueError\\0max() iterable argument is empty\")) }}", a)
                                } else {
                                    format!("{}.iter().cloned().max().unwrap_or_else(|| panic!(\"ValueError\\0max() iterable argument is empty\"))", a)
                                }));
                            } else {
                                // (card b557b9c1) n-ary max: mirror the `min` n-ary
                                // fold above with a FIRST-WINS `>` comparison — the
                                // prior arm consumed only args[0]/args[1] and silently
                                // dropped args[2..] (`max(1,2,3)` evaluated `max(1,2)`
                                // -> 2). `>` (STRICTLY greater) keeps the EARLIER arg
                                // on a tie, matching CPython's `max()` (Rust's own
                                // `::std::cmp::max` keeps the LATER, and needs `Ord`);
                                // `PartialOrd` covers int/str/f64/`__lt__`-class alike.
                                let mut parts = Vec::with_capacity(args.len());
                                for arg in args {
                                    parts.push(self.emit_consuming(arg)?);
                                }
                                let mut fold = format!("let mut __best = {};", parts[0]);
                                for p in &parts[1..] {
                                    fold.push_str(&format!(
                                        " {{ let __c = {}; if __c > __best {{ __best = __c; }} }}",
                                        p
                                    ));
                                }
                                return Ok(Some(format!("{{ {} __best }}", fold)));
                            }
                        }
                        "sorted" => {
                            let a = self.emit_expr(&args[0])?;
                            let list_ty = self.type_of_expr(&args[0]);
                            // (LAZY-GEN V1-c) A generator source can't be
                            // `.clone()`d (`Gen<T>` isn't `Clone`) — materialize
                            // it into an owned `Vec<T>` via `.collect()` first
                            // (a VARIABLE source is consumed `&mut`, Python-exact
                            // "exhausted but still bound"; a fresh call is
                            // consumed by value — see `iter_arg_source`).
                            // `sorted(...)` always returns a real `list[T]`,
                            // matching Python (which also materializes a
                            // generator when sorting it).
                            let list_src = if matches!(list_ty, Ty::Iterator(_)) {
                                format!("{}.collect::<Vec<_>>()", Self::iter_arg_source(&args[0], &a))
                            } else if matches!(list_ty, Ty::Bytes) {
                                // (W5-a) `sorted(bytes)` yields `list[int]`: widen
                                // each byte to i64 into an owned `Vec<i64>` so the
                                // sorted result prints as ints (CPython), not as a
                                // `Vec<u8>` the bytes `PyRepr` impl would render
                                // `b'...'`. Agrees with the `list[int]` oracle type.
                                format!("{}.iter().map(|&__b| __b as i64).collect::<Vec<i64>>()", a)
                            } else {
                                format!("{}.clone()", a)
                            };

                            if let Some((_, key_expr)) = kwargs.iter().find(|(n, _)| n == "key") {
                                // sorted with key parameter
                                // Determine the return type of the key expression
                                let key_ret_ty = if let Expr::Lambda { params: _, body, .. } = key_expr {
                                    // For lambdas, infer from the body expression
                                    // We need to temporarily register the parameter to type-check the body
                                    // But since type_of_expr is &self, we can't do that easily
                                    // So we'll just check common patterns
                                    if let Expr::Attr { name, .. } = body.as_ref() {
                                        // Lambda body is field access - check the field type
                                        // (LAZY-GEN V1-c) A generator source's element
                                        // class fields resolve the same way a list's do.
                                        if let Ty::List(ref elem_ty) | Ty::Iterator(ref elem_ty) = list_ty {
                                            if let Ty::Class(cls, _) = elem_ty.as_ref() {
                                                if let Some(c) = self.ctx.classes.get(cls.as_str()) {
                                                    if let Some(f) = c.fields.iter().find(|f| &f.name == name) {
                                                        Ty::from_type_expr(&f.ty, f.span).unwrap_or(Ty::Unknown)
                                                    } else {
                                                        Ty::Unknown
                                                    }
                                                } else {
                                                    Ty::Unknown
                                                }
                                            } else {
                                                Ty::Unknown
                                            }
                                        } else {
                                            Ty::Unknown
                                        }
                                    } else if let Expr::Call { callee, .. } = body.as_ref() {
                                        // Lambda body is a method call - check method return type
                                        if let Expr::Attr { name, .. } = callee.as_ref() {
                                            // (LAZY-GEN V1-c) Same as above: a
                                            // generator source's element methods
                                            // resolve the same way a list's do.
                                            if let Ty::List(ref elem_ty) | Ty::Iterator(ref elem_ty) = list_ty {
                                                if let Ty::Class(cls, _) = elem_ty.as_ref() {
                                                    if let Some(method_sig) = self.ctx.get_method(cls.as_str(), name) {
                                                        method_sig.ret.clone()
                                                    } else {
                                                        Ty::Unknown
                                                    }
                                                } else {
                                                    Ty::Unknown
                                                }
                                            } else {
                                                Ty::Unknown
                                            }
                                        } else {
                                            Ty::Unknown
                                        }
                                    } else {
                                        Ty::Unknown
                                    }
                                } else {
                                    Ty::Unknown
                                };

                                let key_code = if let Expr::Lambda { params, body, .. } = key_expr {
                                    // Lambda: extract parameter name and body, rename param to __x
                                    let param_name = params.first().map(|(n, _)| n.clone()).unwrap_or_else(|| "__x".to_string());
                                    // (CARD 55343eaa) The param was previously bound
                                    // to `Ty::Unknown` here, so a tuple-indexing body
                                    // (`t[0]`) couldn't see — via `type_of_expr` in
                                    // `emit_expr`'s Index arm — that `t` is a
                                    // `Ty::Tuple(..)`, and fell through to the
                                    // LIST-indexing lowering (`__py_list_get`,
                                    // slice-based) instead of the tuple field-access
                                    // lowering (`.0`/`.1`/...) the non-lambda
                                    // tuple-index path already uses — raw rustc
                                    // E0599 (`Vec` has no field `.0`) at BUILD.
                                    // Bind the param to the SOURCE's actual element
                                    // type (list/set/generator elem, or dict KEY —
                                    // mirrors the `base` element-source dispatch in
                                    // the no-key branch below) so the body sees the
                                    // real shape.
                                    let key_param_ty = match &list_ty {
                                        Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => (**inner).clone(),
                                        Ty::Dict(k, _) => (**k).clone(),
                                        _ => Ty::Unknown,
                                    };
                                    let saved_local = self.locals.get(&param_name).cloned();
                                    self.locals.insert(param_name.clone(), key_param_ty);
                                    let body_s = self.emit_expr(body)?;
                                    if let Some(ty) = saved_local {
                                        self.locals.insert(param_name.clone(), ty);
                                    } else {
                                        self.locals.remove(param_name.as_str());
                                    }
                                    // Replace param_name with __x in the body (word-boundary aware).
                                    // (EPIC-6) The body emitted the param through
                                    // emit_expr's Ident arm, which ESCAPES a keyword
                                    // param to `r#<name>`; search for that escaped
                                    // form so a keyword sort-key param is renamed
                                    // correctly (replace_identifier treats `r#kw` as
                                    // one token).
                                    Self::replace_identifier(&body_s, escape_ident(&param_name).as_str(), "__x")
                                } else {
                                    // Regular expression: wrap in closure that calls the key function
                                    self.emit_expr(key_expr)?
                                };

                                // (CARD 0bab32ed) `reverse=` was being silently
                                // dropped whenever `key=` was also present. CPython's
                                // `reverse=True` is a REVERSED STABLE SORT: elements
                                // with equal keys keep their ORIGINAL relative order.
                                // That is NOT the same as sorting ascending and then
                                // `.reverse()`-ing the whole vec, which flips the
                                // relative order within an equal-key run. Verified
                                // against `python3`:
                                //   sorted([(1,'a'),(2,'b'),(1,'c')], key=lambda t: t[0], reverse=True)
                                //   -> [(2,'b'),(1,'a'),(1,'c')]   (a before c, unchanged)
                                // A reversed COMPARATOR fed into Rust's stable
                                // `sort_by` reproduces this exactly: when two keys
                                // are equal, `.reverse()`'d Ordering::Equal is still
                                // Equal, so the stable sort leaves their relative
                                // input order untouched; only actually-unequal pairs
                                // flip. Handle a runtime (non-literal) `reverse=`
                                // expression too, not just `True`/`False` literals.
                                if let Some((_, rev_expr)) = kwargs.iter().find(|(n, _)| n == "reverse") {
                                    let rev_s = self.emit_expr(rev_expr)?;
                                    return Ok(Some(match key_ret_ty {
                                        Ty::Float => {
                                            format!(
                                                "{{ let mut __sorted = {}; let __rev = {}; __sorted.sort_by(|a, b| {{ let ka = {{ let __x = a.clone(); {} }}; let kb = {{ let __x = b.clone(); {} }}; let __ord = ka.partial_cmp(&kb).unwrap_or(::std::cmp::Ordering::Equal); if __rev {{ __ord.reverse() }} else {{ __ord }} }}); __sorted }}",
                                                list_src, rev_s, key_code, key_code
                                            )
                                        }
                                        _ => {
                                            format!(
                                                "{{ let mut __sorted = {}; let __rev = {}; __sorted.sort_by(|a, b| {{ let ka = {{ let __x = a.clone(); {} }}; let kb = {{ let __x = b.clone(); {} }}; let __ord = ka.cmp(&kb); if __rev {{ __ord.reverse() }} else {{ __ord }} }}); __sorted }}",
                                                list_src, rev_s, key_code, key_code
                                            )
                                        }
                                    }));
                                }

                                // Use appropriate sorting method based on key return type
                                return Ok(Some(match key_ret_ty {
                                    Ty::Float => {
                                        format!(
                                            "{{ let mut __sorted = {}; __sorted.sort_by(|a, b| {{ let ka = {{ let __x = a.clone(); {} }}; let kb = {{ let __x = b.clone(); {} }}; ka.partial_cmp(&kb).unwrap_or(::std::cmp::Ordering::Equal) }}); __sorted }}",
                                            list_src, key_code, key_code
                                        )
                                    }
                                    _ => {
                                        format!(
                                            "{{ let mut __sorted = {}; __sorted.sort_by_key(|__x| {}); __sorted }}",
                                            list_src, key_code
                                        )
                                    }
                                }));
                            } else {
                                // Pick the comparator by the ELEMENT type being
                                // sorted (list/generator/set element, or dict KEY).
                                // `f64` and a user class with `__lt__` are only
                                // `PartialOrd`, so both need `.sort_by(partial_cmp)`
                                // rather than `.sort()` (which requires `Ord`) —
                                // see `sort_suffix_for_elem`. This generalises the
                                // former float-only check to comparable user classes
                                // (closing the `sorted(list_of_obj)` E0277 leak, W0
                                // p12b) and to `set[float]`/float-keyed dicts.
                                let elem_ty = match &list_ty {
                                    Ty::List(inner) | Ty::Iterator(inner) | Ty::Set(inner) => (**inner).clone(),
                                    Ty::Dict(k, _) => (**k).clone(),
                                    // (W5-a) `sorted(bytes)` sorts the widened
                                    // `Vec<i64>` (list_src above) — its element is
                                    // `int`, so pick the plain `.sort()` comparator.
                                    Ty::Bytes => Ty::Int,
                                    _ => Ty::Unknown,
                                };
                                let sort_code = self.sort_suffix_for_elem(&elem_ty);

                                // `sorted` operates on a Vec. A list arg is cloned
                                // directly; a set is materialized from its elements;
                                // a dict from its KEYS (Python semantics — both
                                // HashMap/HashSet lack `.sort()`); a generator via
                                // `list_src` (`.collect()`, computed above).
                                let base = match &list_ty {
                                    Ty::Set(_) => format!("{}.iter().cloned().collect::<Vec<_>>()", a),
                                    Ty::Dict(_, _) => format!("{}.keys().cloned().collect::<Vec<_>>()", a),
                                    _ => list_src.clone(),
                                };

                                if let Some((_, rev_expr)) = kwargs.iter().find(|(n, _)| n == "reverse") {
                                    // sorted with reverse parameter
                                    let rev_s = self.emit_expr(rev_expr)?;
                                    return Ok(Some(format!(
                                        "{{ let mut __sorted = {}; __sorted{}; if {} {{ __sorted.reverse(); }} __sorted }}",
                                        base, sort_code, rev_s
                                    )));
                                } else {
                                    // Default sorted
                                    return Ok(Some(format!("{{ let mut __sorted = {}; __sorted{}; __sorted }}", base, sort_code)));
                                }
                            }
                        }
                        "sum" => {
                            let a = self.emit_expr(&args[0])?;
                            let arg_ty = self.type_of_expr(&args[0]);
                            // The iterable's element type (float elements sum in f64).
                            let elem_is_float = matches!(&arg_ty,
                                Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner)
                                    if matches!(inner.as_ref(), Ty::Float));
                            // (card aabf4ada) The 2-arg CPython form `sum(iterable,
                            // start)` folds `start` in as the accumulator SEED. The
                            // prior arm consumed ONLY args[0] and silently DROPPED the
                            // start — a P0 miscompile (`sum([1,2,3],10)` -> 6 not 16;
                            // `sum(gen,100)` -> 14 not 114) of the same
                            // accept-N-consume-fewer class as the n-ary min/max fix
                            // (b557b9c1). A 3+-arg call is rejected at typeck (CPython
                            // TypeError), so args.len() is 1 or 2 here.
                            let start = if args.len() >= 2 {
                                let s = self.emit_expr(&args[1])?;
                                let start_is_float = matches!(self.type_of_expr(&args[1]), Ty::Float);
                                Some((s, start_is_float))
                            } else {
                                None
                            };
                            // A FLOAT start promotes an int-element sum to f64
                            // (CPython: `sum([1,2,3],1.0)` -> 7.0), matching the typeck
                            // result-type oracle so the print-formatter's DISPLAY type
                            // agrees. `Iterator::sum::<f64>` has no impl over `&i64`, so
                            // when promoting int elements we map each element `as f64`
                            // BEFORE summing.
                            let result_is_float = elem_is_float
                                || start.as_ref().is_some_and(|(_, f)| *f);
                            let sum_type = if result_is_float { "f64" } else { "i64" };
                            // (LAZY-GEN V1-c) A generator source (`Gen<T>`) is itself an
                            // `Iterator` yielding OWNED `T` — consume it directly (no
                            // `.iter()`/deref, `Gen` has none); a list/set is borrowed
                            // via `.iter()` yielding `&T` (deref before the `as f64`).
                            let body = if matches!(arg_ty, Ty::Bytes) {
                                // (W5-a) sum(bytes): widen each byte to i64 before
                                // summing (`i64: Sum<&u8>` does not exist).
                                format!("{}.iter().map(|&__b| __b as i64).sum::<i64>()", a)
                            } else if matches!(arg_ty, Ty::Iterator(_)) {
                                let src = Self::iter_arg_source(&args[0], &a);
                                if result_is_float && !elem_is_float {
                                    format!("{}.map(|__x| __x as f64).sum::<f64>()", src)
                                } else {
                                    format!("{}.sum::<{}>()", src, sum_type)
                                }
                            } else if result_is_float && !elem_is_float {
                                format!("{}.iter().map(|__x| *__x as f64).sum::<f64>()", a)
                            } else {
                                format!("{}.iter().sum::<{}>()", a, sum_type)
                            };
                            return Ok(Some(match start {
                                None => body,
                                // Coerce an int start to f64 only when the result is
                                // promoted to float (an already-float start is emitted
                                // as-is — no redundant cast).
                                Some((s, start_is_float)) => {
                                    let seed = if result_is_float && !start_is_float {
                                        format!("({} as f64)", s)
                                    } else {
                                        s
                                    };
                                    format!("({} + {})", seed, body)
                                }
                            }));
                        }
                        "input" => {
                            if args.is_empty() {
                                return Ok(Some("{ let mut __s = String::new(); ::std::io::stdin().read_line(&mut __s).unwrap(); __s.trim_end().to_string() }".to_string()));
                            } else {
                                let p = self.emit_expr(&args[0])?;
                                return Ok(Some(format!("{{ print!(\"{{}}\" , {}); ::std::io::stdout().flush().ok(); let mut __s = String::new(); ::std::io::stdin().read_line(&mut __s).unwrap(); __s.trim_end().to_string() }}", p)));
                            }
                        }
                        "any" => {
                            let a = self.emit_expr(&args[0])?;
                            // (LAZY-GEN V1-c) A generator source yields owned
                            // `bool` directly (no `.iter()`/deref); short-circuit
                            // laziness is Rust `Iterator::any`'s native behavior,
                            // matching Python.
                            if matches!(self.type_of_expr(&args[0]), Ty::Iterator(_)) {
                                let src = Self::iter_arg_source(&args[0], &a);
                                return Ok(Some(format!("{}.any(|x| x)", src)));
                            }
                            return Ok(Some(format!("{}.iter().any(|x| *x)", a)));
                        }
                        "all" => {
                            let a = self.emit_expr(&args[0])?;
                            if matches!(self.type_of_expr(&args[0]), Ty::Iterator(_)) {
                                let src = Self::iter_arg_source(&args[0], &a);
                                return Ok(Some(format!("{}.all(|x| x)", src)));
                            }
                            return Ok(Some(format!("{}.iter().all(|x| *x)", a)));
                        }
                        "round" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("({} as f64).round() as i64", a)));
                        }
                        "pow" => {
                            let base = self.emit_expr(&args[0])?;
                            let exp = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("({} as f64).powi({} as i32) as i64", base, exp)));
                        }
                        "chr" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("(char::from_u32({} as u32).unwrap()).to_string()", a)));
                        }
                        "ord" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("({}.chars().next().unwrap() as i64)", a)));
                        }
                        "reversed" => {
                            let a = self.emit_expr(&args[0])?;
                            // (W5-a) `reversed(bytes)` yields `list[int]` (oracle
                            // type `list[int]`): widen each byte to i64 into an owned
                            // `Vec<i64>` BEFORE reversing, so the result prints as
                            // ints (CPython), not as a `Vec<u8>` the bytes `PyRepr`
                            // impl would render `b'...'`. This also feeds
                            // `list(reversed(b))` — whose `_` list arm returns this
                            // already-`Vec<i64>` expression verbatim.
                            if matches!(self.type_of_expr(&args[0]), Ty::Bytes) {
                                return Ok(Some(format!(
                                    "{{ let mut __r = {}.iter().map(|&__b| __b as i64).collect::<Vec<i64>>(); __r.reverse(); __r }}",
                                    a
                                )));
                            }
                            return Ok(Some(format!("{{ let mut __r = {}.clone(); __r.reverse(); __r }}", a)));
                        }
                        "map" => {
                            let f = self.emit_expr(&args[0])?;
                            let it = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("{}.iter().cloned().map({}).collect::<Vec<_>>()", it, f)));
                        }
                        "filter" => {
                            let f = self.emit_expr(&args[0])?;
                            let it = self.emit_expr(&args[1])?;
                            return Ok(Some(format!("{}.iter().cloned().filter(|__x| ({})((__x).clone())).collect::<Vec<_>>()", it, f)));
                        }
                        "isinstance" => {
                            if args.len() != 2 {
                                return Err(crate::diag::Error::Codegen("isinstance requires exactly 2 arguments".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            // Check if args[1] is a builtin type identifier
                            if let Expr::Ident(type_name, _) = &args[1] {
                                let matches = match type_name.as_str() {
                                    "int" => matches!(&obj_type, Ty::Int),
                                    "str" => matches!(&obj_type, Ty::Str),
                                    "float" => matches!(&obj_type, Ty::Float),
                                    "bool" => matches!(&obj_type, Ty::Bool),
                                    "list" => matches!(&obj_type, Ty::List(_)),
                                    "dict" => matches!(&obj_type, Ty::Dict(_, _)),
                                    "set" => matches!(&obj_type, Ty::Set(_)),
                                    // (W5-a) `isinstance(x, bytes)` is a REAL static
                                    // test now — the pyrst type system knows `bytes`.
                                    // Without this arm it fell to the `_` placeholder
                                    // below and returned constant `true` for EVERY x
                                    // (so `isinstance(5, bytes)` printed True). NOTE:
                                    // the `_ => true` placeholder is a PRE-EXISTING
                                    // silent-miscompile for tuple / every user class
                                    // (out of W5-a scope; reported for a follow-up card).
                                    "bytes" => matches!(&obj_type, Ty::Bytes),
                                    _ => {
                                        // For custom classes, emit runtime check
                                        let _obj = self.emit_expr(&args[0])?;
                                        return Ok(Some(format!("true"))); // Placeholder for custom class check
                                    }
                                };
                                return Ok(Some(if matches { "true" } else { "false" }.to_string()));
                            } else {
                                // Dynamic type check (not a literal type name)
                                return Ok(Some("true".to_string())); // Conservative: assume true for dynamic checks
                            }
                        }
                        "type" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("type requires exactly 1 argument".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            let type_name = match obj_type {
                                Ty::Int => "<class 'int'>",
                                Ty::Str => "<class 'str'>",
                                Ty::Float => "<class 'float'>",
                                Ty::Bool => "<class 'bool'>",
                                Ty::List(_) => "<class 'list'>",
                                Ty::Dict(_, _) => "<class 'dict'>",
                                Ty::Set(_) => "<class 'set'>",
                                // (W5-a) `type(b)` now reports the real builtin name
                                // (was `<class 'object'>` via the `_` fallback).
                                Ty::Bytes => "<class 'bytes'>",
                                // Both the `None` literal (NoneVal) and a void
                                // result (Unit) report Python's NoneType, matching
                                // the pre-NoneVal behavior of `type(None)`.
                                Ty::Unit | Ty::NoneVal => "<class 'NoneType'>",
                                _ => "<class 'object'>",
                            };
                            return Ok(Some(format!("String::from(\"{}\")", type_name)));
                        }
                        "hex" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("format!(\"{{:#x}}\", {})", a)));
                        }
                        "oct" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("format!(\"{{:#o}}\", {})", a)));
                        }
                        "bin" => {
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("format!(\"{{:#b}}\", {})", a)));
                        }
                        "callable" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("callable requires exactly 1 argument".into()));
                            }
                            // Check if the argument is a function name
                            if let Expr::Ident(name, _) = &args[0] {
                                let is_callable = self.ctx.funcs.contains_key(name.as_str()) ||
                                                 self.ctx.classes.contains_key(name.as_str());
                                return Ok(Some(if is_callable { "true" } else { "false" }.to_string()));
                            } else {
                                // For non-identifier expressions, conservatively return false
                                return Ok(Some("false".to_string()));
                            }
                        }
                        "repr" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("repr requires exactly 1 argument".into()));
                            }
                            // A bare `None` literal has an ambiguous `Option<T>`
                            // type (T uninferred), so `.py_repr()` can't resolve —
                            // and its repr is always the constant `'None'` anyway.
                            if matches!(&args[0], Expr::None_(_)) {
                                return Ok(Some("\"None\".to_string()".to_string()));
                            }
                            // Route EVERY other type through the CPython-parity
                            // `PyRepr` trait: floats keep their `.0`/scientific form,
                            // strs get the `%r` quote-choice matrix, containers/tuples
                            // recurse, an `Optional[X]` value reprs its payload or
                            // `None`, and a user class routes through its `__repr__`
                            // (via the per-class `impl PyRepr`). A class without
                            // `__repr__` is rejected at CHECK time (see `check_expr`),
                            // so no Display-fallback silent miscompile survives here.
                            let a = self.emit_expr(&args[0])?;
                            return Ok(Some(format!("({}).py_repr()", a)));
                        }
                        "ascii" => {
                            if args.len() != 1 {
                                return Err(crate::diag::Error::Codegen("ascii requires exactly 1 argument".into()));
                            }
                            let obj_type = self.type_of_expr(&args[0]);
                            let a = self.emit_expr(&args[0])?;
                            let ascii_expr = match obj_type {
                                Ty::Str => {
                                    // (enabler-fix-2 #4) ascii() = repr()'s quote-choice
                                    // matrix + escape EVERY non-ASCII code point as
                                    // \xXX/\uXXXX/\UXXXXXXXX. The old `escape_default`
                                    // used Rust's escaping (e.g. `\u{e9}`, wrong quote
                                    // logic); `__py_ascii` is the shared CPython engine.
                                    format!("__py_ascii(&({}))", a)
                                }
                                Ty::Bool => {
                                    format!("format!(\"{{}}\" , if {} {{ \"True\" }} else {{ \"False\" }})", a)
                                }
                                _ => format!("format!(\"{{}}\" , {})", a),
                            };
                            return Ok(Some(ascii_expr));
                        }
                        "list" => {
                            if args.is_empty() {
                                return Ok(Some("Vec::<i64>::new()".to_string()));
                            } else {
                                // (card 00fb0e6d) `list(range(...))` — a `range(...)`
                                // lowers to a Rust Range / StepBy ITERATOR, but
                                // `range()` types as `list[int]` (its Python result),
                                // so the already-a-list fast path below would return
                                // the bare iterator and fail rustc (E0308 vs Vec, or
                                // E0599 on the StepBy step form). Detect the range CALL
                                // structurally and `.collect()` it — the same iterator
                                // the `for`-loop lowering consumes, now materialized.
                                if let Expr::Call { callee: rc, .. } = &args[0] {
                                    if matches!(rc.as_ref(), Expr::Ident(rn, _) if rn == "range") {
                                        let r = self.emit_expr(&args[0])?;
                                        // (card 87bd8eb4) A 1/2-arg range is a lazy
                                        // Rust `..` iterator -> `.collect()`. A 3-arg
                                        // range already materializes a `Vec<i64>`
                                        // (__py_range_step, direction-aware) -> return
                                        // it directly (`.collect()` on a Vec is E0599).
                                        if r.contains("..") {
                                            return Ok(Some(format!("({}).collect::<Vec<_>>()", r)));
                                        }
                                        return Ok(Some(r));
                                    }
                                }
                                // (phase2-fix2 item 3) `list(enumerate(it[, start]))`
                                // and `list(zip(...))`: these builtins are INLINED
                                // (the `enumerate`/`zip` arms earlier in this fn) into
                                // a LAZY Rust adapter chain (`.enumerate().map(..)`,
                                // `.zip(..)`), yet they TYPE as `list[...]` (their
                                // Python result), so the already-a-list fast path below
                                // returned the bare adapter and leaked a rustc E0308
                                // (a `Map`/`Zip` is not a `Vec`). Detect the adapter
                                // CALL structurally — exactly like `range` above — and
                                // `.collect()` the emitted chain. typeck has already
                                // validated the callee's arity, so the inlined chain is
                                // always a collectible iterator here.
                                if let Expr::Call { callee: ac, .. } = &args[0] {
                                    if matches!(ac.as_ref(), Expr::Ident(an, _) if an == "enumerate" || an == "zip") {
                                        let a = self.emit_expr(&args[0])?;
                                        return Ok(Some(format!("({}).collect::<Vec<_>>()", a)));
                                    }
                                }
                                let a = self.emit_expr(&args[0])?;
                                let arg_type = self.type_of_expr(&args[0]);
                                // If the argument is already a list, just return it. Otherwise collect the iterator.
                                match arg_type {
                                    Ty::List(_) => return Ok(Some(a)),
                                    // (LAZY-GEN V1-c, review finding) A generator
                                    // arg is TYPE-DRIVEN, not sniffed from the
                                    // emitted call text — a generator function
                                    // named e.g. `unsorted_gen` would otherwise
                                    // mis-fire the "looks like a Vec already"
                                    // heuristic below (its emitted call text
                                    // contains the substring "sort") and skip the
                                    // needed `.collect()`. This explicit arm runs
                                    // FIRST, so the happy path is deliberate, not
                                    // an accident of the fallback.
                                    Ty::Iterator(_) => {
                                        let src = Self::iter_arg_source(&args[0], &a);
                                        return Ok(Some(format!("{}.collect::<Vec<_>>()", src)));
                                    }
                                    // A set/dict is a concrete container, not an
                                    // iterator: take an owned Vec of its elements
                                    // (dict -> its KEYS, Python semantics).
                                    Ty::Set(_) => {
                                        return Ok(Some(format!("{}.iter().cloned().collect::<Vec<_>>()", a)));
                                    }
                                    Ty::Dict(_, _) => {
                                        return Ok(Some(format!("{}.keys().cloned().collect::<Vec<_>>()", a)));
                                    }
                                    // (W5-a) list(bytes) -> list[int]: each byte widened
                                    // to i64 (Python's `list(b'AB') == [65, 66]`).
                                    Ty::Bytes => {
                                        return Ok(Some(format!("{}.iter().map(|&__x| __x as i64).collect::<Vec<i64>>()", a)));
                                    }
                                    _ => {
                                        // Check if the expression looks like it returns a Vec (contains reverse, sort, etc.)
                                        if a.contains("reverse") || a.contains("sort") || a.contains("clone()") {
                                            return Ok(Some(a));
                                        }
                                        return Ok(Some(format!("{}.collect::<Vec<_>>()", a)));
                                    }
                                }
                            }
                        }
                        "dict" => {
                            if args.is_empty() && kwargs.is_empty() {
                                return Ok(Some("std::collections::HashMap::new()".to_string()));
                            } else {
                                return Err(crate::diag::Error::Codegen("dict() constructor with arguments not yet supported".into()));
                            }
                        }
                        "tuple" => {
                            if args.is_empty() {
                                return Ok(Some("()".to_string()));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                return Ok(Some(format!("({},)", a)));
                            }
                        }
                        "getattr" => {
                            if args.len() < 2 || args.len() > 3 {
                                return Err(crate::diag::Error::Codegen("getattr requires 2 or 3 arguments".into()));
                            }
                            let _obj = self.emit_expr(&args[0])?;
                            let attr_name = self.emit_expr(&args[1])?;

                            // For now, just access the field directly (works for simple cases)
                            // This assumes the object is a struct with the matching field name
                            return Ok(Some(format!("{{ let __attr_name = {}; format!(\"{{:?}}\", __attr_name) }}", attr_name)));
                        }
                        "setattr" => {
                            if args.len() != 3 {
                                return Err(crate::diag::Error::Codegen("setattr requires exactly 3 arguments".into()));
                            }
                            // Note: In Python, setattr modifies the object. In Rust, we can't modify through a reference.
                            // For now, just return None
                            return Ok(Some("()".to_string()));
                        }
                        "hasattr" => {
                            if args.len() != 2 {
                                return Err(crate::diag::Error::Codegen("hasattr requires exactly 2 arguments".into()));
                            }
                            // For now, just return true (conservative assumption)
                            return Ok(Some("true".to_string()));
                        }
                        "set" => {
                            if args.is_empty() {
                                return Ok(Some("::std::collections::HashSet::new()".to_string()));
                            } else {
                                let a = self.emit_expr(&args[0])?;
                                let arg_type = self.type_of_expr(&args[0]);
                                // If the argument is already a set, just return it. Otherwise convert to set.
                                match arg_type {
                                    Ty::Set(_) => return Ok(Some(a)),
                                    Ty::List(_) | Ty::Unknown => {
                                        // Check if it looks like a vec literal or variable
                                        if a.starts_with("vec!") {
                                            return Ok(Some(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a)));
                                        } else {
                                            return Ok(Some(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a)));
                                        }
                                    }
                                    _ => {
                                        // For other iterables, try to convert
                                        return Ok(Some(format!("{}.into_iter().collect::<::std::collections::HashSet<_>>()", a)));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
        Ok(None)
    }
    /// (card 18682938) Emit `e` as a Rust `bool` for a TRUTHINESS context
    /// (if/while/assert conditions, `not`, `and`/`or`, `bool(x)`). When `e` is an
    /// instance of a user class that defines `__bool__`, lower object truthiness to
    /// a `.__bool__()` call (CPython semantics). Every other type emits exactly as
    /// `emit_expr` does — so a `bool`-typed condition (comparison, bool local, …) is
    /// byte-for-byte unchanged, and pyrst's existing "condition must be bool"
    /// requirement is untouched for non-class operands.
    pub(crate) fn emit_truthy(&mut self, e: &Expr) -> Result<String> {
        // `and`/`or` in a truthiness context: RECURSE so a class-with-`__bool__`
        // operand on either side lowers via `__bool__` too (`if a and b:`). Handled
        // here (not in the generic BinOp path) to keep the hot arithmetic path
        // untouched; pyrst's `and`/`or` already lower to `&&`/`||` (bool-only), so
        // this stays consistent. A nested `not` reaches the UnOp arm, which itself
        // routes its operand through `emit_truthy`.
        if let Expr::BinOp { op: op @ (BinOp::And | BinOp::Or), lhs, rhs, .. } = e {
            let l = self.emit_truthy(lhs)?;
            let r = self.emit_truthy(rhs)?;
            let op_s = if matches!(op, BinOp::And) { "&&" } else { "||" };
            return Ok(format!("({} {} {})", l, op_s, r));
        }
        let s = self.emit_expr(e)?;
        if let Ty::Class(cls, _) = self.type_of_expr(e) {
            if self.ctx.get_method(&cls, "__bool__").is_some() {
                return Ok(format!("({}).__bool__()", s));
            }
        }
        Ok(s)
    }

    pub(crate) fn emit_expr(&mut self, e: &Expr) -> Result<String> {
        Ok(match e {
            // A numeric literal is a primary expression, so bare `0i64` / `1.5f64`
            // is precedence-safe in every position (receiver, exponent, as-cast,
            // operand). The lexer only produces non-negative literals (a leading
            // `-` is a separate `UnOp::Neg` node that adds its own parens), BUT
            // `try_fold_const` can fold e.g. `2 - 5` into `Expr::Int(-3)`, so the
            // guard below is load-bearing: a negative literal must parenthesize
            // (`(-3i64)`) or it would bind as `-(3i64.method())` in receiver
            // position. Non-negative literals emit bare (was unconditionally
            // `(..)`).
            Expr::Int(n, _) => {
                let lit = format!("{}i64", n);
                if lit.starts_with('-') { format!("({})", lit) } else { lit }
            }
            Expr::Float(f, _) => {
                let lit = format!("{}f64", f);
                if lit.starts_with('-') { format!("({})", lit) } else { lit }
            }
            Expr::Bool(b, _) => b.to_string(),
            Expr::None_(_) => "None".to_string(),
            Expr::Str(s, _) => format!("String::from({:?})", s),
            // (W5-a) A `bytes` literal lowers to an owned `Vec<u8>`, byte-exact.
            // Each byte is spelled `NNu8`; an empty literal uses a turbofish so its
            // element type is unambiguous in an inference-free position.
            Expr::Bytes(v, _) => {
                if v.is_empty() {
                    "Vec::<u8>::new()".to_string()
                } else {
                    let elems = v.iter().map(|b| format!("{}u8", b)).collect::<Vec<_>>().join(", ");
                    format!("vec![{}]", elems)
                }
            }
            Expr::FStr(parts, _) => {
                let mut fmt_str = String::new();
                let mut args = Vec::new();
                for part in parts {
                    match part {
                        crate::ast::FStrPart::Lit(s) => {
                            // Escape { and } in the format string
                            fmt_str.push_str(&s.replace('{', "{{").replace('}', "}}"));
                        }
                        crate::ast::FStrPart::Interp(expr, spec) => {
                            match spec {
                                None => {
                                    // No spec: match print()'s Python-style Display
                                    // so bare floats/bools render as `1.0` / `True`.
                                    fmt_str.push_str("{}");
                                    let raw = self.emit_expr(expr)?;
                                    let arg = match self.type_of_expr(expr) {
                                        Ty::Float => format!("__py_fmt_float({})", raw),
                                        Ty::Bool => format!("__py_fmt_bool({})", raw),
                                        Ty::Bytes | Ty::List(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Tuple(_) =>
                                            format!("({}).py_repr()", raw),
                                        // (enabler-fix-2 #3) f"{opt}" -> payload-or-None.
                                        Ty::Option(inner) => self.emit_str_option(&raw, inner.as_ref(), 0),
                                        _ => raw,
                                    };
                                    args.push(arg);
                                }
                                Some(s) => {
                                    // Explicit spec: emit a Rust format spec and pass the
                                    // raw value (the spec drives formatting, e.g. {:.2}).
                                    let clean = s.trim_end_matches(|c: char| "fdsge%".contains(c));
                                    fmt_str.push_str(&format!("{{:{}}}", clean));
                                    args.push(self.emit_expr(expr)?);
                                }
                            }
                        }
                    }
                }
                if args.is_empty() {
                    format!("String::from(\"{}\")", fmt_str)
                } else {
                    format!("format!(\"{}\", {})", fmt_str, args.join(", "))
                }
            }
            Expr::List(elems, _) => {
                // (EPIC-5 C2-2b-i, Step 3) A list literal whose elements' common
                // type is a polymorphic base is `Vec<B__>`: each raw-struct/ctor
                // element is wrapped into its enum variant (`[Dog(), Cat()]` ->
                // `vec![Animal__::Dog(..), Animal__::Cat(..)]`). A list of already-
                // `B__` places passes through element-wise. (list+list `+` CONCAT
                // element wrapping stays a documented C2-3 gap — not handled here.)
                if let Some(base) = self.list_poly_base(elems) {
                    let base_ty = Ty::Class(base, vec![]);
                    let mut parts = Vec::with_capacity(elems.len());
                    for e in elems { parts.push(self.emit_into_base_slot(e, &base_ty)?); }
                    return Ok(format!("vec![{}]", parts.join(", ")));
                }
                // When the literal's unified element type is Float but some
                // elements are int literals (`[1, 2.0]`), cast the int elements
                // to f64 so the vec is a homogeneous `Vec<f64>` (card 5c2f31d8).
                let widen = matches!(self.list_elem_ty(elems), Ty::Float);
                let mut parts = Vec::new();
                for e in elems { parts.push(self.emit_collection_elem(e, widen)?); }
                format!("vec![{}]", parts.join(", "))
            }
            Expr::Tuple(elems, _) => {
                let parts: Result<Vec<_>> = elems.iter().map(|e| self.emit_consuming(e)).collect();
                let parts = parts?;
                match parts.len() {
                    0 => "()".to_string(),
                    1 => format!("({},)", parts[0]),
                    _ => format!("({})", parts.join(", ")),
                }
            }
            Expr::ListComp { elt, targets, iter, cond, .. } => {
                let iter_s = self.emit_expr(iter)?;
                // (CARD fd65dc99) `zip(...)`/`enumerate(...)` are inlined by
                // `emit_expr` (above) straight into a REAL lazy Rust iterator
                // adapter chain (`.zip(..)`, `.enumerate().map(..)`) — not a
                // `.iter().cloned()`-able `Vec` — so the `.iter().cloned()`
                // fallback below doesn't compile against it (`Zip`/`Map` have no
                // `.iter()`). Detect that emitted shape the same way `Stmt::For`
                // already does (string-sniffing the generated code, since
                // codegen's own `type_of_expr` oracle types the call as a
                // `Ty::List`, not `Ty::Iterator`) and consume it directly. Must
                // be checked before `is_range`: a range nested inside
                // (`zip(range(..), ys)`) would otherwise match `is_range` on the
                // OUTER string and get double-wrapped in `.into_iter()`.
                let is_lazy_adapter = iter_s.contains(".enumerate()") || iter_s.contains(".zip(");
                let is_range = iter_s.contains("..");
                let chain = if is_lazy_adapter {
                    iter_s.clone()
                } else if is_range {
                    format!("({}).into_iter()", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Str) {
                    // Iterating a str yields 1-character strings (Python semantics)
                    format!("{}.chars().map(|__c| __c.to_string())", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Iterator(_)) {
                    // (LAZY-GEN V1-b) A generator source (`Gen<T>`) is itself an
                    // `Iterator` yielding owned `T`; consume it DIRECTLY — no
                    // `.iter().cloned()` (`Gen` has no `.iter()`), no double clone.
                    // The map/filter_map adapters compose straight onto the `Gen`.
                    // (review fix) A generator VARIABLE is borrowed `&mut`, not
                    // moved — the binding stays live and advances in place
                    // (Python: a comprehension drains the generator; reuse then
                    // yields nothing instead of E0382).
                    if matches!(iter.as_ref(), Expr::Ident(..)) {
                        format!("(&mut {})", iter_s)
                    } else {
                        iter_s.clone()
                    }
                } else if matches!(self.type_of_expr(iter), Ty::Bytes) {
                    // (W5-a, blocker B1) Iterating bytes yields INTS (u8 as i64):
                    // the comprehension target is typed `int` by typeck, so widen
                    // each `&u8` element instead of `.cloned()`-ing a `u8`. Mirrors
                    // the `Stmt::For` bytes arm (codegen/stmts.rs). Without it a
                    // bytes source PASSED `check` (typeck promised `int`) and FAILED
                    // rustc (E0308 / "cannot multiply u8 by i64") inside every
                    // list/set/dict comprehension — the exact check/build divergence
                    // the keystone exists to close.
                    format!("{}.iter().map(|&__b| __b as i64)", iter_s)
                } else {
                    format!("{}.iter().cloned()", iter_s)
                };
                // Bind the loop target(s) to the iterable's element type for the
                // closure body, so a method call on the loop variable inside it
                // (`it.get()`) resolves to the element's CLASS method (BUG: an
                // unbound loop var fell through to a dict builtin and panicked).
                let saved = self.bind_comp_targets(targets, iter);
                let elt_s = self.emit_comp_value(elt)?;
                // (EPIC-6) Escape each comprehension target in the closure pattern;
                // the elt/cond bodies reference it via emit_expr Ident (same escape).
                // A single target is a bare name; multiple targets (tuple-unpacking,
                // e.g. `[v for k, v in d.items()]`) form a tuple pattern `(k, v)`
                // (mirrors the `Stmt::For` lowering).
                let target = comp_target_pat(targets);
                let result = if let Some(cond_expr) = cond {
                    // (card c34ac64a fix C) A comprehension `if`-filter is a
                    // truthiness context — a `__bool__` class lowers via
                    // `.__bool__()` (was E0308); plain bool filters pass through.
                    let cond_s = self.emit_truthy(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some({}) }} else {{ None }} ).collect::<Vec<_>>()",
                        chain, target, cond_s, elt_s)
                } else {
                    format!("{}.map(|{}| {}).collect::<Vec<_>>()", chain, target, elt_s)
                };
                self.restore_comp_targets(saved);
                result
            }
            Expr::SetComp { elt, targets, iter, cond, .. } => {
                let iter_s = self.emit_expr(iter)?;
                // (CARD fd65dc99) See the ListComp arm above: a zip/enumerate
                // source is already a real lazy Rust iterator chain, not a
                // `.iter().cloned()`-able Vec.
                let is_lazy_adapter = iter_s.contains(".enumerate()") || iter_s.contains(".zip(");
                let is_range = iter_s.contains("..");
                let chain = if is_lazy_adapter {
                    iter_s.clone()
                } else if is_range {
                    format!("({}).into_iter()", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Str) {
                    format!("{}.chars().map(|__c| __c.to_string())", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Iterator(_)) {
                    // (LAZY-GEN V1-b) A generator source is itself an `Iterator`
                    // (`Gen<T>`) yielding owned `T`; consume it DIRECTLY — no
                    // `.iter().cloned()` (`Gen` has no `.iter()`), no double clone.
                    // (review fix) A VARIABLE source borrows `&mut` (see ListComp).
                    if matches!(iter.as_ref(), Expr::Ident(..)) {
                        format!("(&mut {})", iter_s)
                    } else {
                        iter_s.clone()
                    }
                } else if matches!(self.type_of_expr(iter), Ty::Bytes) {
                    // (W5-a, blocker B1) Iterating bytes yields INTS (u8 as i64):
                    // the comprehension target is typed `int` by typeck, so widen
                    // each `&u8` element instead of `.cloned()`-ing a `u8`. Mirrors
                    // the `Stmt::For` bytes arm (codegen/stmts.rs). Without it a
                    // bytes source PASSED `check` (typeck promised `int`) and FAILED
                    // rustc (E0308 / "cannot multiply u8 by i64") inside every
                    // list/set/dict comprehension — the exact check/build divergence
                    // the keystone exists to close.
                    format!("{}.iter().map(|&__b| __b as i64)", iter_s)
                } else {
                    format!("{}.iter().cloned()", iter_s)
                };
                let saved = self.bind_comp_targets(targets, iter);
                let elt_s = self.emit_comp_value(elt)?;
                // (EPIC-6) Escape the comprehension target(s) (see ListComp above).
                let target = comp_target_pat(targets);
                let result = if let Some(cond_expr) = cond {
                    // (card c34ac64a fix C) See ListComp: filter is a truthiness ctx.
                    let cond_s = self.emit_truthy(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some({}) }} else {{ None }} ).collect::<::std::collections::HashSet<_>>()",
                        chain, target, cond_s, elt_s)
                } else {
                    format!("{}.map(|{}| {}).collect::<::std::collections::HashSet<_>>()", chain, target, elt_s)
                };
                self.restore_comp_targets(saved);
                result
            }
            Expr::DictComp { key, val, targets, iter, cond, .. } => {
                let iter_s = self.emit_expr(iter)?;
                // (CARD fd65dc99) See the ListComp arm above: a zip/enumerate
                // source is already a real lazy Rust iterator chain, not a
                // `.iter().cloned()`-able Vec.
                let is_lazy_adapter = iter_s.contains(".enumerate()") || iter_s.contains(".zip(");
                let is_range = iter_s.contains("..");
                let chain = if is_lazy_adapter {
                    iter_s.clone()
                } else if is_range {
                    format!("({}).into_iter()", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Str) {
                    format!("{}.chars().map(|__c| __c.to_string())", iter_s)
                } else if matches!(self.type_of_expr(iter), Ty::Iterator(_)) {
                    // (LAZY-GEN V1-b) A generator source is itself an `Iterator`
                    // (`Gen<T>`) yielding owned `T`; consume it DIRECTLY — no
                    // `.iter().cloned()` (`Gen` has no `.iter()`), no double clone.
                    // (review fix) A VARIABLE source borrows `&mut` (see ListComp).
                    if matches!(iter.as_ref(), Expr::Ident(..)) {
                        format!("(&mut {})", iter_s)
                    } else {
                        iter_s.clone()
                    }
                } else if matches!(self.type_of_expr(iter), Ty::Bytes) {
                    // (W5-a, blocker B1) Iterating bytes yields INTS (u8 as i64):
                    // the comprehension target is typed `int` by typeck, so widen
                    // each `&u8` element instead of `.cloned()`-ing a `u8`. Mirrors
                    // the `Stmt::For` bytes arm (codegen/stmts.rs). Without it a
                    // bytes source PASSED `check` (typeck promised `int`) and FAILED
                    // rustc (E0308 / "cannot multiply u8 by i64") inside every
                    // list/set/dict comprehension — the exact check/build divergence
                    // the keystone exists to close.
                    format!("{}.iter().map(|&__b| __b as i64)", iter_s)
                } else {
                    format!("{}.iter().cloned()", iter_s)
                };
                let saved = self.bind_comp_targets(targets, iter);
                let key_s = self.emit_comp_value(key)?;
                let val_s = self.emit_comp_value(val)?;
                // (EPIC-6) Escape the comprehension target(s) (see ListComp above).
                let target = comp_target_pat(targets);
                let result = if let Some(cond_expr) = cond {
                    // (card c34ac64a fix C) See ListComp: filter is a truthiness ctx.
                    let cond_s = self.emit_truthy(cond_expr)?;
                    format!("{}.filter_map(|{}| if {} {{ Some(({}, {})) }} else {{ None }} ).collect::<::std::collections::HashMap<_,_>>()",
                        chain, target, cond_s, key_s, val_s)
                } else {
                    format!("{}.map(|{}| ({}, {})).collect::<::std::collections::HashMap<_,_>>()", chain, target, key_s, val_s)
                };
                self.restore_comp_targets(saved);
                result
            }
            Expr::Set(elems, _) => {
                if elems.is_empty() {
                    return Ok("::std::collections::HashSet::new()".to_string());
                }
                // Mirror the list case: cast int elements to f64 when the set's
                // unified element type is Float. NOTE: a Float-element set
                // (`HashSet<f64>`) does not compile (f64 is not Eq/Hash) and is
                // unsupported in pyrst today — this widening only keeps the
                // emission consistent with the list path; it does not make a
                // numeric set literal compilable (card 5c2f31d8).
                let widen = matches!(self.list_elem_ty(elems), Ty::Float);
                let mut items = Vec::new();
                for e in elems {
                    items.push(self.emit_collection_elem(e, widen)?);
                }
                format!("vec![{}].into_iter().collect::<::std::collections::HashSet<_>>()",
                    items.join(", "))
            }
            Expr::Dict(pairs, _) => {
                if pairs.is_empty() {
                    return Ok("::std::collections::HashMap::new()".to_string());
                }
                let mut inserts = Vec::new();
                for (k, v) in pairs {
                    let ks = self.emit_consuming(k)?;
                    let vs = self.emit_consuming(v)?;
                    inserts.push(format!("({}, {})", ks, vs));
                }
                format!("vec![{}].into_iter().collect::<::std::collections::HashMap<_,_>>()",
                    inserts.join(", "))
            }
            // (EPIC-6) THE central identifier-use emission. Covers a bare
            // variable read AND a free-function call name (a user-fn call falls
            // through to `emit_expr(callee)` here), so escaping once here keeps
            // def and every use in sync. `self` is not a keyword and passes
            // through unchanged (legitimate receiver).
            Expr::Ident(n, _) => {
                // A bare reference to a MODULE CONSTANT emits its MANGLED Rust
                // name (`mangle_const`) — never the bare pyrst name — so the const
                // can't be captured as a Rust const-pattern. A local shadowing the
                // const name keeps the local's value (locals win, matching normal
                // name resolution), so the mangling only applies when `n` is NOT a
                // local. A str const additionally recovers a `String` from its
                // `&str` const.
                if let Some(m) = self.shadow_read_name(n) {
                    // (card 575bcf3a, poison2) A read of a HOISTED local that is
                    // currently divergently shadowed inside a block resolves to the
                    // mangled shadow binding (`__pyrst_shadow_..`) that holds the
                    // block-local value, not the hidden function-scope slot. Empty
                    // shadow_map (the common case) never reaches here, so shadow-free
                    // code is byte-for-byte unchanged. A shadowed name is always a
                    // local, so this correctly precedes the module-const path.
                    m
                } else if self.locals.contains_key(n) {
                    // A local always wins its bare name (shadows a module const/fn).
                    escape_ident(n)
                } else if let Some((owner, gty)) = self.resolve_bare_global(n) {
                    // (W4-a) A bare read of a promoted MUTABLE GLOBAL that is NOT a
                    // local: `G.with(|c| c.get())` (Cell) / `G.with(|c| c.borrow()
                    // .clone())` (RefCell) — a value-semantics snapshot, evaluated at
                    // the read site (so a read inside a closure body is call-time
                    // LIVE, never captured). Locals win above (a rebind without
                    // `global` is a local shadow), matching Python name resolution.
                    self.emit_global_read(owner.as_deref(), n, &gty)
                } else {
                    // (W3-2 / W3-fix F8,F9) Resolve the owner ONCE the same way typeck
                    // does (current-module own def / from-import / root), then decide
                    // const-vs-fn/class by OWNER-KEYED membership: a bare `FOO` in a
                    // module that OWNS a function `FOO` no longer matches a same-named
                    // CONST owned by a co-imported module, and the str-ness `.to_string()`
                    // fix-up is likewise per-(owner,name). The owner also owner-qualifies
                    // the mangled name so co-imported same-named symbols stay distinct;
                    // a root-owned name / builtin resolves to `None` → `escape_ident`
                    // (import-free output byte-identical).
                    let owner = self.bare_owner_for(n);
                    let key = (owner.clone(), n.clone());
                    if self.const_names.contains(&key) {
                        if self.const_strs.contains(&key) {
                            format!("{}.to_string()", mangle_const(owner.as_deref(), n))
                        } else {
                            mangle_const(owner.as_deref(), n)
                        }
                    } else {
                        match owner {
                            Some(o) => self.emit_name(Some(&o), n),
                            None => escape_ident(n),
                        }
                    }
                }
            }
            Expr::Call { callee, args, kwargs, .. } => {
                self.emit_call(callee, args, kwargs)?
            }
            Expr::Attr { obj, name, .. } => {
                // (card 03eb4e2c) Class-level CONSTANT (enum member) access:
                // `Color.RED` (class name), `self.RED` (inside a method), and
                // `inst.RED` (an instance) all lower to the associated const
                // `Color::RED`. A str const is stored as `&str`, so recover a
                // `String` with `.to_string()` (mirrors module-const handling).
                // This MUST run before `emit_expr(obj)` below — for a class-name
                // receiver `emit_expr(Color)` is not a value and would not compile.
                {
                    let const_class: Option<String> = match obj.as_ref() {
                        Expr::Ident(cn, _) if self.ctx.classes.contains_key(cn.as_str()) => {
                            Some(cn.clone())
                        }
                        _ => match self.type_of_expr(obj) {
                            Ty::Class(cn, _) => Some(cn),
                            _ => None,
                        },
                    };
                    if let Some(cn) = const_class {
                        if self.is_class_const_field(&cn, name) {
                            // (enabler-fix-1 #3c) A const lives on the impl of the class
                            // that DECLARES it, so an inherited access (`Sub.KIND`) must
                            // resolve to the defining class (`Base::KIND`) — emitting
                            // `Sub::KIND` was an E0599 (Sub's impl has no such const).
                            let dc = self.ctx.defining_class(&cn, name).unwrap_or_else(|| cn.clone());
                            let is_str = matches!(
                                self.ctx
                                    .classes
                                    .get(dc.as_str())
                                    .and_then(|cd| cd.fields.iter().find(|f| f.name == *name))
                                    .and_then(|f| f.default.as_ref()),
                                Some(Expr::Str(..))
                            );
                            // (W3-2) `Color::RED` targets the DEFINING class's
                            // owner-qualified struct (the assoc const lives on its
                            // impl block); `dc` stays bare for the lookup above.
                            let path = format!("{}::{}", self.emit_class_name(&dc), escape_ident(name));
                            return Ok(if is_str {
                                format!("{}.to_string()", path)
                            } else {
                                path
                            });
                        }
                    }
                }
                // Qualified MODULE CONSTANT `X.CONST` for a REAL imported module:
                // when X is a tracked module and CONST is one of its module-level
                // constants, lower to the MANGLED Rust `const __pyrst_const_CONST`
                // (the const namespace is flat, mirroring qualified module CALLS;
                // the mangling prevents const-pattern capture). A str const
                // recovers a `String` from its `&str` const. This GENERALIZES the
                // former hardcoded `math.pi`/`math.e`/`math.tau` arm — `math` is
                // now a real embedded module (`lib/math.pyrs`), so its constants
                // flow through here like any other module's.
                if let Expr::Ident(modname, _) = obj.as_ref() {
                    if self
                        .ctx
                        .module_consts
                        .get(modname)
                        .is_some_and(|cs| cs.iter().any(|(c, _)| c == name))
                        // (W4-a) A scalar-literal binding promoted to a mutable global
                        // is registered in `module_consts` too, but it is a
                        // `thread_local!` static — NOT a `const`. Skip the const arm
                        // so it routes to the mutable-global read below.
                        && !self.ctx.is_mutable_global(Some(modname), name)
                    {
                        // (W3-2 / W3-fix F9) The qualifier IS the owner: emit the
                        // owner-qualified mangled const so `sys.version` and any
                        // co-imported module's same-named const stay distinct Rust
                        // items, and check str-ness OWNER-KEYED so a same-named int
                        // const elsewhere can't flip the `.to_string()` fix-up.
                        return Ok(if self.const_strs.contains(&(Some(modname.clone()), name.clone())) {
                            format!("{}.to_string()", mangle_const(Some(modname), name))
                        } else {
                            mangle_const(Some(modname), name)
                        });
                    }
                }
                // (W4-a) Qualified MUTABLE-GLOBAL READ `m.x` for a real imported
                // module: the qualifier IS the owner, so emit the owner-qualified
                // `thread_local!` read (value-semantics snapshot) — the same owner
                // path a qualified const read uses (W3 resolves the owner for free).
                // Guarded on `m` not being a local (a class-typed local's field of
                // the same name must not be intercepted). Cross-module WRITES are a
                // v1 honest error (rejected at `check`); only reads reach here.
                if let Expr::Ident(modname, _) = obj.as_ref() {
                    if !self.locals.contains_key(modname)
                        && self.ctx.is_mutable_global(Some(modname), name)
                    {
                        let gty = self
                            .ctx
                            .mutable_global_ty(Some(modname), name)
                            .cloned()
                            .unwrap_or(Ty::Unknown);
                        return Ok(self.emit_global_read(Some(modname), name, &gty));
                    }
                }

                let o = self.emit_expr(obj)?;
                // Check if this is a @property access
                let is_property = self.is_property_access(obj, name);
                if is_property {
                    // A @property getter call: the method name (`name`) is a user
                    // method name — escaped so a keyword-named property still
                    // compiles. (Plain field reads below are escaped likewise.)
                    format!("{}.{}()", o, escape_ident(name))
                } else if !matches!(obj.as_ref(),
                                    Expr::Ident(n, _) if n == "self"
                                        || self.concrete_struct_params.contains(n))
                    && matches!(&self.type_of_expr(obj),
                                Ty::Class(b, _) if self.is_polymorphic_base(b)) {
                    // (EPIC-5 C2-2b-i) FIELD READ through a polymorphic-base var
                    // (a local/param/field whose static type is a polymorphic base).
                    // The receiver is Rust `B__` (an enum with no fields), so a
                    // direct `.{name}` won't compile. Lower to the companion enum's
                    // field-accessor `__field_{name}()` (emitted by
                    // emit_companion_enum for every base field — only base fields
                    // are reachable here; typeck already rejects a derived-only
                    // field on a base var). `self` is EXEMPT: inside a method body
                    // `self` is the concrete struct (`Account`/`Savings`), so
                    // `self.balance` is an ordinary struct-field read. A field-WRITE
                    // through a base var is a deferred honest error (AttrAssign).
                    // The companion-enum accessor is named `__field_<name>` (the
                    // `__field_` prefix makes it a non-keyword), so it is NOT
                    // escaped here — it must match the unescaped accessor emitted
                    // by emit_companion_enum.
                    format!("{}.__field_{}()", o, name)
                } else {
                    // (card 30e4fdd0) A boxed-recursive field STORES `Option<Box<Node>>`.
                    // As an RVALUE, UNBOX it back to the type system's Box-blind
                    // `Option<Node>`: `.clone().map(|__b| *__b)` deep-clones (pyrst
                    // value semantics — the documented aliasing divergence from
                    // Python's reference semantics) and unboxes each element. A
                    // non-recursive field keeps the ordinary clone-on-use read.
                    if self.attr_read_is_boxed_recursive(obj, name) {
                        // (EPIC-6) escape the keyword field name as elsewhere.
                        format!("{}.{}.clone().map(|__b| *__b)", o, escape_ident(name))
                    } else {
                        // (EPIC-6) Ordinary struct-field read: escape a keyword field
                        // name so it matches the (escaped) struct field definition.
                        format!("{}.{}", o, escape_ident(name))
                    }
                }
            }
            Expr::Index { obj, idx, .. } => {
                // type_of_expr (not just an Ident lookup) so nested/chained
                // receivers resolve — e.g. grid["row"]["x"] sees the inner Dict.
                let obj_ty = self.type_of_expr(obj);
                let o = self.emit_expr(obj)?;
                // Tuple subscript with a literal index -> Rust field access (t.N),
                // cloned so the element can be used without moving out of the tuple.
                if let Ty::Tuple(_) = obj_ty {
                    if let Expr::Int(n, _) = idx.as_ref() {
                        return Ok(format!("({}).{}.clone()", o, n));
                    }
                }
                let i = self.emit_expr(idx)?;
                match &obj_ty {
                    Ty::Dict(k, _) => {
                        // .expect() produces a Rust message without the NUL delimiter;
                        // unwrap_or_else lets us emit a matchable "KeyError\0..." payload.
                        // A GENERIC key type (a bare `Ty::TypeVar`, e.g. a class
                        // indexing its own `dict[K, V]` field) has no `Debug` bound
                        // — we never infer `K: Debug` — so the `{:?}` form would
                        // force `K: Debug` and fail to build (E0277). For such a key
                        // emit a key-less message; the `KeyError\0` payload prefix is
                        // preserved so `try/except KeyError` still matches. A concrete
                        // key keeps the value-bearing message byte-for-byte.
                        if crate::typeck::ty_contains_typevar(k) {
                            format!("({}.get(&{}).cloned().unwrap_or_else(|| panic!(\"KeyError\\0<key>\")))", o, i)
                        } else {
                            format!("({}.get(&{}).cloned().unwrap_or_else(|| panic!(\"KeyError\\0{{:?}}\", &{})))", o, i, i)
                        }
                    }
                    Ty::Str => {
                        // String indexing with negative index support.
                        // Explicit bounds check emits "IndexError\0..." so the
                        // try/except dispatcher can catch it as IndexError.
                        //
                        // The index expression is bound ONCE to `__i_idx: i64`
                        // before use. This is required for correctness when the
                        // index is itself a *block* expression (e.g. a nested list
                        // subscript `s[xs[i]]`, which lowers to `{ ... }`): inlining
                        // the raw block at each `{} < 0` / `+ {}` / `{} as usize`
                        // site produces unparenthesized `{ block } as usize`, which
                        // is a Rust parse error ("expected expression, found `as`").
                        // Binding also evaluates a side-effecting index (e.g. a call
                        // `s[f()]`) exactly once instead of three times.
                        format!(
                            "{{ let __chars: Vec<char> = {}.chars().collect(); let __i_idx: i64 = {}; let __idx = if __i_idx < 0 {{ ((__chars.len() as i64) + __i_idx) as usize }} else {{ __i_idx as usize }}; if __idx >= __chars.len() {{ panic!(\"IndexError\\0string index out of range\") }}; __chars[__idx].to_string() }}",
                            o, i
                        )
                    }
                    // (W5-a) `b[i]` -> int: byte-offset index over the natural
                    // `Vec<u8>` (neg-norm + catchable IndexError), the u8 widened
                    // to i64. OPPOSITE to the `Str` arm's char path above.
                    Ty::Bytes => {
                        format!("__py_bytes_index(&({}), {})", o, i)
                    }
                    _ => {
                        // List indexing with negative index support.
                        // Explicit bounds check emits "IndexError\0..." so the
                        // try/except dispatcher can catch it as IndexError.
                        //
                        // FAST PATH: when the base is a borrowable place and the
                        // index cannot mutate it, borrow the container and clone
                        // only the element (O(1) vs cloning the whole Vec). The
                        // `len(self.items) - 1` form (peek) qualifies: `len()` only
                        // shared-borrows, compatible with the `&base` read borrow.
                        if self.is_borrowable_place(obj) && self.is_borrow_safe_index(idx) {
                            format!("__py_list_get(&{}, {})", o, i)
                        } else {
                            // FALLBACK: clone the base into `__list` FIRST (so a
                            // mutating/moving index still compiles), then bounds-
                            // check and clone the element. The index is bound ONCE
                            // to `__i_idx: i64` — see the Str arm above for why
                            // (nested-subscript parse error + single-evaluation of
                            // side-effecting indices).
                            format!(
                                "{{ let __list = {}.clone(); let __i_idx: i64 = {}; let __idx = if __i_idx < 0 {{ ((__list.len() as i64) + __i_idx) as usize }} else {{ __i_idx as usize }}; if __idx >= __list.len() {{ panic!(\"IndexError\\0list index out of range\") }}; __list[__idx].clone() }}",
                                o, i
                            )
                        }
                    }
                }
            }
            Expr::Slice { obj, start, stop, step, .. } => {
                let obj_ty = self.type_of_expr(obj);
                let o = self.emit_expr(obj)?;

                match obj_ty {
                    Ty::Str => {
                        // Every string slice (with or without an explicit step)
                        // routes through __py_str_slice_step, which does all the
                        // negative-index resolution, CPython PySlice_AdjustIndices
                        // clamping, and step-directional (char-based) fill. Absent
                        // start/stop pass as `None` so the helper can apply the
                        // step-sign-dependent default at runtime; an absent step is
                        // the literal 1. Borrow the base when it is a place and no
                        // present bound can mutate it (the helper needs only `&str`);
                        // otherwise snapshot-clone the base into a local and call the
                        // SAME helper, so borrow and fallback agree exactly.
                        let start_arg = start.as_ref().map(|e| self.emit_expr(e)).transpose()?
                            .map(|s| format!("Some({})", s)).unwrap_or_else(|| "None".to_string());
                        let stop_arg = stop.as_ref().map(|e| self.emit_expr(e)).transpose()?
                            .map(|s| format!("Some({})", s)).unwrap_or_else(|| "None".to_string());
                        let step_arg = step.as_ref().map(|e| self.emit_expr(e)).transpose()?
                            .unwrap_or_else(|| "1i64".to_string());
                        let subs_safe = start.as_ref().map_or(true, |e| self.is_borrow_safe_index(e))
                            && stop.as_ref().map_or(true, |e| self.is_borrow_safe_index(e))
                            && step.as_ref().map_or(true, |e| self.is_borrow_safe_index(e));
                        if self.is_borrowable_place(obj) && subs_safe {
                            return Ok(format!("__py_str_slice_step(&{}, {}, {}, {})", o, start_arg, stop_arg, step_arg));
                        }
                        return Ok(format!("{{ let __s = {}.clone(); __py_str_slice_step(&__s, {}, {}, {}) }}", o, start_arg, stop_arg, step_arg));
                    }
                    // (W5-a) `b[i:j]` -> bytes. A `bytes` is a `Vec<u8>`, so it
                    // reuses the SAME generic `__py_list_slice`/`__py_list_slice_step`
                    // helpers (both `<T: Clone>`) — a sub-`Vec<u8>`, never `str`'s
                    // char-offset path.
                    Ty::List(_) | Ty::Bytes => {
                        // List slicing with step support and negative index handling
                        match (start, stop, step) {
                            (Some(s), Some(e), None) => {
                                // Simple: x[start:stop]. __py_list_slice resolves the
                                // negative bounds, clamps to [0,len], and empties when
                                // stop <= start (no usize underflow on out-of-range).
                                let start_s = self.emit_expr(s)?;
                                let stop_s = self.emit_expr(e)?;
                                // Borrow the base when it is a place and neither
                                // bound can mutate it; else snapshot-clone and call
                                // the SAME helper so both paths agree exactly.
                                if self.is_borrowable_place(obj)
                                    && self.is_borrow_safe_index(s)
                                    && self.is_borrow_safe_index(e)
                                {
                                    format!("__py_list_slice(&{}, {}, {})", o, start_s, stop_s)
                                } else {
                                    format!(
                                        "{{ let __list = {}.clone(); __py_list_slice(&__list, {}, {}) }}",
                                        o, start_s, stop_s
                                    )
                                }
                            }
                            _ => {
                                // General (stepped / one-sided) slice. All bound
                                // handling lives in __py_list_slice_step; absent
                                // start/stop pass as `None` so the helper applies the
                                // step-sign-dependent default at runtime, and an
                                // absent step is the literal 1.
                                let start_arg = start.as_ref().map(|e| self.emit_expr(e)).transpose()?
                                    .map(|s| format!("Some({})", s)).unwrap_or_else(|| "None".to_string());
                                let stop_arg = stop.as_ref().map(|e| self.emit_expr(e)).transpose()?
                                    .map(|s| format!("Some({})", s)).unwrap_or_else(|| "None".to_string());
                                let step_val = step.as_ref().map(|e| self.emit_expr(e)).transpose()?
                                    .unwrap_or_else(|| "1i64".to_string());

                                // Borrow the base when it is a place and no present
                                // bound (start/stop/step) can mutate it; else
                                // snapshot-clone and call the SAME helper.
                                let subs_safe = start.as_ref().map_or(true, |e| self.is_borrow_safe_index(e))
                                    && stop.as_ref().map_or(true, |e| self.is_borrow_safe_index(e))
                                    && step.as_ref().map_or(true, |e| self.is_borrow_safe_index(e));
                                if self.is_borrowable_place(obj) && subs_safe {
                                    format!("__py_list_slice_step(&{}, {}, {}, {})", o, start_arg, stop_arg, step_val)
                                } else {
                                    format!(
                                        "{{ let __list = {}.clone(); __py_list_slice_step(&__list, {}, {}, {}) }}",
                                        o, start_arg, stop_arg, step_val
                                    )
                                }
                            }
                        }
                    }
                    _ => return Err(crate::diag::Error::Codegen(format!("slicing not supported for type {}", obj_ty))),
                }
            }
            Expr::BinOp { op, lhs, rhs, span } => {
                // Try constant folding first
                if let Some(folded) = try_fold_const(&Expr::BinOp {
                    op: *op,
                    lhs: lhs.clone(),
                    rhs: rhs.clone(),
                    span: *span,
                }) {
                    return self.emit_expr(&folded);
                }

                // Handle sequence repetition: "abc" * 3 and [0] * 5
                if *op == BinOp::Mul {
                    let lt = self.type_of_expr(lhs);
                    let rt = self.type_of_expr(rhs);
                    if lt == Ty::Str || rt == Ty::Str {
                        let (str_e, num_e) = if lt == Ty::Str { (lhs, rhs) } else { (rhs, lhs) };
                        let s = self.emit_expr(str_e)?;
                        let n = self.emit_expr(num_e)?;
                        // (W5-a fix) CLAMP a negative count to 0 — CPython `"ab" * -1
                        // == ""`. A bare `n as usize` wraps a negative i64 to a huge
                        // usize and `<str>::repeat` then panics "capacity overflow"
                        // (a crash on VALID Python). `(n as i64).max(0)` keeps a
                        // legitimate positive count untouched.
                        return Ok(format!("{}.repeat(({} as i64).max(0) as usize)", s, n));
                    }
                    if matches!(&lt, Ty::List(_)) || matches!(&rt, Ty::List(_)) {
                        let (lst_e, num_e) = if matches!(&lt, Ty::List(_)) { (lhs, rhs) } else { (rhs, lhs) };
                        let v = self.emit_expr(lst_e)?;
                        let n = self.emit_expr(num_e)?;
                        return Ok(format!(
                            "{{ let mut __rep: Vec<_> = Vec::new(); for _ in 0..({} as usize) {{ __rep.extend({}.clone().into_iter()); }} __rep }}",
                            n, v
                        ));
                    }
                    // (W5-a) bytes * int / int * bytes -> repeat the byte sequence.
                    // `<[u8]>::repeat` (u8 is Copy) yields a fresh owned Vec<u8>.
                    if matches!(&lt, Ty::Bytes) || matches!(&rt, Ty::Bytes) {
                        let (b_e, num_e) = if matches!(&lt, Ty::Bytes) { (lhs, rhs) } else { (rhs, lhs) };
                        let b = self.emit_expr(b_e)?;
                        let n = self.emit_expr(num_e)?;
                        // (W5-a fix) CLAMP a negative count to 0 — CPython `b'ab' *
                        // -1 == b''`. `<[u8]>::repeat(n as usize)` on a negative i64
                        // wraps to a huge usize and panics "capacity overflow" (a
                        // crash on VALID Python), same idiom as the str repeat above.
                        return Ok(format!("({}).repeat(({} as i64).max(0) as usize)", b, n));
                    }
                }

                // Handle division - always returns float in Python.
                // (Generics v2 does NOT admit `/` on a bare `T`: pyrst `/` is true
                // float division, which Rust's `Div` does not reproduce for an
                // integer `T` — so typeck rejects `T / T` and only concrete
                // operands ever reach here.)
                if *op == BinOp::Div {
                    let l = self.emit_expr(lhs)?;
                    let r = self.emit_expr(rhs)?;
                    // Convert both operands to f64 for division
                    return Ok(format!("(({} as f64) / ({} as f64))", l, r));
                }

                // Handle set operations: union, intersection, difference
                let lt = self.type_of_expr(lhs);
                let rt = self.type_of_expr(rhs);
                if matches!(&lt, Ty::Set(_)) || matches!(&rt, Ty::Set(_)) {
                    let l = self.emit_expr(lhs)?;
                    let r = self.emit_expr(rhs)?;
                    match op {
                        BinOp::BitOr => {
                            // Set union: s1 | s2
                            return Ok(format!("{{ let mut __union = {}.clone(); __union.extend({}.iter().cloned()); __union }}", l, r));
                        }
                        BinOp::BitAnd => {
                            // Set intersection: s1 & s2
                            return Ok(format!("{{ let mut __inter = std::collections::HashSet::new(); for __x in {}.iter() {{ if {}.contains(__x) {{ __inter.insert(__x.clone()); }} }} __inter }}", l, r));
                        }
                        BinOp::BitXor => {
                            // Set symmetric difference: s1 ^ s2
                            return Ok(format!("{{ let mut __diff = {}.clone(); for __x in {}.iter() {{ if __diff.contains(__x) {{ __diff.remove(__x); }} else {{ __diff.insert(__x.clone()); }} }} __diff }}", l, r));
                        }
                        BinOp::Sub => {
                            // Set difference: s1 - s2
                            return Ok(format!("{{ let mut __diff = {}.clone(); for __x in {}.iter() {{ __diff.remove(__x); }} __diff }}", l, r));
                        }
                        _ => {}
                    }
                }

                // Handle string concatenation: str + str needs special handling
                if *op == BinOp::Add {
                    let lt = self.type_of_expr(lhs);
                    let rt = self.type_of_expr(rhs);
                    if lt == Ty::Str || rt == Ty::Str {
                        let l = self.emit_expr(lhs)?;
                        let r = self.emit_expr(rhs)?;
                        // Use format! for robust string concatenation
                        return Ok(format!(r#"format!("{{}}{{}}", {}, {})"#, l, r));
                    }
                    // (W5-a) bytes + bytes -> concatenation into a fresh Vec<u8>.
                    // `[&a[..], &b[..]].concat()` borrows both (no clone) and owns
                    // the result. typeck has already rejected every mixed `+`.
                    if matches!(lt, Ty::Bytes) && matches!(rt, Ty::Bytes) {
                        let l = self.emit_expr(lhs)?;
                        let r = self.emit_expr(rhs)?;
                        return Ok(format!("[&({})[..], &({})[..]].concat()", l, r));
                    }
                    // (EPIC-5 C2-3) `list + list` concatenation is a PRE-EXISTING
                    // gap: typeck accepts it, but the generic numeric `+` lowering
                    // below emits `vec![..] + vec![..]`, and Rust's `Vec` has no
                    // `Add` impl — so it leaked a raw rustc E0369 (a miscompile,
                    // for ANY element type, not just subtypes). Refuse honestly
                    // here rather than emit invalid Rust; the documented workaround
                    // is `.extend()` / a comprehension. (Element-wise subtype
                    // wrapping for a base-typed result is the follow-on once concat
                    // itself is implemented.) NOT an EPIC-4 path.
                    if matches!(lt, Ty::List(_)) && matches!(rt, Ty::List(_)) {
                        return Err(crate::diag::Error::Codegen(
                            "list `+` list concatenation is not yet supported — \
                             build the combined list with `.extend()` (e.g. \
                             `xs.extend(ys)`) or a comprehension instead".into(),
                        ));
                    }
                }

                // Handle `x is None` / `x is not None` → .is_none() / .is_some()
                if matches!(op, BinOp::Is | BinOp::IsNot) && matches!(rhs.as_ref(), Expr::None_(_)) {
                    let l = self.emit_expr(lhs)?;
                    return Ok(match op {
                        BinOp::Is => format!("{}.is_none()", l),
                        BinOp::IsNot => format!("{}.is_some()", l),
                        _ => unreachable!(),
                    });
                }

                // (card cc7ae370, item 3) A class-typed arithmetic dunder
                // (`+`/`-`/`*`) lowers to a `std::ops::{Add,Sub,Mul}` impl whose
                // method takes BOTH operands BY VALUE, moving them. A bare
                // `h + h2` would therefore consume `h`/`h2`, so reusing either
                // afterward is E0382 — breaking Python value semantics. Route each
                // operand through emit_consuming (the single ownership-decision
                // point): a non-Copy class PLACE (Ident / plain field chain) is
                // `.clone()`d so the original stays usable, while a fresh owned
                // rvalue (call/ctor result, literal, nested BinOp temp) is emitted
                // bare — cloning it would double-clone. Comparisons (`==`/`<`, via
                // PartialEq/PartialOrd) borrow their operands, so they are excluded
                // and stay on the generic path unchanged.
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
                    && matches!(self.type_of_expr(lhs), Ty::Class(..))
                {
                    let l = self.emit_consuming(lhs)?;
                    let r = self.emit_consuming(rhs)?;
                    let op_s = match op {
                        BinOp::Add => "+",
                        BinOp::Sub => "-",
                        BinOp::Mul => "*",
                        _ => unreachable!(),
                    };
                    return Ok(format!("({} {} {})", l, op_s, r));
                }
                let l = self.emit_expr(lhs)?;
                let r = self.emit_expr(rhs)?;

                // Get types for numeric type conversion
                let lt = self.type_of_expr(lhs);
                let rt = self.type_of_expr(rhs);

                match op {
                    BinOp::Pow => {
                        // int ** int -> integer power (matches type_of_expr inferring Int);
                        // any float operand -> float power. Use the __py_ipow helper for
                        // the integer case so a negative exponent panics with a clear
                        // message instead of silently wrapping `as u32` to a huge value.
                        if matches!(lt, Ty::Int) && matches!(rt, Ty::Int) {
                            return Ok(format!("__py_ipow(({}), ({}))", l, r));
                        }
                        return Ok(format!("(({} as f64).powf({} as f64))", l, r));
                    }
                    BinOp::In => {
                        // Use contains_key for dicts, contains for lists/sets
                        let contains_method = match rt {
                            Ty::Dict(_, _) => format!("{}.contains_key(&{})", r, l),
                            Ty::List(_) => format!("{}.iter().any(|__x| __x == &{})", r, l),
                            Ty::Set(_) => format!("{}.contains(&{})", r, l),
                            // (W5-b) `x in b` — `int in bytes` is a RANGE-CHECKED
                            // byte-value test (256/-1 raise a catchable ValueError);
                            // `bytes in bytes` is a subsequence search. typeck
                            // rejected `str in bytes`, so only Int/Bytes lhs reach here.
                            Ty::Bytes => match lt {
                                Ty::Int => format!("__py_int_in_bytes({}, &({}))", l, r),
                                _ => format!("__py_bytes_contains(&({}), &({}))", l, r),
                            },
                            _ => format!("{}.contains(&{})", r, l),
                        };
                        return Ok(contains_method);
                    }
                    BinOp::NotIn => {
                        // Use !contains_key for dicts, !contains for lists/sets
                        let contains_method = match rt {
                            Ty::Dict(_, _) => format!("!{}.contains_key(&{})", r, l),
                            Ty::List(_) => format!("!{}.iter().any(|__x| __x == &{})", r, l),
                            Ty::Set(_) => format!("!{}.contains(&{})", r, l),
                            // (W5-b) negated bytes membership (see BinOp::In).
                            Ty::Bytes => match lt {
                                Ty::Int => format!("!__py_int_in_bytes({}, &({}))", l, r),
                                _ => format!("!__py_bytes_contains(&({}), &({}))", l, r),
                            },
                            _ => format!("!{}.contains(&{})", r, l),
                        };
                        return Ok(contains_method);
                    }
                    BinOp::FloorDiv => {
                        // Python `//` floors toward negative infinity; Rust integer `/`
                        // truncates toward zero and Rust float `/` does not floor at all.
                        // For integer operands use __py_floordiv which also panics on /0
                        // with a catchable ZeroDivisionError payload.
                        // For float operands keep the f64 path (float //0.0 -> INF in
                        // Python is also a ZeroDivisionError but lower-priority; noted as
                        // a known gap).
                        let is_float = matches!(lt, Ty::Float) || matches!(rt, Ty::Float);
                        if is_float {
                            return Ok(format!("((({} as f64) / ({} as f64)).floor())", l, r));
                        }
                        return Ok(format!("__py_floordiv(({}), ({}))", l, r));
                    }
                    BinOp::Mod => {
                        // Python `%` returns a result with the sign of the divisor; Rust
                        // `%` returns the sign of the dividend. Use the divisor-signed
                        // helper for ints (single evaluation), rem_euclid-style for floats.
                        let is_float = matches!(lt, Ty::Float) || matches!(rt, Ty::Float);
                        if is_float {
                            // (card 2f615b52) CPython fmod-based float modulo — see
                            // __py_fmod. The old `(((a%b)+b)%b)` double-rounded
                            // (`0.1 % 1.0` -> 0.10000000000000009 instead of 0.1) and
                            // returned a silent NaN on a zero divisor.
                            return Ok(format!("__py_fmod(({}) as f64, ({}) as f64)", l, r));
                        }
                        return Ok(format!("__py_mod(({}), ({}))", l, r));
                    }
                    _ => {
                        let op_s = match op {
                            BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*",
                            BinOp::Div => "/",
                            BinOp::Eq => "==", BinOp::Ne => "!=",
                            BinOp::Lt => "<", BinOp::Le => "<=",
                            BinOp::Gt => ">", BinOp::Ge => ">=",
                            BinOp::And => "&&", BinOp::Or => "||",
                            BinOp::Is => "==", BinOp::IsNot => "!=",
                            BinOp::BitAnd => "&", BinOp::BitOr => "|", BinOp::BitXor => "^",
                            BinOp::LShift => "<<", BinOp::RShift => ">>",
                            BinOp::In | BinOp::NotIn => unreachable!(), // handled above
                            BinOp::Pow => unreachable!(), // handled above
                            BinOp::FloorDiv | BinOp::Mod => unreachable!(), // handled above
                        };

                        // Handle numeric type promotion: int op float -> convert to float
                        // Also handle cases where type inference failed (Unknown) but we know one side is float
                        let (l_final, r_final) = match (&lt, &rt) {
                            // int op float -> promote both to float
                            (Ty::Int, Ty::Float) => (format!("({} as f64)", l), format!("({})", r)),
                            // float op int -> promote both to float
                            (Ty::Float, Ty::Int) => (format!("({})", l), format!("({} as f64)", r)),
                            // Unknown op float -> try to promote Unknown as int/numeric
                            (Ty::Unknown, Ty::Float) => (format!("({} as f64)", l), format!("({})", r)),
                            // float op Unknown -> try to promote Unknown as int/numeric
                            (Ty::Float, Ty::Unknown) => (format!("({})", l), format!("({} as f64)", r)),
                            // Both same type or non-numeric: no conversion
                            _ => (l, r),
                        };

                        format!("({} {} {})", l_final, op_s, r_final)
                    }
                }
            }
            Expr::UnOp { op, expr, span } => {
                // Try constant folding first
                if let Some(folded) = try_fold_const(&Expr::UnOp {
                    op: *op,
                    expr: expr.clone(),
                    span: *span,
                }) {
                    return self.emit_expr(&folded);
                }

                // (card cc7ae370, item 3) `-x` on a class value invokes `__neg__`
                // (`std::ops::Neg::neg(self)`), which takes the operand BY VALUE
                // (move). Clone a non-Copy class PLACE via emit_consuming so the
                // original stays usable (value semantics); a fresh owned rvalue
                // (call/BinOp temp, e.g. `-(a + b)`) is emitted bare. `not`/`~`
                // are not value-consuming dunders, so they stay on emit_expr.
                let e = if matches!(op, UnOp::Neg) && matches!(self.type_of_expr(expr), Ty::Class(..)) {
                    self.emit_consuming(expr)?
                } else if matches!(op, UnOp::Not) {
                    // (card 18682938) `not obj` is a truthiness context: a
                    // class-with-`__bool__` operand lowers to `!obj.__bool__()`.
                    // emit_truthy emits the operand exactly once (no double-eval).
                    self.emit_truthy(expr)?
                } else {
                    self.emit_expr(expr)?
                };
                match op {
                    UnOp::Neg => format!("(-{})", e),
                    UnOp::Not => format!("(!{})", e),
                    UnOp::BitNot => format!("(!({}))", e),
                }
            }
            Expr::Lambda { params, body, .. } => {
                // Emit closure params WITHOUT a type annotation and let Rust infer
                // each param's type from the use site: the call-site argument for
                // an inline-invoked lambda `(lambda x: ...)(5)`, or the iterator
                // element type for a lambda passed to map()/filter(). Hardcoding
                // `: i64` was only correct for int iterables and broke e.g.
                // `map(lambda w: len(w), words)` over a list[str].
                // (EPIC-6) Escape each lambda param; the body references it via
                // emit_expr Ident (same escape), so `|r#type| r#type + 1` stays
                // consistent.
                let param_strs: Vec<String> = params.iter()
                    .map(|(name, _ty)| escape_ident(name))
                    .collect();
                // (card 97e02e09) Register each param with its ANNOTATED type (a bare
                // lambda's params are `Any`->Unknown, but a `Callable`-slot / map /
                // key lambda reaches other arms; this arm also serves inline-invoked
                // and annotated lambdas) so `type_of_expr` inside the body sees the
                // param type and a `Str` param's `a + b` routes through the statement
                // path's `format!("{}{}", ..)` lowering instead of the numeric `(a+b)`
                // that rustc rejects for `String + String`. Restored after the body.
                //
                // (card c34ac64a fix A) Each param is a FRESH binding that SHADOWS any
                // outer local / narrowing shadow of the same name for the body's
                // duration. scope_enter snapshots locals + shadow_map (+ declared);
                // scope_exit restores them. A BARE (Unknown) param must be REMOVED from
                // `locals` — otherwise an outer same-named local (e.g. `a: str`) bleeds
                // its type in and mis-lowers `a + b` as string concat (silent
                // miscompile: prints "34" for `f(3,4)`); after removal `type_of_expr`
                // falls back to inference and rustc types the param from the call site.
                // The `shadow_map` entry is dropped for EVERY param name so a live outer
                // narrowing shadow (unwrapped binding) never resolves a param read.
                // (card c34ac64a fix A) Consume any inline-invoke param-type hints
                // (`(lambda a,b: ..)(x,y)`), so a bare param pinned to a call arg's
                // type keeps it; `.take()` clears them so a nested lambda in the body
                // does not inherit them.
                let hints = self.pending_inline_lambda_params.take();
                let __lam_snap = self.scope_enter();
                for (name, ty) in params.iter() {
                    let pty = crate::typeck::lambda_param_ty(ty);
                    self.shadow_map.remove(name);
                    if !matches!(pty, Ty::Unknown) {
                        self.locals.insert(name.clone(), pty);
                    } else if let Some(hint) = hints.as_ref().and_then(|h| h.get(name)) {
                        self.locals.insert(name.clone(), hint.clone());
                    } else {
                        self.locals.remove(name);
                    }
                }
                let body_s = self.emit_expr(body)?;
                self.scope_exit(__lam_snap);
                format!("|{}| {}", param_strs.join(", "), body_s)
            }
            Expr::IfExp { test, body, orelse, .. } => {
                // Python's `body if test else orelse` -> Rust's if-expression.
                // (card c34ac64a fix C) The ternary TEST is a truthiness context —
                // a class-with-`__bool__` lowers via `.__bool__()` (was E0308).
                let t = self.emit_truthy(test)?;
                let b = self.emit_expr(body)?;
                let o = self.emit_expr(orelse)?;
                format!("(if {} {{ {} }} else {{ {} }})", t, b, o)
            }
        })
    }

    /// Lower a `match`'s arms to an `if`/`else if`/`else` chain over the owned
    /// scrutinee temp `match_val` (`__match_val`). `is_first` is true at the head of
    /// the chain (emit `if`) and false for a continuation (emit `else`/`else if`).
    ///
    /// Pattern semantics:
    ///  * A `Wildcard` (`case _:`) or `Capture` (`case y:`) with NO guard ALWAYS
    ///    matches and makes the match exhaustive — it is lowered as a real `else`
    ///    (or an UNCONDITIONAL body when it heads the chain), never as `if true {}`
    ///    with no else (which let a non-unit function fall off the end -> rustc
    ///    E0317). Any arms after it are unreachable and are dropped.
    ///  * A `Capture(name)` BINDS `name` to the subject for its guard AND body. A
    ///    GUARDED capture (`case y if g:`) therefore needs `name` in scope before
    ///    the guard is tested, so it lowers to a nested block
    ///    `{ let name = mv.clone(); if g { body } else { <rest> } }` — the binding
    ///    precedes the guard, and the remaining arms continue in the `else`.
    ///  * A `Literal`/`Or` arm (optionally guarded) tests `emit_pattern_cond`
    ///    (`&& guard`) and chains the rest into the following `else if`.
    pub(crate) fn emit_match_arms(
        &mut self,
        match_val: &str,
        arms: &[crate::ast::MatchArm],
        is_first: bool,
        subj_ty: &Ty,
    ) -> Result<()> {
        use crate::ast::MatchPattern;
        let Some((arm, rest)) = arms.split_first() else {
            return Ok(()); // no arms left to emit
        };

        let capture_name = match &arm.pattern {
            MatchPattern::Capture(n) => Some(n.clone()),
            _ => None,
        };
        let is_catchall = matches!(
            &arm.pattern,
            MatchPattern::Wildcard | MatchPattern::Capture(_)
        );
        let always_matches = arm.guard.is_none() && is_catchall;

        // (1) Unguarded catchall — always matches, exhaustive. Drop unreachable rest.
        if always_matches {
            if is_first {
                // No `if`/`else` wrapper: emit the body unconditionally.
                self.emit_match_arm_body(match_val, capture_name.as_deref(), &arm.body, subj_ty)?;
            } else {
                self.line("} else {");
                self.emit_match_arm_body(match_val, capture_name.as_deref(), &arm.body, subj_ty)?;
                self.line("}");
            }
            return Ok(());
        }

        // (2) Guarded CAPTURE — bind the name before the guard, then test the guard,
        // with the remaining arms continuing in the `else`. Lowered via a nested
        // block so the binding scopes over the guard and body.
        if let (Some(name), Some(guard)) = (&capture_name, &arm.guard) {
            if is_first {
                self.line("{");
            } else {
                self.line("} else {");
            }
            self.indent += 1;
            let bind = escape_ident(name);
            self.line(&format!("let mut {} = {}.clone();", bind, match_val));
            self.declared.insert(name.clone());
            // (W0-b) Record the capture as a LOCAL of the subject's type so a read
            // of it in the guard/body resolves to this binding — not to a
            // same-named module constant (`const_names`-vs-`locals` resolution in
            // emit_expr's Ident arm). Save/restore around the whole guarded block
            // so it does not leak past the arm.
            let cap_saved = self.locals.insert(name.clone(), subj_ty.clone());
            let g = self.emit_expr(guard)?;
            self.line(&format!("if {} {{", g));
            self.indent += 1;
            let __arm_scope = self.scope_enter();
            for s in &arm.body {
                self.emit_stmt(s)?;
            }
            self.scope_exit(__arm_scope);
            self.indent -= 1;
            if rest.is_empty() {
                self.line("}");
            } else {
                // Continue the remaining arms inside this guard's `else`.
                self.emit_match_arms(match_val, rest, false, subj_ty)?;
            }
            match cap_saved {
                Some(t) => { self.locals.insert(name.clone(), t); }
                None => { self.locals.remove(name.as_str()); }
            }
            self.indent -= 1;
            self.line("}");
            return Ok(());
        }

        // (3) Literal / Or / guarded-non-capture arm: a discriminating test.
        let cond = self.emit_pattern_cond(match_val, &arm.pattern)?;
        let guard_str = if let Some(guard) = &arm.guard {
            let g = self.emit_expr(guard)?;
            format!(" && {}", g)
        } else {
            String::new()
        };
        if is_first {
            self.line(&format!("if {}{} {{", cond, guard_str));
        } else {
            self.line(&format!("}} else if {}{} {{", cond, guard_str));
        }
        self.indent += 1;
        let __arm_scope = self.scope_enter();
        for s in &arm.body {
            self.emit_stmt(s)?;
        }
        self.scope_exit(__arm_scope);
        self.indent -= 1;
        if rest.is_empty() {
            self.line("}");
        } else {
            self.emit_match_arms(match_val, rest, false, subj_ty)?;
        }
        Ok(())
    }

    /// Emit a match arm body INDENTED one level, preceded by a capture binding
    /// when the arm pattern is `case <name>:`. `match_val` is the owned scrutinee
    /// temp (`__match_val`); a `Capture` binds `let <name> = __match_val.clone();`
    /// so the body sees the subject value under `<name>` (a `.clone()` keeps the
    /// scrutinee usable for sibling arms and matches pyrst's clone-on-use; it is a
    /// no-op move for `Copy` subjects and a real clone otherwise).
    pub(crate) fn emit_match_arm_body(
        &mut self,
        match_val: &str,
        capture_name: Option<&str>,
        body: &[crate::ast::Stmt],
        subj_ty: &Ty,
    ) -> Result<()> {
        self.indent += 1;
        // (card 575bcf3a) Isolate the arm body's block-scope emission state; the
        // capture binding below is captured inside this window.
        let __arm_scope = self.scope_enter();
        if let Some(name) = capture_name {
            let bind = escape_ident(name);
            self.line(&format!("let mut {} = {}.clone();", bind, match_val));
            // The binding is a `let`-declared local for the rest of this arm.
            self.declared.insert(name.to_string());
            // (W0-b) Record it in `locals` with the subject's type so a read of the
            // capture inside the body resolves to THIS local — not to a same-named
            // module constant (emit_expr's Ident arm prefers a `const_names` entry
            // only when the name is absent from `locals`). Previously a `case M:`
            // capturing a const-named subject read the stale const (silent wrong
            // output). `scope_exit` below restores `locals`, so it does not leak.
            self.locals.insert(name.to_string(), subj_ty.clone());
        }
        for s in body {
            self.emit_stmt(s)?;
        }
        self.scope_exit(__arm_scope);
        self.indent -= 1;
        Ok(())
    }

    pub(crate) fn emit_pattern_cond(&self, var: &str, pattern: &crate::ast::MatchPattern) -> Result<String> {
        use crate::ast::MatchPattern;
        match pattern {
            MatchPattern::Wildcard => Ok("true".to_string()),
            MatchPattern::Capture(_) => Ok("true".to_string()),
            MatchPattern::Literal(Expr::Int(n, _)) => {
                Ok(format!("{} == {}i64", var, n))
            }
            MatchPattern::Literal(Expr::Bool(b, _)) => {
                Ok(format!("{} == {}", var, b))
            }
            MatchPattern::Literal(Expr::Str(s, _)) => {
                Ok(format!("{} == {:?}", var, s))
            }
            MatchPattern::Literal(Expr::None_(_)) => {
                Ok(format!("{} == None", var))
            }
            MatchPattern::Literal(_) => {
                Ok("true".to_string())
            }
            MatchPattern::Or(patterns) => {
                let conds: Result<Vec<String>> = patterns.iter()
                    .map(|p| self.emit_pattern_cond(var, p))
                    .collect();
                let conds = conds?;
                Ok(format!("({})", conds.join(" || ")))
            }
        }
    }

    pub(crate) fn line(&mut self, s: &str) {
        for _ in 0..self.indent { self.out.push_str("    "); }
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// Fold the declaration-hoisting DOUBLE-INIT artifact: a hoisted local is
    /// emitted at the top of the function as `let mut x: T = <default>;`, and its
    /// first (unconditional, top-level) assignment then re-emits as `x = <init>;`
    /// — two writes where one suffices. When the immediately-preceding emitted
    /// line is EXACTLY this variable's hoisted default declaration (same name,
    /// same type, same indent = same block, adjacent = nothing between), splice
    /// the real initializer straight into that `let` and skip the separate
    /// assignment. Fires only when the discarded initializer is the pure
    /// `default_val` (no side effect dropped) and only on true emitted adjacency,
    /// so it is exactly the card's "immediately-next statement in the same block"
    /// rule and cannot reorder or drop any effect. Returns true iff it folded.
    ///
    /// NOTE: this handles the SINGLE-hoist / adjacent case. When 2+ locals are
    /// hoisted, the sorted decl preamble separates all but the last-sorted var's
    /// decl from the buffer tail, so those do not fold and keep the double-init.
    /// The residual `unused_assignments` on that dead default is suppressed by the
    /// emitted `#![allow(..)]` header (see `emit_program`) — the order-independent
    /// completeness fix for card adc0d1c4; a name-based multi-hoist splice was
    /// prototyped but is order-sensitive (a later-sorting var folded first blocks
    /// the earlier ones), so it was not adopted.
    pub(crate) fn try_fold_hoisted_init(&mut self, target_e: &str, ty: &Ty, new_rhs: &str) -> bool {
        let def = match self.default_val(ty) {
            Some(d) => d,
            None => return false,
        };
        let rust_ty = self.rust_ty(ty);
        let indent = "    ".repeat(self.indent);
        let expected = format!("{}let mut {}: {} = {};\n", indent, target_e, rust_ty, def);
        if self.out.ends_with(&expected) {
            let keep = self.out.len() - expected.len();
            self.out.truncate(keep);
            self.line(&format!("let mut {}: {} = {};", target_e, rust_ty, new_rhs));
            true
        } else {
            false
        }
    }

    /// Maps a pyrst `Ty` to its emitted Rust type text.
    ///
    /// (EPIC-5 C2-1) This is a `Codegen` METHOD (not a free fn) specifically so
    /// the `Class` arm can consult `self.poly_map` via `is_polymorphic_base` —
    /// the method form avoids threading a `poly_map` parameter through every one
    /// of the call sites (emit_func params/returns, emit_class fields/dunder
    /// impls, emit_stmt hoists). See design §F. C2-1 is BEHAVIOR-PRESERVING: the
    /// `Class` arm still returns plain `n` for every class; the single marked
    /// hook below is what C2-2 flips to `format!("{n}__")` for a polymorphic base.
    pub(crate) fn rust_ty(&self, t: &Ty) -> String {
        match t {
            Ty::Int => "i64".into(),
            Ty::Float => "f64".into(),
            Ty::Bool => "bool".into(),
            Ty::Str => "String".into(),
            // (W5-a) `bytes` -> `Vec<u8>` — the same ownership shape as `list`.
            Ty::Bytes => "Vec<u8>".into(),
            Ty::Unit => "()".into(),
            // The `None` literal's type. It never appears as a real binding
            // annotation (annotations come from `from_type_expr`, which yields
            // `Unit`/`Option`, never `NoneVal`); this arm exists for
            // exhaustiveness and mirrors `Unit` (`None` as a bare value is an
            // upstream type error).
            Ty::NoneVal => "()".into(),
            Ty::List(inner) => format!("Vec<{}>", self.rust_ty(inner)),
            // LAZY-GEN V1-b: a generator lowers to the lazy `__PyrstGen<T>`
            // coroutine (docs/design/lazy-generators.md §C.1). `__PyrstGen<T>` is a
            // concrete, nameable struct that `impl`s `Iterator<Item = T>`, so this
            // emission is uniform across return / param / field / local positions
            // (the reason it is a named struct rather than `impl Iterator`). V1-d
            // renamed it under the reserved `__Pyrst` prefix (collision-proof).
            Ty::Iterator(inner) => format!("__PyrstGen<{}>", self.rust_ty(inner)),
            Ty::Set(inner) => format!("::std::collections::HashSet<{}>", self.rust_ty(inner)),
            Ty::Dict(k, v) => format!("::std::collections::HashMap<{}, {}>", self.rust_ty(k), self.rust_ty(v)),
            Ty::Tuple(parts) => {
                let inner = parts.iter().map(|p| self.rust_ty(p)).collect::<Vec<_>>().join(", ");
                if parts.len() == 1 {
                    format!("({},)", inner)
                } else {
                    format!("({})", inner)
                }
            }
            Ty::Option(inner) => format!("Option<{}>", self.rust_ty(inner)),
            // A first-class function value lowers to a reference-counted boxed
            // closure `Rc<dyn Fn(A, B) -> R>`. `Rc` is `Clone`, so it round-trips
            // through pyrst's value semantics (clone-on-use = a cheap refcount
            // bump that shares the same callable) and is storable in a list/dict,
            // passable as an argument, and returnable. A `() -> R` return is
            // omitted in Rust only for `()`, but writing `-> ()` is also valid and
            // keeps the formatting uniform.
            Ty::Func(args, ret) => {
                let arg_strs = args.iter().map(|a| self.rust_ty(a)).collect::<Vec<_>>().join(", ");
                format!("::std::rc::Rc<dyn Fn({}) -> {}>", arg_strs, self.rust_ty(ret))
            }
            Ty::Class(n, args) => {
                // Generics v2 (generic CLASSES): a parametrized instance type
                // `Ty::Class("Box", [Int])` lowers to `Box<i64>` — the class's
                // Rust struct/impl carry a `<T, ..>` clause (see `emit_class`), and
                // every arg position lowers recursively. A generic class is never a
                // polymorphic base (generic-class inheritance is out of scope), so
                // the two paths don't overlap; the args-empty branch below is the
                // unchanged legacy lowering (plain `n` / companion enum `n__`).
                if !args.is_empty() {
                    let arg_strs = args.iter().map(|a| self.rust_ty(a)).collect::<Vec<_>>().join(", ");
                    // (W3-2) Owner-qualify the generic class's name (root class stays
                    // bare, an imported one becomes `__pyrst_m_<owner>__<name>`).
                    return format!("{}<{}>", self.emit_class_name(n), arg_strs);
                }
                // (EPIC-5 C2-2b-i) Polymorphism activation. A class that is a
                // polymorphic base (has ≥1 subclass in this unit) lowers to its
                // companion enum `n__` — emitted by emit_companion_enum with
                // method/field/dunder dispatch — for EVERY param/return/field/
                // var/element position. A leaf or non-subclassed class stays its
                // plain value-struct `n`. The C2-2b-i wrapping (at the 3 former
                // gate sites + list literals) and field-read lowering keep the
                // emitted Rust well-typed against this `n__` slot.
                if self.is_polymorphic_base(n) {
                    // (W3-2) Companion enum type: the base's owner-qualified name +
                    // `__` (matches emit_companion_enum's `enum_name`).
                    format!("{}__", self.emit_class_name(n))
                } else {
                    // (W3-2) Plain value-struct type, owner-qualified (root = bare).
                    self.emit_class_name(n)
                }
            }
            // (W5-g) A handle lowers to its opaque Rust struct. The built-in `file`
            // handle is `PyFile` (FILE_PRELUDE); lib-declared handle kinds map their
            // name to the struct the `@extern class` decl emits (W5-h). An unknown
            // handle kind here would be a compiler bug (the typeck only ever mints
            // `Handle("file")` in W5-g), so fall back to a name that fails LOUDLY at
            // rustc rather than silently — but `file` is the only reachable kind.
            Ty::Handle(n) => {
                if n == "file" { "PyFile".into() } else { format!("__PyHandle_{}", n) }
            }
            // Generics v1: a bound type variable lowers to its bare Rust generic
            // parameter name (e.g. `T`). The enclosing `fn` declares it as
            // `<T: Clone>` (see `emit_func`), so the name is in scope wherever a
            // param/return/local of type `T` is emitted; monomorphization at the
            // call site fills it in.
            Ty::TypeVar(n) => n.clone(),
            Ty::Unknown => "()".into(),
        }
    }
}
