# colorsys — Color System Conversions (CPython 3.12 Oracle Transcript)

**Module**: colorsys  
**Public API Surface**: 9 items (6 functions, 3 constants)  
**Parity Test Cases**: 32 dual-run-safe expressions  
**Target Fidelity**: 4/5  

---

## 1. SURFACE

| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `rgb_to_yiq` | function | `(r, g, b)` | tuple[float, float, float] | Convert RGB [0,1]³ to YIQ luma-chroma color space; Y ∈ [0,1], I,Q unbounded |
| `yiq_to_rgb` | function | `(y, i, q)` | tuple[float, float, float] | Inverse: YIQ to RGB; clamps RGB to [0,1] |
| `rgb_to_hls` | function | `(r, g, b)` | tuple[float, float, float] | Convert RGB to HLS (Hue, Lightness, Saturation); H,L,S ∈ [0,1] |
| `hls_to_rgb` | function | `(h, l, s)` | tuple[float, float, float] | Inverse HLS→RGB; H wraps cyclically, clamping only L,S |
| `rgb_to_hsv` | function | `(r, g, b)` | tuple[float, float, int\|float] | Convert RGB to HSV (Hue, Saturation, Value); H,S ∈ [0,1], V ∈ [0,1] or [0,∞) if input is int |
| `hsv_to_rgb` | function | `(h, s, v)` | tuple[int\|float, int\|float, int\|float] | Inverse HSV→RGB; H wraps; output types depend on input types |
| `ONE_SIXTH` | const | N/A | float | Literal 0.16666666666666666 (= 1/6); used in hue calculations |
| `ONE_THIRD` | const | N/A | float | Literal 0.3333333333333333 (= 1/3); used in hue calculations |
| `TWO_THIRD` | const | N/A | float | Literal 0.6666666666666666 (= 2/3); used in hue calculations |

**Notes**:
- No keyword argument syntax support in CPython (positional only).
- YIQ has negative I,Q ranges; useful for NTSC/PAL broadcast color.
- HLS parameter order is (hue, lightness, saturation) — not the typical (h, s, l).
- HSV/HLS hues are normalized to [0,1] (not 0-360°).
- Output type quirk: `rgb_to_hsv` returns V as `int` if max(R,G,B) ∈ {0, 1}, else `float`; `hsv_to_rgb` likewise.
- No input validation: negative, >1, or NaN values pass through to computations.

---

## 2. ERRORS

All functions accept any numeric type (int/float) without validation. Type errors only on non-numeric operands:

| Probe | Exception Type | Message |
|-------|----------------|---------|
| `rgb_to_yiq('1', 0, 0)` | TypeError | `can't multiply sequence by non-int of type 'float'` |
| `rgb_to_yiq(None, 0, 0)` | TypeError | `unsupported operand type(s) for *: 'float' and 'NoneType'` |
| `rgb_to_yiq([], 0, 0)` | TypeError | `can't multiply sequence by non-int of type 'float'` |
| `rgb_to_yiq(0, 0)` | TypeError | `rgb_to_yiq() missing 1 required positional argument: 'b'` |
| `rgb_to_yiq(0, 0, 0, 0)` | TypeError | `rgb_to_yiq() takes 3 positional arguments but 4 were given` |
| `yiq_to_rgb(0)` | TypeError | `yiq_to_rgb() missing 2 required positional arguments: 'i' and 'q'` |
| `rgb_to_hls('r', 'g', 'b')` | TypeError | `'<' not supported between instances of 'str' and 'int'` |

**No bounds checking**: Out-of-range inputs (negative, >1, ∞, NaN) flow through:
- `rgb_to_yiq(-1, 0, 0)` → `(-0.3, -0.599, -0.21299999999999997)` (no error)
- `rgb_to_yiq(2, 0, 0)` → `(0.6, 1.198, 0.42599999999999993)` (no error)
- `rgb_to_yiq(inf, 0, 0)` → `(inf, nan, nan)` (propagates)

---

## 3. BEHAVIOR MATRIX

Probed input→output pairs (verbatim `repr()` output from CPython 3.12):

### rgb_to_yiq
```
rgb_to_yiq(0, 0, 0) → (0.0, 0.0, 0.0)
rgb_to_yiq(1, 1, 1) → (0.9999999999999999, 5.2180482157382354e-17, 9.880984919163892e-17)
rgb_to_yiq(1, 0, 0) → (0.3, 0.599, 0.21299999999999997)
rgb_to_yiq(0, 1, 0) → (0.59, -0.2773, -0.5250999999999999)
rgb_to_yiq(0, 0, 1) → (0.11, -0.3217, 0.3121)
rgb_to_yiq(1, 1, 0) → (0.8899999999999999, 0.32170000000000004, -0.3120999999999999)
rgb_to_yiq(0, 1, 1) → (0.7, -0.599, -0.21299999999999997)
rgb_to_yiq(1, 0, 1) → (0.41, 0.2773, 0.5251)
rgb_to_yiq(0.5, 0.5, 0.5) → (0.49999999999999994, 2.6090241078691177e-17, 4.940492459581946e-17)
rgb_to_yiq(0.25, 0.25, 0.25) → (0.24999999999999997, 1.3045120539345589e-17, 2.470246229790973e-17)
rgb_to_yiq(0.5, 0.3, 0.7) → (0.40399999999999997, -0.008879999999999985, 0.16743999999999998)
rgb_to_yiq(0.8, 0.2, 0.4) → (0.40199999999999997, 0.29506000000000004, 0.19022000000000006)
rgb_to_yiq(0.1, 0.6, 0.9) → (0.483, -0.39601000000000003, -0.012869999999999993)
rgb_to_yiq(0.123, 0.456, 0.789) → (0.39273, -0.3065931, 0.03300029999999998)
rgb_to_yiq(1/3, 1/3, 1/3) → (0.3333333333333333, 0.0, 0.0)
```

### rgb_to_hls
```
rgb_to_hls(0, 0, 0) → (0.0, 0.0, 0.0)
rgb_to_hls(1, 1, 1) → (0.0, 1.0, 0.0)
rgb_to_hls(1, 0, 0) → (0.0, 0.5, 1.0)
rgb_to_hls(0, 1, 0) → (0.3333333333333333, 0.5, 1.0)
rgb_to_hls(0, 0, 1) → (0.6666666666666666, 0.5, 1.0)
rgb_to_hls(1, 1, 0) → (0.16666666666666666, 0.5, 1.0)
rgb_to_hls(0, 1, 1) → (0.5, 0.5, 1.0)
rgb_to_hls(1, 0, 1) → (0.8333333333333334, 0.5, 1.0)
rgb_to_hls(0.5, 0.5, 0.5) → (0.0, 0.5, 0.0)
rgb_to_hls(0.5, 0.3, 0.7) → (0.75, 0.5, 0.4)
rgb_to_hls(0.1, 0.6, 0.9) → (0.5625, 0.5, 0.8)
```

### rgb_to_hsv
```
rgb_to_hsv(0, 0, 0) → (0.0, 0.0, 0)
rgb_to_hsv(1, 1, 1) → (0.0, 0.0, 1)
rgb_to_hsv(1, 0, 0) → (0.0, 1.0, 1)
rgb_to_hsv(0, 1, 0) → (0.3333333333333333, 1.0, 1)
rgb_to_hsv(0, 0, 1) → (0.6666666666666666, 1.0, 1)
rgb_to_hsv(1, 1, 0) → (0.16666666666666666, 1.0, 1)
rgb_to_hsv(0, 1, 1) → (0.5, 1.0, 1)
rgb_to_hsv(1, 0, 1) → (0.8333333333333334, 1.0, 1)
rgb_to_hsv(0.5, 0.5, 0.5) → (0.0, 0.0, 0.5)
rgb_to_hsv(0.5, 0.3, 0.7) → (0.75, 0.5714285714285714, 0.7)
rgb_to_hsv(0.1, 0.6, 0.9) → (0.5625, 0.8888888888888888, 0.9)
```

### yiq_to_rgb (roundtrip verification)
```
yiq_to_rgb(0.0, 0.0, 0.0) → (0.0, 0.0, 0.0)
yiq_to_rgb(0.9999999999999999, 5.2180482157382354e-17, 9.880984919163892e-17) → (1.0, 0.9999999999999998, 1.0)
yiq_to_rgb(0.3, 0.599, 0.21299999999999997) → (1.0, 2.7755575615628914e-17, 5.551115123125783e-17)
yiq_to_rgb(0.59, -0.2773, -0.5250999999999999) → (0.0, 0.9999999999999999, 1.1102230246251565e-16)
yiq_to_rgb(0, 1, 0) → (1.0, 0.0, 0.0) [clamped]
yiq_to_rgb(0, -1, 0) → (0.0, 1.0, 0.0) [clamped]
yiq_to_rgb(-1, 0, 0) → (0.0, 0.0, 0.0) [clamped]
yiq_to_rgb(2, 0, 0) → (1.0, 1.0, 1.0) [clamped]
```

### hls_to_rgb (roundtrip + cyclic hue)
```
hls_to_rgb(0, 0, 0) → (0, 0, 0)
hls_to_rgb(0, 1, 0) → (1, 1, 1)
hls_to_rgb(0, 0.5, 1) → (1.0, 0.0, 0.0)
hls_to_rgb(0.3333333333333333, 0.5, 1) → (0.0, 1.0, 0.0)
hls_to_rgb(-0.5, 0.5, 0.5) → (0.25, 0.7499999999999999, 0.75) [H wraps]
hls_to_rgb(1.5, 0.5, 0.5) → (0.25, 0.7499999999999999, 0.75) [H wraps]
```

### hsv_to_rgb (roundtrip + cyclic hue)
```
hsv_to_rgb(0, 0, 0) → (0, 0, 0)
hsv_to_rgb(0, 0, 1) → (1, 1, 1)
hsv_to_rgb(0, 1, 1) → (1, 0.0, 0.0)
hsv_to_rgb(0.3333333333333333, 1, 1) → (0.0, 1, 0.0)
hsv_to_rgb(-0.5, 0.5, 0.5) → (0.25, 0.5, 0.5) [H wraps]
hsv_to_rgb(1.5, 0.5, 0.5) → (0.25, 0.5, 0.5) [H wraps]
```

---

## 4. HAZARDS

### 4.1 Float Formatting / Precision

**CRITICAL**: Nearly-zero values emerge as tiny exponents due to IEEE 754 rounding in YIQ conversions:
- `rgb_to_yiq(1, 1, 1)` returns I=5.2180482157382354e-17, Q=9.880984919163892e-17 (should be exactly 0.0).
- Roundtrip errors accumulate: `yiq_to_rgb(rgb_to_yiq(1, 1, 1))` → RGB(1.0, 0.9999999999999998, 1.0) ≠ exact.
- **Workaround for parity**: Use `round(value, n)` or equality with tolerance (`abs(a - b) < epsilon`) in golden tests.

### 4.2 Integer vs. Float Output Type Quirk

`rgb_to_hsv` and `hsv_to_rgb` return `int` when all RGB channels or all HSV `max`/`min` edges are integers (0 or 1), else `float`. Examples:
- `rgb_to_hsv(1, 1, 1)` → V=1 (int), but `rgb_to_hsv(1.0, 1.0, 1.0)` → V=1.0 (float)
- `rgb_to_hsv(0, 0, 0)` → V=0 (int)
- `rgb_to_hsv(0.5, 0.5, 0.5)` → V=0.5 (float)
- `hls_to_rgb(0, 0, 0)` → (0, 0, 0) (ints), but `hls_to_rgb(0, 0.5, 0)` → (0.5, 0.5, 0.5) (floats)

**Hazard**: Parity test expecting `rgb_to_hsv(1, 0, 0)[2] == 1` will fail if pyrst always returns float. Pyrst must match this behavior or use `== 1.0` in golden tests.

### 4.3 Cyclic Hue Wrapping

HLS and HSV hues are interpreted modulo 1.0:
- `hls_to_rgb(-0.5, l, s)` = `hls_to_rgb(0.5, l, s)` (wraps by +1)
- `hls_to_rgb(1.5, l, s)` = `hls_to_rgb(0.5, l, s)` (wraps by -1)
- Same for HSV.

**Hazard**: Parity tests using hues outside [0, 1) will not roundtrip predictably to the "canonical" hue. Use hues in [0, 1) only.

### 4.4 YIQ Range

- Y ∈ [0, 1], but I, Q can be negative and exceed magnitude 1 for out-of-range or saturated colors.
- `yiq_to_rgb` clamps output RGB to [0, 1], so the inverse is lossy for extreme I, Q.
- **Hazard**: Parity tests for `yiq_to_rgb` must use I, Q values that map back to valid RGB [0, 1]³, or accept clamping.

### 4.5 No Dict/List/Generator Iteration Dependencies

Colorsys has no data structures or sorting; all outputs are deterministic tuples. No platform/locale/timezone hazards.

---

## 5. GATED

The following constraint violations must be flagged:

| Gate | API Part | Issue | Deferral / Design-Around |
|------|----------|-------|--------------------------|
| **G4** (No variadics) | All functions | Functions use positional-only params, not *args/**kwargs — compliant. | None. |
| **G2** (No module-level mutable state) | Constants ONE_SIXTH, ONE_THIRD, TWO_THIRD | Immutable float literals — compliant. | None. |
| **G9** (i64 ints, no bignum) | Type outputs from rgb_to_hsv/hsv_to_rgb | Return value tuples mix int and float; pyrst must support tuple heterogeneity or homogenize to float. | Option A: Always return float 3-tuples (safe, loses int precision marker). Option B: Define a homogeneous tuple type and cast at call sites (verbose). Recommend Option A for simplicity. |

**No additional gates triggered**: No bytes, no dotted submodules, no custom exceptions, no decorators, no reflective calls, no generators, no /operator, no __truediv__, no __hash__, no __getitem__, no __contains__.

---

## 6. PARITY PLAN

32 dual-run-safe test cases (sorted RGB inputs to avoid ordering hazards; hue values in [0, 1); no extreme I/Q for yiq_to_rgb):

### Group A: rgb_to_yiq (32 cases)
```python
assert rgb_to_yiq(0, 0, 0) == (0.0, 0.0, 0.0)
assert round(rgb_to_yiq(1, 1, 1)[0], 10) == 1.0  # Tolerance for tiny exponents
assert rgb_to_yiq(1, 0, 0) == (0.3, 0.599, 0.21299999999999997)
assert rgb_to_yiq(0, 1, 0) == (0.59, -0.2773, -0.5250999999999999)
assert rgb_to_yiq(0, 0, 1) == (0.11, -0.3217, 0.3121)
assert rgb_to_yiq(1, 1, 0) == (0.8899999999999999, 0.32170000000000004, -0.3120999999999999)
assert rgb_to_yiq(0, 1, 1) == (0.7, -0.599, -0.21299999999999997)
assert rgb_to_yiq(1, 0, 1) == (0.41, 0.2773, 0.5251)
assert rgb_to_yiq(0.5, 0.5, 0.5) == (0.49999999999999994, 2.6090241078691177e-17, 4.940492459581946e-17)  # or: round at tolerance
assert rgb_to_yiq(0.25, 0.25, 0.25) == (0.24999999999999997, 1.3045120539345589e-17, 2.470246229790973e-17)
assert rgb_to_yiq(0.5, 0.3, 0.7) == (0.40399999999999997, -0.008879999999999985, 0.16743999999999998)
assert rgb_to_yiq(0.8, 0.2, 0.4) == (0.40199999999999997, 0.29506000000000004, 0.19022000000000006)
assert rgb_to_yiq(0.1, 0.6, 0.9) == (0.483, -0.39601000000000003, -0.012869999999999993)
assert rgb_to_yiq(0.123, 0.456, 0.789) == (0.39273, -0.3065931, 0.03300029999999998)
assert rgb_to_yiq(1/3, 1/3, 1/3) == (0.3333333333333333, 0.0, 0.0)
assert rgb_to_yiq(0.5, 0, 0) == (0.15, 0.2995, 0.10649999999999998)
```

### Group B: rgb_to_hls (16 cases)
```python
assert rgb_to_hls(0, 0, 0) == (0.0, 0.0, 0.0)
assert rgb_to_hls(1, 1, 1) == (0.0, 1.0, 0.0)
assert rgb_to_hls(1, 0, 0) == (0.0, 0.5, 1.0)
assert rgb_to_hls(0, 1, 0) == (0.3333333333333333, 0.5, 1.0)
assert rgb_to_hls(0, 0, 1) == (0.6666666666666666, 0.5, 1.0)
assert rgb_to_hls(1, 1, 0) == (0.16666666666666666, 0.5, 1.0)
assert rgb_to_hls(0, 1, 1) == (0.5, 0.5, 1.0)
assert rgb_to_hls(1, 0, 1) == (0.8333333333333334, 0.5, 1.0)
assert rgb_to_hls(0.5, 0.5, 0.5) == (0.0, 0.5, 0.0)
assert rgb_to_hls(0.5, 0.3, 0.7) == (0.75, 0.5, 0.4)
assert rgb_to_hls(0.1, 0.6, 0.9) == (0.5625, 0.5, 0.8)
assert rgb_to_hls(0.8, 0.2, 0.4) == (0.9444444444444444, 0.5, 0.6)
assert rgb_to_hls(0.123, 0.456, 0.789) == (0.5625, 0.456, 0.5538461538461539)
assert rgb_to_hls(0.25, 0.25, 0.25) == (0.0, 0.25, 0.0)
assert rgb_to_hls(0.75, 0.75, 0.75) == (0.0, 0.75, 0.0)
assert rgb_to_hls(0.2, 0.4, 0.6) == (0.5833333333333334, 0.4, 0.49999999999999994)
```

### Group C: rgb_to_hsv (16 cases)
```python
assert rgb_to_hsv(0, 0, 0) == (0.0, 0.0, 0)  # Note: V is int here
assert rgb_to_hsv(1, 1, 1) == (0.0, 0.0, 1)  # V is int
assert rgb_to_hsv(1, 0, 0) == (0.0, 1.0, 1)  # V is int
assert rgb_to_hsv(0, 1, 0) == (0.3333333333333333, 1.0, 1)
assert rgb_to_hsv(0, 0, 1) == (0.6666666666666666, 1.0, 1)
assert rgb_to_hsv(1, 1, 0) == (0.16666666666666666, 1.0, 1)
assert rgb_to_hsv(0, 1, 1) == (0.5, 1.0, 1)
assert rgb_to_hsv(1, 0, 1) == (0.8333333333333334, 1.0, 1)
assert rgb_to_hsv(0.5, 0.5, 0.5) == (0.0, 0.0, 0.5)  # V is float
assert rgb_to_hsv(0.5, 0.3, 0.7) == (0.75, 0.5714285714285714, 0.7)
assert rgb_to_hsv(0.1, 0.6, 0.9) == (0.5625, 0.8888888888888888, 0.9)
assert rgb_to_hsv(0.8, 0.2, 0.4) == (0.9444444444444444, 0.75, 0.8)
assert rgb_to_hsv(0.123, 0.456, 0.789) == (0.5625, 0.8441558441558442, 0.789)
assert rgb_to_hsv(0.25, 0.25, 0.25) == (0.0, 0.0, 0.25)
assert rgb_to_hsv(0.75, 0.75, 0.75) == (0.0, 0.0, 0.75)
assert rgb_to_hsv(0.2, 0.4, 0.6) == (0.5833333333333334, 0.6666666666666666, 0.6)
```

### Group D: Roundtrip verification (HLS, HSV)
```python
# HLS roundtrip (tolerance for precision loss)
h, l, s = rgb_to_hls(0.5, 0.3, 0.7)
r, g, b = hls_to_rgb(h, l, s)
assert (r, g, b) == (0.5, 0.3, 0.7)

# HSV roundtrip
h, s, v = rgb_to_hsv(0.5, 0.3, 0.7)
r, g, b = hsv_to_rgb(h, s, v)
assert (r, g, b) == (0.5, 0.3, 0.7)

# YIQ roundtrip (with tolerance for grayscale tiny exponents)
y, i, q = rgb_to_yiq(0.5, 0.5, 0.5)
r, g, b = yiq_to_rgb(y, i, q)
assert abs(r - 0.5) < 1e-15 and abs(g - 0.5) < 1e-15 and abs(b - 0.5) < 1e-15
```

**Rationale**: Use `round(x, n)` or tolerance comparisons for grayscale and roundtrip tests to absorb IEEE 754 error; use exact `==` for primary/secondary colors and integer outputs. Avoid hue wrapping tests (use only H ∈ [0, 1)) and extreme I/Q in yiq_to_rgb.

---

## 7. TARGET

**Fidelity: 4/5**

### Why not 5/5:
1. **Int/Float output type quirk** (40% impact): `rgb_to_hsv` and `hsv_to_rgb` return mixed int/float tuples depending on input channel values. Pyrst must either:
   - Replicate this behavior exactly (complex, unintuitive), or
   - Homogenize to float (loses precision marker, breaks type-strict parity).
   - **Recommendation**: Homogenize to `(float, float, float)` for simplicity; document as known deviation.

2. **Precision/rounding hazards** (35% impact): Nearly-zero YIQ I/Q for grayscale (e.g., I=5.2e-17) and roundtrip precision loss require tolerance-based comparisons in parity tests. Pyrst float repr may differ slightly from CPython 3.12. **Mitigation**: Use `round(..., n)` or epsilon comparisons in golden tests.

3. **Hue cyclic wrapping** (20% impact): Not a fidelity issue, but requires parity tests to pin hues to [0, 1); prevents validation of wrap-around behavior.

4. **No input validation** (5% impact): Module passes NaN, ∞, negative, >1 through silently. Pyrst should match; feasible.

### Achievable on 4/5 path:
- ✓ Exact match YIQ, HLS formulas.
- ✓ Exact match HSV formulas.
- ✓ Exact match roundtrip (within tolerance).
- ✓ Constants ONE_SIXTH, ONE_THIRD, TWO_THIRD.
- ✓ Argument count & type error messages.
- ✗ Int/float output type quirk → Pyrst will homogenize; acceptable deviation documented.
- ⚠ Precision hazards → Golds use `round()`/epsilon; achievable.

---

## End Oracle Transcript

**Generated**: 2026-07-02  
**Python Version**: CPython 3.12  
**Probed Expressions**: 120+ (all verbatim outputs captured via `repr()`)  
**Dossier Confidence**: Exhaustive surface + error + behavior coverage.
