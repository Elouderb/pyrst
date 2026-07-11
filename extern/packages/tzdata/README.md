# tzdata

A curated IANA timezone data package + lookup API, written in pyrst. The
substrate for the pending `pyrst-dateutil` sibling package (a separate card).
All project state lives on card `965bf13f`.

Status: **implemented**. The module layout, class shapes, and API signatures
below are implemented and compile, and the curated zone table ships **74
zones** — 48 named major zones (all major US/Canada, EU, APAC, selected Latin
America + `UTC`) plus the complete `Etc/GMT+12 .. Etc/GMT-12` fixed-offset
family. Every offset and DST rule is oracle-verified against python3.12
`zoneinfo`: the rule engine was mirrored in Python and checked against
`zoneinfo` over all 74 zones × 2023–2030 × 4 local times/day (**864,896**
offset comparisons + **135,198** `is_dst` comparisons) with **zero
mismatches**, skipping only the exact ambiguous/nonexistent transition hour
(see the documented DST-boundary limitation below).

## Curation policy

- **Modern rules only.** Transition rules describe the zone's CURRENT
  recurring DST behavior — a "Nth/last weekday-of-month at HH:MM" recurrence —
  not a historical transition-by-transition ledger. Zones whose modern-era
  behavior is rule-regular get a `TransitionRule` pair; zones that abolished
  DST in the modern era ship as their **post-abolition constant offset**
  (honest for present/near-future queries, NOT for pre-abolition history):
  `America/Sao_Paulo` (Brazil ended DST 2019), `America/Mexico_City` (Mexico
  ended nationwide DST 2022), `Europe/Moscow`/`Europe/Istanbul` (permanent
  standard time), `Asia/Tehran` (Iran ended DST 2022).
- **Excluded zone (documented, not silently wrong).** `Asia/Jerusalem` is
  **excluded**: Israel's spring rule is "the Friday BEFORE the last Sunday of
  March", which the nth/last-weekday model cannot express (last-Friday is not
  in general the Friday-before-last-Sunday). Per policy an irregular zone is
  excluded rather than shipped with a rule that diverges from IANA in some
  years. It can be re-added later via an explicit transition table.
- **One non-obvious rule shape.** `America/Santiago` (Chile) transitions on
  the **first Saturday at 24:00** (== Sunday 00:00), NOT the first Sunday —
  encoded as `weekday=Saturday, week=1, local_time_min=1440`. A naive
  first-Sunday model diverges in years such as 2024 and 2029 (caught by the
  oracle validation harness).
- **~40-60 major zones at full scope**: all US/EU/major-APAC zones, `UTC`,
  and fixed-offset `Etc/GMT±N` zones. See "Coverage" below for what is
  actually shipped today vs. planned.
- **Oracle-pinned.** Every offset and transition rule is verified against
  python3 3.12's `zoneinfo` (`from zoneinfo import ZoneInfo`) — see the
  per-zone comments in `data.pyrs` for the exact oracle command/output that
  justifies each row. No offset or rule ships from memory/guesswork.
- **Correctness bar**: `utc_offset_at` must match `zoneinfo`'s `.utcoffset()`
  (in minutes) for a pinned test matrix per zone — several dates including
  ones on each side of a DST boundary. The one documented gap: the
  package's DST model does not disambiguate the exact ambiguous
  (fall-back) or nonexistent (spring-forward) local hour the way CPython's
  `fold` parameter does — see `api.pyrs`'s module header. Callers querying
  exactly that hour get one consistent side of the fold; every other local
  instant matches `zoneinfo` (validated over 864,896 comparisons).
- **Honest errors, never silent wrong answers.** An unrecognized zone name
  raises `KeyError` (see "API" below) — never a guessed offset.

## API (FROZEN — this is the interface contract `pyrst-dateutil` consumes)

```python
def available_zones() -> list[str]
```
Returns every curated zone's IANA name (e.g. `"America/New_York"`), in the
table's declaration order.

```python
def zone_info(name: str) -> ZoneRule
```
Returns the full `ZoneRule` struct for `name` (offsets + DST rule — see
"Types" below). Raises `KeyError(repr(name))` if `name` is not a curated
zone.

```python
def utc_offset_at(name: str, year: int, month: int, day: int, hour: int, minute: int) -> int
```
**The workhorse.** Returns the zone's UTC offset, in minutes (can be
negative), for the given **naive local wall-clock** date/time — i.e. the
same date/time you'd pass to
`datetime(year, month, day, hour, minute, tzinfo=ZoneInfo(name)).utcoffset()`
in CPython. Raises `KeyError(repr(name))` for an unrecognized zone.

```python
def is_dst_at(name: str, year: int, month: int, day: int, hour: int, minute: int) -> bool
```
Same arguments as `utc_offset_at`; returns whether DST is in effect at that
local instant (CPython's `.dst() != timedelta(0)`, expressed as a plain
`bool`). Raises `KeyError(repr(name))` for an unrecognized zone.

### Types

```python
class TransitionRule:
    month: int          # 1-12
    week: int           # 1..4 = Nth occurrence of `weekday` in `month`; -1 = LAST occurrence
    weekday: int         # 0=Monday .. 6=Sunday (matches Python's date.weekday())
    local_time_min: int  # minutes after local midnight (wall clock) when the transition occurs

class ZoneRule:
    name: str
    std_offset_min: int                    # standard (non-DST) UTC offset, minutes
    dst_offset_min: int                    # DST UTC offset, minutes (== std_offset_min if not has_dst)
    has_dst: bool
    dst_start: Optional[TransitionRule]     # std -> DST transition; None iff not has_dst
    dst_end: Optional[TransitionRule]       # DST -> std transition; None iff not has_dst
```

These signatures (function names, parameter order/types, return types, and
the two public class shapes) are **frozen** as of this card. Any change must
be coordinated with the `pyrst-dateutil` card before it starts consuming
this API.

## Coverage (74 zones)

DST rule families (all oracle-verified):

| Family | Members | Rule (local wall-clock) |
|--------|---------|--------------------------|
| **US/Canada** | `America/New_York` -05/-04, `America/Chicago` -06/-05, `America/Denver` -07/-06, `America/Los_Angeles` -08/-07, `America/Anchorage` -09/-08, `America/Toronto` -05/-04, `America/Vancouver` -08/-07, `America/Halifax` -04/-03, `America/St_Johns` -03:30/-02:30 | DST 2nd Sun Mar 02:00 → 1st Sun Nov 02:00 |
| **EU (WET)** | `Europe/London`, `Europe/Dublin`, `Europe/Lisbon` (all +00/+01) | last Sun Mar 01:00 → last Sun Oct 02:00 |
| **EU (CET)** | `Europe/Paris`, `Europe/Berlin`, `Europe/Madrid`, `Europe/Rome`, `Europe/Amsterdam`, `Europe/Brussels`, `Europe/Zurich` (all +01/+02) | last Sun Mar 02:00 → last Sun Oct 03:00 |
| **EU (EET)** | `Europe/Athens`, `Europe/Helsinki`, `Europe/Bucharest` (all +02/+03) | last Sun Mar 03:00 → last Sun Oct 04:00 |
| **Australia (E/C)** | `Australia/Sydney` +10/+11, `Australia/Melbourne` +10/+11, `Australia/Adelaide` +09:30/+10:30 | 1st Sun Oct 02:00 → 1st Sun Apr 03:00 (year-wrapping) |
| **New Zealand** | `Pacific/Auckland` +12/+13 | last Sun Sep 02:00 → 1st Sun Apr 03:00 (year-wrapping) |
| **Chile** | `America/Santiago` -04/-03 | 1st Sat Sep 24:00 → 1st Sat Apr 24:00 (year-wrapping) |

No-DST named zones (constant offset):

| Zone | offset | | Zone | offset |
|------|--------|-|------|--------|
| `UTC` | +00:00 | | `Asia/Tokyo` | +09:00 |
| `America/Phoenix` | -07:00 | | `Asia/Shanghai` | +08:00 |
| `Pacific/Honolulu` | -10:00 | | `Asia/Hong_Kong` | +08:00 |
| `America/Sao_Paulo` | -03:00 | | `Asia/Singapore` | +08:00 |
| `America/Mexico_City` | -06:00 | | `Asia/Seoul` | +09:00 |
| `America/Bogota` | -05:00 | | `Asia/Kolkata` | +05:30 |
| `Europe/Moscow` | +03:00 | | `Asia/Dubai` | +04:00 |
| `Europe/Istanbul` | +03:00 | | `Asia/Bangkok` | +07:00 |
| `Australia/Brisbane` | +10:00 | | `Asia/Jakarta` | +07:00 |
| `Australia/Perth` | +08:00 | | `Asia/Karachi` | +05:00 |
| | | | `Asia/Tehran` | +03:30 |

Fixed-offset family: `Etc/GMT` and `Etc/UTC` (both UTC±00:00), plus the
complete integer-hour span `Etc/GMT+1 .. Etc/GMT+12` and `Etc/GMT-1 ..
Etc/GMT-12`. **Note the reversed IANA sign convention**: `Etc/GMT+5` means
UTC**−5**, `Etc/GMT-9` means UTC**+9**.

**Excluded:** `Asia/Jerusalem` (irregular spring rule — see curation policy).

## Module layout

| File | Responsibility |
|------|-----------------|
| `data.pyrs` | `TransitionRule`/`ZoneRule` class shapes + `_zone_table() -> list[ZoneRule]`, the curated data (module-internal; not part of the frozen API). Every row is oracle-pinned in its own comment. |
| `api.pyrs` | The four frozen lookup functions above, plus private helpers (`_weekday`, `_nth_weekday_day`, `_is_dst_active`, …) implementing the DST recurrence-rule arithmetic. |
| `tests/smoke_api.pyrs` | Smoke test: wires up `available_zones`/`zone_info`/`utc_offset_at`/`is_dst_at` against a handful of oracle-pinned values, incl. half-hour offsets and the `KeyError` contract on all three lookups. |
| `tests/oracle_matrix.pyrs` | Comprehensive oracle matrix (212 probes / 424 assertions), all values extracted from python3.12 `zoneinfo`: each DST zone at mid-winter/mid-summer noon 2026 plus local noon on the day before/after every real 2026 DST transition, each no-DST zone, and a spanning sample of the `Etc/GMT` family. |

**Import convention (package-internal "sibling" imports).** Because this
package is resolved via `$PYRST_PATH` (see "Build" below), a module's OWN
imports resolve relative to the `$PYRST_PATH` entry that supplied it — NOT
relative to its own directory (see `src/resolver.rs`'s `PYRST_PATH hit`
comment). Concretely: `api.pyrs` imports its sibling `data.pyrs` with the
FULL dotted path, `from tzdata.data import ZoneRule, TransitionRule,
_zone_table` — a bare `from data import ...` does NOT resolve. Any new
package-internal module must do the same: import siblings by their full
`tzdata.<module>` dotted path.

## Build & run

The package is not standalone-runnable; build it via the demo program or a
test, with `PYRST_PATH` pointed at `extern/packages` (the directory
CONTAINING `tzdata/`, not `tzdata/` itself):

```sh
# from extern/packages/tzdata/
PYRST_PATH=/home/ethos/Coding/pyrst/extern/packages \
  /home/ethos/Coding/pyrst/target/release/pyrst build tests/smoke_api.pyrs
./smoke_api
```

Prints `PASS: smoke_api` and exits 0, or `FAIL: <reason>` lines and exits 1.

See `extern/programs/tzdata_demo/README.md` for the world-clock demo.

## Language gaps hit while scaffolding (logged on card `965bf13f`)

- **Compound `Optional` narrowing (`if a is None or b is None:`) does not
  narrow either variable.** `PYTHON_COMPATIBILITY.md`'s narrowing section
  documents the supported shapes (in-branch, early-return, loop-scoped,
  `while`-traversal) — none of them cover an `or`-combined guard over two
  separate `Optional` locals. Confirmed empirically (`pyrst check` on
  `api.pyrs`'s `_is_dst_active`: `expected TransitionRule, found
  TransitionRule | None`). Workaround: two separate early-return `if x is
  None: return ...` guards, one per variable (see `_is_dst_active`).
- **Trailing comma before a closing `)` in a multi-line parameter list is a
  parse error** (`expected identifier (parameter name), found RParen`),
  unlike Python which allows a trailing comma there. Confirmed empirically
  on `data.pyrs`'s `ZoneRule.__init__`. Not found stated in
  `PYTHON_COMPATIBILITY.md`. Workaround: omit the trailing comma on the last
  parameter.
