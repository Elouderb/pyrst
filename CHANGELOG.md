# Changelog

All notable changes to pyrst are documented here. This project adheres to [Semantic Versioning](https://semver.org).

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
