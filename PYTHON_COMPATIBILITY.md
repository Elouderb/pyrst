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

**Key Semantic Difference:** Classes (and all non-`Copy` values) use **value semantics** (Rust), not reference semantics (Python). Assignment and argument passing **deep-copy** the value (clone-on-use) — there is no shared-mutable aliasing. A callee that should mutate the caller's object opts in explicitly with a `Mut[T]` (by-reference) parameter; otherwise mutating a by-value parameter is a compile error. See *Notable Limitations* for the full model.

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
| `print()` | ✅ Supported | Scalars, strings, and collections (CPython-style repr) |
| `len()` | ✅ Supported | Sequences/mappings; char count for `str` |
| `range()` | ✅ Supported | `range(n)`, `range(a, b)`, `range(a, b, step)` |
| `enumerate()` | ✅ Supported | Yields `(index, value)` tuples |
| `zip()` | ✅ Supported | Zips two iterables |
| `int()`, `float()`, `str()`, `bool()` | ✅ Supported | Type conversions; `str()` of a collection yields its repr |
| `list()`, `dict()`, `set()`, `tuple()` | ✅ Supported | Constructors |
| `sorted()` | ✅ Supported | Returns a new list |
| `min()`, `max()`, `sum()`, `abs()` | ✅ Supported | Numeric builtins |
| `isinstance()` | ⚠️ Limited | Compiles; limited utility under static typing |
| `type()` | ⚠️ Limited | Compiles; no general runtime type objects |
| `input()` | ✅ Supported | Reads a line from stdin |
| `hasattr()` | ❌ Not Supported | No dynamic attribute checking |
| `getattr()` / `setattr()` | ❌ Not Supported | No dynamic attribute access |
| `eval()` / `exec()` | ❌ Not Supported | No dynamic code execution |
| `open()` / file I/O | ⚠️ MVP | `open(path[, mode])` with `with`; `read()`/`readlines()`/`write()`/`close()`; modes `r`/`w`/`a`. No `for line in f`, seek/tell, binary, or encoding; I/O errors panic |
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

## Optional / None Semantics

`Optional[T]` and the equivalent `T | None` annotation both lower to Rust
`Option<T>`; the bare `None` literal lowers to `Option::None`. The model is
deliberately explicit (no implicit `Option`-to-`T` coercion) so a missing value
can never be read as if it were present.

**What is accepted (auto-wrapping at the boundary):**

| Pattern | Example | Lowers to |
|---------|---------|-----------|
| `None` into an Optional slot | `x: Optional[int] = None` | `let x: Option<i64> = None;` |
| bare `T` into an Optional slot (auto-`Some`) | `x: Optional[int] = 5` | `let x: Option<i64> = Some(5);` |
| `Optional[T]` into an `Optional[T]` slot | `y: Optional[int] = f()` | passed through unchanged |
| `return None` / `return 5` in an `-> Optional[int]` fn | | `return None;` / `return Some(5);` |
| bare `T` / `None` as an `Optional[T]` **function** argument | `f(5)`, `f(None)` | `f(Some(5))`, `f(None)` |

The auto-`Some` / `None` wrapping happens at the **consuming site** (annotated
assignment, `return` in an Optional-returning function, and arguments to a
**named function** whose parameter is `Optional[T]`). Method parameters and
class-constructor fields do not yet auto-wrap — pass an explicit `Optional`
value there.

**Narrowing — the only way to use the inner value.** A value of type
`Optional[T]` supports exactly two operations directly: testing it with
`x is None` / `x is not None` (and `==`/`!=` against `None`). To use the inner
`T` you must narrow with a None-guard; inside the narrowed branch the name has
type `T` and lowers to `x.unwrap()`:

```python
def double_or_zero(x: Optional[int]) -> int:
    if x is not None:
        return x * 2        # x is `int` here
    return 0

def describe(x: Optional[int]) -> str:
    if x is None:
        return "none"
    else:
        return "value " + str(x)   # x is `int` in the else branch
```

`if x is not None:` narrows in the *then* branch; `if x is None:` narrows in the
*else* branch (when there is no intervening `elif`). The narrowing is scoped to
that branch only and does not leak past the `if`.

**Honest rejection (chosen semantics).** Using an `Optional[T]` as a bare `T`
**without narrowing** is a hard typeck error — it is never silently miscompiled.
Any operator other than the None-identity tests above (`is`/`is not`/`==`/`!=`)
applied to a raw Optional operand is rejected:

```python
def add_one(x: Optional[int]) -> int:
    return x + 1        # ERROR: operator on an Optional value requires
                        # narrowing first — use `if x is not None:`
```

This is the deliberate trade-off: pyrst will refuse the program rather than
emit code that could dereference a `None`. Narrow first.

The literal `None` is the *only* thing that fills an Optional slot on its own.
The **result of a void function** (a `def f() -> None` call, or a built-in like
`print(...)` / `list.append(...)`) is *not* a value and is rejected when used as
an `Optional[T]`:

```python
def sink() -> None:
    print("hi")

def use() -> None:
    x: int | None = sink()   # ERROR: declared Option(Int), got Unit —
                             # a void result is not `None` and not a value
```

The type checker keeps the `None` *literal* and a *void return* as separate
types precisely so this case is caught at `pyrst check`, not deferred to the Rust
compiler (which would otherwise reject the emitted `Some(sink())` as `Option<()>`).

---

## Notable Limitations

- **Printing collections:** `print([...])`, `print({...})`, `str([...])`, and f-string interpolation render lists/tuples/sets/dicts in CPython `repr` form (str elements quoted, bools as `True`/`False`, nested collections recursing). Because the backing `HashSet`/`HashMap` have no insertion order, **set and dict entries are emitted in a stable sorted-by-`repr` order**, which may differ from Python's insertion order. Empty collections render as `[]`, `set()`, and `{}`; str elements are quoted with single quotes and escaped. Tuples up to 6 elements are covered. Dict views (`keys()`/`values()`/`items()`) and set/list method results (`union()`, `copy()`, …) carry their collection type and render via the same repr path; multi-key dict-view order is unspecified.
- **No first-class function values to builtins:** e.g. `map(str, xs)` does not work; use a comprehension.
- **`@classmethod`:** the `cls` parameter cannot be cleanly annotated, so classmethods are effectively unsupported (use `@staticmethod` or a module function).
- **Caught exceptions** print no stderr noise; uncaught ones still surface a message and a non-zero exit code.
- **Mutating through a subscript does not persist:** indexing yields a value (a clone, per the value-semantics model), so `local[k].append(x)` or `matrix[i][j] = v` on a *nested* collection of a **local** mutates a temporary, not the stored element. Pull the element into a variable, mutate it, and reassign the whole element (`row = matrix[i]; row[j] = v; matrix[i] = row`). (When the subscripted collection is rooted at a **by-value parameter**, this is no longer a silent no-op but a hard compile error — see the by-value-parameter bullet below; use `Mut[T]` to mutate the caller's value in place.)
- **Mutating a by-value non-Copy parameter is a compile error:** pyrst compiles a plain (by-value) parameter to an owned Rust value — a *deep clone* of the caller's value, taken at the call site (clone-on-use). The callee therefore mutates its own copy, and the change is NOT visible to the caller. Rather than let that miscompile silently, the typeck pass rejects every mutation of a by-value non-`Copy` (`list`, `dict`, `set`, `str`, or user-defined class) parameter — whether the mutation is **direct** or reaches **through a field or index** of the parameter:
  1. Field assignment — `param.field = v`
  2. Index assignment — `param[k] = v`
  3. In-place mutating method on the param **or on any place rooted at it** — `param.append(x)`, `param.add(x)`, `param.update(d)`, **and** `param.field.append(x)`, `param[0].add(x)`, `param.a.b.sort()`, etc. (the mutating methods are the 13 in-place list/set/dict mutators: `append`, `extend`, `insert`, `remove`, `sort`, `reverse`, `clear`, `add`, `discard`, `update`, `pop`, `setdefault`, `popitem`).

  The nested case (`param.field.append(x)`) used to compile and silently produce wrong output; it is now a loud error like the rest. The error always names the remedy:

  ```text
  mutation of by-value parameter `ds` is not visible to the caller;
  mutate via a method on it or return the updated value;
  or declare the parameter `Mut[T]` to mutate it in place
  ```

  You have three remedies:
  - **(a) Declare the parameter `Mut[T]`** — opt into by-reference mode so the mutation persists to the caller (see the next bullet). This is the most direct fix for "the callee should mutate the caller's object."
  - **(b) Return the updated value** and let the caller reassign:
    ```python
    # WRONG — mutation invisible to caller
    def push(items: list[int], x: int) -> None:
        items.append(x)              # compile error: by-value param
    # CORRECT — return the new value
    def push(items: list[int], x: int) -> list[int]:
        result = list(items)
        result.append(x)
        return result
    ```
  - **(c) Make it a method on `self`** (for state owned by a class) — a mutating method takes `&mut self`, so `self.values.append(x)` is fine.

  > A param that is *reassigned* before mutation (`p = ...; p.append(x)`) or that *flows into a `return`* (`xs.append(x); return xs`) is exempt — in both cases the mutation is the callee's own value, not a lost write.

- **Opt-in by-reference parameters — `Mut[T]`:** annotate a parameter `Mut[T]` to pass it **by mutable reference** (`&mut T` in the emitted Rust) instead of by value. The callee's mutations to a `Mut[T]` parameter — direct, nested, or via a mutating method — **persist to the caller**, and the by-value backstop above is suppressed for that parameter.

  ```python
  class Account:
      balance: int
      def __init__(self, balance: int) -> None:
          self.balance = balance

  # `account` is borrowed &mut Account; the deposit is visible to the caller.
  def deposit(account: Mut[Account], amt: int) -> None:
      account.balance = account.balance + amt

  def main() -> None:
      a: Account = Account(100)
      deposit(a, 25)
      deposit(a, 5)
      print(a.balance)   # 130 — the mutation persisted
  ```

  It composes with the nested case the backstop now guards. The graph/DFS shape — fill the caller's set in place — is written by declaring the collection `Mut[...]`:

  ```python
  def visit(seen: Mut[set[int]], node: int) -> None:
      seen.add(node)            # persists to the caller's set

  def record(ds: Mut[DataSet], x: int) -> None:
      ds.values.append(x)       # nested field mutation, now legal via Mut[T]
  ```

  Rules and limits:
  - **Place requirement:** a `Mut[T]` argument must be a **place** — a variable, field, or index (`deposit(a, 5)`), never a temporary. `deposit(make_account(), 5)` is an honest typeck error (*"by-reference parameter `account` requires a variable, not a temporary"*): a temporary has no caller-visible storage to borrow.
  - **Parameter-only:** `Mut[T]` is a parameter *mode*, not a type. It is rejected anywhere else — return types, variable/field annotations, or nested forms like `list[Mut[T]]` (*"Mut[...] is only valid on a parameter"*).
  - **The aliasing trade (the conscious price of not using `Rc`):** `&mut` forbids aliasing, so passing the **same** variable as two `Mut[T]` arguments — or as a `Mut[T]` arg while it is also borrowed elsewhere in the same call — surfaces an **honest Rust borrow-check error**, never silent-wrong output and never a runtime aliasing panic. Python permits such aliasing; pyrst deliberately does not. Rewrite by **sequencing** the mutations or by **return-and-reassign**:
    ```python
    # REJECTED — `acc` aliased as two &mut args at once
    transfer(acc, acc, 10)
    # OK — sequence the two mutations instead
    withdraw(acc, 10)
    deposit(acc, 10)
    ```
  - **`Mut[set]` / `Mut[dict]` need element types:** write `Mut[set[int]]` / `Mut[dict[str, int]]`, not bare `Mut[set]` — a bare `set`/`dict` head parses as an (unknown) class, so the argument-type check rejects the call.
  - **`Mut[<primitive>]` has a known deref limitation:** `Mut[int]`/`Mut[float]`/`Mut[bool]` emit `&mut i64` etc., but the codegen does not auto-dereference the reference in expression position, so arithmetic on the parameter (`n + 1`) fails to compile, and reassigning the parameter would not write back anyway. Use a `Mut[T]` of a collection or class, or the return idiom, for primitives.
- **Block scope follows Python:** a variable first assigned inside an `if`/`elif`/`else`/`for`/`while`/`with`/`try` body is visible after the block (it is hoisted to function scope). Edge case: a name is not hoisted — and so stays block-local — if its type cannot be statically inferred, or is a tuple or an all-numeric-field class (which has no `Default`). Also: a hoisted name is initialized to a default (`0`/`""`/empty), so reading it on a path where it was never assigned yields that default rather than raising Python's `UnboundLocalError`.
- **No subtype polymorphism:** classes compile to plain Rust structs with value semantics and no inheritance relationship, so a `list[Base]` cannot hold `Derived` instances and a `Base`-typed variable cannot be reassigned a `Derived`. Use a single concrete type per collection. (`super()`, method inheritance, and overriding within one type still work.)
- **Builtin runtime errors are not catchable exceptions:** index-out-of-range, missing dict key, and divide-by-zero abort the program (Rust panic, non-zero exit). `try`/`except` only catches values from an explicit `raise` of the matching type — a bare `except IndexError` will **not** catch an out-of-bounds subscript.

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
