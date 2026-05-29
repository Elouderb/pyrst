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
            let mut cycle: Vec<String> = self.dfs_stack[cycle_start..]
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

        let mut m = crate::parser::parse(&src)?;
        m.source_path = Some(abs_path.clone());

        // Recurse into this module's imports BEFORE adding self to order (post-order DFS)
        for stmt in &m.stmts {
            if let Stmt::Import { path, span, .. } = stmt {
                // Phase 8: only use first component of dotted path (same-directory imports only)
                let mod_name = &path[0];
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
                ctx.funcs.insert(f.name.clone(), crate::typeck::FuncSig {
                    params: f.params.iter().map(|p| (p.name.clone(), crate::typeck::Ty::Unknown)).collect(),
                    ret: crate::typeck::Ty::Unknown,
                });
            }
            Stmt::Class(c) => {
                ctx.classes.insert(c.name.clone(), c.clone());
                // Register method signatures
                for m_fn in &c.methods {
                    let method_name = format!("{}.{}", c.name, m_fn.name);
                    ctx.funcs.insert(method_name, crate::typeck::FuncSig {
                        params: m_fn.params.iter().map(|p| (p.name.clone(), crate::typeck::Ty::Unknown)).collect(),
                        ret: crate::typeck::Ty::Unknown,
                    });
                }
            }
            Stmt::Assign { target, ty: Some(_), .. } => {
                ctx.vars.insert(target.clone(), crate::typeck::Ty::Unknown);
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
        let (m, _src) = &resolver.cache[path];
        let is_root = idx == total_modules - 1;
        merge_ctx_from_module(m, &mut ctx, is_root)?;
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
