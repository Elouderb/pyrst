# Unified Inference Oracle — Design Document

**Roadmap card:** 10d7a97b / EPIC-3
**Status:** Design approved-pending. No source modified by the design pass.
**Date:** 2026-06-21

## Problem

There are (effectively) four parallel type-inference implementations, and their
drift is the documented root cause of the whole "typeck accepts X but codegen
miscompiles/can't emit X" bug class we've been fixing one at a time:

1. **typeck `check_expr`** (`typeck.rs:1327`) — `fn check_expr(e, env: &mut FuncEnv) -> Result<Ty>`.
   Side-effecting body checker; produces type errors; inheritance-aware
   (`get_all_fields` for Attr); Dict folds all pairs; Str index → Str; Pow → Float.
2. **codegen `type_of_expr`** (`codegen.rs:264`) — `fn type_of_expr(&self, e) -> Ty`.
   Pure, never errors, falls to `Ty::Unknown`. Attr reads `c.fields` directly
   (misses inherited); Index has no Str arm; Dict uses FIRST pair only; Pow is
   int-biased; abs/sum return the *correct* arg/elem type.
3. **prescan field-discovery** `infer_expr_type` (`typeck.rs:371`) → returns `TypeExpr`,
   a structural heuristic with hardcoded guesses; used only by `extract_init_fields`.
4. **codegen `prescan_types`** (`codegen.rs:1274`) — forward-scans a function body to
   populate `self.locals` before emit; calls `type_of_expr` internally.

`Ty` (`typeck.rs:7-22`) is the canonical type. `TyCtx` is built once by the resolver
and passed immutably to both typeck and codegen (`driver.rs:141-146`).

## Known divergences (source-confirmed)

| # | Expression | typeck | codegen | Correct |
|---|-----------|--------|---------|---------|
| D1 | `s[i]`, `s: str` | `Str` | `Unknown` (no Str arm) | typeck |
| D3 | `abs(x: float)` | `Int` (FuncSig hardcoded) | `Float` (arg type) | codegen |
| D4 | `sum([1.0,2.0])` | `Int` (FuncSig hardcoded) | `Float` (elem type) | codegen |
| D5 | `a ** b` | `Float` | `Int` if both Int (int-biased arm) | typeck (Python ** → float) |
| D6 | `{a:1, b:2.0}` | fold all pairs → `Dict(Str,Float)` | first pair → `Dict(Str,Int)` | typeck |
| D7 | `obj.inherited_field` | `get_all_fields()` finds it | `c.fields` misses it → `Unknown` | typeck |
| D2/D8/D9/D10 | abs(int)/method call/unknown-iter/`/` | — | — | already agree |

Most dangerous: D3, D4, D5, D6, D7.

`builtin_method_ret` (`typeck.rs`, pub) is already shared (codegen calls it at
`codegen.rs:412`) — the established cross-module pattern the oracle extends.

## Target design

One **pure** function, single source of truth, living next to `builtin_method_ret`:

```rust
/// Infer the type of `expr` from `locals` + `ctx`, no side effects.
/// Returns Ty::Unknown on any ambiguity (preserves the types_compatible escape hatch).
pub fn infer_expr_ty(expr: &Expr, locals: &HashMap<String, Ty>, ctx: &TyCtx) -> Ty
```

- Both callers already hold what it needs: typeck `env.locals`/`env.ctx`, codegen
  `self.locals`/`self.ctx` (identical types).
- `check_expr` KEEPS its `Result<Ty>` signature, error production, and `FuncEnv`
  mutation — it delegates only the pure type-derivation portion to `infer_expr_ty`.
- codegen `type_of_expr` becomes a **one-line wrapper** calling `infer_expr_ty`
  (strangler-fig; zero caller churn; step-by-step rollback).
- `infer_expr_ty` bakes in the *correct* side of every divergence (D1,D3,D4,D5,D6,D7).

**Side-table deferred (deliberately).** A keyed expr→Ty table needs stable expr IDs;
`Expr` carries only `Span` (no unique id; `Span::DUMMY` collides). Adding `u32 id` to
every variant is rewrite-scale (breaks every match arm in lexer/parser/typeck/codegen/
formatter/linter/resolver). The shared pure function achieves the same semantic goal
(one answer per expression) without that cost. Revisit only if profiling shows repeated
inference is a bottleneck.

**Also:** rename `infer_expr_type` → `guess_field_type` (it's a heuristic, not inference);
delete the redundant `extract_init_fields` call in codegen (`codegen.rs:634-635` — the
resolver already populated the fields in TyCtx); correct `abs`/`sum` FuncSig `ret` to
`Ty::Unknown` (the precise case is handled by `infer_expr_ty`'s Call arm).

## Incremental sequence (strangler-fig; suite green at EVERY step)

Prereqs (DONE): 746b30f7 arithmetic correctness, 88bd3ce7 golden harness.

- **E.1** add `infer_expr_ty` to typeck.rs (additive, dead code) — `complex-implementer`, M
- **E.2** replace codegen `type_of_expr` body with the wrapper — `implementer`, S
  *(semantically significant: tests relying on codegen's WRONG prior answer for D1/D5/D6/D7
  will flip — those are CORRECT FINDINGS; update goldens only where codegen was wrong.)*
- **E.3** delete duplicate `extract_init_fields` call in codegen — `implementer`, XS
- **E.4** rename `infer_expr_type` → `guess_field_type` — `implementer`, XS
- **E.5** correct abs/sum FuncSig `ret` → `Ty::Unknown` — `implementer`, XS
- **E.6** add golden fixtures for D1/D3/D4/D5/D6/D7 — `test-engineer`, S

Order: E.1 → E.2 → {E.3, E.4, E.5 parallelizable} → E.6. Each step touches ≤1 file and
is independently revertible; E.1's function is harmless dead code until E.2 wires it.

## Risk analysis

- **`Ty::Unknown` escape hatch** (`types_compatible` `typeck.rs:671-688`, `(Unknown,_)=>true`)
  must stay. The oracle returns `Unknown` (never errors) on ambiguity, same contract as
  `type_of_expr`. Risk: where `infer_expr_ty` now returns a *concrete* type that
  `type_of_expr` left Unknown, a previously-invisible type mismatch may surface — that's
  desirable, but categorize each failing test.
- **Blast radius:** typeck additive (new fn + rename + 2 table edits, no signature changes);
  codegen loses ~284 LOC body + 2 lines; resolver/ast/driver untouched. `infer_comp_elt_type_with_var`
  and `prescan_types` keep working (they call the wrapper → oracle).
- **Coverage gaps (no current example):** abs(float), sum(float-list), str index, inherited
  field access, mixed-value dict — E.6 closes these.
- **D5/Pow** coordinates with the already-landed arithmetic fix (emission level) — composes.

## Recommendation

Proceed E.1→E.6. E.1 is the hard part; E.2 is the keystone (eliminates the drift surface);
E.3–E.6 are mechanical. Do NOT attempt the side-table in this pass. The pattern throughout:
don't impose typeck's answers on codegen — establish which answer is *correct* and emit it
from one place.

## Relevant files
- `src/typeck.rs` — check_expr (1327), infer_expr_type (371), extract_init_fields (261),
  builtin_method_ret, types_compatible (671), TyCtx::new builtins (89-258), Ty (7-22)
- `src/codegen.rs` — type_of_expr (264), prescan_types (1274), emit_top_stmt (623), Codegen (24)
- `src/resolver.rs` — extract_init_fields call (132), merge_ctx_from_module (110)
- `src/driver.rs` — pipeline resolve → check_bodies → emit_program (141-146)
- `src/ast.rs` — Expr (144), Span as the only identity mechanism
