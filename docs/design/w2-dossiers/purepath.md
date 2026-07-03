# pyrst stdlib purepath dossier

## 1. SURFACE

| Name | Kind | Signature | Return | Semantics |
|------|------|-----------|--------|-----------|
| PurePosixPath | class | `(self, *args)` | PurePosixPath | Pure path object; normalizes and immutably represents Posix filesystem paths |
| parts | property | `(self)` | tuple[str,...] | Tuple of path components; root as first element if absolute; empty if path is empty |
| name | property | `(self)` | str | Final path component (filename); empty string for root or empty path |
| stem | property | `(self)` | str | Final component without its last suffix; handles leading dots as part of name, not suffix |
| suffix | property | `(self)` | str | Last file extension (e.g., '.txt', '.gz'); empty if none; only last dot-separated component |
| suffixes | property | `(self)` | list[str] | All dot-separated suffixes in reverse order, excluding leading dots; e.g. file.tar.gz → ['.tar', '.gz'] |
| parent | property | `(self)` | PurePosixPath | Direct parent directory; root's parent is root; empty/relative paths' parent is '.' |
| parents | property | `(self)` | _PathParents | Sequence of ancestor paths from immediate parent to root; indexable and iterable; len() supported |
| is_absolute | method | `(self) -> bool` | bool | True if path starts with '/' or '//' (network path); False otherwise |
| as_posix | method | `(self) -> str` | str | String representation using forward slashes (identity on PurePosixPath) |
| joinpath | method | `(self, *pathsegments) -> PurePosixPath` | PurePosixPath | Concatenate path segments; stops at first absolute path argument |
| with_name | method | `(self, name: str) -> PurePosixPath` | PurePosixPath | Return path with final component replaced; raises ValueError if name empty or contains '/' or if current name empty |
| with_suffix | method | `(self, suffix: str) -> PurePosixPath` | PurePosixPath | Return path with suffix replaced (last dot-separated component); raises ValueError if current name empty; suffix can be multi-part (e.g. '.tar.gz') |
| with_stem | method | `(self, stem: str) -> PurePosixPath` | PurePosixPath | Return path with stem replaced; preserves existing suffix; works by replacing all but final suffix |
| relative_to | method | `(self, other, /, *_deprecated, walk_up=False) -> PurePosixPath` | PurePosixPath | Return relative path from other to self; raises ValueError if not possible unless walk_up=True; walk_up=True allows .. traversal |
| match | method | `(self, path_pattern: str, *, case_sensitive=None) -> bool` | bool | Match path against glob pattern; case_sensitive defaults to platform default; raises ValueError for empty pattern |
| is_relative_to | method | `(self, other) -> bool` | bool | Return True if self is relative to (under) other; False otherwise; does not raise |

---

## 2. ERRORS

### Empty pattern in match
```
>>> pathlib.PurePosixPath('/usr/local').match('')
ValueError: empty pattern
```

### Invalid name in with_name
```
>>> pathlib.PurePosixPath('/usr/local/file.txt').with_name('')
ValueError: Invalid name ''

>>> pathlib.PurePosixPath('/usr/local/file.txt').with_name('new/file.txt')
ValueError: Invalid name 'new/file.txt'
```

### Empty name in with_suffix or with_stem
```
>>> pathlib.PurePosixPath('/').with_suffix('.txt')
ValueError: PurePosixPath('/') has an empty name

>>> pathlib.PurePosixPath('.').with_suffix('.txt')
ValueError: PurePosixPath('.') has an empty name
```

### with_name on root
```
>>> pathlib.PurePosixPath('/').with_name('newname')
ValueError: "PurePosixPath('/') has an empty name"
```

### relative_to: not in subpath
```
>>> pathlib.PurePosixPath('/usr/local/bin').relative_to('/home/user')
ValueError: "'/usr/local/bin' is not in the subpath of '/home/user'"

>>> pathlib.PurePosixPath('/a/b').relative_to('/a/b/c')
ValueError: "'/a/b' is not in the subpath of '/a/b/c'"
```

### joinpath: type error
```
>>> pathlib.PurePosixPath('/usr').joinpath(123)
TypeError: "argument should be a str or an os.PathLike object where __fspath__ returns a str, not 'int'"
```

### with_name: type error
```
>>> pathlib.PurePosixPath('/usr/local/file.txt').with_name(123)
TypeError: "argument of type 'int' is not iterable"
```

---

## 3. BEHAVIOR MATRIX

### Initialization and basic properties

```
PurePosixPath('/') → parts=('/',), name='', stem='', suffix='', suffixes=[], is_absolute=True
PurePosixPath('/usr') → parts=('/', 'usr'), name='usr', stem='usr', suffix='', suffixes=[]
PurePosixPath('/usr/local/bin/python3.12') → parts=('/', 'usr', 'local', 'bin', 'python3.12'), name='python3.12', stem='python3', suffix='.12', suffixes=['.12']
PurePosixPath('relative/path/file.tar.gz') → parts=('relative', 'path', 'file.tar.gz'), name='file.tar.gz', stem='file.tar', suffix='.gz', suffixes=['.tar', '.gz']
PurePosixPath('file.txt') → parts=('file.txt',), name='file.txt', stem='file', suffix='.txt', suffixes=['.txt']
PurePosixPath('.hidden') → parts=('.hidden',) , name='.hidden', stem='.hidden', suffix='', suffixes=[]
PurePosixPath('.hidden.txt') → parts=('.hidden.txt',), name='.hidden.txt', stem='.hidden', suffix='.txt', suffixes=['.txt']
PurePosixPath('') → parts=(), name='', is_absolute=False
PurePosixPath('.') → parts=(), name='', parent='.'
PurePosixPath('..') → parts=('..',), name='..', stem='..', suffix='', suffixes=[]
PurePosixPath('/path/with/trailing/slash/') → parts=('/', 'path', 'with', 'trailing', 'slash'), name='slash'
PurePosixPath('//network/path') → parts=('//', 'network', 'path'), anchor='//'
```

### Multiple arguments to constructor

```
PurePosixPath('/usr', 'local', 'bin') → str='/usr/local/bin'
PurePosixPath('/base', 'relative/path', 'file.txt') → str='/base/relative/path/file.txt'
PurePosixPath('/usr', '/override') → str='/override'
PurePosixPath(PurePosixPath('/usr'), 'local', 'bin') → str='/usr/local/bin'
```

### parent and parents

```
PurePosixPath('/a/b/c/d').parent → '/a/b/c'
PurePosixPath('/a/b/c/d').parents[0] → '/a/b/c'
PurePosixPath('/a/b/c/d').parents[1] → '/a/b'
PurePosixPath('/a/b/c/d').parents[-1] → '/'
PurePosixPath('/a/b/c/d').parents list → ['/a/b/c', '/a/b', '/a', '/']
PurePosixPath('/a/b/c/d') len(parents) → 4
PurePosixPath('/usr').parent → '/'
PurePosixPath('/').parent → '/'
PurePosixPath('file.txt').parent → '.'
PurePosixPath('.').parent → '.'
PurePosixPath('/').parents list → []
```

### joinpath

```
PurePosixPath('/usr/local').joinpath('bin', 'python') → '/usr/local/bin/python'
PurePosixPath('/usr/local').joinpath('lib') → '/usr/local/lib'
PurePosixPath('relative').joinpath('path', 'to', 'file') → 'relative/path/to/file'
PurePosixPath('/usr').joinpath('') → '/usr'
PurePosixPath('/usr').joinpath('/absolute') → '/absolute'
PurePosixPath('/usr/local').joinpath('bin', '/etc/passwd') → '/etc/passwd'
```

### with_name

```
PurePosixPath('/usr/local/bin/python3.12').with_name('python3.11') → '/usr/local/bin/python3.11'
PurePosixPath('file.txt').with_name('newfile.txt') → 'newfile.txt'
PurePosixPath('relative/path/file.tar.gz').with_name('archive.zip') → 'relative/path/archive.zip'
```

### with_suffix

```
PurePosixPath('/usr/local/bin/python3.12').with_suffix('.11') → '/usr/local/bin/python3.11'
PurePosixPath('/usr/local/bin/python3.12').with_suffix('') → '/usr/local/bin/python3'
PurePosixPath('/usr/local/bin/python3.12').with_suffix('.tar.gz') → '/usr/local/bin/python3.tar.gz'
PurePosixPath('file.txt').with_suffix('.md') → 'file.md'
PurePosixPath('file').with_suffix('.txt') → 'file.txt'
PurePosixPath('file.tar.gz').with_suffix('.zip') → 'file.tar.zip'
```

### with_stem

```
PurePosixPath('/usr/local/bin/python3.12').with_stem('python') → '/usr/local/bin/python.12'
PurePosixPath('file.tar.gz').with_stem('archive') → 'archive.gz'
PurePosixPath('file.txt').with_stem('newfile') → 'newfile.txt'
PurePosixPath('..').with_suffix('.txt') → '...txt'
```

### as_posix

```
PurePosixPath('/usr/local/bin').as_posix() → '/usr/local/bin'
```

### is_absolute

```
PurePosixPath('/usr/local').is_absolute() → True
PurePosixPath('relative/path').is_absolute() → False
PurePosixPath('/').is_absolute() → True
PurePosixPath('').is_absolute() → False
```

### relative_to

```
PurePosixPath('/usr/local/bin/python').relative_to('/usr/local') → 'bin/python'
PurePosixPath('/a/b/c').relative_to('/a/b/c') → '.'
PurePosixPath('/a/b/c').relative_to('/a/b') → 'c'
PurePosixPath('relative').relative_to('relative') → '.'
PurePosixPath('a/b/c').relative_to('a') → 'b/c'
PurePosixPath('/a/b/c').relative_to('/a/x/y', walk_up=True) → '../../b/c'
PurePosixPath('/a/b/c').relative_to('/a', walk_up=True) → 'b/c'
```

### match

```
PurePosixPath('/usr/local/bin/python3.12').match('*.12') → True
PurePosixPath('/usr/local/bin/python3.12').match('python*') → True
PurePosixPath('/usr/local/bin/python3.12').match('bin/python*') → True
PurePosixPath('/usr/local/bin/python3.12').match('/usr/local/bin/python*') → True
PurePosixPath('/usr/local/bin/python3.12').match('nonmatch') → False
PurePosixPath('/home/file.TXT').match('*.txt') → False
PurePosixPath('/home/file.TXT').match('*.txt', case_sensitive=False) → True
PurePosixPath('/home/file.TXT').match('*.TXT', case_sensitive=True) → True
PurePosixPath('/home/user/file.txt').match('user/*') → True
PurePosixPath('/home/user/file.txt').match('*/file.txt') → True
PurePosixPath('/a/b/c/d/file.txt').match('**/file.txt') → True
```

### is_relative_to

```
PurePosixPath('/usr/local/bin').is_relative_to('/usr/local') → True
PurePosixPath('/usr/local/bin').is_relative_to('/home') → False
PurePosixPath('/a/b/c').is_relative_to('/a/b') → True
```

### Equality

```
PurePosixPath('/usr/local') == PurePosixPath('/usr/local') → True
PurePosixPath('/usr/local') == PurePosixPath('/usr/local/') → True
```

### String representations

```
str(PurePosixPath('/usr/local/bin/python3.12')) → '/usr/local/bin/python3.12'
repr(PurePosixPath('/usr/local/bin/python3.12')) → "PurePosixPath('/usr/local/bin/python3.12')"
```

---

## 4. HAZARDS

### Dictionary/set ordering
pyrst dict iteration is **sorted-key order**, not insertion order. Path parts tuple is ordered by construction (not a dict), so no hazard.

### Platform/locale dependence
- `case_sensitive` parameter defaults to platform default (True on POSIX, False on Windows); Pyrst must implement this explicitly if porting to other platforms.
- Paths like `/home/使用者/file.txt` work (Unicode safe); no special handling required.

### Normalization side-effects
- Trailing slashes are normalized away: `'/path/with/trailing/slash/'` becomes `'/path/with/trailing/slash'`
- Multiple slashes collapse: `'//usr//local//bin'` normalizes to `('//', 'usr', 'local', 'bin')` with `'//'` as anchor (network path)
- Empty strings in joinpath are ignored: `joinpath('')` returns the same path

### Empty path edge cases
- `PurePosixPath('')` has empty parts tuple, empty name, is not absolute
- `PurePosixPath('.')` has empty parts tuple, empty name, parent is '.'
- Calling `with_name()` or `with_suffix()` on paths with empty name raises ValueError

### Suffix extraction edge cases
- Leading dots are never part of suffix: `.gitignore` has suffix='', not '.gitignore'
- Multiple dot-separated components each become a suffix: `file..txt` → suffixes=['.', '.txt']
- `..` treated as a normal name, not special: `PurePosixPath('..').with_suffix('.txt')` → `'...txt'`

### Parent chain termination
- Root path's parent is itself: `PurePosixPath('/').parent` → `'/'`
- `len(PurePosixPath('/').parents)` → 0

### Relative path behavior with absolute arguments
- `joinpath()` stops at first absolute path: `'/usr/local'.joinpath('bin', '/etc')` → `'/etc'`
- `relative_to()` requires both paths be absolute or both relative (relative_to with walk_up can bridge roots)

---

## 5. GATED

| Gate | API Part | Issue | Suggested Deferral |
|------|----------|-------|-------------------|
| G4 (*args/**kwargs) | `__init__(self, *args)` | Variadic positional args not supported | Design: require a single optional `paths: list[str]` parameter instead, or use `from_segments` classmethod |
| G4 (*args/**kwargs) | `joinpath(self, *pathsegments)` | Variadic positional args not supported | Design: accept `paths: list[str]` parameter instead; can use `joinpath(path, *[p1, p2])` workaround to loop-call single arg |
| No __truediv__ | `/` operator | `__truediv__` not available in Pyrst class dunders | Design-around: use `joinpath()` exclusively; `path.joinpath('segment')` instead of `path / 'segment'` |
| No bytes | bytes paths | `PurePosixPath` supports bytes in CPython but Pyrst has no bytes type | Deferral: restrict to str-only paths; document as str-only |

---

## 6. PARITY PLAN

Parity test lines (dual-run safe; avoid ordering/dict hazards):

```python
# Initialization
pp = PurePosixPath('/usr/local/bin/python3.12')
assert pp.parts == ('/', 'usr', 'local', 'bin', 'python3.12')
assert pp.name == 'python3.12'
assert pp.stem == 'python3'
assert pp.suffix == '.12'
assert pp.suffixes == ['.tar', '.gz']  if pp.name == 'file.tar.gz' else []

# parents access
pp = PurePosixPath('/a/b/c/d')
assert str(pp.parents[0]) == '/a/b/c'
assert str(pp.parents[1]) == '/a/b'
assert len(pp.parents) == 4

# joinpath
assert str(PurePosixPath('/usr/local').joinpath('bin')) == '/usr/local/bin'
assert str(PurePosixPath('relative').joinpath('path')) == 'relative/path'
assert str(PurePosixPath('/usr').joinpath('/override')) == '/override'

# with_name
assert str(PurePosixPath('/usr/local/bin/python3.12').with_name('python3.11')) == '/usr/local/bin/python3.11'
assert str(PurePosixPath('file.txt').with_name('newfile.txt')) == 'newfile.txt'

# with_suffix
assert str(PurePosixPath('/usr/local/bin/python3.12').with_suffix('.11')) == '/usr/local/bin/python3.11'
assert str(PurePosixPath('file.tar.gz').with_suffix('.zip')) == 'file.tar.zip'

# with_stem
assert str(PurePosixPath('file.tar.gz').with_stem('archive')) == 'archive.gz'
assert str(PurePosixPath('/usr/local/bin/python3.12').with_stem('python')) == '/usr/local/bin/python.12'

# as_posix
assert PurePosixPath('/usr/local').as_posix() == '/usr/local'

# is_absolute
assert PurePosixPath('/usr/local').is_absolute() == True
assert PurePosixPath('relative/path').is_absolute() == False

# relative_to
assert str(PurePosixPath('/usr/local/bin/python').relative_to('/usr/local')) == 'bin/python'
assert str(PurePosixPath('/a/b/c').relative_to('/a/b/c')) == '.'
assert str(PurePosixPath('a/b/c').relative_to('a')) == 'b/c'

# match
assert PurePosixPath('/usr/local/bin/python3.12').match('*.12') == True
assert PurePosixPath('/usr/local/bin/python3.12').match('bin/python*') == True
assert PurePosixPath('/usr/local/bin/python3.12').match('nonmatch') == False
assert PurePosixPath('/home/file.TXT').match('*.txt') == False
assert PurePosixPath('/home/file.TXT').match('*.txt', case_sensitive=False) == True

# is_relative_to
assert PurePosixPath('/usr/local/bin').is_relative_to('/usr/local') == True
assert PurePosixPath('/usr/local/bin').is_relative_to('/home') == False

# Equality
assert PurePosixPath('/usr/local') == PurePosixPath('/usr/local')
assert PurePosixPath('/usr/local') == PurePosixPath('/usr/local/')

# Edge cases
assert str(PurePosixPath('/').parent) == '/'
assert PurePosixPath('/').is_absolute() == True
assert str(PurePosixPath('file.txt').parent) == '.'
assert PurePosixPath('.hidden').suffix == ''
assert PurePosixPath('.hidden.txt').stem == '.hidden'
```

---

## 7. TARGET

**Fidelity: 4/5**

**Reasons not 5:**
1. **Variadic arguments (G4 gate)**: `__init__(*args)` and `joinpath(*pathsegments)` require design-around. Pyrst cannot express *args, so API must shift to list-based or single-arg versions; changes user-facing surface.
2. **No __truediv__**: The `/` operator is unavailable in Pyrst dunders; users must use `.joinpath()` exclusively. While functionally equivalent, breaks CPython idiom and reduces ergonomics.
3. **glob.glob() backend not specified**: `match()` uses glob patterns but doesn't expose the glob engine; Pyrst must implement its own glob matcher (fnmatch-like). Edge cases around case sensitivity and platform defaults need careful implementation.

**Achievable in scope:** All core path manipulation (parts, name, stem, suffix, parent, parents, with_name, with_suffix, with_stem, relative_to, is_absolute, as_posix, is_relative_to) are pure operations with no variadics or special dunders. Pattern matching (match) requires a glob parser but is isolable. Value semantics align well with Pyrst's model.

