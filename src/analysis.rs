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
/// Returns ALL top-level-item type errors as `Diagnostic`s (EPIC-LSP L4): a file
/// with type errors in functions A, B, and method C.m yields three diagnostics,
/// ordered top-to-bottom by source position. Parse errors are still reported
/// one-at-a-time — the parser is fail-fast, so a syntax error short-circuits to
/// a single diagnostic before any typechecking runs.
///
/// Returns an empty `Vec` on a clean program.
///
/// Within a single function/method, checking is still fail-fast (the first type
/// error in that item), so at most one diagnostic is produced per top-level
/// function or class. Full per-expression recovery is out of scope.
///
/// `import` statements are **not** resolved; a program importing other modules
/// may produce spurious unresolved-name diagnostics — acceptable for v1.
pub fn analyze_str(src: &str) -> Vec<Diagnostic> {
    // Step 1: parse. The parser is fail-fast — a syntax error short-circuits to
    // exactly one diagnostic.
    let module = match parser::parse(src) {
        Ok(m) => m,
        Err(e) => return vec![diag_from_error(&e, src)],
    };

    // Step 2: build a single-module TyCtx (builtins + this module's definitions).
    // We reuse the same `merge_ctx_from_module` the multi-file resolver uses so
    // that function/class signatures are registered identically. This pass is
    // also fail-fast (it builds the signature table the per-body checks read).
    let mut ctx = TyCtx::new();
    // `is_root = true` so top-level `main()` is not filtered out.
    if let Err(e) = merge_ctx_from_module(&module, &mut ctx, true) {
        return vec![diag_from_error(&e, src)];
    }

    // Step 3: collect EVERY top-level-item type error (one squiggle per failing
    // function/method) and map each to an LSP diagnostic. `check_all` already
    // sorts its errors by source position, so the diagnostics come out
    // top-to-bottom.
    typeck::check_all(&module, &ctx)
        .iter()
        .map(|e| diag_from_error(e, src))
        .collect()
}

/// Parse `src` and build a single-module [`TyCtx`], returning both on success.
///
/// This is the shared entry point for hover, go-to-definition, and (later)
/// completion — all three need the same `(module, ctx)` pair that
/// [`analyze_str`] builds internally.  Re-parsing per request is intentional:
/// it is fast, stateless, and avoids any cache-invalidation complexity.
///
/// Returns `None` when the source cannot be parsed (e.g. a syntax error while
/// the user is mid-edit).  Callers should treat `None` as "stay silent" rather
/// than as an error.
pub fn analyze_document(src: &str) -> Option<(crate::ast::Module, TyCtx)> {
    let module = parser::parse(src).ok()?;
    let mut ctx = TyCtx::new();
    merge_ctx_from_module(&module, &mut ctx, true).ok()?;
    Some((module, ctx))
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

/// Inverse of [`byte_offset_to_position`]: convert a 0-indexed LSP
/// `(line, character)` position to a byte offset in `src`.
///
/// `line` is a 0-indexed line number and `character` is a 0-indexed count of
/// Unicode scalar values from the start of that line (the same encoding
/// `byte_offset_to_position` produces — see its UTF-16 note; correct for ASCII).
///
/// # Clamping
/// - A `line` past the last line clamps to the end of `src`.
/// - A `character` past the end of its line clamps to that line's end (just
///   before the trailing `\n`, or `src.len()` for the final line).
///
/// This never panics and always returns an offset that lands on a UTF-8 char
/// boundary, so the result is safe to slice `src` with.
pub fn position_to_byte_offset(src: &str, line0: u32, col0: u32) -> usize {
    let mut cur_line: u32 = 0;
    let mut cur_col: u32 = 0;
    for (off, ch) in src.char_indices() {
        if cur_line == line0 && cur_col == col0 {
            return off;
        }
        if ch == '\n' {
            // If we are still before the target line, advance to the next line.
            // If we are ON the target line and the requested column is past the
            // line's content, clamp to this newline's offset (line end).
            if cur_line == line0 {
                return off;
            }
            cur_line += 1;
            cur_col = 0;
        } else {
            cur_col += 1;
        }
    }
    // Target is at or past end of source.
    src.len()
}

// ── Position → AST node navigation (EPIC-LSP L5) ───────────────────────────────
//
// The pure, filesystem-free engine that powers hover, go-to-definition, and
// (later) autocomplete. Everything here is additive: it READS the AST + an
// already-built `TyCtx` and REUSES `typeck::infer_expr_ty` as the type oracle —
// it never reimplements inference and never mutates the parser/typeck/codegen.

use std::collections::HashMap;

use crate::ast::{ClassDef, Expr, Func, Module, Stmt};
use crate::typeck::Ty;

/// The result of locating the AST node under a cursor.
///
/// `expr` is the INNERMOST expression whose span contains the position. The
/// `ancestors` chain is ordered outermost → innermost-parent (so the last
/// element is the direct parent of `expr`, and `ancestors.last()` answers "is
/// `expr` the object of an `Attr`/`Call`?"). `func`/`class` record the enclosing
/// function/method and class so callers can reconstruct scope without re-walking.
///
/// Borrows from the `Module` it was computed over (no cloning of AST nodes).
pub struct NodePath<'a> {
    /// Innermost `Expr` whose span contains the cursor.
    pub expr: &'a Expr,
    /// Enclosing expressions, outermost first; `ancestors.last()` is the parent
    /// of `expr`. Empty when `expr` is a top-level expression of a statement.
    pub ancestors: Vec<&'a Expr>,
    /// The function/method body the cursor sits in, if any.
    pub func: Option<&'a Func>,
    /// The class the cursor sits in (when inside a method), if any.
    pub class: Option<&'a ClassDef>,
}

impl<'a> NodePath<'a> {
    /// The direct parent of `expr`, if any.
    pub fn parent(&self) -> Option<&'a Expr> {
        self.ancestors.last().copied()
    }
}

/// True when `off` lies within `expr`'s span (inclusive of both ends).
///
/// End-inclusive on purpose: an editor cursor sits *between* characters, so a
/// cursor at the very end of an identifier token (`off == span.end`) should
/// still resolve that identifier. Spans whose byte range is empty
/// (`start == end`, e.g. some synthesized nodes) never contain a position and
/// are skipped — the line/col fallback is intentionally NOT used here because a
/// span carries line/col only for its START, so it cannot bound an end.
fn span_contains(expr: &Expr, off: usize) -> bool {
    let s = expr.span();
    s.end > s.start && off >= s.start && off <= s.end
}

/// Width of an expression's span in bytes — used to pick the TIGHTEST containing
/// node when several nested expressions all contain the cursor.
fn span_width(expr: &Expr) -> usize {
    let s = expr.span();
    s.end.saturating_sub(s.start)
}

/// Locate the innermost `Expr` under an LSP `(line0, col0)` cursor, plus its
/// ancestor chain and enclosing function/class.
///
/// `src` is required (the spec's #1 signature omits it, but a span stores
/// `line`/`col` only for its START, so the cursor must be converted to a byte
/// offset to be compared against span END offsets — there is no robust line/col
/// containment test). Returns `None` when the cursor is not inside any
/// expression (whitespace, a keyword, a bare `pass`, between top-level items).
///
/// Boundary semantics: containment is end-inclusive (see [`span_contains`]); the
/// returned node is the one with the smallest span among all that contain the
/// cursor.
pub fn node_at_position<'a>(
    module: &'a Module,
    src: &str,
    line0: u32,
    col0: u32,
) -> Option<NodePath<'a>> {
    let off = position_to_byte_offset(src, line0, col0);
    node_at_offset(module, off)
}

/// Byte-offset core of [`node_at_position`].
fn node_at_offset(module: &Module, off: usize) -> Option<NodePath<'_>> {
    let mut best: Option<NodePath> = None;
    walk_stmts(&module.stmts, off, None, None, &mut best);
    best
}

/// Walk a statement list, descending into the function/class that contains the
/// cursor and into every expression, recording the tightest containing node.
fn walk_stmts<'a>(
    stmts: &'a [Stmt],
    off: usize,
    func: Option<&'a Func>,
    class: Option<&'a ClassDef>,
    best: &mut Option<NodePath<'a>>,
) {
    for s in stmts {
        walk_stmt(s, off, func, class, best);
    }
}

fn walk_stmt<'a>(
    s: &'a Stmt,
    off: usize,
    func: Option<&'a Func>,
    class: Option<&'a ClassDef>,
    best: &mut Option<NodePath<'a>>,
) {
    match s {
        Stmt::Expr(e) => walk_expr(e, off, &[], func, class, best),
        Stmt::Assign { value, .. } => walk_expr(value, off, &[], func, class, best),
        Stmt::AugAssign { value, .. } => walk_expr(value, off, &[], func, class, best),
        Stmt::Unpack { value, .. } => walk_expr(value, off, &[], func, class, best),
        Stmt::Return(Some(e), _) => walk_expr(e, off, &[], func, class, best),
        Stmt::Return(None, _) => {}
        Stmt::If { cond, then, elifs, else_, .. } => {
            walk_expr(cond, off, &[], func, class, best);
            walk_stmts(then, off, func, class, best);
            for (c, body) in elifs {
                walk_expr(c, off, &[], func, class, best);
                walk_stmts(body, off, func, class, best);
            }
            if let Some(body) = else_ {
                walk_stmts(body, off, func, class, best);
            }
        }
        Stmt::While { cond, body, .. } => {
            walk_expr(cond, off, &[], func, class, best);
            walk_stmts(body, off, func, class, best);
        }
        Stmt::For { iter, body, .. } => {
            walk_expr(iter, off, &[], func, class, best);
            walk_stmts(body, off, func, class, best);
        }
        Stmt::Assert { cond, msg, .. } => {
            walk_expr(cond, off, &[], func, class, best);
            if let Some(m) = msg {
                walk_expr(m, off, &[], func, class, best);
            }
        }
        Stmt::Raise { exc: Some(e), .. } => walk_expr(e, off, &[], func, class, best),
        Stmt::Raise { exc: None, .. } => {}
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            walk_stmts(body, off, func, class, best);
            for h in handlers {
                walk_stmts(&h.body, off, func, class, best);
            }
            if let Some(b) = else_ {
                walk_stmts(b, off, func, class, best);
            }
            if let Some(b) = finally_ {
                walk_stmts(b, off, func, class, best);
            }
        }
        Stmt::With { ctx_expr, body, .. } => {
            walk_expr(ctx_expr, off, &[], func, class, best);
            walk_stmts(body, off, func, class, best);
        }
        Stmt::Del { target, .. } => walk_expr(target, off, &[], func, class, best),
        Stmt::Match { subject, arms, .. } => {
            walk_expr(subject, off, &[], func, class, best);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    walk_expr(g, off, &[], func, class, best);
                }
                walk_stmts(&arm.body, off, func, class, best);
            }
        }
        Stmt::AttrAssign { obj, value, .. } => {
            walk_expr(obj, off, &[], func, class, best);
            walk_expr(value, off, &[], func, class, best);
        }
        Stmt::IndexAssign { obj, idx, value, .. } => {
            walk_expr(obj, off, &[], func, class, best);
            walk_expr(idx, off, &[], func, class, best);
            walk_expr(value, off, &[], func, class, best);
        }
        Stmt::Func(f) => {
            walk_func(f, off, class, best);
        }
        Stmt::Class(c) => {
            for m in &c.methods {
                walk_func(m, off, Some(c), best);
            }
        }
        // Param defaults can hold expressions, but they live in the signature,
        // not the body; hover over a default is out of scope for v1.
        Stmt::Pass(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Import { .. } => {}
    }
}

/// Walk into a function/method body, setting it as the enclosing `func`.
fn walk_func<'a>(
    f: &'a Func,
    off: usize,
    class: Option<&'a ClassDef>,
    best: &mut Option<NodePath<'a>>,
) {
    walk_stmts(&f.body, off, Some(f), class, best);
}

/// Recursively descend an expression, recording the tightest span that contains
/// `off`. `ancestors` is the chain from the outermost expression down to (but
/// not including) `e`.
fn walk_expr<'a>(
    e: &'a Expr,
    off: usize,
    ancestors: &[&'a Expr],
    func: Option<&'a Func>,
    class: Option<&'a ClassDef>,
    best: &mut Option<NodePath<'a>>,
) {
    if span_contains(e, off) {
        let tighter = match best {
            Some(b) => span_width(e) <= span_width(b.expr),
            None => true,
        };
        if tighter {
            *best = Some(NodePath {
                expr: e,
                ancestors: ancestors.to_vec(),
                func,
                class,
            });
        }
    }

    // Descend regardless of whether `e` itself contained the cursor: a child's
    // span can lie outside a parent that has a conservative (wider) span, and
    // some container spans may not perfectly enclose every child.
    let mut child_chain = ancestors.to_vec();
    child_chain.push(e);
    let visit = |child: &'a Expr, best: &mut Option<NodePath<'a>>| {
        walk_expr(child, off, &child_chain, func, class, best);
    };

    match e {
        Expr::Int(..)
        | Expr::Float(..)
        | Expr::Str(..)
        | Expr::Bool(..)
        | Expr::None_(_)
        | Expr::Ident(..) => {}
        Expr::FStr(parts, _) => {
            for p in parts {
                if let crate::ast::FStrPart::Interp(ex, _) = p {
                    visit(ex, best);
                }
            }
        }
        Expr::List(elems, _) | Expr::Tuple(elems, _) | Expr::Set(elems, _) => {
            for el in elems {
                visit(el, best);
            }
        }
        Expr::Dict(pairs, _) => {
            for (k, v) in pairs {
                visit(k, best);
                visit(v, best);
            }
        }
        Expr::ListComp { elt, iter, cond, .. } | Expr::SetComp { elt, iter, cond, .. } => {
            visit(elt, best);
            visit(iter, best);
            if let Some(c) = cond {
                visit(c, best);
            }
        }
        Expr::DictComp { key, val, iter, cond, .. } => {
            visit(key, best);
            visit(val, best);
            visit(iter, best);
            if let Some(c) = cond {
                visit(c, best);
            }
        }
        Expr::Call { callee, args, kwargs, .. } => {
            visit(callee, best);
            for a in args {
                visit(a, best);
            }
            for (_, v) in kwargs {
                visit(v, best);
            }
        }
        Expr::Attr { obj, .. } => visit(obj, best),
        Expr::Index { obj, idx, .. } => {
            visit(obj, best);
            visit(idx, best);
        }
        Expr::Slice { obj, start, stop, step, .. } => {
            visit(obj, best);
            if let Some(x) = start {
                visit(x, best);
            }
            if let Some(x) = stop {
                visit(x, best);
            }
            if let Some(x) = step {
                visit(x, best);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            visit(lhs, best);
            visit(rhs, best);
        }
        Expr::UnOp { expr, .. } => visit(expr, best),
        Expr::Lambda { body, .. } => visit(body, best),
        Expr::IfExp { test, body, orelse, .. } => {
            visit(test, best);
            visit(body, best);
            visit(orelse, best);
        }
    }
}

// ── type_at_position (hover) ───────────────────────────────────────────────────

/// Best-effort type of the expression under the cursor, for hover.
///
/// Finds the enclosing function/method, reconstructs the `locals` map visible at
/// the cursor (mirroring codegen's `prescan_types`), then runs the shared
/// `infer_expr_ty` oracle on the innermost expression. Returns `None` when the
/// cursor is on nothing or the oracle yields `Ty::Unknown` (so hover stays quiet
/// rather than printing "unknown").
///
/// # Approximation
/// Local reconstruction is best-effort and intentionally conservative:
/// - Parameters are typed exactly (from their annotations); `self` is typed as
///   its class.
/// - Annotated locals (`x: T = ...`) are typed from the annotation.
/// - Un-annotated locals (`x = e`) are typed by inferring `e` against the
///   locals accumulated SO FAR — so a name only gains a type after its first
///   assignment is processed. Forward references and flow-sensitive refinement
///   (e.g. a later `append` narrowing an empty list) are not modelled; that
///   matches the spec's "un-annotated-local inference is best-effort" note.
/// - Only statements lexically before (or containing) the cursor in the
///   enclosing body contribute, so a local is untyped when hovered before its
///   own definition.
pub fn type_at_position(
    module: &Module,
    ctx: &TyCtx,
    src: &str,
    line0: u32,
    col0: u32,
) -> Option<Ty> {
    let off = position_to_byte_offset(src, line0, col0);

    // Primary path: cursor is on an expression node.
    if let Some(path) = node_at_position(module, src, line0, col0) {
        let locals = build_locals_for_func(&path, off, ctx);

        // When the cursor is on the bare callee identifier of a call (`g` in
        // `g()`), a function name is not itself a value, so the oracle would
        // return Unknown. Hovering a called function is most useful as the
        // CALL's result type, so retarget to the parent `Call` in that case.
        let target: &Expr = match (path.expr, path.parent()) {
            (Expr::Ident(_, _), Some(parent @ Expr::Call { callee, .. }))
                if std::ptr::eq(callee.as_ref(), path.expr) =>
            {
                parent
            }
            _ => path.expr,
        };

        let ty = crate::typeck::infer_expr_ty(target, &locals, ctx);
        if ty != Ty::Unknown {
            return Some(ty);
        }
    }

    // Fallback path: cursor is on a BINDING identifier (a declaration site),
    // not on an expression. Params and assignment targets are stored as bare
    // `String` fields in the AST, so `node_at_position` finds nothing there.
    binding_at_position(module, ctx, off)
}

/// Reconstruct the locals map visible at byte offset `off` inside the
/// enclosing function described by `path`. Seeds `self` for methods and
/// seeds params from their annotations before running `reconstruct_locals` on
/// the body up to `off`.
///
/// Shared by `type_at_position` and callers in `binding_at_position`.
fn build_locals_for_func<'a>(
    path: &NodePath<'a>,
    off: usize,
    ctx: &TyCtx,
) -> HashMap<String, Ty> {
    let mut locals: HashMap<String, Ty> = HashMap::new();
    if let Some(f) = path.func {
        // Seed `self` for methods (mirrors codegen.rs:1209).
        if let Some(c) = path.class {
            if f.params.iter().any(|p| p.name == "self") {
                locals.insert("self".to_string(), Ty::Class(c.name.clone()));
            }
        }
        // Seed parameters from their annotations (mirrors codegen.rs:1212-1217).
        for p in &f.params {
            if p.name == "self" {
                continue;
            }
            if let Ok(ty) = Ty::from_type_expr(&p.ty, p.span) {
                locals.insert(p.name.clone(), ty);
            }
        }
        // Reconstruct body locals up to the cursor.
        reconstruct_locals(&f.body, off, ctx, &mut locals);
    }
    locals
}

/// Return the type of the BINDING identifier (declaration site) at byte offset
/// `off`, or `None` when the cursor is not on a binding.
///
/// Covers three declaration kinds:
///
/// - **Function / method parameter** — if `off` falls within `Param.span` for
///   any param of the enclosing function, return that param's declared type.
///   `self` returns the enclosing class type.
///
/// - **Assignment target** — if `off` falls within `Stmt::Assign.span` (which
///   is the target-identifier token span), return the annotation type when
///   present, or infer the RHS type otherwise.
///
/// - **`for`-loop target** — `Stmt::For` stores targets as `Vec<String>` with
///   no per-target sub-span, so for-targets cannot be hovered at the
///   declaration site. This case is intentionally skipped.
fn binding_at_position(
    module: &Module,
    ctx: &TyCtx,
    off: usize,
) -> Option<Ty> {
    // Walk all top-level statements looking for the enclosing function/class,
    // then check params and assignment targets.
    for s in &module.stmts {
        if let Some(ty) = check_stmt_for_binding(s, off, ctx, None, None) {
            return Some(ty);
        }
    }
    None
}

/// Recursively check a statement (and its nested bodies) for a binding at `off`.
/// Returns the type when a binding site is found, `None` otherwise.
fn check_stmt_for_binding(
    s: &Stmt,
    off: usize,
    ctx: &TyCtx,
    enclosing_func: Option<&Func>,
    enclosing_class: Option<&crate::ast::ClassDef>,
) -> Option<Ty> {
    match s {
        Stmt::Func(f) => {
            // Check whether the cursor is in this function's params or body.
            if f.span.start > off {
                return None;
            }
            // Check params first.
            for p in &f.params {
                if span_offset_contains(p.span, off) {
                    if p.name == "self" {
                        // `self` hover → the enclosing class type.
                        return enclosing_class
                            .map(|c| Ty::Class(c.name.clone()));
                    }
                    return Ty::from_type_expr(&p.ty, p.span).ok();
                }
            }
            // Recurse into the body with this function as the enclosing scope.
            for stmt in &f.body {
                if let Some(ty) = check_stmt_for_binding(stmt, off, ctx, Some(f), enclosing_class) {
                    return Some(ty);
                }
            }
        }
        Stmt::Class(c) => {
            if c.span.start > off {
                return None;
            }
            for m in &c.methods {
                // Treat the method as a Stmt::Func for recursion.
                let method_stmt = Stmt::Func(m.clone());
                if let Some(ty) = check_stmt_for_binding(&method_stmt, off, ctx, enclosing_func, Some(c)) {
                    return Some(ty);
                }
            }
        }
        Stmt::Assign { target: _, ty: Some(te), span, .. } => {
            if span_offset_contains(*span, off) {
                // Annotated assignment: return the annotation type directly.
                return Ty::from_type_expr(te, *span).ok();
            }
        }
        Stmt::Assign { target, ty: None, value, span, .. } => {
            if span_offset_contains(*span, off) {
                // Un-annotated assignment: infer type from RHS, using locals
                // visible before this assignment.
                if let Some(f) = enclosing_func {
                    let mut locals: HashMap<String, Ty> = HashMap::new();
                    // Seed self for methods.
                    if let Some(c) = enclosing_class {
                        if f.params.iter().any(|p| p.name == "self") {
                            locals.insert("self".to_string(), Ty::Class(c.name.clone()));
                        }
                    }
                    // Seed params.
                    for p in &f.params {
                        if p.name == "self" { continue; }
                        if let Ok(ty) = Ty::from_type_expr(&p.ty, p.span) {
                            locals.insert(p.name.clone(), ty);
                        }
                    }
                    // Reconstruct locals up to (but not including) this span,
                    // so we see what was known before this assignment.
                    reconstruct_locals(&f.body, span.start, ctx, &mut locals);
                    let vt = crate::typeck::infer_expr_ty(value, &locals, ctx);
                    // Merge with any prior type for `target`.
                    let merged = match locals.get(target.as_str()) {
                        Some(existing) if vt == Ty::Unknown => existing.clone(),
                        _ => vt,
                    };
                    if merged != Ty::Unknown {
                        return Some(merged);
                    }
                } else {
                    // Top-level un-annotated assignment: infer without locals.
                    let locals: HashMap<String, Ty> = HashMap::new();
                    let vt = crate::typeck::infer_expr_ty(value, &locals, ctx);
                    if vt != Ty::Unknown {
                        return Some(vt);
                    }
                }
            }
        }
        // Recurse into nested statement bodies.
        Stmt::If { then, elifs, else_, .. } => {
            for stmt in then {
                if let Some(ty) = check_stmt_for_binding(stmt, off, ctx, enclosing_func, enclosing_class) {
                    return Some(ty);
                }
            }
            for (_, body) in elifs {
                for stmt in body {
                    if let Some(ty) = check_stmt_for_binding(stmt, off, ctx, enclosing_func, enclosing_class) {
                        return Some(ty);
                    }
                }
            }
            if let Some(body) = else_ {
                for stmt in body {
                    if let Some(ty) = check_stmt_for_binding(stmt, off, ctx, enclosing_func, enclosing_class) {
                        return Some(ty);
                    }
                }
            }
        }
        Stmt::While { body, .. } | Stmt::With { body, .. } => {
            for stmt in body {
                if let Some(ty) = check_stmt_for_binding(stmt, off, ctx, enclosing_func, enclosing_class) {
                    return Some(ty);
                }
            }
        }
        Stmt::For { body, .. } => {
            // Note: for-loop TARGET identifiers (`for x in ...`) have no
            // per-target sub-span in the AST (only `Vec<String>`), so the
            // declaration site of loop variables cannot be resolved by span
            // matching. The loop body is still searched for nested bindings.
            for stmt in body {
                if let Some(ty) = check_stmt_for_binding(stmt, off, ctx, enclosing_func, enclosing_class) {
                    return Some(ty);
                }
            }
        }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            for stmt in body {
                if let Some(ty) = check_stmt_for_binding(stmt, off, ctx, enclosing_func, enclosing_class) {
                    return Some(ty);
                }
            }
            for h in handlers {
                for stmt in &h.body {
                    if let Some(ty) = check_stmt_for_binding(stmt, off, ctx, enclosing_func, enclosing_class) {
                        return Some(ty);
                    }
                }
            }
            if let Some(b) = else_ {
                for stmt in b {
                    if let Some(ty) = check_stmt_for_binding(stmt, off, ctx, enclosing_func, enclosing_class) {
                        return Some(ty);
                    }
                }
            }
            if let Some(b) = finally_ {
                for stmt in b {
                    if let Some(ty) = check_stmt_for_binding(stmt, off, ctx, enclosing_func, enclosing_class) {
                        return Some(ty);
                    }
                }
            }
        }
        _ => {}
    }
    None
}

/// True when byte offset `off` falls within a span (end-inclusive, non-empty).
/// Mirrors the containment semantics of `span_contains` for `Expr` spans.
#[inline]
fn span_offset_contains(s: crate::diag::Span, off: usize) -> bool {
    s.end > s.start && off >= s.start && off <= s.end
}

/// Mirror of codegen's `prescan_types`, but POSITION-AWARE: only statements that
/// end at or before `off`, plus the single statement that CONTAINS `off`
/// (descending into its body), contribute to `locals`. This reconstructs the
/// scope visible at the cursor rather than the whole function.
fn reconstruct_locals(stmts: &[Stmt], off: usize, ctx: &TyCtx, locals: &mut HashMap<String, Ty>) {
    for s in stmts {
        let sp = stmt_span(s);
        // Statement entirely after the cursor: stop (locals are only visible
        // from their definition onward).
        if sp.start > off {
            break;
        }
        let contains = sp.end >= off && sp.start <= off;
        apply_stmt_to_locals(s, off, contains, ctx, locals);
        if contains {
            // The cursor is inside this statement; deeper siblings are not in
            // scope yet, so stop after recursing into its body (done above).
            break;
        }
    }
}

/// Apply a single statement's binding effect to `locals`, mirroring the relevant
/// arms of codegen's `prescan_types`. When `into_body` is true the cursor lies
/// inside this statement, so we descend into the matching nested block.
fn apply_stmt_to_locals(
    s: &Stmt,
    off: usize,
    into_body: bool,
    ctx: &TyCtx,
    locals: &mut HashMap<String, Ty>,
) {
    match s {
        Stmt::Assign { target, ty: Some(te), span, .. } => {
            if let Ok(t) = Ty::from_type_expr(te, *span) {
                locals.insert(target.clone(), t);
            }
        }
        Stmt::Assign { target, ty: None, value, .. } => {
            let vt = crate::typeck::infer_expr_ty(value, locals, ctx);
            let merged = match locals.get(target) {
                Some(existing) if vt == Ty::Unknown => existing.clone(),
                _ => vt,
            };
            locals.insert(target.clone(), merged);
        }
        Stmt::Unpack { targets, value, .. } => {
            // Tuple unpack: distribute element types when the RHS is a tuple.
            if let Ty::Tuple(elems) = crate::typeck::infer_expr_ty(value, locals, ctx) {
                if elems.len() == targets.len() {
                    for (name, ty) in targets.iter().zip(elems) {
                        locals.insert(name.clone(), ty);
                    }
                }
            }
        }
        Stmt::For { targets, iter, body, .. } => {
            if targets.len() == 1 {
                let elem = match crate::typeck::infer_expr_ty(iter, locals, ctx) {
                    Ty::List(inner) | Ty::Set(inner) => *inner,
                    Ty::Dict(key, _) => *key,
                    Ty::Str => Ty::Str,
                    _ => Ty::Int,
                };
                locals.entry(targets[0].clone()).or_insert(elem);
            }
            if into_body {
                reconstruct_locals(body, off, ctx, locals);
            }
        }
        Stmt::If { then, elifs, else_, .. } => {
            if into_body {
                reconstruct_locals(then, off, ctx, locals);
                for (_, body) in elifs {
                    reconstruct_locals(body, off, ctx, locals);
                }
                if let Some(body) = else_ {
                    reconstruct_locals(body, off, ctx, locals);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::With { body, .. } => {
            // `with ... as name`: bind the as-name to the context type.
            if let Stmt::With { ctx_expr, as_name: Some(name), .. } = s {
                let t = crate::typeck::infer_expr_ty(ctx_expr, locals, ctx);
                locals.insert(name.clone(), t);
            }
            if into_body {
                reconstruct_locals(body, off, ctx, locals);
            }
        }
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            if into_body {
                reconstruct_locals(body, off, ctx, locals);
                for h in handlers {
                    if let Some(name) = &h.exc_name {
                        locals.insert(name.clone(), Ty::Str);
                    }
                    reconstruct_locals(&h.body, off, ctx, locals);
                }
                if let Some(b) = else_ {
                    reconstruct_locals(b, off, ctx, locals);
                }
                if let Some(b) = finally_ {
                    reconstruct_locals(b, off, ctx, locals);
                }
            }
        }
        _ => {}
    }
}

// ── definition_at_position (go-to-definition) ──────────────────────────────────

/// Resolve the declaration span of the symbol under the cursor.
///
/// Returns the `Span` of:
/// - a local variable / parameter → the `Param.span` or the annotated/bare
///   assignment target's span in the enclosing function (the latter is the
///   identifier-token span, exactly the declaration site);
/// - a top-level function call/reference `f` → the `Stmt::Func` span of `f`;
/// - a class name / constructor `C(...)` → the `Stmt::Class` (ClassDef) span;
/// - an attribute/method `obj.m` → the field's `Param.span` or the method's
///   `Func.span` on the resolved class of `obj`.
///
/// Returns `None` when the cursor is not on a resolvable name.
pub fn definition_at_position(
    module: &Module,
    ctx: &TyCtx,
    src: &str,
    line0: u32,
    col0: u32,
) -> Option<crate::diag::Span> {
    let path = node_at_position(module, src, line0, col0)?;
    let off = position_to_byte_offset(src, line0, col0);

    match path.expr {
        Expr::Ident(name, _) => {
            // Resolve a bare name: local/param first, then top-level func, then
            // class. (A class name used as a constructor appears as the callee
            // `Ident` of a `Call`, handled by this same arm.)
            resolve_name(module, ctx, &path, off, name)
        }
        Expr::Attr { obj, name, .. } => {
            // `obj.name` — resolve obj's type to a class, then the field/method.
            resolve_attr(module, ctx, &path, off, obj, name)
        }
        Expr::Call { callee, .. } => {
            // Cursor on the call itself (e.g. on the parens) — resolve via callee.
            match callee.as_ref() {
                Expr::Ident(name, _) => resolve_name(module, ctx, &path, off, name),
                Expr::Attr { obj, name, .. } => {
                    resolve_attr(module, ctx, &path, off, obj, name)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Resolve a bare identifier to its declaration span.
fn resolve_name(
    module: &Module,
    ctx: &TyCtx,
    path: &NodePath,
    off: usize,
    name: &str,
) -> Option<crate::diag::Span> {
    // 1. Enclosing function param.
    if let Some(f) = path.func {
        if let Some(p) = f.params.iter().find(|p| p.name == name) {
            return Some(p.span);
        }
        // 2. Local variable: the first assignment target in the body that
        //    introduces `name` and lexically precedes the cursor. The Assign
        //    span IS the target-identifier token span (the declaration site).
        if let Some(sp) = find_local_decl(&f.body, off, name) {
            return Some(sp);
        }
    }
    // 3. Top-level function definition.
    if let Some(sp) = find_top_level_func(module, name) {
        return Some(sp);
    }
    // 4. Class name / constructor.
    if let Some(c) = ctx.classes.get(name) {
        return Some(c.span);
    }
    None
}

/// Resolve `obj.name` (a field or method access) to its declaration span on the
/// class `obj` resolves to.
fn resolve_attr(
    module: &Module,
    ctx: &TyCtx,
    path: &NodePath,
    off: usize,
    obj: &Expr,
    name: &str,
) -> Option<crate::diag::Span> {
    // Reconstruct locals so the receiver's type is known (e.g. `self`, a typed
    // param, or an annotated local).
    let mut locals: HashMap<String, Ty> = HashMap::new();
    if let Some(f) = path.func {
        if let Some(c) = path.class {
            if f.params.iter().any(|p| p.name == "self") {
                locals.insert("self".to_string(), Ty::Class(c.name.clone()));
            }
        }
        for p in &f.params {
            if p.name == "self" {
                continue;
            }
            if let Ok(ty) = Ty::from_type_expr(&p.ty, p.span) {
                locals.insert(p.name.clone(), ty);
            }
        }
        reconstruct_locals(&f.body, off, ctx, &mut locals);
    }

    let recv = crate::typeck::infer_expr_ty(obj, &locals, ctx);
    let class_name = match recv {
        Ty::Class(c) => c,
        _ => return None,
    };

    // Prefer a field declaration (inheritance-aware), then a method definition.
    let fields = ctx.get_all_fields(&class_name);
    if let Some(field) = fields.iter().find(|f| f.name == name) {
        return Some(field.span);
    }
    if let Some(sp) = find_method_span(module, &class_name, name) {
        return Some(sp);
    }
    None
}

/// First assignment in `body` (lexically before/at `off`) that introduces
/// `name`, returning its target-identifier span. Recurses into nested blocks so
/// a local introduced inside an `if`/`for` is still found.
fn find_local_decl(body: &[Stmt], off: usize, name: &str) -> Option<crate::diag::Span> {
    let mut found: Option<crate::diag::Span> = None;
    collect_first_decl(body, off, name, &mut found);
    found
}

fn collect_first_decl(
    stmts: &[Stmt],
    off: usize,
    name: &str,
    found: &mut Option<crate::diag::Span>,
) {
    for s in stmts {
        if found.is_some() {
            return;
        }
        match s {
            Stmt::Assign { target, span, .. } if target == name => {
                if span.start <= off {
                    *found = Some(*span);
                    return;
                }
            }
            Stmt::If { then, elifs, else_, .. } => {
                collect_first_decl(then, off, name, found);
                for (_, b) in elifs {
                    collect_first_decl(b, off, name, found);
                }
                if let Some(b) = else_ {
                    collect_first_decl(b, off, name, found);
                }
            }
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::With { body, .. } => {
                collect_first_decl(body, off, name, found);
            }
            Stmt::Try { body, handlers, else_, finally_, .. } => {
                collect_first_decl(body, off, name, found);
                for h in handlers {
                    collect_first_decl(&h.body, off, name, found);
                }
                if let Some(b) = else_ {
                    collect_first_decl(b, off, name, found);
                }
                if let Some(b) = finally_ {
                    collect_first_decl(b, off, name, found);
                }
            }
            _ => {}
        }
    }
}

/// Span of a top-level `def name(...)`, scanning `module.stmts` (TyCtx strips
/// spans from `FuncSig`, so the AST is the source of truth here).
fn find_top_level_func(module: &Module, name: &str) -> Option<crate::diag::Span> {
    module.stmts.iter().find_map(|s| match s {
        Stmt::Func(f) if f.name == name => Some(f.span),
        _ => None,
    })
}

/// Span of a method `name` on `class_name` (inheritance-aware via the same base
/// chain `TyCtx` uses), scanning the AST classes for the owning `Func.span`.
fn find_method_span(
    module: &Module,
    class_name: &str,
    name: &str,
) -> Option<crate::diag::Span> {
    let mut visited = std::collections::HashSet::new();
    find_method_span_rec(module, class_name, name, &mut visited)
}

fn find_method_span_rec(
    module: &Module,
    class_name: &str,
    name: &str,
    visited: &mut std::collections::HashSet<String>,
) -> Option<crate::diag::Span> {
    if !visited.insert(class_name.to_string()) {
        return None;
    }
    // Find the ClassDef in the AST (carries method `Func.span`s).
    let class_def = module.stmts.iter().find_map(|s| match s {
        Stmt::Class(c) if c.name == class_name => Some(c),
        _ => None,
    })?;
    if let Some(m) = class_def.methods.iter().find(|m| m.name == name) {
        return Some(m.span);
    }
    // Walk bases (the AST's base list — resolution mirrors `TyCtx`'s base chain).
    for base in &class_def.bases {
        if let Some(sp) = find_method_span_rec(module, base, name, visited) {
            return Some(sp);
        }
    }
    None
}

/// The span of a statement (used for position-ordering in local reconstruction).
fn stmt_span(s: &Stmt) -> crate::diag::Span {
    match s {
        Stmt::Expr(e) => e.span(),
        Stmt::Assign { span, .. }
        | Stmt::AugAssign { span, .. }
        | Stmt::Unpack { span, .. }
        | Stmt::If { span, .. }
        | Stmt::While { span, .. }
        | Stmt::For { span, .. }
        | Stmt::Pass(span)
        | Stmt::Break(span)
        | Stmt::Continue(span)
        | Stmt::Assert { span, .. }
        | Stmt::Raise { span, .. }
        | Stmt::Try { span, .. }
        | Stmt::With { span, .. }
        | Stmt::Del { span, .. }
        | Stmt::Match { span, .. }
        | Stmt::Import { span, .. }
        | Stmt::AttrAssign { span, .. }
        | Stmt::IndexAssign { span, .. } => *span,
        Stmt::Return(_, span) => *span,
        Stmt::Func(f) => f.span,
        Stmt::Class(c) => c.span,
    }
}

// ── Autocomplete (EPIC-LSP L7) ─────────────────────────────────────────────────
//
// Member completion on `obj.` (the fields + methods of `obj`'s class) and
// scope-symbol completion otherwise. Like the rest of this module these helpers
// are pure: they READ the AST + an already-built `TyCtx` and REUSE
// `type_at_position` / `infer_expr_ty` as the type oracle — never re-implementing
// inference and never mutating the parser/typeck/codegen.
//
// The key wrinkle is that completion fires on a buffer that often does NOT parse
// (`p.` has a trailing incomplete member access). The text pre-scan
// [`completion_context`] therefore does NOT require the buffer to parse, and the
// member path [`member_completions_at`] REPAIRS the source (deleting the trailing
// `.<partial>`) so the receiver alone parses and can be typed.

/// What KIND of symbol a [`Completion`] refers to. The LSP layer maps each
/// variant to a `CompletionItemKind`; kept as an internal enum so `analysis`
/// stays free of any LSP type dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Field,
    Method,
    Function,
    Class,
    Variable,
    Keyword,
}

/// A single completion candidate. `label` is the inserted text (the bare name);
/// `detail` is an optional human-readable annotation (a field's `": int"` or a
/// method's `"(a: int) -> str"`); `kind` drives the editor's icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
}

/// The lexical situation immediately before the cursor, derived by a pure text
/// pre-scan that does NOT need the buffer to parse.
///
/// - `Member` — the text before the cursor is `<receiver>.<partial>` (a member
///   access). `receiver` is the source text of the receiver expression, and
///   `receiver_end` is its end as a 0-indexed `(line, col)` position (which is
///   exactly the position of the `.`). `partial` is the word already typed after
///   the dot (possibly empty for a bare `obj.`).
/// - `Scope` — anything else; `partial` is the word being typed before the
///   cursor (possibly empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionContext {
    Member { receiver: String, receiver_end: (u32, u32), partial: String },
    Scope { partial: String },
}

/// True for characters that may appear in an identifier (the pyrst lexer's
/// identifier alphabet: ASCII letters, digits, and `_`).
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Render a `TypeExpr` annotation as a `Ty`-formatted string (e.g. `int`,
/// `list[int]`, `MyClass`), reusing `Ty::from_type_expr` + `Ty::Display` so
/// completion detail matches hover exactly. Falls back to a best-effort name on
/// the (practically unreachable) lowering error so a bad annotation never hides
/// the whole completion.
fn render_type_expr(te: &crate::ast::TypeExpr) -> String {
    match Ty::from_type_expr(te, crate::diag::Span::DUMMY) {
        Ok(ty) => ty.to_string(),
        Err(_) => match te {
            crate::ast::TypeExpr::Named(n) => n.clone(),
            crate::ast::TypeExpr::None_ => "None".to_string(),
            _ => "?".to_string(),
        },
    }
}

/// Build a method's `detail` signature string, e.g. `(a: int, b: str) -> bool`.
/// `self` is omitted (it is implicit at the call site). The return type is always
/// shown (`-> None` for a void method), mirroring how pyrst sources are written.
fn method_detail(m: &Func) -> String {
    let params: Vec<String> = m
        .params
        .iter()
        .filter(|p| p.name != "self")
        .map(|p| format!("{}: {}", p.name, render_type_expr(&p.ty)))
        .collect();
    format!("({}) -> {}", params.join(", "), render_type_expr(&m.ret))
}

/// Inheritance-aware method collector mirroring `TyCtx::collect_fields`: walks the
/// base chain first, then this class, so a method defined on a derived class
/// SHADOWS the same-named method on a base. Returns `(name, detail)` pairs in a
/// stable order (bases first, then this class), deduped by name keeping the most
/// derived definition.
fn collect_methods(ctx: &TyCtx, class_name: &str) -> Vec<(String, String)> {
    let mut ordered: Vec<String> = Vec::new();
    let mut details: HashMap<String, String> = HashMap::new();
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_methods_rec(ctx, class_name, &mut ordered, &mut details, &mut visited);
    ordered
        .into_iter()
        .map(|name| {
            let d = details.remove(&name).unwrap_or_default();
            (name, d)
        })
        .collect()
}

fn collect_methods_rec(
    ctx: &TyCtx,
    class_name: &str,
    ordered: &mut Vec<String>,
    details: &mut HashMap<String, String>,
    visited: &mut std::collections::HashSet<String>,
) {
    if !visited.insert(class_name.to_string()) {
        return;
    }
    let Some(class_def) = ctx.classes.get(class_name) else { return };
    // Bases first (so derived definitions overwrite inherited details below).
    for base in &class_def.bases {
        collect_methods_rec(ctx, base, ordered, details, visited);
    }
    for m in &class_def.methods {
        if !details.contains_key(&m.name) {
            ordered.push(m.name.clone());
        }
        // Always (re)write the detail with the most-derived signature seen.
        details.insert(m.name.clone(), method_detail(m));
    }
}

/// Fields + methods of the class `ty` resolves to, for `obj.` member completion.
///
/// When `ty` is `Ty::Class(name)`: returns its FIELDS (inheritance-aware via
/// `TyCtx::get_all_fields`; `label = name`, `detail = ": <type>"`,
/// `kind = Field`) followed by its METHODS (inheritance-aware; `label = name`,
/// `detail = "(params) -> ret"`, `kind = Method`). For any non-class type
/// (`int`, `list[..]`, `Unknown`, …) returns an empty vec — pyrst models no
/// methods on builtins for completion.
///
/// Fields come before methods; both are deduped by label (a field shadows an
/// inherited field of the same name via `get_all_fields`; a derived method
/// shadows a base method via `collect_methods`).
pub fn member_completions(ty: &Ty, ctx: &TyCtx) -> Vec<Completion> {
    let class_name = match ty {
        Ty::Class(n) => n.clone(),
        _ => return Vec::new(),
    };
    if !ctx.classes.contains_key(&class_name) {
        return Vec::new();
    }

    let mut out: Vec<Completion> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Fields (inheritance-aware). `get_all_fields` lists bases first then this
    // class; keep the FIRST occurrence of each name so a derived field wins is
    // not required here (fields are not normally redeclared), but dedup defends
    // against it regardless.
    for field in ctx.get_all_fields(&class_name) {
        if !seen.insert(field.name.clone()) {
            continue;
        }
        out.push(Completion {
            label: field.name.clone(),
            kind: CompletionKind::Field,
            detail: Some(format!(": {}", render_type_expr(&field.ty))),
        });
    }

    // Methods (inheritance-aware, derived shadows base).
    for (name, detail) in collect_methods(ctx, &class_name) {
        if !seen.insert(name.clone()) {
            continue;
        }
        out.push(Completion {
            label: name,
            kind: CompletionKind::Method,
            detail: Some(detail),
        });
    }

    out
}

/// In-scope names visible at the cursor, for non-member (`Scope`) completion.
///
/// Collects, in priority order and deduped by label:
/// 1. the enclosing function's params + locals visible at the cursor (`kind =
///    Variable`, `detail = the type`), reconstructed exactly as
///    `type_at_position` does (params from annotations, `self` as its class,
///    body locals up to the cursor);
/// 2. top-level functions from `ctx.funcs` (`kind = Function`) — this includes
///    the builtins seeded into `TyCtx::new`;
/// 3. classes from `ctx.classes` (`kind = Class`);
/// 4. a small set of pyrst keywords (`kind = Keyword`).
///
/// Earlier entries win on a label collision (a local named `len` shadows the
/// builtin in the list).
pub fn scope_completions(
    module: &Module,
    ctx: &TyCtx,
    src: &str,
    line0: u32,
    col0: u32,
) -> Vec<Completion> {
    let mut out: Vec<Completion> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 1. Locals + params of the enclosing function, reconstructed at the cursor.
    let off = position_to_byte_offset(src, line0, col0);
    if let Some(path) = node_at_position(module, src, line0, col0) {
        let locals = build_locals_for_func(&path, off, ctx);
        // Sort for a stable, name-ordered presentation.
        let mut names: Vec<&String> = locals.keys().collect();
        names.sort();
        for name in names {
            if !seen.insert(name.clone()) {
                continue;
            }
            let ty = &locals[name];
            out.push(Completion {
                label: name.clone(),
                kind: CompletionKind::Variable,
                detail: Some(ty.to_string()),
            });
        }
    }

    // 2. Top-level functions (and builtins). Sorted for determinism.
    let mut func_names: Vec<&String> = ctx.funcs.keys().collect();
    func_names.sort();
    for name in func_names {
        if !seen.insert(name.clone()) {
            continue;
        }
        out.push(Completion {
            label: name.clone(),
            kind: CompletionKind::Function,
            detail: None,
        });
    }

    // 3. Classes. Sorted for determinism.
    let mut class_names: Vec<&String> = ctx.classes.keys().collect();
    class_names.sort();
    for name in class_names {
        if !seen.insert(name.clone()) {
            continue;
        }
        out.push(Completion {
            label: name.clone(),
            kind: CompletionKind::Class,
            detail: None,
        });
    }

    // 4. Keywords.
    for kw in ["def", "class", "if", "for", "return", "None", "True", "False"] {
        if !seen.insert(kw.to_string()) {
            continue;
        }
        out.push(Completion {
            label: kw.to_string(),
            kind: CompletionKind::Keyword,
            detail: None,
        });
    }

    out
}

/// Pure text pre-scan of the situation immediately before an LSP `(line0, col0)`
/// cursor, WITHOUT requiring the buffer to parse.
///
/// Returns `Member { receiver, receiver_end, partial }` when the text before the
/// cursor is `<receiver-expr>.<word>` (a member access — there is a `.` after an
/// identifier / `)` / `]`, and only word chars sit between that `.` and the
/// cursor). Otherwise returns `Scope { partial }`, where `partial` is the word
/// being typed before the cursor (possibly empty).
///
/// Receiver detection is deliberately simple but correct for the common cases —
/// `ident.`, `ident.partial`, `self.`, dotted `a.b.`, and a trailing call/index
/// like `f().` or `xs[0].` — by walking backward over a balanced run of word
/// chars, `.`, and matched `()` / `[]`.
pub fn completion_context(src: &str, line0: u32, col0: u32) -> CompletionContext {
    let cursor = position_to_byte_offset(src, line0, col0);
    let before = &src[..cursor];
    let bytes = before.as_bytes();

    // The trailing run of word chars before the cursor is the `partial`.
    let mut word_start = before.len();
    while word_start > 0 && is_word_char(bytes[word_start - 1] as char) {
        word_start -= 1;
    }
    let partial = before[word_start..].to_string();

    // Is the char immediately before the word a `.`? If so this is a member
    // access; the receiver is whatever precedes that `.`.
    if word_start > 0 && bytes[word_start - 1] == b'.' {
        let dot = word_start - 1;
        if let Some(recv_start) = receiver_start(bytes, dot) {
            if recv_start < dot {
                let receiver = before[recv_start..dot].to_string();
                let receiver_end = byte_offset_to_position(src, dot);
                return CompletionContext::Member { receiver, receiver_end, partial };
            }
        }
        // A `.` with no resolvable receiver before it (e.g. a leading `.` or a
        // float like `3.`): fall through to scope completion.
    }

    CompletionContext::Scope { partial }
}

/// Walk backward from `dot` (the byte index of the `.`) over a balanced receiver
/// expression, returning the byte index where it starts. Accepts word chars,
/// `.` (dotted access), and matched `)` / `]` groups; stops at anything else.
fn receiver_start(bytes: &[u8], dot: usize) -> Option<usize> {
    let mut i = dot;
    while i > 0 {
        let c = bytes[i - 1] as char;
        if is_word_char(c) || c == '.' {
            i -= 1;
        } else if c == ')' || c == ']' {
            // Skip a balanced bracket group.
            let (open, close) = if c == ')' { (b'(', b')') } else { (b'[', b']') };
            let mut depth = 0usize;
            let mut j = i;
            loop {
                if j == 0 {
                    return None; // unbalanced — give up
                }
                let cj = bytes[j - 1];
                if cj == close {
                    depth += 1;
                } else if cj == open {
                    depth -= 1;
                    if depth == 0 {
                        j -= 1;
                        break;
                    }
                }
                j -= 1;
            }
            i = j;
        } else {
            break;
        }
    }
    Some(i)
}

/// Member completion driven by the REPAIR strategy: given a cursor at
/// `(line0, col0)` whose preceding text is `<receiver>.<partial>`, build a
/// repaired source (delete the trailing `.<partial>` so the receiver stands
/// alone and the buffer parses), type the receiver via `type_at_position`, and
/// return its members filtered by the `partial` prefix.
///
/// Returns an empty vec (never panics) when the context is not a member access,
/// the repaired buffer still doesn't parse, or the receiver type is unknown /
/// non-class.
pub fn member_completions_at(src: &str, line0: u32, col0: u32) -> Vec<Completion> {
    let (receiver_end, partial) = match completion_context(src, line0, col0) {
        CompletionContext::Member { receiver_end, partial, .. } => (receiver_end, partial),
        CompletionContext::Scope { .. } => return Vec::new(),
    };

    let cursor = position_to_byte_offset(src, line0, col0);
    // `receiver_end` is the position of the `.`; its byte offset is the start of
    // the slice to delete, through the cursor (the end of `partial`).
    let dot_off = position_to_byte_offset(src, receiver_end.0, receiver_end.1);
    if dot_off >= cursor {
        return Vec::new();
    }

    // Build the repaired buffer: original with `[dot_off, cursor)` removed. This
    // leaves the receiver expression intact and syntactically valid in place
    // (`    p.di` → `    p`, `    foo(p.)` → `    foo(p)`).
    let mut repaired = String::with_capacity(src.len() - (cursor - dot_off));
    repaired.push_str(&src[..dot_off]);
    repaired.push_str(&src[cursor..]);

    let Some((module, ctx)) = analyze_document(&repaired) else {
        return Vec::new();
    };

    // Type the receiver at its end position (unchanged by the deletion, which
    // happened at/after the dot). `type_at_position` reconstructs locals and runs
    // the shared inference oracle — no forked inference.
    let Some(ty) = type_at_position(&module, &ctx, &repaired, receiver_end.0, receiver_end.1) else {
        return Vec::new();
    };

    let mut members = member_completions(&ty, &ctx);
    if !partial.is_empty() {
        members.retain(|c| c.label.starts_with(&partial));
    }
    members
}

// ── Semantic tokens (EPIC-LSP L7) ──────────────────────────────────────────────
//
// The pure token builder that powers editor *semantic highlighting*: the
// TextMate grammar only colors fixed keywords/types, so user-defined names
// (variables, parameters, functions, methods, classes, fields) are left
// uncolored. `semantic_tokens` walks the whole AST and emits ONE token per
// identifier OCCURRENCE, classified by its ROLE — a definition site, a binding,
// or a use resolved through the same scope/type machinery hover and completion
// already use (no forked inference).
//
// Like the rest of this module these helpers are pure: they READ the AST + an
// already-built `TyCtx` and REUSE `type_at_position` / the locals reconstruction
// as the oracle. They never mutate the parser/typeck/codegen. The LSP layer
// delta-encodes the result to the wire format; this builder deals only in
// absolute 0-indexed positions so it is trivially unit-testable.

/// The semantic ROLE of an identifier occurrence. The LSP layer maps each
/// variant to an index into the `SemanticTokensLegend` (see
/// `crate::lsp::sem_tok_legend_index`); kept as an internal enum so `analysis`
/// stays free of any LSP type dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemTokKind {
    Function,
    Method,
    Variable,
    Parameter,
    Class,
    Property,
}

/// A single semantic token at an ABSOLUTE 0-indexed position. `line` and
/// `start_char` are 0-indexed (LSP line/character numbering, same encoding as
/// [`byte_offset_to_position`]); `length` is the token's width in characters.
/// The LSP layer converts a sorted `Vec<SemTok>` into the protocol's relative
/// (delta) wire encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemTok {
    pub line: u32,
    pub start_char: u32,
    pub length: u32,
    pub kind: SemTokKind,
}

/// Locate the identifier `name` as a whole word at or after byte offset
/// `from`, returning its absolute `(line, start_char, length)` 0-indexed
/// position. Used to pin a token onto the NAME inside a wider span (a `Func`
/// span covers the whole `def NAME(...)`, a `Param` span may cover `name: T`,
/// an `Attr` span covers `obj.name`).
///
/// "Whole word" means the match is not flanked by identifier characters, so
/// searching for `m` does not match the `m` inside `format`. Returns `None`
/// when the name does not occur after `from` (e.g. a synthesized node or a
/// name the source spells differently) — the caller then SKIPS that token
/// rather than emit a wrong span.
fn locate_name(src: &str, from: usize, name: &str) -> Option<(u32, u32, u32)> {
    if name.is_empty() {
        return None;
    }
    let from = from.min(src.len());
    let hay = &src[from..];
    let nbytes = name.as_bytes();
    let hbytes = hay.as_bytes();
    let mut i = 0usize;
    while i + nbytes.len() <= hbytes.len() {
        if &hbytes[i..i + nbytes.len()] == nbytes {
            let before_ok = i == 0 || !is_word_char(hbytes[i - 1] as char);
            let after_idx = i + nbytes.len();
            let after_ok =
                after_idx >= hbytes.len() || !is_word_char(hbytes[after_idx] as char);
            if before_ok && after_ok {
                let abs = from + i;
                let (line, col) = byte_offset_to_position(src, abs);
                // `name` is the lexer's identifier alphabet (ASCII), so its char
                // length equals its byte length.
                return Some((line, col, name.chars().count() as u32));
            }
        }
        i += 1;
    }
    None
}

/// Push a token for `name` located at/after byte offset `from`, classified as
/// `kind`. A no-op when the name cannot be precisely located (skip rather than
/// mis-position).
fn push_named(out: &mut Vec<SemTok>, src: &str, from: usize, name: &str, kind: SemTokKind) {
    if let Some((line, start_char, length)) = locate_name(src, from, name) {
        out.push(SemTok { line, start_char, length, kind });
    }
}

/// Build the full set of semantic tokens for `module`.
///
/// Emits ONE token per identifier OCCURRENCE, classified by role:
/// - `def NAME` → [`SemTokKind::Function`] (top-level) or [`SemTokKind::Method`]
///   (inside a class); `class NAME` → [`SemTokKind::Class`]; the precise NAME
///   span is located within the (wider) def/class span via [`locate_name`].
/// - function/method PARAMETERS in the signature → [`SemTokKind::Parameter`].
/// - class FIELD declarations (`x: int` in a class body) →
///   [`SemTokKind::Property`].
/// - `Expr::Ident` USES → classified by scope: an enclosing-function parameter →
///   `Parameter`; a reconstructed local → `Variable`; a `ctx.funcs` name →
///   `Function`; a `ctx.classes` name → `Class`; otherwise `Variable`.
/// - `Expr::Attr` attribute name → [`SemTokKind::Method`] when the receiver's
///   type is a class whose (inheritance-aware) method set contains the name,
///   else [`SemTokKind::Property`].
///
/// The result is SORTED by `(line, start_char)` and de-overlapped (a later
/// token whose start lies before the previous token's end is dropped), so the
/// delta encoding in the LSP layer is monotonic and the editor never sees
/// overlapping ranges.
pub fn semantic_tokens(module: &Module, ctx: &TyCtx, src: &str) -> Vec<SemTok> {
    let mut out: Vec<SemTok> = Vec::new();
    for s in &module.stmts {
        collect_stmt_tokens(s, ctx, src, None, None, &mut out);
    }

    // Sort by position, then drop any token that overlaps the one before it so
    // the wire stream is strictly non-overlapping and monotonic.
    out.sort_by_key(|t| (t.line, t.start_char));
    let mut deduped: Vec<SemTok> = Vec::with_capacity(out.len());
    for t in out {
        if let Some(prev) = deduped.last() {
            if prev.line == t.line {
                let prev_end = prev.start_char + prev.length;
                if t.start_char < prev_end {
                    // Overlaps (or duplicates) the previous token — skip it.
                    continue;
                }
            }
        }
        deduped.push(t);
    }
    deduped
}

/// Walk a top-level (or nested) statement, emitting definition/binding tokens
/// for the functions/classes it introduces and use tokens for the expressions
/// it contains. `func`/`class` carry the enclosing scope so uses can be
/// classified.
fn collect_stmt_tokens<'a>(
    s: &'a Stmt,
    ctx: &TyCtx,
    src: &str,
    func: Option<&'a Func>,
    class: Option<&'a ClassDef>,
    out: &mut Vec<SemTok>,
) {
    match s {
        Stmt::Func(f) => collect_func_tokens(f, ctx, src, class, out),
        Stmt::Class(c) => {
            // The class NAME (located within the `class NAME(...)` span).
            push_named(out, src, c.span.start, &c.name, SemTokKind::Class);
            // Field declarations → Property. The field `Param.span` may cover
            // `name: T`; pin to the name within it.
            for field in &c.fields {
                push_named(out, src, field.span.start, &field.name, SemTokKind::Property);
            }
            // Methods → Method-kind def + their own params/bodies.
            for m in &c.methods {
                collect_func_tokens(m, ctx, src, Some(c), out);
            }
        }
        // Statements that hold expressions: emit use tokens from each.
        Stmt::Expr(e) => collect_expr_tokens(e, ctx, src, func, class, out),
        Stmt::Assign { target, value, span, .. } => {
            // The assignment TARGET is a binding stored as a bare `String` (no
            // `Expr::Ident`), so it is colored here. `span` is the target-
            // identifier token span (see `find_local_decl`); classify it the
            // same way a USE of that name would be (a param-shadowing target →
            // Parameter, otherwise Variable) so the declaration matches its uses.
            let kind = classify_ident_use(target, ctx, func);
            push_named(out, src, span.start, target, kind);
            collect_expr_tokens(value, ctx, src, func, class, out);
        }
        Stmt::AugAssign { target, value, span, .. } => {
            // `target op= value`: the target is an in-place use of an existing
            // binding; its `span` is the target-identifier token. Color it like
            // any other use of that name.
            let kind = classify_ident_use(target, ctx, func);
            push_named(out, src, span.start, target, kind);
            collect_expr_tokens(value, ctx, src, func, class, out);
        }
        Stmt::Unpack { value, .. } => collect_expr_tokens(value, ctx, src, func, class, out),
        Stmt::Return(Some(e), _) => collect_expr_tokens(e, ctx, src, func, class, out),
        Stmt::Return(None, _) => {}
        Stmt::If { cond, then, elifs, else_, .. } => {
            collect_expr_tokens(cond, ctx, src, func, class, out);
            for st in then {
                collect_stmt_tokens(st, ctx, src, func, class, out);
            }
            for (c, body) in elifs {
                collect_expr_tokens(c, ctx, src, func, class, out);
                for st in body {
                    collect_stmt_tokens(st, ctx, src, func, class, out);
                }
            }
            if let Some(body) = else_ {
                for st in body {
                    collect_stmt_tokens(st, ctx, src, func, class, out);
                }
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_expr_tokens(cond, ctx, src, func, class, out);
            for st in body {
                collect_stmt_tokens(st, ctx, src, func, class, out);
            }
        }
        Stmt::For { iter, body, .. } => {
            collect_expr_tokens(iter, ctx, src, func, class, out);
            for st in body {
                collect_stmt_tokens(st, ctx, src, func, class, out);
            }
        }
        Stmt::Assert { cond, msg, .. } => {
            collect_expr_tokens(cond, ctx, src, func, class, out);
            if let Some(m) = msg {
                collect_expr_tokens(m, ctx, src, func, class, out);
            }
        }
        Stmt::Raise { exc: Some(e), .. } => collect_expr_tokens(e, ctx, src, func, class, out),
        Stmt::Raise { exc: None, .. } => {}
        Stmt::Try { body, handlers, else_, finally_, .. } => {
            for st in body {
                collect_stmt_tokens(st, ctx, src, func, class, out);
            }
            for h in handlers {
                for st in &h.body {
                    collect_stmt_tokens(st, ctx, src, func, class, out);
                }
            }
            if let Some(b) = else_ {
                for st in b {
                    collect_stmt_tokens(st, ctx, src, func, class, out);
                }
            }
            if let Some(b) = finally_ {
                for st in b {
                    collect_stmt_tokens(st, ctx, src, func, class, out);
                }
            }
        }
        Stmt::With { ctx_expr, body, .. } => {
            collect_expr_tokens(ctx_expr, ctx, src, func, class, out);
            for st in body {
                collect_stmt_tokens(st, ctx, src, func, class, out);
            }
        }
        Stmt::Del { target, .. } => collect_expr_tokens(target, ctx, src, func, class, out),
        Stmt::Match { subject, arms, .. } => {
            collect_expr_tokens(subject, ctx, src, func, class, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_expr_tokens(g, ctx, src, func, class, out);
                }
                for st in &arm.body {
                    collect_stmt_tokens(st, ctx, src, func, class, out);
                }
            }
        }
        Stmt::AttrAssign { obj, value, .. } => {
            collect_expr_tokens(obj, ctx, src, func, class, out);
            collect_expr_tokens(value, ctx, src, func, class, out);
        }
        Stmt::IndexAssign { obj, idx, value, .. } => {
            collect_expr_tokens(obj, ctx, src, func, class, out);
            collect_expr_tokens(idx, ctx, src, func, class, out);
            collect_expr_tokens(value, ctx, src, func, class, out);
        }
        Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::Import { .. } => {}
    }
}

/// Emit the definition token for a function/method's NAME, its PARAMETER tokens,
/// and the use tokens from its body (with `f` set as the enclosing function).
fn collect_func_tokens<'a>(
    f: &'a Func,
    ctx: &TyCtx,
    src: &str,
    class: Option<&'a ClassDef>,
    out: &mut Vec<SemTok>,
) {
    // The function/method NAME: located after the `def ` keyword inside the
    // (wider) def span. Method when inside a class, Function at top level.
    let kind = if class.is_some() { SemTokKind::Method } else { SemTokKind::Function };
    push_named(out, src, f.span.start, &f.name, kind);

    // Parameters in the signature → Parameter. `self` is a real binding too, so
    // color it like any other parameter. The `Param.span` may cover `name: T`;
    // pin to the name within it.
    for p in &f.params {
        push_named(out, src, p.span.start, &p.name, SemTokKind::Parameter);
    }

    // Body uses, scoped to this function.
    for st in &f.body {
        collect_stmt_tokens(st, ctx, src, Some(f), class, out);
    }
}

/// Emit use tokens for every identifier occurrence inside an expression,
/// classified against the enclosing scope (`func`/`class`).
fn collect_expr_tokens<'a>(
    e: &'a Expr,
    ctx: &TyCtx,
    src: &str,
    func: Option<&'a Func>,
    class: Option<&'a ClassDef>,
    out: &mut Vec<SemTok>,
) {
    match e {
        Expr::Ident(name, span) => {
            let kind = classify_ident_use(name, ctx, func);
            // The Ident span IS the bare name; pin precisely all the same.
            push_named(out, src, span.start, name, kind);
        }
        Expr::Attr { obj, name, span } => {
            // Color the receiver first (it may be an Ident/another Attr/Call).
            collect_expr_tokens(obj, ctx, src, func, class, out);
            // Then the attribute name: Method when the receiver's class has a
            // method of that name (inheritance-aware), else Property.
            let kind = classify_attr(obj, name, ctx, src, func, class);
            // The attribute name sits AFTER the receiver inside `obj.name`, so
            // search from the receiver's end (its span end) to skip a same-named
            // receiver token.
            let from = obj.span().end.max(span.start);
            push_named(out, src, from, name, kind);
        }
        Expr::FStr(parts, _) => {
            for p in parts {
                if let crate::ast::FStrPart::Interp(ex, _) = p {
                    collect_expr_tokens(ex, ctx, src, func, class, out);
                }
            }
        }
        Expr::List(elems, _) | Expr::Tuple(elems, _) | Expr::Set(elems, _) => {
            for el in elems {
                collect_expr_tokens(el, ctx, src, func, class, out);
            }
        }
        Expr::Dict(pairs, _) => {
            for (k, v) in pairs {
                collect_expr_tokens(k, ctx, src, func, class, out);
                collect_expr_tokens(v, ctx, src, func, class, out);
            }
        }
        Expr::ListComp { elt, iter, cond, .. } | Expr::SetComp { elt, iter, cond, .. } => {
            collect_expr_tokens(elt, ctx, src, func, class, out);
            collect_expr_tokens(iter, ctx, src, func, class, out);
            if let Some(c) = cond {
                collect_expr_tokens(c, ctx, src, func, class, out);
            }
        }
        Expr::DictComp { key, val, iter, cond, .. } => {
            collect_expr_tokens(key, ctx, src, func, class, out);
            collect_expr_tokens(val, ctx, src, func, class, out);
            collect_expr_tokens(iter, ctx, src, func, class, out);
            if let Some(c) = cond {
                collect_expr_tokens(c, ctx, src, func, class, out);
            }
        }
        Expr::Call { callee, args, kwargs, .. } => {
            collect_expr_tokens(callee, ctx, src, func, class, out);
            for a in args {
                collect_expr_tokens(a, ctx, src, func, class, out);
            }
            for (_, v) in kwargs {
                collect_expr_tokens(v, ctx, src, func, class, out);
            }
        }
        Expr::Index { obj, idx, .. } => {
            collect_expr_tokens(obj, ctx, src, func, class, out);
            collect_expr_tokens(idx, ctx, src, func, class, out);
        }
        Expr::Slice { obj, start, stop, step, .. } => {
            collect_expr_tokens(obj, ctx, src, func, class, out);
            if let Some(x) = start {
                collect_expr_tokens(x, ctx, src, func, class, out);
            }
            if let Some(x) = stop {
                collect_expr_tokens(x, ctx, src, func, class, out);
            }
            if let Some(x) = step {
                collect_expr_tokens(x, ctx, src, func, class, out);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_expr_tokens(lhs, ctx, src, func, class, out);
            collect_expr_tokens(rhs, ctx, src, func, class, out);
        }
        Expr::UnOp { expr, .. } => collect_expr_tokens(expr, ctx, src, func, class, out),
        Expr::Lambda { body, .. } => collect_expr_tokens(body, ctx, src, func, class, out),
        Expr::IfExp { test, body, orelse, .. } => {
            collect_expr_tokens(test, ctx, src, func, class, out);
            collect_expr_tokens(body, ctx, src, func, class, out);
            collect_expr_tokens(orelse, ctx, src, func, class, out);
        }
        Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) | Expr::None_(_) => {}
    }
}

/// Classify a bare-name USE against the enclosing scope, mirroring the spec's
/// priority: an enclosing-function parameter → [`SemTokKind::Parameter`]; a
/// reconstructed local of that function → [`SemTokKind::Variable`]; a
/// `ctx.funcs` name → [`SemTokKind::Function`]; a `ctx.classes` name →
/// [`SemTokKind::Class`]; otherwise [`SemTokKind::Variable`].
fn classify_ident_use(name: &str, ctx: &TyCtx, func: Option<&Func>) -> SemTokKind {
    if let Some(f) = func {
        if f.params.iter().any(|p| p.name == name) {
            return SemTokKind::Parameter;
        }
        if func_introduces_local(f, name) {
            return SemTokKind::Variable;
        }
    }
    if ctx.funcs.contains_key(name) {
        return SemTokKind::Function;
    }
    if ctx.classes.contains_key(name) {
        return SemTokKind::Class;
    }
    SemTokKind::Variable
}

/// True when `name` is bound anywhere in `f`'s body as a local — an assignment
/// target, an unpack/for/with target, or an except-name. Used to color a USE of
/// a local as a Variable. Scans the whole body (not just up to a cursor), which
/// is what classification needs: a name written before its first lexical
/// assignment is still that local in pyrst's single-scope-per-function model.
fn func_introduces_local(f: &Func, name: &str) -> bool {
    fn scan(stmts: &[Stmt], name: &str) -> bool {
        for s in stmts {
            let hit = match s {
                Stmt::Assign { target, .. } | Stmt::AugAssign { target, .. } => target == name,
                Stmt::Unpack { targets, .. } | Stmt::For { targets, .. } => {
                    targets.iter().any(|t| t == name)
                }
                Stmt::With { as_name: Some(n), .. } => n == name,
                _ => false,
            };
            if hit {
                return true;
            }
            // Recurse into nested blocks (a local introduced inside an `if`/`for`
            // is still a local of the function).
            let nested = match s {
                Stmt::If { then, elifs, else_, .. } => {
                    scan(then, name)
                        || elifs.iter().any(|(_, b)| scan(b, name))
                        || else_.as_ref().is_some_and(|b| scan(b, name))
                }
                Stmt::While { body, .. }
                | Stmt::For { body, .. }
                | Stmt::With { body, .. } => scan(body, name),
                Stmt::Try { body, handlers, else_, finally_, .. } => {
                    scan(body, name)
                        || handlers.iter().any(|h| {
                            h.exc_name.as_deref() == Some(name) || scan(&h.body, name)
                        })
                        || else_.as_ref().is_some_and(|b| scan(b, name))
                        || finally_.as_ref().is_some_and(|b| scan(b, name))
                }
                Stmt::Match { arms, .. } => arms.iter().any(|a| scan(&a.body, name)),
                _ => false,
            };
            if nested {
                return true;
            }
        }
        false
    }
    scan(&f.body, name)
}

/// Classify an `obj.name` attribute access: [`SemTokKind::Method`] when `obj`
/// resolves to a class whose (inheritance-aware) method set contains `name`,
/// else [`SemTokKind::Property`].
///
/// Receiver typing reuses the same locals reconstruction the hover/definition
/// paths use, so `self`, typed params, and annotated locals all resolve to
/// their class — never a forked inference. When the receiver's type is unknown
/// or non-class, `Property` is the documented default.
fn classify_attr(
    obj: &Expr,
    name: &str,
    ctx: &TyCtx,
    _src: &str,
    func: Option<&Func>,
    class: Option<&ClassDef>,
) -> SemTokKind {
    // Reconstruct the locals visible across the enclosing function so the
    // receiver can be typed (mirrors `resolve_attr`). Using the function's full
    // extent as the cutoff is sufficient for classification.
    let mut locals: HashMap<String, Ty> = HashMap::new();
    if let Some(f) = func {
        if let Some(c) = class {
            if f.params.iter().any(|p| p.name == "self") {
                locals.insert("self".to_string(), Ty::Class(c.name.clone()));
            }
        }
        for p in &f.params {
            if p.name == "self" {
                continue;
            }
            if let Ok(ty) = Ty::from_type_expr(&p.ty, p.span) {
                locals.insert(p.name.clone(), ty);
            }
        }
        reconstruct_locals(&f.body, f.span.end, ctx, &mut locals);
    }

    let recv = crate::typeck::infer_expr_ty(obj, &locals, ctx);
    if let Ty::Class(class_name) = recv {
        if ctx.get_method(&class_name, name).is_some() {
            return SemTokKind::Method;
        }
    }
    SemTokKind::Property
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

    // ── analyze_str: collect-all (EPIC-LSP L4) ────────────────────────────────

    #[test]
    fn two_functions_with_distinct_type_errors_yield_two_diagnostics() {
        // Each function has its own type mismatch (str assigned to int-annotated
        // local). Collect-all must report BOTH, not just the first.
        let src = concat!(
            "def f() -> None:\n",
            "    a: int = \"s\"\n",
            "\n",
            "def g() -> None:\n",
            "    b: int = \"t\"\n",
        );
        let diags = analyze_str(src);
        assert_eq!(diags.len(), 2, "expected 2 diagnostics, got: {:?}", diags);
        // Ordered top-to-bottom: f's error (line 1) before g's error (line 4).
        assert!(
            diags[0].start.0 < diags[1].start.0,
            "diagnostics should be ordered by line, got starts {:?} and {:?}",
            diags[0].start,
            diags[1].start
        );
        assert_eq!(diags[0].start.0, 1, "first error on 0-indexed line 1");
        assert_eq!(diags[1].start.0, 4, "second error on 0-indexed line 4");
    }

    #[test]
    fn two_methods_with_type_errors_yield_two_diagnostics() {
        // A class whose two methods each have a distinct type error → 2 squiggles.
        let src = concat!(
            "class C:\n",
            "    def m1(self) -> None:\n",
            "        a: int = \"s\"\n",
            "    def m2(self) -> None:\n",
            "        b: int = \"t\"\n",
        );
        let diags = analyze_str(src);
        assert_eq!(diags.len(), 2, "expected 2 diagnostics, got: {:?}", diags);
        assert!(
            diags[0].start.0 < diags[1].start.0,
            "diagnostics should be ordered by line, got starts {:?} and {:?}",
            diags[0].start,
            diags[1].start
        );
    }

    #[test]
    fn clean_module_yields_no_diagnostics_via_collect_all() {
        let src = concat!(
            "def f(x: int) -> int:\n",
            "    return x + 1\n",
            "\n",
            "def g(y: int) -> int:\n",
            "    return y * 2\n",
        );
        let diags = analyze_str(src);
        assert!(diags.is_empty(), "expected no diagnostics, got: {:?}", diags);
    }

    #[test]
    fn single_type_error_still_yields_one_diagnostic() {
        // Unchanged single-error behavior: exactly one diagnostic.
        let src = "def main() -> None:\n    x: int = \"s\"\n";
        let diags = analyze_str(src);
        assert_eq!(diags.len(), 1, "expected exactly 1 diagnostic, got: {:?}", diags);
    }

    #[test]
    fn parse_error_still_yields_exactly_one_diagnostic() {
        // The parser is fail-fast: even with later type errors a syntax error
        // short-circuits to a single diagnostic.
        let src = concat!(
            "def f(\n",            // unclosed paren → parse error
            "def g() -> None:\n",
            "    b: int = \"t\"\n", // would be a type error, but never reached
        );
        let diags = analyze_str(src);
        assert_eq!(diags.len(), 1, "expected exactly 1 parse diagnostic, got: {:?}", diags);
        assert_eq!(diags[0].severity, Severity::Error);
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

    // ── analyze_document (EPIC-LSP L6) ───────────────────────────────────────

    #[test]
    fn analyze_document_clean_program_returns_some() {
        let src = "def f(x: int) -> int:\n    return x + 1\n";
        let result = analyze_document(src);
        assert!(result.is_some(), "clean program should yield Some((module, ctx))");
        let (module, ctx) = result.unwrap();
        // The module has exactly one top-level statement (the function).
        assert_eq!(module.stmts.len(), 1);
        // The TyCtx knows about `f`.
        assert!(ctx.funcs.contains_key("f"), "TyCtx should contain fn `f`");
    }

    #[test]
    fn analyze_document_parse_error_returns_none() {
        let src = "def main(\n"; // unclosed paren
        let result = analyze_document(src);
        assert!(result.is_none(), "parse error should yield None");
    }

    #[test]
    fn analyze_document_type_error_program_still_returns_some() {
        // analyze_document builds the module + ctx regardless of type errors;
        // type errors are the caller's concern (analyze_str, hover, etc.).
        let src = "def main() -> None:\n    x: int = \"bad\"\n";
        let result = analyze_document(src);
        assert!(result.is_some(), "type-erroneous program still parses and yields Some");
    }

    // ── Navigation engine (EPIC-LSP L5) ───────────────────────────────────────

    /// Parse `src` and build a single-module `TyCtx`, the same way `analyze_str`
    /// does. Panics on parse error so tests fail loudly on a malformed fixture.
    fn build(src: &str) -> (crate::ast::Module, TyCtx) {
        let module = parser::parse(src).expect("fixture should parse");
        let mut ctx = TyCtx::new();
        merge_ctx_from_module(&module, &mut ctx, true).expect("fixture should build ctx");
        (module, ctx)
    }

    // ── position_to_byte_offset round-trips ───────────────────────────────────

    #[test]
    fn offset_roundtrip_origin() {
        assert_eq!(position_to_byte_offset("hello\nworld", 0, 0), 0);
    }

    #[test]
    fn offset_roundtrip_mid_first_line() {
        // (0, 2) = 'l' at byte 2.
        assert_eq!(position_to_byte_offset("hello\nworld", 0, 2), 2);
    }

    #[test]
    fn offset_roundtrip_second_line() {
        // "hello\n" is 6 bytes; (1, 0) = byte 6.
        assert_eq!(position_to_byte_offset("hello\nworld", 1, 0), 6);
        // (1, 3) = 'l' at byte 9.
        assert_eq!(position_to_byte_offset("hello\nworld", 1, 3), 9);
    }

    #[test]
    fn offset_clamps_col_past_line_end() {
        // "hello" then newline at byte 5: a column past the content clamps to
        // the newline offset (line end), not into the next line.
        assert_eq!(position_to_byte_offset("hello\nworld", 0, 99), 5);
    }

    #[test]
    fn offset_clamps_line_past_end() {
        assert_eq!(position_to_byte_offset("hi", 9, 0), 2);
    }

    #[test]
    fn offset_inverts_byte_offset_to_position() {
        // Round-trip every byte offset through both directions.
        let src = "def f(x: int) -> int:\n    return x + 1\n";
        for off in 0..=src.len() {
            if !src.is_char_boundary(off) {
                continue;
            }
            let (l, c) = byte_offset_to_position(src, off);
            // A position that lands exactly on a char maps back to that offset
            // (positions at a line-trailing newline are the one exception, and
            // are covered by the clamp tests).
            let back = position_to_byte_offset(src, l, c);
            assert_eq!(back, off, "roundtrip failed at offset {} -> ({},{})", off, l, c);
        }
    }

    // ── node_at_position ──────────────────────────────────────────────────────

    #[test]
    fn node_on_identifier_returns_that_identifier() {
        // `    return x + 1` — cursor on `x` (line 1, col 11).
        let src = "def f(x: int) -> int:\n    return x + 1\n";
        let (module, _ctx) = build(src);
        let path = node_at_position(&module, src, 1, 11).expect("should find a node");
        match path.expr {
            Expr::Ident(n, _) => assert_eq!(n, "x"),
            other => panic!("expected Ident(x), got {:?}", other),
        }
        // It is inside function f.
        assert_eq!(path.func.map(|f| f.name.as_str()), Some("f"));
    }

    #[test]
    fn node_inside_a_plus_b_on_b_returns_b() {
        // `    return a + b` — `a` at col 11, `b` at col 15.
        let src = "def g(a: int, b: int) -> int:\n    return a + b\n";
        let (module, _ctx) = build(src);
        let path = node_at_position(&module, src, 1, 15).expect("should find b");
        match path.expr {
            Expr::Ident(n, _) => assert_eq!(n, "b"),
            other => panic!("expected Ident(b), got {:?}", other),
        }
        // Its parent in the ancestor chain is the BinOp.
        assert!(
            matches!(path.parent(), Some(Expr::BinOp { .. })),
            "expected BinOp parent, got {:?}",
            path.parent()
        );
    }

    #[test]
    fn node_inside_a_plus_b_on_a_returns_a() {
        let src = "def g(a: int, b: int) -> int:\n    return a + b\n";
        let (module, _ctx) = build(src);
        let path = node_at_position(&module, src, 1, 11).expect("should find a");
        match path.expr {
            Expr::Ident(n, _) => assert_eq!(n, "a"),
            other => panic!("expected Ident(a), got {:?}", other),
        }
    }

    #[test]
    fn node_outside_any_expr_returns_none() {
        // Cursor on the `def` keyword (line 0, col 0) — not inside any Expr.
        let src = "def f(x: int) -> int:\n    return x + 1\n";
        let (module, _ctx) = build(src);
        assert!(node_at_position(&module, src, 0, 0).is_none());
    }

    #[test]
    fn node_boundary_start_and_end_of_span() {
        // `    return value` — `value` occupies cols 11..=15 (len 5: 11..16).
        let src = "def f() -> int:\n    return value\n";
        let (module, _ctx) = build(src);
        // Start of the identifier span (col 11): inside.
        let at_start = node_at_position(&module, src, 1, 11).expect("start should resolve");
        assert!(matches!(at_start.expr, Expr::Ident(n, _) if n == "value"));
        // End of the identifier span (col 16, just past the last char): still
        // resolves the identifier (end-inclusive cursor semantics).
        let at_end = node_at_position(&module, src, 1, 16).expect("end should resolve");
        assert!(matches!(at_end.expr, Expr::Ident(n, _) if n == "value"));
    }

    // ── type_at_position ──────────────────────────────────────────────────────

    #[test]
    fn type_of_int_param_use() {
        // Hover over `x` (an `int` param) in the body.
        let src = "def f(x: int) -> int:\n    return x + 1\n";
        let (module, ctx) = build(src);
        let ty = type_at_position(&module, &ctx, src, 1, 11);
        assert_eq!(ty, Some(Ty::Int));
    }

    #[test]
    fn type_of_annotated_local() {
        // `    y: str = "hi"` then `    return y` — hover over the use of `y`.
        let src = "def f() -> str:\n    y: str = \"hi\"\n    return y\n";
        let (module, ctx) = build(src);
        // `return y`: `y` is on line 2 at col 11.
        let ty = type_at_position(&module, &ctx, src, 2, 11);
        assert_eq!(ty, Some(Ty::Str));
    }

    #[test]
    fn type_of_call_to_str_returning_function() {
        // `g()` returns str; hover over the call.
        let src = concat!(
            "def g() -> str:\n",
            "    return \"x\"\n",
            "\n",
            "def f() -> str:\n",
            "    return g()\n",
        );
        let (module, ctx) = build(src);
        // `return g()` on line 4; the call `g()` starts at col 11.
        let ty = type_at_position(&module, &ctx, src, 4, 11);
        assert_eq!(ty, Some(Ty::Str));
    }

    #[test]
    fn type_of_class_typed_local() {
        // A local typed as a class via constructor inference.
        let src = concat!(
            "class C:\n",
            "    x: int\n",
            "\n",
            "def f() -> int:\n",
            "    c = C()\n",
            "    return c.x\n",
        );
        let (module, ctx) = build(src);
        // `return c.x` on line 5; `c` (the receiver) is at col 11.
        let ty = type_at_position(&module, &ctx, src, 5, 11);
        assert_eq!(ty, Some(Ty::Class("C".to_string())));
    }

    #[test]
    fn type_of_list_literal() {
        // Hover over a `[1, 2, 3]` list literal.
        let src = "def f() -> int:\n    xs = [1, 2, 3]\n    return xs[0]\n";
        let (module, ctx) = build(src);
        // The list literal `[1, 2, 3]` starts at col 9 on line 1.
        let ty = type_at_position(&module, &ctx, src, 1, 9);
        assert_eq!(ty, Some(Ty::List(Box::new(Ty::Int))));
    }

    #[test]
    fn type_of_unknown_is_none() {
        // Hover over an undefined name → Unknown → None (hover stays quiet).
        let src = "def f() -> int:\n    return undefined_name\n";
        let (module, ctx) = build(src);
        let ty = type_at_position(&module, &ctx, src, 1, 11);
        assert_eq!(ty, None);
    }

    // ── definition_at_position ────────────────────────────────────────────────

    #[test]
    fn def_of_param_use_is_param_span() {
        // Use of param `x` resolves to the param's declaration span.
        let src = "def f(x: int) -> int:\n    return x + 1\n";
        let (module, ctx) = build(src);
        let def = definition_at_position(&module, &ctx, src, 1, 11).expect("should resolve param");
        // The param `x` is declared at line 0, col 6 (1-indexed line/col on the span).
        // Span line/col are 1-indexed; verify via byte_offset_to_position on start.
        let (dl, dc) = byte_offset_to_position(src, def.start);
        assert_eq!((dl, dc), (0, 6), "param decl should be at (0,6)");
    }

    #[test]
    fn def_of_local_use_is_assignment_target_span() {
        // Use of local `y` resolves to its assignment target identifier span.
        let src = "def f() -> str:\n    y: str = \"hi\"\n    return y\n";
        let (module, ctx) = build(src);
        let def = definition_at_position(&module, &ctx, src, 2, 11).expect("should resolve local");
        // `y` is declared on line 1 at col 4 (0-indexed).
        let (dl, dc) = byte_offset_to_position(src, def.start);
        assert_eq!((dl, dc), (1, 4), "local decl should be at (1,4)");
    }

    #[test]
    fn def_of_function_call_is_func_def_span() {
        // `f()` call resolves to f's `def` line.
        let src = concat!(
            "def helper() -> int:\n",
            "    return 1\n",
            "\n",
            "def main() -> int:\n",
            "    return helper()\n",
        );
        let (module, ctx) = build(src);
        // `helper()` call on line 4 at col 11.
        let def = definition_at_position(&module, &ctx, src, 4, 11).expect("should resolve func");
        let (dl, _dc) = byte_offset_to_position(src, def.start);
        assert_eq!(dl, 0, "helper def should be on line 0");
    }

    #[test]
    fn def_of_constructor_is_class_span() {
        // `C()` constructor resolves to the class definition span.
        let src = concat!(
            "class C:\n",
            "    x: int\n",
            "\n",
            "def f() -> int:\n",
            "    c = C()\n",
            "    return c.x\n",
        );
        let (module, ctx) = build(src);
        // `C()` on line 4 at col 8.
        let def = definition_at_position(&module, &ctx, src, 4, 8).expect("should resolve class");
        let (dl, _dc) = byte_offset_to_position(src, def.start);
        assert_eq!(dl, 0, "class C should be on line 0");
    }

    #[test]
    fn def_of_attribute_is_field_span() {
        // `c.x` (a field access) resolves to the field's declaration span.
        let src = concat!(
            "class C:\n",
            "    x: int\n",
            "\n",
            "def f() -> int:\n",
            "    c = C()\n",
            "    return c.x\n",
        );
        let (module, ctx) = build(src);
        // The `.x` attribute access: cursor on `x` after the dot. `c.x` starts at
        // col 11; `c`=11, `.`=12, `x`=13 on line 5.
        let def = definition_at_position(&module, &ctx, src, 5, 13).expect("should resolve field");
        let (dl, _dc) = byte_offset_to_position(src, def.start);
        assert_eq!(dl, 1, "field x should be declared on line 1");
    }

    #[test]
    fn def_of_method_call_is_method_span() {
        // `self.m()` method call resolves to the method's `def` span.
        let src = concat!(
            "class C:\n",
            "    def m(self) -> int:\n",
            "        return 1\n",
            "    def caller(self) -> int:\n",
            "        return self.m()\n",
        );
        let (module, ctx) = build(src);
        // `self.m()` on line 4; `self`=15..19, `.`=19, `m`=20. Cursor on `m`.
        let def = definition_at_position(&module, &ctx, src, 4, 20).expect("should resolve method");
        let (dl, _dc) = byte_offset_to_position(src, def.start);
        assert_eq!(dl, 1, "method m should be defined on line 1");
    }

    #[test]
    fn def_of_unresolved_is_none() {
        let src = "def f() -> int:\n    return undefined_name\n";
        let (module, ctx) = build(src);
        // `undefined_name` is not a param, local, func, or class.
        assert!(definition_at_position(&module, &ctx, src, 1, 11).is_none());
    }

    // ── binding_at_position (hover on declaration sites) ─────────────────────

    #[test]
    fn hover_annotated_assign_target_returns_annotation_type() {
        // `total: int = add(2, 3)` — hover the `total` identifier (col 4).
        let src = concat!(
            "def add(a: int, b: int) -> int:\n",  // line 0
            "    return a + b\n",                  // line 1
            "def f() -> int:\n",                   // line 2
            "    total: int = add(2, 3)\n",        // line 3
            "    return total\n",                  // line 4
        );
        let (module, ctx) = build(src);
        // `total` on line 3, col 4.
        let ty = type_at_position(&module, &ctx, src, 3, 4);
        assert_eq!(ty, Some(Ty::Int), "annotated target should return annotation type");
    }

    #[test]
    fn hover_bare_assign_target_returns_inferred_type() {
        // `n = 5` — hover the `n` identifier (col 4); type inferred from RHS.
        let src = concat!(
            "def f() -> int:\n",  // line 0
            "    n = 5\n",        // line 1
            "    return n\n",     // line 2
        );
        let (module, ctx) = build(src);
        // `n` on line 1, col 4.
        let ty = type_at_position(&module, &ctx, src, 1, 4);
        assert_eq!(ty, Some(Ty::Int), "bare target should return inferred RHS type");
    }

    #[test]
    fn hover_param_name_in_signature_returns_param_type() {
        // `def f(a: int, b: int) -> int:` — hover on `a` in the signature.
        // `def f(` = 6 chars, so `a` is at col 6.
        let src = concat!(
            "def f(a: int, b: int) -> int:\n",  // line 0
            "    return a + b\n",               // line 1
        );
        let (module, ctx) = build(src);
        // `a` on line 0, col 6.
        let ty = type_at_position(&module, &ctx, src, 0, 6);
        assert_eq!(ty, Some(Ty::Int), "hovering param `a` should return int");
    }

    #[test]
    fn hover_second_param_name_in_signature() {
        // `def f(a: int, b: str) -> str:` — hover on `b`.
        // `def f(a: int, ` = 14 chars, so `b` is at col 14.
        let src = concat!(
            "def f(a: int, b: str) -> str:\n",  // line 0
            "    return b\n",                   // line 1
        );
        let (module, ctx) = build(src);
        // `b` on line 0, col 14.
        let ty = type_at_position(&module, &ctx, src, 0, 14);
        assert_eq!(ty, Some(Ty::Str), "hovering param `b` should return str");
    }

    #[test]
    fn hover_self_param_in_method_returns_class_type() {
        // `    def m(self) -> int:` — hover on `self` (col 10).
        // `    def m(` = 10 chars, so `self` starts at col 10.
        let src = concat!(
            "class C:\n",                        // line 0
            "    x: int\n",                      // line 1
            "    def m(self) -> int:\n",         // line 2
            "        return self.x\n",           // line 3
        );
        let (module, ctx) = build(src);
        // `self` on line 2, col 10.
        let ty = type_at_position(&module, &ctx, src, 2, 10);
        assert_eq!(ty, Some(Ty::Class("C".to_string())), "hovering `self` should return the class type");
    }

    #[test]
    fn hover_use_of_param_in_body_still_works() {
        // Regression: expression USE path must still work after the refactor.
        // `    return x + 1` — hover on `x` (col 11).
        let src = "def f(x: int) -> int:\n    return x + 1\n";
        let (module, ctx) = build(src);
        let ty = type_at_position(&module, &ctx, src, 1, 11);
        assert_eq!(ty, Some(Ty::Int), "USE of param should still resolve to int");
    }

    #[test]
    fn hover_on_whitespace_returns_none() {
        // Cursor between two functions on a blank line → no binding, no expr → None.
        let src = concat!(
            "def f() -> int:\n",  // line 0
            "    return 1\n",     // line 1
            "\n",                 // line 2  (blank)
            "def g() -> int:\n",  // line 3
            "    return 2\n",     // line 4
        );
        let (module, ctx) = build(src);
        // Cursor on the blank line (line 2, col 0).
        let ty = type_at_position(&module, &ctx, src, 2, 0);
        assert_eq!(ty, None, "whitespace cursor should yield None");
    }

    #[test]
    fn hover_annotated_assign_in_method() {
        // `        result: str = "hello"` inside a method — hover on `result`.
        // 8 spaces + `result` → `result` at col 8.
        let src = concat!(
            "class C:\n",                                  // line 0
            "    def greet(self) -> str:\n",               // line 1
            "        result: str = \"hello\"\n",           // line 2
            "        return result\n",                     // line 3
        );
        let (module, ctx) = build(src);
        // `result` on line 2, col 8.
        let ty = type_at_position(&module, &ctx, src, 2, 8);
        assert_eq!(ty, Some(Ty::Str), "annotated method-local target should return str");
    }

    // ── Autocomplete: member_completions (EPIC-LSP L7) ────────────────────────

    /// Look up a completion by label in a slice (helper for assertions).
    fn find<'a>(cs: &'a [Completion], label: &str) -> Option<&'a Completion> {
        cs.iter().find(|c| c.label == label)
    }

    #[test]
    fn member_completions_two_fields_one_method() {
        // A class with two fields (x: int, name: str) and one method greet().
        let src = concat!(
            "class P:\n",
            "    x: int\n",
            "    name: str\n",
            "    def greet(self, a: int) -> str:\n",
            "        return self.name\n",
        );
        let (_module, ctx) = build(src);
        let cs = member_completions(&Ty::Class("P".to_string()), &ctx);
        // Exactly 3 completions: 2 fields + 1 method.
        assert_eq!(cs.len(), 3, "expected 3 completions, got: {:?}", cs);

        let x = find(&cs, "x").expect("field x");
        assert_eq!(x.kind, CompletionKind::Field);
        assert_eq!(x.detail.as_deref(), Some(": int"));

        let name = find(&cs, "name").expect("field name");
        assert_eq!(name.kind, CompletionKind::Field);
        assert_eq!(name.detail.as_deref(), Some(": str"));

        let greet = find(&cs, "greet").expect("method greet");
        assert_eq!(greet.kind, CompletionKind::Method);
        // `self` is omitted from the rendered signature.
        assert_eq!(greet.detail.as_deref(), Some("(a: int) -> str"));
    }

    #[test]
    fn member_completions_includes_inherited_for_subclass() {
        // Base has field `base_field` + method `base_method`; Derived adds
        // `own_field`. Completing on Derived must include the inherited members.
        let src = concat!(
            "class Base:\n",
            "    base_field: int\n",
            "    def base_method(self) -> None:\n",
            "        pass\n",
            "class Derived(Base):\n",
            "    own_field: str\n",
            "    def own_method(self) -> None:\n",
            "        pass\n",
        );
        let (_module, ctx) = build(src);
        let cs = member_completions(&Ty::Class("Derived".to_string()), &ctx);
        // Inherited field + method present.
        let bf = find(&cs, "base_field").expect("inherited field");
        assert_eq!(bf.kind, CompletionKind::Field);
        let bm = find(&cs, "base_method").expect("inherited method");
        assert_eq!(bm.kind, CompletionKind::Method);
        // Own members present too.
        assert!(find(&cs, "own_field").is_some(), "own field present");
        assert!(find(&cs, "own_method").is_some(), "own method present");
    }

    #[test]
    fn member_completions_non_class_is_empty() {
        let src = "def f() -> int:\n    return 1\n";
        let (_module, ctx) = build(src);
        assert!(member_completions(&Ty::Int, &ctx).is_empty(), "int has no members");
        assert!(
            member_completions(&Ty::List(Box::new(Ty::Int)), &ctx).is_empty(),
            "list has no class members"
        );
        // A class name that doesn't exist → empty (no panic).
        assert!(member_completions(&Ty::Class("Nope".into()), &ctx).is_empty());
    }

    // ── Autocomplete: completion_context ──────────────────────────────────────

    #[test]
    fn context_member_bare_dot() {
        // `    p.` → Member, receiver "p", empty partial. Cursor at end (col 6).
        let src = "    p.";
        match completion_context(src, 0, 6) {
            CompletionContext::Member { receiver, partial, receiver_end } => {
                assert_eq!(receiver, "p");
                assert_eq!(partial, "");
                // The `.` is at col 5 (0-indexed): "    p" = 5 chars.
                assert_eq!(receiver_end, (0, 5));
            }
            other => panic!("expected Member, got {:?}", other),
        }
    }

    #[test]
    fn context_member_with_partial() {
        // `    p.di` → Member, receiver "p", partial "di". Cursor at end (col 8).
        let src = "    p.di";
        match completion_context(src, 0, 8) {
            CompletionContext::Member { receiver, partial, .. } => {
                assert_eq!(receiver, "p");
                assert_eq!(partial, "di");
            }
            other => panic!("expected Member, got {:?}", other),
        }
    }

    #[test]
    fn context_scope_partial_word() {
        // `    tot` → Scope, partial "tot". Cursor at end (col 7).
        let src = "    tot";
        match completion_context(src, 0, 7) {
            CompletionContext::Scope { partial } => assert_eq!(partial, "tot"),
            other => panic!("expected Scope, got {:?}", other),
        }
    }

    #[test]
    fn context_member_self_dot() {
        // `self.` → Member, receiver "self", empty partial. Cursor at col 5.
        let src = "self.";
        match completion_context(src, 0, 5) {
            CompletionContext::Member { receiver, partial, receiver_end } => {
                assert_eq!(receiver, "self");
                assert_eq!(partial, "");
                assert_eq!(receiver_end, (0, 4)); // `.` after "self"
            }
            other => panic!("expected Member, got {:?}", other),
        }
    }

    #[test]
    fn context_empty_prefix_is_scope_empty_partial() {
        // Cursor at the very start of a line → Scope with empty partial.
        let src = "    ";
        match completion_context(src, 0, 4) {
            CompletionContext::Scope { partial } => assert_eq!(partial, ""),
            other => panic!("expected Scope, got {:?}", other),
        }
    }

    #[test]
    fn context_dotted_receiver() {
        // `a.b.c` → Member, receiver "a.b", partial "c" (cursor at end, col 5).
        let src = "a.b.c";
        match completion_context(src, 0, 5) {
            CompletionContext::Member { receiver, partial, .. } => {
                assert_eq!(receiver, "a.b");
                assert_eq!(partial, "c");
            }
            other => panic!("expected Member, got {:?}", other),
        }
    }

    // ── Autocomplete: scope_completions ───────────────────────────────────────

    #[test]
    fn scope_completions_local_param_func_class() {
        // A top-level function `helper`, a class `C`, and a function `f` with a
        // param `arg` and a local `total`. Inside f's body we should see all four.
        let src = concat!(
            "class C:\n",                       // line 0
            "    x: int\n",                     // line 1
            "def helper() -> int:\n",           // line 2
            "    return 1\n",                   // line 3
            "def f(arg: int) -> int:\n",        // line 4
            "    total: int = 5\n",             // line 5
            "    return total\n",               // line 6
        );
        let (module, ctx) = build(src);
        // Cursor inside f's body on the `return total` line (line 6, col 11 → on
        // the `total` use, well within f's scope).
        let cs = scope_completions(&module, &ctx, src, 6, 11);

        let total = find(&cs, "total").expect("local total");
        assert_eq!(total.kind, CompletionKind::Variable);

        let arg = find(&cs, "arg").expect("param arg");
        assert_eq!(arg.kind, CompletionKind::Variable);

        let helper = find(&cs, "helper").expect("top-level func helper");
        assert_eq!(helper.kind, CompletionKind::Function);

        let c = find(&cs, "C").expect("class C");
        assert_eq!(c.kind, CompletionKind::Class);

        // A builtin function is present too (seeded into TyCtx::new).
        let len = find(&cs, "len").expect("builtin len");
        assert_eq!(len.kind, CompletionKind::Function);

        // No duplicate labels.
        let mut labels: Vec<&str> = cs.iter().map(|c| c.label.as_str()).collect();
        let n = labels.len();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), n, "scope completions must be deduped by label");
    }

    // ── Autocomplete: the repair path (THE key case) ──────────────────────────

    #[test]
    fn unparseable_dot_buffer_does_not_parse() {
        // Sanity: the buffer ending in `p.` genuinely fails to parse, so the
        // repair strategy is load-bearing (analyze_document returns None).
        let src = concat!(
            "class P:\n",
            "    x: int\n",
            "    name: str\n",
            "    def greet(self) -> str:\n",
            "        return self.name\n",
            "def f() -> None:\n",
            "    p: P = P()\n",
            "    p.\n",
        );
        assert!(
            analyze_document(src).is_none(),
            "the trailing `p.` must make the raw buffer unparseable"
        );
    }

    #[test]
    fn member_completions_at_repairs_trailing_dot() {
        // THE case the LEAD pipes: a class with fields + method, a `p: P = P()`
        // local, and a trailing `    p.` line. Completion at end-of-line must
        // return the class members despite the buffer not parsing.
        let src = concat!(
            "class P:\n",                          // line 0
            "    x: int\n",                        // line 1
            "    name: str\n",                     // line 2
            "    def greet(self) -> str:\n",       // line 3
            "        return self.name\n",          // line 4
            "def f() -> None:\n",                  // line 5
            "    p: P = P()\n",                    // line 6
            "    p.\n",                            // line 7
        );
        // Cursor right after the `.` on line 7: "    p." = 6 chars → col 6.
        let cs = member_completions_at(src, 7, 6);
        assert!(find(&cs, "x").is_some(), "field x present, got: {:?}", cs);
        assert!(find(&cs, "name").is_some(), "field name present");
        assert!(find(&cs, "greet").is_some(), "method greet present");
        // Exactly the 2 fields + 1 method.
        assert_eq!(cs.len(), 3, "expected 3 members, got: {:?}", cs);
    }

    #[test]
    fn member_completions_at_filters_by_partial() {
        // Same fixture but a partial `na` after the dot → only `name` survives.
        let src = concat!(
            "class P:\n",
            "    x: int\n",
            "    name: str\n",
            "    def greet(self) -> str:\n",
            "        return self.name\n",
            "def f() -> None:\n",
            "    p: P = P()\n",
            "    p.na\n",                           // line 7
        );
        // Cursor after `na`: "    p.na" = 8 chars → col 8.
        let cs = member_completions_at(src, 7, 8);
        assert_eq!(cs.len(), 1, "only `name` matches prefix `na`, got: {:?}", cs);
        assert_eq!(cs[0].label, "name");
    }

    #[test]
    fn member_completions_at_self_receiver() {
        // `self.` inside a method repairs to `self` and types as the class.
        let src = concat!(
            "class P:\n",
            "    x: int\n",
            "    def greet(self) -> str:\n",
            "        self.\n",                      // line 3
        );
        // Cursor after `self.`: 8 spaces + "self." = 13 chars → col 13.
        let cs = member_completions_at(src, 3, 13);
        assert!(find(&cs, "x").is_some(), "self.x field present, got: {:?}", cs);
        assert!(find(&cs, "greet").is_some(), "self.greet method present");
    }

    #[test]
    fn member_completions_at_unknown_receiver_is_empty() {
        // `q.` where `q` has no known type → empty (no panic).
        let src = concat!(
            "def f() -> None:\n",
            "    q.\n",                             // line 1
        );
        let cs = member_completions_at(src, 1, 6);
        assert!(cs.is_empty(), "unknown receiver yields no members, got: {:?}", cs);
    }

    #[test]
    fn member_completions_at_on_scope_context_is_empty() {
        // A non-member context fed to the member path returns empty.
        let src = "    tot";
        assert!(member_completions_at(src, 0, 7).is_empty());
    }

    // ── Semantic tokens (EPIC-LSP L7) ─────────────────────────────────────────

    /// Find a token at a 0-indexed `(line, start_char)` position.
    fn tok_at(toks: &[SemTok], line: u32, start_char: u32) -> Option<&SemTok> {
        toks.iter().find(|t| t.line == line && t.start_char == start_char)
    }

    #[test]
    fn sem_tokens_def_yields_function_and_parameter() {
        // `def add(a: int) -> int:` — `add` is a Function token, `a` a Parameter.
        // `def ` = 4 chars → `add` at col 4 (len 3); `add(` → `a` at col 8 (len 1).
        let src = "def add(a: int) -> int:\n    return a\n";
        let (module, ctx) = build(src);
        let toks = semantic_tokens(&module, &ctx, src);

        let add = tok_at(&toks, 0, 4).expect("function name token at (0,4)");
        assert_eq!(add.kind, SemTokKind::Function);
        assert_eq!(add.length, 3, "`add` is 3 chars");

        let a_param = tok_at(&toks, 0, 8).expect("parameter token at (0,8)");
        assert_eq!(a_param.kind, SemTokKind::Parameter);
        assert_eq!(a_param.length, 1);

        // The USE of `a` in `return a` (line 1) is also a Parameter.
        let a_use = toks.iter().find(|t| t.line == 1).expect("a use on line 1");
        assert_eq!(a_use.kind, SemTokKind::Parameter);
    }

    #[test]
    fn sem_tokens_class_name_and_field_property() {
        // `class P:` → Class token on `P`; `x: int` field → Property.
        // `class ` = 6 chars → `P` at col 6 (len 1); field `    x` → col 4.
        let src = concat!(
            "class P:\n",      // line 0
            "    x: int\n",    // line 1
        );
        let (module, ctx) = build(src);
        let toks = semantic_tokens(&module, &ctx, src);

        let p = tok_at(&toks, 0, 6).expect("class name token at (0,6)");
        assert_eq!(p.kind, SemTokKind::Class);
        assert_eq!(p.length, 1);

        let field = tok_at(&toks, 1, 4).expect("field token at (1,4)");
        assert_eq!(field.kind, SemTokKind::Property);
        assert_eq!(field.length, 1, "`x` is 1 char");
    }

    #[test]
    fn sem_tokens_method_kind_inside_class() {
        // A method `def greet(self) ...` inside a class is a Method (not Function),
        // and `self` is a Parameter.
        let src = concat!(
            "class P:\n",                       // line 0
            "    x: int\n",                     // line 1
            "    def greet(self) -> int:\n",    // line 2
            "        return self.x\n",          // line 3
        );
        let (module, ctx) = build(src);
        let toks = semantic_tokens(&module, &ctx, src);

        // `    def ` = 8 chars → `greet` at col 8.
        let greet = tok_at(&toks, 2, 8).expect("method name token at (2,8)");
        assert_eq!(greet.kind, SemTokKind::Method, "method def, not a top-level function");

        // `self` parameter present on the signature line.
        let self_param = toks
            .iter()
            .find(|t| t.line == 2 && t.kind == SemTokKind::Parameter)
            .expect("self parameter on signature line");
        assert_eq!(self_param.length, 4);
    }

    #[test]
    fn sem_tokens_attr_method_vs_property() {
        // `self.x` is a Property (a field); `self.greet()` is a Method.
        let src = concat!(
            "class P:\n",                              // line 0
            "    x: int\n",                            // line 1
            "    def greet(self) -> int:\n",           // line 2
            "        return self.x\n",                 // line 3
            "    def caller(self) -> int:\n",          // line 4
            "        return self.greet()\n",           // line 5
        );
        let (module, ctx) = build(src);
        let toks = semantic_tokens(&module, &ctx, src);

        // Line 3 `return self.x`: the `.x` access → Property. `        return ` =
        // 8+7 = 15 chars → `self`=15, `.`=19, `x`=20.
        let x_attr = tok_at(&toks, 3, 20).expect("attr `x` at (3,20)");
        assert_eq!(x_attr.kind, SemTokKind::Property, "field access is a Property");

        // Line 5 `return self.greet()`: the `.greet` access → Method. `greet`=20.
        let greet_attr = tok_at(&toks, 5, 20).expect("attr `greet` at (5,20)");
        assert_eq!(greet_attr.kind, SemTokKind::Method, "method access is a Method");
    }

    #[test]
    fn sem_tokens_local_use_is_variable_and_func_use_is_function() {
        // A top-level function `helper`, and `f` with a local `total`. Inside f:
        // the `total` use is a Variable; the `helper()` call is a Function.
        let src = concat!(
            "def helper() -> int:\n",           // line 0
            "    return 1\n",                    // line 1
            "def f() -> int:\n",                 // line 2
            "    total: int = helper()\n",       // line 3
            "    return total\n",                // line 4
        );
        let (module, ctx) = build(src);
        let toks = semantic_tokens(&module, &ctx, src);

        // Line 3 `    total: int = helper()`: `total`=4 (Variable use of a local),
        // `helper`=17 (Function use).
        let total = tok_at(&toks, 3, 4).expect("total at (3,4)");
        assert_eq!(total.kind, SemTokKind::Variable, "local use is a Variable");

        let helper = tok_at(&toks, 3, 17).expect("helper at (3,17)");
        assert_eq!(helper.kind, SemTokKind::Function, "top-level func use is a Function");

        // Line 4 `    return total`: the `total` use is a Variable too.
        let total_use = tok_at(&toks, 4, 11).expect("total use at (4,11)");
        assert_eq!(total_use.kind, SemTokKind::Variable);
    }

    #[test]
    fn sem_tokens_class_use_is_class() {
        // `C()` constructor use → the `C` callee is a Class token.
        let src = concat!(
            "class C:\n",           // line 0
            "    x: int\n",         // line 1
            "def f() -> int:\n",    // line 2
            "    c = C()\n",        // line 3
            "    return c.x\n",     // line 4
        );
        let (module, ctx) = build(src);
        let toks = semantic_tokens(&module, &ctx, src);

        // Line 3 `    c = C()`: `c`=4 (Variable local), `C`=8 (Class use).
        let c_use = tok_at(&toks, 3, 8).expect("C at (3,8)");
        assert_eq!(c_use.kind, SemTokKind::Class, "constructor callee is a Class");
    }

    #[test]
    fn sem_tokens_sorted_and_non_overlapping() {
        // The output must be sorted by (line, start_char) and non-overlapping.
        let src = concat!(
            "def add(a: int, b: int) -> int:\n",
            "    return a + b\n",
        );
        let (module, ctx) = build(src);
        let toks = semantic_tokens(&module, &ctx, src);
        assert!(!toks.is_empty());
        for w in toks.windows(2) {
            let (p, n) = (w[0], w[1]);
            // Sorted.
            assert!(
                (p.line, p.start_char) <= (n.line, n.start_char),
                "tokens must be sorted: {:?} then {:?}",
                p,
                n
            );
            // Non-overlapping on the same line.
            if p.line == n.line {
                assert!(
                    n.start_char >= p.start_char + p.length,
                    "tokens must not overlap: {:?} then {:?}",
                    p,
                    n
                );
            }
        }
    }

    #[test]
    fn sem_tokens_unknown_receiver_attr_is_property() {
        // `q.field` where `q` has no known type → Property (the documented default).
        let src = concat!(
            "def f(q: int) -> int:\n",   // q is int (non-class)
            "    return q.bit_length\n",
        );
        let (module, ctx) = build(src);
        let toks = semantic_tokens(&module, &ctx, src);
        // `    return q.bit_length`: `q`=11, `.`=12, `bit_length`=13.
        let attr = tok_at(&toks, 1, 13).expect("attr token at (1,13)");
        assert_eq!(attr.kind, SemTokKind::Property, "non-class receiver → Property default");
    }
}
