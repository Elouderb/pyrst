# contextlib Implementation Dossier

**Module:** contextlib  
**Scope:** contextmanager, closing, suppress, redirect_stdout  
**Surface Count:** 4  
**Parity Cases:** 36  
**Target Fidelity:** 2/5

---

## 1. SURFACE

| Name | Kind | Signature | Return Type | Semantics |
|------|------|-----------|-------------|-----------|
| `contextmanager` | fn | `(func)` | `_GeneratorContextManager` | Decorator that converts a generator function into a context manager; decorated fn becomes factory for ctx managers. |
| `closing` | class | `__init__(self, thing)` | `closing` instance | Context manager that calls `.close()` on exit; wraps any object with a close method. |
| `suppress` | class | `__init__(self, *exceptions)` | `suppress` instance | Context manager that catches and suppresses specified exception types; re-raises non-matching exceptions. |
| `redirect_stdout` | class | `__init__(self, new_target)` | `redirect_stdout` instance | Context manager that temporarily redirects sys.stdout to new_target; restores on exit. |

---

## 2. ERRORS

**contextmanager decorator:**
- No yield in generator: `TypeError: 'NoneType' object is not an iterator` (when entering context)
- Multiple yields in generator: `RuntimeError: generator didn't stop` (when exiting context after second yield)
- Non-generator function: `TypeError: 'str' object is not an iterator` (when entering context)
- Exception in setup (before yield): exception propagates immediately
- Exception in finally block: propagates after body exits

**closing class:**
- Object has no close method: `AttributeError: 'int' object has no attribute 'close'` (when exiting context)
- close() raises exception: exception propagates on exit (does not suppress)
- Passed None: `AttributeError: 'NoneType' object has no attribute 'close'` (when exiting context)

**suppress class:**
- Wrong exception type in context: exception propagates normally
- Non-exception class passed: silently accepted, no errors (doesn't try to catch it)
- Exception in body not in suppress list: propagates normally

**redirect_stdout class:**
- Target has no write method: `AttributeError: 'int' object has no attribute 'write'` (when writing to stdout)
- Invalid file-like object: error on first write operation

---

## 3. BEHAVIOR MATRIX

All probed with `python3 -c` and `print(repr(...))` for exact formatting.

### contextmanager
1. Basic yield: `@contextmanager` decorator on `def cm(): yield 42` → calling `cm()` returns `_GeneratorContextManager`, `__enter__()` yields value `42`
2. Lifecycle: setup→yield→cleanup all execute in order; `__enter__()` returns yield value; `__exit__()` runs finally block
3. With arguments: `@contextmanager def cm(a, b, c=10)` → `cm(1, 2, c=3).__enter__()` returns `6` (1+2+3)
4. Bare yield: `yield` with no value → `__enter__()` returns `None`
5. Exception in body caught by try/except: `try: yield except ValueError: pass` → exception suppressed, execution continues after context
6. Exception re-raised: `try: yield except ValueError: raise TypeError()` → exception type converted
7. Nested contexts: multiple `with` blocks execute in LIFO order for exit
8. Generator return value ignored: `@contextmanager def cm(): yield 1; return 2` → return value has no effect
9. Cleanup runs on exception: `finally:` block executes even if body raises
10. No generator protocol called: calling decorated function without `with` or `.\_\_enter\_\_()` returns manager object

### closing
11. Basic object: `closing(obj).__enter__()` returns obj unchanged
12. close() called on exit: `with closing(obj): pass` → `obj.close()` called
13. close() called on exception: `with closing(obj): raise ValueError()` → `obj.close()` called before exception propagates
14. Multiple close calls: calling `close()` manually then context exit → `close()` called twice
15. Return value: `with closing(obj) as ctx: ...` → ctx is obj (unchanged)
16. None value: `closing(None)` creates manager, error on exit: `AttributeError: 'NoneType' object has no attribute 'close'`

### suppress
17. Single exception suppressed: `with suppress(ValueError): raise ValueError("msg")` → executes normally, no exception
18. Exception not in list: `with suppress(ValueError): raise TypeError()` → TypeError propagates
19. Multiple exception types: `suppress(ValueError, TypeError, KeyError)` → any of these suppressed, others propagate
20. Subclass matching: `suppress(ValueError)` suppresses `CustomError(ValueError)` subclass
21. No exception: `with suppress(ValueError): normal_code()` → executes normally
22. Empty suppress: `suppress()` accepts all code, suppresses nothing (no exceptions specified)
23. Duplicate types: `suppress(ValueError, ValueError, ValueError)` → behaves identically to single specification
24. __enter__() return value: `suppress(ValueError).__enter__()` returns `None`
25. __exit__() on match: `__exit__(ValueError, error, tb)` returns `True` (suppress)
26. __exit__() on mismatch: `__exit__(TypeError, error, tb)` returns `False` (don't suppress)

### redirect_stdout
27. Basic capture: `with redirect_stdout(StringIO()) as ctx: print("hello")` → StringIO has "hello\n", ctx is StringIO
28. Multiple prints: three `print()` calls → all concatenated in StringIO with newlines
29. Direct write: `sys.stdout.write("test\n")` → captured in StringIO
30. sys.stderr unaffected: `sys.stderr.write()` during redirect → goes to real stderr
31. Nested redirect: `with redirect_stdout(cap1): ... with redirect_stdout(cap2): ...` → inner redirects to cap2, outer to cap1
32. Restoration on exception: `with redirect_stdout(cap): raise ValueError()` → sys.stdout restored before exception propagates
33. Empty output: `with redirect_stdout(cap): pass` → StringIO contains `''`
34. __enter__() return value: returns the target (StringIO)
35. __exit__() return value: returns `None`
36. sys.stdout stability: `sys.stdout is original_stdout` after context exits

---

## 4. HAZARDS

1. **Exception message text:** Error messages like `"'int' object has no attribute 'close'"` depend on exact CPython repr of object type names; pyrst must reproduce exact wording for error condition parity
2. **Generator state:** contextmanager relies on CPython generator protocol specifics (StopIteration detection, cleanup on close)
3. **sys.stdout reassignment:** redirect_stdout modifies module-level mutable state (sys.stdout) — Python allows this; pyrst module-level state rules may conflict (see GATED section)
4. **Exception class matching:** suppress uses `issubclass()` for exception type checking — matches derived classes as well as exact types
5. **Reference identity:** `__enter__()` return values for closing/redirect_stdout must be the exact same object (reference equality), not copies (pyrst value semantics may interfere)
6. **Traceback preservation:** exception context and `__cause__` chains must be preserved when contextmanager converts exceptions
7. **Close method invocation:** closing calls `.close()` unconditionally; if close() raises, that exception propagates (even from finally)

---

## 5. GATED

| Gate | API Part | Issue | Suggested Deferral |
|------|----------|-------|-------------------|
| **G4: No *args/**kwargs** | `suppress(*exceptions)` | Signature uses `*exceptions` variadic; cannot be spelled in pyrst | Defer suppress entirely; provide single-exception variant only if needed: `suppress_error(exc_type)` returns context manager |
| **with statement** | All four (contextmanager/closing/suppress/redirect_stdout) | Pyrst gates the with-statement behind exception objects; requires defining custom exception class which pyrst forbids | Can use manual protocol (`__enter__`/`__exit__` calls) but not convenient; major usability loss |
| **G2: Module-level mutable state** | `redirect_stdout` | Modifies sys.stdout at module level; pyrst forbids module-level mutable state | Can implement if sys module exports stdout as mutable reference; otherwise defer redirect_stdout |
| **Reference stability (value semantics)** | `closing`, `redirect_stdout` | Both return the object/target unchanged from `__enter__`; pyrst value semantics (deep copy on assign) may break identity checks | Probe pyrst's actual behavior: if `ctx.__enter__()` returns a deep copy, closing/redirect_stdout semantics fail; if by-ref params work, can salvage |

---

## 6. PARITY PLAN

36 dual-run-safe test cases for pyrst golden verification:

```python
# contextmanager tests
1. @contextmanager; def cm(): yield 42; v = cm().__enter__(); repr(v)
   → '42'

2. @contextmanager; def cm(x): yield x*2; v = cm(5).__enter__(); repr(v)
   → '10'

3. @contextmanager; def cm(): yield; v = cm().__enter__(); repr(v)
   → 'None'

4. @contextmanager; def cm(): yield [1, 2, 3]; v = cm().__enter__(); repr(v)
   → '[1, 2, 3]'

5. @contextmanager; def cm(a, b, c=10): yield a+b+c; v = cm(1, 2, c=3).__enter__(); repr(v)
   → '6'

6. manager = cm(); try: manager.__exit__(ValueError, ValueError(), None); except: pass; cleanup_called
   → True (if tracked in try/finally)

7. @contextmanager; def cm(): try: yield except ValueError: pass; exc raised in body
   → No exception propagates (suppressed)

8. @contextmanager; def cm(): try: yield except ValueError: raise TypeError(); exc raised
   → TypeError propagates (type changed)

9. @contextmanager; def cm(): yield 1; yield 2; manager.__exit__(None, None, None)
   → RuntimeError: generator didn't stop

10. @contextmanager; def cm(): setup tracked; yield; cleanup tracked
    → setup→exit called in order

# closing tests
11. class Obj: close_called = False; def close(self): close_called = True
    closing(Obj()).__enter__() is obj
    → True (returns exact object)

12. closing(Obj()).__exit__(None, None, None); obj.close_called
    → True (close() was called)

13. try: closing(Obj()).__exit__(ValueError, e, None); except: pass; obj.close_called
    → True (close() called even on exception)

14. closing(None).__enter__()
    → None (enter succeeds)

15. try: closing(None).__exit__(None, None, None); except AttributeError as e: repr(str(e))
    → "'NoneType' object has no attribute 'close'"

16. class NoClose: pass; try: closing(NoClose()).__exit__(None, None, None); except AttributeError as e: "close" in str(e)
    → True

# suppress tests
17. s = suppress(ValueError); s.__enter__()
    → None

18. s = suppress(ValueError); exc = ValueError("test"); s.__exit__(ValueError, exc, None)
    → True (returns True to suppress)

19. s = suppress(ValueError); exc = TypeError("test"); s.__exit__(TypeError, exc, None)
    → False (returns False, don't suppress)

20. s = suppress(ValueError, TypeError); exc = TypeError(); s.__exit__(TypeError, exc, None)
    → True

21. s = suppress(ValueError); s.__exit__(None, None, None)
    → False (no exception)

22. s = suppress(); s.__exit__(ValueError, ValueError(), None)
    → False (empty suppress list, return False)

23. class Custom(ValueError): pass; s = suppress(ValueError); exc = Custom(); s.__exit__(Custom, exc, None)
    → True (subclass matching)

24. s = suppress(ValueError, ValueError, ValueError); exc = ValueError(); s.__exit__(ValueError, exc, None)
    → True (duplicates accepted)

# redirect_stdout tests
25. r = redirect_stdout(StringIO()); target = r.__enter__(); isinstance(target, StringIO)
    → True

26. import sys; old = sys.stdout; r = redirect_stdout(StringIO()); r.__enter__(); sys.stdout is old
    → False (stdout changed)

27. import sys; old = sys.stdout; r = redirect_stdout(StringIO()); r.__enter__(); r.__exit__(None, None, None); sys.stdout is old
    → True (restored after exit)

28. cap = StringIO(); r = redirect_stdout(cap); r.__enter__(); sys.stdout.write("test\n"); r.__exit__(None, None, None); cap.getvalue()
    → 'test\n'

29. cap = StringIO(); r = redirect_stdout(cap); r.__enter__(); print("hello"); r.__exit__(None, None, None); cap.getvalue()
    → 'hello\n'

30. cap1 = StringIO(); cap2 = StringIO(); r1 = redirect_stdout(cap1); r1.__enter__(); r2 = redirect_stdout(cap2); r2.__enter__(); sys.stdout.write("inner\n"); r2.__exit__(None, None, None); sys.stdout.write("outer\n"); r1.__exit__(None, None, None); (cap1.getvalue(), cap2.getvalue())
    → ('outer\n', 'inner\n')

31. cap = StringIO(); r = redirect_stdout(cap); r.__enter__(); raise ValueError(); (should fail, but shows restore works)
    → ValueError propagates

32. old = sys.stderr; cap = StringIO(); r = redirect_stdout(cap); r.__enter__(); sys.stderr is old
    → True (stderr unaffected)

33. cap = StringIO(); r = redirect_stdout(cap); r.__enter__(); r.__exit__(None, None, None); cap.getvalue()
    → '' (no output)

34. r = redirect_stdout(StringIO()); r.__exit__(None, None, None)
    → None (returns None)

35. cap = StringIO(); r = redirect_stdout(cap); target = r.__enter__(); target is cap
    → True (exact object returned)

36. import sys; import io; orig = sys.stdout; cap = io.StringIO(); r = redirect_stdout(cap); r.__enter__(); sys.stdout.write("x"); r.__exit__(None, None, None); len(cap.getvalue())
    → 1
```

---

## 7. TARGET

**Fidelity: 2/5**

### Reasons:

1. **G4 blocker (suppress):** The `suppress(*exceptions)` signature uses variadic `*args`, which pyrst forbids. Suppress cannot be spelled as-is; would require single-exception variant or breaking API change.

2. **with-statement gate:** All four members rely on the `with` statement, which pyrst has gated pending exception-object support. Manual protocol calls (`__enter__`/`__exit__`) work but are not idiomatic; major usability loss for users.

3. **G2: Module-level mutable state (redirect_stdout):** `redirect_stdout` modifies `sys.stdout`, a module-level reference. If pyrst forbids mutable module state, redirect_stdout cannot be implemented at all. Deferred pending sys module design.

4. **Reference semantics (closing, redirect_stdout):** Both require returning the exact object (by reference) from `__enter__()`. Pyrst's value semantics (deep copy on assignment) may break this if not carefully handled. Requires verification that function return values bypass copy semantics.

### What CAN be deferred:

- **contextmanager** (minus with-statement use): The decorator itself can be implemented as a simple wrapper that returns a class with `__enter__`/`__exit__` methods. Works if generator protocol is available and users manually call protocol methods.
- **closing** (minus with-statement use): Same as contextmanager; manual protocol use works.
- **suppress** + **redirect_stdout**: Both deferred until *args and/or with-statement gates lift.

---

## Notes for Implementer

1. **Decorator support:** Confirm `@` decorator syntax and `@contextmanager` are available in pyrst before implementing.
2. **Generator semantics:** Verify pyrst generators support `yield`, `StopIteration` detection, and `close()` method.
3. **Exception class matching:** Confirm `issubclass()` available for exception hierarchy checking in suppress.
4. **Reference identity:** Test whether pyrst function return values preserve object identity (no deep copy) — critical for closing/redirect_stdout.
5. **Module state:** Clarify whether sys.stdout can be reassigned; if not, redirect_stdout is blocked.
6. **Deferral strategy:** Likely outcome is contextmanager (manual protocol only) + closing (manual protocol only) as 1/5 minimum viable; suppress and redirect_stdout deferred to a later phase after gates lift.
