# kodiak

A dataframe library for pyrst: pandas ergonomics + polars-style columnar
storage (and, in a later phase, an optional lazy pipeline). Card `974fdff3`
(epic `47cafe10`, the epic's capstone/FINAL package). Bear theme.

**Status: Phase 2 (eager analytical core) IMPLEMENTED.** On top of the Phase-1
scaffold, kodiak now has Series element-wise arithmetic + scalar broadcast,
comparison → bool-mask methods, `df.filter(mask)`, stable `sort_values`,
`groupby(...).sum()/.mean()/.count()/.min()/.max()`, `merge` (inner/left),
a `nulls` validity-mask null model with `dropna`/`fillna_float`/`fillna_str`,
`describe`, and CSV `read_csv`/`to_csv` with per-column dtype inference. All
covered by `tests/op_matrix.pyrs` (hand-computed expected values) and driven
end-to-end by the flagship demo `extern/programs/kodiak_demo/`. The lazy
pipeline (Phase 3, `df.lazy()`) and the numpyrs/dateutil/tzdata capstone
integration (Phase 4) are **not yet implemented** — see card `974fdff3`.

> **The single most important divergence to know before reading further:**
> comparison **operators** cannot produce a mask. pyrst hard-types `<`, `>`,
> `==`, … to return `bool` regardless of a `__lt__`/`__eq__` return
> annotation, so `df["x"] > 3` is *not* a Series — a mask comes from a
> **method** (`df["x"].gt(3.0)`), and row filtering is `df.filter(mask)`, not
> `df[mask]`. Details in "Phase 2 forced divergences" below.

## Importing this package

Like numpyrs, kodiak is a plain pyrst package (no build step of its own) — a
consumer sets `PYRST_PATH` to the **parent** of `kodiak/` (i.e.
`extern/packages/`) and imports submodules with dotted names:

```sh
PYRST_PATH=/home/ethos/Coding/pyrst/extern/packages \
  /home/ethos/Coding/pyrst/target/release/pyrst build main.pyrs
```

```python
from kodiak.series import Series
from kodiak.frame import DataFrame
```

There is no `kodiak` package-init module (matching the numpyrs convention):
import the symbols you need from the dotted submodule paths above, not
`from kodiak import Series` and not `from kodiak import series`.

## The column representation (load-bearing — read before touching this code)

pyrst has no general union types, and class-based subtyping / polymorphic
collections are gated/buggy (card `ed56edfb`). A dataframe column is
therefore **not** a single polymorphic value; it is a discriminated
struct-of-arrays:

```python
class Series:
    name: str
    dtype: str            # "i64" | "f64" | "str" | "bool"
    ints: list[int]
    floats: list[float]
    strs: list[str]
    bools: list[bool]     # only the store matching `dtype` is populated
    length: int
```

Every operation switches on `dtype`. A `DataFrame` is order-preserving
parallel arrays:

```python
class DataFrame:
    names: list[str]
    cols: list[Series]    # index-aligned with names
```

`df["col"]` is `names.index(col) -> cols[i]` (a `Series`). This compiles to
plain Rust `Vec`s (no boxing, no dynamic dispatch) — that's half of "polars
speed"; the other half (lazy query-plan fusion) is a later phase.

## Construction — `from_columns`, not a mixed dict-of-lists

```python
age: Series = Series.from_ints("age", [34, 29, 41])
name: Series = Series.from_strs("name", ["Ana", "Bo", "Chi"])
df: DataFrame = DataFrame.from_columns([age, name])
```

### GAP — mixed-dtype dict-of-lists construction is inexpressible

pandas' headline constructor `DataFrame({"a": [1], "b": ["x"]})` needs a dict
whose **value type varies per key** (`list[int]` for `"a"`, `list[str]` for
`"b"`, in the same literal). pyrst's `dict[K, V]` requires exactly one `V`
for every value in the dict. This was probed directly against
`target/release/pyrst`:

```python
d = {"a": [1, 2, 3], "b": ["x", "y", "z"]}
# => type error: dict values have incompatible types: list[int] vs list[str]
```

So `DataFrame.from_columns(cols: list[Series])` is the honest, documented
constructor: the list argument is homogeneous (every element is the *same*
class, `Series`) even though each `Series` internally carries a different
`dtype`. Logged on card `974fdff3` as
`GAP: mixed-dtype dict-of-lists DataFrame construction is inexpressible (dict[K,V] requires one V per dict, no unions) — from_columns(list[Series]) is the constructor.`

## API surface (Phase 1)

### `Series` (`series.pyrs`)

- Constructors: `Series.from_ints(name, list[int])`, `.from_floats(name,
  list[float])`, `.from_strs(name, list[str])`, `.from_bools(name,
  list[bool])` — all `@staticmethod`s, called `ClassName.method(...)`
  exactly like a Python classmethod (same idiom `lib/datetime.pyrs` already
  established for `date.today()` / `date.fromordinal()` etc.).
- `len(s)` (`__len__`) → row count.
- `s.to_strings() -> list[str]` — every cell rendered to a `str` (used by the
  `DataFrame` table renderer and directly usable by consumers).
- `s.head(n)` / `s.tail(n) -> Series` — row-sliced copies (value semantics;
  clamped to `[0, length]`, never raises on an out-of-range `n`).
- `s.row_slice(start, stop) -> Series` — the shared, self-clamping slice
  primitive `head`/`tail` and `DataFrame.head`/`tail` are built on.
- `__str__` — a deterministic single-column pandas-flavored renderer (row
  index + right-justified value, footer `Name: <name>, dtype: <dtype>`).

### `DataFrame` (`frame.pyrs`)

- `DataFrame.from_columns(cols: list[Series])` — the primary constructor
  (see the GAP note above for why there is no dict-of-lists alternative).
- `df["col"] -> Series` (`__getitem__`) — raises `KeyError(repr(col))` for an
  unknown column name (matching pandas' `KeyError: 'col'` shape), *not* a
  bare panic — checked with `in` before calling `list.index` so the error is
  always a real, catchable `KeyError` (a raw `list.index` miss lowers to a
  `ValueError`-tagged `panic!`, which was not verified to be catchable —
  avoided here on purpose).
- `df.shape() -> tuple[int, int]` — `(rows, cols)`. Tuple **indexing must use
  a literal int** (`shape()[0]`) — pyrst types a variable/non-literal tuple
  index as `Unknown` and rejects it; this is a general pyrst constraint, not
  kodiak-specific.
- `df.columns() -> list[str]`.
- `df.head(n)` / `df.tail(n) -> DataFrame` — row-sliced copies; every column
  is sliced against the *same* `[start, stop)` range, preserving row
  alignment.
- `df.with_column(name, s) -> DataFrame` — polars-style (see the FORCED API
  DIVERGENCES note below); returns a **new** frame (value semantics), never
  mutates `self`. Overwrites an existing column of the same name in place;
  otherwise appends.
- `__str__` — a deterministic, pandas-flavored aligned table: an unnamed
  index column plus one column per `Series`, all right-justified (matching
  pandas' actual `to_string()` default — header *and* body are
  right-justified for every dtype, not just numeric), column width =
  `max(len(header), max cell width)`. More than 10 rows triggers pandas'
  head/last-5-plus-ellipsis-row default (deterministic — not
  user-configurable in this scaffold).

## API surface (Phase 2 — the eager analytical core)

### `Series` element-wise arithmetic (`series.pyrs`)

Two numeric columns combine element-wise; result dtype is `i64` when both are
`i64`, otherwise `f64`; `truediv` is always `f64`. Exposed via **both**
operators and method aliases (a result cell is null if either input is null;
lengths must match, or one side may be length 1 for broadcast):

```python
c = df["a"] + df["b"]      # __add__      (also .add)
c = df["a"] - df["b"]      # __sub__      (also .sub)
c = df["a"] * df["b"]      # __mul__      (also .mul)
c = df["a"].truediv(df["b"])   # __truediv__  (also .truediv)
```

**Scalar broadcast** is a set of `float`-taking methods (result `f64`):
`s.add_scalar(x)`, `s.sub_scalar(x)`, `s.mul_scalar(x)`, `s.div_scalar(x)` —
e.g. `s.mul_scalar(2.0)`. (`s * 2` as an operator is *not* available; see the
divergence note.)

### `Series` comparisons → bool-mask `Series` (methods, not operators)

Numeric-vs-scalar (the mask for `filter`): `s.gt(x)`, `s.ge(x)`, `s.lt(x)`,
`s.le(x)`, `s.eq(x)`, `s.ne(x)` (each takes a `float`). String equality:
`s.eq_str(v)`, `s.ne_str(v)`. Numeric Series-vs-Series: `s.gt_series(o)`,
`ge_series`, `lt_series`, `le_series`, `eq_series`, `ne_series`. A null cell
never satisfies a comparison (its mask bit is `False`).

### `DataFrame` operations (`frame.pyrs`)

- `df.filter(mask: Series) -> DataFrame` — keep rows where the bool mask is
  `True`. (Row-mask indexing `df[mask]` is inexpressible; see divergences.)
- `df.sort_values(by: str, ascending: bool = True) -> DataFrame` — a **stable**
  sort of all columns by the `by` column (hand-rolled merge sort); nulls sort
  last. Deterministic.
- `df.groupby(key: str) -> GroupBy`, then `.sum()`, `.mean()`, `.count()`,
  `.min()`, `.max()`, `.std()`. Each returns a **DataFrame**: the sorted-unique
  group-key column followed by one aggregated column per value column. Groups
  are emitted in **ascending key order** (deterministic); a **null key forms no
  group** (pandas' `dropna=True` default). `sum`/`min`/`max` preserve the source
  column's dtype; `mean`/`std` are `f64`; `count` is `i64`. Numeric aggregations
  skip nulls. `sum`/`mean`/`min`/`max`/`std` aggregate numeric value columns
  only; `count` counts every value column.
- `df.merge(other: DataFrame, on: str, how: str = "inner") -> DataFrame` —
  inner or left join on a single key column. **Left row order is preserved**;
  duplicate keys produce a per-row cartesian expansion (each left row × each
  matching right row, in right order). The join key appears once; other right
  columns are appended (a name colliding with a left column is suffixed
  `_right`). For a `left` join, an unmatched left row gets **null** right cells.
- `df.dropna() -> DataFrame` — drop every row that is null in **any** column.
- `df.fillna_float(x: float) -> DataFrame` — fill nulls in **numeric** columns
  with `x`; `df.fillna_str(v: str) -> DataFrame` — fill nulls in **str**
  columns with `v`. (Split by fill-value type because pyrst has no union
  parameter; see divergences.)
- `df.describe() -> DataFrame` — a `statistic` label column
  (`count`/`mean`/`std`/`min`/`max`) plus one `f64` column per numeric input
  column (`std` uses `ddof=1`; a stat that is undefined, e.g. `std` of <2
  values, is null).
- `df.to_csv(path: str) -> None` — write CSV via `lib/csv`; a null renders as
  an empty field.

### CSV input (`io.pyrs`)

- `read_csv(path: str) -> DataFrame` (a free function:
  `from kodiak.io import read_csv`). The first row is the header/column names.
  Per-column dtype is **inferred** from the non-empty cells: all-int → `i64`;
  else all-float-or-int → `f64`; else all `true`/`false` (any case) → `bool`;
  else `str`. An **empty field is a null** in every dtype, so a `to_csv` →
  `read_csv` round-trip reproduces the frame (nulls included). A wholly-empty
  column defaults to `str`.

### The null model (`nulls` validity mask)

Every `Series` carries a parallel `nulls: list[bool]` mask: `nulls[i] == True`
means row `i` is missing (NA); the value in the typed store there is an ignored
placeholder. This is the general model (works for **all** dtypes, unlike an
f64-only NaN sentinel). `from_ints`/`from_floats`/`from_strs`/`from_bools` keep
their `(name, values)` signatures and default to all-valid; the `from_*_n(name,
values, nulls)` variants take an explicit mask (used by `read_csv` and
null-producing ops). A null renders as `NaN` in the table printer and as an
empty field in CSV. Comparisons treat a null as not-satisfying; arithmetic
propagates null; numeric aggregations skip nulls; `dropna` removes any-null
rows; `fillna_*` fills and clears the flag.

## Phase 2 forced divergences (pyrst static-typing limits — logged on card `974fdff3`)

- **Comparison operators can't build a mask.** pyrst types `a < b` / `a == b`
  (and `>`, `>=`, `<=`, `!=`) as `bool` regardless of the `__lt__`/`__eq__`
  declared return type, and dispatches `>`/`>=` through `__lt__`. So a
  comparison cannot yield a bool-mask `Series` via an operator. Masks are
  **methods**: `df["x"].gt(3.0)` (not `df["x"] > 3`). Arithmetic dunders are
  unaffected — they may return the class — so `df["a"] + df["b"]` reads
  pandas-like.
- **`df[mask]` row-masking is inexpressible.** `__getitem__` takes exactly one
  key type (`str`, for column select); a bool-`Series` key is a different type
  and pyrst has no union key types (`DataFrame[...] expects a key of type str,
  but the index is Series`). Use `df.filter(mask)` — the documented API. This
  was the card's anticipated GAP; confirmed.
- **Scalar arithmetic via operators is unavailable.** An arithmetic dunder has
  a single parameter type (`Series`), so `s * 2` / `s + 1.0` as operators
  don't type-check against a scalar; scalar broadcast is the `*_scalar(x:
  float)` methods, and scalars are `float` (an int literal does not coerce to
  a `float` parameter in pyrst). Scalar broadcast therefore promotes to `f64`.
- **`fillna` is split by fill-value type** (`fillna_float` / `fillna_str`)
  because a method cannot take a union-typed fill value.

## Forced API divergences (pyrst static-typing limits — see card `974fdff3`)

- **`assign(**kwargs)` → `with_column(name, series)`.** pandas'
  `df.assign(newcol=expr)` needs `**kwargs`; pyrst has no variadic
  keyword parameters in a `def` signature (confirmed: named/keyword
  arguments *at a call site* do work and map to positional parameters, but a
  function/method **cannot declare** a `**kwargs`-style catch-all). Use the
  polars-style `df.with_column("newcol", series)` instead.
- **Row-mask indexing (`df[mask]`) is deferred to Phase 2**, not implemented
  in this scaffold at all (no comparison ops / bool `Series` yet). The
  design already anticipates `__getitem__` cannot overload both a `str` key
  (column select) and a bool-`Series` key (row filter) in one method — see
  card `974fdff3` comment 608. `df.filter(mask)` is the planned, documented
  fallback once Phase 2 lands boolean comparisons.

## Value semantics — read this before using either class

Like numpyrs, pyrst classes here are **clone-on-use**, not Python reference
semantics: `head`/`tail`/`row_slice`/`with_column` all allocate and return a
**new** object; none mutate `self`. Verified directly in
`tests/smoke_scaffold.pyrs` (`with_column` on `df` does not change `df`'s own
`shape()`).

## Errors

- Unknown `Series` `dtype` (anything other than `i64`/`f64`/`str`/`bool`) →
  `ValueError` from `Series.__init__`, naming the bad value via `repr()`.
- `df["missing_col"]` → `KeyError(repr("missing_col"))`.

## Layout

- `series.pyrs` — the `Series` class: typed stores + the `nulls` validity
  mask, `from_*`/`from_*_n` constructors, `take`/`take_null`/`row_slice`,
  element-wise arithmetic (`__add__`/`add`/…, `*_scalar`), comparison
  methods (`gt`/…/`eq_str`/`*_series`), aggregations (`agg_sum`/`agg_mean`/
  `agg_min`/`agg_max`/`agg_std`/`count`), `fillna_float`/`fillna_str`,
  `to_strings`/`to_field_strings`, `__len__`, `__str__`.
- `frame.pyrs` — the `DataFrame` class (`from_columns`, `__getitem__`,
  `shape`/`columns`/`head`/`tail`/`with_column`, `filter`, `sort_values`,
  `groupby`, `merge`, `dropna`, `fillna_float`/`fillna_str`, `describe`,
  `to_csv`, `__str__`) and the `GroupBy` class
  (`sum`/`mean`/`count`/`min`/`max`/`std`), plus module-private helpers for
  the stable merge-sort (`_argsort`/`_msort`/`_merge_idx`/`_key_*`), grouping
  (`_group`/`_agg_numeric_col`/`_count_col`), gathering
  (`_gather_frame`/`_slice_frame`), and the table renderer.
- `io.pyrs` — `read_csv(path) -> DataFrame` (free function) with per-column
  dtype inference; uses `lib/csv` + `lib/os`.
- `tests/smoke_scaffold.pyrs` — Phase 1 scaffold check.
- `tests/op_matrix.pyrs` — Phase 2 op matrix: hand-computed asserts for
  arithmetic, every comparison, filter, sort (incl. stability), each groupby
  aggregation, inner + left merge, dropna/fillna, null propagation, and a CSV
  round-trip. Prints `PASS`/`FAIL`, nonzero exit on failure.

See card `974fdff3` for the full staged plan (Phase 2 eager core, Phase 3
lazy pipeline, Phase 4 capstone integration with numpyrs/dateutil/tzdata) and
the lead-design comment (608) this scaffold implements.
