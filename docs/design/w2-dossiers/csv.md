# csv Module Implementation Dossier

**Module:** `csv`  
**Surface Count:** 14  
**Parity Cases:** 38  
**GATED Count:** 6  
**Target Fidelity:** 3/5  
**Dossier Path:** `/tmp/claude-1000/-home-ethos-Coding-pyrst/a33a952b-bec2-4e9d-8c5b-5bd85bfdac8d/scratchpad/w2prep/dossiers/csv.md`

---

## 1. SURFACE

| Name | Kind | Signature | Return | Semantics |
|------|------|-----------|--------|-----------|
| `reader` | fn | `reader(iterable, *, dialect='excel', delimiter=None, quotechar='"', escapechar=None, doublequote=True, skipinitialspace=False, quoting=QUOTE_MINIMAL, lineterminator=None)` | Iterator[List[str]] | Parse CSV lines from text iterable into field lists; strips line terminators. |
| `writer` | fn | `writer(f, *, dialect='excel', delimiter=None, quotechar='"', escapechar=None, doublequote=True, skipinitialspace=False, quoting=QUOTE_MINIMAL, lineterminator=None)` | writer object | Create CSV writer for f (StringIO only); write methods return None. |
| `DictReader` | class | `DictReader(f, *, fieldnames=None, restkey=None, restval=None, dialect='excel', *args, **kwargs)` | DictReader instance | Iterate rows as dicts with fieldnames as keys; fieldnames from first row if not provided; extra fields collected in restkey list. |
| `DictWriter` | class | `DictWriter(f, fieldnames, *, restval='', extrasaction='raise', dialect='excel', *args, **kwargs)` | DictWriter instance | Write dicts as CSV; writeheader() writes fieldname row; writerow(dict) omits restval-filled fields, raises or ignores extra fields per extrasaction. |
| `Sniffer` | class | `Sniffer()` | Sniffer instance | Dialect detection: sniff(sample, delimiters=None) infers dialect; has_header(sample) detects header row. |
| `Dialect` | class | `Dialect()` | Dialect subclass instance | Abstract base for dialect classes (excel, excel_tab, unix); attributes: delimiter, quotechar, escapechar, doublequote, skipinitialspace, quoting, lineterminator. |
| `excel` | class | `excel()` | excel dialect | Dialect: delimiter=',', quotechar='"', doublequote=True, quoting=QUOTE_MINIMAL, lineterminator='\r\n'. |
| `excel_tab` | class | `excel_tab()` | excel_tab dialect | Dialect: delimiter='\t', quotechar='"', doublequote=True, quoting=QUOTE_MINIMAL, lineterminator='\r\n'. |
| `unix_dialect` | class | `unix_dialect()` | unix dialect | Dialect: delimiter=',', quotechar='"', doublequote=True, quoting=QUOTE_ALL, lineterminator='\n'. |
| `register_dialect` | fn | `register_dialect(name, dialect=None, **fmtparams)` | None | Register custom dialect; raises Error if name exists or params invalid. |
| `unregister_dialect` | fn | `unregister_dialect(name)` | None | Unregister dialect; raises Error if name not found. |
| `get_dialect` | fn | `get_dialect(name)` | Dialect class | Return dialect by name; raises Error if unknown. |
| `list_dialects` | fn | `list_dialects()` | List[str] | Return list of registered dialect names. |
| `field_size_limit` | fn | `field_size_limit(new_limit=None)` | int | Get/set max field size; returns old limit; default 131072; raises TypeError if not int. |

**Constants:**
```
QUOTE_MINIMAL = 0    # Quote only if field contains delimiter/quote/newline
QUOTE_ALL = 1        # Quote all non-numeric fields
QUOTE_NONNUMERIC = 2 # Quote all fields (deprecated in favor of QUOTE_ALL)
QUOTE_NONE = 3       # Never quote; use escapechar for embedded delimiters/quotes
QUOTE_STRINGS = 4    # Quote all str fields, not numbers
QUOTE_NOTNULL = 5    # Quote all non-empty fields
```

**Exception:**
```
Error(Exception)     # CSV-specific exception for parsing errors
```

**Reader/Writer object methods:**
- `reader.__iter__()` → self; `reader.__next__()` → next row as list
- `writer.writerow(row: List[str]) → None`
- `writer.writerows(rows: List[List[str]]) → None`
- `reader.line_num` (property) → current line number (int)
- `DictReader.line_num`, `DictReader.fieldnames` (lazy: computed on first __next__)

---

## 2. ERRORS

| Probe | Exception Type | Message |
|-------|----------------|---------|
| `reader(f, delimiter=None)` | `TypeError` | "delimiter" must be string, not NoneType |
| `writer(f, quotechar='')` | `TypeError` | "quotechar" must be a 1-character string |
| `writer(f, lineterminator=123)` | `TypeError` | "lineterminator" must be a string |
| `writer(f, delimiter='')` | `TypeError` | "delimiter" must be a 1-character string |
| `writer(f, delimiter='::')` | `TypeError` | "delimiter" must be a 1-character string |
| `writer(f, escapechar='')` | `TypeError` | "escapechar" must be a 1-character string |
| `csv.field_size_limit("string")` | `TypeError` | limit must be an integer |
| `csv.field_size_limit(10); reader(huge_field)` | `Error` | field larger than field limit (10) |
| `parse "to quote in unquoted field without newline=''` | `Error` | new-line character seen in unquoted field - do you need to open the file with newline=''? |
| `csv.get_dialect('nonexistent')` | `Error` | unknown dialect |
| `csv.unregister_dialect('nonexistent')` | `Error` | unknown dialect |
| `csv.register_dialect('bad', delimiter='')` | `TypeError` | "delimiter" must be a 1-character string |
| `DictWriter(f, ['a','b']).writerow({'a':'1','b':'2','c':'3'})` (no extrasaction='ignore') | `ValueError` | dict contains fields not in fieldnames: 'c' |
| `writer(io.BytesIO())` | `TypeError` | a bytes-like object is required, not 'str' |
| `reader(f); f.close(); next(reader)` | `ValueError` | I/O operation on closed file |

---

## 3. BEHAVIOR MATRIX

### Basic Round-Trip
```python
# Input: simple row
data = '"test""quote","with,comma",normal\r\n'
csv.reader(io.StringIO(data)) → ['test"quote', 'with,comma', 'normal']
csv.writer writes ['test"quote', 'with,comma', 'normal'] → '"test""quote","with,comma",normal\r\n'
# Multiline round-trip
csv.writer writes [['a', 'b\nc'], ['d', 'e']] → 'a,"b\nc"\r\nd,e\r\n'
csv.reader(io.StringIO('a,"b\nc"\r\nd,e\r\n')) → [['a', 'b\nc'], ['d', 'e']]
```

### Quote Modes
```python
# QUOTE_MINIMAL (0) - quote only if needed
csv.writer(quoting=QUOTE_MINIMAL).writerow(['simple', 'with,comma', 'with"quote'])
→ 'simple,"with,comma","with""quote"\r\n'

# QUOTE_ALL (1) - quote all fields
csv.writer(quoting=QUOTE_ALL).writerow(['simple', 'with,comma', 'with"quote'])
→ '"simple","with,comma","with""quote"\r\n'

# QUOTE_NONE (3) - use escapechar instead
csv.writer(quoting=QUOTE_NONE, escapechar='\\').writerow(['a\\b', 'c,d', 'e"f'])
→ 'a\\\\b,c\\,d,e\\"f\r\n'

# QUOTE_NONNUMERIC (2) - quote non-numeric
csv.writer(quoting=QUOTE_NONNUMERIC).writerow(['text', 3.14, 42])
→ '"text",3.14,42\r\n'

# QUOTE_STRINGS (4) - quote string fields only
csv.writer(quoting=QUOTE_STRINGS).writerow(['text', 123, '456'])
(floats and ints unquoted if numeric; strings quoted)

# QUOTE_ALL with embedded newline
csv.writer(quoting=QUOTE_ALL).writerow(['normal', 'with"quote', 'with,comma', 'with\nnewline'])
→ '"normal","with""quote","with,comma","with\nnewline"\r\n'
```

### Embedded Newlines in Quoted Fields
```python
data = '"a","b\nc","d"'
csv.reader(io.StringIO(data)) → [['a', 'b\nc', 'd']]

# Multiline field handling
csv.writer writes [['field1', 'line1\nline2', 'field3']]
→ 'field1,"line1\nline2",field3\r\n'
csv.reader reads back → [['field1', 'line1\nline2', 'field3']]
```

### Quote Escaping
```python
# Doublequote mode (default: True)
csv.writer(doublequote=True) writes ['a"b', 'c']
→ '"a""b",c\r\n'
csv.reader → ['a"b', 'c']

# Escapechar mode (doublequote=False)
csv.writer(doublequote=False, escapechar='\\') writes ['a"b', 'c']
→ 'a\\"b,c\r\n'
csv.reader(escapechar='\\') → ['a"b', 'c']
```

### Field Size
```python
# Long field (10000 chars)
csv.writer writes ['x'*10000, 'b']
csv.reader reads → ['x'*10000, 'b']  # length preserved

# Many fields (100 fields)
csv.writer writes [str(i) for i in range(100)]
csv.reader reads → [str(i) for i in range(100)]  # all fields preserved

# Field size limit (default 131072)
csv.field_size_limit() → 131072
csv.field_size_limit(10); reader(field_over_10) → Error: field larger than field limit
```

### Empty and Edge Cases
```python
# Empty input
csv.reader(io.StringIO('')) → []  # no rows

# Single quoted empty field
csv.reader(io.StringIO('""')) → ['']

# Multiple empty quoted fields
csv.reader(io.StringIO('"","",""')) → ['', '', '']

# Trailing empty field (trailing comma)
csv.reader(io.StringIO('a,b,')) → ['a', 'b', '']

# Only delimiters
csv.reader(io.StringIO(',,,')) → ['', '', '', '']

# Only newlines
csv.reader(io.StringIO('\n\n\n')) → [[], [], []]

# Single column
csv.reader(io.StringIO('a\n1\n2')) → [['a'], ['1'], ['2']]
```

### Delimiter and Quotechar Options
```python
# Custom delimiter (pipe)
csv.writer(delimiter='|').writerow(['a', 'b,c']) → 'a,"b,c"\r\n'
csv.reader(delimiter='|', quoting=QUOTE_ALL) with 'a|b'... → ['a', 'b']

# Delimiter = space
csv.writer(delimiter=' ').writerow(['a', 'b', 'c']) → 'a b c\r\n'
csv.reader(delimiter=' ') with 'a b c' → ['a', 'b', 'c']

# Tab delimiter (Excel-tab)
csv.writer(dialect='excel-tab').writerow(['a', 'b\tc', 'd'])
→ 'a\t"b\tc"\td\r\n'

# Custom quotechar (single quote)
csv.register_dialect('sq', quotechar="'", delimiter=';')
csv.writer(dialect='sq').writerow(['a', "b;c", "d'e"])
→ "'a;'b;c';'d''e'\n"  (doublequote applies)
```

### Newline Handling
```python
# LF only (input)
csv.reader(io.StringIO('a,b\n1,2\n')) → [['a', 'b'], ['1', '2']]

# CRLF (standard)
csv.reader(io.StringIO('a,b\r\n1,2\r\n')) → [['a', 'b'], ['1', '2']]

# Mixed newlines (LF and CRLF)
csv.reader(io.StringIO('a,b\n1,2\r\n3,4\r')) (CR only fails; requires newline='')

# Custom lineterminator in write
csv.writer(lineterminator='\n').writerow(['a', 'b']) → 'a,b\n'
csv.writer(lineterminator='|').writerow(['a', 'b']) → 'a,b|'
csv.writer(lineterminator='').writerow(['a', 'b']) → 'a,b'  # no terminator
```

### skipinitialspace
```python
# Without skipinitialspace (default False)
csv.reader(io.StringIO('a, b, c')) → ['a', ' b', ' c']

# With skipinitialspace (True)
csv.reader(skipinitialspace=True) → ['a', 'b', 'c']

# Works with quoted fields
csv.reader(skipinitialspace=True) with 'a, " b ", c'
→ ['a', ' b ', 'c']  # space outside quotes not stripped, inside preserved
```

### Unicode and Special Characters
```python
# Unicode round-trip
csv.writer.writerow(['café', '日本語', 'Ñoño'])
→ 'café,日本語,Ñoño\r\n'
csv.reader → ['café', '日本語', 'Ñoño']

# Backslash (not special in CSV core)
csv.writer.writerow(['a\\b', 'c']) → 'a\\b,c\r\n'

# Tab in field
csv.writer.writerow(['a\tb', 'c']) → 'a\tb,c\r\n'

# Carriage return in quoted field
csv.reader(io.StringIO('"a\r\nb",c')) → ['a\r\nb', 'c']

# Newline in quoted field
csv.reader(io.StringIO('"a\nb",c')) → ['a\nb', 'c']
```

### DictReader/DictWriter
```python
# DictReader basic
data = 'name,age,city\nAlice,30,NYC\nBob,25,LA\n'
reader = DictReader(io.StringIO(data))
→ [{'name':'Alice','age':'30','city':'NYC'}, {'name':'Bob','age':'25','city':'LA'}]

# DictReader with custom fieldnames
data = '30,NYC,Alice\n25,LA,Bob\n'
reader = DictReader(io.StringIO(data), fieldnames=['age','city','name'])
→ [{'age':'30','city':'NYC','name':'Alice'}, ...]

# DictReader with restkey (extra fields)
data = 'a,b\n1,2,3\n'
reader = DictReader(io.StringIO(data), restkey='_extra')
→ [{'a':'1','b':'2','_extra':['3']}]

# DictWriter basic
writer = DictWriter(out, fieldnames=['name','age','city'])
writer.writeheader()
writer.writerow({'name':'Alice','age':'30','city':'NYC'})
→ 'name,age,city\r\nAlice,30,NYC\r\n'

# DictWriter with restval (missing fields)
writer = DictWriter(out, fieldnames=['a','b'], restval='DEFAULT')
writer.writerow({'a':'1'})  # missing 'b'
→ 'a,b\r\n1,DEFAULT\r\n'

# DictWriter with extrasaction='ignore' (extra fields)
writer = DictWriter(out, fieldnames=['a','b'], extrasaction='ignore')
writer.writerow({'a':'1','b':'2','c':'3'})
→ 'a,b\r\n1,2\r\n'  # 'c' ignored

# DictWriter with extrasaction='raise' (default)
writer.writerow({'a':'1','b':'2','c':'3'}) → ValueError: dict contains fields not in fieldnames: 'c'

# DictReader.line_num (lazy: counts from after header)
reader = DictReader(io.StringIO('a,b\n1,2\n3,4\n'))
next(reader) → line_num = 2; next(reader) → line_num = 3

# DictReader dict key order (insertion order from fieldnames arg or header row)
data = 'z,a,m\n1,2,3\n'
reader = DictReader(io.StringIO(data))
row = next(reader)
list(row.keys()) → ['z', 'a', 'm']  # insertion/header order, not sorted
```

### Reader/Writer Object Behavior
```python
# reader is an iterator
reader = csv.reader(io.StringIO('a,b\n1,2\n3,4\n'))
next(reader) → ['a', 'b']
next(reader) → ['1', '2']
next(reader) → ['3', '4']
# StopIteration raised after exhaustion

# writer.writerows (batch)
writer = csv.writer(out)
writer.writerows([['a','b'], ['1','2'], ['3','4']])
→ 'a,b\r\n1,2\r\n3,4\r\n'

# reader.line_num (current line)
reader = csv.reader(io.StringIO('a,b\n1,2\n3,4\n'))
after row 1: reader.line_num = 1
after row 2: reader.line_num = 2
after row 3: reader.line_num = 3

# writer.writerow with type coercion
writer.writerow(['a', None, 'c']) → 'a,,c\r\n'  # None → empty field
writer.writerow(['a', 123, 'c']) → 'a,123,c\r\n'  # int → str
writer.writerow(['a', True, False]) → 'a,True,False\r\n'  # bool → str
writer.writerow(['a', 3.14, 'c']) → 'a,3.14,c\r\n'  # float → str
```

### Dialects
```python
# excel (default)
d = csv.excel()
→ delimiter=',', quotechar='"', doublequote=True, quoting=QUOTE_MINIMAL, lineterminator='\r\n'

# excel_tab
d = csv.excel_tab()
→ delimiter='\t', quotechar='"', doublequote=True, quoting=QUOTE_MINIMAL, lineterminator='\r\n'

# unix
d = csv.unix_dialect()
→ delimiter=',', quotechar='"', doublequote=True, quoting=QUOTE_ALL, lineterminator='\n'

# list_dialects()
csv.list_dialects() → ['excel', 'excel-tab', 'unix']

# register/unregister
csv.register_dialect('custom', delimiter='|', quoting=QUOTE_ALL)
csv.list_dialects() → includes 'custom'
csv.unregister_dialect('custom')
csv.list_dialects() → 'custom' removed
```

### Sniffer
```python
# sniff (infer dialect)
sniffer = csv.Sniffer()
data = 'a,b,c\n1,2,3\n4,5,6\n'
dialect = sniffer.sniff(data)
→ delimiter=',', quotechar='"', escapechar=None, doublequote=False, skipinitialspace=False, quoting=QUOTE_MINIMAL, lineterminator='\r\n'

# sniff with different delimiters
data = 'a|b|c\n1|2|3\n'
sniffer.sniff(data) → delimiter='|'

data = 'a\tb\tc\n1\t2\t3\n'
sniffer.sniff(data) → delimiter='\t'

# sniff with delimiters arg (restrict set to try)
data = 'a:b:c\n1:2:3\n'
sniffer.sniff(data, delimiters=':;') → delimiter=':'

# has_header (detect header row)
sniffer.has_header('name,age,city\nAlice,30,NYC\nBob,25,LA\n') → True
sniffer.has_header('1,2,3\n4,5,6\n7,8,9\n') → False
```

### Type Coercion in Writer
```python
# QUOTE_NONNUMERIC: quote non-numeric fields
writer = csv.writer(out, quoting=QUOTE_NONNUMERIC)
writer.writerow(['text', 3.14, 42]) → '"text",3.14,42\r\n'
# float and int unquoted; str quoted

writer.writerow(['text', '3.14', '42']) → '"text","3.14","42"\r\n'
# all strings quoted
```

### Edge: Multiple Quotes and Delimiters
```python
# Multiple quotes in field
reader(io.StringIO('"a""b","c"""')) → ['a"b', 'c"']

# Quote after field value (invalid → preserved)
reader(io.StringIO('a,"b",c,"d"e')) → ['a', 'b', 'c', 'de']
# "d"e parsed as "d" + e (content after close quote continues)

# Space outside quotes
reader(io.StringIO(' "a" , "b" , "c" ')) → [' "a" ', ' "b" ', ' "c" ']
# spaces not stripped; quotes preserved as part of unquoted field
```

---

## 4. HAZARDS

### 1. **Dict Key Ordering Hazard (PYRST SPECIFIC)**
- DictReader/DictWriter preserve **header order**, not insertion order.
- Probe: `data = 'z,a,m\n1,2,3\n'; reader = DictReader(io.StringIO(data)); next(reader).keys() → ['z', 'a', 'm']`
- **Pyrst hazard:** pyrst dicts iterate in SORTED-KEY order (not insertion). Header fields 'z,a,m' will iterate as ['a', 'm', 'z'] in pyrst, breaking field order.
- **Flag:** DictReader/DictWriter output will not preserve header order under pyrst iteration; output CSV will have fields in alphabetical order.

### 2. **Line Terminator Variation**
- Writer defaults to `\r\n` (CRLF); reader transparently accepts LF, CRLF, or CR.
- Reader requires `newline=''` in real file open() to handle raw CR; StringIO bypasses this.
- **Hazard:** Parity tests must normalize output to the written lineterminator (or control it explicitly).

### 3. **Newline-in-Unquoted-Field Error**
- Real files opened without `newline=''` will raise `Error: new-line character seen in unquoted field...` if CSV contains bare CR.
- StringIO does not validate; file-based tests will fail differently.
- **Flag:** File-object dependence — pyrst implementation must handle or defer file I/O semantics.

### 4. **Float Representation in QUOTE_NONNUMERIC**
- `writer(quoting=QUOTE_NONNUMERIC).writerow([3.14])` → `3.14` (unquoted, float repr).
- Float str() output is locale-independent but may have precision issues.
- **Hazard:** Minimal in practice (3.14 is stable), but numeric literal repr could differ if pyrst has different float repr.

### 5. **Field Size Limit Global State**
- `csv.field_size_limit()` is module-level mutable state (sets/gets a global default).
- **Pyrst constraint:** No module-level mutable state (G2).
- **Flag:** Cannot port `field_size_limit()` as a stateful global; must be a const or removed.

### 6. **Dialect Registration Global State**
- `register_dialect()`, `unregister_dialect()` mutate module-level dialect registry.
- **Pyrst constraint:** No module-level mutable state (G2).
- **Flag:** Cannot port dialect registration; pre-built dialects (excel, excel_tab, unix) only; no dynamic registration.

### 7. **Type Coercion Hazard**
- Writer accepts int, float, bool, None and coerces to str without failing: `123 → "123"`, `None → ""`, `True → "True"`.
- Reader always returns str (no numeric inference).
- **Hazard:** Parity requires that writer input is already str; non-str types should fail or be wrapped by caller.

### 8. **Unicode and Escape Sequences**
- `\n`, `\r`, `\t` in fields are embedded (not escaped as `\\n`, `\\r`, `\\t`) unless using escapechar with QUOTE_NONE.
- **Hazard:** Round-trip is lossless; no ambiguity, but care needed in test data literals.

### 9. **restkey Behavior in DictReader**
- Extra fields collected into a list under the restkey: `{'a': '1', 'b': '2', '_extra': ['3']}`.
- **Hazard:** The value is a list, not a scalar; iteration order of extras matters if pyrst sorts.

---

## 5. GATED (Pyrst Constraints Hit)

| Gate | API Part | Issue | Design-Around |
|------|----------|-------|----------------|
| **G2: No module-level mutable state** | `field_size_limit(new_limit)` | Stateful global register. | Remove or convert to a const (e.g., `FIELD_SIZE_LIMIT = 131072`). |
| **G2: No module-level mutable state** | `register_dialect(name, ...)`, `unregister_dialect(name)` | Mutates dialect registry. | Provide only built-in dialects (excel, excel_tab, unix); no dynamic registration. |
| **G3: No dotted submodules** | None in current scope (csv.Sniffer is flat). | N/A | N/A |
| **G4: No *args/**kwargs variadics** | `DictReader(f, *args, **kwargs)`, `DictWriter(f, fieldnames, *args, **kwargs)` | Fallback for dialect and other args. | Explicit keyword args only: `dialect='excel'`, not `*args`. |
| **G7: No bytes type** | Reader/Writer work on str (StringIO) only; no BytesIO support. | Writer rejects BytesIO: `TypeError: a bytes-like object is required, not 'str'`. | Design around file-object abstraction; implement only StringIO paths. |
| **G9: i64 ints, no bignum** | `field_size_limit()` takes large ints; default 131072. | No bignum concern; i64 covers range. | Minimal risk; field size limit can be i64. |

---

## 6. PARITY PLAN

38 Parity Cases (dual-run safe, using repr() to control formatting):

```python
# 1. reader basic row
io.StringIO('a,b,c\n1,2,3\n')
Expected: [['a','b','c'], ['1','2','3']]

# 2. reader empty input
io.StringIO('')
Expected: []

# 3. reader single field
io.StringIO('x')
Expected: [['x']]

# 4. reader quoted empty
io.StringIO('""')
Expected: [['']]

# 5. reader trailing empty field
io.StringIO('a,b,')
Expected: [['a','b','']]

# 6. reader only delimiters
io.StringIO(',,,')
Expected: [['','','','']]

# 7. reader multiline in quotes
io.StringIO('"a","b\nc","d"')
Expected: [['a','b\nc','d']]

# 8. reader quote escaping (doublequote)
io.StringIO('"a""b","c"""')
Expected: [['a"b','c"']]

# 9. reader with delimiter space
io.StringIO('a b c').reader(delimiter=' ')
Expected: [['a','b','c']]

# 10. reader with delimiter pipe
io.StringIO('a|b|c').reader(delimiter='|')
Expected: [['a','b','c']]

# 11. reader skipinitialspace
io.StringIO('a, b, c').reader(skipinitialspace=True)
Expected: [['a','b','c']]

# 12. reader tab delimiter
io.StringIO('a\tb\tc')
Expected: [['a\tb\tc']]  # tab not recognized as delimiter by default

# 13. reader tab delimiter explicit
io.StringIO('a\tb\tc').reader(delimiter='\t')
Expected: [['a','b','c']]

# 14. writer basic row
writerow(['a','b','c'])
Expected: 'a,b,c\r\n'

# 15. writer with comma in field
writerow(['a','b,c','d'])
Expected: 'a,"b,c",d\r\n'

# 16. writer with quote in field
writerow(['a"b','c'])
Expected: '"a""b",c\r\n'

# 17. writer with newline in field
writerow(['a\nb','c'])
Expected: '"a\nb",c\r\n'

# 18. writer QUOTE_ALL
writer(quoting=QUOTE_ALL).writerow(['a','b'])
Expected: '"a","b"\r\n'

# 19. writer QUOTE_NONE
writer(quoting=QUOTE_NONE, escapechar='\\').writerow(['a,b','c'])
Expected: 'a\\,b,c\r\n'

# 20. writer QUOTE_NONNUMERIC with float
writer(quoting=QUOTE_NONNUMERIC).writerow(['text', 3.14, 42])
Expected: '"text",3.14,42\r\n'

# 21. writer custom lineterminator
writer(lineterminator='\n').writerow(['a','b'])
Expected: 'a,b\n'

# 22. writer type coercion (int)
writerow(['a', 123, 'b'])
Expected: 'a,123,b\r\n'

# 23. writer type coercion (None)
writerow(['a', None, 'b'])
Expected: 'a,,b\r\n'

# 24. writer type coercion (bool)
writerow(['a', True, 'b'])
Expected: 'a,True,b\r\n'

# 25. writer writerows batch
writerows([['a','b'], ['1','2']])
Expected: 'a,b\r\n1,2\r\n'

# 26. DictReader basic
DictReader(io.StringIO('name,age\nAlice,30\nBob,25\n'))
Expected: [{'name':'Alice','age':'30'}, {'name':'Bob','age':'25'}]

# 27. DictReader with fieldnames
DictReader(io.StringIO('30,Alice\n25,Bob\n'), fieldnames=['age','name'])
Expected: [{'age':'30','name':'Alice'}, {'age':'25','name':'Bob'}]

# 28. DictReader restval
DictReader(io.StringIO('a,b\n1,2\n'), restval='X').next()
Expected: {'a':'1','b':'2'}  # restval used for missing fields only

# 29. DictReader restkey with extra fields
DictReader(io.StringIO('a,b\n1,2,3\n'), restkey='_extra').next()
Expected: {'a':'1','b':'2','_extra':['3']}

# 30. DictWriter basic
DictWriter(out, fieldnames=['name','age']).writeheader()
Expected: 'name,age\r\n'

# 31. DictWriter writerow
DictWriter(out, fieldnames=['name','age']).writerow({'name':'Alice','age':'30'})
Expected: 'Alice,30\r\n'

# 32. DictWriter restval for missing field
DictWriter(out, fieldnames=['a','b'], restval='X').writerow({'a':'1'})
Expected: '1,X\r\n'

# 33. DictWriter extrasaction ignore
DictWriter(out, fieldnames=['a','b'], extrasaction='ignore').writerow({'a':'1','b':'2','c':'3'})
Expected: '1,2\r\n'

# 34. Sniffer sniff comma delimiter
Sniffer().sniff('a,b,c\n1,2,3\n')
Expected: dialect.delimiter == ','

# 35. Sniffer sniff pipe delimiter
Sniffer().sniff('a|b|c\n1|2|3\n')
Expected: dialect.delimiter == '|'

# 36. Sniffer sniff tab delimiter
Sniffer().sniff('a\tb\tc\n1\t2\t3\n')
Expected: dialect.delimiter == '\t'

# 37. Sniffer has_header true
Sniffer().has_header('name,age\nAlice,30\n')
Expected: True

# 38. Sniffer has_header false
Sniffer().has_header('1,2,3\n4,5,6\n')
Expected: False
```

---

## 7. TARGET

**Fidelity Estimate: 3/5**

**Dominant Gaps:**

1. **Module-Level Mutable State (G2 gate):**
   - `field_size_limit()` and `register_dialect()` maintain global state, incompatible with pyrst's immutability constraint.
   - **Deferral:** Remove dynamic field_size_limit and dialect registration; hardcode limits and provide only static dialects.

2. **File Object Abstraction (G7, file-object semantics hazard):**
   - CSV module is tightly coupled to file-like iteration semantics (`iterable` in reader, `f` in writer).
   - Pyrst lacks file-typed function parameters (not yet spellable in language); file-object abstraction must be deferred to caller.
   - Reader/writer can work on list-of-strings input/output, but error messages will differ for file I/O edge cases (e.g., `newline=''` validation).
   - **Deferral:** Implement reader/writer to work on line-list abstractions (parse/emit str lists), flag file-object behavior as out-of-scope.

3. **Dict Ordering Hazard (pyrst-specific):**
   - DictReader/DictWriter preserve header order in dicts, but pyrst iterates dicts in sorted-key order.
   - Output CSV from DictWriter will have fields in alphabetical order, not header order.
   - **Deferral:** Document that dict field order is not guaranteed; callers must sort or use explicit fieldname lists; parity tests will not rely on field order in output.

**Secondary Gaps:**

- **Type Coercion:** Writer accepts int/float/bool/None; pyrst must either accept all or require str-only inputs (design choice).
- **Unicode Handling:** Full UTF-8 support; no known hazards, but locale-dependent behavior in has_header heuristic not ported.
- **Sniffer Heuristics:** has_header() uses heuristics (type inference); behavior may vary under pyrst's numeric inference.

**Achievable Subset (3/5 fidelity):**
- ✅ Reader/writer core (delimiter, quotechar, doublequote, quote modes, escape handling)
- ✅ Embedded newlines and quote escaping
- ✅ DictReader/DictWriter (with field-order caveat)
- ✅ Sniffer basic detection (delimiter sniffing; has_header with caveats)
- ✅ Built-in dialects (excel, excel_tab, unix)
- ❌ Field size limit (global state)
- ❌ Dynamic dialect registration
- ❌ File-object error semantics (newline='' validation)
- ❌ Dict insertion-order preservation (pyrst sorts)

---

## Summary

**Module:** csv  
**Surface Count:** 14 (reader, writer, DictReader, DictWriter, Sniffer, Dialect, excel, excel_tab, unix_dialect, register_dialect, unregister_dialect, get_dialect, list_dialects, field_size_limit, Error)  
**Parity Cases:** 38  
**GATED:** 6 (G2×2: field_size_limit, register_dialect; G4×2: DictReader/**kwargs, DictWriter/**kwargs; G7×1: BytesIO rejection; G9: minimal)  
**Target Fidelity:** 3/5 (core CSV parsing/generation solid; state mutation and file-object semantics deferred)

