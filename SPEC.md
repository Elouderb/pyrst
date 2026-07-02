# pyrst Language Specification

**Status:** Current specification, describing the compiler as it actually behaves
today. pyrst is a **statically-typed, Python-like language that compiles to Rust**
— "Python-like," not "Python-compatible." It is the TypeScript-to-JavaScript idea
with Rust as the target instead of JS.

This document is the formal language reference. The companion
[PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md) is the authoritative,
row-by-row support matrix; this SPEC must never contradict it. Where this SPEC
states that a feature is supported, that claim is backed either by a row in
PYTHON_COMPATIBILITY.md or by a passing program in the example corpus
(`examples/*.pyrs` with a golden `examples/expected/*.txt`), and usually both.

**Corpus anchor (the ground truth this spec describes):** 194 passing example
programs (`examples/<name>.pyrs`, each with a golden `examples/expected/<name>.txt`),
plus 64 honest-rejection fixtures (`examples/*fail*.pyrs` — the `fail_*` and
`set_fail_*` files) and one multi-file negative scenario (`examples/multi_file_fail/`).
The full pipeline (lexer → parser → resolver → type checker → Rust codegen →
`rustc`) is exercised by these programs and by 199 in-crate `#[test]` cases.

---

## 1. Design Goals and Non-Goals

### Goals
- Compile statically typed, Python-like programs to readable, idiomatic Rust.
- Preserve Python's ergonomic surface syntax and its *observable* semantics for
  the common case.
- Provide strong compile-time guarantees through mandatory static typing.
- **Fail loudly, never silently miscompile.** When a Python construct cannot be
  faithfully lowered to Rust under the value-semantics model, the compiler emits
  a clean pyrst diagnostic (lex / parse / typeck / codegen) pointing at the
  remedy, rather than emitting wrong-but-compiling Rust or leaking a raw `rustc`
  error. This "honest errors over silent miscompiles" principle is a primary
  design constraint, not an aspiration — see §15.

### Non-Goals
- Full Python compatibility or a drop-in interpreter replacement.
- Dynamic typing, runtime type changes, or an `Any`/`Dynamic` escape hatch.
- Python standard-library compatibility.
- The dynamic half of Python: metaclasses, descriptors, monkey-patching,
  `eval`/`exec`, reflection, `async`, and `*args`/`**kwargs`. These are
  fundamentally incompatible with mandatory static typing and ahead-of-time
  compilation to Rust, and are intentionally out of scope. (Generators are a
  scoped exception: `yield` is supported with lazy semantics — see
  PYTHON_COMPATIBILITY.md's Generators section.)
- Multiple inheritance.

### Intentional Restrictions
- Every binding has a static type — explicitly annotated at boundaries
  (parameters, returns, class attributes, module-level variables) or inferred
  locally inside a function body. There is no implicit `Any`.
- No dynamic attribute access (`getattr`/`setattr`/`hasattr`).
- Classes are fixed structures: their attributes are declared and typed in the
  class body; no runtime attribute injection or class mutation.
- Inheritance is single-level only.
- Functions are not first-class values that can be stored in variables or passed
  to builtins (lambdas/closures exist as call expressions — see §5).

---

## 2. Lexical Structure

### Keywords
```
and     as      assert  async   await   break   case    class
continue def     del     elif    else    except  False   finally
for     from    global  if      import  in      is      lambda
match   None    nonlocal not     or      pass    raise   return
True    try     while   with    yield
```

`async`, `await`, `global`, `nonlocal`, `yield`, and `del` are reserved /
recognized but their corresponding features are not supported (see §15).
`match`/`case` are fully supported (§7).

### Operators
- Arithmetic: `+`, `-`, `*`, `/`, `//`, `%`, `**`
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Logical: `and`, `or`, `not`
- Membership: `in`, `not in`
- Identity: `is`, `is not`
- Bitwise: `&`, `|`, `^`, `~`, `<<`, `>>`
- Assignment: `=`, `+=`, `-=`, `*=`, `/=`, `//=`, `%=`, `**=`, `&=`, `|=`, `^=`,
  `<<=`, `>>=`

### Comments and Docstrings
- Line comments: `# comment text` (consumed by the lexer; trailing comments
  allowed).
- Docstrings are tokenized as ordinary string literals but are **not preserved**
  (no doc retention).

### Whitespace and Indentation
- Indentation-based block structure, Python-style (INDENT/DEDENT/NEWLINE tokens
  on an indentation stack).
- A logical line may continue across physical lines inside `()`/`[]`/`{}` or with
  a trailing backslash; such continuations suppress INDENT/DEDENT generation.
- Blank and comment-only lines do not affect indentation level.

### Literals
- Integers: `123`, `-456`, and the bases `0x2A`, `0o52`, `0b101010` → `i64`.
- Floats: `1.5`, `3.14e-10`, `2.5e3` → `f64`.
- Strings: `"hello"`, `'hello'`, and triple-quoted / multi-line `"""multi
  line"""` / `'''…'''` → `String`. Escapes: `\n`, `\t`, `\r`, `\\`, `\'`, `\"`.
- Booleans: `True`, `False` → `bool`.
- None: `None` (only meaningful in optional types — see §3, §14).
- Collections: list `[1, 2, 3]`, dict `{"a": 1}`, set `{1, 2, 3}`, tuple `(1, 2)`.
- f-strings: `f"value: {expr}"` with arbitrary embedded expressions, compiled to
  interpolation (corpus: `collection_repr.pyrs`, `unicode_strings.pyrs`,
  `animal_super.pyrs`).

### Identifiers and Rust-keyword escaping
Identifiers are alphanumeric plus underscore and must start with a letter or
underscore. A pyrst identifier that collides with a Rust keyword (`type`, `loop`,
`fn`, `let`, `mut`, `struct`, `enum`, `impl`, `trait`, `match`, `move`, `ref`,
`use`, `mod`, `crate`, …) is automatically emitted as a Rust **raw identifier**
(`r#type`, `r#loop`), so Python code may freely use such names as variables,
fields, or function names. Corpus: `rust_keyword_idents.pyrs` (uses `type` as a
field and a local, `loop` as a function name).

---

## 3. Type System

pyrst uses a **static, compile-time type system** with local inference inside
function bodies and mandatory annotations at boundaries. There is no implicit
`Any`. This section states the rules the current compiler enforces.

> **Inference is a single source of truth.** Type inference for expressions is
> served by one shared, pure oracle consulted by both the type checker and the
> code generator, so the two never disagree about an expression's type. Where the
> oracle cannot determine a type it returns an `Unknown` that is permissively
> compatible with everything; a few such cases are therefore validated by the
> downstream `rustc` invocation rather than by pyrst itself. (Design rationale:
> `docs/design/inference-oracle.md`.)

### Primitive Types
```
int    →  i64           (64-bit signed; the only integer width)
float  →  f64           (IEEE 754 double precision)
str    →  String        (owned UTF-8; len and indexing are char-based)
bool   →  bool
None   →  Option<T>::None   (only inside an optional type — see §14)
```

`int` is **always** `i64` and `str` is **always** `String` — these are fixed, not
"TBD." `int ** int` yields a `float` (Python `**` semantics).

### Collection Types
```
list[T]             →  Vec<T>          (homogeneous, mutable)
dict[K, V]          →  HashMap<K, V>   (hash map)
set[T]              →  HashSet<T>      (unique elements)
tuple[T1, T2, ...]  →  (T1, T2, ...)   (fixed-size, heterogeneous)
```

Collections are homogeneous in their element type(s); type parameters are
invariant (instantiations must match exactly, except for the documented base/
subclass widening described in §6.4).

### Optional Types
```
Optional[T]   →  Option<T>
T | None      →  Option<T>      (equivalent spelling)
None          →  Option::None
```

Optionals are explicit and require **narrowing** before the inner value is used.
Full semantics in §14.

### Class Types
A class compiles to a Rust struct plus an `impl` block:
```python
class Point:
    x: int
    y: int
```
maps to (approximately):
```rust
#[derive(Clone, Debug, PartialEq)]
struct Point { x: i64, y: i64 }
```
A class whose fields are all primitive may additionally derive `Copy`/`Default`;
a class participating in subtype polymorphism is compiled differently (a closed-
set companion enum — see §6.4). Class values have **value semantics** (§4, §13).

### Type Inference
- Function parameters and return types require explicit annotations.
- Class attributes require explicit annotations.
- Local variables are inferred from their first assignment; a later assignment
  must be type-consistent with that first one.
- Forward references and out-of-order definitions work via two-pass checking
  (signatures collected first, then bodies checked).

### Type Compatibility
- Exact match is required, with two exceptions: (1) a `Derived` value is
  compatible with a `Base` slot under the subtyping rules of §6.4, and (2) the
  auto-wrapping into `Optional[T]` slots of §14.
- No implicit numeric widening and no duck typing. `int(x)` / `float(x)` /
  `str(x)` / `bool(x)` are the explicit conversions.

---

## 4. Variables and Mutability

### Declaration
```python
x: int = 5      # explicit type
x = 5           # inferred type
```

### Mutability
- Bindings are mutable by reassignment (`x = 10`); there is no `const`.
- Collections are mutated in place through their methods and subscript
  assignment: `xs.append(4)`, `d[key] = value`, `s.add(1)`.
- **Note:** the dict mutation API is subscript assignment `d[key] = val` — there
  is **no** `dict.insert(key, value)` method (corpus: `dict_ctor_test.pyrs`,
  `data_analysis_tool.pyrs`, `text_processor.pyrs`).

### Scope Rules (Python-style hoisting)
A name first assigned inside an `if`/`elif`/`else`/`for`/`while`/`with`/`try`
body is visible after the block — it is hoisted to function scope, matching
Python's flat function-level scoping. Two edges to know:

- A name whose type cannot be statically inferred, or that is a tuple or an
  all-numeric-field class (no `Default`), is **not** hoisted and stays
  block-local.
- A hoisted name is initialized to a type default (`0` / `""` / empty
  collection). Reading it on a path where it was never assigned yields that
  default rather than raising Python's `UnboundLocalError`.

(Loop and comprehension bodies do not create a new scope; their bound names
persist, per Python.)

---

## 5. Functions

### Definition
```python
def name(param1: Type1, param2: Type2) -> ReturnType:
    return value
```

### Requirements
- Parameter types and the return type are **mandatory**.
- `return` values must match the declared return type; all return paths agree.
- Functions may be forward-declared / out-of-order (two-pass checking).
- Recursion is supported (corpus: `fib.pyrs` and others).

### Default Arguments — **supported**
```python
def greet(name: str, greeting: str = "Hello") -> str: ...
def power(base: int, exp: int = 2) -> int: ...
```
Corpus: `default_params.pyrs`.

### Keyword Arguments — **supported**
Arguments may be passed by name at the call site.

### Lambdas and Closures — **supported**
Lambda expressions (`lambda x: x + 1`) are implemented, including closures that
capture enclosing variables **by value**. Corpus: `lambda_demo.pyrs`,
`lambda_closure.pyrs`. Note that functions/lambdas are still not first-class values
you can pass to *builtins* like `map`/`filter` (see §11, §15).

### Variadic Arguments — not supported
`*args` and `**kwargs` are not supported.

### Argument-passing semantics — value semantics, with an opt-in by-reference mode
By default every non-`self` parameter is passed **by value**: pyrst deep-clones
the caller's value at the call site (uniform "clone-on-use"), so a function
freely reuses a variable after passing it, exactly as Python observably behaves,
**without** shared-mutable aliasing. Because the callee holds its own copy,
mutating a by-value non-`Copy` parameter would not be visible to the caller — so
pyrst rejects that mutation with a loud error rather than miscompiling it (§13).
To let a callee mutate the caller's value in place, declare the parameter
`Mut[T]` (by-reference mode — §13). Design rationale: `docs/design/value-semantics.md`.

---

## 6. Classes and Objects

### 6.1 Definition
```python
class Point:
    x: int
    y: int

    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y

    def move(self, dx: int, dy: int) -> None:
        self.x = self.x + dx
        self.y = self.y + dy
```

### 6.2 Object model
- Each class compiles to a Rust struct with an `impl` block.
- Instances are **value types**: assignment and argument passing deep-copy the
  struct (clone-on-use). There is no shared-mutable aliasing (§4, §13).
- A method receiver is `&self` or `&mut self`, chosen automatically: a method is
  emitted `&mut self` when it (directly or transitively through other
  `self`-methods it calls) mutates `self`.

### 6.3 Constructors, fields, methods
- **`__init__` is supported** — a user-defined `__init__` is honored (corpus:
  `accounts.pyrs`, `vector2d.pyrs`, `animal_super.pyrs`, many others). A class without
  `__init__` gets a default field-wise constructor.
- Instance attributes must be declared and typed in the class body; direct field
  read `obj.field` and assignment `obj.field = value`.
- Instance methods take an implicit `self`.
- **`@staticmethod`** and **`@property`** are supported (`@staticmethod` =
  no-`self` methods, corpus: `staticmethod_demo.pyrs`; `@property` = computed
  read-only attributes, corpus: `property_demo.pyrs`).
- **`@dataclass`** is supported (corpus: `dataclass_demo.pyrs`).
- `@classmethod` is effectively unsupported: the `cls` parameter cannot be
  cleanly annotated. Use `@staticmethod` or a module-level function.
- Arbitrary / user-defined decorators are not supported.
- Class variables (as opposed to instance attributes) are not supported.

### 6.4 Inheritance, `super()`, and subtype polymorphism — **supported**

Single inheritance only (`class Derived(Base):`; multiple inheritance is
rejected). Method lookup checks the derived class first, then the base.

**`super()` is supported** — a subclass can call base-class methods, including
`super().__init__(...)` (corpus: `animal_super.pyrs` line 16, `accounts.pyrs`).

**Subtype polymorphism is supported** — you may pass, assign, or return a
`Derived` where a `Base` is expected, build heterogeneous `list[Base]` literals,
and dispatch methods virtually (corpus: `polymorphism.pyrs`, `subtype_assign.pyrs`,
`subtype_field.pyrs`, `subtype_three_level.pyrs`). Design rationale:
`docs/design/class-subtyping.md`.

**How it is compiled (the model in one line).** Trait objects / `dyn` / `Rc` are
off the table (they conflict with value semantics), so a base class **that has at
least one subclass in the program** is compiled to a **closed-set companion
enum** `Base__` with one variant per class in its hierarchy. Every base-typed
slot (variable, parameter, return, struct field, list element) becomes that enum;
a `Derived` value is wrapped into its variant; method calls dispatch through a
generated `match`. A base class with no subclasses stays a plain struct, so
non-inheriting code is unaffected.

**What works:**

| Pattern | Example |
|---|---|
| `Derived` where `Base` expected | `a: Animal = Dog("Rex")` (assignment, arg passing, `-> Base` return all wrap) |
| Heterogeneous collection | `animals: list[Animal] = [Dog("a"), Cat("b")]` |
| Two siblings in a bare list literal | `[Dog(), Cat()]` unifies to the nearest common base |
| Polymorphic dispatch | `for a in animals: print(a.speak())` calls the subclass override |
| Base-field **read** through a base var | `a.name` where `a: Animal` (generated accessor) |
| Base-typed **field** init + read | `class Zoo: star: Animal` then `Zoo(Dog("Rex"))`, `z.star.speak()` |
| Direct construct of a leaf into an ancestor slot | `a: A = C(...)` for `A <- B <- C` |
| `print` / `==` / `<` on a base var | when the base defines `__str__`/`__eq__`/`__lt__`, the enum forwards `Display`/`PartialEq`/`PartialOrd` |

**Honest-error limits (rejected with a clean message — never a miscompile):**

| Pattern | Behavior | Workaround |
|---|---|---|
| **Upcast an *intermediate* polymorphic base** (`b: B = B(1); a: A = b` for `A <- B <- C`) | codegen error: an intermediate base is already a `B__` enum, not an `A__` variant | construct directly at the target slot: `a: A = B(1)` |
| **Field *write* through a base var** (`a.field = x`, `a: Base`) | codegen error (read-only base-field access is supported) | mutate via a method on the class, dispatched through the enum |
| **Read a *derived-only* field through a base var** (`a.breed`, `a: Animal`) | typeck error — the field is not on the declared base type | use the concrete `Dog` type, or move the field/accessor onto the base |
| **`list` + `list` concatenation** (`[Dog()] + [Cat()]`, even `[1] + [2]`) | codegen error: list `+` list is not supported for *any* element type | use `.extend()` or a comprehension |
| **Dict-literal subtype values** (`dict[str, Animal] = {"a": Dog("Rex")}`) | typeck error: a list literal wraps elements but a dict literal does not yet | `[]`-assign already-`Base` values, or type the values as the base |
| **Exception subtyping** | `Exception` is a builtin, not a user class in the type graph, so user exception *hierarchies* are not part of the companion-enum machinery | catch by exact class name (§12) |

### 6.5 Object identity
- No attribute validation at runtime; all attributes are statically known.
- Subtyping aside, there is no runtime type object / reflection.

---

## 7. Control Flow

### If / Elif / Else
```python
if condition:
    ...
elif condition:
    ...
else:
    ...
```

### Ternary (conditional expression) — **supported**
```python
c = a if cond else b
```
Both branches must share a type; right-associative (corpus: `vs_ternary_of_vars.pyrs`).

### While loops
```python
while condition:
    ...
```

### For loops
```python
for item in iterable:        # over list / set / dict (keys) / str / range
    ...
for i, item in enumerate(items):   # tuple unpacking
    ...
```
Iterates over lists, sets, dict keys, strings, and `range(...)`, and supports
tuple unpacking. **Caveat:** iterating a dict yields its keys, and because the
backing `HashMap`/`HashSet` have no insertion order, set/dict iteration order is
a stable **sorted-by-`repr`** order, not Python's insertion order (§13).

### break / continue / pass
- `break` exits the loop; `continue` skips to the next iteration; `pass` is a
  no-op.
- `for`/`else` and `while`/`else` `else`-blocks are **not** supported.

### Pattern matching — **supported**
```python
match value:
    case 1:
        ...
    case _:
        ...
```
Literal patterns and the `_` wildcard are supported (corpus: `match_demo.pyrs`).

---

## 8. Operators and Expressions

### Precedence (highest to lowest)
1. Primary: `()`, `[]`, `.`, function call
2. Exponentiation: `**` (right-associative)
3. Unary: `-x`, `+x`, `~x`
4. Multiplicative: `*`, `/`, `//`, `%`
5. Additive: `+`, `-`
6. Shift: `<<`, `>>`
7. Bitwise AND: `&`
8. Bitwise XOR: `^`
9. Bitwise OR: `|`
10. Comparison / membership / identity: `==`, `!=`, `<`, `<=`, `>`, `>=`,
    `in`, `not in`, `is`, `is not`
11. Logical NOT: `not`
12. Logical AND: `and`
13. Logical OR: `or`

### Arithmetic
`+`, `-`, `*`, `/` on `int`/`float`; `//` floor division; `%` modulo; `**`
exponentiation. `int ** int → float` (Python semantics).

### Short-circuit evaluation
`and`/`or` short-circuit left-to-right (Python semantics).

### Comparison chaining — **supported**
`a < b < c` is supported with Python semantics (`a < b and b < c`), corpus:
`comparison_chain.pyrs`.

### Operator overloading — **supported**
Dunder methods are honored: `__add__`, `__sub__`, `__mul__`, `__eq__`, `__lt__`,
`__str__`/`__repr__`, etc. (corpus: `vector2d.pyrs` for `__add__`/`__lt__`,
`accounts.pyrs` for `__str__`, `inherit_dunders.pyrs`). For a polymorphic base, the
companion enum forwards the relevant Rust traits to the variant structs (§6.4).

### Bitwise / shift / augmented assignment
`&`, `|`, `^`, `~`, `<<`, `>>` on `int`; the augmented forms (`+=`, `-=`, `*=`,
…, `<<=`, `>>=`) are supported.

### Identity and membership
`is` / `is not` are identity checks, primarily meaningful against `None` for
optional narrowing (§14). `in` / `not in` are membership tests on collections.

---

## 9. Collections

### Lists (`Vec<T>`)
```python
items: list[int] = [1, 2, 3]
items.append(4)
x: int = items[0]
items[0] = 10
sub: list[int] = items[1:3]      # slicing returns a new list
```
Methods: `.append`, `.extend`, `.insert`, `.remove`, `.clear`, `.sort`,
`.reverse`, `.index`, `.count`, `.copy`, `.pop()` / `.pop(i)`; indexing and index
assignment; slicing.

### Dictionaries (`HashMap<K, V>`)
```python
config: dict[str, int] = {"a": 1, "b": 2}
config["c"] = 3                  # mutation is subscript assignment
value: int = config.get("a", 0) # safe access
present: bool = "a" in config
for k, v in config.items(): ...
```
Methods: subscript access / assignment, `in`, `.get(key, default)`, `.keys()`,
`.values()`, `.items()`, `.pop(key)`, `.clear()`, `.copy()`, `.update()`.
There is **no** `.insert(key, value)` method — use `d[key] = val`.

### Sets (`HashSet<T>`) — **supported**
```python
s: set[int] = {1, 2, 3}
s.add(4)
present: bool = 2 in s
u: set[int] = s.union({5, 6})
```
Literals, membership, iteration, comprehensions, and the set algebra / predicate
methods are supported (corpus: `set_methods.pyrs`, `set_comp_test.pyrs`):
`.add`, `.clear`, `.discard`, `.remove`, `.update`, `.union`, `.intersection`,
`.difference`, `.symmetric_difference`, `.issubset`, `.issuperset`, `.isdisjoint`.
**Caveat:** `.discard` / `.remove` do not raise `KeyError` on an absent element
(unlike Python). `frozenset` is not supported.

### Tuples (`(T1, T2, …)`)
```python
pair: tuple[int, str] = (42, "hello")
(a, b) = pair
```
Fixed-size, heterogeneous; support unpacking in assignment and `for` loops;
immutable in generated code.

### Comprehensions — **supported**
```python
squares: list[int]   = [x * x for x in range(10)]
filtered: list[int]  = [x for x in items if x > 0]
uniq: set[int]       = {x for x in items}            # set comprehension
table: dict[int,int] = {x: x * x for x in nums}      # dict comprehension
```
List, set, and dict comprehensions are supported, with an optional `if` filter
(corpus: `set_comp_test.pyrs`, `dict_comp_test.pyrs`). Generator expressions are
**not** supported — use a comprehension.

---

## 10. Strings

```python
s: str = "hello"
s2: str = 'hello'
s3: str = """multi
line"""
greeting: str = f"Hello, {name}!"    # f-string interpolation
ch: str = s[0]                       # char-based indexing → single-char str
```

`str` is owned `String`; `len(s)` and indexing are char-based. A broad method
surface is supported (PYTHON_COMPATIBILITY.md): `.upper`/`.lower`,
`.strip`/`.lstrip`/`.rstrip`, `.split`/`.rsplit`/`.splitlines`, `.replace`,
`.find`/`.rfind`/`.index`/`.count`, `.startswith`/`.endswith`, `.join`,
`.capitalize`/`.title`/`.swapcase`/`.zfill`, and the `is*` predicates
(`.isdigit`, `.isalpha`, `.isspace`, `.isalnum`, …). `.format()` is **not**
supported — use f-strings.

---

## 11. Built-in Functions

### Supported
- I/O: `print(...)` (space-separated, newline; renders collections in CPython
  `repr` form — see §13.4), `input()` (reads a line from stdin).
- Sequence/mapping: `len(...)` (char count for `str`), `range(n)` /
  `range(a, b)` / `range(a, b, step)`, `enumerate(...)` → `(index, value)`,
  `zip(a, b)`.
- Conversions: `int()`, `float()`, `str()`, `bool()` (`str()` of a collection
  yields its repr).
- Constructors: `list()`, `dict()`, `set()`, `tuple()`.
- Numeric / sequence: `sorted()` (returns a new list), `min()`, `max()`, `sum()`,
  `abs()`.

### Limited
- `isinstance()` / `type()` compile but have limited utility under static typing
  (no general runtime type objects).
- File I/O (`open`) is an MVP context manager — see §15.

### Not supported
- `hasattr` / `getattr` / `setattr` (no dynamic attribute access).
- `eval` / `exec` (no dynamic code execution).
- `map` / `filter` / `reduce` — first-class function *values to builtins* are not
  supported; use a comprehension instead.

---

## 12. Error Handling and Exceptions

### Assertions
```python
assert x > 0, "x must be positive"
assert condition
```
Maps to Rust `assert!`; panics on failure.

### Raise
```python
raise ValueError("message")
```
Maps to `panic!` with a typed payload. `raise ... from ...` (exception chaining)
is not supported.

### try / except — **supported**
```python
try:
    risky()
except ValueError as e:
    print("caught: " + e)     # `e` is the exception message (str)
else:
    print("no error")
finally:
    cleanup()
```
`try`/`except` matches on exception **type**, binds `except E as e` (the bound
value is the exception message string), and honors `else` and `finally`
(corpus: `div_zero.pyrs`, `catch_value_error.pyrs`, `except_bound_len.pyrs`). Lowered
via Rust `catch_unwind` (see `RUST_BACKEND.md`).

**Limitations (honest):**
- **No exception class hierarchy.** Catching a base type does not catch a more
  specific one; catch by the exact class name. Custom exception classes
  (`class MyErr(Exception)`) can be defined, raised, and caught by exact name,
  but `Exception` is a builtin (not a user class in the type graph), so there is
  no user-defined subclass hierarchy.
- **Builtin runtime errors are not catchable exceptions.** Index-out-of-range,
  missing dict key, and divide-by-zero abort the program (Rust panic, non-zero
  exit). `try`/`except` catches only values from an explicit `raise` of the
  matching type — a bare `except IndexError` will **not** catch an out-of-bounds
  subscript.
- A caught exception still prints Rust's panic message to stderr before the
  handler runs (cosmetic; stdout and exit code are correct). Uncaught exceptions
  surface a message and a non-zero exit code.

---

## 13. Runtime Model and Value Semantics

pyrst gives every object **value semantics** (each class / collection / string is
an independent, owned Rust value), and routes every consuming site through a
single uniform **clone-on-use** policy: when a non-`Copy` *place* (variable,
field, index) is consumed (passed, returned, assigned, stored in a literal,
matched), it is deep-cloned. This reproduces Python's *observable* behavior for
the common case — you keep using a variable after passing it; a callee cannot
reach back into your object — **without** importing Python's shared-mutable
aliasing. Design rationale: `docs/design/value-semantics.md`.

Two consequences follow, and pyrst makes both **loud** rather than silent:

### 13.1 Mutating a by-value non-`Copy` parameter is a compile error
Because a by-value parameter is a deep clone of the caller's value, a callee's
mutation of it would not be visible to the caller. Rather than silently
miscompile, pyrst rejects every such mutation of a by-value non-`Copy` (`list`,
`dict`, `set`, `str`, or user class) parameter — whether direct, or reaching
**through a field or index** of the parameter:

```text
mutation of by-value parameter `ds` is not visible to the caller;
mutate via a method on it or return the updated value;
or declare the parameter `Mut[T]` to mutate it in place
```

The 13 in-place mutators that trigger this are `append`, `extend`, `insert`,
`remove`, `sort`, `reverse`, `clear`, `add`, `discard`, `update`, `pop`,
`setdefault`, `popitem` (plus field/index assignment). Three remedies:
**(a)** declare the parameter `Mut[T]` (§13.2); **(b)** return the updated value
and let the caller reassign; **(c)** make it a method on `self` (a mutating method
takes `&mut self`). A param that is *reassigned* before mutation, or that *flows
into a `return`*, is exempt — that mutation is the callee's own value.

### 13.2 Opt-in by-reference parameters — `Mut[T]`
Annotate a parameter `Mut[T]` to pass it **by mutable reference** (`&mut T` in the
emitted Rust). The callee's mutations to a `Mut[T]` parameter — direct, nested,
or via a mutating method — **persist to the caller**, and the §13.1 backstop is
suppressed for that parameter (corpus: `mut_method_arg.pyrs`, `mut_put.pyrs`).

```python
def deposit(account: Mut[Account], amt: int) -> None:
    account.balance = account.balance + amt   # visible to the caller
```

Rules and documented limits:
- **Place requirement.** A `Mut[T]` argument must be a *place* (variable, field,
  or index), never a temporary. `deposit(make_account(), 5)` is an honest typeck
  error ("by-reference parameter requires a variable, not a temporary").
- **Parameter-only.** `Mut[T]` is a parameter *mode*, not a type; it is rejected
  on return types, variable/field annotations, or nested forms like
  `list[Mut[T]]`.
- **No aliasing (the conscious price of not using `Rc`).** `&mut` forbids
  aliasing, so passing the **same** variable as two `Mut[T]` arguments at once
  surfaces an honest Rust borrow-check error, never silent-wrong output. Rewrite
  by sequencing the mutations or by return-and-reassign.
- **`Mut[set]` / `Mut[dict]` need element types** — write `Mut[set[int]]` /
  `Mut[dict[str, int]]`, not bare `Mut[set]` (a bare `set`/`dict` head parses as
  an unknown class and the argument-type check rejects the call).
- **`Mut[<primitive>]` has a known limitation** — `Mut[int]`/`Mut[float]`/
  `Mut[bool]` emit `&mut i64` etc., but the codegen does not auto-deref in
  expression position, so arithmetic on the parameter fails to compile. Use a
  `Mut[T]` of a collection/class, or the return idiom, for primitives.

### 13.3 Subscript mutation on nested collections
Indexing yields a value (a clone, per value semantics). For a **local**, mutating
through a subscript on a *nested* collection (`matrix[i][j] = v`,
`local[k].append(x)`) mutates a temporary, not the stored element — pull the
element into a variable, mutate it, and reassign the whole element. When the
subscripted collection is rooted at a **by-value parameter**, this is no longer a
silent no-op but the hard §13.1 error (use `Mut[T]`).

### 13.4 Collection printing and ordering
`print([...])`, `print({...})`, `str([...])`, and f-string interpolation render
lists/tuples/sets/dicts in CPython `repr` form (str elements single-quoted and
escaped, bools as `True`/`False`, nesting recurses). Because `HashSet`/`HashMap`
have no insertion order, **set and dict entries are emitted in a stable
sorted-by-`repr` order**, which may differ from Python's insertion order. Empty
collections render as `[]`, `set()`, `{}`. Tuples up to 6 elements are covered.

---

## 14. Optional / None Semantics

`Optional[T]` and `T | None` both lower to `Option<T>`; the bare `None` literal
lowers to `Option::None`. The model is deliberately explicit — there is no
implicit `Option`-to-`T` coercion, so a missing value can never be read as if it
were present (corpus: `optional_narrowing.pyrs`, `optional_pipe_none.pyrs`,
`optional_return_none.pyrs`, `optional_class.pyrs`).

### Auto-wrapping at the boundary
A bare `T` or `None` is auto-wrapped into an `Optional[T]` slot at the consuming
site — annotated assignment, `return` in an `Optional`-returning function, and
arguments to a **named function** whose parameter is `Optional[T]`:

| Pattern | Lowers to |
|---|---|
| `x: Optional[int] = None` | `let x: Option<i64> = None;` |
| `x: Optional[int] = 5` | `let x: Option<i64> = Some(5);` |
| `return None` / `return 5` in `-> Optional[int]` | `return None;` / `return Some(5);` |
| `f(5)` / `f(None)` where the param is `Optional[int]` | `f(Some(5))` / `f(None)` |

Method parameters and class-constructor fields do **not** yet auto-wrap — pass an
explicit `Optional` value there.

### Narrowing — the only way to use the inner value
An `Optional[T]` supports directly only the None-identity tests `is None` /
`is not None` (and `==`/`!=` against `None`). To use the inner `T`, narrow with a
None-guard; inside the narrowed branch the name has type `T`:

```python
def double_or_zero(x: Optional[int]) -> int:
    if x is not None:
        return x * 2        # x is `int` here
    return 0
```

`if x is not None:` narrows the *then* branch; `if x is None:` narrows the *else*
branch (when there is no intervening `elif`). The narrowing is scoped to that
branch and does not leak past the `if`.

### Honest rejection
Using an `Optional[T]` as a bare `T` **without narrowing** is a hard typeck error
— any operator other than the None-identity tests applied to a raw `Optional`
operand is rejected (`return x + 1` on `x: Optional[int]` → error: narrow first).
pyrst refuses the program rather than emit code that could dereference `None`.
The `None` literal is the *only* thing that fills an `Optional` slot on its own;
the result of a **void function** (`def f() -> None`, or `print(...)` /
`list.append(...)`) is not a value and is rejected when used as an `Optional[T]`.

---

## 15. Context Managers, Advanced Features, and the Unsupported Surface

### Context managers / `with` — **supported (MVP)**
```python
with open(path) as f:
    data: str = f.read()
```
`with X() as y:` is supported. The exercised context manager is file I/O via
`open(path[, mode])` with `read()` / `readlines()` / `write()` / `close()` and
modes `r` / `w` / `a` (corpus: `file_io.pyrs`, `csv_parse.pyrs`). File I/O is MVP:
no `for line in f`, no seek/tell, no binary or encoding control, and I/O errors
panic.

### Supported elsewhere in this spec
Operator overloading (§8), comprehensions including set/dict (§9), `super()` and
subtype polymorphism (§6.4), `Mut[T]` by-reference params (§13), Optional
narrowing (§14), multi-file imports (§16).

### Not supported (by design)
- Coroutines / `async` / `await` (generators/`yield` ARE supported — lazy; see PYTHON_COMPATIBILITY.md).
- `*args` / `**kwargs`.
- `global` / `nonlocal` (no module-level mutable-state rebinding).
- Metaclasses, descriptors, abstract base classes, multiple inheritance.
- Dynamic attribute access, monkey-patching, `eval`/`exec`, reflection
  (`inspect`).
- General / user decorators (only `@dataclass`, `@staticmethod`, `@property`).
- First-class function *values* to builtins (`map`/`filter`/`reduce`); class
  variables; `for`/`else`; generator expressions; `frozenset`; `bytes`;
  `.format()`; Python standard-library imports.

### The honest-error philosophy
Throughout, when a Python construct cannot be lowered faithfully, pyrst emits a
**clean diagnostic** — a lex / parse / typeck / codegen error with a source span,
context lines, visual carets, and (where applicable) a named remedy — rather than
emitting wrong-but-compiling Rust or leaking a raw `rustc` failure. The
honest-rejection corpus (`examples/*fail*.pyrs`, 64 fixtures, plus
`examples/multi_file_fail/`) pins these errors.

---

## 16. Module System — **supported (multi-file)**

```python
import foo                    # import module foo
from foo import bar           # named import
from foo import bar as baz    # aliased import
```

Multi-file programs are supported: imports are resolved via DFS over the import
graph and the modules' declarations are merged into a flat namespace. In the
corpus, multi-file programs are organized as **sibling modules in a
subdirectory** — e.g. `examples/multi_file_demo/` (`main.pyrs` doing
`from common import clamp` and `from math_utils import safe_div, bounded_sum`,
alongside `common.pyrs` and `math_utils.pyrs`).

**Limitations:**
- Circular imports are **detected** (reported via cycle detection) but not
  resolved — see `examples/multi_file_fail/` for the negative scenario.
- No package hierarchy (`foo/__init__.pyrs`), no relative imports, no import-time
  side effects (modules are declarations only), and no Python stdlib imports.

---

## 17. Implementation Notes

### Code generation target
- Emits readable Rust source using the standard library: `Vec`, `HashMap`,
  `HashSet`, `Option`, `String`, etc.
- Compiled to a native binary via `rustc`.

### Pipeline and CLI
The pipeline is lexer → parser → resolver → type checker → Rust codegen → `rustc`.

```bash
pyrst check <file.pyrs>    # parse and type-check
pyrst emit  <file.pyrs>    # print generated Rust to stdout
pyrst build <file.pyrs>    # compile to a native binary via rustc
```

### Performance posture
The default is uniform clone-on-use (§13): correctness-first, with redundant deep
copies tolerated. Last-use move-elision is a deferred optimization, not a
correctness requirement.

### Type-checker soundness (honest status)
The static type checker is **best-effort, not yet fully sound**: a few expressions
still infer to an `Unknown` type that is permissively compatible with everything,
so a small set of type/ownership errors are surfaced by the downstream `rustc`
invocation against generated Rust rather than by pyrst itself. The shared
inference oracle (§3) has narrowed this escape hatch substantially; the remaining
gaps are tracked as deferred items.

### Diagnostics
Type, parse, and lex errors are reported with source spans, context lines, and
visual indicators; runtime errors panic via Rust's panic mechanism.

---

## Related Documentation

- [PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md) — authoritative support
  matrix (this SPEC mirrors it).
- [GRAMMAR.md](GRAMMAR.md) — formal grammar.
- [RUST_BACKEND.md](RUST_BACKEND.md) — rationale, lowering, and implementation detail.
- `docs/design/` — subsystem design notes: `inference-oracle.md` (unified
  inference), `value-semantics.md` (clone-on-use + `Mut[T]`),
  `class-subtyping.md` (companion-enum polymorphism).
</content>
