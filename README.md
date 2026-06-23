# pyrst

A statically typed Python-like language that compiles to efficient Rust. Combines Python's ergonomics with Rust's safety and performance.

**Think:** TypeScript-to-JavaScript, but the target is Rust instead of JS.

## Key Features

- **Python-like syntax:** Indentation-based blocks, familiar control flow, readable declarations
- **Mandatory static typing:** All variables have compile-time types; strong guarantees
- **Rust compilation:** Generates readable Rust code, compiled via `rustc` to native binaries
- **Core functionality:** Functions, classes (single inheritance), collections, operators, pattern matching

## Status

**Active development** — Full compiler pipeline (lexer → parser → resolver → type checker → Rust codegen → `rustc`). **All 194 single-file examples transpile and run successfully** (`./test_all.sh`: 194/194 positives, 64/64 rejection fixtures, 199 in-crate `#[test]` cases).

The core pipeline is working end-to-end, including multi-file imports, classes with single inheritance and dunder methods, comprehensions, and a broad set of string/list/dict methods. Lambdas are implemented (see `examples/lambda_demo.pyrs`, `examples/lambda_closure.pyrs`).

**Known limitations (honest status):**
- The static type checker is **best-effort, not yet fully sound**: some expressions still infer to an `Unknown` type that is permissively compatible with everything, so a few type/ownership errors are surfaced by the downstream `rustc` invocation against generated Rust rather than by pyrst itself. Recent work (type-inference soundness pass) has narrowed this escape hatch substantially; the remaining gaps are tracked as deferred items.
- `try`/`except` matches on exception type and binds `except E as e` (the bound value is the exception message string). The builtin exception hierarchy is modeled — a base catches its builtin subclasses (e.g. `except LookupError:` catches `KeyError`/`IndexError`) — and caught exceptions print no stderr noise. Remaining limitation: user-defined exception classes match by exact type name (no user-defined subclass catching).

## Documentation Structure

**Understand the language:**

1. **[SPEC.md](SPEC.md)** — Formal language specification (what's supported, what's not)
2. **[GRAMMAR.md](GRAMMAR.md)** — Formal grammar for the parser
3. **[PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md)** — Compatibility matrix (honest comparison)

**Design & implementation:**

4. **[RUST_BACKEND.md](RUST_BACKEND.md)** — How pyrst constructs map to Rust
5. **[docs/design/](docs/design/)** — Design documents (inference oracle, class subtyping, etc.)

## Quick Example

```python
def fibonacci(n: int) -> int:
    if n <= 1:
        return n
    return fibonacci(n - 1) + fibonacci(n - 2)

def main() -> None:
    for i in range(10):
        print(fibonacci(i))
```

Compiles to Rust, then to native binary:

```bash
pyrst build examples/fib.pyrs
./fib
```

## What's Implemented ✅

- Functions with type annotations and default arguments
- Classes and methods (single inheritance, `super()`/`__init__`, dunder methods)
- Class subtyping via companion-enum polymorphism (closed-set dispatch)
- Decorators: `@property`, `@staticmethod`
- Value semantics: clone-on-use; `Mut[T]` parameter mode for by-reference mutation
- `Optional[T]` / `T | None` with explicit narrowing (`is None` / `is not None`)
- Variables with static types and type inference
- Collections: `list[T]`, `dict[K, V]`, `tuple[T1, T2, ...]`, `set[T]`
- Operators: arithmetic, comparison (including chaining `a < b < c`), logical, bitwise
- Ternary expressions: `x if cond else y`
- Lambdas: `lambda x: expr`
- Control flow: if/elif/else, while, for, break, continue, `with`/context managers
- Pattern matching: `match`/`case` with literal patterns and `_` wildcard
- String operations and f-strings with interpolation; triple-quoted strings
- List, dict, and set comprehensions with filters
- `try`/`except` with type-matched exception handling and `except E as e` binding
- Tuple unpacking in assignments and for loops
- `enumerate()`, `zip()`, `range()` with optional step
- `assert` statements and `raise` (maps to panic)
- Rust-keyword escaping: pyrst identifiers that collide with Rust keywords are emitted as raw identifiers (`r#type`, `r#loop`, etc.)
- Type checking with two-pass inference
- Code generation to readable Rust
- Multi-file programs with `import` and `from...import`
- Circular import detection with clear error messages

## What's NOT Implemented ❌

See [PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md) for a complete matrix.

Notable omissions (by design):
- Generators and `yield`
- Multiple inheritance
- User-defined exception-subclass catching (the *builtin* hierarchy works; user-defined exceptions match by exact type name)
- Metaclasses and dynamic attribute access
- `eval`/`exec` and reflection
- Python standard library compatibility
- Package directories (`foo/__init__.pyrs`)
- Module visibility / private modules
- Shared-mutable aliasing (`Rc`/`RefCell`-style); mutation is explicit via `Mut[T]`

## Building from Source

```sh
# Install Rust first: https://rustup.rs
git clone https://github.com/yourusername/pyrst.git
cd pyrst
cargo build --release

# Test it
cargo run --release -- build examples/hello.pyrs
./hello
```

## CLI Usage

```bash
pyrst check <file.pyrs>    # Parse and type-check
pyrst emit <file.pyrs>     # Print generated Rust to stdout
pyrst build <file.pyrs>    # Compile to native binary via rustc
```

## Project Philosophy

pyrst aims to preserve **Python's programming experience** (readable syntax, familiar semantics) while gaining **Rust's guarantees** (static types, memory safety, zero-cost abstractions).

This means:
- ✅ Python-like syntax and control flow
- ✅ Static types and compile-time safety
- ✅ Readable, efficient generated code
- ❌ NOT a Python-compatible subset
- ❌ NOT a Python runtime emulator

See [SPEC.md](SPEC.md) and [RUST_BACKEND.md](RUST_BACKEND.md) for design details and key tradeoffs.

## Status Summary

**Strengths:**
- Full compiler pipeline working end-to-end (lexer → parser → resolver → type checker → codegen → `rustc`)
- 194/194 single-file examples passing, covering core features and multi-file programs
- Clear separation of lexer, parser, type checker, code generator
- Readable generated Rust code
- Error messages with source code context

**Current Status:**
The pipeline is functional end-to-end (lexer → type checker → Rust codegen →
`rustc`) and matures with each epic — 194 passing example programs. Remaining work
is incremental (diagnostics polish, parser edge cases, docs). See SPEC.md and
PYTHON_COMPATIBILITY.md for the current feature surface and its honest limitations.

## Contributing

See [SPEC.md](SPEC.md) and [docs/design/](docs/design/) before proposing major features.

The project prioritizes **semantic clarity** and **compiler maturity** over raw feature count. New features should have explicit design decisions before implementation.

## License

MIT (or your preferred open-source license)

---

**For detailed information on language design, Python compatibility, or implementation details, see the documentation files linked above.**
