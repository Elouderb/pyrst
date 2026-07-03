# ConfigParser Module Dossier

## SURFACE

| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `ConfigParser` | class | `ConfigParser(defaults=None, dict_type=dict, allow_no_value=False, *, delimiters=('=', ':'), comment_prefixes=('#', ';'), inline_comment_prefixes=(), strict=True, empty_lines_in_values=True, default_section='DEFAULT', converters=None)` | ConfigParser | Config file parser with INI-style sections and key=value pairs |
| `add_section` | method | `add_section(section: str) -> None` | None | Add a new section; raises DuplicateSectionError if exists |
| `sections` | method | `sections() -> list[str]` | list[str] | Return list of section names (insertion order) |
| `options` | method | `options(section: str) -> list[str]` | list[str] | Return list of option names in section (includes defaults, insertion order) |
| `items` | method | `items(section: str, raw=False, *, vars=None) -> list[tuple[str, str]]` | list[tuple[str, str]] | Return (key, value) pairs; includes defaults; vars dict for interpolation override |
| `has_section` | method | `has_section(section: str) -> bool` | bool | Check if section exists |
| `has_option` | method | `has_option(section: str, option: str) -> bool` | bool | Check if option exists (includes defaults) |
| `defaults` | method | `defaults() -> dict[str, str]` | dict[str, str] | Return dict of default values |
| `get` | method | `get(section: str, option: str, *, raw=False, vars=None, fallback=<unset>) -> str \| Any` | str | Get option value; interpolates by default; fallback returned on NoOptionError |
| `getint` | method | `getint(section: str, option: str, *, vars=None, fallback=<unset>) -> int` | int | Get option as integer; raises ValueError on invalid; fallback on NoOptionError |
| `getfloat` | method | `getfloat(section: str, option: str, *, vars=None, fallback=<unset>) -> float` | float | Get option as float; raises ValueError on invalid; fallback on NoOptionError |
| `getboolean` | method | `getboolean(section: str, option: str, *, vars=None, fallback=<unset>) -> bool` | bool | Get option as boolean; accepts yes/no/true/false/on/off/1/0; raises ValueError; fallback on NoOptionError |
| `set` | method | `set(section: str, option: str, value: str) -> None` | None | Set option value; raises NoSectionError if section missing |
| `remove_option` | method | `remove_option(section: str, option: str) -> bool` | bool | Remove option; returns True if existed |
| `remove_section` | method | `remove_section(section: str) -> bool` | bool | Remove section and all options; returns True if existed |
| `read_string` | method | `read_string(string: str, source='<string>') -> None` | None | Parse INI text; raises DuplicateSectionError or ParsingError on error |
| `write` | method | `write(fp, space_around_delimiters=True) -> None` | None | Write configuration to file object; section headers in [brackets]; key = value format |
| `optionxform` | method | `optionxform(optionstr: str) -> str` | str | Transform option name (default: lowercase); can override |

## ERRORS

| Edge Case | Exception | Message |
|-----------|-----------|---------|
| `get('missing_section', 'key')` | NoSectionError | `"No section: 'missing_section'"` |
| `cp.add_section('s'); cp.get('s', 'missing_key')` | NoOptionError | `"No option 'missing_key' in section: 's'"` |
| `cp.add_section('s'); cp.add_section('s')` | DuplicateSectionError | `"Section 's' already exists"` |
| `cp.set('missing', 'k', 'v')` | NoSectionError | `"No section: 'missing'"` |
| `cp.add_section('s'); cp.set('s', 'k', 'notint'); cp.getint('s', 'k')` | ValueError | `"invalid literal for int() with base 10: 'notint'"` |
| `cp.add_section('s'); cp.set('s', 'k', 'notfloat'); cp.getfloat('s', 'k')` | ValueError | `"could not convert string to float: 'notfloat'"` |
| `cp.add_section('s'); cp.set('s', 'k', 'maybe'); cp.getboolean('s', 'k')` | ValueError | `"Not a boolean: maybe"` |
| `cp.add_section('s'); cp.set('s', 'k', '%(undefined)s'); cp.get('s', 'k')` | InterpolationMissingOptionError | `"Bad value substitution: option 'k' in section 's' contains an interpolation key 'undefined' which is not a valid option name. Raw value: '%(undefined)s'"` |
| `cp.add_section('s'); cp.set('s', 'a', '%(a)s'); cp.get('s', 'a')` | InterpolationDepthError | `"Recursion limit exceeded in value substitution: option 'a' in section 's' contains an interpolation key which cannot be substituted in 10 steps. Raw value: '%(a)s'"` |
| `cp.read_string('[s]\nkey3 value3')` (no delimiter) | ParsingError | Source contains parsing errors |
| `cp.read_string('[s]\nkey=v1\nkey=v2')` (duplicate option) | DuplicateOptionError | `"While reading from '<string>' [line 3]: option 'key' in section 's' already exists"` |

## BEHAVIOR MATRIX

### Basic Operations
```python
cp = configparser.ConfigParser()
cp.add_section('section1')
cp.set('section1', 'key1', 'value1')
cp.sections()  # ['section1']

cp.options('section1')  # ['key1']

cp.items('section1')  # [('key1', 'value1')]

cp.get('section1', 'key1')  # 'value1'

cp.has_section('section1')  # True
cp.has_option('section1', 'key1')  # True
```

### Case Sensitivity
```python
cp = configparser.ConfigParser()
cp.add_section('Section')
cp.set('Section', 'Key', 'value')
cp.get('Section', 'Key')  # 'value' (case-sensitive for sections)
cp.get('Section', 'key')  # 'value' (case-insensitive for options)
cp.optionxform('KEY')  # 'key' (default transforms to lowercase)
```

### Numeric Getters
```python
cp = configparser.ConfigParser()
cp.add_section('nums')
cp.set('nums', 'int_val', '42')
cp.set('nums', 'float_val', '3.14')
cp.getint('nums', 'int_val')  # 42 (type: int)
cp.getfloat('nums', 'float_val')  # 3.14 (type: float)
```

### Boolean Values
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'b', 'yes')
cp.getboolean('s', 'b')  # True

cp.set('s', 'b', 'no')
cp.getboolean('s', 'b')  # False

cp.set('s', 'b', 'true')
cp.getboolean('s', 'b')  # True

cp.set('s', 'b', '1')
cp.getboolean('s', 'b')  # True

cp.set('s', 'b', '0')
cp.getboolean('s', 'b')  # False
```

### Default Section
```python
cp = configparser.ConfigParser(defaults={'global_key': 'global_val'})
cp.add_section('section1')
cp.set('section1', 'local_key', 'local_val')

cp.defaults()  # {'global_key': 'global_val'}
cp.get('section1', 'global_key')  # 'global_val' (inherited from defaults)
cp.options('section1')  # ['local_key', 'global_key'] (includes defaults)
cp.items('section1')  # [('global_key', 'global_val'), ('local_key', 'local_val')]
```

### DEFAULT Section in Config
```python
config_text = """[DEFAULT]
global_key=global_value

[section1]
local_key=local_value
"""
cp = configparser.ConfigParser()
cp.read_string(config_text)

cp.defaults()  # {'global_key': 'global_value'}
cp.get('section1', 'global_key')  # 'global_value'
cp.get('section1', 'local_key')  # 'local_value'
```

### BasicInterpolation
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'base', '/home')
cp.set('s', 'path', '%(base)s/user')

cp.get('s', 'path')  # '/home/user'
cp.get('s', 'path', raw=True)  # '%(base)s/user' (uninterpolated)
```

### Nested Interpolation
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'base', '/home')
cp.set('s', 'user', 'alice')
cp.set('s', 'path', '%(base)s/%(user)s')

cp.get('s', 'path')  # '/home/alice'
```

### Interpolation with Defaults
```python
cp = configparser.ConfigParser(defaults={'base': '/home'})
cp.add_section('s')
cp.set('s', 'path', '%(base)s/user/config')

cp.get('s', 'path')  # '/home/user/config'
```

### Interpolation with vars Parameter
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'path', '%(home)s/config')

cp.get('s', 'path', vars={'home': '/opt'})  # '/opt/config'
cp.items('s', vars={'name': 'Alice'})  # Applies vars to interpolation
```

### Literal Percent
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'literal', 'value with %% embedded')

cp.get('s', 'literal', raw=False)  # 'value with % embedded' (%% -> %)
cp.get('s', 'literal', raw=True)  # 'value with %% embedded'
```

### read_string
```python
config_text = """[section1]
key1=value1
key2=value2

[section2]
key3=value3
"""
cp = configparser.ConfigParser()
cp.read_string(config_text)

cp.sections()  # ['section1', 'section2']
cp.get('section1', 'key1')  # 'value1'
```

### Multiline Values
```python
config_text = """[s]
key1=line1
 line2
 line3
"""
cp = configparser.ConfigParser()
cp.read_string(config_text)

cp.get('s', 'key1')  # 'line1\nline2\nline3'
```

### allow_no_value
```python
config_text = """[s]
key1
key2=value2
"""
cp = configparser.ConfigParser(allow_no_value=True)
cp.read_string(config_text)

cp.get('s', 'key1')  # None
cp.get('s', 'key2')  # 'value2'
```

### Empty String Value
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'empty', '')

cp.get('s', 'empty')  # ''
```

### Fallback Parameter
```python
cp = configparser.ConfigParser()
cp.add_section('s')

cp.get('s', 'missing', fallback='default_value')  # 'default_value'
cp.getint('s', 'missing', fallback=999)  # 999
cp.getfloat('s', 'missing', fallback=3.14)  # 3.14
cp.getboolean('s', 'missing', fallback=True)  # True
```

### Whitespace Handling
```python
config_text = """[section1]
  key1  =  value1  
key2=value2
"""
cp = configparser.ConfigParser()
cp.read_string(config_text)

cp.get('section1', 'key1')  # 'value1' (leading/trailing whitespace stripped)
```

### Value with Special Characters
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'k', 'a=b')

cp.get('s', 'k')  # 'a=b'
```

### Write Round-trip
```python
cp1 = configparser.ConfigParser()
cp1.add_section('test')
cp1.set('test', 'key', 'value')

output = io.StringIO()
cp1.write(output)
written = output.getvalue()  # '[test]\nkey = value\n\n'

cp2 = configparser.ConfigParser()
cp2.read_string(written)
cp2.get('test', 'key')  # 'value'
```

### Write Format Control
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'k', 'v')

output1 = io.StringIO()
cp.write(output1, space_around_delimiters=False)
output1.getvalue()  # '[s]\nk=v\n\n'

output2 = io.StringIO()
cp.write(output2, space_around_delimiters=True)
output2.getvalue()  # '[s]\nk = v\n\n'
```

### Insertion Order Preservation
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'z', '1')
cp.set('s', 'a', '2')
cp.set('s', 'm', '3')

cp.options('s')  # ['z', 'a', 'm'] (insertion order)
```

### Section Ordering
```python
cp = configparser.ConfigParser()
cp.add_section('z')
cp.add_section('a')
cp.add_section('m')

cp.sections()  # ['z', 'a', 'm'] (insertion order)
```

### Remove Operations
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'k1', 'v1')
cp.set('s', 'k2', 'v2')

cp.remove_option('s', 'k1')
cp.options('s')  # ['k2']

cp.add_section('s2')
cp.remove_section('s2')
cp.has_section('s2')  # False
```

### Unicode Support
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'unicode', '你好')

cp.get('s', 'unicode')  # '你好'

# Round-trip
cp1 = configparser.ConfigParser()
cp1.add_section('s')
cp1.set('s', 'key', '你好 world')
output = io.StringIO()
cp1.write(output)
cp2 = configparser.ConfigParser()
cp2.read_string(output.getvalue())
cp2.get('s', 'key')  # '你好 world'
```

### Edge Cases
```python
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'k', '0')
cp.getint('s', 'k')  # 0

cp.set('s', 'k', '-42')
cp.getint('s', 'k')  # -42

cp.set('s', 'k', '9223372036854775807')
cp.getint('s', 'k')  # 9223372036854775807 (max i64 value)

cp.set('s', 'k', '0.0')
cp.getfloat('s', 'k')  # 0.0

cp.set('s', 'k', '1e-10')
cp.getfloat('s', 'k')  # 1e-10
```

### Option Name Transformation
```python
config_text = """[s]
MyOption=value
"""
cp = configparser.ConfigParser()
cp.read_string(config_text)

cp.options('s')  # ['myoption'] (lowercased)
cp.get('s', 'MyOption')  # 'value' (case-insensitive access)
cp.get('s', 'myoption')  # 'value'
```

### DEFAULT Section Modification
```python
cp = configparser.ConfigParser()
cp.read_string('[DEFAULT]\nd1=v1')

cp.set('DEFAULT', 'd2', 'v2')
cp.defaults()  # {'d1': 'v1', 'd2': 'v2'}
```

## HAZARDS

1. **Dict Iteration Order**: Items are returned in insertion order (Python 3.7+), not sorted. pyrst iterates dicts sorted by key — this will reorder all output.

2. **Float Representation**: `getfloat()` returns Python float. Values like `1e-10` print with exponential notation. Exact float formatting depends on Python's repr.

3. **Section Name Case Sensitivity**: Sections are case-sensitive, but option names are case-insensitive via `optionxform`. This asymmetry must be preserved.

4. **Boolean String Values**: `getboolean` accepts 8 specific string forms (yes/no/true/false/on/off/1/0) as exact matches. Custom boolean acceptance requires subclassing or avoiding.

5. **Interpolation Regex Matching**: `%(name)s` is strict — any malformed interpolation syntax raises `InterpolationMissingOptionError` or `InterpolationDepthError`. Circular references hit a 10-step depth limit.

6. **% Escaping**: `%%` in a value becomes `%` when interpolation is applied (`raw=False`). This is a non-obvious transformation.

7. **Insertion Order Dependency**: All ordered outputs (sections, options, items) depend on insertion order. Tests comparing against fixed lists will fail if pyrst sorts.

8. **Write Format Spaces**: Default write uses ` = ` (spaces around delimiter). Non-default requires `space_around_delimiters=False` parameter.

9. **Unicode Round-trip**: Unicode values survive read/write cycles cleanly in modern CPython, but platform encoding or text mode issues could emerge.

10. **Comment and Delimiter Flexibility**: Default comment prefixes are `#` and `;`, delimiters are `=` and `:`. These can be customized; defaults shown here.

## GATED

1. **G2 (Module-Level Mutable State)**: `ConfigParser()` constructor accepts `defaults` parameter (dict-typed). This is a constructor arg, not module-level state — OK.

2. **G4 (No *args/**kwargs)**: Constructor signature uses keyword-only args (delimiters, comment_prefixes, etc.). pyrst does NOT support keyword-only markers; these must be positional or require a design workaround (e.g., separate builder method).

3. **G7 (No bytes type)**: All I/O is text (str). File object params to `read_file()` and `write()` expect text mode. pyrst has no bytes type — **deferred until file-typed params are spellable**.

4. **Interpolation Depth Limit**: 10-step recursion limit is hardcoded in CPython. Must match exactly or tests fail.

5. **Exception Classes**: CPython defines `NoSectionError`, `NoOptionError`, `DuplicateOptionError`, `DuplicateSectionError`, `InterpolationMissingOptionError`, `InterpolationDepthError`, `ParsingError`. pyrst has no custom exception types — **these must map to ValueError/KeyError/RuntimeError with CPython's message text preserved**.

6. **optionxform Override**: Callers can override `optionxform()` to customize option name transformation. pyrst does not support instance method override — **this feature is deferred; default behavior (lowercase) is portable**.

## PARITY PLAN

All expressions verified via CPython 3.12.9:

```python
# 1. Basic section and option operations
cp = configparser.ConfigParser()
cp.add_section('test')
cp.set('test', 'key', 'value')
cp.sections() == ['test']  # True

# 2. Get operation
cp.get('test', 'key') == 'value'  # True

# 3. Has section
cp.has_section('test') == True  # True

# 4. Has option
cp.has_option('test', 'key') == True  # True

# 5. Options list
cp.options('test') == ['key']  # True

# 6. Items list
list(cp.items('test')) == [('key', 'value')]  # True

# 7. Getint
cp.set('test', 'num', '42')
cp.getint('test', 'num') == 42  # True

# 8. Getfloat
cp.set('test', 'flt', '3.14')
cp.getfloat('test', 'flt') == 3.14  # True

# 9. Getboolean true
cp.set('test', 'bool', 'yes')
cp.getboolean('test', 'bool') == True  # True

# 10. Getboolean false
cp.set('test', 'bool', 'no')
cp.getboolean('test', 'bool') == False  # True

# 11. Defaults parameter
cp = configparser.ConfigParser(defaults={'def': 'val'})
cp.defaults() == {'def': 'val'}  # True

# 12. Defaults in get
cp.add_section('s')
cp.get('s', 'def') == 'val'  # True

# 13. Defaults in options
'def' in cp.options('s')  # True

# 14. Defaults in items
('def', 'val') in list(cp.items('s'))  # True

# 15. read_string basic
cp = configparser.ConfigParser()
cp.read_string('[s]\nk=v')
cp.get('s', 'k') == 'v'  # True

# 16. read_string DEFAULT section
cp = configparser.ConfigParser()
cp.read_string('[DEFAULT]\nd=dv\n[s]\nl=lv')
cp.get('s', 'd') == 'dv'  # True

# 17. Interpolation basic
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'base', '/home')
cp.set('s', 'path', '%(base)s/user')
cp.get('s', 'path') == '/home/user'  # True

# 18. Interpolation raw
cp.get('s', 'path', raw=True) == '%(base)s/user'  # True

# 19. Interpolation with defaults
cp = configparser.ConfigParser(defaults={'home': '/opt'})
cp.add_section('s')
cp.set('s', 'cfg', '%(home)s/conf')
cp.get('s', 'cfg') == '/opt/conf'  # True

# 20. Interpolation with vars
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'cfg', '%(home)s/conf')
cp.get('s', 'cfg', vars={'home': '/var'}) == '/var/conf'  # True

# 21. write round-trip
cp1 = configparser.ConfigParser()
cp1.add_section('t')
cp1.set('t', 'k', 'v')
out = io.StringIO()
cp1.write(out)
cp2 = configparser.ConfigParser()
cp2.read_string(out.getvalue())
cp2.get('t', 'k') == 'v'  # True

# 22. Empty value
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'empty', '')
cp.get('s', 'empty') == ''  # True

# 23. Value with equals
cp.set('s', 'eq', 'a=b')
cp.get('s', 'eq') == 'a=b'  # True

# 24. fallback parameter
cp = configparser.ConfigParser()
cp.add_section('s')
cp.get('s', 'missing', fallback='def') == 'def'  # True

# 25. getint fallback
cp.getint('s', 'missing', fallback=99) == 99  # True

# 26. getfloat fallback
cp.getfloat('s', 'missing', fallback=2.71) == 2.71  # True

# 27. getboolean fallback
cp.getboolean('s', 'missing', fallback=False) == False  # True

# 28. Whitespace strip
cp = configparser.ConfigParser()
cp.read_string('[s]\n  k  =  v  ')
cp.get('s', 'k') == 'v'  # True

# 29. Multiline value
cp = configparser.ConfigParser()
cp.read_string('[s]\nk=line1\n line2\n line3')
cp.get('s', 'k') == 'line1\nline2\nline3'  # True

# 30. Option name lowercase
cp = configparser.ConfigParser()
cp.read_string('[s]\nMyOpt=val')
cp.options('s') == ['myopt']  # True

# 31. Option case-insensitive access
cp.get('s', 'MYOPT') == 'val'  # True

# 32. Option case-insensitive has_option
cp.has_option('s', 'MyOpt') == True  # True

# 33. Section case-sensitive
cp.has_section('S') == False  # True

# 34. Boolean yes
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'b', 'yes')
cp.getboolean('s', 'b') == True  # True

# 35. Boolean no
cp.set('s', 'b', 'no')
cp.getboolean('s', 'b') == False  # True

# 36. Boolean true
cp.set('s', 'b', 'true')
cp.getboolean('s', 'b') == True  # True

# 37. Boolean false
cp.set('s', 'b', 'false')
cp.getboolean('s', 'b') == False  # True

# 38. Boolean on
cp.set('s', 'b', 'on')
cp.getboolean('s', 'b') == True  # True

# 39. Boolean off
cp.set('s', 'b', 'off')
cp.getboolean('s', 'b') == False  # True

# 40. Boolean 1
cp.set('s', 'b', '1')
cp.getboolean('s', 'b') == True  # True

# 41. Boolean 0
cp.set('s', 'b', '0')
cp.getboolean('s', 'b') == False  # True

# 42. remove_option
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'k', 'v')
cp.remove_option('s', 'k')
cp.has_option('s', 'k') == False  # True

# 43. remove_section
cp.add_section('s2')
cp.remove_section('s2')
cp.has_section('s2') == False  # True

# 44. Literal percent escape
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'pct', 'val %% here')
cp.get('s', 'pct', raw=False) == 'val % here'  # True

# 45. Literal percent raw
cp.get('s', 'pct', raw=True) == 'val %% here'  # True

# 46. Numeric edge: zero
cp = configparser.ConfigParser()
cp.add_section('s')
cp.set('s', 'k', '0')
cp.getint('s', 'k') == 0  # True

# 47. Numeric edge: negative
cp.set('s', 'k', '-99')
cp.getint('s', 'k') == -99  # True

# 48. Numeric edge: max i64
cp.set('s', 'k', '9223372036854775807')
cp.getint('s', 'k') == 9223372036854775807  # True

# 49. Float zero
cp.set('s', 'k', '0.0')
cp.getfloat('s', 'k') == 0.0  # True

# 50. Float scientific
cp.set('s', 'k', '1e-10')
cp.getfloat('s', 'k') == 1e-10  # True
```

## TARGET

**Fidelity: 3.5/5**

**Dominant Reasons for Gap:**

1. **Custom Exception Classes (G2)**: CPython defines 6 custom exception types; pyrst has no custom exceptions. Must map to ValueError/KeyError/RuntimeError, losing specific error discrimination. Tests checking `isinstance(e, InterpolationMissingOptionError)` will break.

2. **Keyword-Only Arguments (G4)**: Constructor and methods use keyword-only args (e.g., `raw=`, `vars=`, `fallback=`, `space_around_delimiters=`). pyrst does not support keyword-only markers — requires design workaround or full refactor to positional args (breaks API).

3. **File-Typed Parameters**: `read_file(fp)` and `write(fp)` expect file objects; pyrst cannot spell file-typed function params yet. Blocks `read_file()` and `write()` until G7 is addressed.

4. **Insertion-Order Semantics**: All ordered outputs depend on insertion order, not sorted order. pyrst dict iteration is sorted-key. Work-arounds (e.g., storing insertion order separately) are possible but add complexity and deviate from CPython spec.

**Achievable with Constraints:**
- Core API (add_section, set, get, getint, getfloat, getboolean, sections, options, items, defaults, has_section, has_option, remove_option, remove_section) ✓
- read_string ✓
- BasicInterpolation ✓
- Fallback parameters (via optional args) ✓
- raw parameter ✓
- vars parameter ✓
- write with space_around_delimiters parameter ✓

**Blocked or Degraded:**
- Custom exception classes (map to ValueError/KeyError, lose specificity)
- read_file() (file-typed param)
- write() output ordering (sorted, not insertion-order)
- optionxform override (no instance method override)
- Keyword-only arg enforcement (positional fallback required)
