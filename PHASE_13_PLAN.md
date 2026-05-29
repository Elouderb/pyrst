# Phase 13: Optimization Passes — Implementation Plan

**Date:** May 28, 2026  
**Status:** Planning Phase

---

## Strategic Goals

1. **Performance:** Compiled pyrst code matches hand-written Rust performance
2. **Code Quality:** Remove unnecessary allocations and dead code
3. **Binary Size:** Reduce compiled executable size
4. **Compile Time:** Minimize pyrst-to-Rust codegen time

## Implementation Priorities

### Priority 1: High-Impact, Low-Effort (Phase 13.1)

#### 1.1 Dead Code Elimination
**Goal:** Remove unused variables, functions, and classes

**Scope:**
- Warn about unused local variables
- Remove unused function definitions
- Remove unused class definitions
- Track variable usage through control flow

**Implementation:**
- AST walk in type checker to identify definitions
- Track usage in all statements/expressions
- Warning phase before code generation
- Skip emitting unused functions/classes in codegen

**Effort:** Medium (20-30 lines of analyzer code)

#### 1.2 Constant Folding
**Goal:** Evaluate constant expressions at compile time

**Scope:**
- Arithmetic: `1 + 2` → `3`
- Boolean: `true && false` → `false`
- String concatenation: `"a" + "b"` → `"ab"`

**Implementation:**
- Add constant evaluation pass on Expr nodes
- Replace constant exprs with their values
- Happens during type checking or codegen

**Effort:** Medium (40-60 lines of evaluator code)

#### 1.3 Unused Import Warnings
**Goal:** Warn when imports are not used

**Scope:**
- Track which imported symbols are actually used
- Warn at import sites
- Suggest removal

**Effort:** Low (15-20 lines)

### Priority 2: Medium-Impact, Medium-Effort (Phase 13.2)

#### 2.1 String Literal Deduplication
**Goal:** Emit string literals only once

**Implementation:**
- Track all string literals in a map
- Assign IDs to unique strings
- Emit string constants at module level
- Reference by ID in code

**Effort:** Medium-High (50-80 lines)

#### 2.2 Loop Strength Reduction
**Goal:** Optimize common loop patterns

**Examples:**
- `for i in range(0, n):` where n is constant → Optimize bounds
- `for x in list:` where list is constant → Inline iteration
- Loop-invariant code motion → Move invariant expressions outside loop

**Effort:** High (80-120 lines)

### Priority 3: Advanced Optimizations (Phase 13.3+)

#### 3.1 Type-Directed Optimization
- Use type information to eliminate unnecessary clones
- Example: `for x in [1,2,3]:` → Use `.copied()` not `.cloned()`
- Already partially done in Phase 10

#### 3.2 Inlining Strategy
- Mark small functions for inlining
- Generate inline hints to rustc

#### 3.3 SIMD Detection
- Identify vectorizable loops
- Suggest SIMD opportunities

---

## Implementation Order

### Phase 13.1: Quick Wins
1. Implement constant folding in codegen
2. Add dead variable detection (warnings only, no removal yet)
3. Create performance benchmarking suite

### Phase 13.2: Code Quality
1. String literal deduplication
2. Unused import warnings
3. Basic loop strength reduction

### Phase 13.3: Advanced
1. Full dead code elimination
2. Advanced loop optimizations
3. Profiling integration

---

## Benchmarking Infrastructure

### New Benchmarks to Create

1. **benchmark_fib.py** — Recursive Fibonacci (20-30 iterations)
2. **benchmark_matrix.py** — 2D matrix operations
3. **benchmark_string.py** — String manipulation heavy
4. **benchmark_sort.py** — List sorting performance

### Metrics to Track

- Compilation time (pyrst + rustc)
- Binary size (release build)
- Runtime performance (wall clock)
- Memory usage (for large data structures)

### Benchmark Harness

Create a script that:
1. Compiles each benchmark
2. Runs 10 iterations
3. Reports min/max/avg times
4. Compares before/after optimization

---

## Success Criteria for Phase 13

- ✅ Constant folding working for arithmetic expressions
- ✅ Unused variable warnings implemented
- ✅ Dead function removal working
- ✅ Benchmarking suite with 4+ benchmarks
- ✅ 10-20% performance improvement in at least one benchmark
- ✅ No regressions in prior examples

---

## Files to Modify

| File | Changes | Effort |
|------|---------|--------|
| src/codegen.rs | Add constant folding, dead code tracking | Medium |
| src/typeck.rs | Add usage tracking for dead code detection | Medium |
| src/ast.rs | No changes needed | - |
| examples/ | Add new benchmark files | Low |

---

## Timeline Estimate

- Phase 13.1: 1 week (constant folding + benchmarks)
- Phase 13.2: 1-2 weeks (string dedup + warnings)
- Phase 13.3+: 1+ weeks (advanced optimizations)

---

## References

- Phase 10 Benchmarks: PHASE_10_BENCHMARKS.md
- Current Performance: ~0.5s compilation, 1-2MB executable
- Target: <0.3s compilation, <1MB executable, 20%+ performance gain

---

*Phase 13 Plan created: May 28, 2026*
