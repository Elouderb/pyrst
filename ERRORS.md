# pyrst Error Messages and Diagnostics Philosophy

This document defines pyrst's approach to compiler diagnostics, error messages, and recovery strategies.

---

## Core Principles

### 1. **Clarity Over Brevity**

Error messages should be **clear and helpful**, not terse. A 3-line message that teaches the user something is better than a 1-line message that leaves them confused.

### 2. **Show Context**

Every error should include:
- **Location**: File, line, and column of the problem
- **Source snippet**: The actual code that caused the error
- **Visual indicator**: A caret (^) or underline showing exactly where the problem is
- **Explanation**: What went wrong and why
- **Suggestion**: How to fix it (when possible)

### 3. **Errors Before Warnings**

pyrst uses a strict model:
- **Errors** — Compilation stops. Fix required.
- **Warnings** — Compilation proceeds. Advisory.

No warnings for now; only errors. This keeps the signal-to-noise ratio high.

### 4. **Actionable Suggestions**

Every error message ends with "did you mean..." or "try..." suggestions when applicable.

---

## Error Message Format

```
error[CODE]: brief description
  ┌─ file.py:LINE:COL
  │
  LINENUMBER │ source code here
             │ ^^^^^^ indicator
  │
  = note: detailed explanation
  = hint: suggestion for fixing
```

### Example Current Format (Phase 6)

```
Error: Unknown identifier 'items' at file.py:5:10
```

### Target Format (Phase 7)

```
error[E0425]: unknown identifier
  ┌─ examples/demo.py:5:10
  │
  5 │     for item in items:
  │                    ^^^^^ not found in this scope
  │
  = note: 'items' was not declared. Did you mean to declare it first?
  = hint: try: items: list[int] = [...]
```

---

## Error Categories and Codes

### Lexer Errors (L000-L099)

| Code | Message | Fix |
|------|---------|-----|
| L001 | Invalid token | Check syntax; maybe typo in operator |
| L002 | Unterminated string | Check for missing closing quote |
| L003 | Invalid number literal | Check numeric format (e.g., hex literals not supported) |
| L004 | Invalid escape sequence | Use valid escape: \n, \t, \", \\ |

### Parser Errors (P000-P099)

| Code | Message | Fix |
|------|---------|-----|
| P001 | Expected X but found Y | Check grammar; maybe missing colon or bracket |
| P002 | Unexpected indentation | Check indentation consistency (spaces vs tabs) |
| P003 | Unexpected token | Syntax error; check operator or keyword usage |
| P004 | Unmatched bracket | Check for missing closing `]`, `)`, `}` |
| P005 | Invalid assignment target | Left-hand side of `=` must be variable, field, or tuple |

### Type Errors (E000-E099)

| Code | Message | Fix |
|------|---------|-----|
| E001 | Type mismatch | Ensure both sides of operation have compatible types |
| E002 | Unknown identifier | Declare variable before use |
| E003 | Unknown function | Function not defined or typo in name |
| E004 | Unknown type | Type annotation references undefined type |
| E005 | Invalid function call | Wrong number of arguments or incompatible types |
| E006 | Invalid method call | Method doesn't exist on this type |
| E007 | Invalid field access | Field doesn't exist on this type |
| E008 | Cannot index type | Type does not support indexing (e.g., `int[0]`) |
| E009 | Unsupported operator | Operator not defined for this type pair |
| E010 | Unsupported language feature | Feature not implemented yet (e.g., try/except) |

### Semantic Errors (S000-S099)

| Code | Message | Fix |
|------|---------|-----|
| S001 | Duplicate name | Function/class/variable already defined |
| S002 | Invalid return type | Return statement type doesn't match function signature |
| S003 | Unreachable code | Code after `return`, `break`, `continue` is unreachable |
| S004 | Missing return | Function requires return but none provided |

---

## Current Error Output Examples

### Present (Phase 6)

```
Error: Type mismatch: expected int, got str
```

### Future Target (Post-Phase 7)

```
error[E001]: type mismatch
  ┌─ examples/demo.py:10:8
  │
  10 │     x: int = "hello"
  │               ^^^^^^^ expected int, found str
  │
  = note: string literals have type `str`
  = hint: remove quotes if you meant the integer 0, or change the annotation to `str`
```

---

## Diagnostic Improvements (Phase 7 Tasks)

### Task 1: Source Code Snippet Display

**Current state:** Errors report line numbers only.

**Goal:** Include 1-3 lines of source code with visual indicator.

**Implementation:**
- Store source text in compiler
- On error, retrieve lines around error location
- Print with line numbers and caret indicator
- Handle edge cases: multi-line expressions, very long lines (wrap or truncate)

### Task 2: Structured Error Codes

**Current state:** Free-form error strings.

**Goal:** All errors have unique codes (L/P/E/S + number).

**Implementation:**
- Define error enum with variants for each unique error type
- Each variant maps to a code + message template
- Pass context (location, type info) to error formatter
- Ensure codes are searchable in documentation

### Task 3: Actionable Suggestions

**Current state:** Some errors include "did you mean" for simple cases.

**Goal:** All common errors suggest a fix.

**Implementation:**
- For each error code, define 1-2 fix suggestions
- Include example corrected code in hint section
- For identifier errors, check similar names (typos)
- For operator errors, explain valid types

### Task 4: Error Recovery

**Current state:** Most errors stop compilation immediately.

**Goal:** Collect multiple errors before stopping.

**Implementation:**
- Instead of early return on error, store error in vec and continue
- Report all errors at end of each phase (lex, parse, type check)
- Use conservative recovery: assume unknown type is `Unknown`, skip problematic statements
- Cap total errors reported (e.g., max 10 errors to avoid spam)

---

## Error Message Guidelines

### DO:

1. ✅ Say what the problem **is** (not just where)
2. ✅ Say **why** it's a problem
3. ✅ Say **how** to fix it
4. ✅ Include the offending code
5. ✅ Use positive language ("try this" not "don't do that")
6. ✅ Be specific (not "invalid type", but "expected int, got str")

### DON'T:

1. ❌ Assume the user knows compiler internals
2. ❌ Say "syntax error" without explaining what's wrong
3. ❌ Blame the user ("you forgot" → "missing")
4. ❌ Make jokes or be sarcastic
5. ❌ Dump stack traces or internal compiler state
6. ❌ Report the same error multiple times (deduplicate)

---

## Common Errors and Improved Messages

### 1. Undefined Variable

**Before:**
```
Error: Unknown identifier 'items'
```

**After:**
```
error[E0002]: unknown identifier
  ┌─ examples/loop.py:3:10
  │
  3 │     for x in items:
  │              ^^^^^ not found in scope
  │
  = note: 'items' must be declared before use
  = hint: try: items: list[int] = [1, 2, 3]
  = help: did you mean one of: item, items_list
```

### 2. Type Mismatch

**Before:**
```
Error: Type mismatch: expected int, got str
```

**After:**
```
error[E0001]: type mismatch
  ┌─ examples/demo.py:5:8
  │
  5 │     x: int = "hello"
  │             ^^^^^^^^^ expected int, found str
  │
  = note: cannot assign string to integer variable
  = hint: remove quotes to convert to symbol, or change annotation to: x: str = "hello"
```

### 3. Invalid Function Call

**Before:**
```
Error: Wrong number of arguments to 'add'
```

**After:**
```
error[E0005]: invalid function call
  ┌─ examples/demo.py:8:5
  │
  8 │     result = add(1, 2, 3)
  │                  ^^^^^^^^^ too many arguments
  │
  = note: function 'add' expects 2 arguments but 3 were given
  = help: signature: def add(a: int, b: int) -> int
  = hint: try: result = add(1, 2)
```

### 4. Unknown Method

**Before:**
```
Error: Unknown method 'uppper' on str
```

**After:**
```
error[E0006]: unknown method
  ┌─ examples/demo.py:6:8
  │
  6 │     s: str = "hello"
  7 │     x: str = s.uppper()
  │                  ^^^^^^ method not found
  │
  = note: type 'str' has no method 'uppper'
  = help: did you mean: upper() ?
  = hint: try: x = s.upper()
```

### 5. Unsupported Feature

**Before:**
```
Error: try statements not supported
```

**After:**
```
error[E0010]: unsupported language feature
  ┌─ examples/demo.py:5:1
  │
  5 │     try:
  │     ^^^ try/except not yet implemented
  │
  = note: exception handling is planned for Phase 11
  = help: as a workaround, use assert to check preconditions
  = help: see ROADMAP.md for feature timeline
```

---

## Notes for Implementation

### Error Struct

```rust
#[derive(Debug)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    pub location: Span,
    pub hints: Vec<String>,
    pub note: Option<String>,
}

pub enum ErrorCode {
    // Lexer
    InvalidToken,
    UnterminatedString,
    InvalidNumber,
    
    // Parser
    UnexpectedToken,
    UnmatchedBracket,
    
    // Type checker
    TypeMismatch(String, String), // expected, found
    UnknownIdentifier(String),
    UnknownFunction(String),
    InvalidFunctionCall,
    
    // Semantic
    DuplicateName(String),
    Unsupported(String),
}
```

### Error Formatter

```rust
fn format_error(error: &Error, source: &str) -> String {
    let span = &error.location;
    let lines: Vec<&str> = source.lines().collect();
    let line_no = span.line;
    let col_no = span.col;
    
    let mut output = format!("error[{}]: {}\n", error.code_str(), error.message);
    output.push_str(&format!("  ┌─ {}:{}:{}\n", "file.py", line_no, col_no));
    
    if line_no <= lines.len() {
        let line = lines[line_no - 1];
        output.push_str(&format!("  │\n"));
        output.push_str(&format!("{:>3} │ {}\n", line_no, line));
        output.push_str(&format!("      │ {}^\n", " ".repeat(col_no - 1)));
    }
    
    if let Some(note) = &error.note {
        output.push_str(&format!("  │\n"));
        output.push_str(&format!("  = note: {}\n", note));
    }
    
    for hint in &error.hints {
        output.push_str(&format!("  = hint: {}\n", hint));
    }
    
    output
}
```

---

## Multi-Error Reporting

For now (Phase 6-7), report first error and stop. Future improvements:

- **Phase 8**: Collect all parse errors, report up to 5, continue to type check
- **Phase 9**: Collect all type errors, report up to 10, print summary
- **Phase 10**: Smart error recovery (assume types, skip bad definitions)

---

## User Experience Goals

**After Phase 7:**
- New users can understand what went wrong from error messages alone
- Error messages are 3-5 lines, not 20-line stack traces
- Every error suggests a fix
- No "compiler internal error" or panic messages in normal error paths

---

## Related Documentation

- **DESIGN_DECISIONS.md §14** — Readable generated code principle applies to error messages
- **SPEC.md §15** — Lists unsupported features; errors should reference this
- **ROADMAP.md §Phase 7** — Specification and diagnostics phase

---

*Document created: May 28, 2026*  
*Status: Error philosophy and format defined for Phase 7*  
*Next: Implement structured error codes and source snippet display*
