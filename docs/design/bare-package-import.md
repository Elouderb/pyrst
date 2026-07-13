# Bare Package Import — `import <package>` via `__init__.pyrs` (design)

Status: design (2026-07-12). Follows `docs/design/w3-modules.md` (the module/namespace
system this builds on). User-requested: a bare `import kodiak` should work, Python-style,
**while keeping** the existing `from kodiak.series import Series` dotted form.

## 1. Motivation

Today a package is a *directory* of submodules (`kodiak/series.pyrs`, `kodiak/frame.pyrs`).
You must import each submodule explicitly (`from kodiak.series import Series`), and a bare
`import kodiak` is an honest error (`IsPackageNotModule`, added in card 587a9dcb) because
there is no `kodiak.pyrs` module file. This is un-Pythonic: `import numpy; numpy.array(...)`
is the expected ergonomic. We add a **package entry file** so a package can publish a
curated public surface reachable as `kodiak.Name`.

Crucially, the investigation (this feature's prerequisite) established that **qualified
member access `module.member` already works** (`import os; os.getcwd()` compiles via a
qualifier→symbol-table lookup keyed by the dotted `module_id`, mangled `__pyrst_m_os__getcwd`
— see `w3-modules.md` §C/§D). So this feature is a **re-export/aggregation** layer over that
existing machinery, **not** a new "module-as-value" type (which `w3-modules.md:129-131`
explicitly defers and this design keeps deferred).

## 2. The model

- **Package entry file.** A directory `kodiak/` may contain `kodiak/__init__.pyrs`. When
  `import kodiak` resolves to a directory (no leaf `kodiak.pyrs`) and that directory contains
  `__init__.pyrs`, that file is the **package entry module**, with `module_id = "kodiak"`.
- **Public surface.** `import kodiak` exposes, as `kodiak.<Name>`, exactly the **top-level
  names bound in `__init__.pyrs`**: its own top-level `def`/`class`/const definitions, **and**
  the names it brings in via `from kodiak.<sub> import <Name>` (re-exports). Nothing else in
  the package is auto-exposed (a submodule not referenced by `__init__` still needs its own
  `from kodiak.other import ...`) — consistent with the existing "explicit-import-required,
  no auto-expose" stance (`w3-modules.md:114-122`).
- **Re-export is package-init-specific (scoped).** A `from X import Y` in `__init__.pyrs`
  makes `Y` part of the package's public surface. Regular (non-init) modules keep today's
  behavior — their `from`-imports stay local-only, NOT re-exported (a possible future
  unification, deferred, documented as a non-goal). This keeps the change additive.
- **True-owner mangling (the load-bearing detail).** A re-exported `kodiak.Series` must lower
  to `Series`'s **real defining module**, `__pyrst_m_kodiak_dseries__Series`, NOT to any
  `kodiak`-owned name. So the package's qualifier entry records the **true owner module_id**
  (`kodiak.series`), and codegen mangles against that owner. Names *defined directly in*
  `__init__.pyrs` are owned by `kodiak` and mangle `__pyrst_m_kodiak__<name>` as usual. No new
  mangling scheme — only a redirect table reusing the existing owner-keyed mangler
  (`emit_name`, `src/codegen/analysis.rs:81`).
- **`from kodiak import Name` companion.** `from <pkg> import Name` where `<pkg>` is a package
  with an `__init__.pyrs` binds `Name` into local scope, resolving through the same public
  surface (to the true owner). This is the direct-binding counterpart of `kodiak.Name` and
  falls out of the same surface table.

### Usage

```
# kodiak/__init__.pyrs — the package's public API
from kodiak.series import Series
from kodiak.frame  import DataFrame
from kodiak.io     import read_csv

def version() -> str:        # a name defined in __init__ itself
    return "1.0"

# ---- importer ----
import kodiak
df = kodiak.read_csv("data.csv")   # -> __pyrst_m_kodiak_dio__read_csv(...)
s  = kodiak.Series([1, 2, 3])      # -> __pyrst_m_kodiak_dseries__Series(...)
v  = kodiak.version()              # -> __pyrst_m_kodiak__version()

from kodiak import Series           # companion direct-binding form
from kodiak.series import Series     # the pre-existing dotted form — UNCHANGED
```

## 3. Resolution

In `resolver.rs::process_module`, the `Stmt::Import` handling currently: root-relative file →
search-dir file → embedded stdlib → (587a9dcb) if a same-named DIRECTORY exists,
`IsPackageNotModule`. This feature changes the **directory branch**:

1. On a directory hit `<dir>/kodiak/`, look for `<dir>/kodiak/__init__.pyrs`.
2. **If present:** resolve it as a module with `module_id = "kodiak"`; record `kodiak` as a
   package in the resolver with a **public-surface table** built from `__init__.pyrs`'s
   top-level bindings (own defs → owner `kodiak`; `from kodiak.<sub> import Y` → owner
   `kodiak.<sub>`, and the sub is resolved+compiled as usual). Bare `import kodiak` and
   `from kodiak import Y` resolve against this table.
3. **If absent:** keep an honest error, reworded — "`kodiak` is a package with no
   `__init__.pyrs`; import a submodule (`from kodiak.<sub> import <name>`) or add
   `kodiak/__init__.pyrs`." (Narrows the existing `IsPackageNotModule` copy.)

Only `__init__.pyrs`'s transitive import closure is compiled by `import kodiak` (submodules it
doesn't touch are not pulled in). Reuses the existing search-dir precedence (root-relative →
env `packages/` → `$PYRST_PATH` → embedded) and `package_dir_for` (587a9dcb). Env-installed
packages ship their `__init__.pyrs` in the store (install copies the package_root, which
includes it).

## 4. Semantics & edge cases (honest errors)

- **Name collisions in the surface.** Two re-exports of the same name, or a re-export
  colliding with an `__init__`-local def → honest resolver error naming both sources. (Reuses
  the spirit of `detect_cross_module_collisions`.)
- **Importing a name not in the surface.** `from kodiak import Nope` where `Nope` isn't in
  `__init__`'s surface → honest "`kodiak` has no public name `Nope` (available: …)".
- **Class-name global uniqueness** (`w3-modules.md` §D) is unchanged: a re-exported class is
  the same globally-unique class, just reachable under a package qualifier — no new class.
- **Privacy.** v1: every top-level name in `__init__.pyrs` is public (no `__all__`, no
  underscore-privacy). Underscore-privacy is a possible later refinement (non-goal now).
- **Cycles.** `__init__.pyrs` re-exports from its own submodules; a submodule must not import
  the package entry back (`import kodiak` from inside `kodiak/series.pyrs`) — the existing
  cycle detector catches it and errors honestly.
- **No `__init__` + explicit submodule still works.** `from kodiak.series import Series` never
  consults `__init__.pyrs`; it stays byte-for-byte the current dotted resolution.

## 5. Non-goals (deferred, documented)

Module-as-first-class-value (`Ty::Module`, passing modules around, dynamic attribute access);
`import X as Y` / `from X import Y as Z` aliasing (still an honest parse error); relative
imports (`from . import x`); `__all__` / underscore privacy; re-export of `from`-imports by
**regular** (non-init) modules. None are required for the ergonomic above.

## 6. Change surface

- `src/resolver.rs` — the directory branch of `process_module`; a package public-surface table
  (extend the existing per-module `module_funcs`/`module_consts` shape with a
  true-owner-carrying entry, or a parallel `package_exports: map<pkg_id, map<name, (owner_id,
  name)>>`); `from <pkg> import Name` binding through the surface; collision detection.
- `src/typeck` — resolve `pkg.Name` / `from pkg import Name` against the surface table to the
  true owner's signature/type.
- `src/codegen` — lower a surfaced name to its **true owner**'s mangled name (reuse
  `emit_name`/`mangle_mod_id`; no new scheme).
- `src/diag.rs` — reword/narrow `IsPackageNotModule` (no-`__init__` case) + new surface-miss /
  collision errors.
- `docs/design/w3-modules.md` cross-link + `PYTHON_COMPATIBILITY.md:504` (imports section).

## 7. Rollout (dogfood)

- Add `__init__.pyrs` to the 4 packages (`kodiak`, `numpyrs`, `dateutil`, `tzdata`) re-exporting
  each one's public API; mirror to the GitHub repos (extern/ copy is the mirror source).
- A demo under `extern/programs/` that uses the bare form (`import kodiak; kodiak.read_csv(...)`)
  proving it end-to-end against the env store.

## 8. Tests

- Resolver: `import pkg` with `pkg/__init__.pyrs` re-exporting a submodule name → resolves;
  `pkg.Name` and `from pkg import Name` both work; a name defined in `__init__` itself works;
  `import pkg` with NO `__init__.pyrs` → the honest reworded error; surface-miss + collision
  errors; the dotted `from pkg.sub import X` form still resolves unchanged.
- Codegen: `pkg.ReexportedClass(...)` / `pkg.reexported_fn(...)` lower to the **true owner**'s
  mangled name (golden), proving no mis-mangle to `pkg`.
- e2e: build a program using `import kodiak` against the installed env store.
- INVARIANT: `./test_all.sh` at exact baseline (no package/`__init__` in the corpus → the new
  branch is inert). cargo + test_pkg suites green.
