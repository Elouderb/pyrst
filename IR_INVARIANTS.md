# pyrst IR Invariants and Lowering Rules

## Overview

pyrst's compiler uses two intermediate representations:

1. **Typed HIR (High-level IR):** Close to the source syntax; preserves semantic structure (loops, conditionals, method calls, etc.).
2. **MIR (Mid-level IR):** Control-flow graph (CFG) based; explicit temporaries, simplified control flow, and lowered complex operations.

This document defines the invariants that both IRs must maintain and the lowering rules from AST → HIR → MIR → Rust codegen.

## Typed HIR Invariants

### Structure

The HIR is a tree-like representation of the typed AST. Each node has a type and a source location.

```rust
// Pseudocode
enum HirExpr {
    Literal(LiteralKind, Type),
    Var(DefId, Type),  // Reference to a definition (function, variable, class)
    BinOp(BinOp, Box<HirExpr>, Box<HirExpr>, Type),
    UnOp(UnOp, Box<HirExpr>, Type),
    Call(Box<HirExpr>, Vec<HirExpr>, Type),
    MethodCall(Box<HirExpr>, MethodId, Vec<HirExpr>, Type),
    FieldAccess(Box<HirExpr>, FieldId, Type),
    Index(Box<HirExpr>, Box<HirExpr>, Type),
    Assign(Box<HirExpr>, Box<HirExpr>, Type),
    AugAssign(AugOp, Box<HirExpr>, Box<HirExpr>, Type),
    If(Box<HirExpr>, Block, Option<Block>, Type),
    While(Box<HirExpr>, Block, Type),
    For(Name, Box<HirExpr>, Block, Type),
    Break(Type),
    Continue(Type),
    Return(Option<Box<HirExpr>>, Type),
    Raise(Box<HirExpr>, Type),
    ListLiteral(Vec<HirExpr>, Type),
    DictLiteral(Vec<(HirExpr, HirExpr)>, Type),
    ListComp(Box<HirExpr>, CompFor, Type),
    // ... more variants
}

struct HirFn {
    name: Name,
    params: Vec<(Name, Type)>,
    return_type: Type,
    body: HirBlock,
}

struct HirClass {
    name: Name,
    fields: Vec<(Name, Type)>,
    methods: Vec<HirFn>,
    base_class: Option<DefId>,  // Single inheritance only
}

struct HirModule {
    functions: Vec<HirFn>,
    classes: Vec<HirClass>,
    globals: Vec<(Name, Type, Option<HirExpr>)>,
}
```

### Typing Invariants

1. **Every expression has a type:**
   Every HIR expression node is annotated with its type. Type inference has already happened; there is no untyped code.

2. **All names are resolved:**
   Names are replaced with `DefId` references (pointing to function definitions, class definitions, or variable bindings). There are no unresolved name references in the HIR.

3. **All overloads are resolved:**
   Method calls and operators are resolved to specific implementations (trait methods, built-in operators, etc.). There is no late binding or dynamic dispatch in v0.

4. **No implicit coercions:**
   All type conversions are explicit (via function calls or constructor calls). Operand types must match expected types exactly.

### Control Flow Invariants

1. **Implicit return:**
   A function that reaches the end without an explicit `return` returns `None` (if the return type is `None`). Otherwise, it's a compile error.

2. **Unreachable code:**
   Code after an unconditional `return`, `raise`, or `break` in the same block is flagged as unreachable but does not cause a compile error (warning only in v0).

3. **Loop invariants:**
   - `break` can only appear inside a loop (`for` or `while`).
   - `continue` can only appear inside a loop.
   - All paths through a loop must eventually exit (no infinite loops detected in v0).

4. **Exception handling invariants:**
   - `raise` must pass an exception object (type to be defined).
   - `try`/`except` blocks catch specific exception types (deferred to v0.2 for full matching).

## MIR Invariants

### Structure

The MIR is a control-flow graph representation. It is more explicit than the HIR and is closer to the target (Rust/LLVM).

```rust
// Pseudocode
enum MirStmt {
    Assign(Lvalue, Rvalue),
    Call(Lvalue, DefId, Vec<Operand>),
    MethodCall(Lvalue, MethodId, Operand, Vec<Operand>),  // implicit self
    Return(Option<Operand>),
    Raise(Operand),
    Goto(BasicBlockId),
    Branch(Operand, BasicBlockId, BasicBlockId),  // if-else
    Switch(Operand, Vec<(Value, BasicBlockId)>, BasicBlockId),  // match
}

enum Rvalue {
    Use(Operand),
    BinOp(BinOp, Operand, Operand),
    UnOp(UnOp, Operand),
    Ref(Operand),  // Borrow
    Deref(Operand),
    Index(Operand, Operand),
    FieldAccess(Operand, FieldId),
    Call(DefId, Vec<Operand>),
    Literal(LiteralValue),
    ListLiteral(Vec<Operand>),
    DictLiteral(Vec<(Operand, Operand)>),
    // ... more variants
}

enum Lvalue {
    Local(LocalId),
    Field(Box<Lvalue>, FieldId),
    Index(Box<Lvalue>, Box<Rvalue>),
}

struct BasicBlock {
    id: BasicBlockId,
    stmts: Vec<MirStmt>,
    terminator: Terminator,  // goto, branch, return, raise
}

struct MirFn {
    name: Name,
    params: Vec<(LocalId, Type)>,
    return_type: Type,
    locals: Vec<(LocalId, Type)>,
    basic_blocks: Vec<BasicBlock>,
    entry_block: BasicBlockId,
}
```

### Control Flow Invariants

1. **CFG structure:**
   Every basic block ends with a terminator (`Goto`, `Branch`, `Return`, `Raise`, or `Switch`).
   No dangling statements after a terminator.

2. **All jumps are valid:**
   Every `Goto`, `Branch`, or `Switch` target must be a valid `BasicBlockId` in the function.

3. **All names are local or global:**
   All variable references use `LocalId` (local variable slot numbers) or global definitions. No free variables in v0.

4. **Single assignment form (SSA-like):**
   Each local variable is assigned exactly once (in its defining statement). Reassignments create new temporaries.
   (Strict SSA is deferred; MIR may have multiple assignments in v0.1+, but dataflow analysis must handle it.)

5. **Operands are pure:**
   An operand is either a local, a constant, or a reference. It has no side effects.

### Lowering Invariants (AST → HIR → MIR)

1. **Preserve source locations:**
   Every HIR and MIR node must be associated with a source span (file, line, column range). This enables accurate error reporting and source mapping in debug info.

2. **Type information flows:**
   Type information from the HIR is preserved in the MIR. Every temporary and local variable has an explicit type.

3. **Loops are lowered to branches:**
   - `for` loops are lowered to:
     1. Iterator initialization
     2. Branching on `iterator.next()` availability
     3. Loop body basic block
     4. Goto back to the branch
   - `while` loops are lowered similarly

4. **Complex expressions become temporaries:**
   Nested expressions are flattened into assignments to temporaries. E.g., `print(f(g(x)))` becomes:
   ```
   temp1 = g(x)
   temp2 = f(temp1)
   call print(temp2)
   ```

5. **Control flow is explicit:**
   - `if`/`elif`/`else` are lowered to conditional branches.
   - Exception edges are marked explicitly (deferred to v0.2 for full CFG-based exception handling).

6. **Method resolution is complete:**
   All method calls are resolved to either:
   - A specific function definition (DefId)
   - A trait method (via impl block)
   - A runtime helper (for dynamic dispatch, deferred)

## Exception and Control Flow Edges

### Exception Edges (Deferred)

In v0, exception handling is minimal. Exception edges (paths where an exception can be raised) are not explicitly modeled in the CFG. v0.2+ will add:

- Exception edges from function calls that can raise
- Exception edges from indexing/field access that can fail
- Explicit `try`/`except`/`finally` edges in the CFG

### Cleanup and Finally Blocks

`finally` blocks introduce cleanup semantics (deferred to v0.2). The MIR must model:
- Guaranteed execution of finally block on all paths (return, exception, normal exit)
- Preservation of exception state through finally block
- Nested try/finally handling

## Lowering Rules: Specific Operations

### Arithmetic Operations

| Operation | Lowering |
|---|---|
| `a + b` | Call to `__add__` impl or operator codegen (Rust `+`) |
| `a - b` | Call to `__sub__` impl or operator codegen |
| `a * b` | Call to `__mul__` impl or operator codegen |
| `a / b` | Call to `__truediv__` impl or operator codegen (note: Rust `/` is integer div for int types) |
| `a // b` | Integer division; operator codegen (Rust `/` for int) |
| `a % b` | Modulo; operator codegen (Rust `%`) |
| `a ** b` | Power; call to `pow` function or built-in |
| `a & b` | Bitwise AND; operator codegen (Rust `&`) |
| `a \| b` | Bitwise OR; operator codegen (Rust `\|`) |
| `a ^ b` | Bitwise XOR; operator codegen (Rust `^`) |
| `~a` | Bitwise NOT; operator codegen (Rust `!`) |
| `a << b` | Left shift; operator codegen (Rust `<<`) |
| `a >> b` | Right shift; operator codegen (Rust `>>`) |

### Comparison Operations

| Operation | Lowering | Notes |
|---|---|---|
| `a == b` | Call to `__eq__` impl or operator codegen (Rust `==`) | Returns bool |
| `a != b` | Negation of `==` | |
| `a < b` | Call to `__lt__` impl or operator codegen (Rust `<`) | |
| `a > b` | Desugared to `b < a` | |
| `a <= b` | Negation of `b < a` | |
| `a >= b` | Negation of `a < b` | |
| `a in b` | Call to `__contains__` on `b` or built-in (deferred) | |
| `a is b` | Identity check; pointer equality in Rust | |

### Boolean Operations

| Operation | Lowering | Notes |
|---|---|---|
| `a and b` | Conditional branch: if `a` is falsy, short-circuit to `a`; else evaluate and return `b` | Returns operand type or bool |
| `a or b` | Conditional branch: if `a` is truthy, short-circuit to `a`; else evaluate and return `b` | |
| `not a` | Unary operator; codegen (Rust `!`) | Returns bool |

### Method Calls

```
HirExpr::MethodCall(obj, method_id, args, return_type)
  ↓
MirStmt::MethodCall(temp, method_id, operand(obj), operands(args))
  ↓
Rust codegen: obj.method(args) or Module::method(obj, args) if method is a free function
```

Method calls are resolved at compile time. The method is looked up in:
1. The object's class definition
2. Base class definitions (if inheritance is involved)
3. Trait implementations (for dunder methods)

### Field Access and Indexing

```
HirExpr::FieldAccess(obj, field_id, return_type)
  ↓
MirRvalue::FieldAccess(operand(obj), field_id)
  ↓
Rust codegen: obj.field or (*obj).field if obj is a reference
```

Field access does not call getters or setters in v0 (properties deferred to v0.2).

```
HirExpr::Index(container, index, return_type)
  ↓
MirRvalue::Index(operand(container), operand(index))
  ↓
Rust codegen: container[index]
```

Indexing calls the `__getitem__` method or uses Rust's indexing trait.

### List and Dict Literals

```
HirExpr::ListLiteral([expr1, expr2, ...], list[T])
  ↓
MirRvalue::ListLiteral([operand(expr1), operand(expr2), ...])
  ↓
Rust codegen: vec![expr1, expr2, ...]
```

Dict literals are lowered similarly to a `HashMap::from_iter` or `BTreeMap::from_iter` call.

### Comprehensions

List and dict comprehensions are lowered to loops:

```python
[f(x) for x in items if g(x)]
```

Lowering:

```
temp_list = empty list
for x in items:
    if g(x):
        temp_list.push(f(x))
return temp_list
```

## Ownership Inference (v0 Minimal)

In v0, ownership is inferred conservatively. The compiler inserts `clone()` calls and `Rc` wrappers where necessary:

1. **Move semantics:** If a value is consumed by a function call and the type is not `Copy`, insert a `clone()` before the call.
2. **Reference counting:** If a value is shared among multiple basic blocks (e.g., loop iteration), wrap it in `Rc<T>` or `Rc<RefCell<T>>`.
3. **Borrowing:** If a value is passed by reference to a function, emit a `&` or `&mut` depending on the function's signature.

Ownership inference is **very conservative** in v0; it may insert more `clone()` and `Rc` than necessary. Optimization passes (deferred to v1.0+) can reduce these.

## Codegen Targets (Rust Source)

The final lowering target is Rust source code. The MIR is lowered to:

1. **Function signatures:** `fn name(param: Type, ...) -> ReturnType { ... }`
2. **Variable declarations:** `let mut x: Type = ...;`
3. **Statements:** Assignments, function calls, control flow (if, loop, match, return).
4. **Expressions:** Binary ops, unary ops, method calls, field access, indexing, literals.

The generated Rust code is then passed to `rustc` for final compilation to object files and linking.

## Debug Information

Source spans (file, line, column) are preserved through all IR levels and used to:

- Generate Rust source comments with original line numbers
- Emit debug symbols for debugger integration (deferred to v0.2+)
- Provide accurate error locations in compiler diagnostics

## Invariant Violations and Recovery

If an invariant is violated at any IR stage:

1. **Parse time:** Syntax error, emit diagnostic and stop.
2. **Type check time:** Type error, emit diagnostic and stop (no partial compilation).
3. **HIR construction:** Internal compiler error; should not happen if type check passed. Panic with diagnostic.
4. **MIR construction:** Internal compiler error; should not happen if HIR is valid. Panic with diagnostic.
5. **Codegen:** Internal compiler error; should not happen if MIR is valid. Panic with diagnostic.

In v0, there is **no error recovery**; the compiler stops at the first error. Error recovery is deferred to v0.2+.

## Future Extensions

- **v0.1:** More dunder methods, tighter ownership analysis.
- **v0.2:** Exception edges in CFG, pattern matching in match statements, type narrowing.
- **v1.0:** Full generics with bounds, subtyping, higher-order functions, tail-call optimization.
