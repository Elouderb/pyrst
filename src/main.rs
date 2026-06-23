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
mod formatter;
mod linter;
mod repl;
mod lsp;
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
    eprintln!("  repl                start interactive shell");
    eprintln!("  lsp                 start the language server (stdin/stdout, for editors)");
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
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
