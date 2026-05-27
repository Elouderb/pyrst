# pyrst

A Pythonic language that compiles to Rust. Think *TypeScript-to-JavaScript*, but the
target is Rust instead of JS.

- **Pythonic surface**: indentation-based blocks, `def`, `class`, dunder methods,
  `match`/`case`, dataclass-style fields.
- **Strict typing**: type annotations are mandatory. Inference exists, but every
  binding has a known type.
- **Compiled**: pyrst lowers your `.py` source to Rust, then invokes `rustc`.
- **Memory model**: ownership is inferred. `clone()` and `Rc` are inserted where
  the analyzer can't prove a borrow is safe. Power users can drop down to explicit
  `&` / `&mut` annotations (planned).

## Status

Pre-alpha. v0 of the compiler (Phase 2):

**Implemented:**
- [x] Indentation-aware lexer (all v0 tokens)
- [x] Recursive-descent parser (functions, classes, if/while/for, expressions)
- [x] Full function body type checking with name resolution and arity checking
- [x] Keyword arguments in function/class constructor calls
- [x] `self` parameter support (no type annotation required)
- [x] `for` loop syntax and codegen
- [x] Class constructor calls → Rust struct literals
- [x] Rust codegen for the v0 subset
- [x] `pyrst build`, `emit`, `check` subcommands

**Working example programs:**
- `examples/hello.py` — print statement
- `examples/fib.py` — recursive functions, arithmetic, control flow
- `examples/point.py` — classes, methods, keyword arguments ✨
- `examples/count.py` — for loops ✨

**Not yet implemented (v0.1+):**
- [ ] Class inheritance lowering (traits + default methods)
- [ ] Dunder methods → trait impls (`__add__` → `Add`, `__eq__` → `PartialEq`, …)
- [ ] `match` / `case` pattern matching semantics
- [ ] Ownership inference pass
- [ ] Standard library methods (`list`, `dict`, `str` methods beyond basics)
- [ ] Default argument values, variadic args (`*args`, `**kwargs`)

## Building

```sh
# install rustup first: https://rustup.rs
cargo build --release
./target/release/pyrst build examples/hello.py
./hello
```

## CLI

```
pyrst check <file.py>   # parse + typecheck
pyrst emit  <file.py>   # print generated Rust to stdout
pyrst build <file.py>   # compile to a native binary via rustc
```
