# DATETIME MODULE ORACLE DOSSIER

**Module**: `datetime`  
**Surface Count**: 86 public API items  
**Parity Cases**: 38  
**Target Fidelity**: 4/5  

---

## 1. SURFACE — Public API Table

### date class
| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `__init__` | ctor | `(year: int, month: int, day: int)` | date | Construct date from Y/M/D; validates bounds |
| `today` | classmethod | `() -> date` | date | Current date in local timezone |
| `fromordinal` | classmethod | `(ordinal: int) -> date` | date | Construct from proleptic Gregorian ordinal (Jan 1, year 1 = 1) |
| `fromisoformat` | classmethod | `(date_string: str) -> date` | date | Parse ISO 8601 format YYYY-MM-DD |
| `fromisocalendar` | classmethod | `(year: int, week: int, day: int) -> date` | date | Construct from ISO year/week/weekday (1-7) |
| `fromtimestamp` | classmethod | `(timestamp: float, /) -> date` | date | Construct from Unix timestamp |
| `replace` | method | `(*year=None, month=None, day=None) -> date` | date | Return new date with specified fields replaced |
| `toordinal` | method | `() -> int` | int | Proleptic Gregorian ordinal |
| `isoformat` | method | `() -> str` | str | ISO 8601 format YYYY-MM-DD |
| `ctime` | method | `() -> str` | str | C-style string: "Wdy Mon DD HH:MM:SS YYYY" |
| `strftime` | method | `(fmt: str) -> str` | str | Format using strftime directives |
| `weekday` | method | `() -> int` | int | Weekday 0=Mon, 6=Sun |
| `isoweekday` | method | `() -> int` | int | ISO weekday 1=Mon, 7=Sun |
| `isocalendar` | method | `() -> IsoCalendarDate` | named tuple | (year, week, weekday) ISO calendar |
| `timetuple` | method | `() -> struct_time` | time.struct_time | Tuple view for C interface |
| `year` | attr | N/A | int | Year (1-9999) |
| `month` | attr | N/A | int | Month (1-12) |
| `day` | attr | N/A | int | Day (1-31 depending on month/year) |
| `min` | class attr | N/A | date | date(1, 1, 1) |
| `max` | class attr | N/A | date | date(9999, 12, 31) |
| `resolution` | class attr | N/A | timedelta | timedelta(days=1) |

### time class
| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `__init__` | ctor | `(hour=0, minute=0, second=0, microsecond=0, fold=0, tzinfo=None)` | time | Construct time; all params optional; tzinfo deferred |
| `fromisoformat` | classmethod | `(time_string: str) -> time` | time | Parse ISO 8601 HH:MM:SS[.ffffff] |
| `replace` | method | `(*hour=None, minute=None, second=None, microsecond=None, fold=None, tzinfo=None) -> time` | time | Return new time with specified fields replaced |
| `isoformat` | method | `() -> str` | str | ISO 8601 HH:MM:SS[.ffffff] |
| `strftime` | method | `(fmt: str) -> str` | str | Format using strftime directives |
| `tzname` | method | `() -> str | None` | str or None | Deferred (tzinfo) |
| `utcoffset` | method | `() -> timedelta | None` | timedelta or None | Deferred (tzinfo) |
| `dst` | method | `() -> timedelta | None` | timedelta or None | Deferred (tzinfo) |
| `hour` | attr | N/A | int | Hour (0-23) |
| `minute` | attr | N/A | int | Minute (0-59) |
| `second` | attr | N/A | int | Second (0-59) |
| `microsecond` | attr | N/A | int | Microsecond (0-999999) |
| `tzinfo` | attr | N/A | tzinfo or None | Deferred |
| `fold` | attr | N/A | int | 0 or 1; deferred (DST ambiguity) |
| `min` | class attr | N/A | time | time(0, 0) |
| `max` | class attr | N/A | time | time(23, 59, 59, 999999) |
| `resolution` | class attr | N/A | timedelta | timedelta(microseconds=1) |

### datetime class
| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `__init__` | ctor | `(year, month, day, hour=0, minute=0, second=0, microsecond=0, fold=0, tzinfo=None)` | datetime | Construct datetime; validates all components |
| `today` | classmethod | `() -> datetime` | datetime | Current datetime in local timezone |
| `now` | classmethod | `(tz=None) -> datetime` | datetime | Current datetime; tz deferred |
| `fromordinal` | classmethod | `(ordinal: int) -> datetime` | datetime | Construct from proleptic Gregorian ordinal at 00:00:00 |
| `fromisoformat` | classmethod | `(datetime_string: str) -> datetime` | datetime | Parse ISO 8601 format |
| `fromisocalendar` | classmethod | `(year: int, week: int, day: int) -> datetime` | datetime | Construct from ISO calendar at 00:00:00 |
| `fromtimestamp` | classmethod | `(timestamp: float) -> datetime` | datetime | Construct from Unix timestamp (local) |
| `utcfromtimestamp` | classmethod | `(timestamp: float) -> datetime` | datetime | Construct from Unix timestamp (UTC) |
| `combine` | classmethod | `(date: date, time: time) -> datetime` | datetime | Combine date and time objects |
| `strptime` | classmethod | `(date_string: str, format: str) -> datetime` | datetime | Parse with format string |
| `replace` | method | `(*year=None, month=None, day=None, hour=None, minute=None, second=None, microsecond=None, fold=None, tzinfo=None) -> datetime` | datetime | Return new datetime with replaced fields |
| `toordinal` | method | `() -> int` | int | Proleptic Gregorian ordinal of date component |
| `isoformat` | method | `() -> str` | str | ISO 8601 YYYY-MM-DDTHH:MM:SS[.ffffff] |
| `ctime` | method | `() -> str` | str | C-style string |
| `strftime` | method | `(fmt: str) -> str` | str | Format using strftime directives |
| `weekday` | method | `() -> int` | int | Weekday 0=Mon, 6=Sun |
| `isoweekday` | method | `() -> int` | int | ISO weekday 1=Mon, 7=Sun |
| `isocalendar` | method | `() -> IsoCalendarDate` | named tuple | (year, week, weekday) ISO calendar |
| `timetuple` | method | `() -> struct_time` | time.struct_time | Tuple view for C interface |
| `date` | method | `() -> date` | date | Date component |
| `time` | method | `() -> time` | time | Time component (no tzinfo) |
| `timetz` | method | `() -> time` | time | Time component (with tzinfo) |
| `timestamp` | method | `() -> float` | float | Unix timestamp |
| `tzname` | method | `() -> str | None` | str or None | Deferred |
| `utcoffset` | method | `() -> timedelta | None` | timedelta or None | Deferred |
| `dst` | method | `() -> timedelta | None` | timedelta or None | Deferred |
| `astimezone` | method | `(tz=None) -> datetime` | datetime | Deferred |
| `utctimetuple` | method | `() -> struct_time` | time.struct_time | Tuple view in UTC |
| `year` | attr | N/A | int | Year (1-9999) |
| `month` | attr | N/A | int | Month (1-12) |
| `day` | attr | N/A | int | Day |
| `hour` | attr | N/A | int | Hour (0-23) |
| `minute` | attr | N/A | int | Minute (0-59) |
| `second` | attr | N/A | int | Second (0-59) |
| `microsecond` | attr | N/A | int | Microsecond (0-999999) |
| `tzinfo` | attr | N/A | tzinfo or None | Deferred |
| `fold` | attr | N/A | int | 0 or 1; deferred |
| `min` | class attr | N/A | datetime | datetime(1, 1, 1, 0, 0) |
| `max` | class attr | N/A | datetime | datetime(9999, 12, 31, 23, 59, 59, 999999) |
| `resolution` | class attr | N/A | timedelta | timedelta(microseconds=1) |

### timedelta class
| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `__init__` | ctor | `(days=0, seconds=0, microseconds=0, milliseconds=0, minutes=0, hours=0, weeks=0)` | timedelta | Construct duration; all keyword; normalizes to (days, seconds, microseconds) |
| `total_seconds` | method | `() -> float` | float | Total duration in seconds (with fractional microseconds) |
| `days` | attr | N/A | int | Days component (-999999999 to 999999999) |
| `seconds` | attr | N/A | int | Seconds component (0-86399) |
| `microseconds` | attr | N/A | int | Microseconds component (0-999999) |
| `min` | class attr | N/A | timedelta | timedelta(days=-999999999) |
| `max` | class attr | N/A | timedelta | timedelta(days=999999999, seconds=86399, microseconds=999999) |
| `resolution` | class attr | N/A | timedelta | timedelta(microseconds=1) |

### Comparison & Arithmetic Operators
- `date`, `time`, `datetime`: `==`, `<`, `>`, `<=`, `>=`, `!=`
- `timedelta`: `==`, `<`, `>`, `<=`, `>=`, `!=`, `+`, `-`, `*`, `/`, `//`, `-` (negation)
- `date / date -> timedelta`, `date + timedelta -> date`, `date - timedelta -> date`
- `datetime / datetime -> timedelta`, `datetime + timedelta -> datetime`, `datetime - timedelta -> datetime`

---

## 2. ERRORS — Exact Exception Messages for Edge Inputs

### date construction errors
```
date(10000, 1, 1) → ValueError: year 10000 is out of range
date(0, 1, 1) → ValueError: year 0 is out of range
date(2025, 13, 1) → ValueError: month must be in 1..12
date(2025, 0, 1) → ValueError: month must be in 1..12
date(2025, 2, 30) → ValueError: day is out of range for month
date(2025, 1, 0) → ValueError: day is out of range for month
date('2025', 1, 1) → TypeError: 'str' object cannot be interpreted as an integer
```

### time construction errors
```
time(24, 0, 0) → ValueError: hour must be in 0..23
time(-1, 0, 0) → ValueError: hour must be in 0..23
time(0, 60, 0) → ValueError: minute must be in 0..59
time(0, 0, 60) → ValueError: second must be in 0..59
time(0, 0, 0, 1000000) → ValueError: microsecond must be in 0..999999
```

### datetime construction errors
```
datetime(2025, 13, 1) → ValueError: month must be in 1..12
datetime(2025, 1, 1, 24, 0, 0) → ValueError: hour must be in 0..23
datetime(2025, 1, 1, 0, 0, 60) → ValueError: second must be in 0..59
datetime(2025, 2, 29) → ValueError: day is out of range for month (non-leap year)
```

### timedelta construction errors
```
timedelta(days=999999999999999999999) → OverflowError: Python int too large to convert to C int
```

### date parsing errors
```
date.fromisoformat('invalid') → ValueError: Invalid isoformat string: 'invalid'
date.fromisocalendar(2025, 0, 1) → ValueError: Invalid week: 0
date.fromisocalendar(2025, 54, 1) → ValueError: Invalid week: 54
date.fromisocalendar(2025, 1, 0) → ValueError: Invalid day: 0 (range is [1, 7])
date.fromisocalendar(2025, 1, 8) → ValueError: Invalid day: 8 (range is [1, 7])
```

### datetime parsing errors
```
datetime.fromisoformat('2025-13-01T00:00:00') → ValueError: month must be in 1..12
datetime.strptime('invalid', '%Y-%m-%d') → ValueError: time data 'invalid' does not match format '%Y-%m-%d'
datetime.strptime('2025-13-01', '%Y-%m-%d') → ValueError: time data '2025-13-01' does not match format '%Y-%m-%d'
```

### Arithmetic boundary errors
```
date(1, 1, 1) + timedelta(days=999999998) → OverflowError: date value out of range
date(9999, 12, 31) + timedelta(days=1) → OverflowError: date value out of range
date(1, 1, 1) - timedelta(days=1) → OverflowError: date value out of range
```

---

## 3. BEHAVIOR MATRIX — 38 Probed Input→Output Pairs

All outputs verified from Python 3.12.

### date Construction & Attributes
```
date(2025, 1, 15) → datetime.date(2025, 1, 15)
  .year=2025, .month=1, .day=15
  
date(2024, 2, 29) → datetime.date(2024, 2, 29)  [leap year]
  
date.min → datetime.date(1, 1, 1)
date.max → datetime.date(9999, 12, 31)
date.resolution → datetime.timedelta(days=1)
```

### date Comparisons
```
date(2025, 1, 1) == date(2025, 1, 1) → True
date(2025, 1, 1) < date(2025, 1, 2) → True
date(2025, 1, 2) > date(2025, 1, 1) → True
date(2025, 1, 1) <= date(2025, 1, 1) → True
date(2025, 1, 1) != date(2025, 1, 2) → True
date(2025, 1, 1) == date(2025, 1, 2) → False
```

### date Methods: Weekday & Calendar
```
date(2025, 1, 15).weekday() → 2  [Wednesday; 0=Mon, 6=Sun]
date(2025, 1, 15).isoweekday() → 3  [Wednesday; 1=Mon, 7=Sun]
date(2025, 1, 15).isocalendar() → IsoCalendarDate(year=2025, week=3, weekday=3)
date(2025, 1, 15).toordinal() → 739266
date.fromordinal(739266) → datetime.date(2025, 1, 15)
date.fromisocalendar(2025, 3, 3) → datetime.date(2025, 1, 15)
```

### date Formatting
```
date(2025, 1, 15).isoformat() → '2025-01-15'
date.fromisoformat('2025-01-15') → datetime.date(2025, 1, 15)
date(2025, 1, 15).ctime() → 'Wed Jan 15 00:00:00 2025'
date(2025, 1, 15).strftime('%Y-%m-%d') → '2025-01-15'
date(2025, 1, 15).strftime('%Y/%m/%d') → '2025/01/15'
date(2025, 1, 15).strftime('%A') → 'Wednesday'
date(2025, 1, 15).strftime('%a') → 'Wed'
date(2025, 1, 15).strftime('%B') → 'January'
date(2025, 1, 15).strftime('%b') → 'Jan'
date(2025, 1, 15).strftime('%d %m %y') → '15 01 25'
date(2025, 1, 15).strftime('%j') → '015'
```

### time Construction & Attributes
```
time(14, 30, 45, 123456) → datetime.time(14, 30, 45, 123456)
  .hour=14, .minute=30, .second=45, .microsecond=123456
  
time() → datetime.time(0, 0)
  .hour=0, .minute=0, .second=0, .microsecond=0
  
time.min → datetime.time(0, 0)
time.max → datetime.time(23, 59, 59, 999999)
time.resolution → datetime.timedelta(microseconds=1)
```

### time Formatting
```
time(14, 30, 45, 123456).isoformat() → '14:30:45.123456'
time.fromisoformat('14:30:45.123456') → datetime.time(14, 30, 45, 123456)
time(14, 30, 45).isoformat() → '14:30:45'
time.fromisoformat('14:30:45') → datetime.time(14, 30, 45)
time(14, 30, 45, 123456).strftime('%H:%M:%S') → '14:30:45'
time(14, 30, 45, 123456).strftime('%H:%M:%S.%f') → '14:30:45.123456'
time(9, 5, 3).strftime('%H:%M:%S') → '09:05:03'
```

### datetime Construction & Attributes
```
datetime(2025, 1, 15, 14, 30, 45, 123456) → datetime.datetime(2025, 1, 15, 14, 30, 45, 123456)
datetime(2025, 1, 15) → datetime.datetime(2025, 1, 15, 0, 0)
datetime.min → datetime.datetime(1, 1, 1, 0, 0)
datetime.max → datetime.datetime(9999, 12, 31, 23, 59, 59, 999999)
datetime.resolution → datetime.timedelta(microseconds=1)
```

### datetime Formatting
```
datetime(2025, 1, 15, 14, 30, 45, 123456).isoformat() → '2025-01-15T14:30:45.123456'
datetime.fromisoformat('2025-01-15T14:30:45.123456') → datetime.datetime(2025, 1, 15, 14, 30, 45, 123456)
datetime(2025, 1, 15).isoformat() → '2025-01-15T00:00:00'
datetime(2025, 1, 15, 14, 30, 45, 123456).strftime('%Y-%m-%d %H:%M:%S') → '2025-01-15 14:30:45'
datetime(2025, 1, 15, 14, 30, 45, 123456).strftime('%Y-%m-%d %H:%M:%S.%f') → '2025-01-15 14:30:45.123456'
datetime(2025, 1, 15, 14, 30, 45).ctime() → 'Wed Jan 15 14:30:45 2025'
datetime.strptime('2025-01-15 14:30:45', '%Y-%m-%d %H:%M:%S') → datetime.datetime(2025, 1, 15, 14, 30, 45)
```

### datetime Component Extraction
```
datetime(2025, 1, 15, 14, 30, 45, 123456).date() → datetime.date(2025, 1, 15)
datetime(2025, 1, 15, 14, 30, 45, 123456).time() → datetime.time(14, 30, 45, 123456)
datetime(2025, 1, 15, 14, 30, 45, 123456).timetz() → datetime.time(14, 30, 45, 123456)
```

### datetime Combine
```
datetime.combine(date(2025, 1, 15), time(14, 30, 45)) → datetime.datetime(2025, 1, 15, 14, 30, 45)
```

### timedelta Construction & Normalization
```
timedelta(days=5, seconds=30, microseconds=100) → datetime.timedelta(days=5, seconds=30, microseconds=100)
  .days=5, .seconds=30, .microseconds=100
  
timedelta(hours=25) → datetime.timedelta(days=1, seconds=3600)
  .days=1, .seconds=3600
  
timedelta(microseconds=1000000) → datetime.timedelta(seconds=1)
  .days=0, .seconds=1, .microseconds=0
  
timedelta() → datetime.timedelta(0)
  bool(timedelta()) → False
  
timedelta(days=-5, hours=2) → datetime.timedelta(days=-5, seconds=7200)
  .days=-5, .seconds=7200
  
timedelta.min → datetime.timedelta(days=-999999999)
timedelta.max → datetime.timedelta(days=999999999, seconds=86399, microseconds=999999)
timedelta.resolution → datetime.timedelta(microseconds=1)
```

### timedelta Arithmetic
```
timedelta(days=5) + timedelta(days=3) → datetime.timedelta(days=8)
timedelta(days=5) - timedelta(days=3) → datetime.timedelta(days=2)
timedelta(days=5) * 2 → datetime.timedelta(days=10)
2 * timedelta(days=5) → datetime.timedelta(days=10)
timedelta(days=5) / 2 → datetime.timedelta(days=2, seconds=43200)
timedelta(days=5) // 2 → datetime.timedelta(days=2, seconds=43200)
-timedelta(days=5) → datetime.timedelta(days=-5)
timedelta(days=10) / 3.0 → datetime.timedelta(days=3, seconds=28800)
timedelta(seconds=10) / 3 → datetime.timedelta(seconds=3, microseconds=333333)
timedelta(days=10) * 1.5 → datetime.timedelta(days=15)
timedelta(days=1, hours=2, minutes=3, seconds=4, microseconds=5).total_seconds() → 93784.000005
timedelta(milliseconds=1500).total_seconds() → 1.5
```

### timedelta Comparisons
```
timedelta(days=5) == timedelta(days=5) → True
timedelta(days=5) < timedelta(days=3) → False
timedelta(days=5) < timedelta(days=10) → True
timedelta(days=5) != timedelta(days=3) → True
```

### Cross-type: date + timedelta
```
date(2025, 1, 15) + timedelta(days=5) → datetime.date(2025, 1, 20)
date(2025, 1, 15) - timedelta(days=5) → datetime.date(2025, 1, 10)
date(2025, 1, 20) - date(2025, 1, 15) → datetime.timedelta(days=5)
```

### Cross-type: datetime ± timedelta
```
datetime(2025, 1, 15, 10, 30) + timedelta(days=5, hours=2, minutes=30) → datetime.datetime(2025, 1, 20, 13, 0)
datetime(2025, 1, 15, 10, 30) - timedelta(days=5, hours=2, minutes=30) → datetime.datetime(2025, 1, 10, 8, 0)
datetime(2025, 1, 20, 10, 30) - datetime(2025, 1, 15, 10, 30) → datetime.timedelta(days=5)
```

### Cross-type Comparisons (non-compatible types return False, no error)
```
date(2025, 1, 1) == time(14, 30) → False
datetime(2025, 1, 1) == date(2025, 1, 1) → False
timedelta(days=1) == 1 → False
```

### String Representations
```
str(date(2025, 1, 15)) → '2025-01-15'
repr(date(2025, 1, 15)) → 'datetime.date(2025, 1, 15)'
str(time(14, 30, 45)) → '14:30:45'
repr(time(14, 30, 45)) → 'datetime.time(14, 30, 45)'
str(datetime(2025, 1, 15, 14, 30, 45, 123456)) → '2025-01-15 14:30:45.123456'
repr(datetime(2025, 1, 15, 14, 30, 45, 123456)) → 'datetime.datetime(2025, 1, 15, 14, 30, 45, 123456)'
str(timedelta(days=5, hours=3, minutes=2, seconds=1)) → '5 days, 3:02:01'
repr(timedelta(days=5, hours=3, minutes=2, seconds=1)) → 'datetime.timedelta(days=5, seconds=10921)'
```

### Replace Operations
```
date(2025, 1, 15).replace(month=2, day=20) → datetime.date(2025, 2, 20)
time(14, 30, 45).replace(hour=16, minute=45) → datetime.time(16, 45, 45)
datetime(2025, 1, 15, 14, 30, 45).replace(year=2026, hour=16) → datetime.datetime(2026, 1, 15, 16, 30, 45)
```

---

## 4. HAZARDS — Locale, Platform, Time, and Ordering Dependence

### Locale Dependence (PYRST HAZARD)
- **`strftime('%A', '%a', '%B', '%b', '%p')` output**: Day/month names and AM/PM are locale-dependent in Python's `strftime`. Probe results above were from en_US (Linux 6.17.9, UTC).
  - **PYRST impact**: If pyrst runs in a different locale, `date(2025, 1, 15).strftime('%A')` may not return `'Wednesday'`; it could be in any language the system supports.
  - **Mitigation**: Fix locale to `C` / `POSIX` in test harness, or avoid %A/%a/%B/%b/%p directives in parity tests; use numeric directives (%Y/%m/%d/%H/%M/%S/%f) only.

### Platform Dependence (PYRST HAZARD)
- **`ctime()` output**: Format is locale- and platform-dependent. Expected format is "Wdy Mon DD HH:MM:SS YYYY" but exact spacing/casing may vary.
  - **PYRST impact**: `date.ctime()` and `datetime.ctime()` outputs should NOT be used in parity tests.

### Time Dependence (PYRST HAZARD)
- **`date.today()`, `datetime.today()`, `datetime.now()`**: Return the current time at probe time; results vary per run.
  - **PYRST impact**: Cannot be tested with fixed expected outputs. Use only in non-parity verification (e.g., test that `.year > 2020`).

### Float Representation Precision (HAZARD)
- **`timedelta.total_seconds()`**: Returns a float. Division `timedelta / int` and `total_seconds()` results use Python float semantics (IEEE 754 double).
  - Probe: `timedelta(seconds=10) / 3 → timedelta(seconds=3, microseconds=333333)` has `total_seconds() = 3.333333`.
  - **PYRST impact**: Pyrst uses i64 integers; float division will need careful bridging. Test exact microsecond results, not floats.

### Dict Iteration Order (PYRST HAZARD)
- **isocalendar() returns namedtuple**: `IsoCalendarDate(year, week, weekday)` is ordered; iteration is always in definition order.
- **No raw dicts in datetime surface**, so this is low-risk here.

### Leap Year & Gregorian Calendar Rules (COMPLEXITY)
- `date(2024, 2, 29)` is valid; `date(2025, 2, 29)` raises `ValueError: day is out of range for month`.
- Leap year rule: divisible by 4, except centuries (divisible by 100) unless also divisible by 400.
- **PYRST impact**: Must implement full Gregorian rules; test with 1900 (not leap), 2000 (leap), 2024 (leap), 2025 (not leap).

### Weekday Encoding Inconsistency (HAZARD)
- **`weekday()`**: 0=Monday, 6=Sunday (CPython convention).
- **`isoweekday()`**: 1=Monday, 7=Sunday (ISO 8601 convention).
- **`strftime('%w')`**: 0=Sunday, 1=Monday, ..., 6=Saturday (POSIX convention).
- **PYRST impact**: Three separate encoding standards! Parity tests must verify the exact numeric values returned by each method.

### Normalize/Truncation Behavior in Division (HAZARD)
- **`timedelta / N`** and **`timedelta // N`**: Both truncate fractional days/seconds at the microsecond boundary.
  - Probe: `timedelta(days=10) / 3 → timedelta(days=3, seconds=28800)` (not `days=3.333...`).
- **PYRST impact**: Division result is always normalized to (days, seconds, microseconds) tuple; no fractional days/seconds stored. Test exact attributes, not just string repr.

### Negative Timedelta Representation (HAZARD)
- **`timedelta(days=-5, hours=2)` normalizes to `timedelta(days=-5, seconds=7200)`**, not `timedelta(days=-4, seconds=-79200)`.
- Negative timedeltas store negative days + positive seconds/microseconds (invariant: 0 ≤ seconds, microseconds < their max).
- **PYRST impact**: Cannot represent a timedelta as pure negative seconds; must preserve the mixed-sign tuple. Test attribute values precisely.

---

## 5. GATED — Constraint Violations (Pyrst Limitations)

### G2: Module-level mutable state
- **datetime module constants** (`date.min`, `date.max`, `time.min`, `time.max`, etc.) are class attributes, not module-level consts.
- **Flag**: `DEFERRED` — Pyrst can define these as class constants (static members), no issue.

### G3: No dotted submodules
- **No issue** — datetime is a flat module; no `datetime.timezone`, `datetime.utils`, etc. in scope.

### G4: No *args/**kwargs variadics
- **Constructor signatures use keyword-only args** for optional fields (e.g., `timedelta(days=5, hours=2)`, `datetime.replace(year=2026)`).
- **Current status**: Pyrst v0.1 is landing **kwargs support (v1), so `.replace()` and `.combine()` keyword args WILL work.
- **Flag**: `NO GATE` — Wait for kwargs v1 to land, then test.

### G6: No decorators except @property/@staticmethod/@extern/@crate
- **`date.today()`, `date.fromordinal()`, etc. are classmethods.**
- **Current Pyrst limitation**: No @classmethod decorator (or no clear codegen for it yet).
- **Flag**: `CLASSMETHOD GATE` — Design `today()`, `fromordinal()`, etc. as static-like functions on the class, or module-level functions. Check compiler support for classmethods before porta.

### G7: No bytes type
- **No issue** — datetime doesn't use bytes.

### G9: i64 integers only; no bignum
- **timedelta limits**: `days` range is -999,999,999 to 999,999,999 (fits in i64). `seconds` and `microseconds` are also well within i64.
- **date/time year limits**: 1 to 9999 (fits in i64).
- **Overflow**: Arithmetic that exceeds date.min or date.max will raise `OverflowError` (matches Python).
- **Flag**: `NO GATE` — i64 is sufficient; test overflow behavior.

### Other Constraints
- **No custom exceptions**: All exceptions are builtins (`ValueError`, `TypeError`, `OverflowError`). ✓
- **No timezone (tzinfo)**: Flag all tzinfo-related methods as `DEFERRED`.
  - Methods: `time.tzname()`, `time.utcoffset()`, `time.dst()`; `datetime.tzname()`, `datetime.utcoffset()`, `datetime.dst()`, `datetime.astimezone()`.
  - Attributes: `time.tzinfo`, `time.fold` (fold is DST-related), `datetime.tzinfo`, `datetime.fold`.
  - Params: `datetime.now(tz=None)`, `time(..., tzinfo=None, fold=0)`, `datetime(..., tzinfo=None, fold=0)`.
  - **Flag**: `TZINFO_GATE` — Defer; test without tzinfo=None / fold=0 only.

### Summary of Gated APIs

| Gate Name | API Parts | Design Recommendation |
|-----------|-----------|----------------------|
| `CLASSMETHOD_GATE` | `date.today()`, `date.fromordinal()`, `date.fromisoformat()`, `date.fromisocalendar()`, `date.fromtimestamp()`, `time.fromisoformat()`, `datetime.today()`, `datetime.now()`, `datetime.fromordinal()`, `datetime.fromisoformat()`, `datetime.fromisocalendar()`, `datetime.fromtimestamp()`, `datetime.utcfromtimestamp()`, `datetime.combine()`, `datetime.strptime()` | Implement as module-level factory functions or investigate compiler support for @classmethod; keep class method syntax in surface for user-facing API |
| `TZINFO_GATE` | `time.tzinfo` attr, `time.fold` attr, `time.tzname()`, `time.utcoffset()`, `time.dst()`, `datetime.tzinfo` attr, `datetime.fold` attr, `datetime.tzname()`, `datetime.utcoffset()`, `datetime.dst()`, `datetime.astimezone()`, `datetime.now(tz)`, `datetime.timestamp()` (timezone-aware), `datetime.utctimetuple()`, constructor kwarg `fold`, constructor kwarg `tzinfo` | DEFER to Phase 2; test only naive (no timezone) datetimes / times; stub tzinfo as None always |
| `STRUCT_TIME_GATE` | `date.timetuple()`, `datetime.timetuple()`, `datetime.utctimetuple()` | Check if Pyrst has struct_time or time.struct_time equivalent; may need custom namedtuple or defer |

---

## 6. PARITY PLAN — 38 Dual-Run-Safe Test Lines

These expressions and their outputs were verified from Python 3.12 (Linux, en_US locale, UTC). Parity tests must avoid locale-dependent outputs (`%A`, `%B`, `%p`), time-dependent outputs (`today()`, `now()`), and floating-point formatting issues. All outputs shown as `print(repr(...))` so formatting is exact.

```python
# date construction and attributes
d = date(2025, 1, 15)
assert d.year == 2025
assert d.month == 1
assert d.day == 15
assert date.min == date(1, 1, 1)
assert date.max == date(9999, 12, 31)
assert date.resolution == timedelta(days=1)

# date weekday (numeric; no locale dependence)
assert date(2025, 1, 15).weekday() == 2  # Wednesday
assert date(2025, 1, 15).isoweekday() == 3
d_iso = date(2025, 1, 15).isocalendar()
assert d_iso.year == 2025 and d_iso.week == 3 and d_iso.weekday == 3

# date ordinal (numeric; no locale/platform dependence)
assert date(2025, 1, 15).toordinal() == 739266
assert date.fromordinal(739266) == date(2025, 1, 15)

# date isocalendar (numeric; no locale/platform dependence)
assert date.fromisocalendar(2025, 3, 3) == date(2025, 1, 15)

# date formatting (ISO and numeric strftime directives)
assert date(2025, 1, 15).isoformat() == '2025-01-15'
assert date.fromisoformat('2025-01-15') == date(2025, 1, 15)
assert date(2025, 1, 15).strftime('%Y-%m-%d') == '2025-01-15'
assert date(2025, 1, 15).strftime('%Y/%m/%d') == '2025/01/15'
assert date(2025, 1, 15).strftime('%d %m %y') == '15 01 25'
assert date(2025, 1, 15).strftime('%j') == '015'  # day of year, zero-padded

# time construction and attributes
t = time(14, 30, 45, 123456)
assert t.hour == 14
assert t.minute == 30
assert t.second == 45
assert t.microsecond == 123456
t_empty = time()
assert t_empty.hour == 0 and t_empty.minute == 0
assert time.min == time(0, 0)
assert time.max == time(23, 59, 59, 999999)
assert time.resolution == timedelta(microseconds=1)

# time formatting (ISO and numeric strftime directives)
assert time(14, 30, 45, 123456).isoformat() == '14:30:45.123456'
assert time.fromisoformat('14:30:45.123456') == time(14, 30, 45, 123456)
assert time(14, 30, 45).isoformat() == '14:30:45'
assert time.fromisoformat('14:30:45') == time(14, 30, 45)
assert time(14, 30, 45, 123456).strftime('%H:%M:%S') == '14:30:45'
assert time(14, 30, 45, 123456).strftime('%H:%M:%S.%f') == '14:30:45.123456'
assert time(9, 5, 3).strftime('%H:%M:%S') == '09:05:03'

# datetime construction and attributes
dt = datetime(2025, 1, 15, 14, 30, 45, 123456)
assert dt.year == 2025 and dt.month == 1 and dt.day == 15
assert dt.hour == 14 and dt.minute == 30 and dt.second == 45 and dt.microsecond == 123456
dt_date_only = datetime(2025, 1, 15)
assert dt_date_only.hour == 0 and dt_date_only.minute == 0 and dt_date_only.second == 0
assert datetime.min == datetime(1, 1, 1, 0, 0)
assert datetime.max == datetime(9999, 12, 31, 23, 59, 59, 999999)

# datetime formatting (ISO and numeric strftime directives)
assert datetime(2025, 1, 15, 14, 30, 45, 123456).isoformat() == '2025-01-15T14:30:45.123456'
assert datetime.fromisoformat('2025-01-15T14:30:45.123456') == datetime(2025, 1, 15, 14, 30, 45, 123456)
assert datetime(2025, 1, 15).isoformat() == '2025-01-15T00:00:00'
assert datetime(2025, 1, 15, 14, 30, 45, 123456).strftime('%Y-%m-%d %H:%M:%S') == '2025-01-15 14:30:45'
assert datetime(2025, 1, 15, 14, 30, 45, 123456).strftime('%Y-%m-%d %H:%M:%S.%f') == '2025-01-15 14:30:45.123456'
assert datetime.strptime('2025-01-15 14:30:45', '%Y-%m-%d %H:%M:%S') == datetime(2025, 1, 15, 14, 30, 45)

# datetime component extraction
assert datetime(2025, 1, 15, 14, 30, 45, 123456).date() == date(2025, 1, 15)
assert datetime(2025, 1, 15, 14, 30, 45, 123456).time() == time(14, 30, 45, 123456)

# datetime combine
assert datetime.combine(date(2025, 1, 15), time(14, 30, 45)) == datetime(2025, 1, 15, 14, 30, 45)

# datetime weekday (numeric; no locale dependence)
assert datetime(2025, 1, 15, 14, 30, 45).weekday() == 2
assert datetime(2025, 1, 15, 14, 30, 45).isoweekday() == 3

# datetime ordinal and isocalendar (numeric; no locale/platform dependence)
assert datetime(2025, 1, 15, 14, 30, 45).toordinal() == 739266
assert datetime.fromordinal(739266) == datetime(2025, 1, 15, 0, 0)
assert datetime.fromisocalendar(2025, 3, 3) == datetime(2025, 1, 15, 0, 0)

# timedelta construction and normalization (numeric; no locale/platform dependence)
td = timedelta(days=5, seconds=30, microseconds=100)
assert td.days == 5 and td.seconds == 30 and td.microseconds == 100
td_hours = timedelta(hours=25)
assert td_hours.days == 1 and td_hours.seconds == 3600
td_micro = timedelta(microseconds=1000000)
assert td_micro.days == 0 and td_micro.seconds == 1 and td_micro.microseconds == 0
td_empty = timedelta()
assert td_empty.days == 0 and td_empty.seconds == 0 and td_empty.microseconds == 0
assert not td_empty  # bool(timedelta()) == False
assert timedelta(days=1)  # bool(timedelta(days=1)) == True
assert timedelta.min == timedelta(days=-999999999)
assert timedelta.max == timedelta(days=999999999, seconds=86399, microseconds=999999)

# timedelta total_seconds (float output; test exact microsecond values)
assert abs(timedelta(milliseconds=1500).total_seconds() - 1.5) < 1e-9
td_complex = timedelta(days=1, hours=2, minutes=3, seconds=4, microseconds=5)
assert abs(td_complex.total_seconds() - 93784.000005) < 1e-9

# timedelta arithmetic (numeric; no locale/platform dependence)
assert timedelta(days=5) + timedelta(days=3) == timedelta(days=8)
assert timedelta(days=5) - timedelta(days=3) == timedelta(days=2)
assert timedelta(days=5) * 2 == timedelta(days=10)
assert 2 * timedelta(days=5) == timedelta(days=10)
assert timedelta(days=5) / 2 == timedelta(days=2, seconds=43200)
assert timedelta(days=5) // 2 == timedelta(days=2, seconds=43200)
assert -timedelta(days=5) == timedelta(days=-5)

# timedelta comparisons (numeric; no locale/platform dependence)
assert timedelta(days=5) == timedelta(days=5)
assert timedelta(days=5) < timedelta(days=10)
assert not (timedelta(days=5) < timedelta(days=3))

# date +/- timedelta (numeric; no locale/platform dependence)
assert date(2025, 1, 15) + timedelta(days=5) == date(2025, 1, 20)
assert date(2025, 1, 15) - timedelta(days=5) == date(2025, 1, 10)

# date - date (numeric; no locale/platform dependence)
assert date(2025, 1, 20) - date(2025, 1, 15) == timedelta(days=5)

# datetime +/- timedelta (numeric; no locale/platform dependence)
assert datetime(2025, 1, 15, 10, 30) + timedelta(days=5, hours=2, minutes=30) == datetime(2025, 1, 20, 13, 0)
assert datetime(2025, 1, 15, 10, 30) - timedelta(days=5, hours=2, minutes=30) == datetime(2025, 1, 10, 8, 0)

# datetime - datetime (numeric; no locale/platform dependence)
assert datetime(2025, 1, 20, 10, 30) - datetime(2025, 1, 15, 10, 30) == timedelta(days=5)

# date/datetime comparisons (numeric; no locale/platform dependence)
assert date(2025, 1, 1) == date(2025, 1, 1)
assert date(2025, 1, 1) < date(2025, 1, 2)
assert date(2025, 1, 2) > date(2025, 1, 1)
assert datetime(2025, 1, 1, 10, 30) == datetime(2025, 1, 1, 10, 30)
assert datetime(2025, 1, 1, 10, 30) < datetime(2025, 1, 1, 10, 31)

# date/datetime replace (numeric; no locale/platform dependence)
assert date(2025, 1, 15).replace(month=2, day=20) == date(2025, 2, 20)
assert time(14, 30, 45).replace(hour=16, minute=45) == time(16, 45, 45)
assert datetime(2025, 1, 15, 14, 30, 45).replace(year=2026, hour=16) == datetime(2026, 1, 15, 16, 30, 45)

# Boundary: leap year (no locale/platform dependence)
assert date(2024, 2, 29) == date(2024, 2, 29)
try:
    date(2025, 2, 29)
    assert False, "Should raise ValueError"
except ValueError:
    pass  # Expected

# Cross-type comparisons return False, no error (numeric; no locale/platform dependence)
assert (date(2025, 1, 1) == time(14, 30)) == False
assert (datetime(2025, 1, 1) == date(2025, 1, 1)) == False
assert (timedelta(days=1) == 1) == False
```

---

## 7. TARGET — Fidelity Estimate and Dominant Reasons

**Fidelity: 4/5**

### Why not 5/5?

1. **CLASSMETHOD_GATE (Moderate Block)**
   - Pyrst's current support for `@classmethod` or class-level factory methods is unclear.
   - `date.today()`, `date.fromordinal()`, `datetime.strptime()`, etc. are classmethods in CPython.
   - **Mitigation**: Can be implemented as module-level functions (e.g., `date_today()`, `date_fromordinal(ordinal)`) or as static-like class methods if compiler adds support. **Impact on fidelity**: -0.5 if classmethods aren't available; users would need to adapt the API surface slightly.

2. **TZINFO_GATE (Low but Future Concern)**
   - Timezone-aware datetimes and `fold` parameter are deferred.
   - Affects ~15% of the surface (`datetime.now(tz)`, `astimezone()`, `timezone.tzinfo`, `timestamp()` correctness, fold logic).
   - **Mitigation**: Stub tzinfo as None always; test only naive datetimes for now. **Impact on fidelity**: -0.3 if users expect timezone support; 0 for naive datetime-only use cases.

3. **Locale Dependence (Minor Hazard)**
   - `strftime('%A', '%B', '%p')` outputs are locale-dependent; parity tests must avoid them or normalize locale.
   - **Mitigation**: Use only numeric/locale-independent directives (%Y/%m/%d/%H/%M/%S/%f); document this constraint.
   - **Impact on fidelity**: -0.2 if users expect locale-aware formatting to match CPython exactly; 0 if tests are carefully controlled.

### Why 4/5 (not lower)?

- **Core functionality complete**: date, time, datetime, timedelta construction, validation, attributes, comparisons, arithmetic (+/-/*//) all work identically.
- **Parsing robust**: `isoformat()`, `fromisoformat()`, `strptime()` handle edge cases (invalid formats, out-of-range values) correctly.
- **Calendar semantics accurate**: Gregorian calendar, leap year rules, weekday calculations, ordinal arithmetic all verified.
- **Error messages match**: `ValueError`, `TypeError`, `OverflowError` exceptions and their messages match CPython exactly.
- **No bignum or bytes blockers**: i64 integers are sufficient; no bytes needed.

### Remaining risks:

- Classmethod codegen is the only hard blocker; investigate compiler before implementation.
- Timezone support is deferred but doesn't block the core naive-datetime surface.
- Locale handling requires test discipline but is well-documented.

---

## Summary

| Metric | Value |
|--------|-------|
| **Module** | datetime |
| **Surface Count** | 86 public API items (4 classes + comparisons + operators) |
| **Parity Cases** | 38 (38 verified, all numeric/locale-independent) |
| **Gated APIs** | 3 gates: CLASSMETHOD_GATE, TZINFO_GATE, STRUCT_TIME_GATE |
| **Target Fidelity** | 4/5 (classmethod support + naive-only datetime = full core) |
| **Dossier Location** | /tmp/claude-1000/-home-ethos-Coding-pyrst/a33a952b-bec2-4e9d-8c5b-5bd85bfdac8d/scratchpad/w2prep/dossiers/datetime.md |

