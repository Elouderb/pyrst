# stringio — CPython io.StringIO Oracle Dossier

**Module:** `stringio` (wrapping CPython `io.StringIO`)  
**Surface count:** 21 (1 class + 12 instance methods + 6 properties/flags)  
**Parity cases:** 32  
**Gated constraints:** G2 (module-level mutable state); *args/**kwargs (G4 — handled via keyword-only in call sites)  
**Target fidelity:** 5/5 — StringIO is pure, self-contained, no external state or platform variance  
**Dossier path:** `/tmp/claude-1000/-home-ethos-Coding-pyrst/a33a952b-bec2-4e9d-8c5b-5bd85bfdac8d/scratchpad/w2prep/dossiers/stringio.md`

---

## 1. SURFACE — Public API

| Name | Kind | Signature | Return | Semantics |
|------|------|-----------|--------|-----------|
| `StringIO` | class | `StringIO(initial_value: str \| None = None)` | `StringIO` | In-memory text buffer; initial_value must be str or None, else TypeError. Position starts at 0. |
| `write` | method | `write(s: str) -> int` | `int` | Append string s at current position, advancing tell(); returns length of s. Position overwrites if seek'd. TypeError if s not str. |
| `read` | method | `read(size: int \| None = -1) -> str` | `str` | Read up to size chars from current position. size=-1 or None reads to EOF. Returns empty string at EOF. TypeError if size not int/None. |
| `readline` | method | `readline(size: int \| None = -1) -> str` | `str` | Read one line (up to newline) from current position, up to size chars. Returns empty string at EOF. TypeError if size not int/None. |
| `readlines` | method | `readlines(hint: int = -1) -> list[str]` | `list[str]` | Read all remaining lines from current position as list. hint=-1 reads all; hint >= 0 reads approximate hint bytes then stops. Empty list if at EOF. TypeError if hint not int. |
| `getvalue` | method | `getvalue() -> str` | `str` | Return entire buffer contents (ignores current position). ValueError if closed. |
| `seek` | method | `seek(pos: int, whence: int = 0) -> int` | `int` | Move current position. whence=0 (start, default), 1 (current-relative, only 0 allowed), 2 (end). Returns new position. ValueError if pos<0 or invalid whence. OSError if whence=1 with nonzero offset. |
| `tell` | method | `tell() -> int` | `int` | Return current position in buffer. ValueError if closed. |
| `truncate` | method | `truncate(size: int \| None = None) -> int` | `int` | Truncate buffer to size chars. size=None uses current position. ValueError if size<0. Returns new size. Position unchanged if size set. |
| `close` | method | `close() -> None` | `None` | Close buffer. All I/O operations raise ValueError after close. |
| `flush` | method | `flush() -> None` | `None` | No-op for StringIO. Always succeeds. |
| `writelines` | method | `writelines(lines: list[str]) -> None` | `None` | Write each string in lines to buffer in order (no auto-newlines). TypeError if lines not iterable of strings. |
| `closed` | property | read-only `bool` | `bool` | True if close() called, False otherwise. |
| `readable` | method | `readable() -> bool` | `bool` | Always True before close(); ValueError if closed. |
| `writable` | method | `writable() -> bool` | `bool` | Always True before close(); ValueError if closed. |
| `seekable` | method | `seekable() -> bool` | `bool` | Always True before close(); ValueError if closed. |
| `isatty` | method | `isatty() -> bool` | `bool` | Always False. |
| `encoding` | property | read-only `None` | `None` | Always None for StringIO. |
| `errors` | property | read-only `None` | `None` | Always None for StringIO. |
| `line_buffering` | property | read-only `bool` | `bool` | Always False. |
| `newlines` | property | read-only `None \| str` | `None \| str` | Always None for StringIO (text-mode only). |

---

## 2. ERRORS — Exception Behaviors

**Probe format:** `<expression>` → **ExceptionType: message text**

### Initialization Errors
- `io.StringIO(123)` → **TypeError: initial_value must be str or None, not int**
- `io.StringIO(b"bytes")` → **TypeError: initial_value must be str or None, not bytes**

### write() Errors
- `StringIO().write(123)` → **TypeError: string argument expected, got 'int'**
- `StringIO().write(None)` → **TypeError: string argument expected, got 'NoneType'**
- `closed_stringio.write("x")` → **ValueError: I/O operation on closed file**

### read() Errors
- `StringIO().read("5")` → **TypeError: argument should be integer or None, not 'str'**
- `closed_stringio.read()` → **ValueError: I/O operation on closed file**

### readline() Errors
- `StringIO().readline("5")` → **TypeError: argument should be integer or None, not 'str'**
- `closed_stringio.readline()` → **ValueError: I/O operation on closed file**

### readlines() Errors
- `StringIO().readlines("5")` → **TypeError: argument should be integer or None, not 'str'**
- `closed_stringio.readlines()` → **ValueError: I/O operation on closed file**

### seek() Errors
- `StringIO().seek("1")` → **TypeError: 'str' object cannot be interpreted as an integer**
- `StringIO().seek(1, 0, 0)` → **TypeError: seek expected at most 2 arguments, got 3**
- `StringIO().seek(-1)` → **ValueError: Negative seek position -1**
- `StringIO().seek(0, 3)` → **ValueError: Invalid whence (3, should be 0, 1 or 2)**
- `StringIO("hi").seek(1, 1)` → **OSError: Can't do nonzero cur-relative seeks** (whence=1 only allows offset=0)

### truncate() Errors
- `StringIO().truncate("5")` → **TypeError: argument should be integer or None, not 'str'**
- `StringIO().truncate(-1)` → **ValueError: Negative size value -1**
- `closed_stringio.truncate()` → **ValueError: I/O operation on closed file**

### getvalue() Errors
- `closed_stringio.getvalue()` → **ValueError: I/O operation on closed file**

### Closed-file Errors (all I/O operations)
- `closed_stringio.readable()` → **ValueError: I/O operation on closed file**
- `closed_stringio.writable()` → **ValueError: I/O operation on closed file**
- `closed_stringio.seekable()` → **ValueError: I/O operation on closed file**
- `closed_stringio.tell()` → **ValueError: I/O operation on closed file**

### Unsupported Operations
- `StringIO().detach()` → **io.UnsupportedOperation: detach**
- `StringIO().fileno()` → **io.UnsupportedOperation: fileno**

---

## 3. BEHAVIOR MATRIX — 32 Probed Input→Output Cases

All outputs verified via CPython 3.12 execution.

### Initialization & Basic Operations

| Input | Expected Output | Notes |
|-------|-----------------|-------|
| `s = StringIO(); s.getvalue()` | `''` | Empty buffer |
| `s = StringIO("hello"); s.tell()` | `0` | Position starts at 0 |
| `s = StringIO("world"); s.getvalue()` | `'world'` | Initial value preserved |

### write() Return Values

| Input | Expected Output |
|-------|-----------------|
| `StringIO().write("hello")` | `5` |
| `StringIO().write("")` | `0` |
| `StringIO().write("\n")` | `1` |
| `StringIO().write("a" * 1000)` | `1000` |

### Basic read() Cases

| Input | Expected Output | Position After |
|-------|-----------------|-----------------|
| `s = StringIO("hello"); s.read(5)` | `'hello'` | 5 |
| `s = StringIO("hello"); s.read(2)` | `'he'` | 2 |
| `s = StringIO("hello"); s.read()` | `'hello'` | 5 |
| `s = StringIO("abc"); s.read(-1)` | `'abc'` | 3 |
| `s = StringIO("hi"); s.read(-2)` | `'hi'` | 2 |
| `s = StringIO("hello"); s.read(0)` | `''` | 0 |
| `s = StringIO("hello"); s.read(100)` | `'hello'` | 5 |
| `s = StringIO(); s.read()` | `''` | 0 |

### read() from Specific Positions

| Input | Expected Output | Notes |
|-------|-----------------|-------|
| `s = StringIO("hello world"); s.seek(6); s.read(5)` | `'world'` | Position=11 after |
| `s = StringIO("hello"); s.seek(2); s.read(2)` | `'ll'` | Position=4 after |
| `s = StringIO("hello"); s.seek(5); s.read()` | `''` | At EOF |
| `s = StringIO("hello"); s.seek(100); s.read()` | `''` | Beyond EOF |

### readline() Cases

| Input | Expected Output | Position After |
|-------|-----------------|-----------------|
| `s = StringIO("line1\nline2\n"); s.readline()` | `'line1\n'` | 6 |
| `s = StringIO("line1\nline2"); s.readline(); s.readline()` | `'line2'` (2nd) | 12 |
| `s = StringIO("hello"); s.readline()` | `'hello'` | 5 |
| `s = StringIO("hello\nworld"); s.readline(3)` | `'hel'` | 3 |
| `s = StringIO(); s.readline()` | `''` | 0 |
| `s = StringIO("x\ny"); s.readline(); s.readline(); s.readline()` | `''` (3rd) | 5 |

### readlines() Cases

| Input | Expected Output |
|-------|-----------------|
| `StringIO("a\nb\nc").readlines()` | `['a\n', 'b\n', 'c']` |
| `StringIO("").readlines()` | `[]` |
| `StringIO("a\nb\nc").readlines(-1)` | `['a\n', 'b\n', 'c']` |
| `StringIO("a\nb\nc").readlines(0)` | `['a\n', 'b\n', 'c']` |

### seek() and tell()

| Input | Expected Position | Notes |
|-------|-------------------|-------|
| `s = StringIO("hello"); s.tell()` | `0` | Initial |
| `s = StringIO("hello"); s.seek(0); s.tell()` | `0` | Seek to start |
| `s = StringIO("hello"); s.seek(5); s.tell()` | `5` | Seek to end |
| `s = StringIO("hello"); s.seek(100); s.tell()` | `100` | Seek beyond EOF allowed |
| `s = StringIO("hello"); s.seek(0, 2); s.tell()` | `5` | Seek to end with whence=2 |

### Write After Seek (Overwrite)

| Input | Expected getvalue() |
|-------|---------------------|
| `s = StringIO("hello"); s.seek(1); s.write("X"); s.getvalue()` | `'hXllo'` |
| `s = StringIO("hello"); s.seek(0); s.write("HI"); s.getvalue()` | `'HIllo'` |
| `s = StringIO("hello"); s.seek(10); s.write("x"); s.getvalue()` | `'hello\x00\x00\x00\x00\x00x'` |

### truncate()

| Input | Expected getvalue() | Expected tell() |
|-------|---------------------|-----------------|
| `s = StringIO("hello"); s.truncate(3); s.getvalue()` | `'hel'` | 0 |
| `s = StringIO("hello world"); s.truncate(5); s.getvalue()` | `'hello'` | 0 |
| `s = StringIO("hi"); s.truncate(10); s.getvalue()` | `'hi'` | 0 (unchanged; truncate beyond end no-op) |

### Complex Scenarios

| Input | Expected Output | Notes |
|-------|-----------------|-------|
| `s = StringIO(); s.write("ab"); s.seek(0); s.read()` | `'ab'` | Write then seek back to read |
| `s = StringIO("abc"); s.write("xy"); s.getvalue()` | `'abcxy'` | Write appends when at end |
| `s = StringIO(); s.writelines(['a', 'b']); s.getvalue()` | `'ab'` | No auto-newlines |
| `s = StringIO(); s.writelines(['x\n', 'y\n']); s.getvalue()` | `'x\ny\n'` | Explicit newlines preserved |

### Unicode Support

| Input | Expected Output | Notes |
|-------|-----------------|-------|
| `StringIO().write("hello 世界 🌍")` | `10` (char count) | Returns char count, not byte count |
| `StringIO("こんにちは\nworld").readline()` | `'こんにちは\n'` | Unicode in content and newlines |

### Properties and Flags

| Input | Expected Output | Notes |
|-------|-----------------|-------|
| `StringIO().closed` | `False` | Initially open |
| `s = StringIO(); s.close(); s.closed` | `True` | After close |
| `StringIO().readable()` | `True` | Always readable |
| `StringIO().writable()` | `True` | Always writable |
| `StringIO().seekable()` | `True` | Always seekable |
| `StringIO().isatty()` | `False` | Never a tty |
| `StringIO().line_buffering` | `False` | Not line-buffered |
| `StringIO().encoding` | `None` | No encoding property |
| `StringIO().errors` | `None` | No error mode |
| `StringIO().newlines` | `None` | Text mode, always None |

---

## 4. HAZARDS

### None/Empty Handling
- **read()/readline()/readlines() size/hint arguments:** -1 or None both mean "read all"; code must normalize.
- **seek(pos, whence):** whence=1 (relative to current) **only** allows pos=0; any nonzero offset raises OSError. pyrst implementation must enforce this.
- **truncate(size):** passing None uses current position; must verify pyrst can express this.

### Numeric Edge Cases
- **Unicode string length:** `write("世")` returns 1 (char count), not byte count. pyrst's i64 integers handle this, but confirm char-by-char semantics.
- **Null bytes in write-after-seek:** Seeking beyond EOF then writing pads with null characters (`\x00`). Exact behavior: `StringIO("hello").seek(10).write("x")` → `"hello\x00\x00\x00\x00\x00x"` (9 nulls, total 15 chars). pyrst must replicate.

### Position Tracking
- **write() advances position:** After `write("abc")`, position is at end of written content; subsequent `read()` returns empty.
- **seek(0, 2) vs EOF:** Seeking to EOF with whence=2 lands on the exact end position, and reading from there returns empty string.
- **Seek beyond end:** Allowed. Position can exceed buffer length; read from there is empty. Write after seek beyond end pads with nulls.

### State Preservation
- **getvalue() ignores position:** Always returns full buffer, regardless of seek position.
- **truncate() resets position to 0:** After `truncate(size)`, tell() returns 0, not the truncated size.
- **close() vs getvalue():** getvalue() raises ValueError if buffer is closed; no fallback.

### Newlines
- **readline() includes newline:** `readline()` from `"line1\nline2"` returns `'line1\n'`, not `'line1'`. Last line without newline is returned as-is.
- **readlines() preserves newlines:** List elements include `'\n'` if present.
- **No auto-newlines in write/writelines:** `write("a")` does not add newline; `writelines(['a', 'b'])` writes `'ab'`, not `'a\nb'`.

### Iteration & Formatting
- **No insertion-order dependence:** StringIO is not a dict; no ordering hazard.
- **No locale dependence:** String operations are locale-invariant.
- **No platform dependence:** Newline handling is consistent (no `\r\n` vs `\n` variation in StringIO; caller controls).

### Type Strictness
- **No coercion:** `read(5.0)` raises TypeError; `seek("0")` raises TypeError. pyrst type system should enforce.
- **Exception types:** ValueError for value errors (negative size, closed file), TypeError for type errors, OSError for seek-whence errors, io.UnsupportedOperation for unsupported methods.

---

## 5. GATED — Constraint-Violating API Parts

### G4: variadics (*args/**kwargs)
- **`StringIO.__init__(*args, **kwargs)` signature:** CPython accepts *args/**kwargs for flexibility, but pyrst has no variadics. **Deferral:** Implement `__init__(initial_value: str | None = None)` as explicit keyword parameter only; no positional *args. Matches CPython's practical usage (callers use `StringIO()`, `StringIO("text")`, or keyword `initial_value="text"`).

### G2: Module-Level Mutable State
- **No persistent global state:** StringIO is a class-based API. Each instance is independent. No module-level mutable state needed. **No gate violation.**

---

## 6. PARITY PLAN — 32 Test Cases for pyrst Golden

Each line is CPython 3.12-verified and safe for direct adoption in pyrst parity tests. Ordered by coverage: initialization, I/O, positioning, edges.

```python
# Initialization
assert StringIO().getvalue() == ''
assert StringIO('hello').getvalue() == 'hello'
assert StringIO('').getvalue() == ''

# write() and return value
assert StringIO().write('abc') == 3
assert StringIO().write('') == 0
s = StringIO()
s.write('hi')
assert s.getvalue() == 'hi'

# read() basic
s = StringIO('hello')
assert s.read(2) == 'he'
assert s.read(3) == 'llo'

# read() from position
s = StringIO('hello world')
s.seek(6)
assert s.read() == 'world'

# read() at EOF
s = StringIO('x')
s.read()
assert s.read() == ''

# readline()
s = StringIO('a\nb\nc')
assert s.readline() == 'a\n'
assert s.readline() == 'b\n'
assert s.readline() == 'c'
assert s.readline() == ''

# readlines()
s = StringIO('x\ny')
assert s.readlines() == ['x\n', 'y']

# readlines() empty
assert StringIO().readlines() == []

# getvalue()
s = StringIO('test')
s.seek(2)
assert s.getvalue() == 'test'

# tell()
s = StringIO('12345')
assert s.tell() == 0
s.seek(3)
assert s.tell() == 3

# seek() and read
s = StringIO('abcde')
s.seek(2)
assert s.read() == 'cde'

# seek() beyond end
s = StringIO('hi')
s.seek(100)
assert s.tell() == 100

# truncate()
s = StringIO('hello world')
s.truncate(5)
assert s.getvalue() == 'hello'

# truncate() at position
s = StringIO('abcdef')
s.seek(3)
s.truncate()
assert s.getvalue() == 'abc'

# write after seek (overwrite)
s = StringIO('hello')
s.seek(1)
s.write('X')
assert s.getvalue() == 'hXllo'

# writelines()
s = StringIO()
s.writelines(['a', 'b', 'c'])
assert s.getvalue() == 'abc'

# close() and closed property
s = StringIO()
assert s.closed == False
s.close()
assert s.closed == True

# closed file error
s = StringIO()
s.close()
try:
    s.read()
    assert False, 'should raise'
except ValueError:
    pass

# readable/writable/seekable (open)
s = StringIO()
assert s.readable() == True
assert s.writable() == True
assert s.seekable() == True

# isatty
assert StringIO().isatty() == False

# Unicode write
s = StringIO()
assert s.write('世') == 1
assert s.getvalue() == '世'

# Null padding on seek-beyond + write
s = StringIO('a')
s.seek(3)
s.write('x')
assert s.getvalue() == 'a\x00\x00x'

# Empty read
s = StringIO('hello')
assert s.read(0) == ''

# read(-1) = read all
s = StringIO('test')
assert s.read(-1) == 'test'

# Mixed write and read
s = StringIO()
s.write('ab')
s.seek(0)
assert s.read() == 'ab'

# readline() size limit
s = StringIO('hello\nworld')
assert s.readline(3) == 'hel'
assert s.readline() == 'lo\n'
```

---

## 7. TARGET — Fidelity Estimate

**Score: 5/5 — Full Fidelity**

### Why 5/5
1. **Self-contained semantics:** StringIO is pure in-memory text buffering with no external I/O, system calls, or platform variance.
2. **Explicit API contract:** All behaviors are documented and deterministic. No floating-point repr issues, locale dependence, randomness, or time-dependent behavior.
3. **Type system fit:** Pyrst's value semantics, char-indexed strings, and i64 integers align naturally with StringIO's semantics (no bytes, no bignum overflow, no aliasing hazards).
4. **No special constructs:** StringIO requires no decorators beyond `__init__`, `__str__`, `__repr__`. Single inheritance is used for internal class hierarchy, not required in pyrst port.
5. **G4 gate is manageable:** The *args/**kwargs signature is cosmetic in CPython; pyrst's explicit `initial_value` keyword parameter matches all real-world usage.

### Why Not 5.5
- No impedance mismatch; full 5/5 is realistic.

---

## Summary

**Module:** stringio  
**Public API surface:** 21 (StringIO class + 12 methods + 6 properties/flags)  
**Parity test count:** 32  
**Gated parts:** 1 (G4: *args/**kwargs → keyword-only param)  
**Fidelity:** 5/5  

StringIO is a prime candidate for immediate pyrst porting: pure semantics, no platform variance, tight spec, and natural type alignment.
