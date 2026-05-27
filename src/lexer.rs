use crate::diag::{Error, Result, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    True,
    False,
    None_,

    // Identifiers / keywords
    Ident(String),
    Def,
    Return,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    Class,
    Pass,
    Import,
    From,
    As,
    Match,
    Case,
    Break,
    Continue,
    And,
    Or,
    Not,
    Is,

    // Punctuation
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Semicolon,
    Dot,
    Arrow,        // ->
    Assign,       // =
    PlusAssign,   // +=
    MinusAssign,  // -=
    StarAssign,   // *=
    SlashAssign,  // /=
    Plus,
    Minus,
    Star,
    DoubleStar,   // **
    Slash,
    DoubleSlash,  // //
    Percent,
    Eq,           // ==
    Ne,           // !=
    Lt,
    Le,
    Gt,
    Ge,
    Amp,
    Pipe,
    Caret,
    Tilde,
    At,           // for decorators

    // Layout
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub fn lex(src: &str) -> Result<Vec<Token>> {
    let bytes = src.as_bytes();
    let mut tokens: Vec<Token> = Vec::new();
    let mut indent_stack: Vec<usize> = vec![0];
    let mut i: usize = 0;
    let mut line: u32 = 1;
    let mut line_start: usize = 0;
    let mut at_line_start = true;
    // Track logical-line continuation via paren depth (Python rule:
    // newlines inside (), [], {} are not statement-ending).
    let mut bracket_depth: i32 = 0;

    while i < bytes.len() {
        // Beginning of a logical line — handle indentation.
        if at_line_start && bracket_depth == 0 {
            let mut indent = 0usize;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                indent += if bytes[i] == b'\t' { 8 } else { 1 };
                i += 1;
            }
            // Skip blank lines and comment-only lines without emitting indent changes.
            if i < bytes.len() && (bytes[i] == b'\n' || bytes[i] == b'#') {
                // fall through to normal handling below
            } else if i >= bytes.len() {
                break;
            } else {
                let cur = *indent_stack.last().unwrap();
                if indent > cur {
                    indent_stack.push(indent);
                    tokens.push(Token {
                        tok: Tok::Indent,
                        span: Span::new(i, i, line, (i - line_start) as u32 + 1),
                    });
                } else {
                    while indent < *indent_stack.last().unwrap() {
                        indent_stack.pop();
                        tokens.push(Token {
                            tok: Tok::Dedent,
                            span: Span::new(i, i, line, (i - line_start) as u32 + 1),
                        });
                    }
                    if indent != *indent_stack.last().unwrap() {
                        return Err(Error::Lex {
                            span: Span::new(i, i, line, (i - line_start) as u32 + 1),
                            msg: "inconsistent indentation".into(),
                        });
                    }
                }
            }
            at_line_start = false;
            continue;
        }

        let c = bytes[i];
        let col = (i - line_start) as u32 + 1;
        let start = i;

        // Whitespace within a line
        if c == b' ' || c == b'\t' {
            i += 1;
            continue;
        }
        // Line continuation: backslash-newline
        if c == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            i += 2;
            line += 1;
            line_start = i;
            continue;
        }
        // Comment
        if c == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Newline
        if c == b'\n' {
            if bracket_depth == 0 {
                // Suppress consecutive Newlines
                if !matches!(tokens.last().map(|t| &t.tok), Some(Tok::Newline) | None) {
                    tokens.push(Token {
                        tok: Tok::Newline,
                        span: Span::new(start, start + 1, line, col),
                    });
                }
                at_line_start = true;
            }
            i += 1;
            line += 1;
            line_start = i;
            continue;
        }

        // String literal
        if c == b'"' || c == b'\'' {
            let quote = c;
            i += 1;
            let mut s = String::new();
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    let esc = bytes[i + 1];
                    let ch = match esc {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'\\' => '\\',
                        b'\'' => '\'',
                        b'"' => '"',
                        b'0' => '\0',
                        other => {
                            return Err(Error::Lex {
                                span: Span::new(i, i + 2, line, col),
                                msg: format!("unknown escape '\\{}'", other as char),
                            });
                        }
                    };
                    s.push(ch);
                    i += 2;
                } else if bytes[i] == b'\n' {
                    return Err(Error::Lex {
                        span: Span::new(start, i, line, col),
                        msg: "unterminated string".into(),
                    });
                } else {
                    s.push(bytes[i] as char);
                    i += 1;
                }
            }
            if i >= bytes.len() {
                return Err(Error::Lex {
                    span: Span::new(start, i, line, col),
                    msg: "unterminated string".into(),
                });
            }
            i += 1;
            tokens.push(Token {
                tok: Tok::Str(s),
                span: Span::new(start, i, line, col),
            });
            continue;
        }

        // Number
        if c.is_ascii_digit() {
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let mut is_float = false;
            if j < bytes.len() && bytes[j] == b'.'
                && j + 1 < bytes.len() && bytes[j + 1].is_ascii_digit()
            {
                is_float = true;
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
            }
            let text = std::str::from_utf8(&bytes[i..j]).unwrap();
            let tok = if is_float {
                Tok::Float(text.parse().unwrap())
            } else {
                Tok::Int(text.parse().map_err(|_| Error::Lex {
                    span: Span::new(i, j, line, col),
                    msg: "integer literal out of range".into(),
                })?)
            };
            tokens.push(Token { tok, span: Span::new(i, j, line, col) });
            i = j;
            continue;
        }

        // Identifier / keyword
        if c == b'_' || c.is_ascii_alphabetic() {
            let mut j = i;
            while j < bytes.len() && (bytes[j] == b'_' || bytes[j].is_ascii_alphanumeric()) {
                j += 1;
            }
            let text = std::str::from_utf8(&bytes[i..j]).unwrap();
            let tok = match text {
                "def" => Tok::Def,
                "return" => Tok::Return,
                "if" => Tok::If,
                "elif" => Tok::Elif,
                "else" => Tok::Else,
                "while" => Tok::While,
                "for" => Tok::For,
                "in" => Tok::In,
                "class" => Tok::Class,
                "pass" => Tok::Pass,
                "import" => Tok::Import,
                "from" => Tok::From,
                "as" => Tok::As,
                "match" => Tok::Match,
                "case" => Tok::Case,
                "break" => Tok::Break,
                "continue" => Tok::Continue,
                "and" => Tok::And,
                "or" => Tok::Or,
                "not" => Tok::Not,
                "is" => Tok::Is,
                "True" => Tok::True,
                "False" => Tok::False,
                "None" => Tok::None_,
                _ => Tok::Ident(text.to_string()),
            };
            tokens.push(Token { tok, span: Span::new(i, j, line, col) });
            i = j;
            continue;
        }

        // Multi-char operators
        let two = if i + 1 < bytes.len() {
            Some((bytes[i], bytes[i + 1]))
        } else {
            None
        };
        let (tok, len) = match two {
            Some((b'-', b'>')) => (Tok::Arrow, 2),
            Some((b'=', b'=')) => (Tok::Eq, 2),
            Some((b'!', b'=')) => (Tok::Ne, 2),
            Some((b'<', b'=')) => (Tok::Le, 2),
            Some((b'>', b'=')) => (Tok::Ge, 2),
            Some((b'+', b'=')) => (Tok::PlusAssign, 2),
            Some((b'-', b'=')) => (Tok::MinusAssign, 2),
            Some((b'*', b'=')) => (Tok::StarAssign, 2),
            Some((b'/', b'=')) => (Tok::SlashAssign, 2),
            Some((b'*', b'*')) => (Tok::DoubleStar, 2),
            Some((b'/', b'/')) => (Tok::DoubleSlash, 2),
            _ => match c {
                b'(' => { bracket_depth += 1; (Tok::LParen, 1) }
                b')' => { bracket_depth -= 1; (Tok::RParen, 1) }
                b'[' => { bracket_depth += 1; (Tok::LBracket, 1) }
                b']' => { bracket_depth -= 1; (Tok::RBracket, 1) }
                b'{' => { bracket_depth += 1; (Tok::LBrace, 1) }
                b'}' => { bracket_depth -= 1; (Tok::RBrace, 1) }
                b',' => (Tok::Comma, 1),
                b':' => (Tok::Colon, 1),
                b';' => (Tok::Semicolon, 1),
                b'.' => (Tok::Dot, 1),
                b'=' => (Tok::Assign, 1),
                b'+' => (Tok::Plus, 1),
                b'-' => (Tok::Minus, 1),
                b'*' => (Tok::Star, 1),
                b'/' => (Tok::Slash, 1),
                b'%' => (Tok::Percent, 1),
                b'<' => (Tok::Lt, 1),
                b'>' => (Tok::Gt, 1),
                b'&' => (Tok::Amp, 1),
                b'|' => (Tok::Pipe, 1),
                b'^' => (Tok::Caret, 1),
                b'~' => (Tok::Tilde, 1),
                b'@' => (Tok::At, 1),
                other => {
                    return Err(Error::Lex {
                        span: Span::new(i, i + 1, line, col),
                        msg: format!("unexpected character '{}'", other as char),
                    });
                }
            },
        };
        tokens.push(Token { tok, span: Span::new(i, i + len, line, col) });
        i += len;
    }

    // Final newline + close any open indents
    if !matches!(tokens.last().map(|t| &t.tok), Some(Tok::Newline) | None) {
        tokens.push(Token {
            tok: Tok::Newline,
            span: Span::new(i, i, line, (i - line_start) as u32 + 1),
        });
    }
    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(Token {
            tok: Tok::Dedent,
            span: Span::new(i, i, line, 1),
        });
    }
    tokens.push(Token {
        tok: Tok::Eof,
        span: Span::new(i, i, line, 1),
    });

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        lex(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn simple_def() {
        let toks = kinds("def main() -> None:\n    pass\n");
        // def main ( ) -> None : NL INDENT pass NL DEDENT EOF
        assert!(matches!(toks[0], Tok::Def));
        assert!(matches!(toks[1], Tok::Ident(ref s) if s == "main"));
        assert!(matches!(toks[2], Tok::LParen));
        assert!(matches!(toks[3], Tok::RParen));
        assert!(matches!(toks[4], Tok::Arrow));
        assert!(matches!(toks[5], Tok::None_));
        assert!(matches!(toks[6], Tok::Colon));
        assert!(matches!(toks[7], Tok::Newline));
        assert!(matches!(toks[8], Tok::Indent));
        assert!(matches!(toks[9], Tok::Pass));
        assert!(matches!(toks[10], Tok::Newline));
        assert!(matches!(toks[11], Tok::Dedent));
        assert!(matches!(toks[12], Tok::Eof));
    }

    #[test]
    fn string_and_number() {
        let toks = kinds("x = \"hi\\n\"\ny = 3.14\n");
        assert!(matches!(toks[0], Tok::Ident(ref s) if s == "x"));
        assert!(matches!(toks[1], Tok::Assign));
        assert!(matches!(toks[2], Tok::Str(ref s) if s == "hi\n"));
    }
}
