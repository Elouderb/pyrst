# W5 — The `bytes` Type (G7) + Opaque External Handles (G1)

**Roadmap:** stdlib-full §E/§F W5 (G7 verdict: BUILD — "gates the whole binary-data
family"; G1 verdict: BUILD last — "OR pure reimpl", design-around first).
**Builds on:** EPIC-4 value semantics (`docs/design/value-semantics.md` — uniform
deep clone-on-use, no aliasing, `Mut[T]`→`&mut T`), W4 module globals
(`docs/design/w4-globals.md` — `thread_local!` Cell/RefCell, the clone-on-read
hazard), and the proven File proto-handle (`Ty::File`→`PyFile`). **Status:** design
only, no source modified. **Date:** 2026-07-07. **Baseline:** HEAD `8f1c3cc`
(v0.4.0). **Suite:** 685 examples (471 positive / 214 `fail_*` negative), 112
dual-run `parity_*` goldens, 49 embedded modules.

## Bottom line

**`bytes` is a VALUE; a handle is a REFERENCE.** That one distinction resolves both
gates and keeps EPIC-4 intact.

**G7 `bytes` → Rust `Vec<u8>`.** It rides pyrst's *existing* value-semantics
clone-on-use unchanged: `Vec<u8>` is `Clone` + non-`Copy`, the identical ownership
shape to `list` (`Vec<T>`), so `emit_consuming` already clones a `bytes` place with
**no new consuming-site rule** (rust-probe 1). Literals `b'...'` are a new lexer
prefix (mirroring the `f'...'` prefix at `lexer.rs:408`) with a **byte-valued** escape
path — the existing `\n\t\r\\\'\"\0\b\f` table plus a new `\xNN` hex — producing a new
`Tok::Bytes(Vec<u8>)`. The shape that must be gotten right and is **opposite to
`str`**: `b[i]` → **int** (`u8 as i64`), `b[i:j]` → **bytes** (`Vec<u8>`), iteration
yields **ints** — byte-offset indexing over the natural `Vec`, never `str`'s
char-offset `chars()` path (the W2 find-family lesson). `repr`/`str`/`print`/f-string
all emit the CPython repr `b'\x00abc'` byte-identically via a `__py_bytes_repr` prelude
(rust-probe 1 == py-probe 1). Equality/ordering/hashing/dict-key are free from
`Vec<u8>`'s derives. **bytearray is honestly deferred** (a mutable in-place API that
buys no additional module). The negative surface — `bytes + str`, `bytes == str` — is
an **explicit honest `check` error**, NOT the loose generic `+`/`==` path that today
lets `list + str` pass `check` and fail `rustc` (probe PN1/PN2).

**G1 handles = a distinct opaque kind, `Ty::Handle(name)`, v1 MOVE-ONLY**, generalizing
*and fixing* the File proto-handle. File today is a latent honesty hole: `g = f` on a
File local emits `let g = f.clone()` → **E0599** at `rustc` because `PyFile` is not
`Clone`, while `pyrst check` says OK (probe PF-A) — and File is **un-nameable** in a
signature (`fh: file` becomes a phantom `Class("file")` → "expected file, found file",
probe PF-B). A move-only `Ty::Handle` is a non-`Clone` opaque Rust struct (like
`PyFile`) whose consuming sites **move** (never `.clone()`) with a typeck
use-after-move check, is **nameable** in signatures, mutates through `&mut self`, and
closes via `Drop` + an explicit `close()` guarded by a `closed` flag (double-close =
honest `ValueError`). It stays 100% inside EPIC-4's static guarantee — no aliasing, no
runtime panic, no `unsafe` (rust-probe 2A). The **`Rc<RefCell<Inner>>` reference-handle**
(aliasing, Python-faithful resource sharing, refcount-close) is the *analyzed
alternative and documented v2*, deferred until `sqlite3`'s connection↔cursor sharing
actually funds it (rust-probe 2B).

**The unlock map shrinks G1 sharply.** `hashlib`/`hmac` are **pure value classes**
(Mut[self] `update`, `.copy()` free from clone-on-use) needing only G7 — no G1
(py-probe 2: incremental == oneshot). `re.Match` is a **pure struct** (eager group
extraction) needing neither gate. Only `re.Pattern` (regex cache), `subprocess`
(Popen), and — deferred — `sqlite3`/compression truly need G1. **bytes-before-handles,
compiler keystones separated from lib waves** (the W4 pattern). Validated by 2 `rustc`
probes, ~5 `python3` oracles, 8 `pyrst` today-probes (§G).

---

## A. What exists today — the baseline W5 must preserve

Source-anchored, with real-compiler probes (`pyrst check`/`emit`/`build`, this worktree).

| # | Behavior today | Evidence |
|---|---|---|
| Closed type set has **no `bytes`** | `enum Ty { Int Float Bool Str Unit NoneVal List Set Dict Tuple Option Iterator Class Func File TypeVar Unknown }`; `str`=`String`, `list`=`Vec`, no `Vec<u8>` variant | `types.rs:4` (enum), `:42` (`File`) |
| `b'...'` is a **parse error** (honest) | the `b` lexes as an `Ident`, the string as a separate token → "expected end of statement, found Str"; single- and double-quote both | probe **PT1/PT8** |
| `bytes()` / `.encode()` are **honest type errors** | "undefined function `bytes`"; "type `str` has no method `encode`" | probe **PT2/PT3** |
| `\x`/hex escape is a **lex error** (honest) | "unknown escape '\x'"; the escape table is `\n\t\r\\\'\"\0\b\f` only, in **three copies** (single-line/triple/f-string) each producing a **`char`** into a Rust `String` | probe **PT4**; `lexer.rs:469`, `:545`, `:592` |
| **`x: bytes` phantom-class hole** | an *unknown* type annotation (`x: bytes`, `x: Foo`) **passes `check`** as an opaque `Class` (would fail `rustc`) — a latent check-passes/rustc-fails hole | probe **PT6/PT7** |
| `str` is `String`, **char-indexed** | `maketrans`/`find`/index compare CODE POINTS via `.chars()`, not UTF-8 bytes (deliberate, python3-diffed) | `exprs.rs:269` (maketrans "Compare CODE POINTS") |
| **File is the proto-handle** | `open()`→`Ty::File`; lowers to `struct PyFile { inner: std::fs::File }` with `&mut self` `read/readlines/write/close`; RAII-`Drop` closes it in `with`; non-`Copy` | `types.rs:529` (`open`), `mod.rs:780` (`FILE_PRELUDE`), `exprs.rs:3931` (`rust_ty` File→`PyFile`), `flow.rs:3686` (`FILE_METHODS`), `:3101` (context-mgr) |
| **File dodges value semantics by being *restricted*, not sound** | (1) un-nameable in a signature → `fh: file` is a phantom `Class` ("expected file, found file"); (2) `g = f` emits `f.clone()` → **E0599** at `rustc` (PyFile not `Clone`) though `check` passes | probe **PF-B/PF-C**, **PF-A** |
| `is_copy(File)=false`; `emit_consuming` clones a place | `emit_consuming` clones an `Ident`/`Attr` place of a non-`Copy` type — correct for `Vec<u8>`, **broken for `PyFile`** | `flow.rs:1034` (`is_copy`), `items.rs:1666` (`emit_consuming`) |
| `@extern` binds a Rust **expression template** | body = one string literal; `{param}` holes substituted; return type must be in the **closed set** — no way to return/hold a foreign Rust struct (except the special-cased `File`); `@crate("name","ver")` adds a Cargo dep | `items.rs:167`; `re.pyrs` header (G1 tell: "recompiles a fresh `regex::Regex` per call") |
| Bytes stand-ins ship as `list[int]` | `os.urandom(n)`→`list[int]` each 0..=255 ("NOT a `bytes` object — pyrst has no `bytes` type"); `random.randbytes`/`getrandbits`>62 deferred on G7; `io.StringIO(b"...")`, `copy` bytes, `configparser` binary I/O, `difflib.diff_bytes`, `pprint b'...'` all G7-deferred | `os.pyrs:56`, `random.pyrs:77`, `io.pyrs:199`, `copy.pyrs:19` |
| Cross-type `+`/`==` are **loose** (pre-existing hole) | `[1,2] + "ab"` and `1 == "x"` **pass `check`**, fail `rustc` (E0277/E0308) — so bytes operators must be **explicitly typed**, not left to the generic path | probe **PN1/PN2** |

**The two load-bearing consequences.** (1) **Migration risk is zero:** every
bytes-ish surface is an honest reject *today* (parse/type/lex error) — W5 is purely
additive, exactly as `global` was for W4. (2) **File is a proto-handle with a real
latent hole** (`g=f`→E0599; un-nameable), so G1 should *generalize File's mechanism
and fix it*, not invent a parallel one — and `bytes`, being a genuine `Vec<u8>` value,
needs *none* of that machinery.

---

## B. Decision 1 — THE `bytes` LOWERING (G7)

**Decision.** Add `Ty::Bytes` → Rust **`Vec<u8>`**. It is a **value**, not a handle:
`Vec<u8>` is `Clone` + non-`Copy`, the same ownership shape as `list` (`Vec<T>`), so
it rides the *existing* clone-on-use pipeline with **no new consuming-site rule**
(rust-probe 1: `snap` stays `b'ABCDE'` while `live` grows). Access shapes, all
byte-offset over the natural `Vec` — **opposite to `str`**:

```
literal   b'\x00abc'         -> vec![0u8, 97, 98, 99]        (or b"...".to_vec())
index     b[i]      -> INT   -> __py_bytes_index(&b, i)  ==  u8 as i64  (neg-norm + IndexError)
slice     b[i:j]    -> BYTES -> b[i..j].to_vec()             (Vec<u8>)
iterate   for x in b         -> for &x in &b { let x = x as i64; ... }   (yields ints)
len/eq/ord/hash/dict-key     -> Vec<u8>'s derives (free)
repr/str/print/f-string      -> __py_bytes_repr(&b)          (byte-identical to CPython)
```

**Literals + the lexer.** Add a `b`/`B` prefix recognizer beside the `f` prefix
(`lexer.rs:408`) and a new `Tok::Bytes(Vec<u8>)` beside `Tok::Str`/`Tok::FStr`
(`lexer.rs:49`). A **byte-valued escape path** (a 4th copy, or a shared helper the four
callers pass a "byte vs char" flag to): the existing `\n\t\r\\\'\"\0\b\f` plus a **new
`\xNN` hex escape** producing a raw `u8` 0–255. Critically, a byte literal escape
yields a **`u8`, not a `char`** — `b'\x80'` is the single byte 0x80, which is **not a
valid UTF-8 scalar** and could never live in the `str` path. (The `\x` escape is added
for `bytes` only in W5; extending it to `str` — where it must be a Unicode scalar — is
a separate lexer item, noted deferred.)

**repr fidelity (byte-identical or honestly rejected).** `__py_bytes_repr(&[u8])`
(rust-probe 1, verified == py-probe 1): quote = `'` by default, `"` iff the bytes
contain `'` and not `"`; escape `\\`→`\\\\`, `\t`/`\n`/`\r`, the active quote; a
printable byte **0x20–0x7e** as itself; **every other byte** (0x00–0x1f, 0x7f–0xff) as
`\xNN` lowercase two-digit hex. (Note 0x7f DEL and 0x20 space: DEL escapes, space is
literal — a bytes-specific range, *not* the lexer's `fmt_byte` 0x21–0x7e used for error
rendering.) `print(b)`, `str(b)`, `f"{b}"`, and `"{}".format(b)` all emit this repr
(py-probe 1 §10) — a `bytes` value is never displayed as raw text.

**`str`↔`bytes` codecs (utf-8 first).** `str.encode(enc='utf-8')` → `s.into_bytes()` /
`s.clone().into_bytes()` (a `String`'s bytes *are* UTF-8). `bytes.decode(enc='utf-8',
errors='strict')`: **`strict`** → `String::from_utf8(b).map_err(…)` → on `Err`, panic
with the catchable `UnicodeDecodeError\0…` payload (the NUL-delimited convention every
runtime error uses); **`replace`** → `String::from_utf8_lossy` (U+FFFD); **`ignore`** →
lossy-then-strip the replacement char (py-probe 6: strict raises, replace→`'��'`,
ignore→`''`). **utf-8 only in W5**; `ascii`/`latin-1`/`utf-16` are a documented
follow-on (each a distinct byte→scalar mapping).

**Method surface, scoped by the unlocks first, then general use.** *Needed by
hashlib/base64/struct* (W5-a/b): `hex()`, `bytes.fromhex(s)`, `decode`, plus
construction `bytes(n)` (n zero bytes), `bytes(list[int])`, `bytes(b)`; and on the
`str` side `str.encode`. *General usefulness* (W5-b, all byte-level, py-probe 9):
`startswith`/`endswith`/`find`/`replace`/`split`/`join`/`strip`/`count`/`upper`/`lower`/
`ljust`/`rjust`. Every one takes/returns `bytes` (or int), never `str` — `b.split(b',')`
not `b.split(',')`.

**bytes vs bytearray — bytes ONLY in W5 (justified).** CPython `bytearray` is the
*mutable* sibling: in-place `+=`, `append`, slice-assign, `del`. That is exactly the
in-place-mutation contract EPIC-4 expresses only via `Mut[T]`/`&mut` (value-semantics
§V2), and it adds a whole second method surface. **bytes (immutable) covers the entire
binary-data unlock family** — hashlib input/output, base64, struct all consume/produce
immutable bytes — so bytearray blocks *no* module and is deferred. (A `bytes` local can
still be *rebound* — `b = b + chunk` — which is value-semantics-clean; only *in-place*
mutation waits.)

**Equality/ordering/hashing.** `Vec<u8>` derives `PartialEq/Eq/PartialOrd/Ord/Hash`, so
`bytes == bytes` (bool), `bytes < bytes` (lexicographic, py-probe 4), `set[bytes]`, and
`dict[bytes]` keys are free (rust-probe 1: `HashMap<Vec<u8>,i64>` works).

**Rationale.** (a) *Zero new ownership machinery* — `bytes` is byte-for-byte a `list`
at the value-semantics layer, so V1 clone-on-use, `Mut[bytes]`, and containers-of-bytes
all compose for free; contrast the handle, which needs a whole new kind. (b) *The index
shape is the one real trap* — `b[i]`→int and `b[i:j]`→bytes differ from `str`, and a
`bytes[i]` that returned a 1-element `bytes` (str-shaped) would be a **silent
miscompile** vs CPython's int; the natural `Vec` index (`v[i] as i64`) gives the right
shape and dodges `str`'s `.chars()` char-offset path entirely (W2 lesson). (c) *repr is
provably byte-exact* (rust-probe 1 output equals py-probe 1 across `\x00`, both quotes,
`\x7f/\x80/\xff`, `\\`, `\t`).

**Rejected alternatives.** • **`bytes` = a wrapper newtype `struct PyBytes(Vec<u8>)`** —
would need hand-written `Clone`/`Hash`/`Ord`/`Index` and break the "rides `list`'s
machinery" win; a bare `Vec<u8>` inherits all of it. Rejected. • **`bytes` = `String`
with a flag** — `String` is UTF-8-validated; a `bytes` holds arbitrary 0xFF bytes that
are not valid UTF-8, so it *cannot* be a `String`. Rejected (unsound). • **Ship
bytearray in W5 too** — doubles the surface and drags in `Mut[bytes]`/in-place mutation
for no additional module unlock. Deferred. • **`bytes[i]`→a 1-byte `bytes`** — the
str-shaped answer; a silent divergence from CPython's int. Rejected.

---

## C. Decision 2 — THE HANDLE MODEL (G1)

**Decision.** Add an opaque kind `Ty::Handle(String)` — a **non-user-constructible,
nameable-in-signatures foreign-struct value**, produced only by `@extern`-backed lib
constructors. **v1 is MOVE-ONLY**, generalizing *and fixing* the File proto-handle.

**Lowering (v1, move-only — rust-probe 2A).** A handle lowers to a bare opaque Rust
struct (exactly like `PyFile`), **non-`Clone`, non-`Copy`**:

```
struct Hasher { state: …, closed: bool }         // the foreign inner state
impl Hasher { fn update(&mut self, b: &[u8]) {…}  // &mut self mutates the receiver
              fn hexdigest(&self) -> String {…}   // &self reads
              fn close(&mut self) { if self.closed { panic!("ValueError\0already closed"); } … } }
impl Drop for Hasher { fn drop(&mut self) { /* release */ } }   // RAII close
```

- **Consuming sites MOVE, never `.clone()`.** `emit_consuming` (`items.rs:1666`) gains a
  handle arm: a handle place emits a **bare move** (the identifier, no `.clone()`), and
  typeck runs a **use-after-move check** — reusing a moved handle is an honest pyrst
  diagnostic ("`h` was moved into `consume(...)`; a handle cannot be reused after it is
  passed or reassigned"). This is the single new rule, and it is what Rust already does;
  typeck merely surfaces it as a pyrst message instead of a raw E0382 — or, today, the
  **broken `f.clone()`→E0599** (PF-A). **This closes the File hole.**
- **Repeated method use is fine.** `pat.match(a); pat.findall(b)` works: a method call
  borrows `&self`/`&mut self`, it does not *consume* the receiver. Only a
  cross-function **pass** or a **reassignment** moves.
- **Nameable in signatures.** `Ty::Handle("Pattern")` resolves from a lib decl (below),
  so `def f(p: Pattern) -> …` typechecks and lowers to `p: Pattern` — closing the
  "expected file, found file" phantom-class gap (PF-B).
- **Lifetime/cleanup.** `Drop` = close (RAII, as `with open()` already does). An explicit
  `close()` sets `self.closed`; a second `close()` (or a use after close) is an honest
  `ValueError`/`ProgrammingError`, and `Drop` after an explicit close is a no-op
  (rust-probe 2A: double-close panics honestly). **Double-close honesty is a `closed`
  flag, not UB.**

**The lib decl form.** A handle type + its methods are declared in `lib/*.pyrs` via a
new **`@extern class`** form (the natural extension of the existing `@extern def` +
`@crate`): the class body holds `@extern`-templated methods (Rust expression/stmt
templates with `{self}`/`{param}` holes) and an `@extern` constructor. The typeck
registers the class name as a `Ty::Handle`; codegen emits the struct + `impl` from the
templates. This reuses the entire `@extern` template + `@crate` dependency machinery
(`items.rs:167`) — the only new codegen is "emit an opaque struct with a `Drop`", which
`FILE_PRELUDE` already prototypes.

**Interaction with W4 globals.** A handle living in a module global (e.g. a
process-wide DB connection) uses the W4 **mutate-in-place** path
(`G.with(|c| c.borrow_mut().method())`), **never the clone-out read path** — a
move-only handle is not `Clone`, so `G.with(|c| c.borrow().clone())` is unavailable by
construction. This is expressible and safe (the borrow ends at the `.with` boundary,
W4 §B), and it is *the correct restriction*: you mutate a global resource in place, you
do not snapshot it. Documented; no W4 change needed.

**Does `re.Match` need G1? NO.** A `re.Match` is populated **eagerly at match time** —
the match fn compiles the regex, runs it, and extracts every group into owned
`str`/`Optional[str]` + `(start, end)` int spans, returning a **pure value struct**.
`group(n)`, `start(n)`, `end(n)`, `groupdict()`, `finditer()` are then pure reads over
that struct — no live `regex::Captures<'h>` (which borrows the haystack) is held across
a call, so no handle, no G7 even (just `str`/`int`). **Only `re.Pattern`** (caching a
compiled `regex::Regex` to stop the per-call recompile, `re.pyrs` header) needs a
handle — and it is the *easiest* case: read-only, all `&self` methods.

**Justification against EPIC-4's no-aliasing rule.** Move-only introduces **no
aliasing** — a handle is simply a non-`Clone` value that moves on consume. It stays
entirely inside EPIC-4's static guarantee: no `Rc`, no `RefCell`, no runtime borrow
panic, no `unsafe`, honoring the value-semantics doc's explicit rejection of
`Rc<RefCell>`/`Mutex`. The move-only restriction *diverges* from Python's reference
semantics (Python lets you pass a file and keep using it), but **honestly** — a compile
error, never silent-wrong — which is the project's iron rule.

**Rejected / deferred alternative — `Rc<RefCell<Inner>>` reference-handle
(rust-probe 2B).** Lower a handle to `Rc<RefCell<Inner>>` (immutable read-only handles
like `re.Pattern` to `Rc<Inner>`). It **is `Clone`** (clone the `Rc`), so it gives
Python-faithful **reference/aliasing** semantics: `g = f` shares, mutation is visible
through every alias, and **refcount-`Drop` closes the resource when the last alias dies**
— exactly CPython's file-close-on-refcount-zero (rust-probe 2B: shared alias sees both
`execute`s; no cross-borrow panic when each method releases its borrow before
returning). It even rides `emit_consuming`'s existing `.clone()` *unchanged*. **The
scoped-exemption argument:** EPIC-4's no-aliasing rule governs **value objects** (user
classes, containers) that must behave independently; a **handle is a reference object**
(an external resource), and Python *already* gives it reference semantics — so
`Rc<RefCell>` is not a violation but the *faithful lowering of a distinct, opt-in,
non-user-constructible kind*, contained because handles are a closed lib-only set the
user cannot construct. **Its cost:** a **runtime borrow panic** if a lib method holds a
`borrow()` across a re-entrant call on the same handle (e.g. `for row in cursor:` that
re-enters `cursor`) — loud and honest (never silent), but a *runtime* not compile error,
and the lib author's discipline to avoid. **Verdict:** DEFER. Move-only suffices for
every non-`sqlite3` unlock (§D); adopt `Rc<RefCell>` as the **v2** only when
`sqlite3`'s connection↔cursor sharing genuinely needs a handle to be aliased as two
values — at which point it is an *additive* second lowering keyed on the lib decl, not a
rework. **This is the one decision the lead may wish to ratify up front (§H).**

---

## D. Decision 3 — THE UNLOCK MAP + oracle strategy

Per target module: the G7/G1 pieces it needs, its CPython-fidelity ceiling, and its
oracle. The headline: **G7 unlocks the whole near-term binary-data family with no G1**;
G1 is reserved for three genuinely-foreign-stateful modules, one of them deferred.

| Module | Needs | Ceiling | Oracle | Notes |
|---|---|---|---|---|
| **base64** / **binascii** | **G7 only** (pure fns over bytes) | 4–5 | **dual-run** (CPython is golden) | `b64/urlsafe/b16/b32 encode/decode` all `bytes→bytes` (py-probe 3); round-trips probed |
| **struct** | **G7 only** (pure fns; a format-string mini-language) | 3–4 | **dual-run** | subset `> < ! =` byteorder × `b B h H i I q Q f d`; `pack→bytes`, `unpack→tuple`, `calcsize` (py-probe 3). Deferred: native `@`-align, `s`/`p` strings |
| **hashlib** / **hmac** | **G7 only** — a **pure value class** | 4–5 | **dual-run** | `sha1/sha256/sha512/md5` as value structs; `update(bytes)` `&mut self`; `.copy()` **free** from clone-on-use; `digest()→bytes`, `hexdigest()→str`. **No G1** — py-probe 2: incremental `update` == oneshot |
| **re.Match** (upgrade) | **neither gate** — a **pure struct** | 3–4 | **dual-run** (vs python3 `re`) | eager group extraction → `group(n)`/`start`/`end`/`groupdict`/`finditer`; can precede W5-a |
| **re.Pattern** | **G1** (read-only handle: cached `regex::Regex`) | 3–4 | **dual-run** | stops the per-call recompile (`re.pyrs` header); all `&self` methods — easiest handle |
| **subprocess** | **G1** (move-only `Popen` = `std::process::Child`) | 2–3 | **python3-reference golden** | env discipline: spawn only deterministic coreutils present in the harness (`true`/`echo`/`cat`); single-owner fits move-only |
| **sqlite3** *(deferred)* | **G1 + `Rc<RefCell>` v2** (connection↔cursor aliasing) | 2–3 | **dual-run** (`:memory:` DB — deterministic, no filesystem) | the aliasing case that funds the v2 reference-handle; the hardest, sequenced last |

**Oracle discipline.** `hashlib`/`base64`/`struct`/`hmac` are pure functions → the same
`.pyrs` dual-runs under `pyrst` and `python3` byte-identically (stdlib-full §G harness),
so **CPython is the golden** — no hand-written expected blocks. `re.Match`/`re.Pattern`
dual-run against python3's `re`. `subprocess` cannot dual-run its *side effects*, so it
gets a python3-reference golden spawning only known-present, deterministic binaries.
`sqlite3` **can** dual-run: an in-memory `:memory:` database is deterministic and needs
no environment, so `python3`'s own `sqlite3` is the oracle — which is *why* it is
testable at all despite being the hardest to build.

---

## E. Decision 4 — SOUNDNESS + MIGRATION (the iron rule)

**Migration is purely additive.** Every bytes surface is an honest reject *today* — `b'…'`
parse error (PT1), `bytes()`/`.encode()` type error (PT2/PT3), `\x` lex error (PT4) — so
no existing valid program changes meaning. W5 in fact **closes two latent holes** rather
than opening any.

**Every new silent-wrong shape, and its kill:**

1. **`bytes[i]` returning bytes (str-shaped) instead of int.** The one real divergence
   trap: CPython `b[i]`→int, `b[i:j]`→bytes (py-probe 2). *Kill:* the codegen `Index`
   arm branches on `Ty::Bytes` to emit `u8 as i64` for a scalar index and `.to_vec()`
   for a slice — the natural `Vec` shape, never the `str` `.chars()` char-offset path
   (`exprs.rs:269` W2 lesson). A golden asserts `b'ABC'[0] == 65` and `type`-shape.
2. **`bytes + str` / `bytes == str` riding the loose generic path.** `[1,2]+"ab"` and
   `1=="x"` pass `check` and fail `rustc` today (PN1/PN2). *Kill:* give `bytes` operators
   **explicit typing** — `bytes+bytes→bytes`, `bytes*int→bytes`, `bytes==bytes→bool` —
   and make `bytes+str` and `bytes==str` **honest `check` errors** (`bytes==str`:
   "`bytes` and `str` are never equal in Python; decode/encode first" — CPython returns
   `False`, a documented divergence pyrst rejects rather than silently answering False).
   Negatives assert both.
3. **`x: bytes` / `bytearray` phantom-class annotation.** `x: bytes` passes `check`
   today as an opaque `Class` → `rustc`-fails (PT6/PT7). *Kill:* `Ty::Bytes` **claims the
   name `bytes`**, so the annotation is now a real type; `bytearray` becomes an **honest
   "deferred" `check` error**, never a silent phantom.
4. **File's `.clone()`-on-non-`Clone` hole.** `g = f` → `f.clone()` → E0599 (PF-A).
   *Kill:* `Ty::Handle`'s consuming sites **move** (never clone) + a use-after-move
   check; File migrates onto the handle kind (or the handle arm covers `Ty::File`),
   turning `g = f` into an honest pyrst move error and making the type nameable.
5. **`decode` strict-mode silently lossy.** CPython `strict` **raises**
   `UnicodeDecodeError` (py-probe 6). *Kill:* `strict` is the default and maps to
   `String::from_utf8(…).map_err → panic("UnicodeDecodeError\0…")` (catchable);
   `replace`/`ignore` are explicit opt-ins. A negative asserts strict-raises.
6. **hashlib clone-on-read hazard (the W4-c random lesson).** A *module-global* hasher
   would advance a discarded clone under clone-on-read. *Kill:* hashlib has **no global**
   — each `sha256()` constructs a fresh local object, and `update` is `&mut self` on that
   local (mutate-in-place, no clone-out). Documented in the module header.
7. **Handle double-close / use-after-close.** `Drop` + explicit `close()` both fire.
   *Kill:* a `closed` flag — double-close and use-after-close are honest
   `ValueError`/`ProgrammingError`; `Drop` after explicit close is a no-op (rust-probe 2A).

**Emit determinism / warnings.** `bytes` adds arms to existing `Ty` matches (`rust_ty`,
`is_copy`, `emit_consuming`, `type_has_default`, method dispatch) — mechanical and
deterministic. `__py_bytes_repr` emits once per program like `FILE_PRELUDE`/`REPR_PRELUDE`
(`mod.rs:1133`), under the crate `#![allow(dead_code)]`. No `unsafe`.

**`PYTHON_COMPATIBILITY.md`** gains a `bytes` capability row + divergences (bytearray
deferred; non-utf8 codecs deferred; `bytes==str` rejected-not-False; handle move-only
reference-divergence; `str.encode`/`bytes.decode` utf-8-first).

---

## F. Staged plan — compiler keystones first, then the lib waves (the W4 pattern)

**bytes BEFORE handles** (bytes is the value keystone: lowest risk, widest family; the
handle is the harder, narrower kind). Each stage is independently gate-green (full
`test_all.sh`, 0-warning, emit-deterministic; each module hits its declared score with
a parity golden). W5-a and W5-g are the two compiler keystones; the lib waves ride them.

### W5-a — `bytes` type keystone · complex-implementer, **L** · risk HIGH
- **Do:** (1) lexer — `b`/`B` prefix beside `f` (`:408`); `Tok::Bytes(Vec<u8>)`; a
  byte-valued escape path with a new `\xNN` hex (the four escape callers share a
  helper). (2) `Ty::Bytes` in `enum Ty` (`types.rs:4`) + `rust_ty`→`Vec<u8>`, `is_copy`
  (false), `Display`("bytes"), all match arms. (3) typeck — `bytes` builtin type +
  **explicit operators** (`+`,`*`,`==`,`<`, index→int, slice→bytes, iterate→int);
  honest `check` errors for `bytes+str`/`bytes==str`; claim the name (kills the phantom).
  (4) codegen — literal, `__py_bytes_index` (neg-norm + IndexError), slice `.to_vec()`,
  `for &x` iterate, `__py_bytes_repr` prelude, print/str/f-string→repr. (5) negatives
  (§E items 1–3).
- **Files:** `lexer.rs`, `parser.rs`, `ast.rs`; `typeck/types.rs`, `typeck/flow.rs`
  (`is_copy`, method dispatch), `typeck/exprs.rs`/`checks.rs` (operators, index shape);
  `codegen/exprs.rs` (index/slice/iterate/repr), `codegen/mod.rs` (prelude),
  `codegen/analysis.rs`; `examples/fail_bytes_*`.
- **AC:** `b'\x00abc'` literal/index(→65)/slice(→bytes)/iterate(→ints)/clone build+run;
  `repr`/`print`/f-string byte-identical to `python3`; `bytes+str` & `bytes==str` are
  honest `check` errors; `x: bytes` no longer a phantom; suite green, 0-warn.

### W5-b — bytes methods + `str`↔`bytes` codecs · implementer, **M** · depends W5-a
- **Do:** `hex`/`fromhex`/`bytes(n|list|bytes)`; `str.encode`/`bytes.decode` with
  `strict`(raise)/`replace`/`ignore` (utf-8); the general method surface
  (`startswith`/`endswith`/`find`/`replace`/`split`/`join`/`strip`/`count`/`upper`/
  `lower`/`ljust`/`rjust`). Negative: strict-mode raises (§E item 5).
- **AC:** each method dual-run-parity vs `python3`; decode error modes match py-probe 6.

### W5-c/d/e — the G7 lib unlocks (parallel-friendly on W5-a/b)
- **W5-c base64 + binascii** · implementer, **M** — pure bytes fns; dual-run.
- **W5-d struct** · implementer, **M** — format mini-language subset; `pack/unpack/calcsize`; dual-run.
- **W5-e hashlib + hmac (pure)** · implementer/complex, **M** — value classes, `update`
  `&mut self`, `.copy()` free, `digest`/`hexdigest`; dual-run (incremental==oneshot).

### W5-f — `re.Match` via eager extraction · implementer, **M** · NO gate
- Pure struct (`group`/`start`/`end`/`groupdict`/`finditer`); needs neither G7 nor G1,
  so it can land **any time** (even before W5-a). Dual-run vs `python3 re`.

### W5-g — G1 move-only handle keystone · complex-implementer, **L** · risk HIGH
- **Do:** `Ty::Handle(String)`; the `@extern class` decl form (typeck registers the name,
  codegen emits opaque struct + `impl` + `Drop` from templates); `emit_consuming` handle
  arm (**move, not clone**) + a typeck **use-after-move** check; `close()` + `closed`
  flag (double-close honesty); **migrate File onto the handle kind** (fix PF-A/PF-B).
- **Files:** `parser.rs`/`ast.rs` (`@extern class`), `typeck/types.rs`/`flow.rs`
  (`Ty::Handle`, move check, nameable), `codegen/items.rs` (`emit_consuming` `:1666`,
  emit handle struct/impl/Drop), `codegen/mod.rs` (retire/absorb `FILE_PRELUDE`);
  `examples/fail_handle_*`.
- **AC:** a handle is nameable in a signature, moves on pass (use-after-move = honest
  error), mutates via `&mut self`, closes via `Drop`; **File's `g=f` is now an honest
  move error, not E0599**; suite green.

### W5-h — `re.Pattern` (read-only handle) + `subprocess` (move-only Popen) · complex-implementer, **L** · depends W5-g
- `re.Pattern` caches `regex::Regex` (stops the recompile); `subprocess` wraps
  `std::process::Child`. Dual-run (`re`) / python3-reference golden (subprocess, safe
  binaries only).

### W5-i — DEFERRED: `sqlite3` + the `Rc<RefCell>` reference-handle v2 (+ compression)
- The aliasing-required tail. Funds the `Rc<RefCell>` v2 lowering (§C, rust-probe 2B);
  `:memory:` dual-run oracle. `zlib`/`gzip`/`bz2`/`lzma` (stream handles) ride the same v2.

**Deferred list (honest).** bytearray; non-utf8 codecs (ascii/latin-1/utf-16);
`str` `\x`/`\u` escapes; the full `struct` surface (native align, `s`/`p`); `memoryview`;
the `Rc<RefCell>` reference-handle + `sqlite3`/compression; `datetime`-via-chrono
(pure `datetime` is already a W2 plan, not W5).

**Total ≈ 9 stages** (2 compiler keystones W5-a/W5-g; 6 lib waves; 1 deferred tail).
W5-a and W5-g are the funded epics; the rest are the surfaces they unlock.

---

## G. Probe appendix — validated patterns (verbatim)

**Rust lowering (2 compiling `rustc 1.95 --edition 2021` probes; not committed,
`scratchpad/w5probes/`).**

- **rust1 (`bytes` = `Vec<u8>` — the whole lowering).** literal `b"\x00abc".to_vec()`;
  `__py_bytes_index` (neg-norm+IndexError); slice `.to_vec()`; iterate `for &e`;
  clone-on-use (`snap` vs `live`); `hex`; `__py_bytes_repr`; `HashMap<Vec<u8>,_>` key.
  **COMPILED + ran** — `repr=b'\x00abc' / b[0]=65 / b[-1]=69 / b[1:3]=b'BC' / sum=335 /
  snap=b'ABCDE' live=b'ABCDEZ' / hex=00616263 / dict[k]=1`, and the repr edge set
  `b"a'b" · b'a"b' · b'a\'b"c' · b'\x00A\x7f\x80\xff' · b'\\' · b'tab\there'` —
  **byte-identical to py-probe 1**.
- **rust2 (both handle shapes).** **2A move-only:** `HasherA` `&mut self update`
  (incremental==oneshot=`true`), move-into-`consume()`, double-close panics (`true`),
  `Drop`. **2B `Rc<RefCell>`:** `Conn(Rc<RefCell<ConnInner>>)` `#[derive(Clone)]`,
  `&self execute` via `borrow_mut`, shared alias count=2 (both see mutations), no
  cross-borrow panic, refcount-`Drop` close. **BOTH COMPILED + ran.**

**CPython `bytes` oracle (`python3 3.12.9`).** **(1) repr/escaping:** `b''`, `b'\x00abc'`,
`b"a'b"` (has `'` not `"`→`"`), `b'a\'b"c'` (both→`'`+escape), `b'\x00A\x7f\x80\xff'`
(0x7f/0x80/0xff escape, 0x20–0x7e literal), `b'\\'`, `b'tab\there'`. **(2) shapes:**
`b[0]=65` int, `b[-1]=69` int, `b[1:3]=b'BC'` bytes; `for x in b'AB'`→`[65,66]`.
**(3) eq/ord/hash:** `b'abc'==b'abc'` T, `b'abc'<b'abd'` T, `b'abc'=='abc'` **False**
(not error), `dict[b'k']` ok. **(4) codec:** `'héllo'.encode('utf-8')=b'h\xc3\xa9llo'`;
round-trips; default utf-8. **(5) decode errors:** strict→`UnicodeDecodeError`,
replace→`'��'`, ignore→`''`. **(6) `b'a'+'b'`→`TypeError` "can't concat str to
bytes".** **(7) hex:** `b'\x00\xff\x10'.hex()='00ff10'`, `bytes.fromhex(...)` round-trips.
**(8) interp:** `f"{b'\x00ab'}"='val=b'\x00ab''`, `str(b)`/`.format(b)` = repr.
**(9) ctors:** `bytes(3)=b'\x00\x00\x00'`, `bytes([65,66,67])=b'ABC'`.

**CPython unlock oracle.** **hashlib:** `sha256` incremental (`update(b'hello ')` +
`update(b'world')`) `hexdigest` == oneshot `sha256(b'hello world')` == `b94d27b9…`;
`.copy()` forks state; `.digest()`→bytes. md5/sha1/sha512 standard. **base64:**
`b64encode(b'\x00\x01\x02hello\xff')=b'AAECaGVsbG//'`, decode round-trips; urlsafe/b16/b32;
input & output **bytes**. **struct:** `pack('>I',1234)=b'\x00\x00\x04\xd2'` bytes,
`unpack('>I',…)=(1234,)`, `calcsize('>I')=4`, `pack('<I',1234)=b'\xd2\x04\x00\x00'`,
`pack('>f',1.5)=b'?\xc0\x00\x00'`.

**pyrst-today oracle (`pyrst check`/`emit`/`build`).** **PT1/PT8** `b"…"`/`b'…'` → parse
error (honest). **PT2** `bytes()` → "undefined function". **PT3** `.encode()` → "type
`str` has no method". **PT4** `"\x41"` → "unknown escape '\x'". **PT6/PT7** `x: bytes`,
`x: Foo` → **`check` passes** (phantom-class hole). **PF-A** `g = f` (File) →
`let mut g = f.clone();` → **`rustc` E0599** "no method `clone` … for `PyFile`" (`check`
passed — the latent File hole). **PF-B** `use(f)` File-arg → "expected file, found file"
(phantom-class param). **PF-C** `p: File` → `check` passes as a phantom class. **PN1/PN2**
`[1,2]+"ab"`, `1=="x"` → `check` passes, `rustc` E0277/E0308 (loose cross-type ops — so
bytes operators must be explicitly typed).

---

## Relevant files

**This design:** `docs/design/w5-bytes-handles.md` (this file). **Builds on:**
`docs/design/value-semantics.md` (clone-on-use, `Mut[T]`, the anti-`Rc<RefCell>`
stance), `docs/design/w4-globals.md` (`thread_local` RefCell, the clone-on-read hazard,
the keystone-first staging), `docs/design/stdlib-full.md` §E/§F (the G1/G7 verdicts +
W5 card sketch). **Style precedent:** `w4-globals.md`, `w3-modules.md`.

**Compiler surfaces W5-a touches:** `src/lexer.rs` (`f`-prefix `:408`, escape tables
`:469`/`:545`/`:592`, `Tok::Str`/`FStr` `:49`); `src/parser.rs` + `src/ast.rs`
(`Tok::Bytes`); `src/typeck/types.rs` (`enum Ty` `:4`, `open` `:529`, `rust_ty`,
`Display`), `src/typeck/flow.rs` (`is_copy` `:1034`, method dispatch `:3686`/`:3779`),
`src/typeck/exprs.rs`/`checks.rs` (bytes operators, index/slice shape);
`src/codegen/exprs.rs` (index/slice/iterate/repr; `rust_ty` File→`PyFile` `:3931`,
str char-index `:269`), `src/codegen/mod.rs` (`__py_bytes_repr` beside `FILE_PRELUDE`
`:780`, prelude emit `:1133`), `src/codegen/analysis.rs` (`is_copy_type` `:246`).

**Compiler surfaces W5-g touches:** `src/typeck/types.rs` (`Ty::Handle`),
`src/typeck/flow.rs` (nameable, use-after-move), `src/codegen/items.rs`
(`emit_consuming` `:1666` handle arm, `@extern` lowering `:167`, emit handle
struct/impl/Drop), `src/codegen/mod.rs` (absorb `FILE_PRELUDE`).

**Stdlib surfaces (lib waves):** `lib/base64.pyrs`, `lib/binascii.pyrs`,
`lib/struct.pyrs`, `lib/hashlib.pyrs`, `lib/hmac.pyrs` (new); `lib/re.pyrs` (Match/Pattern
upgrade); `lib/subprocess.pyrs`, `lib/sqlite3.pyrs` (deferred); `src/stdlib.rs`
(`EMBEDDED_STDLIB` register `:25`); `examples/parity_*` + `expected/`,
`PYTHON_COMPATIBILITY.md`; `lib/os.pyrs` (`urandom`→bytes), `lib/random.pyrs`
(`randbytes`) fidelity follow-ons.

**Empirical probes (scratchpad, not committed):**
`scratchpad/w5probes/` (`rust1_bytes.rs`, `rust2_handle.rs`; `oracle_bytes.py`,
`oracle_unlocks.py`; `pt1`–`pt8`, `pf a`–`c`, `pn1`–`pn4` pyrst).
