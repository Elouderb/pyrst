# numpyrs

An n-dimensional `f64` array library for pyrst, numpy-flavored. Card
0182f2b0 (epic 47cafe10, Track B). Built on E1 (user-class
`__getitem__`/`__setitem__`/`__len__`, including tuple-key subscripts, and
operator dunders).

**Status: IMPLEMENTED.** The core `NDArray` type, all listed constructors,
elementwise arithmetic (same-shape + scalar/size-1 broadcast), reductions,
1D dot / 2D matmul, transpose, and elementwise math ufuncs are implemented and
covered by `tests/test_ops.pyrs` (real numpy-1.26 oracle values pinned in
comments) plus `tests/smoke_ndarray.pyrs`. A worked demo (matrix ops + a
from-scratch gradient-descent linear fit that recovers `y = 2x + 1`) lives at
`extern/programs/numpyrs_demo/`.

Two compiler gaps were found and worked around (never hacked); both are called
out inline below and logged on the card — **`/` and `**` operators do not
dispatch to user dunders** (use `.div()` / `.power()`), and a pre-sized list
declared inside a nested block is mis-lowered (an internal codegen detail, not
user-visible in this API).

## Importing this package

numpyrs is a plain pyrst package (no build step of its own) — a consumer sets
`PYRST_PATH` to the **parent** of `numpyrs/` (i.e. `extern/packages/`) and
imports submodules with dotted names:

```sh
PYRST_PATH=/home/ethos/Coding/pyrst/extern/packages \
  /home/ethos/Coding/pyrst/target/release/pyrst build main.pyrs
```

```python
from numpyrs.ndarray import NDArray
from numpyrs.constructors import array, array2d, zeros, ones, full, arange, linspace
from numpyrs.ufunc import absolute, sqrt, exp, log, sin, cos
```

**All imports (internal and consumer) are dotted, and `PYRST_PATH` must be set
even to `check`/`build` a single file under this package.** A module found via a
`PYRST_PATH` entry is re-rooted at that *entry* directory (`extern/packages/`),
not at the subdirectory holding it, so a bare sibling import (`from ndarray
import ...`) misses — write `from numpyrs.ndarray import ...`. There is no
`numpyrs` package-init module, so `from numpyrs import ufunc` (importing a
submodule as a name) does **not** resolve — import the symbols you need
(`from numpyrs.ufunc import sqrt`).

## Value semantics — read this before using the class

pyrst classes are **clone-on-use**, not Python's reference semantics:

- **`reshape()` / `copy()` COPY.** Every operation that "returns an array"
  allocates a new buffer; there is no numpy-style view sharing anywhere.
- **Chained writes through indexing are a compile error.** `a[i][j] = v` is
  rejected at check time (a class `__getitem__` returns a fresh copy). Use the
  **tuple-key form** `a[i, j] = v` for 2D writes.
- **Arithmetic and scalar methods never mutate.** `a + b`, `a.muls(s)`, etc.
  always allocate and return a new `NDArray`.

## API surface

### Core type — `NDArray`

Fields `data: list[float]` (flat, row-major/C-order) and `shape: list[int]`.
`__init__(data, shape)` validates `len(data) == product(shape)` (else a
`ValueError` naming both). Introspection: `.ndim`, `.size` (`@property`),
`len(a)` (size of the leading axis). `.reshape(new_shape)` and `.copy()` return
copies; `reshape` validates the size match.

### Indexing

- **`a[i, j]` / `a[i, j] = v`** — 2D tuple-key `__getitem__`/`__setitem__`, with
  bounds checks (out-of-range → `ValueError` naming the index and shape).
- **`a.get(i)` / `a.set(i, v)`** — flat (1D) element read/write by a single int.

  > **Why two forms:** pyrst fixes a class's `__getitem__` key type to one
  > annotation, and a `class | float`/`int | tuple`-style **union param is a
  > codegen miscompile** (typechecks, then fails `rustc` — logged as a GAP). So
  > one class cannot overload an `int` key (`a[i]`) *and* a `tuple` key
  > (`a[i, j]`). We chose the 2D tuple key for the bracket form (matmul/2D work
  > is the point); 1D/flat access uses `.get`/`.set`.

### Constructors (`constructors.pyrs`)

| Function | Result |
|----------|--------|
| `array(data: list[float])` | 1D array, shape `[len(data)]` |
| `array2d(rows: list[list[float]])` | 2D array; ragged rows → `ValueError` |
| `zeros(shape)` / `ones(shape)` | filled with `0.0` / `1.0` |
| `full(shape, value)` | filled with `value` |
| `arange(start, stop, step)` | 1D half-open range (numpy semantics; `step != 0`) |
| `linspace(start, stop, num)` | 1D, `num` samples, **endpoint inclusive** (numpy default) |

Nested-list `array()` is split into a distinct `array2d()` entry point because
pyrst has no union/variadic list-literal type to overload one `array()` over
both `list[float]` and `list[list[float]]`.

### Elementwise arithmetic

**Operator dunders** `a + b`, `a - b`, `a * b`, `-a` route correctly. Semantics:
same-shape elementwise, **or** one operand of size 1 (scalar-as-array)
broadcasts across the other. Any other shape pairing raises a numpy-worded
`ValueError` (`operands could not be broadcast together with shapes (…) (…)`).

> **GAP — `/` and `**` do NOT dispatch to dunders.** pyrst routes `+ - *` to
> `__add__/__sub__/__mul__`, but `/` and `**` fall through to builtin float
> arithmetic (a user-class `a / b` is a type error). `__truediv__`/`__pow__`
> are defined (correct, ready for when dispatch lands) but you must call the
> methods today:
> **`a.div(b)`** (elementwise divide) and **`a.power(b)`** (elementwise power).

**Scalar methods** (a scalar `float`, not an array): `a.adds(s)`, `a.subs(s)`,
`a.muls(s)`, `a.divs(s)`, `a.pows(s)`. Provided both for ergonomics and because
the union-typed scalar-overload dunder is the miscompile noted above.

### Comparisons

- `a == b` (`__eq__`) → **`bool`**: whole-array equality (equal shape and all
  elements equal). numpy's elementwise-`==`-returns-a-bool-array is not
  expressible with an f64-only dtype + pyrst's `__eq__ -> bool` rule.
- `a.equal(b)` / `a.less(b)` / `a.greater(b)` → an **NDArray 0.0/1.0 mask**
  (same broadcasting subset as arithmetic). This is the honest elementwise-
  comparison substitute given there is no bool dtype.

### Reductions

`a.sum()`, `a.mean()`, `a.min()`, `a.max()` → `float`; `a.argmin()`,
`a.argmax()` → `int` (index of first extremum). All operate over the whole flat
buffer. `mean`/`min`/`max`/`arg*` on an empty array raise `ValueError`. Per-axis
(`axis=`) reductions are **not** shipped (deferred).

### Linear algebra

- `a.dot(b)` — **1D·1D** inner product → a shape-`[1]` array (pyrst has no 0-d
  scalar array type); **2D@2D** matmul → `[m, p]`. Dimension mismatch →
  `ValueError`. Mixed 1D/2D (matrix·vector) is deferred — reshape the vector to
  a 2D row/column to bridge.
- `a.transpose()` — 2D only (non-2D → `ValueError`).

### Elementwise math (`ufunc.pyrs`, numpy `np.sqrt(a)` style, free functions)

`absolute(a)`, `sqrt(a)`, `exp(a)`, `log(a)`, `sin(a)`, `cos(a)` — each returns a
new same-shape array. `sqrt`/`log` inherit lib/math's CPython domain checks
(`sqrt(x<0)` and `log(x<=0)` raise `ValueError("math domain error")`).

## Errors

Shape mismatches raise `ValueError` with both shapes named, numpy-worded where
cheap (e.g. `operands could not be broadcast together with shapes (2, 3)
(3, 2)`). No silent broadcasting beyond the same-shape / size-1 subset above.

## Non-goals / honest scope limits (what does NOT ship in v1)

- **f64-only.** No separate int dtype; every element is `float`. Pass floats as
  floats (`3.0`, not `3`) — pyrst does not coerce `int`→`float` at call args.
- **No view sharing.** Every reshape/index-read/op copies (value semantics).
  Perf note: clone-on-use makes repeated whole-array ops allocate each time;
  correct but not tuned (not prematurely optimized — logged on the card).
- **General N-D broadcasting** (unequal-rank axis stretch) — only same-shape and
  size-1/scalar broadcast ship. Anything else is an honest `ValueError`.
- **No fancy/boolean-mask indexing**, no slicing of an `NDArray` (`a[i:j]`), no
  per-axis reductions, no mixed 1D/2D `dot`.
- **`/` and `**` operators, `axis=` kwargs** — see the arithmetic/reduction
  GAP notes above.

## Layout

- `ndarray.pyrs` — the `NDArray` class (indexing, arithmetic + scalar ops,
  comparisons, reductions, `dot`/`transpose`, `reshape`/`copy`, `__str__`) plus
  the free helpers `_prod`/`_shape_str`.
- `constructors.pyrs` — `array`/`array2d`/`zeros`/`ones`/`full`/`arange`/
  `linspace`.
- `ufunc.pyrs` — elementwise math free functions.
- `tests/` — `smoke_ndarray.pyrs` (construction/indexing) and `test_ops.pyrs`
  (arithmetic/broadcast/reductions/linalg/ufunc vs pinned numpy oracle values).
  Both print `PASS` and exit nonzero on failure; run them the same way a
  consumer builds (`PYRST_PATH`-relative dotted imports).

See card 0182f2b0 for the full implementation notes and the two `GAP:` findings.
