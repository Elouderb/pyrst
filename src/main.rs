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

fn print_usage() {
    eprintln!("pyrst {} — Pythonic language that compiles to Rust", env!("CARGO_PKG_VERSION"));
    eprintln!();
    eprintln!("usage: pyrst <command> [args]");
    eprintln!();
    eprintln!("commands:");
    eprintln!("  build <file.py>     compile a pyrst source file to a native binary");
    eprintln!("  emit  <file.py>     emit generated Rust source to stdout (no rustc)");
    eprintln!("  check <file.py>     parse and typecheck only");
    eprintln!("  fmt   <file.py>     format a pyrst source file in-place");
    eprintln!("  lint  <file.py>     check code style and common issues");
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
            let source = std::fs::read_to_string(&path).ok();
            let formatted = e.format_with_source(source.as_deref());
            eprintln!("{}", formatted);
            ExitCode::FAILURE
        }
    }
}
