//! `pyrst lsp` — a Language Server Protocol backend over stdin/stdout.
//!
//! This module wires the existing in-crate analysis and formatting pipeline to
//! editors via [`tower_lsp_server`]. It is intentionally thin: it owns no
//! language logic of its own. Diagnostics come from [`crate::analysis::analyze_str`]
//! and formatting reuses [`crate::formatter::format`] with the exact same guards
//! as the `pyrst fmt` CLI command (see [`crate::driver::fmt`]).
//!
//! # Capabilities advertised
//! Only what is actually implemented:
//! - `textDocumentSync = FULL` (with `save`) — the editor sends the whole
//!   document on every change, which we re-analyze.
//! - `documentFormatting = true` — whole-document formatting.
//!
//! Hover / completion / definition are deliberately NOT advertised: advertising
//! a capability without a handler makes editors show empty popups. Those are
//! later cards.
//!
//! # Threading / runtime
//! `main` is synchronous and returns an [`ExitCode`], so [`run`] builds its own
//! multi-threaded Tokio runtime and `block_on`s the server loop rather than
//! relying on `#[tokio::main]`.

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::Mutex;

use tower_lsp_server::jsonrpc::Result as RpcResult;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use crate::analysis::{self, Severity};

// ── Backend ─────────────────────────────────────────────────────────────────

/// The language server backend.
///
/// `docs` maps a document URI (as its string form) to the latest full text the
/// editor has sent us. We key on the string rather than `Uri` directly so the
/// store is independent of `Uri`'s trait impls and trivially `Hash + Eq`.
pub struct Backend {
    client: Client,
    docs: Mutex<HashMap<String, String>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Backend { client, docs: Mutex::new(HashMap::new()) }
    }

    /// Record (or replace) the stored text for `uri`.
    fn store_text(&self, uri: &Uri, text: String) {
        if let Ok(mut docs) = self.docs.lock() {
            docs.insert(uri.as_str().to_string(), text);
        }
    }

    /// Fetch the stored text for `uri`, if any.
    fn get_text(&self, uri: &Uri) -> Option<String> {
        self.docs.lock().ok().and_then(|docs| docs.get(uri.as_str()).cloned())
    }

    /// Drop the stored text for `uri`.
    fn drop_text(&self, uri: &Uri) {
        if let Ok(mut docs) = self.docs.lock() {
            docs.remove(uri.as_str());
        }
    }

    /// Re-analyze `text` and publish the resulting diagnostics for `uri`.
    ///
    /// A clean program publishes an EMPTY vector, which clears any stale
    /// squiggles the editor is still showing for this document.
    async fn on_change(&self, uri: Uri, text: &str) {
        let diags: Vec<Diagnostic> =
            analysis::analyze_str(text).iter().map(to_lsp_diagnostic).collect();
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

// ── Pure mappings (unit-testable without a running server) ────────────────────

/// Convert an [`analysis::Diagnostic`] into an LSP [`Diagnostic`].
///
/// Pure: it depends only on its argument, so it is unit-testable without a
/// running server or a [`Client`].
pub fn to_lsp_diagnostic(d: &analysis::Diagnostic) -> Diagnostic {
    let severity = match d.severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
    };
    Diagnostic {
        range: Range::new(
            Position::new(d.start.0, d.start.1),
            Position::new(d.end.0, d.end.1),
        ),
        severity: Some(severity),
        message: d.message.clone(),
        source: Some("pyrst".to_string()),
        ..Default::default()
    }
}

/// Compute the LSP [`Position`] of the very end of `text`.
///
/// The end line is the number of `\n` characters (so a 2-line file ending in a
/// newline has its end on line 2, character 0). The end character is the length
/// (in chars) of the final line. A whole-document edit ranges from `(0, 0)` to
/// this position, which covers every byte regardless of a trailing newline.
///
/// Pure, so it is unit-testable directly.
pub fn document_end_position(text: &str) -> Position {
    let mut line: u32 = 0;
    let mut last_line_chars: u32 = 0;
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            last_line_chars = 0;
        } else {
            last_line_chars += 1;
        }
    }
    Position::new(line, last_line_chars)
}

/// Format `text` exactly the way `pyrst fmt` does, returning the formatted
/// source on success or `None` when formatting must be refused (comments
/// present) or fails (parse / format / re-parse error).
///
/// Mirrors [`crate::driver::fmt`] so editor formatting never diverges from the
/// CLI:
/// 1. normalize line endings,
/// 2. refuse if the source contains comments (the lexer drops them, so
///    formatting would silently delete them),
/// 3. parse → format,
/// 4. re-parse the formatted output as a round-trip safety check.
///
/// Returning `None` (rather than the original text) tells the `formatting`
/// handler to send no edit, leaving the buffer untouched.
fn format_source(text: &str) -> Option<String> {
    let source = crate::lexer::normalize_line_endings(text);

    // The lexer discards comments; reformatting would delete them. Refuse.
    if crate::lexer::has_comment(&source) {
        return None;
    }

    let module = crate::parser::parse(&source).ok()?;
    let formatted = crate::formatter::format(&module).ok()?;

    // Round-trip safety: the formatted output must itself parse.
    crate::parser::parse(&formatted).ok()?;

    Some(formatted)
}

// ── LanguageServer impl ───────────────────────────────────────────────────────

impl LanguageServer for Backend {
    async fn initialize(&self, _params: InitializeParams) -> RpcResult<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "pyrst".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                // FULL sync: the editor resends the whole document on every
                // change. `save` is enabled (without includeText) so we also
                // re-analyze on save.
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        ..Default::default()
                    },
                )),
                // We implement whole-document formatting.
                document_formatting_provider: Some(OneOf::Left(true)),
                // Hover: type-on-cursor (EPIC-LSP L6).
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                // Go-to-definition: jump to declaration site (EPIC-LSP L6).
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client.log_message(MessageType::INFO, "pyrst lsp ready").await;
    }

    async fn shutdown(&self) -> RpcResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.store_text(&uri, text.clone());
        self.on_change(uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        // FULL sync guarantees exactly one change containing the entire
        // document. Take the last one defensively in case a client batches.
        if let Some(change) = params.content_changes.into_iter().last() {
            let text = change.text;
            self.store_text(&uri, text.clone());
            self.on_change(uri, &text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        // `text` is only present when the client opts into includeText; fall
        // back to the last text we stored otherwise.
        let text = params.text.or_else(|| self.get_text(&uri));
        if let Some(text) = text {
            self.store_text(&uri, text.clone());
            self.on_change(uri, &text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.drop_text(&params.text_document.uri);
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> RpcResult<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let original = match self.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        match format_source(&original) {
            Some(formatted) => {
                // One full-document edit replacing the whole buffer. The range
                // is measured over the ORIGINAL text so it covers everything
                // currently in the editor.
                let edit = TextEdit {
                    range: Range::new(Position::new(0, 0), document_end_position(&original)),
                    new_text: formatted,
                };
                Ok(Some(vec![edit]))
            }
            // Comments present, or parse/format/re-parse failure: emit no edit
            // rather than corrupt the buffer.
            None => Ok(None),
        }
    }

    // ── Hover (EPIC-LSP L6) ───────────────────────────────────────────────────

    async fn hover(&self, params: HoverParams) -> RpcResult<Option<Hover>> {
        let tdp = params.text_document_position_params;
        let uri = tdp.text_document.uri;
        let position = tdp.position;

        let text = match self.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        let (module, ctx) = match analysis::analyze_document(&text) {
            Some(pair) => pair,
            None => return Ok(None),
        };

        let ty = analysis::type_at_position(
            &module,
            &ctx,
            &text,
            position.line,
            position.character,
        );

        match ty {
            Some(t) => Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("```pyrst\n{}\n```", t),
                }),
                range: None,
            })),
            None => Ok(None),
        }
    }

    // ── Go-to-definition (EPIC-LSP L6) ────────────────────────────────────────

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> RpcResult<Option<GotoDefinitionResponse>> {
        let tdp = params.text_document_position_params;
        let uri = tdp.text_document.uri;
        let position = tdp.position;

        let text = match self.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        let (module, ctx) = match analysis::analyze_document(&text) {
            Some(pair) => pair,
            None => return Ok(None),
        };

        let span = analysis::definition_at_position(
            &module,
            &ctx,
            &text,
            position.line,
            position.character,
        );

        match span {
            Some(sp) => {
                let (sl, sc) = analysis::byte_offset_to_position(&text, sp.start);
                let (el, ec) = analysis::byte_offset_to_position(&text, sp.end);
                let range = Range::new(Position::new(sl, sc), Position::new(el, ec));
                Ok(Some(GotoDefinitionResponse::Scalar(Location::new(uri, range))))
            }
            None => Ok(None),
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Start the language server, speaking LSP over stdin/stdout until the client
/// disconnects. Builds its own Tokio runtime because `main` is synchronous.
pub fn run() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: failed to start LSP runtime: {}", e);
            return ExitCode::FAILURE;
        }
    };

    runtime.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = LspService::build(Backend::new).finish();
        Server::new(stdin, stdout, socket).serve(service).await;
    });

    ExitCode::SUCCESS
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_error_severity() {
        let d = analysis::Diagnostic {
            message: "boom".to_string(),
            severity: Severity::Error,
            start: (0, 0),
            end: (0, 1),
        };
        let lsp = to_lsp_diagnostic(&d);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn maps_warning_severity() {
        let d = analysis::Diagnostic {
            message: "careful".to_string(),
            severity: Severity::Warning,
            start: (0, 0),
            end: (0, 1),
        };
        let lsp = to_lsp_diagnostic(&d);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn maps_range_from_start_and_end() {
        let d = analysis::Diagnostic {
            message: "m".to_string(),
            severity: Severity::Error,
            start: (3, 7),
            end: (4, 2),
        };
        let lsp = to_lsp_diagnostic(&d);
        assert_eq!(lsp.range.start, Position::new(3, 7));
        assert_eq!(lsp.range.end, Position::new(4, 2));
    }

    #[test]
    fn maps_message_and_source() {
        let d = analysis::Diagnostic {
            message: "type mismatch".to_string(),
            severity: Severity::Error,
            start: (1, 0),
            end: (1, 5),
        };
        let lsp = to_lsp_diagnostic(&d);
        assert_eq!(lsp.message, "type mismatch");
        assert_eq!(lsp.source.as_deref(), Some("pyrst"));
    }

    #[test]
    fn end_position_empty_string() {
        assert_eq!(document_end_position(""), Position::new(0, 0));
    }

    #[test]
    fn end_position_single_line_no_newline() {
        // "hello" — end is line 0, char 5.
        assert_eq!(document_end_position("hello"), Position::new(0, 5));
    }

    #[test]
    fn end_position_trailing_newline() {
        // "a\nb\n" — two newlines → end is line 2, char 0.
        assert_eq!(document_end_position("a\nb\n"), Position::new(2, 0));
    }

    #[test]
    fn end_position_last_line_without_newline() {
        // "a\nbc" — one newline, last line "bc" → end line 1, char 2.
        assert_eq!(document_end_position("a\nbc"), Position::new(1, 2));
    }

    #[test]
    fn format_source_refuses_comments() {
        let src = "def main() -> None:\n    x: int = 1  # keep me\n";
        assert!(format_source(src).is_none(), "must refuse to format when comments are present");
    }

    #[test]
    fn format_source_formats_clean_program() {
        // Already-valid program with collapsible spacing; formatter normalizes it.
        let src = "def main() -> None:\n    x: int = 1+2\n";
        let formatted = format_source(src).expect("clean program should format");
        // The formatter inserts spaces around the binary operator.
        assert!(formatted.contains("1 + 2"), "got: {:?}", formatted);
        // And the output must itself re-parse (already guaranteed internally).
        crate::parser::parse(&formatted).expect("formatted output must re-parse");
    }

    #[test]
    fn format_source_returns_none_on_parse_error() {
        let src = "def main(\n"; // unclosed paren
        assert!(format_source(src).is_none(), "unparseable source must not be formatted");
    }
}
