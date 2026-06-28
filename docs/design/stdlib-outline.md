# pyrst Standard Library — Outline & Roadmap

**Date:** 2026-06-28
**Status:** Planning outline (mirrors the Python standard library). No stdlib code yet.
**Foundation:** Rust interop Phase 1 (`@extern`) is DONE — verified that `@extern` binds Rust std functions returning `str`/`int`/`float`/`bool`/`list[T]`, multi-arg, with real side effects (env, fs).

## 1. Approach

The pyrst stdlib is **pyrst source modules** (`.pyrs` files), each function backed by one of three strategies:

1. **`@extern` over Rust std** (Phase 1, available now) — wrap a Rust std function in a `@extern` template. Covers file I/O, paths, env, process, time, encoding, most string ops. *The bulk of the stdlib.*
2. **Pure pyrst** — algorithms written in pyrst itself (statistics, bisect, heapq, parts of functools/itertools). Needs no FFI; some need **generics** (`b0c719d9`) for reuse across types, or **generators** (`cad4da39`) for lazy iterators.
3. **`@extern` over an external crate** (Phase 2, `4cbf44bd`) — `json`→serde_json, `re`→regex, `datetime`→chrono, `hashlib`→sha2. Needs the Cargo-project build (Phase 2).

This mirrors how CPython's stdlib is split between C-accelerated modules and pure-Python modules — pyrst's "C layer" is Rust std/crates via `@extern`.

## 2. Prerequisite: stdlib home + import resolution (FIRST implementation task)

Today the resolver (`resolver.rs`) **skips** `import math|sys|os|json|re|collections|itertools` as no-op "builtins" (only `math` has real hardcoded codegen — `sys`/`os`/etc. silently produce nothing), and a normal `import foo` resolves to `foo.pyrs` in the *same directory*. To ship a file-based stdlib we need:

- **A stdlib home** — a `lib/` (or `std/`) directory of `.pyrs` modules, shipped with the compiler (located relative to the `pyrst` binary, or a `PYRST_STDLIB` env override).
- **Resolver search path** — `import os` resolves to `<stdlib>/os.pyrs` when not found locally; local files still shadow. Remove each name from the skip-list as it becomes a real module.
- **Migrate `math`** from hardcoded codegen into a real `math.pyrs` (`@extern` wrappers) — dogfoods the path and removes special-casing. (Keep back-compat: `import math; math.sqrt(x)` must still work.)

This is a small, well-scoped first card — nothing else can be `import`ed until it exists.

## 3. Module catalog (mirroring Python)

Legend — backing: **STD** = `@extern` over Rust std (Phase 1) · **PY** = pure pyrst · **CRATE** = `@extern` over a crate (Phase 2). Gates: **[gen]** needs generics · **[yield]** needs generators · **[fc]** uses first-class functions (done).

### Tier 1 — high-value, buildable in Phase 1 (the starter set)
| Module | Key functions | Backing |
|---|---|---|
| **os** | `getcwd chdir listdir mkdir makedirs remove rmdir rename getenv setenv environ` | STD (`std::env`, `std::fs`) |
| **os.path** | `join exists isfile isdir basename dirname split splitext abspath` | STD (`std::path`) |
| **sys** | `argv exit platform stdin.read/readline stdout.write stderr.write` | STD (`std::env::args`, `std::process`, `std::io`) |
| **io / files** | read/write/readlines/append (pyrst already has `open()`; formalize) | STD (`std::fs`) |
| **math** | `sqrt floor ceil trunc pow exp log log2 log10 sin cos tan gcd factorial isqrt comb pi e tau inf nan` | STD (migrate the hardcoded set + extend) |
| **time** | `time monotonic perf_counter sleep` | STD (`std::time`, `std::thread::sleep`) |
| **string** | constants `ascii_lowercase ascii_uppercase digits hexdigits punctuation whitespace`; `capwords` | PY + str methods |
| **random** | `seed random randint randrange choice shuffle uniform` | PY (Rust std has NO RNG → a seeded LCG/xorshift in pyrst; or CRATE `rand` later) |
| **functools** | `reduce partial` [fc] | PY (now possible — first-class functions) |
| **operator** | `add sub mul lt eq …` as function values [fc] | PY (pairs with reduce/map) |
| **statistics** | `mean median mode stdev variance` | PY |

### Tier 2 — pure pyrst, gated on language features
| Module | Key functions | Backing / gate |
|---|---|---|
| **bisect** | `bisect_left bisect_right insort` | PY [gen] (int/float-specialized until generics) |
| **heapq** | `heappush heappop heapify nlargest nsmallest` | PY [gen] |
| **collections** | `Counter defaultdict deque OrderedDict namedtuple` | PY ([gen] for general; `Counter`=`dict[str,int]` feasible now) |
| **itertools** | `accumulate chain count cycle repeat combinations permutations product` | PY ([yield] for lazy; eager forms feasible now) |
| **textwrap** | `wrap fill shorten` | PY |

### Tier 3 — needs Phase 2 (external crates)
| Module | Crate | Notes |
|---|---|---|
| **json** | serde_json | **Highest Phase-2 value** — parse/dump. The boundary (dynamic JSON ↔ typed pyrst) needs design (typed structs vs a `JsonValue` type). |
| **re** | regex | `match search findall sub split compile`. |
| **datetime** | chrono (or std partial) | `now date time timedelta strftime`. |
| **hashlib** | sha2 / md5 | `md5 sha1 sha256`. |
| **csv** | csv | reader/writer. |
| **urllib.request / http** | reqwest / ureq | `urlopen get post`. |
| **argparse** | clap (or PY) | CLI parsing — could be pure pyrst. |
| **logging** | log / env_logger (or PY) | could be pure pyrst (print-based). |

## 4. Recommended build order

1. **Prerequisite** — stdlib home + resolver search path + migrate `math` (§2). Unblocks everything.
2. **Starter pack (Phase 1, this milestone)** — `os` + `os.path` + `sys` + `time` + `string` + `functools`/`operator` + `statistics`. These make pyrst genuinely useful for scripts (files, args, env, time, reduce/partial) and are all buildable today. Ship with goldens for each.
3. **`random`** (pure-pyrst LCG) — small, high-utility, no crate needed.
4. **Phase 2 build pipeline** (`4cbf44bd`) → then **`json`** + **`re`** — the ecosystem payoff.
5. **Tier 2 collections/itertools/heapq** as **generics** (`b0c719d9`) and **generators** (`cad4da39`) land.

## 5. Cross-roadmap dependencies

- **Generics (`b0c719d9`)** — general `collections`/`bisect`/`heapq` need type-parametric containers; until then, type-specialized (`dict[str,int]` Counter, `list[int]` heap).
- **Generators (`cad4da39`)** — lazy `itertools` (`count`/`cycle`/`chain`) need `yield`; eager forms ship first.
- **First-class functions (DONE)** — unlock `functools.reduce`/`partial`, `operator.*`, `map`/`filter` with named functions.
- **Interop Phase 2 (`4cbf44bd`)** — gates `json`/`re`/`datetime`/`hashlib`/`csv`/`http`.
- **Modules + distribution (`1e010d08`)** — the stdlib home overlaps the project/module system; coordinate.

## 6. Notes / open design questions

- **`str` is owned `String`** in pyrst; std functions taking `&str`/`&Path` need `&{p}` (or `&{p} as &str`) glue in the template — the binding author handles this (verified).
- **Errors**: Rust `Result` → `.unwrap()`/`.unwrap_or…` in the template today (a failed `read_file` panics as a pyrst exception). A future nicety: bindings that return `Optional[T]` map `Result→Option`, or raise typed pyrst exceptions.
- **`json`/dynamic data** is the one module whose *type design* (not just FFI) needs real thought — pyrst is statically typed, JSON is dynamic. Likely a `JsonValue` union/class + typed accessors, or serde-into-declared-structs. Defer the decision to the Phase-2 `json` card.
