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
mod fetch;
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
    eprintln!("  install [target]    install a package (+ its deps) into the active env, where");
    eprintln!("                      <target> is a git URL (url, url@<ref>, or url#<sha>) or a");
    eprintln!("                      local path; no arg reproduces the env from pyrst.lock");
    eprintln!("                      (--force reinstalls over a name/source collision)");
    eprintln!("  init                scaffold a pyrst.yaml for the current directory");
    eprintln!("  list                list packages installed in the active env (name@version + source)");
    eprintln!("  freeze              print the pinned lock set");
    eprintln!("  cache <subcommand>  manage the clone cache: `dir` (print its path),");
    eprintln!("                      `list` (cached clones + sizes), `clean` (remove them all)");
    eprintln!("  repl                start interactive shell");
    eprintln!("  lsp                 start the language server (stdin/stdout, for editors)");
    eprintln!();
    eprintln!("global flags:");
    eprintln!("  --venv <dir>        use <dir> as the active environment (overrides PYRST_VENV/auto-detect)");
    eprintln!("  --cache <dir>       use <dir> as the clone-cache root (overrides PYRST_CACHE)");
    eprintln!();
    eprintln!("environment:");
    eprintln!("  PYRST_CACHE         clone-cache root for `install`/`cache` (default ~/.cache/pyrst)");
    eprintln!();
    eprintln!("security: `install` clones and `build` COMPILES third-party source. pyrst verifies a");
    eprintln!("  target is a real pyrst package (a valid pyrst.yaml) and pins the exact commit SHA");
    eprintln!("  (no silent upstream drift), but does NOT sandbox installed code — the same trust");
    eprintln!("  model as `pip install` / `cargo add`. Requires `git` on PATH.");
}

/// Make a path absolute WITHOUT requiring it to exist (a relative path is joined
/// onto the process CWD). Used for `--cache`, whose target may not exist yet
/// (`cache dir`/`clean` operate on a possibly-absent directory).
fn absolutize(d: &str) -> PathBuf {
    let p = PathBuf::from(d);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|c| c.join(&p)).unwrap_or(p)
    }
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
        if let Some(d) = venv_dir {
            let abs = std::fs::canonicalize(&d).unwrap_or_else(|_| {
                std::env::current_dir().map(|c| c.join(&d)).unwrap_or_else(|_| PathBuf::from(&d))
            });
            std::env::set_var("PYRST_VENV", abs);
            continue;
        }
        // `--cache <dir>` / `--cache=<dir>` mirrors PYRST_CACHE (the clone-cache
        // root). It need not exist yet, so absolutize without requiring existence.
        let cache_dir = if a == "--cache" {
            match raw.next() {
                Some(d) => Some(d),
                None => {
                    eprintln!("error: --cache requires a directory argument");
                    return ExitCode::from(2);
                }
            }
        } else {
            a.strip_prefix("--cache=").map(|s| s.to_string())
        };
        if let Some(d) = cache_dir {
            std::env::set_var("PYRST_CACHE", absolutize(&d));
            continue;
        }
        args.push(a);
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
        "install" => {
            // `install [--force] [target]` — <target> is a git URL or local path;
            // no target reproduces from pyrst.lock. `--force` reinstalls over a
            // name/source collision.
            let mut force = false;
            let mut spec: Option<String> = None;
            for a in args.by_ref() {
                if a == "--force" {
                    force = true;
                } else if spec.is_none() {
                    spec = Some(a);
                } else {
                    eprintln!("error: install takes at most one target (got an extra '{}')", a);
                    return ExitCode::from(2);
                }
            }
            return finish_pkg(venv::install(spec, force));
        }
        "init" => return finish_pkg(venv::init()),
        "list" => return finish_pkg(venv::list()),
        "freeze" => return finish_pkg(venv::freeze()),
        "cache" => return finish_pkg(fetch::cache_command(args.next().as_deref())),
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
