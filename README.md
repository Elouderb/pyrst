# pyrst

A statically typed, Python-like language that compiles to efficient Rust. Combines Python's ergonomics with Rust's safety and performance.

**Think:** TypeScript-to-JavaScript, but the target is Rust instead of JS.

```python
def first[T](xs: list[T]) -> T:
    return xs[0]

class Box[T]:
    value: T
    def __init__(self, v: T) -> None:
        self.value = v
    def get(self) -> T:
        return self.value

def main() -> None:
    print(first([10, 20, 30]))      # 10
    print(first(["a", "b"]))        # a  (same function, monomorphized)
    b: Box[int] = Box(42)
    print(b.get())                  # 42
```

## Key Features

- **Python-like syntax:** indentation-based blocks, familiar control flow, readable declarations
- **Mandatory static typing:** every binding has a compile-time type; honest errors over silent miscompiles
- **Compiles to readable Rust**, then to a native binary via `rustc` (no runtime, no GC)
- **Generics:** generic functions, **bounded** generics (trait bounds inferred from use), and **generic classes** (`class Box[T]`) — monomorphized
- **First-class functions:** lambdas, nested-`def` closures, `Callable[[A], R]` values
- **Generators** (`yield`), **pattern matching** (`match`/`case`, incl. capture patterns), **exceptions** (`try`/`except`/`finally`/`raise`)
- **Rust interop (`@extern`):** wrap Rust `std` — and **external crates** (`@crate`) — behind typed pyrst signatures
- **A standard library** written in pyrst + `@extern` (see below)
- **Editor support:** a language server (`pyrst lsp`) with diagnostics, hover, go-to-definition, completion, and semantic tokens; plus a VS Code extension

## Status

**v0.1.3.** Full compiler pipeline (lexer → parser → resolver → type checker → Rust codegen → `rustc`), end-to-end.

`./test_all.sh`: **310/310 positive examples** build + run with matching output, **140/140 negative fixtures** correctly rejected (at both `check` and `build`), plus multi-file import demos. **524 in-crate `cargo test` cases**, 0 compiler warnings, CI green. Generators are **lazy** (infinite generators are safe); slice semantics are CPython-exact (verified against python3 on a 5,700+-case oracle); builtin runtime errors are catchable by their Python exception type; emitted Rust is rustfmt-formatted and deterministic. There are **no known cases where an accepted program produces wrong output** — every such bug found by this release's adversarial reviews was fixed or is honestly rejected at `check`.

pyrst is **not** a Python-compatible subset or a Python runtime — it's its own statically typed language with Python-flavored syntax.

## Quick start

```sh
# Install Rust first: https://rustup.rs
cargo build --release
cargo run --release -- build examples/fib.pyrs
./fib
```

### CLI

```bash
pyrst build <file.pyrs>    # compile to a native binary via rustc (a Cargo project if it uses @crate)
pyrst emit  <file.pyrs>    # print the generated Rust to stdout
pyrst check <file.pyrs>    # parse + type-check only
pyrst fmt   <file.pyrs>    # format in place
pyrst lint  <file.pyrs>    # style / common-issue checks
pyrst repl                 # interactive shell
pyrst lsp                  # language server (stdin/stdout, for editors)
```

## What's implemented

**Types & data**
- Functions with type annotations and default arguments
- Classes & methods: single inheritance, `super()`/`__init__`, dunder methods (`__eq__`/`__lt__`/`__add__`/`__str__`/…), `@property`/`@staticmethod`
- Class subtyping via companion-enum polymorphism (closed-set dispatch)
- **Generics:** `def f[T](..)`; **bounded** generics — `PartialOrd`/`PartialEq`/`Add`/`Display`/`Hash+Eq` inferred from the operations used on `T`, with transitive propagation across generic calls; **generic classes** `class Box[T]`; generic `Callable` parameters
- Collections: `list[T]`, `dict[K, V]`, `tuple[..]`, `set[T]`
- `Optional[T]` / `T | None` with explicit narrowing (`is None` / `is not None`)
- Value semantics (clone-on-use); `Mut[T]` parameter mode for by-reference mutation

**Control & functions**
- if/elif/else, while, for, break, continue, `with open(...) as f:` (file context managers only — the general `__enter__`/`__exit__` protocol over user classes is a documented honest error, not silent)
- **Generators** (`yield`), lazy (on-demand execution, O(1) memory) — consumable by `for`/comprehensions/`list`/`sum`/`min`/`max`/`any`/`all`/`enumerate`/`zip`/`sorted`; infinite generators are safe (`while True: yield ...` + `break`)
- **First-class functions:** lambdas, nested-`def` closures (lexical capture), `Callable[[A], R]` values
- **Pattern matching:** `match`/`case` with literal, `_` wildcard, and capture (`case y:`) patterns + guards
- **Exceptions:** `try`/`except`/`else`/`finally`, `raise`, type-matched handlers with the builtin hierarchy (`except LookupError:` catches `KeyError`/`IndexError`), `except E as e`
- Comprehensions (list/set/dict) with filters; f-strings; triple-quoted strings; tuple unpacking
- `enumerate()`/`zip()`/`range()`, `assert`, operator chaining (`a < b < c`)

**Interop & modules**
- **`@extern`:** bind a Rust expression template behind a typed pyrst signature
- **`@crate("name", "ver")`:** depend on an external crate (the build switches to a Cargo project); names/versions are validated to prevent `Cargo.toml` injection
- Multi-file programs (`import` / `from … import`), an embedded standard library, circular-import detection

## Standard library

Written in pyrst (pure pyrst and/or `@extern`), embedded in the compiler binary and resolved on `import`:

| | Modules |
|---|---|
| **Core** | `math`, `os`, `time`, `operator`, `functools` (`reduce`), `statistics`, `string` |
| **Data structures / algorithms** | `bisect`, `heapq`, `collections` (`Counter`, `deque`, `defaultdict`), `itertools`, `textwrap` |
| **Parsing / external** | `re` (via the `regex` crate), `json` (a pure-pyrst recursive-descent parser + serializer over a dynamic `JsonValue`), `random` (a seedable `Random` class) |

Both `import math; math.sqrt(x)` and `from math import sqrt` forms work, including for generic stdlib functions (`import heapq; heapq.heappush(h, x)`).

## Known limitations (honest status)

By design (see [SPEC.md](SPEC.md) / [PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md)): not Python-compatible; multiple inheritance, metaclasses, dynamic attribute access, `eval`/`exec`, and shared-mutable aliasing (`Rc`/`RefCell`) are out.

Current v0.1.3 gaps (tracked, with workarounds):
- **Generators (`yield`) are lazy**, but a few shapes are deferred: `Iterator[T]` as a *parameter* type, generator **methods** (`yield` in a class method), `yield` inside `try`/`except`/`finally`, nested generator `def`s, generator expressions (`(x for x in ...)`), and explicit `next(g)`. Every non-lazy consumption (`len`/`gen[i]`/slicing/`reversed`/`str`/binops/`x in gen`/passing a generator where `list[T]` is required) is an honest `pyrst check` error suggesting `list(gen)` to materialize — see [PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md#generators-yield).
- **Generic classes** can't be instantiated via a *qualified* name (`collections.deque[int]()`); use a flat import (`from collections import deque; d: deque[int] = deque()`).
- **Generic methods inside a class** (`def m[U](self)`) are not yet supported (top-level generic functions are).
- **No module-level mutable state**, so `random`'s module-level convenience API and `sys.argv` are not available (use the `Random` class; pass args explicitly).
- A generic `Callable[[T], R]` with **two distinct** type variables (where `R` comes from a lambda's return) is an honest error; same-type-var forms (`Callable[[T], T]`, `Callable[[T, T], T]`) work.
- The type checker leans on `rustc` for a few residual ownership/edge cases; the honest-errors invariant (no silent miscompiles) is the priority and is enforced by an extensive negative-test suite.

## Documentation

- **[SPEC.md](SPEC.md)** — language specification
- **[GRAMMAR.md](GRAMMAR.md)** — parser grammar
- **[PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md)** — compatibility matrix
- **[RUST_BACKEND.md](RUST_BACKEND.md)** — how pyrst maps to Rust
- **[CHANGELOG.md](CHANGELOG.md)** — release history
- **[docs/design/](docs/design/)** — design documents

## Project philosophy

pyrst preserves **Python's programming experience** (readable syntax, familiar semantics) while gaining **Rust's guarantees** (static types, memory safety, native performance). The guiding principle is **honest errors over silent miscompiles**: when something can't be compiled correctly, pyrst reports it rather than emitting wrong code.

## License

MIT
