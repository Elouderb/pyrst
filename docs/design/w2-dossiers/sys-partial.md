# sys-partial Implementation Dossier

**Module**: `sys` (partial scope)  
**Scope**: `sys.maxsize`, `sys.version`, `sys.version_info`, `sys.platform`, `sys.exit()`  
**Target CPython**: 3.12.9 (linux, x86_64)

---

## 1. SURFACE

| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `maxsize` | const | — | `int` | Maximum size of i64 container (2^63 - 1 = 9223372036854775807 on 64-bit) |
| `version` | const | — | `str` | Full version string including build info & compiler |
| `version_info` | const | — | `sys.version_info` | Tuple-like: (major, minor, micro, releaselevel, serial) with named attributes |
| `version_info.major` | attribute | — | `int` | Major version number (3) |
| `version_info.minor` | attribute | — | `int` | Minor version number (12) |
| `version_info.micro` | attribute | — | `int` | Micro version number (9) |
| `version_info.releaselevel` | attribute | — | `str` | Release level: 'alpha', 'beta', 'candidate', 'final' |
| `version_info.serial` | attribute | — | `int` | Serial number (0 for final, increments for pre-releases) |
| `platform` | const | — | `str` | System platform identifier ('linux', 'darwin', 'win32', etc.) |
| `exit` | fn | `exit(code: int \| str \| None = None) -> !` | — | Raise SystemExit with given code; None or no arg → code=None |

---

## 2. ERRORS

All edge cases verified via CPython 3.12.9 probe.

### 2.1 sys.exit() exception mapping

| Input | Probe | Raised Exception | code attribute |
|-------|-------|------------------|-----------------|
| `sys.exit()` | `sys.exit()` | `SystemExit` | `None` |
| `sys.exit(None)` | `sys.exit(None)` | `SystemExit` | `None` |
| `sys.exit(0)` | `sys.exit(0)` | `SystemExit` | `0` |
| `sys.exit(1)` | `sys.exit(1)` | `SystemExit` | `1` |
| `sys.exit(-1)` | `sys.exit(-1)` | `SystemExit` | `-1` |
| `sys.exit(255)` | `sys.exit(255)` | `SystemExit` | `255` |
| `sys.exit("")` | `sys.exit("")` | `SystemExit` | `""` |
| `sys.exit("error")` | `sys.exit("error")` | `SystemExit` | `"error"` |

### 2.2 version_info edge cases

| Input | Probe | Result | Notes |
|-------|-------|--------|-------|
| Access nonexistent attr | `sys.version_info.nonexistent` | AttributeError: `'sys.version_info' object has no attribute 'nonexistent'` | Named tuple validates attributes |
| Out-of-bounds indexing | `sys.version_info[5]` | IndexError: tuple index out of range | Length is always 5 |
| Negative indexing | `sys.version_info[-1]` | `0` (serial) | Standard tuple negative indexing works |

### 2.3 version string edge cases

| Input | Probe | Result | Notes |
|-------|-------|--------|-------|
| `sys.version[:30]` | `sys.version[:30]` | `'3.12.9 (main, Apr  9 2025, 15:'` | Slicing works, platform-dependent after first part |
| `len(sys.version)` | `len(sys.version)` | `49` | Varies by build/compiler; includes build timestamp & compiler version |

---

## 3. BEHAVIOR MATRIX

Comprehensive input→output pairs from CPython 3.12.9 probes.

### 3.1 sys.maxsize

```python
sys.maxsize                                 # 9223372036854775807
sys.maxsize == 2**63 - 1                    # True
sys.maxsize == 9223372036854775807          # True
sys.maxsize > 0                             # True
sys.maxsize < 2**64                         # True
type(sys.maxsize).__name__                  # 'int'
```

### 3.2 sys.version

```python
sys.version[:30]                            # '3.12.9 (main, Apr  9 2025, 15:'
len(sys.version)                            # 49
isinstance(sys.version, str)                # True
'GCC' in sys.version or 'Clang' in sys.version  # True (compiler info present)
sys.version.split('\n')[0]                  # '3.12.9 (main, Apr  9 2025, 15:59:41) [GCC 11.4.0]'
```

### 3.3 sys.version_info structure and access

```python
sys.version_info.major                      # 3
sys.version_info.minor                      # 12
sys.version_info.micro                      # 9
sys.version_info.releaselevel               # 'final'
sys.version_info.serial                     # 0
sys.version_info[0]                         # 3
sys.version_info[1]                         # 12
sys.version_info[2]                         # 9
sys.version_info[3]                         # 'final'
sys.version_info[4]                         # 0
len(sys.version_info)                       # 5
list(sys.version_info)                      # [3, 12, 9, 'final', 0]
sys.version_info[0] == sys.version_info.major  # True
```

### 3.4 sys.version_info comparison and iteration

```python
sys.version_info >= (3, 0, 0)               # True
sys.version_info >= (3, 12, 0)              # True
sys.version_info >= (3, 12, 10)             # False
sys.version_info >= (3, 11, 0)              # True
sys.version_info < (4, 0, 0)                # True
sys.version_info == (3, 12, 9, 'final', 0)  # True (with full tuple)
tuple(sys.version_info)                     # (3, 12, 9, 'final', 0)
```

### 3.5 sys.platform

```python
sys.platform                                # 'linux'
isinstance(sys.platform, str)               # True
len(sys.platform)                           # 5
sys.platform == 'linux'                     # True
sys.platform in ['linux', 'darwin', 'win32'] # True (common platforms)
```

### 3.6 sys.exit() behavior — code parameter propagation

```python
# Trapped as SystemExit exception, code attribute preserved:
try:
    sys.exit(0)
except SystemExit as e:
    e.code                                  # 0

try:
    sys.exit(1)
except SystemExit as e:
    e.code                                  # 1

try:
    sys.exit(127)
except SystemExit as e:
    e.code                                  # 127

try:
    sys.exit(-1)
except SystemExit as e:
    e.code                                  # -1

try:
    sys.exit("")
except SystemExit as e:
    e.code                                  # ''

try:
    sys.exit("fatal error")
except SystemExit as e:
    e.code                                  # 'fatal error'

try:
    sys.exit()
except SystemExit as e:
    e.code                                  # None

try:
    sys.exit(None)
except SystemExit as e:
    e.code                                  # None
```

### 3.7 sys.exit() — SystemExit exception properties

```python
try:
    sys.exit(42)
except SystemExit as e:
    isinstance(e, BaseException)            # True
    isinstance(e, Exception)                # False (BaseException only)
    issubclass(SystemExit, BaseException)   # True
    issubclass(SystemExit, Exception)       # False
    type(e).__name__                        # 'SystemExit'
    e.code                                  # 42
```

---

## 4. HAZARDS

### 4.1 Platform-dependent behavior

**Flag**: `sys.platform` and `sys.version` contain platform/compiler-specific strings.

- `sys.platform` varies: `'linux'`, `'darwin'` (macOS), `'win32'` (Windows), `'cygwin'`, etc.
- `sys.version` includes build timestamp and compiler version, so exact string is unreliable.
- **Mitigation**: Test only semantic parts (version_info tuple structure, maxsize magnitude); avoid string-equality assertions on `version` or `platform`.

### 4.2 Immutability and value equality

**Flag**: `sys.maxsize` and `sys.platform` are immutable; comparison by value is safe, not by identity.

- Probe shows `sys.maxsize == 2**63 - 1` is the reliable check, not string parsing.
- `sys.platform == 'linux'` is safe; avoid `sys.platform is 'linux'`.

### 4.3 version_info tuple-like semantics

**Flag**: `sys.version_info` is a named tuple, not a plain tuple; both indexing and attribute access work.

- Indexing: `version_info[0]`, `version_info[1]`, etc. — indices 0-4.
- Attributes: `version_info.major`, `.minor`, `.micro`, `.releaselevel`, `.serial`.
- Comparison works with tuple literals: `version_info >= (3, 12, 0)`.
- **Mitigation**: Pyrst namedtuple support must ensure both indexing and named-attribute access work.

### 4.4 SystemExit exception hierarchy

**Flag**: `SystemExit` is a `BaseException`, NOT an `Exception`.

- `except Exception: ...` will NOT catch `SystemExit`.
- Must use `except BaseException:` or `except SystemExit:`.
- **Mitigation**: Pyrst's exception hierarchy must include SystemExit as direct BaseException subclass, not under Exception.

### 4.5 sys.exit() with no arguments vs. None

**Flag**: `sys.exit()` and `sys.exit(None)` both set `.code = None`.

- Default parameter is optional but semantically equivalent to `None`.
- Pyrst function signature: `def exit(code: int | str | None = None)` or `def exit(code: int | str | None = ...)?`.

---

## 5. GATED

Parts of sys module that hit pyrst constraints:

| Gate | API Part | Constraint | Issue | Suggested Deferral |
|------|----------|-----------|-------|-------------------|
| **G2** | `sys.argv` | Module-level mutable state | Pyrst prohibits module-level mutable state; `argv` is a list modified by runtime | **Defer** — do not expose sys.argv in pyrst sys-partial |
| **G2** | `sys.stdin` | Module-level mutable file object | Mutable I/O stream state | **Defer** — do not expose sys.stdin in pyrst sys-partial |
| **G2** | `sys.stdout` | Module-level mutable file object | Mutable I/O stream state | **Defer** — do not expose sys.stdout in pyrst sys-partial |
| **G2** | `sys.stderr` | Module-level mutable file object | Mutable I/O stream state | **Defer** — do not expose sys.stderr in pyrst sys-partial |
| **G3** | dotted submodules | N/A for sys | sys is flat, no submodules like `os.path` | **Not applicable** — sys has no submodules |
| **G4** | `sys.exit(*args)` | Variadic positional args | Pyrst prohibits `*args` | **Not applicable** — sys.exit takes single keyword arg `code`, not variadic |

**Summary**: Only G2 (mutable state) gates apply. The four I/O streams are deferred; all in-scope parts (maxsize, version, version_info, platform, exit) have no gated constraints.

---

## 6. PARITY PLAN

Dual-run-safe test cases (Python3 baseline → Pyrst parity).

All avoid platform-dependent string parsing, locale, or formatting hazards.

```python
# Maxsize tests (value, no string hazards)
sys.maxsize == 9223372036854775807                          # True
sys.maxsize == 2**63 - 1                                    # True
sys.maxsize > 0                                             # True
sys.maxsize < 2**64                                         # True

# version_info attribute access (named tuple)
sys.version_info.major == 3                                 # True
sys.version_info.minor >= 12                                # True
sys.version_info.micro >= 0                                 # True
isinstance(sys.version_info.releaselevel, str)              # True
sys.version_info.serial >= 0                                # True

# version_info indexing (tuple-like)
sys.version_info[0] == 3                                    # True
sys.version_info[1] >= 12                                   # True
sys.version_info[2] >= 0                                    # True
isinstance(sys.version_info[3], str)                        # True
sys.version_info[4] >= 0                                    # True
len(sys.version_info) == 5                                  # True

# version_info comparison
sys.version_info >= (3, 0, 0)                               # True
sys.version_info < (4, 0, 0)                                # True
sys.version_info >= (3, 12, 0)                              # True

# version_info tuple conversion
tuple(sys.version_info) == (3, 12, 9, 'final', 0)          # True (may vary by build)
list(sys.version_info) == [3, 12, 9, 'final', 0]           # True (may vary by build)

# version string (structural checks only, not content)
isinstance(sys.version, str)                                # True
len(sys.version) > 20                                       # True
'.' in sys.version                                          # True

# platform string (semantic, no equality)
isinstance(sys.platform, str)                               # True
len(sys.platform) > 0                                       # True
sys.platform in ['linux', 'darwin', 'win32', 'cygwin']      # True (on common platforms)

# sys.exit() → SystemExit (code preservation)
try:
    sys.exit(0)
except SystemExit as e:
    e.code == 0                                             # True

try:
    sys.exit(42)
except SystemExit as e:
    e.code == 42                                            # True

try:
    sys.exit(-1)
except SystemExit as e:
    e.code == -1                                            # True

try:
    sys.exit("error")
except SystemExit as e:
    e.code == "error"                                       # True

try:
    sys.exit()
except SystemExit as e:
    e.code is None                                          # True

try:
    sys.exit(None)
except SystemExit as e:
    e.code is None                                          # True

# SystemExit exception hierarchy
issubclass(SystemExit, BaseException)                       # True
not issubclass(SystemExit, Exception)                       # True
try:
    sys.exit(1)
except BaseException as e:
    type(e).__name__ == 'SystemExit'                        # True

# version_info releaselevel is str
isinstance(sys.version_info.releaselevel, str)              # True
sys.version_info.releaselevel in ['alpha', 'beta', 'candidate', 'final']  # True
```

**Count**: 40 parity cases

---

## 7. TARGET

**Fidelity Estimate**: **4 / 5**

### Rationale

#### Why not 5:

1. **SystemExit exception hierarchy complexity** (1 point)
   - Pyrst currently has no custom exception classes; SystemExit must be a builtin class in the exception hierarchy.
   - Requires careful integration into Pyrst's exception design (BaseException vs. Exception).
   - Feasible but requires non-trivial exception infrastructure.

2. **Named tuple `version_info` structure** (0.5 point)
   - `sys.version_info` combines tuple semantics (indexing, iteration, comparison) with named attributes (`.major`, `.minor`, etc.).
   - Pyrst lacks first-class named tuple support or dataclass infrastructure; implementing a bespoke `version_info` type is straightforward but non-standard.
   - Could alias to a plain tuple but lose `.major` et al. convenience.

#### Why not lower:

- **In-scope APIs are semantically simple**: `maxsize` is a literal i64 constant (perfect fit). `version` and `platform` are plain strings. `exit()` is a single-parameter function with clear semantics.
- **No G4 (variadics) or G9 (bignum) constraints**: sys.exit takes one optional arg, not `*args`. maxsize fits exactly in i64 without overflow handling.
- **No G3 (dotted submodules)**: sys is flat.
- **G2 deferral is clean**: argv/stdin/stdout/stderr are clearly gated as mutable state and can be honestly deferred without affecting core sys identity.

### Dominant reasons it isn't 5:

1. **SystemExit as a bona fide exception class** — requires exception hierarchy design and builtin class support.
2. **version_info named-tuple hybrid** — requires both tuple indexing AND named-attribute access; Pyrst's type system may not have idiomatic support.
3. **I/O stream deferral** — while honest and appropriate, it does mean the pyrst sys module is a partial compatibility shim, not full parity.

---

## Implementation Notes

- **Const declaration**: `maxsize`, `version`, `platform`, `version_info` are immutable constants at module level (allowed by Pyrst).
- **SystemExit definition**: Must be a builtin exception class inheriting directly from `BaseException` with a `.code: int | str | None` attribute.
- **version_info type**: Implement as a readonly named tuple with 5 fields; support both indexing and attribute access.
- **exit() signature**: `def exit(code: int | str | None = None) -> !` (diverges, never returns).
- **Constant values**: On 64-bit systems, populate at build/init time via CPython introspection or hardcode; version strings should reflect pyrst identity, not CPython.
- **Platform mapping**: Decide whether `sys.platform` in pyrst reports the actual OS platform or a static `'pyrst'` identifier for honest identity (recommended).

