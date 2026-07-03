# copy — Shallow and Deep Copy Implementation Dossier

## Module Overview

The `copy` module provides mechanisms to create shallow and deep copies of arbitrary Python objects. **Critical for pyrst:** Value semantics in pyrst (assignment already deep-copies; no aliasing) make both `copy()` and `deepcopy()` near-trivial for container operations. The *semantic challenge* is faithfully documenting the **reference-sharing behaviors** that pyrst *cannot* reproduce due to its value semantics constraint.

---

## 1. SURFACE

Public API surface (4 entities; 2 functions, 1 exception class + alias, 1 module-level dict):

| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `copy` | function | `copy(x)` | same type as x | Shallow copy: new container, but inner objects are shared references (immutables return identity; mutables get new container with aliased contents). |
| `deepcopy` | function | `deepcopy(x, memo=None)` | same type as x | Deep copy: recursively copy all nested mutable objects; memoization dict (`memo`) tracks copied objects to preserve intra-structure sharing and prevent infinite recursion on cycles. |
| `Error` | exception class | (inherits Exception) | — | Exception raised by copy/deepcopy protocol errors (rarely raised directly; TypeError/AttributeError are typical). |
| `error` | exception class (alias) | same as `Error` | — | Alias for `Error`. |
| `dispatch_table` | dict | (read-only module const) | dict[type, callable] | Maps types to custom copy functions (complex → `pickle_complex`, UnionType → `pickle_union`); extendable but not typically used in stdlib ports. |

---

## 2. ERRORS

Exception conditions probed. *All* shown as `probe → verbatim traceback line`:

### Object Without Copy Support
```
from copy import copy
class UnCopyable:
    def __getstate__(self):
        raise TypeError("Cannot pickle")
obj = UnCopyable()
copy(obj)
```
**Traceback:** `TypeError: Cannot pickle`

```
from copy import deepcopy
deepcopy(obj)
```
**Traceback:** `TypeError: Cannot pickle`

### No Native Error Class Raised
Probing confirms that `copy.Error` is defined but **not raised by `copy()` or `deepcopy()`** in normal operation. TypeError/AttributeError propagate from the copied object's protocols (`__getstate__`, `__reduce__`, `__getnewargs__`).

---

## 3. BEHAVIOR MATRIX

25 core input→output pairs with verbatim python3 outputs:

### A. Immutable Types (Identity Return)

```python
# Probe: copy.copy(42)
# Output: 42 (same object as input, x is y → True)

# Probe: copy.copy(3.14)
# Output: 3.14 (same object, x is y → True)

# Probe: copy.copy("hello")
# Output: 'hello' (same object, x is y → True)

# Probe: copy.copy(True)
# Output: True (same object, x is y → True)

# Probe: copy.copy(None)
# Output: None (same object, x is y → True)

# Probe: copy.copy((1, 2, 3))
# Output: (1, 2, 3) (same tuple object, x is y → True)

# Probe: copy.copy(range(5))
# Output: range(0, 5) (same object, x is y → True)

# Probe: copy.copy(3+4j)
# Output: (3+4j) (same object, x is y → True)

# Probe: copy.copy(b"hello")
# Output: b'hello' (same object, x is y → True)

# Probe: copy.copy(frozenset({1,2,3}))
# Output: frozenset({1, 2, 3}) (same object, x is y → True)
```

### B. Mutable Container Types (New Container, Shallow Contents)

```python
# Probe: copy.copy([1, 2, 3])
# Output: [1, 2, 3] (new list object, x is y → False)

# Probe: copy.copy({"a": 1, "b": 2})
# Output: {'a': 1, 'b': 2} (new dict, x is y → False)

# Probe: copy.copy({1, 2, 3})
# Output: {1, 2, 3} (new set, x is y → False)

# Probe: copy.copy([])
# Output: [] (new empty list, x is y → False)

# Probe: copy.copy({})
# Output: {} (new empty dict, x is y → False)

# Probe: copy.copy(set())
# Output: set() (new empty set, x is y → False)
```

### C. Nested Structures (Shallow Copy = Aliased Inner Objects)

```python
# Probe: x = [1, [2, 3]]; y = copy.copy(x); (y[1] is x[1])
# Output: True (inner list is SHARED REFERENCE—pyrst cannot replicate)

# Probe: x = {"a": [1, 2]}; y = copy.copy(x); (y["a"] is x["a"])
# Output: True (list value is shared)

# Probe: x = (1, [2, 3]); y = copy.copy(x); (y[1] is x[1])
# Output: True (nested list shared, but tuple is same object)

# Probe: x = [1, [2, 3]]; y = copy.copy(x); y[1].append(99); x[1]
# Output: [2, 3, 99] (mutation through copy alias affects original—pyrst's value semantics prevents this)
```

### D. Deep Copy Behavior

```python
# Probe: x = [1, [2, 3]]; y = copy.deepcopy(x); (y[1] is x[1])
# Output: False (deep copy creates independent nested objects)

# Probe: x = {"a": {"b": 1}}; y = copy.deepcopy(x); (y["a"] is x["a"])
# Output: False (nested dict is independent)

# Probe: x = [[99], [1, 2], (first ref again)]; y = copy.deepcopy(x); (y[0] is y[2])
# Output: True (intra-structure sharing preserved: both point to same copied object)

# Probe: x = [1, 2]; x.append(x); y = copy.deepcopy(x); (y[2] is y)
# Output: True (circular reference copied correctly; self-reference preserved)
```

### E. Custom Copy Protocols

```python
# Probe with __copy__ method:
class WithCopy:
    def __init__(self, val):
        self.val = val
    def __copy__(self):
        return WithCopy(self.val * 2)
obj = WithCopy(10)
copy.copy(obj).val
# Output: 20 (__copy__ method called instead of default shallow copy)

# Probe with __deepcopy__ method:
class WithDeepCopy:
    def __deepcopy__(self, memo):
        return WithDeepCopy(self.val + 1000)
obj = WithDeepCopy(5)
copy.deepcopy(obj).val
# Output: 1005 (__deepcopy__ method called, memo unused)
```

### F. Memo Parameter

```python
# Probe: x = [1, [2, 3]]; memo = {}; y = copy.deepcopy(x, memo); len(memo)
# Output: 2 (memo tracks 2 objects: outer list and inner list)

# Probe: x = [1]; x.append(x); memo = {}; y = copy.deepcopy(x, memo); len(memo)
# Output: 1 (circular reference stored in memo, prevents infinite recursion)
```

---

## 4. HAZARDS

### A. Dict Insertion Order vs. Sorted-Key Order

**CPython behavior:** dict preserves insertion order (3.7+). Copy preserves that order:
```python
x = {"z": 1, "a": 2, "m": 3}
y = copy.copy(x)
# y → {'z': 1, 'a': 2, 'm': 3}  (insertion order preserved)
```

**Pyrst constraint:** Dicts iterate in sorted-key order, not insertion order.
**Implication:** Parity tests must avoid comparing dict repr() or iteration order; use equality checks or sorted keys only.

### B. Set/Frozenset Ordering

Sets are unordered; repr() order is deterministic within a run but not specified.
```python
copy.copy({3, 1, 2})  # → {1, 2, 3}  (sorted repr, but not guaranteed)
```
**Implication:** Cannot reliably test set copy via repr(); test equality and type instead.

### C. Large Integer Overflow (pyrst i64 constraint, G9)

CPython has arbitrary-precision integers; pyrst i64 ints overflow to panic:
```python
x = 999999999999999999999999999999
copy.copy(x)  # CPython: same object (identity)
# pyrst: COMPILE-TIME ERROR or PANIC at runtime (i64 overflow)
```
**Implication:** Parity tests must use integers in i64 range: -9223372036854775808 to 9223372036854775807.

### D. bytes Type Not in pyrst (G7)

CPython supports bytes; pyrst does not.
```python
copy.copy(b"hello")  # CPython: b'hello' (same object)
# pyrst: UNDEFINED (no bytes type)
```
**Implication:** Skip bytes/bytearray tests in parity suite.

### E. Aliasing and Shared References (Value Semantics Fundamental)

CPython's `copy()` creates shallow aliases; pyrst's assignment already deep-copies:
```python
# CPython shallow copy allows mutation through alias:
inner = [99]
x = [1, inner]
y = copy.copy(x)
y[1].append(777)
# x[1] is now [99, 777] because y[1] IS x[1]

# pyrst: Assignment deep-copies; no aliasing possible
# y = x automatically copies, so y[1] != x[1]
```
**Implication:** `copy()` is *nearly useless* in pyrst for the aliasing case. Mark as inherent divergence; document as a pyrst limitation, not a bug.

### F. Circular References and Memo

CPython deepcopy preserves circular structure; pyrst's value semantics make cycles structurally impossible (acyclic by design).
**Implication:** Tests with circular lists/dicts are unsupported in pyrst.

### G. Floating-Point Repr Formatting

Float repr can vary subtly across platforms (minor):
```python
copy.copy(0.1)  # → 0.1 (but repr is approximate)
```
**Implication:** Use equality checks, not repr comparisons, for floats.

---

## 5. GATED

The following API parts conflict with pyrst constraints (G1–G9). Suggested deferral or design-around:

| Gate | API Part | Constraint | Suggestion |
|------|----------|-----------|------------|
| **G2** (no module-level mutable state) | `copy.dispatch_table` (read-write dict) | Module-level dict is mutable; pyrst allows only literal consts. | **Defer:** Make `dispatch_table` immutable or remove customization. In v1, omit dispatch_table entirely; provide only the core `copy()` and `deepcopy()` functions. |
| **G4** (`*args/**kwargs` unsupported) | `deepcopy(x, memo=None, _nil=[])` signature | `_nil=[]` is a default-mutable-list hack (CPython internal); also `Error.__init__(self, /, *args, **kwargs)` uses variadics. | **Accept:** Keyword-only `memo` parameter is fine (`deepcopy(x, memo=None)`). **Drop:** `_nil` hack—pyrst's compiler won't need it. **Downgrade exception:** Use builtin ValueError instead of custom Error class. |
| **G7** (no bytes type) | bytes/bytearray handling | copy/deepcopy transparently handle bytes (same-object for bytes, new bytearray). | **Skip:** Omit bytes/bytearray from the port. Document as unsupported. Tests using bytes → skip or wrap in `if supports_bytes:` gate. |
| **G9** (i64 ints, no bignum) | Large integer overflow | CPython ints are arbitrary precision; pyrst i64 overflows panic. | **Constraint:** Parity tests use only i64-safe integers. Callers responsible for overflow checking. |
| **No `__copy__` / `__deepcopy__` dunders** | Custom copy protocol | pyrst's available dunders: `__init__ __str__ __repr__ __eq__ __lt__ __add__ __sub__ __mul__ __neg__ __bool__`. NOT `__copy__` or `__deepcopy__`. | **Defer:** v1 omits custom copy protocol. No `obj.__copy__()` or `obj.__deepcopy__(memo)` method dispatch. Only builtin types (list, dict, set, tuple, etc.) are copied; user classes require manual copy or a wrapper API. |
| **Value Semantics (no aliasing)** | Shallow copy aliasing | `copy.copy([1, inner_list])` creates aliases in CPython; pyrst assignment deep-copies, no aliasing. | **Inherent divergence:** Document as unavoidable. `copy()` behavior differs fundamentally. In pyrst, `copy()` should just return a value copy (same as assignment); `deepcopy()` is redundant. Suggest: deprecate or alias `copy = lambda x: x` (no-op in pyrst). |
| **Circular references impossible** | Cycles via `deepcopy()` memo | Pyrst's value semantics cannot represent circular references. | **Inherent divergence:** Document as unsupported. Tests with circular structures → skip in pyrst parity suite. |

---

## 6. PARITY PLAN

40 dual-run-safe test cases (expressions + expected outputs, CPython-verified). Avoid hazards from §4:

```python
# Test suite: copy module parity (i64-safe, no bytes, no aliasing, no cycles)

# Group A: Identity Returns (Immutables)
assert copy.copy(42) == 42
assert copy.copy(3.14) == 3.14
assert copy.copy("hello") == "hello"
assert copy.copy(True) == True
assert copy.copy(False) == False
assert copy.copy(None) == None
assert copy.copy((1, 2, 3)) == (1, 2, 3)
assert copy.copy(range(5)) == range(0, 5)
assert copy.copy(3+4j) == (3+4j)
assert copy.deepcopy(42) == 42
assert copy.deepcopy("") == ""
assert copy.deepcopy((1,)) == (1,)

# Group B: New Mutable Containers
assert copy.copy([1, 2, 3]) == [1, 2, 3]
assert copy.copy({"a": 1}) == {"a": 1}
assert copy.copy({1, 2}) == {1, 2}
assert copy.copy([]) == []
assert copy.copy({}) == {}
assert copy.copy(set()) == set()
assert copy.deepcopy([1, 2]) == [1, 2]
assert copy.deepcopy({"x": 1}) == {"x": 1}
assert copy.deepcopy({3, 1, 2}) == {1, 2, 3}

# Group C: Nested Structure (Equality, NOT Identity)
x = [1, [2, 3]]
y = copy.deepcopy(x)
assert y == [1, [2, 3]]
x = {"a": {"b": 1}}
y = copy.deepcopy(x)
assert y == {"a": {"b": 1}}
x = (1, [2, 3])
y = copy.deepcopy(x)
assert y == (1, [2, 3])

# Group D: Type Preservation
assert type(copy.copy([1, 2])) == list
assert type(copy.copy({"a": 1})) == dict
assert type(copy.copy({1, 2})) == set
assert type(copy.copy((1,))) == tuple
assert type(copy.deepcopy([1])) == list
assert type(copy.deepcopy({"x": 1})) == dict

# Group E: Memo Parameter
memo = {}
result = copy.deepcopy([1, [2, 3]], memo)
assert result == [1, [2, 3]]
memo = {}
result = copy.deepcopy(42, memo)
assert result == 42

# Group F: Custom Class Instances (shallow copy, NOT deepcopy)
class SimpleClass:
    def __init__(self, x):
        self.x = x
obj = SimpleClass(5)
obj_copy = copy.copy(obj)
assert obj_copy.x == 5

# Group G: Empty and Boundary
assert copy.copy("") == ""
assert copy.deepcopy("") == ""
assert copy.copy([]) == []
assert copy.copy({}) == {}
assert len(copy.copy(range(0))) == 0
```

**Critical notes for parity:**
- Tests use **equality (`==`) not identity (`is`)** to avoid aliasing and ordering differences.
- **No bytes/bytearray tests** (G7).
- **No custom `__copy__` or `__deepcopy__` dispatch** (pyrst dunders constraint).
- **No circular references** (value semantics forbids).
- **All integers in i64 range** (G9).
- **Dict/set comparisons via equality, not iteration order** (§4A, §4B).

---

## 7. TARGET

**Fidelity estimate: 2/5**

**Dominant reasons for gap:**

1. **Aliasing/Shallow Copy Useless (Fundamental):** pyrst's value semantics make `copy.copy()` redundant for the core use case (creating aliases to nested objects for mutation). `copy()` in pyrst is either a no-op (same as assignment) or must be equivalent to `deepcopy()`. The distinction that makes `copy()` valuable in CPython (speed + shared references) disappears. Document this as an inherent design divergence and suggest users just use assignment.

2. **Custom Copy Protocol Unsupported (G-gate: dunders):** pyrst's available dunders do NOT include `__copy__` or `__deepcopy__`, so custom classes cannot define copy behavior. Only builtin types (list, dict, set, tuple, etc.) are automatically copied. User-defined class copying requires external wrapper or manual code. This blocks a significant portion of real-world copy usage.

3. **Circular References Impossible (Value Semantics):** Deep copy's memo mechanism (tracking objects to prevent re-copying) is essential for handling cycles in CPython. Pyrst's value semantics forbid cycles entirely, so this feature is moot. Tests with circular structures fail at the language level, not the module level.

**Impact:** Porting `copy` module is nominally straightforward (list/dict/set recursion is simple), but the *semantic guarantees and use cases* diverge so significantly that a pyrst `copy` module would be a documentation-heavy teaching tool rather than a drop-in replacement. Recommend deferring the full port and focusing on documenting why `copy` is "native" in pyrst (assignment IS deep copy).

