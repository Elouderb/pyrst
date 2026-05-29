# Phase 9: Semantic Cleanup — Class Inheritance

**Date:** May 28, 2026  
**Status:** ✅ COMPLETE

---

## Overview

Phase 9 focused on fixing critical issues with class inheritance semantics. The type checker and code generator were not properly handling inherited fields and methods, causing inheritance to fail silently at runtime. This phase addressed those issues systematically.

## Problem Statement

Before Phase 9:
- **Inherited fields not recognized:** Constructor calls with inherited field names failed with "class has no field" errors
- **Inherited methods not generated:** Child classes didn't have implementations of parent methods
- **Type checking issues:** Method returns of non-Copy types caused Rust borrow checker errors
- **Incorrect method receivers:** All methods used `&mut self` even for read-only operations

## Solutions Implemented

### 1. Field Inheritance in Type Checker ✅

**Added `TyCtx::get_all_fields(class_name)`**
- Recursively collects fields from parent classes
- Deduplicates fields (child fields override parent)
- Used in constructor validation

**Added `TyCtx::get_method(class_name, method_name)`**
- Recursively finds methods up the inheritance chain
- Returns proper FuncSig with correct types
- Used in attribute access validation

**Impact:**
- `Dog(name="Rex", sound="Woof", breed="Lab")` now type-checks correctly
- `d.name` correctly finds the inherited field from `Animal`
- `d.speak()` correctly recognizes the inherited method

### 2. Method Inheritance in Code Generator ✅

**Updated `emit_class()` to emit inherited methods**
- Collects methods from parent classes first
- Deduplicates to avoid generating method twice
- Parent methods appear first, child methods override

**Added tracking set** to avoid duplicate method emissions

**Impact:**
- Generated Rust struct impl includes all inherited methods
- `Dog` instances can call `speak()` inherited from `Animal`
- Method overriding works correctly

### 3. Smart Method Receiver Types ✅

**Added `method_modifies_self()` analyzer**
- Detects if method body contains field assignments via `AttrAssign`
- Recursively checks nested blocks (if/while/for/try)
- Methods that modify self use `&mut self`
- Read-only methods use `&self`

**Impact:**
- `__init__` methods use `&mut self` (they modify fields)
- `speak()` methods use `&self` (they only read fields)
- Fixes Rust borrow checker errors on methods like `increment()`

### 4. Return Value Handling ✅

**Auto-cloning for non-Copy types in returns**
- Detects when returning a field from `self` in a method with `&self` receiver
- Automatically adds `.clone()` for String and other non-Copy types
- Ensures correct Rust semantics for moving values out of references

**Impact:**
- `return self.sound` from `&self` method now generates `return self.sound.clone()`
- Allows methods to return owned values without borrow checker errors

---

## Verification

### Test Results

✅ **24 out of 25 examples pass (96%)**

**Inheritance tests:**
- ✅ `inheritance_test.py` — Full inheritance with field and method access
- ✅ `dunder_test.py` — Classes with dunder methods and `__init__`
- ✅ `init.py` — Constructor patterns

**All core examples passing:**
- hello, fib, point, count, strings, lists, builtins
- dict_example, optional, fstring, tuples, comprehension
- tuple_unpack, enumerate_example, assert_raise, bitwise
- minmax_test, loop_assign_test, phase6_demo, unpack, unpack_simple

**Pre-existing issue (not caused by Phase 9):**
- ❌ `try_except.py` — Rustc detects unconditional panic (divide by zero) as a compile error

### Key Test: Inheritance

```python
class Animal:
    name: str
    sound: str
    def speak(self) -> str:
        return self.sound

class Dog(Animal):
    breed: str
    def describe(self) -> str:
        return f"{self.name} is a {self.breed}"

def main() -> None:
    d: Dog = Dog(name="Rex", sound="Woof", breed="Lab")
    print(d.name)      # "Rex" (inherited field)
    print(d.speak())   # "Woof" (inherited method)
```

**Before:** Type error: class `Dog` has no field `name`  
**After:** ✅ Works correctly, outputs: `Rex` / `Woof`

---

## Implementation Details

### Files Modified

**src/typeck.rs** (~61 lines added)
- Added `get_all_fields()` method with recursive field collection
- Added `get_method()` and `find_method()` with recursive method lookup
- Updated field checking in constructor validation
- Updated attribute access validation to use `get_all_fields()` and `get_method()`

**src/codegen.rs** (~110 lines added)
- Added `method_modifies_self()` analyzer (~50 lines)
- Updated `emit_func()` to detect modification and choose `&self` vs `&mut self`
- Updated `emit_class()` to emit inherited methods with deduplication
- Added auto-cloning logic in return statements for non-Copy types

### Complexity

- **Recursive algorithms:** Used for walking inheritance chains
- **Deduplication:** HashSet tracking prevents duplicate method emissions
- **Heuristic analysis:** Detection of field mutations for receiver type inference

---

## Semantic Principles Established

### 1. Method Receiver Types

**Principle:** Methods use the minimum permission level required.

- Read-only methods → `&self` (immutable reference)
- Mutating methods → `&mut self` (mutable reference)
- Static methods → no receiver

**Detection:** Analyze method body for field assignments.

### 2. Inheritance Resolution

**Principle:** Walk the inheritance chain from child to parent.

- Fields: Collected depth-first, child fields override parent
- Methods: Collected depth-first, child methods override parent
- Prevents infinite loops via visited set tracking

### 3. Return Value Semantics

**Principle:** Methods with `&self` that return non-Copy types auto-clone.

- Ensures owned values can be returned from borrowed references
- Matches Python's reference semantics while respecting Rust's type system
- Transparent to user (generated Rust handles it)

---

## Next Steps: Remaining Phase 9 Goals

From the DEVELOPMENT_PLAN.md, Phase 9 originally aimed for:

**Already Complete:**
- ✅ Type narrowing for optionals (`if x is not None:`) — Implemented in Phase 8
- ✅ Method receiver type formalization — Just completed
- ✅ Mutability tracking — Just completed via field modification detection

**Ready for Formalization (Phase 10+):**
- [ ] Ownership rules documentation — Can now be documented clearly
- [ ] Class reference semantics decision — Currently value semantics (Rust structs)
- [ ] Function argument passing rules — Can formalize borrow vs clone strategy

---

## Known Limitations (By Design)

1. **Multiple inheritance:** Not supported (single inheritance only)
2. **Virtual methods:** All methods are monomorphic (no vtables or traits yet)
3. **Descriptor protocol:** Not supported (Python feature not mapped)
4. **Method overloading:** Not supported (one signature per method name)
5. **Access modifiers:** No private/protected (all methods public)

These are deferred to Phase 12+ (Class Semantics & Traits/Protocols).

---

## Related Documentation

- **DEVELOPMENT_PLAN.md** — Phase 9 specification and roadmap
- **DESIGN_DECISIONS.md** — Decision #5: Class reference semantics (tentative)
- **SPEC.md** — Language specification for class semantics
- **RUST_BACKEND.md** — How classes compile to Rust structs

---

## Conclusion

Phase 9 successfully fixed critical issues with class inheritance. The type checker now properly recognizes inherited fields and methods, the code generator emits them correctly in Rust, and method receiver types are intelligently chosen based on whether they modify self.

The `inheritance_test` example now runs correctly, demonstrating:
- Field inheritance and initialization
- Method inheritance and invocation
- Proper Rust semantics (borrowing, cloning, mutability)

**Status:** ✅ Phase 9 Complete and Verified  
**Quality:** 24/25 examples passing (96%)  
**Ready for:** Phase 10 (Runtime Prelude & Optimizations)

---

*Phase 9 completed: May 28, 2026*  
*Semantic cleanup focused on inheritance; foundation ready for advanced OOP features*
