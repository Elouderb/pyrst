use super::*;

impl<'a> Codegen<'a> {
    pub(crate) fn emit_stmt(&mut self, s: &Stmt) -> Result<()> {
        match s {
            Stmt::Pass(_) => self.line("// pass"),
            Stmt::Break(_) => {
                // (try/except control flow) A `break` at the try-body loop level
                // must escape the catch_unwind closure (it targets the loop that
                // ENCLOSES the try); thread it out as a flow signal that the
                // surrounding lowering re-issues as a real `break` after finally.
                // Inside a nested loop the flag is suspended, so it emits a plain
                // Rust `break;` targeting that inner loop.
                if self.try_loopctl_escape {
                    self.line("return __PyrstTryFlow::Break;");
                } else {
                    self.line("break;");
                }
            }
            Stmt::Continue(_) => {
                if self.try_loopctl_escape {
                    self.line("return __PyrstTryFlow::Continue;");
                } else {
                    self.line("continue;");
                }
            }
            Stmt::Assert { cond, msg, .. } => {
                let c = self.emit_expr(cond)?;
                match msg {
                    Some(m) => {
                        let m_s = self.emit_expr(m)?;
                        self.line(&format!("assert!({}, \"{{}}\", {});", c, m_s));
                    }
                    None => {
                        self.line(&format!("assert!({});", c));
                    }
                }
            }
            Stmt::Raise { exc, .. } => {
                match exc {
                    None => self.line("panic!(\"explicit raise\");"),
                    Some(Expr::Call { callee, args, .. }) => {
                        let exc_type = if let Expr::Ident(n, _) = callee.as_ref() {
                            n.clone()
                        } else {
                            "Exception".into()
                        };
                        if let Some(first_arg) = args.first() {
                            let msg = self.emit_expr(first_arg)?;
                            // Delimit type from message with a NUL byte: it cannot
                            // appear in pyrst user data, so a user message that itself
                            // contains the old " panic: " separator no longer mangles
                            // the type dispatch or the bound `as e` text. See the
                            // try/except dispatcher split for the consuming side.
                            self.line(&format!("panic!(\"{{}}\\0{{}}\", \"{}\", {});", exc_type, msg));
                        } else {
                            // No message: still use the "<Type>\0<msg>" payload format
                            // (empty message) so `except <Type>:` type-matching parses it.
                            self.line(&format!("panic!(\"{{}}\\0\", \"{}\");", exc_type));
                        }
                    }
                    Some(other) => {
                        let e = self.emit_expr(other)?;
                        self.line(&format!("panic!(\"{{}}\", {});", e));
                    }
                }
            }
            Stmt::Return(None, _) => {
                // (LAZY-GEN V1-b) In a generator — now the lazy `async move`
                // coroutine, whose `Output` is `()` — a bare `return` COMPLETES the
                // future: the `Gen` driver sees `Poll::Ready` and reports `None`,
                // ending iteration (Python StopIteration). The old eager
                // `return __pyrst_gen_acc;` is gone. yield-in-`try` is a V1
                // honest-error and no corpus generator carries an escaping `return`
                // inside a `catch_unwind` try body, so the plain `return;` is the
                // correct lowering for the async block. This stays DISTINCT from the
                // non-generator path (which threads a `return` out of a try body as
                // `__PyrstTryFlow::Return(())`); reading `self.in_generator` here
                // also keeps the flag's save/restore discipline (nested defs reset
                // it — see `emit_func`) load-bearing.
                if self.in_generator {
                    self.line("return;");
                } else if self.try_return_escape {
                    self.line("return __PyrstTryFlow::Return(());");
                } else {
                    self.line("return;");
                }
            }
            Stmt::Yield(e, _) => {
                // (LAZY-GEN V1-b) `yield x` suspends the coroutine: store the value
                // in the shared slot and `.await` the one-shot `YieldNow` future so
                // the `Gen` driver's `next()` observes `Poll::Pending` and takes it
                // out. `emit_consuming` deep-clones a non-Copy place (pyrst value
                // semantics) / passes a Copy element by value — a SINGLE clone; the
                // driver's `slot.take()` hands that owned value straight to the
                // consumer, so there is no second clone. The `.await` is valid
                // because the body is emitted inside the `async move` block (see
                // `emit_func`); a `yield` only ever appears in a generator body.
                let s = self.emit_consuming(e)?;
                self.line(&format!("__pyrst_gen_co.yield_({}).await;", s));
            }
            Stmt::Return(Some(e), _) => {
                // (EPIC-5) In an Option-returning function, wrap the value:
                // `None` -> `return None;`, a bare T -> `return Some(T);`, an
                // already-Optional value -> pass through.
                if matches!(self.current_ret_ty, Ty::Option(_)) {
                    // emit_consuming clones a non-Copy place (e.g. `return self.field`)
                    // before coerce_to_option wraps the result in `Some(..)`.
                    let s = self.emit_consuming(e)?;
                    let wrapped = self.coerce_to_option(s, e, &self.current_ret_ty);
                    // (try/except control flow) escape the value out of the try
                    // closure when emitting the try body; otherwise a plain return.
                    if self.try_return_escape {
                        self.line(&format!("return __PyrstTryFlow::Return({});", wrapped));
                    } else {
                        self.line(&format!("return {};", wrapped));
                    }
                } else if matches!(e, Expr::None_(_)) {
                    // `return None` in a non-Option function == a bare `return;`.
                    if self.try_return_escape {
                        self.line("return __PyrstTryFlow::Return(());");
                    } else {
                        self.line("return;");
                    }
                } else {
                    // (EPIC-5 C2-2b-i) `return dog` from a `-> Animal` function —
                    // a raw-struct value into a polymorphic-base `Animal__` return
                    // slot is WRAPPED in the right variant (replaces the C1 gate).
                    // (first-class functions) `return lambda x: x + n` /
                    // `return inc` from a `-> Callable[..]` function — wrap the
                    // lambda/name into the `Rc<dyn Fn>` return slot. Non-poly,
                    // non-func returns keep the uniform clone-on-use path: a
                    // non-Copy place (variable, field, index) is deep-cloned so the
                    // returned value is independent of the binding.
                    let s = if matches!(self.current_ret_ty, Ty::Func(..)) {
                        let ret_ty = self.current_ret_ty.clone();
                        self.emit_into_func_slot(e, &ret_ty)?
                    } else if self.ty_has_poly_base(&self.current_ret_ty) {
                        let ret_ty = self.current_ret_ty.clone();
                        self.emit_into_base_slot(e, &ret_ty)?
                    } else {
                        self.emit_consuming(e)?
                    };
                    // (try/except control flow) thread the (already coerced)
                    // value out of the catch_unwind closure when emitting the try
                    // body; otherwise issue the plain function return as before.
                    if self.try_return_escape {
                        self.line(&format!("return __PyrstTryFlow::Return({});", s));
                    } else {
                        self.line(&format!("return {};", s));
                    }
                }
            }
            Stmt::Expr(e) => {
                let s = self.emit_expr(e)?;
                self.line(&format!("{};", s));
            }
            Stmt::Assign { target, ty, value, span, .. } => {
                // Uniform clone-on-use: assigning from a non-Copy place (`y = x`,
                // `y = self.field`) deep-clones so the two bindings are independent
                // (Python value semantics). Owned temps (call/literal/binop) are bare.
                // (EPIC-5 C2-3 cleanup) `v` is computed lazily per branch: the
                // annotated poly-base path emits via `emit_into_base_slot` directly
                // (which recomputes the clone-on-use emission internally), so the
                // earlier unconditional `emit_consuming(value)` here was redundant
                // work it then discarded. The non-poly annotated path, the inferred
                // path, and the rebind path each compute the clone-on-use `v` once.
                let is_declared = self.declared.contains(target);
                if !is_declared {
                    self.declared.insert(target.clone());
                    // (card 575bcf3a) Declaring a fresh binding of this name means it
                    // has no active shadow in the new scope. A no-op in the enclosing
                    // function (a shadowed name is always already-declared → the else
                    // branch); it matters only inside a nested `def` that declares its
                    // OWN local of a name captured-and-shadowed at the definition site.
                    self.shadow_map.remove(target);
                    match ty {
                        Some(t) => {
                            // Scope with the enclosing generic function's type vars
                            // so a `acc: T` declaration is `TypeVar("T")` and a later
                            // `acc = f(...)` mutates (not shadows) — see the
                            // `current_fn_type_params` field doc.
                            let ty_obj = Ty::from_type_expr_scoped(t, *span, &self.current_fn_type_params)?;
                            // (EPIC-5 C2-2b-i) `a: Animal = Account(...)` — a raw
                            // struct into a polymorphic-base `Animal__` slot is
                            // WRAPPED in the right variant (replaces the C1 gate).
                            // (first-class functions) `g: Callable[..] = inc` /
                            // `= lambda ...` — wrap a function NAME or lambda into
                            // the `Rc<dyn Fn>` slot. Non-poly, non-func slots keep
                            // the clone-on-use emission.
                            let v = if self.ty_has_func(&ty_obj) {
                                self.emit_into_func_slot(value, &ty_obj)?
                            } else if self.ty_has_poly_base(&ty_obj) {
                                self.emit_into_base_slot(value, &ty_obj)?
                            } else {
                                self.emit_consuming(value)?
                            };
                            self.locals.insert(target.clone(), ty_obj.clone());
                            // (EPIC-5) An Optional-annotated binding wraps a bare
                            // value in `Some(..)` (or emits `None` for the None
                            // literal); an already-Optional initializer passes
                            // through. Shared with return/argument sites.
                            let v = self.coerce_to_option(v, value, &ty_obj);
                            // A float-annotated binding may receive an integer-typed
                            // value (e.g. `x: float = 2 ** 3`, where `**` constant-folds
                            // to an int and int**int otherwise emits i64). Cast to f64 so
                            // the declared type matches the initializer (avoids E0308).
                            // `emits_int_pow` covers the case the oracle now types as
                            // Float (D5) but emission still lowers to i64.
                            let value_ty = self.type_of_expr(value);
                            // (EPIC-6) Escape the emitted binding name; the raw
                            // `target` stays the `declared`/`locals` key.
                            let target_e = escape_ident(target);
                            if matches!(ty_obj, Ty::Float)
                                && (matches!(value_ty, Ty::Int) || self.emits_int_pow(value))
                            {
                                self.line(&format!("let mut {}: {} = {} as f64;", target_e, self.rust_ty(&ty_obj), v));
                            } else {
                                self.line(&format!("let mut {}: {} = {};", target_e, self.rust_ty(&ty_obj), v));
                            }
                        }
                        None => {
                            let v = self.emit_consuming(value)?;
                            // Infer type from the value expression, but prefer a
                            // richer type discovered by the forward pre-pass
                            // (e.g. an `acc = 0` later assigned floats).
                            let value_ty = self.type_of_expr(value);
                            let decl_ty = match self.locals.get(target) {
                                // (card 575bcf3a fix) `pre` is the forward pre-pass
                                // type. It legitimately WIDENS this declaration (an
                                // `acc = 0` later assigned floats → declare as f64) —
                                // but only when it is COMPATIBLE with the value being
                                // declared. When `pre` is a genuinely DIVERGENT type
                                // (a later `x = "s"` after `x = [1]`, which
                                // `prescan_merge_ty`'s replace-on-divergence baked into
                                // the slot), adopting it would leave `self.locals[x]`
                                // lying about the just-emitted binding's Rust type
                                // (`Vec` emitted, `Str` tracked) and make the NEXT
                                // divergent reassign wrongly plain-reassign into a
                                // Vec slot (rustc E0308). Keep the declared value's OWN
                                // type in that case; the later reassign then correctly
                                // shadows.
                                Some(pre) if !crate::typeck::branch_divergent(pre, &value_ty) => {
                                    Self::unify_ty(pre.clone(), value_ty.clone())
                                }
                                _ => value_ty.clone(),
                            };
                            self.locals.insert(target.clone(), decl_ty.clone());
                            // If the variable is later widened from int to float,
                            // declare it as f64 and cast the integer initializer.
                            // (EPIC-6) Escape the emitted binding name.
                            let target_e = escape_ident(target);
                            if matches!(decl_ty, Ty::Float)
                                && (matches!(value_ty, Ty::Int) || self.emits_int_pow(value))
                            {
                                self.line(&format!("let mut {}: f64 = {} as f64;", target_e, v));
                            } else {
                                self.line(&format!("let mut {} = {};", target_e, v));
                            }
                        }
                    }
                } else {
                    let cur = self.locals.get(target).cloned().unwrap_or(Ty::Unknown);
                    // (first-class functions) Reassigning a Callable-typed (or
                    // func-containing collection-typed) local: a bare function NAME
                    // / lambda / func-name-bearing literal on the RHS must be
                    // wrapped into the `Rc<dyn Fn>` slot, exactly as in the
                    // declaration branch. Without this, `f = double` would emit
                    // `f = double.clone();` (a fn item has no `.clone() -> Rc<dyn
                    // Fn>`) -> rustc E0308. An `IfExp` RHS (`f = inc if c else
                    // double`) is handled by `emit_into_func_slot` recursing into
                    // its arms via the IfExp case it shares with `emit_consuming`.
                    let v = if self.ty_has_func(&cur) {
                        self.emit_into_func_slot(value, &cur)?
                    } else {
                        self.emit_consuming(value)?
                    };
                    // Python permits rebinding a name to a value of a different
                    // type. When that happens, emit a shadowing `let` (which
                    // always type-checks) instead of a plain reassignment.
                    let value_ty = self.type_of_expr(value);
                    // (EPIC-6) Escape the emitted name (raw `target` stays map key).
                    let target_e = escape_ident(target);
                    if let Some((_, slot_ty)) = self.shadow_map.get(target).cloned() {
                        // (card 575bcf3a, poison2) A divergent shadow of this HOISTED
                        // local is live (a prior block statement retyped it away from
                        // its function-scope slot). `v` above already read the shadow
                        // through its mangled name.
                        if !Self::types_conflict(&slot_ty, &value_ty) {
                            // RECONVERGE: the value fits the slot again -> write the
                            // ORIGINAL (slot) binding and drop the shadow, so the
                            // materialized value reaches the hoisted slot and later
                            // reads resolve to it (`xs = gen(3); xs = list(xs)`).
                            self.shadow_map.remove(target);
                            self.locals.insert(target.clone(), slot_ty);
                            self.line(&format!("{} = {};", target_e, v));
                        } else {
                            // Still divergent from the slot -> a fresh mangled shadow.
                            let m = self.fresh_shadow_name(target);
                            self.shadow_map.insert(target.clone(), (m.clone(), slot_ty));
                            self.locals.insert(target.clone(), value_ty);
                            self.line(&format!("let mut {} = {};", m, v));
                        }
                    } else if Self::types_conflict(&cur, &value_ty) {
                        if self.hoisted.contains(target) {
                            // (card 575bcf3a, poison2) First divergence of a HOISTED
                            // local inside this block: emit the shadow under a MANGLED
                            // name (not `target`) so it does not hide the
                            // function-scope slot by name — a later assign whose value
                            // reconverges to the slot type can then still reach the
                            // slot (above). `cur` is the slot type here. Reads of the
                            // name in the interim resolve to the mangled shadow.
                            let m = self.fresh_shadow_name(target);
                            self.shadow_map.insert(target.clone(), (m.clone(), cur.clone()));
                            self.locals.insert(target.clone(), value_ty);
                            self.line(&format!("let mut {} = {};", m, v));
                        } else {
                            // Non-hoisted (function-scope / param) binding: a
                            // same-named `let` shadow is correct — there is no
                            // separate slot to keep reachable (unchanged behavior).
                            self.locals.insert(target.clone(), value_ty);
                            self.line(&format!("let mut {} = {};", target_e, v));
                        }
                    } else if matches!(cur, Ty::Float)
                        && (matches!(value_ty, Ty::Int) || self.emits_int_pow(value))
                    {
                        // Reassigning an int into a float-typed (e.g. hoisted) var.
                        let rhs = format!("{} as f64", v);
                        if !self.try_fold_hoisted_init(&target_e, &cur, &rhs) {
                            self.line(&format!("{} = {};", target_e, rhs));
                        }
                    } else {
                        // Fold a hoisting double-init when this is the assignment
                        // immediately following the hoisted default declaration.
                        if !self.try_fold_hoisted_init(&target_e, &cur, &v) {
                            self.line(&format!("{} = {};", target_e, v));
                        }
                    }
                }
            }
            Stmt::Unpack { targets, value, .. } => {
                // (card 602b1675) Give tuple-unpack the same declare-vs-reassign
                // distinction `Stmt::Assign` has, so an unpack REASSIGNMENT of
                // pre-existing bindings inside a nested block writes those bindings
                // instead of emitting a discarded block-scoped `let (..)` shadow (the
                // swap idiom `a,b=b,a` and tuple-unpack GCD in a `while` loop).
                //
                // Per-target component types of the RHS (Unknown when the RHS is not
                // a statically-known tuple of matching arity), mirroring the
                // liveness/divergence analyses so the decisions agree.
                let vt = self.type_of_expr(value);
                // LIST-unpacking (`a, b = xs` where the RHS is a list[T], incl. the
                // `a, b = "x=1".split("=")` idiom): a Rust tuple-destructure of a
                // `Vec` is E0308, so lower to a RUNTIME length check + per-element
                // read instead. Bind the whole RHS to one temp FIRST (fully
                // evaluated, swap-safe, and never re-reads a target), length-check
                // it with CPython's exact ValueError text, then dispatch each target
                // through the shared per-target helper (declare / reassign / shadow /
                // duplicate-last-wins) — the same helper the tuple mixed path uses,
                // so declare-vs-reassign, nested-block scoping, and repeated targets
                // all behave identically to tuple-unpack. Tuple RHS keeps the
                // static-arity destructuring paths below (no length check needed).
                if let Ty::List(elem) = &vt {
                    let elem_ty = (**elem).clone();
                    let n = targets.len();
                    let v = self.emit_expr(value)?;
                    let base = self.shadow_counter;
                    self.shadow_counter += 1;
                    let tmp = format!("__pyrst_lunpack_{}", base);
                    self.line(&format!("let {} = __py_unpack_list({}, {});", tmp, v, n));
                    for (i, t) in targets.iter().enumerate() {
                        let rhs = format!("{}[{}].clone()", tmp, i);
                        self.emit_unpack_target(t, &rhs, &elem_ty);
                    }
                    return Ok(());
                }
                let comp_tys: Vec<Ty> = match &vt {
                    Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
                    _ => vec![Ty::Unknown; targets.len()],
                };
                // (card 8bedd8e4) Python allows a REPEATED unpack target
                // (`a, a = 1, 2`, left-to-right, last wins -> a == 2). A single
                // destructuring `let (mut a, mut a) = (1, 2);` — the `all_new` form
                // below — is NOT valid Rust for a repeated name (E0416, "identifier
                // bound more than once in the same pattern"); it was a raw rustc
                // leak from `pyrst build` on an otherwise-typechecked program. A
                // duplicate target forces `all_new` false so the MIXED/temp-split
                // path (below) runs instead: it binds the whole RHS to temps first,
                // then dispatches each target through `emit_unpack_target`
                // SEQUENTIALLY — the second `a` sees the first's binding already
                // `declared` and reassigns it (`a = temp1;`), giving the same
                // left-to-right last-wins result Python does. This does not touch
                // `all_declared`/`all_simple_reassign`: a duplicate target that is
                // ALREADY declared before this statement takes that path unchanged,
                // and Rust's destructuring ASSIGNMENT (unlike `let`) permits a
                // repeated place (`(a, a) = (1, 2);` compiles — it is not a pattern
                // binding), so that form was never broken.
                let has_dup_targets = {
                    let mut seen = std::collections::HashSet::with_capacity(targets.len());
                    !targets.iter().all(|t| seen.insert(t.as_str()))
                };
                let all_new = targets.iter().all(|t| !self.declared.contains(t)) && !has_dup_targets;
                let all_declared = targets.iter().all(|t| self.declared.contains(t));
                let any_shadow = targets.iter().any(|t| self.shadow_map.contains_key(t));
                // A pre-existing target whose slot is incompatible with its component
                // (a real divergence, or an int component into a float slot needing an
                // `as f64` cast) cannot ride the plain destructuring-assignment form.
                let all_simple_reassign = all_declared && !any_shadow
                    && targets.iter().zip(comp_tys.iter()).all(|(t, ct)| {
                        let cur = self.locals.get(t).cloned().unwrap_or(Ty::Unknown);
                        !Self::types_conflict(&cur, ct)
                            && !(matches!(cur, Ty::Float) && matches!(ct, Ty::Int))
                    });

                if all_new {
                    // Every target is a FRESH binding: one destructuring `let` (the
                    // long-standing form — bare_unpack's `x,y=1,2` / `a,b=get_pair()`
                    // stay byte-identical). Record each as declared + typed so a LATER
                    // reassignment of the same name reassigns the binding rather than
                    // shadowing (`a,b=0,1; while ...: a,b=b,a+b`).
                    let v = self.emit_expr(value)?;
                    // Each fresh binding is `mut` (uniform with scalar `let mut x`)
                    // so a LATER reassignment of the same name compiles; unused-mut is
                    // allowed crate-wide. `let (mut a, mut b) = ..` is the Rust form
                    // (a bare `let (a, b) mut` does not exist).
                    let targets_e: Vec<String> = targets.iter().map(|t| format!("mut {}", escape_ident(t))).collect();
                    self.line(&format!("let ({}) = {};", targets_e.join(", "), v));
                    for (t, ct) in targets.iter().zip(comp_tys.iter()) {
                        self.declared.insert(t.clone());
                        self.locals.insert(t.clone(), ct.clone());
                    }
                } else if all_simple_reassign {
                    // Every target is an existing, shadow-free binding whose component
                    // type fits it: a Rust destructuring ASSIGNMENT to the existing
                    // places (valid Rust since 1.59). The RHS tuple is evaluated IN
                    // FULL before any target is written, so `a,b=b,a` swaps correctly
                    // and the tuple-unpack GCD `a,b=b,a%b` updates the outer bindings
                    // (terminates). No locals/declared change (a non-conflict reassign
                    // keeps the richer existing type, exactly like scalar Assign).
                    let v = self.emit_expr(value)?;
                    let targets_e: Vec<String> = targets.iter().map(|t| escape_ident(t)).collect();
                    self.line(&format!("({}) = {};", targets_e.join(", "), v));
                } else {
                    // MIXED (some new + some existing), a per-component conflict, an
                    // int->float widen, or an active shadow. Bind the whole RHS to
                    // fresh temps FIRST — so it is fully evaluated before any target is
                    // written (swap-safe) and the temps never re-read a target (no
                    // LHS/RHS name collision) — then emit each target independently
                    // (declare / reassign / shadow / reconverge) via the shared
                    // per-target helper. This SUPPORTS the mixed shape cleanly.
                    let v = self.emit_expr(value)?;
                    let base = self.shadow_counter;
                    self.shadow_counter += 1;
                    let temps: Vec<String> = (0..targets.len())
                        .map(|i| format!("__pyrst_unpack_{}_{}", base, i))
                        .collect();
                    self.line(&format!("let ({}) = {};", temps.join(", "), v));
                    for ((t, temp), ct) in targets.iter().zip(temps.iter()).zip(comp_tys.iter()) {
                        self.emit_unpack_target(t, temp, ct);
                    }
                }
            }
            Stmt::AugAssign { target, op, value, span } => {
                let target_ty = self.locals.get(target.as_str()).cloned().unwrap_or(Ty::Unknown);
                // (W0-c) Set augmented assignment `s |= t` / `&=` / `-=` / `^=`:
                // Rust `HashSet` has no compound-assignment trait, so desugar to
                // `s = s <op> t` through the set-algebra BinOp lowering (the set
                // typing now exists). This closes the check-OK / build-FAIL gap the
                // reviewer flagged, Python-faithfully (`set.__ior__` etc.). The RHS
                // is emitted ONCE, inside the synthesized BinOp.
                if matches!(target_ty, Ty::Set(_))
                    && matches!(op, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Sub)
                {
                    let desugar = Expr::BinOp {
                        op: *op,
                        lhs: Box::new(Expr::Ident(target.clone(), *span)),
                        rhs: Box::new(value.clone()),
                        span: *span,
                    };
                    let rhs = self.emit_expr(&desugar)?;
                    let tgt = self.shadow_read_name(target).unwrap_or_else(|| escape_ident(target));
                    self.line(&format!("{} = {};", tgt, rhs));
                    return Ok(());
                }
                // (card 12052b4c) `xs += ys` (list target, Add) lowers to
                // `xs.extend(<rhs>)` and `s += t` (str target, Add) lowers to
                // `s += &(<rhs>)` below — both need the RHS built through the uniform
                // clone-on-use helper (a bare-Ident RHS is `.clone()`d so `ys`/`t`
                // stays usable afterward; an owned temp — literal/comprehension/call —
                // stays bare), exactly like every other consuming site
                // (`emit_consuming`, src/codegen/items.rs).
                // (card 12052b4c review, comment 178) The Str+Add arm MUST also route
                // through `emit_consuming`, not just List+Add: a SELF-aliasing append
                // `s += s` otherwise emitted `s += &(s);` — `&mut s` (the `+=`) and
                // `&s` (the borrowed RHS) alias the same binding, a raw rustc E0502
                // leak that passed `check`. Cloning the bare-Ident RHS first
                // (`s += &(s.clone());`) breaks the alias and yields the correct
                // doubled string (`abab`), verified against python3. Every other
                // AugAssign shape keeps the plain `emit_expr` it always used
                // (byte-identical; a str/list literal RHS still emits bare because
                // `emit_consuming` only clones a bare place, not an owned temp).
                let v = if matches!(op, BinOp::Add) && matches!(target_ty, Ty::Str | Ty::List(_)) {
                    self.emit_consuming(value)?
                } else {
                    self.emit_expr(value)?
                };
                // (EPIC-6) `target` names an existing local (emitted escaped by its
                // `let`), so every occurrence here — store target AND read — uses
                // the escaped form.
                // (card 575bcf3a fix) When the target is a HOISTED local under an
                // active divergent shadow, `x += v` reads AND writes the mangled
                // shadow binding (which holds the current value), not the outer slot.
                // Resolve through shadow_map exactly like Assign / emit_expr reads;
                // `target_ty` is already the shadow's tracked type (locals holds it
                // while shadowed). `shadow_map` is empty for all shadow-free code, so
                // this is a no-op there.
                let target = self.shadow_read_name(target).unwrap_or_else(|| escape_ident(target));
                let target = target.as_str();
                match op {
                    BinOp::FloorDiv => {
                        // Python's //= floors toward negative infinity; Rust's /= truncates toward zero.
                        // For float targets keep the f64 floor path.
                        // For int targets route through __py_floordiv which also panics on /0
                        // with a catchable ZeroDivisionError payload.
                        if matches!(target_ty, Ty::Float) {
                            self.line(&format!("{} = ({} as f64 / {} as f64).floor();", target, target, v));
                        } else {
                            self.line(&format!("{} = __py_floordiv(({}), ({}));", target, target, v));
                        }
                    }
                    BinOp::Mod => {
                        // Python's %= takes the sign of the divisor; Rust's %= takes the
                        // sign of the dividend. Mirror the BinOp lowering.
                        if matches!(target_ty, Ty::Float) {
                            self.line(&format!(
                                "{{ let __b = ({} as f64); {} = ((({} as f64) % __b) + __b) % __b; }}",
                                v, target, target
                            ));
                        } else {
                            self.line(&format!("{} = __py_mod(({}), ({}));", target, target, v));
                        }
                    }
                    BinOp::Pow => {
                        // `x **= y` keeps `x`'s declared type (Python semantics),
                        // unlike binary `**` whose oracle type is Float. Mirror the
                        // operand-driven emission of the binary Pow arm:
                        //   int target  -> __py_ipow (i64, panics on negative exp)
                        //   float target-> f64 powf
                        // so `12 **= 2` stays the int 144 and a float target stays float.
                        if matches!(target_ty, Ty::Float) {
                            self.line(&format!("{} = (({} as f64).powf({} as f64));", target, target, v));
                        } else {
                            self.line(&format!("{} = __py_ipow(({}), ({}));", target, target, v));
                        }
                    }
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div
                    | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor
                    | BinOp::LShift | BinOp::RShift => {
                        let op_s = match op {
                            BinOp::Add => "+=", BinOp::Sub => "-=", BinOp::Mul => "*=", BinOp::Div => "/=",
                            BinOp::BitAnd => "&=", BinOp::BitOr => "|=", BinOp::BitXor => "^=",
                            BinOp::LShift => "<<=", BinOp::RShift => ">>=",
                            _ => unreachable!(),
                        };
                        // (card 12052b4c) The bare `target op= value;` below is only
                        // valid for `Copy` scalars (Int/Float): Rust's compound-
                        // assignment traits don't cover the non-Copy heap types this
                        // arm used to emit it for unconditionally, and that mismatch
                        // doesn't fail as a clean pyrst diagnostic — it LEAKS a raw
                        // rustc error at `build` (a `check`-time pass, not `check`
                        // itself, since AugAssign never re-validates its own
                        // op/target-type combination — see flow.rs). Concretely:
                        // `String` implements `AddAssign<&str>`, NOT
                        // `AddAssign<String>` (`s += "llo"` leaked E0308 — the
                        // original bug report); `Vec`/`HashSet` implement NO
                        // compound-assignment trait at all (`xs += ys` / `s1 |= s2`
                        // leaked E0368). Special-case the two shapes Python actually
                        // defines as an in-place mutation — str `+=` (append) and
                        // list `+=` (`list.__iadd__`, i.e. extend; unlike list `+`,
                        // which concatenates into a NEW list and stays an honest
                        // reject in exprs.rs's BinOp::Add arm) — and give every
                        // OTHER non-Copy combination (str `-=`/`*=`, list `-=`/`*=`,
                        // set `|=`/`&=`/`-=`/`^=`, dict, class, …) an honest codegen
                        // error instead of the raw rustc leak. None of those other
                        // combinations is valid Python either (TypeError there), so
                        // refusing is faithful, not merely defensive.
                        if matches!(target_ty, Ty::Str) && matches!(op, BinOp::Add) {
                            // `String` has no `AddAssign<String>` — borrow as `&str`.
                            self.line(&format!("{} += &({});", target, v));
                        } else if matches!(target_ty, Ty::List(_)) && matches!(op, BinOp::Add) {
                            // `v` was already built via `emit_consuming` above.
                            self.line(&format!("{}.extend({});", target, v));
                        } else if !self.is_copy_type(&target_ty) {
                            return Err(crate::diag::Error::Codegen(format!(
                                "augmented assignment `{}` is not supported for type {} — \
                                 rebind explicitly (`{target} = {target} <op> ...`) or use \
                                 an equivalent method (e.g. `.extend()`, `.update()`)",
                                op_s, target_ty
                            )));
                        } else {
                            self.line(&format!("{} {} {};", target, op_s, v));
                        }
                    }
                    // FloorDiv/Mod/Pow are handled by explicit arms above. No other
                    // BinOp can reach an AugAssign target: comparison, logical,
                    // identity, and membership operators are not augmented-assign
                    // operators, so the parser never produces them here. Make an
                    // unhandled op a hard error rather than silently miscompiling
                    // (the previous `_ => "+="` fallback was a latent miscompile).
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                    | BinOp::And | BinOp::Or
                    | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => {
                        unreachable!("non-augmentable BinOp {:?} reached AugAssign codegen", op);
                    }
                }
            }
            Stmt::If { cond, then, elifs, else_, .. } => {
                // (EPIC-5) None-guard narrowing must agree with typeck
                // (check_stmt's If arm): for `x is not None` the THEN branch sees
                // the unwrapped payload; for `x is None` the ELSE branch (when
                // there are no elifs) sees it. The unwrap shadows the Option
                // binding inside the block, so it never leaks past the `if`. Only
                // a local actually typed `Option<_>` is narrowed.
                // `narrowed` = the Option local and its inner type when the
                // condition is a None-guard on a local typed `Option<_>`.
                let narrowed: Option<(String, bool, Ty)> = extract_narrowing(cond)
                    .and_then(|(var, is_not_none)| match self.locals.get(var.as_str()) {
                        Some(Ty::Option(inner)) => Some((var, is_not_none, (**inner).clone())),
                        _ => None,
                    });
                let c = self.emit_expr(cond)?;
                self.line(&format!("if {} {{", c));
                self.indent += 1;
                // (card 575bcf3a) Isolate this branch's block-scope emission state
                // (locals / declared / shadow_map) so a divergent shadow inside it
                // cannot poison the sibling branches or the statement following the
                // whole `if`. Per-branch (not per-`if`) so a `then` shadow also does
                // not leak into an `elif`/`else`.
                let __then_scope = self.scope_enter();
                // THEN branch is the non-None case for `x is not None`. Emit the
                // unwrap and retype the local so type-dispatched emission inside
                // the block (e.g. `str(x)`) sees the inner type; restore after.
                let then_narrow = narrowed.as_ref().filter(|(_, is_not_none, _)| *is_not_none);
                let then_saved = then_narrow.map(|(var, _, inner)| {
                    // (EPIC-6) `var` names an existing Optional local; both the new
                    // shadow binding and the `.unwrap()` read escape identically.
                    // (W1.5, card 71cbd940) `.clone().unwrap()` — a BORROWING
                    // unwrap: the bare `.unwrap()` MOVED the Option, which is
                    // E0382 when the narrowing guard sits inside a loop (the
                    // accumulate2 shape: `if f is None: .. else: f(acc, ..)`
                    // re-entered per iteration). pyrst locals are all Clone.
                    let var_e = escape_ident(var);
                    self.line(&format!("let {} = {}.clone().unwrap();", var_e, var_e));
                    let prev = self.locals.insert(var.clone(), inner.clone());
                    (var.clone(), prev)
                });
                for s in then { self.emit_stmt(s)?; }
                if let Some((var, prev)) = then_saved {
                    match prev { Some(t) => { self.locals.insert(var, t); } None => { self.locals.remove(var.as_str()); } }
                }
                self.scope_exit(__then_scope);
                self.indent -= 1;
                for (c, b) in elifs {
                    let cs = self.emit_expr(c)?;
                    self.line(&format!("}} else if {} {{", cs));
                    self.indent += 1;
                    let __elif_scope = self.scope_enter();
                    for s in b { self.emit_stmt(s)?; }
                    self.scope_exit(__elif_scope);
                    self.indent -= 1;
                }
                if let Some(b) = else_ {
                    self.line("} else {");
                    self.indent += 1;
                    let __else_scope = self.scope_enter();
                    // ELSE is the non-None case only for `x is None` with no elifs.
                    let else_narrow = narrowed.as_ref()
                        .filter(|(_, is_not_none, _)| !*is_not_none && elifs.is_empty());
                    let else_saved = else_narrow.map(|(var, _, inner)| {
                        // (EPIC-6) Same escape + borrowing-unwrap rationale as the
                        // THEN-branch narrowing above.
                        let var_e = escape_ident(var);
                        self.line(&format!("let {} = {}.clone().unwrap();", var_e, var_e));
                        let prev = self.locals.insert(var.clone(), inner.clone());
                        (var.clone(), prev)
                    });
                    for s in b { self.emit_stmt(s)?; }
                    if let Some((var, prev)) = else_saved {
                        match prev { Some(t) => { self.locals.insert(var, t); } None => { self.locals.remove(var.as_str()); } }
                    }
                    self.scope_exit(__else_scope);
                    self.indent -= 1;
                }
                self.line("}");
            }
            Stmt::While { cond, body, .. } => {
                // `while True:` (the LITERAL `True` condition) lowers to Rust
                // `loop { ... }`, NOT `while true { ... }`. Rust's `while true`
                // has type `()` and does NOT diverge, so an always-returning
                // `while True` function would leave an implicit `()` tail and
                // fail rustc E0308. `loop` diverges (its type is `!` unless a
                // `break` carries a value), so such a function compiles; `break`
                // and `continue` inside still behave identically. This mirrors
                // typeck's missing-return gate, which treats a break-free
                // `while True` as diverging. Any other condition stays `while`.
                if matches!(cond, Expr::Bool(true, _)) {
                    self.line("loop {");
                } else {
                    let c = self.emit_expr(cond)?;
                    self.line(&format!("while {} {{", c));
                }
                self.indent += 1;
                // (try/except control flow, don't-descend) A `break`/`continue`
                // inside THIS loop targets THIS loop, so suspend the try-body
                // loop-control escape for the loop body (emit real Rust
                // break/continue). `try_return_escape` is NOT suspended: a
                // `return` inside a loop that sits in a try body must still
                // escape the catch_unwind closure.
                let saved_loopctl = std::mem::replace(&mut self.try_loopctl_escape, false);
                // (card 575bcf3a) The loop body is a Rust `{}` scope; isolate its
                // block-scope emission state so a shadow inside it never leaks out.
                let __while_scope = self.scope_enter();
                for s in body { self.emit_stmt(s)?; }
                self.scope_exit(__while_scope);
                self.try_loopctl_escape = saved_loopctl;
                self.indent -= 1;
                self.line("}");
            }
            Stmt::For { targets, iter, body, .. } => {
                // Check if element type is Copy to use .iter().copied() instead of
                // .iter().cloned(). Copy-ness goes through the single shared
                // predicate (`crate::typeck::is_copy`), so the for-loop lowering
                // can't drift from the rest of codegen — it also picks up `Unit`
                // and recursively-Copy `Tuple`/`Option` elements the old inline
                // `matches!` omitted.
                let is_copy_elem = if let Expr::Ident(name, _) = iter {
                    self.locals.get(name.as_str()).or_else(|| self.ctx.vars.get(name.as_str()))
                        // LAZY-GEN V1-a: a generator stored in a local (`Ty::Iterator`)
                        // iterates like a list — the same `.copied()`-vs-`.cloned()`
                        // element-copy decision applies (keeps `generator_gen_local`
                        // byte-identical).
                        .map(|ty| if let Ty::List(inner) | Ty::Iterator(inner) = ty {
                            self.is_copy_type(inner)
                        } else { false })
                        .unwrap_or(false)
                } else {
                    false
                };
                // Resolve the iterable's static type up front so the iteration
                // lowering matches the Python semantics for each container:
                //   dict -> iterate KEYS; str -> iterate characters.
                let for_iter_ty = self.type_of_expr(iter);
                let i = self.emit_expr(iter)?;
                let is_range = i.contains("..");
                let is_iterator = i.contains(".enumerate()") || i.contains(".zip(") ||
                                 i.contains(".cloned()") || i.contains(".copied()") ||
                                 i.contains(".collect::<Vec<_>>()");
                // For ranges, use into_iter(); for collections, use iter().cloned() or iter().copied().
                // If it's already an iterator (enumerate/zip), use directly.
                let iter_expr = if matches!(for_iter_ty, Ty::Iterator(_)) {
                    // (LAZY-GEN V1-b) A generator value is itself an `Iterator`
                    // (`Gen<T>`); iterate it DIRECTLY — `Gen` has no `.iter()`, and
                    // it already yields OWNED `T`, so no `.cloned()` and no double
                    // clone. This is the type-driven path that retires the emitted-
                    // string `is_iterator` sniff FOR GENERATORS; the sniff below
                    // still handles the `enumerate`/`zip`/`cloned` adapter shapes.
                    //
                    // (review fix) A generator VARIABLE iterates by `&mut` (std's
                    // blanket `Iterator for &mut I`) instead of MOVING: the binding
                    // stays live and ADVANCES in place, so a nested loop over two
                    // generator locals compiles and a second `for x in g` yields
                    // nothing — Python's generator semantics exactly (was E0382).
                    // A fresh rvalue (`for x in gen()`) is consumed by value.
                    if matches!(iter, Expr::Ident(..)) {
                        format!("(&mut {})", i)
                    } else {
                        i
                    }
                } else if is_iterator {
                    i
                } else if is_range {
                    format!("({}).into_iter()", i)
                } else if matches!(for_iter_ty, Ty::Str) {
                    // Iterating a str yields 1-character strings (Python semantics).
                    // Mirrors the comprehension lowering.
                    format!("{}.chars().map(|__c| __c.to_string())", i)
                } else if matches!(for_iter_ty, Ty::Dict(_, _)) {
                    // Iterating a dict yields its KEYS (Python semantics).
                    // Materialize a sorted Vec of the keys so the iteration order
                    // is deterministic — matching the sort-for-stability convention
                    // used by `PyRepr` for HashMap display.
                    format!(
                        "{{ let mut __keys: Vec<_> = {}.keys().cloned().collect(); __keys.sort(); __keys }}.into_iter()",
                        i
                    )
                } else if is_copy_elem {
                    format!("{}.iter().copied()", i)
                } else {
                    format!("{}.iter().cloned()", i)
                };
                // (EPIC-6) Escape each loop-variable name in the `for` pattern;
                // body uses resolve to the same escaped form (emit_expr Ident).
                let pat = if targets.len() == 1 {
                    escape_ident(&targets[0])
                } else {
                    format!("({})", targets.iter().map(|t| escape_ident(t)).collect::<Vec<_>>().join(", "))
                };
                self.line(&format!("for {} in {} {{", pat, iter_expr));
                self.indent += 1;

                // (try/except control flow, don't-descend) break/continue inside
                // this for-loop target THIS loop, so suspend the try-body
                // loop-control escape for the body (real Rust break/continue).
                // `try_return_escape` is intentionally left alone — a `return`
                // inside a for-loop within a try body still escapes the closure.
                let saved_loopctl_for = std::mem::replace(&mut self.try_loopctl_escape, false);

                // Register the loop variable's type so the body sees it. Reuse the
                // iterable type resolved above: list/set yield the element type, a
                // dict yields its KEY type, str yields 1-char strings (Str), and a
                // range yields Int. The loop var must be registered as a LOCAL even
                // when its element type is unknown (fallback Unknown), because the
                // for-pattern binding SHADOWS any module const of the same name:
                // the body must reference the loop variable, not mangle the name to
                // the const (`for i in range(3)` with a module const `i`).
                let loop_elem_ty = match &for_iter_ty {
                    // LAZY-GEN V1-a: a generator source yields elements like a list.
                    Ty::List(inner) | Ty::Iterator(inner) | Ty::Set(inner) => (**inner).clone(),
                    Ty::Dict(key, _) => (**key).clone(),
                    Ty::Str => Ty::Str,
                    _ if is_range => Ty::Int,
                    _ => Ty::Unknown,
                };
                // (card 575bcf3a) Isolate the loop body's block-scope emission state
                // (locals / declared / shadow_map) — the loop-variable registration
                // below is captured INSIDE this window and reverts with it.
                let __for_scope = self.scope_enter();
                if targets.len() == 1 {
                    let saved = self.locals.get(&targets[0]).cloned();
                    self.locals.insert(targets[0].clone(), loop_elem_ty);
                    for s in body { self.emit_stmt(s)?; }
                    if let Some(ty) = saved {
                        self.locals.insert(targets[0].clone(), ty);
                    } else {
                        self.locals.remove(targets[0].as_str());
                    }
                } else {
                    // Multiple targets (tuple unpacking): register each as a local
                    // (Unknown type) for the body's duration so each shadows any
                    // same-named module const, then restore.
                    let saved: Vec<(String, Option<Ty>)> = targets.iter()
                        .map(|t| (t.clone(), self.locals.get(t).cloned()))
                        .collect();
                    for t in targets { self.locals.insert(t.clone(), Ty::Unknown); }
                    for s in body { self.emit_stmt(s)?; }
                    for (t, prev) in saved {
                        match prev {
                            Some(ty) => { self.locals.insert(t, ty); }
                            None => { self.locals.remove(t.as_str()); }
                        }
                    }
                }
                self.scope_exit(__for_scope);
                self.try_loopctl_escape = saved_loopctl_for;

                self.indent -= 1;
                self.line("}");
            }
            Stmt::Import { .. } => {
                // Silently drop imports in v0
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                self.emit_try(body, handlers, else_, finally_)?;
            }
            Stmt::With { ctx_expr, as_name, body, .. } => {
                let ctx_s = self.emit_expr(ctx_expr)?;
                self.line("{");
                self.indent += 1;
                // (card 575bcf3a) Isolate the `with` body's block-scope emission
                // state (the bound name below is captured inside this window).
                let __with_scope = self.scope_enter();
                // The bound name is block-scoped in the generated Rust, so save and
                // restore the outer locals entry around the body (mirrors for-loop).
                let saved = if let Some(name) = as_name {
                    // Register the bound type (e.g. open() -> File) so method calls
                    // on it (f.write/read) resolve to the right emission.
                    let prev = self.locals.get(name).cloned();
                    self.locals.insert(name.clone(), self.type_of_expr(ctx_expr));
                    // (EPIC-6) `with ... as <name>:` binds a user local; escape the
                    // emitted name (raw stays the `locals` key).
                    self.line(&format!("let mut {} = {};", escape_ident(name), ctx_s));
                    Some((name.clone(), prev))
                } else {
                    self.line(&format!("let _ = {};", ctx_s));
                    None
                };
                for s in body { self.emit_stmt(s)?; }
                if let Some((name, prev)) = saved {
                    match prev {
                        Some(ty) => { self.locals.insert(name, ty); }
                        None => { self.locals.remove(name.as_str()); }
                    }
                }
                self.scope_exit(__with_scope);
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Del { target, .. } => {
                let t = self.emit_expr(target)?;
                self.line(&format!("drop({});", t));
            }
            Stmt::AttrAssign { obj, attr, value, .. } => {
                // (EPIC-5 C2-2b-i) A field-WRITE through a polymorphic-base var
                // (`a.balance = ...` where `a: Account` and Account has subclasses)
                // would target a `B__` enum, which has no fields. A mutating
                // accessor on the companion enum is a deferred follow-on — refuse
                // honestly rather than miscompile. `self.field = ...` inside a
                // method is EXEMPT: `self` is the concrete struct (the method body
                // runs on a `Account`/`Savings`, not `Account__`), so the write is
                // an ordinary in-place struct-field store.
                if !matches!(obj.as_ref(),
                             Expr::Ident(n, _) if n == "self"
                                 || self.concrete_struct_params.contains(n)) {
                    if let Ty::Class(b, _) = self.type_of_expr(obj) {
                        if self.is_polymorphic_base(&b) {
                            return Err(crate::diag::Error::Codegen(format!(
                                "writing field `{}` through a polymorphic-base `{}` variable \
                                 is not yet supported — a mutating field accessor on the \
                                 companion enum is a deferred follow-on (read-only base-field \
                                 access is supported)",
                                attr, b
                            )));
                        }
                    }
                }
                // (card 30e4fdd0) Some-wrap a bare value assigned to an
                // Optional[Class] FIELD, mirroring the local-declaration and return
                // positions (which already coerce). Field-assign was the one
                // Optional-coercion position that didn't wrap, so `self.slot = Inner(5)`
                // where `slot: Optional[Inner]` leaked rustc E0308 (`expected
                // Option<Inner>, found Inner`). The field type is resolved from the
                // receiver's class (inheritance-aware); `coerce_to_option` is a no-op
                // for a non-Optional field / an already-Optional value / a `None`
                // literal, so every existing field write is byte-for-byte unchanged.
                let field_info = if let Ty::Class(cn, _) = self.type_of_expr(obj) {
                    self.ctx
                        .classes
                        .get(&cn)
                        .cloned()
                        .and_then(|cd| self.class_field_type(&cd, attr))
                        .map(|ft| (cn, ft))
                } else {
                    None
                };
                let v = self.emit_consuming(value)?;
                let v = if let Some((cn, ft)) = &field_info {
                    let coerced = self.coerce_to_option(v, value, ft);
                    // (card 30e4fdd0) A boxed-recursive field STORES `Option<Box<Node>>`
                    // (the struct boxes the inline self-reference to break E0072).
                    // Box the (Some-wrapped) value at the write boundary:
                    // `.map(Box::new)` boxes the inner value and leaves `None`
                    // unchanged, keeping storage consistent with the boxed field type
                    // while the type system still sees a Box-blind `Option<Node>`.
                    if self.field_needs_box(cn, ft) {
                        format!("({}).map(::std::boxed::Box::new)", coerced)
                    } else {
                        coerced
                    }
                } else {
                    v
                };
                // The base must be emitted as a *place* (lvalue), not the
                // clone-based rvalue emit_expr produces for Attr/Index.
                // (card cc7ae370, item 1) Hoist every subscript index in the base
                // chain into preceding `let __idxN` temps so a self-referential
                // index (`row[len(row) - 1].field = v`) does not shared-borrow the
                // base inside the place while the field store mutably borrows it
                // (E0502).
                let mut prelude = Vec::new();
                let place = self.emit_place_hoisted(obj, &mut prelude)?;
                for l in &prelude { self.line(l); }
                // (EPIC-6) Escape a keyword field name in the field-WRITE target so
                // it matches the (escaped) struct field def.
                self.line(&format!("{}.{} = {};", place, escape_ident(attr), v));
            }
            Stmt::IndexAssign { obj, idx, value, .. } => {
                let v = self.emit_consuming(value)?;
                // (card cc7ae370, item 1) Hoist EVERY subscript index — the
                // base-chain ones (`emit_place_hoisted`) AND the leaf index below —
                // into preceding `let __idxN` temps BEFORE the mutable store. An
                // index that READS the same base (`self.data[len(self.data) - 1] =
                // v`, or a nested `grid[len(grid) - 1][0] = v`) would otherwise
                // shared-borrow the base inside the subscript while the store
                // mutably borrows it — rustc E0502. A place string cannot contain a
                // `let`, so the temps are emitted as preceding statements here. The
                // leaf temp is numbered AFTER the base chain (`prelude.len()`) so it
                // never collides. Binding also parenthesizes any nested-subscript
                // block index cleanly and evaluates a side-effecting index once.
                let mut prelude = Vec::new();
                let place = self.emit_place_hoisted(obj, &mut prelude)?;
                // Dispatch on the base's collection kind (dict -> HashMap::insert,
                // list -> indexed store). type_of_expr resolves chained bases
                // (self.dict, grid[r], ...), not just bare locals.
                let is_dict = matches!(self.type_of_expr(obj), Ty::Dict(..));
                if is_dict {
                    // HashMap::insert takes ownership of the key, so emit it owned
                    // (a String key var becomes `k.clone()`; Copy keys are unchanged).
                    let k = self.emit_consuming(idx)?;
                    for l in &prelude { self.line(l); }
                    self.line(&format!("{}.insert({}, {});", place, k, v));
                } else {
                    let i = self.emit_expr(idx)?;
                    let leaf = format!("__idx{}", prelude.len());
                    for l in &prelude { self.line(l); }
                    self.line(&format!("let {} = {};", leaf, i));
                    self.line(&format!("{}[({}) as usize] = {};", place, leaf, v));
                }
            }
            Stmt::Match { subject, arms, .. } => {
                // Clone (do not move) a non-Copy scrutinee place so it stays usable
                // after the match — uniform clone-on-use.
                let subj = self.emit_consuming(subject)?;
                let subj_ty = self.type_of_expr(subject);
                let temp_var = "__match_val".to_string();
                self.line(&format!("let {} = {};", temp_var, subj));
                self.emit_match_arms(&temp_var, arms, true, &subj_ty)?;
            }
            // (first-class functions, Increment 2) A NESTED `def` lowers to a
            // NAMED local closure `let <name> = Rc::new(move |..| { <block> }) as
            // Rc<dyn Fn(..) -> Ret>;`. typeck has already registered it as a
            // `Ty::Func` local, rejected self-recursion / captured mutation /
            // nested generics+generators, and checked the body with the enclosing
            // scope visible — so here we only emit the closure and record the
            // local so later parent statements (`return <name>` / `<name>(args)`)
            // resolve it as a func-valued place.
            Stmt::Func(f) => self.emit_nested_def(f)?,
            Stmt::Class(_) => {
                // Nested classes — punt.
                self.line("// TODO: nested class");
            }
        }
        Ok(())
    }

    /// (card 602b1675) Emit ONE target of a temp-split tuple unpack: `target` gets
    /// the already-evaluated component value held in `rhs` (a fresh `__pyrst_unpack_`
    /// temp), typed `comp_ty`. `rhs` never re-reads `target`, so — unlike scalar
    /// `Stmt::Assign` whose RHS reads the place — there is no LHS/RHS name collision
    /// and each component is handled independently. The declare / reassign / shadow /
    /// reconverge decisions mirror the scalar reassign path (incl. the hoisted-local
    /// shadow-mangling and int->float slot widening) so a mixed or divergent unpack
    /// component behaves exactly like the equivalent single assignment.
    pub(crate) fn emit_unpack_target(&mut self, target: &str, rhs: &str, comp_ty: &Ty) {
        let target_e = escape_ident(target);
        if !self.declared.contains(target) {
            // Fresh binding.
            self.declared.insert(target.to_string());
            self.locals.insert(target.to_string(), comp_ty.clone());
            self.line(&format!("let mut {} = {};", target_e, rhs));
            return;
        }
        if let Some((_, slot_ty)) = self.shadow_map.get(target).cloned() {
            // A divergent shadow of this hoisted local is live (card 575bcf3a).
            if !Self::types_conflict(&slot_ty, comp_ty) {
                self.shadow_map.remove(target);
                self.locals.insert(target.to_string(), slot_ty);
                self.line(&format!("{} = {};", target_e, rhs));
            } else {
                let m = self.fresh_shadow_name(target);
                self.shadow_map.insert(target.to_string(), (m.clone(), slot_ty));
                self.locals.insert(target.to_string(), comp_ty.clone());
                self.line(&format!("let mut {} = {};", m, rhs));
            }
            return;
        }
        let cur = self.locals.get(target).cloned().unwrap_or(Ty::Unknown);
        if Self::types_conflict(&cur, comp_ty) {
            if self.hoisted.contains(target) {
                let m = self.fresh_shadow_name(target);
                self.shadow_map.insert(target.to_string(), (m.clone(), cur));
                self.locals.insert(target.to_string(), comp_ty.clone());
                self.line(&format!("let mut {} = {};", m, rhs));
            } else {
                self.locals.insert(target.to_string(), comp_ty.clone());
                self.line(&format!("let mut {} = {};", target_e, rhs));
            }
        } else if matches!(cur, Ty::Float) && matches!(comp_ty, Ty::Int) {
            // Int component into a float slot — cast to keep the slot's type.
            self.line(&format!("{} = {} as f64;", target_e, rhs));
        } else {
            self.line(&format!("{} = {};", target_e, rhs));
        }
    }

    /// (first-class functions, Increment 2) Emit a NESTED `def` as a named local
    /// closure bound with `let`. The closure is `move` (it captures every
    /// referenced enclosing variable BY VALUE — Rust moves a captured binding into
    /// the closure; pyrst's value semantics make that the right default, and a
    /// captured non-`Copy` value the closure keeps using is the closure's own
    /// copy). Its body is the def's full STATEMENT BLOCK — the key difference from
    /// a lambda (a single expression): we reuse the SAME body-emission machinery
    /// as a top-level `fn` (prescan → hoist → `emit_stmt` loop), so the def's
    /// `return`s become the closure's returns and all statement forms are
    /// supported. The bound name is a `Rc<dyn Fn(..)>` local, so a later
    /// `<name>(args)` / `return <name>` flows through the existing func-value paths
    /// (clone-on-use = a cheap `Rc` refcount bump), exactly like a `Callable`
    /// param or a lambda-bound local.
    pub(crate) fn emit_nested_def(&mut self, f: &Func) -> Result<()> {
        // Lower the nested signature. The nested def's annotations are scoped to
        // the ENCLOSING function's generic type params (typeck used the same
        // scope), so a `T` named in a nested annotation lowers to the same Rust
        // generic the enclosing `fn` declares.
        let param_tys: Vec<Ty> = f.params.iter()
            .map(|p| Ty::from_type_expr(&p.ty, p.span))
            .collect::<Result<Vec<_>>>()?;
        let ret = Ty::from_type_expr(&f.ret, f.span)?;
        let func_ty = Ty::Func(param_tys.clone(), Box::new(ret.clone()));
        let rc_ty = self.rust_ty(&func_ty);

        // Closure parameter list: `name: T` per param. A by-value closure param is
        // bound `mut` (matching `fn` params) so the body may mutate it / its
        // fields in place; unused-mut is allowed in the generated crate.
        let mut param_strs = Vec::with_capacity(f.params.len());
        for (p, pty) in f.params.iter().zip(param_tys.iter()) {
            param_strs.push(format!("mut {}: {}", escape_ident(&p.name), self.rust_ty(pty)));
        }
        let name_e = escape_ident(&f.name);
        let ret_s = self.rust_ty(&ret);

        // (card 56e46767) Clone-on-capture for non-Copy CAPTURES. The closure is a
        // `move` closure (below), so every captured enclosing binding is MOVED into
        // it — and a captured non-`Copy` value the ENCLOSING scope still reads after
        // the def would be a raw rustc E0382 (borrow of moved). pyrst nested defs
        // document value-semantics capture (a SNAPSHOT taken at the def site), so we
        // CLONE each such capture into a block-scoped binding the closure moves,
        // leaving the enclosing original intact. This is python3-faithful for a READ
        // of the original (it still sees the pre-def value); the snapshot does NOT
        // observe a LATER mutation of the original — the same documented divergence
        // from CPython that `gen_closure_capture` already carries for generators.
        //
        // Compute the def's captured free variables (the same free-var analysis
        // typeck's liveness/capture gates use) and keep only those that must be
        // cloned:
        //   * an enclosing VALUE local of this name exists (in `self.locals`) — a
        //     builtin / top-level fn / the def's own not-yet-registered name is not,
        //     so those are skipped;
        //   * its type is non-`Copy` — a Copy capture (Int/Float/Bool/…) is moved as
        //     a bitwise copy and needs no clone, keeping the Copy-capture goldens
        //     (nested_closures, …) byte-identical (empty set → no block wrapper);
        //   * it is NOT a `Mut[T]` by-ref param (`by_ref_locals`) — cloning a `&mut`
        //     would silently SNAPSHOT a by-REFERENCE param, dropping its aliasing;
        //     keep the current move for those (works when not read after the def).
        // Generator captures are rejected at check (`check_nested_def`): `Gen` is not
        // `Clone` and sharing (not snapshotting) is the correct semantics, so every
        // capture that reaches here is `Clone`. Each name resolves to its READ-name
        // (the mangled shadow binding when an active divergent shadow hides the slot,
        // exactly like the closure body's reads — b5ea0d5 keeps that shadow_map), so
        // `let <rn> = <rn>.clone();` clones the same binding the body will read.
        let mut captured: std::collections::HashSet<String> = std::collections::HashSet::new();
        crate::typeck::nested_def_captured_reads(f, &mut captured);
        let mut clone_caps: Vec<String> = captured.into_iter()
            .filter(|n| {
                self.locals.get(n).map(|ty| !self.is_copy_type(ty)).unwrap_or(false)
                    && !self.by_ref_locals.contains(n)
            })
            .map(|n| self.shadow_read_name(&n).unwrap_or_else(|| escape_ident(&n)))
            .collect();
        clone_caps.sort();
        clone_caps.dedup();
        let wrap_block = !clone_caps.is_empty();

        // Open the binding + closure. The `-> Ret` is always written (uniform with
        // a `() -> ()` unit nested def); the block body follows on its own lines.
        // When there are non-Copy captures, the `Rc::new(..)` becomes the TAIL
        // expression of a wrapping block that first binds the `.clone()` snapshots
        // the `move` closure captures.
        if wrap_block {
            self.line(&format!("let {} = {{", name_e));
            self.indent += 1;
            for rn in &clone_caps {
                self.line(&format!("let {} = {}.clone();", rn, rn));
            }
            self.line(&format!(
                "::std::rc::Rc::new(move |{}| -> {} {{",
                param_strs.join(", "),
                ret_s
            ));
        } else {
            self.line(&format!(
                "let {} = ::std::rc::Rc::new(move |{}| -> {} {{",
                name_e,
                param_strs.join(", "),
                ret_s
            ));
        }
        self.indent += 1;

        // --- enter the nested scope ------------------------------------------------
        // Save every piece of per-function emission state so the nested closure's
        // body emits in its OWN context and the enclosing function's context is
        // restored verbatim afterwards.
        //
        // locals/declared: the closure CAPTURES the enclosing locals (already in
        // `self.locals`), so we KEEP them visible and overlay the nested params.
        // We snapshot both so the nested params / nested locals never leak back to
        // the parent. `declared` starts EMPTY for the closure body (a fresh Rust
        // scope: nothing is `let`-declared yet), and is restored to the parent's
        // set on exit.
        let saved_locals = self.locals.clone();
        let saved_declared = std::mem::take(&mut self.declared);
        let saved_ret_ty = std::mem::replace(&mut self.current_ret_ty, ret.clone());
        // A nested def owns its own control flow: a `return`/`break`/`continue` in
        // it is local to the closure (or its own loops), never an escape from an
        // enclosing `try:` body. Suspend both try-escape flags (typeck also
        // forbade `yield` here, so `in_generator` is irrelevant, but we reset it
        // for safety/symmetry with `emit_func`).
        let saved_try_return = std::mem::replace(&mut self.try_return_escape, false);
        let saved_try_loopctl = std::mem::replace(&mut self.try_loopctl_escape, false);
        let saved_in_generator = std::mem::replace(&mut self.in_generator, false);
        // The closure's by-reference locals start empty (nested defs take no
        // `Mut[T]` params in this increment); save+restore mirrors `emit_func`.
        let saved_by_ref = std::mem::take(&mut self.by_ref_locals);
        // (card 575bcf3a) The hoist-slot set, active shadows, and the shadow
        // counter are per-function: the nested closure gets its OWN (empty / 0) and
        // the enclosing function's are restored on exit, mirroring locals/declared.
        let saved_hoisted = std::mem::take(&mut self.hoisted);
        // (card 575bcf3a fix) KEEP the definition-site shadow_map (only a CLONE is
        // saved for restore) so a READ of a captured enclosing name that is actively
        // shadowed at this point resolves to the mangled binding the `move` closure
        // captures — not the stale, unreconverged outer slot (was rustc E0599, or a
        // silent stale read for overlapping-API type pairs). The closure's OWN new
        // shadows are layered on top and cleaned by the restore on exit. Names the
        // closure RE-BINDS (params, and its own hoisted locals below) drop their
        // captured-shadow entry so they resolve to the closure's binding.
        let saved_shadow_map = self.shadow_map.clone();
        let saved_shadow_counter = std::mem::replace(&mut self.shadow_counter, 0);

        // Overlay the nested params onto the captured locals (a param SHADOWS a
        // captured enclosing name of the same identifier).
        for (p, pty) in f.params.iter().zip(param_tys.iter()) {
            self.locals.insert(p.name.clone(), pty.clone());
            // (param-mutation fix) Seed the nested closure's params into the
            // (freshly-emptied) `declared` set for the SAME reason as `emit_func`:
            // the closure params are emitted `mut <name>: T`, so a reassignment of
            // one — top level or nested in a loop/if — must lower to a mutation,
            // not a block-scoped shadowing `let mut`. Nested defs take no `Mut[T]`
            // params in this increment, so every param is a value binding here.
            self.declared.insert(p.name.clone());
            // A param NAME shadows any captured enclosing shadow of the same
            // identifier — reads inside the closure must resolve to the param, not
            // the captured mangled binding.
            self.shadow_map.remove(&p.name);
        }

        // Same body pipeline as `emit_func`: forward type inference, then hoist
        // block-first-assigned locals to the (closure) scope top, then emit the
        // statements. `prescan_types` / `collect_hoistable` do not descend into a
        // doubly-nested def, so they stay scoped to THIS closure's own body.
        self.prescan_types(&f.body);
        // (LAZY-GEN V1-d BLOCKER insurance) Same divergent-join assertion as
        // `emit_func`, on THIS closure's own body.
        self.assert_no_branch_divergence(&f.body)?;
        // (fix-b insurance) Same read-after-conflicting-reassign assertion.
        let param_names: std::collections::HashSet<String> = f.params.iter().map(|p| p.name.clone()).collect();
        self.assert_no_read_after_divergent_reassign(&f.body, &param_names)?;
        let mut block_assigned = std::collections::HashSet::new();
        Self::collect_hoistable(&f.body, 0, &mut block_assigned);
        let params: std::collections::HashSet<&str> = f.params.iter().map(|p| p.name.as_str()).collect();
        let mut hoist: Vec<String> = block_assigned.into_iter()
            .filter(|n| !params.contains(n.as_str()) && !self.declared.contains(n))
            .collect();
        hoist.sort();
        for hname in hoist {
            let ty = self.locals.get(&hname).cloned().unwrap_or(Ty::Unknown);
            if let Some(def) = self.default_val(&ty) {
                self.line(&format!("let mut {}: {} = {};", escape_ident(&hname), self.rust_ty(&ty), def));
                // (card 575bcf3a) Record the closure's function-scope slot. A closure
                // local of this name is its OWN binding, so drop any captured
                // definition-site shadow of the same identifier.
                self.shadow_map.remove(&hname);
                self.hoisted.insert(hname.clone());
                self.declared.insert(hname);
            }
        }
        for s in &f.body {
            self.emit_stmt(s)?;
        }

        // --- leave the nested scope ------------------------------------------------
        self.locals = saved_locals;
        self.declared = saved_declared;
        self.current_ret_ty = saved_ret_ty;
        self.try_return_escape = saved_try_return;
        self.try_loopctl_escape = saved_try_loopctl;
        self.in_generator = saved_in_generator;
        self.by_ref_locals = saved_by_ref;
        self.hoisted = saved_hoisted;
        self.shadow_map = saved_shadow_map;
        self.shadow_counter = saved_shadow_counter;

        self.indent -= 1;
        if wrap_block {
            // Close the closure as the wrapping block's TAIL expression (no `;`),
            // then close the block that owns the `.clone()` snapshot bindings.
            self.line(&format!("}}) as {}", rc_ty));
            self.indent -= 1;
            self.line("};");
        } else {
            self.line(&format!("}}) as {};", rc_ty));
        }

        // Record the nested def as a func-valued local + declared name in the
        // ENCLOSING scope, so the rest of the parent body resolves `<name>` as an
        // `Rc<dyn Fn>` place (clone-on-use for a return/argument, direct call for
        // `<name>(args)`). Define-before-use: every reference follows this point.
        self.locals.insert(f.name.clone(), func_ty);
        self.declared.insert(f.name.clone());
        Ok(())
    }


    pub(crate) fn emit_try(
        &mut self,
        body: &[Stmt],
        handlers: &[ExceptHandler],
        else_: &Option<Vec<Stmt>>,
        finally_: &Option<Vec<Stmt>>,
    ) -> Result<()> {
                self.line("{");
                self.indent += 1;

                // Run the try body inside catch_unwind. pyrst's `raise` compiles
                // to a panic whose payload is a formatted string (see Stmt::Raise).
                // The exception type and message are separated by a NUL byte (`\0`),
                // a delimiter that cannot occur in pyrst user data:
                //   raise Foo("m")  -> "Foo\0m"
                //   raise Foo       -> "Foo\0"   (empty message)
                //   raise           -> "explicit raise"
                //
                // Suppress the default panic hook while the try body runs so that a
                // *caught* exception produces no stderr noise.  The hook is saved and
                // restored immediately after catch_unwind so that an *uncaught*
                // exception (re-raised via resume_unwind below) still goes through the
                // caller's hook and the Rust runtime prints a useful message + aborts
                // with a non-zero exit code.
                self.line("let __prev_hook = ::std::panic::take_hook();");
                self.line("::std::panic::set_hook(::std::boxed::Box::new(|_| {}));");
                // (try/except control flow) The try BODY runs inside a closure, so
                // a `return`/`break`/`continue` cannot directly leave the enclosing
                // function/loop. The closure instead returns a `__PyrstTryFlow<R>`
                // (R = the enclosing function's Rust return type): escaping control
                // flow in the body is lowered to `return __PyrstTryFlow::Return(v)`
                // / `::Break` / `::Continue` (see the `Stmt::Return`/`Break`/
                // `Continue` arms gated on `try_return_escape`/`try_loopctl_escape`),
                // and the closure's tail is `__PyrstTryFlow::Normal`. The signal is
                // re-issued as a real `return`/`break`/`continue` AFTER the try
                // lowering (and after `finally`) so all of finally / else / handler
                // dispatch still run on every exit. `try_return_escape` stays set
                // through nested loops (a `return` there still escapes the function)
                // but `try_loopctl_escape` is suspended inside nested loops/defs.
                let flow_ty = format!("__PyrstTryFlow<{}>", self.rust_ty(&self.current_ret_ty));
                self.line(&format!(
                    "let __try_result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| -> {} {{",
                    flow_ty
                ));
                self.indent += 1;
                let saved_ret_escape = std::mem::replace(&mut self.try_return_escape, true);
                let saved_loopctl_escape = std::mem::replace(&mut self.try_loopctl_escape, true);
                // (card 575bcf3a) The try body runs in its own closure/scope; isolate
                // its block-scope emission state (locals / declared / shadow_map).
                let __try_scope = self.scope_enter();
                for s in body { self.emit_stmt(s)?; }
                self.scope_exit(__try_scope);
                self.try_return_escape = saved_ret_escape;
                self.try_loopctl_escape = saved_loopctl_escape;
                self.line("__PyrstTryFlow::Normal");
                self.indent -= 1;
                self.line("}));");
                self.line("::std::panic::set_hook(__prev_hook); // restore before any re-raise");

                // Whether the body can actually emit an escaping break / continue
                // at the try-body level — drives whether the post-lowering flow
                // match emits a real `break` / `continue` arm. (Emitting one when
                // none can occur would put `break`/`continue` outside any loop and
                // fail rustc even though unreachable; emitting one when a break is
                // present but the try is NOT in a loop is the honest E0268 the user
                // should see.)
                let body_breaks = try_body_has_loopctl(body, /*want_break=*/ true);
                let body_continues = try_body_has_loopctl(body, /*want_break=*/ false);
                // The flow holder, threaded out of the Ok arm and acted on after
                // `finally`. Only declared/used when the body can escape.
                let body_returns = body_has_try_level_return(body);
                let emit_flow = body_returns || body_breaks || body_continues;
                if emit_flow {
                    self.line(&format!("let mut __pyrst_flow: {} = __PyrstTryFlow::Normal;", flow_ty));
                }

                // (try/except-as-value, BUG 2) Whether this `try` definitely
                // returns on EVERY path — the same rule typeck's all-paths-return
                // gate uses (`stmt_definitely_returns`'s Try arm): a returning
                // `finally`, or every handler returns AND (the body returns OR a
                // returning `else`). When true, the lowering's NORMAL fall-through
                // is genuinely unreachable, so the generated block must DIVERGE
                // rather than fall off with an implicit `()` (which would make a
                // function whose last statement is such a try fail rustc E0308 /
                // E0317). We make the block's tail expression diverge below: the
                // flow `match`'s catch-all becomes `unreachable!()` (when a flow
                // match is emitted), or a trailing `unreachable!()` is appended
                // (when no flow match is emitted, e.g. a returning `finally`).
                // Mirrors typeck's `stmt_definitely_returns` Try arm EXACTLY (incl.
                // the vacuously-true empty-handlers case, so a `try: return v
                // finally: ...` with no `except` is recognized as all-returning and
                // its NORMAL fall-through is diverged below — otherwise it would
                // type-check yet fail rustc with an implicit `()` tail).
                let try_returns = {
                    use crate::typeck::block_definitely_returns as bdr;
                    if finally_.as_ref().is_some_and(|f| bdr(f)) {
                        true
                    } else {
                        handlers.iter().all(|h| bdr(&h.body))
                            && (bdr(body) || else_.as_ref().is_some_and(|e| bdr(e)))
                    }
                };

                // Whether any handler can catch every exception type.
                let has_catch_all = handlers.iter().any(|h| {
                    h.exc_type.is_none() || h.exc_type.as_deref() == Some("Exception")
                });

                // Accumulate the panic message string in case we need to print it to
                // stderr on an unmatched re-raise (the payload Box is moved into
                // resume_unwind, so we must capture the string before that). It is
                // only reassigned on a re-raise path; a catch-all try never re-raises,
                // so emit a non-`mut` binding there to avoid an unused-mut warning.
                let reraise_possible = handlers.is_empty() || !has_catch_all;
                let reraise_binding = if reraise_possible { "let mut" } else { "let" };
                self.line(&format!(
                    "{} __reraise_msg: ::std::option::Option<String> = ::std::option::Option::None;",
                    reraise_binding
                ));

                // __reraise holds the original panic payload when no handler
                // matched, so it can be re-raised after the finally block.
                self.line("let __reraise: ::std::option::Option<::std::boxed::Box<dyn ::std::any::Any + ::std::marker::Send>> = match __try_result {");
                self.indent += 1;

                // Success path (no exception): the closure handed back a flow
                // signal. The `else` body runs ONLY when the body fell through
                // normally (Python: `else` runs iff the try body completed without
                // exception AND without return/break/continue). The signal is then
                // stashed so the post-`finally` match can act on a Return/Break/
                // Continue. No re-raise on this path.
                self.line("::std::result::Result::Ok(__flow) => {");
                self.indent += 1;
                if let Some(else_body) = else_ {
                    self.line("if let __PyrstTryFlow::Normal = &__flow {");
                    self.indent += 1;
                    let __else_scope = self.scope_enter();
                    for s in else_body { self.emit_stmt(s)?; }
                    self.scope_exit(__else_scope);
                    self.indent -= 1;
                    self.line("}");
                }
                if emit_flow {
                    self.line("__pyrst_flow = __flow;");
                } else {
                    self.line("let _ = __flow;");
                }
                self.line("::std::option::Option::None");
                self.indent -= 1;
                self.line("}");

                // Error path: recover the payload string, parse out the type, and
                // dispatch to the matching handler.
                self.line("::std::result::Result::Err(__payload) => {");
                self.indent += 1;
                self.line("let __exc_str: String = if let Some(s) = __payload.downcast_ref::<&str>() {");
                self.line("    (*s).to_string()");
                self.line("} else if let Some(s) = __payload.downcast_ref::<String>() {");
                self.line("    s.clone()");
                self.line("} else {");
                self.line("    String::from(\"unknown panic\")");
                self.line("};");
                // Split "<Type>\0<msg>" on the NUL delimiter (which cannot appear in
                // user data); otherwise type == msg == whole string. split_once takes
                // the message verbatim after the delimiter, so a message that contains
                // the old " panic: " text is preserved intact.
                self.line("let (__exc_type, __exc_msg): (String, String) = match __exc_str.split_once('\\0') {");
                self.line("    Some((t, m)) => (t.to_string(), m.to_string()),");
                self.line("    None => (__exc_str.clone(), __exc_str.clone()),");
                self.line("};");
                self.line("let _ = &__exc_type; let _ = &__exc_msg;");

                if handlers.is_empty() {
                    // No handlers at all: always re-raise.
                    self.line("__reraise_msg = ::std::option::Option::Some(__exc_str.clone());");
                    self.line("::std::option::Option::Some(__payload)");
                } else {
                    let mut first = true;
                    for h in handlers {
                        let is_catch_all =
                            h.exc_type.is_none() || h.exc_type.as_deref() == Some("Exception");
                        let cond = if is_catch_all {
                            "true".to_string()
                        } else {
                            // Build an OR-expansion over the transitive descendant set of
                            // the handler's exception type so that, e.g., `except LookupError`
                            // matches both KeyError and IndexError.  For unknown/user-defined
                            // types exc_descendants returns an empty vec and we fall through to
                            // the plain exact-match path.
                            let exc_name = h.exc_type.as_deref().unwrap();
                            let descendants = exc_descendants(exc_name);
                            if descendants.is_empty() {
                                // Unknown / user-defined type: exact match only (original behaviour).
                                format!("__exc_type == {:?}", exc_name)
                            } else {
                                // OR-expand over base + all transitive subclasses.
                                let clauses: Vec<String> = descendants
                                    .iter()
                                    .map(|d| format!("__exc_type == {:?}", d))
                                    .collect();
                                format!("({})", clauses.join(" || "))
                            }
                        };
                        if first {
                            self.line(&format!("if {} {{", cond));
                            first = false;
                        } else {
                            self.line(&format!("}} else if {} {{", cond));
                        }
                        self.indent += 1;
                        // (EPIC-6) `except E as <name>:` binds a user local; escape
                        // it and the suppression read so a keyword name compiles.
                        // Register it as a SCOPED local (Str) for the handler body
                        // so a same-named MODULE CONST is shadowed only INSIDE the
                        // handler — without this scoping, a bare reference to a
                        // const-named exc binding (e.g. `except ... as e` next to a
                        // const `e`) would mangle to the const, and conversely a
                        // const read outside the handler must still resolve to the
                        // const. Save/restore around the body.
                        // (card 575bcf3a) Isolate the handler body's block-scope
                        // state; the exc binding below is captured inside the window.
                        let __handler_scope = self.scope_enter();
                        let exc_saved = if let Some(name) = &h.exc_name {
                            let name_e = escape_ident(name);
                            self.line(&format!("let {} = __exc_msg.clone();", name_e));
                            self.line(&format!("let _ = &{};", name_e));
                            let prev = self.locals.get(name).cloned();
                            self.locals.insert(name.clone(), Ty::Str);
                            Some((name.clone(), prev))
                        } else {
                            None
                        };
                        for s in &h.body { self.emit_stmt(s)?; }
                        if let Some((name, prev)) = exc_saved {
                            match prev {
                                Some(ty) => { self.locals.insert(name, ty); }
                                None => { self.locals.remove(name.as_str()); }
                            }
                        }
                        self.scope_exit(__handler_scope);
                        self.line("::std::option::Option::None");
                        self.indent -= 1;
                    }
                    // Trailing else: if no catch-all handler exists, propagate.
                    if has_catch_all {
                        self.line("} else { ::std::option::Option::None }");
                    } else {
                        self.line("} else { __reraise_msg = ::std::option::Option::Some(__exc_str.clone()); ::std::option::Option::Some(__payload) }");
                    }
                }
                self.indent -= 1;
                self.line("}");

                self.indent -= 1;
                self.line("};");

                // finally: runs on every path, before any re-raise.
                if let Some(fin) = finally_ {
                    let __finally_scope = self.scope_enter();
                    for s in fin { self.emit_stmt(s)?; }
                    self.scope_exit(__finally_scope);
                }

                // Re-raise an unmatched exception (after finally).
                // Print the exception message to stderr first so the user sees a
                // useful error; resume_unwind then aborts with a non-zero exit code.
                self.line("if let ::std::option::Option::Some(__p) = __reraise { if let ::std::option::Option::Some(ref __msg) = __reraise_msg { eprintln!(\"{}\", __msg); } ::std::panic::resume_unwind(__p); }");

                // (try/except control flow) Now that `finally` has run and any
                // unmatched exception has been re-raised, act on a control-flow
                // signal that escaped the try body: re-issue it as a real function
                // `return` / loop `break` / loop `continue`. A `Break`/`Continue`
                // arm is emitted only when the body can actually produce it (so a
                // loop-free try with only `return` does not put a stray `break`
                // outside a loop), and a `Continue` arm is omitted likewise. The
                // `Return(__v) => return __v` arm is always valid: `__v: R`.
                if emit_flow {
                    self.line("match __pyrst_flow {");
                    self.indent += 1;
                    self.line("__PyrstTryFlow::Return(__v) => return __v,");
                    if body_breaks {
                        self.line("__PyrstTryFlow::Break => break,");
                    }
                    if body_continues {
                        self.line("__PyrstTryFlow::Continue => continue,");
                    }
                    // Normal (and any non-produced variant) falls through past the
                    // try. When the try definitely returns on every path, Normal is
                    // unreachable and the catch-all DIVERGES so the whole try block
                    // (a Rust block-as-statement) has type `!` instead of `()` —
                    // letting a function whose last statement is this try compile.
                    if try_returns {
                        self.line("_ => unreachable!()");
                    } else {
                        self.line("_ => {}");
                    }
                    self.indent -= 1;
                    self.line("}");
                } else if try_returns {
                    // No body-level escape, but the try still definitely returns
                    // (a returning `finally`, or returning handlers + a returning
                    // `else`). Those returns were emitted as REAL `return`s outside
                    // the catch_unwind closure, so control never actually reaches
                    // here — but rustc cannot see that and would type the block by
                    // its `()` re-raise tail. A trailing `unreachable!()` (the
                    // block's tail expression, no `;`) gives the block type `!`.
                    self.line("unreachable!()");
                }

                self.indent -= 1;
                self.line("}");
        Ok(())
    }

    // The body of this helper is moved verbatim from the former `Expr::Call`
    // arm of `emit_expr`, whose match binding typed `callee`/`args`/`kwargs` as
    // `&Box<Expr>` / `&Vec<_>`. Keeping those exact parameter types lets the
    // moved code (`callee.as_ref()`, `args[..]`, `kwargs.iter()`, ...) compile
    // unchanged, so the emitted Rust is byte-for-byte identical.
    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<String> {
                if let Some(__s) = self.emit_builtin_call(callee, args, kwargs)? { return Ok(__s); }

                if let Some(__s) = self.emit_constructor_call(callee, args, kwargs)? { return Ok(__s); }

                if let Some(__s) = self.emit_super_method_call(callee, args)? { return Ok(__s); }

                if let Some(__s) = self.emit_method_call_on_attr(callee, args, kwargs)? { return Ok(__s); }

                self.emit_plain_func_call(callee, args, kwargs)
    }

    /// Emit a REGULAR function call (not a builtin / constructor / super /
    /// method) — the tail of [`Codegen::emit_call`]. Split out so the qualified
    /// module-call re-dispatch can reach it DIRECTLY: a flat module function
    /// whose name COLLIDES with a builtin (e.g. `math.pow` vs the builtin `pow`)
    /// must call the module function, not the builtin, so it must NOT re-enter
    /// `emit_builtin_call`. This applies the same Optional / by-ref /
    /// default-argument coercion as a bare flat call.
    #[allow(clippy::borrowed_box)]
    pub(crate) fn emit_plain_func_call(
        &mut self,
        callee: &Box<Expr>,
        args: &[Expr],
        kwargs: &[(String, Expr)],
    ) -> Result<String> {
                // (kwargs v1, card 8a7b7714) A keyword-bearing call maps each
                // keyword to its parameter slot and lowers as a FULL positional
                // call in parameter order (defaults injected into unfilled
                // slots). typeck already validated the mapping; re-deriving it
                // here is deterministic. A keyword on a callee WITHOUT a known
                // signature (function-valued local, lambda) cannot be mapped —
                // typeck rejects it, this is the defensive backstop.
                let kw_sig: Option<crate::typeck::FuncSig> = if !kwargs.is_empty() {
                    let sig = if let Expr::Ident(n, _) = callee.as_ref() {
                        self.ctx.funcs.get(n.as_str()).cloned()
                    } else {
                        None
                    };
                    match sig {
                        Some(s) => Some(s),
                        None => {
                            return Err(crate::diag::Error::Codegen(
                                "keyword arguments require a callee with a known signature \
                                 (a user function, module function, or method)"
                                    .into(),
                            ));
                        }
                    }
                } else {
                    None
                };
                let kw_slots: Option<Vec<crate::typeck::ArgSlot>> = match (&kw_sig, callee.as_ref()) {
                    (Some(sig), Expr::Ident(n, _)) => Some(crate::typeck::map_kwargs_to_slots(
                        n,
                        sig,
                        args.len(),
                        kwargs,
                        callee.span(),
                    )?),
                    _ => None,
                };
                // Regular function call (not a class).
                // (EPIC-5) Look up the callee signature so an argument flowing
                // into an `Optional[T]` parameter is wrapped (`Some(..)` for a
                // bare value, `None` for the None literal, pass-through for an
                // already-Optional value) — the same coercion as assignment and
                // return. Methods / unknown callees keep the bare emission.
                let mut param_tys: Vec<Ty> = if let Expr::Ident(n, _) = callee.as_ref() {
                    self.ctx.funcs.get(n.as_str())
                        .map(|sig| sig.params.iter().map(|(_, t)| t.clone()).collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                // Generics: monomorphize the param-type slots for a GENERIC callee.
                // A `Callable[[T], T]` param emits its `Rc<dyn Fn(T) -> T>` cast and
                // (for a lambda arg) its closure param types from the slot type; if
                // that slot still names the type variable `T`, the cast leaks an
                // unbound `T` into the caller and rustc rejects it (E0425). So infer
                // the call's `{T -> concrete}` substitution from the ARGUMENT types
                // and apply it to every param-type slot, turning `Rc<dyn Fn(T)->T>`
                // into `Rc<dyn Fn(i64)->i64>`. Value params (`x: T`) are inferred by
                // Rust directly, but substituting them too is harmless. Non-generic
                // callees are unaffected (`generic_call_param_subst` returns None).
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if param_tys.iter().any(|t| Self::ty_mentions_typevar(t)) {
                        // Build the binding-source arg types. A LAMBDA argument is
                        // NOT an authoritative binding source: its parameter types
                        // are unknown, so its inferred body type is unreliable
                        // (`lambda x, y: x + y` defaults to int even when the call's
                        // T is str). It is CHECKED against the expected slot, not the
                        // other way round — so infer it as `Unknown` (non-binding)
                        // and let the concrete value arguments (`a, b: T`) drive the
                        // substitution. A named-function arg keeps its real
                        // `Ty::Func` type (a reliable binding source).
                        // (kwargs v1) With kwargs the binding sources are built
                        // SLOT-ALIGNED (keyword values in their mapped slots,
                        // default holes `Unknown` — non-binding), so unification
                        // sees each type at its declared parameter position.
                        let arg_tys: Vec<Ty> = match &kw_slots {
                            None => args.iter()
                                .map(|a| if matches!(a, Expr::Lambda { .. }) {
                                    Ty::Unknown
                                } else {
                                    self.type_of_expr(a)
                                })
                                .collect(),
                            Some(slots) => slots.iter()
                                .map(|s| {
                                    let a = match s {
                                        crate::typeck::ArgSlot::Pos(i) => &args[*i],
                                        crate::typeck::ArgSlot::Kw(j) => &kwargs[*j].1,
                                        crate::typeck::ArgSlot::Default => return Ty::Unknown,
                                    };
                                    if matches!(a, Expr::Lambda { .. }) {
                                        Ty::Unknown
                                    } else {
                                        self.type_of_expr(a)
                                    }
                                })
                                .collect(),
                        };
                        if let Some(subst) =
                            crate::typeck::generic_call_param_subst(n.as_str(), &arg_tys, self.ctx)
                        {
                            for pt in param_tys.iter_mut() {
                                *pt = crate::typeck::apply_typevar_subst(pt, &subst);
                            }
                        }
                    }
                }
                // (EPIC-4 V2-c) Per-arg by-reference (`Mut[T]`) flags for this
                // free-function callee. Parallel to `args` (free functions have no
                // `self`, so `param_by_ref[i]` lines up with `args[i]` directly).
                let param_by_ref: Vec<bool> = if let Expr::Ident(n, _) = callee.as_ref() {
                    self.ctx.funcs.get(n.as_str())
                        .map(|sig| sig.param_by_ref.clone())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                // (kwargs v1 — item A) A keyword-bearing call lowers through the
                // shared slot mapper, which preserves CPython's source-order
                // call-site evaluation (hoisting EVERY out-of-order argument —
                // positionals AND keyword values — into source-ordered temps,
                // while a by-reference arg hoists only its place-prelude) and
                // injects declared defaults into the unfilled slots. `coerced =
                // true` applies the Optional-wrap / `Callable` cast / poly-base
                // wrap per param slot.
                if let Some(slots) = &kw_slots {
                    let sig = kw_sig.as_ref().expect("kw slots imply a resolved signature");
                    let (prelude, call_parts) =
                        self.emit_slotted_args(slots, args, kwargs, sig, &param_tys, /*coerced=*/ true)?;
                    let callee_s = self.emit_expr(callee)?;
                    let callee_s = if matches!(callee.as_ref(), Expr::Lambda { .. }) {
                        format!("({})", callee_s)
                    } else {
                        callee_s
                    };
                    return Ok(Self::hoist_wrap(
                        &prelude,
                        format!("{}({})", callee_s, call_parts.join(", ")),
                    ));
                }

                // No keyword arguments: emit each positional argument in order.
                // A by-reference (`Mut[T]`) arg borrows the caller's PLACE so the
                // callee's mutation persists — typeck already required `a` to be a
                // place (Ident/Attr/Index), so `&mut place` is sound; `byref_borrow`
                // emits `&mut *x` for a forwarded `&mut T` param (E0596), and
                // `emit_place_hoisted` lifts any subscript index into a `{ let
                // __idxN = ..; &mut .. }` block so the index temp evaluates before
                // the borrow (E0502). Every other arg routes through
                // `emit_call_arg_value` (Func / Optional[Callable] cast, poly-base
                // wrap, Optional[T] Some-wrap, else clone-on-use).
                let mut parts = Vec::with_capacity(args.len());
                for (i, a) in args.iter().enumerate() {
                    if param_by_ref.get(i).copied().unwrap_or(false) {
                        let mut aprelude = Vec::new();
                        let place = self.emit_place_hoisted(a, &mut aprelude)?;
                        let borrow = self.byref_borrow(a, &place);
                        parts.push(Self::hoist_wrap(&aprelude, borrow));
                        continue;
                    }
                    parts.push(self.emit_call_arg_value(a, &param_tys, i, /*coerced=*/ true)?);
                }

                // Inject default arguments for named functions (positional-only
                // under-application).
                if let Expr::Ident(n, _) = callee.as_ref() {
                    if let Some(sig) = self.ctx.funcs.get(n.as_str()).cloned() {
                        let expected = sig.params.len();
                        if parts.len() < expected && !sig.param_defaults.is_empty() {
                            let defaults_needed = expected - parts.len();
                            let defaults_start = sig.param_defaults.len().saturating_sub(defaults_needed);
                            for (off, def_expr) in sig.param_defaults[defaults_start..].iter().enumerate() {
                                let pidx = defaults_start + off;
                                match def_expr {
                                    // (enabler-fix-1 #6) Coerce a filled default into
                                    // its slot (Some-wrap an `Optional[T]=<lit>`
                                    // default, Callable / poly-base casts) instead of
                                    // a bare emit_expr, which emitted `describe(5i64)`
                                    // into an `Option<i64>` slot -> rustc E0308.
                                    Some(e) => parts.push(self.emit_call_arg_value(e, &param_tys, pidx, /*coerced=*/ true)?),
                                    None => return Err(crate::diag::Error::Codegen("missing required argument".into())),
                                }
                            }
                        }
                    }
                }

                let callee_s = self.emit_expr(callee)?;
                // Parenthesize lambda expressions when used as callees
                let callee_s = if matches!(callee.as_ref(), Expr::Lambda { .. }) {
                    format!("({})", callee_s)
                } else {
                    callee_s
                };
                Ok(format!("{}({})", callee_s, parts.join(", ")))
    }

}
