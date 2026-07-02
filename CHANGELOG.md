# Changelog

All notable changes to pyrst are documented here. This project adheres to [Semantic Versioning](https://semver.org).

## [0.1.2] — 2026-07-02

Lazy generators and the silent-miscompile purge. Every change was independently code-reviewed with adversarial probing; runtime semantics were verified against CPython (python3) throughout. As of this release there are **no known cases where an accepted program produces wrong output**.

### Lazy generators (the last major semantic gap)

- **Generators are lazy.** A generator body compiles to an async-coroutine (Rust's compiler performs the state-machine transform; a ~60-line dependency-free prelude drives it as an `Iterator`). The body runs on demand — nothing executes until the first element is requested, side-effect ordering is byte-identical to CPython, and **infinite generators** (`while True: yield ...`) are safe to consume with `break` in O(1) memory. Previously a generator eagerly collected every value into a list, and an infinite generator hung forever.
- `Iterator[T]` is a real, distinct type (no longer an alias of `list[T]`). Generator **variables** advance in place: a drained generator yields nothing forever (Python-exact reuse semantics), nested loops over two generator variables work, and `sum(g)` twice gives the total then 0.
- Iterator-aware builtins: `list`/`sum`/`min`/`max`/`any`/`all`/`enumerate`/`zip`/`sorted` (+`set`) consume generators directly.
- Everything non-lazy is an **honest `check`-time error** with a materialize-with-`list(...)` suggestion: `len`/subscript/slice/`reversed`/`str`/binops/membership on a generator, generator↔`list[T]` passing, `Iterator[T]` parameters, generator methods, `yield` inside `try` (all deferred features are clearly rejected, never miscompiled). Four of those rejections are `TypeError` in CPython too.
- Documented divergence: a generator closing over locals captures a value-semantics **snapshot at construction** (CPython sees later mutations) — stated in the golden and the docs.

### Correctness — the silent-miscompile purge

- **Branch-divergent locals**: a bare local assigned incompatible types across sibling branches (`if`/`elif`/`else`, `try`/`except`, `match` arms, tuple unpacks — including assigns nested one block deep), or conditionally reassigned to a conflicting type and read after the block (liveness-checked), silently dropped one path's value. All shapes now reject at `check` with a what-plus-fix message. Same-type match-arm assignment read after the `match` now *works* (was a build error).
- **Tuple-unpack reassignment in nested blocks**: `if flag: a, b = b, a` silently didn't swap, and a tuple-unpack Euclidean GCD (`while b != 0: a, b = b, a % b`) hung forever. Unpacking now distinguishes declare vs reassign (existing bindings get a real tuple assignment; mixed unpacks evaluate the right-hand side fully first — swap-safe).
- **`sorted(key=..., reverse=True)`** silently ignored `reverse` — fixed with CPython's exact reversed-stable-sort semantics (equal-key elements keep their original order).
- **Comprehensions over `zip(...)`/`enumerate(...)`** failed to build; they now work over lists and generators, with filters, in list/set/dict comprehensions.
- **`with` over a user class** silently skipped `__enter__`/`__exit__` — now an honest error (`with open(...)` unchanged); the full context-manager protocol is tracked, gated on real exception objects.
- Codegen scope hygiene: shadow decisions no longer leak across block boundaries (an abandoned intermediate retype can't poison later assignments), and the bare sequential retype idiom (`x = [1]` … `x = "s"`) is golden-covered.

### Quality

- `./test_all.sh`: 301/301 positive examples (+22), 136/136 negative fixtures (+32) at both gates; 524 `cargo test` cases; 0 compiler warnings; 5× full-corpus emit byte-stable.

## [0.1.1] — 2026-07-02

Quality release: performance, correctness, and output-quality work on the compiler back end. Every change was independently code-reviewed; the slice work was verified against CPython on a 5,744-case oracle (0 mismatches).

### Performance

- **List index/slice reads no longer clone the container.** Every read went through a deep clone of the whole list (indexed loops were O(n²)); reads now borrow the base through generic prelude helpers and clone only the element, with a conservative clone fallback whenever borrowing isn't provably safe. A 50k-element index-sum dropped from 1.52 s to under 10 ms. Stepped string slices got the same treatment.

### Correctness

- **Builtin runtime errors are now catchable by their Python exception type.** `list.pop()` on empty → `IndexError`; missing dict key (including mutable/nested access) → `KeyError`; `list.remove`/`.index`, `str.index`/`.rindex`, negative integer `**=` exponent, zero slice step → `ValueError`; file I/O failures → `OSError`. Previously these panicked with unstructured messages that `except` could not match. The builtin hierarchy applies (`except LookupError:` catches `IndexError`/`KeyError`).
- **Slice semantics are CPython-exact** (`PySlice_AdjustIndices` for both step signs). Previously `xs[4:0:-1]` silently returned `[]`, `xs[-100:2]` panicked, and a list slice with step 0 silently returned `[]` — now `[4, 3, 2, 1]`, `[0, 1]` (clamped), and a catchable `ValueError` respectively. String slicing is char-based (multibyte-correct).
- **Same-base index-assign compiles.** `self.data[len(self.data) - 1] = v` (and every place-chain variant: nested `grid[len(grid)-1][0]`, aug-assign, attribute assign, mutating-method receivers, `Mut[T]` arguments) no longer hits rustc E0502 — all place-chain indices pre-evaluate into temps.
- **Class dunder operands are no longer consumed.** Reusing `h` after `h + h2` (by-value `std::ops` impls) was a compile error; operands now follow value semantics (places clone, temporaries don't).
- Deterministic output: struct fields emit in source declaration order (was HashMap iteration order — same input could produce differently-ordered Rust run to run).

### Emitted-code quality

- `pyrst emit` output is formatted with rustfmt when available (silent fallback), literals emit without redundant parens, and the declaration-hoisting double-init artifact folds away in the common case.

### Internal

- `typeck.rs` (10.7k lines) and `codegen.rs` (7.7k) split into focused submodules (move-only; emit output byte-identical).
- New design doc: `docs/design/exception-lowering.md` — the v0.2+ migration path from panic/`catch_unwind` to `Result`-based exceptions.
- `PYTHON_COMPATIBILITY.md`'s exception-catchability section corrected (it was wrong in both directions).

### Quality

- `./test_all.sh`: 279/279 positive examples (+10), 104/104 negative fixtures (+1) at both gates; 513 `cargo test` cases; 0 compiler warnings.

## [0.1.0] — 2026-06-30

First tagged release. pyrst is a statically typed, Python-like language that compiles to readable Rust and then to a native binary via `rustc`. Guiding principle: **honest errors over silent miscompiles**.

### Language

- **Core:** functions (typed params + defaults), classes with single inheritance, `super()`/`__init__`, dunder methods, `@property`/`@staticmethod`; class subtyping via companion-enum polymorphism.
- **Types:** mandatory static typing; `list`/`dict`/`tuple`/`set`; `Optional[T]` / `T | None` with narrowing; value semantics with `Mut[T]` by-reference parameters.
- **Generics:** generic functions `def f[T](..)`; **bounded** generics with trait bounds (`PartialOrd`/`PartialEq`/`Add`/`Display`/`Hash+Eq`) inferred from the operations on `T`, with transitive propagation across generic calls; **generic classes** `class Box[T]`; generic `Callable` parameters — all monomorphized.
- **Functions:** lambdas, nested-`def` closures with lexical capture, `Callable[[A], R]` first-class values.
- **Generators:** `yield`, consumable by `for` loops and comprehensions (eager evaluation).
- **Pattern matching:** `match`/`case` with literal, wildcard, and capture patterns + guards.
- **Exceptions:** `try`/`except`/`else`/`finally`, `raise`, type-matched handlers over the builtin hierarchy, `except E as e`.
- **Comprehensions** (list/set/dict), f-strings, triple-quoted strings, string escapes incl. `\b`/`\f`, tuple unpacking, operator chaining.
- **Module-level constants** (`NAME: T = <literal>`).

### Rust interop

- **`@extern`:** bind a Rust expression template behind a typed pyrst signature (Rust `std` and beyond).
- **`@crate("name", "ver")`:** depend on external crates — the build switches from single-file `rustc` to a generated Cargo project. Crate names/versions are validated to prevent `Cargo.toml` injection.

### Standard library (embedded; `import`-resolved)

- **Core:** `math`, `os`, `time`, `operator`, `functools` (`reduce`), `statistics`, `string`.
- **Data structures / algorithms:** `bisect`, `heapq`, `collections` (`Counter`, `deque`, `defaultdict`), `itertools`, `textwrap`.
- **Parsing / external:** `re` (via the `regex` crate), `json` (a pure-pyrst recursive-descent parser + serializer over a dynamic `JsonValue`, RFC-compliant error handling), `random` (a seedable `Random` class).
- Both qualified (`import heapq; heapq.heappush(..)`) and flat (`from heapq import heappush`) forms work, including for generic stdlib functions.

### Tooling

- Language server (`pyrst lsp`): diagnostics, hover, go-to-definition, completion, semantic tokens.
- VS Code extension.
- `pyrst fmt` (formatter), `pyrst lint`, `pyrst repl`.

### Quality

- `./test_all.sh`: 269/269 positive examples (build + run, output-matched), 103/103 negative fixtures rejected at `check` and `build`, multi-file import demos.
- 513 in-crate `cargo test` cases; 0 compiler warnings; CI green.

### Known limitations

Eager (non-lazy) generators; no qualified generic-class instantiation (`collections.deque[int]()` — use a flat import); no generic methods inside classes; no module-level mutable state (so no `random` module-level API / `sys.argv`); a generic `Callable[[T], R]` with two distinct type variables is an honest error. See README for the full, honest list.
