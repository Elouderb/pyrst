# Full Standard Library — Coverage Map, Fidelity Policy & Wave Plan

**Design card:** e7708d1d. **Status:** design only, no source modified; every capability claim below is **empirically probed** against the release binary or source-cited. **Date:** 2026-07-02. **Baseline:** HEAD `eb5d3b4` (v0.1.3). **Supersedes:** `docs/design/stdlib-outline.md` (the 2026-06-28 planning outline, written before lazy generators, closures, default args, and the 15 shipped modules landed).

## Bottom line

The user's goal is verbatim: *"replicate the current Python standard library — anything that doesn't have to be installed manually should be included, as closely interchangeable, if not syntactically identical to, the Python standard library."* CPython 3.12 ships **212 public top-level modules** (+ 89 private `_`-prefixed C-internals). This doc maps every one to **(A) feasible now**, **(B) gated on a named compiler feature**, or **(C) out of scope by design**, audits the 15 already-shipped modules against their CPython APIs, de-duplicates the gating features into an effort-classed dependency graph, and sequences the whole epic into six waves.

**The single most important finding, empirically confirmed:** the modules users most expect — `datetime`, `argparse`, `logging`, `subprocess`, `sqlite3`, `hashlib`, `dataclasses` — are exactly the ones gated on pyrst's **three deepest architectural gaps**: no **opaque `@extern` handle type** (a pyrst value can't hold a live `regex::Regex` / `chrono::DateTime` / hasher / socket across calls — today `re` recompiles its regex on *every* call), no **dotted submodules** (`import os.path` silently truncates to `import os`, a latent miscompile), and no **module-level mutable state** (`sys.argv`, `random`'s module-level API). A naive "port the stdlib" effort stalls on this infra. The recommendation is therefore: **ship the ~54 feasible-now modules and the 15 fidelity upgrades first (real, immediate value, zero compiler risk), and fund each gating feature as its own explicit compiler card — never as a hidden dependency smuggled inside a module card.**

Counts (planning estimate, §C): **A ≈ 54 feasible-now · B ≈ 33 gated · C ≈ 125 out-of-scope** (public) + 89 private → 1 grouped row. The good news the outline could not yet see: **lazy generators make `itertools.count`/`cycle`/`islice` real** (probe p15: an infinite `count()` runs `0 1 2 3` and stops), and **closures make `functools.partial` real** (probe p16). Neither is gated on module-mutable-state — answering the outline's open question directly: **no**, `count` is a generator, not a global-backed free function.

**Parity testing is proven** (§G): a *single* `.pyrs` source runs unmodified under both `pyrst` and `python3` — `parity_reduce_bisect.pyrs` emits `120/3/2/15` byte-identical on both — once the Python side prepends `from __future__ import annotations` to neutralize pyrst-only annotations (`Mut[T]`, `Iterator[T]`). This makes CPython itself the golden oracle for every pure-pyrst module.

Six waves, each gate = **per-module python3-diffed parity golden + full suite green + 0 warnings + docs**:

- **W0 — Close the honesty holes + land the parity harness** (compiler, small). Three empirically-found check-passes/rustc-build-fails holes (`"%d" % x`, module-global reassignment, `sorted()` over a user `__lt__` class) violate the project's core invariant and must become honest `check` errors or real features *before* a stdlib leans on them.
- **W1 — Fidelity-upgrade the 15 shipped modules** using features that already exist (default args, closures, lazy generators, generics). ~15 cards.
- **W2 — New pure-pyrst modules feasible today** (`datetime`, `calendar`, `csv`, `difflib`, `fractions`, `copy`, `pathlib.PurePath`, …). ~17 cards.
- **W3 — Dotted-submodule compiler epic** (also fixes the silent-truncation bug) → faithful `os.path`, `urllib.parse`. ~4 cards.
- **W4 — Module-level-mutable-state compiler epic** → `sys.argv`, `random` module API, `logging` root logger. ~5 cards.
- **W5 — `bytes` type + opaque-`@extern`-handle compiler epics** → `base64`/`hashlib`/`struct`, and crate-backed `re.Pattern`/`Match`, `datetime`-via-chrono, `sqlite3`, compression. Highest risk, sequenced last. ~12 cards.
- Networking + async/threading: **explicitly OUT** (§C).

---

## A. Method & the empirical capability envelope

The outline classified modules from *expected* language features. This doc re-grounds every claim in the **v0.1.3 compiler as it actually is**, because the language moved a lot (generators went lazy, closures/default-args/generics landed) and because a stdlib is only as honest as the checker underneath it. Probes live in `scratchpad/probes*/` and were run as `pyrst check` (fast) and, for anything whose *runtime* or *build* behavior matters, `pyrst build && ./bin`.

### A.1 Capability probe results (the load-bearing table)

| # | Capability | Verdict | Evidence |
|---|---|---|---|
| p01 | Default arguments `def f(x, g="Hi")` | **WORKS** | check-ok; full `Expr` defaults (parser.rs:478) |
| p02 | Keyword args at call site `f("w", greeting="Hi")` | **WORKS** | check-ok (`Call.kwargs`, parser.rs:1285) |
| p18b | `Optional` param `y: int \| None = None` + `is not None` narrowing | **WORKS** | check-ok |
| p19 | Tuple return + unpack `q, r = divmod2(...)` | **WORKS** | check-ok |
| p13 | `ord()` / `chr()` | **WORKS** | check-ok (types.rs:403-404) |
| p08 | `float("inf")` / `float("nan")` | **WORKS** (runtime `inf`) | `f64::from_str` accepts it (mod.rs:669) |
| p11 | Custom `class MyError(Exception)`, `raise`/`except MyError` | **WORKS** (by *exact* name) | runtime "caught"; no hierarchy |
| p12b | User-class `__lt__`/`__eq__`, direct `a < b` | **WORKS** | check-ok |
| p15 | **Infinite** generator `count()` (`while True: yield`) | **WORKS**, lazy | runtime `0 1 2 3` then `break` |
| p16 | Closure returning `Callable` (→ `functools.partial`) | **WORKS** | runtime `15` |
| — | f-strings | **WORKS** | PYTHON_COMPATIBILITY.md:202 |
| p03/p04 | `*args` / `**kwargs` in `def` | **NO** (parse error) | "expected identifier, found Star/DoubleStar" |
| p10 | `global` / `nonlocal` keyword | **NO** (parse error) | absent from lexer/parser |
| p06 | `str.format()` | **NO** (typeck) | "type `str` has no method `format`" |
| p07 | `bytes` literal `b"..."` | **NO** (parse error) | PYTHON_COMPATIBILITY.md:38 |
| p14 | `\xNN` / `\uXXXX` string escape | **NO** (lex error) | "unknown escape '\x'" |
| p17 | `2 ** 63` / arbitrary-precision int | **NO** — `**` yields `float`; `int` is i64 | "declared int, got float" |
| p20 | Set algebra `a & b` / `\|` / `-` / `^` | **NO** — `&` types as `int` | "declared set[int], got int" |
| — | Opaque `@extern` handle (hold `regex::Regex` etc.) | **NO** | consultant, types.rs `enum Ty` has no variant |
| — | Dotted submodule `import os.path` | **NO** — silently truncates to `os` | resolver.rs:125 |
| — | Function/method overloading | **NO** | no dispatch mechanism in src |
| — | Generic methods in a class `def m[U](self)` | **NO** (parse reject) | parser.rs:1539; free generics OK |

### A.2 The honesty holes (must be closed before a stdlib leans on them)

Three probes **passed `pyrst check` but failed at `rustc` build** — the exact "silent-until-late" failure mode the project's *honest-errors-over-silent-miscompiles* invariant exists to prevent (README:41 claims no accepted-program-produces-wrong-output; these are accepted-program-fails-to-build, one tier better than wrong output but still a broken contract for a stdlib author):

| Probe | `check` says | `rustc` says | Why it matters for a stdlib |
|---|---|---|---|
| p05 `"%d" % x` | ok | `error[E0308]` | `%`-formatting is *the* classic Python string idiom; a stdlib (and its users) will reach for it. Must be a real feature or an honest `check` error. |
| p09 module-const reassigned in a fn | ok | `error[E0425] cannot find value` | Directly the module-mutable-state gap; a contributor will write it and get a rustc wall, not a pyrst message. |
| p12b `sorted([b, a])` over a user `__lt__` class | ok | `error[E0277] D: Ord` | `datetime`/`Fraction`/`Decimal` sorting: user comparable objects don't get `Ord`, so `sorted(list_of_objects)` mis-checks. Needs `sorted(key=…)` guidance or a real total-order lowering. |

These are **W0** work: each becomes either a real feature or a clear `check`-time diagnostic. They are cheap relative to the credibility cost of a stdlib whose examples fail at `rustc`.

### A.3 The three architectural constraints that shape everything

1. **No opaque `@extern` handle type.** `@extern` binds a Rust *expression template* returning a value in pyrst's **closed structural type set** (`Int Float Bool Str Unit List Set Dict Tuple Option Iterator Class File TypeVar Func`; types.rs `enum Ty`). There is no way to declare a pyrst type whose runtime representation is a foreign Rust struct. `lib/re.pyrs` is the tell: every wrapper returns `bool`/`str`/`list[str]` and **recompiles `regex::Regex::new(...)` on each call** because it cannot store the compiled object. This blocks `re.Pattern`/`re.Match`, `datetime` objects via `chrono`, `hashlib` hasher objects, `sqlite3` connections, compression streams, `Decimal`. Either build G1 (a real opaque-handle `Ty`) or reimplement the module in pure pyrst (feasible for `datetime`/`hashlib`, infeasible for `sqlite3`/`ssl`).

2. **No dotted submodules.** The resolver uses `path[0]` only and silently drops the rest (resolver.rs:125). `import os.path` compiles as `import os` with no `.path` semantics — a *silent* wrong import, not an error. This blocks the entire package-shaped surface (`os.path`, `urllib.parse`, `xml.etree.ElementTree`, `collections.abc`, `concurrent.futures`, `importlib.metadata`, `http.server`, `email.mime`). G3.

3. **No module-level mutable state.** A top-level binding is accepted only if its RHS is a *literal* and is emitted as an immutable Rust `const` (checks.rs:1082, analysis.rs:1207); any other top-level statement is a hard error; there is no `global`/`nonlocal`. This blocks `sys.argv`, `os.environ` *mutation*, `random`'s module-level convenience API, `logging`'s root logger, `warnings` filters, `locale`/`gettext` state. G2.

---

## B. Fidelity policy — what "syntactically identical" means here

pyrst is a statically typed, AOT-compiled language with Python-flavored syntax — **not** a Python runtime (README:43). "Syntactically identical" is therefore a *target*, achieved fully for some modules and honestly bounded for others. The policy is four contracts + a score.

**B.1 Contracts (in priority order).**

1. **Import form.** Both `import X` and `from X import y` must resolve, and qualified `X.y(...)` must work — already true for all 15 shipped modules, including generic ones (`import heapq; heapq.heappush(h, x)`; README:101). Dotted `X.sub` is **not** available until G3; until then a submodule API is either flattened (e.g. `os.path.join` → `os.join`, as `os.pyrs` does today) or deferred, and **must never silently truncate** (that's a W0/W3 fix).
2. **Names & positional shape.** Same public function/class names, same **positional** argument order, same defaults where the type system allows. Keyword args at the call site are supported (p02), so `sorted(xs, reverse=True)`-style calls are in-policy.
3. **Semantics.** Behavior is **python3-verified** by a dual-run parity golden (§G) for pure modules, or a python3-reference golden for `@extern` modules.
4. **Honest, documented divergence.** Where CPython's dynamism is unrepresentable, the module ships the faithful subset and the divergence is stated in the module header (the 15 shipped modules already do this well). The recurring divergence classes:
   - **`*args`/`**kwargs`-only APIs** → provide a `list`-argument form (`max(xs)` not `max(*args)`; `str.join(list)` already this shape). (G4)
   - **method-not-operator / class-not-module** where a protocol is missing → `json` exposes `v.get("k")`/`v.at(i)` not `v["k"]`/`v[i]` (no dual `__getitem__` overload); `random` is a `Random` *class* not module functions (no globals). Documented, not silent.
   - **dynamic returns** → `json.loads` returns a tagged `JsonValue`, not `dict[str, Any]` (no `Any`).
   - **numeric width** → `int` is i64, `**` yields `float`; `math.factorial(30)` and `Fraction` overflow honestly (raise), they do not silently wrap. (G9 documented divergence.)

**B.2 Per-module fidelity score** (a column in the §C/§D inventories):

| Score | Meaning |
|---|---|
| **5 — drop-in** | import + names + positional args + semantics identical; dual-run golden green. |
| **4 — near drop-in** | names/semantics identical; minor signature gaps (a missing keyword/optional param, an `*args` form degraded to a `list`). |
| **3 — shape divergence** | same names, but a structural change users must learn (class-not-module, method-not-operator). Documented in the header. |
| **2 — partial subset** | a useful slice of the API; several members deferred. |
| **1 — stub / deferred** | present enough to import, most behavior deferred. |
| **0 — out of scope** | not provided; §C rationale applies. |

The score is a *gate input*, not decoration: a module card is "done" when it hits its **declared target score** with a green parity golden — this is what stops a card from silently shipping a score-2 subset while claiming parity.

---

## C. CPython 3.12 inventory + classification (the core)

212 public top-level modules, grouped aggressively (dozens of near-identical entries collapse to one row; the 89 private `_`-modules are one row). Each row: representative members, the class **A/B/C**, the gating feature for B, and the rationale/target score.

### C.1 (A) FEASIBLE NOW — ~54 modules · pure pyrst, `@extern` over Rust std, `@crate`, or lazy generators

| Cluster | Representative modules | Backing | Target | Notes |
|---|---|---|---|---|
| **Already shipped (15)** | `math os time operator functools statistics string bisect heapq collections itertools textwrap re json random` | mixed | 3→5 | §D audits each; W1 raises them to target. |
| **Pure algorithms / data** | `datetime calendar graphlib difflib fractions colorsys pprint reprlib copy` | pure | 4–5 | `datetime` = i64-epoch + pure calendar math (comparisons via dunders); `copy` is near-trivial under value semantics; `fractions` uses i64 numer/denom (overflow honesty). |
| **Text / parsing (pure over `str`)** | `csv configparser html shlex fnmatch mimetypes plistlib(text) tomllib netrc` | pure | 4–5 | `fnmatch` builds on `re`; `html.escape`/`unescape`. |
| **Path / FS (`@extern` std)** | `pathlib(PurePath) shutil tempfile filecmp fileinput linecache glob(via os.walk+fnmatch)` | `@extern std::fs/path` | 3–4 | Concrete `pathlib.Path` needs fs `@extern`; `PurePath` is pure. |
| **System info (`@extern` std)** | `platform getpass io(StringIO) stat errno` | `@extern` / pure | 3–4 | `sys` partial: `maxsize/version/platform/exit` feasible now; `argv` is G2. |
| **Annotations-ish / no-op** | `typing numbers` | pure/no-op | 2–3 | `typing` = TypeVar/Generic as no-ops (annotations already work); `numbers` ABCs mostly out. |

*(Class boundaries: `enum`, `dataclasses`, `contextlib`, `abc`, `unicodedata` were considered for A but each needs a specific compiler feature — see B.)*

### C.2 (B) GATED on a named compiler feature — ~33 modules

| Gating feature | Modules it unlocks | Effort | Verdict |
|---|---|---|---|
| **G1 opaque `@extern` handle type** | `sqlite3 ssl` + crate-backed `re`(Pattern/Match) `datetime`(chrono) `hashlib hmac` `zlib gzip bz2 lzma tarfile zipfile` `subprocess`(Popen) `struct`(partial) | **L** | **BUILD (last)**; design-around (pure) where tractable (`datetime`, `hashlib`, `hmac`). |
| **G7 `bytes` type** | `base64 binascii struct array codecs` + `hashlib` output + `secrets uuid randbytes` | **L** | **BUILD**; gates the whole binary-data family. |
| **G2 module-level mutable state** | `sys`(argv) `logging` `warnings` `locale` `gettext` + `random` module API + `os.environ` mutation | **M** | **BUILD (own card)**; unlocks *convenience surface*, not whole modules. |
| **G3 dotted submodules** | `os.path` `urllib.parse` `xml.etree` `collections.abc` (package-shaped APIs) | **M** | **BUILD** (also fixes the silent-truncation bug). |
| **G4 `*args`/`**kwargs`** | `argparse getopt optparse` + n-ary `itertools.product`/`max`/`min` + `namedtuple` | **L** | **DESIGN-AROUND** (list forms) then build; many APIs degrade gracefully. |
| **G5 `%` / `.format()`** | (cross-cutting string idiom) | **S–M** | **BUILD-small or honest-reject** (W0 closes the p05 hole). |
| **G6 int/float overload** | int-returning `math.floor/ceil`, `round`, generic `operator`/`statistics` | **M** | **DESIGN-AROUND via generics** + a few overloaded builtins. |
| **G8 enum / dataclass / protocol codegen** | `enum dataclasses contextlib abc` | **M** | **BUILD-small** per feature (`dataclasses` is currently a *silent no-op* skip-list entry — resolver.rs:144 — worse than absent). |
| **G9 arbitrary-precision int** | exact `decimal`, large `math.factorial`, `fractions` exactness | **L** | **DESIGN-AROUND / OUT** — i64 is the contract; document divergence. |
| **G-data Unicode/tz tables** | `unicodedata zoneinfo` | **M** | crate or subset; sequence with datetime. |
| **G10 set algebra ops** | faithful `set` `&\|-^` (p20) | **S** | **BUILD-small.** |

### C.3 (C) OUT OF SCOPE BY DESIGN — ~125 public + 89 private

| Cluster (count) | Modules (representative) | Rationale |
|---|---|---|
| **Concurrency / async (8)** | `asyncio threading multiprocessing concurrent queue sched selectors contextvars` | pyrst is single-threaded and `Rc`-based (no `Send`); no async runtime; SPEC excludes shared-mutable aliasing. The lazy-generator lowering already relies on a single-threaded `Waker::noop` driver. |
| **Runtime introspection / dynamic (27)** | `ast inspect gc importlib pkgutil runpy dis opcode marshal pickle shelve dbm weakref types tokenize keyword copyreg …` | AOT-compiled, no runtime object model, no `eval`/`exec`/bytecode/metaclass; pickle/marshal serialize arbitrary live objects. |
| **C-FFI / low-level OS (25)** | `ctypes mmap fcntl termios tty pty resource grp pwd crypt syslog select signal posix winreg curses readline …` | unsafe FFI / platform C bindings; no `ctypes`, and `@extern` cannot hold the opaque handles these return. |
| **GUI / interactive / dev-tooling (~30)** | `tkinter turtle idlelib pydoc doctest unittest pdb profile cProfile trace timeit site venv ensurepip encodings this …` | GUI/interactive/packaging/tooling outside a compiled language's remit; pyrst has its own test harness + LSP. |
| **Legacy / "dead batteries" (16)** | `aifc audioop cgi cgitb chunk imghdr mailcap nntplib pipes sndhdr sunau telnetlib uu xdrlib …` | PEP 594 — removed from CPython 3.13; no value replicating what upstream deleted. |
| **Networking stack (13)** | `socket ssl http ftplib poplib imaplib smtplib xmlrpc wsgiref webbrowser urllib(net) xml email` | needs a socket layer + opaque socket/TLS handles; a large epic that is mostly C. (Pure pieces — `urllib.parse`, `ipaddress`, `netrc` — are pulled into A/B.) |
| **Private C-internals (89)** | `_ast _io _socket _pickle _decimal _datetime …` | implementation details behind public modules; never imported directly. One row. |

**Rationale principle:** a module is **C** when replicating it would require abandoning a pyrst design axiom (single-threaded/no-`Send`, no runtime object model, no unsafe FFI) or when CPython itself has deprecated it. Everything else is A or B.

---

## D. Existing-module fidelity audit — the 15 shipped modules

Where each of the 15 deviates from its CPython API **today**, and the W1 upgrade that closes the gap. "Feasible-with" names the *existing* feature that makes the upgrade possible now (the whole point: most upgrades need no new compiler work).

| Module | Today (score) | Key CPython deltas | W1 upgrade → target | Feasible-with |
|---|---|---|---|---|
| **itertools** | EAGER, 5 fns (2) | no `count`/`cycle` (infinite); `repeat` bounded-only; missing `islice takewhile dropwhile starmap tee groupby zip_longest accumulate(func) chain(*)` | LAZY `count`/`cycle`/`repeat(x)`/`islice`/`takewhile`/`dropwhile`/`starmap`/`accumulate(func)` → **4** | **lazy generators** (p15 ✓); n-ary `chain`/`product` wait on G4 |
| **functools** | `reduce` 3-arg only (1) | no `partial`, no 2-arg `reduce`, no `cache`/`lru_cache`/`cmp_to_key`/`wraps` | `partial` (closure), 2-arg `reduce` (Optional), `cmp_to_key`, dict-backed `cache` → **4** | **closures** (p16 ✓), default args, Optional |
| **bisect** | no `lo`/`hi` (3) | missing `lo`/`hi`/`key` bounding args | add `lo=0`, `hi=len`, `key=None` → **5** | **default args** (p01 ✓) |
| **heapq** | 3 fns (3) | missing `nlargest nsmallest heappushpop heapreplace merge` | add all five → **4** | generics (have), lazy `merge` |
| **collections** | `Counter`(fn) `deque` `defaultdict` (3) | `Counter` is a fn not a dict-subclass; no `OrderedDict ChainMap namedtuple`; `deque` missing `rotate/maxlen/extend/count/remove` | `Counter` arithmetic + full `most_common`, `OrderedDict`, `ChainMap`, `deque` rounding-out → **3–4** | classes (have); `namedtuple` needs G4/G8 |
| **operator** | int-only, 11 fns (2) | int-specialized; missing `itemgetter attrgetter truth not_ contains concat` | generic `add/sub/…`, `itemgetter`, `truth` → **4** | **generics**; float/str via type params |
| **statistics** | population-only, 4 fns (2) | `variance`/`stdev` are population (CPython = sample); no `fmean median_low/high mode multimode quantiles pstdev pvariance geometric_mean` | add `ddof`-style sample forms + the rest → **4** | default args, generics |
| **string** | 7 constants + `capwords` (3) | no `whitespace` const (needs `\x0b\x0c`); `capwords` no `sep`; no `Template` | `capwords(sep=None)`, `Template` class; **`whitespace`** waits on `\x` escapes | default args; `whitespace` gated on lexer `\x` (p14) |
| **math** | 14 `@extern` + 3 consts (3) | `floor/ceil/trunc` return **float** (CPython int); no `inf nan fabs atan2 degrees radians isnan isinf isfinite gcd lcm factorial comb perm isqrt copysign fmod modf hypot(have) dist prod remainder` | add the ~20 missing; `inf`/`nan` via `float("inf")` (p08 ✓); int-returning `floor/ceil` → G6 | `@extern` std; `float("inf")` ✓; int-return needs **G6** |
| **os** | 12 `@extern` (3) | flat only (no `os.path`); no `environ walk stat getpid sep urandom rename rmdir makedirs` | add `walk`, `stat`, `getpid`, `rename`, `makedirs`, `sep`/`linesep` consts, `urandom`(@extern getrandom) → **3**; faithful `os.path` → **G3** | `@extern` std; `os.path` needs **G3**; `environ` mutation needs **G2** |
| **time** | 3 `@extern` (2) | no `strftime strptime gmtime localtime struct_time ctime monotonic process_time time_ns` | add pure `strftime`/`gmtime` + `@extern` `monotonic`/`time_ns` → **3–4** | pure calendar math + `@extern` |
| **random** | `Random` class + 2 free fns (3) | class-not-module (no globals); no `gauss normalvariate sample choices getrandbits triangular betavariate` | add the distributions as methods → **4**; module-level `random()`/`randint()` → **G2** | class methods (have); module API needs **G2**; `randbytes` needs **G7** |
| **re** | 4 `@extern` bool/str/list (2) | no `Match`/`Pattern` object (recompiles each call); no groups/`sub` count/`escape`/`fullmatch`/`finditer` | add `escape`, `fullmatch`, `sub(count=)`, `split(maxsplit=)`; **`Match`/`Pattern` need G1** | `@crate regex` (have); objects gated on **G1** |
| **json** | pure `JsonValue` loads/dumps (3) | `v.get`/`v.at` not `v[...]`; no `indent=`/`sort_keys=`/`separators=`; no surrogate-pair decode; no file `load`/`dump` | add `indent=`/`sort_keys=`, surrogate pairs, `load`/`dump` → **4** (the `v[...]` divergence is permanent without dual `__getitem__`) | default args; `\u` astral needs lexer or manual (json already hand-decodes) |
| **textwrap** | 5 fns, positional (3) | no keyword options (`break_long_words expand_tabs initial_indent drop_whitespace placeholder predicate`); `shorten` placeholder fixed | add the keyword options as **default args** → **4** | **default args** (p01 ✓) |

**Headline:** 11 of the 15 upgrades need **only features that already exist** (default args, closures, lazy generators, generics). Just **4** deltas are genuinely gated — `math` int-returning `floor/ceil` (G6), faithful `os.path` (G3), `re.Match`/`Pattern` (G1), `random` module API (G2), and `string.whitespace` (lexer `\x`). W1 is therefore mostly high-ROI, low-risk work.

---

## E. Gating-feature dependency graph

De-duplicated compiler features, the module clusters each unlocks, effort class, and a **build-vs-design-around** verdict. This is the spine of the wave ordering.

```
                         ┌─────────────────────────────────────────────┐
  FEASIBLE NOW (no gate) │ W1 fidelity-upgrades (15) + W2 pure modules  │  ~71 modules, 0 compiler risk
                         └─────────────────────────────────────────────┘
  G5 %/.format (S)  ──▶ close honesty hole p05                 [W0]  BUILD-small / honest-reject
  G10 set ops (S)   ──▶ faithful set algebra p20               [W1]  BUILD-small
  G6 int/float (M)  ──▶ math.floor→int, round, generic ops     [W1]  DESIGN-AROUND (generics) + few overloads
  G8 enum/dataclass (M) ▶ enum dataclasses contextlib          [W2]  BUILD-small per feature (dataclasses is a SILENT no-op today)
  G3 dotted subs (M) ─▶ os.path urllib.parse xml.etree         [W3]  BUILD (also fixes silent-truncation bug)
  G2 mod globals (M) ─▶ sys.argv random-API logging warnings   [W4]  BUILD (own card) — convenience surface, not whole modules
  G7 bytes (L)      ──▶ base64 binascii struct hashlib-out     [W5]  BUILD — gates binary family
  G1 handles (L)    ──▶ re.Match datetime-chrono sqlite ssl    [W5]  BUILD last — OR pure reimpl (datetime, hashlib)
  G4 *args/**kw (L) ─▶ argparse namedtuple n-ary itertools     [W5+] DESIGN-AROUND (list forms) then build
  G9 bignum (L)     ──▶ exact decimal/fractions, big factorial [—]  DESIGN-AROUND / OUT — i64 contract, honest overflow
```

**Verdicts, with the honest math:**

- **G2 module-level mutable state — BUILD, but *not* a prerequisite for ~80% of the stdlib.** This corrects the outline's instinct. It unlocks a *convenience surface* — `sys.argv`, the module-level `random.random()`/`randint()`, `os.environ` mutation, a `logging` root logger — not a swath of otherwise-impossible modules. Crucially, **`itertools.count`/`cycle` are generators now (p15), not global-backed free functions**, so G2 does *not* gate them (the outline's open question, answered: no). Effort M. Sequence it as its own card (W4), after the two big feasible waves have already delivered value.
- **G1 opaque handles — BUILD last, design-around first.** Highest crate-reuse leverage (it's the only path to `sqlite3`/`ssl`/compression and to a *stateful* `re.Match`). But for `datetime` and `hashlib`/`hmac`, a **pure-pyrst reimplementation** (i64-epoch calendar math; a pure SHA-256) is tractable *now* and avoids the L-effort infra entirely — so those go in W2/W1, and G1 is reserved for the modules with no pure path.
- **G3 dotted submodules — BUILD.** Medium effort, and it doubles as a **bug fix** (the silent `os.path` → `os` truncation is a latent miscompile that already violates honest-errors). Do it before it bites a real program.
- **G4 variadics & G9 bignum — DESIGN-AROUND.** Both are L-effort and both degrade gracefully: `*args` → `list` forms (already the shape of `max(xs)`/`str.join`), bignum → honest i64 overflow with a documented divergence. Build G4 eventually for `argparse`/`namedtuple`; keep G9 out.
- **G5/G6/G8/G10 — small, targeted.** Fold into the waves where the modules that need them live; G5 additionally closes a W0 honesty hole.

---

## F. Wave plan

Wave = a parallel-friendly batch with a shared review. **Per-wave gate (all waves):** every module ships a python3-diffed parity golden (§G), the full `test_all.sh` suite stays green, emitted Rust is 0-warning + rustfmt-deterministic, and `PYTHON_COMPATIBILITY.md` + the module header document the fidelity score and any divergence. Card counts are estimates.

### W0 — Close the honesty holes + land the parity harness (compiler; do first)

Small but foundational: a stdlib must not rest on features that pass `check` and fail `rustc`.

| Card | Work | Agent | Size |
|---|---|---|---|
| W0-a | `%`-format (p05): implement a printf-style lowering **or** honest `check` error suggesting f-strings | complex-implementer | S/M |
| W0-b | Module-const reassignment (p09) + `sorted()`-over-user-`__lt__` (p12b): honest `check` errors (point at `global`-not-supported / `sorted(key=…)`) | complex-implementer | S/M |
| W0-c | Set algebra `&\|-^` (p20, G10): implement or honest-reject | implementer | S |
| W0-d | Parity harness in `test_all.sh`: dual-run a `.pyrs` under `pyrst` and under `python3` with the `from __future__ import annotations` prepend; diff (§G) | test-engineer | M |

*(~4 cards.)*

### W1 — Fidelity-upgrade the 15 shipped modules (mostly zero new compiler work)

One card per module (§D), each raising the module to its target score with a parity golden covering the new surface. Parallel-friendly (15 independent files under `lib/`). The 4 genuinely-gated deltas (`math` int-return → G6, `os.path` → G3, `re.Match` → G1, `random` module-API → G2) are explicitly deferred to their waves and noted in the header, not half-built here.

*(~15 cards. Gate additions: `itertools` ships a lazy-`count` parity golden; each upgraded signature has a dual-run golden.)*

### W2 — New pure-pyrst modules feasible today

High-usage additions needing no gating feature. Group into review-batches of ~4:

- **Batch 2a (time/date):** `datetime` (i64-epoch + pure calendar, dunder comparisons — note: no tz until zoneinfo/G-data), `calendar`, richer `time` (`strftime`/`gmtime`).
- **Batch 2b (text/parse):** `csv`, `configparser`, `html`, `shlex`, `fnmatch`, `difflib`.
- **Batch 2c (numeric/util):** `fractions` (i64, honest overflow), `colorsys`, `graphlib`, `copy`, `pprint`, `reprlib`.
- **Batch 2d (path/fs/sys):** `pathlib.PurePath`, `shutil`/`tempfile`/`filecmp` (`@extern` std), `platform`/`getpass`, `io.StringIO`, `stat`/`errno` constants, partial `sys` (`maxsize`/`version`/`exit`).
- **Batch 2e (class-feature, small G8):** `enum`, `dataclasses` (replace the *silent no-op* skip-list entry with real synthesis), `contextlib` (needs the general `__enter__`/`__exit__` protocol — currently a documented honest error; scope its own sub-card).

*(~17 cards.)*

### W3 — Dotted-submodule compiler epic (G3)

| Card | Work | Size |
|---|---|---|
| W3-a | Resolver package model: `import a.b` resolves `a/b.pyrs`; stdlib embed of packages; **remove the silent `path[0]` truncation** (honest error if unresolved) | complex-implementer L |
| W3-b | Faithful `os.path` (re-home the flattened `os.join`/`dirname`/… under `os.path.*`) | implementer M |
| W3-c | `urllib.parse` (pure) | implementer M |

*(~3–4 cards.)*

### W4 — Module-level-mutable-state compiler epic (G2)

| Card | Work | Size |
|---|---|---|
| W4-a | Top-level mutable statics (a `thread_local`/`OnceCell` lowering) + `global`/`nonlocal` keyword + reassignment tracking; closes the p09 hole for real | complex-implementer M/L |
| W4-b | `sys.argv` (+ `sys.stdin`/`stdout` if in scope) | implementer M |
| W4-c | `random` module-level API over a hidden global generator | implementer S |
| W4-d | `logging` (root logger, print-backed), `warnings` filters | implementer M |

*(~4–5 cards.)*

### W5 — `bytes` + opaque-handle epics (highest risk, last)

| Card | Work | Size |
|---|---|---|
| W5-a | `bytes`/`bytearray` type (G7): a new `Ty` + literals + `str`↔`bytes` codecs | complex-implementer L |
| W5-b | Opaque `@extern` handle `Ty` (G1): a foreign-struct value that stores/passes across `@extern` calls, with a lifetime/`'static` story | complex-implementer L |
| W5-c | `base64`/`binascii`/`struct` (on bytes) | implementer M |
| W5-d | `hashlib`/`hmac` — **pure-pyrst** SHA-256 first (no G1); crate-backed later | implementer/complex M |
| W5-e | Crate-backed `re.Pattern`/`re.Match`, `datetime`-via-chrono (if the pure version needs perf), compression (`zlib`/`gzip`/`bz2`/`lzma`), `sqlite3` | complex-implementer L×N |

*(~12 cards. Each carries a `verification-engineer` run against a real program.)*

**Explicitly OUT (no wave):** async/threading/multiprocessing, networking/sockets/TLS, introspection/pickle/importlib, GUI, dead-batteries (§C). Documented as out-of-scope in `PYTHON_COMPATIBILITY.md`.

**Total ≈ 4 + 15 + 17 + 4 + 5 + 12 = ~57 cards.** The first ~36 (W0–W2) deliver the bulk of everyday scripting value with **zero** deep-compiler risk; the last ~21 (W3–W5) are the funded infra epics.

---

## G. Parity-testing policy — dual-run, and its proven verdict

**Verdict: dual-run parity works and is recommended as the primary stdlib oracle for pure-pyrst modules.** CPython *is* the golden — the same source file's `python3` output is the expected output, eliminating hand-written expected blocks and pinning true semantic parity.

**The proven harness** (`scratchpad/probes2/`):

1. **pyrst side:** `pyrst build test.pyrs && ./test > pyrst.out`.
2. **python side:** run the *same file* — no `if __name__` guard needed — via
   `python3 -c "src='from __future__ import annotations\n'+open('test.pyrs').read(); g={}; exec(compile(src,'test','exec'),g); g['main']()" > py.out`.
3. `diff pyrst.out py.out`.

**Prototype results:**

- `parity_reduce_bisect.pyrs` (`from functools import reduce`, `from bisect import bisect_left`, a named `mul`, a loop) → **`120/3/2/15` byte-identical on both interpreters.**
- `parity2_insort.pyrs` (a generic `insort[T](a: Mut[list[T]], x: T)`) → **naive Python fails** (`NameError: Mut` — Python evaluates the annotation), **but with the `from __future__ import annotations` prepend it is identical** (`[0, 1, 3, 4, 5, 7, 8]`). This is the load-bearing trick.

**Which syntax differences break dual-running (and the fix):**

| Difference | Naive dual-run | Fix |
|---|---|---|
| `Mut[T]`, `Iterator[T]` annotations | Python `NameError` (annotation evaluated) | **`from __future__ import annotations`** (PEP 563) stringizes *all* annotations → never evaluated. Covers `Mut`, `Iterator`, `Callable`, string forward-refs. |
| `def f[T](...)`, `class C[T]` (PEP 695) | — | valid in Python **3.12+** (harness requires 3.12). |
| `x: int \| None` | — | valid Python **3.10+**. |
| `@extern` / `@crate` decorators | Python `NameError: extern` | out of dual-run scope: `@extern`/`@crate` modules (`os time math re`) use a **python3-reference golden** (compare to Python's *real* `os`/`math`), not same-source dual-run — or a tiny shim defining no-op `extern`/`crate` decorators. |
| API divergence (`json.v.get` vs `v[...]`; `random.Random` class) | different output | not a syntax issue — these are documented score-3 divergences; their golden is a pyrst-only golden, not dual-run. |

**Policy:** pure-pyrst modules (bisect, heapq, functools, operator, statistics, string, textwrap, collections, itertools, datetime, csv, difflib, fractions, …) get a **dual-run parity golden** (the harness above). `@extern`/`@crate` modules get a **python3-reference golden**. Divergent-by-design APIs get a **pyrst golden** with the divergence asserted. W0-d lands the harness in `test_all.sh`.

---

## H. Open questions & recommendations

1. **G1 build vs. pure reimplementation, per module.** For `datetime`/`hashlib`/`hmac` a pure-pyrst path exists and avoids the L-effort opaque-handle infra; for `sqlite3`/`ssl`/compression there is *no* pure path. **Recommend:** pure-first (W1/W2) for the tractable ones; reserve G1 (W5) strictly for the modules that cannot be pure. Revisit if pure `datetime` proves too slow — then crate-back it behind the same API.
2. **The `float("inf")`/`nan` gap in `math` (p08).** `float("inf")` works at runtime, so `math.inf`/`math.nan` can be provided as **`@extern` niladic functions or module bindings**, sidestepping the "module const must be a literal" rule (there is no inf/nan *literal*). **Recommend:** `math.inf`/`nan` via `@extern` (`f64::INFINITY`/`NAN`), not a const — matches the p08 finding.
3. **`str.format` vs `%` vs f-strings (G5).** f-strings already cover most needs. **Recommend:** W0 makes `%`/`.format` **honest `check` errors that suggest f-strings**, and only implement `%`-lowering if a stdlib module genuinely needs it internally (few do). Cheapest honest path.
4. **`dataclasses` is a *silent no-op* today** (skip-list, resolver.rs:144) — `import dataclasses` succeeds and does nothing. This is worse than absent (it violates honest-errors). **Recommend:** W2 either implements real `@dataclass` synthesis (`__init__`/`__eq__`/`__repr__`) or makes the import an honest error — but do not leave it silent.
5. **Fidelity-score enforcement.** The score is only meaningful if gated. **Recommend:** a module card's Definition-of-Done is "hits its declared target score with a green parity golden"; a reviewer rejects a card that ships a lower score while claiming a higher one. This is the mechanism that stops the epic's biggest failure mode (below).
6. **`sorted()` over user comparable objects (p12b).** Needed for sorting `datetime`/`Fraction` lists. **Recommend:** W0 emits an honest error steering to `sorted(key=…)`; a real total-order lowering (deriving `Ord` from `__lt__`) is a separate, larger card — scope it if W2 `datetime` demands `sorted(dates)`.

**The single biggest risk.** "Syntactically identical" collides head-on with pyrst's static, closed-type, single-threaded nature: the modules users *most expect* (`datetime`, `argparse`, `logging`, `subprocess`, `sqlite3`, `dataclasses`) are precisely the ones gated on the deepest features (opaque handles, globals, variadics, protocols). The failure mode is **scope-creep by stealth** — a module card silently dragging in a compiler epic, or (worse) shipping a subtly divergent API (class-not-module, method-not-operator, a check-passes/rustc-fails hole) that quietly breaks the "interchangeable" promise the whole epic exists to keep. **Mitigation:** (a) the three architectural gaps are funded as explicit standalone compiler cards (W3/W4/W5), never hidden inside a module card; (b) every module is gated by a per-module fidelity score **and** a dual-run parity golden, so divergence is measured, not discovered; (c) the honesty holes are closed in **W0**, up front, so no stdlib code is ever written on top of a feature that passes `check` and fails `rustc`. Ship the ~54 feasible-now modules first — they are real, immediate, low-risk value — and treat the gating features as the funded epics they are.

## Relevant files

**This design:** `docs/design/stdlib-full.md` (this file). **Supersedes** `docs/design/stdlib-outline.md`. **Style/precedent:** `docs/design/lazy-generators.md` (BLUF + staged plan + validated prototype), `docs/design/exception-lowering.md`, `docs/design/value-semantics.md`.

**Shipped stdlib (the 15, §D):** `lib/{math,os,time,operator,functools,statistics,string,bisect,heapq,collections,itertools,textwrap,re,json,random}.pyrs`. **Embedding:** `src/stdlib.rs` (`include_str!` of each; 15 modules). **Resolution:** `src/resolver.rs` (`path[0]`-only dotted truncation at ~:125; skip-list `dataclasses`/`sys` at ~:144).

**Compiler surfaces the waves touch:** `src/typeck/types.rs` (`enum Ty` — the closed type set G1/G7 extend), `src/typeck/checks.rs` (module-const literal rule ~:1082; W0/W4), `src/parser.rs` (param parsing ~:433–480 — G4 `*args`; ~:1539 generic-method reject — G8), `src/lexer.rs` (string escapes — `\x`/`\u` for `string.whitespace`/`json`), `src/codegen/exprs.rs` (`%`-format G5, `int()`/`float()` conversions, set ops G10), `src/codegen/analysis.rs` (top-level statement backstop ~:1207–1222 — G2), `src/codegen/mod.rs` (`__py_float_from_str` ~:669 — the `float("inf")` path).

**Empirical probes (scratchpad, not committed):** `scratchpad/probes/` (p01–p20 capability probes), `scratchpad/probes2/` (`p12b`/`p18b` corrected, `parity_reduce_bisect.pyrs`, `parity2_insort.pyrs` — the dual-run prototypes), `scratchpad/builddir/` (build outputs). CPython inventory: `python3 -c "import sys; sys.stdlib_module_names"` (3.12.9 → 212 public + 89 private).
