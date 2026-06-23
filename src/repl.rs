//! Interactive REPL for pyrst.
//!
//! Provides a Read-Eval-Print Loop for interactive exploration and learning.

use crate::ast::{Module, Stmt};
use crate::diag::Result;
use std::io::{self, Write};

pub fn repl() -> Result<()> {
    println!("pyrst {} interactive shell", env!("CARGO_PKG_VERSION"));
    println!("Type 'exit()' or press Ctrl+D to exit\n");

    let mut session = ReplSession::new();

    loop {
        print!(">>> ");
        io::stdout().flush()?;

        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            // EOF (Ctrl+D)
            println!();
            break;
        }

        let trimmed = line.trim();

        // Handle exit command
        if trimmed == "exit()" {
            break;
        }

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Handle multi-line input
        let mut full_input = trimmed.to_string();
        if trimmed.ends_with(':') {
            loop {
                print!("... ");
                io::stdout().flush()?;
                let mut cont_line = String::new();
                if io::stdin().read_line(&mut cont_line)? == 0 {
                    break;
                }
                let cont_trimmed = cont_line.trim();
                if cont_trimmed.is_empty() {
                    break;
                }
                full_input.push('\n');
                full_input.push_str(cont_trimmed);
            }
        }

        // Execute the line
        match session.execute(&full_input) {
            Ok(Some(output)) => println!("{}", output),
            Ok(None) => {}
            Err(e) => eprintln!("{}", e),
        }
    }

    Ok(())
}

/// How a parsed REPL input is routed into the session model.
///
/// Classification is driven by the PARSED AST (not string heuristics), so it is
/// robust to cases the old keyword sniffing got wrong — e.g. `x == y` is a
/// comparison EXPRESSION, not an assignment statement.
#[derive(Debug, PartialEq, Eq)]
enum Classified {
    /// A top-level definition (`def` / `class` / `import`): lives at module level.
    Decl,
    /// A bare expression (`2 + 3`, `f(x)`): wrapped as `print(<expr>)` so its
    /// value is shown.
    BareExpr,
    /// Any other executable statement (assignment, `if`/`for`/`while`, a call
    /// statement, multiple statements at once): appended to `main`'s body.
    Stmt,
}

/// Classify already-parsed REPL input.
///
/// Pure and `cargo test`-able (no compile/run path). A single top-level
/// `def`/`class`/`import` is a [`Classified::Decl`]; a single bare expression
/// statement is a [`Classified::BareExpr`]; everything else (including multiple
/// statements pasted at once) is a [`Classified::Stmt`].
fn classify(module: &Module) -> Classified {
    if module.stmts.len() == 1 {
        match &module.stmts[0] {
            Stmt::Func(_) | Stmt::Class(_) | Stmt::Import { .. } => return Classified::Decl,
            Stmt::Expr(_) => return Classified::BareExpr,
            _ => return Classified::Stmt,
        }
    }
    Classified::Stmt
}

/// Indent every line of `block` by four spaces (one `main()` body level).
/// Multi-line items (a `def` body, an `if`/`for` block) keep their internal
/// relative indentation; blank lines are left empty rather than space-padded.
fn indent_block(block: &str) -> String {
    block
        .lines()
        .map(|l| if l.is_empty() { String::new() } else { format!("    {}", l) })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Synthesize a complete, compilable pyrst program from the session.
///
/// `decls` (top-level `def`/`class`/`import` source) are emitted at module
/// level; `body` (executable statement source, in input order) goes inside
/// `def main() -> None:`, indented one level. An empty body still yields a valid
/// `main` whose single statement is `pass`.
///
/// Pure and `cargo test`-able: it never touches rustc.
fn synthesize_program(decls: &[String], body: &[String]) -> String {
    let mut out = String::new();

    for d in decls {
        out.push_str(d.trim_end());
        out.push_str("\n\n");
    }

    out.push_str("def main() -> None:\n");
    if body.is_empty() {
        out.push_str("    pass\n");
    } else {
        for stmt in body {
            out.push_str(&indent_block(stmt.trim_end()));
            out.push('\n');
        }
    }

    out
}

/// The portion of `new_output` that follows the longest common prefix it shares
/// with `prev_output`.
///
/// Re-running the whole accumulated program reprints everything prior runs
/// printed; this returns only the newly-produced tail so the user sees just the
/// delta. The split is taken at a UTF-8 char boundary so multibyte output never
/// panics. If `new_output` is a prefix of (or equal to) `prev_output` — e.g. a
/// committed input that produced no new stdout — the delta is empty.
fn output_delta<'a>(prev_output: &str, new_output: &'a str) -> &'a str {
    let prefix_len = prev_output
        .char_indices()
        .zip(new_output.char_indices())
        .take_while(|((_, a), (_, b))| a == b)
        .count();
    // Map the matched char count back to a byte index in `new_output`.
    let byte_idx = new_output
        .char_indices()
        .nth(prefix_len)
        .map(|(i, _)| i)
        .unwrap_or(new_output.len());
    &new_output[byte_idx..]
}

pub struct ReplSession {
    /// Top-level definitions (`def`/`class`/`import`) source, in input order.
    decls: Vec<String>,
    /// Executable statements source, in input order, lowered into `main`.
    body: Vec<String>,
    /// Full stdout of the last successful run; the baseline for the output delta.
    last_output: String,
}

impl ReplSession {
    fn new() -> Self {
        Self {
            decls: Vec::new(),
            body: Vec::new(),
            last_output: String::new(),
        }
    }

    /// Evaluate one (already gathered, possibly multi-line) input.
    ///
    /// Steps: parse → classify → synthesize the full program including the new
    /// item → compile → run. On success, print only the NEW stdout (the delta
    /// past `last_output`) and COMMIT the input into the session. On any
    /// parse / typeck / rustc / runtime error, surface it and leave the session
    /// untouched (the accumulated program stays valid and runnable).
    fn execute(&mut self, input: &str) -> Result<Option<String>> {
        // 1. Parse. A parse error must NOT modify the session.
        let module = crate::parser::parse(input)?;
        let kind = classify(&module);

        // 2. Build the candidate program WITHOUT mutating the session yet, so a
        //    compile/run failure rolls back trivially (we just drop the clones).
        //    `print(<expr>)` shows a bare expression's value.
        let new_item = match kind {
            Classified::BareExpr => format!("print({})", input.trim()),
            _ => input.to_string(),
        };

        let mut trial_decls = self.decls.clone();
        let mut trial_body = self.body.clone();
        match kind {
            Classified::Decl => trial_decls.push(new_item.clone()),
            Classified::BareExpr | Classified::Stmt => trial_body.push(new_item.clone()),
        }

        let program = synthesize_program(&trial_decls, &trial_body);

        // 3. Compile + run. Any error here returns Err and leaves self untouched.
        let rust = crate::driver::compile_str(&program)?;
        let new_output = crate::driver::run_rust(&rust)?;

        // 4. Success: compute and show only the new tail, then commit.
        let delta = output_delta(&self.last_output, &new_output).to_string();
        self.decls = trial_decls;
        self.body = trial_body;
        self.last_output = new_output;

        if delta.is_empty() {
            Ok(None)
        } else {
            // The program's prints already include their own trailing newline;
            // strip exactly one so the REPL's own `println!` doesn't double it.
            let shown = delta.strip_suffix('\n').unwrap_or(&delta).to_string();
            Ok(Some(shown))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_src(src: &str) -> Classified {
        let m = crate::parser::parse(src).expect("parse");
        classify(&m)
    }

    // --- classification: decl vs stmt vs bare-expr ------------------------

    #[test]
    fn classify_def_is_decl() {
        assert_eq!(
            classify_src("def f(n: int) -> int:\n    return n * 2\n"),
            Classified::Decl
        );
    }

    #[test]
    fn classify_class_is_decl() {
        assert_eq!(
            classify_src("class P:\n    x: int\n    def __init__(self, x: int) -> None:\n        self.x = x\n"),
            Classified::Decl
        );
    }

    #[test]
    fn classify_import_is_decl() {
        assert_eq!(classify_src("import math"), Classified::Decl);
    }

    #[test]
    fn classify_annotated_assign_is_stmt() {
        assert_eq!(classify_src("x: int = 5"), Classified::Stmt);
    }

    #[test]
    fn classify_bare_int_expr_is_bare_expr() {
        assert_eq!(classify_src("2 + 3"), Classified::BareExpr);
    }

    #[test]
    fn classify_call_expr_is_bare_expr() {
        // A bare call expression — its value should be printed.
        assert_eq!(classify_src("f(3)"), Classified::BareExpr);
    }

    #[test]
    fn classify_equality_is_bare_expr_not_stmt() {
        // The is_expr_only fix: `x == y` is a comparison EXPRESSION. The old
        // `contains('=')` heuristic wrongly classified it as a statement.
        assert_eq!(classify_src("x == y"), Classified::BareExpr);
    }

    #[test]
    fn classify_print_call_is_bare_expr() {
        // `print(x)` is itself a bare expression statement; it is NOT double-wrapped
        // (the BareExpr path wraps the SOURCE, but a print call already shows output).
        assert_eq!(classify_src("print(x)"), Classified::BareExpr);
    }

    #[test]
    fn classify_if_block_is_stmt() {
        assert_eq!(
            classify_src("if x > 0:\n    y = 1\n"),
            Classified::Stmt
        );
    }

    // --- synthesis --------------------------------------------------------

    #[test]
    fn synthesize_empty_body_emits_pass() {
        let prog = synthesize_program(&[], &[]);
        assert_eq!(prog, "def main() -> None:\n    pass\n");
        // And it must itself be valid pyrst.
        crate::parser::parse(&prog).expect("empty-body program parses");
    }

    #[test]
    fn synthesize_decls_at_module_level_body_indented() {
        let decls = vec!["def f(n: int) -> int:\n    return n * 2".to_string()];
        let body = vec!["x: int = 5".to_string(), "print(f(x))".to_string()];
        let prog = synthesize_program(&decls, &body);

        // The def is at module level (column 0): the program starts with it.
        assert!(prog.starts_with("def f(n: int) -> int:\n    return n * 2\n"),
            "decl emitted at module level:\n{}", prog);
        // Body statements are indented one level inside main.
        assert!(prog.contains("def main() -> None:\n    x: int = 5\n    print(f(x))\n"),
            "body indented inside main:\n{}", prog);
        // The synthesized program must parse.
        crate::parser::parse(&prog).expect("synthesized program parses");
    }

    #[test]
    fn synthesize_multiline_body_item_keeps_relative_indent() {
        // A multi-line executable item (an if-block) must have every line shifted
        // one level, preserving its internal relative indentation.
        let body = vec!["if x > 0:\n    y: int = 1".to_string()];
        let prog = synthesize_program(&[], &body);
        assert!(prog.contains("def main() -> None:\n    if x > 0:\n        y: int = 1\n"),
            "multi-line item re-indented:\n{}", prog);
        crate::parser::parse(&prog).expect("synthesized program parses");
    }

    // --- output delta -----------------------------------------------------

    #[test]
    fn output_delta_returns_new_tail_only() {
        // Re-running reprints prior output; only the new tail is shown.
        assert_eq!(output_delta("5\n", "5\n6\n"), "6\n");
    }

    #[test]
    fn output_delta_empty_when_no_new_output() {
        assert_eq!(output_delta("5\n", "5\n"), "");
    }

    #[test]
    fn output_delta_full_when_no_prior_output() {
        assert_eq!(output_delta("", "5\n"), "5\n");
    }

    #[test]
    fn output_delta_handles_multibyte_without_panic() {
        // The common-prefix split must land on a char boundary.
        let prev = "café\n";
        let new = "café\nnaïve\n";
        assert_eq!(output_delta(prev, new), "naïve\n");
    }
}
