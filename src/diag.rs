use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub const DUMMY: Span = Span { start: 0, end: 0, line: 0, col: 0 };

    pub fn new(start: usize, end: usize, line: u32, col: u32) -> Self {
        Self { start, end, line, col }
    }
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Lex { span: Span, msg: String },
    Parse { span: Span, msg: String },
    Type { span: Span, msg: String },
    Codegen(String),
    Rustc(String),
    ImportNotFound { path: String, span: Span, importing_file: String },
    CircularImport { cycle: Vec<String>, span: Span },
    /// (PKG Phase 1) An import that resolves to nothing WHILE A VIRTUAL ENV IS
    /// ACTIVE. Distinct from `ImportNotFound` (the NO-ENV path, kept byte-for-byte)
    /// so the no-env resolution is entirely unchanged: this variant is constructed
    /// ONLY when `Resolver::active_env` is `Some`. It names the missing module, the
    /// active env, and tells the user to `pyrst install` it — never a downstream
    /// rustc leak (the project's honest-errors invariant, applied to packaging).
    PackageNotInstalled { module: String, env: String, span: Span, importing_file: String },
    /// (card 587a9dcb) A bare `import <pkg>` where `<pkg>.pyrs` is ABSENT but a
    /// same-named DIRECTORY `<pkg>/` DOES exist in a search location (root-relative
    /// base, the env store `<env>/packages/<pkg>/`, or a `$PYRST_PATH` entry). The
    /// name is a PACKAGE (a directory of submodules), not a single importable
    /// module — so this is the HONEST, actionable error (import a submodule, and
    /// here are the ones available), REPLACING the misleading `PackageNotInstalled`
    /// / `ImportNotFound`. Constructed in the resolver's import-miss path ONLY when
    /// the directory actually exists; a genuine not-installed / not-found keeps its
    /// prior error. `submodules` is the (possibly empty) sorted list of top-level
    /// `*.pyrs` stems directly under the package dir.
    IsPackageNotModule { package: String, submodules: Vec<String>, span: Span, importing_file: String },
    /// (PKG Phase 1) A non-span packaging error — manifest parse/verify failures,
    /// env-completeness (a declared dependency not installed), an install-time
    /// dependency cycle, a name collision at a different source, or a command run
    /// without an active env. The `String` is the FULLY-FORMED honest message; the
    /// variant carries no span (these arise at the CLI / manifest / env-store level,
    /// not at a source location the way `Lex`/`Parse`/`Type` do).
    Pkg(String),
    /// Multi-file error sourcing (EPIC-8): a wrapper that pairs an inner error
    /// with the source text (and originating file) it should be rendered
    /// against. Constructed ONLY at the driver/resolver per-module boundary via
    /// [`Error::with_render_source`] — never at the ~90 `Lex`/`Parse`/`Type`
    /// construction sites. The inner span (line:col) indexes into `source`, so
    /// the snippet shows the correct module's line + caret instead of the root
    /// file's text. `Display` delegates to `inner`, so non-snippet rendering
    /// paths (the REPL, the bare `{}` path) are unaffected.
    Sourced { inner: Box<Error>, file: Option<PathBuf>, source: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {}", e),
            Error::Lex { span, msg } => write!(f, "lex error at {}:{}: {}", span.line, span.col, msg),
            Error::Parse { span, msg } => write!(f, "parse error at {}:{}: {}", span.line, span.col, msg),
            Error::Type { span, msg } => write!(f, "type error at {}:{}: {}", span.line, span.col, msg),
            Error::Codegen(msg) => write!(f, "codegen error: {}", msg),
            Error::Rustc(msg) => write!(f, "rustc failed: {}", msg),
            Error::ImportNotFound { path, span, importing_file } => {
                write!(f, "import error at {}:{}: cannot find module '{}' (imported from {})", span.line, span.col, path, importing_file)
            }
            Error::CircularImport { cycle, span } => {
                let cycle_str = cycle.join(" → ");
                write!(f, "import error at {}:{}: circular import detected: {}", span.line, span.col, cycle_str)
            }
            Error::PackageNotInstalled { module, env, span, importing_file } => {
                write!(
                    f,
                    "import error at {}:{}: module '{}' is imported but not installed in the active environment '{}' — `pyrst install` it, or check its package's `pyrst.yaml` dependencies (imported from {})",
                    span.line, span.col, module, env, importing_file
                )
            }
            Error::IsPackageNotModule { package, submodules, span, importing_file } => {
                let example = submodules.first().map(String::as_str).unwrap_or("<submodule>");
                let avail = if submodules.is_empty() {
                    String::new()
                } else {
                    format!(" (available submodules: {})", submodules.join(", "))
                };
                write!(
                    f,
                    "import error at {}:{}: '{}' is a package (a directory of modules), not a single module — import a submodule, e.g. `from {}.{} import <name>`{} (imported from {})",
                    span.line, span.col, package, package, example, avail, importing_file
                )
            }
            Error::Pkg(msg) => write!(f, "{}", msg),
            // The render-source wrapper is transparent to `Display`: the bare
            // `{}` rendering (REPL, main.rs:46) is identical to the inner error.
            Error::Sourced { inner, .. } => write!(f, "{}", inner),
        }
    }
}

impl Error {
    /// Pair this error with the source text (and originating file) it should be
    /// rendered against — used at the driver/resolver per-module boundary so an
    /// error from an imported module renders against THAT module's source rather
    /// than the root file (EPIC-8 multi-file error sourcing).
    ///
    /// Only `Lex`/`Parse`/`Type` carry a span that indexes into per-module
    /// source; for every other variant (already self-contained: import, IO,
    /// codegen, rustc) wrapping would add no value, so they are returned
    /// unchanged. Wrapping is also idempotent: an already-`Sourced` error keeps
    /// its innermost (origin) source and is not double-wrapped.
    pub fn with_render_source(self, file: Option<PathBuf>, source: &str) -> Error {
        match self {
            Error::Lex { .. } | Error::Parse { .. } | Error::Type { .. } => Error::Sourced {
                inner: Box::new(self),
                file,
                source: source.to_string(),
            },
            // Already paired at an inner boundary — keep the origin source.
            Error::Sourced { .. } => self,
            // Self-contained variants gain nothing from a source snippet.
            other => other,
        }
    }

    /// Format error with source code snippet for display.
    /// If source is provided, includes the offending line and visual indicator.
    ///
    /// A `Sourced` error renders its inner error against the source text it was
    /// paired with at the per-module boundary, ignoring the `source` passed in
    /// here (which is the CLI root file). This is what makes an imported
    /// module's error show that module's line + caret (and file name).
    pub fn format_with_source(&self, source: Option<&str>) -> String {
        match self {
            Error::Io(e) => format!("io error: {}", e),
            Error::Codegen(msg) => format!("codegen error: {}", msg),
            Error::Rustc(msg) => format!("rustc failed: {}", msg),
            Error::ImportNotFound { path, span, importing_file } => {
                format!("import error at {}:{}: cannot find module '{}'\n  imported from {}", span.line, span.col, path, importing_file)
            }
            Error::CircularImport { cycle, span } => {
                let cycle_str = cycle.join(" → ");
                format!("import error at {}:{}: circular import detected: {}", span.line, span.col, cycle_str)
            }
            Error::PackageNotInstalled { module, env, span, importing_file } => {
                format!(
                    "import error at {}:{}: module '{}' is imported but not installed in the active environment '{}'\n  `pyrst install` it, or check its package's `pyrst.yaml` dependencies\n  imported from {}",
                    span.line, span.col, module, env, importing_file
                )
            }
            Error::IsPackageNotModule { package, submodules, span, importing_file } => {
                let example = submodules.first().map(String::as_str).unwrap_or("<submodule>");
                let avail = if submodules.is_empty() {
                    String::new()
                } else {
                    format!("\n  available submodules: {}", submodules.join(", "))
                };
                format!(
                    "import error at {}:{}: '{}' is a package (a directory of modules), not a single module\n  import a submodule, e.g. `from {}.{} import <name>`{}\n  imported from {}",
                    span.line, span.col, package, package, example, avail, importing_file
                )
            }
            Error::Pkg(msg) => msg.clone(),
            // Render the wrapped error against its OWN module source + file name,
            // not the root source `main` re-read from the CLI argument.
            Error::Sourced { inner, file, source: module_src } => {
                inner.format_with_source_and_file(Some(module_src), file.as_deref())
            }
            Error::Lex { .. } | Error::Parse { .. } | Error::Type { .. } => {
                self.format_with_source_and_file(source, None)
            }
        }
    }

    /// Snippet rendering for the span-bearing variants, optionally naming the
    /// originating file. Shared by the root-file path (`file = None`, byte-for-
    /// byte identical to the pre-EPIC-8 single-file output) and the per-module
    /// path (`file = Some`, which adds an `in <file>` suffix to the location).
    fn format_with_source_and_file(&self, source: Option<&str>, file: Option<&Path>) -> String {
        match self {
            Error::Lex { span, msg } |
            Error::Parse { span, msg } |
            Error::Type { span, msg } => {
                let error_type = match self {
                    Error::Lex { .. } => "lex error",
                    Error::Parse { .. } => "parse error",
                    Error::Type { .. } => "type error",
                    _ => "error",
                };

                let mut output = format!("{}: {}\n  at {}:{}", error_type, msg, span.line, span.col);

                // Name the originating file only on the multi-file path so the
                // single-file output (`file = None`) is unchanged.
                if let Some(f) = file {
                    output.push_str(&format!(" in {}", f.display()));
                }

                if let Some(src) = source {
                    if let Some(snippet) = Self::extract_snippet(src, *span) {
                        output.push_str(&format!("\n\n{}", snippet));
                    }
                }

                output
            }
            // Only span-bearing variants reach this helper; anything else
            // delegates to the public formatter (defensive — keeps Display
            // semantics intact if a future caller routes a wrapper here).
            other => other.format_with_source(source),
        }
    }

    /// Extract source snippet around the error location.
    /// Returns formatted string with line number, code, and caret indicator.
    fn extract_snippet(source: &str, span: Span) -> Option<String> {
        let lines: Vec<&str> = source.lines().collect();
        let line_idx = (span.line as usize).saturating_sub(1);

        if line_idx >= lines.len() {
            return None;
        }

        let line = lines[line_idx];
        let col = span.col as usize;

        // Ensure col is within line bounds
        if col > line.len() {
            return None;
        }

        // Build visual indicator (caret at column position)
        let mut caret = String::new();
        for (i, c) in line.char_indices() {
            if i < col.saturating_sub(1) {
                // Count visual width: tabs are 4 spaces, other chars are 1
                if c == '\t' {
                    caret.push_str("    ");
                } else {
                    caret.push(' ');
                }
            } else {
                break;
            }
        }
        caret.push('^');

        let mut snippet = String::new();

        // Show previous line if available
        if line_idx > 0 {
            let prev_line = lines[line_idx - 1];
            snippet.push_str(&format!("  {} │ {}\n", span.line - 1, prev_line));
        }

        // Show error line
        snippet.push_str(&format!("  {} │ {}\n", span.line, line));
        snippet.push_str(&format!("      │ {}\n", caret));

        // Show next line if available
        if line_idx + 1 < lines.len() {
            let next_line = lines[line_idx + 1];
            snippet.push_str(&format!("  {} │ {}", span.line + 1, next_line));
        }

        Some(snippet)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self { Error::Io(e) }
}

pub type Result<T> = std::result::Result<T, Error>;
