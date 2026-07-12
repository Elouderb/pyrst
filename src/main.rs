use std::path::PathBuf;
use std::process::ExitCode;

mod lexer;
mod parser;
mod ast;
mod typeck;
mod codegen;
mod driver;
mod diag;
mod resolver;
mod stdlib;
mod formatter;
mod linter;
mod repl;
mod lsp;
mod manifest;
mod venv;
pub mod analysis;

fn print_usage() {
    eprintln!("pyrst {} — Pythonic language that compiles to Rust", env!("CARGO_PKG_VERSION"));
    eprintln!();
    eprintln!("usage: pyrst <command> [args]");
    eprintln!();
    eprintln!("commands:");
    eprintln!("  build <file.pyrs>   compile a pyrst source file to a native binary");
    eprintln!("  emit  <file.pyrs>   emit generated Rust source to stdout (no rustc)");
    eprintln!("  check <file.pyrs>   parse and typecheck only");
    eprintln!("  fmt   <file.pyrs>   format a pyrst source file in-place");
    eprintln!("  lint  <file.pyrs>   check code style and common issues");
    eprintln!("  venv  [dir]         create an isolated package environment (default .pyrstenv)");
    eprintln!("  install [path]      install a local package (+ its deps) into the active env;");
    eprintln!("                      no arg reproduces the env from pyrst.lock");
    eprintln!("  init                scaffold a pyrst.yaml for the current directory");
    eprintln!("  list                list packages installed in the active env");
    eprintln!("  freeze              print the pinned lock set");
    eprintln!("  repl                start interactive shell");
    eprintln!("  lsp                 start the language server (stdin/stdout, for editors)");
    eprintln!();
    eprintln!("global flags:");
    eprintln!("  --venv <dir>        use <dir> as the active environment (overrides PYRST_VENV/auto-detect)");
}

/// Render a packaging-command error (no source file to snippet against) and map
/// it to a failure exit code.
fn finish_pkg(result: diag::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", e.format_with_source(None));
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    // Extract the global `--venv <dir>` flag (it may appear anywhere). Setting
    // PYRST_VENV here funnels every downstream consumer (resolver, install, list,
    // freeze) through the ONE discovery path in `venv`, and makes `--venv`
    // correctly override both an inherited PYRST_VENV and auto-detect. An ABSOLUTE
    // path is stored so discovery is independent of the process CWD.
    let mut args: Vec<String> = Vec::new();
    let mut raw = std::env::args().skip(1);
    while let Some(a) = raw.next() {
        let venv_dir = if a == "--venv" {
            match raw.next() {
                Some(d) => Some(d),
                None => {
                    eprintln!("error: --venv requires a directory argument");
                    return ExitCode::from(2);
                }
            }
        } else {
            a.strip_prefix("--venv=").map(|s| s.to_string())
        };
        match venv_dir {
            Some(d) => {
                let abs = std::fs::canonicalize(&d).unwrap_or_else(|_| {
                    std::env::current_dir().map(|c| c.join(&d)).unwrap_or_else(|_| PathBuf::from(&d))
                });
                std::env::set_var("PYRST_VENV", abs);
            }
            None => args.push(a),
        }
    }

    let mut args = args.into_iter();
    let cmd = match args.next() {
        Some(c) => c,
        None => {
            print_usage();
            return ExitCode::from(2);
        }
    };

    // Handle commands that don't need a file path
    if cmd == "repl" {
        let result = repl::repl();
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{}", e);
                ExitCode::FAILURE
            }
        };
    }

    if cmd == "lsp" {
        return lsp::run();
    }

    // Packaging commands (optional / no file argument).
    match cmd.as_str() {
        "venv" => return finish_pkg(venv::create(args.next().map(PathBuf::from))),
        "install" => return finish_pkg(venv::install(args.next().map(PathBuf::from))),
        "init" => return finish_pkg(venv::init()),
        "list" => return finish_pkg(venv::list()),
        "freeze" => return finish_pkg(venv::freeze()),
        _ => {}
    }

    let path = match args.next() {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("error: expected a source file path");
            return ExitCode::from(2);
        }
    };

    let result = match cmd.as_str() {
        "build" => driver::build(&path),
        "emit" => driver::emit(&path),
        "check" => driver::check(&path),
        "fmt" => driver::fmt(&path),
        "lint" => driver::lint(&path),
        other => {
            eprintln!("error: unknown command '{}'", other);
            print_usage();
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Normalize line endings here too: the lexer saw the resolver's
            // normalized source, so the renderer must read the SAME normalized
            // text or CRLF files would render carets at desynced columns.
            let source = std::fs::read_to_string(&path)
                .ok()
                .map(|s| lexer::normalize_line_endings(&s));
            let formatted = e.format_with_source(source.as_deref());
            eprintln!("{}", formatted);
            ExitCode::FAILURE
        }
    }
}
