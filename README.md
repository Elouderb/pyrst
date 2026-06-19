# pyrst

A statically typed Python-like language that compiles to efficient Rust. Combines Python's ergonomics with Rust's safety and performance.

**Think:** TypeScript-to-JavaScript, but the target is Rust instead of JS.

## Key Features

- **Python-like syntax:** Indentation-based blocks, familiar control flow, readable declarations
- **Mandatory static typing:** All variables have compile-time types; strong guarantees
- **Rust compilation:** Generates readable Rust code, compiled via `rustc` to native binaries
- **Core functionality:** Functions, classes (single inheritance), collections, operators, pattern matching

## Status

**Phase 38 (in development)** — Full compiler pipeline (lexer → parser → resolver → type checker → Rust codegen → `rustc`). **96 of 105 single-file examples (~91%) transpile and run successfully.**

The core pipeline is working end-to-end, including multi-file imports, classes with single inheritance and dunder methods, comprehensions, and a broad set of string/list/dict methods. Lambdas are implemented (see `examples/lambda_demo.py`, `examples/lambda_closure.py`).

**Known limitations (honest status):**
- The static type checker is **best-effort, not yet sound**: many expressions infer to an `Unknown` type that is permissively compatible with everything, so some type/ownership errors are surfaced by the downstream `rustc` invocation against generated Rust rather than by pyrst itself. The 9 currently-failing examples all fail this way.
- `try`/`except` does not yet match on exception type or bind the exception.
- f-string interpolations are emitted as raw source rather than fully compiled expressions (works only for the subset that is coincidentally valid Rust).

## Documentation Structure

**Understand the language:**

1. **[SPEC.md](SPEC.md)** — Formal language specification (what's supported, what's not)
2. **[GRAMMAR.md](GRAMMAR.md)** — Formal grammar for the parser
3. **[TYPE_SYSTEM.md](TYPE_SYSTEM.md)** — Type system and inference
4. **[PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md)** — Compatibility matrix (honest comparison)

**Design & implementation:**

5. **[DESIGN_DECISIONS.md](DESIGN_DECISIONS.md)** — Key design choices and tradeoffs
6. **[RUST_BACKEND.md](RUST_BACKEND.md)** — How pyrst constructs map to Rust
7. **[ERRORS.md](ERRORS.md)** — Diagnostics and error-message philosophy
8. **[DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)** — Roadmap and design principles

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
pyrst build examples/fib.py
./fib
```

## What's Implemented ✅

- Functions with type annotations
- Classes and methods (single inheritance)
- Variables with static types and type inference
- Collections: `list[T]`, `dict[K, V]`, `tuple[T1, T2, ...]`
- Operators: arithmetic, comparison, logical, bitwise
- Control flow: if/elif/else, while, for, break, continue
- String operations and f-strings with interpolation
- List comprehensions with filters
- Tuple unpacking in assignments and for loops
- `enumerate()`, `zip()`, `range()` with optional step
- `assert` statements and `raise` (maps to panic)
- Type checking with two-pass inference
- Code generation to readable Rust
- Multi-file programs with `import` and `from...import`
- Circular import detection with clear error messages

## What's NOT Implemented ❌

See [PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md) for a complete matrix.

Notable omissions (by design):
- Exception handling with type matching / binding (`except E as e`) — `try`/`except` exists but runs handlers unconditionally
- Type narrowing for optionals
- Generators and `yield`
- Multiple inheritance
- Metaclasses and dynamic attribute access
- `eval`/`exec` and reflection
- Python standard library compatibility
- Package directories (`foo/__init__.py`)
- Module visibility / private modules

## Building from Source

```sh
# Install Rust first: https://rustup.rs
git clone https://github.com/yourusername/pyrst.git
cd pyrst
cargo build --release

# Test it
cargo run --release -- build examples/hello.py
./hello
```

## CLI Usage

```bash
pyrst check <file.py>    # Parse and type-check
pyrst emit <file.py>     # Print generated Rust to stdout
pyrst build <file.py>    # Compile to native binary via rustc
```

## Project Philosophy

pyrst aims to preserve **Python's programming experience** (readable syntax, familiar semantics) while gaining **Rust's guarantees** (static types, memory safety, zero-cost abstractions).

This means:
- ✅ Python-like syntax and control flow
- ✅ Static types and compile-time safety
- ✅ Readable, efficient generated code
- ❌ NOT a Python-compatible subset
- ❌ NOT a Python runtime emulator

See [DESIGN_DECISIONS.md](DESIGN_DECISIONS.md) for key tradeoffs.

## Status Summary

**Strengths:**
- Full compiler pipeline working end-to-end (lexer → parser → resolver → type checker → codegen → `rustc`)
- 96/105 single-file examples passing, covering core features and multi-file programs
- Clear separation of lexer, parser, type checker, code generator
- Readable generated Rust code
- Error messages with source code context

**Current Status:**
Phase 38, in active development. The pipeline is functional; the type checker's
soundness and several codegen corner cases (f-strings, `try`/`except`,
int/float promotion, collection mutation) are the main areas of ongoing work.

## Contributing

See [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) and [DESIGN_DECISIONS.md](DESIGN_DECISIONS.md) before proposing major features.

The project prioritizes **semantic clarity** and **compiler maturity** over raw feature count. New features should have explicit design decisions before implementation.

## License

MIT (or your preferred open-source license)

---

**For detailed information on language design, Python compatibility, or implementation details, see the documentation files linked above.**
