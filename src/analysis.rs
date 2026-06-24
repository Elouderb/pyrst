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
}
