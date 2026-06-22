# EPIC-4 Value Semantics / Ownership — Design Document

**Roadmap card:** 10d7a97b (EPIC-4). **Design card:** 2c14a254. **Status:** design only, no source modified. **Date:** 2026-06-21.

## Bottom line

pyrst gives every object **value semantics** (each class is an independent Rust value-struct, non-`Copy`), but the codegen consumes those values **inconsistently**: some sites clone, most move. The result is the two-headed bug the deep review found — *move-then-reuse* (E0382, a loud-but-wrong rejection of valid Python) and *mutation-not-visible* (silent wrong output when a callee mutates a by-value copy). The firm roadmap fix has three parts, all sequenced here:

- **V1 — Uniform clone-on-use.** Route **every** consuming site through one `emit_consuming` helper so a non-`Copy` *place* (variable, field, index) is deep-cloned when consumed. This makes "use a variable after passing it" work the way Python does, **without** aliasing (a deep copy, not an `Rc`). Correctness-complete on its own; a pure optimization (last-use move-elision) is deferred.
- **V2 — Opt-in by-reference param mode** (`Mut[T]` → `&mut T`). Lets a caller's object actually be mutated by a callee, replacing the interim loud-error backstop with a real mechanism. The backstop error becomes the on-ramp ("…or declare the parameter `Mut[T]`").
- **V3 — `&mut self` call-graph fixpoint.** Make `method_modifies_self` transitive so `self.step()` calling a mutating `self.advance()` compiles.

This is explicitly **NOT** `Rc<RefCell>` — we accept that `&mut` aliasing restrictions turn a few Python-shaped aliased-mutation programs into honest borrow-check errors, in exchange for static safety and no runtime aliasing panics. V1 and V2 also unblock **EPIC-5 subtyping C2** (the companion enums derive `Clone`, so clone-on-use and `&mut` compose).

## A. Current state (source-confirmed)

### A.1 Clone-vs-move is ad-hoc, per-site

There is **no central ownership policy.** The nearest thing is `emit_owned` (`codegen.rs:857-869`), which clones a **bare `Expr::Ident`** of an owned type and nothing else:

```rust
// codegen.rs (emit_owned) — clones only a bare owned identifier
Expr::Ident(n, _) if is_owned_ty(&ty) => format!("{}.clone()", n),
```

Everything else is hand-placed. The rvalue path (`emit_expr`) defaults to **move**:

- `Expr::Ident` → `n.clone()`? **No** — bare `n`, a move (`codegen.rs:1827`).
- `Expr::Attr` (`obj.field` read) → **no clone**, a move out of the field (`codegen.rs:3014-3049`). The *only* exception is `return self.field`, which clones via a special `should_clone` heuristic (`codegen.rs:1131-1155`).
- `Expr::Index` reads → **already self-clone**: tuple `(…).N.clone()` (`:3060`), dict `.get(&k).cloned()` (`:3068`), list double-clone `{ let __list = o.clone(); __list[i].clone() }` (`:3084`).
- `emit_place` (`codegen.rs:879-910`) is the **lvalue** builder (assignment targets + subscripted mutating-method receivers); it produces places, not clones.

### A.2 Consuming-site inventory (where ownership transfers)

| Consuming site | Current emission | Clones a non-`Copy` place? | Source |
|---|---|---|---|
| **Constructor args** (`Class(args)`) | bare `emit_expr` | ❌ move | `codegen.rs:2438-2442`, `:2473` |
| **Free-function args** | `emit_owned` | ✅ (Ident only) | `codegen.rs:2971-2977` |
| **Method-call args** | bare `emit_expr` | ❌ move | `codegen.rs:2606` |
| `list.append(x)` / `insert` | bare `emit_expr` | ❌ move | `codegen.rs:2606`→`:2954` |
| `dict[k] = v` (value) | `emit_owned` | ✅ (Ident only) | `codegen.rs:1608` |
| `set.add(x)` | `emit_owned` | ✅ (Ident only) | `codegen.rs:2869` |
| `dict.update` / `list.extend` | hardcoded `.clone()` | ✅ always | `codegen.rs:2896`, `:2926` |
| **`return <expr>`** | bare `emit_expr` (+`self.field` special-clone) | ❌ except `self.field` | `codegen.rs:1125-1155` |
| **Assignment RHS** | bare `emit_expr` | ❌ move | `codegen.rs:1163` |
| **Ternary / `IfExp`** | bare `emit_expr` (both arms) | ❌ move | `codegen.rs:3384-3389` |
| **`match` scrutinee** | bare `emit_expr` | ❌ move | `codegen.rs:1625-1627` |
| **List/Dict/Tuple/Set literal elems** | `emit_owned` | ✅ (Ident only) | `:919`, `:1820-1821`, `:1729`, `:1809` |

Two structural problems jump out: (1) the set of sites that clone vs. move is arbitrary (constructor args and method args move; free-function args clone), and (2) even the cloning sites only handle **`Expr::Ident`**, so passing `self.field` (an `Expr::Attr` place) anywhere moves it out of `&self` → **E0382**. Concrete failing shape today: `Wrapper(self.items)` where `items: list[int]` cannot compile.

### A.3 Parameter & receiver emission

`emit_func` (`codegen.rs:379-409`) emits **every** non-`self` param as by-value, always `mut`:

```rust
// codegen.rs:399-406
let _ = write!(sig, "mut {}: {}", p.name, rust_ty(&Ty::from_type_expr(&p.ty)?));
```

There is **no by-reference param emission**. The receiver is `&self` or `&mut self` (never bare `self`), chosen by `method_modifies_self` (`codegen.rs:388-397`). Call sites never thread `&`/`&mut` — free-function args clone via `emit_owned`; method args move.

### A.4 Copy-ness — three predicates, three answers

| Predicate | Location | `Copy`/owned set |
|---|---|---|
| `is_copy_type` (codegen) | `codegen.rs:49-51` | `Int, Float, Bool, Unit` are Copy |
| `is_owned_ty` (codegen) | `codegen.rs:847-850` | owned = `Str, List, Set, Dict, **Tuple**, Class` |
| `is_non_copy` (typeck) | `typeck.rs:828-830` | non-Copy = `Class, List, Set, Dict, Str` |

They disagree on **`Tuple`** (owned per codegen, not flagged by typeck), and neither codegen predicate has a rule for `Option`/`Tuple` element-wise Copy-ness. Class structs derive `Copy` **iff all fields are primitive**, else `Clone` only (`emit_class`, `codegen.rs:531-557`). This three-way split is a latent clone/no-clone drift source — the exact failure mode EPIC-3 just eliminated for type inference.

### A.5 `&mut self` inference is intra-method

`method_modifies_self` (`codegen.rs:305-377`) returns true when the body directly contains a `self`-rooted `AttrAssign`/`IndexAssign` (via `expr_roots_at_self`, `:294-300`) or a `MUTATING_METHODS` call on a `self`-rooted receiver, recursing into `if/while/for/try/with`. It **does not follow calls to other methods** — `self.step()` calling a mutating `self.advance()` leaves `step` emitted as `&self` → **E0596**. `collect_calls_from_stmt` (`typeck.rs:531-589`) walks the same AST shape but collects only **free-function** callee names (method callees are `Expr::Attr`, not collected). `MUTATING_METHODS` (`codegen.rs:19-23`, 13 names) and `PARAM_MUTATING_METHODS` (`typeck.rs:834-841`, same 13 names) are **content-equivalent duplicates** across modules.

### A.6 Backstop nested-mutation gap

The loud-error backstop (`dd1c6fa`) checks `AttrAssign`/`IndexAssign` via `root_ident(obj)` — so `param.field.sub = v` **is** caught. But the **method-call** check (`typeck.rs:2323-2347`) only fires when the receiver is literally `Expr::Ident(param)`:

```rust
// typeck.rs ~2330 — receiver must be the param ident DIRECTLY
if let Expr::Ident(param_name, _) = obj.as_ref() { /* fire */ }
```

So `param.field.append(x)` and `param[0].add(x)` **silently escape** — a class param whose collection field is mutated in place loses the mutation with no diagnostic, the same silent-wrong-output the backstop exists to prevent. The skip was a deliberate false-positive dodge (`ds.values.append`) made *before* a real remedy existed; V2 provides the remedy.

## B. The model

Value semantics + deep clone-on-use reproduces Python's *observable* behavior for the common case (you keep using a variable after passing it; the callee can't reach back into your object) **without** importing Python's shared-mutable aliasing. A deep clone never changes observable behavior under value semantics (there is no aliasing to preserve), so **uniform clone-on-use is always correctness-safe** — at worst it clones more than a last-use analysis would. By-reference mode (`&mut T`) is the *explicit opt-in* for the one case value semantics can't express: "I want the callee's mutation to persist." The trade we consciously accept (the price of no-`Rc`): `&mut`'s no-aliasing rule rejects a few Python-shaped programs (same object passed as two mutable args) as **honest borrow-check errors**, never as silent-wrong or runtime panics.

## C. Design — three phases

### V1 — Uniform clone-on-use (independent; lands first)

**V1-a — collapse the three copy-ness predicates into one.** A single `fn is_copy(ty: &Ty) -> bool` (and its complement) consumed by both modules, with a *defined* rule for the ambiguous variants: `Tuple` is Copy iff all elements are Copy; `Option<T>` is Copy iff `T` is Copy; `Str/List/Set/Dict/Class` non-Copy. Additive and mechanical, but it **may shift clone decisions for `Tuple`/`Option`**, so re-baseline goldens here in isolation before V1-c. (Same "one source of truth" move as the oracle.) `implementer`, **S**.

**V1-b — `emit_consuming(expr) -> String`.** Generalize `emit_owned` into the single ownership-decision point:

- `ty = infer_expr_ty(expr, locals, ctx)` (the oracle). If `is_copy(ty)` → emit bare (no clone).
- Else if `expr` is a **reusable place** — `Expr::Ident` or `Expr::Attr` (field read) — emit `<place>.clone()`. **This is the new capability:** `Expr::Attr` places now clone, fixing the `self.field`-into-constructor E0382.
- Else if `expr` is `Expr::Index` → emit `emit_expr(expr)` **unchanged** (index reads already self-clone; **must not double-clone** — explicit guard).
- Else (`expr` is an owned rvalue temp — call/constructor/literal/binop result) → emit bare (nothing to clone).

Mental model: **clone iff the operand is a borrowable place of non-`Copy` type; index reads are already owned; everything else is a fresh temp.** Additive helper, no site rewired yet. `complex-implementer`, **M**.

**V1-c — route every consuming site through `emit_consuming`** (the keystone, analogous to oracle E.2). Rewire the ❌-move rows of the A.2 table — constructor args, method-call args, `return`, assignment RHS, ternary arms, `match` scrutinee, `list.append`/`insert` — and migrate the existing `emit_owned` sites to the generalized helper. **Delete** the `return self.field` `should_clone` special-case (`codegen.rs:1131-1155`) — `emit_consuming` subsumes it. New goldens: construct-from-var-then-reuse-var, pass-`self.field`-to-constructor, append-a-var-then-reuse, ternary-of-vars. Suite green throughout. `complex-implementer`, **M/L** (codegen hot file — single writer).

> V1 does **not** weaken the backstop: cloning at the *call site* still hands the callee its own copy, so callee mutation of a by-value param is still lost and still correctly rejected.

### V3 — `&mut self` call-graph fixpoint (codegen-only; lands after V1)

**V3-a — hoist one shared mutating-method const.** Replace the `MUTATING_METHODS`/`PARAM_MUTATING_METHODS` duplicates with a single `pub const` (in typeck, consumed by codegen). `implementer`, **XS**.

**V3-b — transitive `&mut self`.** A codegen pre-pass (like `prescan`), result stored on `Codegen`:
1. Per class, build `self_calls[m]` = the set of `self.<method>()` callees in `m` (reuse `collect_calls_from_stmt`'s traversal shape, but collect `Expr::Attr{obj: self, name}` method callees it currently drops).
2. Seed `mutates[m] = method_modifies_self(m.body)` (the existing **intra**-method analysis).
3. Fixpoint: `mutates[m] |= any(mutates[c] for c in self_calls[m])`. Monotone boolean over a finite set → converges; cap iterations defensively.
4. `emit_func` consults precomputed `mutates` instead of calling `method_modifies_self` directly.

MRO-aware: resolve `self.<method>` through the inheritance chain (`resolved_methods`, already present from EPIC-5 inherited dunders) so an inherited mutating method propagates. `complex-implementer`, **M** (touches `emit_func` receiver decision — sequence to avoid colliding with V2's `emit_func` edits).

### V2 — Opt-in by-reference param mode (largest; lands last)

**Surface syntax:** a typing-style wrapper `Mut[T]` on the parameter annotation — `def deposit(account: Mut[Account], amt: int) -> None`. Chosen over a `ref`/`&` keyword because it **reuses the existing generic-subscript type parser** (`from_type_expr` already handles `Optional[T]`/`list[T]`) and reads Pythonically. Semantics = `&mut T` (mutation visible to caller); a read-only `&T` borrow is a *perf* optimization, deliberately out of scope.

**V2-a — parser/AST.** `Mut[T]` → a param-level `by_ref` mode flag on `ast::Param` (the underlying `Ty` stays `T`; the mode is *how* it's passed, so a flag is cleaner than a `Ty::Ref` variant). `from_type_expr` recognizes the `Mut[...]` head. `implementer`/`complex-implementer`, **M**.

**V2-b — typeck.** Carry the `by_ref` mode; at a by-ref **argument position require a place** (Ident/Attr/Index) — passing a temporary is an honest error ("by-reference parameter `account` requires a variable, not a temporary"). **Skip the backstop** for by-ref params, and change the backstop message to point at the remedy ("…or declare the parameter `Mut[Account]` to mutate in place"). `complex-implementer`, **M**.

**V2-c — codegen.** `emit_func` emits `name: &mut T` for by-ref params; call sites emit `&mut <place>` instead of `<arg>.clone()`. Body composition with V1: reads of a by-ref param that *consume* it go through `emit_consuming` → `param.clone()` (auto-deref handles `&mut`), field access/method calls work through the reference transparently. `complex-implementer`, **M/L** (codegen hot).

**V2-d — close the backstop nested gap.** Now that `Mut[T]` exists as the remedy, route the method-call backstop check through `root_ident(obj)` (like AttrAssign/IndexAssign already do) so `param.field.append(x)` **fires** with the `Mut[T]` directive. Update `PYTHON_COMPATIBILITY.md`. `implementer`, **S**.

## D. Sequencing & risks

**Order (strangler-fig, suite green at each step):** V1-a → V1-b → V1-c → V3-a → V3-b → V2-a → V2-b → V2-c → V2-d. Each step touches one hot file at a time; V1-c and V2-c are the two keystones (both rewire codegen consuming/param emission), so they are never concurrent. Baseline at design time: **163 positives / 46 negatives**, cargo test 149, 0 warnings.

Risks (all addressable):
- **Over-cloning (perf/readability)** — uniform clone is correct but can deep-copy large structures redundantly. Accepted default; **last-use move-elision is a pure optimization, deferred** (don't gate V1 on it — same discipline as the oracle's deferred side-table).
- **Double-clone on `Index`** — `emit_consuming` must pass index reads through unchanged; explicit guard + a golden that asserts a single clone.
- **Copy-predicate unification (V1-a)** may flip clone decisions for `Tuple`/`Option` → re-baseline goldens *in V1-a's isolated step*, before V1-c muddies the diff.
- **V2 aliasing rejection** — `&mut` forbids passing the same variable as two mutable args / aliasing a `&mut`. Python allows it; pyrst will surface an honest Rust borrow-check error. This is the conscious price of no-`Rc`; document it in `PYTHON_COMPATIBILITY.md` with the rewrite (return-and-reassign, or sequence the mutations).
- **V2 temporaries** — `&mut` of a temporary is illegal; the V2-b place-requirement converts it to an honest typeck error rather than a raw rustc failure.
- **V3 inheritance/convergence** — fixpoint must resolve self-calls through the MRO; convergence is trivial (monotone boolean) but cap iterations.
- **EPIC-5 C2 composition** — the subtyping companion enums (`Animal__`) derive `Clone`, so `emit_consuming` clones them uniformly and `Mut[Animal]` → `&mut Animal__` works. V1+V2 are the EPIC-4 prerequisite the subtyping doc names; nothing here blocks C2 beyond completing V1/V2.

## E. Card breakdown (dependency-ordered wave)

| # | Card | Agent | Size | Touches |
|---|---|---|---|---|
| V1-a | Unify copy-ness into one shared `is_copy`/`is_owned`; define `Tuple`/`Option` element-wise rule; re-baseline goldens | implementer | S | typeck + codegen |
| V1-b | `emit_consuming` helper (generalize `emit_owned`: Attr-place clone, Copy-elision, Index passthrough) | complex-implementer | M | codegen |
| V1-c | Route all consuming sites through `emit_consuming`; delete `return self.field` special-case; add goldens | complex-implementer | M/L | codegen (hot) |
| V3-a | Hoist one shared mutating-method `const` (dedup codegen/typeck) | implementer | XS | typeck + codegen |
| V3-b | Per-class self-method call-graph + `&mut self` fixpoint pre-pass; MRO-aware; `emit_func` consults it | complex-implementer | M | codegen |
| V2-a | `Mut[T]` param annotation → `Param.by_ref` mode (parser/AST/`from_type_expr`) | complex-implementer | M | ast + parser + resolver |
| V2-b | typeck by-ref mode: require place at by-ref args; skip backstop for by-ref; redirect backstop message | complex-implementer | M | typeck |
| V2-c | codegen `&mut T` params + `&mut <place>` call-site threading; compose with `emit_consuming` | complex-implementer | M/L | codegen (hot) |
| V2-d | Close backstop nested gap via `root_ident`; fire `param.field.append`; doc the idiom | implementer | S | typeck + docs |

Each card carries its own `code-reviewer` pass; V1-c, V2-c, V3-b additionally warrant a `verification-engineer` run (real programs from `review-programs/` that previously hit value-semantics bugs).

## Relevant files

`src/codegen.rs` — `emit_owned` (857-869), `emit_place` (879-910), `is_copy_type` (49-51), `is_owned_ty` (847-850), `emit_class` derives (531-557), `emit_func` signature (379-409, params 399-406, receiver 388-397), `method_modifies_self` (305-377), `expr_roots_at_self` (294-300), `MUTATING_METHODS` (19-23), `emit_expr` Ident (1827)/Attr (3014-3049)/Index (3051-3088), constructor args (2438-2473), free-func args (2971-2977), method args (2606), return (1125-1155), assign RHS (1163), ternary (3384-3389), match (1625-1627), collection literals (919/1729/1809/1820-1821). `src/typeck.rs` — `is_non_copy` (828-830), `PARAM_MUTATING_METHODS` (834-841), `collect_calls_from_stmt` (531-589), backstop AttrAssign (1157-1199) / IndexAssign (1201-1224) / method-call (2323-2347), `root_ident`, `infer_expr_ty`. `src/ast.rs` — `Param`, `FuncDef`, `AttrAssign`/`IndexAssign`. `src/resolver.rs` — `from_type_expr`, `TyCtx`, `FuncSig`. Precedent: `docs/design/inference-oracle.md` (shared-pure-function pattern, deferred optimization), `docs/design/class-subtyping.md` (C2 gated on this work).
