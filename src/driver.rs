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
    let rust = compile_to_rust(path)?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("a");
    let rs_path = std::env::temp_dir().join(format!("pyrst-{}.rs", stem));
    let cwd = std::env::current_dir()?;
    // rustc emits exactly the `-o` name we give it, so on Windows we must add the
    // `.exe` suffix ourselves — otherwise the produced file is not runnable.
    let bin_name = if cfg!(windows) { format!("{}.exe", stem) } else { stem.to_string() };
    let bin_path = cwd.join(&bin_name);
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

    // Windows (MSVC) drops a `<stem>.pdb` debug-symbols file next to the binary;
    // remove it so a plain `build` doesn't litter the working directory.
    if cfg!(windows) {
        let _ = std::fs::remove_file(cwd.join(format!("{}.pdb", stem)));
    }

    eprintln!("built: {}", bin_path.display());
    Ok(())
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
