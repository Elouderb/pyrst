# pyrst Design Decisions

This document records key design choices, their rationale, and tradeoffs.

---

## 1. Compiler Architecture: Full Pipeline vs Interpreter

**Decision:** Build a full compiler pipeline (Lexer → Parser → Type Checker → Codegen) that targets Rust.

**Rationale:**
- Direct interpretation would be orders of magnitude slower
- Rust compilation provides strong safety guarantees
- A full pipeline is essential for static type checking
- Separating concerns (lex/parse/check/codegen) enables parallel evolution

**Tradeoff:** More complex architecture than a simple interpreter, but foundational for the project goal.

**Status:** ✅ Confirmed effective. Pipeline has proven scalable.

---

## 2. Target Language: Rust vs C/LLVM/Other

**Decision:** Generate Rust source code as an intermediate target, then compile with `rustc`.

**Alternatives Considered:**
- Directly target LLVM (lower-level, but requires learning LLVM IR)
- Target C (simpler codegen, but C has fewer safety guarantees)
- Interpret directly in Rust (slower, defeats purpose)

**Rationale:**
- Rust codegen is readable and debuggable (important for a language)
- `rustc` provides excellent error messages and optimization
- Rust's ownership model aligns with static typing goals
- Rust standard library provides needed data structures
- Reuse of a mature, well-tested compiler

**Tradeoff:** Adds compilation step; generated Rust code is not minimal; less control over low-level details.

**Status:** ✅ Confirmed effective. Readable generated code has proven valuable for debugging.

---

## 3. Type System: Mandatory Static Typing

**Decision:** All variables, parameters, and returns require explicit or inferred static types. No dynamic typing.

**Rationale:**
- Static types enable compile-time safety (the goal of targeting Rust)
- Python's dynamic typing is fundamentally incompatible with Rust's ownership model
- Type annotations improve code clarity and catch errors early
- Rust's type checker is mature and produces helpful errors

**Tradeoff:** Lost Python's flexibility and runtime type adaptation.

**Status:** ✅ Confirmed necessary. The entire type checking and code generation architecture depends on static types.

---

## 4. Two-Pass Type Checking

**Decision:** Collect all function/class signatures in a first pass, then validate bodies in a second pass.

**Rationale:**
- Python allows forward references (calling functions defined later)
- Enables recursive function definitions
- Allows arbitrary ordering of function/class definitions
- Single-pass would be too restrictive

**Alternatives Considered:**
- Single-pass (simpler, but requires definitions before use)
- Three-pass (more complex, not needed)

**Status:** ✅ Confirmed correct decision. Enables natural Python-like declaration ordering.

---

## 5. Class Semantics: Value Types (Rust Structs) Not Reference Types (Python)

**Decision:** Classes compile to Rust structs with methods. Assignment copies the struct (value semantics), not a reference.

```python
a = Point(1, 2)
b = a
b.x = 10
print(a.x)  # Prints 1, not 10
```

**Rationale:**
- Rust's default behavior
- Simpler ownership model (no Rc<RefCell<T>> overhead)
- More efficient code generation
- Aligns with Rust idioms

**Tradeoff:** **Major semantic difference from Python.** Python uses reference semantics; pyrst uses value semantics.

**Consequences:**
- Users familiar with Python may be surprised
- Method calls modify local copies, not shared objects
- Returning modified objects requires explicit return

**Status:** ⚠️ **Tentative.** This is the single largest semantic departure from Python. Future versions may revisit using reference wrappers (Rc<RefCell<T>>) for Python-like behavior. Documented in PYTHON_COMPATIBILITY.md.

**Future Consideration:** Could introduce syntax like `class ref Point:` for reference semantics if needed.

---

## 6. Ownership Strategy: Aggressive Cloning

**Decision:** Function calls receive moved/cloned arguments. Lists, dicts, strings are cloned liberally.

```python
items: list[int] = [1, 2, 3]
consume(items)
print(items)  # Works because items is cloned in consume()
```

**Rationale:**
- Avoids ownership/borrow complexity early in compiler development
- Preserves Python-like semantics (reuse values freely)
- Correct behavior trumps optimization at this stage
- Future passes can optimize later

**Tradeoff:** Generated Rust code has many unnecessary clones. Performance is not optimized.

**Status:** ✅ Acceptable for prototype. Must eventually be addressed via:
- Borrow checker integration
- Copy type detection
- Reference parameters for large collections

**Future Work:** Phase 9 (Semantic Cleanup) should formalize borrowing rules.

---

## 7. Mutability: Inferred Mutability

**Decision:** All variables are generated with `let mut` (mutable). Mutability is inferred implicitly, not explicit.

```python
x: int = 5
x = 10  # Works; x is mutable
```

**Rationale:**
- Python variables are rebindable by default
- Simplifies code generation
- Matches Python user expectations

**Tradeoff:** Loses Rust's compile-time immutability guarantees. All variables are mutable unless explicitly restricted later.

**Status:** ✅ Acceptable for now. Could be refined by tracking which variables are actually mutated.

**Future Refinement:** Track immutable vs mutable variables for better Rust code.

---

## 8. Generics: Monomorphization Instead of Runtime Generics

**Decision:** Generic types are instantiated per concrete usage (monomorphization). No runtime polymorphism yet.

**Rationale:**
- Matches Rust's monomorphization strategy
- Produces efficient code without runtime overhead
- Compile-time code duplication is acceptable

**Tradeoff:** Code bloat if many generic instantiations; no dynamic dispatch.

**Future:** Could add trait-based dynamic dispatch for polymorphism.

**Status:** ✅ Appropriate for a statically compiled language.

---

## 9. Functions: Mandatory Parameter and Return Types

**Decision:** All function parameters and returns require explicit type annotations.

```python
def add(a: int, b: int) -> int:
    return a + b
```

**Alternatives Considered:**
- Full type inference (hard with Rust backend)
- Optional type annotations with inference

**Rationale:**
- Clear function contracts
- Enables separate compilation and optimization
- Rust backend requires explicit types
- Documentation value

**Tradeoff:** More verbose than Python, but unambiguous.

**Status:** ✅ Confirmed necessary.

---

## 10. Classes: Single Inheritance Only

**Decision:** Classes support single inheritance (`class Derived(Base):`) but not multiple inheritance.

**Rationale:**
- Avoids Python's complex method resolution order (MRO)
- Rust traits and structs compose better with single inheritance
- Simplifies type checking and code generation
- Most programs don't need multiple inheritance

**Tradeoff:** Cannot emulate Python's diamond inheritance or mixins.

**Status:** ✅ Appropriate for a static type system.

**Future:** Could add traits for interface-like behavior.

---

## 11. Error Handling: Panic-Based with `catch_unwind`

**Decision:** Exceptions are modeled on Rust's panic/unwind machinery rather than
`Result<T, E>`. `raise` compiles to `panic!` with a structured-string payload, and
`try`/`except` is lowered to `std::panic::catch_unwind` + handler dispatch on the
payload. The lower-risk evolution of the original panic-based placeholder was chosen
over a ground-up `Result` rewrite (see card `be3c6353`).

**Payload format.** Every pyrst `raise` panics with the string `"<Type>\0<msg>"`,
using a NUL byte to separate the exception type from the message. A NUL cannot
appear in pyrst user data, so the separator can never collide with message content:

```python
raise ValueError("message")   # -> panic!("{}\0{}", "ValueError", "message")
raise ValueError              # -> panic!("{}\0", "ValueError")  (empty message)
```

The uniform `"<Type>\0<msg>"` shape lets the handler dispatch recover the
exception type. (A bare `raise` with no active exception emits `"explicit raise"`.)

**`try`/`except` lowering** (see RUST_BACKEND.md for the generated Rust):
- The `try` body runs inside `catch_unwind`. On `Err`, the payload string is split
  on the NUL byte `'\0'` via `split_once` into `(__exc_type, __exc_msg)`.
- Handlers are an `if`/`else if` chain matching on `__exc_type`; **first match wins**.
- **Exception-class hierarchy (builtin):** a base type catches its subclasses — e.g.
  `except LookupError:` catches `KeyError`/`IndexError`, `except ArithmeticError:`
  catches `ZeroDivisionError`/`OverflowError`/`FloatingPointError`. The condition
  OR-expands over the transitive builtin subclass set. `Exception` and a bare `except`
  are the catch-all (`true`) arm.
- **`except E as e` binding:** `e` is bound to the exception **message string**
  (`Ty::Str`), usable in the handler body.
- **`finally`** always runs (success, caught, and unmatched paths), before any re-raise.
- An **unmatched** exception is re-raised via `resume_unwind` after `finally`.
- **stderr hygiene:** the default panic hook is suppressed (`take_hook`/`set_hook`)
  around the `catch_unwind`'d body, so a *caught* exception prints no stderr noise; an
  *uncaught* exception still surfaces a message and a non-zero exit code.

**Status:** ✅ Confirmed / implemented (cards `be3c6353`, `286ac79`, `8789297`,
`15e4263`).

**Known limitations:**
- Only the **builtin** exception hierarchy is modeled; user-defined exception classes
  match by exact type name (no user-defined subclass catching).
- The payload carries only a **message string**, not a structured exception object.
- `take_hook`/`set_hook` run per `try` execution (allocates + global `RwLock`); a
  single global hook + thread-local suppress flag is a possible future optimization.

---

## 12. Collections: Standard Rust Data Structures

**Decision:** Use Rust standard library types directly: `Vec<T>`, `HashMap<K, V>`, native tuples.

**Rationale:**
- Well-tested, optimized implementations
- Natural fit for Rust idioms
- No runtime overhead vs custom wrappers
- Simpler codegen

**Tradeoff:** Some Python-specific collection behavior is lost (e.g., `list` is a class in Python; ours is a Vec<T>).

**Status:** ✅ Confirmed correct.

---

## 13. String Handling: Owned Strings (String, Not &str)

**Decision:** pyrst `str` type maps to Rust `String` (owned), not `&str` (borrowed).

**Rationale:**
- Ownership is clearer (pyrst owns strings)
- No lifetime complexity
- Strings are mutable (can be reassigned)
- Fits aggressive cloning strategy

**Tradeoff:** Heap allocation for every string; less efficient than borrowed refs.

**Future:** Could optimize by detecting immutable strings and using `&'static str` where possible.

**Status:** ✅ Acceptable for now.

---

## 14. Code Generation Target: Readable Rust (Not Minimal)

**Decision:** Generate Rust code that is readable and debuggable, not minimal or fully optimized.

**Tradeoff:** Generated code is larger and slower than possible, but easier to debug and understand.

**Benefit:** Users can inspect generated `.rs` files to understand how their code compiles.

**Status:** ✅ Confirmed valuable design decision. Readability helps with learning and debugging.

---

## 15. Imports: Deferred (All Code in Single File)

**Decision:** Import statements are lexed and parsed but not enforced. All pyrst code lives in one file.

**Rationale:**
- Simplifies initial compiler implementation
- No module system complexity
- Single-file programs are sufficient for prototyping
- Module system is a large undertaking

**Tradeoff:** Cannot build multi-file projects yet. Limits scalability.

**Status:** ⚠️ Acceptable for Phase 6. **Must be addressed in Phase 8.**

**Future:** Implement proper module system with same-directory imports, then packages.

---

## 16. Decorators: Parsed But Not Enforced

**Decision:** Decorator syntax (`@decorator`) is lexed and parsed but has no semantic effect.

```python
@dataclass
class Point:
    x: int
    y: int
```

Compiles to a regular class (decorator ignored).

**Rationale:**
- Avoids decorator transformation complexity early
- Users familiar with Python syntax can write it (even if ignored)
- Correct parsing enables future decorator support

**Tradeoff:** Decorators don't work (yet).

**Status:** ⚠️ Placeholder for future. Design and implement in Phase 12.

---

## 17. Optional Types: T | None with `Option<T>` Mapping

**Decision:** Python's `Optional[T]` or `T | None` maps to Rust's `Option<T>`.

```python
x: int | None = None
if x is not None:
    print(x + 1)  # Cannot use type narrowing yet
```

**Rationale:**
- Natural Rust idiom
- Explicit `None` handling
- Enables compile-time safety

**Tradeoff:** Python's implicit `None` everywhere is lost.

**Missing:** Type narrowing (recognizing that `x` is `int` inside the `if` block). Planned for Phase 9.

**Status:** ✅ Works; could be more ergonomic with type narrowing.

---

## 18. List Comprehensions: Simple Form Only

**Decision:** Support `[expr for var in iterable if condition]` but not more complex forms.

```python
squares: list[int] = [x * x for x in range(10) if x % 2 == 0]
```

**Rationale:**
- Covers 90% of use cases
- Simpler codegen than nested/multiple comprehensions
- Desugars naturally to `.map().filter().collect()`

**Tradeoff:** Cannot do nested or multiple comprehensions.

**Status:** ✅ Appropriate for v0.

---

## 19. Precedence and Associativity: Python-Compatible

**Decision:** Operator precedence matches Python's.

**Rationale:**
- Users expect Python semantics
- Well-defined, well-documented
- No surprises

**Status:** ✅ Confirmed correct.

---

## 20. Specification-First Development

**Decision:** (NEW - from reviewer feedback) Create SPEC.md, PYTHON_COMPATIBILITY.md, and DESIGN_DECISIONS.md **before** Phase 7 features.

**Rationale:**
- Semantic clarity is more valuable than feature count at this stage
- Clear specifications prevent design debt later
- Compatibility matrix sets user expectations
- Recorded decisions prevent re-debating settled issues

**Status:** ⏳ In progress (May 28, 2026).

---

## Summary: Core Design Principles

### 1. **Static Compilation First**
All design decisions favor strong static typing and Rust compilation, even if it diverges from Python.

### 2. **Correctness Before Optimization**
Generate correct code (even with aggressive cloning) before optimizing performance.

### 3. **Clear Semantics Over Feature Count**
Better to have a few features with crystal-clear behavior than many features with subtle edge cases.

### 4. **Readability for Debugging**
Generated Rust code should be readable, not minimal.

### 5. **Python-Inspired, Not Python-Compatible**
Preserve Python's *ergonomics* and *common patterns*, not necessarily its *dynamic behavior*.

---

## Unresolved Decisions (Need Resolution Before Phase 8+)

1. **Class Reference Semantics** — Should classes use Rc<RefCell<T>> for Python-like semantics?
2. **Dynamic Behavior** — Is `Any` type supported? What about reflection?
3. **Type Narrowing** — When does the type system narrow types (e.g., in `if x is not None:`)?
4. **Borrowing Rules** — When do function arguments borrow vs move vs clone?

*Resolved:* **Exception Handling** — panic + `catch_unwind` model with builtin
exception-class hierarchy and `except E as e` binding (see §11). **Module System** —
cross-file imports are implemented.

See SPEC.md section 15 ("Semantics Not Yet Fully Defined") for details.

---

*Last updated: May 28, 2026*  
*Phase: 6 (post-review)*  
*Status: Under active refinement*
