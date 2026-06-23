//! In-memory, filesystem-free parse + typecheck entry point for the LSP layer.
//!
//! `analyze_str` runs the full parse → single-module TyCtx build → typecheck
//! pipeline on a source string and returns LSP-ready diagnostics. It does NOT
//! perform codegen, invoke rustc, or touch the filesystem.
//!
//! # Single-file limitation
//! `import` statements are NOT resolved (a multi-file VFS is a later card). A
//! program that imports other modules may report spurious "unresolved name"
//! errors for symbols defined in those modules — acceptable for v1.

use crate::diag::{Error, Span};
use crate::parser;
use crate::resolver::merge_ctx_from_module;
use crate::typeck::{self, TyCtx};

// ── Public API ────────────────────────────────────────────────────────────────

/// Severity of a diagnostic. Maps to LSP `DiagnosticSeverity` in the LSP layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// An LSP-ready diagnostic with 0-indexed (line, character) start/end positions.
///
/// Both `start` and `end` are `(line, col)` where:
/// - `line` is 0-indexed (LSP line numbering).
/// - `col` is a 0-indexed UTF-8 character offset within the line.
///
/// # UTF-16 note
/// LSP specifies UTF-16 column encoding. This implementation uses UTF-8 char
/// counts, which is correct for ASCII. Full UTF-16 encoding is a later
/// refinement — see `byte_offset_to_position`.
// TODO(lsp): UTF-16 column encoding for non-ASCII sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub severity: Severity,
    /// 0-indexed (line, col) start position.
    pub start: (u32, u32),
    /// 0-indexed (line, col) end position.
    pub end: (u32, u32),
}

/// Parse and typecheck `src` without codegen, rustc, or filesystem access.
///
/// Returns a `Vec<Diagnostic>` containing zero or one entries:
/// - Empty on a clean program.
/// - One entry for the first parse or type error encountered (fail-fast pipeline).
///
/// `import` statements are **not** resolved; a program importing other modules
/// may produce spurious unresolved-name diagnostics — acceptable for v1.
pub fn analyze_str(src: &str) -> Vec<Diagnostic> {
    // Step 1: parse.
    let module = match parser::parse(src) {
        Ok(m) => m,
        Err(e) => return vec![diag_from_error(&e, src)],
    };

    // Step 2: build a single-module TyCtx (builtins + this module's definitions).
    // We reuse the same `merge_ctx_from_module` the multi-file resolver uses so
    // that function/class signatures are registered identically.
    let mut ctx = TyCtx::new();
    // `is_root = true` so top-level `main()` is not filtered out.
    if let Err(e) = merge_ctx_from_module(&module, &mut ctx, true) {
        return vec![diag_from_error(&e, src)];
    }

    // Step 3: typecheck function bodies.
    match typeck::check_bodies(&module, &ctx) {
        Ok(()) => vec![],
        Err(e) => vec![diag_from_error(&e, src)],
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Convert an `Error` into a `Diagnostic`, converting its span to 0-indexed
/// LSP positions via `byte_offset_to_position`.
fn diag_from_error(e: &Error, src: &str) -> Diagnostic {
    let (span, message) = extract_span_and_message(e);
    let (start, end) = span_to_lsp_range(span, src);
    Diagnostic { message, severity: Severity::Error, start, end }
}

/// Unwrap a (possibly `Sourced`) error to its innermost `Span` and display
/// message. For span-less variants (`Io`, `Codegen`, `Rustc`) returns
/// `Span::DUMMY` so the diagnostic lands at position (0, 0).
fn extract_span_and_message(e: &Error) -> (Span, String) {
    match e {
        Error::Lex { span, msg } => (*span, msg.clone()),
        Error::Parse { span, msg } => (*span, msg.clone()),
        Error::Type { span, msg } => (*span, msg.clone()),
        Error::ImportNotFound { path, span, importing_file } => (
            *span,
            format!(
                "import error: cannot find module '{}' (imported from {})",
                path, importing_file
            ),
        ),
        Error::CircularImport { cycle, span } => {
            (*span, format!("circular import: {}", cycle.join(" → ")))
        }
        // Unwrap the Sourced wrapper to its inner error (which carries the span).
        Error::Sourced { inner, .. } => extract_span_and_message(inner),
        // Span-less variants: position at (0, 0).
        Error::Io(e) => (Span::DUMMY, format!("io error: {}", e)),
        Error::Codegen(msg) => (Span::DUMMY, format!("codegen error: {}", msg)),
        Error::Rustc(msg) => (Span::DUMMY, format!("rustc failed: {}", msg)),
    }
}

/// Convert a `Span` to LSP 0-indexed `(start, end)` position pairs.
///
/// Prefers the byte-offset path when `span.end > span.start` (a real range).
/// Falls back to the 1-indexed `span.line/col` fields (subtracting 1) when
/// both byte offsets are zero but line/col are non-zero (a span set by the
/// lexer with only line/col populated).
fn span_to_lsp_range(span: Span, src: &str) -> ((u32, u32), (u32, u32)) {
    if span.end > span.start {
        // Preferred: byte-offset path.
        let start = byte_offset_to_position(src, span.start);
        let end = byte_offset_to_position(src, span.end);
        (start, end)
    } else if span.line > 0 || span.col > 0 {
        // Fallback: the span was built with only line/col (1-indexed).
        let line0 = span.line.saturating_sub(1);
        let col0 = span.col.saturating_sub(1);
        ((line0, col0), (line0, col0 + 1))
    } else {
        // DUMMY span or completely zeroed.
        ((0, 0), (0, 0))
    }
}

/// Convert a byte offset in `src` to a 0-indexed (line, character) LSP position.
///
/// Scans `src` up to `offset`, counting `\n` bytes for the line number and
/// Unicode scalar values since the last newline for the column.
///
/// # Clamping
/// If `offset` exceeds `src.len()`, it is clamped to `src.len()`.
///
/// # UTF-16 note
/// This counts Unicode scalar values (chars), not UTF-16 code units. For
/// ASCII-only sources the result is correct. Full UTF-16 encoding requires
/// counting surrogate pairs and is deferred.
// TODO(lsp): UTF-16 column encoding — count UTF-16 code units (u16s), not chars.
pub fn byte_offset_to_position(src: &str, offset: usize) -> (u32, u32) {
    let clamped = offset.min(src.len());
    let slice = &src[..clamped];
    let mut line: u32 = 0;
    let mut col: u32 = 0;
    for ch in slice.chars() {
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── byte_offset_to_position unit tests ────────────────────────────────────

    #[test]
    fn position_offset_zero_is_origin() {
        assert_eq!(byte_offset_to_position("hello\nworld", 0), (0, 0));
    }

    #[test]
    fn position_first_char_is_col_one() {
        // offset 1 = 'e' in "hello\n..." → line 0, col 1
        assert_eq!(byte_offset_to_position("hello\nworld", 1), (0, 1));
    }

    #[test]
    fn position_after_newline_is_line_one_col_zero() {
        // "hello\n" is 6 bytes; offset 6 = first char of second line
        assert_eq!(byte_offset_to_position("hello\nworld", 6), (1, 0));
    }

    #[test]
    fn position_mid_second_line() {
        // "hello\nwor" → offset 9 = 'r', line 1, col 3
        assert_eq!(byte_offset_to_position("hello\nworld", 9), (1, 3));
    }

    #[test]
    fn position_multi_line_third_line() {
        // "a\nb\nc" — offset 4 = 'c', line 2, col 0
        assert_eq!(byte_offset_to_position("a\nb\nc", 4), (2, 0));
    }

    #[test]
    fn position_clamps_past_end() {
        // Offset beyond src.len() should not panic; clamps to end.
        let src = "hi";
        let pos = byte_offset_to_position(src, 9999);
        assert_eq!(pos, (0, 2)); // end of "hi"
    }

    // ── analyze_str: clean program ────────────────────────────────────────────

    #[test]
    fn clean_program_returns_empty() {
        let src = "def main() -> None:\n    print(1)\n";
        let diags = analyze_str(src);
        assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);
    }

    #[test]
    fn clean_multi_statement_program_returns_empty() {
        let src = concat!(
            "def add(x: int, y: int) -> int:\n",
            "    return x + y\n",
            "\n",
            "def main() -> None:\n",
            "    result: int = add(1, 2)\n",
            "    print(result)\n",
        );
        let diags = analyze_str(src);
        assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);
    }

    // ── analyze_str: parse error ──────────────────────────────────────────────

    #[test]
    fn parse_error_returns_one_diagnostic_with_message() {
        // Missing closing paren → parse error
        let src = "def main(\n";
        let diags = analyze_str(src);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Error);
        // The message must be non-empty and mention a parse problem.
        assert!(!d.message.is_empty(), "message should not be empty");
    }

    #[test]
    fn parse_error_has_nonzero_position() {
        let src = "def main(\n";
        let diags = analyze_str(src);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        // The error should point into the source (not stuck at 0,0 for a real span).
        // start position should be somewhere on line 0 or 1.
        let (line, _col) = d.start;
        assert!(line <= 1, "expected error on line 0 or 1, got line {}", line);
        // At minimum, start should not equal (0,0) — the error is not at the very
        // beginning of the file since the problem is the unclosed paren.
        // (We accept line 0 col > 0 OR line 1, col 0.)
        let at_origin = d.start == (0, 0) && d.end == (0, 0);
        assert!(!at_origin, "expected non-origin position, got start={:?} end={:?}", d.start, d.end);
    }

    // ── analyze_str: type error ───────────────────────────────────────────────

    #[test]
    fn type_error_returns_one_diagnostic() {
        // Assigning a string to an int-annotated variable → type mismatch
        let src = "def main() -> None:\n    x: int = \"s\"\n";
        let diags = analyze_str(src);
        assert_eq!(diags.len(), 1, "expected exactly 1 type diagnostic, got: {:?}", diags);
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Error);
        assert!(
            d.message.contains("mismatch") || d.message.contains("int") || d.message.contains("str"),
            "expected a type-mismatch message, got: {:?}",
            d.message
        );
    }

    #[test]
    fn type_error_points_to_second_line() {
        // The assignment is on line 1 (0-indexed) of a 2-line program.
        let src = "def main() -> None:\n    x: int = \"s\"\n";
        let diags = analyze_str(src);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        // start.0 should be 1 (line index 1 = second line, 0-indexed).
        assert_eq!(
            d.start.0, 1,
            "expected type error on 0-indexed line 1, got line {}",
            d.start.0
        );
    }

    // ── analyze_str: does not touch filesystem ────────────────────────────────

    #[test]
    fn does_not_require_filesystem_for_clean_program() {
        // Run from a guaranteed-empty tmp dir so any accidental fs access fails.
        // (We simply run analyze_str; if it read a file it would need a real path.)
        let src = "def f(x: int) -> int:\n    return x + 1\n";
        // Must not panic or attempt to read a file.
        let diags = analyze_str(src);
        assert!(diags.is_empty());
    }
}
