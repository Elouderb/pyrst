# pyrst Language Specification

## Overview

pyrst is a compiled language with Pythonic surface syntax that targets Rust as an intermediate language. It enforces static typing at compile time, uses Rust's type system and memory model as implementation anchors, and generates human-readable Rust source that is compiled via `rustc`.

**Design motto:** "Python feel, Rust safety, ahead-of-time compilation."

## v0 Supported Profile

The v0 milestone supports a carefully bounded subset of Python-like syntax and semantics. This document defines what is in and what is explicitly deferred.

### ✅ In v0

#### Syntax and Control Flow
- **Indentation-based blocks** (def, class, if, elif, else, while, for, with)
- **Function definitions** (`def name(args: Type) -> ReturnType:`)
- **Class definitions** (`class Name: ...`)
- **Type annotations** (PEP 484 style: `: Type` and `-> Type`)
- **If/elif/else**, **while**, **for...in** (including range and comprehensions)
- **List comprehensions** (`[expr for x in iter if cond]`)
- **Dict comprehensions** (`{k: v for k, v in iter}`)
- **Match/case** (Python 3.10 pattern matching, deferred to v0.2)
- **Arithmetic, comparison, logical, and bitwise operators**
- **String literals** (basic; f-strings deferred)
- **Method calls, attribute access, subscripting**
- **Imports** (`import module`, `from module import name`)
- **Comments** (`# single-line`)
- **Decorators** (syntax parsed, semantics deferred)

#### Type System
- **Scalar types:** `int`, `float`, `bool`, `str`, `None`
- **Container types:** `list[T]`, `dict[K, V]`, `tuple[T, ...]`
- **Function types** (annotations only, no first-class functions yet)
- **Class types** (user-defined, with fixed declared attributes)
- **Union types** (via `T | U` or `Union[T, U]`, basic support)
- **Generic types** (nominal, monomorphized at compile time)
- **Type aliases** (`TypeAlias = SomeType`)

#### Classes and Objects
- **Class definitions with type-annotated fields**
- **Instance methods** (implicit `self` parameter)
- **Class methods** (deferred; `@classmethod`)
- **Static methods** (deferred; `@staticmethod`)
- **Dunder methods** (syntax recognized; lowering to Rust traits deferred)
  - Supported (planned): `__init__`, `__add__`, `__sub__`, `__eq__`, `__lt__`, `__repr__`, `__str__`
- **Single inheritance** (via trait composition; multiple inheritance deferred)
- **Instance attributes** (declared with type annotations; no dynamic attribute injection)

#### Exception Handling
- **Raise statements** (`raise Exception()`)
- **Try/except/finally** (basic; exception type matching deferred)

#### Built-in Functions (planned)
- `print()`, `len()`, `range()`, `int()`, `float()`, `str()`, `bool()`
- `enumerate()`, `zip()`, `map()`, `filter()` (deferred)
- `open()`, `min()`, `max()`, `sum()`, `abs()`

#### Standard Library (v0 shims)
- **Containers:** `list`, `dict`, `set`, `tuple` with basic methods
- **Strings:** `str` with `.split()`, `.join()`, `.strip()`, `.upper()`, `.lower()`
- **Builtins:** exception classes, type conversions

#### Memory and Ownership
- **Implicit ownership inference:** The compiler decides when to clone, wrap in `Rc`, or borrow.
- **No explicit `&` / `&mut` in v0 user syntax** (internal lowering only).
- **Reference semantics for containers and objects** (like Python).
- **Value semantics for scalars** (int, float, bool, str, None).

#### Code Organization
- **Modules** (one file = one module)
- **Import statements** (module and name imports)
- **Module initialization** (`if __name__ == "__main__":` recognized but currently ignored)

### ❌ Explicitly Out of v0 (Deferred)

- **f-strings** (string interpolation; deferred to v0.2)
- **List/dict/set methods** beyond basics (use functions or standard library)
- **Async/await** (deferred to v1.0)
- **Decorators with arguments** (syntax only in v0)
- **Metaclasses, descriptors, `__getattr__`, `__setattr__`**
- **Dynamic attribute injection** (classes have fixed shape)
- **Reflection** (`isinstance()`, `hasattr()`, `getattr()`, `eval()`, `exec()`)
- **`with` statement** (context managers; syntax recognized, semantics deferred)
- **Generator expressions and `yield`** (deferred)
- **Lambda expressions** (deferred)
- **Variadic arguments** (`*args`, `**kwargs`; deferred)
- **Default argument values** (deferred)
- **Keyword-only arguments** (deferred)
- **`@property`, `@classmethod`, `@staticmethod`** (syntax parsed; lowering deferred)
- **Multiple inheritance** (deferred)
- **Operator overloading via dunder methods** (syntax parsed; lowering to Rust traits deferred)
- **Global/nonlocal declarations** (deferred; closure semantics TBD)
- **Slice syntax** (deferred)
- **Walrus operator** (`:=`, deferred)
- **Match/case** (deferred to v0.2; `match` keyword parsed, semantics later)
- **Type narrowing** (e.g., after `isinstance()` checks; deferred)
- **Generics with bounds** (deferred to v1.0)
- **Custom iterators** (deferred; `__iter__`, `__next__`)

### ⚠️ Partial or Provisional Support

- **None type:** `None` is a literal and a type; no `Optional[T]` yet (use `T | None`).
- **Callable types:** Not yet first-class; only in annotations.
- **Static methods/class methods:** Syntax recognized; lowering deferred.

## Type System Guarantees

1. **All bindings are typed.** Every local variable, parameter, class attribute, and return value must have an explicit or inferred type. There is no untyped code in v0.
2. **No implicit dynamic escape.** If a type cannot be inferred, the compiler emits a diagnostic; no silent fallback to `Any`.
3. **Monomorphization:** Generic functions and classes are instantiated per use site; no runtime type information.
4. **Type checking is static.** Type validity is decided at compile time; no runtime type coercion beyond C ABI interop.

## Code Execution Model

- **Compilation:** Parse → Type-check → Lower to Rust → Invoke `rustc` → Executable or library.
- **Entry point:** A `main()` function at module level (currently hardcoded; `if __name__ == "__main__":` recognized but ignored).
- **Module initialization:** Global code in a module is executed in declaration order when the module is imported.
- **No REPL, interpreter, or runtime bytecode** in v0.

## Mapping to Rust

| pyrst | Rust | Notes |
|---|---|---|
| `def f(x: int) -> int:` | `fn f(x: i32) -> i32 { ... }` | Function lowering |
| `class C:` | `struct C { ... }` + `impl C { ... }` | Trait composition for inheritance |
| `x: list[int]` | `Vec<i32>` | Mutable containers; immutable lists deferred |
| `x: str` | `String` or `&str` (TBD) | String handling |
| `x: int` | `i32` (or `i64`, TBD) | Integer size TBD |
| `__init__` method | `fn new(...)` or custom constructor | Constructor lowering |
| `__add__` method | `impl Add for C { ... }` | Dunder method → trait impl |
| `for x in iter:` | `for x in iter { ... }` | Loop lowering |
| List/dict methods | Method calls on container types | Standard library shims |

## Planned v0.1, v0.2, v1.0 Extensions

- **v0.1:** Default argument values, keyword-only arguments, `*args`.
- **v0.2:** f-strings, match/case lowering, more dunder methods, `@property`.
- **v1.0:** Async/await, full generics with bounds, Python interop, LLVM backend option.

## Rationale

This profile balances three design goals:

1. **Pythonic feel:** Code should look and read like Python.
2. **Compile-time safety:** Every type is known at compile time; no dynamic surprises.
3. **Tractable scope:** Avoid the unbounded task of reimplementing all of Python.

The exclusions are intentional. Dynamic features (reflection, eval, monkey-patching) are excluded because they conflict with ahead-of-time compilation and whole-program type checking. Features like async/await, lambdas, and generators are deferred because they require runtime support or significant IR complexity, but they are designed to be added later without breaking the core.
