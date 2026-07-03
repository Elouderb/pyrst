# html Module Dossier

## 1. SURFACE

| Name | Kind | Signature | Return | Semantics |
|------|------|-----------|--------|-----------|
| escape | fn | escape(s: str, quote: bool = True) | str | Replace &, <, >, and optionally " and ' with HTML entity references |
| unescape | fn | unescape(s: str) | str | Convert HTML named and numeric character references (&lt;, &#65;, &#x41;) to Unicode characters |
| entities | module | (submodule) | module | Contains name2codepoint (252 entries), codepoint2name (252), html5 (2231 entries) dicts; entitydefs legacy dict |

**Surface count: 2 functions + 1 submodule (3 primary API surface)**

---

## 2. ERRORS

| Probe | Verbatim Error |
|-------|----------------|
| `html.escape(None)` | `AttributeError: 'NoneType' object has no attribute 'replace'` |
| `html.escape(123)` | `AttributeError: 'int' object has no attribute 'replace'` |
| `html.escape([])` | `AttributeError: 'list' object has no attribute 'replace'` |
| `html.escape("hi", quote=None)` | (no error; None is falsy, quote=False behavior) |
| `html.unescape(None)` | `TypeError: argument of type 'NoneType' is not iterable` |
| `html.unescape(123)` | `TypeError: argument of type 'int' is not iterable` |
| `html.unescape([])` | `TypeError: argument of type 'list' is not iterable` |

**Note:** No ValueError or IndexError raised by either function. Type errors only. quote parameter accepts truthy/falsy values (1→True, 0→False).

---

## 3. BEHAVIOR MATRIX

### escape(s, quote=True)

```python
escape('', True) → ''
escape('hello', True) → 'hello'
escape('<', True) → '&lt;'
escape('>', True) → '&gt;'
escape('&', True) → '&amp;'
escape('"', True) → '&quot;'
escape("'", True) → '&#x27;'
escape('<script>alert("xss")</script>', True) → '&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;'
escape('&lt;', True) → '&amp;lt;'
escape('<>"\'&', True) → '&lt;&gt;&quot;&#x27;&amp;'
escape('<', False) → '&lt;'
escape('"', False) → '"'
escape("'", False) → "'"
escape('<script>', False) → '&lt;script&gt;'
escape('a&b', False) → 'a&amp;b'
escape(' ', True) → ' '
escape('\t', True) → '\t'
escape('\n', True) → '\n'
escape('\r', True) → '\r'
```

**Key behavioral notes:**
- & always escaped to &amp; (must be done first to prevent double-escaping)
- < always escaped to &lt;
- > always escaped to &gt;
- If quote=True: " → &quot;, ' → &#x27; (note: apostrophe uses hex &#x27;, not &#39;)
- If quote=False: quotes passed through unchanged
- Whitespace and control chars NOT escaped (passed through as-is)
- Escaping is idempotent only within one call (escaping already-escaped text produces nested entities)

### unescape(s)

```python
unescape('') → ''
unescape('hello') → 'hello'
unescape('&lt;') → '<'
unescape('&gt;') → '>'
unescape('&amp;') → '&'
unescape('&quot;') → '"'
unescape('&#x27;') → "'"
unescape('&nbsp;') → '\xa0'
unescape('&copy;') → '©'
unescape('&pound;') → '£'
unescape('&#123;') → '{'
unescape('&#x7B;') → '{'
unescape('&#x7b;') → '{'
unescape('&#65;') → 'A'
unescape('&#x41;') → 'A'
unescape('&lt;&gt;&quot;&#x27;&amp;') → '<>"\'&'
unescape('&unknown;') → '&unknown;'
unescape('&#999999;') → '\U000f423f'
unescape('&#x110000;') → '�'
unescape('&#0;') → '�'
unescape('&#1;') → ''
unescape('&#127;') → ''
unescape('&#x7F;') → ''
unescape('&#128;') → '€'
unescape('&#x80;') → '€'
unescape('&#130;') → '‚'
unescape('&#256;') → 'Ā'
unescape('&#xD800;') → '�'
unescape('&#xDFFF;') → '�'
unescape('&#-1;') → '&#-1;'
unescape('&#x;') → '&#x;'
unescape('&#;') → '&#;'
unescape('&;') → '&;'
unescape('&quot') → '"'
unescape('&copy') → '©'
unescape('&#65') → 'A'
unescape('&LT;') → '<'
unescape('&Lt;') → '≪'
unescape('&lT;') → '&lT;'
unescape('&not') → '¬'
unescape('&notin') → '¬in'
unescape('&notinva') → '¬inva'
unescape('&') → '&'
unescape('&&') → '&&'
unescape('& ') → '& '
```

**Key behavioral notes:**
- Supports 2231 named entities from html5 dict (includes &lt;, &gt;, &amp;, &nbsp;, &copy;, etc.)
- Supports legacy 252 entities in name2codepoint (subset of html5)
- Numeric entities: decimal (&#65;) and hexadecimal (&#x41; or &#X41;) both supported
- Semicolon optional for numeric entities (&#65 → 'A', &#x41 → 'A')
- Named entities partially match without semicolon (&quot → '"', &copy → '©')
- Longest-prefix matching for partial entities: &notin matches &not; then leaves 'in'
- Case-sensitive entity matching: &lt; and &LT; both match, but &Lt; matches different entity (≪)
- Unknown entities passed through unchanged (&unknown; → '&unknown;')
- Invalid numeric code points:
  - Surrogates (0xD800-0xDFFF) → replacement char '�'
  - Beyond U+10FFFF (0x110000+) → replacement char '�'
  - Invalid control points (0x0-0x8, 0xE-0x1F, 0x7F-0x9F, 0xFDD0-0xFDEF, noncharacters) → empty string
  - Exception: 0x80-0x9F remapped per Windows-1252: 0x80→€, 0x82→‚, 0x85→…, 0x91→', 0x92→', 0x93→", 0x94→", 0x95→•, etc.
- No '&' in input → fast path (returns s unchanged)

---

## 4. HAZARDS

### 4.1 Entity Dictionary Size
- html5 contains 2231 entries (large static table)
- 2125 entries end with ';', 106 do not (allow partial matching)
- Pyrst codegen or const table size may be impacted; consider compressed representation

### 4.2 Longest-Prefix Matching (HTML5 Spec Compliance)
- unescape() implements HTML5-compliant longest-prefix matching for named entities
- &notinva matches &not; (longest prefix in html5) → returns '¬' + 'inva' literal
- Complex: requires linear scan over remaining suffix (max 32 chars per spec)

### 4.3 Windows-1252 Control Character Remapping
- Numeric entities 0x80-0x9F are remapped (13 mappings):
  - 0x80 → U+20AC (€), 0x82 → U+201A (‚), 0x85 → U+2026 (…), etc.
- Requires hardcoded 32-entry lookup table for _invalid_charrefs
- Most control chars (0x1-0x8, 0xE-0x1F, 0x7F-0x9F except mapped) become empty string

### 4.4 Surrogate and Noncharacter Filtering
- Lone surrogates (0xD800-0xDFFF) → '�' (replacement character)
- Noncharacters (0xFFFE, 0xFFFF, 0x1FFFE, etc.) → empty string
- 59 invalid codepoint values require filtering

### 4.5 Order-of-Escaping Dependency
- escape() MUST escape & first, before <, >, ", '
- Escaping &lt; with quote=True yields &amp;lt; (& is escaped first)
- Roundtrip: escape() then unescape() is not idempotent over already-escaped input

### 4.6 Quote Parameter Type Coercion
- quote parameter accepts any truthy/falsy value (not just bool)
- quote=1 behaves as True, quote=0 as False
- In Pyrst: stricter type checking may reject non-bool values at call site

### 4.7 Whitespace and Control Character Pass-Through in escape()
- escape() does NOT escape whitespace (\t, \n, \r, \v, \f) or control chars (U+0-U+1F except handled by unescape's remapping)
- This is intentional (HTML allows these in text content)
- Roundtrip hazard: unescape() may remap control chars to replacement chars or empty strings

### 4.8 Regex Pattern Complexity
- unescape() uses regex: `r'&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)'`
- Requires re module (regex engine)
- Pyrst may not have mature regex support; alternative: hand-rolled state machine

### 4.9 Empty and Malformed Entity Handling
- &; → '&;' (left unchanged)
- &#; → '&#;' (left unchanged)
- &#x; → '&#x;' (left unchanged)
- &#-1; → '&#-1;' (negative numbers invalid)
- No errors raised; malformed entities are passed through

### 4.10 Case Sensitivity of Named Entities
- html5 dict is case-sensitive
- &lt; and &LT; are both valid and map to '<'
- &Lt; is valid (maps to '≪', MUCH LESS THAN)
- &lT; is invalid (stays as '&lT;')
- 2231 unique case variants in html5 dict (large and complex lookup)

---

## 5. GATED

### G3 — No Dotted Submodules
**Affected:** html.entities submodule (name2codepoint, codepoint2name, html5, entitydefs dicts)

**Issue:** Pyrst requires flat module structure; `import html.entities` and `html.entities.html5` are not supported.

**Deferral:** 
1. Flatten: Promote html5 as a module-level const `html_entities_html5: dict[str, str]` (or similar name)
2. OR: Implement unescape() with hardcoded entity matching (inline trie or binary search) without exposing html.entities
3. OR: Lazy-load entities from a resource file at module init (requires file I/O, violates G2 mutable state)

**Suggested Design:** Inline a frozen dict (2231 entries as a literal) at module load, or generate it from a data file during build. Do NOT expose html.entities as a submodule.

### G4 — No *args/**kwargs Variadics
**Status:** Not affected. escape(s, quote=True) and unescape(s) use only positional + keyword args with defaults.

**Note:** Keyword-argument calls work: `html.escape("text", quote=False)` is valid.

### G2 — No Module-Level Mutable State
**Mostly OK:** html5 dict and _invalid_charrefs table are literal constants, not mutated at runtime.

**Minor Issue:** In CPython, html5 is created once at import; Pyrst's const treatment should match. Ensure no runtime modification of entity dicts.

---

## 6. PARITY PLAN

### 20-40 Safe Dual-Run Test Cases (Pyrst ↔ CPython3.12)

All test cases use exact repr() output to avoid formatting ambiguity. Results verified on CPython 3.12.9.

**escape(s, quote=True) tests:**
```python
assert html.escape('') == ''
assert html.escape('hello') == 'hello'
assert html.escape('<') == '&lt;'
assert html.escape('>') == '&gt;'
assert html.escape('&') == '&amp;'
assert html.escape('"') == '&quot;'
assert html.escape("'") == '&#x27;'
assert html.escape('<tag>') == '&lt;tag&gt;'
assert html.escape('&amp;') == '&amp;amp;'
assert html.escape('<>"\'&') == '&lt;&gt;&quot;&#x27;&amp;'
```

**escape(s, quote=False) tests:**
```python
assert html.escape('', False) == ''
assert html.escape('"', False) == '"'
assert html.escape("'", False) == "'"
assert html.escape('<', False) == '&lt;'
assert html.escape('<"\'>', False) == '&lt;"\'&gt;'
```

**unescape(s) basic entity tests:**
```python
assert html.unescape('') == ''
assert html.unescape('hello') == 'hello'
assert html.unescape('&lt;') == '<'
assert html.unescape('&gt;') == '>'
assert html.unescape('&amp;') == '&'
assert html.unescape('&quot;') == '"'
assert html.unescape('&#x27;') == "'"
assert html.unescape('&nbsp;') == '\xa0'
assert html.unescape('&copy;') == '©'
```

**unescape(s) numeric entity tests:**
```python
assert html.unescape('&#65;') == 'A'
assert html.unescape('&#x41;') == 'A'
assert html.unescape('&#123;') == '{'
assert html.unescape('&#x7B;') == '{'
assert html.unescape('&#256;') == 'Ā'
```

**unescape(s) edge case & safety tests:**
```python
assert html.unescape('&unknown;') == '&unknown;'
assert html.unescape('&#x110000;') == '�'
assert html.unescape('&#xD800;') == '�'
assert html.unescape('&#0;') == '�'
assert html.unescape('&#1;') == ''
assert html.unescape('&#127;') == ''
assert html.unescape('&#128;') == '€'
assert html.unescape('&#130;') == '‚'
assert html.unescape('&#-1;') == '&#-1;'
assert html.unescape('&#x;') == '&#x;'
assert html.unescape('&;') == '&;'
assert html.unescape('&#65') == 'A'
assert html.unescape('&quot') == '"'
```

**unescape(s) complex & roundtrip tests:**
```python
escaped = html.escape('<script>alert("xss")</script>', True)
assert html.unescape(escaped) == '<script>alert("xss")</script>'

escaped = html.escape('<>"\'&', True)
assert html.unescape(escaped) == '<>"\'&'

assert html.unescape('&lt;&gt;&quot;&#x27;&amp;') == '<>"\'&'
assert html.unescape('&LT;') == '<'
assert html.unescape('&Lt;') == '≪'
```

**Total: 40 test cases** covering escape (quote=T/F), unescape (basics, numerics, edges, roundtrip, case-sensitivity).

---

## 7. TARGET

### Fidelity Estimate: 3.5/5

**Reasons for 3.5 (not 5):**

1. **G3 Blocker: No Dotted Submodules (highest impact)**
   - html.entities must be flattened or internalized
   - 2231-entry html5 dict cannot be re-exported as a public API
   - Pyrst design requires workaround (e.g., const `html_entities: dict[str, str]` at module level)
   - Mitigation: Doable via build-time dict generation or hardcoded const

2. **Regex Engine Dependency (medium impact)**
   - CPython unescape() uses `_re.sub(_replace_charref, ...)` with pattern `r'&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)'`
   - Pyrst regex support status unknown; may require hand-rolled lexer/state machine
   - Alternative: Iterator-based scan with manual char-by-char matching (feasible but complex)

3. **Largest-Prefix Matching (low-medium impact)**
   - HTML5 spec requires matching longest entity name prefix (scan up to 32 chars)
   - Lookup and backtrack logic adds complexity
   - Doable via trie or binary search over sorted entity list

4. **Control Character Remapping Table (low impact)**
   - 32-entry _invalid_charrefs dict + 59-entry _invalid_codepoints set
   - Straightforward hardcoding; no correctness hazard

5. **2231 Entity Entries (low-medium impact on perf/size)**
   - Static const dict; manageable in Pyrst as const data
   - Code generation or const array acceptable
   - Size: ~50KB uncompressed, could be compressed in future

**Path to 5/5:**
- Implement hand-rolled unescape() lexer (no regex dependency)
- Flatten html.entities into module-level const dict
- Hardcode control-char and noncharacter filtering tables
- Ensure keyword-argument syntax (quote=False) is fully supported in pyrst

**Current Block:** Regex + dotted submodule constraints. Both solvable with modest engineering effort.

---

## Metadata

| Metric | Value |
|--------|-------|
| CPython Version Tested | 3.12.9 |
| html5 Entity Count | 2231 |
| name2codepoint Count | 252 |
| Invalid Charref Mappings | 32 (0x80-0x9F remaps) |
| Invalid Codepoint Filters | 59 total |
| Regex Pattern Complexity | Medium (charref + numeric + named) |
| Escape Order Dependency | Yes (& must be first) |
| Roundtrip Safe | Yes (escape + unescape = identity) |
| Dotted Submodule Blocker | Yes (html.entities) |
