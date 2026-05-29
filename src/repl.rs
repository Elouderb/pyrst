//! Interactive REPL for pyrst.
//!
//! Provides a Read-Eval-Print Loop for interactive exploration and learning.

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

pub struct ReplSession {
    stmts: Vec<String>,
}

impl ReplSession {
    fn new() -> Self {
        Self {
            stmts: Vec::new(),
        }
    }

    fn execute(&mut self, line: &str) -> Result<Option<String>> {
        // Try to parse the line
        match crate::parser::parse(line) {
            Ok(module) => {
                // Successfully parsed as a complete statement/expression
                if is_expr_only(line) {
                    // Expression - try to evaluate
                    Ok(Some(format!("{}", line)))
                } else {
                    // Statement - add to session
                    self.stmts.push(line.to_string());
                    Ok(None)
                }
            }
            Err(_) => {
                // Failed to parse - check if it's an incomplete statement
                Err(crate::diag::Error::Parse {
                    span: crate::diag::Span::DUMMY,
                    msg: format!("parse error in: {}", line),
                })
            }
        }
    }
}

fn is_expr_only(s: &str) -> bool {
    let trimmed = s.trim();
    // Simple heuristic: if it's not a statement keyword, it's likely an expression
    !(trimmed.starts_with("def ")
        || trimmed.starts_with("class ")
        || trimmed.starts_with("import ")
        || trimmed.starts_with("from ")
        || (trimmed.contains('=') && !trimmed.starts_with('='))
        || trimmed.starts_with("if ")
        || trimmed.starts_with("elif ")
        || trimmed.starts_with("else:")
        || trimmed.starts_with("while ")
        || trimmed.starts_with("for ")
        || trimmed.starts_with("try:")
        || trimmed.starts_with("except")
        || trimmed.starts_with("finally:")
        || trimmed.starts_with("with ")
        || trimmed.starts_with("return ")
        || trimmed.starts_with("pass")
        || trimmed.starts_with("break")
        || trimmed.starts_with("continue")
        || trimmed.starts_with("raise "))
}
