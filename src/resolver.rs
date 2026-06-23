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

    /// DFS traversal of the import graph starting from `root_path`.
    /// After completion, `self.order` is in topological order.
    fn visit(&mut self, abs_path: PathBuf, base_dir: &Path, import_span: Span) -> Result<()> {
        // Already processed (diamond import case)
        if self.order.contains(&abs_path) {
            return Ok(());
        }

        // Circular import detection
        if self.in_flight.contains(&abs_path) {
            // Reconstruct cycle path
            let cycle_start = self.dfs_stack.iter().position(|p| p == &abs_path).unwrap_or(0);
            let cycle: Vec<String> = self.dfs_stack[cycle_start..]
                .iter()
                .chain(&[abs_path.clone()])
                .map(|p| p.file_stem().unwrap_or_default().to_string_lossy().to_string())
                .collect();
            return Err(Error::CircularImport { cycle, span: import_span });
        }

        // Mark as in-flight
        self.in_flight.insert(abs_path.clone());
        self.dfs_stack.push(abs_path.clone());

        // Load and parse this file
        let src = std::fs::read_to_string(&abs_path).map_err(|_| {
            let importing_file = abs_path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            Error::ImportNotFound {
                path: importing_file,
                span: import_span,
                importing_file: base_dir.display().to_string(),
            }
        })?;

        // EPIC-8: a parse error belongs to THIS file — pair its own source (and
        // path) so the snippet renders against the imported file, not the root.
        // Name the file ONLY when it was reached via an import (DFS depth > 1):
        // a parse error in the ROOT of a single-file program must render exactly
        // as before (no `in <file>` suffix), so the source is attached but the
        // file name is withheld for the root.
        let name_file = self.dfs_stack.len() > 1;
        let render_path = if name_file { Some(abs_path.clone()) } else { None };
        let mut m = crate::parser::parse(&src)
            .map_err(|e| e.with_render_source(render_path, &src))?;
        m.source_path = Some(abs_path.clone());

        // Recurse into this module's imports BEFORE adding self to order (post-order DFS)
        for stmt in &m.stmts {
            if let Stmt::Import { path, span, .. } = stmt {
                // Phase 8: only use first component of dotted path (same-directory imports only)
                let mod_name = &path[0];

                // Skip standard library modules (math, dataclasses, etc.)
                if matches!(mod_name.as_str(), "math" | "dataclasses" | "sys" | "os" | "json" | "re" | "collections" | "itertools") {
                    continue;
                }

                let dep_path = base_dir.join(format!("{}.py", mod_name));

                // Resolve to absolute path
                let dep_abs = dep_path.canonicalize().map_err(|_| Error::ImportNotFound {
                    path: mod_name.clone(),
                    span: *span,
                    importing_file: abs_path.display().to_string(),
                })?;

                self.visit(dep_abs, base_dir, *span)?;
            }
        }

        // Post-order: add to order AFTER all dependencies are processed
        self.dfs_stack.pop();
        self.in_flight.remove(&abs_path);
        self.cache.insert(abs_path.clone(), (m, src));
        self.order.push(abs_path);
        Ok(())
    }
}

/// Merge function/class signatures from a single module into a context.
fn merge_ctx_from_module(m: &Module, ctx: &mut TyCtx, is_root: bool) -> Result<()> {
    for s in &m.stmts {
        match s {
            Stmt::Func(f) => {
                // Skip main() from non-root modules
                if f.name == "main" && !is_root {
                    continue;
                }
                let params: Vec<(String, crate::typeck::Ty)> = f.params.iter()
                    .map(|p| crate::typeck::Ty::from_type_expr(&p.ty, p.span).map(|ty| (p.name.clone(), ty)))
                    .collect::<crate::diag::Result<Vec<_>>>()?;
                ctx.funcs.insert(f.name.clone(), crate::typeck::FuncSig {
                    params,
                    // A return annotation carries no span of its own; point at the
                    // function definition so a bad `-> ...` still gets a real caret.
                    ret: crate::typeck::Ty::from_type_expr(&f.ret, f.span)?,
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
            }
            Stmt::Class(c) => {
                let mut c = c.clone();
                crate::typeck::extract_init_fields(&mut c);
                ctx.classes.insert(c.name.clone(), c.clone());
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
                    let method_params: Vec<(String, crate::typeck::Ty)> = m_fn.params.iter()
                        .filter(|p| p.name != "self")
                        .map(|p| crate::typeck::Ty::from_type_expr(&p.ty, p.span).map(|ty| (p.name.clone(), ty)))
                        .collect::<crate::diag::Result<Vec<_>>>()?;
                    ctx.funcs.insert(method_name, crate::typeck::FuncSig {
                        params: method_params,
                        ret: crate::typeck::Ty::from_type_expr(&m_fn.ret, m_fn.span)?,
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
            Stmt::Assign { target, ty: Some(t), span, .. } => {
                // Register module-level annotated globals with their concrete type
                // (ported from typeck::check_module). Propagate errors so that
                // invalid annotations (e.g. set[float]) are rejected at typeck.
                let resolved = crate::typeck::Ty::from_type_expr(t, *span)?;
                ctx.vars.insert(target.clone(), resolved);
            }
            Stmt::Import { .. } => {}
            _ => {}
        }
    }
    Ok(())
}

/// Resolve all imports from a root .py file and return a merged program.
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
        assert!(matches!(sig.params[0].1, Ty::Class(ref c) if c == "Account"),
            "Mut[Account] param's type is the inner Account");
    }
}
