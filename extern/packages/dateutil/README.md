# pyrst-dateutil

A `python-dateutil` subset written in pyrst, over `lib/datetime` +
`extern/packages/tzdata` (the named-zone offset/DST substrate). All project
state lives on card `0223a4b8` (epic `47cafe10`, Track B).

Status: **implemented**. The parser format matrix, relativedelta arithmetic,
rrule expansion engine, and tz glue below are all shipped and oracle-pinned
against python3 3.12 dateutil 2.9.0 / zoneinfo — see `tests/oracle_core.pyrs`
(the full self-checking matrix) and each module's header for the exact probe
commands. `tests/smoke_scaffold.pyrs` remains as a minimal construction/import
smoke.

## Scope

| Module | Surface | Status |
|--------|---------|--------|
| `parser.pyrs` | `parse(s, dayfirst=, yearfirst=)` — ISO-8601 (permissive `datetime_fromisoformat` fast path) + numeric `M/D/Y`, `D.M.Y`, `D-M-Y`, `Y/M/D`, 2-digit years + textual month (`Jan 10, 2026`, `10 January 2026`, `2026 Jul 4`, …) + optional `HH:MM[:SS]`/`AM`/`PM`; `fuzzy=False` only; `ParserError` on unparseable input | **shipped** |
| `relativedelta.pyrs` | `relativedelta` construction (normalized `_fix()`), `rd.apply(dt)` / `rd + dt` (calendar-correct, end-of-month clamp, `weekday=` nth adjust), `rd.subtract_from(dt)`, `relativedelta_between(dt1, dt2)` | **shipped** |
| `rrule.pyrs` | `rrule(freq, dtstart, interval=, count=, until=, byweekday=, bymonthday=)` for `DAILY`/`WEEKLY`/`MONTHLY`/`YEARLY`; `iterate(rule)` (lazy generator) / `materialize(rule, limit)` (eager) | **shipped** |
| tz glue | `parse_with_offset(s)` (`Z`/`+HH:MM`/`-HH:MM`/`+HHMM` → `ParseResult`), `to_utc(pr)`, `astimezone_named(pr, zone)` via `tzdata.utc_offset_at` | **shipped** |

**Correctness bar (binding on every SHIPPED behavior, not the scaffold
stubs):** every behavior must be pinned against python3 3.12's
`dateutil` as the oracle, with the probe command/output in a comment near
the assertion. Example:

```sh
python3 -c "from dateutil import parser; print(parser.parse('2026-07-10'))"
```

## Ambiguity policy (parser)

- ISO-8601-shaped strings are unambiguous and ignore `dayfirst`/`yearfirst`
  (ISO always reads `YYYY-MM-DD`).
- For fully numeric non-ISO dates (`01/02/03`), the default is MONTH-first
  (US) — oracle-pinned: `01/02/03` → `2003-01-02`, `dayfirst=True` →
  `2003-02-01` (DMY), `yearfirst=True` → `2001-02-03` (YMD), both →
  `2001-03-02` (YDM). A 4-digit numeric field is always the year (`YYYY/..` →
  Y/M/D; `../YYYY` → year-last). When the would-be MONTH field exceeds 12 and
  the other candidate is ≤ 12, the two are auto-swapped (`31/12/2026` → Dec 31
  regardless of `dayfirst`), EXCEPT in year-first orderings where a `month > 12`
  is a genuine error (`2026-13-01` → `ParserError`, never silently corrected).
- **Two-digit-year window** = `now.year + 50` (`2000 + yy`, minus 100 if that
  lands ≥ the pivot). This mirrors dateutil's own current-year-relative window
  and is therefore **time-dependent exactly as dateutil is** — tests pin only
  stable cases (`03 → 2003`).
- **Time-only input** (`9:30 AM`) → `ParserError` here (DIVERGENCE): dateutil
  fills in today's date, which would be non-deterministic; this subset needs a
  date.
- `fuzzy=False` only — no fuzzy substring extraction from surrounding text.
- Unparseable input raises `ParserError` (a real, catchable pyrst exception
  class), never a silent best-guess.

## tz glue

`lib/datetime`'s `datetime` has **no `tzinfo` slot** (the TZINFO_GATE
divergence documented in `lib/datetime.pyrs`'s own header) — so a value
parsed from a tz-bearing string (`Z`/`+HH:MM`/`-HH:MM` suffix) cannot be
represented as a tz-AWARE `datetime` the way CPython's dateutil does.
`parser.pyrs`'s `ParseResult` (naive `datetime` + `Optional[int]` offset
minutes) is the answer to that gap. `parse()` (which returns a naive
`datetime`) REJECTS a tz-bearing suffix with `ParserError` and points at
`parse_with_offset()`, which returns a `ParseResult`. `to_utc(pr)` gives the
naive UTC instant (`UTC == local − offset`); `astimezone_named(pr, zone)`
converts to WALL-CLOCK time in a named IANA zone via `tzdata.utc_offset_at`
(a two-step UTC→local fixed point, since tzdata keys the offset on local wall
time — inside the ~1 h DST-transition window it returns one consistent side of
the fold, the same limitation tzdata itself documents). **Named-zone lookups
always go through `tzdata`** (never re-derive offset/DST rules here) — see
`extern/packages/tzdata/README.md`'s frozen API section (`available_zones`,
`zone_info`, `utc_offset_at`, `is_dst_at`).

## Weekday constants (relativedelta/rrule)

Real dateutil's `MO`/`TU`/.../`SU` are CALLABLE `weekday` instances
(`MO(+1)` = "the first Monday"). pyrst has no `__call__` dunder
(`PYTHON_COMPATIBILITY.md`'s Dunders section), so here `MO`..`SU` are plain
`int` constants (0=Monday..6=Sunday, matching `datetime.weekday()` and
`lib/calendar.pyrs`'s `MONDAY..SUNDAY`), and the "Nth weekday" case is
spelled `Weekday(MO, 1)` (a real class in `relativedelta.pyrs`) instead of
a call. `rrule` re-exports the constants from `relativedelta` for
CPython-import-shape fidelity (`from dateutil.rrule import MO, ...` also
works, matching real dateutil's own re-export).

## Build & run (PYRST_PATH + dotted-import convention)

**This is the single most important operational note for this package.**
A module resolved via `$PYRST_PATH` is re-rooted at the PYRST_PATH ENTRY
directory (see `extern/packages/tzdata/README.md`'s matching note, and
`src/resolver.rs`'s `PYRST_PATH hit` comment) — NOT at the package's own
directory. Concretely:

- **Sibling imports inside `dateutil/` must use the full dotted path**:
  `rrule.pyrs` imports `relativedelta.pyrs` as
  `from dateutil.relativedelta import MO, TU, ...` — a bare
  `from relativedelta import MO` does **not** resolve.
- **Consuming `tzdata` from inside (or downstream of) this package** goes
  through its own dotted path from the SAME `$PYRST_PATH` root:
  `from tzdata.api import available_zones, zone_info, utc_offset_at,
  is_dst_at`.

Build any program that imports this package with `$PYRST_PATH` pointed at
`extern/packages` (the directory CONTAINING both `dateutil/` and
`tzdata/`, not either package directory itself):

```sh
PYRST_PATH=/home/ethos/Coding/pyrst/extern/packages \
  /home/ethos/Coding/pyrst/target/release/pyrst build tests/smoke_scaffold.pyrs
./smoke_scaffold
```

(Run from `extern/packages/dateutil/` — `pyrst build` always emits its
binary into the CURRENT directory, named after the source basename; never
commit those binaries, see `.gitignore`.)

See `extern/programs/dateutil_demo/README.md` for the demo program (a
trivial parse-and-print, proving the cross-package `dateutil` + `tzdata`
wiring end to end — it does not yet exercise the `relativedelta`/`rrule`
arithmetic, since that's still `TODO`/`NotImplementedError` at this stage).

## Module layout

| File | Responsibility |
|------|-----------------|
| `parser.pyrs` | `ParserError`, `ParseResult`, `parse()` (ISO fast path + numeric/textual matrix + dayfirst/yearfirst), `parse_with_offset()`, `to_utc()`, `astimezone_named()`. |
| `relativedelta.pyrs` | `MO`..`SU` constants, `Weekday` (nth-occurrence value class), `relativedelta` (normalized `_fix()` constructor), `apply()`/`__add__` (apply to a datetime), `negate()`, `subtract_from()`, `relativedelta_between()`. |
| `rrule.pyrs` | `YEARLY`..`SECONDLY` freq constants (+ `_freq_in_scope`), `rrule` (freq/by-rule validation at construction), `iterate()` (lazy free-function generator — see header for the generator-method gap), `materialize(rule, limit)` (eager). |
| `tests/oracle_core.pyrs` | The full self-checking oracle matrix (parser formats + dayfirst/yearfirst + errors, tz glue, relativedelta normalization/apply/between, rrule DAILY/WEEKLY/MONTHLY/YEARLY + by-rules + scope guards). Prints `PASS: oracle_core` / exits 1 on any mismatch. |
| `tests/smoke_scaffold.pyrs` | Minimal construction/import smoke (every class constructs, cross-package `tzdata` import resolves). |

## rrule expansion model (shipped)

rrule walks **periods** (a day/week/month/year, advanced by `interval`) and,
within each, enumerates candidate instants filtered by the BY-rules, in order,
keeping those ≥ `dtstart` until `count`/`until`. It is a **filter**, NOT a
clamp: MONTHLY from Jan 31 yields Jan 31, **Mar 31, May 31, Jul 31** (skipping
months with no 31st); YEARLY from Feb 29 yields only leap years. `dtstart` is
emitted first only if it satisfies the BY-rules (WEEKLY `byweekday=[TU,TH]`
from a Monday first yields the Tuesday). BY-rule scope (oracle-pinned only):
`byweekday` for DAILY/WEEKLY, `bymonthday` for MONTHLY; any other
freq × by-rule combination raises `NotImplementedError` at construction rather
than diverge silently.

## Language gaps hit while scaffolding (logged on card `0223a4b8`)

- **No `__call__` dunder** — forces the `MO(+1)`-callable-weekday pattern
  into the `Weekday(MO, 1)` class-construction pattern documented above
  (confirmed absent from `PYTHON_COMPATIBILITY.md`'s Dunders coverage).
- **No constructor overloading** (a class has exactly one `__init__`
  signature) — dateutil's `relativedelta(dt1, dt2)` two-datetime
  constructor form cannot share the class's main `__init__`; scaffolded as
  the separate `relativedelta_between()` free function instead.
- **Generator methods (`yield` inside a class method) are not yet
  supported** (`PYTHON_COMPATIBILITY.md`'s Generators table) — dateutil's
  `rrule` is itself iterable; here the lazy expansion is the free function
  `iterate(rule) -> Iterator[datetime]` instead of an `rrule.__iter__`.
- **No reflected dunders / operand-order divergence (RESOLVED).** CPython
  dateutil supports both `dt + rd` and `dt - rd` (via the relativedelta's
  `__radd__`/`__rsub__`), but pyrst has no reflected dunders and
  `datetime.__add__` only accepts a `timedelta`, so `dt + rd` (datetime on the
  LEFT) does not compile. Shipped spelling: the relativedelta goes on the LEFT
  — `rd + dt` (routes to `relativedelta.__add__(self, dt)`) or the alias
  `rd.apply(dt)`; subtraction is `rd.subtract_from(dt)` (== `dt + (-rd)`, via
  `negate()`). Also: because `dt - dt → timedelta` works but `dt - timedelta`
  does NOT (`lib/datetime.pyrs`'s divergence #4), all internal
  datetime−timedelta shifts are written `dt + (-timedelta)` / a negative
  `timedelta`.
- **`datetime` has no `tzinfo` slot** (TZINFO_GATE, `lib/datetime.pyrs`'s
  own header) — drives the `ParseResult` (naive value + separate offset)
  design instead of a tz-aware `datetime` return from `parse()`.
