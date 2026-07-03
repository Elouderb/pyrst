use super::*;
use std::collections::HashSet;

/// The LOUD codegen error for a residual read-after-conflicting-reassign (it
/// should have been rejected at `check`; see
/// [`Codegen::assert_no_read_after_divergent_reassign`]).
fn read_after_reassign_codegen_error(name: &str, outer: &Ty, inner: &Ty) -> crate::diag::Error {
    crate::diag::Error::Codegen(format!(
        "internal invariant violated: local `{}` is reassigned to an incompatible \
         type inside a block ({} before the block, {} inside) and is read after the \
         block; the block-scoped shadow codegen emits for the reassignment is \
         discarded at the block's end, so the read would silently see the stale \
         outer value. This should have been rejected at `check`. Use a distinct name \
         for the block-local value, annotate both with the same type, or — for a \
         generator — materialize with `list(...)`.",
        name, outer, inner
    ))
}

impl<'a> Codegen<'a> {
    pub fn new(ctx: &'a TyCtx) -> Self {
        Self { ctx, out: String::new(), indent: 0, locals: HashMap::new(), declared: Default::default(), current_class: None, current_ret_ty: Ty::Unit, dead_funcs: Default::default(), mut_self: HashMap::new(), by_ref_locals: Default::default(), poly_map: HashMap::new(), concrete_struct_params: Default::default(), const_names: Default::default(), const_strs: Default::default(), in_generator: false, try_return_escape: false, try_loopctl_escape: false, current_class_type_params: Vec::new(), current_fn_type_params: Vec::new(), hoisted: Default::default(), shadow_map: Default::default(), shadow_counter: 0 }
    }

    pub fn with_dead_funcs(mut self, dead: std::collections::HashSet<String>) -> Self {
        self.dead_funcs = dead;
        self
    }

    /// Thin wrapper over the single shared copy-ness predicate
    /// (`crate::typeck::is_copy`) so the derive/Default decisions read cleanly.
    /// The LOGIC lives in one place; this is only sugar for the `self.` call sites.
    pub(crate) fn is_copy_type(&self, ty: &Ty) -> bool {
        crate::typeck::is_copy(ty)
    }

    /// Returns true when `ty` implements the `Default` trait in the emitted Rust.
    /// Copy classes (all-primitive fields) don't derive Default, so they return false.
    pub(crate) fn type_has_default(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Unit => true,
            // LAZY-GEN V1-a: a generator local is a `Vec<T>` (eager) — has `Default`
            // exactly like a list, so hoisting stays byte-identical.
            Ty::List(_) | Ty::Iterator(_) | Ty::Set(_) | Ty::Dict(_, _) | Ty::Option(_) => true,
            Ty::Class(n, _) => {
                // (EPIC-5 C2-3) A polymorphic base lowers (via `rust_ty`) to its
                // companion enum `n__`, a data-variant enum that CANNOT derive
                // `Default` (emit_companion_enum is `#[derive(Clone, Debug)]`
                // only). So an outer struct holding such a field must NOT include
                // `Default` in its own derive list, and such a local is not
                // hoistable with `Default::default()`.
                if self.is_polymorphic_base(n) {
                    return false;
                }
                // Copy classes don't get #[derive(Default)] (see emit_class).
                let all_copy = self.ctx.get_all_fields(n).iter().all(|f| {
                    Ty::from_type_expr(&f.ty, f.span).map(|t| self.is_copy_type(&t)).unwrap_or(false)
                });
                !all_copy
            }
            _ => false,
        }
    }

    /// (EPIC-5 C2-3) True when the companion enum `base__` carries `impl PartialEq`.
    /// `emit_companion_enum` forwards `PartialEq` to the variant structs ONLY when
    /// EVERY variant defines `__eq__` (its `all_have_eq` predicate); otherwise the
    /// enum has no `PartialEq` at all (cross-variant equality is honestly absent).
    /// A struct holding a polymorphic-base field can therefore derive `PartialEq`
    /// only when this returns true — mirrors `emit_companion_enum`'s `all_have_eq`.
    pub(crate) fn companion_enum_has_partial_eq(&self, base: &str) -> bool {
        if !self.is_polymorphic_base(base) {
            return false;
        }
        let mut variants: Vec<String> = vec![base.to_string()];
        if let Some(subs) = self.poly_map.get(base) {
            variants.extend(subs.iter().cloned());
        }
        variants
            .iter()
            .all(|v| self.resolved_methods(v).iter().any(|m| m.name == "__eq__"))
    }

    /// Returns a zero-value Rust initializer for any type, including Copy classes
    /// whose primitive fields are zeroed recursively.  Used in `new()` bodies
    /// where `Default::default()` is unavailable for Copy-only structs.
    pub(crate) fn zeroed_default(&self, ty: &Ty) -> String {
        match ty {
            Ty::Int => "0i64".to_string(),
            Ty::Float => "0.0f64".to_string(),
            Ty::Bool => "false".to_string(),
            Ty::Str => "String::new()".to_string(),
            Ty::Class(n, _) => {
                let all_copy = self.ctx.get_all_fields(n).iter().all(|f| {
                    Ty::from_type_expr(&f.ty, f.span).map(|t| self.is_copy_type(&t)).unwrap_or(false)
                });
                let struct_init = if all_copy {
                    // Build a struct literal with zeroed primitive fields.
                    let fields: Vec<String> = self.ctx.get_all_fields(n).iter().map(|f| {
                        let inner_ty = Ty::from_type_expr(&f.ty, f.span).unwrap_or(Ty::Int);
                        // (EPIC-6) Escape a keyword field name in the zeroed
                        // struct-literal default (matches the struct field def).
                        format!("{}: {}", escape_ident(&f.name), self.zeroed_default(&inner_ty))
                    }).collect();
                    format!("{} {{ {} }}", n, fields.join(", "))
                } else {
                    "Default::default()".to_string()
                };
                // (EPIC-5 C2-2b-i) A polymorphic-base local is Rust `B__`, so the
                // zeroed initializer must be the base variant carrying the zeroed
                // value struct (`B__::B(B{..})`), not a bare struct literal (the
                // wrong type for the enum slot). Leaf/non-polymorphic classes keep
                // the plain struct init.
                if self.is_polymorphic_base(n) {
                    format!("{}__::{}({})", n, n, struct_init)
                } else {
                    struct_init
                }
            }
            _ => "Default::default()".to_string(),
        }
    }

    /// True if a type has no `Unknown` anywhere — only then is it safe to hoist
    /// (an `Unknown` element would render as `()` and mismatch a real value).
    pub(crate) fn fully_concrete(ty: &Ty) -> bool {
        match ty {
            Ty::Unknown => false,
            // LAZY-GEN V1-a: a generator is concrete iff its element type is (== list).
            Ty::List(e) | Ty::Iterator(e) | Ty::Set(e) | Ty::Option(e) => Self::fully_concrete(e),
            Ty::Dict(k, v) => Self::fully_concrete(k) && Self::fully_concrete(v),
            Ty::Tuple(ts) => ts.iter().all(Self::fully_concrete),
            _ => true,
        }
    }

    /// A safe Rust default initializer for hoisting a local, or None for types
    /// with no usable default (Copy class without `Default`, Tuple, Unit,
    /// Unknown, File) — those names are not hoisted and keep their in-place let.
    pub(crate) fn default_val(&self, ty: &Ty) -> Option<String> {
        if !Self::fully_concrete(ty) { return None; }
        Some(match ty {
            Ty::Int => "0i64".to_string(),
            Ty::Float => "0.0f64".to_string(),
            Ty::Bool => "false".to_string(),
            Ty::Str => "String::new()".to_string(),
            Ty::List(_) => "Vec::new()".to_string(),
            // LAZY-GEN V1-b (review fix): a hoisted generator local is a
            // `__PyrstGen<T>` since rust_ty flipped — `Vec::new()` was the
            // eager-era default and is E0308 now. `__PyrstGen::empty()` (prelude)
            // yields nothing, matching the documented hoisting semantics
            // (read-before-assign gives a default, not UnboundLocalError). V1-d
            // renamed the prelude type under the reserved `__Pyrst` prefix.
            Ty::Iterator(_) => "__PyrstGen::empty()".to_string(),
            Ty::Set(_) => "::std::collections::HashSet::new()".to_string(),
            Ty::Dict(_, _) => "::std::collections::HashMap::new()".to_string(),
            Ty::Option(_) => "None".to_string(),
            Ty::Class(n, _) => {
                // Only derive Default when all fields support it (mirrors emit_class).
                if self.type_has_default(&Ty::Class(n.clone(), vec![])) {
                    "Default::default()".to_string()
                } else {
                    return None;  // Not hoistable — no Default impl available.
                }
            }
            _ => return None,
        })
    }

    /// Collect names first-assigned inside a nested block (depth > 0), so they can
    /// be hoisted to a function-scope slot. Recurses through every block but not
    /// into nested function/class definitions (those have their own scope).
    ///
    /// (card 602b1675) An unpack target is now treated EXACTLY like an assign target
    /// (a depth>0 unpack REASSIGNS the hoisted slot rather than declaring a
    /// block-scoped `let (..)` shadow that gets discarded). The former blanket
    /// unpack-EXCLUSION was there because the old codegen always emitted `let (..)`
    /// for an unpack — that no longer holds (`Stmt::Unpack` distinguishes declare vs
    /// reassign via `self.declared`, like `Stmt::Assign`), and the exclusion also
    /// wrongly blocked hoisting a name that is BOTH a nested assign target AND an
    /// unpack target somewhere.
    pub(crate) fn collect_hoistable(
        stmts: &[Stmt],
        depth: usize,
        block_assigned: &mut std::collections::HashSet<String>,
    ) {
        for s in stmts {
            match s {
                Stmt::Assign { target, .. } => { if depth > 0 { block_assigned.insert(target.clone()); } }
                Stmt::Unpack { targets, .. } => { if depth > 0 { for t in targets { block_assigned.insert(t.clone()); } } }
                Stmt::If { then, elifs, else_, .. } => {
                    Self::collect_hoistable(then, depth + 1, block_assigned);
                    for (_, b) in elifs { Self::collect_hoistable(b, depth + 1, block_assigned); }
                    if let Some(b) = else_ { Self::collect_hoistable(b, depth + 1, block_assigned); }
                }
                Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::With { body, .. } => {
                    Self::collect_hoistable(body, depth + 1, block_assigned);
                }
                Stmt::Try { body, handlers, else_, finally_, .. } => {
                    Self::collect_hoistable(body, depth + 1, block_assigned);
                    for h in handlers { Self::collect_hoistable(&h.body, depth + 1, block_assigned); }
                    if let Some(b) = else_ { Self::collect_hoistable(b, depth + 1, block_assigned); }
                    if let Some(b) = finally_ { Self::collect_hoistable(b, depth + 1, block_assigned); }
                }
                Stmt::Match { arms, .. } => {
                    for a in arms { Self::collect_hoistable(&a.body, depth + 1, block_assigned); }
                }
                _ => {}
            }
        }
    }

    /// (LAZY-GEN V1-d BLOCKER — codegen insurance) Assert that no BARE (un-annotated)
    /// local is assigned DIVERGENT types across the sibling value-paths of any join
    /// in `stmts`. Mirrors typeck's `detect_sibling_divergence` — the SAME
    /// `branch_divergent` predicate and the same "direct bare binding per branch,
    /// first-seen vs later" comparison, over the SAME three join shapes: `if`
    /// (then+elifs+else), `try`/`except` (body + each handler), and `match` (each
    /// arm). So it can never fire for a program that passed `check` (a `build` runs
    /// `check` first; see driver.rs). It is defence-in-depth for the reviewer-traced
    /// silent miscompile: the hoisting machinery (`prescan_types`/`unify_ty` pick
    /// ONE Rust slot type, and the shadow-on-conflict path block-scopes the
    /// divergent branch's value) must NEVER silently drop a value at a join, so a
    /// divergent join that somehow reaches codegen becomes a LOUD build error
    /// instead of a wrong-output binary.
    ///
    /// Deliberately narrow — exactly the sibling-branch shape, NOT the legal Python
    /// idiom of reusing a name for a different type inside a single block read only
    /// within it (e.g. `passing = [..]` then `passing = pred()` inside a `for`),
    /// which never crosses a join. Runs AFTER `prescan_types` so `type_of_expr`
    /// resolves un-annotated locals; does NOT descend into nested `def`/`class`
    /// bodies (each runs this pre-pass on its own body).
    pub(crate) fn assert_no_branch_divergence(&self, stmts: &[Stmt]) -> Result<()> {
        for s in stmts {
            match s {
                Stmt::If { then, elifs, else_, .. } => {
                    let mut branches: Vec<&[Stmt]> = vec![then.as_slice()];
                    for (_, b) in elifs { branches.push(b); }
                    if let Some(b) = else_ { branches.push(b); }
                    self.assert_siblings_no_divergence(&branches, "the branches of an `if`")?;
                    self.assert_no_branch_divergence(then)?;
                    for (_, b) in elifs { self.assert_no_branch_divergence(b)?; }
                    if let Some(b) = else_ { self.assert_no_branch_divergence(b)?; }
                }
                Stmt::Try { body, handlers, else_, finally_, .. } => {
                    // body + each handler are the sibling value-paths (else/finally
                    // run sequentially after the body, not as alternatives).
                    let mut branches: Vec<&[Stmt]> = vec![body.as_slice()];
                    for h in handlers { branches.push(&h.body); }
                    self.assert_siblings_no_divergence(&branches, "the branches of a `try`/`except`")?;
                    self.assert_no_branch_divergence(body)?;
                    for h in handlers { self.assert_no_branch_divergence(&h.body)?; }
                    if let Some(b) = else_ { self.assert_no_branch_divergence(b)?; }
                    if let Some(b) = finally_ { self.assert_no_branch_divergence(b)?; }
                }
                Stmt::Match { arms, .. } => {
                    let branches: Vec<&[Stmt]> = arms.iter().map(|a| a.body.as_slice()).collect();
                    self.assert_siblings_no_divergence(&branches, "the arms of a `match`")?;
                    for a in arms { self.assert_no_branch_divergence(&a.body)?; }
                }
                Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::With { body, .. } => {
                    self.assert_no_branch_divergence(body)?;
                }
                // Nested defs/classes run this pre-pass on their own body.
                Stmt::Func(_) | Stmt::Class(_) => {}
                _ => {}
            }
        }
        Ok(())
    }

    /// Compare the DIRECT bare bindings of a set of sibling branches; error if any
    /// name diverges (see [`Self::assert_no_branch_divergence`]). `join_desc` names
    /// the join for the message.
    fn assert_siblings_no_divergence(&self, branches: &[&[Stmt]], join_desc: &str) -> Result<()> {
        // name -> candidate types from PRIOR branches; compare every CROSS-branch
        // pair (a branch may contribute multiple candidates via nested descent).
        let mut seen: HashMap<String, Vec<Ty>> = HashMap::new();
        for branch in branches {
            let bm = self.branch_direct_types(branch);
            for (name, cands) in &bm {
                if let Some(prev) = seen.get(name) {
                    for pty in prev {
                        for vt in cands {
                            if crate::typeck::branch_divergent(pty, vt) {
                                return Err(crate::diag::Error::Codegen(format!(
                                    "internal invariant violated: local `{}` is assigned incompatible \
                                     types across {} (`{}` vs `{}`); the codegen hoist would silently \
                                     drop one branch's value at the join. This should have been rejected \
                                     at `check`. Use a distinct name per branch, annotate both branches \
                                     with the same type, or — for a generator — materialize with `list(...)`.",
                                    name, join_desc, pty, vt
                                )));
                            }
                        }
                    }
                }
            }
            for (name, cands) in bm {
                seen.entry(name).or_default().extend(cands);
            }
        }
        Ok(())
    }

    /// The BARE bindings a branch MAY exit with — a bare `Stmt::Assign` or a
    /// `Stmt::Unpack` (each name → its tuple-component type) via `type_of_expr`,
    /// descending into SINGLE-ALTERNATIVE nested blocks (an `if` with NO `else`,
    /// plus `while`/`for` bodies; a `with` body is inlined as it always runs) so a
    /// divergent reassign nested one level deep participates in the comparison
    /// (card eca0532e). Mirrors typeck's `branch_direct_bare_assign_types`: a DIRECT
    /// (unconditional) assign REPLACES the name's candidates; a CONDITIONAL nested
    /// assign UNIONS its candidates in.
    fn branch_direct_types(&self, branch: &[Stmt]) -> HashMap<String, Vec<Ty>> {
        let mut out: HashMap<String, Vec<Ty>> = HashMap::new();
        self.collect_branch_exit_types(branch, &mut out);
        out
    }

    /// Walk one statement list accumulating, per name, the candidate exit types (see
    /// [`Self::branch_direct_types`]).
    fn collect_branch_exit_types(&self, branch: &[Stmt], out: &mut HashMap<String, Vec<Ty>>) {
        for st in branch {
            match st {
                Stmt::Assign { target, ty: None, value, .. } => {
                    out.insert(target.clone(), vec![self.type_of_expr(value)]); // REPLACE
                }
                Stmt::Unpack { targets, value, .. } => {
                    let vt = self.type_of_expr(value);
                    let elem_tys = match &vt {
                        Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
                        _ => vec![Ty::Unknown; targets.len()],
                    };
                    for (t, ety) in targets.iter().zip(elem_tys.iter()) {
                        out.insert(t.clone(), vec![ety.clone()]); // REPLACE
                    }
                }
                // `with` body always runs — inline (unconditional).
                Stmt::With { body, .. } => {
                    self.collect_branch_exit_types(body, out);
                }
                // Single-alternative `if` (no else) — conditional candidates.
                Stmt::If { then, elifs, else_: None, .. } => {
                    self.merge_conditional_exit_types(then, out);
                    for (_, b) in elifs {
                        self.merge_conditional_exit_types(b, out);
                    }
                }
                // Loop bodies may run zero or more times — conditional candidates.
                Stmt::While { body, .. } | Stmt::For { body, .. } => {
                    self.merge_conditional_exit_types(body, out);
                }
                _ => {}
            }
        }
    }

    /// Collect a CONDITIONAL nested body's exit types and UNION them into `out`.
    fn merge_conditional_exit_types(&self, body: &[Stmt], out: &mut HashMap<String, Vec<Ty>>) {
        let mut sub: HashMap<String, Vec<Ty>> = HashMap::new();
        self.collect_branch_exit_types(body, &mut sub);
        for (name, cands) in sub {
            out.entry(name).or_default().extend(cands);
        }
    }

    /// (fix-b defence-in-depth) The codegen MIRROR of typeck's
    /// `detect_read_after_conflicting_reassign`. typeck rejects this shape at
    /// `check` (which `build` runs first), so this is pure insurance: an outer-scope
    /// bare local reassigned to a divergent type inside a single nested block and
    /// READ after that block would let codegen's block-scoped shadow (stmts.rs,
    /// `types_conflict` → shadow) silently drop the value at the read. If any such
    /// residual reaches codegen it becomes a LOUD build error, never a wrong-output
    /// binary. Shares the type-free liveness engine (`live_in_stmt`,
    /// `loop_body_live_out`) and the `branch_divergent` predicate with typeck, so it
    /// is corpus-safe by construction (a name read only WITHIN its block is not in
    /// `body_live_out`, exactly as at `check`). Runs after `prescan_types` so
    /// `type_of_expr` resolves un-annotated locals; does NOT descend into nested
    /// `def`/`class` bodies (each runs this pre-pass on its own body).
    pub(crate) fn assert_no_read_after_divergent_reassign(&self, body: &[Stmt], params: &HashSet<String>) -> Result<()> {
        // Params are function-scoped: reassigning one inside a block shadows, so
        // they must seed the top-level bound set (they become `outer` on descent).
        self.walk_read_after_cg(body, &HashSet::new(), &HashMap::new(), &HashSet::new(), params)
    }

    fn walk_read_after_cg(
        &self,
        body: &[Stmt],
        outer: &HashSet<String>,
        types_in: &HashMap<String, Ty>,
        body_live_out: &HashSet<String>,
        seed_bound: &HashSet<String>,
    ) -> Result<()> {
        use crate::typeck::{branch_divergent, collect_bound_names_stmt, live_in_stmt, loop_body_live_out};
        let n = body.len();
        let mut live_at: Vec<HashSet<String>> = vec![HashSet::new(); n + 1];
        live_at[n] = body_live_out.clone();
        for i in (0..n).rev() {
            live_at[i] = live_in_stmt(&body[i], &live_at[i + 1]);
        }
        let mut types = types_in.clone();
        let mut bound_here = seed_bound.clone();

        for i in 0..n {
            let after = &live_at[i + 1];
            let s = &body[i];

            // 1) FLAG a direct bare reassign of an OUTER name to a divergent type
            //    read AFTER this block. Outer type = the forward-threaded slot type
            //    (falling back to the prescan type); reassign type via type_of_expr.
            match s {
                Stmt::Assign { target, ty: None, value, .. } => {
                    if outer.contains(target) && body_live_out.contains(target) {
                        if let Some(ot) = types.get(target).cloned().or_else(|| self.locals.get(target).cloned()) {
                            let vt = self.type_of_expr(value);
                            if branch_divergent(&ot, &vt) {
                                return Err(read_after_reassign_codegen_error(target, &ot, &vt));
                            }
                        }
                    }
                }
                Stmt::Unpack { targets, value, .. } => {
                    let vt = self.type_of_expr(value);
                    let elem_tys = match &vt {
                        Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
                        _ => vec![Ty::Unknown; targets.len()],
                    };
                    for (t, ety) in targets.iter().zip(elem_tys.iter()) {
                        if outer.contains(t) && body_live_out.contains(t) {
                            if let Some(ot) = types.get(t).cloned().or_else(|| self.locals.get(t).cloned()) {
                                if branch_divergent(&ot, ety) {
                                    return Err(read_after_reassign_codegen_error(t, &ot, ety));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }

            // 2) RECURSE into nested blocks (their direct reassigns flagged at their level).
            let new_outer: HashSet<String> = outer.union(&bound_here).cloned().collect();
            let empty = HashSet::new();
            match s {
                Stmt::If { then, elifs, else_, .. } => {
                    self.walk_read_after_cg(then, &new_outer, &types, after, &empty)?;
                    for (_, b) in elifs { self.walk_read_after_cg(b, &new_outer, &types, after, &empty)?; }
                    if let Some(b) = else_ { self.walk_read_after_cg(b, &new_outer, &types, after, &empty)?; }
                }
                Stmt::While { cond, body: wb, .. } => {
                    let loop_out = loop_body_live_out(wb, after, None, Some(cond));
                    self.walk_read_after_cg(wb, &new_outer, &types, &loop_out, &empty)?;
                }
                Stmt::For { targets, body: fb, .. } => {
                    let loop_out = loop_body_live_out(fb, after, Some(targets), None);
                    let tset: HashSet<String> = targets.iter().cloned().collect();
                    let mut btypes = types.clone();
                    for t in targets { btypes.entry(t.clone()).or_insert(Ty::Unknown); }
                    self.walk_read_after_cg(fb, &new_outer, &btypes, &loop_out, &tset)?;
                }
                Stmt::Try { body: tb, handlers, else_, finally_, .. } => {
                    self.walk_read_after_cg(tb, &new_outer, &types, after, &empty)?;
                    for h in handlers {
                        let mut htypes = types.clone();
                        let mut hseed = HashSet::new();
                        if let Some(nm) = &h.exc_name {
                            htypes.insert(nm.clone(), Ty::Str);
                            hseed.insert(nm.clone());
                        }
                        self.walk_read_after_cg(&h.body, &new_outer, &htypes, after, &hseed)?;
                    }
                    if let Some(b) = else_ { self.walk_read_after_cg(b, &new_outer, &types, after, &empty)?; }
                    if let Some(b) = finally_ { self.walk_read_after_cg(b, &new_outer, &types, after, &empty)?; }
                }
                Stmt::With { as_name, body: wb, .. } => {
                    let mut wtypes = types.clone();
                    let mut wseed = HashSet::new();
                    if let Some(nm) = as_name {
                        wtypes.entry(nm.clone()).or_insert(Ty::Unknown);
                        wseed.insert(nm.clone());
                    }
                    self.walk_read_after_cg(wb, &new_outer, &wtypes, after, &wseed)?;
                }
                Stmt::Match { subject, arms, .. } => {
                    let subj_ty = self.type_of_expr(subject);
                    for arm in arms {
                        let mut atypes = types.clone();
                        let mut aseed = HashSet::new();
                        if let MatchPattern::Capture(nm) = &arm.pattern {
                            atypes.insert(nm.clone(), subj_ty.clone());
                            aseed.insert(nm.clone());
                        }
                        self.walk_read_after_cg(&arm.body, &new_outer, &atypes, after, &aseed)?;
                    }
                }
                _ => {}
            }

            // 3) Advance the forward slot types + the set of names bound at this level.
            match s {
                Stmt::Assign { target, value, .. } => { types.insert(target.clone(), self.type_of_expr(value)); }
                Stmt::Unpack { targets, value, .. } => {
                    let vt = self.type_of_expr(value);
                    let elem_tys = match &vt {
                        Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
                        _ => vec![Ty::Unknown; targets.len()],
                    };
                    for (t, ety) in targets.iter().zip(elem_tys.iter()) { types.insert(t.clone(), ety.clone()); }
                }
                Stmt::For { targets, .. } => { for t in targets { types.entry(t.clone()).or_insert(Ty::Unknown); } }
                _ => {}
            }
            collect_bound_names_stmt(s, &mut bound_here);
        }
        Ok(())
    }

    /// Replace identifier `old_name` with `new_name` in code, respecting word boundaries
    /// to avoid corrupting field names like "price" when replacing "i"
    pub(crate) fn replace_identifier(code: &str, old_name: &str, new_name: &str) -> String {
        // Build regex pattern: \b (word boundary) + old_name + \b (word boundary)
        if old_name.is_empty() {
            return code.to_string();
        }

        // Simple approach: split on word boundaries and reconstruct
        let mut result = String::new();
        let mut chars = code.chars().peekable();

        while let Some(ch) = chars.next() {
            // Check if we're at the start of an identifier that matches old_name
            if ch.is_alphanumeric() || ch == '_' {
                // Collect the full identifier
                let mut ident = String::from(ch);

                // (EPIC-6) A raw identifier `r#kw` is a single token: if we are at
                // a bare `r` immediately followed by `#` and then an identifier
                // char, absorb the `#` so the whole `r#kw` is collected as one
                // token (and can match a `r#`-escaped `old_name`). Without this,
                // `r#type` would split into `r` / `#` / `type` and a replace of
                // `r#type` would corrupt the raw identifier.
                if ch == 'r' && chars.peek() == Some(&'#') {
                    let mut probe = chars.clone();
                    probe.next(); // consume '#'
                    if matches!(probe.peek(), Some(c) if c.is_alphanumeric() || *c == '_') {
                        chars.next(); // consume '#'
                        ident.push('#');
                    }
                }

                while let Some(&next_ch) = chars.peek() {
                    if next_ch.is_alphanumeric() || next_ch == '_' {
                        ident.push(next_ch);
                        chars.next();
                    } else {
                        break;
                    }
                }

                // Check if this identifier matches old_name
                if ident == old_name {
                    result.push_str(new_name);
                } else {
                    result.push_str(&ident);
                }
            } else {
                result.push(ch);
            }
        }

        result
    }

    /// (LAZY-GEN V1-c) The source expression for a builtin (`sum`/`min`/`max`/
    /// `any`/`all`/`enumerate`/`zip`/`sorted`/`list`) argument that types as a
    /// `Ty::Iterator` (a generator). Mirrors the for-loop/comprehension
    /// convention established in V1-b (review fix): a generator VARIABLE
    /// (`Expr::Ident`) is consumed by `&mut` — std's blanket `impl<I: Iterator>
    /// Iterator for &mut I` — so the binding stays live but advances in place
    /// (`total = sum(g)` leaves `g` bound-but-exhausted, exactly like Python's
    /// generator object, instead of moving it and making a later use an E0382).
    /// A fresh call (`sum(gen(3))`) is a temporary with no caller-visible
    /// binding to preserve, so it is consumed by value unchanged.
    pub(crate) fn iter_arg_source(expr: &Expr, emitted: &str) -> String {
        if matches!(expr, Expr::Ident(..)) {
            format!("(&mut {})", emitted)
        } else {
            emitted.to_string()
        }
    }

    pub(crate) fn type_of_expr(&self, e: &Expr) -> Ty {
        crate::typeck::infer_expr_ty(e, &self.locals, self.ctx)
    }

    /// (card 575bcf3a) Snapshot the per-scope emission state that a Rust `{}` block
    /// must NOT leak past its end: the forward type view (`locals`), the set of
    /// in-scope `let` bindings (`declared`), and the active hoisted-local shadows
    /// (`shadow_map`). Paired with [`Self::scope_exit`] around EACH child-block body
    /// (per-branch, so a `then`-branch shadow never poisons an `elif`/sibling).
    /// Models Rust block scoping exactly: a `let`/shadow inside the block, and any
    /// type-view change it caused, reverts at the block's close — so a divergent
    /// reassign nested one level deep can no longer make the enclosing block's next
    /// statement wrongly shadow (the reviewer-traced miscompile). `hoisted` and
    /// `shadow_counter` are FUNCTION-scoped (not restored here).
    pub(crate) fn scope_enter(
        &self,
    ) -> (
        HashMap<String, Ty>,
        HashSet<String>,
        HashMap<String, (String, Ty)>,
    ) {
        (
            self.locals.clone(),
            self.declared.clone(),
            self.shadow_map.clone(),
        )
    }

    /// Restore the state captured by [`Self::scope_enter`] at a child block's end.
    pub(crate) fn scope_exit(
        &mut self,
        snap: (
            HashMap<String, Ty>,
            HashSet<String>,
            HashMap<String, (String, Ty)>,
        ),
    ) {
        self.locals = snap.0;
        self.declared = snap.1;
        self.shadow_map = snap.2;
    }

    /// (card 575bcf3a, poison2) A fresh, deterministic mangled Rust binding name for
    /// a divergent shadow of hoisted local `name`. The `__pyrst_shadow_` prefix is
    /// reserved (typeck's `reject_if_reserved` blocks user identifiers under
    /// `__pyrst`), so it can never collide with a user binding; the counter makes
    /// repeated shadows of the same name distinct and keeps emission byte-stable.
    pub(crate) fn fresh_shadow_name(&mut self, name: &str) -> String {
        let n = self.shadow_counter;
        self.shadow_counter += 1;
        format!("__pyrst_shadow_{}_{}", name, n)
    }

    /// The Rust identifier a READ of local `name` must resolve to: the mangled
    /// shadow binding when an active divergent shadow hides the function-scope slot,
    /// otherwise `None` (the caller uses the ordinary `escape_ident` name). Consulted
    /// by `emit_expr`'s Ident arm. Empty `shadow_map` (the common case) → always
    /// `None`, so shadow-free code is byte-for-byte unchanged.
    pub(crate) fn shadow_read_name(&self, name: &str) -> Option<String> {
        self.shadow_map.get(name).map(|(m, _)| m.clone())
    }

    /// Bind a comprehension's loop target(s) into `self.locals` with the iterable's
    /// ELEMENT type, so a method call on the loop variable inside the comprehension
    /// body (`[it.get() for it in items]`) resolves its receiver type to the
    /// element class and dispatches to the CLASS method — not a same-named dict/list
    /// builtin (which would mis-emit, or `panic!` for a no-arg `.get()`). Mirrors the
    /// `Stmt::For` element-type derivation (and typeck's `bind_comp_targets`): a
    /// list/set yields its element, a dict yields its KEY (Python iterates keys), a
    /// str yields a 1-char str, and a tuple element distributes across multiple
    /// targets. Returns the prior bindings so the caller can `restore_comp_targets`
    /// after emitting the body — the loop variable must NOT leak past the
    /// comprehension or shadow an outer local of the same name.
    pub(crate) fn bind_comp_targets(
        &mut self,
        targets: &[String],
        iter: &Expr,
    ) -> Vec<(String, Option<Ty>)> {
        let elem = match self.type_of_expr(iter) {
            // LAZY-GEN V1-a: a comprehension over a generator binds its target to
            // the generator's element type, exactly like over a list.
            Ty::List(inner) | Ty::Iterator(inner) | Ty::Set(inner) => *inner,
            Ty::Dict(key, _) => *key,
            Ty::Str => Ty::Str,
            _ => Ty::Unknown,
        };
        let mut saved = Vec::with_capacity(targets.len());
        if targets.len() == 1 {
            saved.push((targets[0].clone(), self.locals.get(&targets[0]).cloned()));
            self.locals.insert(targets[0].clone(), elem);
        } else {
            let elem_tys = match &elem {
                Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
                _ => vec![Ty::Unknown; targets.len()],
            };
            for (i, t) in targets.iter().enumerate() {
                saved.push((t.clone(), self.locals.get(t).cloned()));
                self.locals.insert(t.clone(), elem_tys.get(i).cloned().unwrap_or(Ty::Unknown));
            }
        }
        saved
    }

    /// Undo `bind_comp_targets`: restore each loop-target name to its prior binding
    /// (or remove it when it was unbound before the comprehension).
    pub(crate) fn restore_comp_targets(&mut self, saved: Vec<(String, Option<Ty>)>) {
        for (name, prev) in saved {
            match prev {
                Some(ty) => { self.locals.insert(name, ty); }
                None => { self.locals.remove(name.as_str()); }
            }
        }
    }

    /// (EPIC-6) Emit a user-defined method call `obj_s.method_name(args)` on a
    /// known class receiver `cls`, threading per-param by-reference (`Mut[T]`)
    /// arguments exactly like the long-standing "Regular method call" tail of
    /// the dispatch block. Factored out so the receiver-type-guarded early
    /// return (which routes a user-class receiver PAST the builtin arms, fixing
    /// the silent miscompile where `instance.get(k)` lowered to a dict
    /// `.get(&k).cloned()`) reuses the SAME by-ref/companion-enum emission
    /// rather than duplicating-and-drifting it.
    ///
    /// `method_name` is the user method's RAW name — not the builtin remap (so a
    /// user method legitimately named `append`/`upper`/`pop` calls the real
    /// `obj.append(..)` inherent/dispatch method, not the remapped `.push(..)`).
    /// For a polymorphic-base receiver `cls` is the base name and the per-param
    /// flags come from `get_method(base, name)` (the base's signature), so the
    /// emitted `obj_s.method_name(..)` resolves to the companion enum `cls__`'s
    /// dispatch method — identical to the pre-existing EPIC-5 lowering.
    pub(crate) fn emit_user_method_call(
        &mut self,
        obj_s: &str,
        cls: &str,
        method_name: &str,
        args: &[Expr],
        kwargs: &[(String, Expr)],
        parts: &[String],
        span: crate::diag::Span,
        // (card e10df981) The receiver's full instance type, e.g.
        // `Ty::Class("Box", [Int])`. Threaded so the kwargs / default-fill path can
        // substitute the class's type args into the (type-var-bearing) method
        // signature — a bare `Ty::Class(cls, [])` (non-generic receiver) leaves the
        // substitution empty, so every non-generic call is byte-for-byte unchanged.
        recv_ty: &Ty,
    ) -> Result<String> {
        let sig = self.ctx.get_method(cls, method_name);
        // (kwargs v1, card 8a7b7714) A keyword-bearing OR under-applied
        // (default-taking) method call builds its FULL positional part list from
        // the keyword→slot mapping: positional args left-to-right, each keyword
        // value in its named parameter's slot, declared defaults injected into
        // the unfilled slots. This also closes the long-standing method-default
        // hole (`g.greet()` with a defaulted param previously leaked rustc
        // E0061 — free functions filled trailing defaults, methods never did).
        // typeck already validated the mapping; re-deriving it is deterministic.
        if let Some(sig) = &sig {
            // (card e10df981) A FULLY-APPLIED POSITIONAL call on a GENERIC-CLASS
            // receiver also needs the slotted path: its raw `parts` (the fall-through
            // below) are emitted with NO knowledge of the concrete slot type, so a
            // lambda passed positionally into a `Callable[[T], T]` slot leaks a bare
            // `T` (E0308/E0425 at build) exactly like the kwargs case. Force the
            // substituted-`param_tys` emission when the receiver is generic (its type
            // subst is non-empty) AND some param type still carries a class type var.
            // Guarded on the receiver being generic, so every NON-generic method call
            // keeps its byte-for-byte `parts` path untouched.
            let force_slotted = !crate::typeck::class_type_subst(recv_ty, self.ctx).is_empty()
                && sig.params.iter().any(|(_, t)| crate::typeck::ty_contains_typevar(t));
            if !kwargs.is_empty() || args.len() < sig.params.len() || force_slotted {
                let site = format!("{}.{}", cls, method_name);
                let slots =
                    crate::typeck::map_kwargs_to_slots(&site, sig, args.len(), kwargs, span)?;
                // (item A + C) Lower via the shared slot mapper: it preserves
                // CPython's source-order call-site evaluation (hoisting EVERY
                // out-of-order argument, positionals included) and — with
                // `coerced = true` — applies the SAME Optional-wrap / `Callable`
                // cast the free-call path uses (the method path previously emitted
                // a plain `emit_consuming`, silently dropping both). Per-slot
                // coercion targets come from the method signature's param types.
                // (card e10df981) For a GENERIC-CLASS receiver those types still
                // carry the class's `Ty::TypeVar`s — `get_method` lowers the sig with
                // the class type params in scope but does NOT substitute the
                // instance's args (its doc defers that to the call site). Substitute
                // them with the receiver's concrete args now, exactly as the free
                // path does, so a lambda cast into a `Callable[[T], T]` slot lowers to
                // `Rc<dyn Fn(i64) -> i64>` instead of leaking a bare `T` (E0425 at
                // build). `subst_class_member` is a no-op for a non-generic receiver,
                // so every existing method call is byte-for-byte unchanged.
                let param_tys: Vec<Ty> = sig
                    .params
                    .iter()
                    .map(|(_, t)| crate::typeck::subst_class_member(t, recv_ty, self.ctx))
                    .collect();
                let (prelude, mparts) =
                    self.emit_slotted_args(&slots, args, kwargs, sig, &param_tys, /*coerced=*/ true)?;
                return Ok(Self::hoist_wrap(
                    &prelude,
                    format!("{}.{}({})", obj_s, method_name, mparts.join(", ")),
                ));
            }
        }
        let method_by_ref: Vec<bool> = sig.map(|sig| sig.param_by_ref).unwrap_or_default();
        if method_by_ref.iter().any(|&b| b) {
            let mut mparts = Vec::with_capacity(args.len());
            for (i, a) in args.iter().enumerate() {
                if method_by_ref.get(i).copied().unwrap_or(false) {
                    // (card cc7ae370, item 1) Hoist any subscript index in the arg
                    // place and wrap `&mut place` in a block so the index temp runs
                    // before the borrow (E0502) — see the free-function by-ref path.
                    let mut aprelude = Vec::new();
                    let place = self.emit_place_hoisted(a, &mut aprelude)?;
                    let borrow = self.byref_borrow(a, &place);
                    mparts.push(Self::hoist_wrap(&aprelude, borrow));
                } else {
                    mparts.push(self.emit_consuming(a)?);
                }
            }
            return Ok(format!("{}.{}({})", obj_s, method_name, mparts.join(", ")));
        }
        Ok(format!("{}.{}({})", obj_s, method_name, parts.join(", ")))
    }

    /// (EPIC-5 C1-C) Honest codegen gate for class subtyping.
    ///
    /// Part B made typeck ACCEPT a `Derived` value flowing into a `Base` slot
    /// (`is_subclass(derived, base)`), but codegen cannot yet EMIT it: each pyrst
    /// class is a standalone Rust struct, so a `Dog` value does not fit a slot
    /// typed `Animal` and rustc would reject it with an opaque E0308. Until the
    /// EPIC-5 C2 companion-enum codegen lands, refuse such a flow here with a
    /// clear pyrst error instead of leaking a raw rustc failure.
    ///
    /// Fires ONLY for a strictly-derived class pair (`got != expected` and
    /// `is_subclass(got, expected)` holds). Exact-type flows (`got == expected`),
    /// non-class types, and unrelated classes (which typeck already rejected)
    /// pass through untouched, so no existing exact-typed example is affected.
    /// (EPIC-5 C2-2b-i) True iff `ty` mentions a polymorphic base anywhere — i.e.
    /// a slot of this type lowers (via `rust_ty`) to a companion enum `B__` at
    /// some position, so a raw-struct value flowing in needs WRAPPING. When this
    /// is false the slot is exact-typed and the legacy `emit_consuming` path is
    /// used unchanged (keeps every non-polymorphic example byte-for-byte stable).
    pub(crate) fn ty_has_poly_base(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Class(n, _) => self.is_polymorphic_base(n),
            Ty::List(e) | Ty::Iterator(e) | Ty::Set(e) | Ty::Option(e) => self.ty_has_poly_base(e),
            Ty::Dict(k, v) => self.ty_has_poly_base(k) || self.ty_has_poly_base(v),
            Ty::Tuple(ts) => ts.iter().any(|t| self.ty_has_poly_base(t)),
            _ => false,
        }
    }

    /// (first-class functions) True iff `ty` is a function type or a collection
    /// whose element / value type is one — i.e. a slot of this type contains an
    /// `Rc<dyn Fn>` position into which a bare function NAME or lambda must be
    /// wrapped (`emit_into_func_slot`). When false the slot has no function
    /// position and the legacy clone-on-use path is used unchanged.
    pub(crate) fn ty_has_func(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Func(..) => true,
            Ty::List(e) | Ty::Iterator(e) | Ty::Set(e) | Ty::Option(e) => self.ty_has_func(e),
            Ty::Dict(k, v) => self.ty_has_func(k) || self.ty_has_func(v),
            Ty::Tuple(ts) => ts.iter().any(|t| self.ty_has_func(t)),
            _ => false,
        }
    }

    /// Whether `ty` mentions any `Ty::TypeVar` (a generic-call param-type slot
    /// that needs monomorphization before emission). A cheap guard so the
    /// call-site substitution only runs for an actually-generic callee.
    pub(crate) fn ty_mentions_typevar(ty: &Ty) -> bool {
        match ty {
            Ty::TypeVar(_) => true,
            Ty::List(e) | Ty::Iterator(e) | Ty::Set(e) | Ty::Option(e) => Self::ty_mentions_typevar(e),
            Ty::Dict(k, v) => Self::ty_mentions_typevar(k) || Self::ty_mentions_typevar(v),
            Ty::Tuple(ts) => ts.iter().any(Self::ty_mentions_typevar),
            Ty::Func(args, ret) => {
                args.iter().any(Self::ty_mentions_typevar) || Self::ty_mentions_typevar(ret)
            }
            _ => false,
        }
    }

    /// If `e` is a constructor call `C(...)` for a user class `C`, return `C`.
    /// (Mirrors `infer_expr_ty`'s constructor recognition: a Call whose callee is
    /// a bare Ident registered in `ctx.classes`.) Used to disambiguate a RAW
    /// struct temp (a constructor) from an enum-typed place at a base slot.
    pub(crate) fn constructor_class(&self, e: &Expr) -> Option<String> {
        if let Expr::Call { callee, .. } = e {
            if let Expr::Ident(n, _) = callee.as_ref() {
                if self.ctx.classes.contains_key(n.as_str()) {
                    return Some(n.clone());
                }
            }
        }
        None
    }

    /// (EPIC-5 C2-2b-i, the crux) Emit value expression `value` into a slot whose
    /// declared type `expected` mentions a polymorphic base (caller gated on
    /// `ty_has_poly_base`). Replaces the C1 honest gate: a raw-struct value at a
    /// `B__` slot is WRAPPED in the right enum variant; a value already typed as
    /// the base passes through; a strict-polymorphic-subclass place (multi-level
    /// upcast) is an HONEST Error::Codegen rather than a miscompile.
    pub(crate) fn emit_into_base_slot(&mut self, value: &Expr, expected: &Ty) -> Result<String> {
        match expected {
            // Scalar polymorphic-base slot `B__`.
            Ty::Class(b, _) if self.is_polymorphic_base(b) => {
                // A constructor `C(...)` is a RAW struct temp -> wrap as variant C.
                if let Some(ctor) = self.constructor_class(value) {
                    let inner = self.emit_consuming(value)?;
                    return Ok(format!("{}__::{}({})", b, ctor, inner));
                }
                let et = self.type_of_expr(value);
                match &et {
                    Ty::Class(c, _) if self.is_polymorphic_base(c) => {
                        if c == b {
                            // Already a `B__` value (a base-typed place) -> pass through.
                            self.emit_consuming(value)
                        } else if crate::typeck::is_subclass(c, b, self.ctx) {
                            // `et` is a strict POLYMORPHIC subclass: the value is an
                            // `et__` enum, NOT a `B__` variant. A From<et__> for B__
                            // up-conversion is a deferred follow-on — refuse honestly.
                            Err(crate::diag::Error::Codegen(format!(
                                "upcasting an intermediate polymorphic base `{}` to `{}` \
                                 is not yet supported — construct the value at the `{}` \
                                 slot directly (multi-level upcast deferred)",
                                c, b, b
                            )))
                        } else {
                            // Unrelated polymorphic class — typeck already rejected
                            // this flow; pass through defensively.
                            self.emit_consuming(value)
                        }
                    }
                    // A concrete / non-polymorphic value whose type is `B` or a
                    // (leaf) subclass of `B` -> RAW struct -> wrap as variant `et`.
                    Ty::Class(c, _) => {
                        let inner = self.emit_consuming(value)?;
                        Ok(format!("{}__::{}({})", b, c, inner))
                    }
                    // Non-class value into a base slot — should not occur (typeck);
                    // emit unchanged so any genuine mismatch surfaces as rustc E0308.
                    _ => self.emit_consuming(value),
                }
            }
            // List literal whose element slot mentions a polymorphic base: wrap
            // each element. A non-literal list (already `Vec<B__>`) passes through.
            Ty::List(elem) if self.ty_has_poly_base(elem) => {
                if let Expr::List(elems, _) = value {
                    let mut parts = Vec::with_capacity(elems.len());
                    for el in elems {
                        parts.push(self.emit_into_base_slot(el, elem)?);
                    }
                    Ok(format!("vec![{}]", parts.join(", ")))
                } else {
                    self.emit_consuming(value)
                }
            }
            // Set literal — same element wrapping as the list path.
            Ty::Set(elem) if self.ty_has_poly_base(elem) => {
                if let Expr::Set(elems, _) = value {
                    let mut parts = Vec::with_capacity(elems.len());
                    for el in elems {
                        parts.push(self.emit_into_base_slot(el, elem)?);
                    }
                    Ok(format!(
                        "vec![{}].into_iter().collect::<::std::collections::HashSet<_>>()",
                        parts.join(", ")
                    ))
                } else {
                    self.emit_consuming(value)
                }
            }
            // Tuple literal — wrap element-wise at each polymorphic-base position.
            Ty::Tuple(parts_ty) if self.ty_has_poly_base(expected) => {
                if let Expr::Tuple(elems, _) = value {
                    if elems.len() == parts_ty.len() {
                        let mut parts = Vec::with_capacity(elems.len());
                        for (el, et) in elems.iter().zip(parts_ty.iter()) {
                            if self.ty_has_poly_base(et) {
                                parts.push(self.emit_into_base_slot(el, et)?);
                            } else {
                                parts.push(self.emit_consuming(el)?);
                            }
                        }
                        return Ok(match parts.len() {
                            1 => format!("({},)", parts[0]),
                            _ => format!("({})", parts.join(", ")),
                        });
                    }
                }
                self.emit_consuming(value)
            }
            // Optional polymorphic-base slot: the bare-value case wraps the inner
            // value; the `None` literal and already-Optional values are handled by
            // the caller's `coerce_to_option`, so only a bare value reaches here.
            Ty::Option(inner) if self.ty_has_poly_base(inner) => {
                if matches!(value, Expr::None_(_)) {
                    self.emit_consuming(value)
                } else {
                    self.emit_into_base_slot(value, inner)
                }
            }
            // Dict with a polymorphic-base value/key slot through a literal is not
            // exercised by the corpus; defer element wrapping (honest passthrough —
            // a genuine subtype dict literal would surface as rustc E0308, not a
            // silent miscompile). Documented as a C2-3 gap alongside list+concat.
            _ => self.emit_consuming(value),
        }
    }

    /// (EPIC-5 C2-3) Emit constructor argument `arg` into a slot whose declared
    /// type is `slot` (a `__init__` param type, or a struct field type). When the
    /// slot mentions a polymorphic base, wrap a raw-struct/subclass value into the
    /// companion-enum variant (delegating to `emit_into_base_slot`, the same
    /// wrap-or-passthrough used at the return / annotated-assign / free-fn-arg
    /// sites); otherwise keep the uniform clone-on-use emission. A `None` slot
    /// (untyped / variadic) also keeps clone-on-use. This closes the constructor
    /// arg path, which the keystone's three `ty_has_poly_base` sites did not cover.
    pub(crate) fn emit_arg_into_slot(&mut self, arg: &Expr, slot: Option<&Ty>) -> Result<String> {
        match slot {
            Some(t) if self.ty_has_poly_base(t) => self.emit_into_base_slot(arg, t),
            // A `Callable` parameter slot (`Rc<dyn Fn(..) -> ..>`): a bare function
            // name or lambda argument must be wrapped (`Rc::new(..) as Rc<dyn Fn>`)
            // — a closure does not auto-coerce to `Rc<dyn Fn>` at a call boundary.
            // This mirrors the free-function-call path (`emit_plain_func_call`) so
            // a `Callable` field/param reached through a constructor or method call
            // wraps identically. Values already of `Ty::Func` pass through as a
            // cheap `Rc` clone inside `emit_into_func_slot`.
            Some(t) if self.ty_has_func(t) => self.emit_into_func_slot(arg, t),
            // (card 30e4fdd0) An `Optional[T]` parameter slot (a constructor's
            // `slot: Optional[Inner]`, a struct-literal field): Some-wrap a bare `T`
            // value passed positionally, exactly like the free-function call path
            // (`emit_call_arg_value`, coerced) and the local-decl / return / field-
            // assign positions. `coerce_to_option` is a no-op for a `None` literal,
            // an already-`Option` value, or a non-Option slot, so this only ADDS the
            // wrap that was missing (a bare value into an Optional ctor slot was
            // rustc E0308 before).
            Some(t) if matches!(t, Ty::Option(_)) => {
                let s = self.emit_consuming(arg)?;
                Ok(self.coerce_to_option(s, arg, t))
            }
            _ => self.emit_consuming(arg),
        }
    }

    /// (kwargs v1) Emit one call argument `a` into parameter SLOT `p`, applying
    /// the site's argument→slot coercion.
    ///
    /// `coerced == true` (free-function AND method calls) applies the FULL
    /// coercion the free-call path has always used: a function name / lambda into
    /// a `Callable` (or `Optional[Callable]`) slot is cast to `Rc<dyn Fn>`, a
    /// raw-struct / subclass into a polymorphic-base slot is wrapped in its
    /// variant, and a bare value into an `Optional[T]` slot is `Some(..)`-wrapped
    /// (a `None` literal stays `None`). This is what routes the METHOD kwargs /
    /// default-fill path through the same emission as the free path — previously
    /// it emitted a plain `emit_consuming`, dropping the Optional-wrap and the
    /// `Callable` cast (so a method's `Optional[int] = None` default filled with a
    /// bare value, or a lambda into a method's `Optional[Callable]` slot, miscompiled).
    ///
    /// `coerced == false` (constructor `::new`) routes through
    /// [`Self::emit_arg_into_slot`] (poly-base / `Callable` / clone-on-use),
    /// matching the long-standing constructor struct-field emission.
    pub(crate) fn emit_call_arg_value(
        &mut self,
        a: &Expr,
        param_tys: &[Ty],
        p: usize,
        coerced: bool,
    ) -> Result<String> {
        if !coerced {
            return self.emit_arg_into_slot(a, param_tys.get(p));
        }
        let s = match param_tys.get(p) {
            Some(pt @ Ty::Func(..)) => self.emit_into_func_slot(a, pt)?,
            Some(Ty::Option(inner))
                if matches!(**inner, Ty::Func(..)) && !matches!(a, Expr::None_(_)) =>
            {
                self.emit_into_func_slot(a, inner)?
            }
            Some(pt) if self.ty_has_poly_base(pt) => self.emit_into_base_slot(a, pt)?,
            _ => self.emit_consuming(a)?,
        };
        Ok(match param_tys.get(p) {
            Some(pt) => self.coerce_to_option(s, a, pt),
            None => s,
        })
    }

    /// (kwargs v1 — call-site evaluation-order fix) Lower a keyword / default-
    /// filled call's argument list in PARAMETER-SLOT order, returning
    /// `(prelude, call_parts)`. The caller wraps the assembled call with
    /// [`Self::hoist_wrap`]`(&prelude, format!("<callee>({})", call_parts.join(", ")))`.
    ///
    /// CPython evaluates call arguments left-to-right in SOURCE order (positionals
    /// first, then keywords as written) BEFORE binding them to parameter slots.
    /// When the provided arguments already sit in ascending slot order every part
    /// emits INLINE in slot order — byte-identical to a positional call, so the
    /// hot in-order path is unchanged. Otherwise EVERY provided argument is
    /// hoisted into a source-ordered `let __argN` temp so its side effects run in
    /// source order; a by-reference (`Mut[T]`) argument instead hoists only its
    /// place-prelude (subscript-index side effects) into the shared prelude while
    /// the `&mut place` borrow stays inline at its slot (borrow legality
    /// unchanged). Default-filled slots always emit their declared default inline
    /// in slot position (a signature default has no call-site side effect to order).
    ///
    /// This single helper is shared by all three keyword-bearing call sites
    /// (`emit_plain_func_call`, `emit_user_method_call`, the constructor `::new`
    /// path) — the reviews found the same eval-order miscompile independently in
    /// each because the hoist logic used to be triplicated.
    ///
    /// `param_tys` supplies per-slot coercion targets and `coerced` selects the
    /// coercion strategy (see [`Self::emit_call_arg_value`]); `sig` supplies
    /// `param_by_ref` and `param_defaults`.
    pub(crate) fn emit_slotted_args(
        &mut self,
        slots: &[crate::typeck::ArgSlot],
        args: &[Expr],
        kwargs: &[(String, Expr)],
        sig: &crate::typeck::FuncSig,
        param_tys: &[Ty],
        coerced: bool,
    ) -> Result<(Vec<String>, Vec<String>)> {
        use crate::typeck::ArgSlot;
        let n = slots.len();
        // Source (evaluation) order of the PROVIDED args: positionals left-to-
        // right, then keyword values in source order — each tagged with the slot
        // it fills. Exactly CPython's call-site evaluation order.
        let eval_order = crate::typeck::kwargs_provided_in_eval_order(args, kwargs, slots);
        // Hoist exactly when the provided args are NOT already in ascending slot
        // order (a keyword binds an earlier slot than a later-written argument).
        let needs_hoist = eval_order.windows(2).any(|w| w[0].0 > w[1].0);

        let mut call_part: Vec<Option<String>> = vec![None; n];
        let mut prelude: Vec<String> = Vec::new();

        if needs_hoist {
            for (rank, (p, a)) in eval_order.iter().enumerate() {
                let p = *p;
                if sig.param_by_ref.get(p).copied().unwrap_or(false) {
                    // Hoist the place's index side effects into the shared prelude
                    // (at this arg's SOURCE position); keep the `&mut` borrow
                    // inline at its slot so borrow legality is unchanged. The
                    // shared `prelude` gives each hoisted `__idxN` a unique name.
                    let place = self.emit_place_hoisted(a, &mut prelude)?;
                    call_part[p] = Some(self.byref_borrow(a, &place));
                } else {
                    let v = self.emit_call_arg_value(a, param_tys, p, coerced)?;
                    let tmp = format!("__arg{}", rank);
                    prelude.push(format!("let {} = {};", tmp, v));
                    call_part[p] = Some(tmp);
                }
            }
        } else {
            // In slot order (matches the legacy emission order → byte-identical).
            for (p, s) in slots.iter().enumerate() {
                let a = match s {
                    ArgSlot::Pos(i) => &args[*i],
                    ArgSlot::Kw(j) => &kwargs[*j].1,
                    ArgSlot::Default => continue,
                };
                if sig.param_by_ref.get(p).copied().unwrap_or(false) {
                    let mut aprelude = Vec::new();
                    let place = self.emit_place_hoisted(a, &mut aprelude)?;
                    let borrow = self.byref_borrow(a, &place);
                    call_part[p] = Some(Self::hoist_wrap(&aprelude, borrow));
                } else {
                    call_part[p] = Some(self.emit_call_arg_value(a, param_tys, p, coerced)?);
                }
            }
        }

        // Assemble parts in slot order; fill each Default slot inline with the
        // parameter's declared default.
        let mut call_parts = Vec::with_capacity(n);
        for (p, s) in slots.iter().enumerate() {
            match call_part[p].take() {
                Some(part) => call_parts.push(part),
                None => {
                    debug_assert!(matches!(s, ArgSlot::Default));
                    match sig.param_defaults.get(p).and_then(|d| d.as_ref()) {
                        Some(e) => {
                            let e = e.clone();
                            // (enabler-fix-1 #6) Route an OMITTED default through the
                            // same slot coercion as a provided arg — a bare emit_expr
                            // dropped the `Some(..)` wrap for an `Optional[T] = <lit>`
                            // default (`opt: Optional[int] = 5` filled `5` into an
                            // `Option<i64>` slot -> rustc E0308). emit_call_arg_value
                            // applies coerce_to_option (and the Callable / poly-base
                            // casts) in both the coerced and constructor paths.
                            call_parts.push(self.emit_call_arg_value(&e, param_tys, p, coerced)?);
                        }
                        None => {
                            return Err(crate::diag::Error::Codegen(
                                "missing required argument".into(),
                            ))
                        }
                    }
                }
            }
        }
        Ok((prelude, call_parts))
    }

    /// (first-class functions) Emit value expression `value` into a slot whose
    /// declared type `expected` is `Ty::Func(arg_tys, ret)` — i.e. a
    /// `Rc<dyn Fn(..) -> ..>` slot. Three shapes:
    ///
    ///  - A bare top-level function NAME used as a value: a Rust `fn` item
    ///    coerces to `dyn Fn`, so emit `Rc::new(<name>) as Rc<dyn Fn(..)->..>`.
    ///    The trailing `as` cast pins the type at the slot so an unannotated
    ///    binding / collection element is still well-typed.
    ///  - A LAMBDA: emit `Rc::new(move |x: A, y: B| body) as Rc<dyn Fn(..)->..>`.
    ///    Capture-by-move closes over any enclosing variable (the `make_adder`
    ///    closure captures `n`); the param TYPES come from the slot's `arg_tys`
    ///    so the closure body type-checks without inference from a call site.
    ///  - Anything else already of `Ty::Func` (a func-valued place, or a call
    ///    that already returns `Rc<dyn Fn>`): clone-on-use, which is a cheap `Rc`
    ///    refcount bump for a place and a pass-through for an owned temp.
    pub(crate) fn emit_into_func_slot(&mut self, value: &Expr, expected: &Ty) -> Result<String> {
        // A collection slot whose element / value type is a function
        // (`list[Callable[..]]`, `dict[K, Callable[..]]`) wraps each element /
        // value into the `Rc<dyn Fn>` slot — only when the source is the matching
        // LITERAL (so the element types are known here); a non-literal collection
        // is already `Rc<dyn Fn>`-typed and passes through via clone-on-use.
        match expected {
            Ty::List(elem) if matches!(**elem, Ty::Func(..)) => {
                if let Expr::List(elems, _) = value {
                    let mut parts = Vec::with_capacity(elems.len());
                    for el in elems {
                        parts.push(self.emit_into_func_slot(el, elem)?);
                    }
                    return Ok(format!("vec![{}]", parts.join(", ")));
                }
                return self.emit_consuming(value);
            }
            Ty::Dict(_k, vv) if self.ty_has_func(vv) => {
                if let Expr::Dict(pairs, _) = value {
                    if pairs.is_empty() {
                        return Ok("::std::collections::HashMap::new()".to_string());
                    }
                    let mut inserts = Vec::with_capacity(pairs.len());
                    for (k, v) in pairs {
                        let ks = self.emit_consuming(k)?;
                        let vs = self.emit_into_func_slot(v, vv)?;
                        inserts.push(format!("({}, {})", ks, vs));
                    }
                    return Ok(format!(
                        "vec![{}].into_iter().collect::<::std::collections::HashMap<_,_>>()",
                        inserts.join(", ")
                    ));
                }
                return self.emit_consuming(value);
            }
            // A tuple slot with one or more function-typed positions
            // (`tuple[Callable[..], int]`). Wrap each element into its own slot:
            // a func position routes through `emit_into_func_slot` (recursively),
            // a non-func position keeps the clone-on-use emission. Mirrors the
            // single-element / multi-element tuple emission in `emit_expr`.
            Ty::Tuple(elem_tys) if self.ty_has_func(expected) => {
                if let Expr::Tuple(elems, _) = value {
                    if elems.len() == elem_tys.len() {
                        let mut parts = Vec::with_capacity(elems.len());
                        for (el, et) in elems.iter().zip(elem_tys.iter()) {
                            if self.ty_has_func(et) {
                                parts.push(self.emit_into_func_slot(el, et)?);
                            } else {
                                parts.push(self.emit_consuming(el)?);
                            }
                        }
                        return Ok(match parts.len() {
                            1 => format!("({},)", parts[0]),
                            _ => format!("({})", parts.join(", ")),
                        });
                    }
                }
                return self.emit_consuming(value);
            }
            // NOTE: there is intentionally NO `Ty::Set(Func)` arm. A pyrst `set`
            // lowers to a Rust `HashSet`, which requires `Eq + Hash` elements;
            // `Rc<dyn Fn>` (and `dyn Fn`) implement neither, so `HashSet<Rc<dyn
            // Fn>>` cannot compile. `set[Callable[..]]` is therefore rejected at
            // typeck (`require_hashable`), the same way `set[float]` is — so this
            // arm is unreachable and a positive emission here would only produce
            // known-uncompilable Rust.
            _ => {}
        }
        let Ty::Func(arg_tys, _ret) = expected else {
            return self.emit_consuming(value);
        };
        let rc_ty = self.rust_ty(expected);
        match value {
            // A function NAME used as a value (must be a known top-level function,
            // not a local that happens to share the name — locals shadow and are
            // already `Rc<dyn Fn>` values handled by the clone-on-use arm below).
            Expr::Ident(n, _)
                if self.ctx.funcs.contains_key(n.as_str())
                    && !self.locals.contains_key(n.as_str()) =>
            {
                Ok(format!("::std::rc::Rc::new({}) as {}", escape_ident(n), rc_ty))
            }
            Expr::Lambda { params, body, .. } => {
                // Annotate each closure param with the slot's argument type so the
                // `move` closure is well-typed at a `dyn Fn` coercion (Rust cannot
                // infer closure param types across the boxed-trait-object cast).
                // When the slot's argument type is `Unknown`, emit the param WITHOUT
                // an annotation (let Rust infer) rather than `x: ()` — `rust_ty`
                // lowers `Unknown` to `()`, and a unit-typed param would be wrong
                // for any non-unit argument. Annotated `Callable` slots always have
                // concrete arg types (from `from_type_expr`), so for Increment 1
                // this is a defensive guard; it becomes load-bearing once a func
                // value can flow from an inferred (Unknown-arg) context.
                let param_strs: Vec<String> = params
                    .iter()
                    .enumerate()
                    .map(|(i, (name, _))| {
                        let name_e = escape_ident(name);
                        match arg_tys.get(i) {
                            Some(pty) if !matches!(pty, Ty::Unknown) => {
                                format!("{}: {}", name_e, self.rust_ty(pty))
                            }
                            _ => name_e,
                        }
                    })
                    .collect();
                let body_s = self.emit_expr(body)?;
                Ok(format!(
                    "::std::rc::Rc::new(move |{}| {}) as {}",
                    param_strs.join(", "),
                    body_s,
                    rc_ty
                ))
            }
            // A conditional `f if cond else g` into a function slot: wrap EACH
            // branch into the same slot so a bare fn name / lambda in either arm
            // becomes `Rc<dyn Fn>` (without this the arms fall to `emit_consuming`
            // and emit bare fn names -> E0308). Both arms are already typed
            // `Ty::Func` by typeck's branch unification, so each is a valid
            // func-slot value.
            Expr::IfExp { test, body, orelse, .. } => {
                let t = self.emit_expr(test)?;
                let b = self.emit_into_func_slot(body, expected)?;
                let o = self.emit_into_func_slot(orelse, expected)?;
                Ok(format!("(if {} {{ {} }} else {{ {} }})", t, b, o))
            }
            // A func-valued place / call temp — Rc clone (value semantics) / passthrough.
            _ => self.emit_consuming(value),
        }
    }

    /// (EPIC-5 C2-3) The declared pyrst `Ty` of field `field_name` on class
    /// `class_def`, looking through inherited base fields (mirrors the constructor
    /// branch's own + inherited field walk). `None` when the field is unknown.
    pub(crate) fn class_field_type(&self, class_def: &ClassDef, field_name: &str) -> Option<Ty> {
        self.ctx
            .get_all_fields(&class_def.name)
            .iter()
            .find(|f| f.name == field_name)
            // Scope the field annotation with the class's type params so a generic
            // field (`Callable[[], V]`, `value: T`) lowers `V`/`T` to a
            // `Ty::TypeVar`, not a `Ty::Class("V", [])`. The current call sites only
            // match the OUTER `Ty::Func(..)` (unaffected), so this is hygiene that
            // keeps the inner type honest for any future inspection.
            .and_then(|f| Ty::from_type_expr_scoped(&f.ty, f.span, &class_def.type_params).ok())
    }

    /// (EPIC-5) Coerce an already-emitted expression `s` (for source expr `e`)
    /// into the Rust representation expected by `target` when `target` is
    /// `Option<T>`:
    ///   - a `None` literal  -> `None`
    ///   - a value already typed `Option<_>` (e.g. an Optional var, or a call
    ///     returning Optional) -> passed through unchanged
    ///   - any other bare value -> `Some(s)`  (the auto-Some that mirrors
    ///     typeck's `T ~ Optional[T]` compatibility arm)
    /// When `target` is not an Option, `s` is returned unchanged. This is the
    /// single wrapping point shared by assignment, return, and argument sites so
    /// the three never drift.
    pub(crate) fn coerce_to_option(&self, s: String, e: &Expr, target: &Ty) -> String {
        if !matches!(target, Ty::Option(_)) {
            return s;
        }
        if matches!(e, Expr::None_(_)) {
            return "None".to_string();
        }
        if matches!(self.type_of_expr(e), Ty::Option(_)) {
            return s;
        }
        format!("Some({})", s)
    }

    /// True when `e` emits an integer-valued (`i64`) Rust expression whose
    /// *logical* type (per the inference oracle) is nonetheless `Float`.
    ///
    /// The only such case is integer exponentiation: D5 (Python semantics) makes
    /// the oracle type `int ** int` as `Float`, but emission is operand-driven —
    /// `int ** int` is lowered to the `i64`-returning `__py_ipow` (or a folded
    /// `i64` literal), matching the Pow arm in `emit_expr`. A `float`-typed
    /// binding receiving such a value therefore still needs an `as f64` cast,
    /// which the plain `type_of_expr(value) == Int` coercion check no longer
    /// detects now that the oracle reports `Float`. This predicate restores that
    /// signal so the keystone oracle composes with the pow-into-float emission.
    pub(crate) fn emits_int_pow(&self, e: &Expr) -> bool {
        match e {
            // `-(int ** int)` is still an integer value.
            Expr::UnOp { op: UnOp::Neg, expr, .. } => self.emits_int_pow(expr),
            Expr::BinOp { lhs, op: BinOp::Pow, rhs, .. } => {
                // Mirror the emit_expr Pow rule: int**int -> i64 (__py_ipow);
                // any float operand -> f64 (powf).
                matches!(self.type_of_expr(lhs), Ty::Int)
                    && matches!(self.type_of_expr(rhs), Ty::Int)
            }
            _ => false,
        }
    }

    pub(crate) fn emit_top_stmt(&mut self, s: &Stmt) -> Result<()> {
        match s {
            Stmt::Func(f) => {
                // Skip dead functions (not called anywhere) unless it's main
                if f.name != "main" && self.dead_funcs.contains(&f.name) {
                    self.line(&format!("// Dead function removed: {}", f.name));
                    return Ok(());
                }
                self.emit_func(f, /*method_of=*/ None)
            }
            Stmt::Class(c) => {
                // extract_init_fields is already called by resolver.rs:132 when
                // building TyCtx, so ctx.classes already holds the populated
                // ClassDef.  emit_class reads fields via ctx.get_all_fields, not
                // from c.fields directly, so no clone+mutate is needed here.
                self.emit_class(c)
            }
            other => {
                // Silently accept a bare top-level `main()` call — the Rust
                // `fn main() { user_main(); }` already drives the entry point,
                // so this idiom is a recognised no-op.
                if matches!(
                    other,
                    Stmt::Expr(crate::ast::Expr::Call { callee, args, kwargs, .. })
                        if matches!(callee.as_ref(), crate::ast::Expr::Ident(name, _) if name == "main")
                            && args.is_empty()
                            && kwargs.is_empty()
                ) {
                    return Ok(());
                }
                // A module-level constant (`NAME: T = <literal>`) is already
                // emitted as a top-level Rust `const` by the prepass in
                // `emit_program` (which runs before any function so call sites
                // resolve), so it is a recognised no-op here.
                if crate::typeck::is_module_const_decl(other) {
                    return Ok(());
                }
                // Any other unsupported top-level statement is an honest error.
                // This arm is a backstop; typeck's check_bodies fires the same
                // rejection earlier (at `pyrst check` time).
                Err(crate::diag::Error::Codegen(
                    "top-level statements other than function/class/import \
                     definitions (and module-level constants `NAME: T = <literal>`) \
                     are not supported"
                        .to_string(),
                ))
            }
        }
    }

    /// Emit a MODULE-LEVEL CONSTANT (`NAME: T = <literal>`) as a top-level Rust
    /// `const`. Called by `emit_program`'s prepass for every statement that
    /// [`crate::typeck::is_module_const_decl`] accepts, so the value is always one
    /// of the four primitive literals.
    ///
    /// The Rust identifier is MANGLED via [`mangle_const`] (`__pyrst_const_<name>`)
    /// so a lowercase const name (e.g. `k`/`i`/`e`) cannot be captured as a Rust
    /// CONSTANT PATTERN in any closure/`for`/`match` pattern position in the
    /// generated crate (which would silently miscompile, rustc E0308). The same
    /// mangled name is emitted at every reference site.
    ///
    /// int/float/bool lower to `const <mangled>: <i64|f64|bool> = <value>;` — all
    /// `Copy`, so a reference uses the mangled name directly. A `str` constant
    /// lowers to `const <mangled>: &str = "...";` (a `String` is not
    /// const-constructible), so REFERENCES to a str const append `.to_string()`
    /// to preserve pyrst's `str == Rust String` value semantics.
    pub(crate) fn emit_const_decl(&mut self, s: &Stmt) -> Result<()> {
        let Stmt::Assign { target, value, .. } = s else {
            return Err(crate::diag::Error::Codegen(
                "emit_const_decl called on a non-assignment".to_string(),
            ));
        };
        let name = mangle_const(target);
        let decl = match value {
            Expr::Int(n, _) => format!("const {}: i64 = {};", name, n),
            // Suffix `f64` so a whole-number float literal (`6.0` formats as
            // "6") is still a valid f64 const initializer (`6f64`), and a
            // fractional one (`3.14`) stays `3.14f64`.
            Expr::Float(f, _) => format!("const {}: f64 = {}f64;", name, f),
            Expr::Bool(b, _) => format!("const {}: bool = {};", name, b),
            Expr::Str(st, _) => format!("const {}: &str = {:?};", name, st),
            _ => {
                return Err(crate::diag::Error::Codegen(
                    "module constant value must be an int/float/str/bool literal".to_string(),
                ))
            }
        };
        self.line(&decl);
        Ok(())
    }

    /// (card 30e4fdd0) True when a field of lowered type `ty` on class `cname`
    /// contains an INLINE self-reference to `cname` — reachable through
    /// `Option`/`Tuple` but NOT behind a heap-allocated `Vec`/`HashMap`/`HashSet`
    /// (which already break the cycle). Such a field makes the Rust struct
    /// infinite-size (E0072: `next: Option<Node>` inside `Node`), so codegen boxes
    /// the recursive occurrence (`Option<Box<Node>>`) and box/unboxes at the field
    /// boundary. A `list[Node]` self-reference is NOT boxed — the `Vec` is already
    /// indirection, so trees-via-list compile directly.
    pub(crate) fn field_needs_box(&self, cname: &str, ty: &Ty) -> bool {
        fn inline_self_ref(ty: &Ty, cname: &str) -> bool {
            match ty {
                Ty::Class(n, _) => n == cname,
                Ty::Option(inner) => inline_self_ref(inner, cname),
                Ty::Tuple(ts) => ts.iter().any(|t| inline_self_ref(t, cname)),
                _ => false,
            }
        }
        inline_self_ref(ty, cname)
    }

    /// (enabler-fix-1 #4a) Box a STRUCT-LITERAL field value for a self-referential
    /// (boxed) field: the struct stores `Option<Box<Self>>`, so a `Some(payload)`
    /// value is boxed via `.map(Box::new)` (None unchanged), matching the AttrAssign
    /// write combinator. A non-recursive field passes through unchanged. Used at the
    /// dataclass / no-__init__ constructor call sites, which build the struct
    /// directly (the `self.x = ..` boxing path never runs for them).
    pub(crate) fn box_recursive_field_value(&self, cname: &str, fty: &Ty, v: String) -> String {
        if self.field_needs_box(cname, fty) {
            format!("({}).map(::std::boxed::Box::new)", v)
        } else {
            v
        }
    }

    /// (card 30e4fdd0) Rust type for a boxed-recursive field: wrap each INLINE
    /// self-reference to `cname` in `Box<..>` (`Option<Node>` ->
    /// `Option<Box<Node>>`). Non-recursive positions lower via the ordinary
    /// `rust_ty`, so a mixed tuple `(int, Node)` boxes only the `Node`.
    pub(crate) fn rust_ty_box_recursive(&self, ty: &Ty, cname: &str) -> String {
        match ty {
            Ty::Class(n, _) if n == cname => format!("Box<{}>", self.rust_ty(ty)),
            Ty::Option(inner) => {
                format!("Option<{}>", self.rust_ty_box_recursive(inner, cname))
            }
            Ty::Tuple(ts) => {
                let parts: Vec<String> = ts
                    .iter()
                    .map(|t| self.rust_ty_box_recursive(t, cname))
                    .collect();
                if parts.len() == 1 {
                    format!("({},)", parts[0])
                } else {
                    format!("({})", parts.join(", "))
                }
            }
            _ => self.rust_ty(ty),
        }
    }

    /// (card 03eb4e2c; enabler-fix-1 #3) True when class `cname`'s field `field`
    /// (own OR inherited) is a promoted CLASS-LEVEL CONSTANT (enum member) rather than
    /// an instance struct field — it lowers to an associated `const`
    /// (`DefiningClass::FIELD`), is excluded from the struct definition + constructor,
    /// and is accessed via `DefiningClass::FIELD`.
    ///
    /// Delegates to the SHARED, whole-program predicate `TyCtx::is_promoted_const`
    /// (usage-gated: read as `ClassName.FIELD` AND never written) so typeck and
    /// codegen agree exactly. The old own-fields-only, self-write-only heuristic
    /// over-promoted a mutable "options with defaults" record (external `o.x = ..`
    /// or `X(5)` construction) into a `const` — a check-pass / build-fail hole.
    pub(crate) fn is_class_const_field(&self, cname: &str, field: &str) -> bool {
        self.ctx.is_promoted_const(cname, field)
    }

    /// (card 03eb4e2c) The class-level constant fields declared in `cname`'s OWN
    /// body, in declaration order — emitted as associated `const`s in its impl.
    pub(crate) fn class_const_fields(&self, cname: &str) -> Vec<crate::ast::Param> {
        let Some(cd) = self.ctx.classes.get(cname) else { return vec![]; };
        cd.fields
            .iter()
            .filter(|f| self.is_class_const_field(cname, &f.name))
            .cloned()
            .collect()
    }

    /// (card 03eb4e2c) The Rust `const NAME: T = <lit>;` declaration for a
    /// class-level constant field. Mirrors `emit_const_decl`'s literal formatting
    /// (str -> `&str`, whole-number float -> `f64` suffix) but is UNMANGLED — the
    /// const lives in the class impl namespace (`ClassName::NAME`).
    pub(crate) fn class_const_decl(&self, f: &crate::ast::Param) -> Result<String> {
        let n = escape_ident(&f.name);
        Ok(match &f.default {
            Some(Expr::Int(v, _)) => format!("const {}: i64 = {};", n, v),
            Some(Expr::Float(v, _)) => format!("const {}: f64 = {}f64;", n, v),
            Some(Expr::Bool(v, _)) => format!("const {}: bool = {};", n, v),
            Some(Expr::Str(v, _)) => format!("const {}: &str = {:?};", n, v),
            _ => {
                return Err(crate::diag::Error::Codegen(format!(
                    "class constant `{}` must be an int/float/str/bool literal",
                    f.name
                )))
            }
        })
    }

    /// Whether an lvalue / receiver chain bottoms out at the `self` receiver —
    /// i.e. walking through `Attr`/`Index` bases reaches `Expr::Ident("self")`.
    /// Used to decide a method needs `&mut self` when it mutates anything rooted
    /// at self (`self.x`, `self.dict[k]`, `self.rooms[i].field`, ...).
    pub(crate) fn expr_roots_at_self(e: &Expr) -> bool {
        match e {
            Expr::Ident(n, _) => n == "self",
            Expr::Attr { obj, .. } | Expr::Index { obj, .. } => Self::expr_roots_at_self(obj),
            _ => false,
        }
    }

    pub(crate) fn method_modifies_self(&self, body: &[Stmt]) -> bool {
        for stmt in body {
            match stmt {
                // Any assignment whose target base chain roots at `self` mutates
                // it: `self.x = v`, `self.dict[k] = v`, `self.a.b = v`,
                // `self.rooms[i].field = v`.
                Stmt::AttrAssign { obj, .. } | Stmt::IndexAssign { obj, .. } => {
                    if Self::expr_roots_at_self(obj) {
                        return true;
                    }
                }
                // Check for method calls that mutate (like self.items.append()
                // or self.rooms[i].append()) — any mutating call whose receiver
                // chain roots at `self`.
                Stmt::Expr(Expr::Call { callee, .. }) => {
                    if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                        if MUTATING_METHODS.contains(&name.as_str()) {
                            if Self::expr_roots_at_self(obj) {
                                return true;
                            }
                        }
                    }
                }
                Stmt::If { then, elifs, else_, .. } => {
                    if self.method_modifies_self(then) {
                        return true;
                    }
                    for (_, elif_body) in elifs {
                        if self.method_modifies_self(elif_body) {
                            return true;
                        }
                    }
                    if let Some(else_body) = else_ {
                        if self.method_modifies_self(else_body) {
                            return true;
                        }
                    }
                }
                Stmt::While { body, .. } | Stmt::For { body, .. } => {
                    if self.method_modifies_self(body) {
                        return true;
                    }
                }
                Stmt::Try { body, handlers, else_, finally_, .. } => {
                    if self.method_modifies_self(body) {
                        return true;
                    }
                    for handler in handlers {
                        if self.method_modifies_self(&handler.body) {
                            return true;
                        }
                    }
                    if let Some(else_body) = else_ {
                        if self.method_modifies_self(else_body) {
                            return true;
                        }
                    }
                    if let Some(finally_body) = finally_ {
                        if self.method_modifies_self(finally_body) {
                            return true;
                        }
                    }
                }
                Stmt::With { body, .. } => {
                    if self.method_modifies_self(body) {
                        return true;
                    }
                }
                _ => {}
            }
            // (EPIC-4 V2-c / V3 interaction) A call anywhere in this statement that
            // passes a self-rooted place (`self.field`, `self.list[i]`, ...) into a
            // by-reference (`Mut[T]`) parameter MUTATES self — the callee writes
            // through the `&mut self.field` borrow. The intra-method seed above
            // misses it (it only catches `self`-rooted assignments and mutating
            // method calls), so without this a method that mutates self ONLY by
            // handing `self.field` to a by-ref callee would be emitted `&self` and
            // rustc would reject `&mut self.field` with E0596. Detect it here so
            // the method becomes `&mut self` and propagates through the V3 fixpoint.
            if self.stmt_passes_self_by_ref(stmt) {
                return true;
            }
            // A self-rooted in-place mutator (`self.items.pop()`, `self.x.insert(..)`,
            // ...) used in ANY expression position — most importantly as a RETURN
            // value (`return self.items.pop()`) or assignment RHS — also mutates
            // self. The `Stmt::Expr` arm above only catches a BARE statement call;
            // a mutator whose result is consumed (popleft/pop returning the removed
            // element) lives inside `Stmt::Return`/`Stmt::Assign` and was missed,
            // emitting `&self` and tripping E0596 on the `&mut self.field` borrow.
            if self.stmt_mutates_self_in_expr(stmt) {
                return true;
            }
        }
        false
    }

    /// True when any statement-embedded expression contains a call
    /// `<self-rooted>.<mutating_method>(..)`. Walks the same statement-nesting
    /// surface `stmt_passes_self_by_ref` does (return values, RHS, conditions,
    /// iterables, call args), then scans each expression recursively.
    pub(crate) fn stmt_mutates_self_in_expr(&self, stmt: &Stmt) -> bool {
        let mut found = false;
        let mut check = |e: &Expr| { if Self::expr_mutates_self(e) { found = true; } };
        match stmt {
            Stmt::Expr(e) | Stmt::Return(Some(e), _) => check(e),
            Stmt::Assign { value, .. } | Stmt::AugAssign { value, .. } => check(value),
            Stmt::Unpack { value, .. } => check(value),
            Stmt::AttrAssign { obj, value, .. } | Stmt::IndexAssign { obj, value, .. } => {
                check(obj);
                check(value);
            }
            Stmt::If { cond, .. } => check(cond),
            Stmt::While { cond, .. } => check(cond),
            Stmt::For { iter, .. } => check(iter),
            Stmt::With { ctx_expr, .. } => check(ctx_expr),
            _ => {}
        }
        found
    }

    /// Recursively scan `e` for a `<self-rooted>.<mutating_method>(..)` call.
    /// `MUTATING_METHODS` is the shared in-place-mutator name set; the receiver
    /// chain must root at `self` (`self.items`, `self.a.b[i]`, ...).
    pub(crate) fn expr_mutates_self(e: &Expr) -> bool {
        match e {
            Expr::Call { callee, args, kwargs, .. } => {
                if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                    if MUTATING_METHODS.contains(&name.as_str())
                        && Self::expr_roots_at_self(obj)
                    {
                        return true;
                    }
                }
                Self::expr_mutates_self(callee)
                    || args.iter().any(Self::expr_mutates_self)
                    || kwargs.iter().any(|(_, v)| Self::expr_mutates_self(v))
            }
            Expr::Attr { obj, .. } => Self::expr_mutates_self(obj),
            Expr::Index { obj, idx, .. } => {
                Self::expr_mutates_self(obj) || Self::expr_mutates_self(idx)
            }
            Expr::BinOp { lhs, rhs, .. } => {
                Self::expr_mutates_self(lhs) || Self::expr_mutates_self(rhs)
            }
            Expr::UnOp { expr, .. } => Self::expr_mutates_self(expr),
            Expr::IfExp { test, body, orelse, .. } => {
                Self::expr_mutates_self(test)
                    || Self::expr_mutates_self(body)
                    || Self::expr_mutates_self(orelse)
            }
            // Collection LITERALS hold sub-expressions that may each be a
            // self-mutating call: `return [self.items.pop(), self.items.pop()]`
            // mutates self through the list elements. Without recursing here the
            // method is emitted `&self` and the `&mut self.field` borrow inside the
            // popped element trips E0596.
            Expr::List(elems, _) | Expr::Tuple(elems, _) | Expr::Set(elems, _) => {
                elems.iter().any(Self::expr_mutates_self)
            }
            Expr::Dict(pairs, _) => pairs
                .iter()
                .any(|(k, v)| Self::expr_mutates_self(k) || Self::expr_mutates_self(v)),
            _ => false,
        }
    }

    /// True when any `Expr::Call` reachable from `stmt` (in any expression
    /// position) passes a SELF-ROOTED place as a by-reference (`Mut[T]`) argument.
    /// Walks the same statement nesting `method_modifies_self` does and scans
    /// every embedded expression (conditions, RHS, return values, call args).
    pub(crate) fn stmt_passes_self_by_ref(&self, stmt: &Stmt) -> bool {
        let mut found = false;
        let mut check = |e: &Expr| { if self.expr_passes_self_by_ref(e) { found = true; } };
        match stmt {
            Stmt::Expr(e) | Stmt::Return(Some(e), _) => check(e),
            Stmt::Assign { value, .. } | Stmt::AugAssign { value, .. } => check(value),
            Stmt::Unpack { value, .. } => check(value),
            Stmt::AttrAssign { obj, value, .. } | Stmt::IndexAssign { obj, value, .. } => {
                check(obj);
                check(value);
            }
            Stmt::If { cond, .. } => check(cond),
            Stmt::While { cond, .. } => check(cond),
            Stmt::For { iter, .. } => check(iter),
            Stmt::With { ctx_expr, .. } => check(ctx_expr),
            _ => {}
        }
        found
    }

    /// Recursively scan `e` for a call that passes a self-rooted place into a
    /// by-reference param. For each `Expr::Call`, resolve the callee's per-param
    /// by-ref flags (free function via `ctx.funcs`; method via `get_method`,
    /// self-exclusive and index-aligned to the args after STEP 0) and report a
    /// self-rooted place sitting in a by-ref slot. Sub-expressions are walked too
    /// so a by-ref call nested in an argument / operand is still caught.
    pub(crate) fn expr_passes_self_by_ref(&self, e: &Expr) -> bool {
        match e {
            Expr::Call { callee, args, kwargs, .. } => {
                let by_ref: Vec<bool> = match callee.as_ref() {
                    Expr::Ident(n, _) => self.ctx.funcs.get(n.as_str())
                        .map(|s| s.param_by_ref.clone()).unwrap_or_default(),
                    Expr::Attr { obj, name, .. } => {
                        if let Ty::Class(cls, _) = self.type_of_expr(obj.as_ref()) {
                            self.ctx.get_method(&cls, name)
                                .map(|s| s.param_by_ref.clone()).unwrap_or_default()
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                };
                for (i, a) in args.iter().enumerate() {
                    if by_ref.get(i).copied().unwrap_or(false)
                        && Self::expr_roots_at_self(a)
                    {
                        return true;
                    }
                }
                // Walk callee + args + kwargs for nested by-ref-self calls.
                if self.expr_passes_self_by_ref(callee) { return true; }
                if args.iter().any(|a| self.expr_passes_self_by_ref(a)) { return true; }
                if kwargs.iter().any(|(_, v)| self.expr_passes_self_by_ref(v)) { return true; }
                false
            }
            Expr::Attr { obj, .. } => self.expr_passes_self_by_ref(obj),
            Expr::Index { obj, idx, .. } => {
                self.expr_passes_self_by_ref(obj) || self.expr_passes_self_by_ref(idx)
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.expr_passes_self_by_ref(lhs) || self.expr_passes_self_by_ref(rhs)
            }
            Expr::UnOp { expr, .. } => self.expr_passes_self_by_ref(expr),
            Expr::IfExp { test, body, orelse, .. } => {
                self.expr_passes_self_by_ref(test)
                    || self.expr_passes_self_by_ref(body)
                    || self.expr_passes_self_by_ref(orelse)
            }
            _ => false,
        }
    }

    // ───────────────────────── (EPIC-4 V3) transitive &mut self ──────────────
    //
    // `method_modifies_self` above is INTRA-method: it sees `self.x = v` and
    // `self.items.append(x)`, but it does NOT follow a call to another method
    // (`self.advance()`). So a method that mutates `self` only by delegating to
    // a mutating `self.<helper>()` was emitted `&self` → rustc E0596.
    //
    // We close that gap with a call-graph fixpoint, computed once before any
    // emission (`compute_mut_self`, run from `emit_program`) and consulted by
    // `emit_func`:
    //   1. seed `mutates[(C, m)] = method_modifies_self(m.body)` (the precise
    //      intra-method analysis — kept verbatim as the seed),
    //   2. build `self_calls[(C, m)]` = the `self.<name>()` callees in `m`,
    //   3. propagate: `mutates[k] |= any(mutates[resolve(C, c)])` to a fixpoint.
    // Keys are `(emitting_class, method_name)`: `emit_class` emits every
    // RESOLVED method (own + inherited) onto the subclass struct, so an
    // inherited body is keyed under the subclass and its self-calls resolve
    // against the SUBCLASS MRO — an inherited mutating method propagates `&mut`
    // up to a subclass caller.

    /// Collect the set of method names invoked as `self.<name>(...)` anywhere in
    /// `body`, walking the SAME statement nesting `method_modifies_self` does
    /// (if/elif/else, while, for, try body+handlers+else+finally, with) AND the
    /// expression positions a call can hide in (assignment RHS, return value,
    /// conditions, call args, …). Scope is `self.<method>()` chains ONLY: the
    /// receiver must be exactly `self` (`Expr::Attr { obj: Ident("self"), name }`).
    /// `self.child.method()` — a method on a FIELD — is intentionally NOT
    /// collected (that is nested-mutation / V2-d territory, out of scope here).
    pub(crate) fn collect_self_calls(&self, body: &[Stmt], out: &mut std::collections::HashSet<String>) {
        for stmt in body {
            match stmt {
                Stmt::Expr(e) | Stmt::Return(Some(e), _) => Self::collect_self_calls_expr(e, out),
                Stmt::Assign { value, .. } | Stmt::AugAssign { value, .. } => {
                    Self::collect_self_calls_expr(value, out)
                }
                Stmt::Unpack { value, .. } => Self::collect_self_calls_expr(value, out),
                Stmt::AttrAssign { obj, value, .. } => {
                    Self::collect_self_calls_expr(obj, out);
                    Self::collect_self_calls_expr(value, out);
                }
                Stmt::IndexAssign { obj, idx, value, .. } => {
                    Self::collect_self_calls_expr(obj, out);
                    Self::collect_self_calls_expr(idx, out);
                    Self::collect_self_calls_expr(value, out);
                }
                Stmt::If { cond, then, elifs, else_, .. } => {
                    Self::collect_self_calls_expr(cond, out);
                    self.collect_self_calls(then, out);
                    for (c, elif_body) in elifs {
                        Self::collect_self_calls_expr(c, out);
                        self.collect_self_calls(elif_body, out);
                    }
                    if let Some(else_body) = else_ {
                        self.collect_self_calls(else_body, out);
                    }
                }
                Stmt::While { cond, body, .. } => {
                    Self::collect_self_calls_expr(cond, out);
                    self.collect_self_calls(body, out);
                }
                Stmt::For { iter, body, .. } => {
                    Self::collect_self_calls_expr(iter, out);
                    self.collect_self_calls(body, out);
                }
                Stmt::Try { body, handlers, else_, finally_, .. } => {
                    self.collect_self_calls(body, out);
                    for handler in handlers {
                        self.collect_self_calls(&handler.body, out);
                    }
                    if let Some(else_body) = else_ {
                        self.collect_self_calls(else_body, out);
                    }
                    if let Some(finally_body) = finally_ {
                        self.collect_self_calls(finally_body, out);
                    }
                }
                Stmt::With { ctx_expr, body, .. } => {
                    Self::collect_self_calls_expr(ctx_expr, out);
                    self.collect_self_calls(body, out);
                }
                _ => {}
            }
        }
    }

    /// Recurse into an expression collecting `self.<name>(...)` method callees.
    /// Only a call whose callee is `self.<name>` *directly* (receiver is the bare
    /// `self` ident) is recorded; the callee subexpressions are still walked so a
    /// nested `self.a(self.b())` records both `a` and `b`.
    pub(crate) fn collect_self_calls_expr(expr: &Expr, out: &mut std::collections::HashSet<String>) {
        match expr {
            Expr::Call { callee, args, kwargs, .. } => {
                if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                    match obj.as_ref() {
                        // Direct `self.<name>(...)`.
                        Expr::Ident(n, _) if n == "self" => {
                            out.insert(name.clone());
                        }
                        // `super().<name>(...)` lowers to `self.__super_<name>()`
                        // (an alias carrying the immediate parent's body). Record
                        // it under that exact emitted name so the fixpoint can
                        // propagate &mut from a mutating inherited method up to a
                        // delegating-only override (e.g. a `__init__` that does
                        // nothing but `super().__init__()`).
                        Expr::Call { callee: sup, args: sup_args, .. }
                            if sup_args.is_empty()
                                && matches!(sup.as_ref(), Expr::Ident(s, _) if s == "super") =>
                        {
                            out.insert(format!("__super_{}", name));
                        }
                        _ => {}
                    }
                }
                Self::collect_self_calls_expr(callee, out);
                for a in args {
                    Self::collect_self_calls_expr(a, out);
                }
                for (_, v) in kwargs {
                    Self::collect_self_calls_expr(v, out);
                }
            }
            Expr::Attr { obj, .. } => Self::collect_self_calls_expr(obj, out),
            Expr::Index { obj, idx, .. } => {
                Self::collect_self_calls_expr(obj, out);
                Self::collect_self_calls_expr(idx, out);
            }
            Expr::Slice { obj, start, stop, step, .. } => {
                Self::collect_self_calls_expr(obj, out);
                for e in [start, stop, step].into_iter().flatten() {
                    Self::collect_self_calls_expr(e, out);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                Self::collect_self_calls_expr(lhs, out);
                Self::collect_self_calls_expr(rhs, out);
            }
            Expr::UnOp { expr: e, .. } => Self::collect_self_calls_expr(e, out),
            Expr::IfExp { test, body, orelse, .. } => {
                Self::collect_self_calls_expr(test, out);
                Self::collect_self_calls_expr(body, out);
                Self::collect_self_calls_expr(orelse, out);
            }
            Expr::List(elems, _) | Expr::Tuple(elems, _) | Expr::Set(elems, _) => {
                for e in elems {
                    Self::collect_self_calls_expr(e, out);
                }
            }
            Expr::Dict(pairs, _) => {
                for (k, v) in pairs {
                    Self::collect_self_calls_expr(k, out);
                    Self::collect_self_calls_expr(v, out);
                }
            }
            Expr::ListComp { elt, iter, cond, .. } | Expr::SetComp { elt, iter, cond, .. } => {
                Self::collect_self_calls_expr(elt, out);
                Self::collect_self_calls_expr(iter, out);
                if let Some(c) = cond {
                    Self::collect_self_calls_expr(c, out);
                }
            }
            Expr::DictComp { key, val, iter, cond, .. } => {
                Self::collect_self_calls_expr(key, out);
                Self::collect_self_calls_expr(val, out);
                Self::collect_self_calls_expr(iter, out);
                if let Some(c) = cond {
                    Self::collect_self_calls_expr(c, out);
                }
            }
            Expr::Lambda { body, .. } => Self::collect_self_calls_expr(body, out),
            _ => {}
        }
    }

    /// Pre-pass (run once from `emit_program`, before any emission): compute the
    /// transitive `&mut self` decision for every `(class, method)` and store it
    /// in `self.mut_self`. See the block comment above for the algorithm.
    pub(crate) fn compute_mut_self(&mut self) {
        // 1+2: seed `mutates` and build `self_calls`, keyed by (class, method),
        // over the RESOLVED method set of every class (own + inherited).
        let mut mutates: HashMap<(String, String), bool> = HashMap::new();
        let mut self_calls: HashMap<(String, String), std::collections::HashSet<String>> =
            HashMap::new();
        // `resolved[class]` = set of method names visible on the class via MRO,
        // so `resolve(class, name)` can check membership cheaply.
        let mut resolved: HashMap<String, std::collections::HashSet<String>> = HashMap::new();

        // Dunder-trait method names (these become trait impls, not inherent
        // methods, and never get a `__super_` alias — mirrors `emit_class`).
        let dunder_trait_names = DUNDER_TRAIT_NAMES;

        let class_names: Vec<String> = self.ctx.classes.keys().cloned().collect();
        for cls in &class_names {
            let methods = self.resolved_methods(cls);
            let mut names = std::collections::HashSet::new();
            for m in &methods {
                names.insert(m.name.clone());
                let key = (cls.clone(), m.name.clone());
                mutates.insert(key.clone(), self.method_modifies_self(&m.body));
                let mut calls = std::collections::HashSet::new();
                self.collect_self_calls(&m.body, &mut calls);
                self_calls.insert(key, calls);
            }

            // Seed the `__super_<name>` aliases EXACTLY as `emit_class` emits
            // them (codegen.rs ~903): one per OWN method that overrides an
            // immediate-parent method. The alias carries the PARENT's body but is
            // emitted onto THIS class's struct, so its own self-calls resolve
            // against THIS class's MRO. This lets a delegating-only override
            // (`__init__` that just calls `super().__init__()`) inherit `&mut`
            // from the mutating parent method through the fixpoint.
            if let Some(cd) = self.ctx.classes.get(cls) {
                let own_method_names: std::collections::HashSet<&str> =
                    cd.methods.iter().map(|m| m.name.as_str()).collect();
                for base in &cd.bases {
                    if let Some(base_def) = self.ctx.classes.get(base.as_str()) {
                        for m in &base_def.methods {
                            if !dunder_trait_names.contains(&m.name.as_str())
                                && own_method_names.contains(m.name.as_str())
                            {
                                let alias = format!("__super_{}", m.name);
                                names.insert(alias.clone());
                                let key = (cls.clone(), alias);
                                mutates.insert(key.clone(), self.method_modifies_self(&m.body));
                                let mut calls = std::collections::HashSet::new();
                                self.collect_self_calls(&m.body, &mut calls);
                                self_calls.insert(key, calls);
                            }
                        }
                    }
                }
            }

            resolved.insert(cls.clone(), names);
        }

        // 3: fixpoint. `mutates` is monotone (only ever flips false→true) over a
        // finite key set, so it converges; cap iterations at len+1 to defend
        // against mutual-recursion cycles (A↔B) — each pass can newly-true at
        // most one key per chain link, so len passes suffice.
        let max_iters = mutates.len() + 1;
        for _ in 0..max_iters {
            let mut changed = false;
            // Iterate over a stable key snapshot; read `mutates` for callees.
            let keys: Vec<(String, String)> = mutates.keys().cloned().collect();
            for key in &keys {
                if *mutates.get(key).unwrap_or(&false) {
                    continue; // already true — monotone, never reverts
                }
                let (cls, _method) = key;
                let mut now_true = false;
                if let Some(callees) = self_calls.get(key) {
                    for callee in callees {
                        // resolve(cls, callee): the callee is emitted onto THIS
                        // class only if it is visible via the class's MRO; key it
                        // under (cls, callee) so an inherited mutating method
                        // (also seeded under cls) propagates.
                        if resolved.get(cls).map_or(false, |s| s.contains(callee)) {
                            let ckey = (cls.clone(), callee.clone());
                            if *mutates.get(&ckey).unwrap_or(&false) {
                                now_true = true;
                                break;
                            }
                        }
                    }
                }
                if now_true {
                    mutates.insert(key.clone(), true);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        self.mut_self = mutates;
    }

    /// (EPIC-5 C2-1) Pre-pass building the closed-set polymorphism map
    /// `base -> all subclasses in the unit` (direct AND transitive). Run from
    /// `emit_program` right after `compute_mut_self`, BEFORE any emission, so the
    /// map is populated when `rust_ty` consults it. Reads only `ctx.classes`, so
    /// it is independent of module emission order.
    ///
    /// For every ordered pair of registered classes `(sub, base)` with
    /// `is_subclass(sub, base)` and `sub != base`, `sub` is registered under
    /// `base`. Reusing the audited `crate::typeck::is_subclass` (which walks
    /// `bases` edges through `ctx.classes` and terminates at builtins like
    /// `Exception`) gives transitivity for free: in a `C(B(A))` chain,
    /// `is_subclass(C, A)` holds, so `C` lands under `A` as well as under `B`.
    /// Each subclass list is sorted for deterministic, stable codegen.
    pub(crate) fn build_poly_map(&mut self) {
        let class_names: Vec<String> = self.ctx.classes.keys().cloned().collect();
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for sub in &class_names {
            for base in &class_names {
                if sub != base && crate::typeck::is_subclass(sub, base, self.ctx) {
                    map.entry(base.clone()).or_default().push(sub.clone());
                }
            }
        }
        for subs in map.values_mut() {
            subs.sort();
        }
        self.poly_map = map;
    }

    /// (EPIC-5 C2-1) True when `name` is a base class with at least one subclass
    /// in the compilation unit — i.e. it has a non-empty `poly_map` entry. C2-1
    /// only consults this (in `rust_ty`) without changing emitted text; C2-2 will
    /// branch on it to emit the companion-enum name `n__`.
    pub(crate) fn is_polymorphic_base(&self, name: &str) -> bool {
        self.poly_map.get(name).is_some_and(|subs| !subs.is_empty())
    }

    /// The `&mut self` decision for a method, consulted by `emit_func`. Uses the
    /// precomputed transitive result from `compute_mut_self` (the normal path —
    /// the pre-pass map covers every resolved class method, including the
    /// `__super_` aliases). Falls back to the intra-method `method_modifies_self`
    /// seed only for a method absent from the map (a defensive path; the
    /// `__lt_impl` helper is emitted inline and never routed through here).
    pub(crate) fn needs_mut_self(&self, class_name: &str, method_name: &str, body: &[Stmt]) -> bool {
        match self.mut_self.get(&(class_name.to_string(), method_name.to_string())) {
            Some(v) => *v,
            None => self.method_modifies_self(body),
        }
    }

}
