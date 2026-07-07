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
| `bytes` | ✅ Supported | `Vec<u8>` | Immutable byte strings — literals, index→`int`, slice→`bytes`, iteration→`int`s, `+`/`*`/comparisons, `bytes()`/`bytes(n)`/`bytes(list[int])` constructors, CPython-exact `b'...'` repr. (W5-b) BYTE-offset methods: `hex`/`fromhex`, utf-8 `encode`/`decode` (strict, catchable `UnicodeDecodeError`), `find`/`rfind`/`index`/`rindex`/`count`, `startswith`/`endswith`, `replace`/`split`/`rsplit`/`join`/`strip`/`lstrip`/`rstrip`, `upper`/`lower` (ASCII-only), `ljust`/`rjust`/`center`/`zfill`, `isdigit`/`isalpha`/`isalnum`/`isspace`; membership `int in b` / `bytes in b`. Non-utf8 codecs, method `maxsplit`/`count`/start-end params, and `bytearray` are deferred — see **The `bytes` type** below. |

---

## The `bytes` type

`bytes` is an **immutable** byte string backing to Rust `Vec<u8>`. It is a *value*
(the same ownership shape as `list`), so it rides the existing clone-on-use value
semantics unchanged.

### What works

- **Literals** `b'...'` / `b"..."` (single/double quoted). Escape table: `\n \t \r
  \\ \' \" \0 \b \f` plus `\xNN` hex (a raw 0x00–0xff byte). `b'\x80'` is the single
  byte 0x80 — not a UTF-8 scalar, which is exactly why `bytes` is not a `str`.
- **Access shapes — the OPPOSITE of `str`.** `b[i]` → `int` (the byte value, a `u8`
  widened to `i64`; negative indices and a catchable `IndexError` like a list).
  `b[i:j]` / `b[i:j:k]` → `bytes`. Iteration (`for x in b`, comprehensions) yields
  `int`s. `len(b)` is the byte count. `list(b)` → `list[int]`; `sum(b)` → `int`.
- **Operators** `b1 + b2` (concat), `b * n` / `n * b` (repeat), `==` `!=` `<` `<=`
  `>` `>=` (lexicographic), plus hashing — so `bytes` is a valid `dict`/`set` key.
- **Constructors** `bytes()` / `bytes(0)` → empty; `bytes(n)` → `n` zero bytes;
  `bytes(list[int])` → each element range-checked to 0–255; `bytes(b)` → a copy.
- **Display** `print(b)`, `str(b)`, `repr(b)`, `f"{b}"`, and container reprs
  (`list[bytes]`, `dict[bytes, _]`) all emit the **CPython-exact** `b'...'` repr:
  single quotes by default, double iff the payload has a `'` and no `"`; escape
  `\\`, the active quote, and `\t`/`\n`/`\r`; a printable byte 0x20–0x7e is literal;
  every other byte is a lowercase `\xNN`.

### Honest divergences and deferrals (errors, never miscompiles)

- **`bytes == str` is rejected**, not `False`. CPython answers `False`; pyrst treats
  a mixed `bytes`/`str` `==` as almost certainly a bug and rejects it at check time
  (decode/encode to bridge). `bytes + str` / `str + bytes` are likewise rejected
  (CPython `TypeError`).
- **Methods (W5-b) — all BYTE-offset, python3-oracle-validated.** Supported:
  `hex()` / `bytes.fromhex(s)`; the codecs `str.encode()` / `bytes.decode()`
  (utf-8 only — a String's bytes *are* UTF-8); the search family `find` / `rfind`
  / `index` / `rindex` / `count` (byte offsets — never str's char offsets; `index`
  / `rindex` raise a catchable `ValueError: subsection not found`); `startswith` /
  `endswith`; the transforms `replace` / `split` / `rsplit` / `join` / `strip` /
  `lstrip` / `rstrip` / `upper` / `lower` / `ljust` / `rjust` / `center` / `zfill`;
  and the ASCII-only predicates `isdigit` / `isalpha` / `isalnum` / `isspace`.
  `upper`/`lower`/predicates are ASCII-only (a non-ASCII byte passes through / is
  never "alpha" — unlike `str`, where `'²'.isdigit()` is `True`). `strip`'s
  argument is a **set of bytes**, not a substring.
- **`bytes.decode` is STRICT.** Invalid UTF-8 raises a catchable
  `UnicodeDecodeError` whose message matches CPython. CPython uses **two**
  message templates and pyrst reproduces both (dual-run pinned by
  `parity_bytes_decode_error`, which covers each form incl. mid-buffer offsets):
  - a **single-byte** form when exactly one byte is at fault —
    `'utf-8' codec can't decode byte 0xNN in position P: {invalid start byte |
    invalid continuation byte | unexpected end of data}` (a bad start byte, a
    lead byte followed immediately by a bad continuation, or a 1-byte
    truncation);
  - a **range** form when a multi-byte run is at fault —
    `'utf-8' codec can't decode bytes in position P-Q: {invalid continuation
    byte | unexpected end of data}` (a valid lead + one or more valid
    continuations before an invalid continuation byte, or a multi-byte
    truncation). `Q = P + error_len − 1`.

  The `errors=` argument (`replace`/`ignore`) is **deferred** (an honest check
  error).
- **Codecs are utf-8 only in W5.** `encode`/`decode` accept no encoding, or the
  literal `'utf-8'` (case/`-`/`_`-insensitive). A different encoding
  (`'ascii'`/`'latin-1'`/…) or a **non-literal** encoding is an honest check error;
  `ascii`/`latin-1`/`utf-16` are a documented follow-on (design §B).
- **Method parameter shapes matched to str's pyrst ceiling (honest arity errors).**
  Deferred, each a check error (never a silent drop): `startswith`/`endswith`
  tuple-of-prefixes and start/end offsets; `split`/`rsplit` `maxsplit`; `replace`
  `count`; the int-argument form of `find`/`index`/`count` (CPython's single
  byte-value search). `join` requires a `list[bytes]` (a `list[int]` is a check
  error, not a rustc leak).
- **Membership `x in b` (W5-b).** `int in bytes` is a byte-value test — an
  out-of-range int raises a catchable `ValueError: byte must be in range(0, 256)`;
  `bytes in bytes` is a subsequence test (`b'' in b` is `True`). `str in bytes`
  stays a type error (CPython `TypeError`) — decode/encode to bridge.
- **Item assignment `b[i] = x` is rejected** — `bytes` is immutable (CPython
  `TypeError`). The mutable sibling **`bytearray` is deferred** (its annotation is a
  clean error, not a silent phantom class).
- **Escapes pyrst rejects that CPython accepts (all honest-STRICTER, documented):**
  octal `\ooo` (e.g. `\012`; use `\xNN`), and `\a`/`\v` and other non-table escapes
  — consistent with pyrst's `str` escape set. `\u`/`\N` inside a bytes literal are
  rejected too. This is pyrst being **stricter than CPython, not matching it**:
  CPython 3.12 *accepts* `b'\u0041'`, emits a `SyntaxWarning: invalid escape
  sequence '\u'`, and keeps the backslash **literally** — `b'\u0041'` is the 6
  bytes `\ u 0 0 4 1` (`[92, 117, 48, 48, 52, 49]`, python3-verified), NOT the
  character `A`. pyrst refuses it rather than silently emit six bytes where a `\u`
  escape was almost certainly intended — exactly the confusion CPython's own
  warning exists to flag (the same honest-stricter framing as the W4-d P9b
  correction). A raw non-ASCII source byte in `b'...'` is a `SyntaxError` in both.
  **Triple-quoted bytes** (`b'''...'''`) and **raw-bytes prefixes** (`rb'...'` /
  `br'...'`, any case) are deferred — both honest lexer errors, never miscompiles.

---

## Functions

| Feature | Status | Notes |
|---------|--------|-------|
| Function definition | ✅ Supported | Requires type annotations |
| Return statements | ✅ Supported | Type checked |
| Recursion | ✅ Supported | Works as expected |
| Positional arguments | ✅ Supported | Order matters |
| Keyword arguments | ✅ Supported | (W1.5, kwargs v1 + review fix round) Full keyword→positional mapping for user functions, module functions (flat + qualified), methods, and constructors (constructor keywords bind the `__init__` **parameters**, CPython semantics); unknown / duplicate / missing keywords are check-time errors; builtins stay positional-only, like CPython. **Call-site evaluation order is CPython SOURCE order** — positionals first, then keywords as written — even when keyword slots invert AND even for by-reference (`Mut[T]`) arguments (their place side effects run in source position); pinned byte-for-byte by the dual-run goldens `parity_kwargs_evalorder` and `parity_ctor_method_kwargs` |
| Default arguments | ✅ Supported | `def f(x: int = 5)`. **Eval-timing divergence (honest):** CPython evaluates a default expression **once**, at `def` time; pyrst **re-evaluates the default on every call that omits the argument** (the default is spliced into each call site). This is observable only with a *side-effecting* default. Silver lining: the classic CPython **mutable-default trap is avoided** — `def acc(x: int, xs: list[int] = []) -> list[int]` gets a **fresh** `[]` on every call (each call returns `[x]`), whereas CPython shares one list across all calls (`[x]`, then `[x1, x2]`, …). |
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
| User truthiness (`__bool__`) | ✅ Supported | A class defining `__bool__(self) -> bool` is usable in every boolean context — `if`/`elif`/`while`/`assert`, `not`, `bool(x)`, `and`/`or`, and comprehension `if`-filters — each lowered to a `.__bool__()` call (CPython semantics; the method runs exactly once per evaluation). A class with **no** `__bool__` in a boolean context is an honest build error, never silently truthy. |
| `@property` | ✅ Supported | Computed read-only attributes |
| `@staticmethod` | ✅ Supported | No-`self` methods |
| `@classmethod` | ⚠️ Limited | `cls` requires a type annotation pyrst cannot express cleanly |
| `@dataclass` (bare) | ✅ Supported | Synthesizes `__init__` (fields in order, defaults honored), `__repr__` (`ClassName(field=value)`), and structural `__eq__`. Flag args (`order=`/`frozen=`/`eq=`/…) are honest-rejected initially; use the bare `@dataclass`. |
| Unknown class decorator | ✅ Honest error | Any class decorator other than `@dataclass` is a check-time error (was silently swallowed before). |
| Class-level constants (`RED: int = 1`) | ✅ Supported | A class-body binding with a literal default that is never reassigned via `self.` becomes an associated const — `Color.RED` / `self.RED` / `inst.RED`. Enum-member substrate. A field mutated in any method stays a normal instance field. |
| Class instances as `dict` keys / `set` elements | ✅ Supported | A class whose fields are all hashable (`int`/`str`/`bool`/tuple/nested-such) derives `Eq + Hash + Ord`. Uses **structural** equality (value semantics) — diverges from CPython's reference identity for a class without `__eq__`/`__hash__`. A `float`/`list`/`dict`/`set`/`Callable` field, or a user `__eq__`/`__lt__`, is an honest error (unhashable). The derive is **usage-gated** and **transitive**: a key class's user-class fields (directly or in a tuple) derive too; an annotation-less dict/set literal keyed by constructor calls (`{Node(1): …}`) opts the class in. **Comparison is separately gated:** `<`/`<=`/`>`/`>=` and key-less `sorted`/`min`/`max` on a user class require a defined `__lt__` (independent of key status), so the derived `Ord` never silently makes an un-`__lt__` class orderable. A **polymorphic base** (a class with subclasses) can't be a key — it lowers to a companion enum with no uniform derive (honest error; key a concrete leaf). A user class reaching a key position **only through a generic type parameter** is an honest error — pyrst emits one generic fn (no monomorphization), so it can't thread the derive; key the class concretely somewhere to opt in. **Residual:** an annotation-less dict built by index-assigning a **variable** key still needs an annotation. |
| Self-referential fields (`next: Optional[Node]`) | ✅ Supported | Inline self-reference is boxed (`Option<Box<Node>>`). Build TAIL-FIRST — `a.next = b` deep-clones `b` (value semantics), so head-first-then-mutate diverges from CPython's aliasing. A `list[Node]` (tree) needs no boxing. **Perf (value semantics):** reading a boxed recursive field (`node.next`) deep-clones the remaining chain, so a chain read/traversal is O(remaining) per step. This is inherent to value semantics (no shared borrow returns an owned Box-blind value); pyrst does not contort the read path to hide it. Consequently a **`while cur is not None: … cur = cur.next` traversal of a boxed recursive chain is O(n²)** overall for an n-length list — each `cur.next` step clones the rest of the chain. Correct, but quadratic; for a hot linear walk prefer a `list[Node]` (contiguous, no per-step clone) over a boxed linked list. |
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
| `range()` | ✅ Supported | `range(n)`, `range(a, b)`, `range(a, b, step)` — including **descending** ranges (a negative step, e.g. `range(5, 0, -1)`), lowered with a runtime-direction step so a `step < 0` yields the correct decreasing sequence rather than silently emptying. `list(range(...))` materializes any of these into a `list[int]`. |
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
| `import foo.bar` (dotted submodule) | ✅ Supported | W3: embedded dotted submodules (`os.path`, `urllib.parse`, `collections.abc`) resolve as real package modules (`lib/os/path.pyrs` keyed `"os.path"`). **`import os` does NOT auto-expose `os.path`** — the submodule needs its own explicit import (an honest v1 divergence from CPython's module-attribute trick) |
| `from foo.bar import baz` | ✅ Supported | Dotted from-import; an unknown name is a check error that **never falls back to the parent module** (`from os.path import listdir` is rejected, not silently rebound to `os.listdir`) |
| `import foo as f` / `from foo import bar as baz` | ❌ Rejected | Import aliases are an honest parse/check error (EPIC-6 B), not a silently-discarded alias |
| Multi-file programs | ✅ Supported | DFS import resolution; per-module namespaced emission (W3-2) |
| Circular imports | ⚠️ Detected | Reported via cycle detection, not resolved |
| Package structure | ⚠️ Partial | Embedded stdlib packages resolve via dotted keys and directory layout (`lib/os/path.pyrs` → `os.path`); a general user-defined package hierarchy (`__init__`, nesting) is not supported |
| Relative imports | ❌ Not Supported | Not yet implemented |
| Side effects at import | ❌ Not Supported | Modules are declarations only |
| Python stdlib imports | ✅ Supported | 46 modules ship embedded in the compiler — see [Standard Library](#standard-library) |

---

## Standard Library

pyrst embeds 46 standard-library modules directly in the compiler binary (`include_str!`, see `src/stdlib.rs`) — no filesystem install, no package manager. Import them exactly like CPython: `import math` / `from bisect import bisect_left`, then call qualified (`math.sqrt(x)`) or unqualified (`bisect_left(a, x)`) forms. Dotted submodules are real (W3): `os.path`, `urllib.parse`, and `collections.abc` ship as embedded package modules (`lib/os/path.pyrs` keyed `"os.path"`, etc.) — note that `import os` does **not** auto-expose `os.path`; import the submodule explicitly.

**Per-module namespacing (W3-2) — the flat-namespace co-import restriction is retired.** Historically (card 6c8b4a39), every imported module's top-level names merged into one flat table, so two modules defining the same top-level public name could not be imported into the same program — there were exactly 8 such colliding pairs (`operator`/`re` `sub`, `copy`/`shutil` `copy`, `html`/`re` `escape`, `os`/`shlex` `join`, `re`/`shlex` `split`, `datetime`/`time` `time`, `platform`/`sys` `platform` and `version`). With per-module namespaced emission (`__pyrst_m_<owner>__<name>`), **all 8 former pairs now co-import cleanly** — proven by goldens (`parity_coimport_operator_re`, `parity_coimport_html_re`, `parity_coimport_re_shlex`, `parity_coimport_platform_sys`, `coimport_copy_shutil`, `coimport_os_shlex`, `coimport_datetime_time`). Two honest limits remain:

1. **Class names stay globally unique (class-vs-class only).** A class type is carried through the type system as a bare `Ty::Class(name)` with no owner, so two modules each defining e.g. `class Point` cannot be told apart at a type reference — co-importing them is a **check-time error** naming both modules (never a silent overwrite). **One shipped stdlib pair is now class-vs-class:** W5-f gave `re` a real `class Match` (`re.Match`), which collides with `difflib`'s pre-existing `class Match`, so `import re` + `import difflib` in the same program is exactly this honest error (*class `Match` is defined by both `re` and `difflib`*). **Neither is renamed** — both `Match` names are CPython-canonical, and a rename would hurt drop-in fidelity more than the collision does — so this pair is the newest motivator for the v2 fix: threading a module owner into the class type (true same-named-class co-import). Pinned by the negative `examples/fail_coimport_re_difflib.pyrs`; other same-named user-module classes hit the same honest error.
2. **Qualified dotted-class construction is unsupported.** `datetime.time(9, 30)`, `urllib.parse.ParseResult(...)`, `fractions.Fraction(...)` — constructing a CLASS through the qualified `module.Class(...)` form — fails to typecheck with an honest error (`module 'X' has no function 'Y'`), never a silent miscompile. Workaround: `from urllib.parse import ParseResult` (from-import construction works), or use the module's factory functions (`urlparse(...)`, `datetime_now()`, …).

**Fidelity philosophy:** honest errors over silent divergence. Where CPython's dynamism can't be represented faithfully (`*args`/`**kwargs`, module-level mutable state, a true opaque-handle object), the module ships the faithful subset and states the gap in its header rather than silently approximating it. Each module carries a **fidelity score out of 5** (5 = drop-in; see `docs/design/stdlib-full.md` §B.2 for the full rubric) and a **parity golden** — a `.pyrs` example under `examples/parity_<module>.pyrs` that pins its behavior. Where the module's surface is CPython-compatible byte-for-byte, that golden is **dual-run**: the identical source file is executed by both the pyrst binary and real `python3` and the outputs are diffed (`docs/design/stdlib-full.md` §G). Where the API deliberately diverges (a forced rename, a class-not-module shape, a different backing algorithm), the golden is marked `# parity: pyrst-only` with the reason stated in its header instead.

| Module | Fidelity (n/5) | Surface highlights | Key divergences / deferrals | Parity |
|---|---|---|---|---|
| `math` | 4.5/5 | 3 float consts (`pi`/`e`/`tau`) + `inf()`/`nan()` niladic externs + guarded float wrappers + pure-pyrst `gcd`/`lcm`/`factorial`/`comb`/`perm`/`isqrt`/`modf`/`dist`/`prod`; (W1.5) 2-arg `log(x, base)` and CPython's full domain/range error shapes on `sin`/`cos`/`tan`(±inf)/`pow`/`fmod`/`remainder` | Float-specialized (no generic numeric kind); `floor`/`ceil`/`trunc` return `float` not `int` (G6, deferred); `gcd`/`lcm`/`perm` lose CPython's variadic/defaulted-`k` shape (no `*args`); `inf`/`nan` are called as functions, not read as attributes; `int` is i64 — `factorial(21)+` overflows honestly instead of going bignum | ✅ dual-run (incl. the W1.5 log/pow/fmod/remainder edge matrix) |
| `os` | 3.5/5 | `@extern`/`@crate("getrandom")` bindings: `getenv getcwd basename join dirname isfile isdir listdir mkdir remove read_file write_file walk stat stat_result getpid rename rmdir makedirs urandom sep linesep` | `os.path` now ships as a real dotted submodule (next row) — the path-shaped names still on this flat module (`basename`/`dirname`/`join`/`path_exists`/`isfile`/`isdir`) are **deprecated aliases**, kept unchanged for back-compat; new code should `from os.path import join` (etc.). `os.environ` mutation deferred (G2); errors surface as a generic panic, not a typed `OSError`; `sep`/`linesep` are hardcoded POSIX values. (W1.5) `basename`/`dirname` are CPython-posixpath-exact pure string logic (trailing slashes, `.`/`..`, all-slash heads, multibyte — 16-case oracle matrix) | ⚠️ pyrst-only — `@extern`/`@crate`-backed end to end (not real Python syntax), so no same-source dual run; each function was cross-checked against real python3 `os` individually |
| `os.path` | 4/5 | Real DOTTED submodule (W3, `lib/os/path.pyrs`): pure `basename dirname isabs split splitext normpath relpath` + `@extern`-backed `join exists isfile isdir abspath expanduser` — all CPython-posixpath-faithful names and shapes | `import os` does NOT auto-expose it (explicit `import os.path` required); `join(a, b)` is 2-ary (no `*args`); `relpath(path, start)` requires both args already absolute; `expanduser` has no `pwd`-module binding — `~user` and `HOME`-unset return the path **unchanged**, a real POSIX divergence for names present in the password database (CPython resolves them via `pwd.getpwnam`/`getpwuid`) | ✅ dual-run (pure functions, incl. the split/splitext/normpath/relpath edge matrices); the six `@extern`-backed functions are pinned pyrst-only in `parity_os_path_extern` |
| `time` | 4/5 | `time perf_counter monotonic process_time time_ns sleep` (`@extern`) + pure-calendar `struct_time gmtime ctime strftime` | No `localtime`/`mktime`/timezone conversion (needs a tz database); `strftime` rejects locale/`%Z`/`%z`-style directives with `ValueError` rather than faking them; `struct_time` has no index-based (`t[0]`) access, attribute-only | ⚠️ pyrst-only — wall-clock/monotonic `@extern` calls are inherently nondeterministic and have no repeatable CPython twin; the deterministic calendar-math portions (`gmtime`/`ctime`/`strftime` on an explicit epoch) were cross-checked separately |
| `operator` | 4.5/5 | 6 comparisons (`lt le gt ge eq ne`) generic over `T`; `itemgetter truth not_ contains concat`; (W1.5) `mod` and `contains` ship under their REAL CPython names (`mod_`/`contains_` remain as aliases) | `add`/`sub`/`mul`/`floordiv`/`mod` stay int-specialized (generics-v2 bound inference doesn't cover `//`/`%`, and a by-value generic breaks the `reduce(add, ...)` idiom); `attrgetter`/`methodcaller` out of scope (need runtime reflection) | ✅ dual-run (W1.5 — the forced renames are gone, so the same source runs unmodified under python3) |
| `functools` | 4/5 | `reduce(f, xs, init=None)` (3-arg and CPython's 2-arg form), `partial(f, a)`, `cmp_to_key(cmp)`, dict-backed `Cache` | `partial` binds only a single leading positional argument (no variadic capture, G4); `partial`/`cmp_to_key` are int-specialized (closures escaping as `Callable` need a `'static` bound pyrst codegen doesn't emit for generics); empty-sequence 2-arg `reduce` raises `ValueError` where CPython raises `TypeError` (no user-facing `TypeError` in pyrst) | ⚠️ pyrst-only — `Cache` is a pyrst-only extension with no equivalent class in real CPython `functools` |
| `statistics` | 4.5/5 | `mean fmean median median_low median_high mode multimode quantiles variance stdev pvariance pstdev geometric_mean` over `list[float]`; (W1.5) `quantiles(n=, method=)`/`fmean(weights=)` keyword calls + degenerate-input guards with CPython's exact message text | Float-list only (no generic numeric bound yet); no `StatisticsError` CLASS — degenerate input raises `ValueError` with CPython's message (CPython's `StatisticsError` subclasses `ValueError`, so `except ValueError` behaves identically in both runtimes) | ✅ dual-run (incl. keyword shapes and 14 error paths) |
| `string` | 4.5/5 | All 9 CPython constants (`ascii_lowercase … whitespace printable`) plus `capwords(s, sep=None)`; (W1.5) unicode-exact `capitalize` backing — titlecase first char (ß→Ss, digraphs), Final_Sigma-aware tail | Only `Template`/`Formatter` (classes) remain deferred | ✅ dual-run (incl. é/ß/CJK capwords) |
| `bisect` | 4.5/5 | `bisect_left bisect_right bisect insort_left insort_right insort`, all with `lo=0, hi=None` — (W1.5) `lo=`/`hi=` KEYWORD calls work | `key=` (CPython 3.10+) is deferred — needs a two-type-param `Callable[[T],K]` narrowing pattern not yet validated; explicit `hi=-1` intentionally does NOT reproduce CPython C-accelerator's undocumented `-1`-sentinel quirk (matches CPython's pure-Python fallback instead) | ✅ dual-run |
| `heapq` | 4/5 | `heappush heappop heapify heappushpop heapreplace nlargest nsmallest` — (W1.5) `n=`/`iterable=` keyword calls work | `nlargest`/`nsmallest` drop the `key=` callable param (no expressible `Any`-returning callable); `merge` is deferred (needs variadic `*iterables`, G4, plus `Iterator[T]`-as-parameter support) | ✅ dual-run |
| `collections` | 4/5 | `Counter` (function) + `most_common`/`counter_update`/`counter_subtract`/`counter_add`/`counter_sub`/`counter_and`/`counter_or`/`counter_total`/`counter_elements`; `deque` class with `rotate`/`extend`/`extendleft`/`count`/`remove`/`set_maxlen`/`to_list` — (W1.5) two-stack ring, amortized O(1) at BOTH ends like CPython; empty-pop / peek messages are CPython-exact, and `remove()`'s not-found message is CPython-exact for **every** element type incl. `str` (`"'zz' is not in deque"`) — generic `repr(x)` routes through the `PyRepr` trait (W2 card 09152b3a), so a `str` element quotes like CPython's `%r` | `Counter` is a function over `dict[T, int]`, not a dict-subclass class (`dict` has no operator-overload/method-attachment point in pyrst) — arithmetic is free functions (`counter_add(a, b)`), not `+`/`-` operators; tie-breaks and iteration order are by ascending key / most-common-first (deterministic), not CPython's first-insertion order, because pyrst dict iteration is unordered per-process | ⚠️ pyrst-only — `Counter` is a function (not CPython's class) and the `counter_*` free functions don't exist under those names in real CPython `collections` |
| `collections.abc` | 0/5 (by design) | Documentation-only DOTTED submodule (W3, `lib/collections/abc.pyrs`): defines **zero** runtime names — its header maps all 25 CPython ABCs to their compile-time pyrst equivalents (`Iterable`/`Iterator` → `Iterator[T]`/generators, `Sequence` → `list[T]`, `Mapping` → `dict[K,V]`, `Set` → `set[T]`, `Sized` → `len()`, `Callable` → the builtin `Callable[[...],R]` type, …) | Runtime ABCs are built on structural `isinstance()` and `ABCMeta` abstract-method enforcement, both permanently outside pyrst's static model — so no name is faked. `import collections.abc` succeeds honestly; any USE of a from-imported ABC name (`Iterable()`, `x: Iterable`) is an honest "undefined" check error (an *unused* `from collections.abc import X` is tolerated by pre-existing resolver leniency for symbol-less modules) | ✅ dual-run (import golden) |
| `itertools` | 4/5 | LAZY generators: `count cycle repeat(x) chain islice takewhile dropwhile starmap accumulate zip_longest` (infinite where CPython's are); (W1.5) `accumulate(xs, func=None, initial=None)` — the FULL CPython form — and `zip_longest(a, b, fillvalue=0)` keyword calls | `chain`/`starmap` pairs are binary/2-tuple only (no `*args`, G4); `zip_longest`'s `fillvalue` stays REQUIRED (a `None` default would widen the tuple element type); `groupby`/`tee` are EAGER, not lazy sub-iterators; `islice` single-signature ambiguity (`islice(xs, 2, None)` reads as "first 2"); `accumulate` default-sum over `str` fails at build (Rust `String` lacks the generic `Add` bound — pass `func=`) | ⚠️ pyrst-only — the file exercises pyrst-only shapes (mandatory `fillvalue`, eager `groupby`/`tee` list forms) that real CPython spells differently; the kwargs shapes are dual-run in `parity_kwargs.pyrs` |
| `textwrap` | 4.5/5 | `wrap fill shorten indent dedent`; (W1.5) keyword calls (`width=`, `initial_indent=`, `placeholder=`, …) work exactly like CPython's keyword-only options, and `wrap` is a faithful port of the DEFAULT TextWrapper pipeline (`expand_tabs` at real 8-col tab stops, `replace_whitespace`, `drop_whitespace` incl. the leading-run rule) + `shorten`'s eager placeholder guard | Turning the pipeline OFF (`expand_tabs=False` etc.) is not exposed; `break_on_hyphens` behaves as CPython's `break_on_hyphens=False` (no hyphen splitting); `fix_sentence_endings`/`max_lines` deferred | ✅ dual-run (CPython's own `textwrap`, incl. keyword shapes and tab/newline/mixed-whitespace inputs) |
| `re` | 4/5 | `@crate("regex")`-backed. **(W5-f) REAL `re.Match` via eager extraction** (`docs/design/w5-bytes-handles.md` §F — a pure value struct, needs NEITHER the `bytes` nor the opaque-handle gate): `search`/`match_`/`fullmatch` → `Optional[Match]`, `finditer` → `list[Match]`; the `Match` object exposes `group(n=0)`/`groups()`/`start(n=0)`/`end(n=0)`/`span(n=0)`/`groupdict()`/`__bool__`, with **named groups** `(?P<name>...)`, **unmatched-group** `None`/`-1`/`(-1,-1)` and out-of-range `IndexError("no such group")` all CPython-exact, and **CHAR-offset spans** on multibyte subjects (byte→code-point conversion in the extractor — `re.search("l+","héllo").span()==(2,4)`). Also `is_match` (bool predicate), `find_all` (legacy whole-matches), `findall` (CPython 0/1-group shape), `sub subn split(maxsplit=) escape` | Named-group ACCESS is `m.groupdict()['n']`, not `m.group('n')` (methods are single-typed — no `int|str` overload); `groups()` returns a **`list[Optional[str]]`** not a tuple (variable-arity tuple-with-None inexpressible), and `groupdict()` whole-repr follows pyrst dict order (read by key); `re.match` is unspellable (`match` keyword) → `re.match_`; `re.findall` with **2+ capture groups** raises an honest `NotImplementedError` (CPython's list-of-tuples shape is inexpressible — use `finditer`); **finditer/findall reproduce CPython-3.7+ ZERO-WIDTH iteration** (Z1) — an empty match adjacent to a non-empty one IS emitted (`finditer("a*","aabaa")` = 4 matches; `findall("","abc")` = `['','','','']`), hand-rolled over `captures_at` since the regex crate's own iterators drop those empties; a **top-level lazy quantifier** (`*?`/`+?`/`??`) that yields a zero-width match is an honest `NotImplementedError` (CPython's must_advance re-match at the same position is inexpressible over the crate's leftmost-first API); a **`$` end-anchor against a newline-terminated subject** raises an honest `ValueError` (Z3) in search/match_/fullmatch/finditer/findall — CPython's `$` also matches just before a single trailing `\n`, unemulatable without look-around (strip the `\n` or match `\n?$`), while `is_match`/`find_all`/`sub`/`subn`/`split` keep the regex-crate `$` semantics unchanged; `sub` repl is literal (no `\1` expansion, no callable); **bare `if m:`** truthiness over `Optional[Match]` is a **check-time error** (Z4 — `check` and `build` now AGREE; before this it passed `check` and died at `rustc` E0308), use `if m is not None:` — real Optional truthiness is the tracked compiler follow-on (card 6a554b41); still no `re.Pattern` cache (per-call recompile — G1 `re.Pattern` follow-on); `escape()` skips `\v`/`\x0b` (no `\x` lexer escape) | ✅ dual-run — the full Match/group/span/finditer/groupdict/multibyte matrix in `parity_re_match.pyrs`; ⚠️ `parity_re.pyrs` stays pyrst-only (`is_match`/`find_all`/`match_` have no CPython attribute) |
| `json` | 4/5 | Pure-pyrst recursive-descent `loads` / serializer `dumps(v, indent=None, sort_keys=False, ensure_ascii=True)` over a tagged `JsonValue` class; surrogate-pair decoding | `JsonValue` is navigated via `.get(k)`/`.at(i)` methods, not `v["k"]`/`v[i]` subscripting (no dual `__getitem__` overload) — permanent, deliberate divergence; `load`/`dump` (file-object forms) deferred (no `file`-typed parameter spelling yet); (W1.5) `ensure_ascii=True` default matches CPython byte-for-byte (`\uXXXX` escapes, surrogate pairs for astral) and `dumps(v, indent=2, sort_keys=True)` keyword calls work | ⚠️ pyrst-only — `JsonValue`/`.get`/`.at` don't exist under CPython's real `json` (which returns native `dict`/`list`), so the API shape can't run unmodified against python3; serialized-string behavior is cross-checked separately |
| `random` | 4.5/5 | `Random` class (seedable) with `random randint randrange uniform getrandbits seed gauss normalvariate triangular gammavariate betavariate`; (W1.5) backed by **MT19937 with CPython's exact derivation chain** — `Random(seed)` sequences are BIT-IDENTICAL to CPython (`Random(42).random() == 0.6394267984578837`). (W4-c) plus the **CPython module-level convenience API** over a hidden global generator `_inst = Random(0)`: bare `random.seed random randint randrange uniform getrandbits` (SCALAR) **and** `random.choice sample choices` (GENERIC) | (W4-c) the module-level API is BYTE-IDENTICAL to CPython **after `random.seed(n)`**, interleaved scalar+generic draws included (one shared hidden generator advances across every call). UNSEEDED it is a fixed `Random(0)` (DETERMINISTIC — pyrst has no OS entropy, a documented divergence from CPython's entropy-seeded default, so seed first). **`random.shuffle` is NOT offered** — CPython mutates the caller's list in place and returns None, inexpressible under pyrst value semantics across a module boundary (EPIC-4 `Mut[T]`); it is an HONEST typeck error, and a shuffled COPY is `random.sample(xs, len(xs))` (a NEW list — NOT a shuffle-sequence-equivalence claim). PER-INSTANCE generic draws (`Random(s).choice(xs)`) are deferred — generic METHODS are gated (only free functions carry type params); seed the module generator and use the module draws. `randbytes` deferred (`bytes`, G7); `getrandbits` capped at 62 bits (i64), seeds are i64; `choices` `k` is keyword-only under CPython 3.12, but pyrst does **not yet enforce keyword-only parameters** (a tracked language item) — it accepts a positional `k` (a call CPython rejects), so **pass `k` by keyword** for compatibility; `cum_weights=` unavailable; `getstate`/`setstate` unavailable (use `seed(n)`) | ✅ dual-run vs python3: seeded METHOD surface (`parity_random.pyrs`); module-level SCALAR (`parity_random_moduleapi.pyrs`) + per-seed streams (`parity_random_moduleapi_seeds.pyrs`); module-level GENERIC draws + scalar/generic INTERLEAVING (`parity_random_moduleapi_draws.pyrs`). The `random.shuffle` honest-error is pinned by the negative `random_shuffle_fail.pyrs`; the pyrst-only `getrandbits` i64-cap divergence by `stdlib_random.pyrs` |
| `datetime` | 3.5/5 | `date`/`time`/`datetime`/`timedelta` classes: construction + range validation, comparisons, cross-type arithmetic, `isoformat`/`fromisoformat` (CPython-3.12-permissive: extended + basic forms, any single separator, 1–6 fractional digits), `strftime`/`strptime` core, `weekday`/`isocalendar`/ordinal | No `tzinfo`/`fold` (TZINFO gate) — `fromisoformat` rejects tz forms and (deferred) ISO-week/ordinal dates; `class datetime`'s factory API is FREE functions (`datetime_fromisoformat`, `datetime_combine`, …) because the class name shadows itself; `.min`/`.max`/`.resolution` are niladic methods; no multi-dispatch operator overloading (use `+ (-delta)` for `date - timedelta`) | ✅ dual-run (`date`/`time` surface incl. the fromisoformat matrix; `datetime` factory free-fns spot-checked) |
| `calendar` | 4/5 | `isleap leapdays weekday monthrange monthcalendar month calendar` text output; `firstweekday=` keyword parameter | `setfirstweekday()` raises `NotImplementedError` (no module-global mutable state, G2 — pass `firstweekday=` directly); `firstweekday()` is a stateless `0` stub; leap logic is a deliberate mirror of `datetime`'s (modules can't import each other) | ✅ dual-run |
| `colorsys` | 4.5/5 | 6 conversions (`rgb_to_yiq`/`yiq_to_rgb`/`rgb_to_hls`/`hls_to_rgb`/`rgb_to_hsv`/`hsv_to_rgb`) + `ONE_SIXTH`/`ONE_THIRD`/`TWO_THIRD` constants | Returns homogenized to `(float,float,float)`; `hsv_to_rgb` mirrors CPython's RAW-hue `int(h*6.0)`+`i%6` (negative/out-of-range hues, and even a negative output channel, pass through exactly); the sole divergence is `rgb_to_hls` out-of-`[0,1]` input → CPython `ZeroDivisionError` vs pyrst `inf` (a pyrst-wide float-division issue) | ✅ dual-run |
| `configparser` | 4/5 | `ConfigParser` (`read_string get getint getfloat getboolean set add_section sections options items has_section/option write`), `%`-`BasicInterpolation`, `DEFAULT` merge, `optionxform` | No custom exception classes (G — `ValueError`/`TypeError` with CPython-exact messages); interpolation depth mirrors CPython's `MAX_INTERPOLATION_DEPTH=10` accounting exactly | ✅ dual-run |
| `copy` | 2/5 | `copy(x)` / `deepcopy(x)` for the shapes pyrst can express | Shallow/deep copy only over concrete generic types; no `__copy__`/`__deepcopy__` hooks, no memo-dict exposure | ✅ dual-run |
| `csv` | 3/5 | `reader`/`writer`/`DictReader`/`DictWriter`/`Sniffer` over `str`/`list[str]` (no file objects, G7); `excel`/`excel-tab`/`unix` dialects; `QUOTE_*` modes | No file-object I/O — operates over whole strings; no `register_dialect` (G2); fields are `str` only (no numeric coercion); `csv.Error` → `ValueError`; `restkey` overflow lands in `DictRow.rest` | ⚠️ pyrst-only (str/list-based API differs from CPython's file-iterator shape) |
| `dataclasses` | 1/5 | `@dataclass` decorator no-ops onto pyrst's own class-synthesis (the `__init__`/`__repr__`/`__eq__` pyrst already generates) | Companion API (`field asdict astuple replace fields`) is INERT — raises honest errors; the module is essentially a compatibility shim, not a runtime implementation | ✅ dual-run (the `@dataclass` no-op path) |
| `difflib` | 3/5 | `SequenceMatcher` (`ratio`/`quick_ratio`/`get_matching_blocks`/`get_opcodes`/`get_grouped_opcodes`), `get_close_matches`, `unified_diff`, `ndiff`, `IS_LINE_JUNK`/`IS_CHARACTER_JUNK` | `SequenceMatcher(...)` positional calls must supply all 4 args; `isjunk`/`key` callables are int/str-specialized; autojunk + ratio/tie-break are CPython-exact | ✅ dual-run |
| `enum` | 2/5 | Class-const namespace pattern for enum-member access + name/value lookup | No metaclass machinery — members are class constants, not real `Enum` instances; message shapes match, but the dynamic `Enum` surface is out of reach | ⚠️ pyrst-only |
| `errno` | 3.5/5 | POSIX error-code constants + `errorcode(code)` LOOKUP FUNCTION (not a dict — G2) | `errorcode` is a function, not the CPython dict; constant VALUES verified host-exact | ✅ dual-run (`parity_errno`; `errorcode` shape pinned pyrst-only) |
| `filecmp` | 3.5/5 | `cmp`/`cmpfiles`/`dircmp` core comparison surface | `dircmp` is a class over concrete paths; missing-parent/uncatchable panics hardened to CPython errors | ⚠️ pyrst-only |
| `fnmatch` | 4/5 | `fnmatch`/`fnmatchcase`/`filter`/`translate` (built on `re`) | `translate()` string diverges from CPython for atomic-group multi-star runs, descending ranges, and interior-`[` escapes (all documented; MATCH behavior is CPython-correct — descending ranges match the literal set, not "nothing"); `filter` collides with the builtin under bare import (use qualified `fnmatch.filter`) | ✅ dual-run (incl. bracket/negation/interior-`[` matrix) |
| `fractions` | 3/5 | `Fraction` (construction, normalization, arithmetic, comparison, `limit_denominator`) | Backed by i64 numerator/denominator — overflows panic honestly instead of going bignum; no `Decimal`/`float` mixed-type promotion beyond what's expressible | ✅ dual-run |
| `getpass` | 3.5/5 | `getuser()` (env chain) + `getpass(prompt)` (interactive `@extern`) | `stream=` param dropped; interactive `getpass` is inherently non-dual-run (spot-checked); `getuser` env fallback chain matches CPython | ✅ dual-run (`getuser`) |
| `graphlib` | 4/5 | `TopologicalSorter` (`add`/`static_order`/`prepare`/`get_ready`/`done`/`is_active`), `CycleError` messages | `CycleError` → `ValueError` (no custom class); tie-break/cycle-message order is CPython-exact | ⚠️ pyrst-only (CPython-native `*predecessors` call shape differs) |
| `html` | 3.5/5 | `escape(s, quote=True)` + `unescape(s)` (the ~252-name `name2codepoint` core + numeric charrefs, backtracking) | `unescape` resolves `lang`/`rang` via the html5 values (U+27E8/U+27E9) where they differ from the HTML4 `name2codepoint`; only the full html5 alias table (`&LT;` etc.) is out of scope | ✅ dual-run |
| `io` | 5/5 | `StringIO` (`read`/`readline`/`readlines`/`write`/`writelines`/`getvalue`/`seek`/`tell`/`truncate`/`close`), full seek/pad matrix | Text `StringIO` only (no `BytesIO`, G7); NUL-padding seek verified in NUL-free form (harness `$()` strips NUL) | ✅ dual-run |
| `pathlib` | 4/5 | `PurePosixPath`: `parts name suffix suffixes stem parent parents joinpath with_name/stem/suffix relative_to is_relative_to match_ as_posix is_absolute` | Single-segment constructor/`joinpath` (no `*args`, chain instead); no `/` operator; `match` spelled `match_`; `relative_to(walk_up=True)` raises on anchor mismatch like CPython (the `'..'`-in-`other` corner remains a documented gap) | ⚠️ pyrst-only (single-`str` argument shape) |
| `platform` | 3.5/5 | `system machine release version python_version platform node` (registered under the real name `platform`) | Minimal subset; `platform(aliased=, terse=)` kwargs accepted-but-ignored (diverges from CPython's `terse=True`); env-coupled values verified host-exact | ⚠️ pyrst-only (ignored kwargs; env-coupled) |
| `pprint` | 4/5 | `pformat`/`pprint`/`pp`/`saferepr`/`isreadable`/`isrecursive`/`PrettyPrinter[T]` via generic "wrap `repr()`" text scanning; width-driven single-level wrapping; `sort_dicts` default; `underscore_numbers`; depth truncation | `compact=True` accepted-but-NOT-implemented (no multi-item-per-line packing — documented); long STRING atoms are NOT wrapped into `('a' 'b' …)` blocks (documented); wrapping is single-level, not fully recursive; `stream=` dropped | ✅ dual-run |
| `reprlib` | 2.5/5 | `Repr`/`repr` size-limited representation for the expressible shapes | Limited generic reach; no `recursive_repr` decorator; size limits match CPython for tested shapes | ⚠️ pyrst-only |
| `shlex` | 5/5 | `split(s, comments=, posix=)`/`join(parts)`/`quote(s)` — the 3 module-level functions | No `shlex.shlex` lexer class (Rust-std pure port of the functions) | ✅ dual-run |
| `shutil` | 3.5/5 | `copyfile copy copy2 copytree move rmtree which disk_usage` core file/tree ops | `copytree` now creates missing intermediate `dst` parents (`os.makedirs`); uncatchable Rust panics hardened to CPython errors (`SameFileError`→`ValueError`); `move` directory-into-subdir is a message-only divergence | ⚠️ pyrst-only |
| `stat` | 4/5 | `S_IS*` predicates + file-mode constants (`S_IMODE`/`S_IFMT`/`filemode` + `S_IF*`/`S_IR*`/`S_IW*`/`S_IX*`) | Constant VALUES verified host-exact | ✅ dual-run |
| `sys` | 3/5 | `maxsize platform version version_info exit argv` | **`argv`** (W4-b) is the process argument vector, a module-level mutable `list[str]` — the first W4 mutable-global unlock. **`argv[0]` diverges by construction** (a pyrst binary path vs python3's `-c`), so a program must observe `argv[1:]`/`len(sys.argv)`, never `argv[0]`; the parity harness threads identical args to both sides via an anchored `# argv:` directive and asserts `argv[1:]`/`len` only (`examples/parity_sys_argv_cli`, `parity_sys_argv_noargs`). Writes are owner-only: `sys.argv = …` / `sys.argv.append(…)` from user code is a cross-module honest error, qualified reads work; reads clone (value semantics). `stdin`/`stdout`/`stderr` remain deferred — NOT for module-level-mutable-state (that shipped in W4) but as opaque stream handles (G1/W5); `print()`/`input()` cover the common cases. `version` is a pyrst identity string (documented) | ✅ dual-run (`argv` incl.; `maxsize`/`platform`/`exit`) |
| `tempfile` | 3.5/5 | `gettempdir`/`mkdtemp`/`mkstemp`/`NamedTemporaryFile`-ish surface | `mkdtemp`/`mkstemp` create owner-only `0o700`/`0o600` objects (security-hardened to match CPython, not the umask default); stream/opaque-handle shapes limited | ⚠️ pyrst-only |
| `urllib.parse` | 3/5 | First non-`os` DOTTED stdlib package (W3, `lib/urllib/parse.pyrs`), pure pyrst: `urlparse` (→ `@dataclass ParseResult` with `.scheme`/`.netloc`/… + `geturl()`), `urlunparse`, full RFC 3986 `urljoin`, `quote`/`quote_plus`/`unquote`/`unquote_plus` (hand-rolled bit-level UTF-8 percent-encoding — pyrst has no `bytes`), `urlencode`, `parse_qs`/`parse_qsl` | `ParseResult` is a `@dataclass`, not a NamedTuple — attribute access only, no tuple-unpacking; qualified `urllib.parse.ParseResult(...)` construction is the documented qualified-class-ctor gap (use `from urllib.parse import ParseResult` or the `urlparse()` factory); `quote`/`unquote` family drops `encoding=`/`errors=` (fixed at CPython's UTF-8 defaults); `urlencode` takes `dict[str,str]` only and iterates in SORTED-key order, not insertion order; `parse_qs`/`parse_qsl` drop the option kwargs; no WHATWG control-char stripping or IPv6-bracket validation on malformed input | ✅ dual-run (incl. the RFC 3986 §5.4 normal + abnormal resolution matrices, astral-plane quote/unquote round trips; core algorithms fuzz-verified against CPython across ~600k cases) |
| `logging` | 3/5 | (W4-d) the ROOT logger over module-level mutable state (`_root_level`/`_configured`): `basicConfig(level=)` + `debug`/`info`/`warning`/`error`/`critical` (level-gated, emit `LEVEL:root:msg` to **stderr** exactly like CPython) + `getLevelName` + level consts `DEBUG`/`INFO`/`WARNING`/`ERROR`/`CRITICAL`/`NOTSET` | Root logger ONLY — `getLogger`/named-logger hierarchy, handlers, formatters, propagation, a module-level `setLevel` are honest typeck errors (never silent no-ops); `basicConfig` takes only `level` (no `handlers=`/`format=`/`stream=`/`force=`), and its repeat-call NO-OP + the implicit-config-on-first-log are CPython-faithful (probes P4/P5); msg-only signatures — no lazy `%`-interpolation (no `*args`), so ANY extra positional arg is an honest ARITY error. That rejection covers the WHOLE multi-arg shape, INCLUDING CPython's canonical matched idiom `logging.warning("x %s", "y")`, which CPython interpolates CLEANLY to `x y` (exit 0) — pyrst is honest-STRICTER there and you pre-format with an f-string; the MISMATCH shape `logging.warning("x", "y")` is where CPython itself degrades to an internal "Logging error" + traceback with nothing interpolated, still exit 0 (probe P9b); `getLevelName` forward (int→str) only — the reverse str→int direction is a return-type union, deferred | ⚠️ pyrst-only for the stderr surface (the stdout-only harness cannot byte-compare stderr; `parity_logging`/`parity_logging_basicconfig`/`parity_logging_warnings_interleave` pin the python3 stderr as oracle evidence). ✅ dual-run for the no-stderr surface — level consts + `getLevelName` (`parity_logging_levels`) |
| `warnings` | 3/5 | (W4-d) `warn(message, category="UserWarning")` + `simplefilter(action)` over module-mutable filter state (`_action` + a `_seen` dedup set): CPython's full action set `ignore`/`always`/`once`/`default`/`error`/`module`; category names `UserWarning`/`DeprecationWarning` in the output; emits `Category: message` to **stderr** exactly like CPython | The `<file>:<lineno>:` location prefix + source-line echo are omitted (pyrst has no Python call frame/linecache — and CPython's own values are corrupted under the harness exec-prepend, probe W1b); the category+message tail is byte-exact; `"default"` is approximated as once-per-MESSAGE (CPython is per-LOCATION — pyrst has no call-site lineno); `category` is a NAME STRING, not a Warning class (documented convention divergence); `"module"` is per-MESSAGE dedup (CPython is per-`(module, message)` — EXACT for a single-module program, diverging only when the same message is warned from two modules); `"error"` raises the warning as a REAL, catchable Warning at `warn()` time (`except UserWarning`/`except DeprecationWarning` catch it, matching CPython — probe D3b), and uncaught it panics (exit 101) where CPython exits 1 (the globally-documented panic-exit divergence); an UNKNOWN action raises `AssertionError: invalid action: '<action>'` at the call, matching CPython 3.12 (probe D3f); unknown category NAMES stay an honest raise; `filterwarnings`/`catch_warnings`/warning classes are honest typeck errors | ⚠️ pyrst-only for the stderr surface — `parity_warnings` (default + DeprecationWarning + ignore), `parity_warnings_once_always` (once-vs-always dedup), and `parity_warnings_module` (the `module` per-message dedup) pin the python3 stderr as oracle evidence. ✅ dual-run for `parity_warnings_error_caught` — `simplefilter("error")` then `warn()` raises a `UserWarning` that `except UserWarning` catches byte-identically to CPython (the caught path; the uncaught path diverges only on the documented panic-exit code) |

**Not planned (out of scope by design, `docs/design/stdlib-full.md` §C):** concurrency/async (`asyncio threading multiprocessing concurrent`, …) — pyrst is single-threaded with no `Send`/async runtime; runtime introspection/dynamic (`ast inspect gc importlib pickle marshal dis`, …) — no runtime object model or `eval`/`exec`; C-FFI/low-level OS (`ctypes mmap fcntl signal`, …) — no unsafe FFI story; GUI/interactive/dev-tooling (`tkinter turtle unittest pdb`, …) — outside a compiled language's remit; legacy "dead battery" modules removed upstream in Python 3.13 (PEP 594); the networking stack (`socket ssl http urllib xml email`, …) — needs a socket/TLS layer pyrst doesn't have.

The 26 modules from `datetime` through `tempfile` in the table above shipped in **wave W2** (`docs/design/stdlib-full.md` §F). The **dotted-submodule epic landed in W3**: `os.path`, `urllib.parse`, and `collections.abc` are the first W3 modules (`docs/design/w3-modules.md`). Everything else — `argparse`, `logging`, `sqlite3`, `hashlib`, and roughly 50 more modules — is **planned but not yet shipped** (waves W3–W5), sequenced behind the remaining named compiler epics (module-level mutable state, an opaque-handle type, a `bytes` type) rather than hidden inside a module card.

---

## Advanced Features

| Feature | Status | Notes |
|---------|--------|-------|
| Context managers / `with` | ⚠️ Files only | `with open(...) as f:` works (the handle is closed via RAII on scope exit). The general context-manager protocol over a **user class** is an **honest typeck error** — `with Guard(...) as g:` would silently skip `__enter__`/`__exit__`, so it is rejected (`context-manager protocol … not yet supported`). Call the methods explicitly. Full support is blocked on real exception objects (pyrst `raise` = panic with a string-encoded type; `__exit__` needs the exception value/traceback and suppression semantics). |
| Operator overloading | ✅ Supported | Dunder methods (see Classes) |
| Generators / `yield` | ✅ Supported (lazy) | `Iterator[T]`-returning functions; on-demand execution, infinite generators OK — see [Generators (`yield`)](#generators-yield) below |
| Coroutines / `async` / `await` | ❌ Not Supported | Not in current roadmap |
| `global` (module-level mutable state) | ✅ Supported | A module binding rebound under `global` (or with a non-scalar-literal initializer) lowers to a `thread_local!` `Cell`/`RefCell` mutable static; a scalar-literal, never-rebound binding stays an immutable `const`. Mutation (`items.append(x)`) needs no `global`; a rebind (`items = …`) needs it — CPython-faithful. See [Module-Level Mutable State](#module-level-mutable-state-global) for the documented divergences. |
| `nonlocal` | ❌ Not Supported | Rebinding an enclosing function's local from a closure needs shared-mutable frame capture, which EPIC-4 clone-on-capture value semantics disallow — honest typeck error (use a class field, a returned value, or a module `global`). |
| Decorators (general) | ⚠️ Partial | Only `@dataclass`/`@staticmethod`/`@property` |
| Descriptors | ❌ Not Supported | Not part of the object model |
| Metaclasses | ❌ Not Supported | Not supported |
| Reflection (`inspect`) | ❌ Not Supported | No runtime introspection |
| Multiple inheritance | ❌ Not Supported | Single inheritance only |
| Abstract base classes | ❌ Not Supported | No ABC support |
| `typing` module metadata | ⚠️ Partial | Static types enforced; no runtime metadata |

---

## Module-Level Mutable State (`global`)

A module-level binding becomes a `thread_local!` mutable static (a `Cell<T>` for a
Copy scalar, a `RefCell<T>` for a `str`/`list`/`dict`/`set`/user class) when either
(a) some function declares `global NAME` and rebinds it, or (b) its initializer is
not a scalar literal (a container/constructor/`@extern` call). A scalar-literal,
never-rebound binding keeps the immutable `const` path unchanged. Initializers run
**eagerly top-down at startup** (`__pyrst_init_globals()` before `main()`), matching
CPython's import-time order (a root initializer that textually precedes an `import`
runs before the imported module's; imported modules interleave around their own
imports, DFS with a visited set). Reads clone out of the cell (value semantics);
rebinds `set`/`*borrow_mut() =`; in-place mutations `borrow_mut().push(…)`. Live at
call time inside closures.

**Documented divergences (honest gaps, not silent miscompiles):**

- **Clone-on-read snapshot semantics (EPIC-4).** Reading a global *clones* it, so a
  binding captured before a later mutation is an independent **snapshot**: `xs = g`
  then `g.append(4)` leaves `xs == [1, 2, 3]` in pyrst where CPython **aliases** it
  (`xs == [1, 2, 3, 4]`). This is pyrst's uniform value-semantics contract (no
  `Rc<RefCell>` aliasing); full alias fidelity is the EPIC-4 `Mut[T]` surface.
- **`del items[i]` on an indexed element is an honest error (W4-b).** `del` on a
  subscript — a list index (`del xs[0]`), a dict key (`del d[k]`), or a qualified
  module global (`del sys.argv[0]`) — is a **check-time typeck error**. It previously
  lowered to a discarded clone-and-drop that silently removed *nothing* (a
  byte-divergence from CPython, which removes the element); the guard converts that
  silent no-op into a loud rejection naming the remedy — `items.pop(i)` to remove a
  list element, `d.pop(k)` to remove a dict entry, or a whole-container rebind under
  `global`. (Bare `del name` and `del obj.attr` are unaffected.)
- **Forward-reference detection is DIRECT-only.** A module global whose initializer
  *directly* references a name defined later in the module (`x: int = y + 1` before
  `y`, or `a: int = helper()` before `def helper`) is an honest check error (CPython
  raises `NameError` at import). A **transitive** forward read — an initializer that
  calls an *earlier-defined* function whose body reads a *later-defined* global — is
  **not** caught and still diverges from CPython's import-time `NameError`. This
  residual is out of W4-a scope; keep an initializer's transitive reachability
  self-contained.
- **`nonlocal` and cross-module writes are deferred.** `nonlocal` is an honest
  typeck error (closures capture by value). A `global NAME` that names a binding
  living only in an *imported* module (or a builtin stub like `int`) is an honest
  error — owner-module rebinds only; cross-module writes (`import m; m.x = 5`, or an
  in-place `m.items.append(x)`) are a v1 deferral (qualified *reads* `m.x` work).
  **`sys.argv` (W4-b) is the first worked example:** reads (`sys.argv[i]`,
  `sys.argv[1:]`, `len(sys.argv)`) work everywhere, but `sys.argv = […]`, an
  in-place mutator (`sys.argv.append(…)`), and an element/`del` write
  (`sys.argv[0] = …`, `del sys.argv[0]`) from user code are honest
  cross-module-write / indexed-`del` errors — while a **non-mutating** qualified
  method (`sys.argv.count(x)`) is a read and works (on the clone). `argv[0]` differs
  from CPython by construction (binary path vs `-c`) — observe `argv[1:]` only.
  Binding it to a local **clones** it: `x = sys.argv; x.append(y)` mutates the copy
  and leaves `sys.argv` unchanged — a deliberate **divergence from CPython**, where
  `sys.argv` is a shared list and that `append` *would* mutate it (the universal
  EPIC-4 value-semantics model, not an argv special case).

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
*else* branch (when there is no intervening `elif`). Beyond that in-branch
narrowing, three CPython-idiomatic shapes are supported:

- **Negative narrowing persists past the guard (early-return idiom).** When the
  `then` block of `if x is None:` *terminates* — `return`, `raise`, `break`, or
  `continue`, or a total nested `if`/`match`/`try` that does — the code AFTER the
  `if` is reached only when `x is not None`, so `x` narrows to `T` for the rest of
  the enclosing scope:

  ```python
  def first_or_zero(x: Optional[int]) -> int:
      if x is None:
          return 0
      return x + 1        # x is `int` here — the guard already returned on None
  ```

- **A narrow dies at a loop boundary.** A narrow *born inside* a `for`/`while`
  body does not leak past the loop — after the loop the name is `Optional[T]`
  again (the loop may run zero times). Using the un-narrowed value after the loop
  is an honest check error, never a leak.

- **Reassignment re-widens.** Assigning a fresh `Optional[T]` (or `None`) to a
  narrowed name kills the narrow; the name is `Optional[T]` again from that
  assignment onward.

- **`while`-traversal narrowing.** `while cur is not None:` narrows `cur` to `T`
  inside the body; the loop-carried `cur = cur.next` reconverges to the outer
  `Optional` slot that the loop header re-tests — the linked-list traversal idiom
  (pairs with self-referential `Optional` fields).

**Attribute/field chains are not narrowed.** `if o.slot is not None: o.slot.v` is
an honest error, not a silent miscompile: an intervening call or assignment could
invalidate the guard between the test and the use, so pyrst does not flow-narrow a
*field place* (the soundness wall CPython's own type-checkers hit). Bind the
Optional to a **local** first, then narrow the local:

```python
s = o.slot
if s is not None:
    use(s.v)            # narrow the local, which cannot be aliased away
```

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
- **Builtin runtime errors ARE catchable by their Python exception type:** an out-of-bounds subscript or `pop()` from an empty list raises `IndexError`; a missing dict key raises `KeyError`; `list.remove`/`list.index`/`str.index` misses, a zero slice step, a negative integer `**=` exponent, and failed `int()`/`float()` parses raise `ValueError`; division/modulo by zero raises `ZeroDivisionError`; file I/O failures raise `OSError` (exact-name match). The builtin hierarchy applies (`except LookupError:` catches `IndexError`/`KeyError`). Uncaught, they abort via a Rust panic: the exception type name and message print on stderr and the process exits **101** — whereas CPython prints a multi-line traceback and exits **1**. The exception type name and message match; the traceback format and the exit code are the documented divergence (e.g. an uncaught `int(sys.argv[1])` on non-numeric input aborts 101 in pyrst, tracebacks-and-exits-1 in CPython).

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
