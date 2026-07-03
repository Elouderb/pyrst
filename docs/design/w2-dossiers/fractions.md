# Fractions Module Implementation Dossier

## SURFACE

| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `Fraction` | class | `__init__(numerator=0, denominator=None)` | `Fraction` | Construct rational number; accepts int pairs, single int, string "num/denom" or "int", float, or another Fraction; normalizes to canonical form (reduced via GCD, denominator always positive). |
| `numerator` | property | `self.numerator` | `int` | The numerator of the canonical reduced form; sign is carried here. |
| `denominator` | property | `self.denominator` | `int` | The denominator of the canonical reduced form; always positive. |
| `real` | property | `self.real` | `Fraction` | Returns self (rational numbers are real). |
| `imag` | property | `self.imag` | `int` | Returns 0 (rational numbers have no imaginary part). |
| `__str__` | dunder | `self.__str__()` | `str` | String representation as "numerator/denominator" (e.g., "3/4", "-5/2", "0"). |
| `__repr__` | dunder | `self.__repr__()` | `str` | Repr as `Fraction(numerator, denominator)` (e.g., `Fraction(3, 4)`). |
| `__eq__` | dunder | `self == other` | `bool` | Equality comparison; compares canonical forms; works with Fraction, int. |
| `__ne__` | dunder | `self != other` | `bool` | Inequality comparison. |
| `__lt__` | dunder | `self < other` | `bool` | Less-than comparison; cross-type with int. |
| `__le__` | dunder | `self <= other` | `bool` | Less-than-or-equal comparison. |
| `__gt__` | dunder | `self > other` | `bool` | Greater-than comparison. |
| `__ge__` | dunder | `self >= other` | `bool` | Greater-than-or-equal comparison. |
| `__add__` | dunder | `self + other` | `Fraction` | Addition; returns reduced Fraction; cross-type with int. |
| `__radd__` | dunder | `other + self` | `Fraction` | Reverse addition (int + Fraction). |
| `__sub__` | dunder | `self - other` | `Fraction` | Subtraction; cross-type with int. |
| `__rsub__` | dunder | `other - self` | `Fraction` | Reverse subtraction. |
| `__mul__` | dunder | `self * other` | `Fraction` | Multiplication; cross-type with int. |
| `__rmul__` | dunder | `other * self` | `Fraction` | Reverse multiplication. |
| `__truediv__` | dunder | `self / other` | `Fraction` | True division; always returns Fraction (not float); raises ZeroDivisionError on division by zero. |
| `__rtruediv__` | dunder | `other / self` | `Fraction` | Reverse true division. |
| `__floordiv__` | dunder | `self // other` | `int` | Floor division; returns int (floor of the quotient). |
| `__rfloordiv__` | dunder | `other // self` | `int` | Reverse floor division. |
| `__mod__` | dunder | `self % other` | `Fraction` | Modulo; returns Fraction or int depending on operand. |
| `__rmod__` | dunder | `other % self` | `Fraction` | Reverse modulo. |
| `__pow__` | dunder | `self ** other` | `Fraction` | Exponentiation; other must be int; negative exponent inverts. |
| `__rpow__` | dunder | `other ** self` | varies | Reverse power. |
| `__neg__` | dunder | `-self` | `Fraction` | Negation; returns Fraction with negated numerator. |
| `__pos__` | dunder | `+self` | `Fraction` | Unary plus; returns self. |
| `__abs__` | dunder | `abs(self)` | `Fraction` | Absolute value; returns Fraction with positive numerator. |
| `__bool__` | dunder | `bool(self)` | `bool` | True if numerator != 0; False if numerator == 0. |
| `__hash__` | dunder | `hash(self)` | `int` | Hash value; equal Fractions have equal hashes; consistent with int hashing. |
| `__int__` | dunder | `int(self)` | `int` | Truncation toward zero to int. |
| `__float__` | dunder | `float(self)` | `float` | Conversion to float (may lose precision). |
| `is_integer` | method | `self.is_integer()` | `bool` | True if denominator == 1 (i.e., represents an integer). |
| `limit_denominator` | method | `self.limit_denominator(max_denominator=1000000)` | `Fraction` | Return best rational approximation with denominator <= max_denominator (Stern-Brocot tree search). |
| `as_integer_ratio` | method | `self.as_integer_ratio()` | `tuple[int, int]` | Return (numerator, denominator) pair. |
| `conjugate` | method | `self.conjugate()` | `Fraction` | Return self (Fraction is its own conjugate). |
| `from_float` | classmethod | `Fraction.from_float(f)` | `Fraction` | Construct from float without intermediate string parsing; raises OverflowError for inf, ValueError for NaN. |
| `from_decimal` | classmethod | `Fraction.from_decimal(dec)` | `Fraction` | Construct from decimal.Decimal; raises OverflowError for inf, ValueError for NaN. |

## ERRORS

| Probe | Exception Type | Message Text |
|-------|---|---|
| `Fraction(1, 0)` | `ZeroDivisionError` | `Fraction(1, 0)` |
| `Fraction(1, 2) / Fraction(0, 1)` | `ZeroDivisionError` | `Fraction(1, 0)` |
| `Fraction(None, None)` | `TypeError` | `argument should be a string or a Rational instance` |
| `Fraction("not a fraction")` | `ValueError` | `Invalid literal for Fraction: 'not a fraction'` |
| `Fraction("1.5/2")` | `ValueError` | `Invalid literal for Fraction: '1.5/2'` |
| `Fraction("1/-2")` | `ValueError` | `Invalid literal for Fraction: '1/-2'` |
| `Fraction(1+2j)` | `TypeError` | `argument should be a string or a Rational instance` |
| `Fraction.from_float(float('inf'))` | `OverflowError` | `cannot convert Infinity to integer ratio` |
| `Fraction.from_float(float('nan'))` | `ValueError` | `cannot convert NaN to integer ratio` |
| `Fraction(float('inf'))` | `OverflowError` | `cannot convert Infinity to integer ratio` |
| `Fraction(float('nan'))` | `ValueError` | `cannot convert NaN to integer ratio` |
| `Fraction.from_decimal(Decimal('Infinity'))` | `OverflowError` | `cannot convert Infinity to integer ratio` |
| `Fraction.from_decimal(Decimal('NaN'))` | `ValueError` | `cannot convert NaN to integer ratio` |

## BEHAVIOR MATRIX

| Input | Output (repr) | Notes |
|-------|---|---|
| `Fraction()` | `Fraction(0, 1)` | Default constructor yields zero |
| `Fraction(0)` | `Fraction(0, 1)` | |
| `Fraction(1)` | `Fraction(1, 1)` | |
| `Fraction(5, 2)` | `Fraction(5, 2)` | No reduction needed |
| `Fraction(6, 9)` | `Fraction(2, 3)` | GCD reduction via normalization |
| `Fraction(-3, 4)` | `Fraction(-3, 4)` | Negative numerator |
| `Fraction(3, -4)` | `Fraction(-3, 4)` | Negative denominator moves to numerator |
| `Fraction(-3, -4)` | `Fraction(3, 4)` | Both negative cancel |
| `Fraction(0, 5)` | `Fraction(0, 1)` | Zero numerator always has denominator 1 |
| `Fraction(0, -1)` | `Fraction(0, 1)` | |
| `Fraction("1/2")` | `Fraction(1, 2)` | String parsing |
| `Fraction("42")` | `Fraction(42, 1)` | Integer string |
| `Fraction("  1  /  2  ")` | `Fraction(1, 2)` | Whitespace tolerance |
| `Fraction("-1/2")` | `Fraction(-1, 2)` | Negative in string |
| `Fraction(0.5)` | `Fraction(1, 2)` | Clean float |
| `Fraction(0.25)` | `Fraction(1, 4)` | Clean float |
| `Fraction(0.75)` | `Fraction(3, 4)` | Clean float |
| `Fraction(1.5)` | `Fraction(3, 2)` | Clean float |
| `Fraction(0.1)` | `Fraction(3602879701896397, 36028797018963968)` | Float precision artifact |
| `Fraction(True)` | `Fraction(1, 1)` | Bool as int |
| `Fraction(False)` | `Fraction(0, 1)` | |
| `Fraction(Fraction(3, 4))` | `Fraction(3, 4)` | Copy-like construction |
| `Fraction.from_float(0.5)` | `Fraction(1, 2)` | Explicit float conversion |
| `Fraction.from_decimal(Decimal('0.5'))` | `Fraction(1, 2)` | Explicit decimal conversion |
| `Fraction.from_decimal(Decimal('1.25'))` | `Fraction(5, 4)` | |
| `Fraction(1, 2) + Fraction(1, 3)` | `Fraction(5, 6)` | Addition with common denominator |
| `Fraction(1, 2) - Fraction(1, 3)` | `Fraction(1, 6)` | Subtraction |
| `Fraction(1, 2) * Fraction(1, 3)` | `Fraction(1, 6)` | Multiplication |
| `Fraction(1, 2) / Fraction(1, 3)` | `Fraction(3, 2)` | Division as inversion-multiply |
| `Fraction(1, 2) + 2` | `Fraction(5, 2)` | Cross-type addition |
| `Fraction(3, 2) - 1` | `Fraction(1, 2)` | |
| `Fraction(1, 2) * 4` | `Fraction(2, 1)` | |
| `Fraction(3, 2) / 2` | `Fraction(3, 4)` | |
| `2 + Fraction(1, 2)` | `Fraction(5, 2)` | Reverse addition |
| `2 - Fraction(1, 4)` | `Fraction(7, 4)` | Reverse subtraction |
| `2 * Fraction(3, 4)` | `Fraction(3, 2)` | Reverse multiplication |
| `1 / Fraction(2, 1)` | `Fraction(1, 2)` | Reverse division |
| `Fraction(7, 2) // 2` | `1` | Floor division returns int |
| `Fraction(7, 2) % 2` | `Fraction(3, 2)` | Modulo returns Fraction |
| `Fraction(1, 2) ** 2` | `Fraction(1, 4)` | Exponentiation |
| `Fraction(1, 2) ** -1` | `Fraction(2, 1)` | Negative exponent inverts |
| `Fraction(2, 1) ** 3` | `Fraction(8, 1)` | |
| `Fraction(2, 1) ** 10` | `Fraction(1024, 1)` | Large exponent |
| `Fraction(5, 3) ** 0` | `Fraction(1, 1)` | Zero exponent yields 1 |
| `-Fraction(1, 2)` | `Fraction(-1, 2)` | Negation |
| `-Fraction(-1, 2)` | `Fraction(1, 2)` | |
| `+Fraction(1, 2)` | `Fraction(1, 2)` | Unary plus |
| `abs(Fraction(-3, 4))` | `Fraction(3, 4)` | Absolute value |
| `abs(Fraction(3, 4))` | `Fraction(3, 4)` | |
| `bool(Fraction(0))` | `False` | Zero is falsy |
| `bool(Fraction(1))` | `True` | Non-zero is truthy |
| `bool(Fraction(-1))` | `True` | |
| `Fraction(1, 2) == Fraction(2, 4)` | `True` | Canonical form equality |
| `Fraction(1, 2) == 0` | `False` | Cross-type equality |
| `Fraction(2, 1) == 2` | `True` | |
| `Fraction(1, 2) < Fraction(2, 3)` | `True` | Ordering |
| `Fraction(1, 2) <= Fraction(2, 4)` | `True` | |
| `Fraction(3, 4) > Fraction(1, 2)` | `True` | |
| `Fraction(3, 4) >= Fraction(3, 4)` | `True` | |
| `Fraction(1, 2) < 1` | `True` | Cross-type comparison |
| `Fraction(1, 2) > 0` | `True` | |
| `str(Fraction(3, 4))` | `'3/4'` | str() format |
| `str(Fraction(-5, 2))` | `'-5/2'` | |
| `str(Fraction(0))` | `'0'` | |
| `repr(Fraction(3, 4))` | `'Fraction(3, 4)'` | repr() format |
| `repr(Fraction(-5, 2))` | `'Fraction(-5, 2)'` | |
| `repr(Fraction(0, 1))` | `'Fraction(0, 1)'` | |
| `Fraction(3, 4).is_integer()` | `False` | Non-integer check |
| `Fraction(2, 1).is_integer()` | `True` | Integer check |
| `Fraction(4, 2).is_integer()` | `True` | Reduces to 2/1 |
| `Fraction(3, 4).numerator` | `3` | Property access |
| `Fraction(3, 4).denominator` | `4` | |
| `Fraction(-3, 4).numerator` | `-3` | |
| `Fraction(-3, 4).denominator` | `4` | |
| `Fraction(3, 4).real` | `Fraction(3, 4)` | |
| `Fraction(3, 4).imag` | `0` | |
| `Fraction(3, 4).conjugate()` | `Fraction(3, 4)` | Self-conjugate |
| `Fraction(7, 3).as_integer_ratio()` | `(7, 3)` | Returns tuple |
| `Fraction(3, 4).limit_denominator(100)` | `Fraction(3, 4)` | Already within limit |
| `Fraction(22, 7).limit_denominator(5)` | `Fraction(16, 5)` | Best approximation |
| `Fraction(355, 113).limit_denominator(100)` | `Fraction(311, 99)` | Close but different |
| `float(Fraction(1, 2))` | `0.5` | Conversion to float |
| `float(Fraction(1, 3))` | `0.3333333333333333` | |
| `int(Fraction(7, 2))` | `3` | Truncation toward zero |
| `int(Fraction(5, 3))` | `1` | |
| `int(Fraction(-5, 3))` | `-1` | |

## HAZARDS

1. **Float repr formatting**: `Fraction(0.1)` exhibits float binary-precision artifacts (numerator = 3602879701896397, denominator = 36028797018963968). Construction from floats is not idempotent with their decimal representation. Pyrst ports must document this or reject float constructors.

2. **Hash consistency**: `Fraction(1, 2)` and `Fraction(2, 4)` have identical hashes (equal Fractions must have equal hashes); this is sound and port-safe.

3. **Denominator always positive**: The canonical form always carries the sign in the numerator. `Fraction(3, -4)` normalizes to `Fraction(-3, 4)`. Pyrst must enforce this invariant in the constructor.

4. **GCD normalization**: All Fraction values are reduced via GCD on construction. `Fraction(6, 9)` becomes `Fraction(2, 3)`. Pyrst implementation must compute GCD before storing numerator/denominator.

5. **String parsing format**: Fraction string literals accept "numerator/denominator" or bare integers, but NOT "numerator/-denominator" (the negative sign on the denominator is rejected). Pyrst parser must match this behavior exactly.

6. **Zero denominator handling**: Always raises `ZeroDivisionError` with message `Fraction(1, 0)` (not just the string "0 denominator"). The message includes the full attempted construction.

7. **Division by Fraction always returns Fraction, never float**: `Fraction(3, 4) / Fraction(1, 2)` returns `Fraction(3, 2)`, not `1.5`. Pyrst must ensure `__truediv__` returns Fraction, not float.

8. **Floor division returns int, not Fraction**: `Fraction(7, 2) // 2` returns `1` (int), not `Fraction(1, 1)`. Pyrst must distinguish `//` from `/`.

9. **Cross-type arithmetic**: Operations with int work bidirectionally: `Fraction(1, 2) + 2` and `2 + Fraction(1, 2)` both work and return Fraction. Pyrst must implement `__radd__`, `__rsub__`, etc.

10. **Cross-type comparisons**: `Fraction(1, 2) < 1` and `1 == Fraction(1, 1)` work seamlessly. Pyrst must implement comparison reflection.

11. **Bignum support in CPython**: CPython's Fraction accepts arbitrary-precision numerators and denominators (tested with 2^200). Pyrst has i64 limits and will overflow—this must be flagged as a fidelity loss.

12. **No locale/platform dependence**: String parsing and arithmetic are deterministic across all platforms; no platform-specific formatting.

13. **No randomness**: All operations are deterministic.

14. **Unicode in error messages**: Error messages use standard ASCII; no emoji or extended unicode.

## GATED

| Gate | API Parts | Pyrst Constraint | Suggested Deferral / Design-Around |
|------|-----------|---|---|
| **G9: Bignum** | Construction with numerators/denominators > i64 max (2^63 - 1) | Pyrst is i64-only; CPython supports arbitrary precision. | Accept overflow panic on construction with large literals; document i64 limits; consider deferred bignum arithmetic post-v1.0. Recommendation: **defer**—honest errors at boundary. |
| **G5: __truediv__** | `Fraction(1, 2) / Fraction(1, 3)` returns `Fraction(3, 2)` | Pyrst dunder list does NOT include `__truediv__` (only __add__, __sub__, __mul__, __neg__, __bool__). | Defer `/` operator until __truediv__ is available; suggest `.divide(other) -> Fraction` method as interim. Recommendation: **defer**—core semantic. |
| **G5: __hash__** | `hash(Fraction(1, 2))` returns int hash value | Pyrst dunder list does NOT include `__hash__`. | Defer Fraction from being hashable (cannot use in dicts/sets); add as future dunder. Recommendation: **defer**—users expect dict keys. |
| **G5: __floordiv__, __mod__, __pow__** | `Fraction(7, 2) // 2` (int), `Fraction(7, 2) % 2` (Fraction), `Fraction(1, 2) ** -1` (Fraction) | Pyrst dunder list does NOT include `__floordiv__`, `__mod__`, `__pow__`. | Defer `//, %, **` operators; offer `.floor_divide(other) -> int`, `.modulo(other) -> Fraction`, `.power(exp: int) -> Fraction` methods. Recommendation: **defer**. |
| **G5: __le__, __ge__, __gt__** | `Fraction(1, 2) <= Fraction(2, 3)`, etc. | Pyrst dunder list explicitly says `__eq__ __lt__` only (NOT `__le__` `__ge__` `__gt__`). | Defer `<=, >=, >` operators; provide only `<, ==` and user must negate/combine. Or: offer these as methods `.lte(other)`, `.gte(other)`, `.gt(other)`. Recommendation: **defer** if Pyrst auto-derives these from __eq__/__lt__; otherwise **add as dunders** (lean toward inclusion). |
| **G5: __pos__, __abs__** | `+Fraction(1, 2)` returns `Fraction(1, 2)`, `abs(Fraction(-3, 4))` returns `Fraction(3, 4)` | Pyrst dunder list does NOT include `__pos__`, `__abs__`. | Defer unary `+` and `abs()` builtin support; provide `.abs() -> Fraction` method. Recommendation: **defer** `+` (uncommon); consider `abs()` inclusion if common-enough in downstream use. |
| **G5: Float constructor** | `Fraction(0.5)` and `Fraction.from_float(0.5)` both work | Float binary-precision artifacts (0.1 → 3602879701896397/36028797018963968) may confuse users. | Defer float constructor if precision loss is unacceptable; keep `from_decimal()` for exact construction. Mark float ctor as "experimental—subject to change." Recommendation: **defer** until design clarity (doc caveat or remove). |
| **G5: __int__, __float__** | `int(Fraction(7, 2))`, `float(Fraction(1, 3))` | Pyrst dunder list does NOT include `__int__`, `__float__`. | Defer implicit conversion; require explicit `.numerator // .denominator` for int or `.to_float()` method. Recommendation: **defer**. |
| **G5: from_decimal classmethod** | `Fraction.from_decimal(Decimal('0.5'))` | Requires `decimal` module import (not in pyrst core scope for MVP). | Defer to future; offer `Fraction.from_string(dec_str: str)` as interim for "1.25"-style inputs. Recommendation: **defer**. |
| **G5: Reverse ops (__radd__, __rsub__, __rmul__, __rtruediv__, etc.)** | `2 + Fraction(1, 2)` → `Fraction(5, 2)` | Pyrst likely supports these auto-reflected from forward ops, but verify dunder support. | Depends on Pyrst's dunder reflection rules; likely OK if forward ops are present. Recommendation: **verify** Pyrst's reflection semantics. |

## PARITY PLAN

Twenty safe, dual-run test lines (avoiding float artifacts, ordering, locale):

```python
# Construction & normalization
assert Fraction() == Fraction(0, 1)
assert Fraction(0) == Fraction(0, 1)
assert Fraction(1) == Fraction(1, 1)
assert Fraction(6, 9) == Fraction(2, 3)
assert Fraction(-3, 4) == Fraction(-3, 4)
assert Fraction(3, -4) == Fraction(-3, 4)
assert Fraction(-3, -4) == Fraction(3, 4)
assert Fraction(0, 5) == Fraction(0, 1)

# String construction
assert Fraction("1/2") == Fraction(1, 2)
assert Fraction("42") == Fraction(42, 1)
assert Fraction("-1/2") == Fraction(-1, 2)

# Arithmetic (exact, no float)
assert Fraction(1, 2) + Fraction(1, 3) == Fraction(5, 6)
assert Fraction(1, 2) - Fraction(1, 3) == Fraction(1, 6)
assert Fraction(1, 2) * Fraction(1, 3) == Fraction(1, 6)
assert Fraction(1, 2) + 2 == Fraction(5, 2)
assert Fraction(3, 2) - 1 == Fraction(1, 2)
assert Fraction(1, 2) * 4 == Fraction(2, 1)

# Unary ops
assert -Fraction(1, 2) == Fraction(-1, 2)
assert bool(Fraction(0)) == False
assert bool(Fraction(1)) == True

# Comparisons
assert Fraction(1, 2) == Fraction(2, 4)
assert Fraction(1, 2) < Fraction(2, 3)
assert Fraction(2, 1) == 2
assert Fraction(1, 2) > 0

# Methods
assert Fraction(3, 4).is_integer() == False
assert Fraction(2, 1).is_integer() == True
assert Fraction(3, 4).numerator == 3
assert Fraction(3, 4).denominator == 4
assert Fraction(7, 3).as_integer_ratio() == (7, 3)

# Conversions (clean values only—avoid float artifacts)
assert str(Fraction(3, 4)) == "3/4"
assert str(Fraction(-5, 2)) == "-5/2"
assert str(Fraction(0)) == "0"
```

These avoid:
- Float input (except `from_float()` exact powers of 2)
- `limit_denominator()` (search heuristic may vary slightly)
- `/`, `//`, `%`, `**` operators (gated; require deferral)
- `abs()`, `+` unary (gated)
- `int()`, `float()` conversions (gated)

## TARGET

**Fidelity: 3/5**

**Reasons it isn't 5:**

1. **Arbitrary-precision integers not supported in pyrst (G9)**: CPython Fraction handles 2^200-bit numerators/denominators seamlessly; pyrst is i64-only. All numerator/denominator values exceeding ±2^63 - 1 will panic at construction. This is a **semantic discontinuity** for large rational numbers; any code constructing `Fraction(10**100, 1)` will fail.

2. **Division operator (__truediv__) gated (G5)**: The `/` operator is the primary interface for division in the fractions module; pyrst cannot express `Fraction(1, 2) / Fraction(1, 3)` using the natural syntax. A `.divide(other)` method is a workaround but breaks API parity and user expectation.

3. **Seven additional dunders unavailable (__hash__, __floordiv__, __mod__, __pow__, __le__, __ge__, __gt__; also __abs__, __pos__, __int__, __float__)**: These cover hashing, advanced arithmetic, comparison sugar, and conversion. Together, they represent ~30% of the public surface and are expected methods in production use (hash for caching, floor division for practical math, etc.). Their absence compounds to significant API friction.

**Why it could reach 4:**
- Fix __truediv__ (landing in Pyrst soon?).
- Accept i64 overflow as honest panic (documented).
- Defer hashability and advanced ops as "future enhancements."
- Reverse ops (__radd__, etc.) likely auto-derive in pyrst.

**Fidelity scorecard:**
- ✓ Construction, normalization, sign handling, basic dunders (__init__, __str__, __repr__, __eq__, __lt__, __add__, __sub__, __mul__, __neg__, __bool__).
- ✓ Properties (numerator, denominator, real, imag).
- ✓ Methods (is_integer, as_integer_ratio, conjugate, limit_denominator).
- ✓ Error handling (ZeroDivisionError, ValueError, TypeError, OverflowError).
- ✗ Arbitrary-precision numerators/denominators (i64 limit).
- ✗ True division operator (gated).
- ✗ Hashing (gated).
- ✗ Floor division, modulo, exponentiation (gated).
- ✗ Comparison completeness (__le__, __ge__, __gt__ gated).
- ✗ Unary plus, abs, int, float conversions (gated).
- ✗ Decimal constructor (out-of-scope).
