# Exception Lowering — from `panic!`+`catch_unwind` to `Result<T, PyExc>` (v0.2+ direction)

**Design card:** eeca5e5b. **Status:** design only, no source modified. **Date:** 2026-07-01.

## Bottom line

pyrst's `raise`/`try`/`except` lowers to a Rust idiom that is **correct and complete for v0.1.x** but is architecturally tied to one fact about the host platform: **unwinding works and is cheap when not taken.** `raise` compiles to `panic!("<Type>\0<msg>")`; `try` wraps its body in `std::panic::catch_unwind`; the exception's type and message are recovered by downcasting the panic payload and splitting on a NUL byte; `return`/`break`/`continue` escaping a try body are threaded out of the `catch_unwind` closure through a small carrier enum, `__PyrstTryFlow<R>`. This is a real ceiling, not a style complaint: it silently stops working the moment the target profile sets `panic = "abort"`, or the target is one of the wasm triples that don't support unwinding, and it is fragile in a second, independent way — **every** raise site (builtin runtime helper, `.expect()` call, `@extern` binding) must hand-craft a `"<Type>\0<msg>"`-shaped string to be catchable at all; a handful of existing builtin-method panic sites already fail to do this and are **silently uncatchable today**, verified below.

The migration this document sketches is smaller than it looks: **`__PyrstTryFlow` does not need to be replaced.** Its job — threading `return`/`break`/`continue` across the try body's closure boundary — is orthogonal to *how the exception itself propagates*. Swap the closure's return type from `__PyrstTryFlow<R>` to `Result<__PyrstTryFlow<R>, PyExc>`, thread `?` through every fallible call inside the body, and match on `Result::Err` instead of downcasting a panic payload — the rest of `emit_try`'s shape (success-path `else`, `finally` always running before propagation, the post-`finally` flow re-issue) carries over almost line for line.

- **V1 — no architecture change; close the "already-uncatchable" gaps.** Several existing builtin-method panic sites (`dict[k]` mutable get, `list.pop()`, `dict.remove()`, `list.remove()`/`.index()`, integer `**` negative exponent, slice step zero) panic with a **plain, unstructured string**, not the `"<Type>\0<msg>"` convention the try dispatcher parses — so `except ValueError`/`except IndexError`/`except KeyError` cannot catch them even though Python raises exactly those types there. Fixing this is a same-mechanism, v0.1.x-shaped bug fix (make every panic site conform), independent of and safely landable before any Result work.
- **V2 — introduce `PyExc` and a can-raise inference pass, dual-tracked behind the existing mechanism.** Define the error type, build the whole-program "does this function (transitively) raise?" fixpoint (same shape as the `compute_mut_self`/`build_poly_map` prescans already in codegen), and prove it on a narrow slice (leaf free functions with no try of their own) before touching `emit_try` itself.
- **V3 — flip `emit_try`'s closure return type and thread `?`.** Retire `panic!`-based `raise` for every function the inference classifies as can-raise; keep `catch_unwind` only as a last-resort boundary (program entry, and optionally around an `@extern` call declared "may panic"). This is the release-gated step — it is the one that actually removes the `panic=abort`/wasm dependency, so it should not ship until can-raise inference is trustworthy on the full test corpus.

None of this is a v0.1.x code change. Everything below is source-confirmed against the current tree; every line number was grepped or read directly (not carried over from an older doc) and cross-checked against `pyrst emit` output on two probe programs.

## A. Current mechanism (source-confirmed)

### A.1 `raise` → `panic!("<Type>\0<msg>")`

`Stmt::Raise` lowering (`codegen.rs:3182-3210`):

| Source form | Emission | Line |
|---|---|---|
| `raise` (bare) | `panic!("explicit raise");` — no type/message structure at all | `codegen.rs:3184` |
| `raise Foo("msg")` | `panic!("{}\0{}", "Foo", <msg>);` | `codegen.rs:3198` |
| `raise Foo` (no call args) | `panic!("{}\0", "Foo");` — empty message, still NUL-delimited so type-matching parses it | `codegen.rs:3202` |
| `raise <other expr>` (rare: raising a bound name/value, not a constructor call) | `panic!("{}", <expr>);` — **no type prefix at all**, falls into the untyped bucket | `codegen.rs:3205-3208` |

The NUL byte is deliberately chosen because it "cannot appear in pyrst user data" (comment at `codegen.rs:3193-3197`), so a message containing arbitrary text — including one that happens to contain the old `" panic: "` separator this format replaced — never desyncs the type/message split.

### A.2 `try`/`except`/`finally` → `catch_unwind` + string-split dispatch

`emit_try` (`codegen.rs:3943-4243`) emits, for every `try`:

1. **Panic-hook suppression** (`codegen.rs:3967-3968`, restored at `:3996`) — a caught exception must print no stderr noise, so the default panic hook is swapped for a no-op for the duration of the `catch_unwind` call and restored immediately after, before any re-raise.
2. **The try body runs inside `catch_unwind(AssertUnwindSafe(|| -> __PyrstTryFlow<R> { .. }))`** (`codegen.rs:3982-3995`), where `R` is the enclosing function's Rust return type. `try_return_escape`/`try_loopctl_escape` are set to `true` for the duration (saved/restored at `:3988-3991`) so a `return`/`break`/`continue` inside the body lowers to `return __PyrstTryFlow::Return(v)` / `::Break` / `::Continue` (the `Stmt::Return`/`Break`/`Continue` arms gated on those flags, `codegen.rs:3150-3169`, `3211-3290`) instead of escaping the closure directly (which would silently return from the *closure*, not the function). The closure's tail is `__PyrstTryFlow::Normal` (`codegen.rs:3993`) when the body falls through normally.
3. **`Result::Ok(__flow)` (no panic):** the `else` body runs only when `__flow` is `Normal` (Python: `else` runs iff the try body completed without exception *and* without return/break/continue), and the flow signal is stashed into `__pyrst_flow` for the post-`finally` re-issue (`codegen.rs:4070-4086`).
4. **`Result::Err(__payload)` (a panic occurred):** the payload is downcast to `&str`/`String` (falling back to the literal string `"unknown panic"` for anything else, `codegen.rs:4092-4098`), then split once on `\0` into `(__exc_type, __exc_msg)` — `split_once` returning `None` (no NUL present) makes `__exc_type == __exc_msg == the whole string` (`codegen.rs:4103-4106`), which is exactly what happens to a `raise <other expr>` or a raw `.expect()`/`.unwrap()` panic from anywhere in the runtime (see A.4).
5. **Handler dispatch** (`codegen.rs:4113-4182`) builds an `if/else if` chain over `__exc_type ==` string-equality tests, one per handler, in source order. A catch-all handler (`except:` or `except Exception:`, `codegen.rs:4043-4045`/`4116-4117`) short-circuits to `"true"`. A typed handler OR-expands over `exc_descendants(name)` (A.3) so `except LookupError` also matches `IndexError`/`KeyError`; an unknown/user-defined type name falls back to an **exact** string match (`codegen.rs:4126-4139`). `except E as e` binds `e` as the message string, scoped as a local for the handler body only (`codegen.rs:4156-4172`).
6. **`finally` runs unconditionally**, after the whole `Ok`/`Err` match, before any re-raise (`codegen.rs:4189-4192`).
7. **Re-raise:** if no handler matched, the original message is printed to stderr and the panic payload is re-thrown via `std::panic::resume_unwind` (`codegen.rs:4197`) — this is what gives an *uncaught* pyrst exception a clean message plus a non-zero exit code (also documented at `PYTHON_COMPATIBILITY.md:396`).
8. **Post-`finally` flow re-issue** (`codegen.rs:4207-4238`): the stashed `__pyrst_flow` is matched to re-emit a real `return`/`break`/`continue` now that `finally` (and any re-raise) has run. When the try provably returns on every path (mirrors typeck's `stmt_definitely_returns` Try arm exactly, `codegen.rs:4032-4040`), the catch-all arm is `unreachable!()` instead of `{}` so the generated block has type `!`, not `()` (needed for a function whose last statement is such a try).

### A.3 `__PyrstTryFlow<R>` — the control-flow carrier

Declared once, in the generated preamble: `enum __PyrstTryFlow<R> { Normal, Return(R), Break, Continue }` (`codegen.rs:7089`). Two `Codegen` struct fields gate its use, both documented in detail at their declaration (`codegen.rs:100-122`):

- `try_return_escape: bool` — true while emitting a try body; stays `true` through a nested loop (a `return` there still escapes the *function*), suspended only inside a nested `def`.
- `try_loopctl_escape: bool` — true only at the try-body **loop level**; suspended inside a nested loop or `def` (a `break`/`continue` there targets the inner loop, a real Rust `break`/`continue`).

This split exists because `return` always targets the enclosing function regardless of nesting depth, while `break`/`continue` target whichever loop lexically encloses them — two different escape scopes multiplexed through one enum.

### A.4 Builtin exception hierarchy — string OR-expansion, two levels deep

`exc_descendants` (`codegen.rs:6824-6838`) is a hardcoded, two-level match over five builtin bases:

| Base | Descendants matched by `except <Base>:` |
|---|---|
| `ArithmeticError` | `ArithmeticError`, `ZeroDivisionError`, `OverflowError`, `FloatingPointError` |
| `LookupError` | `LookupError`, `IndexError`, `KeyError` |
| `RuntimeError` | `RuntimeError`, `RecursionError`, `NotImplementedError` |
| `NameError` | `NameError`, `UnboundLocalError` |
| `OSError` | `OSError`, `FileNotFoundError`, `PermissionError`, `IsADirectoryError` |

Any other name (a leaf builtin, or a user-defined `class MyErr(Exception)`) gets **exact string-match only** (`codegen.rs:4128-4130`). This is deliberate and documented elsewhere: `is_subclass` (`typeck.rs:517-531`) explicitly does not register builtins like `Exception` in `ctx.classes`, so a user exception subclass hierarchy (`class SpecificErr(MyErr)`) is unimplemented by design — confirmed again in `docs/design/class-subtyping.md:79-80,182-183` and `PYTHON_COMPATIBILITY.md:116,257`.

### A.5 Not every builtin-error panic site conforms to the convention — verified, not every one is catchable today

The convention in A.1/A.2 only works if **every** panic site formats its payload as `"<Type>\0<msg>"`. Grepping every `panic!(` in `codegen.rs` turns up two populations:

**Conforming (catchable today):** the arithmetic/parse helpers emitted once in the preamble — `__py_mod`/`__py_floordiv` → `"ZeroDivisionError\0..."` (`codegen.rs:7057,7065`), `__py_int_from_str`/`__py_float_from_str` → `"ValueError\0..."` (`codegen.rs:7072,7076`) — and two per-site structured-index paths: string subscript → `"IndexError\0string index out of range"` (`codegen.rs:6065`), list subscript → `"IndexError\0list index out of range"` (`codegen.rs:6078`), dict `.get()` → `"KeyError\0<key>"` (`codegen.rs:6045,6047`).

**Non-conforming (silently uncatchable — no `\0`, so `__exc_type == __exc_msg ==` the raw message, matching no handler and always propagating):**

| Site | Panic message | Line | Python raises |
|---|---|---|---|
| `dict[k]` mutable get (`d[k] += ...`-style access) | `"key not found"` | `codegen.rs:2860` | `KeyError` |
| `str.find`-as-index-or-panic | `"substring not found"` | `codegen.rs:4767` | (str.index) `ValueError` |
| `list.pop()` (empty list) | `"pop from empty list"` | `codegen.rs:4843` | `IndexError` |
| `dict.remove(k)` (missing key) | `"KeyError: key not found"` | `codegen.rs:4858` | `KeyError` — note the string literally *contains* the word `KeyError` but with no `\0`, so it does **not** parse as type `KeyError`; `except KeyError` will not catch it |
| `list.remove(v)` / `list.index(v)` (value absent) | `"value not found"` | `codegen.rs:4872,4875` | `ValueError` |
| integer `**` with negative exponent | `"negative exponent for integer ** integer"` | `codegen.rs:7082` | `ValueError` (Python actually returns a float; pyrst rejects at codegen) |
| slice step `== 0` | `"slice step cannot be zero"` | `codegen.rs:6106` | `ValueError` |

This was confirmed against real generated output, not just the source: a probe (`try: return xs[i] except IndexError as e: ...` for `xs: list[int]`) emits the *conforming* `panic!("IndexError\0list index out of range")` path and the handler correctly fires (`__exc_type == "IndexError"` matches) — so **`PYTHON_COMPATIBILITY.md:472`'s blanket claim that "builtin runtime errors are not catchable exceptions" is only true for the non-conforming population above, not universally**; list/string out-of-range indexing and dict `.get()`-style key lookup are, in fact, already caught by type today. That compat-doc line is out of scope to fix here (this is a design doc only) but is worth flagging to whoever owns it next.

## B. Limitations analysis

1. **`panic = "abort"` is fatal to the whole mechanism, not a degradation.** `catch_unwind` requires the `unwind` panic strategy; under `panic = "abort"` a panic terminates the process immediately, `catch_unwind` is documented (by `std`) to not catch anything, and every `try`/`except` in a compiled pyrst program silently stops catching **anything** — a raised `ValueError` a user explicitly wrote an `except ValueError:` for now aborts the whole binary. The generated `Cargo.toml` (`driver.rs:187-196`) writes only `[package]`/`[dependencies]`, no `[profile]` section, so today's builds get Rust's default `unwind` strategy — this is *why* it currently works, not evidence it is safe to keep relying on. Any future release-size/perf profile that flips to `panic = "abort"` (a common choice specifically because it shrinks binaries and drops unwind-table codegen) breaks every pyrst program's exception handling at once, with no compiler diagnostic pointing at the cause.
2. **Most wasm targets can't unwind.** `wasm32-unknown-unknown` has no native unwinding support in the toolchain paths pyrst would realistically target; `catch_unwind` there either does not compile the way it does on native, or requires exception-handling proposal support that is not yet a safe default assumption. A pyrst-to-wasm story (not currently pursued per the source, but a plausible v0.2+ ask given the language's "compiles to a real binary" framing) is blocked by this exact mechanism, not by anything else in codegen.
3. **Unwind cost on the exceptional path, plus a fixed tax on every `try` regardless of whether it raises.** Every `try` pays for a `catch_unwind` call, a panic-hook swap/restore pair, and (implicitly) unwind-table codegen for the enclosing function, whether or not the body ever actually raises. This is architecturally different from Rust's own idiom, where `Result` is zero-cost on the `Ok` path and the *type system*, not a runtime hook swap, enforces handling.
4. **String-typed matching is fragile in exactly the way A.5 demonstrates.** The dispatcher has no static guarantee that a panic site's payload conforms to the convention — it is a hand-maintained textual contract enforced by nothing (not a trait, not a lint, not a test that walks every `panic!` call site). New builtin-method panics added later can silently reproduce the A.5 gap unless a human remembers the convention every time.
5. **`@extern`/`@crate` interop is invisible to the mechanism today, which is actually consistent (not obviously wrong) but stops being consistent under Result-lowering.** An `@extern` function's body is an opaque Rust expression template (`codegen.rs:1690-1729`, validated by `validate_extern_func` starting `typeck.rs:913`); nothing declares whether it can panic. Under the *current* panic-based mechanism this is harmless by construction — if the template panics, the enclosing `catch_unwind` (wherever the call happens to be, even several frames up) catches it exactly like a pyrst-native `raise`, because `catch_unwind` doesn't care where inside its dynamic extent a panic originates. Under Result-lowering this symmetry breaks: a Result-returning function does not have a `catch_unwind` boundary anymore, so an `@extern` template that panics would propagate as a real Rust panic (and, absent `panic=abort`, unwind straight past the now-Result-based call stack) unless something explicit reintroduces a boundary at the FFI call site. See §D.

## C. Proposed lowering: `Result<T, PyExc>`

### C.1 `PyExc` — the error type

Not yet designed in detail (flagged as an open question, §F.1), but the shape needs at minimum: a type tag (either an enum discriminant per builtin exception name, or an interned `&'static str`/`String`) and a message `String`, to replace the two things `(__exc_type, __exc_msg)` already carry as a pair of `String`s (`codegen.rs:4103-4106`). A minimal placeholder:

```rust
enum PyExc {
    ZeroDivisionError(String), ValueError(String), IndexError(String), KeyError(String),
    /* … one variant per builtin exception name already known to exc_descendants + leaves … */
    User(String /* class name */, String /* message */),
}
```

An enum-per-name gives exhaustiveness checking and lets rustc catch a missing handler arm at the pyrst-compiler level (not applicable to *pyrst user* code, which still gets runtime dispatch, but useful internally); a flatter `(String, String)` pair is far less code to generate and is what today's mechanism already produces, so it may be the pragmatic V2 starting point with the richer enum as a follow-on. **This choice is the first thing V2-a must settle** — it changes the shape of every downstream `?`/match site.

### C.2 Can-raise inference vs. all-functions-return-`Result`

Two strategies, evaluated against the existing prescan precedent (`compute_mut_self`/`build_poly_map`, both run before emission and consulted by `emit_func`, mirroring the exact shape this analysis would need — see `docs/design/value-semantics.md` §C V3-b for the closest existing analogue, a monotone boolean call-graph fixpoint):

- **All-functions-return-`Result<T, PyExc>` (uniform).** Mechanically the simplest: one signature transform (`rust_ty`'s return-type emission gains a blanket `Result<.., PyExc>` wrapper), no whole-program analysis needed, every call site uniformly gets `?`. The cost is real: every function — including ones that provably cannot raise (`def add(a: int, b: int) -> int: return a + b`) — pays the `Result` wrapping/matching tax at every call site, and every `@extern` signature (which is exactly what the user hand-wrote in the template's surrounding Rust) would need to *also* return `Result` to compose, which is a much bigger ask of the FFI author than today's "just write a Rust expression" contract.
- **Can-raise inference (targeted).** A function is "can-raise" if it (a) contains a `raise` not fully covered by a catch-all `try`/`except` in the same function, (b) calls a can-raise function without a covering `try`, or (c) uses a "risky" builtin operation — list/dict subscript, `.pop()`/`.remove()`/`.index()`, `int()`/`float()` parsing, `**`, `//`/`%`, an un-annotated `@extern` call — without a covering `try`. This is a strictly bigger inference surface than the mut-self fixpoint it's modeled on: `method_modifies_self` only has to recognize a small, closed set of syntactic shapes (`AttrAssign`/`IndexAssign`/`MUTATING_METHODS` calls); "can raise" additionally needs to recognize *builtin operations that are fallible by construction*, which today are scattered across dozens of `emit_expr`/`emit_builtin_call` sites rather than centralized. This is the single biggest complexity driver in this whole migration — bigger than the fixpoint convergence itself (which is a solved, monotone-boolean problem the value-semantics doc already worked through for `&mut self`).

**Recommendation:** can-raise inference, for the same reason V1/V2 of the value-semantics doc chose targeted machinery over a blanket transform — it keeps the common, non-raising case idiomatic Rust with no `Result` noise, at the cost of a genuinely harder static analysis that must be proven incrementally (§E).

### C.3 Call-site `?`-propagation

A call to a can-raise function, from inside a can-raise caller, lowers to `let x = f(args)?;` instead of today's bare `let x = f(args);`. A call to a can-raise function from inside a `try` that has a covering handler does **not** propagate via `?` — it is a normal call inside the Result-returning try-body closure (see C.4), and the closure's own `?` sites are what the try's `match` dispatches on.

### C.4 `try`/`except` as a `match` — the mechanical core of the migration

Change `emit_try`'s closure return type from `__PyrstTryFlow<R>` to `Result<__PyrstTryFlow<R>, PyExc>` and let every can-raise operation inside the body use `?`:

```rust
let __try_result = (|| -> Result<__PyrstTryFlow<R>, PyExc> {
    // body statements; a can-raise call/op inside here uses `?`
    Ok(__PyrstTryFlow::Normal)
})();
match __try_result {
    Ok(__flow) => { /* identical to today: codegen.rs:4070-4086 */ }
    Err(__exc) => {
        // identical shape to today's if/else-if chain (codegen.rs:4113-4182),
        // but matching on __exc's tag directly instead of downcasting a panic
        // payload and splitting a NUL-delimited string
    }
}
```

No `catch_unwind`, no panic-hook dance (A.2 steps 1 and part of 7 disappear entirely for a fully-migrated try) — that's the actual performance/portability win. Everything else — the `else` clause gated on `Normal`, the OR-expansion over `exc_descendants` for a typed handler, the catch-all fallback, the `except .. as e` local binding — carries over as a match arm instead of an `if`/`else if` chain, unchanged in *meaning*.

### C.5 `finally`

Unchanged in position and unconditional-ness: it still runs after the `Ok`/`Err` match and before any propagation. "Re-raise an unmatched exception" (today: `resume_unwind`, `codegen.rs:4197`) becomes "return `Err(exc)` from this function" *if* the enclosing function is itself can-raise (propagate one level further with the same `__reraise: Option<PyExc>` stash-then-check idiom already used for `__reraise: Option<Box<dyn Any>>`, `codegen.rs:4055-4057`/`4197`) — structurally almost identical, just swapping the payload type and `resume_unwind` for a `return Err(..)`.

### C.6 Fate of `__PyrstTryFlow`

**It does not change.** As argued in the bottom line, the enum's entire job is threading `return`/`break`/`continue` across the try body's *closure* boundary — a concern that exists regardless of whether the closure's *error* channel is a panic or a `Result`. The only change is the closure's declared return type gains a `Result<.., PyExc>` wrapper around the existing `__PyrstTryFlow<R>`; `Normal`/`Return`/`Break`/`Continue` and the two escape-flag fields (`try_return_escape`/`try_loopctl_escape`, `codegen.rs:100-122`) are untouched.

## D. `@extern`/`@crate` interop story

Two options, not mutually exclusive:

**(a) Panic-boundary-at-the-call-site (hybrid).** Add an opt-in `@extern` annotation (surface syntax TBD — e.g. a second decorator argument, `@extern(may_panic=True)`) declaring that the binding's Rust template can panic. A may-panic `@extern` call occurring inside Result-lowered code gets a **local** `catch_unwind` wrapped around just that call, converting a caught panic into `Err(PyExc::from_extern_panic(..))` so the rest of the (now-`Result`-based) call chain stays panic-free above it. This is the closest analogue to today's behavior (an `@extern` panic is still catchable by a pyrst `try`) at the cost of one small unwind boundary per may-panic call site — rare, since most `@extern` bindings (arithmetic/string wrappers, per the FFI Phase-1 comment at `codegen.rs:1690-1697`) are not expected to panic.
**(b) Fatal-by-default.** An `@extern` panic is never caught — it aborts the program, same as an unannotated Rust dependency panic would in a normal Rust binary. Simpler to implement (nothing to build), but a real behavior regression from today (where an `@extern` panic happens to already be catchable, as an accidental consequence of `catch_unwind` not caring where a panic originates).

`@crate` itself (the `@crate("name", "version")` decorator, `parser.rs:966-993`, recorded in `Func::crate_deps` per `ast.rs:58-66`, materialized as one `Cargo.toml` dependency line at build time, `driver.rs:187-196`) is pure build metadata today — it has no runtime interop surface of its own; the only place external-crate *code* actually executes is inside an `@extern` template, so the panic-boundary question above is the whole story. If a future "call an arbitrary function from a `@crate`-declared dependency" surface lands (Phase 2+ per the parser comments), it inherits the same (a)/(b) choice.

**Recommendation:** design (a) as the eventual target (preserves today's observable behavior for the common "wrapped a possibly-panicking crate function" case) but do not block V2/V3 on it — an unannotated `@extern` call can conservatively default to (b) until the annotation surface lands, since that is a strict behavioral subset of today (currently-catchable becomes currently-fatal only for annotated-as-safe bindings that turn out to panic anyway, which is already a bug in the binding).

## E. Migration sequencing & what stays in v0.1.x

**Order:** V1 (independent quick-win, no architecture change) → V2 (define `PyExc`, build can-raise inference, prove on a narrow slice, dual-tracked behind the still-shipping panic mechanism) → V3 (flip `emit_try`'s closure return type; retire `panic!`-based `raise` for inferred can-raise code; keep `catch_unwind` only at `main` and, if §D(a) is chosen, at annotated `@extern` call sites).

- **v0.1.x keeps the entire current mechanism as-is.** This document authorizes no source change. If the lead spins V1's non-conforming-panic-site fixes (A.5) out as their own card, that card is a same-mechanism bug fix (make every panic site match the existing `"<Type>\0<msg>"` convention), not a step of this migration — it is listed here because it is the cheapest, safest, most immediately valuable thing adjacent to this analysis, and it should land (if at all) *before* V2 so the can-raise inference in V2 isn't built against a payload convention still known to have gaps.
- **V2 is dual-tracked and low-risk by construction:** it adds a new type (`PyExc`) and a new prescan (can-raise sets) without touching `emit_try` or `Stmt::Raise` emission at all — the existing mechanism keeps shipping untouched while V2 is being proven. The risk is entirely in the analysis' precision (C.2), not in destabilizing anything already working.
- **V3 is the only step with real blast radius** — it rewrites `emit_try` (the hottest, most heavily-commented function in `codegen.rs`) and every can-raise call site. It should not ship until: (1) the can-raise inference has been validated against the full existing try/except test corpus with zero silent-miscategorization regressions, and (2) the performance question in §F.7 has an answer, since "faster" is one of this migration's implicit selling points and should be verified, not assumed.
- **Prerequisite ordering against the rest of the roadmap:** per the current roadmap state, EPIC-4 (value semantics) and class-subtyping C2 are already the next gated big rocks ahead of this work; this document does not compete for that sequencing slot; it is scoped as v0.2+ design, explicitly not queued ahead of those.

## F. Open questions

1. **`PyExc` shape** (§C.1) — a per-builtin-name enum variant set (exhaustive, more codegen) vs. a flat `(type_tag, message)` pair (matches today's `(String, String)` almost exactly, far less new code). Needs a decision before V2-a starts.
2. **Can-raise granularity** — function-level ("this whole function is fallible or not") vs. call-site-level (only specific expressions inside an otherwise-infallible function are fallible). Function-level is far simpler to infer and emit; call-site-level avoids forcing `?`-noise into a function that only raises on one rare branch, but the finer granularity's interaction with `?`-propagation and the existing statement-by-statement emission model is unexplored.
3. **`@extern` panic-policy default** (§D) — fallible-by-default (safe, but a `catch_unwind` boundary and `Result`-wrapping tax on every FFI call) vs. infallible-by-default with an opt-in `may_panic` annotation (cheaper, matches today's implicit assumption, but silently wrong if a binding author forgets to annotate a template that does, in fact, panic).
4. **Should user-defined exception subclassing ride along with this migration?** `is_subclass` (`typeck.rs:517-531`) explicitly excludes builtins like `Exception` from `ctx.classes`, so `class SpecificErr(MyErr): pass` cannot be caught by `except MyErr:` today (confirmed again in `docs/design/class-subtyping.md:79-80,182-183`, `PYTHON_COMPATIBILITY.md:116,257`). A `PyExc` that carries a resolved ancestor chain (rather than a bare type-name string) *could* fix this as a side effect of V2/V3 — but it is a genuinely separate feature with its own scope, and folding it in risks scope creep on an already-large migration. Recommend treating it as an explicit stretch goal, not a requirement, and deciding this before V2-a locks the `PyExc` shape (since the shape decision in Q1 determines whether ancestor-chain support is cheap or expensive to retrofit later).
5. **Top-level unhandled-exception UX.** Today an uncaught `raise` prints the message to stderr and exits non-zero via `resume_unwind` reaching the default (restored) panic hook (`codegen.rs:4197`, `PYTHON_COMPATIBILITY.md:396`). Under Result-lowering, `user_main`'s `Result` needs an equivalent small unwrap-and-report wrapper at the real `fn main()` (`codegen.rs`'s `cg.line("fn main() { user_main(); }")` emission) so an unhandled `PyExc` still produces a clean one-line stderr message and a non-zero exit — not a raw `Result::Err` `Debug`-print, which would be a UX regression from today.
6. **Is wasm/`panic=abort` an active roadmap driver or a soundness concern raised preemptively?** No wasm target and no `[profile]` panic-strategy override exist anywhere in the current tree (`driver.rs:192-195` writes only `[package]`/`[dependencies]`) — this document treats the ceiling as real and worth designing around, but whether V2/V3 should be prioritized *now* versus after EPIC-4/class-subtyping-C2 depends on whether an actual wasm or `panic=abort` target is on the near-term roadmap. Needs a lead decision, not an implementation decision.
7. **Performance validation.** `catch_unwind` is documented as (near-)zero-cost on the non-unwinding path; `Result`-threading via `?` has a small but nonzero cost on every call in a can-raise chain, on *every* invocation, including ones that never raise. Before treating V3 as a strict improvement (rather than a pure portability trade), benchmark a representative can-raise-heavy program (deep call chains, hot loops with fallible indexing) under both lowerings.

## Relevant files

`src/codegen.rs` — `Stmt::Raise` (3182-3210), `emit_try` (3943-4243: hook suppression 3967-3968/3996, closure+flow-type 3982-3995, Ok arm 4070-4086, Err arm decode 4090-4107, handler dispatch 4113-4182, `has_catch_all`/reraise bookkeeping 4043-4057, finally 4189-4192, re-raise 4197, flow re-issue 4207-4238, `try_returns` diverge rule 4032-4040), `__PyrstTryFlow` struct fields (100-122) and enum declaration (7089), `exc_descendants` (6824-6838), non-conforming panic sites (2860, 4767, 4843, 4858, 4872, 4875, 7082, 6106), conforming panic sites (6045, 6047, 6065, 6078, 7057, 7065, 7072, 7076), `@extern` emission (1690-1729), `Codegen` struct (31). `src/typeck.rs` — `Ty` enum (9-onward, no `Result`/error variant exists today), `is_subclass` builtin-exclusion note (517-531), `validate_extern_func`/`validate_decorators` (886-911, extern validation from 913), `Stmt::Raise` typeck touch-points (997, 1144-1146, 1864, 3032, 3316, 3946, 4083). `src/parser.rs` — `@crate`/`@extern` decorator parsing (148-193, 966-993). `src/ast.rs` — `crate_deps` (58-66). `src/driver.rs` — generated `Cargo.toml` (96, 154-196, no `[profile]` section). `docs/design/value-semantics.md` — precedent for a whole-program monotone-boolean prescan (`compute_mut_self`, §C V3-b) that this document's can-raise inference (§C.2) is modeled on. `docs/design/class-subtyping.md` (79-80, 182-183) and `PYTHON_COMPATIBILITY.md` (116, 253-260, 396, 460-472) — existing documentation of the builtin-only exception hierarchy and the (partially stale, per §A.5) catchability claims.

## Probes run (verification, not part of the shipped doc)

Two `pyrst emit` probes confirmed the mechanism description against real generated output (not just source reading), written under the scratch dir, never committed:
- A `try/except ZeroDivisionError as e/finally` over `a // b` — confirmed the `catch_unwind`/hook-suppression/flow-match shape in §A.2 verbatim, including the `__PyrstTryFlow::Return`/`finally`/re-issue sequencing.
- A `try/except IndexError as e` over `xs[i]` (list subscript) — confirmed the *conforming* `"IndexError\0list index out of range"` payload from `codegen.rs:6078` is caught by type today, which is the direct evidence behind the §A.5 correction to `PYTHON_COMPATIBILITY.md:472`.
