# Phase 15: Advanced Language Features — Implementation Plan

**Date:** May 29, 2026  
**Goal:** Deliver high-impact language features for functional programming and completeness  
**Timeline:** 2-3 weeks (1-2 intensive sessions)

---

## Overview

Phase 15 adds three complementary features that expand pyrst's expressiveness:
1. **Lambda expressions** — Functional programming support
2. **Set collection type** — Complete standard collections
3. **Binary/hex/octal literals** — Number format support

---

## Phase 15.1: Lambda Expressions (Priority 1)

### Goal
Enable functional programming with first-class functions via `lambda` syntax.

### Scope
- Parse lambda expressions: `lambda x: x + 1`
- Support multi-argument lambdas: `lambda x, y: x + y`
- Type checking for lambda expressions
- Capture of variables from enclosing scope
- Use in function arguments (e.g., `map(lambda x: x*2, items)`)

### Implementation Strategy
**Approach:** Add lambda as expression-level syntax  
**Complexity:** Medium (closure capture requires variable tracking)  
**Files:** `src/lexer.rs`, `src/parser.rs`, `src/ast.rs`, `src/typeck.rs`, `src/codegen.rs`

### AST Addition
```rust
// In Expr enum:
Lambda { 
    params: Vec<(String, TypeExpr)>,
    body: Box<Expr>,
    span: Span,
}
```

### Commands
```bash
# In pyrst code:
double = lambda x: x * 2
result = map(double, [1, 2, 3])  # [2, 4, 6]
```

### Success Criteria
- ✅ Lambda expressions parse correctly
- ✅ Type checking works for lambdas
- ✅ Closure capture works (can use outer scope variables)
- ✅ Works in function arguments
- ✅ Generates correct Rust closures

### Examples
```python
# Basic lambda
square = lambda x: x * x

# Multi-argument lambda
add = lambda x, y: x + y

# Using with higher-order functions
numbers: list[int] = [1, 2, 3, 4, 5]
doubled: list[int] = [x * 2 for x in numbers]  # or via map

# Storing in variable
fn: (int) -> int = lambda x: x + 1
```

---

## Phase 15.2: Set Collection Type (Priority 2)

### Goal
Add `set[T]` as a built-in collection type for fast membership testing.

### Scope
- `set[T]` type annotation
- Set literal syntax: `{1, 2, 3}` (when context is clear)
- Set constructor: `set([1, 2, 3])`
- Common set operations: `.add()`, `.remove()`, `.contains()`
- Set methods: `.union()`, `.intersection()`, `.difference()`

### Implementation Strategy
**Approach:** Map to Rust's `HashSet<T>`  
**Complexity:** Low (similar to list/dict implementation)  
**Files:** `src/ast.rs`, `src/parser.rs`, `src/typeck.rs`, `src/codegen.rs`

### Commands
```bash
# In pyrst code:
primes: set[int] = {2, 3, 5, 7, 11}
primes.add(13)
if 5 in primes:  # O(1) lookup
    print("Found!")
```

### Success Criteria
- ✅ `set[T]` type checking works
- ✅ Set operations implemented
- ✅ Membership testing with `in` operator
- ✅ Common methods work

### Examples
```python
# Set operations
a: set[int] = {1, 2, 3}
b: set[int] = {2, 3, 4}

union_result: set[int] = a.union(b)      # {1, 2, 3, 4}
intersect: set[int] = a.intersection(b)   # {2, 3}
diff: set[int] = a.difference(b)          # {1}

# Membership testing
if 2 in a:
    print("Found!")
```

---

## Phase 15.3: Number Literal Formats (Priority 3)

### Goal
Support binary, hexadecimal, and octal number literals for systems programming.

### Scope
- Binary literals: `0b1010` = 10
- Octal literals: `0o755` = 493
- Hexadecimal literals: `0xFF` = 255
- Underscore separators: `0b1010_1010`, `0xFF_FF_FF`

### Implementation Strategy
**Approach:** Extend lexer number parsing  
**Complexity:** Low (purely lexical)  
**Files:** `src/lexer.rs`

### Commands
```bash
# In pyrst code:
binary: int = 0b1111_0000
hex: int = 0xFF_FF
octal: int = 0o755
```

### Success Criteria
- ✅ Binary literals parse correctly
- ✅ Octal literals parse correctly
- ✅ Hexadecimal literals parse correctly
- ✅ Underscore separators work
- ✅ Type checking treats them as `int`

### Examples
```python
# Bit masks
MASK_RED: int = 0xFF_00_00
MASK_GREEN: int = 0x00_FF_00
MASK_BLUE: int = 0x00_00_FF

# File permissions (Unix)
PERM_READ: int = 0o400
PERM_WRITE: int = 0o200
PERM_EXEC: int = 0o100

# Binary operations
flags: int = 0b1010_0101
```

---

## Implementation Order

### Session 1: Lambda Expressions
1. **Lexer:** Add `lambda` keyword
2. **Parser:** Parse lambda syntax
3. **AST:** Add Lambda expression variant
4. **Type Checker:** Type lambda expressions
5. **Codegen:** Generate Rust closures

### Session 2: Sets & Number Literals
1. **Sets - Type System:** Add `set[T]` type
2. **Sets - Parser:** Parse set literals and constructor
3. **Sets - Codegen:** Generate Rust HashSet code
4. **Number Literals:** Extend lexer for binary/hex/octal

---

## Testing Strategy

### Lambda Expression Tests
```python
# Basic lambda
square = lambda x: x * x
assert square(3) == 9

# Multi-argument lambda
add = lambda x, y: x + y
assert add(2, 3) == 5

# Closure capture
multiplier: int = 5
times_n = lambda x: x * multiplier
assert times_n(3) == 15

# Higher-order functions
double = lambda x: x * 2
nums: list[int] = [1, 2, 3]
doubled = [double(n) for n in nums]
assert doubled == [2, 4, 6]
```

### Set Tests
```python
# Set operations
a: set[int] = {1, 2, 3}
b: set[int] = {2, 3, 4}

# Union
union_ab: set[int] = a.union(b)
assert 1 in union_ab
assert 4 in union_ab

# Intersection
inter: set[int] = a.intersection(b)
assert 2 in inter
assert 1 not in inter

# Add/remove
s: set[int] = {1, 2}
s.add(3)
assert 3 in s
```

### Number Literal Tests
```python
# Binary
b: int = 0b1010
assert b == 10

# Octal
o: int = 0o755
assert o == 493

# Hex
h: int = 0xFF
assert h == 255

# With underscores
big: int = 0xFF_FF_FF
assert big == 16_777_215
```

---

## Verification Plan

### Regression Testing
- ✅ All 25 examples continue to compile
- ✅ Formatter handles new syntax
- ✅ Linter works with new features

### New Example Programs
```
examples/lambda_demo.py       - Lambda expressions
examples/lambda_closure.py    - Closure capture
examples/set_operations.py    - Set collection
examples/number_formats.py    - Number literals
```

### Build Verification
```bash
cargo build --release 2>&1 | grep "^error"  # Must be empty
for ex in examples/*.py; do
  cargo run --release --quiet -- build "$ex" 2>&1 | grep -q "built:"
done
```

---

## Known Limitations & Future Work

### Lambda Limitations
- No lambda type annotations on parameters (inferred from usage)
- No recursive lambdas (can't reference self)
- Capture by value only (no mutable capture)

### Set Limitations
- No set comprehensions (deferred to Phase 16+)
- No set operations as operators (union via method only)
- No frozenset type (deferred)

### Number Literal Limitations
- No scientific notation (1e10) — could add later
- No complex numbers — out of scope
- No rational numbers — out of scope

---

## Files Modified

| File | Changes |
|------|---------|
| `src/lexer.rs` | Add `lambda` keyword, extend number parsing |
| `src/parser.rs` | Add lambda expression parsing, set literals |
| `src/ast.rs` | Add Lambda variant, set type |
| `src/typeck.rs` | Type check lambdas and sets |
| `src/codegen.rs` | Generate closures and HashSet code |
| `src/formatter.rs` | Format lambda expressions and sets |
| `src/linter.rs` | Lint lambda and set usage |

---

## Success Metrics

### Phase 15.1 (Lambda)
- ✅ Parse lambda expressions
- ✅ Capture variables correctly
- ✅ Generate working Rust closures
- ✅ All 25 examples + 4 new lambda examples compile

### Phase 15.2 (Set)
- ✅ `set[T]` type checking works
- ✅ Set operations functional
- ✅ All examples compile

### Phase 15.3 (Number Literals)
- ✅ Binary/hex/octal parse correctly
- ✅ Type checking treats as `int`
- ✅ All examples compile

---

## Estimated Effort

| Feature | LOC | Time |
|---------|-----|------|
| Lambda | 150-200 | 3-4 hours |
| Set | 100-150 | 2-3 hours |
| Number Literals | 50-100 | 1-2 hours |
| **Total** | **300-450** | **6-9 hours** |

---

## Timeline

### Week 1
- **Day 1-2:** Lambda expressions (lexer, parser, type checker)
- **Day 3:** Lambda codegen and testing
- **Day 4:** Set type implementation

### Week 2
- **Day 1:** Set operations and testing
- **Day 2-3:** Number literal formats
- **Day 4:** Comprehensive testing and documentation

---

*Phase 15 Plan: May 29, 2026*

