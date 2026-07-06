# W3 — Dotted Submodules & Per-Module Namespacing (G3)

**Design card:** 0803c0ca. **Status:** design only, no source modified; every
capability claim is source-anchored or empirically probed against the release
binary. **Date:** 2026-07-05. **Baseline:** HEAD `5f2d84c` (Phase 2 ergonomics),
clean tree. **Precedent style:** `docs/design/lazy-generators.md`,
`docs/design/exception-lowering.md` (BLUF + source-anchored tables + validated
prototype + staged plan). **Context:** `docs/design/stdlib-full.md` §E (G3
verdict: BUILD, also a bug fix) + §F W3.

## Bottom line

W3 turns pyrst's flat module namespace into a real one, and kills a latent
miscompile on the way. Two payoffs, one keystone decision.

1. **Kill the silent truncation (a bug fix).** `import os.path` today is accepted
   as if you wrote `import os` — the resolver takes `path[0]` and drops the rest
   (`src/resolver.rs:126`). Empirically (probe P3b) `import os.path` followed by
   `os.getcwd()` reports **"ok: 2 module(s) typecheck"** with no error — the
   `.path` is silently ignored. `from os.path import join` (P3c) silently binds
   `os`'s flattened `join`. This violates pyrst's honest-errors invariant. W3
   resolves the **full** dotted path and errors honestly when it cannot.

2. **Dissolve the 8 co-import collisions via per-module namespacing.** pyrst's
   emitted crate is flat: every module's top-level `def`/`class`/const merges
   under its bare name, so `operator.sub` and `re.sub` cannot coexist. Card
   6c8b4a39 made that an honest `check` error (probe P1 confirms it fires) as a
   stopgap. W3 emits each imported module's top-level names into their own
   **owner-qualified namespace** so all 8 documented pairs co-import, and retires
   the stopgap.

**The keystone decision — emission strategy — is NAME MANGLING, not Rust `mod`
blocks.** Every imported module's top-level name emits as an owner-qualified flat
identifier `__pyrst_m_<module>__<name>` at crate root; **the root/user file's own
names stay crate-root-unwrapped for v1**. This keeps the *entire* emitted crate
flat — the prelude, the `__py_*` helpers, `PyRepr`, `__PyrstGen`, `__PyrstTryFlow`,
the EPIC-5 companion enums, and every `@extern` template stay exactly where they
are today — so the blast radius is a mechanical "thread the owner module to each
top-level name-emission site," not a structural rewrite. Two compiling Rust
probes (Appendix, §G) show mangling and `mod` blocks are *both* semantically
viable and produce identical output; mangling wins purely on blast radius and on
the cross-module companion-enum interaction (§F). Because the golden harness is
**output-based** (`test_all.sh` builds, runs, and diffs stdout — it never inspects
emitted Rust) and **507 of 603 corpus files import nothing**, leaving root names
unwrapped makes those 507 goldens **byte-identical** in emitted Rust and the
remaining 96 **output-identical**.

Five staged, independently gate-green cards (§H): (1) per-module symbol tables +
qualified resolution, flat emission unchanged; (2) namespaced emission + stopgap
retirement; (3) dotted-import resolution + embedded packages; (4) faithful
`os.path`/`urllib.parse`/`collections.abc`; (5) docs + negative corpus. The
single riskiest interaction — a companion enum whose subclass lives in a
*different* module than its base — is validated end-to-end in §F.

---

## A. What happens today — the baseline the design must preserve

Source-anchored, with real-compiler probes (`scratchpad/`, not committed).

| # | Behavior today | Evidence |
|---|---|---|
| Import parse | `Stmt::Import { path: Vec<String>, names: Vec<(String, Option<String>)> }` keeps the **full** dotted path; `import a.b` → `path=["a","b"]`; `from a.b import f` → `path=["a","b"], names=[("f", None)]` | `ast.rs:189`; `parser.rs:808` (`parse_import`), `:828` (`parse_from_import`) |
| Aliases | `import x as y` **and** `from x import y as z` are honest parse errors ("import aliases (`as`) are not yet supported") | `parser.rs:816`, `:839`, `:855` |
| **Silent truncation** | resolver uses `path[0]` only, drops the rest; `import os.path` compiles as `import os` | `resolver.rs:126` `let mod_name = &path[0];` |
| P3b (silent) | `import os.path` + `os.getcwd()` → **"ok: 2 module(s) typecheck"** — no error, `.path` ignored | probe |
| P3c (silent) | `from os.path import join` → **"ok"**; `join` silently binds `os.join` | probe |
| P3 (masked) | `import os.path` + `os.path.join(...)` → honest "module `os` has no attribute `path`" (only because `os.path` is a non-existent attr access, not because the import was checked) | probe |
| Collision stopgap | co-import `operator` + `re` → check error "name `sub` is provided by both `operator` and `re` … a program cannot import both" | `resolver.rs:385` `detect_cross_module_collisions`; probe P1 |
| Qualified call | `X.f(args)` where `X ∈ module_funcs` and `f ∈ its list` lowers to the **flat** bare call `f(args)` | `exprs.rs:210-219`; typeck `exprs.rs:204`, `:694` |
| Qualified const | `X.CONST` lowers to the flat mangled `const` `__pyrst_const_CONST` | `exprs.rs:2852`; `mangle_const` `mod.rs:258` |
| Flat merge | every module's `def`/`class`/annotated-literal-const merges into one `ctx.funcs`/`ctx.classes`/`ctx.vars`; `module_funcs`/`module_consts` are module→names indices used only to *resolve* qualified access, not to disambiguate emission | `resolver.rs:202` `merge_ctx_from_module`; `types.rs:273`, `:288` |
| **Cross-module polymorphism WORKS** | root `class Square(Shape)` subclassing an imported `Shape`, used as `s: Shape = Square(...)`, dispatches `s.area()`→16 and reads `s.x`→1; emits `enum Shape__ { Shape(Shape), Square(Square) }` at crate root | probe P2 (build+run) |
| Skip-list | only `dataclasses` is hard-skipped (no module body, decorator-only); `sys`/`json`/`itertools`/`collections` are all real embedded modules | `resolver.rs:149` |
| Embedded stdlib | 41 modules, `EMBEDDED_STDLIB: &[(&str, &str)]` keyed by bare name, each `include_str!("../lib/<name>.pyrs")`; `lookup(name)` linear-scans | `stdlib.rs:25`, `:249` |
| Harness | build → run → **stdout diff** vs `expected/*.txt`; parity_* dual-run vs `python3`. **Never inspects emitted Rust.** | `test_all.sh` §1, §4c |
| Corpus | 603 `.pyrs`: 403 positive goldens, 200 `fail_*`, 79 `parity_*`; **only 96 use `import`** | `examples/` |

**The load-bearing consequence of an output-based harness:** the emitted Rust
identifier for an imported module's function is invisible to every golden. A
program that co-imports `operator`+`re` and prints the right numbers passes
regardless of whether `re.sub` emits as `sub`, `__pyrst_m_re__sub`, or
`re::sub`. Mangling is therefore a *behavior-preserving* transform of the emitted
crate, and the 507 import-free goldens don't change at all when root names stay
crate-root.

---

## B. Decision 1 — Package model (how `import a.b` resolves)

**Decision.** Ship three import forms in v1 — `import a.b`, `from a.b import f`,
and qualified `a.b.f()` calls. Resolve the **full** dotted path against a
**directory layout** (local `a/b.pyrs`; embedded `lib/a/b.pyrs`), keyed by the
**dotted module id** `"a.b"`. `import a` alone does **not** expose `a.b`
(explicit-import-required). Aliases stay rejected. Kill `path[0]`: an unresolved
dotted import is an honest `ImportNotFound`.

**Rationale.**

- **Directory layout over flat-filename.** CPython packages *are* directories
  (`urllib/parse.py`, `collections/abc.py`, `xml/etree/ElementTree.py`), and a
  local user package is far more naturally `a/b.pyrs` than a file literally named
  `a.b.pyrs`. The resolver already computes a local candidate as
  `base_dir.join(format!("{}.pyrs", mod_name))` (`resolver.rs:155`); the dotted
  form is the same join with the path components as segments:
  `base_dir.join("a").join("b.pyrs")`. Embedded stdlib mirrors it:
  `EMBEDDED_STDLIB` gains dotted keys (`"os.path"` → `include_str!("../lib/os/path.pyrs")`).
  `os` stays `lib/os.pyrs` (a plain module) **and** gains a sibling package dir
  `lib/os/` for `os.path` — the two coexist on disk, matching CPython where `os`
  is a module and `os.path` is a distinct module it re-exports.
- **`include_str!` embedding is unchanged in shape** — still a static
  `&[(&str, &str)]`, just with dotted keys and nested `lib/` paths. `lookup` still
  linear-scans (41→~44 entries; not hot).
- **Explicit-import-required (`import os` does NOT expose `os.path`).** CPython's
  `import os` exposes `os.path` only because `os.py` does `import posixpath as
  path` and binds it as a module *attribute* — which needs a module-object-as-value
  the pyrst type system does not have. The honest v1 is: `os.path` requires
  `import os.path` (or `from os.path import join`). Documented divergence; it is
  also good practice. `import a.b` binds `a.b` for qualified `a.b.f()`; whether the
  parent `a` is *also* loaded is a v1 detail — resolve the leaf `a.b` and, when
  the same file also references `a.*`, its own `import a` line loads `a` (CPython
  loads the whole chain; v1 resolves each dotted import it is given).
- **Kill `path[0]`.** Replace `let mod_name = &path[0]` with resolution of the
  full dotted id; if neither a local `a/b.pyrs` nor an embedded `"a.b"` exists,
  return `ImportNotFound { path: "a.b", … }`. This closes P3b/P3c — the silent
  truncation becomes an honest error, exactly the §E G3 "also a bug fix" claim.

**Rejected.** *Flat-filename `os.path.pyrs`* (no CPython parallel; awkward for
local packages). *Auto-expose `os.path` on `import os`* (needs module-as-value;
deferred, documented). *Import aliases* (out of scope; already rejected — keep
rejecting, do not silently ignore).

**Module id.** Every module gets a canonical **dotted id** (the import path that
reached it: `"os"`, `"os.path"`, `"a.b"`; root = the sentinel *root*). This id —
not the file stem — is the key for both qualified resolution and mangling, because
`lib/os/path.pyrs`'s file stem is the ambiguous `"path"`. Store it on `Module`
(a `module_id: Option<String>`, or a dotted-name field beside `source_path`).

---

## C. Decision 2 — Namespaced emission: NAME MANGLING (the keystone)

**Decision.** Emit every **imported** module's top-level `def`/`class`/const as an
owner-qualified flat identifier at crate root:

```
fn   __pyrst_m_re__sub(...)              // re.sub
struct __pyrst_m_re__Match { ... }       // re.Match
const __pyrst_const_sys__version: ...    // sys.version  (const mangle gains owner)
enum __pyrst_m_shapes__Shape__ { ... }   // companion enum, owner-qualified
```

Call/type sites emit the same mangled name (`__pyrst_m_re__sub(...)`,
`rust_ty(Ty::Class("Match")) → __pyrst_m_re__Match`). **The root/user file's own
top-level names stay crate-root-unwrapped** (`user_main`, `escape_ident(name)`,
plain `Square`) exactly as today. **Methods, locals, params, and fields are never
mangled** — methods are namespaced by their receiver struct; locals/fields never
collide across modules.

The emitted Rust name is a pure function of two statically-known inputs:

```
emit_name(owner, name) = if owner == ROOT { escape_ident(name) }
                         else { format!("__pyrst_m_{}__{}", owner.replace('.', "_"), name) }
```

The entire design reduces to **"at every top-level name-emission site, know the
owner module."** That is always statically known: a **definition** site knows its
own module (thread the current emit module into `Codegen`, like `current_class`);
a **use** site knows the owner because qualified `a.b.f()` names it, a from-import
`f()` resolves via the importing file's import-binding map (§D), and a bare
same-module `f()` uses the current module.

### C.1 Why mangling, not `mod` blocks — the blast-radius argument

Both patterns are legal Rust and produce identical program output (§G probes A and
B both compile and run). The difference is entirely blast radius and friction.

| Concern | Name mangling (chosen) | `mod` blocks (rejected) |
|---|---|---|
| Crate-root prelude (`FLOAT_FMT_HELPER`, `__py_mod`, `__py_str_find`, `STR_REPR_HELPER`, `REPR_PRELUDE`/`PyRepr`, `TITLECASE`, `FILE_PRELUDE`, `GEN_PRELUDE`, `__PyrstTryFlow`; `mod.rs:842-1018`) | **Untouched.** All refs stay crate-root, unqualified. | Every reference from *inside* a module body needs `crate::` — and these helpers are hit pervasively (every `%`→`__py_mod`, every float print, every `repr`, every generator, every `try`). |
| `impl PyRepr for <struct>` on a module's class | Crate-root impl, `impl PyRepr for __pyrst_m_re__Match` — exactly today's shape. | Legal, but must write `impl crate::PyRepr for Match` inside the mod (probe B verified it compiles). |
| Companion enum whose base+subclass span modules (§F) | Flat `enum __pyrst_m_x__B__ { __pyrst_m_x__B(...), Square(Square) }` — identifiers only, no paths. | Enum must `super::`/`crate::`-path each foreign variant payload, every match arm, and every field accessor. The hard case. |
| Cross-module type reference (`rust_ty` Class arm, `exprs.rs:3694`) | Owner lookup → one mangled identifier. | Owner lookup → a `crate::__pyrst_m_x::Name` **path**. |
| `@extern` templates (`items.rs:165-175` hole-substitution) | Emitted at crate root, unchanged. | A template referencing a crate item would need path rewriting inside the mod. |
| Emit determinism | Identical to today: same topological module order, same sorted companion-enum order; names just gain a prefix. | Same order, but nested-mod emission + glob `use` risk ordering/ambiguity surprises. |
| Generated-code readability | Ugly identifiers (irrelevant — an artifact; goldens are output-based). | Cleaner. The only genuine `mod`-blocks win. |
| DCE (`dead_funcs`, `mod.rs:790`; `analyze_called_functions`, `checks.rs:1540`) | Owner-key the maps (bare→`(owner,name)`); suppression stays a name test. | `#[allow(dead_code)]` per mod is natural, but suppression must resolve mod paths. |

pyrst does not need Rust's module privacy to enforce encapsulation (typeck already
governs visibility) and does not read the emitted crate (it is an artifact). So
`mod` blocks buy readability pyrst doesn't spend, at the cost of `crate::`/`super::`
path-rewriting on the prelude, companion enums, cross-module types, and `@extern`
templates. Mangling keeps the emitted crate byte-for-byte flat except top-level
names gain a prefix. **Chosen: mangling.**

### C.2 The load-bearing emission sites (the migration surface)

Every site that emits a **top-level** name; each becomes `emit_name(owner, name)`:

| Site | File:line | Change |
|---|---|---|
| Free-function definition name | `items.rs:6-18` (`emit_func`) | root main→`user_main`, root fn→`escape_ident`; imported fn→`emit_name(owner, name)`. **Methods unchanged** (`f.name.clone()`, `:17`). |
| Class/struct name + all its `impl`/derive blocks | `items.rs` (`emit_class`) | struct name, `impl <Name>`, `impl Display/PartialEq/PartialOrd/Add for <Name>`, `impl PyRepr for <Name>` → `emit_name(owner, name)`. |
| Class-type reference (the chokepoint) | `exprs.rs:3694-3718` (`rust_ty` `Ty::Class` arm) | `n` → `emit_name(class_owner[n], n)`; polymorphic-base `n__` → `emit_name(...) + "__"`; generic `n<...>` likewise. |
| Companion enum + dispatch + field accessors | `items.rs:1152` (`emit_companion_enum`) | enum name, each variant tag + payload struct, dispatch/accessor match arms → mangle each by *its own* owner. |
| Qualified call `X.f()` / `a.b.f()` | `exprs.rs:210-219` | flat `f` → `emit_name(owner=X | "a.b", f)`. Extend the `Expr::Ident(modname)` match to also accept `Expr::Attr{Ident(a), b}` (dotted). |
| From-imported bare call `f()` | `stmts.rs:1766` (`emit_plain_func_call`) | when `f` is a from-import binding, `emit_name(binding_owner, f)`. |
| Const definition + refs | `mod.rs:1027-1042` prepass; `mangle_const` `mod.rs:258`; refs `exprs.rs:2852` and bare-`CONST` Ident arm | `mangle_const(owner, name) = __pyrst_const_{owner}__{name}` (owner-qualify — currently owner-blind, a latent const-vs-const collision). |
| DCE keys | `mod.rs:790` (`dead_funcs`), `checks.rs:1540` (`analyze_called_functions`) | key by `(owner, name)` so a name live in one module and dead in another suppresses correctly. |

Untouched: `build_poly_map` (`analysis.rs:2242`) keys by bare class name and stays
correct **because class names remain globally unique** (§D); it only feeds
`is_polymorphic_base`, which the `rust_ty` arm already consults.

---

## D. Decision 3 — Name-resolution layering

**Decision.** Replace the "flat merge, then a stopgap collision check" with
**owner-aware resolution**:

- **Per-module symbol tables.** Alongside the merged view, carry each imported
  module's own top-level `def`/`class`/const signatures keyed by `(module_id,
  name)` (or a `HashMap<ModuleId, ModuleSymbols>`). Colliding names (`operator.sub`
  *and* `re.sub`, with *different* signatures) coexist here; the flat `ctx.funcs`
  cannot hold both, which is exactly why the stopgap exists.
- **Qualified `X.f()` / `a.b.f()`** resolve `f` against module `X`'s table
  specifically (not the flat table). `module_funcs`/`module_consts` already tell us
  `f` belongs to `X`; the *signature* now comes from `X`'s table.
- **`from X import f`** binds `(local f → owner X)` in the **importing file's**
  scope. A bare `f()` in that file resolves owner-first via this binding.
- **Bare name in the defining module** resolves to that module's own table.
- **Root shadows imports** (unchanged): a root `def f` wins over any imported `f`
  (`resolver.rs:679` test; the root is last in topological order).
- **Owner maps for emission:** `func_owner`, `class_owner`, `const_owner`
  (`name`/`(local,file)` → `ModuleId`), built by the resolver, consumed by
  `emit_name`.

**`detect_cross_module_collisions` — retired for fn/const, KEPT (narrowed) for
class-vs-class.** `Ty::Class(String, Vec<Ty>)` carries only a **bare** name
(`ast`/`types.rs`), and `rust_ty`/`build_poly_map`/`emit_companion_enum` all key
class identity by that bare name. Two co-imported modules each defining `class
Foo` would be indistinguishable (`rust_ty(Ty::Class("Foo"))` can't pick an owner).
So v1 keeps **class-name global uniqueness** an honest error, while fn-vs-fn,
const-vs-const, and fn-vs-const collisions become co-importable. **None of the 8
documented pairs is class-vs-class** — the `time` pair is a *class* `datetime.time`
vs a *function* `time.time`, distinct Rust item kinds that mangle to distinct
symbols.

**(W3-fix correction.)** "Distinct symbols" is necessary but was **not
sufficient**: emission mangling alone did **not** make the `datetime` + `time`
co-import work. `time.pyrs` calls its OWN bare `time()` internally, and a
flat/class-first *resolution* bound that call to `datetime`'s CLASS `time`,
breaking `check` *inside* `<stdlib>/time.pyrs`. The fix is **owner-first
resolution**: a module's own top-level function shadows a foreign same-named
class both in typeck (`with_module_symbols_promoted` drops the foreign class from
the module's checking view) and in codegen (`emit_constructor_call` yields to a
same-scope own function). With that, the co-import **checks, builds, and runs**;
qualified `time.time()` / `time.time_ns()` work in both import orders (see
`examples/coimport_datetime_time.pyrs`). The **residual** is orthogonal:
qualified module-**CLASS** construction (`datetime.time(9, 30)`) is still
unsupported — the qualified-call path resolves `module_funcs` only, not classes,
the same gap as `fractions.Fraction(...)` — so it is a **v2 deferral**, not a
"loses nothing" claim. Threading an owner into `Ty::Class` (true same-named-class
co-import) remains the documented v2 extension.

**From-import local-name conflicts — HONEST ERROR (fidelity decision).** If a
single file does `from datetime import time` **and** `from time import time`, the
local name `time` has two owners. CPython silently rebinds (last import wins).
pyrst keeps its honest-errors-over-silent-shadowing invariant: this is a
check-time error, with the workaround `import datetime; import time` +
`datetime.time(...)` / `time.time()` (both qualified, unambiguous). A static,
closed-type language cannot reproduce CPython's dynamic last-wins rebinding
cleanly, and silently picking one would be exactly the miscompile class W3 exists
to kill. Documented divergence.

**Skip-list.** `dataclasses` (`resolver.rs:149`) is orthogonal — it is
decorator-only with no module body; it stays skipped and is unaffected.

**LSP / `analysis.rs` single-file path.** `analysis.rs:77`/`:104` call
`merge_ctx_from_module(&module, &mut ctx, /*is_root=*/true)` on a single file and
never emit code. With `is_root=true` the module is the root (unwrapped owner) and
there are no cross-module tables to build — the path stays a pure passthrough. The
existing editor gap (qualified calls don't resolve single-file because
`module_funcs` is empty there) is unchanged, not worsened. Keep the `is_root=true`
signature working exactly as-is.

---

## E. Decision 4 — Compatibility & migration

**Decision.** Output-identical corpus, co-importable pairs, `os.path` re-homed
with **kept aliases**, stopgap flipped to negatives, docs updated.

- **All 403 positive goldens stay OUTPUT-identical.** The harness is output-based
  (§A), and root names stay crate-root-unwrapped, so the **507 import-free files
  are byte-identical in emitted Rust** and the **96 import-using files are
  output-identical** (only their imported-module identifiers gain a prefix,
  invisible to stdout). Emit determinism holds (same order, prefixed names). This
  is the behavior-identical bar the epic mandates.
- **The 8 collision pairs become co-importable** under mangling — every pair
  resolves because both sides gain distinct owner prefixes:

  | Name | A | B | After mangling |
  |---|---|---|---|
  | `sub` | operator (fn) | re (fn) | `__pyrst_m_operator__sub` / `__pyrst_m_re__sub` |
  | `copy` | copy (fn) | shutil (fn) | owner-prefixed fns |
  | `escape` | html (fn) | re (fn) | owner-prefixed fns |
  | `join` | os (fn) | shlex (fn) | owner-prefixed fns (+ os.join re-homes to os.path) |
  | `split` | re (fn) | shlex (fn) | owner-prefixed fns |
  | `time` | datetime (**class**) | time (fn) | `struct __pyrst_m_datetime__time` / `fn __pyrst_m_time__time` |
  | `platform` | platform (fn) | sys (**const**) | `fn __pyrst_m_platform__platform` / `const __pyrst_const_sys__platform` |
  | `version` | platform (fn) | sys (**const**) | same shape as `platform` |

  New goldens: one co-import golden per pair (e.g. `examples/coimport_operator_re.pyrs`
  asserting both `operator.sub(5,3)`==2 and `re.sub(...)`), replacing the negatives.
- **`fail_stdlib_name_collision` retargets.** The stopgap negatives that assert
  "cannot import both" now *succeed*; flip them into positive co-import goldens. Any
  remaining collision negative retargets to the **class-vs-class** case still kept
  as an error (§D) so the honest-error path stays covered.
- **`os.path` re-homing — KEEP flat `os` aliases for one release + add faithful
  `os.path`.** `lib/os.pyrs` currently carries `join`/`dirname`/`basename`/`isfile`/
  `isdir`/`sep` (`os.pyrs:80,111,116,144,150,176`). Hard-moving them breaks the 9
  os-importing goldens. Instead: add `lib/os/path.pyrs` with faithful
  `os.path.join`/`dirname`/`basename`/`split`/`splitext`/`isabs`/… and **keep the
  `os.*` aliases** (deprecated, header-noted) for one release. Under mangling the
  alias is free of collision cost (`os.join`→`__pyrst_m_os__join`,
  `shlex.join`→`__pyrst_m_shlex__join` coexist), so keeping it reintroduces no
  flat collision. A later cleanup card removes the aliases. Rationale: minimizes
  parity-test blast radius (no golden churn) while delivering faithful `os.path`.
- **PYTHON_COMPATIBILITY.md.** Retire the "Flat-namespace co-import restriction
  (card 6c8b4a39)" section and its 8-pair table (`PYTHON_COMPATIBILITY.md:358-371`);
  replace with the dotted-import + per-module-namespacing capability, the
  explicit-`import os.path`-required divergence, the from-import-local-conflict
  divergence, and the class-vs-class v2 note. Update the `os` row (`:378`) and add
  `os.path`/`urllib.parse`/`collections.abc` rows.

---

## F. The single riskiest interaction — cross-module companion enum

**The risk.** EPIC-5 closed-set polymorphism lowers a polymorphic base `B` to a
companion enum `B__ { B(B), Sub(Sub), … }` with a method-dispatch impl and
per-field accessors (`items.rs:1152`), built from a global `poly_map`
(`analysis.rs:2242`) over **all** classes. When a subclass lives in a *different
module* than its base, the enum must name types owned by two different namespaces.
Under `mod` blocks this needs `super::`/`crate::` paths on every variant payload,
match arm, and accessor — the exact friction §C flags.

**It is a real, exercised scenario, and it works today.** Probe P2: a root
`class Square(Shape)` subclassing an imported `Shape` (from `shapes.pyrs`), used as
`s: Shape = Square(1, 4)`. It **builds and runs** (`s.area()`→16, `s.x`→1). The
emitted crate (via `pyrst emit`) is:

```rust
struct Shape { x: i64 }                       // owned by module `shapes`
struct Square { x: i64, side: i64 }           // owned by ROOT
enum Shape__ { Shape(Shape), Square(Square) } // crate-root, mixes both owners
impl Shape__ {
    fn area(&self) -> i64 { match self { Shape__::Shape(x)=>x.area(), Shape__::Square(x)=>x.area() } }
    fn __field_x(&self) -> i64 { match self { Shape__::Shape(x)=>x.x, Shape__::Square(x)=>x.x } }
}
fn user_main() { let mut s: Shape__ = Shape__::Square(Square::new(1i64, 4i64)); /* … */ }
```

**Under mangling (root unwrapped) this is exactly probe A**, which compiles and
runs identically:

```rust
struct __pyrst_m_shapes__Shape { x: i64 }     // base: owner `shapes` → mangled
struct Square { x: i64, side: i64 }           // subclass: ROOT → unwrapped
enum __pyrst_m_shapes__Shape__ {              // enum owned by the base's module
    __pyrst_m_shapes__Shape(__pyrst_m_shapes__Shape),
    Square(Square),
}
// dispatch + __field_x match arms: all flat identifiers, NO paths.
```

Because everything is a crate-root identifier, a base in one owner and a subclass
in another coexist in one flat enum with **zero path juggling**. Probe A ran:
`area=0 x=1` / `area=16 x=2`. The companion enum's home is the **base's** module
(where `poly_map` keys it); each variant tag + payload is mangled by *its own*
owner (base by `shapes`, `Square` by root). The generic-fn-called-cross-module
interaction (the task's other candidate) is also validated in probe A
(`__pyrst_m_a__ident::<i64>(3)` called from `__pyrst_m_b__make`) and already works
today via generic stdlib modules (`heapq.heappush`, `bisect.insort`).

**Conclusion:** the riskiest interaction is *harder* under the rejected `mod`-block
scheme (probe B needs `Shape__::Shape(__pyrst_m_shapes::Shape)` paths) and
*trivial* under the chosen mangling scheme. This is the decisive tie-breaker.

---

## G. Probe appendix — validated Rust patterns

Four required scenarios, validated by two self-contained Rust files that **compile
and run** (`rustc 1.95, --edition 2021`; `scratchpad/probe_*.rs`, not committed).
Both emit identical program output, proving the choice is blast-radius, not
capability.

**Probe A — `probe_mangle.rs` (chosen: mangling, flat crate root).** One file
covering all four scenarios at once: (1) a crate-root `PyRepr` prelude trait +
`impl` for a mangled module struct; (2) a value struct `__pyrst_m_re__Match` with
`Display` + `PartialEq` + `PyRepr`; (3) a companion-style
`enum __pyrst_m_shapes__Shape__ { __pyrst_m_shapes__Shape(...), Square(...) }` with
dispatch + `__field_x` accessor, **base and subclass in different owners**;
(4) same-named `__pyrst_m_operator__sub` / `__pyrst_m_re__sub`; plus a
cross-module struct return (`__pyrst_m_b__make → __pyrst_m_re__Match`) and a
cross-module generic call (`__pyrst_m_a__ident::<i64>`). **Result: COMPILED OK**,
prints `2 / bbnbnb / Match(3, hi) / <Match 3 'hi'> / true / area=0 x=1 / area=16 x=2`.

**Probe B — `probe_modblocks.rs` (rejected: `mod` blocks).** The same four
scenarios as `mod` blocks. **Result: COMPILED OK** (identical output), but requires:
`impl crate::PyRepr for Match` and `crate::__py_mod(...)` inside the module body;
`crate::__pyrst_m_re::Match` / `crate::__pyrst_m_a::ident::<i64>` cross-module
paths; and — the crux — `enum Shape__ { Shape(__pyrst_m_shapes::Shape), Square(Square) }`
path-qualifying the foreign companion-enum variant. These are exactly the
`crate::`/`super::` insertions mangling avoids.

Verdict: **both viable; mangling has the smaller, mechanical blast radius and the
trivial cross-module companion enum.**

---

## H. Staged implementation plan

Five cards, each independently gate-green (full `test_all.sh` green, 0-warning,
emit deterministic). Dependencies are linear except Stage 3 (dotted imports) is
independent of Stages 1–2 and could interleave; Stage 4 needs Stages 2 **and** 3.

### W3-1 — Per-module symbol tables + qualified resolution (flat emission unchanged) · M

Introduce the owner-aware resolution layer with **no emission change**: the
plumbing on which everything else rests.

- **Do:** add the dotted `module_id` to `Module`/resolver (§B); build per-module
  symbol tables + `func_owner`/`class_owner`/`const_owner` maps in `TyCtx`; route
  qualified `X.f()`/`X.CONST` resolution through the owning module's table; add the
  from-import local-binding map. Emission stays flat; the collision stopgap stays
  **active** (co-import still errors — this card does not yet dissolve collisions).
- **Files:** `resolver.rs` (`merge_ctx_from_module`, `resolve`), `typeck/types.rs`
  (`TyCtx`), `typeck/exprs.rs` (qualified paths `:204/:422/:694/:2297/:2585`),
  `ast.rs` (`Module.module_id`).
- **Risk:** low — no emitted bytes change. **Regression:** entire suite
  byte-identical (assert emit determinism unchanged). **Gate:** green + identical.

### W3-2 — Namespaced emission + collision-stopgap retirement (the keystone) · L

Flip emission to owner-qualified mangling; dissolve the 8 pairs.

- **Do:** add `emit_name(owner, name)` + owner-qualified `mangle_const`; thread the
  current emit module into `Codegen`; apply at all §C.2 sites (fn/class/const defs,
  `rust_ty` Class arm, companion enum, qualified + from-import calls, DCE keys).
  Leave **root unwrapped**. Retire `detect_cross_module_collisions` for fn/const;
  **narrow** it to class-vs-class + from-import local conflicts. Add the 8 co-import
  goldens; flip `fail_stdlib_name_collision`.
- **Files:** `codegen/items.rs` (`emit_func`, `emit_class`, `emit_companion_enum`),
  `codegen/exprs.rs` (`rust_ty` Class arm, qualified call/const), `codegen/stmts.rs`
  (`emit_plain_func_call`), `codegen/mod.rs` (const prepass, `mangle_const`,
  `dead_funcs`), `typeck/checks.rs` (`analyze_called_functions` owner-key),
  `resolver.rs` (`detect_cross_module_collisions` narrow), `examples/` (co-import
  goldens + retargeted negatives).
- **Risk:** highest — the cross-module companion enum (§F, mitigated by probe
  A/P2) and the const-mangle owner-qualification. **Regression:** 507 import-free
  goldens byte-identical; 96 import goldens output-identical; new co-import goldens
  green; re-run emit-determinism. **Gate:** green + the 8 pairs co-import.

### W3-3 — Dotted-import resolution + embedded packages (kills the truncation) · M

The bug-fix half; independent of W3-1/2 but sequenced here so W3-4 has both.

- **Do:** resolve the full dotted path in `process_module` (directory layout,
  local `a/b.pyrs` then embedded `"a.b"`); honest `ImportNotFound` on failure (kill
  `path[0]`). Dotted `EMBEDDED_STDLIB` keys + nested `lib/` embedding. Extend the
  qualified-call site to accept two-level `a.b.f()` (`Attr{Attr{Ident,_},_}`).
- **Files:** `resolver.rs` (`process_module`, `stdlib_synthetic_dir` dotted keys),
  `stdlib.rs` (`EMBEDDED_STDLIB` dotted entries, `lookup`), `codegen/exprs.rs` +
  `typeck/exprs.rs` (dotted qualified callee), a small local-package example.
- **Risk:** medium — cycle detection / topological order with dotted keys; the
  synthetic `<stdlib>/a/b.pyrs` key. **Regression:** add P3b/P3c as **negatives**
  (must now error), plus a positive local-package golden. **Gate:** green;
  `import os.path` + bare `os.getcwd()` no longer silently accepted.

### W3-4 — Faithful `os.path` + `urllib.parse` + `collections.abc` · M (×~3, batchable)

The stdlib payoff on top of the infra.

- **Do:** `lib/os/path.pyrs` (faithful `join`/`dirname`/`basename`/`split`/
  `splitext`/`isabs`/`normpath`/…), **keep** `os.*` aliases one release (§E);
  `lib/urllib/parse.pyrs` (pure `quote`/`unquote`/`urlencode`/`urlparse`/
  `parse_qs`); `lib/collections/abc.pyrs` shapes. Each ships a dual-run parity
  golden (`docs/design/stdlib-full.md` §G).
- **Files:** `lib/os/path.pyrs`, `lib/urllib/parse.pyrs`, `lib/collections/abc.pyrs`,
  `stdlib.rs` (register dotted keys), `examples/parity_os_path.pyrs` etc.
- **Risk:** low (pure modules on landed infra). **Regression:** dual-run parity vs
  `python3`. **Gate:** each hits its declared fidelity score with a green parity
  golden. Split per module if review load warrants.

### W3-5 — Docs + negative corpus · S

- **Do:** retire the flat-namespace restriction section + 8-pair table
  (`PYTHON_COMPATIBILITY.md:358-371`); document dotted imports, per-module
  namespacing, explicit-`import os.path`-required, from-import-conflict divergence,
  class-vs-class v2 note; update the `os` row + add new rows. Ensure negatives
  cover: unresolved dotted import, from-import local conflict, class-vs-class
  cross-module collision, alias rejection.
- **Files:** `PYTHON_COMPATIBILITY.md`, `docs/design/stdlib-full.md` §F W3 (mark
  done), `examples/fail_*` negatives.
- **Risk:** none. **Gate:** docs match behavior; negatives all rejected at `check`.

**Total: 5 cards (~7 if W3-4 splits per module).** W3-1→W3-2 is the namespacing
spine; W3-3 is the parallel bug-fix; W3-4 the payoff; W3-5 the close-out.

---

## Relevant files

**This design:** `docs/design/w3-modules.md` (this file). **Context:**
`docs/design/stdlib-full.md` §E (G3 verdict) + §F W3. **Style precedent:**
`docs/design/lazy-generators.md`, `docs/design/exception-lowering.md`.

**Resolver / embedding:** `src/resolver.rs` (`process_module` `path[0]` truncation
:126; `merge_ctx_from_module` :202; `detect_cross_module_collisions` :385;
skip-list :149), `src/stdlib.rs` (`EMBEDDED_STDLIB` :25, `lookup` :249),
`src/ast.rs` (`Stmt::Import` :189), `src/parser.rs` (`parse_import` :808,
`parse_from_import` :828).

**Typeck tables:** `src/typeck/types.rs` (`TyCtx.module_funcs` :273,
`module_consts` :288), `src/typeck/exprs.rs` (qualified resolution :204/:422/:694/
:2297/:2585), `src/typeck/checks.rs` (`analyze_called_functions` :1540),
`src/analysis.rs` (LSP single-file `merge_ctx_from_module(is_root=true)` :77/:104).

**Codegen (emission surface):** `src/codegen/mod.rs` (prelude :842-1018; const
prepass :1027; `mangle_const` :258; `emit_program`/`dead_funcs` :780-1071),
`src/codegen/items.rs` (`emit_func` name :6-18; `emit_class`; `emit_companion_enum`
:1152), `src/codegen/exprs.rs` (`rust_ty` Class arm :3694-3718; qualified call
:210-219; qualified const :2852), `src/codegen/stmts.rs` (`emit_plain_func_call`
:1766), `src/codegen/analysis.rs` (`build_poly_map` :2242).

**Migration:** `lib/os.pyrs` (flattened path fns :80-176 to re-home under
`lib/os/path.pyrs`), `PYTHON_COMPATIBILITY.md` (flat-namespace restriction
:358-371), `test_all.sh` (output-based harness), `examples/` (403 goldens, 8
collision negatives).

**Probes (scratchpad, not committed):** `probe_mangle.rs` (chosen pattern — all 4
scenarios + cross-module companion enum, COMPILED+ran), `probe_modblocks.rs`
(rejected pattern, COMPILED+ran, path friction annotated); real-compiler probes
P1 (collision stopgap), P2 (cross-module polymorphism build+run), P3/P3b/P3c
(`os.path` silent truncation).
