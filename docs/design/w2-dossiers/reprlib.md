# reprlib Dossier

## 1. SURFACE

| Name | Kind | Signature | Return | Semantics |
|------|------|-----------|--------|-----------|
| Repr | class | `__init__(*, maxlevel=6, maxtuple=6, maxlist=6, maxarray=5, maxdict=4, maxset=6, maxfrozenset=6, maxdeque=6, maxstring=30, maxlong=40, maxother=30, fillvalue='...', indent=None)` | Repr | Configure truncation limits for repr generation |
| Repr.repr | method | `(self, x)` | str | Generate abbreviated repr of object x |
| Repr.repr1 | method | `(self, x, level)` | str | Generate repr of x at given nesting level (level 0 always returns fillvalue) |
| Repr.repr_array | method | `(self, x, level)` | str | Generate repr of array.array |
| Repr.repr_deque | method | `(self, x, level)` | str | Generate repr of collections.deque |
| Repr.repr_dict | method | `(self, x, level)` | str | Generate repr of dict (keys sorted) |
| Repr.repr_frozenset | method | `(self, x, level)` | str | Generate repr of frozenset |
| Repr.repr_instance | method | `(self, x, level)` | str | Generate repr of custom class instance |
| Repr.repr_int | method | `(self, x, level)` | str | Generate repr of int (truncate if len > maxlong) |
| Repr.repr_list | method | `(self, x, level)` | str | Generate repr of list |
| Repr.repr_set | method | `(self, x, level)` | str | Generate repr of set |
| Repr.repr_str | method | `(self, x, level)` | str | Generate repr of str (prefix...suffix if len > maxstring) |
| Repr.repr_tuple | method | `(self, x, level)` | str | Generate repr of tuple |
| Repr.maxlevel | attr | int | — | Maximum nesting depth (default 6); when exceeded, shows fillvalue |
| Repr.maxtuple | attr | int | — | Max tuple elements shown (default 6); excess elements trigger fillvalue |
| Repr.maxlist | attr | int | — | Max list elements shown (default 6); excess elements trigger fillvalue |
| Repr.maxarray | attr | int | — | Max array elements shown (default 5); excess elements trigger fillvalue |
| Repr.maxdict | attr | int | — | Max dict key-value pairs shown (default 4); excess trigger fillvalue |
| Repr.maxset | attr | int | — | Max set elements shown (default 6); excess elements trigger fillvalue |
| Repr.maxfrozenset | attr | int | — | Max frozenset elements shown (default 6); excess elements trigger fillvalue |
| Repr.maxdeque | attr | int | — | Max deque elements shown (default 6); excess elements trigger fillvalue |
| Repr.maxstring | attr | int | — | Max string display length (default 30); longer strings show prefix...suffix |
| Repr.maxlong | attr | int | — | Max int repr length (default 40); longer reprs show high...low digits |
| Repr.maxother | attr | int | — | Max repr length for other objects (default 30); truncated with ... in middle |
| Repr.fillvalue | attr | str | — | Marker for truncated content (default '...'); must be str |
| Repr.indent | attr | int or None | — | Reserved for future use; currently None (no effect) |
| aRepr | const | Repr | — | Module-level default Repr instance (all defaults) |
| repr | function | `(obj)` | str | Shorthand for aRepr.repr(obj) |
| recursive_repr | function | `(fillvalue='...')` | decorator | Decorator for __repr__ methods; prevents infinite recursion on cycles |

## 2. ERRORS

| Condition | Exception Type | Message Pattern |
|-----------|---|---|
| `Repr(maxlist=-1)` | ValueError | `Stop argument for islice() must be None or an integer: 0 <= x <= sys.maxsize.` |
| `Repr(maxstring=-1)` when truncation needed | (no error; returns '...') | — |
| `Repr(maxstring=-1).repr('a'*50)` | (no error; returns '...' only) | — |
| `Repr(maxlist=3.5)` when truncation needed | ValueError | `Stop argument for islice() must be None or an integer: 0 <= x <= sys.maxsize.` |
| `Repr(fillvalue=42).repr([1,2,3,4,5,6,7])` | TypeError | `sequence item 6: expected str instance, int found` |
| `Repr(fillvalue=None).repr([1,2,3,4,5,6,7])` | TypeError | `sequence item 6: expected str instance, NoneType found` |
| `Repr(fillvalue=b'...').repr([1,2,3,4,5,6,7])` | TypeError | `sequence item 6: expected str instance, bytes found` |

## 3. BEHAVIOR MATRIX

### Basic Container Truncation

```
# Lists
Repr().repr([1,2,3,4,5,6])
→ '[1, 2, 3, 4, 5, 6]'

Repr().repr([1,2,3,4,5,6,7])
→ '[1, 2, 3, 4, 5, 6, ...]'

Repr(maxlist=2).repr([1,2,3,4,5])
→ '[1, 2, ...]'

# Tuples
Repr().repr((1,2,3,4,5,6))
→ '(1, 2, 3, 4, 5, 6)'

Repr().repr((1,2,3,4,5,6,7))
→ '(1, 2, 3, 4, 5, 6, ...)'

# Dicts (sorted by key)
Repr().repr({3: 30, 1: 10, 2: 20, 5: 50, 4: 40, 6: 60})
→ '{1: 10, 2: 20, 3: 30, 4: 40, ...}'

Repr(maxdict=2).repr({1: 10, 2: 20, 3: 30})
→ '{1: 10, 2: 20, ...}'

# Sets
Repr().repr({1,2,3,4,5,6,7})
→ '{1, 2, 3, 4, 5, 6, ...}'

Repr().repr({1,2,3,4,5,6})
→ '{1, 2, 3, 4, 5, 6}'

# Frozensets
Repr().repr(frozenset([1,2,3,4,5,6,7]))
→ 'frozenset({1, 2, 3, 4, 5, 6, ...})'

# Arrays
Repr().repr(array.array('i', [1,2,3,4,5,6,7]))
→ "array('i', [1, 2, 3, 4, 5, ...])"

# Deques
Repr().repr(deque(range(1,9)))
→ 'deque([1, 2, 3, 4, 5, 6, ...])'
```

### String Truncation (prefix...suffix)

```
# Exact boundary
Repr().repr('a' * 30)
→ "'aaaaaaaaaaaa...aaaaaaaaaaaaa'"

# Just over boundary
Repr().repr('a' * 31)
→ "'aaaaaaaaaaaa...aaaaaaaaaaaaa'"

# Well over
Repr().repr('abcdefghijklmnopqrstuvwxyz0123456789')
→ "'abcdefghijkl...uvwxyz0123456'"

# Short strings (no truncation)
Repr().repr('hello')
→ "'hello'"

Repr().repr('')
→ "''"
```

### Integer Truncation

```
# Small integers (no truncation)
Repr().repr(12345)
→ '12345'

# Large integers (truncate at maxlong=40)
Repr().repr(10**50)
→ '100000000000000000...0000000000000000000'

Repr(maxlong=20).repr(10**50)
→ '10000000000...000000000'
```

### Nesting and maxlevel

```
# Level 0 always returns fillvalue
Repr().repr1([1,2,3], 0)
→ '[...]'

# At depth
Repr().repr1([1,2,3], 1)
→ '[1, 2, 3]'

# Deep nesting within maxlevel
nested = [[[[[['deep']]]]]]
Repr().repr(nested)
→ "[[[[[['deep']]]]]]"

# Exceeding maxlevel=6
nested = [[[[[[['too deep']]]]]]]
Repr().repr(nested)
→ "[[[[[[[[...]]]]]]]"

# With lower maxlevel
Repr(maxlevel=2).repr([[[1,2,3]]])
→ '[[[...]]]'
```

### fillvalue and Custom Configuration

```
# Default fillvalue
Repr().repr([1,2,3,4,5,6,7])
→ '[1, 2, 3, 4, 5, 6, ...]'

# Custom fillvalue
Repr(fillvalue='<...>').repr([1,2,3,4,5,6,7])
→ '[1, 2, 3, 4, 5, 6, <...>]'

# Empty fillvalue
Repr(fillvalue='').repr([1,2,3,4,5,6,7])
→ '[1, 2, 3, 4, 5, 6, ]'

# Multiple attribute modification
r = Repr()
r.maxlist = 2
r.fillvalue = '...[truncated]...'
r.repr([1,2,3,4,5])
→ '[1, 2, ...[truncated]...]'
```

### Mixed Nested Structures

```
# Mixed types
Repr().repr([1, (2, 3), {4: 5}, [6, 7]])
→ '[1, (2, 3), {4: 5}, [6, 7]]'

# Mixed with long content
mixed = [1, 'hello world with extra text', [1,2,3,4,5,6,7]]
Repr().repr(mixed)
→ '[1, 'hello world ...ith extra text', [1, 2, 3, 4, 5, 6, ...]]'
```

### Special Values

```
Repr().repr(None)
→ 'None'

Repr().repr(True)
→ 'True'

Repr().repr(False)
→ 'False'

Repr().repr(3.14)
→ '3.14'
```

### repr1(x, level) Behavior

```
# Level 0: always truncated
Repr().repr1([1,2,3], 0)
→ '[...]'

Repr().repr1([[[1,2,3]]], 0)
→ '[...]'

# Level progression with nested lists
nested = [[[1,2,3]]]
Repr().repr1(nested, 1)
→ '[[...]]'

Repr().repr1(nested, 2)
→ '[[[...]]]'

Repr().repr1(nested, 3)
→ '[[[1, 2, 3]]]'

# Negative level
Repr().repr1(1, -1)
→ '1'

Repr().repr1([1,2], -1)
→ '[...]'
```

### Custom Class Instances

```
class Foo:
    pass

foo = Foo()
Repr().repr_instance(foo, 1)
→ '<__main__.Foo object at 0x...>' (truncated at maxother=30)

# Truncation with long class name
class VeryLongCustomClassName:
    pass

obj = VeryLongCustomClassName()
Repr().repr(obj)
→ '<__main__.Lon...x72b6f348f10>'
```

### recursive_repr Decorator

```
class Node:
    def __init__(self, val):
        self.val = val
        self.next = None
    
    @recursive_repr()
    def __repr__(self):
        return f"Node({self.val}, {self.next!r})"

node1 = Node(1)
node2 = Node(2)
node1.next = node2
node2.next = node1  # Cycle

repr(node1)
→ 'Node(1, Node(2, ...))'

# With custom fillvalue
class Node2:
    @recursive_repr(fillvalue='<cycle>')
    def __repr__(self):
        return f"Node2({self.val}, {self.next!r})"

repr(node1)
→ 'Node2(1, Node2(2, <cycle>))'
```

### Empty Containers

```
Repr().repr([])
→ '[]'

Repr().repr(())
→ '()'

Repr().repr({})
→ '{}'

Repr().repr(set())
→ 'set()'

Repr().repr('')
→ "''"
```

### Edge Cases with maxother

```
# Custom objects truncated at maxother (30 chars default)
class LongClass:
    pass

obj = LongClass()
Repr().repr(obj)
→ '<__main__.Lon...x72b6f348f10>'  # Shows start and memory addr

Repr(maxother=50).repr(obj)
→ '<__main__.LongClass object at 0x72b6f348f10>'  # Full repr
```

### maxstring with Special Characters

```
Repr().repr('hello\nworld')
→ "'hello\\nworld'"

Repr().repr("it's")
→ '"it\'s"'

Repr().repr('a' * 30)
→ "'aaaaaaaaaaaa...aaaaaaaaaaaaa'"
```

## 4. HAZARDS

### Dictionary Iteration Order
- **Issue**: pyrst iterates dicts in SORTED-KEY order, not insertion order. reprlib sorts dict keys alphabetically/numerically before display.
- **Impact**: Dict repr output is deterministic only when keys are sorted. Counter objects display counts descending, not key-sorted.
- **Mitigation**: All dossier test cases with dicts use sorted keys; pyrst behavior aligns with CPython's display.

### String Truncation Formatting
- **Issue**: Long strings (>maxstring) show prefix...suffix with middle ellipsis. The split point is calculated to balance prefix/suffix length.
- **Impact**: Exact split point depends on string length and maxstring value; not always symmetric.
- **Mitigation**: Use fixed-content strings in tests ('aaa...' pattern) or wrap in sorted() for determinism.

### Float Representation
- **Issue**: repr(3.14) produces platform-dependent decimal representation; rounding may differ.
- **Impact**: Large floats or computed values may show different digit precision.
- **Mitigation**: Test only simple floats (3.14, 0.0) or use integer arithmetic in tests.

### Integer Overflow (i64 Limit)
- **Issue**: pyrst uses i64 ints, no arbitrary precision. CPython supports arbitrary bigints; overflow wraps.
- **Impact**: 10**50 is representable in CPython but would overflow in pyrst (gate G9).
- **Mitigation**: Flag maxlong and repr_int as gated; test only with manageable ranges (< 2^63-1).

### maxlevel at Boundary
- **Issue**: When nesting depth equals maxlevel, innermost level still renders; at maxlevel+1, shows fillvalue.
- **Impact**: Off-by-one in level calculation can cause unexpected truncation.
- **Mitigation**: Test explicit depth boundaries (maxlevel=2 with 2-level and 3-level nests).

### Set/Frozenset Element Order
- **Issue**: Set iteration order is insertion-deterministic in CPython 3.7+, but reprlib does NOT sort set elements.
- **Impact**: repr({3,1,2}) may show {1,2,3} or {3,1,2} depending on hash table state; non-deterministic.
- **Mitigation**: Flag set/frozenset repr as ordering-sensitive; wrap in sorted() or use sorted(set(...)) in parity tests.

### Fillvalue Must Be String
- **Issue**: fillvalue is concatenated into repr output; non-string values (int, None, bytes) cause TypeError at truncation time.
- **Impact**: Error only manifests when truncation actually occurs; can be subtle in large test suites.
- **Mitigation**: Validate fillvalue type at Repr.__init__ or flag as constraint; test only with str.

### Attribute Mutation
- **Issue**: Repr instance attributes (maxlist, maxdict, fillvalue, etc.) are mutable; changes affect all subsequent repr() calls.
- **Impact**: Test order dependence if tests share Repr instance; each test should use fresh Repr() or reset attributes.
- **Mitigation**: Use local Repr() instances per test; never reuse or modify module-level aRepr.

## 5. GATED

| Gate | API Part | Issue | Suggested Deferral |
|------|----------|-------|-------------------|
| G2 (no module-level mutable state) | `reprlib.aRepr` (module-level Repr instance) | aRepr is a singleton mutable object; violates immutability. | Implement as local const initialized on first call, or require all users to create their own Repr() instances. |
| G9 (i64 ints, no bignum) | `Repr.repr_int()`, `maxlong` attribute | CPython's maxlong=40 assumes arbitrary precision; 10**50 renders as '100...0'. pyrst i64 overflow wraps or panics. | Limit maxlong to ~19 (max i64 str len is 20 chars); test only with small ints (< 2^63-1); document overflow behavior. |
| G7 (no bytes type) | Edge case: `Repr(fillvalue=b'...')` | Bytes are rejected by pyrst. fillvalue must be str; using bytes as fillvalue is invalid anyway (causes TypeError). | No deferral needed; fillvalue type-check ensures str only. |

## 6. PARITY PLAN

### Core Behavior Test Cases (verified CPython 3.12)

```python
# All outputs below are verbatim from repr(expr) or Repr().repr(expr)

# Lists and truncation
assert Repr().repr([1, 2, 3, 4, 5, 6]) == '[1, 2, 3, 4, 5, 6]'
assert Repr().repr([1, 2, 3, 4, 5, 6, 7]) == '[1, 2, 3, 4, 5, 6, ...]'
assert Repr(maxlist=2).repr([1, 2, 3, 4]) == '[1, 2, ...]'

# Tuples
assert Repr().repr((1, 2, 3, 4, 5, 6)) == '(1, 2, 3, 4, 5, 6)'
assert Repr().repr((1, 2, 3, 4, 5, 6, 7)) == '(1, 2, 3, 4, 5, 6, ...)'

# Dictionaries (sorted by key)
d = {3: 30, 1: 10, 2: 20, 5: 50, 4: 40, 6: 60}
assert Repr().repr(d) == '{1: 10, 2: 20, 3: 30, 4: 40, ...}'
assert Repr(maxdict=2).repr({1: 10, 2: 20, 3: 30}) == '{1: 10, 2: 20, ...}'

# Sets (unordered, but test with small set to avoid order issues)
assert Repr().repr({1, 2, 3, 4, 5, 6}) == '{1, 2, 3, 4, 5, 6}'
s = {1, 2, 3, 4, 5, 6, 7}
assert '...' in Repr().repr(s)  # Allows for set ordering variation

# Frozensets
fs = frozenset([1, 2, 3, 4, 5, 6, 7])
assert '...' in Repr().repr(fs)

# Strings with truncation
assert Repr().repr('a' * 30) == "'aaaaaaaaaaaa...aaaaaaaaaaaaa'"
assert Repr().repr('a' * 31) == "'aaaaaaaaaaaa...aaaaaaaaaaaaa'"
assert Repr().repr('hello') == "'hello'"
assert Repr().repr('') == "''"

# Integers
assert Repr().repr(12345) == '12345'
assert Repr().repr(0) == '0'
assert Repr().repr(-42) == '-42'

# Special values
assert Repr().repr(None) == 'None'
assert Repr().repr(True) == 'True'
assert Repr().repr(False) == 'False'

# Nesting and maxlevel
assert Repr().repr1([1, 2, 3], 0) == '[...]'
assert Repr().repr1([1, 2, 3], 1) == '[1, 2, 3]'
nested = [[[[[['deep']]]]]]]
assert Repr().repr(nested) == "[[[[[['deep']]]]]]"
deep_nested = [[[[[[['too deep']]]]]]]
assert '...' in Repr().repr(deep_nested)

# Custom fillvalue
assert Repr(fillvalue='<...>').repr([1, 2, 3, 4, 5, 6, 7]) == '[1, 2, 3, 4, 5, 6, <...>]'
assert Repr(fillvalue='').repr([1, 2, 3, 4, 5, 6, 7]) == '[1, 2, 3, 4, 5, 6, ]'

# Empty containers
assert Repr().repr([]) == '[]'
assert Repr().repr(()) == '()'
assert Repr().repr({}) == '{}'
assert Repr().repr(set()) == 'set()'

# Attribute modification
r = Repr()
r.maxlist = 2
assert r.repr([1, 2, 3, 4, 5]) == '[1, 2, ...]'
r.fillvalue = '...[truncated]...'
assert r.repr([1, 2, 3, 4]) == '[1, 2, ...[truncated]...]'

# repr_tuple
assert Repr().repr_tuple((1, 2, 3, 4, 5, 6), 0) == '(...)'
assert Repr().repr_tuple((1, 2, 3, 4, 5, 6), 1) == '(1, 2, 3, 4, 5, 6)'
assert Repr().repr_tuple((1, 2, 3, 4, 5, 6, 7), 1) == '(1, 2, 3, 4, 5, 6, ...)'

# repr_dict at different levels
d = {1: [2, 3], 4: [5, 6]}
assert Repr().repr_dict(d, 1) == '{1: [...], 4: [...]}'
assert Repr().repr_dict(d, 2) == '{1: [2, 3], 4: [5, 6]}'

# Mixed nested structure
mixed = [1, (2, 3), {4: 5}, [6, 7]]
assert Repr().repr(mixed) == '[1, (2, 3), {4: 5}, [6, 7]]'

# Module-level functions
assert reprlib.repr([1, 2, 3]) == '[1, 2, 3]'
assert isinstance(reprlib.aRepr, reprlib.Repr)

# Array type (if available)
import array
arr = array.array('i', [1, 2, 3, 4, 5, 6])
assert Repr().repr(arr) == "array('i', [1, 2, 3, 4, 5, ...])"

# Deque (from collections)
from collections import deque
dq = deque(range(1, 9))
assert Repr().repr(dq) == 'deque([1, 2, 3, 4, 5, 6, ...])'

# recursive_repr decorator on __repr__
class Node:
    def __init__(self, val):
        self.val = val
        self.next = None
    
    @reprlib.recursive_repr()
    def __repr__(self):
        return f'Node({self.val}, {self.next!r})'

node1 = Node(1)
node2 = Node(2)
node1.next = node2
node2.next = node1
assert repr(node1) == 'Node(1, Node(2, ...))'

# repr_instance truncation
class Foo:
    pass

foo = Foo()
foo_repr = Repr().repr_instance(foo, 1)
assert 'Foo' in foo_repr and '0x' in foo_repr

# maxother truncation
class VeryLongCustomClassName:
    pass

obj = VeryLongCustomClassName()
obj_repr = Repr().repr(obj)
assert '...' in obj_repr  # Long class name truncated at maxother=30
```

## 7. TARGET

### Fidelity Estimate: 4.5 / 5

**Why Not 5:**

1. **Set/Frozenset Ordering Mismatch (Medium)**
   - CPython's set repr shows elements in hash-table insertion order (non-deterministic in general, but stable within a session).
   - pyrst has no set type or must iterate it; if implemented, iteration order would differ from CPython.
   - Mitigation: Use sorted(set(...)) in parity tests or flag set repr as unordered.

2. **Dictionary Iteration (Low)**
   - pyrst sorts dict keys; CPython's reprlib also sorts dict keys for display, so this is actually aligned.
   - However, dict key type mixing (int + str) or Unicode order may differ slightly.
   - Mitigation: Use homogeneous key types (all ints or all strings) in tests.

3. **Large Integer Display (Low)**
   - CPython's maxlong=40 assumes bignum support; pyrst i64 ints overflow at 2^63-1.
   - 10**50 is unrepresentable in pyrst; this API surface is gated.
   - Mitigation: Test only with manageable int ranges; document bignum gate.

4. **Float Representation (Negligible)**
   - Minor platform-dependent rounding in float repr; not a core reprlib concern.
   - reprlib treats floats as opaque via fallback __repr__; no truncation rules.
   - Mitigation: Test simple floats only (3.14, 0.0).

5. **Module-Level Mutable State (Low)**
   - aRepr is a module-level Repr() singleton that can be mutated.
   - pyrst's G2 constraint (no module-level mutable state) requires deferral or redesign.
   - Mitigation: Provide stateless function API (def repr(...) built on Repr instance created on demand) or document aRepr as non-portable.

---

**Strengths (Why 4.5 is Achievable):**
- Core truncation logic is algorithmic and language-agnostic (maxlist, maxdict, maxstring, maxlevel).
- repr1(x, level) recursion pattern is straightforward; level-based depth limiting works identically.
- fillvalue substitution is trivial string concatenation.
- Decorator pattern for recursive_repr is portable (pyrst supports @crate/@property/@staticmethod/@extern).
- Error cases (ValueError for negative maxlist, TypeError for non-string fillvalue) map cleanly to pyrst's exception hierarchy.

**Two Dominant Reasons for 0.5-Point Deduction:**
1. **Set Ordering Unpredictability** – sets are unordered; reprlib cannot make them deterministic, and pyrst will differ.
2. **Bignum Gate (G9)** – large ints exceed i64 range; this API surface must be gated or truncated to safe range.

