use std::path::Path;
use std::process::Command;

use crate::diag::{Error, Result};

pub fn check(path: &Path) -> Result<()> {
    let prog = crate::resolver::resolve(path)?;
    // Name the originating file only when more than one module is involved, so
    // single-file error output stays byte-for-byte identical (EPIC-8).
    let multi = prog.modules.len() > 1;
    for (m, src) in &prog.modules {
        // EPIC-8: a body-check error belongs to THIS module — pair its own
        // source (and file) so `main` renders the snippet against the right
        // file instead of the root file.
        let render_path = if multi { m.source_path.clone() } else { None };
        crate::typeck::check_bodies(m, &prog.ctx)
            .map_err(|e| e.with_render_source(render_path, src))?;
    }
    eprintln!("ok: {} module(s) typecheck", prog.modules.len());
    Ok(())
}

pub fn emit(path: &Path) -> Result<()> {
    let rust = compile_to_rust(path)?;
    print!("{}", rust);
    Ok(())
}

pub fn build(path: &Path) -> Result<()> {
    // Resolve + typecheck ONCE, then collect the program's external-crate
    // dependencies (Rust interop Phase 2) from every reachable module before
    // deciding which build path to take. The Rust SOURCE is identical either way
    // (only `build` differs); a program with NO crate deps stays on the
    // unchanged single-file rustc path with zero overhead/regression.
    let prog = crate::resolver::resolve(path)?;
    let multi = prog.modules.len() > 1;
    for (m, src) in &prog.modules {
        let render_path = if multi { m.source_path.clone() } else { None };
        crate::typeck::check_bodies(m, &prog.ctx)
            .map_err(|e| e.with_render_source(render_path, src))?;
    }
    let rust = crate::codegen::emit_program(&prog.modules, &prog.ctx)?;
    let crates = collect_crate_deps(&prog.modules);

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("a");
    let cwd = std::env::current_dir()?;
    // rustc / cargo emit exactly the binary name we ask for, so on Windows we add
    // the `.exe` suffix ourselves — otherwise the produced file is not runnable.
    let bin_name = if cfg!(windows) { format!("{}.exe", stem) } else { stem.to_string() };
    let bin_path = cwd.join(&bin_name);

    if crates.is_empty() {
        // ── NO external crates: the EXISTING single-file rustc path, UNCHANGED.
        let rs_path = std::env::temp_dir().join(format!("pyrst-{}.rs", stem));
        std::fs::write(&rs_path, rust)?;

        let rustc_path = rustc_path();
        let status = Command::new(&rustc_path)
            .arg(&rs_path)
            .arg("-o")
            .arg(&bin_path)
            .arg("--edition")
            .arg("2021")
            .status()
            .map_err(|e| Error::Rustc(format!("failed to invoke rustc: {}", e)))?;

        if !status.success() {
            return Err(Error::Rustc(format!("rustc exited with status {}", status)));
        }
    } else {
        // ── External crates declared: build as a CARGO PROJECT so the
        // dependencies are fetched and linked. Only reached when at least one
        // `@crate(...)` is declared in a reachable module.
        build_cargo_project(stem, &rust, &crates, &bin_path)?;
    }

    // Windows (MSVC) drops a `<stem>.pdb` debug-symbols file next to the binary;
    // remove it so a plain `build` doesn't litter the working directory.
    if cfg!(windows) {
        let _ = std::fs::remove_file(cwd.join(format!("{}.pdb", stem)));
    }

    eprintln!("built: {}", bin_path.display());
    Ok(())
}

/// Collect the UNION (deduped by crate name, first-declaration order preserved)
/// of every `@crate("name", "version")` dependency declared across ALL reachable
/// modules — the root program plus every embedded/imported module merged by the
/// resolver. This is the dependency set the Cargo build path writes into
/// `Cargo.toml`. An EMPTY result means the program needs no external crates and
/// stays on the single-file rustc path.
///
/// `@crate` decorators live on `def`s, so we scan both top-level functions and
/// class methods. If the same crate name is declared twice (e.g. two `re`
/// helpers each carrying `@crate("regex", "1")`), the FIRST version wins and
/// later duplicates are ignored — a single crate cannot appear twice in
/// `[dependencies]`.
fn collect_crate_deps(modules: &[(crate::ast::Module, String)]) -> Vec<(String, String)> {
    use crate::ast::Stmt;
    let mut seen = std::collections::HashSet::new();
    let mut deps: Vec<(String, String)> = Vec::new();
    let mut push = |name: &str, version: &str| {
        if seen.insert(name.to_string()) {
            deps.push((name.to_string(), version.to_string()));
        }
    };
    for (m, _src) in modules {
        for stmt in &m.stmts {
            match stmt {
                Stmt::Func(f) => {
                    for (name, version) in &f.crate_deps {
                        push(name, version);
                    }
                }
                Stmt::Class(c) => {
                    for method in &c.methods {
                        for (name, version) in &method.crate_deps {
                            push(name, version);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    deps
}

/// Build `rust` as a CARGO PROJECT depending on `crates`, copying the produced
/// release binary to `bin_path`.
///
/// Materializes a minimal crate (a `Cargo.toml` with one `[dependencies]` line
/// per declared crate and `src/main.rs` = the generated Rust) in a per-stem temp
/// directory, runs `cargo build --release` there, and copies
/// `target/release/<name>` to the requested output. A SHARED, stable target dir
/// (`CARGO_TARGET_DIR`) is reused across builds so the dependency crates
/// (`regex` and friends) compile ONCE and are cached on every subsequent build
/// — repeat builds do not recompile them.
///
/// On a cargo failure the captured stderr is surfaced verbatim as the build
/// error, so a bad template or a missing crate version is reported honestly
/// (the same contract as the rustc path).
fn build_cargo_project(
    stem: &str,
    rust: &str,
    crates: &[(String, String)],
    bin_path: &Path,
) -> Result<()> {
    // The cargo crate name must be a valid Rust identifier; the source stem can
    // contain characters cargo rejects (`-` is fine, but `.`/spaces are not), so
    // sanitize to a safe fixed name. The OUTPUT binary still lands at `bin_path`
    // (named after the stem) via the copy below, so this internal name is never
    // user-visible.
    let pkg_name = "pyrst_program";

    let proj_dir = std::env::temp_dir().join(format!("pyrst-cargo-{}-{}", stem, std::process::id()));
    let src_dir = proj_dir.join("src");
    std::fs::create_dir_all(&src_dir)?;

    // Cargo.toml: one `name = "version"` dependency line per declared crate.
    let mut deps_block = String::new();
    for (name, version) in crates {
        deps_block.push_str(&format!("{} = \"{}\"\n", name, version));
    }
    let cargo_toml = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{}",
        pkg_name, deps_block
    );
    std::fs::write(proj_dir.join("Cargo.toml"), cargo_toml)?;
    std::fs::write(src_dir.join("main.rs"), rust)?;

    // A shared target dir caches compiled dependencies across builds: prefer an
    // explicit `CARGO_TARGET_DIR` if the environment already sets one, else a
    // stable per-user temp location. NOT inside `proj_dir` (which is per-pid and
    // discarded), so the regex build is reused on the next invocation.
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("pyrst-cargo-target"));

    let cargo_path = cargo_path();
    let output = Command::new(&cargo_path)
        .current_dir(&proj_dir)
        .env("CARGO_TARGET_DIR", &target_dir)
        .arg("build")
        .arg("--release")
        .output()
        .map_err(|e| Error::Rustc(format!("failed to invoke cargo: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Clean up the per-pid project dir before surfacing the error.
        let _ = std::fs::remove_dir_all(&proj_dir);
        return Err(Error::Rustc(format!(
            "cargo build exited with status {}\n{}",
            output.status, stderr
        )));
    }

    // Copy the produced binary to the requested output path. Cargo names it after
    // the package (`pyrst_program`), so map that back to the user's `<stem>` name.
    let produced_name = if cfg!(windows) { format!("{}.exe", pkg_name) } else { pkg_name.to_string() };
    let produced = target_dir.join("release").join(&produced_name);
    std::fs::copy(&produced, bin_path).map_err(|e| {
        Error::Rustc(format!(
            "cargo build succeeded but copying {} -> {} failed: {}",
            produced.display(),
            bin_path.display(),
            e
        ))
    })?;

    // Best-effort cleanup of the per-pid project dir (the shared target dir is
    // intentionally KEPT for caching).
    let _ = std::fs::remove_dir_all(&proj_dir);
    Ok(())
}

/// Locate `cargo`: prefer `~/.cargo/bin/cargo` (the rustup shim), else fall back
/// to bare `cargo` on PATH. Mirrors [`rustc_path`] so the Cargo build path's
/// toolchain lookup never drifts from the rustc path's.
fn cargo_path() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let cargo_bin = Path::new(&home).join(".cargo/bin/cargo");
        if cargo_bin.exists() {
            return cargo_bin;
        }
    }
    Path::new("cargo").to_path_buf()
}

/// Locate `rustc`: prefer `~/.cargo/bin/rustc` (the rustup shim), else fall back
/// to bare `rustc` on PATH. Shared by [`build`] and [`run_rust`] so the REPL's
/// rustc invocation never drifts from the `build` command's.
fn rustc_path() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        let cargo_rustc = Path::new(&home).join(".cargo/bin/rustc");
        if cargo_rustc.exists() {
            return cargo_rustc;
        }
    }
    Path::new("rustc").to_path_buf()
}

/// A self-deleting temp path: removes the file at `path` on drop. Keeps the
/// REPL's compile/run helpers from leaking temp `.pyrs`/`.rs`/binary artifacts
/// even on an early `?` return.
struct TempArtifact {
    path: std::path::PathBuf,
}

impl Drop for TempArtifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Build a unique-enough temp path in the system temp dir from `prefix`/`ext`,
/// disambiguated by pid + a monotonically increasing counter so concurrent or
/// rapid successive REPL evaluations never collide.
fn unique_temp_path(prefix: &str, ext: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{}-{}-{}.{}", prefix, std::process::id(), n, ext))
}

/// Compile pyrst SOURCE TEXT (not a path) to Rust source.
///
/// The REPL accumulates a session and recompiles the whole program on each
/// input, so it needs a text-in / Rust-out entry point. The existing
/// [`compile_to_rust`] is path-based (it goes through the import resolver, which
/// reads a file and resolves `import` statements relative to that file's
/// directory), so we materialize `source` to a temp `.pyrs` and feed that path
/// through the same resolve → typeck → codegen pipeline. The temp file is
/// removed on return (including on the `?` error path) via [`TempArtifact`].
///
/// Imports in REPL input resolve relative to the temp dir, which generally has
/// no sibling `.pyrs` files — a documented limitation (the REPL targets compute /
/// learning snippets, not multi-file projects).
pub fn compile_str(source: &str) -> Result<String> {
    let py_path = unique_temp_path("pyrst-repl", "pyrs");
    let _py_guard = TempArtifact { path: py_path.clone() };
    std::fs::write(&py_path, source)?;
    compile_to_rust(&py_path)
}

/// Compile `rust_source` with `rustc` and run the resulting binary, returning
/// its captured STDOUT.
///
/// Mirrors [`build`]'s rustc invocation (same `--edition 2021`, same
/// [`rustc_path`] lookup) so REPL evaluation matches the `build` command's
/// semantics. On a rustc compile failure the captured stderr is returned as an
/// [`Error::Rustc`]; on a non-zero exit from the user's program, the program's
/// stderr is surfaced the same way. All temp artifacts (the `.rs` and the
/// produced binary) are cleaned up on every exit path via [`TempArtifact`].
pub fn run_rust(rust_source: &str) -> Result<String> {
    let rs_path = unique_temp_path("pyrst-repl", "rs");
    let _rs_guard = TempArtifact { path: rs_path.clone() };
    std::fs::write(&rs_path, rust_source)?;

    // Windows needs the `.exe` suffix; on Unix the bare path is the executable.
    let bin_ext = if cfg!(windows) { "exe" } else { "bin" };
    let bin_path = unique_temp_path("pyrst-repl", bin_ext);
    let _bin_guard = TempArtifact { path: bin_path.clone() };

    let rustc_path = rustc_path();
    let compile = Command::new(&rustc_path)
        .arg(&rs_path)
        .arg("-o")
        .arg(&bin_path)
        .arg("--edition")
        .arg("2021")
        .output()
        .map_err(|e| Error::Rustc(format!("failed to invoke rustc: {}", e)))?;

    if !compile.status.success() {
        let stderr = String::from_utf8_lossy(&compile.stderr);
        return Err(Error::Rustc(format!(
            "rustc exited with status {}\n{}",
            compile.status, stderr
        )));
    }

    let run = Command::new(&bin_path)
        .output()
        .map_err(|e| Error::Rustc(format!("failed to run compiled program: {}", e)))?;

    if !run.status.success() {
        let stderr = String::from_utf8_lossy(&run.stderr);
        return Err(Error::Rustc(format!(
            "program exited with status {}\n{}",
            run.status, stderr
        )));
    }

    Ok(String::from_utf8_lossy(&run.stdout).into_owned())
}

pub fn fmt(path: &Path) -> Result<()> {
    // Normalize CRLF / bare CR -> LF at the read site so the lexer (via parse)
    // and any diagnostic snippet operate on byte-identical `\n`-only text.
    let source = crate::lexer::normalize_line_endings(&std::fs::read_to_string(path)?);

    // The lexer discards comments, so reformatting would silently delete them.
    // Until comment attachment exists, refuse rather than destroy user code.
    if crate::lexer::has_comment(&source) {
        return Err(Error::Codegen(format!(
            "pyrst fmt cannot yet preserve comments; formatting aborted: {}",
            path.display()
        )));
    }

    let module = crate::parser::parse(&source)?;

    // `format` aborts (Err) rather than emitting a placeholder for any node it
    // cannot render, so a failure here leaves the file untouched.
    let formatted = crate::formatter::format(&module)?;

    // Round-trip safety: the formatted output must itself parse. If it does
    // not, the formatter produced something that does not round-trip, so we
    // abort the in-place write instead of corrupting the source.
    crate::parser::parse(&formatted).map_err(|e| {
        Error::Codegen(format!(
            "pyrst fmt: formatted output failed to re-parse ({}); aborting write to {}",
            e,
            path.display()
        ))
    })?;

    // Write to a temp file in the same directory, then atomically rename over
    // the original. This guarantees the original is never left half-written if
    // the process dies mid-write.
    write_atomic(path, &formatted)?;

    eprintln!("formatted: {}", path.display());
    Ok(())
}

/// Write `contents` to `path` atomically by writing to a sibling temp file and
/// renaming it over `path`. The rename is atomic on the same filesystem, so a
/// reader either sees the old file or the fully-written new file — never a
/// partial write. On any failure the temp file is removed and `path` is left
/// unchanged.
fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("pyrst-fmt");
    let tmp_path = dir.join(format!(".{}.pyrst-fmt.{}.tmp", file_name, std::process::id()));

    if let Err(e) = std::fs::write(&tmp_path, contents) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(Error::Io(e));
    }
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(Error::Io(e));
    }
    Ok(())
}

pub fn lint(path: &Path) -> Result<()> {
    // Normalize CRLF / bare CR -> LF at the read site (see fmt/resolver).
    let source = crate::lexer::normalize_line_endings(&std::fs::read_to_string(path)?);
    let module = crate::parser::parse(&source)?;
    let lints = crate::linter::lint(&module);

    if lints.is_empty() {
        eprintln!("ok: no issues found");
        return Ok(());
    }

    for lint in lints {
        let level_str = match lint.level {
            crate::linter::LintLevel::Error => "error",
            crate::linter::LintLevel::Warning => "warning",
            crate::linter::LintLevel::Info => "info",
        };
        eprintln!("{}: {} [{}] {}", level_str, lint.code, path.display(), lint.message);
    }

    Ok(())
}

fn compile_to_rust(path: &Path) -> Result<String> {
    let prog = crate::resolver::resolve(path)?;
    // Name the originating file only in the multi-module case (EPIC-8).
    let multi = prog.modules.len() > 1;
    for (m, src) in &prog.modules {
        // EPIC-8: render any body-check error against this module's own source.
        let render_path = if multi { m.source_path.clone() } else { None };
        crate::typeck::check_bodies(m, &prog.ctx)
            .map_err(|e| e.with_render_source(render_path, src))?;
    }
    crate::codegen::emit_program(&prog.modules, &prog.ctx)
}

#[cfg(test)]
mod crate_dep_tests {
    use super::*;

    /// Build a one-module slice from pyrst source for `collect_crate_deps`.
    fn modules_from(src: &str) -> Vec<(crate::ast::Module, String)> {
        let m = crate::parser::parse(src).expect("parse");
        vec![(m, src.to_string())]
    }

    /// A program with no `@crate` declarations collects an EMPTY dependency set,
    /// so `build` stays on the unchanged single-file rustc path.
    #[test]
    fn collect_crate_deps_empty_without_crate_decorator() {
        let mods = modules_from("def main() -> None:\n    pass\n");
        assert!(collect_crate_deps(&mods).is_empty());
    }

    /// `@crate` deps are collected from top-level `@extern` functions.
    #[test]
    fn collect_crate_deps_gathers_top_level_funcs() {
        let src = "@crate(\"regex\", \"1\")\n@extern\ndef is_match(p: str, t: str) -> bool:\n    \"true\"\n\ndef main() -> None:\n    pass\n";
        let deps = collect_crate_deps(&modules_from(src));
        assert_eq!(deps, vec![("regex".to_string(), "1".to_string())]);
    }

    /// The SAME crate declared on multiple functions is deduped by name (first
    /// version wins) — a crate cannot appear twice in `[dependencies]`. This is
    /// the real `re` shape: four wrappers each carry `@crate("regex", "1")`.
    #[test]
    fn collect_crate_deps_dedups_by_name() {
        let src = "\
@crate(\"regex\", \"1\")\n@extern\ndef a(p: str) -> bool:\n    \"true\"\n
@crate(\"regex\", \"1\")\n@extern\ndef b(p: str) -> bool:\n    \"true\"\n
def main() -> None:\n    pass\n";
        let deps = collect_crate_deps(&modules_from(src));
        assert_eq!(deps, vec![("regex".to_string(), "1".to_string())],
            "the same crate declared twice must appear once");
    }
}

#[cfg(test)]
mod fmt_tests {
    use super::*;

    /// Create a uniquely-named temp file with `contents` and return its path.
    fn temp_file(name: &str, contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pyrst-fmt-test-{}-{}-{}.pyrs",
            std::process::id(),
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// Card 8d63b3af acceptance (b): formatting a file with comments aborts
    /// with the comment message and leaves the file byte-for-byte unmodified.
    #[test]
    fn fmt_refuses_files_with_comments_and_leaves_them_unmodified() {
        let src = "def main() -> None:\n    x: int = 1  # this comment must survive\n";
        let path = temp_file("comment", src);

        let result = fmt(&path);

        assert!(result.is_err(), "fmt must abort on a file containing comments");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("pyrst fmt cannot yet preserve comments"),
            "expected comment-refusal message, got: {}",
            msg
        );

        // File must be untouched (comment intact, nothing rewritten).
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, src, "fmt must not modify a file it refuses to format");

        let _ = std::fs::remove_file(&path);
    }

    /// A comment-free file with a set/dict comprehension formats successfully,
    /// the comprehension is preserved, and no placeholder is ever written.
    #[test]
    fn fmt_formats_comprehensions_without_placeholder() {
        let src = "def main() -> None:\n    s: set[int] = {x*2 for x in range(5)}\n    d: dict[int, int] = {x: x*x for x in range(5)}\n";
        let path = temp_file("comp", src);

        let result = fmt(&path);
        assert!(result.is_ok(), "fmt should succeed: {:?}", result.err());

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            !after.contains("/* complex expr */"),
            "fmt must never write a placeholder, got:\n{}",
            after
        );
        assert!(after.contains("{(x * 2) for x in range(5)}"), "got:\n{}", after);
        assert!(after.contains("{x: (x * x) for x in range(5)}"), "got:\n{}", after);

        // The written output must itself re-parse (round-trip).
        crate::parser::parse(&after).expect("formatted file should re-parse");

        let _ = std::fs::remove_file(&path);
    }
}
