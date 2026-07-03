# W2 Pre-Flight Compiler Probe — findings & recommended cards

**Purpose.** Empirically probe the pyrst compiler *before* ~23 new stdlib modules (W2 waves 2a–2e of `docs/design/stdlib-full.md` §F) are written in parallel, so every shared compiler gap becomes **one explicit card** instead of 23 bespoke per-module workarounds (the W1 functools/itertools lesson).

**Baseline.** Worktree HEAD `7440b90` (W1 shipped). `cargo build --release` clean (exit 0). Compiler CLI: `pyrst check|build|emit` (there is **no `run`** subcommand — `build <f>.pyrs` drops a native binary named `<f>` in cwd; you execute it). CPython reference: `python3` 3.12. Probe files + harness: this directory (`probe.sh <file>` runs check → build → run, capturing each phase).

**Verdict legend.**
- **WORKS** — check + build + run all succeed and match python3.
- **GATED-honest** — `check` rejects with a clear diagnostic (safe: author is told up front).
- **GATED-silent(!)** — `check` **accepts** but the program is wrong. Two tiers, both P0:
  - **P0-CRITICAL** — check passes, build passes, **run produces WRONG output** (true silent miscompile — the worst failure the project's honesty invariant exists to prevent).
  - **P0** — check passes, **`rustc` build fails** (silent-until-late: no pyrst diagnostic, a wall of rustc errors instead).
- **MISSING** — feature entirely absent; honest parse/type error.

---

## Verdict summary

| # | Probe | Verdict | One-line |
|---|---|---|---|
| 1 | Cross-type dunder returns (`Moment - Moment -> Duration`, `+ Duration -> Moment`, `__lt__`, chains) | **WORKS** | datetime arithmetic shape is fully supported; matches python3. |
| 2 | Class-level constants / enum members (`Color.RED`) | **P0 (silent build-fail)** | `check` OK; `rustc` E0423 (`Color.RED` emits field-access, not `Color::RED`). Enum blocked. |
| — | Module-level int const table (errno ~40) | **WORKS** | 45 consts + 30 fns: check 4 ms, build 263 ms, 4.3 MB bin. errno fine. |
| 3 | `@dataclass` | **P0 (silent no-op) confirmed + broader** | ANY class decorator is swallowed (even `@totally_made_up`). Ctor/`==` come free from plain classes; `@dataclass` adds nothing and `repr()` of the result build-fails. |
| 4 | Missing str methods (casefold, translate, maketrans, rsplit, format, isdecimal) | **GATED-honest** | Clean `type error: str has no method X`. |
| — | `str.expandtabs(n)` | **P0-CRITICAL (wrong output)** | Naive tab→n-spaces; `"x\ty".expandtabs(4)` len 6 vs python 5. Also mis-serves the *shipped* textwrap. |
| — | Existing str methods (splitlines, partition, center, ljust/rjust, zfill, isidentifier, title, swapcase) | **WORKS (1 divergence)** | partition/rpartition return **list**, python returns **tuple**. |
| 5 | `repr()` fidelity | **P0 (containers) + P0-CRITICAL (float)** | `repr(list/dict/tuple/None)` build-fails; `repr(1.0)`→`"1"` (WRONG, python `"1.0"`). `repr(str)` OK. |
| 6 | StringIO as pure pyrst (mutable self, buffer, seek/read/write) | **WORKS** | Full pattern builds + runs. Slicing `buf[pos:]`, default args, mutable `self.field` all fine. |
| — | File-typed parameter (`def f(x: file)`) | **MISSING (honest)** | Confirmed unspellable: passing `open()` gives `expected file, found file`. File I/O must stay inline in `with open()`. |
| 7 | Class instance as dict key / set member (`dict[Node,int]`, `set[Node]`) | **P0 (silent build-fail)** | No `Eq`/`Hash` derive even with a `__hash__` method. graphlib's core structure blocked. |
| — | `Optional[Class]` field assignment (`obj.f = bare`) | **P0 (silent build-fail)** | No `Some`-wrap in **field-assign** position (E0308). *Return* + *var-decl* positions DO wrap. |
| — | Recursive self-referential field (`next: Optional[LNode]`) | **P0 (silent build-fail)** | E0072 infinite size; no `Box` indirection. Linked lists / trees / graph nodes blocked. |
| 8 | Exceptions: raise/catch builtin + user, subclass catch, `except Exception`, `as e`+`str(e)` | **WORKS** | All robust incl. TypeError/OSError/FileNotFoundError, user `Error(Exception)`, hierarchy catch, message extraction. |
| — | `except (A, B)` tuple | **MISSING (honest)** | Parse error; workaround = separate `except` clauses. |
| 9 | Recursion depth (difflib/graphlib) | **WORKS (divergence)** | 100 k deep OK (python limit is 1 000!). 500 k → uncatchable stack-overflow abort, not `RecursionError`. |
| 10 | Float formatting parity (`str`/f-string) | **WORKS** | Full parity incl. hard round-trip `10.0/3.0`→`3.3333333333333335`. Only `repr(float)` is broken (see #5). |
| — | Scientific-notation float literal `1e20` | **MISSING (honest)** | Parse error. |
| 11 | Mini Fraction end-to-end (`__add__/__sub__/__mul__/__lt__/__eq__/__str__` + gcd) | **WORKS** | Correct output; tuple-swap `a,b = b,a%b` works. |
| — | `sorted(list[UserClass])` over `__lt__` | **WORKS** | p12b honesty hole is now FIXED for `sorted()`. |
| — | `min()`/`max()` over `__lt__` class | **P0 (silent build-fail)** | Still needs `Ord` (E0277). Inconsistent with `sorted()`. datetime/fractions `min(dates)` blocked. |
| 12 | Module scale (45 consts + 30 fns) | **WORKS** | See row under #2. DCE/emit/binary-size all fine. |
| — | Hex/octal/binary/underscore int literals (`0xFF 0o777 0b1010 1_000`) | **MISSING (honest)** | All parse errors; only plain decimal. stat octal perms blocked. |
| — | Module const = expression (`A = B \| C`) | **GATED-honest** | Literal-only rule rejects it. stat's ORed perms must be pre-computed decimals. |
| — | List-unpacking `a, b, c = xs` (list) | **P0 (silent build-fail)** | E0308 Vec-vs-tuple. **Tuple**-unpack works; **list**-unpack doesn't. partition-unpack inherits this. |
| — | `with` over user `__enter__/__exit__` (contextlib) | **GATED-honest** | Clean "context-manager protocol not yet supported" error + workaround. |
| — | `@property` | **WORKS** | |

**Headline P0s (must not ship a stdlib on top of these):**
- **P0-CRITICAL, wrong output:** `str.expandtabs` (wrong tab math), `repr(float)` (drops `.0`). These *silently miscompile* — accepted, built, and run with the wrong answer.
- **P0, silent build-fail (check-pass → rustc-wall):** `repr()`/`str(tuple)` of containers; list-unpacking; `Optional[Class]` field-assign; recursive class fields; class as dict-key/set-member; `min()/max()` over a user class; class-level constants; `str/print` of a class without `__str__`.

---

## The print / str / repr matrix (load-bearing for pprint, reprlib, csv, colorsys)

`print(x)` uses a full python-parity formatter for **every** type. `str(x)` and `repr(x)` route through Rust `Display` (`format!("{}", …)`) and are only partially wired.

| call | list | dict | tuple | str | int | float | class w/`__str__` | class w/o `__str__` | None |
|---|---|---|---|---|---|---|---|---|---|
| `print(x)` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ (parity) | ✓ | ✗ build-fail | (n/a) |
| `str(x)`   | ✓ | ✓ | ✗ build-fail | ✓ | ✓ | ✓ (parity) | ✓ | ✗ build-fail | ✗ |
| `repr(x)`  | ✗ build-fail | ✗ build-fail | ✗ build-fail | ✓ `'…'` | ✓ | ✗ **`1.0`→`1` WRONG** | ✗ build-fail | ✗ build-fail | ✗ build-fail |

**Consequence:** `pprint` and `reprlib` are *fundamentally* repr-of-containers modules → **blocked** until `repr()` covers containers and `repr(float)` is fixed. They cannot be faithful without hand-rolling their own formatter (exactly the per-module workaround this pre-flight exists to prevent).

---

## Per-probe detail (repro + verbatim observed behavior)

### Probe 1 — Cross-type dunder returns — **WORKS**
```python
class Duration:
    secs: int
    def __init__(self, secs: int) -> None: self.secs = secs
    def __lt__(self, other: Duration) -> bool: return self.secs < other.secs
class Moment:
    epoch: int
    def __init__(self, epoch: int) -> None: self.epoch = epoch
    def __sub__(self, other: Moment) -> Duration: return Duration(self.epoch - other.epoch)   # -> DIFFERENT class
    def __add__(self, other: Duration) -> Moment: return Moment(self.epoch + other.secs)        # -> Moment
    def __lt__(self, other: Moment) -> bool: return self.epoch < other.epoch
    def __eq__(self, other: Moment) -> bool: return self.epoch == other.epoch
```
`check ok · build ok · run` → `400 / 1000 / False / True / False` — byte-identical to python3. Cross-type returns, `__lt__`/`__eq__`, and mixed chains `(a-b) < (c-Moment(700))` all work. **The datetime arithmetic shape (`datetime-datetime=timedelta`, `date+timedelta=date`) is fully supported today.**

### Probe 2 — Class-level constants / enum members — **P0 (silent build-fail)**
```python
class Color:
    RED: int = 1
    GREEN: int = 2
def main() -> None: print(Color.RED)
```
`check` → `ok`. `build` → **`error[E0423]: expected value, found struct 'Color' ... help: use Color::RED`**. Class-level data becomes struct fields; `Color.RED` emits `.`-access on the type. No associated-const codegen. **Enum members are unavailable this way.** Fallback = module-level consts (works, below), but that loses namespacing + the enum type.

Module-const table (errno/stat scale) — **WORKS**: a generated module with **45** `NAME: int = <lit>` consts + 30 fns → `check` 4 ms, `build` 263 ms, 4.3 MB binary, correct output. errno's ~40 int consts are a non-issue.

### Probe 3 — `@dataclass` — **P0 (silent no-op), and broader than dataclass**
- Shipped `examples/dataclass_demo.pyrs` builds + runs (`3.0/4.0/15.0`) — but so does a **plain class with no `__init__` and no decorator** (`Point(3.0,4.0)` works): the positional constructor is a *general class feature*, not a dataclass one.
- `@dataclass` adds **nothing**: `repr(p)` on a `@dataclass` → **`error[E0277]: Point doesn't implement Display`** (no `__repr__` synthesized). `p == q` returns `True`, but that is pyrst's default struct `PartialEq`, not the decorator.
- **`@totally_made_up_decorator class Point:` also builds + runs.** So *every* class decorator is silently swallowed, not just `@dataclass`.

Verdict: confirmed live honesty hole. **Recommend synthesize-or-reject** (card below).

### Probe 4 — String builtins — mostly **WORKS**; one **P0-CRITICAL**
Missing (all **GATED-honest** `type error: str has no method X`): `casefold`, `translate`, `maketrans`, `rsplit`, `format`, `isdecimal`.
Present + correct vs python3: `splitlines`, `center`, `ljust`, `rjust`, `zfill`, `isidentifier`, `title`, `swapcase`.
Divergences:
- **`partition`/`rpartition` return a `list`**, python returns a `tuple` → `['k','=','v']` vs `('k','=','v')`. Combined with the list-unpack bug, the idiomatic `k, sep, v = line.partition("=")` **build-fails** (configparser/csv pattern).
- **`str.expandtabs` — P0-CRITICAL wrong output.** `"x\ty".expandtabs(4)` → pyrst `len 6` (`x`+4 spaces), python `len 5` (column-aware tab stop). Naive replace-tab-with-n-spaces. Silently miscompiles; also affects the already-shipped `textwrap`.

### Probe 5 + 10 — repr / str / float formatting — see matrix above
`str()`/`print()`/f-string of **float** have full parity incl. the hard round-trip case (`10.0/3.0` → `3.3333333333333335`). **`repr(float)` is the sole float bug** (`repr(1.0)`→`1`, `repr(100.0)`→`100` — P0-CRITICAL). `repr()` of any container and `str(tuple)` build-fail. Scientific literal `1e20` is a parse error.

### Probe 6 — StringIO pure-pyrst — **WORKS**; file-param **MISSING**
```python
class StringIO:
    buf: str
    pos: int
    def __init__(self, initial: str = "") -> None: self.buf = initial; self.pos = 0
    def write(self, s: str) -> int: self.buf = self.buf + s; return len(s)
    def read(self) -> str: r = self.buf[self.pos:]; self.pos = len(self.buf); return r
    def seek(self, p: int) -> None: self.pos = p
```
Builds + runs correctly. Mutable `self`, slicing, default args all fine.
File-typed params: `def f(fh: file)` / `File` / `TextIO` / `IO` all *type-check* but passing a real `open()` handle → **`type error: argument 1 ... expected file, found file`** — no surface syntax names the builtin `Ty::File`. **Confirmed unspellable.** `with open(...) as f:` itself works. StringIO therefore cannot share a parameter type with real files (each helper must be written against one or the other).

### Probe 7 — Class instances in dicts/sets + Optional[Class] fields — **P0 (silent build-fail) ×3**
- `dict[Node,int]` / `set[Node]` (with `__eq__` + `__hash__` defined) → **`E0599 ... Node: Eq / Node: Hash not satisfied`**. No derive emitted. (Generic `T` used as a set element *does* infer `Hash+Eq` — see `examples/bounded_generics_hashable.pyrs` — but a **concrete** user class does not.)
- `obj.optfield = Inner(5)` where field is `Optional[Inner]` → **`E0308 ... expected Option<Inner>, found Inner (help: wrap in Some)`**. Field-assign position doesn't coerce; **return + var-decl positions do** (`examples/optional_class.pyrs` builds — `return Point(3,4)` auto-wraps).
- `next: Optional[LNode]` self-reference → **`E0072 recursive type LNode has infinite size (help: Box)`**. No indirection inserted.

### Probe 8 — Exceptions — **WORKS** (one honest gap)
`raise X("msg")` + `except X` works for **all** of ValueError/TypeError/OSError/FileNotFoundError/KeyError/IndexError/RuntimeError/NotImplementedError/StopIteration/OverflowError. Subclass catch (`except OSError` catches `FileNotFoundError`) ✓; `except Exception` catch-all ✓; `except X as e: print(str(e))` → `the message` ✓; user `class CsvError(Exception)` ✓. Only `except (A, B)` tuple is a **MISSING** parse error (workaround: separate clauses). **W2 modules can map their CPython exception hierarchies to existing names cleanly.**

### Probe 9 — Recursion — **WORKS** (documented divergence)
Linear recursion to depth 1 000 / 10 000 / 100 000 all succeed (CPython's default limit is **1 000**). Depth 500 000 → `thread 'main' has overflowed its stack, aborting` (exit 134) — an **uncatchable abort, not `RecursionError`**. difflib/graphlib recursion (bounded by input size, typically ≪100 k) is fine; prefer iterative algorithms (e.g. Kahn topo-sort) for unbounded inputs. No artificial limit needed.

### Probe 11 — Fraction end-to-end — **WORKS**; `min/max` **P0**
Full mini-`Fraction` (`__add__/__sub__/__mul__/__lt__/__eq__/__str__` + gcd normalization + tuple-swap) → correct (`5/6 · 1/6 · 1/6 · False · True · 2/1`). `sorted(list[Fraction])` over `__lt__` **now works** (p12b fixed). BUT `min(list[Fraction])`/`max(...)` → **`E0277 Frac: Ord not satisfied`** — silent build-fail, inconsistent with `sorted()`. Methods returning the class's own type work (that's the Fraction constructor path).

### Probe 12 — Module scale + literals
Scale fine (see #2). **Hex/octal/binary/underscore literals all parse-error** (`0o170000`, `0xFF`, `0b1010`, `1_000`) — only decimal. Module const = expression (`256|128|64` or `A+B+C`) → honest `top-level constants must be NAME: T = <literal>`. **stat is the casualty**: octal perms + ORed permission constants must be hand-converted to decimal literals.

---

## RECOMMENDED CARDS (ordered by leverage)

Leverage = (# W2 modules unblocked) × severity, P0-CRITICAL wrong-output weighted highest. Sizes: **S** ≈ a focused codegen/lexer change, **M** ≈ multi-site + tests, **L** ≈ new type-system surface.

### C1 — `repr()`/`str()` completeness + `repr(float)` parity — **P0-CRITICAL · M**
Wire the existing python-parity value-formatter (the one `print()` and `str(list/dict)` already use) into **`repr()` for all container types** (list, dict, tuple, `None`, and classes with `__str__`/`__repr__`) and into **`str(tuple)`**; and route **`repr(float)`** through the parity float formatter so `repr(1.0)=="1.0"` (today `"1"` — a silent miscompile). Contains the worst finding class.
Unblocks: **pprint, reprlib** (hard-blocked today), colorsys, fractions, csv, and every module that reprs a value. Also closes two silent miscompiles.

### C2 — List-unpacking `a, b, c = <list>` (+ decide partition/rpartition shape) — **P0 · S/M**
Emit an index-based destructure (or a length-checked `try_into`) when the RHS of a multi-target assignment is a `list`, instead of `let (a,b,c) = <Vec>` (E0308). Decide partition/rpartition: either return a real 3-tuple (so `k,sep,v = s.partition("=")` works and repr matches python) or keep list + fix unpack. Tuple-unpack already works; this is the list gap.
Unblocks: **csv, configparser, shlex, difflib, datetime** — the single most widely-hit silent hole in W2.

### C3 — `str.expandtabs` correct tab-stop algorithm — **P0-CRITICAL · S**
Replace naive tab→N-spaces with the column-aware algorithm (advance to next multiple of tabsize, reset on `\n`). Small, self-contained, and it fixes a live wrong-output bug in the **already-shipped** `textwrap` as well.
Unblocks: textwrap (correctness), any 2b/2d text module using expandtabs.

### C4 — User classes as dict keys / set members (`Eq` + `Hash`) — **P0 · M**
When a concrete user class is used as a `dict` key or `set` element (or defines `__hash__`), emit `#[derive(PartialEq, Eq, Hash)]` (or a manual `impl Hash` delegating to `__hash__`). The generic-`T` path already infers `Hash+Eq`; extend it to concrete classes.
Unblocks: **graphlib** (nodes-as-keys is its central structure), and any module keying on objects (difflib equivalence classes, ordered-set patterns).

### C5 — `Optional[Class]` auto-`Some` in field-assignment + recursive-field indirection — **P0 · M**
Two linked coercion gaps: (a) `obj.optfield = bare_value` must auto-wrap in `Some` like return/var-decl positions already do (E0308 today); (b) self-/mutually-recursive class fields (`next: Optional[LNode]`) must get `Box` indirection to break the infinite-size cycle (E0072 today).
Unblocks: **graphlib, difflib**, datetime (optional components), and any linked-list / tree / adjacency structure.

### C6 — `@dataclass` synthesis OR honest rejection (close the decorator honesty hole) — **P0 · M (synthesize) / S (reject)**
Today **every** class decorator (incl. `@dataclass` and arbitrary names) is silently swallowed. Either (a) implement real `@dataclass` synthesis (`__repr__` as `Point(x=…, y=…)`, structural `__eq__`, optional `order=`), reusing the free constructor; or (b) make an unrecognized class decorator an honest `check` error. Do not leave it silent.
Unblocks: **dataclasses** (2e); restores the honesty invariant for all class decorators.

### C7 — Enum members / class-level constants — **P0 · M** (or **S** interim honest-reject)
Emit class-level `NAME: T = <literal>` as Rust associated `const`s and lower `Class.NAME` to `Class::NAME` (today it emits field-access → E0423). This is also the natural substrate for a real `enum` (variants + `.value`/`.name`). Interim: an honest `check` error steering to module-level consts.
Unblocks: **enum** (2e); any class exposing named constants.

### C8 — `min()`/`max()` over user comparable classes — **P0 · S**
`sorted()` over a `__lt__` class already works (p12b fixed); make `min()`/`max()` consistent — lower them via a `PartialOrd` fold (`.min_by`/`reduce`) instead of Rust `Ord`-requiring `.min()` (E0277 today).
Unblocks: **datetime, fractions** (`min(dates)`, `max(fractions)`); any module reducing user objects.

### C9 — Non-decimal integer literals + underscores — **S · (mostly stat)**
Lex `0x…`, `0o…`, `0b…`, and `1_000` underscore separators (all parse errors today). Optionally fold in scientific float literals (`1e20`). Honest today, but forces ugly hand-conversion.
Unblocks: **stat** (octal perms) cleanly; quality-of-life for os/errno/colorsys authors.

### C10 — Module-level constant expressions — **S/M · (stat)**
Allow constant-foldable expressions in `NAME: T = <expr>` (e.g. `S_IRUSR | S_IWUSR`, `A + B`) instead of literal-only, with compile-time folding. Honest today (clear reject), so lower urgency than the P0s.
Unblocks: **stat** (ORed permission masks) without pre-computing decimals; combined-flag constants generally.

### C11 — Missing str methods (`casefold`, `translate`/`maketrans`, `rsplit`) — **S each; batch · GATED-honest**
Add the emittable subset. `casefold` → configparser (case-insensitive keys), `rsplit` → shlex/general splitting, `translate`/`maketrans` → text sanitizing. Honest rejects today, so schedulable after the P0s, but batching avoids N modules each re-deriving a workaround.
Unblocks: configparser, shlex, and misc text modules (2b).

### C12 — Context-manager protocol (`__enter__`/`__exit__`) — **M · GATED-honest**
`with` over a user class is a clean honest error today; a faithful `contextlib` needs the general protocol lowered (RAII/`Drop`-style or try/finally desugar). Honest, so not a silent risk — scope as contextlib's own sub-card (as the design doc already flags).
Unblocks: **contextlib** (2e), user context managers generally.

---

## W2 module readiness map (which cards gate each module)

| Batch | Module | Status today | Gated on |
|---|---|---|---|
| 2a | **datetime** | mostly GREEN (cross-type dunders + sorted work) | C8 (min/max of dates), C5 (optional fields), define `__str__` for printing |
| 2a | calendar, time(strftime/gmtime) | GREEN | — (pure list/loop/format) |
| 2b | **csv** | gated | C2 (row unpacking), C11 (rsplit) |
| 2b | **configparser** | gated | C2 (`k,sep,v=partition`), C11 (casefold) |
| 2b | **shlex** | gated | C2, C11 (rsplit) |
| 2b | **difflib** | gated | C2, C4/C5 (sequence structures), C1 (repr of opcodes) |
| 2b | html, fnmatch | GREEN | — (replace/re-backed) |
| 2c | **fractions** | mostly GREEN (probe 11) | C8 (min/max), C1 (repr(float) if reprd) |
| 2c | **colorsys** | GREEN-ish | C1 (repr(float)) if it reprs tuples; `print(tuple)` already works |
| 2c | **graphlib** | **most gated** | **C4** (nodes as dict keys — central), C5 (adjacency/optional refs) |
| 2c | copy | GREEN | — (value semantics) |
| 2c | **pprint, reprlib** | **hard-blocked** | **C1** (repr of containers + repr(float)) |
| 2d | **io.StringIO** | GREEN standalone (probe 6) | — (file-param interop unspellable, but StringIO itself works) |
| 2d | **errno** | GREEN (probe 12) | — (decimal int consts) |
| 2d | **stat** | gated | C9 (octal), C10 (ORed masks) |
| 2d | pathlib.PurePath, shutil, tempfile, filecmp, platform, getpass, sys(partial) | GREEN-ish | — (mostly `@extern`/pure; not deeply probed here) |
| 2e | **enum** | gated | **C7** (class consts / enum feature) |
| 2e | **dataclasses** | honesty hole | **C6** (synthesize or reject) |
| 2e | **contextlib** | gated (honest) | C12 (CM protocol) |

**Net:** the highest-leverage compiler cards for W2 are **C1 (repr/str + float)**, **C2 (list-unpack)**, **C4 (class hash keys)**, **C5 (Optional-field + recursive fields)** — each unblocks 3+ modules and each is a *silent* failure today. **C3 (expandtabs)** and the **`repr(float)` half of C1** are P0-CRITICAL wrong-output miscompiles and should land first regardless of size (they're small). Everything else is honest and schedulable behind them.
