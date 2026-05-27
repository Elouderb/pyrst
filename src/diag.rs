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
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self { Error::Io(e) }
}

pub type Result<T> = std::result::Result<T, Error>;
