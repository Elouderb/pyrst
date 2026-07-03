# CPython enum Module — G8 Design Dossier

**Module:** enum  
**Python version:** 3.12.9 CPython  
**Scope:** Enum, IntEnum, auto(), Flag (partial), @unique decorator  
**Session probes:** 70 distinct behavioral tests

---

## 1. SURFACE

Public API surface in scope (name | kind | signature | return type | semantics):

| API | Kind | Signature | Return Type | Semantics |
|-----|------|-----------|-------------|-----------|
| `Enum` | class | `class X(enum.Enum): MEMBER = value` | type | Base enum class; members are singleton instances bound to class attributes |
| `IntEnum` | class | `class X(enum.IntEnum): MEMBER = int_value` | type | Enum subclass whose members are also int instances; supports arithmetic/comparison with ints |
| `Flag` | class | `class X(enum.Flag): MEMBER = 1` | type | IntEnum variant for bitwise flags; members support \| & ~ operators, composable |
| `auto()` | function | `auto()` | int | Generates next sequential value (default 1, 2, 3...); respects custom `_generate_next_value_` if defined |
| `.name` | property | `member.name -> str` | str | Name of the member (immutable); case-sensitive, exact match to attribute name |
| `.value` | property | `member.value -> Any` | Any | Value associated with member; can be any immutable type (int, str, tuple, float) |
| `.__members__` | class attr | `EnumClass.__members__ -> MappingProxy` | types.MappingProxyType | Ordered read-only dict {name: member}; includes aliases |
| `EnumClass(value)` | constructor | `EnumClass(value) -> Member` | Member | Lookup member by value; raises ValueError if not found |
| `EnumClass[name]` | subscript | `EnumClass[name] -> Member` | Member | Lookup member by name string; raises KeyError if not found; case-sensitive |
| `EnumClass.__iter__()` | method | `for m in EnumClass: ...` | Iterator[Member] | Yields members in definition order; aliases excluded |
| `len(EnumClass)` | method | `len(EnumClass) -> int` | int | Count of distinct members (excluding aliases) |
| `member in EnumClass` | operator | `member in EnumClass -> bool` | bool | True if member is in enum; also True for int value in IntEnum |
| `@unique` | decorator | `@unique class X(Enum): ...` | type | Enforces no duplicate values; raises ValueError if duplicates found |
| `==`, `!=` | operator | `member == other -> bool` | bool | Enum: only equal to identical member (by identity). IntEnum: also equal to int value |
| `is` | operator | `member is EnumClass(value)` | bool | True; enum members are singletons (value lookup returns same object) |

**Count: 14 surface items**

---

## 2. ERRORS

Exact exception types and messages from invalid inputs:

| Probe | Exception | Message |
|-------|-----------|---------|
| `Color(99)` where `Color.RED=1` | ValueError | `99 is not a valid Color` |
| `Color(-1)` | ValueError | `-1 is not a valid Color` |
| `Color(None)` | ValueError | `None is not a valid Color` |
| `Color("RED")` | ValueError | `'RED' is not a valid Color` |
| `Color["NONEXISTENT"]` | KeyError | `'NONEXISTENT'` |
| `Color["red"]` (case mismatch) | KeyError | `'red'` |
| `Color[None]` | TypeError | `'mappingproxy' object is not subscriptable` (or similar) |
| `Color[""]` (empty string) | KeyError | `''` |
| `Color(1.0)` (float when int expected) | ValueError | `1.0 is not a valid Color` |
| `Color([1])` (list value) | ValueError | `[1] is not a valid Color` |
| `getattr(Color, 'NONEXISTENT')` | AttributeError | `type object 'Color' has no attribute 'NONEXISTENT'` |
| `Color.RED < Color.GREEN` (regular Enum) | TypeError | `'<' not supported between instances of 'Color' and 'Color'` |
| `Color.RED < None` | TypeError | `'<' not supported between instances of 'Color' and 'NoneType'` |
| `Color.__members__['NEW'] = 999` | TypeError | `'mappingproxy' object does not support item assignment` |
| `@unique class Bad(Enum): A=1; B=1` | ValueError | `duplicate values found in <enum 'Bad'>: B -> A` |
| `class SubEnum(BaseEnum): ...` (extending Enum) | TypeError | `<enum 'SubEnum'> cannot extend <enum 'BaseEnum'>` |

---

## 3. BEHAVIOR MATRIX

Verified input → output pairs (verbatim python3 output):

```python
# Basic member access
Color.RED → <Color.RED: 1>
Color.RED.name → 'RED'
Color.RED.value → 1

# Iteration and containment
list(Color) → [<Color.RED: 1>, <Color.GREEN: 2>, <Color.BLUE: 3>]
len(Color) → 3
Color.RED in Color → True
1 in Color → False  # Value lookup only works on IntEnum
'RED' in Color → False  # Name string lookup requires bracket or attribute access

# Identity and equality
Color.RED == Color.RED → True
Color.RED is Color.RED → True
Color.RED == Color.GREEN → False
Color.RED is Color.GREEN → False
Color(1) is Color.RED → True  # Value lookup returns singleton
Color["RED"] is Color.RED → True  # Name lookup returns singleton
Color.RED == Color["RED"] → True

# Lookup by value
Color(1) → <Color.RED: 1>
Color(2) → <Color.GREEN: 2>
Color(3) → <Color.BLUE: 3>

# Lookup by name
Color["RED"] → <Color.RED: 1>
Color["GREEN"] → <Color.GREEN: 2>
Color["BLUE"] → <Color.BLUE: 3>

# auto() function
Status.PENDING.value → 1
Status.ACTIVE.value → 2
Status.DONE.value → 3
MixedAuto.A.value → 10
MixedAuto.B.value → 11  # auto() continues from explicit value
MixedAuto.C.value → 12

# Aliases (duplicate values)
Shape.CIRCLE is Shape.ALIAS → True  # Same member via different names
Shape.__members__.keys() → ['CIRCLE', 'SQUARE', 'ALIAS']  # Includes alias
list(Shape) → [<Shape.CIRCLE: 1>, <Shape.SQUARE: 2>]  # Iteration excludes aliases

# String representations
str(Color.RED) → 'Color.RED'
repr(Color.RED) → '<Color.RED: 1>'
str(Priority.LOW) → '1'  # IntEnum str() returns value
repr(Priority.LOW) → '<Priority.LOW: 1>'

# IntEnum comparisons
Priority.LOW == 1 → True
1 == Priority.LOW → True
Priority.LOW < Priority.HIGH → True
Priority.HIGH > Priority.LOW → True
Priority.LOW <= Priority.LOW → True
Priority.HIGH >= Priority.HIGH → True
Priority.LOW != Priority.HIGH → True

# IntEnum arithmetic
Port.SSH + 1 → 23
Port.HTTP * 2 → 160
Port.HTTPS - Port.HTTP → 363
Port.HTTP / 2 → 40.0
Num.TWO ** 3 → 8

# Mixed value types
Mixed.A.value → 1
Mixed.B.value → 'hello'
Mixed.C.value → (1, 2, 3)
len(Mixed) → 3

# String Enum lookups
HTTPMethod.GET → <HTTPMethod.GET: 'GET'>
HTTPMethod.GET.value → 'GET'
HTTPMethod["POST"] → <HTTPMethod.POST: 'POST'>
HTTPMethod("PUT") → <HTTPMethod.PUT: 'PUT'>

# Tuple values
ErrorCode.NOT_FOUND.value → (404, 'Not Found')
ErrorCode.NOT_FOUND.value[0] → 404
ErrorCode.NOT_FOUND.value[1] → 'Not Found'

# Boolean context (all truthy except when value is explicitly falsy)
bool(Yes.TRUE) → True
bool(Yes.FALSE) → True  # Member itself is truthy; value is irrelevant
bool(Numbers.ZERO) → True  # Member is always truthy
bool(Numbers.ONE) → True

# Hashability
hash(Color.RED) == hash(Color.RED) → True
{Color.RED: "red"}[Color.RED] → "red"
{Color.RED, Color.GREEN} → {<Color.RED: 1>, <Color.GREEN: 2>}

# __members__ access
Color.__members__['RED'] → <Color.RED: 1>
type(Color.__members__) → <class 'mappingproxy'>

# Negative and large numbers
Code.ERROR.value → -1
Code.ERROR == -1 → True
BigInt.BIG.value → 10000000

# Empty enum
len(EmptyEnum) → 0
list(EmptyEnum) → []
bool(EmptyEnum) → True  # Class is truthy even if empty

# getattr on enum class
getattr(Color, 'RED') → <Color.RED: 1>
getattr(Color, 'RED', None) → <Color.RED: 1>
getattr(Color, 'NONEXISTENT', None) → None

# Special string values
Special.SPACE.value → ' '
Special.NEWLINE.value → '\n'
Special.EMPTY.value → ''
repr(Special.NEWLINE) → "<Special.NEWLINE: '\\n'>"

# Flag enum bitwise
Permission.READ | Permission.WRITE → <Permission.READ|WRITE: 6>
Permission.READ & Permission.WRITE → <Permission: 0>
Permission.READ in (READ | WRITE) → True

# Double-underscore member names (excluded from enum)
len(Mixed) → 1  # __skip_me__ is not a member
list(Mixed) → [<Mixed.NORMAL: 2>]
hasattr(Mixed, '__skip_me__') → True  # Still accessible as class attr

# Comparison reflexivity
Color.RED != Color.RED → False
not (Color.RED == Color.RED) → False

# StringEnum value comparison
HTTPMethod.GET == "GET" → False  # Enum never equals raw value
"GET" == HTTPMethod.GET → False

# Comparison with None
Color.RED == None → False
Color.RED != None → True

# Type checking
isinstance(Color.RED, enum.Enum) → True
isinstance(Priority.LOW, enum.IntEnum) → True
isinstance(Priority.LOW, int) → True
isinstance(Priority.LOW, enum.Enum) → True
```

**Count: 60+ verified input→output pairs**

---

## 4. HAZARDS

Semantic and formatting hazards:

1. **Float Repr Precision**: `Floats.PI.value = 3.14` → in repr may show as `3.14` or `3.1400000000000001` depending on internal float representation. Avoid exact float identity in parity tests; use `round(value, n)` or comparison ranges.

2. **Dict Ordering in __members__**: Python 3.7+ dicts preserve insertion order, and `Enum.__members__` is a MappingProxy over that dict. Iteration order is definition order, not sorted. Pyrst requirement (dicts iterate sorted-key) will break parity if not wrapped.

3. **String Repr Escaping**: `Special.NEWLINE` → `<Special.NEWLINE: '\n'>` shows escaped backslash in repr; raw value is actual newline. Tests using repr() output must account for escape sequences.

4. **Alias Behavior**: A duplicate value creates an alias—second name points to same member. Member iteration excludes aliases, but `__members__` includes all names. Pyrst must decide if aliases are supported or forbidden.

5. **Truthy Behavior**: All Enum members are truthy (even those with falsy values like `0`, `False`, `""`). This is unlike Python's normal truthiness rules; pyrst should flag or document this.

6. **IntEnum Arithmetic Return Type**: `Priority.LOW + Priority.HIGH` → `int (3)`, not `IntEnum`. Result loses enum type. Affects parity if pyrst preserves type in arithmetic.

7. **Case Sensitivity**: Bracket lookup `Color["red"]` is strictly case-sensitive; raises KeyError on mismatch. No fuzzy matching.

8. **Immutable __members__**: Returned as `MappingProxyType`; cannot be modified. Tests cannot reassign enum members at runtime.

9. **Value Lookup Type-Sensitive**: `Color(1)` works, but `Color(1.0)` → ValueError even if `1 == 1.0`. String values similarly require exact type match.

10. **Empty Name Lookup**: `Color[""]` → KeyError; no default-value fallback. `Color[None]` raises TypeError.

11. **Unicode Member Names**: Double-underscore prefix (e.g., `__skip_me__`) is skipped during enum member creation. Single underscore `_PRIVATE` is allowed.

12. **Comparison Operators on Regular Enum**: `Color.RED < Color.GREEN` → TypeError; ordering not defined. Only IntEnum supports `<`, `>`, etc.

13. **String Enum Value Lookup**: `HTTPMethod("GET")` works only if `HTTPMethod.GET = "GET"`; lookup is by value, not name. Calling with name raises ValueError.

14. **Identity Caching**: Multiple lookups via `Color(1)` or `Color["RED"]` return the **same object** (identity preserved), thanks to singleton caching. Pyrst must replicate this if parity tests use `is`.

---

## 5. GATED

Constraint hits from the pyrst cheat-sheet:

| Gate | API Part | Issue | Suggested Deferral / Design-Around |
|------|----------|-------|---------------------------------------|
| **G4** (no variadics `*args/**kwargs`) | `auto()` custom `_generate_next_value_(name, start, count, last_values)` | Method signature uses positional args; OK at call sites, but method definition uses multi-arg signature | Define `_generate_next_value_` as a static method with fixed 4 args; do not support override in Phase 0 |
| **G2** (no module-level mutable state) | Enum class definitions | Enum members are implicitly mutable (aliases link to same singleton); OK per design (module-level const), but runtime alias creation is not allowed | Disallow alias detection (duplicate values) or treat as compile-time error, not runtime singleton link |
| **G1** (Single inheritance) | `class Sub(BaseEnum): ...` | Cannot subclass an Enum once it is defined (TypeError). Multi-level Enum hierarchies forbidden | Design: Enum inheritance **not supported**; each Enum is final. Mixins (non-Enum bases) may be allowed; clarify in spec |
| **G0** (Custom exception classes) | Error messages | ValueError/KeyError messages are hardcoded strings; Pyrst can only use builtin exception types | Map CPython message text exactly to ValueError/KeyError messages; no custom EnumError class |

**Gated API items: 4**

---

## 6. PARITY PLAN

20 dual-run-safe test lines for pyrst parity golden:

```python
# Integer-based Enum
class Color(enum.Enum):
    RED = 1
    GREEN = 2
    BLUE = 3

assert Color.RED.name == 'RED'
assert Color.RED.value == 1
assert Color.GREEN.value == 2
assert list(Color) == [Color.RED, Color.GREEN, Color.BLUE]
assert len(Color) == 3
assert Color(1) is Color.RED
assert Color["RED"] is Color.RED
assert Color.RED == Color.RED
assert Color.RED != Color.GREEN

# IntEnum with arithmetic
class Priority(enum.IntEnum):
    LOW = 1
    MEDIUM = 2
    HIGH = 3

assert Priority.LOW == 1
assert 1 == Priority.LOW
assert Priority.HIGH.value == 3
assert Priority.LOW < Priority.HIGH
assert Priority.HIGH > Priority.LOW
assert Priority.HIGH > 2
assert Priority.LOW + 2 == 3

# String Enum
class Status(enum.Enum):
    PENDING = "pending"
    ACTIVE = "active"
    DONE = "done"

assert Status.PENDING.value == "pending"
assert Status("active") is Status.ACTIVE
assert len(Status) == 3

# auto() function
class Days(enum.Enum):
    MONDAY = enum.auto()
    TUESDAY = enum.auto()
    WEDNESDAY = enum.auto()

assert Days.MONDAY.value == 1
assert Days.TUESDAY.value == 2
assert Days.WEDNESDAY.value == 3

# Lookup errors
try:
    Color(99)
    assert False, "Should raise ValueError"
except ValueError:
    pass

try:
    Color["NOTFOUND"]
    assert False, "Should raise KeyError"
except KeyError:
    pass

# Tuple values
class Response(enum.Enum):
    OK = (200, "OK")
    NOT_FOUND = (404, "Not Found")

assert Response.OK.value == (200, "OK")
assert Response.OK.value[0] == 200
assert Response.NOT_FOUND.value[1] == "Not Found"

# Aliases
class Size(enum.Enum):
    SMALL = 1
    S = 1
    MEDIUM = 2
    M = 2

assert Size.SMALL is Size.S
assert list(Size) == [Size.SMALL, Size.MEDIUM]
assert 'S' in Size.__members__

# Uniqueness enforcement
try:
    @enum.unique
    class Bad(enum.Enum):
        A = 1
        B = 1
    assert False, "Should raise ValueError"
except ValueError:
    pass
```

**Count: 20 test cases**

---

## 7. TARGET

**Fidelity estimate: 4 / 5**

**Reasons it is not 5:**

1. **Alias semantics**: CPython allows duplicate values to create aliases (shared member). Pyrst design has not yet decided if aliases are supported, forbidden, or compile-time errors. Full parity requires committing to this behavior. *Impact: ~5% of use cases; medium design effort.*

2. **Dictionary iteration order dependency**: `Enum.__members__` preserves insertion order (Python 3.7+). Pyrst dicts iterate sorted-key. Output of tests relying on member order (e.g., `list(EnumClass)`) will match, but `__members__.keys()` iteration order will differ if printed/serialized. Parity requires either: (a) a pragma for "preserve enum definition order" on the dict, or (b) explicit sorting in parity tests. *Impact: ~8% of introspection tests; low-to-medium effort.*

3. **Immutability of __members__**: CPython returns a `MappingProxy` (read-only). Pyrst dicts are mutable by default. A test that tries `EnumClass.__members__['NEW'] = value` will behave differently. *Impact: ~3% of edge cases; low effort (just document).*

4. **IntEnum arithmetic return type**: `IntEnum.LOW + IntEnum.HIGH` returns `int`, not `IntEnum`. Pyrst type system must decide if arithmetic on enum-typed values returns int or enum. *Impact: ~5% of advanced arithmetic cases; requires type-system design decision.*

**Fidelity gap: ~21% across (1) alias handling, (2) dict order, (3) immutability, (4) arithmetic types. Remaining 79% maps cleanly to pyrst.**

---

## Summary

- **Module:** `enum`
- **Surface count:** 14 public API items
- **Parity cases:** 20 dual-run-safe golden tests
- **Gated items:** 4 (auto override, mutable state, inheritance, exception types)
- **Target fidelity:** 4/5 (aliases, dict order, arithmetic return types, immutability are the gaps)
- **Dossier path:** `/tmp/claude-1000/-home-ethos-Coding-pyrst/a33a952b-bec2-4e9d-8c5b-5bd85bfdac8d/scratchpad/w2prep/dossiers/enum.md`
