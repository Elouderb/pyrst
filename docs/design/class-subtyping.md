# EPIC-5 Class Subtyping — Design Document

**Roadmap card:** 10d7a97b (EPIC-5). **Status:** ✅ C1 + C2 COMPLETE — shipped `7a649d6..926cd7b` (see §G). **Date:** 2026-06-21 (design) / 2026-06-22 (complete).

## Bottom line

"Accept a `Derived` where a `Base` is expected" is **easy in typeck, hard in codegen**. Each pyrst
class compiles to an independent Rust value-struct (`Dog` is not `Animal`; `Vec<Animal>` cannot hold a
`Dog`), and trait objects / `dyn` / `Rc` are off the table per project philosophy. So the feature is
**not a quick fidelity win** — full support requires generating closed-set companion enums, and that
codegen work is **gated on EPIC-4 (uniform clone-on-use)**. Recommendation: a cheap **Phase C1** now
(typeck infrastructure + honest errors, no polymorphism), and the heavy **Phase C2** (enum codegen)
deferred until after EPIC-4.

## A. Current state (source-confirmed)

- `Ty::Class(String)` carries only a name — no base chain (typeck.rs:9-24).
- `types_compatible(val, declared) -> bool` (typeck.rs:677) has NO `ctx` and NO subtype awareness; two
  classes match only by `a == b`. No `is_subclass` exists anywhere.
- Base chain lives in `ClassDef.bases: Vec<String>` (ast.rs:56), reachable via `TyCtx.classes`. `TyCtx`
  is already in every `types_compatible` caller (`env.ctx`) — just not passed into the function.
- 5 call sites: check_stmt Return (965) + Assign (985), check_expr arg-check (2237) + collection-mutator
  (2325), unify_branch_types (752, gates list-literal element unification).
- MRO infra already present: `get_all_fields` (200), `get_method` (225). Single inheritance enforced
  (typeck.rs:491, rejects >1 base).
- `emit_class` (codegen.rs:512) emits each class as a standalone `#[derive(Clone,Debug,PartialEq)]`
  struct; `rust_ty(Class(n)) -> n` (codegen.rs:3483). **No Rust type relationship between base/derived.**
- Limitation already documented (PYTHON_COMPATIBILITY.md:380). EPIC-3 oracle complete; EPIC-4 not.

## B. Representation options

1. **Closed-set enum polymorphism** — for each base with subclasses, generate `enum Animal__ {
   Animal(Animal), Dog(Dog), ... }` + a method-dispatch impl; `rust_ty` emits `Animal__` for a
   polymorphic base; constructor calls in base positions wrap (`Animal__::Dog(Dog{..})`). The ONLY
   option giving full polymorphism (incl. heterogeneous `Vec<Animal>`) without `dyn`/`Rc`. Highest
   complexity: a second codegen pass + rust_ty duality + wrapping at every base-typed position.
2. **Field-embedding / upcast-by-slice** (`Dog { __base: Animal, .. }`) — **ruled out**: upcasting
   slices off the Derived state (C++-style slicing), a silent correctness hazard the "honest errors"
   principle forbids.
3. **Narrow / honest-error** — accept subtype in typeck, but every base-typed position still needs
   wrapping to compile, so there is no position a raw `Dog` struct can occupy as an `Animal`. Reduces to
   "reject honestly with a clear message" + lay infrastructure. Zero polymorphism, zero codegen risk.
4. **Generics/monomorphization** — scalar args only without `dyn`; heterogeneous collections still need
   the enum. No advantage over Option 1. Ruled out as primary.

## C. Recommendation — two phases

### C1 (do now; typeck-only, honest errors, no EPIC-4 dependency)
- **C1-A** add `is_subclass(child, ancestor, ctx) -> bool` (walks `bases`). Additive. `implementer`, S.
- **C1-B** thread `&TyCtx` into `types_compatible`; add `(Class(d), Class(b)) if is_subclass(d,b,ctx)`;
  fix `unify_elem_types`/`unify_branch_types` to return the BASE (wider) type, not first-seen.
  `complex-implementer`, M. Must keep all existing examples green (they use exact types).
- **C1-C** codegen honest-error guard: a `Class(derived)` value into a `Class(base)` slot (derived≠base)
  emits an `Error::Codegen` pointing at EPIC-5, NOT a silent rustc failure. `implementer`, S.

C1 delivers infrastructure + honest errors, no user-facing polymorphism. NOTE the awkwardness: after C1,
typeck *accepts* subtyping but codegen *rejects* it pending C2 — acceptable as an explicit, honest gate,
but only worth doing if C2 is actually on the roadmap.

### C2 (defer until AFTER EPIC-4 value semantics)
- **C2-A** `build_poly_map(ctx)` pre-pass (base→subclasses), stored on Codegen. `implementer`, M.
- **C2-B** emit companion enums + dispatch impls for polymorphic bases. `complex-implementer`, L.
- **C2-C** `rust_ty` (+ is_copy/default helpers) emit `n__` for polymorphic bases. `implementer`, M.
- **C2-D** wrap constructor calls in base-typed positions (`Animal__::Dog(..)`). `complex-implementer`, M.
- **C2-E** list-literal element wrapping, list `+` concat (`extend`), base-typed arg wrapping.
  `complex-implementer`, M.
- **C2-F** remove the C1 guard, update docs, add positive/negative goldens. `implementer`, XS.

## D. Sequencing & risks

Strangler-fig, suite green at each step (currently 158 pos / 46 neg). Key risks (all addressable):
- **rust_ty duality** — only classes WITH a subclass in the unit become polymorphic; a sub-less class is
  unchanged → no regression for non-inheritance code. animal_super.py / inheritance_test.py use `Dog`
  only in `Dog`-typed positions, so unaffected.
- **Field access on a base-typed var** (`a.breed` where `a: Animal`) must stay a typeck error — already
  true via `get_all_fields` on the declared type. Verify in C1-B.
- **Store the DECLARED type in locals** on annotated assignment (not the RHS subtype) — else `a.breed`
  would wrongly resolve. Verify in C1-B.
- **Exception classes** (`class MyErr(Exception)`) — `Exception` is builtin, not in `ctx.classes`, so
  `is_subclass` returns false → exception subtyping stays unimplemented (correct).
- **EPIC-4 interaction** — clone-on-use composes (the enum derives Clone). C2 genuinely needs EPIC-4's
  uniform clone model first.

## E. Decision this gates
Full subtyping (C2) is a large effort gated on EPIC-4 — it is NOT the lightweight fidelity item the
roadmap framing implied. The cheap, honest C1 phase can land now; C2 should follow EPIC-4. The natural
"next big rock" is therefore EPIC-4 (value semantics), which both subtyping and other work depend on.

## Relevant files
typeck.rs (types_compatible 677, unify_branch_types 751, unify_elem_types 781, TyCtx 91, get_all_fields
200, get_method 225, Ty 9, check_stmt 965/985, check_expr 2237/2325, list-+ infer 2460); codegen.rs
(emit_class 512, resolved_methods 479, Codegen struct 25, rust_ty 3483, list-+ 3183); ast.rs (ClassDef
56); PYTHON_COMPATIBILITY.md (limitation 380); docs/design/inference-oracle.md (precedent).

---

## F. C2 revalidation addendum (post-EPIC-4, 2026-06-22)

C1 landed (commit `7a649d6`: is_subclass, ctx-threaded types_compatible, unify-to-base, honest codegen
gate). Before building C2, the design was re-validated against the now-complete EPIC-4 codebase (a
read-only 8-axis source audit). **The companion-enum approach holds**, with these EPIC-4-specific
refinements the original (pre-EPIC-4) plan did not anticipate. Current anchors (post-EPIC-4 drift):
`rust_ty` codegen.rs:4149, `emit_class` :981, the 3 `check_subtype_gate` sites :1700 (return) / :1726
(annotated-assign) / :3599 (call-arg), list-literal :2288, `zeroed_default` :84, `is_copy` typeck.rs:1040,
`is_subclass` typeck.rs:321.

**Composes cleanly, no change (audited):** `is_copy` (`Ty::Class(_)` is always non-Copy — the `n__`
substitution is pure Rust-text, the `Ty` stays `Class("Animal")`); `emit_consuming` (keys on the Python
`Ty`, the enum derives `Clone` → `x.clone()` works on `Animal__`); `byref_borrow` (name-based — `&mut
Animal__` and the `&mut *x` reborrow compose).

**Refinements to the C2-A..F breakdown:**
- **C2-C (`rust_ty`):** implement `rust_ty` as a `Codegen` METHOD reading `self.poly_map` (emit `n__` for a
  polymorphic base), NOT a free fn with an added param — else every call site (emit_func param/return,
  emit_class fields, emit_stmt hoists) must change. Big blast radius; the method form is the low-friction path.
- **C2-B (companion enum + dispatch):** (1) the enum always gets `#[derive(Clone, Debug)]` — do NOT reuse
  `emit_class`'s `all_fields_copy` derive logic (a data-variant enum has no Default/Copy). (2) **the dispatch
  method's receiver must be `&mut self` if ANY variant's concrete method is `&mut self`** (query the V3
  `needs_mut_self` map per-subclass-variant, not just the base) — a direct V3 interaction. Dispatch impl per
  method in `resolved_methods(base)`: `fn m(&self|&mut self) { match self { Base__::Dog(x) => x.m(), ... } }`.
- **C2-G (NEW — the crux, was design §D's open risk):** field access on a polymorphic-base var. After C2,
  `a: Animal` is Rust `Animal__` (an enum with no fields), so `a.name` won't compile. Reading a BASE field
  polymorphically is legitimate Python (C1 already rejects *derived-only* fields like `a.breed`). So C2 must
  generate **field-accessor dispatch methods** on the enum (`fn name(&self) -> T { match self { ... =>
  x.name.clone() } }`) and lower `a.name` (on a polymorphic base) to the accessor. (Fallback if it balloons:
  an honest "read base fields via a method" error — but accessors are mechanically the same as method dispatch.)
- **C2-H (NEW):** `zeroed_default` for a polymorphic-base local must wrap the zeroed struct in the enum
  variant (`Animal__::Animal(Animal{..})`), not emit a bare struct literal. Address with C2-D.
- **C2-E:** list-literal element wrapping at the `vec![..]` site is fine; **list+list `+` concat element
  wrapping is a PRE-EXISTING gap → DEFER** and document as a §D limitation (the intermediate `+` Vec already
  holds constructed elements).
- **C2-D / C2-F:** the 3 `check_subtype_gate` sites are the correct & complete wrapping interception points;
  replace the gate error with `format!("{base}__::{derived}({inner})")`; C2-F then deletes the gate + the
  `examples/codegen_gate/` harness section (the gate fixture becomes a positive).

**Sequencing (warning-gate-aware):** an emitted-but-unused enum trips the 0-warning gate, and `rust_ty`
emitting `n__` with no enum defined won't compile — so emission + activation are coupled. Land as:
**C2-1** poly_map pre-pass + `rust_ty`-as-method that consults poly_map but still emits plain `n`
(behavior-preserving prep; de-risks the wide rust_ty refactor) → **C2-2** the atomic keystone (emit enums +
dispatch + accessors + flip rust_ty to `n__` + wrap at the 3 gate sites + list-literal + zeroed_default +
field-access lowering + remove the C1 gate + positive goldens) → **C2-3** docs (PYTHON_COMPATIBILITY.md +
this file) + extra goldens (3-level hierarchy, base-typed param/field) + the documented concat limitation.

---

## G. C1 + C2 COMPLETE (status, 2026-06-22)

**Both phases shipped. EPIC-5 class subtyping is live.** Commit range `7a649d6..926cd7b`, closed out by the
C2-3 polish card (`0a48385b`).

**Shipped (what works, with goldens):**
- **C1** (`7a649d6`): `is_subclass`, `&TyCtx`-threaded `types_compatible`, unify-to-base, and the honest C1
  codegen gate. Sibling subclasses also unify to their nearest common ancestor (`nearest_common_ancestor`
  in `unify_branch_types`), so `[Dog(), Cat()]` types as `list[Animal]`.
- **C2-1** (`918b467`): `poly_map` pre-pass + `rust_ty` as a `Codegen` method (behavior-preserving prep).
- **C2-2a / C2-2a2** (`68607d9`, `bacff0d`): companion enum + method dispatch + `__field_*` accessors +
  `Display`/`PartialEq`/`PartialOrd` forwarding, emitted as compiling dead code.
- **C2-2b keystone** (`926cd7b`): flipped `rust_ty` to `B__` for a polymorphic base; `emit_into_base_slot`
  wrapping at the return / annotated-assign / free-fn-arg sites + list-literal + `zeroed_default`; base-field
  READ lowering; removed the C1 gate + its harness section. Verified by an independent code-review +
  verification-engineer pass.
- **C2-3** (this card, `0a48385b`): probed the remaining shapes and **fixed the base-typed FIELD path** —
  the previously-missing **constructor-argument** wrapping (struct-literal, `::new()`, and kwargs paths now
  route base-typed args through `emit_into_base_slot`, keyed on the field / `__init__`-param type) **plus the
  containing-struct derive correctness** (a struct with a polymorphic-base field omits `PartialEq`/`Default`
  from its derives, since the companion enum carries `PartialEq` only when every variant defines `__eq__`
  and never carries `Default`). New goldens: `subtype_field.py` (base-typed field init + read + dispatch),
  `subtype_three_level.py` (3-level direct construct). Also a pure-cleanup of the redundant `emit_consuming`
  at the annotated-assign site, and docs (PYTHON_COMPATIBILITY.md + this file).

**Deferred (honest errors today — never a miscompile):**
- **Upcast of an intermediate polymorphic base** (`b: B = B(1); a: A = b`) → `Error::Codegen` (a
  `From<B__> for A__` up-conversion is the follow-on). Direct leaf/derived construction at any ancestor slot
  works.
- **Field-WRITE through a base var** (`a.field = x`, `a: Base`) → `Error::Codegen` (needs a mutating enum
  accessor). Read is supported.
- **`list` + `list` concatenation** — a PRE-EXISTING gap for **all** element types (Rust `Vec` has no `Add`):
  C2-3 turned the former raw-rustc leak into an honest `Error::Codegen` pointing at `.extend()`. Element-wise
  subtype wrapping is a follow-on once concat itself is implemented.
- **Dict-literal subtype values** (`dict[str, Base] = {"k": Derived()}`) → typeck rejects (no dict-value
  subtype unification / element wrapping yet). Locked in by `fail_dict_subtype_value.py`.
- **Exception subtyping** — `Exception` is a builtin, not in `ctx.classes`, so user exception hierarchies stay
  out of the companion-enum machinery (catch by exact name).
