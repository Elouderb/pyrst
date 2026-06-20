# pyrst Python Compatibility Matrix

This document clarifies which Python features are supported, partially supported, or intentionally unsupported in pyrst.

> Every row below was verified against the actual compiler at Phase 38 (the AST, codegen, and/or a real `pyrst build` of a minimal program). pyrst is a **statically-typed subset of Python that compiles to Rust** — it is "Python-like," not "Python-compatible." See the Design Philosophy at the end.

---

## Syntax and Basic Constructs

| Feature | Status | Notes |
|---------|--------|-------|
| Indentation-based blocks | ✅ Supported | Full support |
| Comments (`#`) | ✅ Supported | Line comments only |
| Docstrings | ⚠️ Parsed | Tokenized but not preserved |
| Function definitions | ✅ Supported | Requires type annotations |
| Class definitions | ✅ Supported | Single inheritance |
| Variable assignment | ✅ Supported | Requires type consistency |
| Type annotations | ✅ Supported | `x: int`, `def f() -> str` |
| Union types | ✅ Supported | `T \| None` syntax (maps to `Option<T>`) |

---

## Data Types

| Type | Status | Backing | Notes |
|------|--------|---------|-------|
| `int` | ✅ Supported | `i64` | 64-bit signed integers only |
| `float` | ✅ Supported | `f64` | IEEE 754 double precision |
| `str` | ✅ Supported | `String` | Owned UTF-8 strings; `len`/indexing are char-based |
| `bool` | ✅ Supported | `bool` | True/False |
| `None` | ✅ Supported | `Option<T>::None` | Only in optional types |
| `list[T]` | ✅ Supported | `Vec<T>` | Homogeneous, mutable |
| `dict[K, V]` | ✅ Supported | `HashMap<K, V>` | Hash-based mapping |
| `tuple[T, ...]` | ✅ Supported | Rust tuple `(T, ...)` | Fixed-size, heterogeneous |
| `set[T]` | ✅ Supported | `HashSet<T>` | Literals, comprehensions, membership, iteration — but **not** the mutation/algebra methods (see Set Methods) |
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
| Keyword arguments | ✅ Supported | Pass by name at call sites |
| Default arguments | ✅ Supported | `def f(x: int = 5)` |
| `*args` | ❌ Not Supported | Variadic arguments not supported |
| `**kwargs` | ❌ Not Supported | Keyword unpacking not supported |
| Lambda expressions | ✅ Supported | `lambda x: x + 1` |
| Closures | ✅ Supported | Capture enclosing variables (by value) |
| Decorators | ⚠️ Partial | `@dataclass`, `@staticmethod`, `@property` work; arbitrary/user decorators do not |
| Type hints | ✅ Supported | Mandatory, checked at compile time |
| Forward references | ✅ Supported | Two-pass type checking enables this |

---

## Classes and Objects

| Feature | Status | Notes |
|---------|--------|-------|
| Class definition | ✅ Supported | Compiles to Rust struct + `impl` |
| Instance attributes | ✅ Supported | Must be typed |
| Methods | ✅ Supported | Can modify `self` |
| `self` parameter | ✅ Supported | Required first parameter |
| `__init__` constructor | ✅ Supported | User-defined `__init__` honored |
| Inheritance (single) | ✅ Supported | `class Derived(Base):` |
| `super()` | ✅ Supported | Calls base-class methods |
| Operator overloading | ✅ Supported | `__add__`, `__eq__`, `__lt__`, `__str__`, etc. |
| `@property` | ✅ Supported | Computed read-only attributes |
| `@staticmethod` | ✅ Supported | No-`self` methods |
| `@classmethod` | ⚠️ Limited | `cls` requires a type annotation pyrst cannot express cleanly |
| Class variables | ❌ Not Supported | Only instance attributes |
| Inheritance (multiple) | ❌ Not Supported | Single inheritance only |
| Monkey patching | ❌ Not Supported | Classes are immutable |
| Dynamic attribute access | ❌ Not Supported | No runtime `getattr`/`setattr` |
| Metaclasses | ❌ Not Supported | Not part of the type system |

**Key Semantic Difference:** Classes use **value semantics** (Rust), not reference semantics (Python). Assignment copies the struct.

---

## Control Flow

| Feature | Status | Notes |
|---------|--------|-------|
| `if`/`elif`/`else` | ✅ Supported | Full support |
| Ternary operator (`a if c else b`) | ✅ Supported | Conditional expression; both branches must share a type; right-associative |
| `while` loops | ✅ Supported | Full support |
| `for` loops | ✅ Supported | Over list/set/dict/str/`range`; supports tuple unpacking |
| `for`/`else` | ❌ Not Supported | `else` block not supported |
| `break` | ✅ Supported | Exits loop |
| `continue` | ✅ Supported | Skips iteration |
| `pass` | ✅ Supported | No-op placeholder |
| `return` | ✅ Supported | Type checked |
| Pattern matching (`match`/`case`) | ✅ Supported | Literal and `_` patterns |

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
| **Comparison Chaining** | `a < b < c` | ✅ Supported | Python semantics (`a < b and b < c`) |

---

## Built-in Functions

| Function | Status | Notes |
|----------|--------|-------|
| `print()` | ✅ Supported | Scalars and strings; **not** raw collections (see limitations) |
| `len()` | ✅ Supported | Sequences/mappings; char count for `str` |
| `range()` | ✅ Supported | `range(n)`, `range(a, b)`, `range(a, b, step)` |
| `enumerate()` | ✅ Supported | Yields `(index, value)` tuples |
| `zip()` | ✅ Supported | Zips two iterables |
| `int()`, `float()`, `str()`, `bool()` | ✅ Supported | Type conversions (`str()` on collections unsupported) |
| `list()`, `dict()`, `set()`, `tuple()` | ✅ Supported | Constructors |
| `sorted()` | ✅ Supported | Returns a new list |
| `min()`, `max()`, `sum()`, `abs()` | ✅ Supported | Numeric builtins |
| `isinstance()` | ⚠️ Limited | Compiles; limited utility under static typing |
| `type()` | ⚠️ Limited | Compiles; no general runtime type objects |
| `input()` | ✅ Supported | Reads a line from stdin |
| `hasattr()` | ❌ Not Supported | No dynamic attribute checking |
| `getattr()` / `setattr()` | ❌ Not Supported | No dynamic attribute access |
| `eval()` / `exec()` | ❌ Not Supported | No dynamic code execution |
| `open()` / file I/O | ❌ Not Supported | No file I/O yet |
| `map()`, `filter()`, `reduce()` | ❌ Not Supported | First-class function values unsupported; use comprehensions |

---

## String Methods

A broad surface is supported. Representative coverage:

| Method | Status | Notes |
|--------|--------|-------|
| `.upper()` / `.lower()` | ✅ Supported | Returns new string |
| `.strip()` / `.lstrip()` / `.rstrip()` | ✅ Supported | Whitespace trimming |
| `.split(sep)` / `.rsplit()` / `.splitlines()` | ✅ Supported | Returns `list[str]` |
| `.replace()` | ✅ Supported | Returns new string |
| `.find()` / `.rfind()` / `.index()` / `.count()` | ✅ Supported | Returns `int` |
| `.startswith()` / `.endswith()` | ✅ Supported | Returns `bool` |
| `.join()` | ✅ Supported | Joins an iterable of strings |
| `.capitalize()` / `.title()` / `.swapcase()` / `.zfill()` | ✅ Supported | Returns new string |
| `.isdigit()` / `.isalpha()` / `.isspace()` / `.isalnum()` … | ✅ Supported | Predicate methods returning `bool` |
| `s[i]` indexing | ✅ Supported | Returns a single character (char-based) |
| f-strings | ✅ Supported | Interpolated expressions are compiled |
| `.format()` | ❌ Not Supported | Use f-strings instead |

---

## List Methods

| Method | Status | Notes |
|--------|--------|-------|
| `.append()` / `.extend()` / `.insert()` | ✅ Supported | In-place mutation |
| `.remove()` / `.clear()` | ✅ Supported | In-place removal |
| `.sort()` / `.reverse()` | ✅ Supported | In-place reordering |
| `.index()` / `.count()` | ✅ Supported | Returns `int` |
| `.copy()` | ✅ Supported | Shallow copy |
| `.pop()` / `.pop(i)` | ✅ Supported | `pop()` removes/returns the last element; `pop(i)` removes by index |
| `list[i]` / `list[i] = val` | ✅ Supported | Index access / assignment |
| List slicing (`list[1:3]`) | ✅ Supported | Returns a new list |

---

## Dictionary Methods

| Method | Status | Notes |
|--------|--------|-------|
| `dict[key]` / `dict[key] = val` | ✅ Supported | Direct access / assignment |
| `key in dict` | ✅ Supported | Membership test |
| `.get(key, default)` | ✅ Supported | Safe key lookup |
| `.keys()` / `.values()` | ✅ Supported | Iterable in a `for` loop |
| `.pop(key)` / `.clear()` / `.copy()` | ✅ Supported | `pop` takes an explicit key |
| `.items()` | ✅ Supported | `for k, v in d.items()` iterates key/value pairs |
| `.update()` | ✅ Supported | Merges another mapping in place |

---

## Set Methods

| Method | Status | Notes |
|--------|--------|-------|
| `.add()` / `.clear()` | ✅ Supported | In-place mutation |
| `.discard()` / `.remove()` | ⚠️ Supported | In-place removal — but neither raises `KeyError` on an absent element (unlike Python) |
| `.update()` | ✅ Supported | Adds all elements of another set |
| `.union()` / `.intersection()` / `.difference()` / `.symmetric_difference()` | ✅ Supported | Returns a new set |
| `.issubset()` / `.issuperset()` / `.isdisjoint()` | ✅ Supported | Returns `bool` |

---

## Error Handling

| Feature | Status | Notes |
|---------|--------|-------|
| `assert` | ✅ Supported | Maps to Rust `assert!` |
| `raise` | ✅ Supported | Maps to `panic!` with a typed payload |
| `try`/`except` | ✅ Supported | Matches on exception type; builtin class hierarchy |
| `except E as e` | ✅ Supported | Binds the exception message (`str`) |
| `else` / `finally` | ✅ Supported | Both clauses honored |
| Custom exception classes | ⚠️ Partial | Can define/raise (`class MyErr(Exception)`); caught by exact type name (no user-defined subclass hierarchy) |
| `raise ... from ...` | ❌ Not Supported | Exception chaining not supported |

See `DESIGN_DECISIONS.md` §11 and `RUST_BACKEND.md` for the `catch_unwind` lowering.

---

## Comprehensions and Iterators

| Feature | Status | Notes |
|---------|--------|-------|
| List comprehensions | ✅ Supported | `[x*2 for x in items if x > 0]` |
| Set comprehensions | ✅ Supported | `{x for x in items}` |
| Dict comprehensions | ✅ Supported | `{k: v for k, v in pairs}` |
| Generator expressions | ❌ Not Supported | Use comprehensions |
| `for`/`else` | ❌ Not Supported | `else` block not supported |

---

## Imports and Modules

| Feature | Status | Notes |
|---------|--------|-------|
| `import foo` | ✅ Supported | Multi-file compilation |
| `from foo import bar` | ✅ Supported | Named imports |
| `from foo import bar as baz` | ✅ Supported | Aliased imports |
| Multi-file programs | ✅ Supported | DFS import resolution, flat namespace merge |
| Circular imports | ⚠️ Detected | Reported via cycle detection, not resolved |
| Package structure | ❌ Not Supported | No package hierarchy |
| Relative imports | ❌ Not Supported | Not yet implemented |
| Side effects at import | ❌ Not Supported | Modules are declarations only |
| Python stdlib imports | ❌ Not Supported | No Python library integration |

---

## Advanced Features

| Feature | Status | Notes |
|---------|--------|-------|
| Context managers / `with` | ✅ Supported | `with X() as y:` |
| Operator overloading | ✅ Supported | Dunder methods (see Classes) |
| Generators / `yield` | ❌ Not Supported | Use lists instead |
| Coroutines / `async` / `await` | ❌ Not Supported | Not in current roadmap |
| `global` / `nonlocal` | ❌ Not Supported | No module-level mutable state rebinding |
| Decorators (general) | ⚠️ Partial | Only `@dataclass`/`@staticmethod`/`@property` |
| Descriptors | ❌ Not Supported | Not part of the object model |
| Metaclasses | ❌ Not Supported | Not supported |
| Reflection (`inspect`) | ❌ Not Supported | No runtime introspection |
| Multiple inheritance | ❌ Not Supported | Single inheritance only |
| Abstract base classes | ❌ Not Supported | No ABC support |
| `typing` module metadata | ⚠️ Partial | Static types enforced; no runtime metadata |

---

## Notable Limitations

- **Printing collections:** `print([...])`, `print({...})`, and `str([...])` are unsupported — the backing `Vec`/`HashMap`/`HashSet` is not `Display`. Print elements individually, or build a string with `", ".join(...)`.
- **No first-class function values to builtins:** e.g. `map(str, xs)` does not work; use a comprehension.
- **`@classmethod`:** the `cls` parameter cannot be cleanly annotated, so classmethods are effectively unsupported (use `@staticmethod` or a module function).
- **Caught exceptions** print no stderr noise; uncaught ones still surface a message and a non-zero exit code.

---

## Design Philosophy

pyrst aims to be **"Python-like with Rust compilation"** rather than **"Python-compatible."**

The compatibility strategy:
1. ✅ Support the most common, frequently-used Python patterns
2. ✅ Provide clear error messages for unsupported patterns
3. ❌ Do not attempt to emulate Python's dynamic behavior
4. ❌ Do not support features that conflict with static typing or Rust idioms

The dynamic half of Python — metaclasses, monkey-patching, `eval`/`exec`, generators/`async`, `*args`/`**kwargs`, reflection, and the full stdlib — is intentionally out of scope; it is fundamentally incompatible with mandatory static typing and ahead-of-time compilation to Rust.

---

*Last updated: June 20, 2026*  
*Phase: 38 — verified against the live compiler (112/112 examples passing)*
