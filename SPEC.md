# pyrst Language Specification (v0.1)

**Status:** Preliminary specification. Subject to change as design decisions solidify.

---

## 1. Design Goals and Non-Goals

### Goals
- Compile statically typed Python-like programs to efficient Rust
- Preserve Python's ergonomic syntax and common semantics
- Enable strong compile-time safety guarantees via static typing
- Generate readable, idiomatic Rust code
- Support a growing subset of Python patterns

### Non-Goals
- Full Python compatibility or drop-in replacement
- Dynamic typing or runtime type changes
- Python standard library compatibility
- Metaclasses, descriptors, or advanced reflection
- Monkey patching or runtime class mutation
- Decorator-based transformation
- Multiple inheritance
- `eval`/`exec` or dynamic code execution

### Intentional Restrictions
- All variables must have static types (inferred or explicit)
- No dynamic attribute access (no `getattr`/`setattr`)
- Classes are immutable structures with methods (no runtime modification)
- Inheritance is single-level only
- Functions cannot be dynamically created or modified
- Module system is explicit (no circular imports)

---

## 2. Lexical Structure

### Keywords
```
and     as      assert  break   case    class   continue  def
del     elif    else    False   finally for     from      global
if      import  in      is      match   None    not       or
pass    raise   return  True    try     while   with      yield
```

### Operators
- Arithmetic: `+`, `-`, `*`, `/`, `//`, `%`, `**`
- Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Logical: `and`, `or`, `not`
- Membership: `in`, `not in`
- Identity: `is`, `is not`
- Bitwise: `&`, `|`, `^`, `~`, `<<`, `>>`
- Assignment: `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `//=`, `**=`, `&=`, `|=`, `^=`

### Comments
- Line comments: `# comment text`
- Docstrings not supported (tokens stripped)

### Whitespace and Indentation
- Indentation-based block structure (like Python)
- Tabs and spaces cannot be mixed
- One statement per line (except `;` separator)

### Literals
- Integers: `123`, `-456` (maps to i64)
- Floats: `1.5`, `3.14e-10` (maps to f64)
- Strings: `"hello"`, `'hello'`, `"""multiline"""` (maps to String)
- Booleans: `True`, `False`
- None: `None`
- Collections: `[1, 2, 3]`, `{"a": 1}`, `(1, 2)`
- F-strings: `f"value: {expr}"` with arbitrary expressions

---

## 3. Type System

### Primitive Types
```
int      →  i64
float    →  f64
str      →  String
bool     →  bool
None     →  Option<T>::None (in Optional types)
```

### Collection Types
```
list[T]     →  Vec<T>
dict[K, V]  →  HashMap<K, V>
tuple[T1, T2, ...]  →  (T1, T2, ...)
```

### Optional Types
```
T | None    →  Option<T>
```

Type narrowing is NOT currently supported. Use explicit `.unwrap()` or if-checks.

### Class Types
```
class Point:
    x: int
    y: int
```

Maps to:
```rust
#[derive(Clone)]
struct Point {
    x: i64,
    y: i64,
}
```

### Type Inference
- Function parameters require explicit types
- Return types require explicit annotations
- Local variables inferred from assignment (first assignment determines type)
- No type inference across multiple assignments (must be consistent)

### Generics and Monomorphization
Currently not explicitly supported in syntax, but monomorphization happens internally for:
- Standard container types (list, dict, tuple)
- Function parameter and return types inferred from usage

---

## 4. Variables and Mutability

### Variable Declaration
```python
x: int = 5              # explicit type
x = 5                   # inferred type
```

### Mutability
- All variables are mutable by default (generated with `mut` keyword in Rust)
- No explicit `const` or `immutable` declarations
- Mutation happens through reassignment: `x = 10`
- Collections are mutable: `list.append()`, `dict[key] = value`

### Scope Rules
- Variables exist from declaration to end of enclosing function or block
- Inner scopes shadow outer scopes
- Functions create new scopes
- Loop bodies do NOT create new scopes (updates persist)

---

## 5. Functions

### Definition
```python
def name(param1: Type1, param2: Type2) -> ReturnType:
    # body
    return value
```

### Requirements
- Parameter types are mandatory
- Return type is mandatory
- Return statements require matching types
- Functions can be forward-declared or out-of-order (two-pass type checking)
- Recursive functions are supported

### Default Arguments
Not yet supported.

### Keyword Arguments
Partially supported. Can be passed but not declared with defaults.

### Variadic Arguments
Not supported (`*args`, `**kwargs`).

### Closures and Lambdas
Not supported.

### Function Arguments Semantics
Functions receive arguments by move (ownership transfers).

**Important:** If you want to reuse a value after passing it to a function, you must clone it explicitly or design the function to return the value.

---

## 6. Classes and Objects

### Definition
```python
class Point:
    x: int
    y: int
    
    def move(self, dx: int, dy: int) -> None:
        self.x = self.x + dx
        self.y = self.y + dy
```

### Object Model
- Classes are compiled to Rust structs with methods
- Instances are **value types** (Rust semantics)
- **Important:** Assignment copies the struct, not a reference
- Methods receive `self` by mutable reference (`&mut self`)

### Inheritance
- Single inheritance only: `class Derived(Base):`
- Methods are looked up in the derived class first, then base
- `super()` is not supported (use base class name directly if needed)

### Constructors
- No `__init__` currently supported
- Default constructor takes all fields as arguments

### Field Access
- Direct field access: `obj.field`
- Field assignment: `obj.field = value`
- No attribute validation at runtime

### Methods
- Instance methods take `self` parameter
- Methods modify `self` in place
- Class methods and static methods not supported

---

## 7. Control Flow

### If/Elif/Else
```python
if condition:
    # block
elif condition:
    # block
else:
    # block
```

### While Loops
```python
while condition:
    # body
```

### For Loops
```python
for item in iterable:
    # body

for i, item in enumerate(items):
    # body (tuple unpacking)
```

Loop variables are bound for the loop duration.

### Break and Continue
- `break` exits the loop
- `continue` skips to next iteration

### Pass Statement
- `pass` is a no-op placeholder

---

## 8. Operators and Expressions

### Operator Precedence (highest to lowest)
1. Primary: `()`, `[]`, `.`, function call
2. Exponentiation: `**`
3. Unary: `-`, `not`, `~`
4. Multiplicative: `*`, `/`, `//`, `%`
5. Additive: `+`, `-`
6. Shift: `<<`, `>>`
7. Bitwise AND: `&`
8. Bitwise XOR: `^`
9. Bitwise OR: `|`
10. Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`
11. Membership: `in`, `not in`
12. Identity: `is`, `is not`
13. Logical NOT: `not`
14. Logical AND: `and`
15. Logical OR: `or`

### Short-Circuit Evaluation
- `and` and `or` short-circuit (like Python)
- `and` evaluates left-to-right, stops at first falsy value
- `or` evaluates left-to-right, stops at first truthy value

### Comparison Chaining
Not supported. Use explicit `and`: `x > 0 and x < 10`

### Truthiness
- `0`, `0.0`, `""`, `[]`, `{}`, `None` are falsy
- All other values are truthy

---

## 9. Collections

### Lists
```python
items: list[int] = [1, 2, 3]
items.append(4)
x: int = items[0]
items[0] = 10
```

- Homogeneous (all elements same type)
- Backed by Rust `Vec<T>`
- Support indexing and method calls (`.append()`, `.pop()`)

### Dictionaries
```python
config: dict[str, int] = {"a": 1, "b": 2}
value: int = config.get("a", 0)
config["c"] = 3
```

- Backed by Rust `HashMap<K, V>`
- Key and value types must be consistent
- `.get(key, default)` for safe access
- `.insert(key, value)` for insertion

### Tuples
```python
pair: tuple[int, str] = (42, "hello")
(a, b) = pair
```

- Fixed-size, heterogeneous
- Support tuple unpacking in assignments and for loops
- Cannot be modified (immutable in generated code)

### List Comprehensions
```python
squares: list[int] = [x * x for x in range(10)]
filtered: list[int] = [x for x in items if x > 0]
```

- Single target variable
- Single iterable
- Optional `if` filter

---

## 10. Strings

### String Literals
```python
s: str = "hello"
s2: str = 'hello'
s3: str = """multi
line"""
```

### F-Strings
```python
name: str = "World"
greeting: str = f"Hello, {name}!"
expr: str = f"2 + 2 = {2 + 2}"
```

### String Methods
- `.upper()`, `.lower()`
- `.strip()`, `.lstrip()`, `.rstrip()`
- `.split(sep)` - returns `list[str]`
- Indexing: `s[0]` (returns single character as `str`)

---

## 11. Built-in Functions

### I/O
```python
print(1, "hello", 3.14)  # prints space-separated values with newline
```

### Introspection
```python
length: int = len(items)      # length of sequences/mappings
```

### Type Conversion
```python
i: int = int(3.14)            # truncates to i64
f: float = float(42)          # converts to f64
s: str = str(value)           # converts to string
b: bool = bool(value)         # converts to bool
```

### Iteration
```python
for i, item in enumerate(items):  # yields (index, value) tuples
for a, b in zip(list1, list2):    # yields tuple pairs
for i in range(10):               # yields integers
for i in range(1, 11, 2):         # start, end, step
```

### Collections
```python
items: list[int] = list()         # empty list
empty: dict[str, int] = dict()    # empty dict
```

---

## 12. Assertions and Error Handling

### Assertions
```python
assert x > 0, "x must be positive"
assert condition
```

Maps to Rust `assert!` macro. Panic on failure.

### Raise Statements
```python
raise ValueError("message")
raise
```

Currently maps to `panic!`. Full exception handling deferred.

### Note on Exceptions
Python-style `try`/`except` are parsed but not yet implemented. Use assertions and panics for now.

---

## 13. Module System

**Status:** Not yet implemented.

Planned:
```python
import foo                    # import module foo
from foo import bar          # import symbol bar from foo
from foo import bar as baz   # rename on import
```

Currently all code must be in a single file.

---

## 14. Unsupported Python Features

Explicitly not supported (by design):

- **Dynamic typing** — All variables must have static types
- **Monkey patching** — Classes cannot be modified at runtime
- **Metaclasses** — Only basic classes supported
- **Descriptors** — Not part of object model
- **Multiple inheritance** — Single inheritance only
- **Method resolution order** — Not applicable
- **Decorators** — Parsed but not enforced
- **Generators and `yield`** — Not supported
- **Context managers** — `with` statements not supported
- **Exception handling** — `try`/`except` not yet implemented
- **`*args` and `**kwargs`** — Variadic arguments not supported
- **Default arguments** — Not supported
- **Lambda expressions** — Not supported
- **Comprehension scoping** — Comprehension variables leak to enclosing scope
- **Operator overloading** — No `__add__`, `__str__`, etc.
- **Property decorators** — No `@property`
- **Class variables** — Only instance attributes supported
- **Module-level code** — Top-level statements other than function/class defs not supported

---

## 15. Semantics Not Yet Fully Defined

These require explicit design decisions:

### Reference vs Value Semantics for Classes
- **Current behavior:** Value semantics (Rust structs)
- **Python semantics:** Reference semantics
- **Status:** ⚠️ May change in future versions

### Cloning and Argument Passing
- **Current:** Functions receive moved/cloned values
- **Future:** May support borrowed references
- **Status:** ⚠️ Subject to refinement

### Type Narrowing for Optionals
- **Current:** Not supported
- **Future:** `if x is not None:` should narrow type
- **Status:** ⚠️ Planned for Phase 9

### Dynamic Behavior and `Any`
- **Current:** Not supported
- **Future:** May add escape hatch
- **Status:** ⚠️ Under consideration

---

## 16. Implementation Notes

### Code Generation Target
- Generates valid Rust source code
- Uses standard library types: `Vec`, `HashMap`, `Option`, etc.
- Compilation via `rustc` produces native binaries

### Performance Considerations
- Aggressive cloning for simplicity (will be optimized later)
- No inlining hints or optimization attributes currently
- Collection operations use default Rust algorithms

### Error Handling
- Type errors reported with source spans, context lines, and visual indicators
- Parse errors reported with source locations and code snippets
- Lex errors show the exact token and surrounding code
- Runtime errors panic (via Rust panic mechanism)
- See [ERRORS.md](ERRORS.md) for error message philosophy and diagnostics approach

---

## Version History

- **v0.2** (May 28, 2026): Added references to ERRORS.md, updated error handling section with diagnostic improvements
- **v0.1** (May 28, 2026): Initial specification based on 19 working examples and Phase 6 completion

---

*This specification is preliminary and subject to change as design decisions solidify. See DESIGN_DECISIONS.md for rationale on key choices.*
