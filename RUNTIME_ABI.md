# pyrst Runtime ABI

## Overview

This document defines the Application Binary Interface (ABI) for pyrst's runtime: object model, calling conventions, memory layout, and C interop.

## Object Model

### Value Types (Stack-Allocated)

These types are small, fixed-size, and stored directly on the stack:

| Type | Rust Mapping | Size | Layout | Notes |
|---|---|---|---|---|
| `int` | `i32` or `i64` | 4 or 8 bytes | 2's complement | Signedness: signed (like Python `int`) |
| `float` | `f64` | 8 bytes | IEEE 754 | Double precision |
| `bool` | `bool` | 1 byte | `0x00` (false), `0x01` (true) | No boolean object in Python; native Rust bool |
| `None` | `()` | 0 bytes | No data | Unit type; no runtime representation |

### Reference Types (Heap-Allocated)

These types are heap-allocated and managed via reference counting or garbage collection:

| Type | Rust Mapping | Heap Layout | Notes |
|---|---|---|---|
| `str` | `String` or `&str` | Header + data | String metadata (length, capacity if mutable); UTF-8 encoded |
| `list[T]` | `Vec<T>` | Header + element array | Dynamic array; length and capacity in header |
| `dict[K, V]` | `HashMap<K, V>` | Hash table | Load factor, hash array, bucket list |
| User class | `struct { fields... }` | Fields in order | Flat layout; no vtable in v0 (no dynamic dispatch) |

### Class Instance Layout

A class instance is laid out as a Rust struct with fields in declaration order:

```python
class Point:
    x: float
    y: float
    name: str
```

Rust layout:

```rust
struct Point {
    x: f64,
    y: String,
    name: String,
}
```

Fields are in the same order as declared. No padding or alignment beyond what Rust's default struct layout provides.

### String Representation

- **Immutable by default:** `str` maps to Rust's `String` (which is mutable but treated as immutable in pyrst).
- **Encoding:** UTF-8 only in v0. Non-UTF-8 sequences are not supported.
- **Length:** Strings are length-prefixed (header contains byte length).

### Container Representation

- **Lists (`list[T]`):** Homogeneous arrays backed by `Vec<T>`. Length and capacity are tracked by `Vec`.
- **Dicts (`dict[K, V]`):** Hash maps backed by `HashMap<K, V>`. Hash function is Rust's default (SipHash).
- **Tuples (`tuple[...]`):** Fixed-length tuples backed by Rust tuples. Heterogeneous.

## Calling Convention

### Function Call ABI

pyrst functions follow Rust's calling convention (which varies by platform but is standardized for `rustc`):

1. **Parameter passing:**
   - Small types (int, float, bool): pass by value in registers.
   - Large types (str, containers, class instances): pass by reference (shared borrow `&T` or mutable borrow `&mut T`).

2. **Return values:**
   - Small types: return by value in registers.
   - Large types: return by value (moved into caller's stack frame or hidden allocation).

3. **Self parameter (methods):**
   - Instance methods receive `&self` (shared borrow) by default.
   - Methods that mutate receive `&mut self` (mutable borrow), if supported in future versions.

4. **Implicit parameters:**
   - None in v0. Python's implicit `self` is explicit in Rust (`&self`).

### Function Signature Examples

```python
# pyrst
def add(x: int, y: int) -> int:
    return x + y

def append_name(names: list[str], name: str) -> None:
    names.append(name)
```

Rust equivalents:

```rust
fn add(x: i32, y: i32) -> i32 {
    x + y
}

fn append_name(names: &mut Vec<String>, name: &String) -> () {
    names.push(name.clone())  // or names.push(name.to_string())
}
```

### Method Call ABI

Instance methods receive `self` as the first implicit parameter. Method dispatch is static (compile-time resolved in v0):

```python
class Point:
    x: float
    y: float

    def distance_from_origin(self) -> float:
        return (self.x ** 2 + self.y ** 2) ** 0.5

p = Point(x=3.0, y=4.0)
d = p.distance_from_origin()
```

Lowering to Rust:

```rust
impl Point {
    fn distance_from_origin(&self) -> f64 {
        ((self.x * self.x) + (self.y * self.y)).sqrt()
    }
}

let p = Point { x: 3.0, y: 4.0 };
let d = p.distance_from_origin();  // Desugar to Point::distance_from_origin(&p)
```

## Ownership and Memory Management

### Ownership Rules (Conservative v0 Implementation)

1. **Value types (int, float, bool) are `Copy`:** They are copied on assignment and function call.
2. **Reference types (str, containers, classes) are moved by default:** They transfer ownership on assignment.
3. **Cloning is explicit or inferred:** If ownership inference detects that a value is used multiple times, it inserts a `clone()` call.
4. **Borrows are inferred:** If a value is passed to a function that doesn't consume it, the compiler infers a `&` borrow.

### Reference Counting (Deferred)

In v0, reference counting is **not explicitly modeled**. The compiler may wrap types in `Rc<T>` or `Arc<T>` if needed (e.g., for shared mutable containers), but this is internal to the codegen and not visible to the user.

Full reference counting support is deferred to v1.0.

### Garbage Collection (Not Supported)

pyrst does not use garbage collection. Memory is managed via Rust's ownership system and explicit deallocation (RAII). Cyclic data structures are deferred to v1.0.

## Exception Handling ABI

### Exception Representation (Deferred)

In v0, exceptions are minimally supported. Exception objects are represented as Rust enums or structs that implement a common trait.

```rust
// Placeholder
pub trait Exception {
    fn message(&self) -> String;
}

#[derive(Debug)]
pub struct ValueError {
    message: String,
}

impl Exception for ValueError { ... }
```

### Raising Exceptions

A `raise` statement panics in Rust (via `panic!`). This is a temporary implementation; v0.2+ will use Rust's standard error types or custom exception types.

### Catching Exceptions (Deferred)

`try`/`except` blocks are deferred to v0.2.

## C ABI Interop

### FFI Strategy

pyrst can call C functions by importing them with explicit signatures. This is deferred to v0.2, but the design is:

```python
# Hypothetical v0.2 syntax
extern "C":
    def c_function(x: int) -> int
    def c_malloc(size: int) -> int  # returns pointer as int

# Call from pyrst
result = c_function(42)
```

The extern declarations are lowered to Rust `extern "C" { ... }` blocks.

### Calling Rust from pyrst

pyrst code can be used from Rust by:
1. Compiling pyrst to an executable (links via `main()`).
2. Generating a Rust library that exposes pyrst functions (deferred).
3. Generating C bindings to pyrst functions (deferred to v1.0).

### Pointer Representation

Pointers are not first-class in pyrst v0. They are deferred to v1.0 or limited to C interop contexts.

## Panic and Error Handling

### Panics

Unrecoverable errors (e.g., division by zero, out-of-bounds access, assertion failures) are handled by panicking. The panic message is printed to stderr.

In v0, all panics cause the program to abort. Stack unwinding and cleanup are deferred.

### Assertions

`assert` statements are deferred to v0.1. They will be compiled to Rust assertions.

## Standard Library Runtime

### Built-in Functions

The following built-in functions are provided at runtime:

| Function | Signature | Behavior | Notes |
|---|---|---|---|
| `print` | `(value: str) -> None` | Print to stdout | Can take multiple args (deferred) |
| `len` | `(container) -> int` | Return length | Works on str, list, dict, etc. |
| `range` | `(start: int, stop: int, step: int) -> range` | Return range object | Supports start, stop, step; step defaults to 1 |
| `int` | `(value: str or float) -> int` | Convert to int | Raises ValueError if invalid |
| `float` | `(value: str or int) -> float` | Convert to float | Raises ValueError if invalid |
| `str` | `(value) -> str` | Convert to str | Calls `__str__` or default repr |
| `bool` | `(value) -> bool` | Convert to bool | Truthiness; deferred to v0.2 |
| `enumerate` | `(iterable) -> enumerate` | Return enumerated iterator | Deferred to v0.1+ |
| `zip` | `(*iterables) -> zip` | Return zipped iterator | Deferred to v0.1+ |

### String Methods

| Method | Type | Behavior |
|---|---|---|
| `s.upper()` | `str -> str` | Return uppercase copy |
| `s.lower()` | `str -> str` | Return lowercase copy |
| `s.split(sep)` | `str -> list[str]` | Split by separator |
| `s.join(items)` | `str -> str` | Join list of strings |
| `s.strip()` | `str -> str` | Remove leading/trailing whitespace |
| `s.startswith(prefix)` | `str -> bool` | Check prefix (deferred) |
| `s.endswith(suffix)` | `str -> bool` | Check suffix (deferred) |
| `s.replace(old, new)` | `str -> str` | Replace substring (deferred) |

### List Methods

| Method | Type | Behavior |
|---|---|---|
| `lst.append(item)` | `list[T] -> None` | Append item to list |
| `lst.pop()` | `list[T] -> T` | Remove and return last item |
| `lst.pop(index)` | `list[T] -> T` | Remove and return item at index (deferred) |
| `lst.extend(items)` | `list[T] -> None` | Extend list with items (deferred) |
| `lst.insert(index, item)` | `list[T] -> None` | Insert item at index (deferred) |
| `lst.remove(item)` | `list[T] -> None` | Remove first occurrence of item (deferred) |
| `lst.clear()` | `list[T] -> None` | Clear list (deferred) |
| `lst.reverse()` | `list[T] -> None` | Reverse list in place (deferred) |
| `lst.sort()` | `list[T] -> None` | Sort list in place (deferred) |
| `lst.copy()` | `list[T] -> list[T]` | Return shallow copy (deferred) |

### Dict Methods

Methods on `dict[K, V]`:

| Method | Type | Behavior |
|---|---|---|
| `d.keys()` | `dict[K, V] -> list[K]` | Return list of keys (deferred) |
| `d.values()` | `dict[K, V] -> list[V]` | Return list of values (deferred) |
| `d.items()` | `dict[K, V] -> list[(K, V)]` | Return list of key-value pairs (deferred) |
| `d.get(key, default)` | `dict[K, V] -> V` | Get value or default (deferred) |
| `d.pop(key)` | `dict[K, V] -> V` | Remove and return value (deferred) |
| `d.clear()` | `dict[K, V] -> None` | Clear dict (deferred) |

## Platform and ABI Variants

### Supported Platforms (v0)

- **Linux x86_64:** Primary target.
- **macOS (Apple Silicon and Intel):** Secondary target.
- **Windows:** Deferred to v0.2+.

### Integer Size

The size of `int` (32-bit vs. 64-bit) is deferred to v0.1. Currently assumed to be platform-dependent (typically `i64`).

### Floating Point

`float` is always 64-bit (IEEE 754 double precision) across all platforms.

## Runtime Initialization

When a pyrst program starts:

1. **Module initialization:** Global code in the main module is executed in declaration order.
2. **Standard library initialization:** Built-in functions and classes are registered.
3. **Entry point:** The `main()` function is called.
4. **Exit:** The return value of `main()` (or `None`) determines the exit code (0 if `None`, 1 if an exception occurred).

## Future Extensions

- **v0.1:** More container methods, proper exception handling.
- **v0.2:** C FFI, reference counting, full exception mechanism.
- **v1.0:** Python interop, Python extension compatibility, full standard library.
