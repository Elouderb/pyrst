# EPIC-5 Class Subtyping — Design Document

**Roadmap card:** 10d7a97b (EPIC-5). **Status:** design only, no source modified. **Date:** 2026-06-21.

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
