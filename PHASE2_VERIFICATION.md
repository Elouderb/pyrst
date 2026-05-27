# Phase 2 Implementation Verification

This document explains how to verify that Phase 2 implementation is complete and working. You'll need Rust installed via rustup.

## Installation (one-time)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

Then, in the pyrst repository:
```bash
cargo build --release
```

## Expected Test Results

Once built, you should be able to run these commands and see the expected outputs.

### Test 1: hello.py (simple print)
```bash
cargo run --release -- build examples/hello.py
./hello
# Expected output: hello, pyrst
```

### Test 2: fib.py (recursion and arithmetic)
```bash
cargo run --release -- build examples/fib.py
./fib
# Expected output: 6765
```

### Test 3: point.py (classes with keyword arguments) ✨ NEW
```bash
cargo run --release -- build examples/point.py
./point
# Expected output: 5 (or 5.0)
```

This tests the three Phase 2 blockers:
- `self` parameter without type annotation
- Keyword arguments in constructor call: `Point(x=3.0, y=4.0)`
- Struct literal codegen for classes

### Test 4: count.py (for loops) ✨ NEW
```bash
cargo run --release -- build examples/count.py
./count
# Expected output:
# 0
# 1
# 2
# 3
# 4
```

### Test 5: Type checker error detection

Create a file with a type error:
```bash
cat > /tmp/bad_arity.py <<'EOF'
def add(x: int, y: int) -> int:
    return x + y

def main() -> None:
    print(add(1))
EOF

cargo run --release -- check /tmp/bad_arity.py
# Expected: type error: function `add` takes 2 argument(s), 1 given
```

### Test 6: Undefined name detection

```bash
cat > /tmp/undefined.py <<'EOF'
def main() -> None:
    print(undefined_var)
EOF

cargo run --release -- check /tmp/undefined.py
# Expected: type error: undefined name `undefined_var`
```

### Test 7: Inspect generated Rust

```bash
cargo run --release -- emit examples/point.py
# Expected to see:
# - Point struct definition with x: f64 and y: f64 fields
# - magnitude method with &mut self signature
# - Main function that constructs Point with struct literal:
#   Point { x: 3.0f64, y: 4.0f64 }
```

## What Was Implemented

### AST Changes (`src/ast.rs`)
- Added `kwargs: Vec<(String, Expr)>` field to `Expr::Call`
- Added `Stmt::For { target: String, iter: Expr, body: Vec<Stmt>, span: Span }`

### Parser Changes (`src/parser.rs`)
- Added `peek2()` helper for lookahead
- Fixed `parse_param()` to detect `self` and use sentinel type
- Rewrote call-arg parsing in `parse_postfix()` to detect and accumulate keyword arguments
- Added `parse_for()` function and match arm in `parse_stmt()`

### Codegen Changes (`src/codegen.rs`)
- Added `__pyrst_range(n)` and `__pyrst_range2(a, b)` shims to preamble
- Rewrote `Expr::Call` arm in `emit_expr()` to:
  - Detect class constructor calls by name lookup in `ctx.classes`
  - Emit Rust struct literals `Point { x: val, y: val }` for constructors
  - Support positional arguments (mapped to field order) and keyword arguments
  - Rewrite `range` builtin to `__pyrst_range`
- Added `Stmt::For` arm in `emit_stmt()` to generate Rust `for...in` loops

### Type Checker Changes (`src/typeck.rs`)
- Added `FuncSig { params: Vec<(String, Ty)>, ret: Ty }` struct
- Changed `TyCtx.funcs` from `HashMap<String, Ty>` to `HashMap<String, FuncSig>`
- Added `FuncEnv` struct for local scope during body checking
- Implemented two-pass type checking:
  - **Pass 1:** Collect function signatures and class definitions
  - **Pass 2:** Type-check function bodies with:
    - Name resolution (locals, globals, class names)
    - Arity checking (parameter count matching)
    - Return type verification
    - Type inference from literals and expressions
    - Field/attribute access validation
    - Lenient compatibility via `Ty::Unknown`

## Why Lenient Type Checking?

In v0, we use `Ty::Unknown` as a wildcard that's compatible with any type. This avoids false positives in cases where we don't yet have precise type information:

- Iterator element types (loop variables)
- Method return types (no method signature tracking yet)
- Dynamic operations (indexing, attribute access on unknown types)

This will be tightened in v0.1+ once more type inference machinery is in place.

## Known Limitations (v0)

- No type narrowing (e.g., after `if isinstance(x, int):`)
- No method signature tracking (method returns are `Unknown`)
- No generics (each instantiation is a copy)
- No subtyping or inheritance (base class methods not available)
- No operator overloading beyond dunder-to-trait mapping in codegen
- No `@property`, `@classmethod`, `@staticmethod` (syntax recognized, semantics deferred)

## Next Steps (v0.1+)

1. Dunder method → trait impl lowering (`__add__` → `impl Add`)
2. More standard library methods (string, list, dict)
3. Default argument values and variadic args
4. Pattern matching in match statements
5. Type narrowing and isinstance checks
6. Full exception handling (try/except with CFG-based exception edges)

## Files Modified

- `src/ast.rs` — +3 lines (Expr::Call kwargs field, Stmt::For variant)
- `src/parser.rs` — +40 lines (peek2, self param, kw args, for loop)
- `src/codegen.rs` — +73 lines (range shims, class constructor, for loops)
- `src/typeck.rs` — +313 lines (FuncSig, FuncEnv, two-pass checking, expr checking)
- `examples/count.py` — +3 lines (new example for for loops)

**Total: 416 lines added/modified across Phase 2 implementation**
