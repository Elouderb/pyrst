use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{Module, Stmt};
use crate::diag::{Error, Result, Span};
use crate::typeck::TyCtx;

/// Result of resolving all imports from a root file.
/// Modules are in topological order (dependencies before dependents).
pub struct ResolvedProgram {
    /// List of (Module, source_text) in dependency order: imports first, root last
    pub modules: Vec<(Module, String)>,
    /// Merged type context from all modules
    pub ctx: TyCtx,
}

/// Internal resolver state for DFS traversal
struct Resolver {
    /// Cached parsed modules: path → (Module, source_text)
    cache: HashMap<PathBuf, (Module, String)>,
    /// Paths currently being processed (for cycle detection)
    in_flight: HashSet<PathBuf>,
    /// DFS stack for cycle path reconstruction
    dfs_stack: Vec<PathBuf>,
    /// Topologically sorted paths: dependencies before dependents
    order: Vec<PathBuf>,
}

impl Resolver {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
            in_flight: HashSet::new(),
            dfs_stack: Vec::new(),
            order: Vec::new(),
        }
    }

    /// DFS traversal of the import graph starting from a FILE on disk
    /// (`abs_path`). Reads + normalizes the source, then hands off to
    /// [`Resolver::process_module`] for the shared parse / recurse / cache work.
    /// After completion, `self.order` is in topological order.
    ///
    /// `module_id` is the canonical DOTTED import path that reached this module
    /// (`None` for the ROOT), threaded through so [`Resolver::process_module`] can
    /// stamp it onto the parsed [`Module`] — the per-module-namespace key, set
    /// from the import path rather than the file stem (W3-1).
    fn visit(&mut self, abs_path: PathBuf, base_dir: &Path, import_span: Span, module_id: Option<String>) -> Result<()> {
        // Cheap pre-check before reading the file: a module already finished or
        // currently in-flight needs no re-read. (`process_module` re-checks under
        // the post-read key, but for a file the key IS `abs_path`, so this also
        // short-circuits the disk read on a diamond/cycle.)
        if self.order.contains(&abs_path) {
            return Ok(());
        }

        // Load and parse this file. Normalize line endings (CRLF / bare CR -> LF)
        // at the read site so the SAME `\n`-only text feeds both the lexer (spans)
        // and the diagnostic renderer (this `src` is cached below and later paired
        // into `Error::Sourced` via `with_render_source`); see
        // `lexer::normalize_line_endings`.
        let src = std::fs::read_to_string(&abs_path)
            .map(|s| crate::lexer::normalize_line_endings(&s))
            .map_err(|_| {
                let importing_file = abs_path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                Error::ImportNotFound {
                    path: importing_file,
                    span: import_span,
                    importing_file: base_dir.display().to_string(),
                }
            })?;

        self.process_module(abs_path, src, base_dir, import_span, module_id)
    }

    /// Shared DFS body for a single module, given its module KEY (`key`) and
    /// already-loaded `src`. Used by both [`Resolver::visit`] (file modules,
    /// where `key` is the canonical on-disk path) and the embedded-stdlib path
    /// (where `key` is a SYNTHETIC `<stdlib>/<name>.pyrs` path that exists only
    /// for cycle-detection, caching, and diagnostics).
    ///
    /// `base_dir` is the directory against which THIS module's own `import`s are
    /// resolved as local files (for a file module, its parent dir; for an
    /// embedded module, the synthetic `<stdlib>` dir — which has no real files,
    /// so local lookups there harmlessly miss and fall through to embedded).
    ///
    /// `module_id` (the dotted import path that reached this module; `None` for
    /// the ROOT) is stamped onto the parsed module as its per-module-namespace key
    /// (W3-1), taken from the import path — NOT the file stem, which is ambiguous
    /// for a dotted submodule (`lib/os/path.pyrs` → stem `"path"`, id `"os.path"`).
    fn process_module(
        &mut self,
        key: PathBuf,
        src: String,
        base_dir: &Path,
        import_span: Span,
        module_id: Option<String>,
    ) -> Result<()> {
        // Already processed (diamond import case)
        if self.order.contains(&key) {
            return Ok(());
        }

        // Circular import detection
        if self.in_flight.contains(&key) {
            // Reconstruct cycle path
            let cycle_start = self.dfs_stack.iter().position(|p| p == &key).unwrap_or(0);
            let cycle: Vec<String> = self.dfs_stack[cycle_start..]
                .iter()
                .chain(&[key.clone()])
                .map(|p| p.file_stem().unwrap_or_default().to_string_lossy().to_string())
                .collect();
            return Err(Error::CircularImport { cycle, span: import_span });
        }

        // Mark as in-flight
        self.in_flight.insert(key.clone());
        self.dfs_stack.push(key.clone());

        // EPIC-8: a parse error belongs to THIS file — pair its own source (and
        // path) so the snippet renders against the imported file, not the root.
        // Name the file ONLY when it was reached via an import (DFS depth > 1):
        // a parse error in the ROOT of a single-file program must render exactly
        // as before (no `in <file>` suffix), so the source is attached but the
        // file name is withheld for the root.
        let name_file = self.dfs_stack.len() > 1;
        let render_path = if name_file { Some(key.clone()) } else { None };
        let mut m = crate::parser::parse(&src)
            .map_err(|e| e.with_render_source(render_path, &src))?;
        m.source_path = Some(key.clone());
        // (W3-1) Stamp the dotted import path that reached this module as its
        // per-module-namespace key. Set from the import path (threaded via
        // `module_id`), so a dotted submodule keeps its FULL id even though its
        // file stem is ambiguous. `None` here means the ROOT program.
        m.module_id = module_id;

        // Recurse into this module's imports BEFORE adding self to order (post-order DFS)
        for stmt in &m.stmts {
            if let Stmt::Import { path, span, .. } = stmt {
                // (W3-3) Resolve the FULL dotted import path against a DIRECTORY
                // layout, keyed by the canonical dotted module id (`path.join(".")`)
                // — killing the old `path[0]` truncation (`import os.path` used to
                // silently compile as `import os`, dropping `.path`; a violation of
                // pyrst's honest-errors invariant). A single-component `import os`
                // has `mod_id == path[0]`, so its resolution is byte-identical to the
                // pre-W3-3 path. An empty path cannot arise from the parser, but the
                // `unwrap_or` keeps this total rather than panicking.
                let mod_id = path.join(".");
                let mod_name = mod_id.as_str();

                // Skip standard library modules that are NOT yet real modules
                // (no codegen, no embedded source). `dataclasses` has dedicated
                // handling elsewhere; the rest are silent no-ops until
                // implemented. `os`/`math`/`re`/`string`/`bisect`/`heapq`/
                // `collections` are NOT here: they are real embedded modules
                // (under `lib/`, resolved below), so they must reach the
                // resolution path. `re` (Rust interop Phase 2) is backed by the
                // external `regex` crate. `collections` is an embedded module
                // for `Counter`/`most_common`; its generic-class members
                // (`deque`/`defaultdict`/…) are a later batch. `json` is NO
                // LONGER skipped: it is now a real PURE-PYRST embedded module
                // (`lib/json.pyrs` — a recursive-descent `loads`/`dumps` over a
                // recursive tagged `JsonValue` class), so it must reach the
                // resolution path. `itertools` is NO LONGER skipped: it is now a
                // real embedded module (`lib/itertools.pyrs`, an eager subset), so
                // it must reach the resolution path. (`textwrap`/`random` were
                // never skipped and resolve as embedded modules too.) `sys` is
                // NO LONGER skipped (W2 card cd3aa7b7): it is now a real
                // embedded module (`lib/sys.pyrs`, partial scope: `maxsize`/
                // `platform`/`version`/`version_info`/`exit`), so it must
                // reach the resolution path. `dataclasses` stays: it has no
                // real module body, only decorator handling.
                if mod_name == "dataclasses" {
                    continue;
                }

                // (W3-3) DIRECTORY layout: a dotted id `a.b.c` maps to the nested
                // relative path `a/b/c.pyrs` — the leaf `.pyrs` under a package
                // directory chain (`import a.b` → `a/b.pyrs`; `import os` →
                // `os.pyrs`, byte-identical to the pre-W3-3 single-file join). This
                // mirrors CPython packages (`urllib/parse.py`) and is the natural
                // shape for a LOCAL user package. `path` is always non-empty from the
                // parser; `split_last` therefore always succeeds.
                let rel_layout = |base: &Path| -> PathBuf {
                    match path.split_last() {
                        Some((last, prefix)) => {
                            let mut p = base.to_path_buf();
                            for seg in prefix {
                                p = p.join(seg);
                            }
                            p.join(format!("{}.pyrs", last))
                        }
                        None => base.join(format!("{}.pyrs", mod_name)),
                    }
                };

                // Resolution order: a LOCAL `<base_dir>/a/b.pyrs` on disk SHADOWS an
                // embedded stdlib module keyed by the same dotted id.
                let dep_path = rel_layout(base_dir);
                if let Ok(dep_abs) = dep_path.canonicalize() {
                    // Local file found. Its module id is the FULL dotted import path
                    // that reached it (W3-1/W3-3) — for a single-component `import
                    // mod` that is `mod_name` itself.
                    self.visit(dep_abs, base_dir, *span, Some(mod_id.clone()))?;
                } else if let Some(embedded_src) = crate::stdlib::lookup(mod_name) {
                    // No local file → resolve from the EMBEDDED stdlib, keyed by the
                    // DOTTED id (`EMBEDDED_STDLIB` gained dotted entries, e.g.
                    // `"os.path"` → `lib/os/path.pyrs`). Use a synthetic
                    // `<stdlib>/a/b.pyrs` key (mirroring the directory layout) so
                    // caching, cycle detection, and diagnostics all work like a file
                    // module, and resolve the embedded module's OWN imports against
                    // the synthetic stdlib dir (local-then-embedded, recursively).
                    let stdlib_dir = stdlib_synthetic_dir();
                    let dep_key = rel_layout(&stdlib_dir);
                    let dep_src = crate::lexer::normalize_line_endings(embedded_src);
                    self.process_module(dep_key, dep_src, &stdlib_dir, *span, Some(mod_id.clone()))?;
                } else {
                    // Neither a local file nor an embedded module exists for the FULL
                    // dotted id. This is the honest death of the `path[0]` truncation:
                    // `import os.nonexistent` (parent `os` real, submodule not) and
                    // `import os.path` (before `lib/os/path.pyrs` exists) both error
                    // here naming the FULL missing id `os.nonexistent` / `os.path` —
                    // never silently resolving to the parent `os`.
                    return Err(Error::ImportNotFound {
                        path: mod_id.clone(),
                        span: *span,
                        importing_file: key.display().to_string(),
                    });
                }
            }
        }

        // Post-order: add to order AFTER all dependencies are processed
        self.dfs_stack.pop();
        self.in_flight.remove(&key);
        self.cache.insert(key.clone(), (m, src));
        self.order.push(key);
        Ok(())
    }
}

/// The SYNTHETIC base directory used as the module-key prefix for embedded
/// stdlib modules. It is a marker path for caching / cycle detection /
/// diagnostics only — it is never read from disk (embedded sources come from
/// [`crate::stdlib`]). A `<stdlib>/<mod>.pyrs` local-file lookup against this dir
/// therefore always misses, so an embedded module's own imports fall through to
/// the embedded lookup, matching the root resolution order.
fn stdlib_synthetic_dir() -> PathBuf {
    PathBuf::from("<stdlib>")
}

/// Merge function/class signatures from a single module into a context.
/// Exposed as `pub(crate)` so `analysis.rs` can build a single-module TyCtx
/// without touching the filesystem resolver.
pub(crate) fn merge_ctx_from_module(m: &Module, ctx: &mut TyCtx, is_root: bool) -> Result<()> {
    // Qualified-module-call support (card 81db88e0): a NON-ROOT (imported)
    // module is addressable by NAME — its source-file stem — so `import X;
    // X.f(args)` can resolve `f` against this module. The ROOT program's own
    // functions are NOT a qualifiable module (you don't write `root.f()`), so we
    // record nothing for the root. The stem comes from `m.source_path`, which the
    // resolver always sets to the canonical on-disk path (file module) or the
    // synthetic `<stdlib>/<name>.pyrs` key (embedded module) — both yield the
    // right stem (e.g. "os"). On the LSP single-file path `merge_ctx_from_module`
    // is called with `is_root = true`, so this branch never runs there and
    // `module_funcs` stays empty (qualified calls don't resolve in the editor —
    // the same gap the rest of the stdlib has; it does not crash).
    //
    // (W3-1) The qualifier key is now the DOTTED module id the resolver stamped
    // from the IMPORT PATH (`m.module_id`), not the file stem — a dotted submodule
    // (`lib/os/path.pyrs`, id `"os.path"`) has the ambiguous stem `"path"`. For a
    // single-component `import mod` the id equals the stem, so this is byte-for-byte
    // the prior behaviour. The `or_else` stem fallback covers a non-root module a
    // caller built without an id (the resolver always sets one); it never fires on
    // the real path.
    let module_name: Option<String> = if is_root {
        None
    } else {
        m.module_id.clone().or_else(|| {
            m.source_path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
        })
    };

    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
                // (W3-fix) Record the ROOT's own top-level fn names so owner-first
                // resolution (typeck + codegen) lets a root def shadow a from-import
                // of the same name (root-shadows-imports).
                if is_root {
                    ctx.root_defined.insert(f.name.clone());
                }
                // Skip main() from non-root modules
                if f.name == "main" && !is_root {
                    continue;
                }
                // Record this function as qualifiable via its module name (e.g.
                // `os.basename`). FLAT `ctx.funcs` (below) still holds the actual
                // signature under the bare name; `module_funcs` is only the
                // module→names index the qualified-call paths consult.
                if let Some(mod_name) = &module_name {
                    ctx.module_funcs
                        .entry(mod_name.clone())
                        .or_default()
                        .push(f.name.clone());
                }
                // Generics v1: lower this function's annotations with its declared
                // type parameters in scope, so a `T`/`list[T]`/`tuple[A, B]` param
                // or return resolves to `Ty::TypeVar` in the stored signature.
                // Empty `type_params` => identical to the non-generic path.
                let params: Vec<(String, crate::typeck::Ty)> = f.params.iter()
                    .map(|p| ctx.resolve_annot(&p.ty, p.span, &f.type_params).map(|ty| (p.name.clone(), ty)))
                    .collect::<crate::diag::Result<Vec<_>>>()?;
                let sig = crate::typeck::FuncSig {
                    params,
                    // A return annotation carries no span of its own; point at the
                    // function definition so a bad `-> ...` still gets a real caret.
                    ret: ctx.resolve_annot(&f.ret, f.span, &f.type_params)?,
                    param_defaults: f.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| p.default.clone())
                        .collect(),
                    // EPIC-4 V2: per-param by-reference (`Mut[T]`) mode, parallel
                    // to `params`/`param_defaults` (self filtered out).
                    param_by_ref: f.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| p.by_ref)
                        .collect(),
                };
                // (W3-1) Record this function in its OWNING module's REAL per-module
                // table + the owner index, alongside the flat facade below. Gated on
                // `module_name` (Some iff non-root) exactly like `module_funcs`, so
                // the per-module table, the owner index, and the qualifier index stay
                // in lockstep. Owner-first qualified resolution reads the sig from
                // here; the flat `ctx.funcs` insert stays for emission + every
                // non-module lookup (unchanged this stage).
                if let Some(mod_name) = &module_name {
                    ctx.module_symbols.entry(mod_name.clone()).or_default().funcs.insert(f.name.clone(), sig.clone());
                    ctx.func_owner.insert(f.name.clone(), mod_name.clone());
                }
                ctx.funcs.insert(f.name.clone(), sig);
                // Generics v1: record the declared type-parameter list so call
                // sites can unify and detect uninferable parameters. Only generic
                // functions are inserted (a plain `def` is absent).
                if !f.type_params.is_empty() {
                    ctx.generic_funcs.insert(f.name.clone(), f.type_params.clone());
                    // Generics v2: store the full body so `infer_func_typevar_bounds`
                    // can recompute a CALLEE's required bounds for transitive
                    // propagation (a generic `f` calling a generic `g` folds `g`'s
                    // bounds into `f`'s clause). Only generic functions are stored.
                    ctx.generic_func_bodies.insert(f.name.clone(), f.clone());
                }
            }
            Stmt::Class(c) => {
                let mut c = c.clone();
                crate::typeck::extract_init_fields(&mut c);
                if is_root {
                    ctx.root_defined.insert(c.name.clone());
                }
                ctx.classes.insert(c.name.clone(), c.clone());
                // (W3-1) Record this class in its OWNING module's real per-module
                // table + the owner index (non-root only). Class names stay globally
                // unique this stage (the stopgap keeps class-vs-class an error), so
                // the flat `ctx.classes` and `class_owner` never conflict; the owner
                // index is what stage 2's `rust_ty` will mangle a cross-module type
                // reference by.
                if let Some(mod_name) = &module_name {
                    ctx.module_symbols.entry(mod_name.clone()).or_default().classes.insert(c.name.clone(), c.clone());
                    ctx.class_owner.insert(c.name.clone(), mod_name.clone());
                }
                // Generics v2 (generic CLASSES): record the declared type-param
                // names so substitution sites can zip them against an instance's
                // `Ty::Class(name, args)`. ONLY a generic class (non-empty
                // `type_params`) is registered — a plain class stays absent, so
                // the non-generic hot path is byte-for-byte unchanged.
                if !c.type_params.is_empty() {
                    ctx.generic_classes.insert(c.name.clone(), c.type_params.clone());
                }
                // Register method signatures
                for m_fn in &c.methods {
                    let method_name = format!("{}.{}", c.name, m_fn.name);
                    // EPIC-4 V2-c STEP 0: filter `self` from `params` so all three
                    // parallel vectors (params / param_defaults / param_by_ref) are
                    // self-EXCLUSIVE and index-aligned at length N — mirroring
                    // typeck::TyCtx::find_method (which already filters self from
                    // all three). Previously `params` kept self (len N+1) while the
                    // other two dropped it (len N), so `param_by_ref[i]` lined up
                    // with `params[i+1]` — a latent off-by-one that would assign the
                    // first real param's by-ref flag to the `self` slot once method
                    // call-site by-ref logic started reading it (this card). No
                    // existing reader consumed method-keyed `params` (only `.ret`),
                    // so aligning here is non-breaking.
                    // Generics v2: lower the method signature with the CLASS's
                    // type parameters in scope, so a field/param/return naming a
                    // class type var `T` lowers to `Ty::TypeVar("T")` (not the
                    // bogus `Ty::Class("T", [])`). For a non-generic class
                    // `type_params` is empty, so this is identical to the old
                    // unscoped `from_type_expr` and the non-generic path is
                    // unaffected. A call site on a concrete `Ty::Class(name, args)`
                    // instance substitutes the args back in (see typeck).
                    let method_params: Vec<(String, crate::typeck::Ty)> = m_fn.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| ctx.resolve_annot(&p.ty, p.span, &c.type_params).map(|ty| (p.name.clone(), ty)))
                        .collect::<crate::diag::Result<Vec<_>>>()?;
                    // Hoist the return resolution to a local: `resolve_annot` borrows
                    // `&ctx` immutably, so it cannot be evaluated inside the
                    // `ctx.funcs.insert(...)` call (which borrows `ctx.funcs` mutably).
                    let method_ret = ctx.resolve_annot(&m_fn.ret, m_fn.span, &c.type_params)?;
                    ctx.funcs.insert(method_name, crate::typeck::FuncSig {
                        params: method_params,
                        ret: method_ret,
                        param_defaults: m_fn.params.iter()
                            .filter(|p| p.name != "self")
                            .map(|p| p.default.clone())
                            .collect(),
                        param_by_ref: m_fn.params.iter()
                            .filter(|p| p.name != "self")
                            .map(|p| p.by_ref)
                            .collect(),
                    });
                }
            }
            Stmt::Assign { target, ty: Some(t), value, span } => {
                // Register module-level annotated globals with their concrete type
                // (ported from typeck::check_module). Propagate errors so that
                // invalid annotations (e.g. set[float]) are rejected at typeck.
                // This also makes a BARE reference to a module-level constant
                // resolve inside its defining module (it lands in `ctx.vars`).
                let resolved = crate::typeck::Ty::from_type_expr(t, *span)?;
                if is_root && crate::typeck::is_const_literal(value) {
                    ctx.root_defined.insert(target.clone());
                }
                ctx.vars.insert(target.clone(), resolved.clone());
                // MODULE CONSTANTS (mirror of `module_funcs`): when the value is a
                // const literal (int/float/str/bool) and this is a NON-ROOT
                // (imported) module, record `(name, type)` under the module name so
                // a qualified `X.CONST` access resolves (e.g. `math.pi`). The root
                // program is not a qualifiable module (you don't write `root.X`),
                // so the root's own consts are reachable only by their bare name
                // (via `ctx.vars` above), never as `root.CONST`.
                if let Some(mod_name) = &module_name {
                    if crate::typeck::is_const_literal(value) {
                        // (W3-1) Record the const in its OWNING module's real
                        // per-module table + owner index, alongside the flat
                        // `module_consts` facade the fallback path still reads.
                        ctx.module_symbols.entry(mod_name.clone()).or_default().consts.insert(target.clone(), resolved.clone());
                        ctx.const_owner.insert(target.clone(), mod_name.clone());
                        ctx.module_consts
                            .entry(mod_name.clone())
                            .or_default()
                            .push((target.clone(), resolved));
                    }
                }
            }
            Stmt::Import { .. } => {}
            _ => {}
        }
    }
    // (card e131f8b0) Record every class used as a dict KEY / set ELEMENT anywhere
    // in this module (field / param / return / local annotations), accumulating
    // across modules into the shared ctx. Codegen adds `Eq/Hash/Ord` derives for a
    // class in this set; `check` validates each is hashable (`class_hash_eligible`).
    crate::typeck::collect_hash_key_classes(m, &mut ctx.hash_key_classes);
    Ok(())
}

/// (card 6c8b4a39) Detect two DIFFERENT imported modules that define the same
/// top-level public name (func / class / const). pyrst's module namespace is
/// FLAT — every module's top-level names merge into one `ctx.funcs`/`ctx.classes`
/// under their bare name — so co-importing e.g. `operator` (`def sub`) and `re`
/// (`def sub`) let whichever imported SECOND silently overwrite the first, and
/// even a QUALIFIED `operator.sub(...)` then type-checked against re's signature
/// (a silent wrong-function miscompile once two colliders share an arity). Make
/// it an honest error instead.
///
/// Scope, mirroring the two deliberate-shadow paths preserved by this card:
///   - The ROOT module is SKIPPED (it is last in topological order). A user
///     file's own top-level `def` deliberately shadows an imported name
///     (last-write-wins in the merge) — unchanged; only imported×imported
///     duplicates error.
///   - A LOCAL user module shadowing an EMBEDDED stdlib module of the same name
///     is resolved at import time (a local `<dir>/os.pyrs` wins over embedded
///     `os`), so only ONE module of a given stem ever enters `order`; comparing
///     by owning module NAME (source-file stem) means this never fires here.
///
/// `main` is excluded (a non-root module's `main` is not merged, and is not a
/// public API). Const detection matches the merge's `module_consts` rule
/// (annotated const-literal globals).
fn detect_cross_module_collisions(
    order: &[PathBuf],
    cache: &HashMap<PathBuf, (Module, String)>,
) -> Result<()> {
    // top-level public name -> (owning imported module name, its source text).
    let mut owner: HashMap<String, String> = HashMap::new();
    for path in order.iter() {
        // (W3-fix / F11) The root's own top-level FUNCTIONS / CONSTANTS may still
        // shadow imports (only class-vs-class is tracked here). But a root CLASS
        // sharing a name with an imported class is the SAME `Ty::Class`
        // owner-blindness that makes import-vs-import class collisions unresolvable
        // — `rust_ty`/`class_owner`/the companion enum key class identity by the
        // bare name, so a root `class Point` + an imported `class Point` cannot be
        // told apart at a type reference. So the root participates in the global
        // class-name uniqueness check too (coherent with import-vs-import), rather
        // than silently mangling the root's own class to the import's owner. Imports
        // are visited BEFORE the root (topological order), so a collision is
        // reported against the root's class span.
        let (m, src) = &cache[path];
        let module_name: String = m
            .source_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        for s in &m.stmts {
            // (W3-2) NARROWED to CLASS-vs-CLASS only. With per-module namespaced
            // emission (`__pyrst_m_<owner>__<name>`), two co-imported modules that
            // each define a same-named FUNCTION or CONST now emit DISTINCT Rust
            // items, so fn-vs-fn / const-vs-const / fn-vs-const collisions are
            // co-importable — they are no longer collected here. A CLASS name,
            // however, is carried through the type system as a BARE `Ty::Class(name,
            // ..)` (no owner), and `rust_ty` / `class_owner` / the companion enum all
            // key class identity by that bare name — so two same-named classes in
            // different modules would be indistinguishable at a type reference. v1
            // therefore keeps CLASS-name global uniqueness an honest error (threading
            // an owner into `Ty::Class` for true same-named-class co-import is the
            // documented v2 extension). A fn-vs-class name pair is fine (distinct
            // Rust item kinds, distinct mangling), so only class-vs-class is tracked.
            let named: Option<(&str, Span)> = match s {
                Stmt::Class(c) => Some((c.name.as_str(), c.span)),
                _ => None,
            };
            let Some((name, span)) = named else { continue };
            match owner.get(name) {
                // Same module re-declaring its own name (or an intra-module
                // duplicate) is not a cross-module collision — not our concern.
                Some(prev) if prev == &module_name => {}
                Some(prev) => {
                    return Err(Error::Type {
                        span,
                        msg: format!(
                            "class `{}` is defined by both `{}` and `{}`; pyrst v1 \
                             keeps class names globally unique (a class type is carried \
                             as a bare name with no owner, so two same-named classes \
                             cannot be told apart at a type reference) — rename one, or \
                             (v2) thread a module owner into the class type",
                            name, prev, module_name
                        ),
                    })
                    .map_err(|e| e.with_render_source(m.source_path.clone(), src));
                }
                None => {
                    owner.insert(name.to_string(), module_name.clone());
                }
            }
        }
    }
    Ok(())
}

/// Resolve all imports from a root .pyrs file and return a merged program.
pub fn resolve(root_path: &Path) -> Result<ResolvedProgram> {
    let abs_root = root_path.canonicalize().map_err(|e| crate::diag::Error::Io(e))?;
    let root_dir = abs_root.parent().unwrap_or_else(|| Path::new("."));

    let mut resolver = Resolver::new();
    // The ROOT program has no dotted import path (`module_id = None`): it is the
    // sentinel root whose own top-level names stay crate-root-unwrapped (W3-1).
    resolver.visit(abs_root.clone(), root_dir, Span::DUMMY, None)?;

    // (card 6c8b4a39) Reject co-importing two modules that share a top-level
    // public name BEFORE the flat merge silently picks a last-write-wins winner.
    detect_cross_module_collisions(&resolver.order, &resolver.cache)?;

    // Build merged context from all modules in dependency order
    let mut ctx = TyCtx::new();
    let total_modules = resolver.order.len();
    // (W5-h) PRE-SCAN every module's top-level classes for the `@extern class` decl
    // form (an opaque move-only HANDLE kind, e.g. `re.Pattern`) BEFORE any signature
    // is lowered below. A handle class's NAME must resolve to `Ty::Handle` in every
    // signature that mentions it — including a constructor defined ABOVE its class or
    // in an IMPORTING module — so the set has to be complete before the first
    // `resolve_annot`. Empty for programs importing no handle lib.
    for path in &resolver.order {
        let (m, _src) = &resolver.cache[path];
        for s in &m.stmts {
            if let Stmt::Class(c) = s {
                if c.decorators.iter().any(|d| d == "extern") {
                    ctx.handle_classes.insert(c.name.clone());
                }
            }
        }
    }
    for (idx, path) in resolver.order.iter().enumerate() {
        let (m, src) = &resolver.cache[path];
        let is_root = idx == total_modules - 1;
        // EPIC-8: signature-merge errors (e.g. an invalid annotation such as
        // `set[float]`) originate in THIS module — pair its own source/path so
        // the snippet renders against the right file. Name the file only in the
        // multi-file case so single-file output is byte-for-byte unchanged.
        let render_path = if total_modules > 1 { m.source_path.clone() } else { None };
        merge_ctx_from_module(m, &mut ctx, is_root)
            .map_err(|e| e.with_render_source(render_path, src))?;
    }

    // (W3-1) Build the from-import local-binding map: `from X import f` records
    // `f -> ("X", "f")` in the IMPORTING module's scope (the root keyed under
    // ROOT_MODULE_ID; every other module under its own dotted id). Owner-first bare
    // resolution consults this before the root-locals; stage 2 consumes it to
    // owner-qualify a bare from-imported call. This stage only makes it REAL and
    // queryable — emission stays flat. The `dataclasses` skip-list is mirrored so
    // its decorator-only import is excluded exactly as in the resolution loop.
    for (idx, path) in resolver.order.iter().enumerate() {
        let (m, src) = &resolver.cache[path];
        let importer: String = if idx + 1 == total_modules {
            crate::typeck::ROOT_MODULE_ID.to_string()
        } else {
            m.module_id.clone().unwrap_or_else(|| crate::typeck::ROOT_MODULE_ID.to_string())
        };
        for s in &m.stmts {
            if let Stmt::Import { path: imp_path, names, span } = s {
                // A plain `import X` (empty `names`) binds no LOCAL name — only
                // `from X import f, g` records local bindings.
                if names.is_empty() {
                    continue;
                }
                if matches!(imp_path.first().map(String::as_str), Some("dataclasses")) {
                    continue;
                }
                // (W3-3, was F16) The binding OWNER is the FULL dotted module id the
                // resolver registered this import under (`imp_path.join(".")`) — the
                // same id `process_module` stamped onto the module and
                // `merge_ctx_from_module` keyed its per-module table by. The old
                // `path[0]` truncation bound `from os.path import join` to owner `os`
                // (silently binding `os.join`); the full id binds it to `os.path`, so
                // the bare `join` mangles to `os.path`'s namespace — matching its def
                // (or, when `os.path` has no `join`, caught by the validation below).
                // Byte-identical for a single-component import (`join(".") == path[0]`).
                let owner = imp_path.join(".");

                // (W3-3) DOTTED from-import NAME validation — the honest death of the
                // from-import truncation. `from os.path import join` must NEVER fall
                // back to `os.join`: if the (real, resolved) submodule `os.path` does
                // not export `join`, that is an honest check error here, not a bare
                // `join()` that flat-resolves to a co-imported `os`'s `join` and then
                // dies at rustc as an undefined `__pyrst_m_os_dpath__join` (E0425).
                // SCOPED to dotted owners: a single-component `from os import getenv`
                // keeps its exact prior behaviour (no new validation, zero regression
                // risk) — truncation only ever affected dotted from-imports. The
                // owner is always in `module_symbols` once resolved (merge records
                // every non-root module's own funcs/classes/consts); if it somehow is
                // not, we stay lenient rather than reject.
                if owner.contains('.') {
                    if let Some(syms) = ctx.module_symbols.get(&owner) {
                        for (local, _alias) in names {
                            let exported = syms.funcs.contains_key(local)
                                || syms.classes.contains_key(local)
                                || syms.consts.contains_key(local);
                            if !exported {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!(
                                        "cannot import name `{}` from submodule `{}` \
                                         (it exports no top-level function, class, or \
                                         constant named `{}`); a dotted `from {} import \
                                         {}` never falls back to the parent module",
                                        local, owner, local, owner, local
                                    ),
                                })
                                .map_err(|e| e.with_render_source(m.source_path.clone(), src));
                            }
                        }
                    }
                }

                for (local, _alias) in names {
                    // Aliases are rejected at parse, so the local name IS the
                    // imported name; record `local -> (owner, original)`.
                    ctx.import_bindings
                        .entry(importer.clone())
                        .or_default()
                        .insert(local.clone(), (owner.clone(), local.clone()));
                }
            }
        }
    }

    // Build output: (Module, source_text) pairs in dependency order
    let modules: Vec<(Module, String)> = resolver
        .order
        .iter()
        .map(|p| {
            let (m, src) = resolver.cache[p].clone();
            (m, src)
        })
        .collect();

    // (enabler-fix-1 #3) Finalize class-constant promotion over the WHOLE program
    // (needs every class registered AND every read/write site across all modules),
    // so typeck and codegen share one usage-gated decision.
    crate::typeck::collect_promoted_consts(&modules, &mut ctx);

    // (W4-a) Compute the whole-program MUTABLE-GLOBAL set (module-level bindings
    // promoted to `thread_local!` statics — global+rebind OR non-scalar-literal
    // initializer), the single source of truth typeck's promotion/trap-exclusion
    // and codegen's static/read/write emission share. Empty for programs with no
    // module-level mutable state, so the const path stays byte-identical.
    crate::typeck::collect_mutable_globals(&modules, &mut ctx);

    // (enabler-fix-2 #1a/#1c) Close `hash_key_classes` over the WHOLE program:
    // TRANSITIVE user-class fields of a key class (nested classes need the derive
    // too) + annotation-less constructor-keyed dict/set literals. Runs after every
    // module's annotation scan has accumulated into `ctx.hash_key_classes`.
    crate::typeck::finalize_hash_key_classes(&modules, &mut ctx);

    Ok(ResolvedProgram { modules, ctx })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeck::Ty;

    /// EPIC-4 V2-c STEP 0 regression: a method whose FIRST real param is `Mut[T]`
    /// must register with `param_by_ref[0] == true` aligned to the FIRST REAL
    /// param (`amt`), NOT to the implicit `self` slot. Before the fix, `params`
    /// kept `self` (so `params[0]` was `self` and `params[1]` was `amt`) while
    /// `param_by_ref` dropped `self` (so `param_by_ref[0]` described `amt`) — an
    /// off-by-one that would mis-assign the by-ref flag once method call sites
    /// began reading it. After the fix all three vectors are self-exclusive and
    /// index-aligned.
    #[test]
    fn method_param_by_ref_aligns_to_first_real_param_not_self() {
        let src = "\
class Bank:
    total: int
    def __init__(self, total: int) -> None:
        self.total = total
    def transfer(self, amt: Mut[Account], note: int) -> None:
        pass

class Account:
    balance: int
    def __init__(self, balance: int) -> None:
        self.balance = balance
";
        let m = crate::parser::parse(src).expect("parse");
        let mut ctx = TyCtx::new();
        merge_ctx_from_module(&m, &mut ctx, true).expect("merge");

        let sig = ctx.funcs.get("Bank.transfer").expect("Bank.transfer registered");

        // Self is filtered from params: exactly the two REAL params remain, in order.
        assert_eq!(
            sig.params.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            vec!["amt", "note"],
            "params must exclude self and keep real params in order"
        );
        // All three parallel vectors share length N (the real-param count).
        assert_eq!(sig.params.len(), 2);
        assert_eq!(sig.param_by_ref.len(), 2, "param_by_ref must align to params (self-exclusive)");
        assert_eq!(sig.param_defaults.len(), 2, "param_defaults must align to params (self-exclusive)");

        // The Mut[T] flag maps to the FIRST REAL param (`amt`), not the self slot.
        assert!(sig.param_by_ref[0], "amt (first real param) is Mut[T] -> by_ref at index 0");
        assert!(!sig.param_by_ref[1], "note (second real param) is by-value");

        // And `amt`'s type is the UNWRAPPED inner T (Account), not a Mut wrapper.
        assert!(matches!(sig.params[0].1, Ty::Class(ref c, _) if c == "Account"),
            "Mut[Account] param's type is the inner Account");
    }

    /// Write `src` to a uniquely-named temp `.pyrs` file in a fresh temp dir and
    /// return its path. The dir is created so the file has no sibling modules
    /// (so a `from os import …` cannot accidentally resolve to a LOCAL `os.pyrs`
    /// and must go through the embedded stdlib).
    fn temp_root(src: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pyrst-resolver-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("main.pyrs");
        std::fs::write(&path, src).unwrap();
        path
    }

    /// Card f5cd1d10 acceptance: `from os import getenv` resolves through the
    /// EMBEDDED stdlib (there is no local `os.pyrs`). The embedded `os` module
    /// must parse, appear in the resolved program's modules, and have its
    /// `@extern` `getenv` signature merged into the shared ctx so call sites
    /// type-check.
    #[test]
    fn embedded_os_module_resolves_and_merges() {
        let root = temp_root("from os import getenv\n\ndef main() -> None:\n    print(getenv(\"X\", \"y\"))\n");
        let prog = resolve(&root).expect("`from os import getenv` must resolve via the embedded stdlib");

        // The embedded `getenv` binding is registered in the merged ctx (so a
        // call site type-checks) with its declared (str, str) -> str signature.
        let sig = prog.ctx.funcs.get("getenv").expect("embedded os::getenv must be registered in ctx");
        assert!(matches!(sig.ret, Ty::Str), "getenv returns str");
        assert_eq!(sig.params.len(), 2, "getenv(key, default)");

        // The embedded `os` module is part of the program, carrying a synthetic
        // `<stdlib>/os.pyrs` source path for diagnostics/caching, and comes
        // BEFORE the root (dependency-first topological order).
        let os_mod = prog.modules.iter().find(|(m, _)| {
            m.source_path.as_ref().map_or(false, |p| p.ends_with("os.pyrs"))
        });
        assert!(os_mod.is_some(), "the embedded os module must appear in the resolved modules");
        assert_eq!(prog.modules.len(), 2, "exactly the root + the embedded os module");

        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// Card 81db88e0: qualified module calls. `import os` must make the embedded
    /// `os` module's functions QUALIFIABLE by name — i.e. `ctx.module_funcs["os"]`
    /// lists the module's top-level functions (so `os.basename(...)` resolves),
    /// while the FLAT `ctx.funcs` still holds the actual signatures. The ROOT
    /// program's own functions must NOT appear in `module_funcs` (you don't write
    /// `root.f()`).
    #[test]
    fn imported_module_functions_are_qualifiable_root_is_not() {
        let root = temp_root("import os\n\ndef main() -> None:\n    print(os.basename(\"/a/b.txt\"))\n");
        let prog = resolve(&root).expect("`import os` must resolve via the embedded stdlib");

        // The embedded os module is registered as a qualifiable module name, and
        // its functions are listed (so `os.basename` / `os.getenv` resolve).
        let os_fns = prog.ctx.module_funcs.get("os")
            .expect("os must be a tracked qualifiable module");
        assert!(os_fns.iter().any(|n| n == "basename"), "os.basename must be qualifiable");
        assert!(os_fns.iter().any(|n| n == "getenv"), "os.getenv must be qualifiable");

        // The flat signature still lives in ctx.funcs (codegen emits the flat name).
        assert!(prog.ctx.funcs.contains_key("basename"), "flat basename sig must exist");

        // The ROOT module name is not a qualifiable module: only imported modules
        // are keyed in module_funcs, and `main` is the root's own function.
        for (_mod, fns) in &prog.ctx.module_funcs {
            assert!(!fns.iter().any(|n| n == "main"), "root main() must not be a qualifiable module function");
        }

        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// Negative: importing a module that is neither a local file nor an embedded
    /// stdlib module is rejected with `ImportNotFound` (the embedded lookup must
    /// not make unknown imports silently resolve).
    #[test]
    fn unknown_import_is_not_found() {
        let root = temp_root("from notamodule import x\n\ndef main() -> None:\n    print(x())\n");
        // `ResolvedProgram` is not `Debug`, so match the Result directly rather
        // than `expect_err` (which would require `Ok` to be printable).
        match resolve(&root) {
            Ok(_) => panic!("an unknown module must not resolve"),
            Err(Error::ImportNotFound { path, .. }) => {
                assert_eq!(path, "notamodule", "ImportNotFound must name the missing module");
            }
            Err(other) => panic!("expected ImportNotFound for `notamodule`, got: {:?}", other),
        }
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// (W3-2) The KEYSTONE payoff: co-importing two modules that each define the
    /// same top-level FUNCTION (`operator` and `re` both `def sub`) now RESOLVES —
    /// per-module namespaced emission (`__pyrst_m_operator__sub` /
    /// `__pyrst_m_re__sub`) dissolves the flat collision. The former card-6c8b4a39
    /// stopgap rejected this; W3-2 narrows the stopgap to class-vs-class only.
    #[test]
    fn cross_module_function_collision_is_now_coimportable() {
        let root = temp_root(
            "import operator\nimport re\n\ndef main() -> None:\n    print(operator.sub(5, 3))\n    print(re.sub(\"a\", \"b\", \"banana\"))\n",
        );
        assert!(
            resolve(&root).is_ok(),
            "co-importing operator and re (both define `sub`) must resolve under per-module namespacing"
        );
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// (W3-2) The narrowed stopgap STILL rejects a class-vs-class name collision:
    /// two local modules each defining `class Point` cannot co-import (a class type
    /// is carried as a bare `Ty::Class("Point")` with no owner, so `rust_ty` cannot
    /// pick which owner to mangle — v1 keeps class names globally unique). Written
    /// as sibling local modules since NO stdlib pair is class-vs-class.
    #[test]
    fn cross_module_class_collision_is_rejected() {
        // The root just co-imports both modules; the class-vs-class collision is
        // detected purely from the two imported modules each defining `Point`
        // (independent of how the root uses them).
        let root = temp_root(
            "import geo_a\nimport geo_b\n\ndef main() -> None:\n    print(1)\n",
        );
        let dir = root.parent().unwrap();
        std::fs::write(
            dir.join("geo_a.pyrs"),
            "class Point:\n    x: int\n    y: int\n    def __init__(self, x: int, y: int) -> None:\n        self.x = x\n        self.y = y\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("geo_b.pyrs"),
            "class Point:\n    px: int\n    py: int\n    def __init__(self, px: int, py: int) -> None:\n        self.px = px\n        self.py = py\n",
        )
        .unwrap();
        match resolve(&root) {
            Ok(_) => panic!("co-importing two modules that each define `class Point` must be rejected"),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("Point") && msg.contains("geo_a") && msg.contains("geo_b"),
                    "class-collision error must name the class and both modules, got: {msg}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The collision guard must NOT over-reject: importing ONLY `re` (its `sub`
    /// uncontested) resolves cleanly, and `re.sub` is registered.
    #[test]
    fn single_import_of_a_colliding_module_still_resolves() {
        let root = temp_root(
            "import re\n\ndef main() -> None:\n    print(re.sub(\"a\", \"b\", \"banana\"))\n",
        );
        let prog = resolve(&root).expect("importing `re` alone must resolve");
        assert!(prog.ctx.funcs.contains_key("sub"), "re.sub must be registered");
    }

    /// A ROOT (user-file) top-level `def` that shadows an imported name is
    /// PRESERVED (last-write-wins, root shadows import) — only imported×imported
    /// duplicates are a collision. Here the root defines its own `sub` while
    /// importing `re` (which also defines `sub`); resolution must succeed.
    #[test]
    fn root_def_shadowing_imported_name_is_allowed() {
        let root = temp_root(
            "import re\n\ndef sub(a: int, b: int) -> int:\n    return a - b\n\ndef main() -> None:\n    print(sub(5, 3))\n",
        );
        assert!(
            resolve(&root).is_ok(),
            "a root def may shadow an imported name (only imported x imported collides)"
        );
    }

    // ── W3-1: per-module symbol tables + owner-first qualified resolution ──────

    /// W3-1: `import os` stamps the embedded module with the DOTTED module id from
    /// the import path (`"os"`), builds a REAL per-module table keyed by that id,
    /// and records the owner index; the ROOT carries no module id.
    #[test]
    fn w3_import_builds_dotted_module_id_and_owner_maps() {
        let root = temp_root("import os\n\ndef main() -> None:\n    print(os.basename(\"/a/b.txt\"))\n");
        let prog = resolve(&root).expect("`import os` resolves via the embedded stdlib");

        // The os module carries module_id = "os" (from the import path, not a stem
        // coincidence); the root is the sentinel with no dotted id.
        assert!(
            prog.modules.iter().any(|(m, _)| m.module_id.as_deref() == Some("os")),
            "os module carries the dotted module_id \"os\""
        );
        let (root_mod, _) = prog.modules.last().expect("root is last in topological order");
        assert_eq!(root_mod.module_id, None, "the root program has no dotted module id");

        // Real per-module table, keyed by the dotted id, holds os's own functions.
        let os_syms = prog.ctx.module_symbols.get("os").expect("os has a per-module symbol table");
        assert!(os_syms.funcs.contains_key("basename"), "os.basename lives in os's per-module table");
        assert!(os_syms.funcs.contains_key("getenv"), "os.getenv lives in os's per-module table");
        // Owner index maps the bare definition name back to its owning dotted id.
        assert_eq!(prog.ctx.func_owner.get("basename").map(String::as_str), Some("os"));

        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// W3-1: owner-first qualified function/const resolution — a real member HITS
    /// the owning module's table; a non-member (with no flat/builtin entry) MISSES
    /// to `None` rather than fabricating a signature.
    #[test]
    fn w3_qualified_resolution_owner_first_hit_and_miss() {
        let root = temp_root("import os\n\ndef main() -> None:\n    print(os.getenv(\"X\", \"y\"))\n");
        let prog = resolve(&root).expect("`import os` resolves");

        // HIT: os.getenv resolves owner-first to os's declared (str, str) -> str sig.
        let hit = prog.ctx.resolve_module_func("os", "getenv").expect("os.getenv hits its owner table");
        assert_eq!(hit.params.len(), 2, "os.getenv(key, default)");
        assert!(matches!(hit.ret, Ty::Str), "os.getenv returns str");

        // MISS: a name os does not define (and that is not a builtin/flat entry)
        // resolves to None through both helpers.
        assert!(
            prog.ctx.resolve_module_func("os", "no_such_os_function").is_none(),
            "a qualified function miss must not fabricate a signature"
        );
        assert!(
            prog.ctx.resolve_module_const("os", "NO_SUCH_CONST").is_none(),
            "a qualified const miss must be None"
        );

        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// W3-1: `from X import f` binds the LOCAL name to `(owner, original)` in the
    /// importing file's scope (root under `ROOT_MODULE_ID`); a plain `import X`
    /// binds no local name.
    #[test]
    fn w3_from_import_records_local_binding() {
        let root = temp_root("from os import getenv\n\ndef main() -> None:\n    print(getenv(\"X\", \"y\"))\n");
        let prog = resolve(&root).expect("`from os import getenv` resolves");

        let root_scope = prog
            .ctx
            .import_bindings
            .get(crate::typeck::ROOT_MODULE_ID)
            .expect("the root has a from-import binding scope");
        assert_eq!(
            root_scope.get("getenv"),
            Some(&("os".to_string(), "getenv".to_string())),
            "`from os import getenv` binds getenv -> (\"os\", \"getenv\")"
        );

        // A plain `import os` in a separate program records NO local binding.
        let plain = temp_root("import os\n\ndef main() -> None:\n    print(os.getenv(\"X\", \"y\"))\n");
        let plain_prog = resolve(&plain).expect("`import os` resolves");
        let has_local = plain_prog
            .ctx
            .import_bindings
            .get(crate::typeck::ROOT_MODULE_ID)
            .is_some_and(|s| s.contains_key("getenv"));
        assert!(!has_local, "plain `import os` must not create a from-import local binding");

        let _ = std::fs::remove_dir_all(root.parent().unwrap());
        let _ = std::fs::remove_dir_all(plain.parent().unwrap());
    }

    /// W3-1: shadow precedence (documented root-over-import). A root `def basename`
    /// (3-arg) shadows the imported os.basename (1-arg) in the FLAT / bare
    /// namespace, but the OWNER-FIRST qualified path still resolves os's real
    /// signature from os's per-module table, unpolluted by the root shadow.
    #[test]
    fn w3_root_shadow_flat_but_owner_first_resolves_module() {
        let root = temp_root(
            "import os\n\ndef basename(a: int, b: int, c: int) -> int:\n    return a + b + c\n\ndef main() -> None:\n    print(basename(1, 2, 3))\n",
        );
        let prog = resolve(&root).expect("a root def may shadow an imported name");

        // Flat / bare namespace: the ROOT's 3-arg basename wins (root merged last).
        let flat = prog.ctx.funcs.get("basename").expect("flat basename registered");
        assert_eq!(flat.params.len(), 3, "root's 3-arg basename shadows the flat table");
        assert!(matches!(flat.ret, Ty::Int), "root's basename returns int");

        // Owner index + per-module table still point at os's real 1-arg definition.
        assert_eq!(
            prog.ctx.func_owner.get("basename").map(String::as_str),
            Some("os"),
            "the DEFINING owner of basename is os (root shadows are not recorded as owners)"
        );
        let qualified = prog.ctx.resolve_module_func("os", "basename").expect("os.basename resolves owner-first");
        assert_eq!(qualified.params.len(), 1, "os.basename is 1-arg — owner-first ignores the root shadow");
        assert!(matches!(qualified.ret, Ty::Str), "os.basename returns str");

        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// W3-1: the KEYSTONE table-level property — two co-imported modules that each
    /// define `sub` with DIFFERENT signatures coexist in the per-module tables and
    /// resolve via their OWN qualified path, even though the flat table (and thus
    /// stage-1 emission) can hold only one. This is a pure table/helper unit test:
    /// it never runs the resolver, so the emission-level collision stopgap is
    /// neither exercised nor relaxed — retiring it is stage 2's job.
    #[test]
    fn w3_same_name_two_modules_resolve_via_each_qualified_path() {
        use crate::typeck::{FuncSig, ModuleSymbols};
        let mut ctx = TyCtx::new();

        // operator.sub(a: int, b: int) -> int
        let op_sub = FuncSig {
            params: vec![("a".into(), Ty::Int), ("b".into(), Ty::Int)],
            ret: Ty::Int,
            param_defaults: vec![None, None],
            param_by_ref: vec![false, false],
        };
        // re.sub(pattern: str, repl: str, s: str) -> str
        let re_sub = FuncSig {
            params: vec![("pattern".into(), Ty::Str), ("repl".into(), Ty::Str), ("s".into(), Ty::Str)],
            ret: Ty::Str,
            param_defaults: vec![None, None, None],
            param_by_ref: vec![false, false, false],
        };

        let mut operator = ModuleSymbols::default();
        operator.funcs.insert("sub".into(), op_sub);
        ctx.module_symbols.insert("operator".into(), operator);
        let mut re = ModuleSymbols::default();
        re.funcs.insert("sub".into(), re_sub);
        ctx.module_symbols.insert("re".into(), re);

        // The FLAT table can hold only ONE `sub` — leave it empty to prove
        // owner-first resolution does not depend on it.
        assert!(!ctx.funcs.contains_key("sub"), "flat table intentionally holds no `sub`");

        let a = ctx.resolve_module_func("operator", "sub").expect("operator.sub resolves via its own table");
        let b = ctx.resolve_module_func("re", "sub").expect("re.sub resolves via its own table");
        // Each qualified path resolves to its OWN module's distinct signature.
        assert_eq!(a.params.len(), 2, "operator.sub is binary");
        assert!(matches!(a.ret, Ty::Int), "operator.sub returns int");
        assert_eq!(b.params.len(), 3, "re.sub is ternary");
        assert!(matches!(b.ret, Ty::Str), "re.sub returns str");
    }

    // ── W3-3: dotted-import resolution + embedded packages ────────────────────

    /// W3-3: `import os.path` resolves the EMBEDDED submodule (`lib/os/path.pyrs`,
    /// dotted key `"os.path"`) under the FULL dotted module id — NOT truncated to
    /// `os`. Its per-module table + qualifier index are keyed by the dotted id, and
    /// `os` itself is NOT loaded (explicit-import-required: `import os.path` does
    /// not pull in the parent module).
    #[test]
    fn w3_dotted_embedded_submodule_resolves_under_full_id() {
        let root = temp_root("import os.path\n\ndef main() -> None:\n    print(os.path.basename(\"/a/b.txt\"))\n");
        let prog = resolve(&root).expect("`import os.path` resolves via the embedded stdlib");

        // The submodule carries the DOTTED id "os.path" (not the ambiguous stem
        // "path", and not the truncated "os").
        assert!(
            prog.modules.iter().any(|(m, _)| m.module_id.as_deref() == Some("os.path")),
            "the submodule carries module_id \"os.path\""
        );
        // Its per-module table + qualifier index are keyed by the dotted id.
        let syms = prog.ctx.module_symbols.get("os.path").expect("os.path has a per-module table");
        assert!(syms.funcs.contains_key("basename"), "os.path.basename lives in os.path's table");
        assert!(
            prog.ctx.module_funcs.get("os.path").is_some_and(|fns| fns.iter().any(|n| n == "basename")),
            "os.path.basename is qualifiable under the dotted id"
        );
        // Explicit-import-required: `import os.path` does NOT load the parent `os`.
        assert!(
            !prog.ctx.module_symbols.contains_key("os"),
            "`import os.path` must NOT auto-load the parent `os` module"
        );
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// W3-3: the death of the `path[0]` truncation. An unresolved dotted import is
    /// an honest `ImportNotFound` naming the FULL dotted id — even when the PARENT
    /// resolves (`import os.nonexistent`) and when the parent does not
    /// (`import nope.whatever`). It must NEVER silently resolve to the parent.
    #[test]
    fn w3_unresolved_dotted_import_names_full_id() {
        // Parent `os` is real, submodule is not: names the submodule, not `os`.
        let root = temp_root("import os.nonexistent\n\ndef main() -> None:\n    print(1)\n");
        match resolve(&root) {
            Err(Error::ImportNotFound { path, .. }) => {
                assert_eq!(path, "os.nonexistent", "ImportNotFound must name the FULL dotted id");
            }
            other => panic!("expected ImportNotFound(os.nonexistent), got: {:?}", other.map(|_| ())),
        }
        let _ = std::fs::remove_dir_all(root.parent().unwrap());

        // Parent also unresolved: names the full dotted id too.
        let root2 = temp_root("import nope.whatever\n\ndef main() -> None:\n    print(1)\n");
        match resolve(&root2) {
            Err(Error::ImportNotFound { path, .. }) => {
                assert_eq!(path, "nope.whatever");
            }
            other => panic!("expected ImportNotFound(nope.whatever), got: {:?}", other.map(|_| ())),
        }
        let _ = std::fs::remove_dir_all(root2.parent().unwrap());
    }

    /// W3-3: LOCAL user package via DIRECTORY layout — `import pkg.geo` resolves the
    /// on-disk `pkg/geo.pyrs` relative to the importing file, under the dotted id
    /// `"pkg.geo"`, with its functions qualifiable.
    #[test]
    fn w3_local_package_directory_layout_resolves() {
        let root = temp_root("import pkg.geo\n\ndef main() -> None:\n    print(pkg.geo.area(3, 4))\n");
        let dir = root.parent().unwrap();
        std::fs::create_dir_all(dir.join("pkg")).unwrap();
        std::fs::write(
            dir.join("pkg").join("geo.pyrs"),
            "def area(w: int, h: int) -> int:\n    return w * h\n",
        )
        .unwrap();

        let prog = resolve(&root).expect("`import pkg.geo` resolves the local package file pkg/geo.pyrs");
        assert!(
            prog.modules.iter().any(|(m, _)| m.module_id.as_deref() == Some("pkg.geo")),
            "the local submodule carries module_id \"pkg.geo\""
        );
        assert!(
            prog.ctx.module_funcs.get("pkg.geo").is_some_and(|fns| fns.iter().any(|n| n == "area")),
            "pkg.geo.area is qualifiable under the dotted id"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// W3-3 (was F16): `from a.b import f` binds the FULL dotted owner `"a.b"` (not
    /// the truncated `"a"`), so the bare `f` mangles into the submodule's namespace
    /// and matches its definition.
    #[test]
    fn w3_dotted_from_import_binds_full_owner() {
        let root = temp_root("from os.path import basename\n\ndef main() -> None:\n    print(basename(\"/a/b.txt\"))\n");
        let prog = resolve(&root).expect("`from os.path import basename` resolves");
        let root_scope = prog
            .ctx
            .import_bindings
            .get(crate::typeck::ROOT_MODULE_ID)
            .expect("the root has a from-import binding scope");
        assert_eq!(
            root_scope.get("basename"),
            Some(&("os.path".to_string(), "basename".to_string())),
            "`from os.path import basename` binds basename -> owner \"os.path\" (NOT truncated to \"os\")"
        );
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// W3-3 (probe retargeted in W3-4, card a7488511): a DOTTED from-import naming
    /// a symbol the submodule does not export is an honest check error — it must
    /// NEVER fall back to the PARENT module. Here `os` (imported too) DOES define
    /// `listdir`, but `os.path` does not (and deliberately never should — it is a
    /// path-string module, not a filesystem-listing one), so
    /// `from os.path import listdir` is rejected rather than silently binding
    /// `os.listdir` (the from-import truncation the stage kills). The probe was
    /// originally `join`, but W3-4 gave `os.path` a real faithful `join`, so that
    /// name no longer tests the invariant.
    #[test]
    fn w3_dotted_from_import_unknown_name_is_rejected_not_truncated() {
        let root = temp_root(
            "import os\nfrom os.path import listdir\n\ndef main() -> None:\n    print(listdir(\".\"))\n",
        );
        match resolve(&root) {
            Ok(_) => {
                panic!("`from os.path import listdir` must be rejected (os.path has no `listdir`)")
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("listdir") && msg.contains("os.path"),
                    "error must name the missing symbol and the submodule, got: {msg}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }
}
