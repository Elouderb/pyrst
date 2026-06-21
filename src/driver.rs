use std::path::Path;
use std::process::Command;

use crate::diag::{Error, Result};

pub fn check(path: &Path) -> Result<()> {
    let prog = crate::resolver::resolve(path)?;
    for (m, _src) in &prog.modules {
        crate::typeck::check_bodies(m, &prog.ctx)?;
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
    let bin_path = std::env::current_dir()?.join(stem);
    std::fs::write(&rs_path, rust)?;

    // Try to find rustc in standard locations
    let rustc_path = if let Ok(home) = std::env::var("HOME") {
        let cargo_rustc = Path::new(&home).join(".cargo/bin/rustc");
        if cargo_rustc.exists() {
            cargo_rustc
        } else {
            Path::new("rustc").to_path_buf()
        }
    } else {
        Path::new("rustc").to_path_buf()
    };

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

    eprintln!("built: {}", bin_path.display());
    Ok(())
}

pub fn fmt(path: &Path) -> Result<()> {
    let source = std::fs::read_to_string(path)?;

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
    let source = std::fs::read_to_string(path)?;
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
    for (m, _src) in &prog.modules {
        crate::typeck::check_bodies(m, &prog.ctx)?;
    }
    crate::codegen::emit_program(&prog.modules, &prog.ctx)
}

#[cfg(test)]
mod fmt_tests {
    use super::*;

    /// Create a uniquely-named temp file with `contents` and return its path.
    fn temp_file(name: &str, contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pyrst-fmt-test-{}-{}-{}.py",
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
