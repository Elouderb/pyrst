# pyrst Agent Roles and Guidelines

This document defines specialized agent roles for pyrst development and guidelines for effective, token-efficient collaboration.

## Agent Roles

The project uses a small, disciplined set of agent roles rather than a free swarm. Each role has specific responsibilities, required context, and tool access.

### 1. Spec and Acceptance Agent

**Responsibility:** Maintain and evolve the language specification. Ensure future work aligns with spec. Verify that completed work meets acceptance criteria.

**Context:**
- `LANGUAGE_SPEC.md`, `GRAMMAR.md`, `TYPE_SYSTEM.md`, `IR_INVARIANTS.md`, `RUNTIME_ABI.md`
- Current test suite and failing tests
- Recent commits and PRs

**When to invoke:**
- Before implementing a feature, validate the spec (is it explicit enough?)
- After completing a feature, verify against spec and acceptance tests
- When a design decision conflicts with the spec, document the conflict and propose a spec update

**Outputs:**
- Spec document edits (if clarification or change needed)
- Acceptance test cases (if missing)
- Sign-off on implementation (if correct)
- Diagnostic (if implementation doesn't match spec)

### 2. Frontend Agent

**Responsibility:** Lexer, parser, and CST construction. Syntax validation and error recovery.

**Context:**
- `GRAMMAR.md` (source of truth for syntax rules)
- `src/lexer.rs`, `src/parser.rs`, `src/ast.rs`
- Parser test suite (examples of valid/invalid syntax)
- Indentation handling and trivia preservation rules

**When to invoke:**
- Add new grammar rules (e.g., f-strings, match/case)
- Fix parser bugs
- Improve error messages
- Optimize lexer performance

**Constraints:**
- Do not change AST structure without consulting spec agent
- Preserve token spans and trivia for formatter and IDE support
- Keep parser deterministic (no backtracking unless explicitly noted)

**Outputs:**
- Parser changes
- New parser test cases
- Updated grammar documentation if needed

### 3. Semantic and Type Agent

**Responsibility:** Name resolution, type checking, type inference, and HIR/MIR construction.

**Context:**
- `TYPE_SYSTEM.md` (typing rules)
- `IR_INVARIANTS.md` (HIR and MIR invariants)
- `src/typeck.rs`, `src/ast.rs` (or HIR module if created)
- Type checker test suite (positive and negative cases)
- Typing diagnostics examples

**When to invoke:**
- Implement type inference for a feature
- Add type checking rules
- Construct HIR for a language construct
- Build the import/name resolution graph
- Fix type errors in generated code

**Constraints:**
- Type checking must be exact (no silent fallback to `Any` or `Dynamic` in v0)
- Every binding must have a type by HIR stage
- HIR must preserve all semantic information from the AST
- MIR must be fully typed before lowering to Rust

**Outputs:**
- Type checking changes
- New typing test cases (positive and negative)
- Diagnostic text for typing errors
- HIR/MIR node definitions if needed

### 4. Backend and Runtime Agent

**Responsibility:** MIR to Rust codegen, runtime ABI, standard library shims, and object model lowering.

**Context:**
- `IR_INVARIANTS.md` (MIR semantics and lowering rules)
- `RUNTIME_ABI.md` (object model, calling conventions)
- `src/codegen.rs`
- Backend test suite (input MIR → expected Rust output)
- Runtime helper signatures and behaviors

**When to invoke:**
- Lower new MIR nodes to Rust
- Implement dunder method lowering (e.g., `__add__` → `impl Add`)
- Add runtime helpers (e.g., for exception handling)
- Implement standard library shims
- Fix ownership inference or memory management bugs

**Constraints:**
- Generated Rust code must compile without warnings
- Preserve source spans for debugging
- Do not introduce new runtime dependencies without approval
- All lowering must match the MIR invariants

**Outputs:**
- Codegen changes
- New codegen test cases (MIR → Rust)
- Runtime helper definitions (in Rust)
- Standard library shim implementations

### 5. Tooling and Ecosystem Agent

**Responsibility:** LSP, formatter, package manager, build integration, and CLI.

**Context:**
- `src/main.rs`, `src/driver.rs` (CLI entry points)
- Test cases for tooling behavior
- Example pyrst projects (for testing end-to-end workflows)
- LSP specification (as reference)
- Formatter design goals

**When to invoke:**
- Add new CLI commands
- Implement LSP server
- Build package/build tool integration
- Improve formatter or diagnostics
- Add CI/CD configuration

**Constraints:**
- All tools must provide clear diagnostics
- Tooling must not require manual steps that can be automated
- LSP must follow DAP/LSP protocols for debugger/editor integration

**Outputs:**
- CLI changes and tests
- LSP server implementation
- Formatter implementation
- Build/package tool integration
- CI configuration

### 6. Verification and Compatibility Agent

**Responsibility:** Test suite, compatibility checks, fuzzing, performance regression detection, and release validation.

**Context:**
- Supported language profile (from `LANGUAGE_SPEC.md`)
- Existing compatibility tests and golden outputs
- Benchmark suite and performance baselines
- Example programs (hello.py, point.py, fib.py, etc.)
- CI matrix (platforms, Rust versions)

**When to invoke:**
- Write compatibility test cases for a feature area
- Run test suite and report failures
- Fuzz the parser or type checker
- Measure performance regressions
- Validate a release candidate

**Outputs:**
- Test cases and test infrastructure
- Compatibility reports
- Fuzz-found input cases
- Performance regression analysis
- Release sign-off

## Communication Protocol

### Task Handoff Format

When creating a task for an agent, include:

1. **Source of truth documents:** Exact sections from `LANGUAGE_SPEC.md`, `GRAMMAR.md`, etc. (paste, don't paraphrase).
2. **Failing test or requirement:** The exact failing test case or behavior specification.
3. **Constraints:** What must be preserved (AST stability, MIR invariants, error message wording, etc.).
4. **Acceptance criteria:** How the agent should verify the work.
5. **Implementation scope:** Minimal diff list; what files to edit.

### Example Task Template

```
Task: Add `int` literal parsing to the lexer

Source of truth:
- GRAMMAR.md, "Lexical Elements" → "Literals" section
- LANGUAGE_SPEC.md, "Scalar Types" → `int` description
- RUNTIME_ABI.md, "Integer Size" (note: size TBD)

Failing test:
- Input: "42"
- Expected: Token(INT, "42", span...)
- Current: ParsingError("unknown token")

Constraints:
- Do not change token types for other literals (STRING, FLOAT, etc.)
- Preserve token spans exactly (column start and length)
- Support decimal, hex (0x...), octal (0o...), binary (0b...) formats

Acceptance:
- All tests in test/lexer/int_literals.py pass
- No regression in existing lexer tests
- Token spans are correct for error reporting

Files to edit:
- src/lexer.rs (token type definition, lexing logic)
- tests/lexer.rs (test cases)
```

### Retrieval Pack (for Agent Context)

Create a `.claude/retrieval_pack.txt` file containing:

```
# pyrst Retrieval Pack for Agents

## Grammar Rules (excerpt)
[paste from GRAMMAR.md: rules for current target]

## Type System Rules (excerpt)
[paste from TYPE_SYSTEM.md: rules for current target]

## MIR Invariants (excerpt)
[paste from IR_INVARIANTS.md: relevant lowering rules]

## Runtime ABI (excerpt)
[paste from RUNTIME_ABI.md: calling conventions, object model]

## Standard Diagnostics

Error categories and examples:
[list common error messages and their format]

Diagnostic spans should include:
- File path, line, column
- Code snippet with error highlighted
- Explanation and suggestion

Example:
  error: type mismatch
    expected: int
    found: str
    at src/test.py:5:10
      5 | x: int = "hello"
          |         ^^^^^^^
```

This retrieval pack is cached and reused across agent tasks.

## Efficient Agent Collaboration

### Token Optimization Strategies

1. **Small diffs only:** Each task should modify 1-3 files, change <500 lines total. Do not ask an agent to rewrite an entire subsystem in one step.

2. **Exact specs:** Paste exact test cases and spec excerpts instead of summarizing. The agent needs the precise requirements.

3. **Machine-checkable output:** Require agents to:
   - List files changed
   - Run tests and report results
   - Confirm invariants are preserved
   - Return a summary suitable for a commit message

4. **Separate implementation and review:** Do not ask one agent to generate and validate its own patch. Use the implementation agent for code, then the spec agent to verify.

5. **Batch flaky tests:** If multiple tests fail for related reasons, ask the agent to triage them together, not one per task.

6. **Prefer local tools first:** Before asking an agent to debug, run the compiler locally:
   - Compile and see the error
   - Extract the minimal failing case
   - Ask the agent only about the remaining defect

### Example Workflow

1. **Spec Agent:** Define what a feature should do (update LANGUAGE_SPEC.md if needed).
2. **Frontend Agent:** Parse the syntax (update parser, add tests).
3. **Semantic Agent:** Type check it (update type checker, add typing tests).
4. **Backend Agent:** Generate Rust (update codegen, add codegen tests).
5. **Verification Agent:** Test end-to-end (run full test suite, check compatibility, report).
6. **Spec Agent:** Verify all outputs match spec.

### Token Budget Allocation (Rough)

For a typical feature (e.g., adding support for a new operator):

- **Spec:** 1-2M tokens (define the feature)
- **Frontend:** 2-4M tokens (parsing)
- **Semantic:** 3-6M tokens (typing and inference)
- **Backend:** 2-4M tokens (codegen)
- **Verification:** 1-2M tokens (testing and validation)
- **Total:** ~10-20M tokens for a complete feature

Keeping tasks small and focused usually saves tokens compared to one large task.

## Required Pre-Task Checks

Before assigning a task to any agent, ensure:

1. ✅ The spec is explicit (not vague or contradictory).
2. ✅ Test cases exist (golden inputs and expected outputs).
3. ✅ No conflicting tasks are in flight (agents should not race to modify the same file).
4. ✅ The implementation scope is bounded (not open-ended).
5. ✅ Required dependencies are already merged (don't ask a backend agent to codegen MIR nodes that haven't been defined yet).

## Code Review Checkpoints

All agent-generated code should pass through a review gate:

- **Syntax correctness:** Does it parse and compile without warnings?
- **Spec compliance:** Does it match the specification?
- **Test coverage:** Are there test cases covering the happy path and edge cases?
- **Diagnostic quality:** Are error messages clear and actionable?
- **No regressions:** Do existing tests still pass?
- **Commit message:** Is the message clear and does it mention what was done and why?

If review finds issues, create a new task for the relevant agent with the diagnostic details, rather than asking them to fix it in-place.

## Roadmap Coordination

The `PLAN.md` document defines phases:

1. **Charter and executable spec** (8 weeks, 4M–10M tokens)
2. **Frontend and surface syntax** (12 weeks, 8M–20M tokens)
3. **Type system and semantic core** (20 weeks, 18M–45M tokens)
4. **Backend and runtime** (18 weeks, 15M–40M tokens)
5. **Tooling and ecosystem alpha** (16 weeks, 12M–35M tokens)
6. **Compatibility and hardening** (24 weeks, 20M–60M tokens)

Agent assignments should follow this roadmap. For example:

- **Phase 1:** Spec Agent finalizes all spec documents. No other agents active.
- **Phase 2:** Frontend Agent works on lexer and parser. Spec Agent reviews in parallel.
- **Phase 3:** Semantic Agent works on type checking. Frontend Agent refines parser as needed.
- **Phase 4:** Backend Agent works on codegen. Semantic Agent refines type checking. Frontend and Spec Agents in support role.
- And so on...

## Success Metrics

A successful agent task:

- ✅ All requested changes are complete
- ✅ All tests pass (new and existing)
- ✅ Code compiles without warnings
- ✅ Spec is satisfied
- ✅ Commit message is clear
- ✅ No regressions in other parts of the system

## Escalation

If an agent encounters:

- **Ambiguous spec:** Escalate to Spec Agent for clarification before proceeding.
- **Conflicting requirements:** Escalate to user or Spec Agent for decision.
- **Blocked on another phase:** Escalate; do not work around the blocker.
- **Internal compiler error:** Escalate to user with minimal reproducer.

## Future Enhancements (v1.0+)

This agent structure is designed to scale as the project grows:

- Add a **Mojo/Codon compatibility agent** once the compiler is mature.
- Add a **Performance optimization agent** once profiling infrastructure is in place.
- Add a **Python interop agent** once C ABI is stable.

For now, these roles are consolidated into the Backend Agent.
