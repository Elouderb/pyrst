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

Pre-alpha. v0 of the compiler implements:

- [x] Indentation-aware lexer
- [x] Recursive-descent parser (functions, classes, if/while, expressions)
- [x] Skeleton type checker (signatures only — bodies are TODO)
- [x] Rust codegen for the v0 subset
- [x] `pyrst build` invokes `rustc`
- [ ] Function body typechecking, name resolution, arity checks
- [ ] Class inheritance lowering (traits + default methods)
- [ ] Dunder methods → trait impls (`__add__` → `Add`, `__eq__` → `PartialEq`, …)
- [ ] `match` / `case` lowering
- [ ] Ownership inference pass
- [ ] Standard library shims (`list`, `dict`, `str` methods)

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
