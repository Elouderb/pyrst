# pyrst Rust Backend Mapping

This document explains how pyrst constructs are compiled to Rust.

---

## Type Mappings

### Primitive Types

```
pyrst               Rust
─────────────────────────────
int                 i64
float               f64
str                 String
bool                bool
None                Option<T>::None  (only in Optional[T])
```

### Collection Types

```
pyrst               Rust
─────────────────────────────────────────────
list[T]             Vec<T>
dict[K, V]          HashMap<K, V>
tuple[T1, T2]       (T1, T2)
T | None            Option<T>
```

### Class Types

```pyrst
class Point:
    x: int
    y: int
```

Compiles to:

```rust
#[derive(Clone, Debug)]
struct Point {
    x: i64,
    y: i64,
}

impl Point {
    fn new(x: i64, y: i64) -> Point {
        Point { x, y }
    }
}
```

---

## Variable Declaration and Mutability

### Simple Variable

```pyrst
x: int = 5
x = 10
```

Compiles to:

```rust
let mut x: i64 = 5i64;
x = 10i64;
```

**All pyrst variables are mutable** by default, generating Rust `mut` bindings.

### Type Inference

```pyrst
x = 5
y = x + 1
```

Compiles to:

```rust
let mut x = 5i64;
let mut y = x + 1i64;
```

Type is inferred from the right-hand side expression.

---

## Function Definition

### Simple Function

```pyrst
def add(a: int, b: int) -> int:
    return a + b
```

Compiles to:

```rust
fn add(a: i64, b: i64) -> i64 {
    return a + b;
}
```

### Function with Local Variables

```pyrst
def calculate(x: int) -> int:
    y: int = x * 2
    z: int = y + 1
    return z
```

Compiles to:

```rust
fn calculate(x: i64) -> i64 {
    let mut y: i64 = x * 2i64;
    let mut z: i64 = y + 1i64;
    return z;
}
```

### Void Functions

```pyrst
def print_value(x: int) -> None:
    print(x)
```

Compiles to:

```rust
fn print_value(x: i64) -> () {
    println!("{}", x);
}
```

---

## Classes and Methods

### Basic Class

```pyrst
class Point:
    x: int
    y: int
    
    def distance(self) -> float:
        return (self.x ** 2 + self.y ** 2) ** 0.5
```

Compiles to:

```rust
#[derive(Clone, Debug)]
struct Point {
    x: i64,
    y: i64,
}

impl Point {
    fn new(x: i64, y: i64) -> Point {
        Point { x, y }
    }
    
    fn distance(&mut self) -> f64 {
        ((self.x as f64).powf(2.0) + (self.y as f64).powf(2.0)).powf(0.5)
    }
}
```

**Note:** All methods receive `&mut self`, allowing in-place mutation.

### Field Access

```pyrst
p: Point = Point(1, 2)
x: int = p.x
p.x = 5
```

Compiles to:

```rust
let mut p: Point = Point::new(1i64, 2i64);
let mut x: i64 = p.x;
p.x = 5i64;
```

### Object Construction

pyrst class constructors are auto-generated for all fields:

```pyrst
p: Point = Point(1, 2)
```

Becomes:

```rust
let mut p: Point = Point::new(1i64, 2i64);
```

---

## Control Flow

### If/Elif/Else

```pyrst
if x > 0:
    print("positive")
elif x < 0:
    print("negative")
else:
    print("zero")
```

Compiles to:

```rust
if x > 0i64 {
    println!("positive");
} else if x < 0i64 {
    println!("negative");
} else {
    println!("zero");
}
```

### While Loop

```pyrst
count: int = 0
while count < 10:
    print(count)
    count = count + 1
```

Compiles to:

```rust
let mut count: i64 = 0i64;
while count < 10i64 {
    println!("{}", count);
    count = count + 1i64;
}
```

### For Loop

```pyrst
for i in range(10):
    print(i)
```

Compiles to:

```rust
for i in (0i64..10i64).into_iter() {
    println!("{}", i);
}
```

### For Loop with Enumerate

```pyrst
for i, item in enumerate(items):
    print(i)
```

Compiles to:

```rust
for (i, item) in items.iter().cloned().enumerate().map(|(i, v)| (i as i64, v)) {
    println!("{}", i);
}
```

### For Loop with Tuple Unpacking

```pyrst
for a, b in zip(list1, list2):
    print(a, b)
```

Compiles to:

```rust
for (a, b) in list1.iter().cloned().zip(list2.iter().cloned()) {
    println!("{} {}", a, b);
}
```

---

## Operators

### Arithmetic

```pyrst
a + b           (a + b)
a - b           (a - b)
a * b           (a * b)
a / b           (a / b)
a // b          (a / b)                    [integer division]
a % b           (a % b)
a ** b          ((a as f64).powf(b as f64))  [exponentiation]
```

### Comparison

```pyrst
a == b          (a == b)
a != b          (a != b)
a < b           (a < b)
a <= b          (a <= b)
a > b           (a > b)
a >= b          (a >= b)
```

### Logical

```pyrst
a and b         (a && b)            [short-circuit]
a or b          (a || b)            [short-circuit]
not a           (!a)
```

### Bitwise

```pyrst
a & b           (a & b)
a | b           (a | b)
a ^ b           (a ^ b)
~a              (!a)
a << b          (a << b)
a >> b          (a >> b)
```

### Membership

```pyrst
x in items      items.contains(&x)
x not in items  !items.contains(&x)
```

### Identity

```pyrst
x is None       matches!(x, None)
x is not None   !matches!(x, None)
```

---

## Collections

### List Literal

```pyrst
items: list[int] = [1, 2, 3]
```

Compiles to:

```rust
let mut items: Vec<i64> = vec![1i64, 2i64, 3i64];
```

### List Methods

```pyrst
items.append(4)
x: int = items.pop()
```

Compiles to:

```rust
items.push(4i64);
let mut x: i64 = items.pop().unwrap();
```

### List Indexing

```pyrst
x: int = items[0]
items[0] = 10
```

Compiles to:

```rust
let mut x: i64 = items[0usize as usize];
items[0usize as usize] = 10i64;
```

### Dictionary Literal

```pyrst
config: dict[str, int] = {"a": 1, "b": 2}
```

Compiles to:

```rust
let mut config: HashMap<String, i64> = vec![
    (String::from("a"), 1i64),
    (String::from("b"), 2i64)
].into_iter().collect::<HashMap<_, _>>();
```

### Dictionary Access

```pyrst
value: int = config.get("a", 0)
config["c"] = 3
```

Compiles to:

```rust
let mut value: i64 = config.get(&String::from("a")).cloned().unwrap_or(0i64);
config.insert(String::from("c"), 3i64);
```

### Tuple Literal

```pyrst
pair: tuple[int, str] = (42, "hello")
```

Compiles to:

```rust
let mut pair: (i64, String) = (42i64, String::from("hello"));
```

### Tuple Unpacking

```pyrst
(a, b) = pair
```

Compiles to:

```rust
let (a, b) = pair;
```

---

## Strings

### String Literal

```pyrst
s: str = "hello"
```

Compiles to:

```rust
let mut s: String = String::from("hello");
```

### F-String

```pyrst
name: str = "World"
greeting: str = f"Hello, {name}!"
```

Compiles to:

```rust
let mut name: String = String::from("World");
let mut greeting: String = format!("Hello, {}!", name);
```

### String Methods

```pyrst
s.upper()
s.lower()
s.strip()
s.split(",")
```

Compiles to:

```rust
s.to_uppercase()
s.to_lowercase()
s.trim()
s.split(",").map(|s| s.to_string()).collect::<Vec<_>>()
```

---

## Built-in Functions

### Print

```pyrst
print(1, "hello", 3.14)
```

Compiles to:

```rust
println!("{} {} {}", 1i64, "hello", 3.14f64);
```

### Len

```pyrst
length: int = len(items)
```

Compiles to:

```rust
let mut length: i64 = items.len() as i64;
```

### Range

```pyrst
for i in range(10):
    pass
    
for i in range(1, 11):
    pass
    
for i in range(0, 20, 2):
    pass
```

Compiles to:

```rust
for i in (0i64..10i64).into_iter() { }

for i in (1i64..11i64).into_iter() { }

for i in (0i64..20i64).step_by(2usize) { }
```

### Enumerate

```pyrst
for i, item in enumerate(items):
    print(i)
```

Compiles to:

```rust
for (i, item) in items.iter().cloned().enumerate().map(|(i, v)| (i as i64, v)) {
    println!("{}", i);
}
```

### Zip

```pyrst
for a, b in zip(list1, list2):
    print(a)
```

Compiles to:

```rust
for (a, b) in list1.iter().cloned().zip(list2.iter().cloned()) {
    println!("{}", a);
}
```

### Type Conversions

```pyrst
i: int = int(3.14)
f: float = float(42)
s: str = str(value)
b: bool = bool(1)
```

Compiles to:

```rust
let mut i: i64 = (3.14f64 as i64);
let mut f: f64 = (42i64 as f64);
let mut s: String = format!("{}", value);
let mut b: bool = ((1i64) != 0i64);
```

---

## Error Handling

### Assert

```pyrst
assert x > 0, "x must be positive"
```

Compiles to:

```rust
assert!(x > 0i64, "x must be positive");
```

### Raise

```pyrst
raise ValueError("message")
```

Compiles to:

```rust
panic!("{}\0{}", "ValueError", "message");
```

The payload is always the string `"<Type>\0<msg>"` (a NUL byte separates the type
from the message — it cannot occur in pyrst user data) so that `try`/`except`
can recover the exception type at the catch site.

### Try / Except / Finally

`try`/`except` is lowered onto `std::panic::catch_unwind` + handler dispatch on the
panic payload (see DESIGN_DECISIONS.md §11). A `base` exception catches its builtin
subclasses, `except E as e` binds `e` to the message string, `finally` always runs,
and an unmatched exception is re-raised after `finally`.

```pyrst
try:
    raise ValueError("bad")
except KeyError as e:
    print("key: " + e)
except LookupError as e:          # base type: also catches IndexError / KeyError
    print("lookup: " + e)
finally:
    print("cleanup")
```

Compiles to (shape; some boilerplate elided):

```rust
{
    // Suppress the default panic hook so a *caught* panic prints no stderr noise.
    let __prev_hook = ::std::panic::take_hook();
    ::std::panic::set_hook(::std::boxed::Box::new(|_| {}));
    let __try_result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
        panic!("{}\0{}", "ValueError", "bad");
    }));
    ::std::panic::set_hook(__prev_hook);            // restore before any re-raise

    let mut __reraise_msg: ::std::option::Option<String> = ::std::option::Option::None;
    let __reraise = match __try_result {
        ::std::result::Result::Ok(__ok) => { let _ = __ok; ::std::option::Option::None }
        ::std::result::Result::Err(__payload) => {
            let __exc_str: String = /* downcast payload to String / &str */;
            // Recover "<Type>\0<msg>".
            let (__exc_type, __exc_msg) = match __exc_str.split_once('\0') {
                Some((t, m)) => (t.to_string(), m.to_string()),
                None => (__exc_str.clone(), __exc_str.clone()),
            };
            if (__exc_type == "KeyError") {
                let e = __exc_msg.clone();          // `except KeyError as e`
                println!("{}", format!("key: {}", e));
                ::std::option::Option::None
            } else if (__exc_type == "LookupError"  // base OR-expands over subclasses
                   || __exc_type == "IndexError"
                   || __exc_type == "KeyError") {
                let e = __exc_msg.clone();
                println!("{}", format!("lookup: {}", e));
                ::std::option::Option::None
            } else {
                __reraise_msg = ::std::option::Option::Some(__exc_str.clone());
                ::std::option::Option::Some(__payload)   // no handler matched
            }
        }
    };

    // `finally` runs on every path, before any re-raise.
    println!("cleanup");

    // Unmatched: print the message (so uncaught exceptions stay visible) and re-raise.
    if let ::std::option::Option::Some(__p) = __reraise {
        if let ::std::option::Option::Some(ref __msg) = __reraise_msg { eprintln!("{}", __msg); }
        ::std::panic::resume_unwind(__p);
    }
}
```

`Exception` and a bare `except` compile to the catch-all (`true`) arm; in that case
the trailing `else` is `{ None }` and `__reraise_msg` is bound without `mut`.

---

## Compilation Strategy

### Overview

1. **Lexer** reads source text → tokens
2. **Parser** builds abstract syntax tree (AST)
3. **Type checker** validates types and symbols
4. **Code generator** walks AST → Rust source code
5. **rustc** compiles Rust source → native binary

### Ownership and Cloning

**Current strategy:** Aggressive cloning to avoid ownership complexity.

- Variables are bound with `let mut` (always mutable)
- Lists and dicts are cloned when passed to functions
- String interpolation uses `format!` macro
- Collections iterate via `.iter().cloned()`

**Impact:** Code is correct but not optimized. Future passes can insert borrowing and copy elision.

### Code Generation Preamble

All generated files include:

```rust
#![allow(unused_parens, unused_variables, unused_mut, dead_code)]

// ----- user code -----

fn main() {
    user_main();
}
```

This allows rustc to ignore some unused items while compiling.

---

## Notable Codegen Decisions

### Why `Vec<T>` for Lists?

- Dynamic sizing maps naturally to Python lists
- Efficient runtime behavior
- Standard Rust idiom
- Supports `.append()`, `.pop()`, indexing

### Why `HashMap<K, V>` for Dicts?

- Hash-based lookup matches Python dicts
- Good average-case performance
- No ordering guarantees (acceptable for now)
- Supports `.insert()`, `.get()`, indexing

### Why Value Semantics for Classes?

- Rust's default struct behavior
- Simpler ownership model
- Efficient compilation
- **Tradeoff:** Differs from Python reference semantics
- **Future:** May add reference wrappers if needed

### Why Aggressive Cloning?

- Avoids borrow checker complexity early on
- Allows correct semantics before optimizations
- Easy to remove later (backward compatible)
- Acceptable performance for prototype phase

---

## Limitations and Future Work

### Current Limitations

1. **No copy elision** — `i64`, `f64`, `bool` still cloned unnecessarily
2. **No borrowing** — Function arguments always moved/cloned
3. **No generics** — Built-in types only; no user-defined generics yet
4. **No inlining hints** — Generated code not annotated for optimization
5. **No const evaluation** — All computations at runtime

### Future Optimizations

1. Borrow checker integration
2. Copy elision for `Copy` types
3. Reference parameters for large collections
4. Inline hints and monomorphization
5. Const evaluation where possible

---

## Example: Full Program Compilation

### pyrst Source

```pyrst
def is_positive(x: int) -> bool:
    return x > 0

def main() -> None:
    items: list[int] = [1, -2, 3]
    for item in items:
        result: bool = is_positive(item)
        if result:
            print("positive:", item)
```

### Generated Rust

```rust
#![allow(unused_parens, unused_variables, unused_mut, dead_code)]

// ----- user code -----

fn is_positive(x: i64) -> bool {
    return x > 0i64;
}

fn user_main() -> () {
    let mut items: Vec<i64> = vec![1i64, (-2i64), 3i64];
    for item in items.iter().cloned() {
        let mut result: bool = is_positive(item);
        if result {
            println!("positive: {}", item);
        }
    }
}

fn main() {
    user_main();
}
```

---

*Last updated: May 28, 2026*  
*Reflects Phase 6 (post-review) compiler pipeline*
