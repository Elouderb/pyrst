# Language Server + Editor Support — Design / Scoping Document

**Date:** 2026-06-23
**Status:** Scoping — direction approved (language server + VSCode extension); per-phase implementation pending greenlight.
**Decision locked:** standardize the source file extension on **`.pyrs`** (was `.py`).

## 1. Goal

Make pyrst behave like a first-class language in VSCode (and any LSP editor):
syntax highlighting, **live error diagnostics**, **format-on-save**, hover types,
go-to-definition, and autocomplete. Today the compiler exposes `build`/`check`/
`emit`/`fmt`/`lint`/`repl` but there is **no language server, no editor extension,
and no `.pyrs` registration** — so the only in-editor experience is borrowed
(and partly wrong) Python highlighting plus terminal builds.

## 2. Current-state findings (source-confirmed, 2026-06-23)

### Reusable as-is
- `parser::parse(src: &str) -> Result<Module>` (`parser.rs:1409`) — string in, AST out. Ready for in-memory analysis.
- `infer_expr_ty(expr, locals: &HashMap<String,Ty>, ctx: &TyCtx) -> Ty` (`typeck.rs:1995`) — the EPIC-3 oracle; pure, never fails (→ `Ty::Unknown`). Gives a type for any `Expr` node once you have the node + its scope's locals.
- `TyCtx` (`typeck.rs:148`) — `funcs`, `classes`, `vars`. `classes[X]` carries `fields` + `methods` **with spans** (`ClassDef.span`, `Func.span`, `Param.span`) → reusable for completion and class/method go-to-definition. `get_all_fields` walks inheritance.
- `Span` (`diag.rs:4`) — `{ start, end: byte offset; line, col: u32, 1-indexed }`. LSP wants 0-indexed line + UTF-16 character → subtract 1 (UTF-16 is a refinement for non-ASCII).
- `Error` (`diag.rs:20`) — `Lex/Parse/Type/ImportNotFound/CircularImport` all carry a `Span`; `Sourced` wraps multi-file. Maps directly to an LSP `Diagnostic`.
- `formatter` / `driver::fmt` — wrap directly as the LSP formatting provider.

### Net-new work required
1. **Collect-all diagnostics.** The whole pipeline is **fail-fast** (`parse`, `check_bodies` `typeck.rs:897`, `driver::check` `driver.rs:6` all `Result<T, Error>`, `?` at first error). Reporting *every* error needs `Vec<Diagnostic>` threaded through parser + typeck + driver — the largest single refactor. The linter (`lint() -> Vec<Lint>` `linter.rs:452`) is the only collect-all path, but its `line`/`col` are stubbed to `0`.
2. **In-memory `analyze_str`.** No function takes a source string and returns diagnostics without the filesystem. `driver::check` is parse+typeck (no codegen) but takes a `Path`; `compile_str` writes a temp file *and* runs codegen. Net-new: `parse` → `TyCtx::new()` + `merge_ctx_from_module` → `check_bodies`, single-file (no resolver).
3. **Position→AST-node walk.** Entirely absent. Needed for hover + go-to-definition. Structurally feasible (every node carries `Span`) but 100% new.
4. **Type-at-position cache.** Types are computed transiently and discarded. Hover needs either a `(Span, Ty)` map collected during the typeck walk, or on-demand `infer_expr_ty` after reconstructing scope `locals` up to the cursor.
5. **FuncSig spans.** `TyCtx.funcs` strips spans when lowering `Func`→`FuncSig` (`resolver.rs:133`); top-level-function go-to-def must scan `Module.stmts` (or we retain a span on `FuncSig`).

### Multi-file / resolver
`resolver::resolve` is filesystem-only (`canonicalize` + `read_to_string`); imports resolve to `<dir>/<mod>.py` at `resolver.rs:102` (the one hardcoded extension). Single-file LSP analysis bypasses the resolver entirely; multi-file LSP later needs a VFS abstraction.

## 3. Library choices

- **Server:** [`tower-lsp-server`](https://github.com/tower-lsp-community/tower-lsp-server) — the actively-maintained community fork of `tower-lsp` (Tower/async, stdio transport, `LanguageServer` trait). Alternative considered: `async-lsp`. The original `ebkalderon/tower-lsp` is less maintained.
- **Extension:** VSCode `vscode-languageclient` (TypeScript thin client) + a TextMate grammar (`.tmLanguage.json`) for highlighting + a language contribution registering `.pyrs`.
- **Packaging:** add a `pyrst lsp` subcommand to the existing binary (reuses the lib crate — simplest), or split a `pyrst-lsp` workspace member if pulling tokio/tower into the compiler binary proves undesirable. Recommend the subcommand first.

## 4. Server architecture (maps the boilerplate onto pyrst)

```
initialize        → ServerCapabilities { text_document_sync: FULL + save,
                     document_formatting, [later: hover, definition, completion,
                     semantic_tokens] }
did_open/did_change/did_save → on_change(uri, text):
                     analyze_str(text) -> Vec<Diagnostic>   (Phase B: 0/1 error;
                     Phase C: all errors) → span→Range (line-1, col-1) →
                     client.publish_diagnostics(uri, diags)
                     ; cache the AST + TyCtx in a DashMap<Url, Analysis>
formatting        → driver::fmt(text) -> String → single full-document TextEdit
hover             → position→offset→node (Phase D walker) → infer_expr_ty → markdown
definition        → name at cursor → AST scan (funcs) / TyCtx.classes (classes) → span
completion        → node at cursor: `foo.` → TyCtx.classes[T].fields/methods ;
                     else → in-scope symbols + TyCtx.funcs/classes
```

State: `Backend { client, docs: DashMap<Url, Analysis> }` where `Analysis` holds the
text, parsed `Module`, and `TyCtx` (recomputed on each change).

## 5. Phased plan & cards

Each phase is independently shippable. **Phase A + B** already turn the editor
experience from "borrowed Python highlighting + terminal builds" into "real `.pyrs`
language with live red squiggles and format-on-save" — the bulk of the perceived
"first-class" jump. C–E layer on IDE richness.

| Card | Phase | Objective | Key files | Acceptance | Deps |
|------|-------|-----------|-----------|------------|------|
| **L0** | A | `.py`→`.pyrs` migration | rename corpus (261 + 3 + 2, `git mv`); `resolver.rs:102` `.py`→`.pyrs`; `test_all.sh` ×8 globs; `driver.rs:105/270` temp ext; `main.rs:22` usage; README/SPEC/design docs | `test_all.sh` green on `.pyrs`; multi-file imports resolve; build+REPL work; no `.py` left except intentional | none |
| **L1** | B | In-memory analysis entry | new `analyze_str(src) -> Vec<Diagnostic>` (parse + single-module TyCtx + check_bodies; fail-fast → ≤1 error as a 1-elem Vec); a `Span`→LSP-`Range` helper | unit tests: clean source → `[]`; a parse error and a type error → 1 diagnostic with correct 0-indexed range | L0 |
| **L2** | B | LSP server: diagnostics + formatting | `pyrst lsp` subcommand; `tower-lsp-server` + tokio deps; `Backend`/`initialize`/`on_change`/`did_*`/`formatting`; DashMap doc cache | `pyrst lsp` speaks LSP over stdio; an LSP test client sees diagnostics on change + format result | L1 |
| **L3** | B | VSCode extension (MVP) | `editors/vscode/`: `package.json` (register `.pyrs`, activation), TS client launching `pyrst lsp`, `.tmLanguage.json` grammar, `language-configuration.json` | open a `.pyrs` file → highlighting + live error squiggles + format-on-save in a real VSCode | L2 |
| **L4** | C | Collect-all diagnostics | thread `Vec<Diagnostic>` (or an error sink) through parser + typeck + driver; give linter real spans | one file with 3 independent errors → 3 squiggles at once; `test_all` negatives still pass | L2 (parallel-ish) |
| **L5** | D | Position→node index + type-at-position | net-new AST-walk-by-position; `(Span,Ty)` capture during typeck (or on-demand `infer_expr_ty`) | `node_at(pos)` unit-tested on nested exprs; type-at-position correct for locals/params/fields | L1 |
| **L6** | D | Hover + go-to-definition | `hover` (type markdown via L5); `definition` (funcs via AST scan, classes/methods via `TyCtx` spans; retain `FuncSig` span) | hover a var shows its type; F12 on a call/class jumps to its def | L5 |
| **L7** | E | Completion + semantic tokens | `completion` (dot-access fields/methods from `TyCtx.classes`; scope symbols otherwise); optional semantic-tokens highlighting | typing `obj.` lists fields/methods; identifiers complete from scope | L5, L6 |

## 6. Risks & decisions

- **One-error-at-a-time until L4.** Acceptable for an MVP (fix the first, the next
  appears); L4 (collect-all) is the bigger refactor and is deliberately deferred
  behind a usable v1. Flag this in the extension README so it isn't read as a bug.
- **UTF-16 columns.** LSP character offsets are UTF-16; byte/char `col-1` is correct
  for ASCII and wrong past a multibyte char. Ship ASCII-correct in B; encode UTF-16 in C/D.
- **Multi-file LSP needs a VFS.** The resolver is filesystem-only; single-file analysis
  (the common case) is unaffected. Defer cross-file LSP (workspace symbols, cross-module
  go-to-def) until after E.
- **Pipeline/LSP drift.** `analyze_str` must stay the same parse+typeck the compiler runs
  (don't fork a second analyzer) — reuse `check_bodies`, never reimplement it.
- **Binary weight.** `pyrst lsp` pulls tokio/tower into the main binary; split to a
  workspace member if that matters. Low risk.

## 7. Recommended sequence

**L0 → L1 → L2 → L3** delivers the MVP (real `.pyrs` language: highlight + live
diagnostics + format). Then **L4** (all-errors) and **L5 → L6** (hover/definition),
then **L7** (completion). L0 is greenlit (the `.pyrs` decision) and shallow — a clean
first commit. Each card keeps `cargo test` + `test_all.sh` green; the extension is
verified in a real VSCode (frontend-engineer / verification-engineer).
