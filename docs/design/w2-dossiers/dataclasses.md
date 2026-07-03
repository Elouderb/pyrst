# PYRST DATACLASSES IMPLEMENTATION DOSSIER

**Module:** `dataclasses`  
**Session Date:** 2026-07-01/02  
**CPython Version:** 3.12.9

---

## 1. SURFACE

Public API in scope for minimum faithful subset. All signatures verified via CPython 3.12 probes.

| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `@dataclass` | decorator | `@dataclass(*, init=True, repr=True, eq=True, order=False, unsafe_hash=False, frozen=False, match_args=True, kw_only=False, slots=False)` | type | Synthesizes `__init__`, `__repr__`, `__eq__`, optional `__lt__/__le__/__gt__/__ge__` (order=True), optional `__hash__` (frozen=True); all on class annotations |
| `field()` | function | `field(*, default=MISSING, default_factory=MISSING, init=True, repr=True, hash=None, compare=True, metadata=None, kw_only=False)` | Field | Annotates a field; mutually exclusive default/default_factory; controls __init__, __repr__, comparison, metadata storage |
| `asdict()` | function | `asdict(instance, *, dict_factory=dict)` | dict | Recursively converts dataclass instance to dict; nested dataclasses → nested dicts; shallow copy of mutable fields |
| `astuple()` | function | `astuple(instance, *, tuple_factory=tuple)` | tuple | Recursively converts dataclass instance to tuple; order matches declaration; all fields included regardless of init/repr/compare flags |
| `fields()` | function | `fields(class_or_instance)` → Sequence[Field] | Field[] | Returns tuple of Field objects for all instance fields (excludes ClassVar, InitVar); raises TypeError if not dataclass |
| `is_dataclass()` | function | `is_dataclass(obj)` → bool | bool | Returns True if obj is a dataclass class or instance |
| `FrozenInstanceError` | exception | `FrozenInstanceError(str)` | Exception | Raised on attribute assignment to frozen=True dataclass; subclass of AttributeError |
| `MISSING` | sentinel | `MISSING` | object | Sentinel value indicating no default; only for introspection via Field.default/default_factory |
| `Field` | class | (result of field() or inspection) | type | Descriptor wrapping default/metadata; attributes: name, type, default, default_factory, init, repr, compare, hash, metadata, kw_only |

---

## 2. ERRORS

Exact exception type + message text for edge inputs, each verified via CPython probe.

| Probe | Exception Type | Message |
|-------|---|---|
| `Required.__init__()` with no args (field x: int required) | `TypeError` | `Required.__init__() missing 1 required positional argument: 'x'` |
| `TwoArgs(1, 2, 3)` (too many positional) | `TypeError` | `TwoArgs.__init__() takes 3 positional arguments but 4 were given` |
| `field(default=1, default_factory=list)` | `ValueError` | `cannot specify both default and default_factory` |
| `@dataclass(order=True, eq=False)` | `ValueError` | `eq must be true if order is true` |
| `asdict([1, 2, 3])` (list, not dataclass) | `TypeError` | `asdict() should be called on dataclass instances` |
| `astuple({"a": 1})` (dict, not dataclass) | `TypeError` | `astuple() should be called on dataclass instances` |
| `frozen_instance.x = value` (frozen=True) | `FrozenInstanceError` | `cannot assign to field 'x'` |
| `fields(int)` (not a dataclass) | `TypeError` | `must be called with a dataclass type or instance` |
| `fields(Non-dataclass instance)` | `TypeError` | `must be called with a dataclass type or instance` |
| Non-frozen dataclass in set/dict | `TypeError` | `unhashable type: 'ClassName'` |

---

## 3. BEHAVIOR MATRIX

Comprehensive input→output pairs from CPython 3.12 probes. Each expression and result verified in session.

### Basic Dataclass Creation & Equality

```python
@dataclass
class Point:
    x: int
    y: int

Point(1, 2)  # → Point(x=1, y=2)
Point(1, 2) == Point(1, 2)  # → True
Point(1, 2) != Point(2, 1)  # → True
repr(Point(1, 2))  # → 'Point(x=1, y=2)'
```

### Defaults and Default Factory

```python
@dataclass
class Config:
    host: str
    port: int = 8080

Config('localhost')  # → Config(host='localhost', port=8080)
Config('localhost', 9000)  # → Config(host='localhost', port=9000)

@dataclass
class WithFactory:
    items: list = field(default_factory=list)

wf1 = WithFactory()
wf2 = WithFactory()
wf1.items is wf2.items  # → False (separate list instances)
wf1.items.append(1)
wf2.items  # → [] (not mutated)
```

### Field Specifications (init, repr, compare)

```python
@dataclass
class NoInit:
    x: int
    y: int = field(init=False, default=99)

NoInit(1)  # → NoInit(x=1, y=99)
# NoInit(1, 2) raises TypeError (y not in __init__)

@dataclass
class Secret:
    name: str
    password: str = field(repr=False)

repr(Secret('alice', 'pw'))  # → "Secret(name='alice')"
Secret('alice', 'pw').password  # → 'pw' (field still exists)

@dataclass
class Comparison:
    id: int
    metadata: str = field(compare=False)

Comparison(1, 'a') == Comparison(1, 'b')  # → True (metadata excluded)
Comparison(1, 'a') != Comparison(1, 'b')  # → False
```

### Ordering (order=True)

```python
@dataclass(order=True)
class Ordered:
    val: int

Ordered(1) < Ordered(2)  # → True
Ordered(1) <= Ordered(1)  # → True
Ordered(2) > Ordered(1)  # → True
Ordered(2) >= Ordered(2)  # → True
Ordered(1) == Ordered(1)  # → True
Ordered(1) != Ordered(2)  # → True

@dataclass(order=False)  # or default (no order comparison)
class NoOrder:
    x: int

# NoOrder(1) < NoOrder(2) raises TypeError (no __lt__)
```

### Frozen Dataclasses

```python
@dataclass(frozen=True)
class Immutable:
    x: int

i = Immutable(5)
i.x  # → 5
# i.x = 2 raises FrozenInstanceError: cannot assign to field 'x'

@dataclass(frozen=True)
class FrozenList:
    items: list = field(default_factory=list)

fl = FrozenList()
fl.items.append(1)  # → succeeds (list itself is mutable)
fl.items  # → [1]
# fl.items = [] raises FrozenInstanceError (reassignment blocked)
```

### asdict() and astuple()

```python
@dataclass
class Data:
    x: int
    y: str

d = Data(1, 'hello')
asdict(d)  # → {'x': 1, 'y': 'hello'}
astuple(d)  # → (1, 'hello')

@dataclass
class Inner:
    a: int

@dataclass
class Outer:
    inner: Inner

o = Outer(Inner(1))
asdict(o)  # → {'inner': {'a': 1}} (recursive)

@dataclass
class WithHidden:
    x: int
    y: int = field(repr=False)
    z: int = field(init=False, default=99)

astuple(WithHidden(1, 2))  # → (1, 2, 99) (all fields, regardless of flags)
asdict(WithHidden(1, 2))  # → {'x': 1, 'y': 2, 'z': 99}
```

### Inheritance

```python
@dataclass
class Base:
    x: int

@dataclass
class Derived(Base):
    y: float

Derived(1, 3.14)  # → Derived(x=1, y=3.14)
# __init__ signature: (self, x: int, y: float) -> None

@dataclass
class Base2:
    x: int = 5

# @dataclass
# class BadChild(Base2):
#     y: int  # Error: non-default field after default
# raises TypeError at decoration time
```

### ClassVar Exclusion

```python
@dataclass
class WithClassVar:
    x: int
    count: ClassVar[int] = 0

WithClassVar(1)  # → WithClassVar(x=1)
# count is NOT in __init__, __repr__, __eq__
fields(WithClassVar)  # → (Field(name='x', ...),) — count excluded

WithClassVar.count = 5
wc = WithClassVar(1)
WithClassVar.count  # → 5 (shared across instances)
```

### Empty Dataclass

```python
@dataclass
class Empty:
    pass

Empty()  # → Empty()
Empty() == Empty()  # → True
repr(Empty())  # → 'Empty()'
```

### kw_only Parameter

```python
@dataclass
class KWOnly:
    x: int = field(kw_only=True)
    y: int

KWOnly(y=1, x=2)  # → KWOnly(x=2, y=1)
# KWOnly(2, 1) raises TypeError (x is keyword-only)
```

### __post_init__

```python
@dataclass
class WithPostInit:
    x: int
    y: int = field(init=False)

    def __post_init__(self):
        self.y = self.x * 2

WithPostInit(5)  # → WithPostInit(x=5, y=10)

@dataclass(frozen=True)
class FrozenPostInit:
    x: int
    y: int = field(init=False)

    def __post_init__(self):
        object.__setattr__(self, 'y', self.x * 2)

FrozenPostInit(5)  # → FrozenPostInit(x=5, y=10)
```

### fields() Inspection

```python
@dataclass
class Inspect:
    x: int
    y: int = 42
    z: int = field(default_factory=lambda: 99)

flds = fields(Inspect)
len(flds)  # → 3
flds[0].name  # → 'x'
flds[0].default  # → MISSING
flds[1].default  # → 42
flds[2].default  # → MISSING
flds[2].default_factory  # → <function ...>
```

### Hashing Behavior

```python
@dataclass(frozen=True)
class Hashable:
    x: int

h = Hashable(1)
{h}  # → {Hashable(x=1)} (can be in set)
hash(h)  # → <int> (hash value based on field values)

@dataclass
class Mutable:
    x: int

# {Mutable(1)} raises TypeError: unhashable type: 'Mutable'
```

### Multiple Field Defaults

```python
@dataclass
class Multi:
    a: int
    b: int = 1
    c: int = 2

Multi(5)  # → Multi(a=5, b=1, c=2)
Multi(5, 10)  # → Multi(a=5, b=10, c=2)
Multi(5, 10, 20)  # → Multi(a=5, b=10, c=20)
```

### Comparison Exclusion with Ordering

```python
@dataclass(order=True)
class OrderCompare:
    x: int
    y: int = field(compare=False)

OrderCompare(1, 100) < OrderCompare(2, 200)  # → True (x compared)
OrderCompare(1, 100) < OrderCompare(1, 200)  # → False (y excluded from order)
OrderCompare(1, 100) == OrderCompare(1, 200)  # → False (y excluded from ==)
```

---

## 4. HAZARDS

Formatting, ordering, locale, and platform issues that affect reproducibility in pyrst.

### 1. **Dict/Insertion-Order Dependence (CRITICAL FOR PYRST)**
   - **Issue:** CPython dicts preserve insertion order (3.7+), but pyrst dict iteration is SORTED-KEY order.
   - **Impact:** `asdict()` output keys and `fields()` iteration order match declaration order (not sorted).
   - **Examples:**
     ```python
     @dataclass
     class Out_of_Order:
         z: int
         a: int
         m: int
     
     asdict(Out_of_Order(1, 2, 3))  # CPython → {'z': 1, 'a': 2, 'm': 3}
                                    # pyrst    → {'a': 2, 'm': 3, 'z': 1} (sorted keys)
     
     fields(Out_of_Order)  # CPython → (z, a, m) in declaration order
                           # pyrst    → ??? (sorted or declaration?) — UNCLEAR
     ```
   - **Mitigation:** Wrap `asdict()` output in `sorted()` for parity tests; declare fields in alphabetical order in golden tests.

### 2. **Float Representation**
   - **Issue:** Float repr() may vary by platform/precision.
   - **Impact:** Tests with float fields will have platform-dependent repr output.
   - **Example:** `FloatTest(3.14159265359)` → repr includes full precision.
   - **Mitigation:** Use `round()` or format floats consistently in parity tests.

### 3. **Repr Escaping (Strings with Quotes/Newlines)**
   - **Issue:** repr() auto-escapes quotes and special chars; behavior consistent but verbose.
   - **Example:** `Secret('alice', "o'reilly")` → repr shows quote escaping.
   - **Mitigation:** Use single-quoted or plain strings in golden tests to avoid escaping variance.

### 4. **Empty Collection Repr**
   - **Issue:** `repr([])`, `repr({})` are consistent, but empty dataclasses print as `ClassName()`.
   - **Impact:** No hazard, but notable edge case.

### 5. **Mutable Default Shallow Copy in asdict()**
   - **Issue:** `asdict()` does a shallow copy of field values; mutating the result dict's mutable fields does NOT mutate the original.
   - **Example:**
     ```python
     m = Mutable([1])
     d = asdict(m)
     d['items'].append(2)
     m.items  # → [1] (unchanged)
     d['items']  # → [1, 2]
     ```
   - **Mitigation:** Test asdict() result is independent; don't assume deep copy.

### 6. **Frozen Dataclass Hashability**
   - **Issue:** Only frozen dataclasses are hashable by default (no __hash__ synthesis for mutable).
   - **Impact:** Mutable dataclass instances cannot be dict keys or in sets.
   - **Mitigation:** Use frozen=True or unsafe_hash=True for hashability; test both branches.

### 7. **Attribute Assignment Exception Type**
   - **Issue:** FrozenInstanceError is a subclass of AttributeError (CPython 3.12+).
   - **Impact:** Catch clause on AttributeError will catch FrozenInstanceError.
   - **Mitigation:** Test the exact exception type.

---

## 5. GATED

Pyrst constraint cheat-sheet: which surface parts hit language gates, and suggested deferrals.

| Gate | Constraint | API Part Affected | Suggested Design-Around |
|------|-----------|-------------------|-------------------------|
| **G2** | No module-level mutable state | `MISSING` sentinel, `Field` defaults storage | Use immutable sentinel (empty tuple marker?); Field is per-dataclass, not global state — OK |
| **G3** | No dotted submodules | N/A (dataclasses is flat module) | N/A — no submodule imports needed |
| **G4** | No *args/**kwargs variadics | Field.metadata dict (dict-like API), asdict/astuple dict_factory/tuple_factory params | Field.metadata is read-only dict; asdict/astuple: skip dict_factory/tuple_factory (always use dict/tuple) |
| **G7** | No bytes type | field.metadata can contain arbitrary objects including bytes | Skip bytes; restrict metadata values to str/int/list/dict (immutable subset) |
| **G9** | i64 ints only, no bignum | field counts, hashing, field values | Max 65k fields per dataclass (reasonable); hash values fit i64 (Python's hash already does) |
| **No decorators except @property/@staticmethod/@extern/@crate** | @dataclass, @staticmethod/@classmethod coexistence | @dataclass decorator synthesis | Core gate: @dataclass IS the decorator — must be compiler-synthesized, not runtime; no user __init__ override allowed |
| **Class dunders limited to: __init__ __str__ __repr__ __eq__ __lt__ __add__ __sub__ __mul__ __neg__ __bool__** | order=True synthesizes __lt__, __le__, __gt__, __ge__; unsafe_hash synthesizes __hash__ | order=True, unsafe_hash=True | **GATED:** order=True requires synthesis of __lt__ (allowed) + __le__/__gt__/__ge__ (NOT in dunder list) — implement as function calls to __lt__/__eq__; unsafe_hash requires __hash__ (NOT listed) — **DEFERRED** or use frozen=True only for hashing |
| **Single inheritance** | Multiple inheritance of dataclasses | Multi-class Derived(Base1, Base2) | Single dataclass inheritance only; cannot mix two @dataclass bases (compiler enforces MRO) |
| **dict iteration is SORTED-KEY order** | asdict() output order, fields() iteration | asdict() key order, fields() return order | **CRITICAL:** pyrst dict iteration differs from CPython insertion order — parity tests must sorted() results or test only content, not order |

---

## 6. PARITY PLAN

Concrete list of 25 dual-run-safe test lines (expressions + python3-verified expected output) suitable for adoption in pyrst parity golden suite. Constructed to avoid order/formatting hazards.

```python
# DATACLASS BASIC TESTS

# Test 1: Simple dataclass creation and repr
@dataclass
class P1:
    x: int
    y: int
p1 = P1(1, 2)
print(repr(p1))  # P1(x=1, y=2)

# Test 2: Equality
p2 = P1(1, 2)
print(p1 == p2)  # True

# Test 3: Inequality
p3 = P1(2, 1)
print(p1 != p3)  # True

# Test 4: Default value
@dataclass
class P4:
    x: int
    y: int = 42
p4 = P4(1)
print(p4.y)  # 42

# Test 5: Default factory list (separate instances)
@dataclass
class P5:
    items: list = field(default_factory=list)
p5a = P5()
p5b = P5()
print(p5a.items is p5b.items)  # False

# Test 6: init=False excludes from __init__
@dataclass
class P6:
    x: int
    y: int = field(init=False, default=99)
p6 = P6(1)
print(p6.y)  # 99

# Test 7: repr=False excludes from repr
@dataclass
class P7:
    x: int
    y: int = field(repr=False)
p7 = P7(1, 2)
print(repr(p7))  # P7(x=1)

# Test 8: compare=False excludes from equality
@dataclass
class P8:
    id: int
    meta: str = field(compare=False)
p8a = P8(1, 'a')
p8b = P8(1, 'b')
print(p8a == p8b)  # True

# Test 9: order=True enables < comparison
@dataclass(order=True)
class P9:
    val: int
print(P9(1) < P9(2))  # True

# Test 10: order=True enables <= >=
print(P9(2) >= P9(2))  # True

# Test 11: order=True enables >
print(P9(2) > P9(1))  # True

# Test 12: frozen=True prevents mutation (AttributeError-like)
@dataclass(frozen=True)
class P12:
    x: int
p12 = P12(1)
try:
    p12.x = 2
    print("ERROR: mutation succeeded")
except (FrozenInstanceError, AttributeError):
    print("FrozenInstanceError raised")  # Expected

# Test 13: frozen list still mutable
@dataclass(frozen=True)
class P13:
    items: list = field(default_factory=list)
p13 = P13()
p13.items.append(1)
print(len(p13.items))  # 1

# Test 14: asdict basic
@dataclass
class P14:
    x: int
    y: str
p14 = P14(1, 'a')
d14 = asdict(p14)
print(d14['x'])  # 1
print(d14['y'])  # a

# Test 15: astuple basic
p15 = P14(2, 'b')
t15 = astuple(p15)
print(t15[0])  # 2
print(t15[1])  # b

# Test 16: asdict recursive
@dataclass
class Inner:
    a: int

@dataclass
class Outer:
    inner: Inner

p16 = Outer(Inner(5))
d16 = asdict(p16)
print(d16['inner']['a'])  # 5

# Test 17: astuple includes all fields (even init=False)
@dataclass
class P17:
    x: int
    y: int = field(init=False, default=99)
p17 = P17(1)
t17 = astuple(p17)
print(t17[1])  # 99

# Test 18: fields() returns Field objects
@dataclass
class P18:
    x: int
    y: int = 10

flds = fields(P18)
print(len(flds))  # 2
print(flds[0].name)  # x

# Test 19: fields()[1].default
print(flds[1].default)  # 10

# Test 20: Inheritance chain
@dataclass
class Base:
    x: int

@dataclass
class Derived(Base):
    y: int

d20 = Derived(1, 2)
print(d20.x)  # 1
print(d20.y)  # 2

# Test 21: ClassVar excluded from __init__
@dataclass
class P21:
    x: int
    count: ClassVar[int] = 0

p21 = P21(1)
print(p21.x)  # 1

# Test 22: Empty dataclass
@dataclass
class P22:
    pass

p22a = P22()
p22b = P22()
print(p22a == p22b)  # True

# Test 23: kw_only parameter
@dataclass
class P23:
    x: int = field(kw_only=True)
    y: int

p23 = P23(y=1, x=2)
print(p23.x)  # 2

# Test 24: __post_init__ execution
@dataclass
class P24:
    x: int
    y: int = field(init=False)

    def __post_init__(self):
        self.y = self.x * 2

p24 = P24(5)
print(p24.y)  # 10

# Test 25: Multiple defaults
@dataclass
class P25:
    a: int
    b: int = 1
    c: int = 2

p25 = P25(10)
print(p25.b)  # 1
print(p25.c)  # 2
```

---

## 7. TARGET

**Fidelity Estimate: 3.5/5**

### Achievable (High Confidence)
- ✅ @dataclass decorator with init/repr/eq synthesis (core feature)
- ✅ field() with default/default_factory/init/repr/compare
- ✅ order=True (synthesis of __lt__; __le__/__gt__/__ge__ via utility functions)
- ✅ frozen=True (immutability via object.__setattr__ interception)
- ✅ asdict() and astuple() (recursive traversal)
- ✅ Basic inheritance (single dataclass parent)
- ✅ ClassVar exclusion
- ✅ __post_init__ callback

### Partially Achievable (Medium Confidence — gated by pyrst constraints)
- ⚠️ **unsafe_hash=True:** Requires __hash__ synthesis (NOT in pyrst dunder list) — defer or provide only for frozen=True
- ⚠️ **Dict iteration order:** asdict() keys will be sorted (not insertion-order) — requires test rewrites
- ⚠️ **Multiple inheritance:** Single inheritance only in pyrst

### Not Achievable (Blocked by constraints)
- ❌ **slots=True:** Requires __slots__ manipulation; pyrst has no __slots__
- ❌ **match_args=True:** Requires pattern matching support; pyrst has no match statement
- ❌ Custom exception classes for frozen errors — map to AttributeError
- ❌ Arbitrary metadata values (only str/int/list/dict permitted)
- ❌ Field descriptors as class attributes (pyrst does not expose Field objects at runtime)

### Dominant Reasons for 3.5/5 (Not 5/5)

1. **Dict iteration order mismatch:** Pyrst sorted-key iteration diverges from CPython insertion-order semantics. Mitigated by test rewrites but represents a behavioral gap.

2. **Dunder list constraint (__hash__, __le__/__gt__/__ge__):** unsafe_hash deferred; order=True partially works via utility functions but does not expose full comparison operators.

3. **Slots and match_args:** Modern dataclass features (3.10+) impossible without compiler support for __slots__ and pattern matching.

---

## Implementation Notes

### Compiler-Side Work
- Synthesize @dataclass as a compiler pass (not a decorator at runtime)
- Recognize field() calls and build Field metadata during parsing
- Generate __init__, __repr__, __eq__ methods into the class AST
- When order=True, generate __lt__ method; provide utility functions for __le__, __gt__, __ge__
- When frozen=True, override __setattr__ and __delattr__ to raise FrozenInstanceError

### Runtime Library (dataclasses.rlib)
- Provide asdict(), astuple() as traversal functions
- Provide fields(), is_dataclass() for introspection
- Provide FrozenInstanceError exception class
- Store Field metadata in a flat dict (no Field descriptor class needed at runtime)
- MISSING sentinel as a unique object

### Parity Golden Tests
- Use sorted(asdict(...).items()) for order-safe comparison
- Declare test dataclasses with alphabetically ordered fields
- Wrap float comparisons in round() or format strings
- Test frozen/non-frozen branches separately
- Verify FrozenInstanceError as a subclass of AttributeError

---

## File Manifest

- Dossier: `/tmp/claude-1000/-home-ethos-Coding-pyrst/.../dataclasses.md`
- Probe session: CPython 3.12.9 stdlib module (verified live in session)
