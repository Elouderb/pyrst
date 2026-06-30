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
    fn visit(&mut self, abs_path: PathBuf, base_dir: &Path, import_span: Span) -> Result<()> {
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

        self.process_module(abs_path, src, base_dir, import_span)
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
    fn process_module(
        &mut self,
        key: PathBuf,
        src: String,
        base_dir: &Path,
        import_span: Span,
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

        // Recurse into this module's imports BEFORE adding self to order (post-order DFS)
        for stmt in &m.stmts {
            if let Stmt::Import { path, span, .. } = stmt {
                // Phase 8: only use first component of dotted path (same-directory imports only)
                let mod_name = &path[0];

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
                // never skipped and resolve as embedded modules too.)
                if matches!(mod_name.as_str(), "dataclasses" | "sys") {
                    continue;
                }

                // Resolution order: a LOCAL `<base_dir>/<mod>.pyrs` on disk
                // SHADOWS an embedded stdlib module of the same name.
                let dep_path = base_dir.join(format!("{}.pyrs", mod_name));
                if let Ok(dep_abs) = dep_path.canonicalize() {
                    // Local file found.
                    self.visit(dep_abs, base_dir, *span)?;
                } else if let Some(embedded_src) = crate::stdlib::lookup(mod_name) {
                    // No local file → resolve from the EMBEDDED stdlib. Use a
                    // synthetic `<stdlib>/<mod>.pyrs` key so caching, cycle
                    // detection, and diagnostics all work like a file module, and
                    // resolve the embedded module's OWN imports against the same
                    // synthetic stdlib dir (local-then-embedded, recursively).
                    let stdlib_dir = stdlib_synthetic_dir();
                    let dep_key = stdlib_dir.join(format!("{}.pyrs", mod_name));
                    let dep_src = crate::lexer::normalize_line_endings(embedded_src);
                    self.process_module(dep_key, dep_src, &stdlib_dir, *span)?;
                } else {
                    // Neither a local file nor an embedded module exists.
                    return Err(Error::ImportNotFound {
                        path: mod_name.clone(),
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
    let module_name: Option<String> = if is_root {
        None
    } else {
        m.source_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
    };

    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
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
                    .map(|p| crate::typeck::Ty::from_type_expr_scoped(&p.ty, p.span, &f.type_params).map(|ty| (p.name.clone(), ty)))
                    .collect::<crate::diag::Result<Vec<_>>>()?;
                ctx.funcs.insert(f.name.clone(), crate::typeck::FuncSig {
                    params,
                    // A return annotation carries no span of its own; point at the
                    // function definition so a bad `-> ...` still gets a real caret.
                    ret: crate::typeck::Ty::from_type_expr_scoped(&f.ret, f.span, &f.type_params)?,
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
                });
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
                ctx.classes.insert(c.name.clone(), c.clone());
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
                        .map(|p| crate::typeck::Ty::from_type_expr_scoped(&p.ty, p.span, &c.type_params).map(|ty| (p.name.clone(), ty)))
                        .collect::<crate::diag::Result<Vec<_>>>()?;
                    ctx.funcs.insert(method_name, crate::typeck::FuncSig {
                        params: method_params,
                        ret: crate::typeck::Ty::from_type_expr_scoped(&m_fn.ret, m_fn.span, &c.type_params)?,
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
    Ok(())
}

/// Resolve all imports from a root .pyrs file and return a merged program.
pub fn resolve(root_path: &Path) -> Result<ResolvedProgram> {
    let abs_root = root_path.canonicalize().map_err(|e| crate::diag::Error::Io(e))?;
    let root_dir = abs_root.parent().unwrap_or_else(|| Path::new("."));

    let mut resolver = Resolver::new();
    resolver.visit(abs_root.clone(), root_dir, Span::DUMMY)?;

    // Build merged context from all modules in dependency order
    let mut ctx = TyCtx::new();
    let total_modules = resolver.order.len();
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

    // Build output: (Module, source_text) pairs in dependency order
    let modules = resolver
        .order
        .iter()
        .map(|p| {
            let (m, src) = resolver.cache[p].clone();
            (m, src)
        })
        .collect();

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
}
