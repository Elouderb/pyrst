# CPython `pprint` → pyrst Implementation Dossier

**Module:** `pprint`  
**Scope:** `pformat`, `pprint`, `pp`, `isreadable`, `isrecursive`, `saferepr`, `PrettyPrinter` class  
**Key Features:** width/indent/depth/compact kwargs, dict key sorting, line wrapping, recursive structure detection

---

## 1. SURFACE

| Name | Kind | Signature | Return | Semantics |
|------|------|-----------|--------|-----------|
| `pformat` | fn | `pformat(object, indent=1, width=80, depth=None, *, compact=False, sort_dicts=True, underscore_numbers=False)` | `str` | Format object to indented string; line-wrap at width; limit nesting to depth |
| `pprint` | fn | `pprint(object, stream=None, indent=1, width=80, depth=None, *, compact=False, sort_dicts=True, underscore_numbers=False)` | `None` | Pretty-print to stream (default sys.stdout); prints pformat output + newline |
| `pp` | fn | `pp(object, *args, sort_dicts=False, **kwargs)` | `None` | Shorthand pprint with sort_dicts defaulting to False; passes remaining args to pprint |
| `isreadable` | fn | `isreadable(object)` | `bool` | True if saferepr(object) is eval-safe (no lambdas/custom objects) |
| `isrecursive` | fn | `isrecursive(object)` | `bool` | True if object contains circular references |
| `saferepr` | fn | `saferepr(object)` | `str` | repr() that marks recursive refs as `<Recursion on TYPE with id=...>` instead of crashing |
| `PrettyPrinter` | class | `__init__(indent=1, width=80, depth=None, stream=None, *, compact=False, sort_dicts=True, underscore_numbers=False)` | — | Configurable pretty-printer; hold settings across multiple format/pprint calls |
| `PrettyPrinter.pformat` | method | `pformat(object)` | `str` | Format using instance settings |
| `PrettyPrinter.pprint` | method | `pprint(object)` | `None` | Print using instance settings + configured stream |
| `PrettyPrinter.isreadable` | method | `isreadable(object)` | `bool` | Readability check using instance depth |
| `PrettyPrinter.isrecursive` | method | `isrecursive(object)` | `bool` | Recursion check using instance depth |

---

## 2. ERRORS

| Input | Exception | Message |
|-------|-----------|---------|
| `pformat(..., width=0)` | `ValueError` | `width must be != 0` |
| `pformat(..., width=-5)` | (no error; treated as tiny width) | — |
| `pformat(..., indent=-1)` | `ValueError` | `indent must be >= 0` |
| `pformat(..., depth=0)` | `ValueError` | `depth must be > 0` |
| `pformat(..., depth=-1)` | `ValueError` | `depth must be > 0` |
| `pformat(..., width='invalid')` | `ValueError` | `invalid literal for int() with base 10: 'invalid'` |
| `pformat(..., width=1.5)` | (no error; int() coerces) | — |
| `pp(obj, arg1, arg2, ...)` | (passed to pprint) | — |

---

## 3. BEHAVIOR MATRIX

**Format Tests (width/indent/depth/sort/underscore):**

```python
pformat(42) => '42'
pformat(3.14) => '3.14'
pformat('hello') => "'hello'"
pformat(True) => 'True'
pformat(None) => 'None'
pformat([]) => '[]'
pformat({}) => '{}'
pformat(()) => '()'
pformat(set()) => 'set()'
pformat([1, 2, 3]) => '[1, 2, 3]'
pformat((1, 2, 3)) => '(1, 2, 3)'
pformat({'a': 1}) => "{'a': 1}"

# Width-induced wrapping
pformat([1,2,3,4,5,6,7,8], width=20)
  => '[1,\n 2,\n 3,\n 4,\n 5,\n 6,\n 7,\n 8]'

pformat({'a':1, 'b':2}, width=20)
  => "{'a': 1, 'b': 2}"  # doesn't wrap if fits

pformat({'a':[1,2]}, indent=4, width=30)
  => "{'a': [1, 2]}"  # indent only affects multi-line output

# Nested structures + indent
pformat(
  {"key1": "value1", "key2": "value2", "nested": {"inner1": "data1", "inner2": "data2"}},
  indent=4,
  width=30
)
  => (4-space indent on wrapped lines)
  "{   'key1': 'value1',\n    'key2': 'value2',\n    'nested': {   'inner1': 'data1',\n                  'inner2': 'data2'}}"

# Depth limiting
pformat({'a': {'b': {'c': {'d': 1}}}}, depth=1) => "{'a': {...}}"
pformat({'a': {'b': {'c': {'d': 1}}}}, depth=2) => "{'a': {'b': {...}}}"
pformat([[[1]]], depth=1) => '[[...]]'

# Dict sorting
pformat({3:3, 1:1, 2:2}, sort_dicts=True) => '{1: 1, 2: 2, 3: 3}'
pformat({3:3, 1:1, 2:2}, sort_dicts=False) => '{3: 3, 1: 1, 2: 2}'  # insertion order

# Underscore numbers
pformat(1000000, underscore_numbers=False) => '1000000'
pformat(1000000, underscore_numbers=True) => '1_000_000'
pformat(1_234_567_890, underscore_numbers=True) => '1_234_567_890'

# Special float values
pformat(float('inf')) => 'inf'
pformat(float('-inf')) => '-inf'
pformat(float('nan')) => 'nan'

# String escaping
pformat('hello\nworld') => "'hello\\nworld'"
pformat('hello\tworld') => "'hello\\tworld'"
pformat("hello'world") => '"hello\'world"'
pformat('hello"world') => '\'hello"world\''

# Bytes (still supported, repr'd)
pformat(b'hello') => "b'hello'"

# Mixed type dict keys
pformat({1: 'one', 'two': 2, (3, 4): 'tuple'}) => "{1: 'one', 'two': 2, (3, 4): 'tuple'}"
```

**Function Behavior Tests:**

```python
isreadable(42) => True
isreadable([1,2,3]) => True
isreadable(lambda: None) => False
isreadable(object()) => False

isrecursive([1,2,3]) => False
lst = [1,2]; lst.append(lst)
isrecursive(lst) => True

saferepr([1,2,3]) => '[1, 2, 3]'
lst = [1,2]; lst.append(lst)
saferepr(lst) => '[1, 2, <Recursion on list with id=...>]'

# pprint writes to stream with trailing newline
pprint([1,2,3], stream=stream_obj)  # stream receives '[1, 2, 3]\n'

# pp is wrapper; sort_dicts defaults to False
pp({3:1, 1:2}, sort_dicts=False)  # prints in insertion order (3, 1)
pprint({3:1, 1:2}, sort_dicts=False)  # same behavior

# PrettyPrinter instance
pp_inst = PrettyPrinter(indent=2, width=40)
pp_inst.pformat({'a': [1,2]})  # respects configured indent/width
```

---

## 4. HAZARDS

### Dict Iteration & Sorting
- **CRITICAL:** `sort_dicts=True` (default) sorts dicts by key, overriding Python 3.7+ insertion order.
- **pyrst alignment:** pyrst dicts iterate in sorted-key order already, so `sort_dicts=True` is natural; `sort_dicts=False` must be carefully tested.
- Tests using `{3:..., 1:..., 2:...}` will have different output order if sort_dicts differs.

### Float Formatting
- `float('inf')` formats as literal `'inf'`, not `float('inf')`.
- `float('nan')` formats as `'nan'`.
- Scientific notation: `1.23e-10` and `1.23e+20` pass through as-is.
- **Hazard:** repr(0.1) in Python may differ in precision; test verifies exact CPython output.

### String Escaping
- Single vs. double quotes chosen to minimize escapes: prefers `'string'` if no single quotes inside, else `"string"`.
- Backslash escapes: `\n`, `\t`, `\\` expanded in output.
- Unicode handled natively; no locale dependence observed.

### Recursion Detection
- Uses object identity (id) to detect cycles; `<Recursion on TYPE with id=XXXXXXX>` marker.
- **Hazard:** Recursion marker includes runtime id(), which varies per run — unsuitable for exact text matching in tests.

### Compact Parameter
- **Ambiguous behavior:** `compact=True/False` produces identical output in tested scenarios; may only affect very specific nesting patterns not covered.

### Width Edge Cases
- `width=-1` or other negative widths: treated as very small width (allows one element per line).
- `width=0`: raises ValueError.
- Width applies to overall line length; indentation counts toward it.

---

## 5. GATED

| Constraint | API Part | Reason | Deferral / Workaround |
|------------|----------|--------|----------------------|
| **G4: No *args/**kwargs** | `pp(object, *args, sort_dicts=False, **kwargs)` | Uses *args to pass through to pprint, **kwargs for remaining options. | Design pp as a simple passthrough function without variadics; accept explicit (stream, indent, width, depth, compact, underscore_numbers) params instead. E.g., `pp(object, stream=None, indent=1, width=80, ...)`. |
| **No custom exception classes** | (all funcs) | Module raises standard ValueError/TypeError, which pyrst supports. | No deferral needed; use ValueError/TypeError. |
| **No bytes type** | `saferepr(b'hello')` | bytes currently format as `b'...'` repr; pyrst has no bytes. | **DEFER:** Mark bytes as not-yet-supported in stdlib pprint; return a Type error or unsupported marker for bytes inputs. |
| **Module-level mutable state** | (none detected) | pprint is pure; no global state mutated. | No deferral needed. |

---

## 6. PARITY PLAN

Safe parity test cases (verified on CPython 3.12, no ordering/timing hazards):

```python
# Basic types — deterministic
pformat(42) == '42'
pformat(3.14) == '3.14'
pformat('hello') == "'hello'"
pformat(True) == 'True'
pformat(False) == 'False'
pformat(None) == 'None'

# Empty collections
pformat([]) == '[]'
pformat({}) == '{}'
pformat(()) == '()'
pformat(set()) == 'set()'

# Simple lists (no sorting)
pformat([1, 2, 3]) == '[1, 2, 3]'
pformat([]) == '[]'

# Tuples
pformat((1, 2, 3)) == '(1, 2, 3)'
pformat((1,)) == '(1,)'
pformat(()) == '()'

# Strings with escapes
pformat('hello\\nworld') == "'hello\\\\nworld'"
pformat('hello\\tworld') == "'hello\\\\tworld'"

# Booleans and None
pformat(True) == 'True'
pformat(False) == 'False'
pformat(None) == 'None'

# Floats
pformat(0.1) == '0.1'
pformat(1.0) == '1.0'
pformat(float('inf')) == 'inf'
pformat(float('-inf')) == '-inf'

# Special integer formatting
pformat(1000000, underscore_numbers=True) == '1_000_000'
pformat(1000000, underscore_numbers=False) == '1000000'

# Width-induced wrapping (deterministic)
pformat([1, 2, 3, 4, 5], width=20).count('\\n') >= 1  # multiline
pformat([1], width=80) == '[1]'  # single line

# Depth limiting (deterministic)
isreadable({'a': {'b': 1}}) == True
isreadable(lambda: None) == False

# Recursion detection (deterministic on non-cyclic)
isrecursive([1, 2, 3]) == False
isrecursive({'a': 1}) == False

# saferepr on non-cyclic objects
saferepr([1, 2]) == '[1, 2]'
saferepr({'a': 1}) == "{'a': 1}"

# Dict sorting (deterministic with sort_dicts=True)
pformat({3: 'c', 1: 'a', 2: 'b'}, sort_dicts=True) == "{1: 'a', 2: 'b', 3: 'c'}"

# Dict insertion order (deterministic with sort_dicts=False, given insertion order)
pformat({'z': 1, 'a': 2}, sort_dicts=False) == "{'z': 1, 'a': 2}"

# PrettyPrinter instance
pp = PrettyPrinter(indent=1, width=80)
pp.pformat([1, 2, 3]) == '[1, 2, 3]'
pp.isreadable([1, 2]) == True
pp.isrecursive([1, 2]) == False

# Edge: width=0 raises ValueError
try:
    pformat([], width=0)
    result = False  # should not reach
except ValueError:
    result = True  # expected
assert result == True

# Edge: negative indent raises ValueError
try:
    pformat([], indent=-1)
    result = False
except ValueError:
    result = True
assert result == True

# Edge: depth must be > 0
try:
    pformat([], depth=0)
    result = False
except ValueError:
    result = True
assert result == True
```

---

## 7. TARGET

**Fidelity Estimate: 4/5**

**Reasons pprint is not 5/5:**

1. **variadics gate (pp function):** `pp(object, *args, **kwargs)` relies on *args/**kwargs which pyrst cannot support (G4 gate). Workaround: design pp without variadics or mark as partial-support.

2. **bytes type (saferepr):** CPython pprint formats bytes as `b'...'` repr; pyrst has no bytes type (no bytes literal, no bytes type). Must defer or stub.

3. **Recursion detection marker context:** `saferepr()` on cyclic structures embeds runtime `id()` in the output (e.g., `<Recursion on list with id=12345>`), making the exact output non-deterministic and unsuitable for golden-match testing. Pyrst should either omit the id, use a placeholder, or document this limitation.

**Migration pathway:**

- ✅ Implement pformat, pprint, isreadable, isrecursive, saferepr as module functions.
- ✅ Implement PrettyPrinter class with all methods.
- ✅ Dict sorting (sort_dicts param) naturally aligns with pyrst's sorted-dict-iteration.
- ⚠️ **pp:** Simplify signature; remove *args variadics, expose only (object, stream=None, indent=1, width=80, depth=None, compact=False, sort_dicts=False, underscore_numbers=False).
- ⚠️ **bytes:** Stub or reject with TypeError; document as not-yet-supported.
- ⚠️ **saferepr recursion marker:** Consider omitting or normalizing id; or document that marker format is non-portable.
- ✅ All error cases (ValueError for invalid width/indent/depth) are straightforward.

---

## Appendix: CPython Probe Commands

All claims verified via:
```python
import pprint
pprint.pformat(obj, **kwargs)  # probe output in REPL
pprint.isreadable(obj)
pprint.isrecursive(obj)
pprint.saferepr(obj)
# etc.
```

No undocumented internals relied upon; API is fully public and stable across Python 3.7+.
