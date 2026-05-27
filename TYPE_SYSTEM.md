# pyrst Type System

## Overview

pyrst uses a **static, compile-time type system** with **local bidirectional inference**. All bindings must have a type, either explicitly annotated or inferred from context. Type checking happens after parsing and before code generation.

## Core Principles

1. **Mandatory explicit types at boundaries:**
   - Function parameters: `def f(x: int, y: str) -> bool:`
   - Class attributes: `class C: x: int; y: str`
   - Module-level variables: `x: int = 42`
   - Returns: `def f() -> int:`

2. **Local inference inside function bodies:**
   - Variable initializers: `x = 42` infers `x: int`
   - Expression results: inferred from operators and function calls
   - No global type inference; every function is typed independently

3. **No implicit `Any` or `Dynamic`:**
   - If type cannot be inferred, it's a compile error
   - Optional escape hatch: explicit `Dynamic` type (v1.0+)

4. **Monomorphization of generics:**
   - Generic functions/classes instantiated per use site
   - No runtime polymorphism; all type information available at compile time

## Scalar Types

| Type | Rust Mapping | Size (typical) | Literals | Notes |
|---|---|---|---|---|
| `int` | `i32` or `i64` (TBD) | 4 or 8 bytes | `42`, `-1`, `0x2A`, `0o52`, `0b101010` | Integer type; size TBD |
| `float` | `f64` | 8 bytes | `3.14`, `1.0`, `1e-2`, `2.5e3` | IEEE 754 double precision |
| `bool` | `bool` | 1 byte | `True`, `False` | Boolean; no truthiness coercion |
| `str` | `String` or `&str` (TBD) | Variable | `"hello"`, `'world'`, `"""multi"""` | Immutable strings |
| `None` | `()` | 0 bytes | `None` | Unit type; represents absence |

## Container Types

| Type | Rust Mapping | Notation | Mutable? | Notes |
|---|---|---|---|---|
| List | `Vec<T>` | `list[T]` | Yes | Homogeneous, growable array |
| Dict | `HashMap<K, V>` or `BTreeMap<K, V>` (TBD) | `dict[K, V]` | Yes | Key-value map |
| Tuple | `(T1, T2, ...)` | `tuple[T1, T2, ...]` or `(T1, T2, ...)` | No | Fixed-length, heterogeneous |
| Set | `HashSet<T>` | `set[T]` | Yes | Unique elements (deferred) |

### Container Type Rules

1. **Homogeneity:** `list[int]` holds only `int` values; heterogeneous lists require `list[object]` or `tuple[int, str]`.
2. **Mutability:** Lists and dicts are mutable by default. Immutable container types deferred to v1.0.
3. **Type parameters are invariant** (no covariance/contravariance in v0; all instantiations must match exactly).

## Function Types

### Function Declarations

```python
def f(x: int, y: str) -> bool:
    return len(y) > x

def g() -> None:
    print("no return")

def h(items: list[int]) -> list[int]:
    return [x * 2 for x in items]
```

### Type of Functions

- Function types are **not first-class** in v0 (cannot assign functions to variables).
- Function names are resolved at compile time to specific function symbols.
- Function types in annotations (v1.0): `Callable[[int, str], bool]` or `(int, str) -> bool`.

### Parameter and Return Type Constraints

- All parameters must have explicit type annotations.
- All return types must be explicit (no implicit inference of return type from return statements).
- If a function has multiple return paths, all must return the same type.

## Class Types

### Class Declarations

```python
class Point:
    x: float
    y: float

    def distance(self) -> float:
        return (self.x ** 2 + self.y ** 2) ** 0.5

class Rectangle(Point):
    width: float
    height: float

    def area(self) -> float:
        return self.width * self.height
```

### Class Type Rules

1. **Fixed attributes:** All instance attributes must be declared with type annotations in the class body. Dynamic attribute injection is not allowed in v0.

2. **Self type:** The first parameter of an instance method is `self` (implicit; not declared in parameters).

3. **Constructors:** A class can define an `__init__` method for initialization. If not provided, a default zero-argument constructor is generated.

4. **Method resolution:** Methods are looked up by name at compile time. No method overloading (same name, different arity) in v0.

5. **Instance method types:** Instance methods receive an implicit `self` parameter of the class's type.

6. **Class methods (deferred):** `@classmethod` is syntax-recognized but semantics deferred to v0.2.

7. **Static methods (deferred):** `@staticmethod` is syntax-recognized but semantics deferred to v0.2.

## Type Inference

### Inference Rules

1. **Literal inference:**
   - Integer literal → `int`
   - Float literal → `float`
   - String literal → `str`
   - `True` / `False` → `bool`
   - `None` → `None` (unit type)
   - `[...]` → `list[T]` (element type inferred from contents or annotation)
   - `{...}` → `dict[K, V]` or `set[T]` (context-dependent)

2. **Operator inference:**
   - Arithmetic operators (`+`, `-`, `*`, `/`, `//`, `%`, `**`) preserve or coerce operand types
   - Boolean operators (`and`, `or`, `not`) expect `bool` and return `bool`
   - Comparison operators return `bool`
   - Bitwise operators expect `int` and return `int`

3. **Assignment and variable binding:**
   - `x = expr` infers `x`'s type from `expr`'s type
   - `x: T = expr` requires `expr`'s type to match or be compatible with `T`

4. **Function call inference:**
   - Return type inferred from function signature
   - Argument types checked against parameter types (no implicit coercion)

5. **Container subscripting:**
   - `list[T][i]` → `T`
   - `dict[K, V][k]` → `V`
   - `tuple[T1, T2, ...][0]` → `T1` (literal index only)

6. **Attribute access:**
   - `obj.attr` infers type from class definition or property type

### Bidirectional Inference

In some contexts, inference works **from expected type back to expression**:

- **List/dict comprehensions:** `[f(x) for x in items]` in a context expecting `list[T]` infers return type of `f` must be `T`.
- **Function arguments:** Argument inference considers the function's parameter types.
- **Assignment targets:** Type annotations on left-hand side (if present) inform inference on right-hand side.

### Type Compatibility and Unification

1. **Exact match:** Types must match exactly in v0 (no implicit numeric widening, no duck typing).
2. **Union types:** `T | U` is compatible with either `T` or `U`.
3. **Subtyping (deferred):** Class hierarchy subtyping deferred to v1.0 (inheritance via trait composition).

## Union Types

pyrst supports union types using the `|` operator (Python 3.10 syntax):

```python
def process(x: int | str) -> None:
    if isinstance(x, int):
        print(x + 1)  # type-narrowed to int
    else:
        print(x.upper())  # type-narrowed to str
```

### Union Type Rules

1. **Declaration:** `T | U` is a union of `T` and `U`; order is not significant.
2. **No implicit unions:** Operations on union types are not allowed unless type-narrowed (deferred to v0.2).
3. **Pattern matching:** `match` statements can narrow union types (deferred to v0.2).

## Type Aliases

Type aliases allow reusing complex type expressions:

```python
Vector = list[float]
Dict = dict[str, int]

def process(v: Vector) -> Dict:
    ...
```

Type aliases are purely syntactic; at compile time, they are substituted with their definitions.

## Generic Types (v0: Nominal Monomorphic)

pyrst supports generic function and class definitions, but **only nominal generics** (parameters must be type names, not arbitrary types in v0):

```python
def identity[T](x: T) -> T:
    return x

class Box[T]:
    value: T

    def get(self) -> T:
        return self.value
```

### Generic Type Rules

1. **Monomorphization:** Each unique instantiation (e.g., `identity[int]`, `identity[str]`) generates a new compiled version.
2. **Type parameter bounds (deferred):** Constraints like `T: Comparable` deferred to v1.0.
3. **Variance (deferred):** Covariance/contravariance deferred to v1.0.
4. **Type parameter scope:** Generic parameters are in scope throughout their definition and instantiations.

## Special Types

### Optional Types

```python
def find(items: list[int], value: int) -> int | None:
    # Returns the value if found, or None
    for i, item in enumerate(items):
        if item == value:
            return i
    return None
```

`T | None` represents an optional value. Pattern matching or explicit checks are needed to extract the value (v0.2+).

### Never Type (deferred)

A `Never` or `NoReturn` type (for functions that always raise or infinite-loop) is deferred to v1.0.

## Type Checking Algorithm

The type checker runs in two phases:

1. **Name resolution and import graph construction:**
   - Collect all definitions (functions, classes) and their signatures.
   - Resolve import statements and build the module dependency graph.
   - Check for duplicate definitions and circular imports.

2. **Type checking of function bodies:**
   - For each function, check that all statements and expressions type-check.
   - Emit diagnostics for type mismatches, unresolved names, arity errors, etc.
   - Infer types of local variables where possible; require explicit types at boundaries.

## Diagnostics

When type checking fails, the compiler emits a diagnostic with:

- **Source location:** File, line, column.
- **Severity:** Error (blocking), warning (non-blocking).
- **Message:** Human-readable explanation of the type error.
- **Snippet:** Source code excerpt with the error highlighted.

Example diagnostic:

```
error: type mismatch
  expected: int
  found: str
  at examples/hello.py:5:10
    5 |    x: int = "hello"
        |           ^^^^^^^
```

## Type Coercion and Implicit Conversions

pyrst does **not** implicitly coerce types. If a value of type `str` is required but an `int` is provided, it is a compile error. Explicit conversions use built-in functions:

- `int(x)` converts `x` to `int` (if possible).
- `float(x)` converts `x` to `float`.
- `str(x)` converts `x` to `str`.
- `bool(x)` converts `x` to `bool` (deferred; use `x != 0` or `x != ""`).

## Type Checking Special Cases

### Truthiness (deferred)

In `if` conditions and `while` loops, any value is currently treated as truthy. Explicit boolean expressions are required (e.g., `if x != 0:` instead of `if x:`). This is deferred for reconsideration in v0.2.

### Callable Objects (deferred)

In v0, only named functions are callable. Function pointers, lambda expressions, and callable objects are deferred to v1.0.

### Instance Checks (deferred)

`isinstance(x, T)` is a built-in function for type narrowing (deferred to v0.2). Pattern matching in `match` statements will serve this purpose in v0.2.

## Error Categories

| Category | Example | Handling |
|---|---|---|
| Undeclared name | `y = x + 1` but `x` not defined | Compile error (blocking) |
| Type mismatch | `x: int = "hello"` | Compile error |
| Arity mismatch | `f(1, 2, 3)` but `f` takes 2 args | Compile error |
| Attribute not found | `x.foo` but `x` has no `foo` | Compile error |
| Invalid operator | `"hello" + 3` | Compile error |

## Roadmap

- **v0.1:** Dunder method lowering, more standard library methods.
- **v0.2:** Type narrowing, `isinstance()`, pattern matching in `match` statements.
- **v1.0:** Generics with bounds, subtyping, callable types, `Any`/`Dynamic`, gradual typing.
