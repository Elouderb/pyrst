# pyrst Development Plan & Roadmap

**Last Updated:** May 28, 2026  
**Status:** Phases 1-8 Complete, Phase 9+ Specified

---

## Strategic Vision

### Project Mission

pyrst is a **statically typed Python-like language that compiles to Rust**. It preserves Python's ergonomics and syntax while imposing static typing and leveraging Rust's performance and compile-time guarantees.

### Guiding Principles

1. **Syntax Fidelity:** Python surface syntax (indentation, control flow, expressions) where practical
2. **Bounded Semantics:** Intentionally reject highly dynamic Python patterns incompatible with static compilation
3. **Clear Specification:** Explicit documentation of what pyrst is and is not
4. **Compiler Maturity:** Focus on semantic clarity and architecture over raw feature count
5. **User Experience:** Clear error messages, IDE support, professional tooling

### Position in Language Ecosystem

pyrst occupies the space between:
- **Python** (ergonomics, syntax, semantics) and **Rust** (performance, type safety, compilation)
- **Codon** (compiled Python variant) and **Mojo** (Pythonic systems language)
- **TypeScript** (gradual typing) and **Nim** (compiled, multiple backends)

The sustainable win is a **coherent compiled language ecosystem**, not a forever chase after every dynamic edge of CPython.

---

## Strategic Design Decisions

### Typing Model

**Approach:** Bidirectional static typing with local inference
- **Default mode:** Statically typed and fully compiled
- **Inference:** Local and bidirectional, not "guess everything everywhere"
- **Generics:** Nominal and monomorphized
- **Dynamic escape:** An explicit `Any`-style box (deferred to Phase 15+)
- **Class model:** Fixed declared fields; no undeclared attribute injection by default

### Memory Model

**Approach:** Hybrid user-invisible memory model
- **Value types** for scalars, tuples, and certain immutable compiled aggregates
- **Managed reference types** for mutable containers, strings, user objects
- **No Rust-style ownership syntax in user model** (internal optimization only)

This preserves Python-like default aliasing and container semantics while allowing internal optimizations.

### Implementation Strategy

**Compiler Pipeline:**
```
Source → Lexer → Parser → AST → Type Checker → Code Generator → Rust → rustc → Binary
```

**AST-to-Rust Translation:**
- Direct codegen (no intermediate IR yet) suitable for v0
- All constructs map to Rust equivalents
- Aggressive cloning acceptable for now; optimization deferred to Phase 10+

### Backend Choice

**Recommendation:** Rust transpiler for MVP (current), Cranelift for Phase 10+

**Rationale:**
- Transpiler allows immediate access to `rustc`, Cargo, debuggers, profilers
- Fastest bootstrap path
- Clear path to Cranelift once semantics stabilize
- Preserves backend-neutral design for future flexibility

---

## Feedback Integration: Priority Shift

### Original Approach (Pre-Review)

Early phases focused on **feature velocity**:
- Phase 7: Exception handling
- Phase 8: Missing keywords (del, with)
- Phase 9: Standard library
- Success metric: How many Python features are implemented?

### Critical Inflection Point (May 2026)

At 65% feature completion, reviewer feedback identified a **strategic imperative:**

> "At this stage, semantic clarity matters more than feature count."

**Key insights:**
1. Full vertical slice exists (lexer → code generation)
2. Project is entering "design debt" risk zone
3. Features without explicit semantics create technical debt
4. Specification first, features second

### Revised Approach (Current)

Now focuses on **compiler maturity and semantic clarity**:
- Phase 7: ✅ Specification & Diagnostics
- Phase 8: ✅ Module System
- Phase 9: Semantic Cleanup (in progress)
- Phase 10: Runtime & Optimizations
- Phase 11: Exception Handling Model
- Phase 12: Class Semantics & Traits
- Success metric: How well do users understand pyrst's semantics and boundaries?

### Unresolved Questions → Explicit Roadmap

**Deferred to phases listed:**

| Question | Phase | Status |
|----------|-------|--------|
| Class reference semantics (value vs ref) | 9 | Design pending |
| Exception handling model (try/except strategy) | 11 | Design pending |
| Module visibility & namespacing | 10 | Phase 8 uses flat namespace |
| Type narrowing for optionals | 9 | Ready to implement |
| Mutability inference rules | 9 | Specified, ready |
| Borrowing rules (borrow vs clone vs move) | 9 | Specified, ready |
| `Any` type necessity | 15+ | Deferred to advanced phases |

---

## Development Roadmap

### Phase 9: Semantic Cleanup (Est. 3-4 weeks)

**Goal:** Formalize ownership, mutation, and type semantics.

**Key Tasks:**
- [ ] Resolve reference semantics for classes (value vs reference decision)
- [ ] Implement type narrowing for optionals
  ```python
  if x is not None:
      print(x + 1)  # x narrowed to int here
  ```
- [ ] Improve mutability inference (track actual mutations)
- [ ] Formalize argument passing rules

**Success Metric:** Users understand ownership rules without reading Rust codegen

**Deliverables:**
- Clear ownership semantics documented
- Type narrowing works for optionals
- Generated Rust code is leaner (fewer unnecessary `mut`)

---

### Phase 10: Runtime Prelude & Optimizations (Est. 2-3 weeks)

**Goal:** Create runtime support library and optimize codegen.

**Key Tasks:**
- [ ] Design `pyrst_runtime` crate
  - [ ] String helpers
  - [ ] List/dict wrappers (if needed)
  - [ ] Exception/error machinery
  - [ ] Iterator adapters
  - [ ] Display/printing behavior
- [ ] Reduce aggressive cloning
  - [ ] Copy elision for `Copy` types (`i64`, `f64`, `bool`)
  - [ ] Reference parameters for collections
- [ ] Optimize list/dict operations
- [ ] Add performance benchmarks

**Success Metric:** Compiled pyrst programs run with comparable performance to hand-written Rust

**Deliverables:**
- `pyrst_runtime` crate published
- Generated code has 30-50% fewer clones
- Performance benchmarks in repo

---

### Phase 11: Exception Handling Model (Est. 3-4 weeks)

**Goal:** Design and implement recoverable error handling.

**Key Design Questions:**
- Full exception semantics or `Result<T, E>` style?
- How does `try`/`except` map to Rust?
- Typed exceptions or runtime enums?

**Key Tasks:**
- [ ] Design exception/Result model
- [ ] Implement `try`/`except`/`finally`/`else`
- [ ] Implement exception type hierarchy
- [ ] Exception propagation semantics
- [ ] Integration with standard library functions

**Success Metric:** Can write idiomatic error handling that feels Pythonic

**Deliverables:**
- `try`/`except` statements work
- Exception propagation is clear
- Exception semantics documented

---

### Phase 12: Class Semantics & Traits/Protocols (Est. 4-6 weeks)

**Goal:** Advanced OOP features and polymorphism.

**Key Tasks:**
- [ ] Inheritance semantic review (single vs multiple)
- [ ] `super()` support
- [ ] Class methods and static methods
- [ ] Properties (`@property`)
- [ ] Operator overloading (`__add__`, `__str__`, etc.)
- [ ] Trait/protocol support for polymorphism
- [ ] Abstract base classes
- [ ] Mixins (if time permits)

**Success Metric:** Can write inheritance hierarchies with polymorphic methods

**Deliverables:**
- Full OOP feature set
- Polymorphism via traits/protocols
- Examples of class hierarchies

---

### Phase 13: Optimization Passes (Est. 4-6 weeks)

**Goal:** Advanced code optimizations.

**Key Tasks:**
- [ ] Dead code elimination
- [ ] Constant folding
- [ ] Loop optimizations (unrolling, strength reduction)
- [ ] Inlining hints and monomorphization strategy
- [ ] SIMD opportunities detection
- [ ] Profiling integration
- [ ] Benchmarking suite

**Success Metric:** Compiled pyrst matches or exceeds hand-written Rust performance

---

### Phase 14: Tooling & IDE Integration (Est. 3-4 weeks)

**Goal:** Developer tools and IDE support.

**Key Tasks:**
- [ ] Code formatter (`pyrst fmt`)
- [ ] Linter (`pyrst lint`)
- [ ] Language server (LSP) for IDE integration
- [ ] Package manager / stdlib repository
- [ ] REPL / interactive mode
- [ ] Debugger support (via Rust debugging)
- [ ] Documentation generator

**Success Metric:** IDE autocomplete and error checking work as expected

---

### Phase 15+: Advanced Features & Ecosystem

**Optional Future Work:**
- Generators and `yield`
- Async/await support
- Coroutines
- Lambda closures
- Set collection type
- Binary/hex/octal literals
- Advanced type annotations (`TypeVar`, bounds)
- Overload resolution
- Protocols/structural subtyping
- Broad Python standard library compatibility
- Dynamic `Any` type support

---

## Deferred / Likely Non-Goals

These features are explicitly out of scope or deferred indefinitely:

- ❌ **Multiple inheritance** — Avoid Python's MRO complexity
- ❌ **Metaclasses** — Not needed for static compilation
- ❌ **Descriptors** — Overcomplicates object model
- ❌ **Monkey patching** — Incompatible with static types
- ❌ **`eval` / `exec`** — Dynamic code execution
- ❌ **Reflection API** — No runtime type queries
- ❌ **C extension compatibility** — Out of scope
- ❌ **Full Python stdlib** — Too broad; select useful modules instead

---

## Current Status (May 28, 2026)

### Completed (Phases 1-8)

✅ **Core Compiler (Phases 1-6):**
- Lexer with f-string support
- Recursive descent parser
- Two-pass type checker
- Code generator producing Rust
- 22 working single-file examples

✅ **Phase 7: Specification & Diagnostics**
- SPEC.md, PYTHON_COMPATIBILITY.md, RUST_BACKEND.md, DESIGN_DECISIONS.md
- Source code snippets in error messages
- ERRORS.md philosophy document

✅ **Phase 8: Module System**
- DFS import resolution with cycle detection
- Flat namespace symbol merging
- Multi-file compilation
- Multi-file example (common.py, math_utils.py, main.py)

### In Progress (Phase 9)

🟡 **Phase 9: Semantic Cleanup**
- Type narrowing for optionals (pending)
- Reference semantics decision (pending)
- Mutability inference (pending)

### Ready to Start (Phases 9-14)

✓ All specifications and design decisions in place
✓ No blockers for implementation
✓ Clear acceptance criteria for each phase

---

## Resource Estimates

| Phase | Duration | Effort | Priority |
|-------|----------|--------|----------|
| 9 | 3-4 weeks | High | **HIGH** |
| 10 | 2-3 weeks | Medium | **HIGH** |
| 11 | 3-4 weeks | High | **MEDIUM** |
| 12 | 4-6 weeks | High | **MEDIUM** |
| 13 | 4-6 weeks | High | **MEDIUM** |
| 14 | 3-4 weeks | Medium | **MEDIUM** |
| 15+ | Ongoing | Variable | **LOW** |

**Total to completion of core features (Phase 14):** 6-8 months

---

## Key References

### Closest Precedents

- **Codon** — Compiled Python variant with bidirectional typing and monomorphization
- **Mojo** — Pythonic systems language with ownership and FFI
- **Nim** — Compiled language with multiple backends and ARC/ORC memory model
- **RustPython** — Python implementation in Rust with clean architecture

### Design Documents (Repository)

- `SPEC.md` — Formal language specification
- `PYTHON_COMPATIBILITY.md` — Compatibility matrix
- `DESIGN_DECISIONS.md` — Design decisions and tradeoffs
- `RUST_BACKEND.md` — Rust code generation mapping
- `ERRORS.md` — Error diagnostics philosophy
- `GRAMMAR.md` — Language grammar
- `TYPE_SYSTEM.md` — Type system specification
- `IR_INVARIANTS.md` — Compiler IR constraints
- `RUNTIME_ABI.md` — Runtime ABI specification

---

## Development Workflow Principles

1. **Living Specifications First** — Design documents are source of truth; code follows docs
2. **Vertical Slices** — Complete features end-to-end before moving to next phase
3. **Example-Driven** — Every new feature becomes an executable example
4. **Regression Testing** — All prior examples continue to pass
5. **Transparent Scope** — Limitations and deferred features explicitly documented
6. **Semantic Clarity** — Prefer clear, constrained semantics over broad compatibility

---

## Conclusion

pyrst is at a healthy inflection point: the compiler has demonstrated viability, the semantics are well-defined, and the roadmap is clear. The next 6-8 months focus on deepening semantic understanding and building professional tooling, rather than racing toward feature parity with Python.

Success is measured not by "how many Python features," but by "how well does pyrst work for its intended purpose and how clear are its boundaries."

---

*Development Plan established: May 28, 2026*  
*Feedback-informed, specification-driven, ready for Phase 9*
