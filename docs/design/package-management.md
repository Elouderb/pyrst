# Package Management & Virtual Environments — Design Document

**Epic card:** (created with this doc). **Status:** 📝 DESIGN. **Date:** 2026-07-11.

## Bottom line

pyrst has **no package manager** today — only `$PYRST_PATH` (an ordered list of import-search
directories) + "a package is a bare directory of `.pyrs` files" + the embedded stdlib. Inter-package
dependencies (kodiak → numpyrs/dateutil/tzdata, dateutil → tzdata) are **implicit and unchecked**: a
build works only because every package happens to sit on one `PYRST_PATH`, and a missing dependency is
a confusing "module not found", not an honest error.

The target (user-specified): **pip-from-GitHub + Python-style isolated virtual environments + a manifest
that verifies a repo is a real pyrst package.** GitHub *is* the registry — a package is just a repo with
a `pyrst.yaml`. `pyrst install <github-url>` clones + verifies + installs it (and its transitive deps)
into the **active virtual environment**, which is fully isolated from every other env.

**The reframe that shapes everything:** pyrst has no runtime (programs compile to self-contained static
binaries), so a pyrst "venv" is not an isolated interpreter — it is an isolated **compile-time package
store** the compiler resolves imports against. The UX mirrors Python exactly (create → activate →
`pyrst install` → per-env isolation); what is isolated is the *set of source packages available at build
time*.

**The load-bearing invariant (user-flagged):** `pyrst build` **bakes the active environment's resolved
dependency closure into the binary.** A build resolves the *full transitive closure* of the entry
program's imports against the **active env's** package store, compiles all of it into the one emitted
Rust program, and **fails with an honest build-time error** if anything the program imports is not
installed in the active env. `env + lockfile → reproducible binary`; a build must use ONLY the active
env's packages (isolation is a correctness property, not just hygiene).

## A. Current state (source-confirmed)

- **Resolution** — `src/resolver.rs`: a `Resolver` parses `$PYRST_PATH` once into an ordered `Vec<PathBuf>`
  of search dirs (`resolver.rs:35`, `parse` at `:295`), and `process_module` resolves each import in the
  order **root-relative → `PYRST_PATH` dirs (in order) → embedded stdlib** (`:224-248`). Empty `PYRST_PATH`
  → the branch is a no-op (the `test_all.sh` suite sets none). A `PYRST_PATH` package re-roots at its own
  dir, so internal imports must be **dotted** (`from kodiak.frame import DataFrame`).
- **"Package" = a directory of `.pyrs`** — e.g. `extern/packages/numpyrs/{ndarray,constructors,ufunc}.pyrs`.
  No manifest, no version, no declared deps.
- **CLI** — `src/main.rs:68`: `match cmd { "build" | "emit" | "check" }`. No `venv`/`install`/`init`.
- **The one existing manifest** (`src/driver.rs:261`) is the *generated* `Cargo.toml` for the **Rust output**
  (compiling the emitted program) — not a pyrst-package manifest. `[dependencies]` there = Rust crates the
  emitted code needs, unrelated to pyrst packages.
- **Embedded stdlib** — `lib/*.pyrs` baked into the compiler (`crate::stdlib::lookup`), resolved last.
- **No cards** exist for packaging (grep-confirmed). Related open card: `da51c7b5` (circular imports) —
  the resolver is the natural place to detect cycles honestly; this epic subsumes it.

## B. Model

- **Package** — a git repo (or local dir) whose root (or `package_root`) holds a `pyrst.yaml` + the
  package's `.pyrs` modules. The manifest's presence + valid `name`/`version` is what certifies "this is a
  real pyrst package"; `install` refuses a repo without it.
- **Virtual environment** — a directory holding an **isolated package store** + a lockfile. Everything a
  build resolves against comes from the active env (plus the embedded stdlib). Two envs never share
  packages. Mirrors Python's create/activate/install UX.
- **Build** — compiles the entry program **plus the transitive closure of its imports, resolved against the
  active env**, into one static binary. See §F for the resolution + isolation + reproducibility rules.

**Non-goals** (initially): a runtime environment manager (there is no runtime); a bespoke package registry
(GitHub is the registry); version *constraint solving* (Phase 2+ adds a version field; a SAT-style solver
is out of scope until there is real version pressure); sandboxing installed code (see §H).

## C. Manifest — `pyrst.yaml`

At the package's `package_root` (default the repo root). YAML per the user's ask. Minimum viable fields:

```yaml
name: kodiak                 # the import name + the store key; [a-z0-9_-]+, unique in an env
version: 0.1.0               # semver string; informational in Phase 1, constrained in Phase 2
package_root: .              # subdir (relative to repo root) holding the .pyrs modules + this file
description: pandas-ergonomics dataframes with a polars-style lazy engine   # optional
dependencies:                # other pyrst packages this one imports
  - name: numpyrs
    git: https://github.com/Elouderb/numpyrs
  - name: dateutil
    git: https://github.com/Elouderb/dateutil
  - name: tzdata
    git: https://github.com/Elouderb/tzdata
```

- **Deps carry their own `git` URL** — because GitHub is the registry, that is how `install` locates a
  transitive dependency. (A `path:` alternative for local/workspace deps is a Phase-1 convenience.)
- **Verification** = `pyrst.yaml` parses + has a valid `name` and `version`. A repo without it → honest
  error: *"<url> is not a pyrst package (no pyrst.yaml at its root)."*
- **`package_root`** lets a repo nest its package (e.g. a monorepo, or `src/`); imports re-root there.
- Parser: a small hand-rolled YAML subset (Rust side) — we control the schema, so we need only
  `key: value`, nested lists of maps, and strings; no need for a full YAML dependency if that is against
  the project's zero-heavy-deps grain (decide at Phase 1; `serde_yaml` is the alternative).

## D. Virtual environment

**Layout** (`pyrst venv .pyrstenv` →):
```
.pyrstenv/
  pyrst-env.yaml       # env metadata (pyrst version that created it, created-at is omitted for determinism)
  packages/            # the isolated store: one <name>/ subdir per installed package (its package_root copied in)
    numpyrs/ ...
    dateutil/ ...
  pyrst.lock           # resolved install set: name -> {git, commit-SHA, version} (pinned, reproducible)
  activate             # POSIX shell script: sets PYRST_VENV=<abs .pyrstenv>, prepends nothing to PATH
                       # (pyrst is global); `deactivate` unsets it. Mirrors python venv activate.
```

- **Creation** — `pyrst venv <dir>` (default `.pyrstenv`) writes the skeleton above with an empty store.
- **Activation** — two ways, Python-like:
  1. `source .pyrstenv/activate` → exports `PYRST_VENV=<abs path>` (explicit, like python).
  2. **Auto-detect**: if `PYRST_VENV` is unset, `pyrst` searches CWD → ancestors for a `.pyrstenv/`
     (like cargo finds `Cargo.toml`). Explicit `PYRST_VENV` wins. This makes `pyrst build` "just work"
     inside a project dir without sourcing anything.
- **Isolation** — `install`/`build`/`list` operate **only** on the active env's `packages/`. No global
  store in Phase 1 (a shared read-only cache of *clones* is fine — see §E — but the installed **store** is
  per-env). Env A's `numpyrs` is invisible to env B.

## E. Install — `pyrst install <github-url>`

1. **Resolve source** — shallow `git clone` the URL (a ref/tag/commit may be appended, e.g. `url@v0.2.0`
   or `url#<sha>`) into a **clone cache** (`~/.cache/pyrst/clones/<host>/<owner>/<repo>@<sha>/`), keyed by
   resolved commit SHA so repeated installs are cheap + reproducible. A bare `git` URL resolves to the
   default branch's current HEAD SHA at install time (then pinned).
2. **Verify** — read `<clone>/<package_root>/pyrst.yaml`; error honestly if absent/invalid.
3. **Install** — copy `<clone>/<package_root>/` (the `.pyrs` + manifest) into `<venv>/packages/<name>/`.
   A name collision with a **different** git/SHA already in the env → honest error (or `--force`); same
   SHA → idempotent no-op.
4. **Recurse** — for each entry in `dependencies`, install it (by its `git` URL) unless already satisfied
   in the env. Maintain a visited set → **cycle detection** is an honest error naming the cycle (subsumes
   `da51c7b5`). Diamonds (two deps needing the same package) install once.
5. **Lock** — record every installed package in `pyrst.lock`: `name → {git, commit, version}` (pinned
   SHAs). `pyrst install` with no URL = "install from `pyrst.lock`" (reproduce the env).

`pyrst install <path>` (a local dir) is the Phase-1 workflow that lets the four in-repo packages be
installed without pushing to GitHub yet; the GitHub path is the same flow with a clone step in front.

## F. Resolution & build — the active-env bake-in (the load-bearing part)

**New resolution order** when an env is active:
```
root-relative (the entry program's own dir/subdirs)
  → <active PYRST_VENV>/packages/<name>/...      (the isolated store)
  → embedded stdlib
```
`$PYRST_PATH` remains in the search order as a lower-precedence resolver *fallback* for advanced/local
use, but the env store is the primary mechanism — and **isolation is a hard property**: the §F
completeness gate checks the env **store** directly, so with an env active a package supplied *only* via
`PYRST_PATH` does NOT satisfy the gate and the build fails honestly. So `PYRST_PATH` cannot smuggle a
dependency into an env build; it is a resolution fallback, not a way to bypass env completeness. With an
env active and no manual `PYRST_PATH`, `pyrst build main.pyrs` resolves
`from kodiak.frame import DataFrame` against `<venv>/packages/kodiak/frame.pyrs`.

**Build = compile the transitive closure from the active env:**
- The resolver already gathers the closure of imports; the change is *where* it looks (the env store) and
  that it does so **exclusively** from the active env (isolation).
- **Every** imported module must resolve to (a) the entry program's own tree, (b) an installed package in
  the active env, or (c) the embedded stdlib. Anything else → an **honest build-time error**:
  *"module `foo` is imported but not installed in the active environment `<.pyrstenv>` — `pyrst install`
  it, or check its package's `pyrst.yaml` dependencies."* Never a downstream "module not found"/rustc leak.
- The emitted Rust program bakes in **all** resolved package source → the binary is self-contained, exactly
  as today; the only new thing is that the *set* of source is the env's resolved closure.
- **Consistency check (the user's requirement):** a build must confirm every package the entry program
  transitively needs is present + coherent in the env before emitting. Concretely: walk the import graph;
  for each package boundary, confirm the target package is installed and that ITS declared `dependencies`
  are also installed (the env is "complete"). An incomplete env → honest error naming what to install,
  BEFORE codegen. This guarantees "everything builds properly with packages from the active environment."
- **Reproducibility** — same entry source + same `pyrst.lock` (pinned SHAs) → byte-identical emit (the
  project already guarantees deterministic emit; this extends the guarantee across the pinned dep set).

## G. CLI surface

| Command | Effect |
|---|---|
| `pyrst venv [dir]` | create an isolated env (default `.pyrstenv`) |
| `pyrst install <git-url\|path>` | clone/copy + verify + install a package and its deps into the active env; update `pyrst.lock` |
| `pyrst install` (no arg) | reproduce the env from `pyrst.lock` |
| `pyrst init` | scaffold a `pyrst.yaml` for the current dir (make it a package) |
| `pyrst list` | list packages installed in the active env (name @ version @ short-SHA) |
| `pyrst freeze` | print the lock set (pinned) — for sharing/CI |
| `pyrst build\|check\|emit <file>` | **env-aware** (resolve against the active env; honest error if incomplete) |

`--venv <dir>` global flag overrides auto-detect/`PYRST_VENV` for any command.

## H. Security / trust model

`pyrst install` clones + `pyrst build` **compiles** third-party source → the same trust model as
`pip install` / `cargo add` (arbitrary code is compiled and can run when the built binary runs). We
**verify a repo is a pyrst package** (manifest) and **pin commit SHAs** (no silent upstream drift), but we
**do not sandbox** installed code. This must be documented honestly in `pyrst install`'s help and the docs;
sandboxing / provenance is explicitly a future concern, not a Phase-1 promise.

## I. Phased implementation plan

Rust-side compiler + CLI feature (NOT extern/ dogfood) → normal serialized pipeline, full review stack,
`test_all.sh` gate per phase.

- **Phase 1 — manifest + venv + env-aware local resolution.** `pyrst.yaml` parse+verify; `pyrst venv`
  (skeleton + activate + auto-detect); `pyrst install <path>` (local copy + verify + recurse + lockfile,
  cycle detection); resolver reads the active env store (order in §F); `pyrst build/check` become
  env-aware with the §F completeness check + honest errors; `pyrst init`/`list`/`freeze`. **AC:** create an
  env, `pyrst install` the four in-repo packages by path (deps declared in their `pyrst.yaml`s resolve
  transitively), build the kodiak demos against the env with NO manual `PYRST_PATH`, and an
  import-of-an-uninstalled-package is an honest build error. Determinism: same lock → identical emit.
- **Phase 2 — install from GitHub + lockfile pinning.** `pyrst install <github-url>` (shallow clone into
  the SHA-keyed cache, `url@ref`/`#sha` support, pin SHAs), `pyrst install` (no-arg reproduce from lock).
  **AC:** `pyrst install https://github.com/Elouderb/numpyrs` into an env, build a program that imports it;
  re-install from lock is byte-reproducible; a non-pyrst repo → honest error.
- **Phase 3 — transitive fetch + robustness.** Full transitive install over GitHub (deps' `git` URLs),
  diamond dedup, version field surfaced (informational), better errors (auth/private-repo/offline honest
  messages), `--force`, cache GC. **AC:** `pyrst install <kodiak-url>` pulls numpyrs+dateutil+tzdata
  transitively from GitHub; a cycle is an honest error.
- **Retrofit** — add `pyrst.yaml` to numpyrs/tzdata/dateutil/kodiak (deps declared), push each to its own
  GitHub repo, and prove `pyrst install <kodiak-url>` in a fresh env builds the demos. The dogfood proof
  that the system works end-to-end.

Version constraints (semver ranges + selection) are a **Phase 4** once there is real multi-version
pressure; the `version` field exists from Phase 1 so the data is present.

## J. Open questions

- **YAML dependency vs hand-rolled parser** — schema is ours + small; lean hand-rolled to avoid a heavy
  Rust dep unless the project prefers `serde_yaml`. Decide at Phase 1.
- **Monorepo packages** — `package_root` covers "package nested in a repo"; a repo exporting *multiple*
  packages (subdir-per-package) is a possible `pyrst install <url>#<subdir>` extension, deferred.
- **stdlib as packages** — `lib/` stays embedded (resolved last) for now; making it installable/overridable
  is a later question.
- **Private repos / auth** — Phase 3 should at least give an honest "auth failed" message; credential
  handling (SSH/token) is deferred.
- **Global vs per-env store** — Phase 1 keeps the installed store per-env (true isolation) with a shared
  read-only *clone* cache; a content-addressed global store with per-env symlinks is a future optimization.
