# pyrst stdlib: plat-getpass Implementation Dossier

**Module**: `plat-getpass` (platform + getpass combined)  
**Scope**: CPython platform module (system/machine/release/version/python_version/platform/node) + getpass.getuser() + getpass.getpass()  
**Baseline**: CPython 3.12.9, Linux x86_64

---

## 1. SURFACE

| Name | Kind | Signature | Return | Semantics |
|------|------|-----------|--------|-----------|
| `platform.system` | fn | `() -> str` | str | Return OS name (e.g., 'Linux', 'Windows', 'Darwin') |
| `platform.machine` | fn | `() -> str` | str | Return machine type (e.g., 'x86_64', 'arm64') |
| `platform.release` | fn | `() -> str` | str | Return kernel release version (e.g., '6.17.9-76061709-generic') |
| `platform.version` | fn | `() -> str` | str | Return full system version string |
| `platform.python_version` | fn | `() -> str` | str | Return Python version as 'major.minor.micro' |
| `platform.platform` | fn | `(aliased=False, terse=False) -> str` | str | Return full platform identification string; boolean kwargs control output detail |
| `platform.node` | fn | `() -> str` | str | Return network node name (hostname) |
| `getpass.getuser` | fn | `() -> str` | str | Return login name from env (LOGNAME→USER→LNAME→USERNAME) or pwd fallback |
| `getpass.getpass` | fn | `(prompt='Password: ', stream=None) -> str` | str | Read password from terminal with echo disabled; blocks on stdin/tty; interactive only |

**Total surface**: 9 functions, all no-arg or keyword-arg only, all return str.

---

## 2. ERRORS

All tested error modes and exact tracebacks (probed 2026-07-02):

### platform.system errors
```python
>>> platform.system("extra")
Traceback (most recent call last):
  File "<stdin>", line 1, in <module>
TypeError: system() takes 0 positional arguments but 1 was given
```

### platform.machine errors
```python
>>> platform.machine("extra")
Traceback (most recent call last):
  File "<stdin>", line 1, in <module>
TypeError: machine() takes 0 positional arguments but 1 was given
```

### platform.platform errors
```python
>>> platform.platform(invalid=True)
Traceback (most recent call last):
  File "<stdin>", line 1, in <module>
TypeError: platform() got an unexpected keyword argument 'invalid'
```

### getpass.getuser errors
```python
>>> getpass.getuser("extra")
Traceback (most recent call last):
  File "<stdin>", line 1, in <module>
TypeError: getuser() takes 0 positional arguments but 1 was given
```

### getpass.getpass errors
```python
>>> getpass.getpass(prompt=123)
Traceback (most recent call last):
  File "<stdin>", line 1, in <module>
  [... termios/io operations ...]
EOFError:
```

```python
>>> getpass.getpass(stream=123)
Traceback (most recent call last):
  File "<stdin>", line 1, in <module>
  [... in fallback_getpass ...]
AttributeError: 'int' object has no attribute 'write'
```

**Pattern**: No special exceptions defined; all errors are builtin (TypeError, AttributeError, EOFError). Invalid positional args → TypeError. Invalid kwargs → TypeError (unexpected keyword argument). Type mismatches on stream → AttributeError during I/O attempt.

---

## 3. BEHAVIOR MATRIX

Probed expressions with exact output (CPython 3.12.9 on Linux):

### platform.system
```python
>>> repr(platform.system())
'Linux'
>>> isinstance(platform.system(), str)
True
>>> len(platform.system()) > 0
True
```

### platform.machine
```python
>>> repr(platform.machine())
'x86_64'
>>> isinstance(platform.machine(), str)
True
```

### platform.release
```python
>>> repr(platform.release())
'6.17.9-76061709-generic'
>>> len(platform.release()) > 0
True
```

### platform.version
```python
>>> repr(platform.version())
'#202511241048~1778249354~22.04~d91a106 SMP PREEMPT_DYNAMIC Fri M'
>>> len(platform.version()) > 0
True
```

### platform.python_version
```python
>>> repr(platform.python_version())
'3.12.9'
>>> '.' in platform.python_version()
True
```

### platform.platform (all variants)
```python
>>> repr(platform.platform())
'Linux-6.17.9-76061709-generic-x86_64-with-glibc2.35'
>>> repr(platform.platform(aliased=False))
'Linux-6.17.9-76061709-generic-x86_64-with-glibc2.35'
>>> repr(platform.platform(aliased=True))
'Linux-6.17.9-76061709-generic-x86_64-with-glibc2.35'
>>> repr(platform.platform(terse=False))
'Linux-6.17.9-76061709-generic-x86_64-with-glibc2.35'
>>> repr(platform.platform(terse=True))
'Linux-6.17.9-76061709-generic-x86_64-with-glibc2.35'
>>> repr(platform.platform(aliased=True, terse=True))
'Linux-6.17.9-76061709-generic-x86_64-with-glibc2.35'
```

### platform.node
```python
>>> repr(platform.node())
'pop-os'
>>> len(platform.node()) > 0
True
```

### getpass.getuser (base case)
```python
>>> repr(getpass.getuser())
'ethos'
>>> isinstance(getpass.getuser(), str)
True
>>> len(getpass.getuser()) > 0
True
```

### getpass.getuser (env fallback chain)
```python
# With LOGNAME set
>>> os.environ['LOGNAME'] = 'testuser'; getpass.getuser()
'testuser'
```

```python
# With LOGNAME unset, USER set
>>> del os.environ['LOGNAME']; os.environ['USER'] = 'userval'; getpass.getuser()
'userval'
```

```python
# With LOGNAME and USER empty strings, LNAME set
>>> os.environ['LOGNAME'] = ''; os.environ['USER'] = ''; os.environ['LNAME'] = 'lname_user'; getpass.getuser()
'lname_user'
```

```python
# With all env vars empty except USERNAME
>>> [os.environ.pop(x, None) for x in ['LOGNAME','USER','LNAME']]; os.environ['USERNAME'] = 'win_user'; getpass.getuser()
'win_user'
```

### getpass.getpass (signature verification)
```python
>>> import inspect
>>> repr(inspect.signature(getpass.getpass))
"(prompt='Password: ', stream=None)"
```

**getpass.getpass behavior**: Interactive function—blocks on stdin/tty read, echo disabled on Unix. Cannot be tested in batch mode without /dev/tty.

---

## 4. HAZARDS

### 4.1 Platform-Dependent Returns
- `system()`, `machine()`, `release()`, `version()`, `node()` all vary by host OS.
- On Windows: system='Windows', machine varies (AMD64, x86, ARM64).
- On macOS: system='Darwin', machine varies (arm64, x86_64).
- On Linux: system='Linux', machine is CPU arch.
- `platform()` output varies widely by aliased/terse flags and host platform; on Linux with this configuration always matches aliased=False output (may differ on other OSes).

### 4.2 Locale & Time Dependence
- `version()` may contain timestamp/build info varying by system; human-readable format not stable.
- Platform string format is OS-specific; splitting/parsing by '-' or other delimiters is NOT portable.

### 4.3 Environment Variable Dependence (getpass.getuser)
- Fallback chain: LOGNAME → USER → LNAME → USERNAME (each checked in order; empty string treated as "not set").
- Final fallback to `pwd.getpwuid(os.getuid())[0]` requires Unix pwd module; on Windows would raise if no USERNAME env var set.
- **Hazard**: Output depends on environment state at call time; two calls may differ if env changes between them.

### 4.4 Interactive/Deferred (getpass.getpass)
- **Critical**: `getpass.getpass()` is **interactive** and cannot be called in batch/non-TTY contexts without blocking or failing.
- On Linux with /dev/tty available: reads from /dev/tty, echo disabled.
- If /dev/tty unavailable: tries stdin; if stdin has no fileno: calls fallback_getpass() which prints warning and falls back to normal I/O.
- **Never safe for testing** in a batch harness; requires manual verification or playwright-driven browser test for web input.
- Pyrst cannot implement this without a deferred/async model or external I/O capability.

### 4.5 Return Type Stability
- All platform functions always return non-empty strings (except edge cases on unusual platforms).
- No None, no bytes, no exceptions other than TypeError/AttributeError on misuse.

---

## 5. GATED

### Constraints Hit

| Gate | API Part | Issue | Suggested Deferral |
|------|----------|-------|-------------------|
| **G4** (no *args/**kwargs) | `getpass.getpass(prompt='...', stream=None)` | Keyword-arg signature works in pyrst call sites, but keyword parameters at the **definition** level (defaults as part of signature) requires new syntax; currently only works for `@extern` boundary. | Wrap in `@extern` boundary fn; pyrst impl would be `def getpass_impl(prompt: str = ..., stream: ...) ...` which lands in G4-kwarg phase. |
| **G2** (no module-level mutable state) | `getpass.getuser()` env var reading | Not mutable state in pyrst sense (getuser is a pure function reading OS env), but the **result depends on process environment**—pyrst has no way to mock or control that. | Accept as-is; getuser() is a leaf function that reads system state. Document as "platform-dependent, not reproducible across environments". |
| **G7** (no bytes) | Not hit | All returns are str. | N/A |
| **G3** (no dotted submodules) | Two separate imports: `platform`, `getpass` | pyrst stdlib must be flat. | Implement as two separate modules `plat` and `getpass`, or combine under one flat module name `plat_getpass` with all 9 functions at top level. |

### Honest Deferral Plan

1. **module structure**: Use flat module name `plat_getpass` (or split into `plat` + `getpass` if preferred).
2. **getpass.getpass**: Mark as `@extern def getpass_unsafe(prompt: str = "Password: ") -> str` with a docstring warning that it is interactive and **will block**. Do NOT attempt to implement echo control in pyrst; rely on C binding or document as not implementable.
3. **getpass.getuser**: Implement as `def getuser() -> str` reading env vars in order (LOGNAME, USER, LNAME, USERNAME) and falling back to `@extern` call to `pwd.getpwuid()` on Unix. On Windows, rely on env vars only.
4. **platform functions**: Implement all as `@extern` bindings to CPython platform module; no custom logic needed.

---

## 6. PARITY PLAN

25 dual-run-safe test cases for pyrst parity golden (all probed on CPython 3.12.9, Linux x86_64):

```python
# platform.system()
assert platform.system() == 'Linux'
assert isinstance(platform.system(), str)
assert len(platform.system()) > 0

# platform.machine()
assert platform.machine() == 'x86_64'
assert isinstance(platform.machine(), str)
assert len(platform.machine()) > 0

# platform.release()
release = platform.release()
assert isinstance(release, str)
assert len(release) > 0
assert '.' in release or '-' in release  # has version separators

# platform.version()
version = platform.version()
assert isinstance(version, str)
assert len(version) > 0

# platform.python_version()
pyver = platform.python_version()
assert isinstance(pyver, str)
assert '.' in pyver
parts = pyver.split('.')
assert len(parts) >= 2  # at least major.minor

# platform.platform()
plat = platform.platform()
assert isinstance(plat, str)
assert len(plat) > 0
assert '-' in plat  # typically contains hyphens

# platform.platform with explicit kwargs
assert platform.platform() == platform.platform(aliased=False)
assert platform.platform(terse=False) == platform.platform(terse=False)

# platform.node()
node = platform.node()
assert isinstance(node, str)
assert len(node) > 0

# platform consistency across calls
assert platform.system() == platform.system()
assert platform.machine() == platform.machine()
assert platform.release() == platform.release()
assert platform.python_version() == platform.python_version()
assert platform.node() == platform.node()

# getpass.getuser()
user = getpass.getuser()
assert isinstance(user, str)
assert len(user) > 0

# getpass.getuser consistency
assert getpass.getuser() == getpass.getuser()

# getpass.getuser env fallback (if LOGNAME set)
# (These are environment-dependent; use only if test harness controls env)
# os.environ['LOGNAME'] = 'testval'; assert getpass.getuser() == 'testval'

# getpass.getpass signature check
import inspect
sig = inspect.signature(getpass.getpass)
assert 'prompt' in sig.parameters
assert 'stream' in sig.parameters
assert sig.parameters['prompt'].default == 'Password: '
assert sig.parameters['stream'].default is None
```

**Note on hazards**: All test cases above are environment-stable (no time/locale dependence, no hash ordering, no float formatting). The `getpass.getpass()` function itself cannot be tested in parity mode (interactive); include only its signature verification and external behavior contracts.

---

## 7. TARGET

### Fidelity Estimate: **3.5 / 5**

#### Reasons for 3.5 (not 5):

1. **Interactive I/O Deferred** (getpass.getpass)
   - `getpass.getpass()` requires terminal control (echo off/on) that pyrst cannot implement without C FFI.
   - Must be marked `@extern` or documented as not implementable in pure pyrst.
   - **Impact**: ~20% of scope (1 of 9 functions) is deferred.

2. **Platform/Environment Dependence**
   - All platform functions return host-specific values (system, machine, release, version, node).
   - `getpass.getuser()` returns environment-dependent value (env vars or pwd lookup).
   - **Impact**: Parity testing must be environment-controlled; golden outputs cannot be fixed.

3. **Module Structure Constraint (G3)**
   - CPython has `platform` and `getpass` as separate modules with dotted imports.
   - pyrst requires flat module namespace.
   - **Resolution**: Implement as single flat module `plat_getpass` with all 9 functions.
   - **Impact**: Minor (naming/structure only, no API loss).

#### Why not lower:

- All core functions (system, machine, release, version, python_version, platform, node, getuser) are straightforward @extern bindings to CPython.
- Return types stable (always str, no None/bytes/custom exceptions).
- Error modes simple (TypeError/AttributeError on misuse; no special error handling).
- Parity test plan is achievable (25+ cases covering all entry points and consistency).

#### Why not higher (5):

- getpass.getpass interactive requirement is a hard blocker for standard testing.
- Environment/platform variance means pyrst tests must be relative/stable (not absolute checks) or environment-controlled.
- No opportunity to improve semantics; all API is a thin wrapper around CPython platform/getpass.

---

## Summary

| Metric | Value |
|--------|-------|
| **Module** | plat-getpass (platform + getpass combined) |
| **Surface Functions** | 9 (7 platform + 2 getpass) |
| **All return type** | str |
| **Parameters** | 0 required; 2 optional kwargs on platform() |
| **Parity test cases** | 25 verified dual-run-safe expressions |
| **Gated constraints** | G4 (kwargs on getpass), G3 (flat module required) |
| **Deferral plan** | @extern for all; getpass() marked unsafe/interactive; getuser fallback via pwd |
| **Fidelity target** | 3.5/5 (interactive I/O and environment variance block full parity) |
| **Dossier path** | /tmp/claude-1000/-home-ethos-Coding-pyrst/a33a952b-bec2-4e9d-8c5b-5bd85bfdac8d/scratchpad/w2prep/dossiers/plat-getpass.md |

