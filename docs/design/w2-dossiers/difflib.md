# difflib — Sequence Comparison

## SURFACE

| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| SequenceMatcher | class | `SequenceMatcher(isjunk=None, a='', b='', autojunk=True)` | instance | Compares pairs of sequences (strings, lists, tuples) for matching blocks |
| SequenceMatcher.set_seqs | method | `(a, b)` | None | Replace both sequences |
| SequenceMatcher.set_seq1 | method | `(a)` | None | Replace first sequence |
| SequenceMatcher.set_seq2 | method | `(b)` | None | Replace second sequence |
| SequenceMatcher.find_longest_match | method | `(alo=0, ahi=None, blo=0, bhi=None)` → Match | namedtuple Match(a, b, size) | Find longest contiguous matching subsequence in specified ranges |
| SequenceMatcher.get_matching_blocks | method | `()` | List[Match] | Return all maximal matching blocks; last element always Match(len(a), len(b), 0) |
| SequenceMatcher.get_opcodes | method | `()` | List[tuple] | Return sequence of (tag, i1, i2, j1, j2) where tag ∈ {replace,delete,insert,equal} |
| SequenceMatcher.get_grouped_opcodes | method | `(n=3)` | List[List[tuple]] | Group opcodes by context; each group has n context lines before/after changes |
| SequenceMatcher.ratio | method | `()` | float | Return 2.0 * M / T (M=matching elements, T=total elements) |
| SequenceMatcher.quick_ratio | method | `()` | float | Upper bound on ratio; computes quickly |
| SequenceMatcher.real_quick_ratio | method | `()` | float | Very fast upper bound; always ≥ ratio() |
| get_close_matches | function | `(word, possibilities, n=3, cutoff=0.6)` | List[str] | Return up to n words from possibilities that are close to word; cutoff ∈ [0.0, 1.0] |
| unified_diff | function | `(a, b, fromfile='', tofile='', fromfiledate='', tofiledate='', n=3, lineterm='\n')` | Iterator[str] | Return unified diff of sequences a, b as unified diff format (RFC 3881) |
| ndiff | function | `(a, b, linejunk=None, charjunk=IS_CHARACTER_JUNK)` | Iterator[str] | Return delta lines showing differences; lines start with ' ', '-', '+', or '?' |
| context_diff | function | `(a, b, fromfile='', tofile='', fromfiledate='', tofiledate='', n=3, lineterm='\n')` | Iterator[str] | Return context diff format (lines marked with '!', '-', '+') |
| diff_bytes | function | `(dfunc, a, b, fromfile=b'', tofile=b'', fromfiledate=b'', tofiledate=b'', n=3, lineterm=b'\n')` | Iterator[bytes] | Wrap any diff function to work on bytes sequences |
| IS_CHARACTER_JUNK | const | callable | function | Returns True if char is whitespace (space, tab); used by ndiff for charjunk param |
| IS_LINE_JUNK | const | callable | function | Returns True if line is empty or whitespace-only; used by ndiff for linejunk param |
| restore | function | `(delta, which)` | Iterator[str] | Reconstruct lines from ndiff delta; which ∈ {1, 2} |
| Differ | class | `Differ(linejunk=IS_LINE_JUNK, charjunk=IS_CHARACTER_JUNK)` | instance | Produces ndiff-style deltas between sequences (wrapper around ndiff) |
| HtmlDiff | class | `HtmlDiff(tabsize=8, wrapcolumn=None)` | instance | Produces side-by-side HTML diff output (beyond scope) |
| Match | class | namedtuple | Match(a, b, size) | Result of matching blocks; a=index in seq1, b=index in seq2, size=length of match |

## ERRORS

| Probe | Exception Type | Message |
|-------|---|---------|
| `get_close_matches("apple", ["apple"], n=0)` | ValueError | n must be > 0: 0 |
| `get_close_matches("apple", ["apple"], n=-1)` | ValueError | n must be > 0: -1 |
| `get_close_matches("apple", ["apple"], cutoff=1.5)` | ValueError | cutoff must be in [0.0, 1.0]: 1.5 |
| `get_close_matches("apple", ["apple"], cutoff=-0.5)` | ValueError | cutoff must be in [0.0, 1.0]: -0.5 |
| `get_close_matches(123, ["apple"])` | TypeError | 'int' object is not iterable |
| `get_close_matches("apple", ["apple"], cutoff="0.5")` | TypeError | '<=' not supported between instances of 'float' and 'str' |
| `get_close_matches("apple", ["apple"], n="3")` | TypeError | '>' not supported between instances of 'str' and 'int' |
| `unified_diff([[1], [2]], [[1], [3]])` | TypeError | lines to compare must be str, not list ([1]) |
| `ndiff([["a"], ["b"]], [["a"], ["c"]])` | TypeError | unhashable type: 'list' |

## BEHAVIOR MATRIX

### SequenceMatcher basics

| Input (a, b) | ratio | matching_blocks | opcodes | notes |
|---|---|---|---|---|
| ("", "") | 1.0 | [Match(0, 0, 0)] | [] | empty sequences match perfectly |
| ("abc", "") | 0.0 | [Match(3, 0, 0)] | [('delete', 0, 3, 0, 0)] | all of a must be deleted |
| ("", "abc") | 0.0 | [Match(0, 3, 0)] | [('insert', 0, 0, 0, 3)] | all of b must be inserted |
| ("abc", "abc") | 1.0 | [Match(0, 0, 3), Match(3, 3, 0)] | [('equal', 0, 3, 0, 3)] | identical sequences |
| ("abc", "xyz") | 0.0 | [Match(3, 3, 0)] | [('replace', 0, 3, 0, 3)] | no matches at all |
| ("abcd", "aXcd") | 0.75 | [Match(0, 0, 1), Match(2, 2, 2), Match(4, 4, 0)] | [('equal', 0, 1, 0, 1), ('replace', 1, 2, 1, 2), ('equal', 2, 4, 2, 4)] | partial match with replacement |
| ("ab", "aXb") | 0.6667 | [Match(0, 0, 1), Match(1, 2, 1), Match(2, 3, 0)] | [('equal', 0, 1, 0, 1), ('insert', 1, 1, 1, 2), ('equal', 1, 2, 2, 3)] | insert in middle |
| ("abcd", "acd") | 0.75 | [Match(0, 0, 1), Match(2, 1, 2), Match(4, 3, 0)] | [('equal', 0, 1, 0, 1), ('delete', 1, 2, 1, 1), ('equal', 2, 4, 1, 3)] | delete from a |
| ("abc", "abcX") | 0.8571 | [Match(0, 0, 3), Match(3, 3, 0)] | [('equal', 0, 3, 0, 3), ('insert', 3, 3, 3, 4)] | insert at end of b |
| ("abcX", "abc") | 0.8571 | [Match(0, 0, 3), Match(3, 3, 0)] | [('equal', 0, 3, 0, 3), ('delete', 3, 4, 3, 3)] | delete from end of a |
| ("abcdef", "aXcXef") | 0.6667 | [Match(0, 0, 1), Match(2, 2, 1), Match(3, 3, 1), Match(4, 4, 2), Match(6, 6, 0)] | [('equal', 0, 1, 0, 1), ('replace', 1, 2, 1, 2), ('equal', 2, 3, 2, 3), ('replace', 3, 4, 3, 4), ('equal', 4, 6, 4, 6)] | multiple replacements |
| ([1, 2, 3], [1, 2, 3]) | 1.0 | [Match(0, 0, 3), Match(3, 3, 0)] | [('equal', 0, 3, 0, 3)] | list comparison works |
| ([1, 2, 3, 4], [1, 'x', 3, 4]) | 0.75 | [Match(0, 0, 1), Match(2, 2, 2), Match(4, 4, 0)] | [('equal', 0, 1, 0, 1), ('replace', 1, 2, 1, 2), ('equal', 2, 4, 2, 4)] | lists can have mixed types |
| ((1, 2, 3), (1, 2, 4)) | 0.6667 | [Match(0, 0, 2), Match(3, 3, 0)] | [('equal', 0, 2, 0, 2), ('replace', 2, 3, 2, 3)] | tuple comparison works |
| ("aaa", "aaa") | 1.0 | [Match(0, 0, 3), Match(3, 3, 0)] | [('equal', 0, 3, 0, 3)] | repeated elements |
| ("aaa", "aab") | 0.6667 | [Match(0, 0, 2), Match(3, 3, 0)] | [('equal', 0, 2, 0, 2), ('replace', 2, 3, 2, 3)] | duplicates with replacement |
| ("abab", "baba") | 0.75 | [Match(0, 1, 1), Match(2, 0, 2), Match(4, 4, 0)] | [('delete', 0, 1, 0, 0), ('equal', 1, 2, 0, 1), ('insert', 2, 2, 1, 2), ('equal', 2, 4, 2, 4)] | rotated sequences |

### SequenceMatcher ratios (formula: 2.0 * M / T)

| Input | ratio | quick_ratio | real_quick_ratio | notes |
|---|---|---|---|---|
| ('', '') | 1.0 | 1.0 | 1.0 | empty always matches |
| ('a', 'b') | 0.0 | 0.0 | 1.0 | quick_ratio equals ratio; real_quick overestimates |
| ('abc', 'abc') | 1.0 | 1.0 | 1.0 | identical |
| ('abc', 'xyz') | 0.0 | 0.0 | 1.0 | no matches; real_quick very loose bound |
| ('abcdef', 'abcdefgh') | 0.8571 | 0.8571 | 0.8571 | quick_ratio matches ratio for slightly different |
| ('a'*100, 'b'*100) | 0.0 | 0.0 | 1.0 | long non-matching sequences |

### get_close_matches behavior

| word | possibilities | n | cutoff | result | notes |
|---|---|---|---|---|---|
| "apple" | ["apple", "apples", "apply", "banana"] | 3 | 0.6 | ["apple", "apples", "apply"] | default returns 3, all ≥ 0.6 |
| "apple" | ["apple", "apples", "apply", "banana"] | 2 | 0.6 | ["apple", "apples"] | limited to n=2 |
| "apple" | ["apple", "apples", "apply", "banana"] | 3 | 0.8 | ["apple", "apples", "apply"] | cutoff=0.8 still includes all three |
| "zzz" | ["apple", "apples", "apply", "banana"] | 3 | 0.6 | [] | no matches above cutoff |
| "apple" | [] | 3 | 0.6 | [] | empty possibilities |
| "apple" | ["apple"] | 1 | 0.6 | ["apple"] | exact match |
| "abc" | ["xyz", "aaa"] | 10 | 0.0 | ["aaa", "xyz"] | low cutoff; returns all, sorted by ratio desc |
| "abc" | ["abcd"] | 1 | 0.75 | ["abcd"] | ratio=0.8571 passes cutoff |
| "apple" | ["apple", "apples", "apply"] | 1000 | 0.0 | ["apple", "apples", "apply"] | n larger than results returns all matches |

### unified_diff behavior

| a | b | n | headers | output snippet | notes |
|---|---|---|---|---|---|
| ["a\n", "b\n", "c\n"] | ["a\n", "b\n", "c\n"] | 3 | '', '' | [] | no diff; empty output |
| ["a\n"] | ["b\n"] | 3 | 'file1', 'file2' | ['--- file1\n', '+++ file2\n', '@@ -1 +1 @@\n', '-a\n', '+b\n'] | headers show file names |
| ["a\n", "b\n", "c\n"] | ["a\n", "X\n", "c\n"] | 1 | '', '' | ['--- \n', '+++ \n', '@@ -2,3 +2,3 @@\n', ' b\n', '-c\n', '+X\n', ' d\n'] | context lines reduced |
| ["a\n", "b\n", "c\n"] | ["a\n", "X\n", "c\n"] | 2 | '', '' | ['--- \n', '+++ \n', '@@ -1,5 +1,5 @@\n', ' a\n', ' b\n', '-c\n', '+X\n', ' d\n', ' e\n'] | more context lines |
| ["a\n"] | [] | 3 | '', '' | ['--- \n', '+++ \n', '@@ -1 +0,0 @@\n', '-a\n'] | deletion |
| [] | ["a\n"] | 3 | '', '' | ['--- \n', '+++ \n', '@@ -0,0 +1 @@\n', '+a\n'] | insertion |
| ["a", "b"] | ["a", "c"] | 3 | '', '' | ['--- \n', '+++ \n', '@@ -1,2 +1,2 @@\n', ' a', '-b', '+c'] | no newlines; lineterm='\n' still added to headers |
| ["a\n", "b\n"] | ["a\n", "c\n"] | 3 | '', '' | ['--- \n', '+++ \n', '@@ -1,2 +1,2 @@\n', ' a\n', '-b\n', '+c\n'] | with lineterm='' |

### ndiff behavior

| a | b | linejunk | charjunk | output | notes |
|---|---|---|---|---|---|
| [] | [] | None | IS_CHARACTER_JUNK | [] | empty |
| ["a\n"] | [] | None | IS_CHARACTER_JUNK | ['- a\n'] | deletion marked |
| [] | ["a\n"] | None | IS_CHARACTER_JUNK | ['+ a\n'] | insertion marked |
| ["abc\n"] | ["aXc\n"] | None | IS_CHARACTER_JUNK | ['- abc\n', '?  ^\n', '+ aXc\n', '?  ^\n'] | char-level diff with '?' markers |
| ["line1\n", "line2\n", "line3\n"] | ["line1\n", "line2_changed\n", "line3\n"] | None | IS_CHARACTER_JUNK | ['  line1\n', '- line2\n', '+ line2_changed\n', '  line3\n'] | unchanged marked with '  ' |
| ["a", "b"] | ["a", "c"] | None | IS_CHARACTER_JUNK | ['  a', '- b', '+ c'] | no newlines; still works |
| ["code\n", "\n", "more\n"] | ["code\n", "stuff\n", "more\n"] | is_blank | IS_CHARACTER_JUNK | ['  code\n', '- \n', '+ stuff\n', '  more\n'] | linejunk doesn't suppress blank line diff |

### get_opcodes composition

| Input | opcodes | reconstructs both sequences | notes |
|---|---|---|---|
| ("ABRACADABRA", "YABBADABBADOO") | [('equal', 0, 2, 1, 3), ...] | yes | final Match always (len(a), len(b), 0) |
| ("a", "aXb") | [('equal', 0, 1, 0, 1), ('insert', 1, 1, 1, 2), ('equal', 1, 2, 2, 3)] | yes | opcodes fully decompose sequences |

### matching_blocks invariants

| a | b | blocks[-1] | blocks cover distinct ranges | notes |
|---|---|---|---|---|
| "abc" | "abc" | Match(3, 3, 0) | yes | final always has size=0 at end |
| "abc" | "xyz" | Match(3, 3, 0) | yes | even when no real matches |
| "ABRACADABRA" | "YABBADABBADOO" | Match(11, 13, 0) | yes | indices match sequence lengths |

### find_longest_match

| a | b | alo, ahi, blo, bhi | result | notes |
|---|---|---|---|---|
| "abcdef" | "facbde" | 0, 6, 0, 6 | Match(3, 4, 2) | finds "de" |
| "abcdef" | "facbde" | 1, 4, 1, 4 | Match(1, 3, 1) | subset of full search |
| "abc" | "xyz" | 0, 3, 0, 3 | Match(0, 0, 0) | no match returns sentinel |
| "abcdefgh" | "cdefijkl" | 0, 8, 0, 8 | Match(2, 0, 4) | finds "cdef" |

## HAZARDS

1. **Float formatting**: ratio() returns Python floats (e.g., 0.6666666666666666); repr and str differ on precision. Use == for exact comparisons only if values are computed the same way. No rounding issues in pyrst; just flag that float comparisons may need tolerance.

2. **Dict iteration order**: Not relevant to difflib scope (no dicts returned), but opcodes and blocks are lists so iteration order is deterministic.

3. **String newlines**: unified_diff and ndiff do NOT add or strip newlines; they pass through lineterm parameter. Lines without '\n' in input won't have it added to output. The header lines (--- +++ @@) always get lineterm appended, but content lines depend on input.

4. **Unified diff header format**: Hunk header is `@@ -start,count +start,count @@` but if count=1, the ",1" is omitted: `@@ -1 +1 @@`. Edge case: empty diffs (no changes) produce no output at all, not even headers.

5. **Character vs. line junk**: IS_CHARACTER_JUNK returns True for ' ' and '\t'; IS_LINE_JUNK returns True for empty strings and '\n'. These are used by ndiff; passing custom junk functions changes behavior. Callable signature is not validated; TypeError on call if wrong signature.

6. **opcodes tuple structure**: Opcodes are plain tuples, not named tuples (unlike Match). Order is (tag_str, i1, i2, j1, j2) where tags are string constants. No type safety; easy to misindex.

7. **Matching blocks endpoint sentinel**: get_matching_blocks() always ends with a zero-size Match at (len(a), len(b), 0). This is guaranteed and should be relied on for loop termination.

8. **quick_ratio vs real_quick_ratio**: quick_ratio can equal ratio (is a true bound), but real_quick_ratio may be >> ratio; both are >= ratio. Don't assume quick_ratio < ratio.

9. **Locale/encoding**: All string operations are byte-level (UTF-8 strings are sequences of code points). No locale sensitivity. Unicode normalization not applied.

10. **autojunk parameter behavior**: Affects how frequently-repeated elements are treated. On by default but doesn't always change results (depends on length and repetition frequency). No API to query which elements were junked.

11. **get_close_matches sorts by similarity, then preserves input order**: Returns highest-ratio matches first; ties are broken by input order in possibilities list.

12. **ndiff '?' lines**: The '?' line immediately follows a '- ' or '+ ' line and shows character-level differences with '^' markers. Not a separate delta; purely informational.

## GATED

| Gate | API Part | Issue | Suggested Deferral/Workaround |
|------|----------|-------|-----|
| **G4** (no *args/**kwargs) | ndiff(a, b, linejunk=None, charjunk=IS_CHARACTER_JUNK) | charjunk is a keyword-only param with a default callable; pyrst v1 kwargs now support defaults | Use keyword call site: `ndiff(a, b, charjunk=my_fn)` but cannot omit and rely on default |
| **G3** (no dotted submodules) | difflib.HtmlDiff, difflib.Differ | HtmlDiff and Differ are in the same module; not an issue | No workaround needed; flat import |
| **G1** (no bytes type) | diff_bytes function | Takes and returns bytes sequences | Omit diff_bytes from initial port; pyrstr strings are always text |
| **G1** (no bytes type) | unified_diff(..., fromfiledate=b'', tofiledate=b'') | diff_bytes uses bytes for date headers | If including diff_bytes later, date headers must be str only |

**No gates on core scope:** SequenceMatcher, get_close_matches, unified_diff, ndiff all use only str/list/tuple sequences, floats, and ints. Match is a namedtuple (available). Callables for isjunk/linejunk/charjunk are callable objects, not decorators (G5 not hit).

## PARITY PLAN

```python
# Core SequenceMatcher behavior (16 cases)
SequenceMatcher(None, "", "").ratio() == 1.0
SequenceMatcher(None, "abc", "abc").ratio() == 1.0
SequenceMatcher(None, "abc", "xyz").ratio() == 0.0
SequenceMatcher(None, "abcd", "aXcd").ratio() == 0.75
SequenceMatcher(None, "ab", "aXb").ratio() == 2.0/3.0
SequenceMatcher(None, "abc", "").ratio() == 0.0
SequenceMatcher(None, "", "abc").ratio() == 0.0
SequenceMatcher(None, [1, 2, 3], [1, 2, 3]).ratio() == 1.0
SequenceMatcher(None, [1, 2, 3, 4], [1, 'x', 3, 4]).ratio() == 0.75
SequenceMatcher(None, (1, 2, 3), (1, 2, 4)).ratio() == 2.0/3.0
SequenceMatcher(None, "aaa", "aab").ratio() == 2.0/3.0
SequenceMatcher(None, "abab", "baba").ratio() == 0.75
SequenceMatcher(None, "abc", "abc").get_matching_blocks()[0] == Match(a=0, b=0, size=3)
SequenceMatcher(None, "abc", "xyz").get_matching_blocks()[0].size == 0
len(SequenceMatcher(None, "abc", "def").get_matching_blocks()) == 1
SequenceMatcher(None, "ABRACADABRA", "YABBADABBADOO").get_matching_blocks()[-1] == Match(a=11, b=13, size=0)

# Opcodes (10 cases)
SequenceMatcher(None, "abc", "abc").get_opcodes() == [('equal', 0, 3, 0, 3)]
SequenceMatcher(None, "abc", "xyz").get_opcodes()[0][0] == 'replace'
SequenceMatcher(None, "abc", "abcX").get_opcodes()[-2][0] == 'insert'
SequenceMatcher(None, "abcX", "abc").get_opcodes()[-2][0] == 'delete'
SequenceMatcher(None, "ab", "aXb").get_opcodes()[1][0] == 'insert'
SequenceMatcher(None, "abcd", "acd").get_opcodes()[1][0] == 'delete'
len(SequenceMatcher(None, "abc", "abc").get_opcodes()) == 1
len(SequenceMatcher(None, "abcdef", "aXcXef").get_opcodes()) == 5
list(SequenceMatcher(None, "abc", "abc").get_opcodes())[0][1:] == (0, 3, 0, 3)

# find_longest_match (4 cases)
SequenceMatcher(None, "abcdef", "facbde").find_longest_match(0, 6, 0, 6).size >= 1
SequenceMatcher(None, "abc", "xyz").find_longest_match(0, 3, 0, 3).size == 0
SequenceMatcher(None, "abcdefgh", "cdefijkl").find_longest_match(0, 8, 0, 8).a == 2
SequenceMatcher(None, "abcdefgh", "cdefijkl").find_longest_match(0, 8, 0, 8).size == 4

# Ratio formulae (6 cases)
2 * sum(m.size for m in SequenceMatcher(None, "abcd", "aXcY").get_matching_blocks()) / (4 + 4) == SequenceMatcher(None, "abcd", "aXcY").ratio()
SequenceMatcher(None, "", "").quick_ratio() == 1.0
SequenceMatcher(None, "a", "b").quick_ratio() == 0.0
SequenceMatcher(None, "a", "b").real_quick_ratio() == 1.0
SequenceMatcher(None, "abcdef", "abcdefgh").quick_ratio() >= SequenceMatcher(None, "abcdef", "abcdefgh").ratio()
SequenceMatcher(None, "abc", "xyz").real_quick_ratio() >= SequenceMatcher(None, "abc", "xyz").ratio()

# get_close_matches (12 cases)
get_close_matches("apple", ["apple", "apples", "apply", "banana"]) == ["apple", "apples", "apply"]
get_close_matches("apple", ["apple", "apples", "apply", "banana"], n=2) == ["apple", "apples"]
get_close_matches("apple", ["apple", "apples", "apply", "banana"], n=1) == ["apple"]
get_close_matches("zzz", ["apple", "apples", "apply"]) == []
get_close_matches("apple", []) == []
get_close_matches("apple", ["apple"], cutoff=1.0) == ["apple"]
get_close_matches("abc", ["abcd"], cutoff=0.74) == ["abcd"]
get_close_matches("abc", ["xyz", "aaa"], n=10, cutoff=0.0) == ["aaa", "xyz"]
len(get_close_matches("apple", ["apple", "apples", "apply"], n=1000)) == 3
get_close_matches("apple", ["apply", "apples", "apple"], n=1)[0] == "apple"
len(get_close_matches("a", ["b"], cutoff=0.6)) == 0
get_close_matches("abc", ["abcd"], cutoff=0.8571) == ["abcd"]

# unified_diff basics (10 cases)
list(unified_diff(["a\n"], ["b\n"])) != []
list(unified_diff(["a\n", "b\n", "c\n"], ["a\n", "b\n", "c\n"])) == []
list(unified_diff(["a\n"], ["b\n"], fromfile="f1", tofile="f2"))[0] == "--- f1\n"
list(unified_diff(["a\n"], ["b\n"], fromfile="f1", tofile="f2"))[1] == "+++ f2\n"
list(unified_diff([], ["a\n"]))[2][0:2] == "@@"
list(unified_diff(["a\n"], []))[2][0:2] == "@@"
list(unified_diff(["a\n"], ["b\n"], n=0))[2].count(",") == 2
all(line.endswith("\n") for line in unified_diff(["a\n", "b\n"], ["a\n", "c\n"])) == True
list(unified_diff(["a", "b"], ["a", "c"]))[3] == " a"
list(unified_diff(["a\n", "b\n"], ["a\n", "c\n"], lineterm=''))[0] == "--- "

# ndiff basics (8 cases)
list(ndiff(["a\n"], ["b\n"]))[0][0] == "-"
list(ndiff(["a\n"], ["b\n"]))[1][0] == "+"
list(ndiff(["a\n", "b\n"], ["a\n", "b\n"]))[0][0] == " "
list(ndiff([], [])) == []
list(ndiff([], ["a\n"]))[0][0] == "+"
list(ndiff(["a\n"], []))[0][0] == "-"
all(len(line) > 0 for line in ndiff(["abc\n"], ["aXc\n"])) == True
list(ndiff(["abc\n"], ["aXc\n"]))[1][0] == "?"

# Error boundary (4 cases)
try: get_close_matches("apple", ["apple"], n=0); assert False
except ValueError: pass
try: get_close_matches("apple", ["apple"], cutoff=1.5); assert False
except ValueError: pass
try: unified_diff([[1], [2]], [[1], [3]]); assert False
except TypeError: pass
try: ndiff([["a"]], [["b"]]); assert False
except TypeError: pass
```

## TARGET

**Fidelity: 4/5**

**Rationale:**
1. **Core algorithms work** (matching blocks, opcodes, ratio formulae): SequenceMatcher is a straightforward dynamic-programming sequence-comparison engine. The Match namedtuple, opcode tuples, and ratio formula are all computable in pyrst with no special library dependencies. Single inheritance on SequenceMatcher, all key methods are pure (no global state).

2. **Callables for junk predicates** (isjunk, linejunk, charjunk): pyrst supports callable params and keyword defaults (G4 now landing). IS_CHARACTER_JUNK and IS_LINE_JUNK are simple pure functions; easy to port as module-level consts.

3. **String/list/tuple/int/float only**: No bytes, no bignum, no exotic types. All IO is str sequences and floats; well within pyrst's capabilities.

4. **Generators** (unified_diff, ndiff return iterators): pyrst has lazy generators (Iterator[T] with yield). No issue.

**Why not 5/5:**
- **diff_bytes requires bytes type** (G1 constraint): Omit from initial port or stub it. Not in core scope but in public API.
- **HtmlDiff and Differ classes**: Beyond scope but in public API. Differ is a thin wrapper around ndiff; HtmlDiff requires string formatting and HTML escaping (no issue, but scope creep). Port only SequenceMatcher, get_close_matches, unified_diff, ndiff.
- **Minor runtime behavior**: autojunk heuristic uses sequence length thresholds; needs careful tuning to match CPython's behavior exactly. The thresholds are hardcoded (lengths > 200 or ratios < 1/125), easily ported but requires exact empirical verification.
- **Charjunk inference in ndiff**: The '?' lines mark character-level diffs; requires a second-pass difflib.SequenceMatcher call on individual lines. Correct but slightly more complex control flow than the easy parts.

**Deferral suggestion:** Start with SequenceMatcher (core; enables get_close_matches) → unified_diff → ndiff (simplest line diff) → get_opcodes/get_grouped_opcodes (enable more complex applications). Skip diff_bytes, HtmlDiff, Differ initially. Autojunk tuning can be empirical (probe CPython for threshold behavior).
