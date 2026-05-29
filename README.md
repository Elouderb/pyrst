# pyrst

A statically typed Python-like language that compiles to efficient Rust. Combines Python's ergonomics with Rust's safety and performance.

**Think:** TypeScript-to-JavaScript, but the target is Rust instead of JS.

## Key Features

- **Python-like syntax:** Indentation-based blocks, familiar control flow, readable declarations
- **Mandatory static typing:** All variables have compile-time types; strong guarantees
- **Rust compilation:** Generates readable Rust code, compiled via `rustc` to native binaries
- **Core functionality:** Functions, classes (single inheritance), collections, operators, pattern matching

## Status

**Phases 1-8 Complete** — Full compiler with multi-file module system. 22+ examples passing.

**Core compiler stable** with Phases 9-14 formally specified and ready for implementation. Focus shifted to semantic clarity and compiler maturity (from Phase 7 forward).

Recent completion (May 28, 2026):
- Phase 7: Formal specifications (SPEC.md, PYTHON_COMPATIBILITY.md, DESIGN_DECISIONS.md)
- Phase 7: Improved diagnostics with source code snippets
- Phase 8: Multi-file imports with DFS resolution and cycle detection
- Phase 8: `examples/multi_file_demo/` — 3-file example with shared utilities

## Documentation Structure

**Start here:**

1. **[PHASES_7_8_COMPLETION.md](PHASES_7_8_COMPLETION.md)** — What was completed in Phases 7 & 8 (specifications, module system, diagnostics)
2. **[DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)** — Strategic vision and complete roadmap (Phases 9-15+)

**Understand the language:**

3. **[SPEC.md](SPEC.md)** — Formal language specification (what's supported, what's not)
4. **[PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md)** — Compatibility matrix (honest comparison)
5. **[DESIGN_DECISIONS.md](DESIGN_DECISIONS.md)** — Key design choices and tradeoffs

**Implement features:**

6. **[RUST_BACKEND.md](RUST_BACKEND.md)** — How pyrst constructs map to Rust
7. **[DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md#strategic-design-decisions)** — Design principles for new features

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
- Exception handling recovery (`try`/`except`/`finally` — planned Phase 11)
- Type narrowing for optionals (planned Phase 9)
- Reference semantics for classes (planned Phase 9)
- Generators and `yield`
- Lambda expressions
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
- Full compiler pipeline working end-to-end (Phases 1-8)
- 22+ passing examples covering core features and multi-file programs
- Clear separation of lexer, parser, type checker, code generator
- Readable generated Rust code
- Formal specifications for language, backend, and design decisions
- Improved error messages with source code context

**Recent Completion (Phases 7-8):**
- ✅ Formal language specification and compatibility matrix
- ✅ Improved diagnostics with source code snippets
- ✅ Multi-file module system with DFS import resolution
- ✅ Circular import detection and error reporting
- ✅ Development plan and roadmap through Phase 15

**Current Status:**
Ready for Phase 9 (Semantic Cleanup). All specifications written; no blockers.

## Contributing

See [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) and [DESIGN_DECISIONS.md](DESIGN_DECISIONS.md) before proposing major features.

The project prioritizes **semantic clarity** and **compiler maturity** over raw feature count. New features should have explicit design decisions before implementation.

## License

MIT (or your preferred open-source license)

---

**For detailed information on language design, Python compatibility, or implementation details, see the documentation files linked above.**
