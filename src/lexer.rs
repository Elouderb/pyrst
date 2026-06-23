use crate::diag::{Error, Result, Span};

/// Normalize platform line endings to a single `\n`.
///
/// `\r\n` (Windows/CRLF) collapses to `\n` and any remaining bare `\r`
/// (classic-Mac) also becomes `\n`. This MUST be applied at the point each
/// source string is READ FROM A FILE, before it is lexed AND before it is
/// stored for error rendering, so the lexer's byte/line/col spans and the
/// diagnostic renderer's `source.lines()` view operate on byte-identical text.
/// Normalizing inside `lex` alone would desync caret columns from a renderer
/// that holds the raw (still-CRLF) source.
///
/// `\n`-only input is returned untouched in spirit (the two `replace`s are
/// no-ops), so LF files are unaffected.
pub fn normalize_line_endings(src: &str) -> String {
    // Order matters: collapse CRLF first so a `\r\n` does not leave a stray
    // `\n` behind, then map any lone `\r` (old-Mac) to `\n`.
    src.replace("\r\n", "\n").replace('\r', "\n")
}

fn split_fstr_spec(s: &str) -> (String, Option<String>) {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 => {
                let expr = s[..i].trim().to_string();
                let spec = s[i+1..].trim().to_string();
                return (expr, Some(spec));
            }
            _ => {}
        }
    }
    (s.trim().to_string(), None)
}

#[derive(Debug, Clone, PartialEq)]
pub enum RawFStrPart {
    Lit(String),
    Interp(String, Option<String>),  // (expr_source, format_spec)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    FStr(Vec<RawFStrPart>),
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
    Assert,
    Raise,
    Try,
    Except,
    Finally,
    With,
    Del,
    Lambda,

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
    PercentAssign,  // %=
    DoubleSlashAssign,  // //=
    DoubleStarAssign,   // **=
    AmpAssign,    // &=
    PipeAssign,   // |=
    CaretAssign,  // ^=
    LShiftAssign, // <<=
    RShiftAssign, // >>=
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
    LShift,       // <<
    RShift,       // >>
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

/// Decode the full UTF-8 codepoint starting at byte offset `i` of `src`
/// (a valid `&str`). Returns the char, or None if `i` is not a char boundary
/// or is out of range — so callers never panic and never split a multi-byte
/// character into Latin-1 bytes.
fn char_at(src: &str, i: usize) -> Option<char> {
    src.get(i..).and_then(|rest| rest.chars().next())
}

/// Format a single byte for use in a diagnostic error message.
/// ASCII-printable bytes (0x21–0x7e) are shown as the character itself;
/// all others (non-ASCII, control chars) are shown as a `\xNN` hex escape
/// to avoid Latin-1 mojibake in error output.
fn fmt_byte(b: u8) -> String {
    if b.is_ascii_graphic() {
        format!("{}", b as char)
    } else {
        format!("\\x{:02x}", b)
    }
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

        // f-String literal
        if c == b'f' && i + 1 < bytes.len() && (bytes[i + 1] == b'"' || bytes[i + 1] == b'\'') {
            i += 1; // consume 'f'
            let quote = bytes[i];
            i += 1; // consume opening quote
            let mut parts = Vec::new();
            let mut current_lit = String::new();

            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'{' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                        // {{ → literal {
                        current_lit.push('{');
                        i += 2;
                    } else {
                        // Start of interpolation
                        if !current_lit.is_empty() {
                            parts.push(RawFStrPart::Lit(current_lit.clone()));
                            current_lit.clear();
                        }
                        i += 1; // consume {
                        let mut expr = String::new();
                        let mut brace_depth = 1;
                        while i < bytes.len() && brace_depth > 0 {
                            if bytes[i] == b'{' {
                                brace_depth += 1;
                            } else if bytes[i] == b'}' {
                                brace_depth -= 1;
                                if brace_depth == 0 { break; }
                            }
                            match char_at(src, i) {
                                Some(ch) => { expr.push(ch); i += ch.len_utf8(); }
                                None => return Err(Error::Lex {
                                    span: Span::new(i, i + 1, line, col),
                                    msg: "invalid UTF-8 byte in f-string interpolation".into(),
                                }),
                            }
                        }
                        if i >= bytes.len() {
                            return Err(Error::Lex {
                                span: Span::new(start, i, line, col),
                                msg: "unterminated f-string interpolation".into(),
                            });
                        }
                        let (expr_part, spec_part) = split_fstr_spec(&expr);
                        parts.push(RawFStrPart::Interp(expr_part, spec_part));
                        i += 1; // consume closing }
                    }
                } else if bytes[i] == b'}' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                        // }} → literal }
                        current_lit.push('}');
                        i += 2;
                    } else {
                        return Err(Error::Lex {
                            span: Span::new(i, i + 1, line, col),
                            msg: "unmatched } in f-string".into(),
                        });
                    }
                } else if bytes[i] == b'\\' && i + 1 < bytes.len() {
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
                                msg: format!("unknown escape '\\{}'", fmt_byte(other)),
                            });
                        }
                    };
                    current_lit.push(ch);
                    i += 2;
                } else if bytes[i] == b'\n' {
                    return Err(Error::Lex {
                        span: Span::new(start, i, line, col),
                        msg: "unterminated f-string".into(),
                    });
                } else {
                    match char_at(src, i) {
                        Some(ch) => { current_lit.push(ch); i += ch.len_utf8(); }
                        None => return Err(Error::Lex {
                            span: Span::new(i, i + 1, line, col),
                            msg: "invalid UTF-8 byte in f-string".into(),
                        }),
                    }
                }
            }
            if i >= bytes.len() {
                return Err(Error::Lex {
                    span: Span::new(start, i, line, col),
                    msg: "unterminated f-string".into(),
                });
            }
            if !current_lit.is_empty() {
                parts.push(RawFStrPart::Lit(current_lit));
            }
            i += 1; // consume closing quote
            tokens.push(Token {
                tok: Tok::FStr(parts),
                span: Span::new(start, i, line, col),
            });
            continue;
        }

        // String literal (single-line or triple-quoted)
        if c == b'"' || c == b'\'' {
            let quote = c;
            // Detect triple-quoted opener: """...""" or '''...'''
            let triple = i + 2 < bytes.len() && bytes[i + 1] == quote && bytes[i + 2] == quote;
            let mut s = String::new();
            if triple {
                i += 3; // consume the three opening quotes
                loop {
                    if i >= bytes.len() {
                        return Err(Error::Lex {
                            span: Span::new(start, i, line, col),
                            msg: "unterminated triple-quoted string".into(),
                        });
                    }
                    // Check for closing triple quote
                    if bytes[i] == quote
                        && i + 1 < bytes.len() && bytes[i + 1] == quote
                        && i + 2 < bytes.len() && bytes[i + 2] == quote
                    {
                        i += 3; // consume the three closing quotes
                        break;
                    }
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
                            b'\n' => {
                                // Backslash-newline: line continuation inside triple string
                                i += 2;
                                line += 1;
                                line_start = i;
                                continue;
                            }
                            other => {
                                return Err(Error::Lex {
                                    span: Span::new(i, i + 2, line, col),
                                    msg: format!("unknown escape '\\{}'", fmt_byte(other)),
                                });
                            }
                        };
                        s.push(ch);
                        i += 2;
                    } else if bytes[i] == b'\n' {
                        // Embedded newline: include it verbatim and track line/col
                        s.push('\n');
                        i += 1;
                        line += 1;
                        line_start = i;
                    } else {
                        match char_at(src, i) {
                            Some(ch) => { s.push(ch); i += ch.len_utf8(); }
                            None => return Err(Error::Lex {
                                span: Span::new(i, i + 1, line, col),
                                msg: "invalid UTF-8 byte in string literal".into(),
                            }),
                        }
                    }
                }
            } else {
                i += 1; // consume opening single quote
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
                                    msg: format!("unknown escape '\\{}'", fmt_byte(other)),
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
                        match char_at(src, i) {
                            Some(ch) => { s.push(ch); i += ch.len_utf8(); }
                            None => return Err(Error::Lex {
                                span: Span::new(i, i + 1, line, col),
                                msg: "invalid UTF-8 byte in string literal".into(),
                            }),
                        }
                    }
                }
                if i >= bytes.len() {
                    return Err(Error::Lex {
                        span: Span::new(start, i, line, col),
                        msg: "unterminated string".into(),
                    });
                }
                i += 1; // consume closing single quote
            }
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
                Tok::Float(text.parse().map_err(|_| Error::Lex {
                    span: Span::new(i, j, line, col),
                    msg: "invalid float literal".into(),
                })?)
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
                "assert" => Tok::Assert,
                "raise" => Tok::Raise,
                "try" => Tok::Try,
                "except" => Tok::Except,
                "finally" => Tok::Finally,
                "with" => Tok::With,
                "del" => Tok::Del,
                "lambda" => Tok::Lambda,
                "True" => Tok::True,
                "False" => Tok::False,
                "None" => Tok::None_,
                _ => Tok::Ident(text.to_string()),
            };
            tokens.push(Token { tok, span: Span::new(i, j, line, col) });
            i = j;
            continue;
        }

        // Multi-char operators (check 3-char first, then 2-char)
        let three = if i + 2 < bytes.len() {
            Some((bytes[i], bytes[i + 1], bytes[i + 2]))
        } else {
            None
        };
        let two = if i + 1 < bytes.len() {
            Some((bytes[i], bytes[i + 1]))
        } else {
            None
        };
        let (tok, len) = match three {
            Some((b'*', b'*', b'=')) => (Tok::DoubleStarAssign, 3),
            Some((b'/', b'/', b'=')) => (Tok::DoubleSlashAssign, 3),
            // `<<=` / `>>=` MUST be checked before the 2-char `<<` / `>>` cases
            // below, or the shift token would be emitted and the trailing `=`
            // would parse as a separate assignment.
            Some((b'<', b'<', b'=')) => (Tok::LShiftAssign, 3),
            Some((b'>', b'>', b'=')) => (Tok::RShiftAssign, 3),
            _ => match two {
                Some((b'-', b'>')) => (Tok::Arrow, 2),
                Some((b'=', b'=')) => (Tok::Eq, 2),
                Some((b'!', b'=')) => (Tok::Ne, 2),
                Some((b'<', b'=')) => (Tok::Le, 2),
                Some((b'>', b'=')) => (Tok::Ge, 2),
                Some((b'+', b'=')) => (Tok::PlusAssign, 2),
                Some((b'-', b'=')) => (Tok::MinusAssign, 2),
                Some((b'*', b'=')) => (Tok::StarAssign, 2),
                Some((b'/', b'=')) => (Tok::SlashAssign, 2),
                Some((b'%', b'=')) => (Tok::PercentAssign, 2),
                Some((b'&', b'=')) => (Tok::AmpAssign, 2),
                Some((b'|', b'=')) => (Tok::PipeAssign, 2),
                Some((b'^', b'=')) => (Tok::CaretAssign, 2),
                Some((b'*', b'*')) => (Tok::DoubleStar, 2),
                Some((b'/', b'/')) => (Tok::DoubleSlash, 2),
                Some((b'<', b'<')) => (Tok::LShift, 2),
                Some((b'>', b'>')) => (Tok::RShift, 2),
            _ => match c {
                b'(' => { bracket_depth += 1; (Tok::LParen, 1) }
                b')' => { bracket_depth = (bracket_depth - 1).max(0); (Tok::RParen, 1) }
                b'[' => { bracket_depth += 1; (Tok::LBracket, 1) }
                b']' => { bracket_depth = (bracket_depth - 1).max(0); (Tok::RBracket, 1) }
                b'{' => { bracket_depth += 1; (Tok::LBrace, 1) }
                b'}' => { bracket_depth = (bracket_depth - 1).max(0); (Tok::RBrace, 1) }
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
                        msg: format!("unexpected character '{}'", fmt_byte(other)),
                    });
                }
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

/// Return `true` if `src` contains a `#` comment that the lexer would discard.
///
/// The lexer (and codegen) silently drop comments, so `pyrst fmt` cannot yet
/// preserve them. This scanner mirrors the lexer's literal handling — the only
/// string forms are single-line `'...'` / `"..."` strings and `f'...'` /
/// `f"..."` f-strings (no triple-quoted or raw strings) — so a `#` reached
/// outside any literal is exactly a comment the lexer would throw away. This
/// lets `pyrst fmt` refuse to format such a file rather than delete comments.
pub fn has_comment(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        // f-string prefix: treat `f"`/`f'` as the start of an f-string literal.
        if c == b'f' && i + 1 < bytes.len() && (bytes[i + 1] == b'"' || bytes[i + 1] == b'\'') {
            i += 1; // consume 'f'; fall through to string scan below
            i = skip_string_literal(bytes, i);
            continue;
        }
        // String literal: skip to the matching closing quote, honoring `\`.
        if c == b'"' || c == b'\'' {
            i = skip_string_literal(bytes, i);
            continue;
        }
        // A bare `#` outside any literal is a comment.
        if c == b'#' {
            return true;
        }
        i += 1;
    }
    false
}

/// Advance past a string/f-string literal starting at the opening quote
/// (`bytes[start]` is `'` or `"`). Returns the index just past the closing
/// quote, or `bytes.len()` if the literal is unterminated. `\`-escapes are
/// honored so an escaped quote does not end the literal. Triple-quoted strings
/// (`"""..."""` or `'''...'''`) are handled: they consume embedded newlines and
/// single/double quotes until the matching 3-quote closer is found. For
/// f-strings this intentionally treats interpolations as ordinary characters:
/// we only need to avoid mistaking a `#` inside the literal for a comment.
fn skip_string_literal(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    // Detect triple-quoted opener
    let triple = start + 2 < bytes.len()
        && bytes[start + 1] == quote
        && bytes[start + 2] == quote;
    let mut i = if triple { start + 3 } else { start + 1 };
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            i += 2; // skip escaped char
            continue;
        }
        if triple {
            // Look for closing triple quote
            if b == quote
                && i + 1 < bytes.len() && bytes[i + 1] == quote
                && i + 2 < bytes.len() && bytes[i + 2] == quote
            {
                return i + 3; // past closing triple quote
            }
            // Embedded newlines and single quotes are allowed; just advance.
            i += 1;
        } else {
            if b == quote {
                return i + 1; // past closing quote
            }
            if b == b'\n' {
                // Unterminated single-line string; stop so we don't run away.
                // The lexer would reject this, but for comment detection we bail.
                return i + 1;
            }
            i += 1;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        lex(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn has_comment_detects_real_comments() {
        assert!(has_comment("x = 1  # trailing comment\n"));
        assert!(has_comment("# leading comment\nx = 1\n"));
        assert!(has_comment("    # indented comment\n"));
    }

    #[test]
    fn has_comment_ignores_hash_in_strings() {
        // `#` inside string / f-string literals must NOT be treated as a comment.
        assert!(!has_comment("x = \"#not a comment\"\n"));
        assert!(!has_comment("x = '#also not'\n"));
        assert!(!has_comment("x = f\"value #{n}\"\n"));
        assert!(!has_comment("x = \"a\\\"# still in string\"\n"));
    }

    #[test]
    fn has_comment_false_when_absent() {
        assert!(!has_comment("def main() -> None:\n    x: int = 1\n"));
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

    // ───────── EPIC-10 Card 1 Part A: CRLF / bare-CR normalization ─────────

    /// Normalizing CRLF (`\r\n`) yields exactly the LF text, so a Windows-saved
    /// file lexes identically — same token KINDS and same SPANS (line/col/byte) —
    /// to its LF twin. Equal spans is the property that keeps error carets
    /// correctly positioned, since the renderer indexes source by span.line/col.
    #[test]
    fn crlf_normalizes_and_lexes_identically_to_lf() {
        let lf = "a = 1\nb = 2\n";
        let crlf = "a = 1\r\nb = 2\r\n";

        // The normalizer reproduces the LF text byte-for-byte.
        assert_eq!(normalize_line_endings(crlf), lf);

        // And lexing the normalized CRLF gives the same tokens AND spans as LF.
        let lf_tokens = lex(lf).unwrap();
        let crlf_tokens = lex(&normalize_line_endings(crlf)).unwrap();
        assert_eq!(lf_tokens.len(), crlf_tokens.len());
        for (a, b) in lf_tokens.iter().zip(crlf_tokens.iter()) {
            assert_eq!(a.tok, b.tok, "token kinds must match");
            assert_eq!(a.span.start, b.span.start, "byte start must match");
            assert_eq!(a.span.end, b.span.end, "byte end must match");
            assert_eq!(a.span.line, b.span.line, "line must match");
            assert_eq!(a.span.col, b.span.col, "col must match");
        }
    }

    /// A bare `\r` (classic-Mac line ending) also normalizes to `\n` and lexes
    /// identically to the LF twin. Without normalization the lexer's catch-all
    /// would reject `\r` as an unexpected character.
    #[test]
    fn bare_cr_normalizes_and_lexes_identically_to_lf() {
        let lf = "a = 1\nb = 2\n";
        let cr = "a = 1\rb = 2\r";

        assert_eq!(normalize_line_endings(cr), lf);

        // Raw bare-CR source fails to lex today (catch-all rejects '\r')...
        assert!(lex(cr).is_err(), "raw bare-CR must fail to lex without normalization");
        // ...but the normalized form matches the LF token stream exactly.
        let lf_tokens = lex(lf).unwrap();
        let cr_tokens = lex(&normalize_line_endings(cr)).unwrap();
        assert_eq!(lf_tokens.len(), cr_tokens.len());
        for (a, b) in lf_tokens.iter().zip(cr_tokens.iter()) {
            assert_eq!(a.tok, b.tok);
            assert_eq!(a.span.start, b.span.start);
            assert_eq!(a.span.line, b.span.line);
            assert_eq!(a.span.col, b.span.col);
        }
    }

    /// LF-only source is unaffected by the normalizer (no spurious rewrites).
    #[test]
    fn lf_source_passes_through_unchanged() {
        let lf = "def main() -> None:\n    x: int = 1\n";
        assert_eq!(normalize_line_endings(lf), lf);
    }

    // ───────── EPIC-10 Card 1 Part B: new aug-assign operator tokens ─────────

    /// All six previously-missing augmented-assignment operators lex to their
    /// dedicated single tokens. `<<=` / `>>=` must lex as ONE 3-char token, not
    /// a shift token followed by a stray `=` — the 3-char cases are checked
    /// before the 2-char `<<` / `>>` cases.
    #[test]
    fn new_aug_assign_operators_lex() {
        assert_eq!(kinds("x **= 2\n")[1], Tok::DoubleStarAssign);
        assert_eq!(kinds("x &= 2\n")[1], Tok::AmpAssign);
        assert_eq!(kinds("x |= 2\n")[1], Tok::PipeAssign);
        assert_eq!(kinds("x ^= 2\n")[1], Tok::CaretAssign);
        assert_eq!(kinds("x <<= 2\n")[1], Tok::LShiftAssign);
        assert_eq!(kinds("x >>= 2\n")[1], Tok::RShiftAssign);

        // Precedence guard: `<<=` is a single token, so the stream after the
        // target ident is exactly [LShiftAssign, Int(2), Newline, Eof] — there
        // is NO separate Assign token.
        let toks = kinds("x <<= 2\n");
        assert_eq!(toks[1], Tok::LShiftAssign);
        assert_eq!(toks[2], Tok::Int(2));
        assert!(!toks.contains(&Tok::Assign), "<<= must not split into << then =");
        // And bare `<<` / `>>` still lex as the 2-char shift tokens.
        assert_eq!(kinds("x << 2\n")[1], Tok::LShift);
        assert_eq!(kinds("x >> 2\n")[1], Tok::RShift);
    }
}
