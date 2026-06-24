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
//! - `hover` / `definition` — type-on-cursor and go-to-declaration (EPIC-LSP L6).
//! - `completion` (trigger `.`) — member completion on `obj.` and scope-symbol
//!   completion otherwise (EPIC-LSP L7).
//!
//! A capability is advertised only once its handler exists: advertising a
//! capability without a handler makes editors show empty popups.
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

use crate::analysis::{self, SemTok, SemTokKind, Severity};

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
    ///
    /// Editors send the raw buffer, which on Windows is CRLF; normalize line
    /// endings to `\n` here (as the file-read sites do) so the lexer never trips
    /// on a bare `\r` and so positions stay consistent across every handler.
    /// Line/column positions are unaffected — a `\r` sits at end-of-line, which
    /// editors don't count in character offsets.
    fn store_text(&self, uri: &Uri, text: String) {
        let text = crate::lexer::normalize_line_endings(&text);
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
        // Editors send the raw (CRLF on Windows) buffer; normalize so the lexer
        // doesn't report a bare `\r` as an error on every line. See store_text.
        let text = crate::lexer::normalize_line_endings(text);
        let diags: Vec<Diagnostic> =
            analysis::analyze_str(&text).iter().map(to_lsp_diagnostic).collect();
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

/// Map an [`analysis::CompletionKind`] to the LSP [`CompletionItemKind`] that
/// drives the editor's completion icon.
///
/// Pure: depends only on its argument, so it is unit-testable without a server.
pub fn completion_kind_to_lsp(kind: analysis::CompletionKind) -> CompletionItemKind {
    match kind {
        analysis::CompletionKind::Field => CompletionItemKind::FIELD,
        analysis::CompletionKind::Method => CompletionItemKind::METHOD,
        analysis::CompletionKind::Function => CompletionItemKind::FUNCTION,
        analysis::CompletionKind::Class => CompletionItemKind::CLASS,
        analysis::CompletionKind::Variable => CompletionItemKind::VARIABLE,
        analysis::CompletionKind::Keyword => CompletionItemKind::KEYWORD,
    }
}

/// Convert an [`analysis::Completion`] into an LSP [`CompletionItem`].
///
/// Pure: depends only on its argument, so it is unit-testable without a running
/// server or a [`Client`].
pub fn to_completion_item(c: &analysis::Completion) -> CompletionItem {
    CompletionItem {
        label: c.label.clone(),
        kind: Some(completion_kind_to_lsp(c.kind)),
        detail: c.detail.clone(),
        ..Default::default()
    }
}

// ── Semantic tokens (EPIC-LSP L7) ─────────────────────────────────────────────

/// The semantic-token legend, in the EXACT index order the wire encoding uses:
/// `Function=0, Method=1, Variable=2, Parameter=3, Class=4, Property=5`. The
/// `initialize` capability advertises this list, and [`sem_tok_legend_index`]
/// maps a [`SemTokKind`] to its position in it — the two MUST stay in lockstep,
/// so they are derived from the same source array.
const SEMANTIC_TOKEN_KINDS: [SemTokKind; 6] = [
    SemTokKind::Function,
    SemTokKind::Method,
    SemTokKind::Variable,
    SemTokKind::Parameter,
    SemTokKind::Class,
    SemTokKind::Property,
];

/// Build the [`SemanticTokensLegend`] advertised at `initialize`. `token_types`
/// is the standard [`SemanticTokenType`] const for each [`SemTokKind`] in
/// legend-index order; `token_modifiers` is empty (pyrst emits no modifiers).
pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    let token_types = SEMANTIC_TOKEN_KINDS.iter().map(sem_tok_type).collect();
    SemanticTokensLegend { token_types, token_modifiers: Vec::new() }
}

/// The standard [`SemanticTokenType`] for a [`SemTokKind`]. Pure.
fn sem_tok_type(kind: &SemTokKind) -> SemanticTokenType {
    match kind {
        SemTokKind::Function => SemanticTokenType::FUNCTION,
        SemTokKind::Method => SemanticTokenType::METHOD,
        SemTokKind::Variable => SemanticTokenType::VARIABLE,
        SemTokKind::Parameter => SemanticTokenType::PARAMETER,
        SemTokKind::Class => SemanticTokenType::CLASS,
        SemTokKind::Property => SemanticTokenType::PROPERTY,
    }
}

/// Map a [`SemTokKind`] to its index in the legend (the `tokenType` field of a
/// wire token): `Function=0, Method=1, Variable=2, Parameter=3, Class=4,
/// Property=5`. Pure, so it is unit-testable without a server.
pub fn sem_tok_legend_index(kind: SemTokKind) -> u32 {
    match kind {
        SemTokKind::Function => 0,
        SemTokKind::Method => 1,
        SemTokKind::Variable => 2,
        SemTokKind::Parameter => 3,
        SemTokKind::Class => 4,
        SemTokKind::Property => 5,
    }
}

/// Delta-encode absolute [`SemTok`]s into the LSP wire format: a flat list of
/// [`SemanticToken`]s where each token's position is RELATIVE to the previous
/// one. `prev` starts at line 0, char 0; for each token (which the builder has
/// already sorted by position):
/// - `delta_line   = line - prevLine`
/// - `delta_start  = (delta_line == 0) ? start - prevStart : start`
/// - `length       = the token's length`
/// - `token_type   = sem_tok_legend_index(kind)`
/// - `token_modifiers_bitset = 0` (pyrst emits no modifiers)
///
/// Pure: depends only on its argument, so it is unit-testable without a running
/// server. Inputs are assumed sorted and non-overlapping (the builder guarantees
/// both); `saturating_sub` keeps a stray out-of-order pair from underflowing.
pub fn encode_semantic_tokens(tokens: &[SemTok]) -> Vec<SemanticToken> {
    let mut data = Vec::with_capacity(tokens.len());
    let mut prev_line: u32 = 0;
    let mut prev_start: u32 = 0;
    for t in tokens {
        let delta_line = t.line.saturating_sub(prev_line);
        let delta_start = if delta_line == 0 {
            t.start_char.saturating_sub(prev_start)
        } else {
            t.start_char
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: t.length,
            token_type: sem_tok_legend_index(t.kind),
            token_modifiers_bitset: 0,
        });
        prev_line = t.line;
        prev_start = t.start_char;
    }
    data
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
                // Autocomplete: member completion on `obj.` + scope symbols
                // (EPIC-LSP L7). `.` is a trigger character so the editor fires a
                // completion request the instant the user types a dot.
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                // Semantic highlighting (EPIC-LSP L7): color user-defined
                // variables, parameters, functions, methods, classes, and
                // fields that the static TextMate grammar leaves uncolored.
                // `full` only (no range/delta); the legend order is fixed by
                // `semantic_tokens_legend` and mirrored by the wire encoder.
                semantic_tokens_provider: Some(
                    SemanticTokensOptions {
                        legend: semantic_tokens_legend(),
                        range: Some(false),
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                        ..Default::default()
                    }
                    .into(),
                ),
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

    // ── Completion (EPIC-LSP L7) ──────────────────────────────────────────────

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> RpcResult<Option<CompletionResponse>> {
        let tdp = params.text_document_position;
        let uri = tdp.text_document.uri;
        let position = tdp.position;

        let text = match self.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        // The text pre-scan does NOT require the buffer to parse — crucial,
        // because completion fires on an incomplete `p.` that fails to parse.
        let completions: Vec<analysis::Completion> =
            match analysis::completion_context(&text, position.line, position.character) {
                // Member access (`obj.partial`): repair the buffer, type the
                // receiver, and return its fields + methods (prefix-filtered).
                analysis::CompletionContext::Member { .. } => {
                    analysis::member_completions_at(&text, position.line, position.character)
                }
                // Otherwise: in-scope names. Requires the buffer to parse; when it
                // does not (mid-edit), stay silent rather than guess.
                analysis::CompletionContext::Scope { partial } => {
                    match analysis::analyze_document(&text) {
                        Some((module, ctx)) => {
                            let mut items = analysis::scope_completions(
                                &module,
                                &ctx,
                                &text,
                                position.line,
                                position.character,
                            );
                            if !partial.is_empty() {
                                items.retain(|c| c.label.starts_with(&partial));
                            }
                            items
                        }
                        None => Vec::new(),
                    }
                }
            };

        let items: Vec<CompletionItem> = completions.iter().map(to_completion_item).collect();
        Ok(Some(CompletionResponse::Array(items)))
    }

    // ── Semantic tokens (EPIC-LSP L7) ─────────────────────────────────────────

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> RpcResult<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;

        let text = match self.get_text(&uri) {
            Some(t) => t,
            None => return Ok(None),
        };

        // On a parse failure (mid-edit), emit NO tokens — the TextMate grammar
        // still colors keywords/types, so the editor degrades gracefully rather
        // than flickering stale highlights.
        let (module, ctx) = match analysis::analyze_document(&text) {
            Some(pair) => pair,
            None => return Ok(None),
        };

        // Pure builder → absolute, sorted, non-overlapping tokens; then
        // delta-encode to the protocol's relative wire format.
        let tokens = analysis::semantic_tokens(&module, &ctx, &text);
        let data = encode_semantic_tokens(&tokens);

        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
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

    // ── Completion mappings (EPIC-LSP L7) ─────────────────────────────────────

    #[test]
    fn completion_kind_maps_each_variant() {
        use analysis::CompletionKind as K;
        assert_eq!(completion_kind_to_lsp(K::Field), CompletionItemKind::FIELD);
        assert_eq!(completion_kind_to_lsp(K::Method), CompletionItemKind::METHOD);
        assert_eq!(completion_kind_to_lsp(K::Function), CompletionItemKind::FUNCTION);
        assert_eq!(completion_kind_to_lsp(K::Class), CompletionItemKind::CLASS);
        assert_eq!(completion_kind_to_lsp(K::Variable), CompletionItemKind::VARIABLE);
        assert_eq!(completion_kind_to_lsp(K::Keyword), CompletionItemKind::KEYWORD);
    }

    #[test]
    fn completion_item_carries_label_kind_detail() {
        let c = analysis::Completion {
            label: "name".to_string(),
            kind: analysis::CompletionKind::Field,
            detail: Some(": str".to_string()),
        };
        let item = to_completion_item(&c);
        assert_eq!(item.label, "name");
        assert_eq!(item.kind, Some(CompletionItemKind::FIELD));
        assert_eq!(item.detail.as_deref(), Some(": str"));
    }

    #[test]
    fn completion_item_detail_none_is_preserved() {
        let c = analysis::Completion {
            label: "helper".to_string(),
            kind: analysis::CompletionKind::Function,
            detail: None,
        };
        let item = to_completion_item(&c);
        assert_eq!(item.kind, Some(CompletionItemKind::FUNCTION));
        assert!(item.detail.is_none());
    }

    // ── Semantic tokens (EPIC-LSP L7) ─────────────────────────────────────────

    #[test]
    fn sem_tok_legend_index_maps_each_variant() {
        // The index order is the protocol contract: it MUST match the legend's
        // token_types order (Function=0, Method=1, Variable=2, Parameter=3,
        // Class=4, Property=5).
        assert_eq!(sem_tok_legend_index(SemTokKind::Function), 0);
        assert_eq!(sem_tok_legend_index(SemTokKind::Method), 1);
        assert_eq!(sem_tok_legend_index(SemTokKind::Variable), 2);
        assert_eq!(sem_tok_legend_index(SemTokKind::Parameter), 3);
        assert_eq!(sem_tok_legend_index(SemTokKind::Class), 4);
        assert_eq!(sem_tok_legend_index(SemTokKind::Property), 5);
    }

    #[test]
    fn semantic_tokens_legend_is_exact_order() {
        // The advertised legend must be exactly the six standard token types in
        // index order, with no modifiers.
        let legend = semantic_tokens_legend();
        assert_eq!(
            legend.token_types,
            vec![
                SemanticTokenType::FUNCTION,
                SemanticTokenType::METHOD,
                SemanticTokenType::VARIABLE,
                SemanticTokenType::PARAMETER,
                SemanticTokenType::CLASS,
                SemanticTokenType::PROPERTY,
            ]
        );
        assert!(legend.token_modifiers.is_empty(), "pyrst emits no token modifiers");
    }

    #[test]
    fn encode_semantic_tokens_empty_is_empty() {
        assert!(encode_semantic_tokens(&[]).is_empty());
    }

    #[test]
    fn encode_semantic_tokens_first_token_is_absolute() {
        // The first token's delta is relative to (line 0, char 0), i.e. absolute.
        let toks = [SemTok { line: 2, start_char: 5, length: 3, kind: SemTokKind::Function }];
        let data = encode_semantic_tokens(&toks);
        assert_eq!(data.len(), 1);
        let t = data[0];
        assert_eq!(t.delta_line, 2);
        assert_eq!(t.delta_start, 5);
        assert_eq!(t.length, 3);
        assert_eq!(t.token_type, 0); // Function
        assert_eq!(t.token_modifiers_bitset, 0);
    }

    #[test]
    fn encode_semantic_tokens_same_line_uses_relative_start() {
        // Two tokens on the SAME line: the second's delta_start is relative to
        // the first's start (delta_line == 0).
        let toks = [
            SemTok { line: 1, start_char: 4, length: 5, kind: SemTokKind::Variable }, // type 2
            SemTok { line: 1, start_char: 12, length: 6, kind: SemTokKind::Function }, // type 0
        ];
        let data = encode_semantic_tokens(&toks);
        assert_eq!(data.len(), 2);

        assert_eq!(data[0].delta_line, 1);
        assert_eq!(data[0].delta_start, 4); // absolute (first token)
        assert_eq!(data[0].length, 5);
        assert_eq!(data[0].token_type, 2); // Variable

        assert_eq!(data[1].delta_line, 0, "same line → delta_line 0");
        assert_eq!(data[1].delta_start, 8, "12 - 4 = 8 relative to prev start");
        assert_eq!(data[1].length, 6);
        assert_eq!(data[1].token_type, 0); // Function
    }

    #[test]
    fn encode_semantic_tokens_new_line_resets_start() {
        // On a NEW line, delta_start is the ABSOLUTE start (not relative to the
        // previous line's start).
        let toks = [
            SemTok { line: 0, start_char: 4, length: 3, kind: SemTokKind::Parameter }, // type 3
            SemTok { line: 3, start_char: 8, length: 2, kind: SemTokKind::Property }, // type 5
        ];
        let data = encode_semantic_tokens(&toks);
        assert_eq!(data[1].delta_line, 3, "3 - 0 = 3");
        assert_eq!(data[1].delta_start, 8, "new line → absolute start, not relative");
        assert_eq!(data[1].token_type, 5); // Property
    }

    #[test]
    fn semantic_tokens_full_pipeline_decodes_to_expected_roles() {
        // End-to-end over the SAME pipeline the `semantic_tokens_full` handler
        // runs (analyze_document → semantic_tokens → encode_semantic_tokens),
        // then DECODE the relative wire stream back to absolute positions and
        // check the role at each — exactly what the LEAD does against a live
        // server. This covers the handler's logic without the LSP transport.
        let src = concat!(
            "class Point:\n",                  // line 0
            "    x: int\n",                    // line 1
            "    def dist(self) -> int:\n",    // line 2
            "        return self.x\n",         // line 3
            "def add(a: int, b: int) -> int:\n", // line 4
            "    total: int = a + b\n",        // line 5
            "    return total\n",              // line 6
        );
        let (module, ctx) = analysis::analyze_document(src).expect("fixture parses");
        let toks = analysis::semantic_tokens(&module, &ctx, src);
        let data = encode_semantic_tokens(&toks);

        // Decode the delta stream back to (line, char, len, legend_index).
        let legend = ["function", "method", "variable", "parameter", "class", "property"];
        let mut decoded: Vec<(u32, u32, u32, &str)> = Vec::new();
        let (mut line, mut col) = (0u32, 0u32);
        for t in &data {
            if t.delta_line == 0 {
                col += t.delta_start;
            } else {
                line += t.delta_line;
                col = t.delta_start;
            }
            decoded.push((line, col, t.length, legend[t.token_type as usize]));
        }

        // Helper: find the decoded role at a position.
        let role = |l: u32, c: u32| -> Option<&str> {
            decoded.iter().find(|(dl, dc, _, _)| *dl == l && *dc == c).map(|(_, _, _, r)| *r)
        };

        // `class Point` → Point at (0,6) is a class.
        assert_eq!(role(0, 6), Some("class"), "Point is a class");
        // field `x` at (1,4) is a property.
        assert_eq!(role(1, 4), Some("property"), "x field is a property");
        // method `dist` at (2,8) is a method.
        assert_eq!(role(2, 8), Some("method"), "dist is a method");
        // `self` param at (2,13) is a parameter.
        assert_eq!(role(2, 13), Some("parameter"), "self is a parameter");
        // `self.x` access: receiver self (3,15) parameter, `.x` (3,20) property.
        assert_eq!(role(3, 15), Some("parameter"), "self receiver is a parameter");
        assert_eq!(role(3, 20), Some("property"), "self.x is a property");
        // `add` at (4,4) is a function; params `a` (4,8), `b` (4,15).
        assert_eq!(role(4, 4), Some("function"), "add is a function");
        assert_eq!(role(4, 8), Some("parameter"), "a is a parameter");
        assert_eq!(role(4, 16), Some("parameter"), "b is a parameter");
        // `total: int = a + b`: total (5,4) variable, a (5,17) param, b (5,21) param.
        assert_eq!(role(5, 4), Some("variable"), "total is a variable");
        assert_eq!(role(5, 17), Some("parameter"), "a use is a parameter");
        assert_eq!(role(5, 21), Some("parameter"), "b use is a parameter");
        // `return total`: total (6,11) variable.
        assert_eq!(role(6, 11), Some("variable"), "total use is a variable");

        // The whole stream is a multiple of 5 ints and monotonic by construction.
        assert_eq!(data.len(), decoded.len());
    }

    #[test]
    fn encode_semantic_tokens_three_token_stream() {
        // A full small stream exercising both same-line and new-line transitions.
        let toks = [
            SemTok { line: 0, start_char: 4, length: 3, kind: SemTokKind::Function }, // 0
            SemTok { line: 0, start_char: 8, length: 1, kind: SemTokKind::Parameter }, // 3
            SemTok { line: 1, start_char: 11, length: 1, kind: SemTokKind::Parameter }, // 3
        ];
        let data = encode_semantic_tokens(&toks);
        // Flattened 5-int stream the wire would carry.
        let flat: Vec<u32> = data
            .iter()
            .flat_map(|t| {
                [t.delta_line, t.delta_start, t.length, t.token_type, t.token_modifiers_bitset]
            })
            .collect();
        assert_eq!(
            flat,
            vec![
                0, 4, 3, 0, 0, // def name `add` at (0,4)
                0, 4, 1, 3, 0, // param `a` at (0,8): delta_start 8-4=4
                1, 11, 1, 3, 0, // param use at (1,11): new line → absolute 11
            ]
        );
    }
}
