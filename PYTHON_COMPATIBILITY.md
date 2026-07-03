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
| `float` | ✅ Supported | `f64` | IEEE 754 double precision. `str()`/`repr()`/`print()`/f-string formatting is **CPython-exact**: shortest round-tripping digits, trailing `.0`, scientific form at CPython's thresholds, and **round-half-to-even** on ties (e.g. `-887777373534812.25` → `…812.2`, not `…812.3`) — `str` and `repr` of a float agree everywhere. |
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
| Keyword arguments | ✅ Supported | (W1.5, kwargs v1 + review fix round) Full keyword→positional mapping for user functions, module functions (flat + qualified), methods, and constructors (constructor keywords bind the `__init__` **parameters**, CPython semantics); unknown / duplicate / missing keywords are check-time errors; builtins stay positional-only, like CPython. **Call-site evaluation order is CPython SOURCE order** — positionals first, then keywords as written — even when keyword slots invert AND even for by-reference (`Mut[T]`) arguments (their place side effects run in source position); pinned byte-for-byte by the dual-run goldens `parity_kwargs_evalorder` and `parity_ctor_method_kwargs` |
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
| Subtype polymorphism | ✅ Supported | Pass/assign/return a `Derived` where a `Base` is expected; heterogeneous `list[Base]`; virtual dispatch. See [Class Subtyping / Polymorphism](#class-subtyping--polymorphism). |
| Operator overloading | ✅ Supported | `__add__`, `__eq__`, `__lt__`, `__str__`, etc. |
| `@property` | ✅ Supported | Computed read-only attributes |
| `@staticmethod` | ✅ Supported | No-`self` methods |
| `@classmethod` | ⚠️ Limited | `cls` requires a type annotation pyrst cannot express cleanly |
| `@dataclass` (bare) | ✅ Supported | Synthesizes `__init__` (fields in order, defaults honored), `__repr__` (`ClassName(field=value)`), and structural `__eq__`. Flag args (`order=`/`frozen=`/`eq=`/…) are honest-rejected initially; use the bare `@dataclass`. |
| Unknown class decorator | ✅ Honest error | Any class decorator other than `@dataclass` is a check-time error (was silently swallowed before). |
| Class-level constants (`RED: int = 1`) | ✅ Supported | A class-body binding with a literal default that is never reassigned via `self.` becomes an associated const — `Color.RED` / `self.RED` / `inst.RED`. Enum-member substrate. A field mutated in any method stays a normal instance field. |
| Class instances as `dict` keys / `set` elements | ✅ Supported | A class whose fields are all hashable (`int`/`str`/`bool`/tuple/nested-such) derives `Eq + Hash + Ord`. Uses **structural** equality (value semantics) — diverges from CPython's reference identity for a class without `__eq__`/`__hash__`. A `float`/`list`/`dict`/`set`/`Callable` field, or a user `__eq__`/`__lt__`, is an honest error (unhashable). The derive is **usage-gated** and **transitive**: a key class's user-class fields (directly or in a tuple) derive too; an annotation-less dict/set literal keyed by constructor calls (`{Node(1): …}`) opts the class in. **Comparison is separately gated:** `<`/`<=`/`>`/`>=` and key-less `sorted`/`min`/`max` on a user class require a defined `__lt__` (independent of key status), so the derived `Ord` never silently makes an un-`__lt__` class orderable. A **polymorphic base** (a class with subclasses) can't be a key — it lowers to a companion enum with no uniform derive (honest error; key a concrete leaf). A user class reaching a key position **only through a generic type parameter** is an honest error — pyrst emits one generic fn (no monomorphization), so it can't thread the derive; key the class concretely somewhere to opt in. **Residual:** an annotation-less dict built by index-assigning a **variable** key still needs an annotation. |
| Self-referential fields (`next: Optional[Node]`) | ✅ Supported | Inline self-reference is boxed (`Option<Box<Node>>`). Build TAIL-FIRST — `a.next = b` deep-clones `b` (value semantics), so head-first-then-mutate diverges from CPython's aliasing. A `list[Node]` (tree) needs no boxing. **Perf (value semantics):** reading a boxed recursive field (`node.next`) deep-clones the remaining chain, so a chain read/traversal is O(remaining) per step. This is inherent to value semantics (no shared borrow returns an owned Box-blind value); pyrst does not contort the read path to hide it. |
| Inheritance (multiple) | ❌ Not Supported | Single inheritance only |
| Monkey patching | ❌ Not Supported | Classes are immutable |
| Dynamic attribute access | ❌ Not Supported | No runtime `getattr`/`setattr` |
| Metaclasses | ❌ Not Supported | Not part of the type system |

**Key Semantic Difference:** Classes (and all non-`Copy` values) use **value semantics** (Rust), not reference semantics (Python). Assignment and argument passing **deep-copy** the value (clone-on-use) — there is no shared-mutable aliasing. A callee that should mutate the caller's object opts in explicitly with a `Mut[T]` (by-reference) parameter; otherwise mutating a by-value parameter is a compile error. See *Notable Limitations* for the full model.

---

## Class Subtyping / Polymorphism

Subtype polymorphism — accepting a `Derived` value where a `Base` is expected — **is supported** (single inheritance). Because pyrst compiles each class to an independent value-struct (no `dyn`/`Rc`/trait objects, per the value-semantics model), a base class **that has at least one subclass in the program** is compiled to a **closed-set companion enum** `Base__` with one variant per class in its hierarchy. Every base-typed slot (variable, parameter, return, field, list element) becomes that enum, a `Derived` value is wrapped into its variant, and method calls dispatch through a generated `match`. A base class with no subclasses stays a plain struct, so non-inheriting code is unaffected.

### What works

| Pattern | Example | Notes |
|---------|---------|-------|
| Derived where Base is expected | `a: Animal = Dog("Rex")` | Assignment, parameter passing, and `-> Base` returns all wrap the value into the right variant. |
| Heterogeneous collections | `animals: list[Animal] = [Dog("a"), Cat("b")]` | A `list[Base]` literal holds mixed subclasses; each element is wrapped. Two **sibling** subclasses in a bare list literal (`[Dog(), Cat()]`) unify to their nearest common base. |
| Polymorphic method dispatch | `for a in animals: print(a.speak())` | `a.speak()` calls the **subclass override** for the actual variant (virtual dispatch through the companion enum). |
| Base-field READ through a base var | `a.name` where `a: Animal` | Reading a field declared on the **base** resolves via a generated accessor. (Reading a **derived-only** field through a base var is a typeck error — see below.) |
| Base-typed FIELD init + read | `class Zoo: star: Animal` then `Zoo(Dog("Rex"))`, `z.star.speak()` | A base-typed struct field is the companion enum; a subclass passed to the constructor is wrapped, and reading + dispatching on the field works. |
| Direct construct of a leaf into an ancestor slot | `a: A = C(...)` for `A <- B <- C` | Constructing a leaf directly at any ancestor slot works (the leaf is a variant of the ancestor's enum). |
| `print` / `==` / `<` on a base var | `print(m)`, `a == b`, `a < b` where `m, a, b: Mid` | When the base defines `__str__`/`__repr__`, `__eq__`, `__lt__`, the companion enum forwards `Display`/`PartialEq`/`PartialOrd` to the variant structs. Cross-variant comparison is Python-honest (`==` is `False`, ordering is absent) unless the dunder says otherwise. |
| Single inheritance | `class Dog(Animal):` | One base only (multiple inheritance is unsupported). |

### Limitations (honest errors today — never a miscompile)

Each of the following is reported as a clean pyrst error (typeck or codegen), not a silent miscompile or a raw `rustc` failure. Construct the value differently or use the suggested idiom.

| Pattern | Behavior | Workaround |
|---------|----------|-----------|
| **Upcast an *intermediate* polymorphic base** | `b: B = B(1); a: A = b` for `A <- B <- C` → `codegen error: upcasting an intermediate polymorphic base 'B' to 'A' is not yet supported — construct the value at the 'A' slot directly`. (`b` is already a `B__` enum, which is not an `A__` variant.) | Construct directly at the target slot: `a: A = B(1)`. (Direct leaf/derived construction at any ancestor slot **does** work.) |
| **Field WRITE through a base var** | `a.field = x` where `a: Animal` → `codegen error: writing field 'field' through a polymorphic-base 'Animal' variable is not yet supported … (read-only base-field access is supported)`. | Mutate via a method on the class (`a.set_field(x)` dispatched through the enum), or work with the concrete type. |
| **Read a *derived-only* field through a base var** | `a.breed` where `a: Animal` and `breed` is only on `Dog` → typeck error (the field is not on the declared base type). | Use the concrete `Dog` type, or move the field/accessor onto the base. |
| **`list` + `list` concatenation** | `[Dog()] + [Cat()]` (and even homogeneous `[1] + [2]`) → `codegen error: list '+' list concatenation is not yet supported …`. This is a pre-existing gap for **all** element types, not just subtypes. | Build the result with `.extend()` (`xs.extend(ys)`) or a comprehension. |
| **Dict-literal subtype values** | `d: dict[str, Animal] = {"a": Dog("Rex")}` → typeck error: *type mismatch in assignment: declared `Dict(Str, Class("Animal"))`, got `Dict(Str, Class("Dog"))`*. A `list[Base]` literal wraps its elements, but a dict literal does not yet. | Build the dict and `[]`-assign already-`Base` values, or construct values typed as the base. |
| **Exception subtyping** | `class MyErr(Exception)` can be defined, raised, and caught by exact name, but `Exception` is a builtin (not a user class in the type graph), so it is not part of the companion-enum machinery and there is no user exception *hierarchy*. | Catch by the exact class name. |

**Model in one line:** a base class with subclasses compiles to a closed-set companion enum (`Base__ { Base(Base), Dog(Dog), … }`) with generated method dispatch and base-field accessors; values are wrapped at base-typed slots and dispatched through a `match`. This gives full polymorphism (including heterogeneous collections) within the value-semantics / no-`dyn` model.

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
| `print()` | ✅ Supported | Scalars, strings, and collections (CPython-style repr). An un-narrowed `Optional[T]` prints its payload (via `str()`, so a `str` shows **unquoted**) or `None` — same for `str(opt)` and f-string `f"{opt}"`; `repr(opt)` quotes the payload. |
| `len()` | ✅ Supported | Sequences/mappings; char count for `str`. `len()` of a fixed-shape **tuple** is its constant arity (`len(s.partition("="))` → `3`). |
| `repr()` | ✅ Supported | CPython `%r`: `repr(1.0)` → `1.0`; str quote-choice matrix (single quotes, switch to double when the string has `'` and no `"`); escapes backslash/quote/`\n\t\r`, ASCII controls, the C1 controls (`U+0080–U+009F`), and the common Cf invisibles (`U+00AD`, `U+200B–U+200F`, `U+2028–U+202E`, `U+FEFF`) as `\xXX`/`\uXXXX`. A class needs a `__repr__` (honest error otherwise). **Gap:** exotic Cf/Cn code points outside those ranges pass through (no full Unicode "printable" table). |
| `ascii()` | ✅ Supported | `repr()`'s quote matrix, plus **every** non-ASCII code point escaped as `\xXX`/`\uXXXX`/`\UXXXXXXXX` (`ascii("héllo")` → `'h\xe9llo'`). String arg; other types use their `str`/Display form. |
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
| `.split(sep)` / `.splitlines()` | ✅ Supported | Returns `list[str]` |
| `.rsplit(sep[, maxsplit])` | ✅ Supported | Right-limited split; result is left-to-right `list[str]` (python3-exact). Requires a separator (no-arg whitespace form not supported). |
| `.replace()` | ✅ Supported | Returns new string |
| `.partition(sep)` / `.rpartition(sep)` | ✅ Supported | Returns a 3-**tuple** `(head, sep, tail)` — CPython's real shape (was a `list` before). Unpacks: `head, sep, tail = s.partition("=")`. `len(t)` is the arity; `for x in t` / `x in t` over a tuple are honest check errors (destructure, or `list(t)`); unpacking into the wrong number of names is a check error naming expected/got. |
| `.casefold()` | ⚠️ Simple-fold (context-free) | Per-char Unicode lowercase (`char::to_lowercase`), **context-free** like CPython: a word-final `Σ` folds to `σ` (U+03C3), not the SpecialCasing final `ς` that `str::to_lowercase` produces. Matches CPython for ASCII / İ / Σ (incl. word-final) and all 1:1 mappings. STILL simple-fold: full-fold expansions diverge — `ß` stays `ß` (CPython → `ss`), `ﬁ` stays `ﬁ` (CPython → `fi`); the full-fold table is out of scope. |
| `.translate(table)` / `str.maketrans(x, y)` | ⚠️ Subset | `str.maketrans(x, y)` builds a `dict[int, int]` code-point map from the **equal-length** 2-arg form; unequal lengths raise a catchable `ValueError("the first two maketrans arguments must have equal length")` (CPython-exact — was a silent zip-truncation). `.translate(table)` applies it. The 3-arg delete form (None values) is not supported. |
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

See `RUST_BACKEND.md` for the `catch_unwind` lowering.

---

## Comprehensions and Iterators

| Feature | Status | Notes |
|---------|--------|-------|
| List comprehensions | ✅ Supported | `[x*2 for x in items if x > 0]` |
| Set comprehensions | ✅ Supported | `{x for x in items}` |
| Dict comprehensions | ✅ Supported | `{k: v for k, v in pairs}` |
| Generator expressions `(x for x in ...)` | ❌ Not Supported | Use a comprehension, or a generator **function** — see [Generators (`yield`)](#generators-yield) below |
| `for`/`else` | ❌ Not Supported | `else` block not supported |

---

## Generators (`yield`)

A function whose body contains `yield` is a **generator** and must declare
`-> Iterator[T]` (the yielded element type). `Iterator[T]` is its own distinct
type — **not** an alias for `list[T]` — and generators are **lazy**: nothing in
the body runs until the generator is consumed, and the body advances exactly
one step per value produced. This matches CPython's on-demand timing exactly,
including the interleaving of `print`s in the generator body vs. its consumer.
Because nothing runs ahead of demand, an **infinite** generator
(`while True: yield ...`) is safe to construct and consume with a `break` — O(1)
memory, no eager collection into a list, no hang.

| Feature | Status | Notes |
|---------|--------|-------|
| `yield` inside `while`/`for`/`if`/`with` | ✅ Supported | Lazy; on-demand timing matches Python exactly |
| Infinite generators (`while True: yield`) | ✅ Supported | O(1) memory; safe with `for ... : ... break` |
| `for x in gen(...)` / comprehension source | ✅ Supported | The canonical lazy consumption form |
| `list(gen)` | ✅ Supported | Materializes; the universal escape hatch for every "honest error" shape below |
| `sum`/`min`/`max`/`any`/`all`/`enumerate`/`zip`/`sorted` over a generator | ✅ Supported | Consume the generator directly (fresh call or a variable) |
| `set(gen)` | ✅ Supported (bonus) | Not an explicit design target, but works |
| Generic-element generators (`def g[T](...) -> Iterator[T]`) | ✅ Supported | The coroutine driver is element-agnostic |
| A generator variable, reused/consumed twice | ✅ Matches Python | A drained generator behaves like Python's exhausted generator object across the supported consumption forms above — a second pass yields nothing / `0` / `[]` / `False` rather than re-running the body (no error, same as CPython) |
| A generator closing over a mutable argument | ⚠️ Diverges from Python | pyrst's value semantics **clone** the argument into the generator at construction time; a caller mutation performed *after* construction is **not** visible inside the generator body. Python passes objects by reference, so a Python generator *would* see that mutation. See `examples/gen_closure_capture.pyrs`. |
| `len(gen)` | ❌ Honest error | `TypeError` in CPython too (no `__len__`) — materialize with `list(gen)` first |
| `gen[i]` / `gen[a:b]` | ❌ Honest error | `TypeError` in CPython too (not subscriptable / not sliceable) — materialize with `list(gen)` first |
| `reversed(gen)` | ❌ Honest error | `TypeError` in CPython too — materialize with `list(gen)` first |
| `str(gen)` / `print(gen)` / f-string interpolation | ❌ Honest error | A generator has no printable form (CPython prints an opaque `<generator object ...>`, not its contents) — materialize with `list(gen)` to show contents |
| `gen + gen` / `gen * n` / other binary operators | ❌ Honest error | No lazy analog — materialize first |
| `x in gen` | ❌ Honest error | Would silently drain the generator to test membership — deferred to V2 |
| Passing a generator where `list[T]` is required | ❌ Honest error | An iterator is not a list — materialize with `list(gen)` |
| `Iterator[T]` as a parameter type | ❌ Not yet supported (deferred) | Declare the parameter `list[T]` instead |
| Generator **methods** (`yield` inside a class method) | ❌ Not yet supported (deferred) | Define a free-function generator instead |
| `yield` inside `try`/`except`/`finally` | ❌ Not yet supported (deferred) | Move the `yield` out of the `try` block |
| Nested generator `def`s | ❌ Not yet supported (deferred) | `yield` inside a nested `def` is rejected regardless of return type |
| Generator expressions `(x for x in ...)` | ❌ Not Supported | Use a comprehension or a generator function |
| Explicit `next(g)` | ❌ Not yet supported (deferred) | Consume via `for` / a comprehension / a builtin instead |

Every shape marked "Honest error" above is rejected at `pyrst check` (not
deferred to a confusing `rustc` failure) with a message that names the problem
and suggests the `list(...)` fix. Four of them — `len`, `gen[i]`, `gen[a:b]`,
`reversed(gen)` — are `TypeError` in CPython too, so pyrst is *more* Pythonic
here than a hypothetical eager implementation that silently allowed them.

**A function declared `-> Iterator[T]` must contain a `yield`.** Because
`Iterator[T]` is its own type rather than `list[T]` in disguise, a `yield`-less
function claiming to return `Iterator[T]` is an honest error at `pyrst check` —
declare `-> list[T]` and `return` a materialized list instead, or add a `yield`
to make it a genuine generator.

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
| Python stdlib imports | ✅ Supported | 15 modules ship embedded in the compiler — see [Standard Library](#standard-library) |

---

## Standard Library

pyrst embeds 15 standard-library modules directly in the compiler binary (`include_str!`, see `src/stdlib.rs`) — no filesystem install, no package manager. Import them exactly like CPython: `import math` / `from bisect import bisect_left`, then call qualified (`math.sqrt(x)`) or unqualified (`bisect_left(a, x)`) forms. Because pyrst has no dotted-submodule support yet, every module is flat (`os.path` is not a thing; see `os`'s row below).

**Flat-namespace co-import restriction (card 6c8b4a39):** because every imported module's top-level names merge into one flat table, two modules that define the **same** top-level public name cannot be imported into the same program. This is a **check-time error** naming both modules (never a silent last-import-wins overwrite). Across the 15 shipped modules exactly **one** pair collides: `operator.sub` (arithmetic subtraction) and `re.sub` (regex substitution) — both keep their CPython-faithful names, so a program may import `operator` **or** `re`, not both, until real per-module namespacing (the G3 epic) lands.

**Fidelity philosophy:** honest errors over silent divergence. Where CPython's dynamism can't be represented faithfully (`*args`/`**kwargs`, module-level mutable state, a true opaque-handle object), the module ships the faithful subset and states the gap in its header rather than silently approximating it. Each module carries a **fidelity score out of 5** (5 = drop-in; see `docs/design/stdlib-full.md` §B.2 for the full rubric) and a **parity golden** — a `.pyrs` example under `examples/parity_<module>.pyrs` that pins its behavior. Where the module's surface is CPython-compatible byte-for-byte, that golden is **dual-run**: the identical source file is executed by both the pyrst binary and real `python3` and the outputs are diffed (`docs/design/stdlib-full.md` §G). Where the API deliberately diverges (a forced rename, a class-not-module shape, a different backing algorithm), the golden is marked `# parity: pyrst-only` with the reason stated in its header instead.

| Module | Fidelity (n/5) | Surface highlights | Key divergences / deferrals | Parity |
|---|---|---|---|---|
| `math` | 4.5/5 | 3 float consts (`pi`/`e`/`tau`) + `inf()`/`nan()` niladic externs + guarded float wrappers + pure-pyrst `gcd`/`lcm`/`factorial`/`comb`/`perm`/`isqrt`/`modf`/`dist`/`prod`; (W1.5) 2-arg `log(x, base)` and CPython's full domain/range error shapes on `sin`/`cos`/`tan`(±inf)/`pow`/`fmod`/`remainder` | Float-specialized (no generic numeric kind); `floor`/`ceil`/`trunc` return `float` not `int` (G6, deferred); `gcd`/`lcm`/`perm` lose CPython's variadic/defaulted-`k` shape (no `*args`); `inf`/`nan` are called as functions, not read as attributes; `int` is i64 — `factorial(21)+` overflows honestly instead of going bignum | ✅ dual-run (incl. the W1.5 log/pow/fmod/remainder edge matrix) |
| `os` | 3.5/5 | `@extern`/`@crate("getrandom")` bindings: `getenv getcwd basename join dirname isfile isdir listdir mkdir remove read_file write_file walk stat stat_result getpid rename rmdir makedirs urandom sep linesep` | Flat only — no `os.path` submodule (deferred to G3); `os.environ` mutation deferred (G2); errors surface as a generic panic, not a typed `OSError`; `sep`/`linesep` are hardcoded POSIX values. (W1.5) `basename`/`dirname` are now CPython-posixpath-exact pure string logic (trailing slashes, `.`/`..`, all-slash heads, multibyte — 16-case oracle matrix) | ⚠️ pyrst-only — `@extern`/`@crate`-backed end to end (not real Python syntax), so no same-source dual run; each function was cross-checked against real python3 `os` individually |
| `time` | 4/5 | `time perf_counter monotonic process_time time_ns sleep` (`@extern`) + pure-calendar `struct_time gmtime ctime strftime` | No `localtime`/`mktime`/timezone conversion (needs a tz database); `strftime` rejects locale/`%Z`/`%z`-style directives with `ValueError` rather than faking them; `struct_time` has no index-based (`t[0]`) access, attribute-only | ⚠️ pyrst-only — wall-clock/monotonic `@extern` calls are inherently nondeterministic and have no repeatable CPython twin; the deterministic calendar-math portions (`gmtime`/`ctime`/`strftime` on an explicit epoch) were cross-checked separately |
| `operator` | 4.5/5 | 6 comparisons (`lt le gt ge eq ne`) generic over `T`; `itemgetter truth not_ contains concat`; (W1.5) `mod` and `contains` ship under their REAL CPython names (`mod_`/`contains_` remain as aliases) | `add`/`sub`/`mul`/`floordiv`/`mod` stay int-specialized (generics-v2 bound inference doesn't cover `//`/`%`, and a by-value generic breaks the `reduce(add, ...)` idiom); `attrgetter`/`methodcaller` out of scope (need runtime reflection) | ✅ dual-run (W1.5 — the forced renames are gone, so the same source runs unmodified under python3) |
| `functools` | 4/5 | `reduce(f, xs, init=None)` (3-arg and CPython's 2-arg form), `partial(f, a)`, `cmp_to_key(cmp)`, dict-backed `Cache` | `partial` binds only a single leading positional argument (no variadic capture, G4); `partial`/`cmp_to_key` are int-specialized (closures escaping as `Callable` need a `'static` bound pyrst codegen doesn't emit for generics); empty-sequence 2-arg `reduce` raises `ValueError` where CPython raises `TypeError` (no user-facing `TypeError` in pyrst) | ⚠️ pyrst-only — `Cache` is a pyrst-only extension with no equivalent class in real CPython `functools` |
| `statistics` | 4.5/5 | `mean fmean median median_low median_high mode multimode quantiles variance stdev pvariance pstdev geometric_mean` over `list[float]`; (W1.5) `quantiles(n=, method=)`/`fmean(weights=)` keyword calls + degenerate-input guards with CPython's exact message text | Float-list only (no generic numeric bound yet); no `StatisticsError` CLASS — degenerate input raises `ValueError` with CPython's message (CPython's `StatisticsError` subclasses `ValueError`, so `except ValueError` behaves identically in both runtimes) | ✅ dual-run (incl. keyword shapes and 14 error paths) |
| `string` | 4.5/5 | All 9 CPython constants (`ascii_lowercase … whitespace printable`) plus `capwords(s, sep=None)`; (W1.5) unicode-exact `capitalize` backing — titlecase first char (ß→Ss, digraphs), Final_Sigma-aware tail | Only `Template`/`Formatter` (classes) remain deferred | ✅ dual-run (incl. é/ß/CJK capwords) |
| `bisect` | 4.5/5 | `bisect_left bisect_right bisect insort_left insort_right insort`, all with `lo=0, hi=None` — (W1.5) `lo=`/`hi=` KEYWORD calls work | `key=` (CPython 3.10+) is deferred — needs a two-type-param `Callable[[T],K]` narrowing pattern not yet validated; explicit `hi=-1` intentionally does NOT reproduce CPython C-accelerator's undocumented `-1`-sentinel quirk (matches CPython's pure-Python fallback instead) | ✅ dual-run |
| `heapq` | 4/5 | `heappush heappop heapify heappushpop heapreplace nlargest nsmallest` — (W1.5) `n=`/`iterable=` keyword calls work | `nlargest`/`nsmallest` drop the `key=` callable param (no expressible `Any`-returning callable); `merge` is deferred (needs variadic `*iterables`, G4, plus `Iterator[T]`-as-parameter support) | ✅ dual-run |
| `collections` | 4/5 | `Counter` (function) + `most_common`/`counter_update`/`counter_subtract`/`counter_add`/`counter_sub`/`counter_and`/`counter_or`/`counter_total`/`counter_elements`; `deque` class with `rotate`/`extend`/`extendleft`/`count`/`remove`/`set_maxlen`/`to_list` — (W1.5) two-stack ring, amortized O(1) at BOTH ends like CPython; empty-pop / peek messages are CPython-exact, and `remove()`'s not-found message is CPython-exact for **every** element type incl. `str` (`"'zz' is not in deque"`) — generic `repr(x)` routes through the `PyRepr` trait (W2 card 09152b3a), so a `str` element quotes like CPython's `%r` | `Counter` is a function over `dict[T, int]`, not a dict-subclass class (`dict` has no operator-overload/method-attachment point in pyrst) — arithmetic is free functions (`counter_add(a, b)`), not `+`/`-` operators; tie-breaks and iteration order are by ascending key / most-common-first (deterministic), not CPython's first-insertion order, because pyrst dict iteration is unordered per-process | ⚠️ pyrst-only — `Counter` is a function (not CPython's class) and the `counter_*` free functions don't exist under those names in real CPython `collections` |
| `itertools` | 4/5 | LAZY generators: `count cycle repeat(x) chain islice takewhile dropwhile starmap accumulate zip_longest` (infinite where CPython's are); (W1.5) `accumulate(xs, func=None, initial=None)` — the FULL CPython form — and `zip_longest(a, b, fillvalue=0)` keyword calls | `chain`/`starmap` pairs are binary/2-tuple only (no `*args`, G4); `zip_longest`'s `fillvalue` stays REQUIRED (a `None` default would widen the tuple element type); `groupby`/`tee` are EAGER, not lazy sub-iterators; `islice` single-signature ambiguity (`islice(xs, 2, None)` reads as "first 2"); `accumulate` default-sum over `str` fails at build (Rust `String` lacks the generic `Add` bound — pass `func=`) | ⚠️ pyrst-only — the file exercises pyrst-only shapes (mandatory `fillvalue`, eager `groupby`/`tee` list forms) that real CPython spells differently; the kwargs shapes are dual-run in `parity_kwargs.pyrs` |
| `textwrap` | 4.5/5 | `wrap fill shorten indent dedent`; (W1.5) keyword calls (`width=`, `initial_indent=`, `placeholder=`, …) work exactly like CPython's keyword-only options, and `wrap` is a faithful port of the DEFAULT TextWrapper pipeline (`expand_tabs` at real 8-col tab stops, `replace_whitespace`, `drop_whitespace` incl. the leading-run rule) + `shorten`'s eager placeholder guard | Turning the pipeline OFF (`expand_tabs=False` etc.) is not exposed; `break_on_hyphens` behaves as CPython's `break_on_hyphens=False` (no hyphen splitting); `fix_sentence_endings`/`max_lines` deferred | ✅ dual-run (CPython's own `textwrap`, incl. keyword shapes and tab/newline/mixed-whitespace inputs) |
| `re` | 3/5 | `@crate("regex")`-backed `is_match/search match_ fullmatch findall/find_all sub subn split(maxsplit=) escape` | No `Match`/`Pattern` object — every wrapper recompiles a fresh `regex::Regex` per call and returns `bool`/`str`/`list[str]`/`tuple[str,int]`, never a match object with groups (G1, opaque-handle type, deferred); `escape()` covers every CPython-escaped char except `\v`/`\x0b` (no `\x` lexer escape) | ⚠️ pyrst-only — `is_match`/`find_all`/`match_` are pyrst-only names/aliases with no equivalent attribute on real CPython `re` (a true `Match`/`Pattern` object is G1-deferred) |
| `json` | 4/5 | Pure-pyrst recursive-descent `loads` / serializer `dumps(v, indent=None, sort_keys=False, ensure_ascii=True)` over a tagged `JsonValue` class; surrogate-pair decoding | `JsonValue` is navigated via `.get(k)`/`.at(i)` methods, not `v["k"]`/`v[i]` subscripting (no dual `__getitem__` overload) — permanent, deliberate divergence; `load`/`dump` (file-object forms) deferred (no `file`-typed parameter spelling yet); (W1.5) `ensure_ascii=True` default matches CPython byte-for-byte (`\uXXXX` escapes, surrogate pairs for astral) and `dumps(v, indent=2, sort_keys=True)` keyword calls work | ⚠️ pyrst-only — `JsonValue`/`.get`/`.at` don't exist under CPython's real `json` (which returns native `dict`/`list`), so the API shape can't run unmodified against python3; serialized-string behavior is cross-checked separately |
| `random` | 4.5/5 | `Random` class (seedable) with `random randint randrange uniform getrandbits seed gauss normalvariate triangular gammavariate betavariate` + free `choice shuffle sample choices`; (W1.5) backed by **MT19937 with CPython's exact derivation chain** — `Random(seed)` sequences are BIT-IDENTICAL to CPython (`Random(42).random() == 0.6394267984578837`) | Class-not-module (no mutable globals, G2, so no bare `random.random()`); the generic draws are FREE functions (`choice(rng, xs)` vs CPython's `rng.choice(xs)` — no generic methods); `randbytes` deferred (`bytes`, G7); `getrandbits` capped at 62 bits (i64), seeds are i64; `getstate`/`setstate` unavailable (use `seed(n)`) | ✅ dual-run (seeded method surface byte-identical vs python3; the free-fn call shapes are pinned in `random_freefns.pyrs`, oracle-verified against CPython's method forms) |

**Not planned (out of scope by design, `docs/design/stdlib-full.md` §C):** concurrency/async (`asyncio threading multiprocessing concurrent`, …) — pyrst is single-threaded with no `Send`/async runtime; runtime introspection/dynamic (`ast inspect gc importlib pickle marshal dis`, …) — no runtime object model or `eval`/`exec`; C-FFI/low-level OS (`ctypes mmap fcntl signal`, …) — no unsafe FFI story; GUI/interactive/dev-tooling (`tkinter turtle unittest pdb`, …) — outside a compiled language's remit; legacy "dead battery" modules removed upstream in Python 3.13 (PEP 594); the networking stack (`socket ssl http urllib xml email`, …) — needs a socket/TLS layer pyrst doesn't have.

Everything else — `datetime`, `csv`, `argparse`, `logging`, `sqlite3`, `hashlib`, and roughly 50 more modules — is **planned but not yet shipped** (waves W2–W5 of `docs/design/stdlib-full.md` §F), sequenced behind named compiler epics (dotted submodules, module-level mutable state, an opaque-handle type, a `bytes` type) rather than hidden inside a module card.

---

## Advanced Features

| Feature | Status | Notes |
|---------|--------|-------|
| Context managers / `with` | ⚠️ Files only | `with open(...) as f:` works (the handle is closed via RAII on scope exit). The general context-manager protocol over a **user class** is an **honest typeck error** — `with Guard(...) as g:` would silently skip `__enter__`/`__exit__`, so it is rejected (`context-manager protocol … not yet supported`). Call the methods explicitly. Full support is blocked on real exception objects (pyrst `raise` = panic with a string-encoded type; `__exit__` needs the exception value/traceback and suppression semantics). |
| Operator overloading | ✅ Supported | Dunder methods (see Classes) |
| Generators / `yield` | ✅ Supported (lazy) | `Iterator[T]`-returning functions; on-demand execution, infinite generators OK — see [Generators (`yield`)](#generators-yield) below |
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

**Printing an Optional does NOT require narrowing.** `print(opt)`, `str(opt)`,
and f-string `f"{opt}"` on an un-narrowed `Optional[T]` are allowed and match
CPython: the payload is shown via `str()` (a `str` payload prints **unquoted**,
`Some("x")` → `x`) when present, else the literal `None`. `repr(opt)` routes
through the quoted-repr path instead. Only value-consuming *operators* (arithmetic,
indexing, method calls) still require narrowing.

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
- **Subtype polymorphism is supported (with documented edges):** a base class with subclasses compiles to a closed-set companion enum, so a `list[Base]` *can* hold `Derived` instances, a `Base`-typed slot *can* take a `Derived`, and method calls dispatch to the subclass override. See [Class Subtyping / Polymorphism](#class-subtyping--polymorphism) for the full what-works / honest-limitations table (the edges still rejected with a clear error: upcasting an intermediate base, field-write through a base var, `list`+`list` concat, dict-literal subtype values, and exception subtyping).
- **Builtin runtime errors ARE catchable by their Python exception type:** an out-of-bounds subscript or `pop()` from an empty list raises `IndexError`; a missing dict key raises `KeyError`; `list.remove`/`list.index`/`str.index` misses, a zero slice step, a negative integer `**=` exponent, and failed `int()`/`float()` parses raise `ValueError`; division/modulo by zero raises `ZeroDivisionError`; file I/O failures raise `OSError` (exact-name match). The builtin hierarchy applies (`except LookupError:` catches `IndexError`/`KeyError`). Uncaught, they abort with the message on stderr and a non-zero exit.

---

## Design Philosophy

pyrst aims to be **"Python-like with Rust compilation"** rather than **"Python-compatible."**

The compatibility strategy:
1. ✅ Support the most common, frequently-used Python patterns
2. ✅ Provide clear error messages for unsupported patterns
3. ❌ Do not attempt to emulate Python's dynamic behavior
4. ❌ Do not support features that conflict with static typing or Rust idioms

The dynamic half of Python — metaclasses, monkey-patching, `eval`/`exec`, coroutines/`async`/`await`, `*args`/`**kwargs`, reflection, and the full stdlib — is intentionally out of scope; it is fundamentally incompatible with mandatory static typing and ahead-of-time compilation to Rust. Generators (`yield`) are a deliberate, scoped exception to this stance: they compile to a lazy async-coroutine object under the hood, but the *pyrst-level* surface is a plain `Iterator[T]`-returning function with no exposed `async`/`await` — see [Generators (`yield`)](#generators-yield).

---

*Last updated: June 22, 2026*  
*Phase: 38 + stdlib W1.5 divergence-closing pass (kwargs v1 keyword→positional mapping; MT19937 random; json ensure_ascii; textwrap default pipeline; O(1) deque; unicode capitalize; math/statistics/os edge parity; operator real names) + W1.5 review fix round (call-site evaluation order = CPython source order across the free-fn / method / constructor sites; constructor kwargs bind __init__ params; method kwargs Optional/Callable coercion; str.ljust/rjust/center fillchar; istitle cased-run rule; list.count/index cast parens; statistics.fmean zero-weight guard; O(1) json escape; bisect'd random.choices) — verified against the live compiler (339/339 positive examples, 157 negatives, 12 dual-run + 7 pyrst-only parity goldens, 535 cargo tests, 0 warnings)*
