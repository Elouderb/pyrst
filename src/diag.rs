use std::fmt;

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
        }
    }
}

impl Error {
    /// Format error with source code snippet for display.
    /// If source is provided, includes the offending line and visual indicator.
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

                if let Some(src) = source {
                    if let Some(snippet) = Self::extract_snippet(src, *span) {
                        output.push_str(&format!("\n\n{}", snippet));
                    }
                }

                output
            }
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
