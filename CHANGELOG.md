# Changelog

All notable changes to pyrst are documented here. This project adheres to [Semantic Versioning](https://semver.org).

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
