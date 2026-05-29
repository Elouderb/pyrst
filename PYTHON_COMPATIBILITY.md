# pyrst Python Compatibility Matrix

This document clarifies which Python features are supported, partially supported, or intentionally unsupported in pyrst.

---

## Syntax and Basic Constructs

| Feature | Status | Notes |
|---------|--------|-------|
| Indentation-based blocks | ✅ Supported | Full support |
| Comments (`#`) | ✅ Supported | Line comments only |
| Docstrings | ⚠️ Parsed | Tokenized but not preserved |
| Function definitions | ✅ Supported | Requires type annotations |
| Class definitions | ✅ Supported | Single inheritance only |
| Variable assignment | ✅ Supported | Requires type consistency |
| Type annotations | ✅ Supported | `x: int`, `def f() -> str` |
| Union types | ✅ Supported | `T \| None` syntax (maps to `Option<T>`) |

---

## Data Types

| Type | Status | Backing | Notes |
|------|--------|---------|-------|
| `int` | ✅ Supported | `i64` | 64-bit signed integers only |
| `float` | ✅ Supported | `f64` | IEEE 754 double precision |
| `str` | ✅ Supported | `String` | Owned UTF-8 strings |
| `bool` | ✅ Supported | `bool` | True/False |
| `None` | ✅ Supported | `Option<T>::None` | Only in optional types |
| `list[T]` | ✅ Supported | `Vec<T>` | Homogeneous, mutable |
| `dict[K, V]` | ✅ Supported | `HashMap<K, V>` | Hash-based mapping |
| `tuple[T, ...]` | ✅ Supported | Rust tuple `(T, ...)` | Fixed-size, heterogeneous |
| `set` | ❌ Not Supported | N/A | No set type yet |
| `frozenset` | ❌ Not Supported | N/A | No immutable set |
| `bytes` | ❌ Not Supported | N/A | No byte strings |

---

## Functions

| Feature | Status | Notes |
|---------|--------|-------|
| Function definition | ✅ Supported | Requires type annotations |
| Return statements | ✅ Supported | Type checked |
| Recursion | ✅ Supported | Works as expected |
| Positional arguments | ✅ Supported | Order matters |
| Keyword arguments | ⚠️ Partial | Can pass, not declare with defaults |
| Default arguments | ❌ Not Supported | Must provide all arguments |
| `*args` | ❌ Not Supported | Variadic arguments not supported |
| `**kwargs` | ❌ Not Supported | Keyword unpacking not supported |
| Lambda expressions | ❌ Not Supported | Use `def` instead |
| Closures | ❌ Not Supported | Cannot capture enclosing scope |
| Decorators | ⚠️ Parsed | Syntax recognized but not enforced |
| Type hints | ✅ Supported | Mandatory, checked at compile time |
| Forward references | ✅ Supported | Two-pass type checking enables this |

---

## Classes and Objects

| Feature | Status | Notes |
|---------|--------|-------|
| Class definition | ✅ Supported | Compiles to Rust struct |
| Instance attributes | ✅ Supported | Must be typed |
| Methods | ✅ Supported | Can modify `self` |
| `self` parameter | ✅ Supported | Required first parameter |
| Constructor | ⚠️ Partial | No `__init__`; default constructor auto-generated |
| Inheritance (single) | ✅ Supported | `class Derived(Base):` |
| Inheritance (multiple) | ❌ Not Supported | Single inheritance only |
| `super()` | ❌ Not Supported | Call base class by name |
| Class methods | ❌ Not Supported | No `@classmethod` |
| Static methods | ❌ Not Supported | No `@staticmethod` |
| Properties | ❌ Not Supported | No `@property` decorators |
| Class variables | ❌ Not Supported | Only instance attributes |
| `__init__` | ❌ Not Supported | Use field declarations instead |
| Operator overloading | ❌ Not Supported | No `__add__`, `__str__`, etc. |
| Monkey patching | ❌ Not Supported | Classes are immutable |
| Dynamic attribute access | ❌ Not Supported | No `getattr`/`setattr` |
| Metaclasses | ❌ Not Supported | Not part of type system |

**Key Semantic Difference:** Classes use **value semantics** (Rust), not reference semantics (Python). Assignment copies the struct.

---

## Control Flow

| Feature | Status | Notes |
|---------|--------|-------|
| `if`/`elif`/`else` | ✅ Supported | Full support |
| Ternary operator | ❌ Not Supported | Use `if`/`else` statements |
| `while` loops | ✅ Supported | Full support |
| `for` loops | ✅ Supported | Must have iterable |
| `for`/`else` | ❌ Not Supported | `else` block not supported |
| `break` | ✅ Supported | Exits loop |
| `continue` | ✅ Supported | Skips iteration |
| `pass` | ✅ Supported | No-op placeholder |
| `return` | ✅ Supported | Type checked |
| Pattern matching | ❌ Not Supported | No `match`/`case` yet |

---

## Operators

| Category | Feature | Status | Notes |
|----------|---------|--------|-------|
| **Arithmetic** | `+`, `-`, `*`, `/` | ✅ Supported | Integer and float |
| | `//` | ✅ Supported | Floor division |
| | `%` | ✅ Supported | Modulo |
| | `**` | ✅ Supported | Exponentiation |
| **Comparison** | `==`, `!=` | ✅ Supported | Works on all types |
| | `<`, `<=`, `>`, `>=` | ✅ Supported | Works on numbers and strings |
| | `is`, `is not` | ✅ Supported | Identity checks (on None) |
| | `in`, `not in` | ✅ Supported | Membership tests |
| **Logical** | `and`, `or` | ✅ Supported | Short-circuit evaluation |
| | `not` | ✅ Supported | Negation |
| **Bitwise** | `&`, `\|`, `^` | ✅ Supported | Bitwise AND/OR/XOR |
| | `~` | ✅ Supported | Bitwise NOT |
| | `<<`, `>>` | ✅ Supported | Shift operators |
| **Assignment** | `=` | ✅ Supported | Variable binding |
| | `+=`, `-=`, etc. | ✅ Supported | Augmented assignment |
| **Comparison Chaining** | `a < b < c` | ❌ Not Supported | Use `and`: `a < b and b < c` |

---

## Built-in Functions

| Function | Status | Notes |
|----------|--------|-------|
| `print()` | ✅ Supported | Space-separated, newline-terminated |
| `len()` | ✅ Supported | Works on sequences/mappings |
| `range()` | ✅ Supported | `range(n)`, `range(a, b)`, `range(a, b, step)` |
| `enumerate()` | ✅ Supported | Yields `(index, value)` tuples |
| `zip()` | ✅ Supported | Zips two iterables |
| `int()`, `float()`, `str()`, `bool()` | ✅ Supported | Type conversions |
| `list()`, `dict()` | ✅ Supported | Empty collection constructors |
| `type()` | ❌ Not Supported | No runtime type queries |
| `isinstance()` | ❌ Not Supported | Static types, no runtime checks |
| `hasattr()` | ❌ Not Supported | No dynamic attribute checking |
| `getattr()` / `setattr()` | ❌ Not Supported | No dynamic attribute access |
| `eval()` / `exec()` | ❌ Not Supported | No dynamic code execution |
| `input()` | ❌ Not Supported | No stdin support yet |
| `open()` | ❌ Not Supported | No file I/O yet |
| `sorted()` | ❌ Not Supported | No sort function |
| `map()`, `filter()`, `reduce()` | ❌ Not Supported | Use comprehensions instead |

---

## String Methods

| Method | Status | Notes |
|--------|--------|-------|
| `.upper()` | ✅ Supported | Returns new string |
| `.lower()` | ✅ Supported | Returns new string |
| `.strip()` | ✅ Supported | Removes whitespace |
| `.lstrip()` | ✅ Supported | Removes left whitespace |
| `.rstrip()` | ✅ Supported | Removes right whitespace |
| `.split(sep)` | ✅ Supported | Returns `list[str]` |
| `s[i]` | ✅ Supported | Index returns single character |
| `.replace()` | ❌ Not Supported | Not yet implemented |
| `.find()` | ❌ Not Supported | Not yet implemented |
| `.startswith()` / `.endswith()` | ❌ Not Supported | Not yet implemented |
| `.join()` | ❌ Not Supported | Not yet implemented |
| String formatting | ⚠️ Partial | F-strings supported; `.format()` not |

---

## List Methods

| Method | Status | Notes |
|--------|--------|-------|
| `.append()` | ✅ Supported | Adds element to end |
| `.pop()` | ✅ Supported | Removes and returns last |
| `list[i]` | ✅ Supported | Index access |
| `list[i] = val` | ✅ Supported | Index assignment |
| `.remove()` | ❌ Not Supported | Not yet implemented |
| `.insert()` | ❌ Not Supported | Not yet implemented |
| `.sort()` | ❌ Not Supported | Not yet implemented |
| `.reverse()` | ❌ Not Supported | Not yet implemented |
| `.copy()` | ❌ Not Supported | Use explicit cloning |
| List slicing | ❌ Not Supported | `list[1:3]` not supported |

---

## Dictionary Methods

| Method | Status | Notes |
|--------|--------|-------|
| `.get(key, default)` | ✅ Supported | Safe key lookup |
| `.insert(key, value)` | ✅ Supported | Sets key-value |
| `dict[key]` | ✅ Supported | Direct access |
| `.keys()` | ❌ Not Supported | Not yet implemented |
| `.values()` | ❌ Not Supported | Not yet implemented |
| `.items()` | ❌ Not Supported | Not yet implemented |
| `.pop()` | ❌ Not Supported | Not yet implemented |
| `.clear()` | ❌ Not Supported | Not yet implemented |
| `.update()` | ❌ Not Supported | Not yet implemented |

---

## Error Handling

| Feature | Status | Notes |
|---------|--------|-------|
| `assert` | ✅ Supported | Maps to Rust `assert!` macro |
| `raise` | ✅ Supported | Maps to `panic!` |
| `try`/`except` | ⚠️ Parsed | Not yet implemented |
| `finally` | ⚠️ Parsed | Not yet implemented |
| `raise ... from ...` | ❌ Not Supported | Exception chaining not supported |
| Custom exceptions | ❌ Not Supported | Can only raise built-in types |

---

## Comprehensions and Iterators

| Feature | Status | Notes |
|---------|--------|-------|
| List comprehensions | ✅ Supported | `[x*2 for x in items if x > 0]` |
| Dict comprehensions | ❌ Not Supported | Not yet implemented |
| Set comprehensions | ❌ Not Supported | No set type |
| Generator expressions | ❌ Not Supported | Not yet implemented |
| `for`/`else` | ❌ Not Supported | Not yet implemented |

---

## Imports and Modules

| Feature | Status | Notes |
|---------|--------|-------|
| `import foo` | ⚠️ Parsed | Not yet enforced |
| `from foo import bar` | ⚠️ Parsed | Not yet enforced |
| `from foo import bar as baz` | ⚠️ Parsed | Not yet enforced |
| Module files | ❌ Not Supported | All code in single file |
| Package structure | ❌ Not Supported | No package support |
| Relative imports | ❌ Not Supported | Not yet implemented |
| Circular imports | ❌ Not Supported | Design TBD |
| Side effects at import | ❌ Not Supported | Modules are declarations only |
| Python stdlib imports | ❌ Not Supported | No Python library integration |

---

## Advanced Features

| Feature | Status | Notes |
|---------|--------|-------|
| Generators/`yield` | ❌ Not Supported | Use lists instead |
| Coroutines/`async`/`await` | ❌ Not Supported | Not in current roadmap |
| Context managers/`with` | ⚠️ Parsed | Not yet implemented |
| Decorators | ⚠️ Parsed | Syntax recognized but not enforced |
| `global` keyword | ❌ Not Supported | No module-level state |
| `nonlocal` keyword | ❌ Not Supported | No closures |
| Operator overloading | ❌ Not Supported | Cannot define `__add__`, etc. |
| Descriptors | ❌ Not Supported | Not part of object model |
| Metaclasses | ❌ Not Supported | Not supported |
| Reflection (`inspect` module) | ❌ Not Supported | No runtime introspection |
| Multiple inheritance | ❌ Not Supported | Single inheritance only |
| Abstract base classes | ❌ Not Supported | No ABC support |
| Type hints (`typing` module) | ⚠️ Partial | Static types enforced; no runtime metadata |

---

## Summary Statistics

| Category | Total | Supported | Partial | Unsupported |
|----------|-------|-----------|---------|-------------|
| Core Syntax | 8 | 7 | 1 | 0 |
| Data Types | 10 | 7 | 0 | 3 |
| Functions | 10 | 6 | 2 | 2 |
| Classes | 14 | 4 | 1 | 9 |
| Control Flow | 9 | 6 | 0 | 3 |
| Operators | 30+ | 28+ | 0 | 2 |
| Built-ins | 20+ | 8 | 1 | 11+ |
| String Methods | 10 | 6 | 0 | 4 |
| List Methods | 10 | 2 | 0 | 8 |
| Dict Methods | 8 | 2 | 0 | 6 |
| Error Handling | 5 | 2 | 2 | 1 |
| Comprehensions | 5 | 1 | 0 | 4 |
| Advanced | 12 | 0 | 1 | 11 |
| **TOTAL** | **~150** | **~80** | **~8** | **~60** |

**Overall Compatibility:** ~50% of common Python features; focus on core functionality rather than breadth.

---

## Design Philosophy

pyrst aims to be **"Python-like with Rust compilation"** rather than **"Python-compatible."**

The compatibility strategy:
1. ✅ Support the most common, frequently-used Python patterns
2. ✅ Provide clear error messages for unsupported patterns
3. ❌ Do not attempt to emulate Python dynamic behavior
4. ❌ Do not support features that conflict with static typing or Rust idioms

---

*Last updated: May 28, 2026*  
*Phase: 6 (post-review)*
