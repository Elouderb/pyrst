# Changelog

All notable changes to pyrst are documented here. This project adheres to [Semantic Versioning](https://semver.org).

## [0.5.0] — 2026-07-07

The binary-data release: pyrst gets a real `bytes` type, a move-only handle kind, and the modules they unlock — `base64`, `struct`, `hashlib`, `hmac`, and a real `re.Match`. The stdlib grows from 46 to 50 modules. Every behavior is python3-verified.

### The `bytes` type (new builtin)

- **`b'...'` literals** with byte-valued escapes (`\xNN` and the named set; `\u`/`\N` and octal are honest-stricter rejections, truthfully documented against CPython's warn-and-keep-literal behavior). `bytes()`/`bytes(n)`/`bytes(list)` constructors with range checks.
- **The opposite-of-str shapes, everywhere:** `b[i]` → `int`, `b[i:j]` → `bytes`, iteration/comprehensions/`sorted`/`reversed` all yield ints — byte offsets, never char offsets. `repr` is byte-identical to CPython across all 256 values and both quote-selection cases; equality/ordering/dict-keys are lexicographic byte order.
- **26 methods**, oracle-first: `hex`/`fromhex`, the find family (`index` raises `ValueError`; CPython's empty-needle semantics), `count`, `replace`, `split`/`rsplit`/`join`, the strip family (the argument is a byte *set*), `upper`/`lower` (ASCII-only passthrough), the justify family (CPython's centering bias), sign-aware `zfill`, predicates, membership (`int in b` with CPython's out-of-range `ValueError`).
- **Codecs:** `str.encode()` / `bytes.decode()` (utf-8 strict) with a catchable `UnicodeDecodeError` whose message matches CPython across a 27-case battery, including the multi-byte range form. `bytes * -1` clamps to `b''` like CPython — and the identical pre-existing `str * -1` crash was fixed along the way.

### Opaque handles (move-only) + the File fix

- A new **move-only handle kind**: consuming a handle (assignment, argument, return, `del`) *moves* it, enforced by a use-after-move check with conservative branch/loop analysis. Everything a non-clonable value can't do — containers, class fields, generic parameters, closure captures, printing, equality — is an honest check-time error.
- **`file` is now a real handle**: nameable in signatures (`fh: file`), reassignable as a move, borrowable via `Mut[file]`. Two long-standing holes died: a reassigned file previously *passed check and failed rustc*, and `file` couldn't be named in a signature at all. `close()` is idempotent like CPython; operations after close raise CPython's exact `ValueError`.
- The compiler now emits to **pid-unique self-deleting temp paths** — concurrent builds of same-named files can no longer corrupt each other (previously ~1-in-6 spurious failures under concurrency).

### New stdlib: 46 → 50 modules

- **`base64`** — b64/urlsafe/b32/b16, with the decoder a faithful port of CPython's `binascii.a2b_base64` per-quad machine (verified against a 90+-row adversarial matrix).
- **`struct`** — `pack`/`unpack`/`calcsize` over `< > ! =` with byte-exact output including 32-bit float precision loss; finite f32 overflow raises CPython's `OverflowError`; documented pyrst API shape (list-based, int/float channels).
- **`hashlib` + `hmac`** — sha256/sha1/md5/sha512 via battle-tested RustCrypto crates; byte-identical through known-answer and RFC vectors, block-boundary and megabyte fuzz batteries; case-insensitive names.
- **`re.Match` is real** — `re.search`/`match_`/`fullmatch` return `Optional[Match]` with eagerly-extracted groups and char-offset spans (verified down to astral-plane subjects); `groupdict`, named groups, CPython-faithful `findall` shapes, and hand-rolled zero-width match semantics the underlying regex crate doesn't provide. The `$`-anchor-before-trailing-newline case is a catchable honest refusal (provably inexpressible without lookarounds).

### Language honesty

- Bare truthiness on `Optional` (`if m:`) is now rejected at check time (it previously passed check and failed at build); `is not None` narrows as before.
- `del xs[i]`-style silent no-ops died in the prior release; this release adds the reserved-name guard for `bytes` and truthful docs for every deferral.

### Quality

- `./test_all.sh`: **492/492 positive examples**, **275/275 negative fixtures** rejected at both `check` and `build`; **586 `cargo test` cases**; 0 compiler warnings. The dual-run parity harness runs 143 parity programs — **118 byte-identical against `python3`**, 25 documented `# parity: pyrst-only` with pinned oracle evidence. Roughly 35 review-confirmed defects — including silent-wrong-bytes base64 decodes, a struct float that silently saturated to infinity, and six check-passes-rustc-fails escapes through the new move checker — were found by the per-stage adversarial reviews and fixed before this release. As of this release there remain **no known cases where an accepted program produces wrong output**.

## [0.4.0] — 2026-07-07

The CLI release: pyrst programs now have module-level mutable state (`global`), read `sys.argv`, use `random`'s module-level API, and log through `logging`/`warnings` — the four unlocks of the W4 (G2) epic. Every behavior is python3-verified; the stdlib grows from 44 to 46 modules.

### Module-level mutable state (`global`)

- **The `global` keyword works end-to-end.** A module binding rebound under `global` (or holding a non-scalar-literal initializer) lowers to a `thread_local!` `Cell`/`RefCell` mutable static — zero `unsafe`, zero locking. Reads clone (pyrst's uniform value semantics); rebinds and in-place mutations (`items.append(x)` needs no `global`; `items = …` does — CPython-faithful) write the static. A scalar-literal, never-rebound binding keeps the immutable `const` path **byte-identically**.
- **CPython import-time init order.** Module-level initializers run eagerly before `main()`, interleaved around `import` statements in source order (DFS, first-import-once) — a root-level initializer that textually precedes an import runs before the imported module's, exactly like python3.
- **Rebinds work everywhere**: inside methods, operator dunders (`__eq__`/`__add__`/…), nested defs, and tuple-unpack targets (`global a, b; a, b = b, a`) — each shape golden-proven against python3. Closures read globals live at call time.
- **The honest-error surface** rejects, with messages citing the CPython equivalent: forward-referencing initializers (`x: int = y + 1` before `y` — CPython `NameError`), `global x` colliding with a parameter or appearing after a use of `x` (CPython `SyntaxError`s), `global` names that live in another module or are builtin stubs, type-changing global rebinds (including inside nested defs), plus `nonlocal` and cross-module writes (v1 deferrals). `del xs[i]` — which silently no-opped on locals, dicts, and globals alike — is now an honest error naming `pop()`.

### `sys.argv` — pyrst is a CLI language

- `sys.argv: list[str]` is real (initialized from `std::env::args_os()`, lossy-decoded so non-UTF-8 arguments can never crash startup where CPython would run). Index it, slice it, iterate it, `len()` it, branch on flags — dual-run byte-identical to python3 for `argv[1:]` (`argv[0]` differs by construction and is documented).
- The test harness threads a `# argv: a b c` directive to **both** runtimes with documented token semantics (whitespace-split, no quoting, no globbing), so CLI-shaped goldens dual-run under identical arguments.

### `random` module-level API

- `random.seed/random/randint/randrange/uniform/getrandbits/choice/sample/choices` over a hidden module-global generator — **MT19937 byte-identical to CPython after `seed(n)`**, including interleaved draws across the shared state and even post-error state (an empty-population `choices` advances the generator exactly once before its `IndexError`, like CPython). The old pyrst-only 2-arg free draws (`choice(rng, xs)`) are gone; the module API takes CPython's call shapes and its goldens dual-run. `random.shuffle` is an honest absence (CPython's in-place contract needs `Mut[T]`; `random.sample(xs, len(xs))` is the documented shuffled-copy idiom). Unseeded default is a deterministic `Random(0)` — a documented divergence from CPython's OS entropy; seed first for identical streams.

### `logging` + `warnings` — new modules (stdlib 44 → 46)

- **`logging`**: root-logger level state, `basicConfig(level=)` with CPython's first-call/repeat-no-op/implicit-configure semantics, `debug`/`info`/`warning`/`error`/`critical` emitting CPython's exact `LEVEL:root:message` format to **true stderr** — so `2>/dev/null` and friends behave byte-for-byte like python3. Level constants + `getLevelName`. Handlers/`getLogger` honest-deferred; the varargs `%`-interpolation shape is an honest arity error (pre-format with f-strings).
- **`warnings`**: `warn(message, category)` to stderr with the full `simplefilter` action set — `ignore`/`always`/`once`/`default`/`module`, and `error`, which raises a **catchable** `UserWarning` at warn time (dual-run golden). Unknown actions raise CPython's `AssertionError` shape. Dedup is per-message (CPython dedups per source location — a precisely documented divergence).

### Quality

- A typechecker fix along the way: a keyword-argument-bearing call to a nonexistent module function now gets the correct ``module `X` has no function `Y` `` diagnostic instead of a misleading ``undefined name `X` ``.
- `./test_all.sh`: **458/458 positive examples**, **227/227 negative fixtures** rejected at both `check` and `build`; **577 `cargo test` cases**; 0 compiler warnings. The dual-run parity harness runs 112 parity programs — **87 byte-identical against `python3`**, 25 documented `# parity: pyrst-only` (the stderr-output and bignum shapes, each with pinned python3 oracle evidence). Roughly 30 review-confirmed defects — including two silent-miscompile blockers (a `global` rebind inside an operator dunder lowering to a dead local; tuple-unpack rebinds writing fresh locals) — were found by the per-stage adversarial review stacks and fixed before any of this shipped. As of this release there remain **no known cases where an accepted program produces wrong output**.

## [0.3.0] — 2026-07-06

The module-system release: dotted submodules are real, every module gets its own namespace, and the flat-namespace co-import restriction that shipped in 0.2.0 is gone. Every fixed behavior is python3-verified; the stdlib grows from 41 to 44 modules.

### The module system: dotted submodules, per-module namespacing

- **Dotted imports work end-to-end.** `import os.path`, `from urllib.parse import urlparse`, and qualified calls (`os.path.join(...)`) all resolve real submodules — embedded packages (`lib/os/path.pyrs`, `lib/urllib/parse.pyrs`) alongside local user packages. Unresolved dotted imports are now honest check-time errors naming the missing submodule; the old silent truncation (`import os.path` silently typechecking as `import os`, `from os.path import join` silently binding `os.join`) is dead.
- **Per-module namespacing.** Every module's names emit into their own mangled namespace (`__pyrst_m_<owner>__<name>`) instead of one flat table. This dissolves **all 8 historical co-import restrictions**, each proven by a golden: `operator`+`re`, `html`+`re`, `os`+`shlex`, `re`+`shlex`, `datetime`+`time`, `platform`+`sys`, `copy`+`shutil`. The flat-namespace restriction section and its 8-pair collision table are retired from [PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md). Class names remain globally unique (class-vs-class collisions are still a named check-time error) — the one honest limit that remains.
- **New stdlib: 41 → 44 modules.** A faithful `os.path` (a full posixpath port — `join`/`splitext`/`split`/`normpath`/`abspath`/`relpath`/`expanduser`/the fs predicates — replacing the flat W1 stand-ins, which stay as deprecated aliases for one release); `urllib.parse` (the first non-`os` dotted package: `urlparse`/`urlunparse`/`urljoin` oracled against the RFC 3986 test matrix, `quote`/`unquote`/`urlencode`/`parse_qs`, UTF-8 percent-encoding); `collections.abc` (an honest, zero-runtime-name documentation module — pyrst's ABCs are compile-time-native, so this just explains the mapping).

### Compiler ergonomics

- **Optional narrowing** now covers the negative-guard idiom (`if x is None: return`/`raise`/`continue`/`break`) and `while`-loop traversal (`while cur is not None: ... cur = cur.next` — the linked-list walk), with loop-scoped lifetimes and assignment-kill invalidation so the narrowing can't outlive its guard.
- **Call-site default-fill uniformity:** constructors, methods, and dataclasses fill trailing defaults exactly like free functions do (`ConfigParser()`, `Fraction(0)`, positional defaults everywhere).
- **User `__bool__`** is wired into every truthiness context: `if`/`elif`/`while`/`assert`/`not`/`bool()`/`and`/`or`/ternaries/comprehension filters.
- **`except OSError`** now catches the complete CPython `OSError` family, not just the base class.
- **Descending ranges work:** `range(10, 0, -1)` iterates like CPython (was silently empty) for both literal and runtime steps; a zero step raises a catchable `ValueError`.
- CPython-valid 0-argument `int()`/`float()`/`str()`/`bool()`; `enumerate(it, start)`; `list()` over `range`/`enumerate`/`zip`.
- **Fixed silent miscompiles found en route** (both python3-verified): a bare-lambda parameter shadowing a different-typed outer local silently bound the outer type instead of the parameter's; and, caught by the W3 code review, a cross-module default-argument expression could silently resolve its helper function in the *caller's* module instead of its own, calling the wrong function.

### Quality

- `./test_all.sh`: **436/436 positive examples**, **206/206 negative fixtures** rejected at both `check` and `build`; **572 `cargo test` cases**; 0 compiler warnings. The dual-run parity harness runs 89 parity programs total — **70 byte-identical against `python3`**, 19 documented `# parity: pyrst-only` — on every suite run. 321 import-free goldens were proven byte-identical across the entire module-system refactor by a full emit-diff. As of this release there remain **no known cases where an accepted program produces wrong output**.

## [0.2.0] — 2026-07-06

The standard-library release: the stdlib grows from 15 to 41 modules, keyword arguments work everywhere, and a second honesty purge closes out the compiler's remaining silent-miscompile classes. Every fixed behavior is python3-verified; the dual-run parity harness now runs 51 programs byte-identical against CPython 3.12 on every suite pass.

### Standard library: 15 → 41 modules

- 26 new modules across two waves — a W1 fidelity pass over the original 15, then W2 growth to 41: `datetime`, `calendar`, `csv`, `configparser`, `html`, `shlex`, `fnmatch`, `difflib`, `fractions`, `graphlib`, `pprint`, `pathlib`, `shutil`, `tempfile`, `filecmp`, `io` (`StringIO`), `sys`, `enum`, `dataclasses`, `colorsys`, `copy`, `reprlib`, `stat`, `errno`, `platform`, `getpass`.
- Every module carries a fidelity score and a parity golden — dual-run against real `python3` where the surface is byte-for-byte compatible, marked `# parity: pyrst-only` where it deliberately diverges. See [PYTHON_COMPATIBILITY.md](PYTHON_COMPATIBILITY.md#standard-library)'s Standard Library matrix for per-module fidelity, surface highlights, and divergences rather than duplicating it here.
- The flat import namespace means two modules that export the same top-level name can't be co-imported; the 8 colliding pairs across all 41 modules (e.g. `operator.sub` vs. `re.sub`) now produce a named **check-time error** instead of a silent last-import-wins overwrite.

### Keyword arguments, everywhere

- Free functions, module-qualified calls, methods, and constructors all accept keyword arguments, with CPython's left-to-right evaluation order for mixed positional/keyword calls; constructor keyword arguments bind `__init__` parameters directly.
- `random.Random(seed)` is **seed-compatible with CPython**: a from-scratch MT19937 implementation following CPython's exact derivation chain makes `Random(42)`'s output sequence byte-identical to `python3`'s.

### Language & compiler

- `@dataclass` synthesizes onto pyrst's existing class machinery (the `__init__`/`__repr__`/`__eq__` it already generates).
- User classes as `dict` keys and `set` elements; recursive `Optional[Self]` class fields (linked lists, trees); class-level constants.
- CPython-exact `repr()`/`str()`/float formatting, including round-half-even tie-breaking on doubles, and `Optional[T]` printing.
- List-unpacking raises CPython's exact `ValueError`s on arity mismatch; hex/octal/binary/underscore-separated integer literals and scientific-notation float literals.
- `str.casefold`/`translate`/`maketrans`/`rsplit`/`expandtabs`, and `partition`/`rpartition` returning real tuples.
- `min`/`max` now work over classes that define `__lt__` and accept true n-ary argument lists (`min(a, b, c, ...)`); `sum(iterable, start)`.

### Correctness — the second honesty purge

This release's soul: another round of silent-miscompile classes found and killed, every one python3-verified. N-ary `min`/`max` and `sum(iterable, start)` used to silently drop arguments past the first two; `getattr`/`setattr`/`hasattr` were silent no-op stubs and are now honest errors; `int(s, base)` silently dropped the `base` argument; `str.find`/`rfind`/`index`/`rindex` returned Rust's byte offset instead of CPython's character offset, silently corrupting any downstream slice/index on a string with a multibyte character before the match; float `%`/`%=` double-rounded instead of CPython's fmod-based rule (`0.1 % 1.0` gave `0.10000000000000009` instead of `0.1`); `@crate` release builds silently wrapped `i64` overflow instead of trapping it; `tempfile` created world-readable temp files (now `0o700` dirs / `0o600` files); `str.expandtabs` miscompiled its tab-stop math; several call-argument evaluation-order inversions were corrected to CPython's left-to-right rule; and the module resolver's last-import-wins name collisions are now honest co-import errors (see the flat-namespace note above).

### Quality

- `./test_all.sh`: **393/393 positive examples**, **190/190 negative fixtures** rejected at both `check` and `build`; **543 `cargo test` cases**; 0 compiler warnings. The dual-run parity harness runs 69 parity programs total — **51 byte-identical against `python3`**, 18 documented `# parity: pyrst-only` — on every suite run.

## [0.1.3] — 2026-07-03

The idiom release: the Python you actually write now works. Every fix python3-verified; three adversarial review rounds on the final batch alone.

### Now working (all CPython-faithful)

- **Augmented assignment:** `s += "x"`, `s += s`, `list += list` (including `xs += xs`, doubling like Python; the right-hand list stays usable). Invalid combinations (`set |=`, `str -=`, …) get honest pyrst errors instead of raw rustc leaks.
- **Duplicate unpack targets:** `a, a = 1, 2` gives Python's left-to-right last-wins, with evaluation order preserved under side effects.
- **`zip` with 3–4 arguments** (flat tuples like CPython; 5+ is an honest "nest zip calls" error) and **`zip`/`enumerate` over strings**.
- **Tuple-keyed sorting:** `sorted(pairs, key=lambda t: t[0])` — the key-lambda parameter now knows its element type (`min`/`max` had the same bug). **Nested tuple indexing** (`t[0][1]`, any depth) compiles as field access; dynamic or out-of-range tuple indexes are clear errors.
- **Closures:** a nested `def` clones its non-`Copy` captures (the documented value-semantics snapshot) — reading a variable after a closure captures it now works; two closures over the same variable; returned closures. Capturing a generator or a `Mut[T]` by-reference parameter is an honest error with a verified workaround in the message.
- **`min`/`max`:** results are properly typed (whole-number float results print `2.0`, not `2` — eleven expected-output lines in five older examples were corrected against python3); `key=` sources are reusable; **`max(key=)` ties now return the first element like Python** (was Rust's last-wins — silent wrong output); string/bool arguments work; helper functions inside `key=` lambdas work (they were also being dead-code-pruned — fixed, affected `sorted` too); 2-arg forms don't consume their arguments; **empty `min()`/`max()` raises a catchable `ValueError`** with CPython's message (the float paths silently returned ±infinity).

### Quality

- `./test_all.sh`: 310/310 positive examples (+9), 140/140 negative fixtures (+4) at both gates; 524 `cargo test` cases; 0 compiler warnings; 5× full-corpus emit byte-stable. The no-known-wrong-output claim was re-verified: two silent-wrong bugs found by this release's reviews (max-tie-breaking, empty-min defaults) were fixed before tagging.

## [0.1.2] — 2026-07-02

Lazy generators and the silent-miscompile purge. Every change was independently code-reviewed with adversarial probing; runtime semantics were verified against CPython (python3) throughout. As of this release there are **no known cases where an accepted program produces wrong output**.

### Lazy generators (the last major semantic gap)

- **Generators are lazy.** A generator body compiles to an async-coroutine (Rust's compiler performs the state-machine transform; a ~60-line dependency-free prelude drives it as an `Iterator`). The body runs on demand — nothing executes until the first element is requested, side-effect ordering is byte-identical to CPython, and **infinite generators** (`while True: yield ...`) are safe to consume with `break` in O(1) memory. Previously a generator eagerly collected every value into a list, and an infinite generator hung forever.
- `Iterator[T]` is a real, distinct type (no longer an alias of `list[T]`). Generator **variables** advance in place: a drained generator yields nothing forever (Python-exact reuse semantics), nested loops over two generator variables work, and `sum(g)` twice gives the total then 0.
- Iterator-aware builtins: `list`/`sum`/`min`/`max`/`any`/`all`/`enumerate`/`zip`/`sorted` (+`set`) consume generators directly.
- Everything non-lazy is an **honest `check`-time error** with a materialize-with-`list(...)` suggestion: `len`/subscript/slice/`reversed`/`str`/binops/membership on a generator, generator↔`list[T]` passing, `Iterator[T]` parameters, generator methods, `yield` inside `try` (all deferred features are clearly rejected, never miscompiled). Four of those rejections are `TypeError` in CPython too.
- Documented divergence: a generator closing over locals captures a value-semantics **snapshot at construction** (CPython sees later mutations) — stated in the golden and the docs.

### Correctness — the silent-miscompile purge

- **Branch-divergent locals**: a bare local assigned incompatible types across sibling branches (`if`/`elif`/`else`, `try`/`except`, `match` arms, tuple unpacks — including assigns nested one block deep), or conditionally reassigned to a conflicting type and read after the block (liveness-checked), silently dropped one path's value. All shapes now reject at `check` with a what-plus-fix message. Same-type match-arm assignment read after the `match` now *works* (was a build error).
- **Tuple-unpack reassignment in nested blocks**: `if flag: a, b = b, a` silently didn't swap, and a tuple-unpack Euclidean GCD (`while b != 0: a, b = b, a % b`) hung forever. Unpacking now distinguishes declare vs reassign (existing bindings get a real tuple assignment; mixed unpacks evaluate the right-hand side fully first — swap-safe).
- **`sorted(key=..., reverse=True)`** silently ignored `reverse` — fixed with CPython's exact reversed-stable-sort semantics (equal-key elements keep their original order).
- **Comprehensions over `zip(...)`/`enumerate(...)`** failed to build; they now work over lists and generators, with filters, in list/set/dict comprehensions.
- **`with` over a user class** silently skipped `__enter__`/`__exit__` — now an honest error (`with open(...)` unchanged); the full context-manager protocol is tracked, gated on real exception objects.
- Codegen scope hygiene: shadow decisions no longer leak across block boundaries (an abandoned intermediate retype can't poison later assignments), and the bare sequential retype idiom (`x = [1]` … `x = "s"`) is golden-covered.

### Quality

- `./test_all.sh`: 301/301 positive examples (+22), 136/136 negative fixtures (+32) at both gates; 524 `cargo test` cases; 0 compiler warnings; 5× full-corpus emit byte-stable.

## [0.1.1] — 2026-07-02

Quality release: performance, correctness, and output-quality work on the compiler back end. Every change was independently code-reviewed; the slice work was verified against CPython on a 5,744-case oracle (0 mismatches).

### Performance

- **List index/slice reads no longer clone the container.** Every read went through a deep clone of the whole list (indexed loops were O(n²)); reads now borrow the base through generic prelude helpers and clone only the element, with a conservative clone fallback whenever borrowing isn't provably safe. A 50k-element index-sum dropped from 1.52 s to under 10 ms. Stepped string slices got the same treatment.

### Correctness

- **Builtin runtime errors are now catchable by their Python exception type.** `list.pop()` on empty → `IndexError`; missing dict key (including mutable/nested access) → `KeyError`; `list.remove`/`.index`, `str.index`/`.rindex`, negative integer `**=` exponent, zero slice step → `ValueError`; file I/O failures → `OSError`. Previously these panicked with unstructured messages that `except` could not match. The builtin hierarchy applies (`except LookupError:` catches `IndexError`/`KeyError`).
- **Slice semantics are CPython-exact** (`PySlice_AdjustIndices` for both step signs). Previously `xs[4:0:-1]` silently returned `[]`, `xs[-100:2]` panicked, and a list slice with step 0 silently returned `[]` — now `[4, 3, 2, 1]`, `[0, 1]` (clamped), and a catchable `ValueError` respectively. String slicing is char-based (multibyte-correct).
- **Same-base index-assign compiles.** `self.data[len(self.data) - 1] = v` (and every place-chain variant: nested `grid[len(grid)-1][0]`, aug-assign, attribute assign, mutating-method receivers, `Mut[T]` arguments) no longer hits rustc E0502 — all place-chain indices pre-evaluate into temps.
- **Class dunder operands are no longer consumed.** Reusing `h` after `h + h2` (by-value `std::ops` impls) was a compile error; operands now follow value semantics (places clone, temporaries don't).
- Deterministic output: struct fields emit in source declaration order (was HashMap iteration order — same input could produce differently-ordered Rust run to run).

### Emitted-code quality

- `pyrst emit` output is formatted with rustfmt when available (silent fallback), literals emit without redundant parens, and the declaration-hoisting double-init artifact folds away in the common case.

### Internal

- `typeck.rs` (10.7k lines) and `codegen.rs` (7.7k) split into focused submodules (move-only; emit output byte-identical).
- New design doc: `docs/design/exception-lowering.md` — the v0.2+ migration path from panic/`catch_unwind` to `Result`-based exceptions.
- `PYTHON_COMPATIBILITY.md`'s exception-catchability section corrected (it was wrong in both directions).

### Quality

- `./test_all.sh`: 279/279 positive examples (+10), 104/104 negative fixtures (+1) at both gates; 513 `cargo test` cases; 0 compiler warnings.

## [0.1.0] — 2026-06-30

First tagged release. pyrst is a statically typed, Python-like language that compiles to readable Rust and then to a native binary via `rustc`. Guiding principle: **honest errors over silent miscompiles**.

### Language

- **Core:** functions (typed params + defaults), classes with single inheritance, `super()`/`__init__`, dunder methods, `@property`/`@staticmethod`; class subtyping via companion-enum polymorphism.
- **Types:** mandatory static typing; `list`/`dict`/`tuple`/`set`; `Optional[T]` / `T | None` with narrowing; value semantics with `Mut[T]` by-reference parameters.
- **Generics:** generic functions `def f[T](..)`; **bounded** generics with trait bounds (`PartialOrd`/`PartialEq`/`Add`/`Display`/`Hash+Eq`) inferred from the operations on `T`, with transitive propagation across generic calls; **generic classes** `class Box[T]`; generic `Callable` parameters — all monomorphized.
- **Functions:** lambdas, nested-`def` closures with lexical capture, `Callable[[A], R]` first-class values.
- **Generators:** `yield`, consumable by `for` loops and comprehensions (eager evaluation).
- **Pattern matching:** `match`/`case` with literal, wildcard, and capture patterns + guards.
- **Exceptions:** `try`/`except`/`else`/`finally`, `raise`, type-matched handlers over the builtin hierarchy, `except E as e`.
- **Comprehensions** (list/set/dict), f-strings, triple-quoted strings, string escapes incl. `\b`/`\f`, tuple unpacking, operator chaining.
- **Module-level constants** (`NAME: T = <literal>`).

### Rust interop

- **`@extern`:** bind a Rust expression template behind a typed pyrst signature (Rust `std` and beyond).
- **`@crate("name", "ver")`:** depend on external crates — the build switches from single-file `rustc` to a generated Cargo project. Crate names/versions are validated to prevent `Cargo.toml` injection.

### Standard library (embedded; `import`-resolved)

- **Core:** `math`, `os`, `time`, `operator`, `functools` (`reduce`), `statistics`, `string`.
- **Data structures / algorithms:** `bisect`, `heapq`, `collections` (`Counter`, `deque`, `defaultdict`), `itertools`, `textwrap`.
- **Parsing / external:** `re` (via the `regex` crate), `json` (a pure-pyrst recursive-descent parser + serializer over a dynamic `JsonValue`, RFC-compliant error handling), `random` (a seedable `Random` class).
- Both qualified (`import heapq; heapq.heappush(..)`) and flat (`from heapq import heappush`) forms work, including for generic stdlib functions.

### Tooling

- Language server (`pyrst lsp`): diagnostics, hover, go-to-definition, completion, semantic tokens.
- VS Code extension.
- `pyrst fmt` (formatter), `pyrst lint`, `pyrst repl`.

### Quality

- `./test_all.sh`: 269/269 positive examples (build + run, output-matched), 103/103 negative fixtures rejected at `check` and `build`, multi-file import demos.
- 513 in-crate `cargo test` cases; 0 compiler warnings; CI green.

### Known limitations

Eager (non-lazy) generators; no qualified generic-class instantiation (`collections.deque[int]()` — use a flat import); no generic methods inside classes; no module-level mutable state (so no `random` module-level API / `sys.argv`); a generic `Callable[[T], R]` with two distinct type variables is an honest error. See README for the full, honest list.
