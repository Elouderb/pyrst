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
    /// A `bytes` literal `b'...'` / `b"..."` (W5-a). Carries the DECODED raw
    /// bytes (escapes already applied), mirroring `Str(String)` but at the
    /// byte level: a `bytes` holds arbitrary 0x00–0xff values (not UTF-8).
    Bytes(Vec<u8>),
    True,
    False,
    None_,

    // Identifiers / keywords
    Ident(String),
    Def,
    Return,
    Yield,
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
    Global,
    Nonlocal,

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

/// Decode ONE backslash escape sequence starting at `bytes[i]` (the backslash),
/// shared by the four literal lexers (single-line `str`, triple-quoted `str`,
/// f-string, and the `bytes` literal) so the escape table can never drift.
///
/// Returns the decoded BYTE value and the index just PAST the whole escape (2
/// for a simple escape like `\n`, 4 for a `\xNN` hex escape). The nine common
/// escapes `\n \t \r \\ \' \" \0 \b \f` decode identically in both modes and all
/// yield an ASCII byte < 0x80, so a `str`/f-string caller losslessly re-widens
/// the result with `as char`.
///
/// `byte_mode` selects BYTES-literal semantics (W5): it ENABLES the `\xNN` hex
/// escape (one raw 0x00–0xff byte) and honestly REJECTS a multi-digit octal
/// escape (`\012`), which pyrst does not implement — silently lowering it to
/// `NUL` + literal digits would diverge from CPython's `\012 == 0x0a`. In
/// `str`/f-string mode `\x` stays an honest "unknown escape" (a Unicode `\x`
/// for `str` is a separate, deferred lexer item) and a bare `\0` keeps its
/// legacy NUL-then-literal behaviour untouched.
///
/// The caller guarantees `bytes[i] == b'\\'` and `i + 1 < bytes.len()`. A
/// backslash-newline line continuation is caller-specific (only triple-quoted
/// strings honour it) and is handled BEFORE this helper is reached.
fn lex_escape(bytes: &[u8], i: usize, line: u32, col: u32, byte_mode: bool) -> Result<(u8, usize)> {
    let esc = bytes[i + 1];
    let val: u8 = match esc {
        b'n' => b'\n',
        b't' => b'\t',
        b'r' => b'\r',
        b'\\' => b'\\',
        b'\'' => b'\'',
        b'"' => b'"',
        b'0' => {
            // A `\0` followed by another octal digit (`\012`) is a multi-digit
            // OCTAL escape, which pyrst does not support. In BYTES mode reject it
            // honestly (octal is a documented deferral) rather than emit NUL +
            // literal digits — CPython reads `\012` as the single byte 0x0a, so
            // the silent lowering would be a miscompile. `str`/f-string mode keeps
            // its existing NUL-then-literal behaviour (out of scope here).
            if byte_mode && i + 2 < bytes.len() && (b'0'..=b'7').contains(&bytes[i + 2]) {
                return Err(Error::Lex {
                    span: Span::new(i, i + 3, line, col),
                    msg: "octal escapes (\\ooo) are not supported in bytes literals; use \\xNN".into(),
                });
            }
            0
        }
        b'b' => 0x08,
        b'f' => 0x0C,
        b'x' if byte_mode => {
            // `\xNN`: EXACTLY two hex digits -> one raw byte (CPython rejects
            // `\x`/`\x4` with "invalid \x escape").
            match (bytes.get(i + 2).copied(), bytes.get(i + 3).copied()) {
                (Some(a), Some(b)) if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() => {
                    let hi = (a as char).to_digit(16).unwrap() as u8;
                    let lo = (b as char).to_digit(16).unwrap() as u8;
                    return Ok((hi * 16 + lo, i + 4));
                }
                _ => {
                    return Err(Error::Lex {
                        span: Span::new(i, (i + 4).min(bytes.len()), line, col),
                        msg: "invalid \\x escape: expected two hex digits (e.g. \\x1f)".into(),
                    });
                }
            }
        }
        other => {
            return Err(Error::Lex {
                span: Span::new(i, i + 2, line, col),
                msg: format!("unknown escape '\\{}'", fmt_byte(other)),
            });
        }
    };
    Ok((val, i + 2))
}

/// Lex a numeric literal starting at `bytes[i]` — a decimal digit, or a `.`
/// immediately followed by a digit (the leading-dot float `.5`). Returns the
/// token and the index just past it.
///
/// Implements CPython's numeric-literal grammar (every accept/reject below
/// python3-diffed): base prefixes `0x`/`0o`/`0b` (case-insensitive) parsed as
/// i64; underscore separators allowed STRICTLY between two base digits (plus one
/// optional `_` right after a base prefix, `0x_FF`) and an error anywhere else
/// (`1__0`, `1000_`, `1e_5`, `0xFF_`); a nonzero DECIMAL leading zero rejected
/// (`007`/`08`) while all-zero (`0`, `00`, `0_0`) is allowed; fractions and
/// `e`/`E` exponents with an optional sign and REQUIRED digits (`1e`, `1e+` are
/// errors). Trailing-dot (`1.`) is deliberately NOT consumed as a float — it
/// stays int + `.` so a digit-started token followed by `.` then a non-digit can
/// never silently change meaning; the (rare) dot-then-exponent `1.e3` is thus an
/// honest downstream parse error rather than a silent miscompile. Values are
/// i64/f64; anything out of that range is an honest Lex error.
fn lex_number(bytes: &[u8], i: usize, line: u32, col: u32) -> Result<(Tok, usize)> {
    let len = bytes.len();
    let lex_err = |end: usize, msg: &str| Error::Lex {
        span: Span::new(i, end, line, col),
        msg: msg.into(),
    };
    // Consume `digit (_ digit | digit)*` for `is_d`: an underscore advances ONLY
    // when the next byte is a valid digit, so it is never leading, trailing, or
    // doubled. Returns (end_index, digit_count).
    fn scan<F: Fn(u8) -> bool>(bytes: &[u8], start: usize, is_d: F) -> (usize, usize) {
        let len = bytes.len();
        if start >= len || !is_d(bytes[start]) {
            return (start, 0);
        }
        let mut j = start;
        let mut count = 0usize;
        loop {
            if j < len && is_d(bytes[j]) {
                count += 1;
                j += 1;
            } else if j + 1 < len && bytes[j] == b'_' && is_d(bytes[j + 1]) {
                j += 1;
            } else {
                break;
            }
        }
        (j, count)
    }

    // --- base-prefixed integer: 0x / 0o / 0b (case-insensitive) ---
    if bytes[i] == b'0' && i + 1 < len && matches!(bytes[i + 1], b'x' | b'X' | b'o' | b'O' | b'b' | b'B') {
        let (radix, name): (u32, &str) = match bytes[i + 1] {
            b'x' | b'X' => (16, "hexadecimal"),
            b'o' | b'O' => (8, "octal"),
            _ => (2, "binary"),
        };
        let is_d = move |b: u8| match radix {
            16 => b.is_ascii_hexdigit(),
            8 => (b'0'..=b'7').contains(&b),
            _ => b == b'0' || b == b'1',
        };
        let mut j = i + 2;
        // One optional underscore is permitted directly after the base prefix.
        if j < len && bytes[j] == b'_' {
            j += 1;
        }
        let (j2, cnt) = scan(bytes, j, &is_d);
        j = j2;
        if cnt == 0 {
            return Err(lex_err(j, &format!("invalid {} integer literal", name)));
        }
        if j < len && bytes[j] == b'_' {
            return Err(lex_err(j + 1, &format!("invalid {} integer literal: trailing underscore", name)));
        }
        let cleaned: String = bytes[(i + 2)..j]
            .iter()
            .filter(|&&b| b != b'_')
            .map(|&b| b as char)
            .collect();
        let value = i64::from_str_radix(&cleaned, radix)
            .map_err(|_| lex_err(j, "integer literal out of range"))?;
        return Ok((Tok::Int(value), j));
    }

    // --- decimal integer or float ---
    let mut j = i;
    let mut is_float = false;
    // Integer part (empty only for the leading-dot form, where bytes[i] == '.').
    let (j2, _int_cnt) = scan(bytes, j, |b| b.is_ascii_digit());
    j = j2;
    if j < len && bytes[j] == b'_' {
        return Err(lex_err(j + 1, "invalid decimal literal: misplaced underscore"));
    }
    // A '.' immediately followed by '_' is a malformed numeric literal (CPython
    // rejects `1._5`). pyrst has no `<int-literal>._attr` form, so reject it
    // honestly here instead of lexing `1` + `.` + `_5` (attribute access that
    // would silently build-fail).
    if j < len && bytes[j] == b'.' && j + 1 < len && bytes[j + 1] == b'_' {
        return Err(lex_err(j + 2, "invalid decimal literal: misplaced underscore"));
    }
    // Fraction: a '.' consumed only when a digit follows it (leading-dot `.5`
    // also lands here with an empty integer part). Trailing-dot is left alone.
    if j < len && bytes[j] == b'.' && j + 1 < len && bytes[j + 1].is_ascii_digit() {
        is_float = true;
        j += 1;
        let (j3, _frac) = scan(bytes, j, |b| b.is_ascii_digit());
        j = j3;
        if j < len && bytes[j] == b'_' {
            return Err(lex_err(j + 1, "invalid decimal literal: misplaced underscore"));
        }
    }
    // Exponent: (e|E) [+|-] digits — the digits are REQUIRED.
    if j < len && (bytes[j] == b'e' || bytes[j] == b'E') {
        let mut k = j + 1;
        if k < len && (bytes[k] == b'+' || bytes[k] == b'-') {
            k += 1;
        }
        if k < len && bytes[k].is_ascii_digit() {
            is_float = true;
            let (j4, _exp) = scan(bytes, k, |b| b.is_ascii_digit());
            j = j4;
            if j < len && bytes[j] == b'_' {
                return Err(lex_err(j + 1, "invalid decimal literal: misplaced underscore"));
            }
        } else {
            return Err(lex_err(k, "invalid float literal: exponent has no digits"));
        }
    }

    let cleaned: String = bytes[i..j]
        .iter()
        .filter(|&&b| b != b'_')
        .map(|&b| b as char)
        .collect();
    if is_float {
        let v = cleaned.parse::<f64>().map_err(|_| lex_err(j, "invalid float literal"))?;
        Ok((Tok::Float(v), j))
    } else {
        // CPython forbids a nonzero decimal with a leading zero (`007`, `08`), but
        // allows an all-zero run (`0`, `00`, `0_0`).
        if cleaned.len() > 1 && cleaned.starts_with('0') && cleaned.bytes().any(|b| b != b'0') {
            return Err(lex_err(j, "invalid decimal literal: leading zeros are not permitted (use 0o for octal)"));
        }
        let v = cleaned.parse::<i64>().map_err(|_| lex_err(j, "integer literal out of range"))?;
        Ok((Tok::Int(v), j))
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
                    // Shared escape table (`str`/f-string mode: `\x` stays an
                    // honest "unknown escape"). A common escape yields an ASCII
                    // byte < 0x80, so `as char` is lossless.
                    let (val, next) = lex_escape(bytes, i, line, col, false)?;
                    current_lit.push(val as char);
                    i = next;
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

        // (W5-a) Raw-bytes prefixes `rb'...'` / `br'...'` (ANY case: rB/Rb/RB/bR/…)
        // are a documented deferral — reject them HONESTLY here, BEFORE the bytes
        // and identifier scans. Otherwise `rb'\x41'` lexes as the identifier `rb`
        // plus a `str` `'\x41'`, whose `\x` then fails with a MISLEADING "unknown
        // escape" error — pointing at the escape when it is the raw-bytes PREFIX
        // that is unsupported. `c | 0x20` lowercases an ASCII letter, so a single
        // match covers all eight case combinations; a non-letter never maps onto
        // `r`/`b`, so only genuine `rb`/`br` prefixes are caught. CPython accepts
        // rb/br; pyrst defers them (raw-bytes decoding is a later wave).
        if i + 2 < bytes.len()
            && (bytes[i + 2] == b'"' || bytes[i + 2] == b'\'')
            && matches!((c | 0x20, bytes[i + 1] | 0x20), (b'r', b'b') | (b'b', b'r'))
        {
            return Err(Error::Lex {
                span: Span::new(start, i + 2, line, col),
                msg: "raw bytes literals (rb'...' / br'...') are not supported".into(),
            });
        }

        // Bytes literal `b'...'` / `b"..."` (W5-a). A `b`/`B` IMMEDIATELY followed
        // by a quote is the bytes prefix (mirroring the `f` prefix above); a bare
        // `b`, `banana`, or `bytes(` is an ordinary identifier handled below. The
        // decoded token holds raw bytes (arbitrary 0x00–0xff), never a UTF-8
        // `String`. Single-line only in W5-a: triple-quoted bytes are rejected
        // honestly (deferred), and a raw non-ASCII source byte is a CPython
        // SyntaxError ("bytes can only contain ASCII literal characters").
        if (c == b'b' || c == b'B') && i + 1 < bytes.len() && (bytes[i + 1] == b'"' || bytes[i + 1] == b'\'') {
            i += 1; // consume the b/B prefix
            let quote = bytes[i];
            // Triple-quoted bytes (`b'''...'''`) are a documented W5 deferral —
            // reject honestly rather than mis-lex the first two quotes as `b''`.
            if i + 2 < bytes.len() && bytes[i + 1] == quote && bytes[i + 2] == quote {
                return Err(Error::Lex {
                    span: Span::new(start, i + 3, line, col),
                    msg: "triple-quoted bytes literals are not yet supported".into(),
                });
            }
            i += 1; // consume opening quote
            let mut buf: Vec<u8> = Vec::new();
            while i < bytes.len() && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    // Byte-valued escape path: the shared table PLUS `\xNN`.
                    let (val, next) = lex_escape(bytes, i, line, col, true)?;
                    buf.push(val);
                    i = next;
                } else if bytes[i] == b'\n' {
                    return Err(Error::Lex {
                        span: Span::new(start, i, line, col),
                        msg: "unterminated bytes literal".into(),
                    });
                } else if bytes[i] >= 0x80 {
                    // A raw non-ASCII source byte cannot appear in a bytes literal
                    // (CPython SyntaxError). Use `\xNN` to encode a high byte.
                    return Err(Error::Lex {
                        span: Span::new(i, i + 1, line, col),
                        msg: "bytes can only contain ASCII literal characters (use \\xNN for a byte >= 0x80)".into(),
                    });
                } else {
                    buf.push(bytes[i]);
                    i += 1;
                }
            }
            if i >= bytes.len() {
                return Err(Error::Lex {
                    span: Span::new(start, i, line, col),
                    msg: "unterminated bytes literal".into(),
                });
            }
            i += 1; // consume closing quote
            tokens.push(Token {
                tok: Tok::Bytes(buf),
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
                        // Backslash-newline is a line continuation inside a triple
                        // string (caller-specific — not part of the shared escape
                        // table), so handle it BEFORE delegating to `lex_escape`.
                        if bytes[i + 1] == b'\n' {
                            i += 2;
                            line += 1;
                            line_start = i;
                            continue;
                        }
                        let (val, next) = lex_escape(bytes, i, line, col, false)?;
                        s.push(val as char);
                        i = next;
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
                        // Shared escape table (`str` mode).
                        let (val, next) = lex_escape(bytes, i, line, col, false)?;
                        s.push(val as char);
                        i = next;
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

        // Number — decimal / hex / octal / binary ints (with `_` separators),
        // floats, scientific notation, and the leading-dot float `.5`. The `.`
        // case is intercepted HERE (before the Dot operator below) only when a
        // digit follows, so ordinary attribute access `obj.attr` is untouched.
        // The full CPython numeric grammar lives in `lex_number`.
        if c.is_ascii_digit()
            || (c == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit())
        {
            let (tok, j) = lex_number(bytes, i, line, col)?;
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
                "yield" => Tok::Yield,
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
                "global" => Tok::Global,
                "nonlocal" => Tok::Nonlocal,
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

    // ───────── W5-a: bytes literals + the shared escape helper ─────────

    /// `b'...'` / `b"..."` (and the `B` prefix) lex to `Tok::Bytes` with the
    /// escapes already decoded; a bare `b`/`bytes` stays an identifier.
    #[test]
    fn bytes_literal_lexes_to_tok_bytes() {
        assert_eq!(kinds("x = b'ABC'\n")[2], Tok::Bytes(vec![65, 66, 67]));
        assert_eq!(kinds("x = b\"ABC\"\n")[2], Tok::Bytes(vec![65, 66, 67]));
        assert_eq!(kinds("x = B'AB'\n")[2], Tok::Bytes(vec![65, 66]));
        assert_eq!(kinds("x = b''\n")[2], Tok::Bytes(vec![]));
        // A `b`/`B` NOT followed by a quote is an ordinary identifier.
        assert_eq!(kinds("b = 5\n")[0], Tok::Ident("b".to_string()));
        assert_eq!(kinds("x = bytes(3)\n")[2], Tok::Ident("bytes".to_string()));
    }

    /// The byte-valued escape path: the nine shared escapes plus the new `\xNN`
    /// hex, producing raw bytes (including 0x80–0xff, which no `str` can hold).
    #[test]
    fn bytes_escape_table_and_hex() {
        assert_eq!(kinds("x = b'\\x00A\\x7f\\x80\\xff'\n")[2],
                   Tok::Bytes(vec![0x00, 0x41, 0x7f, 0x80, 0xff]));
        assert_eq!(kinds("x = b'\\n\\t\\r'\n")[2], Tok::Bytes(vec![0x0a, 0x09, 0x0d]));
        assert_eq!(kinds("x = b'\\\\'\n")[2], Tok::Bytes(vec![0x5c]));
        assert_eq!(kinds("x = b'\\0\\b\\f'\n")[2], Tok::Bytes(vec![0x00, 0x08, 0x0c]));
        assert_eq!(kinds("x = b'\\''\n")[2], Tok::Bytes(vec![0x27]));
    }

    /// Honest rejects (iron rule: an error beats a silent miscompile).
    #[test]
    fn bytes_literal_honest_rejects() {
        assert!(lex("x = b'\\x4'\n").is_err(), "\\x needs two hex digits");
        assert!(lex("x = b'\\xGG'\n").is_err(), "\\x needs HEX digits");
        assert!(lex("x = b'\\012'\n").is_err(), "octal escapes are deferred");
        assert!(lex("x = b'\\q'\n").is_err(), "unknown escape rejected");
        assert!(lex("x = b'\u{e9}'\n").is_err(), "raw non-ASCII byte rejected");
        assert!(lex("x = b'''ab'''\n").is_err(), "triple-quoted bytes deferred");
        assert!(lex("x = b'ab\n").is_err(), "unterminated bytes literal");
        // `\x` remains a bytes-ONLY escape: a `str` `\x` is still an error.
        assert!(lex("x = \"\\x41\"\n").is_err(), "str \\x stays unknown-escape");
    }

    /// The escape-helper refactor must keep `str`/f-string decoding byte-identical.
    #[test]
    fn str_escape_unchanged_after_refactor() {
        assert_eq!(kinds("x = \"a\\nb\\t\\\\\"\n")[2], Tok::Str("a\nb\t\\".to_string()));
        assert_eq!(kinds("x = \"\\0\"\n")[2], Tok::Str("\0".to_string()));
        assert_eq!(kinds("x = '\\'q'\n")[2], Tok::Str("'q".to_string()));
    }
}
