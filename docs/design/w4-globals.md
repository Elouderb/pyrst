# W4 — Module-Level Mutable State (G2)

**Roadmap:** stdlib-full §E/§F W4 (G2 verdict: BUILD, effort M, "a convenience
surface, not a swath of modules"). **Builds on:** W3 per-module namespacing
(`docs/design/w3-modules.md` — owner-first resolution, `emit_name`/`mangle_const`,
per-module symbol tables) and EPIC-4 value semantics
(`docs/design/value-semantics.md` — uniform deep clone-on-use, no aliasing,
`Mut[T]`→`&mut T`). **Status:** design only, no source modified. **Date:** 2026-07-06.

## Bottom line

Module-level mutable state lowers to **`thread_local!` + `Cell<T>` (Copy scalars)
/ `RefCell<T>` (everything else)**, one static per mutable global, owner-qualified
exactly like a W3 const (`__pyrst_g_<owner>__<name>`). pyrst is single-threaded,
so a `thread_local` gives non-`Sync` interior mutability at static scope with
**zero `unsafe` and zero locking** — the two properties that eliminate the
alternatives (`static mut` needs `unsafe` on every access — a hard error, E0133;
`OnceLock<Mutex<T>>` locks a mutex per read). A **read clones** out of the cell
(`.with(|c| c.borrow().clone())`), which is *exactly* pyrst's uniform value
semantics — the same `.to_string()`-on-read a `str` const already emits. Writes
`set`/replace; in-place mutations `borrow_mut().push(…)`.

The **surface** is Python's own explicit-intent marker: a module binding becomes a
mutable static iff (a) some function declares **`global NAME`** and rebinds it, or
(b) its initializer is not a scalar literal (a container literal, a constructor, an
`@extern` call — the `sys.argv`/`random` cases). A binding that is a scalar literal
**and** never rebound stays the existing immutable Rust `const` — **byte-identical,
zero regression**. The classic Python trap (assign-without-`global` creates a
function-local → `UnboundLocalError`) is **already implemented and passing**
(`detect_module_const_unbound_local`, the W0-b hole closed); W4 makes `global`-declared
names *opt out* of that trap, a one-line change to an existing filter. **`nonlocal`
is honestly deferred** (it needs shared-mutable frame capture — the `Rc<RefCell>`
aliasing EPIC-4 rules out). **Cross-module rebind `m.x = 5` is a v1 honest error**
(qualified *reads* `m.x` work for free via W3); owner-module mutation only.

Init is **eager, top-down, at `main()` entry** (a generated `__pyrst_init_globals()`),
matching CPython's import-time semantics and removing lazy-init order divergence.
The four unlocks — `sys.argv`, the module-level `random` API, a print-backed
`logging` root logger, `warnings` filters — become W4-b/c/d. Validated by 5
compiling `rustc` probes and 8 `python3` scoping probes (§G).

---

## A. What happens today — the baseline W4 must preserve

Source-anchored, with real-compiler probes (`pyrst check`/`emit`, this worktree).

| # | Behavior today | Evidence |
|---|---|---|
| Module const = scalar literal only | `NAME: T = <literal>` accepted at top level iff the value is an int/float/str/bool literal (`is_const_literal`); anything else — a container literal, a call, an unannotated assign — is an honest error | `checks.rs:1418` `is_const_literal`, `:1431` `is_module_const_decl`, `:1456` `check_top_level_other`; probe **PE** (`items: list[int] = []` → "top-level statements … not supported") |
| Const lowering | int→`const __pyrst_const_<n>: i64`; float→`… f64 = <v>f64`; bool→`… bool`; **str→`… &str` + `.to_string()` at each read** (value-semantics clone) | `analysis.rs:1651` `emit_const_decl`; probe **PD** (`const __pyrst_const_PI: f64 = 3.14f64;`, read `__pyrst_const_NAME.to_string()`) |
| Owner-qualified const (W3) | a ROOT const stays `__pyrst_const_<n>`; an imported module's const gains its owner: `__pyrst_const_<owner>__<n>` (`mangle_const`); reads resolve the owner via `bare_owner_for` / the qualifier | `mod.rs:326` `mangle_const`; `exprs.rs:2862` bare ref, `:2947` qualified `X.CONST` |
| **The assign-without-`global` trap is ALREADY CLOSED** | a module const rebound in a function is rejected with a Python-faithful message ("… makes `counter` a local for the whole function … Python raises `UnboundLocalError` … module-level mutable state is not yet supported") | `flow.rs:1668` `detect_module_const_unbound_local`; probe **PA** |
| `global`/`nonlocal` are not keywords | absent from the lexer; `global counter` is a **parse error** ("expected end of statement, found Ident") — two adjacent idents | probe **PB**; `lexer.rs`/`parser.rs` have no `global` |
| Aug-assign of a module const | `total += 1` in a function → honest "undefined variable `total`" (a different, also-safe rejection path) | probe **PC** |
| A const read inside a closure | works; the const is referenced **directly** in the closure body (`return __pyrst_const_BASE`), NOT captured by value — the `move` closure captures only true locals | probe **PF** (`let read = Rc::new(move || … return __pyrst_const_BASE)`) |
| Value semantics (EPIC-4) | every object is an independent, non-`Copy` Rust value; a read/consume **deep-clones** (uniform clone-on-use); no `Rc<RefCell>` aliasing; `Mut[T]`→`&mut T` for opt-in by-ref mutation | `value-semantics.md` §Bottom-line |
| Closed type set | `Int Float Bool Str Unit List Set Dict Tuple Option Iterator Class Func File TypeVar`; `str`=Rust `String`, `list`=`Vec`, `dict`=`HashMap`, `set`=`HashSet` | `types.rs:4` `enum Ty` |
| 44 embedded lib modules | none uses mutable module state (they can't — it's rejected); `sys.argv`/`random` module-API/`logging` are explicitly G2-deferred in their headers | `lib/sys.pyrs:5`, `lib/random.pyrs` "WHY A CLASS", `stdlib.rs` `EMBEDDED_STDLIB` |

**The load-bearing consequence:** the "assign-without-`global` → local" rule — the
single trap this whole area is famous for — is not a thing W4 must *invent*; it is
already built, tested, and emitting the right diagnostic. W4's job on the soundness
axis is narrow: let a `global`-declared name *escape* that trap and write a real
mutable static, without re-opening it for names that were never declared `global`,
and without perturbing the const path for the never-reassigned common case.

---

## B. Decision 1 — THE LOWERING

**Decision.** Emit each **mutable** module global as a crate-root
`thread_local!` holding **`Cell<T>` for a Copy scalar** (`int`→`i64`, `float`→`f64`,
`bool`) and **`RefCell<T>` for everything non-`Copy`** (`str`→`String`, `list`→`Vec`,
`dict`→`HashMap`, `set`→`HashSet`, `tuple`, a user class). The static's name is
owner-qualified exactly like a const, in a **distinct namespace**:
`__pyrst_g_<mangle_mod_id(owner)>__<name>` (root: `__pyrst_g_<name>`). Access shapes:

```
// int/float/bool  (Cell)                     // str/list/dict/set/class  (RefCell)
read   G.with(|c| c.get())                     read    G.with(|c| c.borrow().clone())   // value-semantics CLONE
rebind G.with(|c| c.set(<v>))                  rebind  G.with(|c| *c.borrow_mut() = <v>)
x+=e   G.with(|c| c.set(c.get() <op> <e>))     mutate  G.with(|c| c.borrow_mut().push(<v>))  // append/index-assign/…
```

**Init is eager and top-down.** A generated `fn __pyrst_init_globals()` runs each
mutable global's initializer **in source (import-topological) order** as the first
statement of `fn main()`, before `user_main()`. This makes the thread-local's
otherwise-lazy (first-access) initialization observably **eager at startup in
dependency order**, matching CPython's import-time top-down evaluation: a global
`B = A * 2` reads `A`'s init-time value (6), and a *later* mutation of `A` does not
retro-change `B` (py-probe 4, rust-probe 1). For the v1 initializer set (§C) — which
never reads another global — this pass is semantically a no-op, but it is cheap
(one fn, N touches), makes the eager semantics explicit, and future-proofs a v2 that
allows an initializer to reference another global.

**Rationale.**

- **Single-threaded ⇒ `thread_local` is the natural fit.** A plain `static G:
  Cell<i64>` is illegal (`Cell: !Sync`; a `static` must be `Sync`). `thread_local!`
  sidesteps `Sync` entirely — one thread, one cell — giving static-scope interior
  mutability with **no `unsafe` and no lock**. pyrst emits zero `unsafe` as a matter
  of policy; this keeps that invariant. Validated: rust-probe 1 (i64 counter bumped
  from two fns; `Vec` appended + read-cloned; a `HashMap`; a user `struct` field
  mutated + read-cloned; eager `A=3 B=6` then `A=100 B=6`) **compiles and runs**.
- **A read *clones* — which IS the value-semantics contract, already precedented.**
  `.with(|c| c.borrow().clone())` yields a fresh owned value per read, so a caller
  that holds the result and then mutates the global sees an independent snapshot
  (rust-probe 1: `snapshot=[1,2,3]` while `live=[1,2,3,4]`). This is the exact shape
  a `str` const already emits (`&str` const + `.to_string()` at each read, probe PD)
  — W4 generalizes the same clone-on-read to every mutable-global type. No new
  ownership model; it rides EPIC-4.
- **Writes are aliasing-free.** Each access is its own `.with()` with a borrow that
  ends when the closure returns; the clone-on-read copies out immediately, so a
  `borrow()` never spans a call that could `borrow_mut()` the same cell — no runtime
  aliasing panic. This is why `RefCell` (not `Rc<RefCell>`) is sufficient: the state
  is a single process-wide cell, never *aliased* across values, only *accessed*
  serially.
- **Init order matches CPython, without surprise.** Lazy first-access init would
  diverge the moment a global's initializer reads another global that is later
  mutated before first access. The eager `__pyrst_init_globals()` forces all inits
  before any user code runs, so nothing can interpose — top-down import semantics,
  reproduced exactly (rust-probe 1 (d)).
- **Emit determinism / DCE / `@crate`.** Statics + the init fn emit in the same
  topological module order as every other item (imports first, root last), each
  module's globals in source order — deterministic. `thread_local!` is `std`, so no
  Cargo/`@crate` interaction. A never-referenced mutable global is dead code, covered
  by the crate-level `#![allow(dead_code)]` like the other preludes.

**Rejected alternatives.**

- **`OnceLock<Mutex<T>>` / `Mutex<T>`** — compiles (rust-probe 3) but **locks a
  mutex on every read and write**, and a read must lock-then-clone. Pointless overhead
  for a single-threaded target, and it moves an aliasing bug from a compile error to
  a runtime deadlock/poison. Rejected as overkill.
- **`OnceLock` init + `static mut`** — `static mut` access requires `unsafe` on
  **every** read and write (rust-probe 3b: bare access is a hard error, `E0133`; and
  it is lint-denied under edition 2024's `static_mut_refs`). pyrst emits no `unsafe`;
  rejected.
- **`Rc<RefCell<T>>` module state** — reintroduces the shared-mutable *aliasing*
  model EPIC-4 explicitly disclaims, trading static safety for runtime borrow panics.
  A single global cell has no aliasing to model, so the `Rc` buys nothing. Rejected.
- **Lazy (no `__pyrst_init_globals`)** — sound *only* while initializers can't read
  another global; adopting the eager pass now costs almost nothing and removes the
  latent v2 foot-gun. Rejected in favor of eager.

---

## C. Decision 2 — THE SURFACE (which bindings become mutable statics)

**Decision.** Two paths, usage-gated exactly like the existing const-promotion
machinery (`collect_promoted_consts` gates a class field on read-without-write; this
mirrors it for module bindings):

- **CONST path (unchanged — the zero-regression common case).** A module binding
  whose initializer is a **scalar literal** (int/float/str/bool) **and** that is
  **never rebound** anywhere stays an immutable Rust `const`, byte-identical to today.
  This is precisely the current `is_module_const_decl` set, so **every existing
  golden is unaffected** (the 44 lib modules, all positive goldens).
- **MUTABLE-STATIC path (new).** A module binding becomes a `thread_local!` static
  iff **either**:
  1. **it is rebound** — some function declares `global NAME` and assigns it
     (`NAME = …`, `NAME op= …`, or a tuple-unpack target); **or**
  2. **its initializer is not a scalar literal** — a container literal (`[]`, `{}`,
     `{1,2}`, `()`), a constructor call (`Random(0)`, `Logger("root")`), or an
     `@extern` call (`sys.argv`'s `std::env::args()`). These are **not
     const-evaluable**, so they *cannot* be a Rust `const` regardless of reassignment
     (probe PE confirms `[]` is rejected today — W4-a legalizes it on this path only).

**The `global` keyword (parse + typeck).**

- **Lexer/parser:** add `global` and `nonlocal` as keywords; parse `Stmt::Global(names,
  span)` / `Stmt::Nonlocal(names, span)` (a comma list). Today `global x` is a parse
  error (probe PB), so this is purely additive.
- **Typeck:** within a function, `global NAME` marks `NAME` as the **module binding**,
  not a local. A subsequent rebind of `NAME` writes the global (and requires `NAME` to
  resolve to a module-level mutable global — else honest error "`global NAME`: no
  module-level `NAME` to bind"). **Faithful scoping is preserved:** a rebind
  *without* `global` still creates a function-local and still trips the existing
  `UnboundLocalError` trap (probe PA) — Python's exact rule (py-probe 1). The
  surgical change: `global`-declared names join `params` in the "not a shadow" set of
  `detect_module_const_unbound_local` (§F).
- **MUTATE needs no `global`; REBIND does.** Faithful to Python (py-probe 7): a
  bare `items.append(x)` / `items[i] = v` inside a function *mutates* the module
  global `items` (no local `items` exists, so the name resolves to the global) and
  needs no `global` declaration; only a *rebind* `items = […]` needs `global items`.
  The mutation lowers to `__pyrst_g_…items.with(|c| c.borrow_mut().push(…))`.

**`nonlocal` — honest defer.** `nonlocal n` rebinds an *enclosing function's* local
from an inner closure (py-probe 8). pyrst closures **clone-capture** their
environment (EPIC-4 value semantics, no aliasing), so writing back to the enclosing
frame would require shared-mutable capture — the `Rc<RefCell>` aliasing EPIC-4
rejected. Parse `nonlocal` and emit an **honest `check` error** ("`nonlocal` is not
supported: pyrst closures capture by value (EPIC-4); use a class field, a returned
value, or a module global via `global`"). `global` is *not* similarly blocked — a
global is a single process-wide cell (a `thread_local`), not an aliased stack frame,
so there is no capture to share.

**Cross-module mutation — v1 honest defer (owner-only writes).** `import m; m.x = 5`
(a rebind of another module's global) and `m.items.append(x)` (mutating another
module's global) are legal Python (py-probe 5) but a **v1 honest error** in pyrst
("cross-module mutation of `m.x` is not supported; mutate it from a function inside
`m`"). Qualified **reads** `m.x` work *for free* — W3 already resolves the owner, and
a mutable-global read is `mangle_global(owner, name).with(|c| …clone())` (the same
owner path the qualified-const read uses, `exprs.rs:2947`). Rationale: keeps the
*write* surface owner-local; all four unlocks mutate their **own** module's globals
through their own functions, so cross-module writes aren't needed for W4-b/c/d.
Document it; revisit in v2 (W3's owner maps make the write path tractable later).

**Rejected alternatives.**

- **A pyrst-specific `mut`/`@global` marker on the declaration** — redundant.
  Python *already* forces the author to write `global NAME` to rebind a module
  binding; that declaration *is* the intent marker. Inventing a second one diverges
  from Python for no gain. Rejected in favor of "`global` + rebind ⇒ mutable."
- **All module bindings become mutable statics** — would regress the zero-regression
  bar (every never-reassigned const becomes a `thread_local`, changing emitted bytes
  for all 44 lib modules and every golden) and lose the efficient compile-time `const`
  for the overwhelmingly-common read-only constant. Rejected.
- **Supporting cross-module rebind in v1** — doable via W3 owner maps but widens the
  write surface and the aliasing story with no unlock depending on it. Deferred.

---

## D. Decision 3 — THE UNLOCKS (each becomes a W4-b/c/d card)

Each is shaped here; none is "a whole module gated on globals" — they are convenience
*surfaces* over machinery that already exists (§E stdlib-full: "not a prerequisite
for ~80% of the stdlib").

### D.1 `sys.argv` (+ `sys.exit` interplay; stdin/stdout decided OUT) — W4-b

- **Shape.** `argv: list[str]` — a compiler-provided mutable global whose initializer
  is the `@extern` runtime expression `std::env::args().collect::<Vec<String>>()`
  (mutable-static path (b): a non-literal, non-const-evaluable initializer). Matches
  CPython's `sys.argv` shape exactly: `argv[0]` = the program path, `argv[1:]` = the
  real arguments (rust-probe 4). Lives in `lib/sys.pyrs`, replacing the current
  G2-deferred header note (`lib/sys.pyrs:5`).
- **Beyond W4-a it needs:** the module-binding-with-`@extern`-call-initializer form
  (mutable-static path (b)) — nothing else. `sys.exit(code)` already exists
  (`@extern std::process::exit`, `lib/sys.pyrs:169`) and composes: a program can read
  `argv`, branch, and `sys.exit(n)`.
- **Parity + the harness change (THE load-bearing detail).** The dual-run harness
  runs the pyrst binary as `./bin <args>` and CPython as `python3 -c "…" <args>`.
  Verified (rust-probe 4 + py invocation): `./bin alpha beta` → `argv[1:] =
  ['alpha','beta']`; `python3 -c CODE alpha beta` → `sys.argv = ['-c','alpha','beta']`
  → `argv[1:] = ['alpha','beta']` — **`argv[1:]` matches; `argv[0]` differs** (`-c`
  vs `./bin`). So:
  - `test_all.sh` §1 and §4c must **pass identical trailing args to both sides.** The
    per-file arg list is read from a directive comment in the golden, e.g.
    `# argv: alpha beta`, applied to *both* `./"$base" alpha beta` (currently
    `./"$base"` with no args, `test_all.sh:96`/`:330`) and the `python3 -c` line
    (`:343`, which already forwards trailing `sys.argv` in `-c` mode).
  - a `parity_sys_argv.pyrs` golden asserts **`len(sys.argv)` and `sys.argv[1:]`
    only**, never `argv[0]` (documented divergence: program name differs by
    construction). This is a small, contained harness edit — specify it in W4-b's card.
- **stdin/stdout streams — OUT of W4 scope.** `sys.stdin`/`stdout`/`stderr` as
  first-class stream *objects* need the opaque-handle `Ty` (G1, W5) — a stream is a
  stateful foreign handle, not a value. `print()` already covers stdout; reading
  stdin is a separate `input()`/`@extern` surface. Keep them G2-deferred in the header
  (honest), do not half-build them here.

### D.2 `random` module-level API over a hidden global generator — W4-c

- **Shape.** CPython's `random` module *is* exactly this pattern: a hidden module
  global `_inst = Random()` plus free functions `random()`, `randint(a,b)`,
  `seed(n)`, `choice(xs)`, … that delegate to `_inst`. pyrst already ships a
  **bit-identical `Random` class** (MT19937, `lib/random.pyrs`, 4.5/5) — W4-c adds the
  module-level convenience layer on top:
  ```
  _inst: Random = Random(0)                 # mutable-static path (b): constructor init
  def seed(n: int) -> None:   global _inst; _inst = Random(n)
  def random() -> float:      return _inst.random()      # MUTATE _inst in place → Mut[Random]
  def randint(a: int, b: int) -> int:  return randint_m(_inst, a, b)
  ```
  Drawing advances the generator, i.e. *mutates* `_inst`; the free draws already take
  `rng: Mut[Random]` (`lib/random.pyrs` "Generic draws"), so the module fns pass the
  global as `Mut`. This needs the mutable-global place to be usable as a `Mut[T]`
  argument — spell that out in W4-c (a `borrow_mut()` place threaded to `&mut`).
- **The seed question (a real divergence to document).** CPython seeds `_inst` from
  OS entropy at import; pyrst has **no entropy source** (`lib/random.pyrs` header:
  "pass a seed"). So the module-level generator is seeded to a **fixed default
  (`Random(0)`)** and the header documents: call `random.seed(n)` for a chosen stream;
  the unseeded module API is **deterministic** across runs (a documented divergence,
  not silent). Its parity golden is therefore **pyrst-only** with an explicit
  `random.seed(42)` first, asserting the same draw sequence `Random(42)` gives (which
  is already python3-verified bit-identical).
- **Beyond W4-a it needs:** only the constructor-init mutable-static (path (b)) and
  the `Mut[global]` argument. Effort **S** (the hard part — MT19937 — is done).

### D.3 `logging` root logger (print-backed) + `warnings` filters — W4-d

- **`logging` shape.** A hidden root-logger global carrying **level state**:
  ```
  _root_level: int = 30                      # WARNING; rebound by basicConfig/setLevel → mutable static (a)
  def basicConfig(level: int = 30) -> None:  global _root_level; _root_level = level
  def warning(msg: str) -> None:  if _root_level <= 30: print("WARNING:" + msg)
  def info(msg: str) -> None:     if _root_level <= 20: print("INFO:" + msg)
  # error/debug/critical analogous; DEBUG=10 INFO=20 WARNING=30 ERROR=40 CRITICAL=50
  ```
  **Honest scope (score ~3, documented):** the root logger's *level* is faithful;
  **handlers, formatters, named-logger hierarchies, and the exact default format
  string are NOT** — they need object/protocol machinery out of W4. The header states
  this plainly (like `sys.pyrs`'s honest divergence notes). Output goes to stdout via
  `print` (CPython's default handler writes stderr — note the stream divergence, or
  route to stderr via an `@extern` `eprintln` shim; decide in the card, lean stderr
  for fidelity).
- **`warnings` shape.** A module global filter-state (`_simplefilter: str = "default"`,
  rebound by `simplefilter(action)`) + `warn(msg)` that consults it and `print`s
  once/always/never. `_seen: set[str]` (a mutable-static container) backs
  `"once"`/`"default"` dedup. Faithful subset; category classes and the full
  `filterwarnings` regex stack are documented-out.
- **Beyond W4-a they need:** nothing — level/filter state is scalar/`set` globals
  rebound via `global`. Parity: `logging` is **pyrst-only** (print-backed, stream +
  format divergence documented); `warnings` likewise. Effort **M** together.

**Cross-cutting:** all four mutate **their own** module's globals through their own
functions, so **none needs cross-module mutation** — the §C v1 owner-only-write
deferral costs the unlocks nothing.

---

## E. Decision 4 — SOUNDNESS + MIGRATION (the iron rule)

**The iron rule: no silent divergence from CPython scoping.** The three classic
traps, each handled:

1. **assign-without-`global` creates a local (`UnboundLocalError`).** *Already
   enforced* (`detect_module_const_unbound_local`, probe PA; py-probe 1). W4 does not
   weaken it — it only lets a `global`-declared name escape it (§F). A rebind of a
   name that is *not* declared `global` stays a local and stays trapped. **Zero silent
   divergence.**
2. **A global read inside a closure/generator reads the LIVE value at call time**
   (py-probe 3: `r()==10`, then `g=99`, `r()==99`). pyrst's lowering gives this **for
   free**: a global read lowers to `G.with(|c| …)` *inside the closure body*, evaluated
   when the closure is *called*, not captured by value (rust-probe 2). The precedent
   is already visible today — a module const is referenced directly in the closure
   body, not captured (probe PF: `move || … __pyrst_const_BASE`). So switching a const
   to a `thread_local` static and reading it as `G.with(…)` inside the same closure
   body preserves call-time-live semantics with no special capture handling. Generators
   (lazy coroutines) read globals in their `next()` body — same call-time read.
   **Matches CPython.**
3. **Init-order divergence — eliminated, not merely documented.** The eager
   `__pyrst_init_globals()` at `main()` entry evaluates every initializer top-down
   before any user code, so `B = A*2` is 6 and a later `A=100` leaves `B=6` (py-probe
   4, rust-probe 1(d)). No lazy-init interposition is possible.

**`is_promoted_const` / zero-regression bar.** W2/W3 goldens must not move. Two
guards: (i) the **class-constant** promotion (`collect_promoted_consts` /
`is_promoted_const`, `flow.rs:614` / `types.rs:575`) is an *orthogonal* mechanism
(class *fields* read as `C.FIELD`) — W4 touches **module-level** bindings, a disjoint
namespace, so class-const promotion is untouched. (ii) the module-const path is
preserved verbatim for scalar-literal, never-rebound bindings (§C) — so a class-level
or module-level constant that W4 does *not* make mutable emits the identical Rust
`const`. A binding only leaves the const path when a `global`+rebind or a non-literal
initializer *demands* it. **The 44 lib modules use no mutable globals today**
(rejected by construction), so none regresses; the ones that *gain* mutable globals
(`sys`, `random`, `logging`, `warnings`) do so in their own W4-b/c/d cards, each
gated by its own parity golden.

**Emit determinism.** Statics + `__pyrst_init_globals()` emit in the fixed
topological module order (imports first, root last), source order within a module —
identical determinism to the const prepass (`mod.rs:1119`).

**Migration.** Purely additive at the language level: `global`/`nonlocal` were parse
errors (probe PB), container/call module initializers were rejected (probe PE), so no
existing valid program changes meaning. `PYTHON_COMPATIBILITY.md` gains a
"module-level mutable state" capability row + the documented divergences (`nonlocal`
deferred; cross-module write deferred; `random` module-API fixed-seed determinism;
`logging` handlers/format out; `sys.argv[0]` differs under the parity harness).

---

## F. The single riskiest interaction — `global` × the `UnboundLocal` trap × the const/static split

**The risk.** Three coupled mechanisms meet at one point, and getting the coupling
wrong yields *either* a re-opened silent miscompile *or* a golden regression:

- `detect_module_const_unbound_local` (`flow.rs:1668`) rejects **any** rebind of a
  module-level name inside a function as an `UnboundLocalError`-shaped local shadow
  (probe PA). This is the p09-hole-closed guarantee.
- W4 adds `global NAME`, which must make a rebind of `NAME` **legal** and route it to
  a mutable static — i.e. it must *suppress* that very rejection, but **only** for
  `global`-declared names.
- Whether `NAME` is a mutable static at all depends on whether a `global NAME`+rebind
  exists — the same signal the trap keys on. So the promotion analysis and the trap
  analysis read the same facts and must agree.

**Why it is safe — the surgical change.** The trap already excludes one set of names
from being "shadows": the function's **params** (`flow.rs:1683`,
`… && !params.contains(*c)`). `global`-declared names join that exclusion:

```
shadowed = consts.filter(|c| local_names.contains(c)
                          && !params.contains(c)
                          && !globals_declared.contains(c))   // <-- W4: the one added clause
```

A `global`-declared rebind is thus **not** a shadow → not trapped → lowered as a
mutable-static write. A rebind **without** `global` is unchanged → still a shadow →
still trapped (probe PA still fires). And a name is promoted to a mutable static
**iff** it is `global`-declared *and* rebound — computed by the same whole-program
prepass, so the promotion set and the trap's exclusion set are derived from one scan
and cannot disagree. The const/static split (§C) then routes never-`global`-rebound
scalar-literals to the untouched `const` path (zero regression) and only the
promoted names to `thread_local`. Each half is independently probed: the trap fires
today (PA), `global` parses to a no-op-today parse error (PB), the const path is
byte-identical (PD), a container initializer is rejected today (PE, so legalizing it
can't regress anything), and a closure references a global directly rather than
capturing it (PF, so call-time-live reads are free). **Mitigation:** implement the
promotion prepass and the trap-exclusion together in W4-a, with negatives asserting
(a) rebind-without-`global` still errors, (b) `global` of a non-existent module name
errors, (c) `nonlocal` errors, (d) cross-module `m.x=5` errors — so every divergence
is a *tested* rejection, never a discovery.

---

## G. Probe appendix — validated patterns

**Rust lowering (5 compiling `rustc 1.95 --edition 2021` probes; not committed,
`scratchpad/w4probes/`).**

- **rust1 (`thread_local` Cell/RefCell — the whole lowering).** i64 counter bumped
  from two fns (=4); a `RefCell<Vec>` appended (mutate) + read-**cloned** (`snapshot
  [1,2,3]` vs `live [1,2,3,4]`); a `RefCell<HashMap>` inserted; a `RefCell<Point>`
  user-struct **field mutated** + whole-struct read-cloned; a `RefCell<String>`; and
  **eager top-down init** via `init_globals()` (`A=3 B=6`, then `A=100 B=6` — no
  retro-change). **COMPILED + ran:** `counter=4 / A=3 B=6 / A=100 B=6 / snapshot=[1,2,3]
  live=[1,2,3,4] / table[k]=9 / origin=(7,2) name=pyrst`.
- **rust2 (global-in-closure call-time read).** `make()` returns `move || G.with(get)`;
  `r()`=10, `set_g(99)`, `r()`=99. **COMPILED + ran** — matches CPython py-probe 3.
- **rust3 (rejected: `OnceLock<Mutex>` + `static mut`).** Both compile but the mutex
  locks per access and the `static mut` accesses are `unsafe`. **rust3b:** a bare
  `static mut` access **fails to compile** (`E0133`, twice) — the "no `unsafe`" cost,
  made concrete.
- **rust4 (`sys.argv`).** `thread_local RefCell<Vec<String>> = env::args().collect()`,
  read-cloned; run `./bin alpha beta` → `argv[0]=./bin argv[1]=alpha argv[2]=beta`.
  **COMPILED + ran.** Cross-checked: `python3 -c CODE alpha beta` → `sys.argv =
  ['-c','alpha','beta']` — `argv[1:]` matches, `argv[0]` differs (the harness note).

**CPython scoping oracle (8 `python3 3.12` probes).** (1) assign-without-`global` →
`UnboundLocalError`; (2) with `global`, rebind mutates the module binding (=3); (3)
closure global read is live at call time (10→99); (4) init is top-down eager
(`B=A*2`=6; later `A=100` leaves `B=6`); (5) cross-module `m.x=5` is legal Python
(the v1-deferred case); (6) `x += 1` without `global` also traps; (7) **mutate
(`append`) needs no `global`; rebind (`=`) needs `global`**; (8) `nonlocal` mutates
the enclosing *frame* (the deferred case).

**pyrst-today oracle (7 `pyrst check`/`emit` probes).** PA (const rebind → the
Python-faithful `UnboundLocal` message, trap already closed); PB (`global` is a parse
error); PC (aug-assign of a const → honest reject); PD (const lowering: `const
__pyrst_const_PI: f64 = 3.14f64`, str read `.to_string()`); PE (`items: list[int] =
[]` rejected today — W4-a legalizes on the static path); PF (a const is referenced
**directly** inside a `move` closure body, not captured — the free call-time-live
read); PG (const-count sanity).

---

## H. Staged implementation plan

Four cards (W4-c may split), each independently gate-green (full `test_all.sh` green,
0-warning, emit deterministic; every module hits its declared fidelity score with a
parity golden). W4-a is the spine; W4-b/c/d are parallel-friendly on top of it.

### W4-a — Mutable-static lowering + `global`/`nonlocal` + promotion + surface rules · complex-implementer, M/L

The compiler epic; closes the p09 hole "for real" (a rebind is now buildable, not
just honestly rejected).

- **Do:** (1) lexer/parser — `global`/`nonlocal` keywords → `Stmt::Global`/`Nonlocal`.
  (2) A whole-program **promotion prepass** (mirror `collect_promoted_consts`):
  `NAME` is a mutable static iff (`global NAME` + rebound) **or** (non-scalar-literal
  initializer); store on `TyCtx` (a `mutable_globals: HashMap<ModuleId, HashSet<Name>>`
  + init exprs). (3) typeck — `global` marks module-binding scope; extend
  `detect_module_const_unbound_local`'s exclusion with `globals_declared` (§F); legalize
  container/constructor/`@extern`-call module initializers **on the static path only**;
  honest errors for `nonlocal`, `global` of a non-module name, and cross-module
  `m.x=5`/`m.x.append()`. (4) codegen — `emit_global_decl` (`thread_local!` Cell/RefCell,
  owner-qualified `__pyrst_g_…` via `mangle_mod_id`); read=`.with(get/borrow().clone())`,
  rebind=`.with(set / *borrow_mut()=)`, mutate=`.with(borrow_mut().m())`;
  `__pyrst_init_globals()` emitted + called first in `main()`; a mutable-global place
  usable as a `Mut[T]` arg. (5) negatives (§F).
- **Files:** `lexer.rs`, `parser.rs`, `ast.rs` (`Stmt::Global`/`Nonlocal`);
  `typeck/flow.rs` (`detect_module_const_unbound_local` + a `collect_mutable_globals`
  prepass), `typeck/checks.rs` (`check_top_level_other` initializer forms,
  `:1456`/`:1479`), `typeck/types.rs` (`TyCtx.mutable_globals`); `codegen/analysis.rs`
  (`emit_const_decl` sibling `emit_global_decl`, top-level backstop `:1602`),
  `codegen/mod.rs` (`emit_program` prepass `:1119`, `__pyrst_init_globals`,
  `mangle_global`), `codegen/exprs.rs` (bare/qualified global read `:2862`/`:2947`),
  `codegen/stmts.rs` (rebind/aug/mutate emission), `examples/fail_*`.
- **Risk:** highest — §F (the trap × promotion coupling). **Regression:** all 44 lib
  modules + every positive golden **byte-identical** (const path untouched); re-run
  emit-determinism. **Gate:** green; a global counter bumped from two fns builds+runs;
  the four negatives reject at `check`.

### W4-b — `sys.argv` (+ the harness argv threading) · implementer, M

- **Do:** `lib/sys.pyrs` `argv: list[str]` = an `@extern` `std::env::args().collect()`
  (static path (b)); retire the G2-deferred header note. **Harness:** `test_all.sh` §1
  + §4c read a `# argv: …` directive and pass identical trailing args to `./"$base"`
  **and** the `python3 -c` line (`:96`, `:330`, `:343`). `parity_sys_argv.pyrs` asserts
  `len(argv)` + `argv[1:]` only (not `argv[0]`; documented).
- **Files:** `lib/sys.pyrs`, `test_all.sh` (§1/§4c arg threading),
  `examples/parity_sys_argv.pyrs` + expected. **Depends:** W4-a. **Risk:** low.
  **Gate:** dual-run parity on `argv[1:]` green; `sys.exit` still composes.

### W4-c — `random` module-level API over a hidden global generator · implementer, S

- **Do:** add the module layer to `lib/random.pyrs` — `_inst: Random = Random(0)`
  (static path (b)) + `seed`/`random`/`randint`/`randrange`/`choice`/… delegating to
  `_inst` (draws pass `Mut[_inst]`); header documents fixed-default-seed determinism
  (no entropy) and the seed-first idiom. `parity_random_moduleapi.pyrs` (pyrst-only)
  seeds then asserts the `Random(seed)`-identical sequence.
- **Files:** `lib/random.pyrs`, `examples/parity_random_moduleapi.pyrs` + expected.
  **Depends:** W4-a. **Risk:** low (MT19937 is done). **Gate:** module API draws ==
  `Random(seed)` draws.

### W4-d — `logging` (root logger, print/stderr-backed) + `warnings` filters · implementer, M

- **Do:** `lib/logging.pyrs` — `_root_level` global + `basicConfig`/`setLevel` +
  `debug/info/warning/error/critical` level-gated (level constants; stderr via
  `@extern` `eprintln` for fidelity — decide in-card); `lib/warnings.pyrs` —
  `_simplefilter` + `_seen: set[str]` globals + `warn`/`simplefilter`. Headers document
  the honest subset (no handlers/formatters/named hierarchy; no category stack).
- **Files:** `lib/logging.pyrs`, `lib/warnings.pyrs`, `stdlib.rs` (register),
  `examples/parity_logging.pyrs` / `parity_warnings.pyrs` (pyrst-only) + expected,
  `PYTHON_COMPATIBILITY.md`. **Depends:** W4-a. **Risk:** low. **Gate:** level/filter
  state observably correct; divergences documented; each hits its declared score.

**Total: 4 cards (~5 if W4-c/d split).** W4-a is the funded compiler epic; W4-b/c/d
are the convenience surfaces it unlocks. Docs (`PYTHON_COMPATIBILITY.md` capability
row + divergences, this file marked done in stdlib-full §F) fold into W4-b..d rather
than a separate card, per each card's "document the divergence" gate.

---

## Relevant files

**This design:** `docs/design/w4-globals.md` (this file). **Builds on:**
`docs/design/w3-modules.md` (owner-first resolution, `emit_name`/`mangle_const`),
`docs/design/value-semantics.md` (clone-on-use, `Mut[T]`), `docs/design/stdlib-full.md`
§E/§F W4 (the G2 verdict + card sketch). **Style precedent:** `w3-modules.md`,
`docs/design/lazy-generators.md`.

**Compiler surfaces W4-a touches:** `src/lexer.rs` + `src/parser.rs` + `src/ast.rs`
(`global`/`nonlocal` → `Stmt::Global`/`Nonlocal`); `src/typeck/flow.rs`
(`detect_module_const_unbound_local` `:1668` + a `collect_mutable_globals` prepass
beside `collect_promoted_consts` `:614`); `src/typeck/checks.rs`
(`is_const_literal` `:1418`, `is_module_const_decl` `:1431`, `check_top_level_other`
`:1456`); `src/typeck/types.rs` (`TyCtx` mutable-globals map; `is_promoted_const`
`:575` untouched); `src/codegen/analysis.rs` (`emit_const_decl` `:1651` → sibling
`emit_global_decl`; top-level backstop `:1602`); `src/codegen/mod.rs` (`mangle_const`
`:326` → sibling `mangle_global`; `emit_program` const prepass `:1119` +
`__pyrst_init_globals`); `src/codegen/exprs.rs` (bare `:2862` / qualified `:2947`
global reads); `src/codegen/stmts.rs` (rebind/aug/mutate emission).

**Stdlib surfaces (W4-b/c/d):** `lib/sys.pyrs` (argv), `lib/random.pyrs` (module API),
`lib/logging.pyrs` + `lib/warnings.pyrs` (new), `src/stdlib.rs` (register),
`test_all.sh` (§1/§4c argv threading), `examples/parity_*` + `expected/`,
`PYTHON_COMPATIBILITY.md`.

**Empirical probes (scratchpad, not committed):** `scratchpad/w4probes/`
(`rust1_threadlocal.rs`, `rust2_closure.rs`, `rust3_rejected.rs`,
`rust3b_staticmut_nounsafe.rs`, `rust4_argv.rs`; `py1`–`py8`; `pa`–`pf` pyrst).
