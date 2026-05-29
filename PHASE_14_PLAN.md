# Phase 14: Tooling & IDE Integration — Implementation Plan

**Date:** May 28, 2026  
**Goal:** Deliver professional developer tools  
**Timeline:** 3-4 weeks (1 intensive session)

---

## Overview

Phase 14 adds professional tooling to pyrst, transforming it from a working compiler into a developer-friendly tool. Focus on tools that enable real-world adoption and education.

## Phase 14.1: Code Formatter (Priority 1)

### Goal
Create `pyrst fmt` command that auto-formats pyrst source code consistently.

### Scope
- Normalize indentation (4 spaces)
- Line length management (80-100 chars default, configurable)
- Space around operators
- Consistent imports formatting
- Blank line normalization

### Implementation Strategy
- **Approach:** AST-based formatter (parse → reformat → emit)
- **Complexity:** Medium (need to preserve comments, handle edge cases)
- **Files:** src/formatter.rs (new)

### Commands
```bash
pyrst fmt <file>              # Format single file
pyrst fmt <dir>               # Format directory
pyrst fmt --check <file>      # Check formatting without changing
pyrst fmt --line-length 100   # Custom line length
```

### Success Criteria
- ✅ Formats all 33 examples consistently
- ✅ Preserves code semantics
- ✅ Handles edge cases (long strings, complex expressions)
- ✅ Fast execution (<100ms for typical file)

---

## Phase 14.2: Linter (Priority 2)

### Goal
Create `pyrst lint` command for style and error checking.

### Scope
- **Style Checks:**
  - Unused imports/variables
  - Naming conventions (snake_case for functions, CamelCase for classes)
  - TODO/FIXME comments
  - Dead code warnings

- **Error Checks:**
  - Undefined names
  - Type mismatches
  - Unreachable code

- **Best Practices:**
  - Function length warnings (>50 lines)
  - Parameter count warnings (>5 params)
  - Cyclomatic complexity

### Implementation Strategy
- **Approach:** Lint rules engine (rules check AST, emit findings)
- **Complexity:** Medium (need to track context)
- **Files:** src/linter.rs (new)

### Commands
```bash
pyrst lint <file>             # Lint single file
pyrst lint --verbose          # Detailed output
pyrst lint --strict           # Strict mode (more warnings)
pyrst lint --ignore W001      # Ignore specific rule
```

### Success Criteria
- ✅ Detects common style issues
- ✅ Zero false positives on clean code
- ✅ Helpful error messages
- ✅ Configurable severity levels

---

## Phase 14.3: REPL (Priority 3)

### Goal
Create interactive `pyrst repl` for exploration and learning.

### Scope
- Read-Eval-Print Loop
- Expression evaluation
- Variable binding and recall
- Multi-line statement support
- History (up/down arrows)
- Exit with `exit()` or Ctrl+D

### Implementation Strategy
- **Approach:** Parse each line, execute, print result
- **Complexity:** Low-Medium (simplified codegen for REPL)
- **Files:** src/repl.rs (new)

### Commands
```bash
pyrst repl                    # Start interactive mode
pyrst repl --history file     # Load history from file
```

### Example Session
```python
>>> x = 5
>>> y = 10
>>> x + y
15
>>> def factorial(n):
...     if n <= 1:
...         return 1
...     return n * factorial(n - 1)
...
>>> factorial(5)
120
>>> exit()
```

### Success Criteria
- ✅ Can execute expressions
- ✅ Can define functions and classes
- ✅ History works (up/down arrows)
- ✅ Clean user experience

---

## Phase 14.4: Language Server (Priority 4)

### Goal
Create LSP implementation for IDE integration (VS Code, JetBrains).

### Scope
- **Core LSP Features:**
  - Document open/close/change
  - Diagnostics (errors/warnings)
  - Hover information
  - Code completion (basic)
  - Go to definition
  - Symbol information

- **Minimal Viable:**
  - Syntax/type error reporting
  - Hover shows type info
  - Go to definition works
  - Basic completion

### Implementation Strategy
- **Approach:** Implement LSP protocol (stdio-based for simplicity)
- **Complexity:** High (protocol implementation)
- **Files:** src/lsp.rs (new)

### Commands
```bash
pyrst lsp                     # Start LSP server (stdio)
```

### IDE Integration
1. **VS Code:** `pyrst.vscode` extension
2. **JetBrains:** Community IDE support

### Success Criteria
- ✅ Errors show in IDE
- ✅ Hover reveals types
- ✅ Go to definition works
- ✅ Completion provides suggestions
- ✅ VS Code extension works

---

## Phase 14.5: Package Manager Foundation (Priority 5)

### Goal
Lay foundation for pyrst package management.

### Scope (Foundation Only)
- **Structure:**
  - Define pyrst.toml format (metadata, dependencies)
  - Package registry concept
  - Version specification format

- **CLI:**
  ```bash
  pyrst new <name>            # Create new project
  pyrst add <package>         # Add dependency
  pyrst build                 # Build with dependencies
  pyrst publish               # Publish to registry (stub)
  ```

- **Metadata (pyrst.toml):**
  ```toml
  [package]
  name = "myapp"
  version = "0.1.0"
  authors = ["Jane Doe"]
  
  [dependencies]
  requests = "1.0"
  ```

### Implementation Strategy
- **Approach:** Metadata parsing, basic project structure
- **Complexity:** Low-Medium (mostly file I/O and TOML parsing)
- **Files:** src/package.rs (new)

### Success Criteria
- ✅ `pyrst new` creates project structure
- ✅ pyrst.toml parsed correctly
- ✅ Can specify and parse dependencies
- ✅ Foundation for future dependency resolution

---

## Implementation Order

### Week 1-2: Core Tools
1. **Code Formatter** (src/formatter.rs)
   - AST-based formatting
   - Handle all example files
   - Test on 33 examples

2. **Linter** (src/linter.rs)
   - Implement rule engine
   - Add 5-10 basic rules
   - Test on examples

### Week 3: Experience Tools
3. **REPL** (src/repl.rs)
   - Interactive evaluation
   - History support
   - Test with various inputs

### Week 4: IDE & Package Foundation
4. **LSP** (src/lsp.rs)
   - LSP protocol implementation
   - VS Code extension stub
   - Test with real IDE

5. **Package Manager** (src/package.rs)
   - Project structure
   - pyrst.toml format
   - Basic CLI commands

---

## CLI Command Structure

### Main Commands
```bash
pyrst <command> [options]

Commands:
  build <file>        # Compile to Rust and binary
  emit <file>         # Generate Rust code
  fmt [file|dir]      # Format code
  lint [file|dir]     # Check code
  repl                # Interactive shell
  lsp                 # Language server
  new <name>          # Create project
  add <package>       # Add dependency
  publish             # Publish package (stub)
```

### Global Options
```bash
--help              # Show help
--version           # Show version
--verbose           # Verbose output
--config <file>     # Config file path
```

---

## Testing Strategy

### Unit Tests
- Formatter: Test indentation, spacing, line wrapping
- Linter: Test each rule independently
- REPL: Test expression evaluation, variable binding
- LSP: Test protocol messages
- Package: Test pyrst.toml parsing

### Integration Tests
- Format all 33 examples, compile result
- Lint all 33 examples, verify output
- REPL: Run multi-statement sessions
- LSP: Test with VS Code extension
- Package: Create and build project

### User Testing
- Have users try formatter on their code
- Run linter on real pyrst projects
- Interactive REPL session walkthrough
- IDE integration with VS Code

---

## Success Criteria (Phase 14 Overall)

### Phase 14.1 (Formatter)
- ✅ Formats all 33 examples successfully
- ✅ No semantic changes after formatting
- ✅ Idempotent (fmt → fmt = no change)
- ✅ Handles edge cases (long strings, comments)

### Phase 14.2 (Linter)
- ✅ Detects unused variables
- ✅ Detects unused functions
- ✅ Checks naming conventions
- ✅ Zero false positives on clean code

### Phase 14.3 (REPL)
- ✅ Can evaluate expressions
- ✅ Can define functions
- ✅ History works (↑/↓)
- ✅ Clean exit (Ctrl+D or exit())

### Phase 14.4 (LSP)
- ✅ VS Code shows errors/warnings
- ✅ Hover shows type information
- ✅ Go to definition works
- ✅ Completion offers suggestions

### Phase 14.5 (Package Manager)
- ✅ `pyrst new` creates project
- ✅ pyrst.toml can be parsed
- ✅ Dependencies can be listed
- ✅ CLI commands respond correctly

---

## Architecture Notes

### Formatter Architecture
```
AST
  ↓
Format visitor (walks AST)
  ├─ Track indent level
  ├─ Track line length
  └─ Emit formatted code
  ↓
Formatted source
```

### Linter Architecture
```
AST
  ↓
Lint rules (multiple passes)
  ├─ Rule 1: Check X
  ├─ Rule 2: Check Y
  └─ Rule N: Check Z
  ↓
List of findings (error/warning)
```

### REPL Architecture
```
Input line
  ↓
Lex/Parse
  ↓
Type check
  ↓
Code generate (to Rust)
  ↓
Compile (rustc)
  ↓
Execute
  ↓
Print result
```

### LSP Architecture
```
IDE client
  ↓ (JSON-RPC over stdio)
LSP Server
  ├─ Parse incoming messages
  ├─ Call handler (diagnosis, hover, etc.)
  └─ Send response
  ↓ (JSON-RPC)
IDE (display results)
```

---

## Configuration File (pyrst.toml)

### Format
```toml
[package]
name = "myapp"
version = "0.1.0"
authors = ["Jane Doe"]
description = "A pyrst application"
edition = "2024"

[tool.pyrst]
line_length = 100
formatting_style = "black"  # or "rustfmt"

[tool.linter]
strict = false
ignore_rules = ["W001", "W002"]

[dependencies]
# Future: package dependencies

[dev-dependencies]
# Future: dev-only dependencies
```

---

## Timeline Estimate

| Component | Estimate | Effort | Effort |
|-----------|----------|--------|--------|
| Formatter | 1 week | Medium | 100-150 lines |
| Linter | 1 week | Medium | 80-120 lines |
| REPL | 3-4 days | Low-Medium | 80-100 lines |
| LSP | 1 week | High | 200-300 lines |
| Package | 2-3 days | Low | 60-80 lines |

**Total Estimated:** 3-4 weeks, ~600-750 lines of new code

---

## References

- LSP Spec: https://microsoft.github.io/language-server-protocol/
- Python Black formatter (inspiration for formatting style)
- Rust rustfmt (formatting approach)

---

*Phase 14 Plan: May 28, 2026*
