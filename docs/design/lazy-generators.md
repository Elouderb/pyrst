# Lazy Generators — Lowering Strategy & `Ty::Iterator` Design

**Design card:** ad018b52. **Status:** design only, no source modified; lowering **prototype-validated** on local `rustc 1.95.0`. **Date:** 2026-07-01. **Baseline:** HEAD `ad22753` (v0.1.1).

## Bottom line

pyrst generators are **eager**: a `yield`ing function collects every value into a `Vec<T>` and returns it, and `Iterator[T]` is lowered to the *same* enum variant as `list[T]` (`Ty::List`). This is faithful for small finite generators and wrong for the two things generators exist for — an **infinite** generator (`while True: yield`) collects forever and hangs, and a generator's **side effects** all run at construction instead of on demand. This doc replaces the eager machinery with **true lazy iteration** and makes `Iterator[T]` a distinct type.

The recommended lowering is **option D — an async-block coroutine driven by a ~55-line prelude executor.** A generator body becomes `async move { … }`, `yield x` becomes `co.yield_(x).await`, and a `Gen<T>` prelude struct (`Box::pin` the future + a shared `Rc<RefCell<Option<T>>>` yield slot, polled with `std::task::Waker::noop()`) exposes it as `Iterator<Item = T>`. **Rust's compiler performs the state-machine transform for us** — no CPS transform in codegen (option A's miscompile surface), no thread/`Send` bound (option B's `Rc` breakage and eager-to-first-yield divergence), no crate dependency, single-file `rustc` preserved, `Rc`-friendly, and Python-exact timing (nothing runs until the first poll). **This is validated, not assumed:** the hand-written target Rust (§C) compiles clean (incl. `-D warnings`) and its interleaved side-effect output is **byte-identical to CPython** (§C.3).

Three-stage plan, each gate = golden suite green + deterministic emit + 0 warnings:

- **V1 — Lazy core.** Introduce `Ty::Iterator(T)`; emit the prelude; lower generator bodies to the coroutine; make the *canonical* consumption idioms (`for`, comprehensions, `list()`, `sum/min/max/sorted/enumerate/zip/any/all`) work lazily; turn the non-lazy shapes (`len(g)`, `g[i]`, `g[a:b]`, `reversed(g)`, passing a generator where `list[T]` is required) into **honest `list(...)`-suggesting errors**. Ship the flagship: an **infinite-generator golden**. Reject the hard shapes with a clear message (yield-in-`try`, generator methods, `Iterator[T]` *parameters*).
- **V2 — Broaden the surface.** `Iterator[T]` as a parameter type (with `list → Iterator` adaptation); generator **methods** (capture `self` fields by clone); lazy `map`/`filter`.
- **V3 — Advanced.** yield-inside-`try` (requires a non-`catch_unwind` try lowering for generator bodies), nested generator defs, generator expressions `(x for x in …)`, explicit `next()`.

The honest-errors principle **improves** here: four of the seven rejected shapes (`len`/subscript/slice/`reversed` on a raw generator) are `TypeError` in CPython too, so being strict makes pyrst *more* Pythonic than the eager version, which silently allowed them.

---

## A. Current state (source-confirmed)

### A.1 The single conflation point

`Iterator[T]` is lowered to `Ty::List(T)` at exactly one site — `Ty::from_type_expr_scoped`, the `("Iterator", [t])` arm:

```rust
// src/typeck/types.rs:168
("Iterator", [t]) => Ty::List(Box::new(Ty::from_type_expr_scoped(t, span, type_params)?)),
```

There is **no `Ty::Iterator` variant** (`enum Ty`, `src/typeck/types.rs:4-46`). A generator's return type and a `list[T]` are the *same* enum value, so every downstream site that inspects a list also transparently accepts a generator result — the source of both the capability (everything "just works") and the bug (nothing is lazy).

### A.2 Generator recognition & signature checks (typeck)

| Concern | Location | Behaviour |
|---|---|---|
| Is this a generator? | `check_generator_signature`, `src/typeck/checks.rs:673-686` | `body_contains_yield` **and** a valid `Iterator[T]` return (`is_iterator_type_expr`, `:690-692`). A `yield` without `Iterator[T]` is an honest error. |
| Set on env (free fn / method) | `checks.rs:631` / `checks.rs:857` | `env.is_generator = check_generator_signature(...)`. |
| Missing-return exemption | `check_all_paths_return`, `checks.rs:650-664` | generators are exempt (they implicitly return the collected `Vec`). |
| `yield` detection | `body_contains_yield` / `stmt_contains_yield`, `src/typeck/flow.rs:327-348` | recurses into `if/while/for/try/with/match` — **but not into a nested `def`**. |
| `return <value>` in a generator | `flow.rs:561-574` | **already an honest error** ("a generator cannot `return` a value … use a bare `return` to stop early"). |
| bare `return` in a generator | `flow.rs:550-559` | allowed — ends collection early (StopIteration). |
| yielded-value type | `flow.rs:600-604` | element `T` is read out of `env.ret_ty` **as `Ty::List(elem)`** — a spot that must learn `Ty::Iterator(elem)`. |

### A.3 Eager codegen (`src/codegen/`, now a directory — not the single `codegen.rs` the value-semantics doc cites)

```rust
// src/codegen/items.rs:178-198  (emit_func)
let is_generator = crate::typeck::body_contains_yield(&f.body) && matches!(ret, Ty::List(_));
// …
if is_generator {
    self.line(&format!("let mut __pyrst_gen_acc: {} = Vec::new();", ret_s)); // Vec<T>
}
```

```rust
// src/codegen/stmts.rs:90-100  (Stmt::Yield)
let s = self.emit_consuming(e)?;                 // value-semantics deep-clone of a place
self.line(&format!("__pyrst_gen_acc.push({});", s));
```

`emit_func` appends `return __pyrst_gen_acc;` at fall-off (`items.rs:286-288`); a bare `return` inside the body lowers the same way (`stmts.rs:78-83`). The `in_generator` flag lives on `Codegen` (`mod.rs:100`, init `analysis.rs:5`) and is saved/restored around nested defs (`stmts.rs:763`). The reserved `__pyrst_` prefix guarantees `__pyrst_gen_acc` never collides with a user local (`ast.rs:137`, `checks.rs:250-255`).

### A.4 The `try` lowering is the yield-in-try blocker

`emit_try` runs the try body inside a **synchronous** `catch_unwind` closure:

```rust
// src/codegen/stmts.rs:866-877  (emit_try)
let flow_ty = format!("__PyrstTryFlow<{}>", self.rust_ty(&self.current_ret_ty));
self.line("let __try_result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| -> … {");
//   … try body emitted here …
self.line("__PyrstTryFlow::Normal");
```

Under option D, `yield` becomes `.await`. **`await` is not allowed inside a non-`async` closure** — proven empirically (§C.4): the probe returns `error[E0728]: `await` is only allowed inside `async` functions and blocks`. So a `yield` inside a `try:` body cannot be lowered while `try` uses `catch_unwind`. This makes yield-in-`try` a mandatory V1 honest-error (cost: **0** corpus files, §E). `with`, by contrast, lowers to a plain Drop-guard block with **no** `catch_unwind` (`stmts.rs:567-595`), so yield-in-`with` is structurally fine — prototype #5 (§C) proves a Drop guard surviving across a yield suspension.

---

## B. Candidate lowerings — recommend D, kill A and B

| | A. Hand-rolled state machine | B. Thread + rendezvous channel | **D. Async-block coroutine (recommended)** |
|---|---|---|---|
| Who does the CPS transform | **codegen** (split body at yields, locals→struct fields) | OS thread runs the body top-to-bottom | **rustc** (async lowering) |
| Miscompile surface | **huge** — a full CPS transform over pyrst's whole stmt/expr grammar; every control-flow shape (nested loops, `try`, `with`, early return) is a new state-machine hazard | small transform | tiny — body is copied almost verbatim into `async move { }`; `yield x`→`co.yield_(x).await` |
| Dependencies | none | none (std thread + channel) | none (std `Future`/`Pin`/`Waker`) |
| `Rc`-holding values (Callable fields, defaultdict) | ok | **broken** — channel needs `Send`; `Rc` is `!Send` | **ok** — single-threaded, no `Send` bound |
| Timing vs CPython | exact if implemented right | **divergent** — body runs eagerly to the first yield at *creation* (thread starts immediately) | **exact** — nothing runs until first poll |
| Infinite generator | ok | ok (but a thread per live generator) | ok, O(1), no thread |
| Single-file `rustc` build | preserved | preserved | preserved |
| Verdict | **rejected** — violates honest-errors: the transform is exactly the kind of large, silent-miscompile-prone machinery the project avoids | **rejected** — `Send` breakage + creation-time side effects are semantic divergences we can't paper over | **chosen** |

Option D delegates the one genuinely hard part — turning straight-line code with suspension points into a resumable state machine — to the Rust compiler, which already does it correctly for every control-flow shape. That is the decisive honest-errors argument: **we write a ~55-line driver and a near-verbatim body copy instead of a CPS compiler pass.**

---

## C. The validated prototype (verbatim)

Hand-written **target Rust** — what codegen emits. Compiled with `rustc --edition 2021 -O gen_proto.rs` (and re-checked under `-D warnings` → **0 warnings**) on the local `rustc 1.95.0`, then run. The `.pyrs` shape each generator models is in its comment.

### C.1 The prelude driver (emitted once per program)

```rust
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

/// A future that returns `Pending` exactly once, then `Ready`. This is the ONLY
/// source of `Pending` in a lowered generator, so `Pending` at the driver means
/// "a value was just stored in the slot".
struct YieldNow { done: bool }
impl Future for YieldNow {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.done { Poll::Ready(()) } else { self.done = true; Poll::Pending }
    }
}

/// The yielder handed to a generator body. `yield x` lowers to `co.yield_(x).await`.
struct Co<T> { slot: Rc<RefCell<Option<T>>> }
impl<T> Co<T> {
    fn yield_(&self, v: T) -> YieldNow {
        *self.slot.borrow_mut() = Some(v);
        YieldNow { done: false }
    }
}

/// A lazy generator object. Iterating it drives the coroutine one yield at a time.
struct Gen<T> {
    fut: Pin<Box<dyn Future<Output = ()>>>,
    slot: Rc<RefCell<Option<T>>>,
}
impl<T> Iterator for Gen<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        let waker = std::task::Waker::noop();          // stable since 1.85; local rustc 1.95
        let mut cx = Context::from_waker(waker);
        match self.fut.as_mut().poll(&mut cx) {
            Poll::Ready(()) => None,                    // body ran off the end / `return`ed
            Poll::Pending  => self.slot.borrow_mut().take(), // Pending ⟺ a value was yielded
        }
    }
}
```

`Gen<T>` is a **concrete, nameable** struct (a boxed `dyn Future` inside), *not* `impl Iterator`. That is deliberate: a nameable return type is required to store a generator in a struct field, pass it as an argument, and return `Iterator[T]` from a generic function — positions where `impl Iterator` is illegal. `T` need not be `Send` (single-threaded `Rc`), matching pyrst's `Rc`-holding values.

### C.2 Representative generator bodies (the codegen target)

```rust
// #1 counter — params + locals + while + if. Prints BEFORE each yield to prove
// side effects run on demand.  pyrst: def count_up(n: int) -> Iterator[int]:
fn count_up(n: i64) -> Gen<i64> {
    let slot: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
    let co = Co { slot: slot.clone() };
    let fut: Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
        let mut i: i64 = 0;
        while i < n {
            if i % 2 == 0 { println!("[body] producing {} (even)", i); }
            else          { println!("[body] producing {} (odd)", i); }
            co.yield_(i).await;                        // <- yield i
            i += 1;
        }
        println!("[body] count_up exhausted");
    });
    Gen { fut, slot }
}

// #2 infinite — consumed with break. O(1) memory, no thread.
fn naturals() -> Gen<i64> {
    let slot = Rc::new(RefCell::new(None));
    let co = Co { slot: slot.clone() };
    let fut: Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
        let mut i: i64 = 0;
        loop { co.yield_(i).await; i += 1; }
    });
    Gen { fut, slot }
}

// #3 generic element T. Driver is element-agnostic; T: Clone + 'static.
fn repeat<T: Clone + 'static>(x: T, times: i64) -> Gen<T> {
    let slot = Rc::new(RefCell::new(None));
    let co = Co { slot: slot.clone() };
    let fut: Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
        let mut k: i64 = 0;
        while k < times { co.yield_(x.clone()).await; k += 1; } // value-semantics clone/emission
    });
    Gen { fut, slot }
}

// #4 closes over an outer local (value-semantics clone into `async move`) + early return.
fn take_until_zero(data: Vec<i64>) -> Gen<i64> {
    let slot = Rc::new(RefCell::new(None));
    let co = Co { slot: slot.clone() };
    let fut: Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
        for v in data.into_iter() {
            if v == 0 { println!("[body] hit sentinel, early return"); return; } // StopIteration
            co.yield_(v).await;
        }
    });
    Gen { fut, slot }
}

// #5 drop mid-iteration: a side-effecting Drop guard live across the yield suspension.
struct Noisy(i64);
impl Drop for Noisy { fn drop(&mut self) { println!("[drop] Noisy({}) dropped", self.0); } }
fn guarded() -> Gen<i64> {
    let slot = Rc::new(RefCell::new(None));
    let co = Co { slot: slot.clone() };
    let fut: Pin<Box<dyn Future<Output = ()>>> = Box::pin(async move {
        let _guard = Noisy(99);
        let mut i: i64 = 0;
        loop { co.yield_(i).await; i += 1; }
    });
    Gen { fut, slot }
}
```

The codegen mapping is mechanical: `def g(params) -> Iterator[T]:` → `fn g(params) -> Gen<T> { <slot/co boilerplate>; let fut = Box::pin(async move { <body> }); Gen { fut, slot } }`; `yield x` → `co.yield_(<emit_consuming(x)>).await;`; bare `return` → Rust `return;`; fall-off-end → block ends. Consumption is unchanged Rust: `for x in g` works because `Gen: Iterator ⇒ IntoIterator`; `g.collect()` materializes; `g.enumerate()`/`g.sum()` compose.

### C.3 Laziness & correctness — byte-identical to CPython

Running `gen_proto` and diffing sections #1–#4 against the equivalent `python3 gen_proto.py`:

```
$ ./gen_proto | sed '/=== #5/,$d' > rust.out
$ python3 gen_proto.py > py.out
$ diff rust.out py.out ; echo EXIT=$?
EXIT=0            # identical
```

The #1 fragment (interleaved body/consumer prints) — identical under both:

```
[main] creating count_up(4) (body must NOT run yet)
[main] created; now iterating
[body] producing 0 (even)      <- body runs on next(), NOT at creation
[main] got 0
[body] producing 1 (odd)
[main] got 1
[body] producing 2 (even)
[main] got 2
[body] producing 3 (odd)
[main] got 3
[body] count_up exhausted
```

This is the empirical laziness proof: no `[body]` line appears between "creating" and "created", and each production is immediately followed by its consumption. #2 breaks out of the infinite generator after 5 (terminates, O(1)); #3 yields `String` and `i64` (element-agnostic driver); #4 stops at the sentinel and — because the captured `data` was cloned into `async move` — the caller's original list is still usable; #5:

```
[main] first two of guarded:  →  0  →  1  →  [main] dropping g5 while suspended...
[drop] Noisy(99) dropped       ← suspended coroutine dropped cleanly: locals dropped, no leak/unwind
[main] after drop
```

### C.4 yield-in-`try` disproof (the one place option D can't reach)

```
$ rustc --edition 2021 await_in_catch.rs      # `.await` inside catch_unwind(|| …)
error[E0728]: `await` is only allowed inside `async` functions and blocks
```

Confirms §A.4: with the current `catch_unwind` try lowering, yield-inside-`try` is impossible under option D. It is a V1 honest-error and a V3 candidate (needs a generator-specific, non-`catch_unwind` try lowering).

**Prototype files:** `scratchpad/gen_proto.rs`, `scratchpad/gen_proto.py`, `scratchpad/await_in_catch.rs`.

---

## D. Consumption-surface inventory — the epic's real cost

Making `Iterator[T]` a distinct `Ty::Iterator(T)` "un-hides" every site that today silently treats a generator result as a list. Each row is source-confirmed; classification is **WORKS** (an iterator flows through, lazily), **MATERIALIZE** (honest error suggesting `list(...)`; cannot be lazy), or **DEFER** (out of V1).

### D.1 Canonical consumption — WORKS lazily

| Site | Source (current `Ty::List` handling) | Note / Python parity |
|---|---|---|
| **for-loop** | typeck element typing: `flow.rs:756`, `generics.rs:1041-1050`, `generics.rs:1322-1338`, `analysis.rs:911-920`; codegen strategy `stmts.rs:450-498` (`is_iterator` **string-sniff** at `:472-474`), prescan `items.rs:1624-1639` | Canonical lazy consumption. `for x in g` = `Gen: IntoIterator`. Element type stays `T`. Codegen must consume a `Ty::Iterator` **directly** (no `.iter().cloned()`); today it's recognized by string-sniffing emitted code (`enumerate`/`zip`/`cloned`) — replace with a type-driven check. |
| **comprehension source** | `exprs.rs:726-818` (List/Set/DictComp, checker) + oracle `exprs.rs:302-365` | `[f(x) for x in g]` lazy. **Danger:** the `_ => Ty::Int` fallback (`exprs.rs:738,777,818`) will mistype an unhandled `Ty::Iterator` loop var as `int` — needs an explicit arm. |
| **`list(g)`** | codegen `exprs.rs:1267-1289`; `Ty::List(_) => return Ok(Some(a))` passthrough (dead today — a "list" is already eager) | **The materialize primitive.** Add `Ty::Iterator(_) => "{}.collect::<Vec<_>>()"`. This is the explicit escape hatch every honest-error below points at. `list(gen)` in Python = eager collection. |
| **`sum` / `min` / `max`** | check `exprs.rs:1012-1030`; oracle `exprs.rs:128-139`; codegen `.iter().sum()`/`.iter()…min()` at `exprs.rs:1101-1112,877-971` | Lazy in Python. Codegen tweak: consume the owned iterator directly (`g.sum()`, `g.min()`), dropping the `.iter()`. |
| **`sorted(g)`** | oracle `exprs.rs:163-177`; codegen `exprs.rs:973-1000` | Naturally collects internally → returns a real `list[T]` (correct & lazy-safe). Python `sorted(gen)` materializes too. |
| **`enumerate` / `zip`** | codegen `exprs.rs:784-809` — **already lazy** (`.iter().cloned().enumerate()`, no `.collect()`) | The load-bearing precedent that the pipeline already tolerates an unmaterialized iterator. Over a `Gen`: `g.enumerate()` / `g.zip(h)`. |
| **`any` / `all`** | codegen `exprs.rs:1121-1127` (`.iter().any(…)`) | Short-circuit lazy in Python. Codegen tweak: `.any()` on the owned iterator. |
| **generic unify (`Iterator[T]` binds `T`)** | `unify_typevar` `generics.rs:122-201`; `substitute_typevars` `generics.rs:207-224` | Add an `Ty::Iterator` arm so `def first(xs: Iterator[T]) -> T` binds `T`. Structural, mechanical. |
| **assignability `list[T]` → `Iterator[T]` param** | `types_compatible` `generics.rs:12-89` | Covariant, WORKS: a concrete list *is* iterable (Python duck-typing). Add the one-directional arm. (Needs a codegen adapter when it reaches a `Gen`-typed slot — see §F, V2.) |
| **codegen type emission** | `rust_ty` `exprs.rs:2338` (`Ty::List => Vec<T>`) | Add `Ty::Iterator(T) => Gen<T>`. Uniform across return/param/field/local — the reason `Gen<T>` is a concrete struct, not `impl Iterator`. |

### D.2 Non-lazy shapes — honest MATERIALIZE error (suggest `list(...)`)

| Site | Source | Python parity |
|---|---|---|
| **`len(g)`** | `len` FuncSig `types.rs:336-341`; codegen `.len()` `exprs.rs:814-821` | `len(gen)` is a **`TypeError`** in CPython (no `__len__`). Rejecting it is **more Pythonic**; the eager version silently allowed it. |
| **subscript `g[i]`** | check `exprs.rs:1601-1614`; codegen `exprs.rs:1658-1732` | `gen[i]` is a **`TypeError`** ("not subscriptable"). More Pythonic to reject. |
| **slice `g[a:b]`** | check `exprs.rs:1616-1639`; codegen slice path | `TypeError` in CPython too. More Pythonic to reject. |
| **`reversed(g)`** | codegen `exprs.rs:1146-1149` (`.reverse()` on an owned Vec) | `reversed(gen)` is a **`TypeError`** ("must be a sequence"). More Pythonic to reject; needs a materialized/`DoubleEndedIterator` source. |
| **`Iterator[T]` → `list[T]` param/var** | `types_compatible` `generics.rs:28-30,88` (falls to `_ => false`) | An iterator is not a list. Keep it `false` → honest "expected `list[T]`, found `Iterator[T]`; wrap in `list(...)`". (A generator *can* be passed where Python wants a list only after `list()`; being explicit is the pyrst-static contract.) |
| **`str(g)` / repr** | codegen `exprs.rs:823-834` (`py_repr()`) | CPython prints an **opaque** `<generator object …>`, not the contents — so pyrst must not print list contents. Reject in V1 (materialize to show contents). |
| **list binary-ops on `g`** (`g + xs`, `g == xs`) | codegen `exprs.rs:1764,1842-1843,1911` | No lazy analog; reject/require `list(...)`. |

### D.3 Defer

| Site | Source | Why defer |
|---|---|---|
| `map` / `filter` laziness | codegen `exprs.rs:1150-1158` (eager `.collect()`) | Keep eager (returns `list[T]`, still correct) in V1; make lazy in V2. |
| membership `x in g` | codegen `exprs.rs:1973-1992` (`.iter().any`) | Python consumes the generator; a codegen tweak (`g.any(...)`) works but is low-value — V1 may MATERIALIZE, V2 make lazy. |
| LSP completions on an iterator | `member_completions` (test ref `analysis.rs:2893`) | An iterator exposes no indexable members; return none. |
| LSP hover | `Display for Ty` (`types.rs:48+`) | Trivial — add an `Iterator[{}]` arm so hover renders correctly (do in V1; near-zero risk). |
| linter | `src/linter.rs` — **zero** `Ty::List` refs | No leak by construction; nothing to change. |

### D.4 Headline counts & the drift risk

**~24 consumption points across 12 categories: ~15 WORK lazily, ~7 honest MATERIALIZE errors (4 of them — `len`/subscript/slice/`reversed` — are `TypeError` in CPython, so pyrst becomes *more* Pythonic), ~2 defer.** The linter needs no change.

The *implementation* cost is the **~19 `match` arms across 8 files** that destructure `Ty::List` and must gain an `Ty::Iterator` arm in lockstep — for-loop element typing (×6: `flow.rs`, `generics.rs`×2, `analysis.rs`, `codegen/stmts.rs`, `codegen/items.rs`), comprehension source (×5: `exprs.rs`×3 + oracle), generic machinery (`unify_typevar`/`substitute_typevars`/`types_compatible`/`map_typevar_edges`, `generics.rs`), and codegen (`rust_ty`, `unify_ty` `items.rs:1491`, `codegen/analysis.rs`). Two arms are **silent-failure** hazards that must be changed deliberately, not left to a fallback: `unify_typevar` `generics.rs:160-163` (`_ => Ok(())` silently fails to bind) and `types_compatible` `generics.rs:28-30`. This duplication is the single biggest correctness risk of the epic (§G).

---

## E. Corpus & stdlib audit

Full sweep of `examples/**/*.pyrs` (403 files), `tests/` (no `.pyrs`), `lib/*.pyrs`.

**Generators defined:** 4 runnable + 3 negative tests. `lib/` defines **zero** generators (the `yield` hits in `lib/math.pyrs:22`, `lib/time.pyrs:18` are English-word comments) — **no stdlib impact**.

| File | Generator | Consumption | Migration |
|---|---|---|---|
| `examples/generators.pyrs:10` | `squares(n)` | `for` (×2), comprehension (`:23`) | none |
| `examples/combo_gen_heapq_topk.pyrs:9` | `squares_up_to(n)` | comprehension (`:42,:60`); `cubed[3]` indexes the *comprehension result* (a real list), not the generator | none |
| `examples/dead_code_raise_yield.pyrs:15` | `squares(n)` | comprehension (`:26`) | none |
| `examples/generator_gen_local.pyrs:5` | `gen_squares(n)` | `for` + comprehension | none |
| `examples/fail_generator_return_value.pyrs` | `gen()` + `return 5` | negative test (must fail) | none |
| `examples/fail_generator_yield_type.pyrs` | `gen_strs()` yields `str` | negative test | none |
| `examples/fail_yield_toplevel.pyrs` | bare `yield` | negative test | none |

**Every consumption in the entire corpus is a `for`-loop or a comprehension — both single-pass, lazy-safe.** There is no `len(g())`, no `g()[i]`/slice, no raw generator assigned to a `list[T]` slot, no `.append`/`sorted`/`reversed` on a raw generator, no re-iteration of a stored handle. **Consumption migrations: 0.**

**One definition-side policy item — `examples/iter_no_yield_return.pyrs`.** It declares `-> Iterator[int]` on `empty()` (`:6`) and `small(c)` (`:10`) that contain **no `yield`** and just `return [...]`/`return []`, relying on `Iterator[T] ≡ list[T]`. Under a distinct `Ty::Iterator`, a list-returning function claiming to return an iterator is confusing. **Recommendation:** require `yield` for an `Iterator[T]` return — an honest error otherwise ("declared `Iterator[T]` but contains no `yield`; return `list[T]`, or add `yield`"). This is the *only* corpus migration: change those two annotations to `-> list[int]` (a 2-line edit), or reclassify the file as a `fail_` negative test.

**Total migration: 0 consumption + 1 definition-side annotation file.**

---

## F. Semantics decisions

| Question | Decision | Rationale (source / prototype) |
|---|---|---|
| `return <value>` in a generator | **Already an honest error** — keep | `flow.rs:561-574`. Python raises at compile-ish time too; bare `return` stops iteration (prototype #4). |
| bare `return` (stop iteration) | **WORKS** | Rust `return;` completes the future → `Poll::Ready` → `None` (prototype #4). |
| yield inside `try:`/`except`/`finally` | **V1 honest-error; V3 feature** | `await` inside `catch_unwind` closure = E0728 (§C.4). Cost = 0 corpus files (§E). V3 needs a non-`catch_unwind` try lowering for generator bodies. |
| yield inside `with:` | **WORKS** (no special handling) | `with` = Drop-guard block, no `catch_unwind` (`stmts.rs:567-595`); a guard survives the suspension (prototype #5). |
| yield in a nested `def` | **Already rejected** — keep | typeck forbids it (codegen comment `stmts.rs:759`); `body_contains_yield` does not recurse into nested defs (`flow.rs:331-348`). Nested *generator* defs = V3. |
| generator **methods** | **V1 honest-error; V2 feature** | `check_one_method` already sets `is_generator` (`checks.rs:857`), but the returned `Gen<T>` outlives the `&self` borrow, so the body must capture `self`'s needed fields **by clone** into `async move` (value semantics makes this sound). 0 corpus methods → defer; reject cleanly in V1. |
| generic generators | **WORKS** with an added `'static` bound | Prototype #3: `repeat<T: Clone + 'static>`. pyrst already emits `T: Clone`; the boxed `dyn Future` adds `T: 'static` (all pyrst value types are owned/`'static`). |
| `Iterator[T]` as a **parameter** type | **V1 honest-error; V2 feature** | Needs `list → Iterator` call-site adaptation (a `Vec` is `Vec<T>`, the param is `Gen<T>` — a real coercion). Sidestep in V1 (use `list[T]` params); 0 corpus uses. |
| explicit `next(g)` | **Defer** | Not exposed today; no corpus use. |
| generator expressions `(x for x in …)` | **Defer** | Parser support absent; V3. |
| drop mid-iteration | **WORKS** | Dropping a suspended pinned future drops its locals; no leak/unwind (prototype #5). |
| `Iterator[T]` return with **no** `yield` | **Require `yield`** (honest error) | §E; migrate `iter_no_yield_return.pyrs`. Removes the last vestige of the list/iterator conflation. |

---

## G. Staging plan

**Strangler-fig, golden suite green + deterministic emit + 0 warnings at every step.** Baseline golden suite: **279** at design time.

### V1 — Lazy core (the whole point of the epic)

Introduce `Ty::Iterator(Box<Ty>)`; lower `Iterator[T]` → `Ty::Iterator(T)` at `types.rs:168` (and teach the yielded-value check `flow.rs:602` to read `Ty::Iterator(elem)`); add the `Display` arm. Emit the §C.1 prelude once per program (guarded like other pyrst preludes). Rewire `emit_func`/`Stmt::Yield`/`Stmt::Return` from the `__pyrst_gen_acc` Vec to the coroutine (`Gen<T>` return, `async move` body, `co.yield_(…).await`); `rust_ty` `Ty::Iterator(T) => Gen<T>`. Make the D.1 idioms consume a `Ty::Iterator` **directly** (type-driven, retiring the for-loop string-sniff for iterators; add the `list()`→`.collect()` arm). Turn the D.2 shapes into honest `list(...)`-suggesting errors, and reject yield-in-`try`, generator methods, and `Iterator[T]` params with clear messages. Require `yield` for `Iterator[T]` returns and migrate `iter_no_yield_return.pyrs`. **New goldens:** the **flagship infinite-generator** (`naturals()` consumed with `break` — previously hung); a laziness-ordering golden (side-effect interleave vs a `# expected:` block); `list(gen)` materialization; `sum`/`enumerate` over a generator; and negatives for each rejected shape. **Gate additions:** the infinite-generator golden must terminate; the ~19 `match` arms (§D.4) all carry an `Ty::Iterator` arm (grep-audited); the two silent-fail arms are changed deliberately.

### V2 — Broaden the surface

`Iterator[T]` parameters (call-site `list → Gen` adapter, e.g. a prelude `Gen::from_vec`); generator **methods** (capture `self` fields by clone into `async move`); lazy `map`/`filter` (return `Gen` instead of collecting); optionally lazy membership `x in g`. Gate as V1 + new goldens for each.

### V3 — Advanced

yield-inside-`try` (a generator-specific, non-`catch_unwind` try lowering — a `Result`-threading transform that composes with `.await`); nested generator defs; generator expressions; explicit `next()`. Highest risk; sequence last.

### Card breakdown (dependency-ordered)

| # | Card | Agent | Size | Touches |
|---|---|---|---|---|
| V1-a | Add `Ty::Iterator(T)` variant + `Display`; lower `Iterator[T]`→`Ty::Iterator` (`types.rs:168`, yield check `flow.rs:602`); add the arm to all ~19 destructure sites (compile-only, no behavior yet) | complex-implementer | M | typeck + analysis + codegen |
| V1-b | Emit the prelude; rewire `emit_func`/`Stmt::Yield`/`Stmt::Return` to the coroutine; `rust_ty`→`Gen<T>`; for-loop & comprehension consume `Ty::Iterator` directly | complex-implementer | M/L | codegen (hot) |
| V1-c | `list()`→`.collect()`; `sum/min/max/any/all/enumerate/zip/sorted` iterator-direct codegen | implementer | M | codegen |
| V1-d | Honest MATERIALIZE errors (`len`/subscript/slice/`reversed`/`str`/list-binops/`Iterator→list`); reject yield-in-`try`, generator methods, `Iterator[T]` params; require `yield` for `Iterator[T]` return | complex-implementer | M | typeck |
| V1-e | Goldens: flagship infinite-generator, laziness-ordering, materialize, negatives; migrate `iter_no_yield_return.pyrs`; update `PYTHON_COMPATIBILITY.md` | test-engineer | S/M | examples + docs |
| V2-a | `Iterator[T]` params + `list→Gen` adapter | complex-implementer | M | typeck + codegen |
| V2-b | Generator methods (self-field clone capture) | complex-implementer | M | typeck + codegen |
| V2-c | Lazy `map`/`filter` (+ membership) | implementer | S/M | codegen |
| V3-a | Non-`catch_unwind` try lowering for generator bodies → yield-in-`try` | complex-implementer | L | codegen |
| V3-b | Nested generator defs; generator expressions; `next()` | complex-implementer | L | parser + typeck + codegen |

Each card carries a `code-reviewer` pass; V1-b/V1-d and V3-a additionally warrant a `verification-engineer` run against real infinite/side-effecting programs.

---

## H. Open questions & recommendations

1. **Materialization ergonomics.** Every D.2 error says "wrap in `list(...)`". Is `list(gen)` enough, or do we also want `tuple(gen)`/set forms in V1? **Recommend:** `list(...)` only in V1 (it's the universal escape); others follow the existing builtin story, no new work.
2. **`list[T]` → `Iterator[T]` param coercion (V2).** A `Vec<T>` argument to a `Gen<T>` param needs an adapter. Options: (a) a prelude `Gen::from_vec(Vec<T>) -> Gen<T>` inserted at the call site, or (b) emit such params as `impl IntoIterator<Item=T>` (loses the nameable-type property for fields). **Recommend (a)** — keeps `Gen<T>` uniform and nameable; the adapter is ~5 lines of prelude.
3. **`Waker::noop()` provenance.** Stable since Rust 1.85; local toolchain is 1.95, and the prototype compiles. **Recommend:** document a minimum-rustc of 1.85 in the build notes; no fallback needed for this repo.
4. **`str(gen)` / repr.** CPython prints an opaque `<generator object>`. **Recommend:** V1 rejects `str(gen)` (materialize to show contents); a faithful opaque repr is not worth the surface.
5. **The ~19-arm drift risk.** These duplicated `Ty::List` destructures already risk drift *today*. **Recommend:** V1-a lands the `Ty::Iterator` arm at *all* of them behind a grep-audited checklist (and consider, as a separate cleanup, a shared `iterable_element_ty(&Ty) -> Option<Ty>` helper so future variants touch one site — analogous to the value-semantics doc's "one source of truth" move).

## Relevant files

**typeck:** `src/typeck/types.rs` — `enum Ty` (`4-46`), `Iterator[T]` lowering (`168`), `len`/builtin FuncSigs (`329-410`), `Display for Ty` (`48+`). `src/typeck/checks.rs` — `check_generator_signature` (`673-692`), `is_iterator_type_expr` (`690-692`), env wiring (`631`, `857`), missing-return exemption (`650-664`). `src/typeck/flow.rs` — `body_contains_yield` (`327-348`), return/yield rules (`550-604`), for-loop element typing (`756`). `src/typeck/exprs.rs` — comprehension sources (`302-365`, `726-818`), index/slice (`385-406`, `1601-1639`), `sum`/`min`/`max`/`enumerate`/`zip` (`128-177`, `1012-1121`), membership (`1653-1707`). `src/typeck/generics.rs` — `types_compatible` (`12-89`), `unify_typevar` (`122-201`), `substitute_typevars` (`207-224`), `map_typevar_edges` (`1147-1172`), bound inference (`1041-1050`, `1322-1338`, `1404+`). **codegen:** `src/codegen/items.rs` — `emit_func` generator path (`178-198`, `286-288`), `prescan_types` For (`1624-1639`), `unify_ty` (`1491`). `src/codegen/stmts.rs` — `Stmt::Yield` (`90-100`), `Stmt::Return` (`68-88`), for-loop strategy (`450-498`, `is_iterator` `472-474`), `Stmt::With` (`567-595`), `emit_try` (`827+`, `catch_unwind` `866-877`), nested def (`755-815`). `src/codegen/exprs.rs` — `rust_ty` (`2338`), `list()` (`1267-1289`), `len` (`814-821`), index/slice (`1658-1734+`), `sum`/`min`/`max`/`sorted`/`reversed`/`any`/`all`/`enumerate`/`zip`/`map`/`filter` (`784-1158`), membership (`1973-1992`), `str`/repr (`823-834`). `src/codegen/mod.rs` — `in_generator` flag (`100`), try-flow docs (`90-116`). `src/codegen/analysis.rs` — struct init (`5`). **other:** `src/analysis.rs` — `reconstruct_locals` For (`911-920`), completions (`2893`). `src/linter.rs` — no `Ty::List` refs (no change). **corpus:** `examples/generators.pyrs`, `examples/combo_gen_heapq_topk.pyrs`, `examples/dead_code_raise_yield.pyrs`, `examples/generator_gen_local.pyrs`, `examples/iter_no_yield_return.pyrs` (migrate), `examples/fail_generator_*.pyrs`. **precedent:** `docs/design/value-semantics.md` (staged strangler-fig, one-source-of-truth), `docs/design/inference-oracle.md`.
