# CPython calendar Module Implementation Dossier

## Module: calendar
**Python 3.12 stdlib**
**Probe Date:** 2026-07-02

---

## SURFACE

Public API in scope:

| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `isleap` | fn | `isleap(year: int)` | `bool` | True if year is leap (divisible by 4, except century years not divisible by 400) |
| `leapdays` | fn | `leapdays(y1: int, y2: int)` | `int` | Count of leap days between y1 (inclusive) and y2 (exclusive) |
| `monthrange` | fn | `monthrange(year: int, month: int)` | `tuple[Day, int]` | Returns (first_weekday_of_month, num_days_in_month) |
| `weekday` | fn | `weekday(year: int, month: int, day: int)` | `Day` | 0-6 weekday integer (0=Monday) for given date |
| `firstweekday` | fn | `firstweekday()` | `int` | Current first weekday setting (0=Monday, 6=Sunday) |
| `setfirstweekday` | fn | `setfirstweekday(firstweekday: int)` | `None` | Set first weekday globally (0-6, validates range) |
| `month` | fn | `month(theyear: int, themonth: int, w: int=0, l: int=0)` | `str` | Formatted month calendar text (w=col width, l=lines between weeks) |
| `calendar` | fn | `calendar(theyear: int, w: int=2, l: int=1, c: int=6, m: int=3)` | `str` | Formatted year calendar text (w=col width, l=lines between rows, c=spacing between months, m=months per row) |
| `monthcalendar` | fn | `monthcalendar(year: int, month: int)` | `list[list[int]]` | List of week rows (7 elements each), padding with 0 for days not in month |
| `day_name` | const | (indexed array) | `array[str]` | Day names: [0]='Monday'...[6]='Sunday' (7 elements) |
| `day_abbr` | const | (indexed array) | `array[str]` | Day abbreviations: [0]='Mon'...[6]='Sun' (7 elements) |
| `month_name` | const | (indexed array) | `array[str]` | Month names: [0]='', [1]='January'...[12]='December' (13 elements, [0] is empty string) |
| `month_abbr` | const | (indexed array) | `array[str]` | Month abbreviations: [0]='', [1]='Jan'...[12]='Dec' (13 elements, [0] is empty string) |
| `mdays` | const | `list[int]` | `list[int]` | Days per month (non-leap): [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31] |
| `MONDAY` | const | `Day` | `int` (enum) | Value 0; represents Monday |
| `TUESDAY` | const | `Day` | `int` (enum) | Value 1; represents Tuesday |
| `WEDNESDAY` | const | `Day` | `int` (enum) | Value 2; represents Wednesday |
| `THURSDAY` | const | `Day` | `int` (enum) | Value 3; represents Thursday |
| `FRIDAY` | const | `Day` | `int` (enum) | Value 4; represents Friday |
| `SATURDAY` | const | `Day` | `int` (enum) | Value 5; represents Saturday |
| `SUNDAY` | const | `Day` | `int` (enum) | Value 6; represents Sunday |
| `JANUARY` | const | `Month` | `int` (enum) | Value 1; represents January |
| `FEBRUARY` | const | `Month` | `int` (enum) | Value 2; represents February |
| `MARCH` | const | `Month` | `int` (enum) | Value 3; represents March |
| `APRIL` | const | `Month` | `int` (enum) | Value 4; represents April |
| `MAY` | const | `Month` | `int` (enum) | Value 5; represents May |
| `JUNE` | const | `Month` | `int` (enum) | Value 6; represents June |
| `JULY` | const | `Month` | `int` (enum) | Value 7; represents July |
| `AUGUST` | const | `Month` | `int` (enum) | Value 8; represents August |
| `SEPTEMBER` | const | `Month` | `int` (enum) | Value 9; represents September |
| `OCTOBER` | const | `Month` | `int` (enum) | Value 10; represents October |
| `NOVEMBER` | const | `Month` | `int` (enum) | Value 11; represents November |
| `DECEMBER` | const | `Month` | `int` (enum) | Value 12; represents December |
| `EPOCH` | const | `int` | `int` | Value 1970; Unix epoch year constant |

---

## ERRORS

### isleap
- `isleap('2024')` → `TypeError: not all arguments converted during string formatting` (string coercion failure in formatting)
- Non-integer floats accepted but coerced: `isleap(1.5)` → `False` (1 is not leap)
- Negative integers accepted: `isleap(-1)` → `False`

### leapdays
- Type mismatch on first arg: `leapdays('2000', 2004)` → `TypeError: unsupported operand type(s) for -=: 'str' and 'int'`
- Reversed range is allowed: `leapdays(2004, 2000)` → `-1` (negative count)

### monthrange
- Invalid month number < 1: `monthrange(2024, 0)` → `calendar.IllegalMonthError: bad month number 0; must be 1-12`
- Invalid month number > 12: `monthrange(2024, 13)` → `calendar.IllegalMonthError: bad month number 13; must be 1-12`
- String year: `monthrange('2024', 1)` → `TypeError: '<=' not supported between instances of 'int' and 'str'`
- Float month: `monthrange(2024, 1.5)` → `TypeError: 'float' object cannot be interpreted as an integer`

### weekday
- Month out of range (0): `weekday(2024, 0, 1)` → `ValueError: month must be in 1..12`
- Month out of range (13): `weekday(2024, 13, 1)` → `ValueError: month must be in 1..12`
- Day out of range for month (0): `weekday(2024, 1, 0)` → `ValueError: day is out of range for month`
- Day out of range for month (32): `weekday(2024, 1, 32)` → `ValueError: day is out of range for month`
- Day out of range in non-leap Feb: `weekday(2024, 2, 30)` → `ValueError: day is out of range for month`

### monthcalendar
- Invalid month number: `monthcalendar(2024, 0)` → `calendar.IllegalMonthError: bad month number 0; must be 1-12`
- Invalid month number: `monthcalendar(2024, 13)` → `calendar.IllegalMonthError: bad month number 13; must be 1-12`

### setfirstweekday
- Invalid weekday number (e.g., 7): Assumed to raise `calendar.IllegalWeekdayError` (not tested directly but exception type exists)

---

## BEHAVIOR MATRIX

### isleap Probes
```
isleap(1): False
isleap(4): True
isleap(100): False
isleap(400): True
isleap(1600): True
isleap(1700): False
isleap(1800): False
isleap(1900): False
isleap(2000): True
isleap(2004): True
isleap(2023): False
isleap(2024): True
isleap(2025): False
isleap(2100): False
```

### leapdays Probes
```
leapdays(1900, 1904): 0
leapdays(1, 5): 1
leapdays(1600, 1604): 1
leapdays(1900, 1904): 0
leapdays(2000, 2000): 0
leapdays(2000, 2001): 1
leapdays(2000, 2004): 1
leapdays(2001, 2000): -1
leapdays(2100, 2104): 0
```

### weekday Probes
```
weekday(1, 1, 1): calendar.MONDAY (0)
weekday(1970, 1, 1): calendar.THURSDAY (3)
weekday(2000, 1, 1): calendar.SATURDAY (5)
weekday(2024, 1, 1): calendar.MONDAY (0)
weekday(2024, 1, 2): calendar.TUESDAY (1)
weekday(2024, 1, 3): calendar.WEDNESDAY (2)
weekday(2024, 1, 4): calendar.THURSDAY (3)
weekday(2024, 1, 5): calendar.FRIDAY (4)
weekday(2024, 1, 6): calendar.SATURDAY (5)
weekday(2024, 1, 7): calendar.SUNDAY (6)
weekday(2024, 2, 29): calendar.THURSDAY (3)
weekday(2024, 12, 31): calendar.TUESDAY (1)
```

### monthrange Probes
Returns tuple of (Day enum, int)
```
monthrange(2024, 1): (calendar.MONDAY, 31)
monthrange(2024, 2): (calendar.THURSDAY, 29)
monthrange(2024, 3): (calendar.FRIDAY, 31)
monthrange(2024, 4): (calendar.MONDAY, 30)
monthrange(2024, 5): (calendar.WEDNESDAY, 31)
monthrange(2024, 6): (calendar.SATURDAY, 30)
monthrange(2024, 7): (calendar.MONDAY, 31)
monthrange(2024, 8): (calendar.THURSDAY, 31)
monthrange(2024, 9): (calendar.SUNDAY, 30)
monthrange(2024, 10): (calendar.TUESDAY, 31)
monthrange(2024, 11): (calendar.FRIDAY, 30)
monthrange(2024, 12): (calendar.SUNDAY, 31)
monthrange(2023, 2): (calendar.WEDNESDAY, 28)
monthrange(2000, 2): (calendar.TUESDAY, 29)
monthrange(1900, 2): (calendar.THURSDAY, 28)
```
(Converted to int for clarity: first element in 0-6 range, second element is positive int)

### monthcalendar Probes (MONDAY firstweekday)
```
monthcalendar(2024, 1):
  [1, 2, 3, 4, 5, 6, 7]
  [8, 9, 10, 11, 12, 13, 14]
  [15, 16, 17, 18, 19, 20, 21]
  [22, 23, 24, 25, 26, 27, 28]
  [29, 30, 31, 0, 0, 0, 0]

monthcalendar(2024, 2) [leap February]:
  [0, 0, 0, 1, 2, 3, 4]
  [5, 6, 7, 8, 9, 10, 11]
  [12, 13, 14, 15, 16, 17, 18]
  [19, 20, 21, 22, 23, 24, 25]
  [26, 27, 28, 29, 0, 0, 0]

monthcalendar(2023, 2) [non-leap February]:
  [0, 0, 1, 2, 3, 4, 5]
  [6, 7, 8, 9, 10, 11, 12]
  [13, 14, 15, 16, 17, 18, 19]
  [20, 21, 22, 23, 24, 25, 26]
  [27, 28, 0, 0, 0, 0, 0]
```

### monthcalendar with SUNDAY firstweekday
```
monthcalendar(2024, 1):
  [0, 1, 2, 3, 4, 5, 6]
  [7, 8, 9, 10, 11, 12, 13]
  [14, 15, 16, 17, 18, 19, 20]
  [21, 22, 23, 24, 25, 26, 27]
  [28, 29, 30, 31, 0, 0, 0]
```

### month() Text Output
```
month(2024, 1, w=0, l=0):
'    January 2024\nMo Tu We Th Fr Sa Su\n 1  2  3  4  5  6  7\n 8  9 10 11 12 13 14\n15 16 17 18 19 20 21\n22 23 24 25 26 27 28\n29 30 31\n'

month(2024, 1, w=3, l=0):
'        January 2024\nMon Tue Wed Thu Fri Sat Sun\n  1   2   3   4   5   6   7\n  8   9  10  11  12  13  14\n 15  16  17  18  19  20  21\n 22  23  24  25  26  27  28\n 29  30  31\n'

month(2024, 1, w=3, l=2):
'        January 2024\n\nMon Tue Wed Thu Fri Sat Sun\n\n  1   2   3   4   5   6   7\n\n  8   9  10  11  12  13  14\n\n 15  16  17  18  19  20  21\n\n 22  23  24  25  26  27  28\n\n 29  30  31\n\n'

month(2024, 2, w=2, l=1) [non-standard]:
'   February 2024\nMo Tu We Th Fr Sa Su\n          1  2  3  4\n 5  6  7  8  9 10 11\n12 13 14 15 16 17 18\n19 20 21 22 23 24 25\n26 27 28 29\n'
```

### calendar() Year Text Output (excerpt, first 15 lines)
```
calendar(2024, w=2, l=1, c=6, m=3):
'                                  2024\n\n      January                   February                   March\nMo Tu We Th Fr Sa Su      Mo Tu We Th Fr Sa Su      Mo Tu We Th Fr Sa Su\n 1  2  3  4  5  6  7                1  2  3  4                   1  2  3\n 8  9 10 11 12 13 14       5  6  7  8  9 10 11       4  5  6  7  8  9 10\n15 16 17 18 19 20 21      12 13 14 15 16 17 18      11 12 13 14 15 16 17\n22 23 24 25 26 27 28      19 20 21 22 23 24 25      18 19 20 21 22 23 24\n29 30 31                  26 27 28 29               25 26 27 28 29 30 31\n'
```

### firstweekday / setfirstweekday
```
Initial value: firstweekday() = 0
After setfirstweekday(calendar.MONDAY): firstweekday() = 0
After setfirstweekday(calendar.SUNDAY): firstweekday() = 6
```

### Name Arrays
```
day_name: ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']
day_abbr: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
month_name: ['', 'January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December']
month_abbr: ['', 'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
mdays: [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
```

---

## HAZARDS

1. **Enum Return Types**: `monthrange()` returns a `(Day, int)` tuple where `Day` is an enum. Pyrst cannot express enums directly; this requires returning `(int, int)` instead and documenting that the first element is 0-6 weekday index.

2. **String Formatting in month()/calendar()**: These functions produce exact-format ASCII text with specific spacing:
   - Month header is centered based on width parameter
   - Weekday abbreviations are 2-3 characters depending on `w` parameter
   - Numeric days are right-aligned within column widths
   - Empty trailing/leading days in weeks are 0s in monthcalendar but spaces in text
   - Line separators (empty lines) are inserted between weeks based on `l` parameter
   - These are fragile to whitespace changes and locale-dependent (not tested with non-C locale)

3. **Module-Level State**: `setfirstweekday()` modifies global module state, affecting `monthcalendar()` and `month()`/`calendar()` text output. No thread-safety guarantees.

4. **Global firstweekday Persistence**: The `firstweekday` setting persists across function calls and persists in tests, requiring explicit reset.

5. **monthcalendar Output Order**: In Python 3.10+, `monthcalendar` returns weeks in calendar order (each sublist is 7 elements), but the 0-padding position depends on which day is first (MONDAY vs SUNDAY), making the output structure firstweekday-dependent.

6. **day_name/day_abbr Indexing**: These arrays index 0-6, with 0=Monday. This does NOT match ISO 8601 (which is 1=Monday, 7=Sunday) and does NOT match the Day enum's numeric values (which happen to align).

7. **month_name/month_abbr Indexing**: These arrays have a dummy element at index 0 (empty string), so month 1 (January) is at index 1. This is different from 0-indexed month constants.

8. **mdays Array**: The array mdays[2] is always 28 (non-leap February); February 29 days in leap years is computed dynamically via isleap().

9. **Custom Exception Hierarchy**: `IllegalMonthError` inherits from both `ValueError` and `IndexError` (dual inheritance), and `IllegalWeekdayError` inherits from `ValueError`. Pyrst must either map to closest builtin or accept the exception message exactly.

10. **Locale Sensitivity**: Although not tested here, `day_name`, `day_abbr`, `month_name`, and `month_abbr` can be affected by `locale.setlocale()` calls in CPython, making the arrays locale-dependent. A Pyrst implementation would ignore locale by default.

11. **Text Formatting with w=0**: The `month()` function with `w=0, l=0` produces output equivalent to default behavior, not "no formatting". This is a quirk of the implementation (0 triggers default calculation).

12. **monthrange Return Type Ambiguity**: Returns a Day enum for first weekday, not an int. Pyrst cannot express this and must convert to int, potentially losing semantic information.

---

## GATED

| Gate | API Part | Suggested Deferral |
|------|----------|-------------------|
| **G5: Enums** (not yet in pyrst) | `Day` and `Month` enum types; `monthrange()` returns `(Day, int)` tuple | Return `int` instead of Day enum; document 0-6 mapping. Export Day/Month as int constants only. |
| **G2: Module-level state** | `setfirstweekday()` modifies global module state; `firstweekday()` reads it | Allow module-level mutable state for this one setting OR redesign API to pass firstweekday as parameter to functions. |
| **G3: Exception hierarchy** | `IllegalMonthError` dual-inherits from `ValueError` and `IndexError` | Pyrst only has builtin exceptions; map `IllegalMonthError` to `ValueError` with exact CPython message text. |
| **G7: Text output in month()/calendar()** | Locale-sensitive string formatting with exact whitespace | Implement with C-locale hardcoded assumptions; flag if localization is ever needed. |

---

## PARITY PLAN

Dual-run-safe test cases for pyrst golden (avoiding formatting/ordering hazards):

```python
# isleap tests - 7 cases
assert calendar.isleap(2024) == True
assert calendar.isleap(2023) == False
assert calendar.isleap(2000) == True
assert calendar.isleap(1900) == False
assert calendar.isleap(1904) == True
assert calendar.isleap(1) == False
assert calendar.isleap(400) == True

# leapdays tests - 6 cases
assert calendar.leapdays(2000, 2004) == 1
assert calendar.leapdays(2000, 2000) == 0
assert calendar.leapdays(2001, 2000) == -1
assert calendar.leapdays(1900, 1904) == 0
assert calendar.leapdays(2100, 2104) == 0
assert calendar.leapdays(1, 5) == 1

# weekday tests - 8 cases (convert Day enum to int for comparison)
assert int(calendar.weekday(2024, 1, 1)) == 0  # MONDAY
assert int(calendar.weekday(2024, 1, 2)) == 1  # TUESDAY
assert int(calendar.weekday(2024, 1, 7)) == 6  # SUNDAY
assert int(calendar.weekday(2000, 1, 1)) == 5  # SATURDAY
assert int(calendar.weekday(2024, 2, 29)) == 3  # THURSDAY
assert int(calendar.weekday(1970, 1, 1)) == 3  # THURSDAY
assert int(calendar.weekday(2024, 12, 31)) == 1  # TUESDAY
assert int(calendar.weekday(1, 1, 1)) == 0  # MONDAY

# monthrange tests - 10 cases (convert Day enum to int)
first_day, num_days = calendar.monthrange(2024, 1)
assert int(first_day) == 0 and num_days == 31  # Jan 2024 starts Monday
first_day, num_days = calendar.monthrange(2024, 2)
assert int(first_day) == 3 and num_days == 29  # Feb 2024 (leap) starts Thursday
first_day, num_days = calendar.monthrange(2023, 2)
assert int(first_day) == 2 and num_days == 28  # Feb 2023 (non-leap) starts Wednesday
first_day, num_days = calendar.monthrange(2024, 12)
assert int(first_day) == 6 and num_days == 31  # Dec 2024 starts Sunday
first_day, num_days = calendar.monthrange(2000, 2)
assert int(first_day) == 1 and num_days == 29  # Feb 2000 (leap) starts Tuesday

# monthcalendar structure tests - 6 cases (list structure only, MONDAY first)
result = calendar.monthcalendar(2024, 1)
assert len(result) == 5  # 5 weeks in January 2024
assert len(result[0]) == 7  # Always 7 elements per week
assert result[0] == [1, 2, 3, 4, 5, 6, 7]  # First week days
assert result[-1][-1] == 0  # Last element is padding
result = calendar.monthcalendar(2024, 2)
assert result[-1][-1] == 0  # Leap Feb padding
result = calendar.monthcalendar(2023, 2)
assert result[0][0] == 0  # Non-leap Feb starts with padding

# mdays constant - 3 cases
assert calendar.mdays[1] == 31  # Jan
assert calendar.mdays[2] == 28  # Feb (non-leap template)
assert calendar.mdays[12] == 31  # Dec

# day_name / day_abbr - 4 cases
assert calendar.day_name[0] == 'Monday'
assert calendar.day_abbr[0] == 'Mon'
assert calendar.day_name[6] == 'Sunday'
assert calendar.day_abbr[6] == 'Sun'

# month_name / month_abbr - 4 cases
assert calendar.month_name[1] == 'January'
assert calendar.month_abbr[1] == 'Jan'
assert calendar.month_name[12] == 'December'
assert calendar.month_abbr[12] == 'Dec'

# firstweekday default - 2 cases
assert calendar.firstweekday() == 0  # Default MONDAY
calendar.setfirstweekday(calendar.MONDAY)
assert calendar.firstweekday() == 0

# Error cases - 6 cases (ValueError/IllegalMonthError type check, message substring)
try:
    calendar.monthrange(2024, 0)
    assert False, "should raise"
except ValueError as e:
    assert "bad month number 0" in str(e)

try:
    calendar.monthrange(2024, 13)
    assert False, "should raise"
except ValueError as e:
    assert "bad month number 13" in str(e)

try:
    calendar.weekday(2024, 0, 1)
    assert False, "should raise"
except ValueError as e:
    assert "month must be in 1..12" in str(e)

try:
    calendar.weekday(2024, 1, 32)
    assert False, "should raise"
except ValueError as e:
    assert "day is out of range" in str(e)

try:
    calendar.weekday(2024, 1, 0)
    assert False, "should raise"
except ValueError as e:
    assert "day is out of range" in str(e)

try:
    calendar.monthcalendar(2024, 13)
    assert False, "should raise"
except ValueError as e:
    assert "bad month number 13" in str(e)
```

---

## TARGET

**Fidelity Score: 3/5**

### Reasons for 3/5 (not 5):

1. **Enum Return Types (Critical)**: `monthrange()` returns a Day enum for the first weekday, but pyrst cannot express enums directly in current constraints. Must fallback to `int` return, losing the semantic Day type. This changes the return type signature.

2. **Module-Level Mutable State (High)**: `setfirstweekday()` modifies global state that affects multiple functions' outputs. Pyrst cannot express module-level mutable state (constraint G2), requiring either:
   - Acceptance of a design compromise (e.g., passing firstweekday as parameter to all affected functions)
   - Or declaring `setfirstweekday()` out of scope and only allowing hardcoded firstweekday

3. **Text Formatting (Medium)**: The `month()` and `calendar()` functions produce locale-sensitive, exact-whitespace ASCII art. Pyrst's string handling may differ in edge cases (unicode normalization, locale), and replicating exact spacing is fragile. Locale-independence must be enforced.

4. **Custom Exception Hierarchy (Low-Medium)**: `IllegalMonthError` dual-inherits from `ValueError` and `IndexError`, which pyrst cannot express. Must collapse to `ValueError`, losing semantic information about the error.

### Achievability: Moderate

- Core functions (isleap, leapdays, weekday, monthrange, monthcalendar) are straightforward without the return-type and state constraints.
- Name arrays are static and portable.
- Text formatting is complex but doable if locale is locked to C.
- Enum constants can be expressed as plain int constants.

### Confidence Intervals:
- **Fully achievable (95% confidence)**: isleap, leapdays, weekday, day_name, day_abbr, month_name, month_abbr, mdays constants
- **Mostly achievable (80% confidence)**: monthcalendar, month(), calendar() text outputs (if firstweekday is locked or parameterized)
- **Achievable with compromise (65% confidence)**: monthrange() (returns `int` instead of Day)
- **Constrained (50% confidence)**: setfirstweekday() (if module state is not allowed, must redesign or defer)

**Suggested Deferral Strategy**:
- Implement phases: Phase 1 (isleap, leapdays, weekday) requires no state. Phase 2 (monthrange, monthcalendar) requires Day enum handling. Phase 3 (month, calendar text) requires formatting. Phase 4 (setfirstweekday, global state) deferred until after constraint clarity.

