use super::*;

/// The source-level spelling of a binary operator, used to build honest error
/// messages that echo the user's actual operator (e.g. the generator-materialize
/// fix `list(g) * 2` for `g * 2`). Mirrors the formatter's operator table.
pub(crate) fn binop_symbol(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/",
        BinOp::FloorDiv => "//", BinOp::Mod => "%", BinOp::Pow => "**",
        BinOp::Eq => "==", BinOp::Ne => "!=", BinOp::Lt => "<", BinOp::Le => "<=",
        BinOp::Gt => ">", BinOp::Ge => ">=", BinOp::And => "and", BinOp::Or => "or",
        BinOp::Is => "is", BinOp::IsNot => "is not", BinOp::In => "in", BinOp::NotIn => "not in",
        BinOp::BitAnd => "&", BinOp::BitOr => "|", BinOp::BitXor => "^",
        BinOp::LShift => "<<", BinOp::RShift => ">>",
    }
}

/// (kwargs v1, card 8a7b7714) Where each parameter SLOT of a keyword-bearing
/// call gets its value: a positional argument, a keyword argument, or the
/// parameter's declared default. Produced by [`map_kwargs_to_slots`] and shared
/// by the checking path (which validates provided slots) and codegen (which
/// lowers the call as a full positional call in parameter order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgSlot {
    /// Filled by positional argument `args[i]`.
    Pos(usize),
    /// Filled by the VALUE of keyword argument `kwargs[j]`.
    Kw(usize),
    /// Filled by the parameter's declared default expression.
    Default,
}

/// (kwargs v1, card 8a7b7714) The keyword→positional mapping: given a resolved
/// signature (param names + defaults, self-exclusive for methods), the number of
/// POSITIONAL arguments at the call, and the keyword arguments, produce one
/// [`ArgSlot`] per declared parameter. CPython semantics: positional arguments
/// fill parameters left-to-right; each keyword argument binds the parameter of
/// that name; every remaining parameter takes its default. Check-time errors
/// (CPython-shaped, pyrst-worded):
///   - a keyword that names no parameter        → "unexpected keyword argument";
///   - a keyword whose slot is already filled
///     (positionally or by an earlier keyword)  → "multiple values for argument";
///   - an unfilled parameter with no default    → "missing a required argument";
///   - more positional arguments than parameters → the classic arity error.
///
/// pyrst has no keyword-only (`*`) marker, so every parameter is
/// positional-or-keyword — a deliberate, documented superset of signatures that
/// use `*` in CPython (calls valid there are valid here; pyrst additionally
/// accepts the positional spelling).
pub fn map_kwargs_to_slots(
    site: &str,
    sig: &FuncSig,
    n_pos: usize,
    kwargs: &[(String, Expr)],
    span: Span,
) -> Result<Vec<ArgSlot>> {
    let n_params = sig.params.len();
    if n_pos > n_params {
        return Err(Error::Type {
            span,
            msg: format!(
                "function `{}` takes {} argument(s), {} given",
                site,
                n_params,
                n_pos + kwargs.len()
            ),
        });
    }
    let mut slots: Vec<Option<ArgSlot>> = vec![None; n_params];
    for (i, slot) in slots.iter_mut().take(n_pos).enumerate() {
        *slot = Some(ArgSlot::Pos(i));
    }
    for (j, (kw, _)) in kwargs.iter().enumerate() {
        let p = sig
            .params
            .iter()
            .position(|(n, _)| n == kw)
            .ok_or_else(|| Error::Type {
                span,
                msg: format!("`{}` got an unexpected keyword argument `{}`", site, kw),
            })?;
        if slots[p].is_some() {
            return Err(Error::Type {
                span,
                msg: format!("`{}` got multiple values for argument `{}`", site, kw),
            });
        }
        slots[p] = Some(ArgSlot::Kw(j));
    }
    slots
        .into_iter()
        .enumerate()
        .map(|(p, s)| match s {
            Some(s) => Ok(s),
            None => {
                if sig.param_defaults.get(p).is_some_and(|d| d.is_some()) {
                    Ok(ArgSlot::Default)
                } else {
                    Err(Error::Type {
                        span,
                        msg: format!(
                            "`{}` missing a required argument: `{}`",
                            site, sig.params[p].0
                        ),
                    })
                }
            }
        })
        .collect()
}

/// (kwargs v1) The provided (parameter-slot, argument-expression) pairs of a
/// keyword-bearing call in EVALUATION order: positional arguments left-to-right,
/// then keyword VALUES in their source order — exactly CPython's call-site
/// evaluation order — each paired with the parameter slot it fills. `Default`
/// slots contribute nothing (the callee-signature default is injected by
/// codegen, in slot position). Shared by the checking loop and codegen so the
/// two walk the same pairs.
pub fn kwargs_provided_in_eval_order<'a>(
    args: &'a [Expr],
    kwargs: &'a [(String, Expr)],
    slots: &[ArgSlot],
) -> Vec<(usize, &'a Expr)> {
    let mut provided: Vec<(usize, &'a Expr)> = args.iter().enumerate().collect();
    let mut kw_order: Vec<(usize, usize)> = slots
        .iter()
        .enumerate()
        .filter_map(|(p, s)| match s {
            ArgSlot::Kw(j) => Some((*j, p)),
            _ => None,
        })
        .collect();
    kw_order.sort_by_key(|(j, _)| *j);
    for (j, p) in kw_order {
        provided.push((p, &kwargs[j].1));
    }
    provided
}

/// (card d8a1ed83, reshaped by kwargs v1 / card 8a7b7714) The uniform check-time
/// kwargs gate. Called with a NON-EMPTY `kwargs`; returns `Ok(())` only when the
/// call site is one pyrst MODELS keyword arguments for, otherwise an honest
/// `Error::Type`.
///
/// Admitted sites:
///   - a class CONSTRUCTOR `C(field=..)` — kwarg names are validated as field
///     names by the constructor arm;
///   - the free builtins `sorted(.., key=, reverse=)` and `min`/`max(.., key=)`
///     and the `list.sort(key=, reverse=)` method — their kwarg NAMES are
///     restricted to the modeled set here;
///   - (kwargs v1) a USER or MODULE function — flat (`fill(text, width=10)`) or
///     qualified (`textwrap.fill(text, width=10)`) — and a user METHOD on a
///     class instance (`g.greet(times=3)`): the keyword→positional mapping in
///     the corresponding call branch validates names/duplicates/missing and the
///     call lowers as a full positional call.
///
/// (W3-3) Extract the MODULE owner named by a qualified-call callee's receiver
/// `obj`, supporting both a single-component `X.f()` (`obj = Ident("X")` → owner
/// `"X"`) and a TWO-component dotted `a.b.f()` (`obj = Attr{Ident("a"), "b"}` →
/// owner `"a.b"`). Any other shape — a longer chain, a non-`Ident` base, or an
/// instance receiver — is `None`, so the caller falls through to the
/// instance-method / field paths unchanged. A returned owner is only ever treated
/// as a module when it is an actually-registered module id (checked against
/// `module_funcs` / `module_symbols`), so a genuine local `a.b.f()` method chain
/// (where `"a.b"` is not a module) never false-matches.
pub(crate) fn module_owner_of(obj: &Expr) -> Option<String> {
    match obj {
        Expr::Ident(a, _) => Some(a.clone()),
        Expr::Attr { obj: inner, name: b, .. } => match inner.as_ref() {
            Expr::Ident(a, _) => Some(format!("{}.{}", a, b)),
            _ => None,
        },
        _ => None,
    }
}

/// Still rejected (honest, check-time): keyword arguments on builtin stubs
/// (`abs(x=5)` — CPython builtins are positional-only), on `@staticmethod`
/// calls via the class name, on function-valued locals / lambdas, and on
/// builtin container methods.
fn reject_unmodeled_kwargs(
    callee: &Expr,
    kwargs: &[(String, Expr)],
    env: &mut FuncEnv,
    span: Span,
) -> Result<()> {
    match callee {
        Expr::Ident(name, _) => {
            // (a) Class constructor: field-name kwargs are validated downstream.
            if env.ctx.classes.contains_key(name.as_str()) {
                return Ok(());
            }
            // (b) Modeled free builtins — restrict the kwarg names too.
            let modeled: &[&str] = match name.as_str() {
                "sorted" => &["key", "reverse"],
                "min" | "max" => &["key"],
                _ => &[],
            };
            if !modeled.is_empty() {
                return reject_unknown_kwarg_names(kwargs, modeled, name, span);
            }
            // (kwargs v1) Builtin stubs stay positional-only: CPython's builtins
            // reject keyword arguments (`abs(x=5)` is a TypeError), and mapping
            // onto a stub's invented param name would accept what CPython
            // rejects.
            if env.ctx.builtin_funcs.contains(name.as_str()) {
                return Err(Error::Type {
                    span,
                    msg: format!("`{}` takes no keyword arguments", name),
                });
            }
            // (kwargs v1) A user or module function with a real signature: the
            // flat-call branch runs the keyword→positional mapping.
            if env.ctx.funcs.contains_key(name.as_str()) {
                return Ok(());
            }
        }
        Expr::Attr { obj, name, .. } => {
            // (kwargs v1) Qualified module function `X.f(kw=..)`: the qualified
            // module branch runs the mapping.
            if let Expr::Ident(base, _) = obj.as_ref() {
                if env
                    .ctx
                    .module_funcs
                    .get(base)
                    .is_some_and(|fns| fns.iter().any(|n| n == name))
                {
                    return Ok(());
                }
                // (card 0a70d607) `base` is a tracked module (it HAS a function
                // table) that does NOT define `name`: emit the SAME "module X has
                // no function Y" diagnostic the kwarg-FREE qualified-call arm
                // produces (the `_ =>` call arm's `module_funcs` lookup, below),
                // instead of falling through to `check_expr(obj)` at the end of
                // this fn — which type-checks the bare module ident and misreports
                // the MODULE NAME ITSELF as an "undefined name". Resolved via the
                // real module registry (`module_funcs`, the exact key the
                // kwarg-free path keys off), NOT a hardcoded stdlib name list, so
                // a kwarg-bearing call to a nonexistent module function (e.g.
                // `warnings.filterwarnings("ignore", message=...)`) yields the
                // identical, correct diagnostic as its kwarg-free form.
                if env.ctx.module_funcs.contains_key(base.as_str()) {
                    return Err(Error::Type {
                        span,
                        msg: format!("module `{}` has no function `{}`", base, name),
                    });
                }
                // `ClassName.method(kw=..)` (static-style call): stays
                // positional-only in v1 — fall through to the rejection below
                // WITHOUT type-checking the class name as a value expression.
                if env.ctx.classes.contains_key(base.as_str()) {
                    return Err(Error::Type {
                        span,
                        msg: format!(
                            "keyword arguments are not supported on `{}.{}` \
                             (a static-style class-name call); pass the argument \
                             positionally",
                            base, name
                        ),
                    });
                }
            }
            // (c) list.sort(key=, reverse=): only when the receiver is statically
            // a list. Re-deriving the receiver type here is idempotent for the
            // place receivers `.sort` takes and propagates any of its own errors.
            let obj_ty = check_expr(obj, env)?;
            if name == "sort" && matches!(obj_ty, Ty::List(_)) {
                return reject_unknown_kwarg_names(kwargs, &["key", "reverse"], "list.sort", span);
            }
            // (kwargs v1) A user METHOD on a class instance: the method branch
            // runs the mapping (inheritance-aware via get_method).
            if let Ty::Class(cls, _) = &obj_ty {
                if env.ctx.get_method(cls, name).is_some() {
                    return Ok(());
                }
            }
        }
        _ => {}
    }
    Err(Error::Type {
        span,
        msg: "keyword arguments are not supported at this call site; pass the \
              argument positionally"
            .into(),
    })
}

/// Reject the first entry of `kwargs` whose name is not in `modeled` (the keyword
/// names a builtin site actually supports), naming the site. Used by the kwargs
/// gate for sorted/min/max/list.sort so an unsupported keyword is a CHECK error.
fn reject_unknown_kwarg_names(
    kwargs: &[(String, Expr)],
    modeled: &[&str],
    site: &str,
    span: Span,
) -> Result<()> {
    if let Some((kw, _)) = kwargs.iter().find(|(k, _)| !modeled.contains(&k.as_str())) {
        let allowed = modeled
            .iter()
            .map(|m| format!("{}=", m))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::Type {
            span,
            msg: format!(
                "`{}` does not support the keyword argument `{}` (supported: {})",
                site, kw, allowed
            ),
        });
    }
    Ok(())
}

/// (W0-c, p20) The ELEMENT type of a set-algebra result (`&` `|` `^` `-`) when
/// the operands permit it. Python overloads these on sets for intersection /
/// union / symmetric-difference / difference; codegen lowers them to `HashSet`
/// operations that preserve the set type. Returns `Some(elem)` when both operands
/// are sets with a common element type, OR one is a set and the other is
/// `Unknown` — a not-yet-inferred operand (a `set()` literal types `set[Unknown]`;
/// a loop variable is `Unknown` in the divergence post-pass), yielding the known
/// element. Returns `None` when neither operand is a set or the element types
/// genuinely conflict, so the caller falls through to the honest bitwise /
/// arithmetic rules. Single source of truth shared by `check_expr` (the gate) and
/// `infer_expr_ty` (codegen's `type_of_expr`), so a NESTED set-op is typed as a
/// set on both sides and the two never drift.
pub(crate) fn set_binop_result_elem(l: &Ty, r: &Ty) -> Option<Ty> {
    match (l, r) {
        (Ty::Set(le), Ty::Set(re)) => {
            if **le == Ty::Unknown {
                Some((**re).clone())
            } else if **re == Ty::Unknown {
                Some((**le).clone())
            } else if le == re {
                Some((**le).clone())
            } else {
                None
            }
        }
        (Ty::Set(le), Ty::Unknown) => Some((**le).clone()),
        (Ty::Unknown, Ty::Set(re)) => Some((**re).clone()),
        _ => None,
    }
}

/// Whether `op` is one of the four set-algebra operators (`&` `|` `^` `-`).
pub(crate) fn is_set_algebra_op(op: BinOp) -> bool {
    matches!(op, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Sub)
}

/// (W5-a) Whether a binary op should route through the EXPLICIT bytes-operator
/// typing (`bytes_binop_ty`) instead of the loose generic path. `bytes` is a new
/// type and MUST NOT ride the generic `+`/`==` path that lets `list + str` /
/// `1 == "x"` pass `check` and fail `rustc` (probes PN1/PN2). For membership,
/// only `_ in bytes` (the container IS bytes) is bytes-specific; `bytes in
/// list/set/dict` is ordinary element membership handled by the normal path.
pub(crate) fn is_bytes_binop(op: BinOp, l: &Ty, r: &Ty) -> bool {
    if matches!(op, BinOp::In | BinOp::NotIn) {
        matches!(r, Ty::Bytes)
    } else {
        matches!(l, Ty::Bytes) || matches!(r, Ty::Bytes)
    }
}

/// (W5-a) Result type of a binary operator with a `bytes` operand — decided
/// EXPLICITLY, never via the loose generic path (design §E kills #2/#3). CPython
/// verdicts (python3-oracled, §G): `bytes+bytes`->bytes; `bytes*int` /
/// `int*bytes`->bytes; `bytes <cmp> bytes`->bool. Every mismatched pair is an
/// HONEST check error rather than a silent-wrong or a deferred `rustc` leak —
/// notably `bytes == str`, which CPython answers `False` but pyrst REJECTS (a
/// documented divergence: silently answering False is the trap). Membership on
/// `bytes` and all other operators are honest deferrals.
pub(crate) fn bytes_binop_ty(op: BinOp, l: &Ty, r: &Ty, span: Span) -> Result<Ty> {
    let err = |msg: String| Err(Error::Type { span, msg });
    // The non-bytes operand, for a natural error message.
    let other = if matches!(l, Ty::Bytes) { r } else { l };
    match op {
        BinOp::Add => match (l, r) {
            (Ty::Bytes, Ty::Bytes) => Ok(Ty::Bytes),
            _ => err(format!(
                "cannot concatenate `bytes` and `{}`: `bytes` joins only with `bytes` \
                 (decode/encode to bridge `str` and `bytes`)",
                other
            )),
        },
        BinOp::Mul => match (l, r) {
            (Ty::Bytes, Ty::Int) | (Ty::Int, Ty::Bytes) => Ok(Ty::Bytes),
            _ => err(format!(
                "cannot multiply `bytes` by `{}`: `bytes` repeats only by an `int`",
                other
            )),
        },
        BinOp::Eq | BinOp::Ne => match (l, r) {
            (Ty::Bytes, Ty::Bytes) => Ok(Ty::Bool),
            _ => err(format!(
                "`bytes` and `{}` are never equal in Python; decode/encode first. \
                 (CPython answers `False` here — pyrst rejects the comparison rather \
                 than silently returning False.)",
                other
            )),
        },
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match (l, r) {
            (Ty::Bytes, Ty::Bytes) => Ok(Ty::Bool),
            _ => err(format!(
                "cannot order `bytes` and `{}`: `<`/`<=`/`>`/`>=` compare `bytes` only \
                 with `bytes`",
                other
            )),
        },
        // (W5-b) Membership on a `bytes` container (`x in b`). `is_bytes_binop`
        // guarantees `r == Bytes`; the LHS decides the shape (python3-oracled §G):
        //   int in bytes    -> byte-value test (RANGE-CHECKED: 256/-1 raise a
        //                      catchable ValueError at runtime, CPython-faithful);
        //   bytes in bytes  -> subsequence search (`b'' in b` is True).
        // `str in bytes` stays an HONEST type error (CPython raises TypeError; the
        // old loose path would silently mis-handle it) — decode/encode to bridge.
        BinOp::In | BinOp::NotIn => match (l, r) {
            (Ty::Int, Ty::Bytes) | (Ty::Bytes, Ty::Bytes) => Ok(Ty::Bool),
            _ => err(format!(
                "unsupported membership `{} in bytes`: only `int in bytes` (byte-value \
                 test) and `bytes in bytes` (subsequence) are supported — `str in bytes` \
                 is a type error (decode/encode to bridge `str` and `bytes`)",
                other
            )),
        },
        _ => err(format!(
            "unsupported operand type(s) for `{}`: `bytes` (immutable bytes support \
             only `+ * == != < <= > >=` in W5-a)",
            binop_symbol(op)
        )),
    }
}

/// (W5-b) Whether `enc` is a string LITERAL naming utf-8 (case/`-`/`_`-insensitive):
/// `Some(true)` = utf-8 literal, `Some(false)` = a literal naming a DIFFERENT
/// (deferred) encoding, `None` = not a string literal (a variable / expression).
fn utf8_encoding_literal(enc: &Expr) -> Option<bool> {
    if let Expr::Str(s, _) = enc {
        let norm = s.trim().to_ascii_lowercase().replace('_', "-");
        Some(norm == "utf-8" || norm == "utf8")
    } else {
        None
    }
}

/// (W5-b) The literal-encoding name for an error message (`'ascii'`), or a generic
/// phrase when the encoding is not a string literal.
fn encoding_display(enc: &Expr) -> String {
    if let Expr::Str(s, _) = enc {
        format!("'{}'", s)
    } else {
        "that encoding".to_string()
    }
}

/// (W5-b) CHECK-level validation of a `bytes` method call's arity + argument types.
/// The iron rule (design §E): every parameter shape pyrst does NOT support is an
/// HONEST typeck error here — never a silent miscompile or a deferred `rustc` leak.
/// pyrst's method machinery exposes no optional/kwargs surface for these, so the
/// expressible subset ships and every extra-arg shape is a documented arity error,
/// matching str's existing pyrst ceiling (no start/end, no maxsplit, no count). A
/// KNOWN wrong-typed argument is rejected; `Unknown` stays permissive (codebase-
/// wide policy). Runs at `check` so each `fail_*` negative rejects before `rustc`.
fn check_bytes_method_call(
    name: &str,
    args: &[Expr],
    kwargs: &[(String, Expr)],
    env: &FuncEnv<'_>,
    span: Span,
) -> Result<()> {
    let err = |msg: String| Err(Error::Type { span, msg });
    if !kwargs.is_empty() {
        return err(format!("bytes.{}() does not take keyword arguments", name));
    }
    let argc = args.len();
    let ty = |i: usize| infer_expr_ty(&args[i], &env.locals, env.ctx);
    let want_bytes = |i: usize, label: &str| -> Result<()> {
        match ty(i) {
            Ty::Bytes | Ty::Unknown => Ok(()),
            Ty::Int => err(format!(
                "bytes.{}(): an int argument (byte value) is not supported — pass a \
                 `bytes` for {}",
                name, label
            )),
            other => err(format!(
                "bytes.{}(): {} must be `bytes`, found `{}`",
                name, label, other
            )),
        }
    };
    let want_int = |i: usize, label: &str| -> Result<()> {
        match ty(i) {
            Ty::Int | Ty::Unknown => Ok(()),
            other => err(format!(
                "bytes.{}(): {} must be an `int`, found `{}`",
                name, label, other
            )),
        }
    };
    match name {
        "hex" | "upper" | "lower" | "isdigit" | "isalpha" | "isalnum" | "isspace" => {
            if argc != 0 {
                return err(format!("bytes.{}() takes no arguments", name));
            }
        }
        "decode" => {
            if argc >= 2 {
                return err(
                    "bytes.decode(): the `errors=` argument is not supported — only \
                     strict utf-8 decoding is available (an invalid byte raises a \
                     catchable UnicodeDecodeError)"
                        .to_string(),
                );
            }
            if argc == 1 {
                match utf8_encoding_literal(&args[0]) {
                    Some(true) => {}
                    Some(false) => {
                        return err(format!(
                            "bytes.decode(): only 'utf-8' is supported ({} is deferred)",
                            encoding_display(&args[0])
                        ))
                    }
                    None => {
                        return err(
                            "bytes.decode(): the encoding must be the string literal \
                             'utf-8' (only utf-8 is supported)"
                                .to_string(),
                        )
                    }
                }
            }
        }
        "find" | "rfind" | "index" | "rindex" | "count" => {
            if argc != 1 {
                return err(format!(
                    "bytes.{}(sub) takes exactly one bytes argument — the optional \
                     start/end arguments are not supported",
                    name
                ));
            }
            want_bytes(0, "the search argument")?;
        }
        "startswith" | "endswith" => {
            if argc != 1 {
                return err(format!(
                    "bytes.{}(prefix) takes exactly one bytes argument — a tuple of \
                     prefixes and the optional start/end arguments are not supported",
                    name
                ));
            }
            if matches!(&args[0], Expr::Tuple(..)) || matches!(ty(0), Ty::Tuple(_)) {
                return err(format!(
                    "bytes.{}(): a tuple of prefixes is not supported — pass a single \
                     `bytes`",
                    name
                ));
            }
            want_bytes(0, "the prefix")?;
        }
        "replace" => {
            if argc != 2 {
                return err(
                    "bytes.replace(old, new) takes exactly two bytes arguments — the \
                     optional count is not supported"
                        .to_string(),
                );
            }
            want_bytes(0, "the search argument")?;
            want_bytes(1, "the replacement")?;
        }
        "split" | "rsplit" => {
            if argc >= 2 {
                return err(format!(
                    "bytes.{}(): the maxsplit argument is not supported — pass only a \
                     separator, or no argument for whitespace splitting",
                    name
                ));
            }
            if argc == 1 {
                want_bytes(0, "the separator")?;
            }
        }
        "join" => {
            if argc != 1 {
                return err(
                    "bytes.join(iterable) takes exactly one argument (a list of bytes)"
                        .to_string(),
                );
            }
            match ty(0) {
                Ty::Unknown => {}
                Ty::List(inner) if matches!(inner.as_ref(), Ty::Bytes | Ty::Unknown) => {}
                other => {
                    return err(format!(
                        "bytes.join(): the argument must be a `list[bytes]`, found `{}`",
                        other
                    ))
                }
            }
        }
        "strip" | "lstrip" | "rstrip" => {
            if argc >= 2 {
                return err(format!(
                    "bytes.{}() takes at most one argument (a set of bytes to strip)",
                    name
                ));
            }
            if argc == 1 {
                want_bytes(0, "the strip set")?;
            }
        }
        "ljust" | "rjust" | "center" => {
            if argc < 1 || argc > 2 {
                return err(format!(
                    "bytes.{}(width[, fillbyte]) takes one or two arguments",
                    name
                ));
            }
            want_int(0, "the width")?;
            if argc == 2 {
                want_bytes(1, "the fill byte")?;
            }
        }
        "zfill" => {
            if argc != 1 {
                return err(
                    "bytes.zfill(width) takes exactly one int argument".to_string(),
                );
            }
            want_int(0, "the width")?;
        }
        _ => {}
    }
    Ok(())
}

/// (W5-b) CHECK-level validation of `str.encode(...)`. utf-8 only (a `String`'s
/// bytes ARE UTF-8, so `encode` is `as_bytes().to_vec()`); a non-utf-8 / non-literal
/// encoding or an `errors=` arg is an honest error (design defers ascii/latin-1/
/// utf-16 to a follow-on).
fn check_str_encode_call(
    args: &[Expr],
    kwargs: &[(String, Expr)],
    span: Span,
) -> Result<()> {
    let err = |msg: String| Err(Error::Type { span, msg });
    if !kwargs.is_empty() {
        return err("str.encode() does not take keyword arguments".to_string());
    }
    if args.len() >= 2 {
        return err(
            "str.encode(): the `errors=` argument is not supported — only utf-8 \
             encoding is available"
                .to_string(),
        );
    }
    if args.len() == 1 {
        match utf8_encoding_literal(&args[0]) {
            Some(true) => {}
            Some(false) => {
                return err(format!(
                    "str.encode(): only 'utf-8' is supported ({} is deferred)",
                    encoding_display(&args[0])
                ))
            }
            None => {
                return err(
                    "str.encode(): the encoding must be the string literal 'utf-8' \
                     (only utf-8 is supported)"
                        .to_string(),
                )
            }
        }
    }
    Ok(())
}

/// Pure inference oracle — the single source of truth for expression types.
///
/// A side-effect-free port of codegen's `type_of_expr` (codegen.rs:264-548) with
/// the SAME contract: it never errors and never mutates; on any ambiguity it
/// falls to `Ty::Unknown` (preserving the `types_compatible` `(Unknown, _) => true`
/// escape hatch). Inputs are exactly what both call sites already hold — typeck's
/// `env.locals`/`env.ctx` and codegen's `self.locals`/`self.ctx` are identical
/// types — so E.2 can route both through here.
///
/// It bakes in the CORRECT side of every documented divergence
/// (docs/design/inference-oracle.md §A.4): D1 str-index → Str; D3 abs(x) → arg
/// type; D4 sum(xs) → element type; D5 `**` → Float; D6 dict literal folds ALL
/// pairs; D7 attribute access is inheritance-aware (`get_all_fields`).
pub fn infer_expr_ty(expr: &Expr, locals: &HashMap<String, Ty>, ctx: &TyCtx) -> Ty {
    match expr {
        Expr::Float(..) => Ty::Float,
        Expr::Int(..) => Ty::Int,
        Expr::Bool(..) => Ty::Bool,
        Expr::Str(..) | Expr::FStr(..) => Ty::Str,
        Expr::Bytes(..) => Ty::Bytes,
        Expr::None_(_) => Ty::NoneVal,
        Expr::IfExp { body, orelse, .. } => {
            // Both branches unify in typeck; prefer the concrete one.
            let b = infer_expr_ty(body, locals, ctx);
            if b == Ty::Unknown {
                infer_expr_ty(orelse, locals, ctx)
            } else {
                b
            }
        }
        Expr::Ident(n, _) => locals
            .get(n.as_str())
            .or_else(|| ctx.vars.get(n.as_str()))
            .cloned()
            // A bare top-level function name in a value position infers to its
            // first-class `Ty::Func` type (`g = inc` -> g: Callable[[int],int]).
            // Locals/vars shadow it (checked first). Call sites never reach this
            // arm for the callee — `Expr::Call` resolves the name itself.
            .or_else(|| ctx.funcs.get(n.as_str()).map(func_sig_to_ty))
            .unwrap_or(Ty::Unknown),
        Expr::UnOp { op: UnOp::Neg, expr, .. } => infer_expr_ty(expr, locals, ctx),
        Expr::UnOp { op: UnOp::Not, .. } => Ty::Bool,
        Expr::UnOp { op: UnOp::BitNot, .. } => Ty::Int,
        Expr::BinOp { lhs, op, rhs, .. } => {
            let l = infer_expr_ty(lhs, locals, ctx);
            let r = infer_expr_ty(rhs, locals, ctx);
            // (W0-c, p20) Set algebra `& | ^ -` -> the set type, mirroring
            // `check_expr`. Codegen's `type_of_expr` delegates here, so a NESTED
            // set-op operand (`(a - b) | (c & a)`) is seen as a set and the outer
            // op fires the `HashSet` lowering instead of an invalid owned-`HashSet`
            // bitwise op.
            if is_set_algebra_op(*op) {
                if let Some(elem) = set_binop_result_elem(&l, &r) {
                    return Ty::Set(Box::new(elem));
                }
            }
            // (W5-a) bytes operators: `bytes+bytes` / `bytes*int` -> bytes.
            // `check_expr` has already rejected every mismatched form, so only
            // valid shapes reach this oracle; comparisons fall through to the Bool
            // arm below. Without this, `b1 + b2` would take the `else Int` fallback
            // and mis-drive codegen's display/type decisions.
            if (l == Ty::Bytes || r == Ty::Bytes) && matches!(op, BinOp::Add | BinOp::Mul) {
                return Ty::Bytes;
            }
            match op {
                // D5: Python `**` always yields a float (split out of the
                // int-biased arithmetic arm below — codegen's bug).
                BinOp::Pow => Ty::Float,
                BinOp::Div => Ty::Float,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod | BinOp::FloorDiv => {
                    // Operator overloading: a class lhs uses its dunder return type.
                    if let Ty::Class(cls, _) = &l {
                        let dunder = match op {
                            BinOp::Add => Some("__add__"),
                            BinOp::Sub => Some("__sub__"),
                            BinOp::Mul => Some("__mul__"),
                            _ => None,
                        };
                        if let Some(ret) =
                            dunder.and_then(|d| ctx.get_method(cls, d)).map(|s| s.ret.clone())
                        {
                            return ret;
                        }
                    }
                    // String concatenation for Add.
                    if *op == BinOp::Add && (l == Ty::Str || r == Ty::Str) {
                        Ty::Str
                    } else if l == Ty::Float || r == Ty::Float {
                        Ty::Float
                    } else if matches!(l, Ty::TypeVar(_)) {
                        // (W1.5, card 71cbd940) Arithmetic over a GENERIC
                        // operand yields the type variable, not the int bias:
                        // `acc + xs[i]` with `acc: T` is `T` (the inferred
                        // Add bound guarantees T op T -> T), so a
                        // guard-narrowed generic reassignment (`acc = acc +
                        // xs[i]` under `if f is None`) is a same-type
                        // rebinding instead of a bogus "T before the block,
                        // int inside" divergence.
                        l
                    } else if matches!(r, Ty::TypeVar(_)) {
                        r
                    } else {
                        Ty::Int
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                | BinOp::And | BinOp::Or | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => {
                    Ty::Bool
                }
                _ => Ty::Unknown,
            }
        }
        Expr::Attr { obj, name, .. } => {
            // Qualified MODULE CONSTANT `X.CONST` for a REAL imported module:
            // when X is a tracked module and CONST is one of its module-level
            // constants, the access has the const's declared type. GENERALIZES
            // the former hardcoded `math.pi` typing — `math` is now a real
            // embedded module whose consts are tracked here.
            if let Expr::Ident(modname, _) = obj.as_ref() {
                // (W3-1) OWNER-FIRST: the const's type comes from `modname`'s own
                // per-module table (flat `module_consts` fallback for synthetic
                // ctxs). Both are module-keyed, so this never diverges.
                if let Some(ty) = ctx.resolve_module_const(modname, name) {
                    return ty.clone();
                }
                // (W4-a) A qualified MUTABLE-GLOBAL read `m.x` (a container global is
                // absent from `module_consts`) types as its declared `Ty`, so a
                // print site formats it correctly (a list as `[..]`, not `{}`).
                // Guarded on `m` not being a local (a class-typed local's field).
                if !locals.contains_key(modname) {
                    if let Some(ty) = ctx.mutable_global_ty(Some(modname), name) {
                        return ty.clone();
                    }
                }
            }
            // (card 03eb4e2c) A class-NAME receiver `ClassName.FIELD` — e.g. an
            // enum-member const `Color.RED` — types as the field's declared type.
            // The bare class-name `Ident` otherwise infers to `Unknown` (it is not
            // a local), which would drop bool/float formatting at a print site
            // (`print(Flags.ON)` -> "true" instead of "True"). Instance receivers
            // are handled by the `infer_expr_ty(obj)` path below.
            if let Expr::Ident(cn, _) = obj.as_ref() {
                if ctx.classes.contains_key(cn.as_str()) {
                    // (enabler-fix-1 #3c) Resolve an INHERITED class-constant access
                    // (`Sub.KIND` where `KIND` is declared on a base) via the base
                    // chain. The old own-fields-only lookup inferred `Unknown`, so an
                    // inherited const dropped its bool/float print formatting and
                    // typed as nothing usable.
                    let all = ctx.get_all_fields(cn.as_str());
                    if let Some(f) = all.iter().find(|f| f.name == *name) {
                        return Ty::from_type_expr(&f.ty, f.span).unwrap_or(Ty::Unknown);
                    }
                }
            }
            // D7: resolve the field inheritance-aware via `get_all_fields`
            // (codegen reads `c.fields` directly and misses inherited fields).
            let recv = infer_expr_ty(obj, locals, ctx);
            if let Ty::Class(cls, _) = &recv {
                let all_fields = ctx.get_all_fields(cls.as_str());
                if let Some(f) = all_fields.iter().find(|f| f.name == *name) {
                    // Generics v2: scope the field annotation with the class's
                    // type params (`value: T` -> `TypeVar(T)`) and substitute the
                    // receiver instance's args (`Box[int]` -> `{T -> int}`) so the
                    // oracle types `b.value` concretely (drives codegen var typing
                    // and print formatting). Non-generic class => empty subst =>
                    // identical to the old unscoped result.
                    let tps = ctx.classes.get(cls.as_str()).map(|c| c.type_params.as_slice()).unwrap_or(&[]);
                    let field_ty = Ty::from_type_expr_scoped(&f.ty, f.span, tps).unwrap_or(Ty::Unknown);
                    return subst_class_member(&field_ty, &recv, ctx);
                }
            }
            Ty::Unknown
        }
        Expr::Call { callee, args, .. } => {
            // (card 49170944) `str.maketrans(x, y)` is a STATIC str call that builds
            // an int->int code-point translation table (`dict[int, int]`) for
            // `s.translate(table)`. Recognized structurally (callee is the attr
            // `maketrans` on the bare name `str`) so it never depends on how `str`
            // itself types. The honest subset is the 2-arg equal-length form; the
            // 3-arg delete form is out of scope (would need dict[int, Optional[int]]).
            if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                if name == "maketrans"
                    && matches!(obj.as_ref(), Expr::Ident(sn, _) if sn == "str")
                {
                    return Ty::Dict(Box::new(Ty::Int), Box::new(Ty::Int));
                }
                // (W5-b) `bytes.fromhex(s)` STATIC constructor -> bytes. Recognized
                // structurally (mirrors str.maketrans) so it never depends on how
                // the bare name `bytes` types.
                if name == "fromhex"
                    && matches!(obj.as_ref(), Expr::Ident(bn, _) if bn == "bytes")
                {
                    return Ty::Bytes;
                }
            }
            if let Expr::Ident(n, _) = callee.as_ref() {
                match n.as_str() {
                    "float" => Ty::Float,
                    "abs" => {
                        // D3: abs returns the same type as its argument.
                        if let Some(arg) = args.first() {
                            infer_expr_ty(arg, locals, ctx)
                        } else {
                            Ty::Unknown
                        }
                    }
                    "sum" => {
                        // D4: sum() returns the type of the iterable's elements.
                        let base = if let Some(arg) = args.first() {
                            match infer_expr_ty(arg, locals, ctx) {
                                Ty::List(inner) => *inner,
                                Ty::Set(inner) => *inner,
                                // (LAZY-GEN V1-c) A generator source sums to its
                                // element type, same as a list/set.
                                Ty::Iterator(inner) => *inner,
                                _ => Ty::Int, // Default to int for other iterables.
                            }
                        } else {
                            Ty::Int
                        };
                        // (card aabf4ada) The 2-arg form `sum(iterable, start)` folds
                        // the start seed in; a FLOAT start promotes an int-element sum
                        // to float (CPython: `sum([1,2,3],1.0)` -> 7.0). Report Float
                        // here so the print-formatter DISPLAYS `7.0`, agreeing with the
                        // codegen sum arm's own promotion (codegen/exprs.rs). A float
                        // element with an int start is already Float via `base`.
                        if args.len() >= 2 {
                            let start_ty = infer_expr_ty(&args[1], locals, ctx);
                            if matches!(base, Ty::Float) || matches!(start_ty, Ty::Float) {
                                Ty::Float
                            } else {
                                base
                            }
                        } else {
                            base
                        }
                    }
                    "int" | "len" | "ord" | "round" | "pow" => Ty::Int,
                    "bool" | "any" | "all" => Ty::Bool,
                    "str" | "chr" | "input" => Ty::Str,
                    // (W5-a) `bytes()` / `bytes(n)` / `bytes(list[int])` / `bytes(b)`.
                    "bytes" => Ty::Bytes,
                    "map" if args.len() == 2 => {
                        // map(f, iterable) -> List(applied return type of f).
                        // Only a List iterable yields a concrete List result;
                        // Set/Str/unknown stay Unknown (permissive).
                        match infer_expr_ty(&args[1], locals, ctx) {
                            Ty::List(e) => {
                                let body_ty = lambda_applied_ty(&args[0], &e, locals, ctx);
                                Ty::List(Box::new(body_ty))
                            }
                            _ => Ty::Unknown,
                        }
                    }
                    "filter" if args.len() == 2 => {
                        // filter(pred, iterable) -> the iterable's list type
                        // unchanged. Only List yields a concrete type.
                        match infer_expr_ty(&args[1], locals, ctx) {
                            Ty::List(e) => Ty::List(e),
                            _ => Ty::Unknown,
                        }
                    }
                    // (CARD fd65dc99) `enumerate`/`zip` previously had no arm
                    // here at all, so they fell through to the generic `n => {..}`
                    // branch below, which resolves through `ctx.funcs.get(n)` —
                    // the hardcoded builtin FuncSig registrations for
                    // "enumerate"/"zip" (typeck/types.rs) both declare `ret:
                    // Ty::Unknown` unconditionally. That made this ORACLE always
                    // report `Unknown` for a zip/enumerate call — starving
                    // `bind_comp_targets` (both here and in codegen's own
                    // `type_of_expr`-backed copy) of the tuple element types
                    // needed to destructure `for a, b in zip(..)` /
                    // `for i, v in enumerate(..)` targets inside a comprehension,
                    // and starving the comprehension `chain` codegen of the fact
                    // that the source is a list at all. Mirror `check_expr`'s
                    // already-correct zip/enumerate typing (used for `Stmt::For`)
                    // here too, so the comprehension path sees the same element
                    // types. Purely additive/widening — an unsupported source
                    // still degrades to `Unknown`, never narrows an existing
                    // `types_compatible` result.
                    "enumerate" if !args.is_empty() => {
                        match infer_expr_ty(&args[0], locals, ctx) {
                            Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => {
                                Ty::List(Box::new(Ty::Tuple(vec![Ty::Int, *inner])))
                            }
                            Ty::Str => Ty::List(Box::new(Ty::Tuple(vec![Ty::Int, Ty::Str]))),
                            _ => Ty::Unknown,
                        }
                    }
                    "zip" => {
                        let mut elem_tys: Vec<Ty> = Vec::with_capacity(args.len());
                        let mut any_unknown = false;
                        for a in args {
                            match infer_expr_ty(a, locals, ctx) {
                                Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => {
                                    elem_tys.push(*inner)
                                }
                                Ty::Str => elem_tys.push(Ty::Str),
                                _ => any_unknown = true,
                            }
                        }
                        if any_unknown || elem_tys.is_empty() {
                            Ty::Unknown
                        } else {
                            Ty::List(Box::new(Ty::Tuple(elem_tys)))
                        }
                    }
                    "sorted" | "list" | "reversed" => {
                        // These return a list; preserve the element type.
                        // Over a dict they operate on its KEYS (Python semantics),
                        // so the result element type is the dict's key type.
                        // (LAZY-GEN V1-c) `sorted(gen)`/`list(gen)` materialize a
                        // generator into `list[T]`, same element type as a
                        // list/set source (`reversed(gen)` is a V1-d MATERIALIZE
                        // error at the codegen/typeck-error layer; this arm is
                        // the pure, non-erroring inference oracle and just
                        // reports the type it WOULD be).
                        if let Some(arg) = args.first() {
                            match infer_expr_ty(arg, locals, ctx) {
                                Ty::List(e) | Ty::Set(e) | Ty::Iterator(e) => Ty::List(e),
                                Ty::Dict(k, _) => Ty::List(k),
                                Ty::Str => Ty::List(Box::new(Ty::Str)),
                                // (W5-a) `sorted`/`list`/`reversed` over `bytes`
                                // yield a `list[int]` — each byte widens to an int
                                // (CPython: `list(b'AB') == [65, 66]`). This must
                                // agree with codegen widening the value to `Vec<i64>`;
                                // otherwise the emitted `Vec<u8>` renders via the
                                // bytes `PyRepr` impl as `b'...'` (silent-wrong).
                                Ty::Bytes => Ty::List(Box::new(Ty::Int)),
                                _ => Ty::List(Box::new(Ty::Unknown)),
                            }
                        } else {
                            Ty::List(Box::new(Ty::Unknown))
                        }
                    }
                    // (CARD bd2bd472) `min`/`max` previously had NO arm here, so a
                    // bare `min(xs)`/`max(xs)` call fell through to the generic
                    // `n => {..}` branch below, which resolves via `ctx.funcs.get`
                    // — the hardcoded builtin `FuncSig` for "min"/"max" declares
                    // `ret: Ty::Unknown` unconditionally (2-arg scalar shape,
                    // `typeck/types.rs`). That starved this ORACLE of the element
                    // type for the single-iterable form, so `print(min([4.0, 2.0]))`
                    // inferred `Unknown` for the call and skipped `__py_fmt_float`
                    // formatting, printing "2" instead of Python's "2.0". Mirror
                    // `check_expr`'s already-correct single-iterable min/max typing
                    // (element type of the List/Set/Iterator argument) here too — a
                    // `key=` kwarg does not change the RESULT's type (the winning
                    // element is still a member of the source, key= only picks
                    // which one), so this covers both the bare and `key=` forms.
                    // The 2-arg scalar shape (`min(a, b)`) is NOT specially typed by
                    // `check_expr` either (falls through to the same Unknown
                    // `FuncSig`), so it is intentionally left to the `n => {..}`
                    // fallback below — purely additive/widening, never narrows an
                    // existing `types_compatible` result.
                    "min" | "max" if args.len() == 1 => {
                        match infer_expr_ty(&args[0], locals, ctx) {
                            Ty::List(elem) | Ty::Set(elem) | Ty::Iterator(elem) => *elem,
                            _ => Ty::Unknown,
                        }
                    }
                    // (card b557b9c1) The n-ary scalar form `min(a, b, c, ...)`
                    // returns one of its (homogeneous) positional args, so the
                    // result type is the first arg's type. The old path fell through
                    // to the hardcoded `min`/`max` FuncSig (`ret: Ty::Unknown`),
                    // which starved the print-formatter of the float type and made
                    // `max(1.0, 2.0, 3.0)` (and even the 2-arg `max(1.0, 2.0)`)
                    // display as `2`/`3` instead of `2.0`/`3.0`. Mirrors the codegen
                    // n-ary fold, which yields exactly this type.
                    "min" | "max" if args.len() >= 2 => {
                        infer_expr_ty(&args[0], locals, ctx)
                    }
                    n => {
                        // A class constructor yields an instance; a named user
                        // function yields its declared return type; a func-VALUED
                        // local/param/var (`f: Callable[[int],int]`) called as
                        // `f(x)` yields the function value's return type.
                        if ctx.classes.contains_key(n) {
                            // Generics v2: for a generic class, INFER its type args
                            // from the constructor argument types (`Box(5)` ->
                            // `Box[int]`), matching the checking path so codegen
                            // sees the same concrete instance type. A non-generic
                            // class yields the legacy `Ty::Class(n, [])`.
                            let arg_tys: Vec<Ty> = args.iter()
                                .map(|a| infer_expr_ty(a, locals, ctx))
                                .collect();
                            infer_class_instantiation(n, &arg_tys, ctx)
                        } else if let Some(sig) = ctx.funcs.get(n) {
                            // Generics v1: for a generic call, infer the concrete
                            // result by unifying the declared param types against
                            // the argument types — so `first([10, 20])` infers
                            // `int` (not `T`) and `swap(5, "x")` infers
                            // `tuple[str, int]`. This is what lets codegen pick the
                            // right print-formatting and variable types for a
                            // generic call's RESULT (the result is always concrete
                            // after substitution). Shared with the qualified arm via
                            // `oracle_generic_call_ret`, which never errors.
                            let arg_tys: Vec<Ty> = args.iter()
                                .map(|a| infer_expr_ty(a, locals, ctx))
                                .collect();
                            oracle_generic_call_ret(n, sig, &arg_tys, ctx)
                        } else if let Some(Ty::Func(_, ret)) =
                            locals.get(n).or_else(|| ctx.vars.get(n))
                        {
                            (**ret).clone()
                        } else {
                            Ty::Unknown
                        }
                    }
                }
            } else if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                // Qualified module call `X.f(args)` for a REAL imported module
                // (card 81db88e0): when X is a tracked module and f is one of its
                // functions, the call's type is f's declared return type — exactly
                // as if `f(args)` were called by its flat name. `math` is now a
                // real embedded module (`lib/math.pyrs`), so `math.sqrt(x)` flows
                // through here (its @extern `sqrt` lives in `module_funcs`); the
                // former hardcoded math return-typing arm is gone.
                if let Some(modname) = module_owner_of(obj) {
                    // (W3-3) `modname` is the single- OR two-component module owner
                    // (`os` for `os.f()`, `os.path` for `os.path.f()`); both resolve
                    // identically once registered under their dotted id. A non-module
                    // `a.b` (local method chain) misses `module_funcs` and falls to
                    // the instance path below.
                    if ctx.module_funcs.get(&modname).is_some_and(|fns| fns.iter().any(|n| n == name)) {
                        // Generics v1: a QUALIFIED generic stdlib call
                        // (`heapq.heappop(h)`) substitutes its inferred type args so
                        // codegen sees a CONCRETE result type — the same handling as
                        // the flat form, via the shared `oracle_generic_call_ret`. A
                        // non-generic module fn returns its declared type unchanged.
                        // (W3-1) OWNER-FIRST: resolve the signature against
                        // module `modname`'s OWN per-module table (guarded above by
                        // its `module_funcs` membership), not the flat table. For a
                        // real program this is the module's own sig; it falls back
                        // to the flat table only for synthetic single-module ctxs.
                        // `oracle_generic_call_ret` still keys generics by the bare
                        // `name` against the (flat, global) generic maps — unchanged.
                        return match ctx.resolve_module_func(&modname, name) {
                            Some(sig) => {
                                let arg_tys: Vec<Ty> = args.iter()
                                    .map(|a| infer_expr_ty(a, locals, ctx))
                                    .collect();
                                oracle_generic_call_ret(name, sig, &arg_tys, ctx)
                            }
                            None => Ty::Unknown,
                        };
                    }
                }
                // Class methods use their declared return; builtin receivers
                // (str/list/set/dict/file) delegate to the shared
                // `builtin_method_ret` so the two never drift and chained calls
                // resolve.
                let recv = infer_expr_ty(obj, locals, ctx);
                // (W5-h) A lib handle (`Ty::Handle(n)`) resolves its methods via the
                // same `ctx.classes` path as a value class (`method_lookup_class`), so
                // `p.search(text)` on a `re.Pattern` handle types like a class method.
                if let Some(cls) = method_lookup_class(&recv, ctx) {
                    // Generics v2: substitute the receiver instance's type args
                    // into the method's (type-var-bearing) return, so a generic
                    // method call types concretely for codegen (`b.get(): int`).
                    if let Some(s) = ctx.get_method(cls, name) {
                        subst_class_member(&s.ret, &recv, ctx)
                    } else {
                        // Not a real method: a CALLABLE FIELD invoked as `obj.f(args)`
                        // (`self.op(x)` where `op: Callable[[int], int]`). Its call type
                        // is the field-`Ty::Func`'s RETURN — the SAME resolution
                        // `check_expr` does for this shape (its value-call fallback,
                        // ~exprs.rs:3197) and codegen's `(obj.f)(args)` lowering. Without
                        // this the oracle returned `Unknown`, so codegen's receiver-typed
                        // dispatch gate was bypassed: a `bytes`-returning field-call
                        // chained with a shared method name (`box.transform(b).strip()`)
                        // fell through to str's name-matched arm -> rustc E0599 (check
                        // passed, build died). Inheritance-aware via `get_all_fields`, and
                        // the same `subst_class_member` the method path uses (a generic
                        // field `f: Callable[[T], T]` resolves `T`). Mirrors the bare-attr
                        // field resolution above (Expr::Attr arm).
                        ctx.get_all_fields(cls)
                            .iter()
                            .find(|f| f.name == *name)
                            .and_then(|f| {
                                let tps = ctx
                                    .classes
                                    .get(cls)
                                    .map(|c| c.type_params.as_slice())
                                    .unwrap_or(&[]);
                                let field_ty = Ty::from_type_expr_scoped(&f.ty, f.span, tps)
                                    .unwrap_or(Ty::Unknown);
                                match subst_class_member(&field_ty, &recv, ctx) {
                                    Ty::Func(_, ret) => Some(*ret),
                                    _ => None,
                                }
                            })
                            .unwrap_or(Ty::Unknown)
                    }
                } else if let Some(t) = dict_get_ret(&recv, name, args.len()) {
                    // dict.get is arg-count-aware: get(k) -> Optional[V],
                    // get(k, default) -> V (see dict_get_ret).
                    t
                } else {
                    builtin_method_ret(&recv, name)
                }
            } else {
                // Calling a function VALUE whose callee is an arbitrary expression
                // (a lambda, an indexed slot `ops["double"]`, an attr, ...). Infer
                // the callee's type and, if it is a `Ty::Func`, surface its return
                // type so `ops["double"](7)` and `(make_adder(5))(10)` are typed.
                match infer_expr_ty(callee, locals, ctx) {
                    Ty::Func(_, ret) => *ret,
                    _ => Ty::Unknown,
                }
            }
        }
        Expr::List(elems, _) => {
            // Unify all element types (not first-element-wins) so a mixed numeric
            // literal like `[1, 2.0]` is typed `List(Float)`.
            Ty::List(Box::new(infer_list_elem_ty(elems, locals, ctx)))
        }
        Expr::Tuple(elems, _) => {
            // Mirror check_expr's tuple arm so this codegen-side oracle knows an
            // INLINE tuple literal's element types. Without it a bare `(1, "x")`
            // typed Unknown, so `print`/`str`/`repr` fell to the Display fallback
            // (a `(f64, String)` build wall) instead of routing through PyRepr.
            Ty::Tuple(elems.iter().map(|e| infer_expr_ty(e, locals, ctx)).collect())
        }
        Expr::Dict(pairs, _) => {
            // D6: fold ALL pairs, unifying key types and value types
            // independently (codegen uses the first pair only). On a both-concrete
            // conflict, degrade THAT position to Unknown — never error (the pure
            // contract; check_expr rejects, this oracle stays permissive).
            if pairs.is_empty() {
                Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown))
            } else {
                let mut k_ty = infer_expr_ty(&pairs[0].0, locals, ctx);
                let mut v_ty = infer_expr_ty(&pairs[0].1, locals, ctx);
                for (k, v) in &pairs[1..] {
                    let kt = infer_expr_ty(k, locals, ctx);
                    let vt = infer_expr_ty(v, locals, ctx);
                    // widen_numeric=false: float dict keys are non-hashable and
                    // dict values have no codegen cast, matching check_expr.
                    k_ty = unify_elem_types(k_ty.clone(), kt, false, ctx).unwrap_or(Ty::Unknown);
                    v_ty = unify_elem_types(v_ty.clone(), vt, false, ctx).unwrap_or(Ty::Unknown);
                }
                Ty::Dict(Box::new(k_ty), Box::new(v_ty))
            }
        }
        Expr::Set(elems, _) => {
            // Unify all element types (mirrors the list case).
            Ty::Set(Box::new(infer_list_elem_ty(elems, locals, ctx)))
        }
        Expr::ListComp { elt, targets, iter, .. } => {
            // Infer element type from the iterable and element expression.
            let iter_ty = infer_expr_ty(iter, locals, ctx);
            let elem_iter_ty = match &iter_ty {
                // LAZY-GEN V1-a: a generator source == a list source, element-wise.
                Ty::List(inner) | Ty::Iterator(inner) | Ty::Set(inner) => Some(inner.as_ref().clone()),
                _ => None,
            };
            // The single-variable oracle only applies to single-target comps;
            // for tuple-unpacking targets we fall through to the iterable-elem
            // fallback (the authoritative element type comes from `check_expr`).
            if let (Some(elem_iter_type), [target]) = (&elem_iter_ty, targets.as_slice()) {
                let inferred =
                    infer_comp_elt_type_with_var(elt, elem_iter_type, target, ctx);
                if inferred != Ty::Unknown {
                    return Ty::List(Box::new(inferred));
                }
            }
            // Fallback: use the iterable's element type.
            match iter_ty {
                // LAZY-GEN V1-a: a comprehension over a generator yields a list of
                // its element type, exactly like a comprehension over a list.
                Ty::List(inner) | Ty::Iterator(inner) => Ty::List(inner),
                Ty::Set(inner) => Ty::List(inner),
                _ => Ty::List(Box::new(Ty::Unknown)),
            }
        }
        Expr::SetComp { elt, targets: _, iter, .. } => {
            let iter_ty = infer_expr_ty(iter, locals, ctx);
            if let Ty::List(ref inner) | Ty::Iterator(ref inner) | Ty::Set(ref inner) = iter_ty {
                match elt.as_ref() {
                    Expr::Attr { name, .. } => {
                        if let Ty::Class(cls, _) = inner.as_ref() {
                            if let Some(c) = ctx.classes.get(cls.as_str()) {
                                if let Some(f) = c.fields.iter().find(|f| f.name == *name) {
                                    // Generics v2: scope the field with the class's
                                    // type params (`value: T` -> `TypeVar(T)`) then
                                    // substitute the element instance's args
                                    // (`Box[int]` -> `{T -> int}`), so a comp over a
                                    // generic-class element infers the concrete
                                    // field type. Non-generic class => no-op.
                                    if let Ok(ty) = Ty::from_type_expr_scoped(&f.ty, f.span, &c.type_params) {
                                        return Ty::Set(Box::new(subst_class_member(&ty, inner, ctx)));
                                    }
                                }
                            }
                        }
                    }
                    Expr::Call { callee, .. } => {
                        if let Expr::Attr { name, .. } = callee.as_ref() {
                            if let Ty::Class(cls, _) = inner.as_ref() {
                                if let Some(method_sig) = ctx.get_method(cls.as_str(), name) {
                                    // Substitute the element instance's type args
                                    // into the (scoped) method return.
                                    return Ty::Set(Box::new(subst_class_member(&method_sig.ret, inner, ctx)));
                                }
                            }
                        }
                    }
                    _ => {}
                }
                Ty::Set(inner.clone())
            } else {
                Ty::Set(Box::new(Ty::Unknown))
            }
        }
        Expr::DictComp { key, val, targets: _, iter, .. } => {
            let iter_ty = infer_expr_ty(iter, locals, ctx);
            let field_ty = |e: &Expr| -> Ty {
                if let Expr::Attr { name, .. } = e {
                    if let Ty::Class(ref cls, _) = iter_ty {
                        if let Some(c) = ctx.classes.get(cls.as_str()) {
                            if let Some(f) = c.fields.iter().find(|f| f.name == *name) {
                                // Generics v2: scope + substitute the field type
                                // against the generic-class instance (mirrors the
                                // non-comprehension field-access path).
                                let ty = Ty::from_type_expr_scoped(&f.ty, f.span, &c.type_params).unwrap_or(Ty::Unknown);
                                return subst_class_member(&ty, &iter_ty, ctx);
                            }
                        }
                    }
                }
                Ty::Unknown
            };
            Ty::Dict(Box::new(field_ty(key)), Box::new(field_ty(val)))
        }
        Expr::Index { obj, idx, .. } => {
            // D1: a Str receiver yields Str (codegen lacks this arm). Dict[k] is
            // the value type; List[i] is the element type.
            match infer_expr_ty(obj, locals, ctx) {
                Ty::Dict(_, val_ty) => *val_ty,
                Ty::List(elem_ty) => *elem_ty,
                Ty::Str => Ty::Str,
                // (W5-a) `b[i]` -> int (a `u8` widened to `i64`) — OPPOSITE to
                // `str`, whose index yields a 1-char `str`.
                Ty::Bytes => Ty::Int,
                // (CARD a40d603e) A Tuple receiver with a LITERAL integer index
                // selects that field's type — mirrors codegen's `emit_expr` Index
                // arm, which lowers `t[N]` (N a literal) to Rust field access
                // (`.N`). Without this arm, a CHAINED tuple index (`t[0][1]`)
                // left the inner `t[0]` untyped (`Unknown`) here, so the outer
                // index's receiver type was lost and codegen's tuple-vs-list
                // dispatch fell through to the list-indexing path for a
                // non-Vec tuple field — a raw rustc E0599/E0308 leak, not an
                // honest pyrst error. A non-literal index has no single result
                // type for a fixed-size Rust tuple; stay permissively `Unknown`
                // here (the pure oracle never errors) — `check_expr`'s own
                // Index arm is the one that REJECTS it honestly.
                Ty::Tuple(elems) => match idx.as_ref() {
                    Expr::Int(n, _) => elems.get(*n as usize).cloned().unwrap_or(Ty::Unknown),
                    _ => Ty::Unknown,
                },
                _ => Ty::Unknown,
            }
        }
        Expr::Slice { obj, .. } => {
            // A slice yields the SAME container kind: str -> str (substring),
            // list[T] -> list[T] (sublist). Without this arm a slice fell through
            // to Unknown, so an inline `int(s[a:b])` / `float(s[a:b])` took the
            // numeric-cast path and miscompiled (`String as i64`) — the oracle had
            // an Index arm but no Slice arm.
            match infer_expr_ty(obj, locals, ctx) {
                Ty::Str => Ty::Str,
                list_ty @ Ty::List(_) => list_ty,
                // (W5-a) `b[i:j]` -> bytes (a sub-`Vec<u8>`), like a list slice.
                Ty::Bytes => Ty::Bytes,
                _ => Ty::Unknown,
            }
        }
        // A lambda is a first-class function value. Its parameters carry no
        // annotation in pyrst, so each argument type is `Unknown`; the return
        // type is the body's type with the parameter names bound to `Unknown`.
        // The result `Callable[[unknown, ...], body_ty]` is permissive — it fills
        // any `Callable` slot of matching arity (see `types_compatible`).
        Expr::Lambda { params, body, .. } => {
            let mut inner = locals.clone();
            for (name, _) in params {
                inner.insert(name.clone(), Ty::Unknown);
            }
            let ret = infer_expr_ty(body, &inner, ctx);
            Ty::Func(vec![Ty::Unknown; params.len()], Box::new(ret))
        }
    }
}

/// Unified element type of a list/set literal's elements, for `infer_expr_ty`.
/// Folds every element's type with `unify_oracle_ty` (not first-element-wins) so
/// a mixed numeric literal like `[1, 2.0]` is typed `Float`. Empty -> `Unknown`.
/// Pure port of codegen's `list_elem_ty`/`unify_ty`.
pub(crate) fn infer_list_elem_ty(elems: &[Expr], locals: &HashMap<String, Ty>, ctx: &TyCtx) -> Ty {
    let mut iter = elems.iter();
    match iter.next() {
        None => Ty::Unknown,
        Some(first) => iter.fold(infer_expr_ty(first, locals, ctx), |acc, e| {
            unify_oracle_ty(acc, infer_expr_ty(e, locals, ctx))
        }),
    }
}

/// Structural element-type unification for collection literals (pure port of
/// codegen's `unify_ty`). Int/Float widen to Float; nested collections recurse;
/// `Unknown` is absorbed; otherwise the left (concrete) side wins.
pub(crate) fn unify_oracle_ty(a: Ty, b: Ty) -> Ty {
    match (a, b) {
        (Ty::Unknown, x) | (x, Ty::Unknown) => x,
        (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => Ty::Float,
        (Ty::Dict(k1, v1), Ty::Dict(k2, v2)) => Ty::Dict(
            Box::new(unify_oracle_ty(*k1, *k2)),
            Box::new(unify_oracle_ty(*v1, *v2)),
        ),
        (Ty::List(e1), Ty::List(e2)) => Ty::List(Box::new(unify_oracle_ty(*e1, *e2))),
        (Ty::Set(e1), Ty::Set(e2)) => Ty::Set(Box::new(unify_oracle_ty(*e1, *e2))),
        (a, _) => a,
    }
}

/// Infer the applied return type of a `map`'s callable over an element of type
/// `elem`, for `infer_expr_ty`'s `map` arm. Pure port of codegen's
/// `lambda_applied_ty` -> `type_of_expr_bound`.
pub(crate) fn lambda_applied_ty(callable: &Expr, elem: &Ty, locals: &HashMap<String, Ty>, ctx: &TyCtx) -> Ty {
    if let Expr::Lambda { params, body, .. } = callable {
        if let Some((param, _)) = params.first() {
            return infer_expr_ty_bound(body, param, elem, locals, ctx);
        }
    }
    Ty::Unknown
}

/// Like `infer_expr_ty`, but the single identifier `param` resolves to `elem`
/// (the bound lambda parameter). Recurses through the compound forms that appear
/// in map lambda bodies; for everything else it delegates to `infer_expr_ty`.
/// Pure port of codegen's `type_of_expr_bound`.
pub(crate) fn infer_expr_ty_bound(
    e: &Expr,
    param: &str,
    elem: &Ty,
    locals: &HashMap<String, Ty>,
    ctx: &TyCtx,
) -> Ty {
    match e {
        Expr::Ident(n, _) if n == param => elem.clone(),
        Expr::UnOp { op: UnOp::Neg, expr, .. } => {
            infer_expr_ty_bound(expr, param, elem, locals, ctx)
        }
        Expr::IfExp { body, orelse, .. } => {
            let b = infer_expr_ty_bound(body, param, elem, locals, ctx);
            if b == Ty::Unknown {
                infer_expr_ty_bound(orelse, param, elem, locals, ctx)
            } else {
                b
            }
        }
        Expr::BinOp { lhs, op, rhs, .. } => {
            let l = infer_expr_ty_bound(lhs, param, elem, locals, ctx);
            let r = infer_expr_ty_bound(rhs, param, elem, locals, ctx);
            match op {
                BinOp::Div | BinOp::Pow => Ty::Float,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod | BinOp::FloorDiv => {
                    if *op == BinOp::Add && (l == Ty::Str || r == Ty::Str) {
                        Ty::Str
                    } else if l == Ty::Float || r == Ty::Float {
                        Ty::Float
                    } else if l == Ty::Int || r == Ty::Int {
                        Ty::Int
                    } else {
                        Ty::Unknown
                    }
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                | BinOp::And | BinOp::Or | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => {
                    Ty::Bool
                }
                _ => Ty::Unknown,
            }
        }
        // Other forms do not depend on `param` for their result type — delegate.
        _ => infer_expr_ty(e, locals, ctx),
    }
}

/// Bind a comprehension's loop target(s) into `locals` from the iterable's
/// element type. A single target gets the full element type; multiple targets
/// (tuple-unpacking, e.g. `for k, v in d.items()`) destructure a matching-arity
/// `Ty::Tuple` into each, falling back to `Unknown`. Mirrors the `Stmt::For`
/// binding in `check_stmt`.
pub(crate) fn bind_comp_targets(targets: &[String], elem_ty: Ty, locals: &mut HashMap<String, Ty>) {
    if targets.len() == 1 {
        locals.insert(targets[0].clone(), elem_ty);
    } else {
        let elem_tys = match &elem_ty {
            Ty::Tuple(tys) if tys.len() == targets.len() => tys.clone(),
            _ => vec![Ty::Unknown; targets.len()],
        };
        for (i, target) in targets.iter().enumerate() {
            locals.insert(target.clone(), elem_tys.get(i).cloned().unwrap_or(Ty::Unknown));
        }
    }
}

/// Infer a comprehension element expression's type given the loop variable's
/// type and name, for `infer_expr_ty`'s comprehension arms. Pure port of
/// codegen's `infer_comp_elt_type_with_var`.
pub(crate) fn infer_comp_elt_type_with_var(
    elt: &Expr,
    loop_var_ty: &Ty,
    loop_var_name: &str,
    ctx: &TyCtx,
) -> Ty {
    match elt {
        // [i.field for i in items] or [i.a.b for i in items]
        Expr::Attr { obj, name, .. } => {
            let obj_ty = if let Expr::Ident(var_name, _) = obj.as_ref() {
                if var_name == loop_var_name {
                    loop_var_ty.clone()
                } else {
                    Ty::Unknown
                }
            } else {
                infer_comp_elt_type_with_var(obj, loop_var_ty, loop_var_name, ctx)
            };
            if let Ty::Class(cls, _) = &obj_ty {
                if let Some(c) = ctx.classes.get(cls.as_str()) {
                    if let Some(f) = c.fields.iter().find(|f| f.name == *name) {
                        // Generics v2: scope the field with the class's type params
                        // and substitute the loop-var instance's args, so
                        // `[item.value for item in boxes]` over `list[Box[int]]`
                        // infers `int` (not the bare `T`). Non-generic class: no-op.
                        let ty = Ty::from_type_expr_scoped(&f.ty, f.span, &c.type_params).unwrap_or(Ty::Unknown);
                        return subst_class_member(&ty, &obj_ty, ctx);
                    }
                }
            }
            Ty::Unknown
        }
        // [i.method() for i in items]
        Expr::Call { callee, .. } => {
            if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                let obj_ty = if let Expr::Ident(var_name, _) = obj.as_ref() {
                    if var_name == loop_var_name {
                        loop_var_ty.clone()
                    } else {
                        Ty::Unknown
                    }
                } else {
                    infer_comp_elt_type_with_var(obj, loop_var_ty, loop_var_name, ctx)
                };
                if let Ty::Class(cls, _) = &obj_ty {
                    if let Some(method_sig) = ctx.get_method(cls.as_str(), name) {
                        // Substitute the loop-var instance's type args into the
                        // (scoped) method return.
                        return subst_class_member(&method_sig.ret, &obj_ty, ctx);
                    }
                }
            }
            Ty::Unknown
        }
        // [i.a + i.b for i in items] - infer from BinOp.
        Expr::BinOp { lhs, op, rhs, .. } => {
            let left_ty = infer_comp_elt_type_with_var(lhs, loop_var_ty, loop_var_name, ctx);
            let right_ty = infer_comp_elt_type_with_var(rhs, loop_var_ty, loop_var_name, ctx);
            match (left_ty, right_ty) {
                (Ty::Float, _) | (_, Ty::Float) => Ty::Float,
                (Ty::Int, Ty::Int) => {
                    if *op == BinOp::Div || *op == BinOp::Pow {
                        Ty::Float
                    } else {
                        Ty::Int
                    }
                }
                _ => Ty::Unknown,
            }
        }
        _ => Ty::Unknown,
    }
}

/// Resolve a lambda parameter's annotation to a `Ty`. Lambda params are
/// untyped in the surface syntax; the parser records the placeholder
/// `TypeExpr::Named("Any")`. That sentinel must resolve to `Ty::Unknown` (not
/// the bogus `Ty::Class("Any", vec![])` the generic resolver would produce), so a
/// param-dependent lambda body stays permissive instead of spuriously typing as
/// a nonexistent class.
pub(crate) fn lambda_param_ty(param_ty: &TypeExpr) -> Ty {
    if let TypeExpr::Named(n) = param_ty {
        if n == "Any" {
            return Ty::Unknown;
        }
    }
    // Inference-only fallback: a lambda param annotation has no carried span and
    // any error is swallowed to `Unknown`, so a dummy span never reaches a user.
    Ty::from_type_expr(param_ty, Span::DUMMY).unwrap_or(Ty::Unknown)
}

/// Infer the return type of a callable applied to a single element of type
/// `elem`, for the `map`/`filter` special cases.
///
/// When `callable` is an inline `lambda` with at least one parameter, its first
/// param is bound to `elem` (or `Unknown` when the iterable element type is
/// unknown) in a temporary env, the body is type-checked, and its inferred type
/// is returned as `Some(body_ty)`. For any other callable (a named function,
/// `def`-bound variable, etc.) or a parameterless lambda, the expression is
/// still type-checked for its own errors and `None` is returned so the caller
/// stays permissive (yields `Ty::Unknown`). This never narrows
/// `types_compatible`; it only widens positive inference.
pub(crate) fn lambda_ret_with_elem(
    callable: &Expr,
    elem: Option<&Ty>,
    env: &mut FuncEnv,
) -> Result<Option<Ty>> {
    if let Expr::Lambda { params, body, .. } = callable {
        // (W5-g, H2) A lambda (map/filter/sorted-key or bare) cannot capture a
        // move-only handle — clone-on-capture is non-`Clone`. Reject before the body
        // check, covering the zero-param lambda that falls through below too.
        reject_lambda_handle_capture(params, body, env)?;
        if !params.is_empty() {
            let mut lambda_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: Ty::Unknown,
                used_vars: env.used_vars.clone(),
                params: std::collections::HashSet::new(),
                reassigned_params: std::collections::HashSet::new(),
                returned_params: std::collections::HashSet::new(),
                by_ref_params: std::collections::HashSet::new(),
                // A lambda body is a single expression and can never contain a
                // `yield` statement, so it is never a generator.
                is_generator: false,
                // A lambda introduces its own (untyped) parameters; the enclosing
                // function's type variables are not in scope for the lambda's own
                // params, so this stays empty.
                type_params: std::collections::HashSet::new(),
                // A lambda cannot contain a `global` statement (it has no
                // statement body), so it declares no module globals.
                globals_declared: std::collections::HashSet::new(),
                // A lambda body is a single expression (no reassignment/guard),
                // so the narrow-tracking map is never consulted — start empty.
                narrowed: std::collections::HashMap::new(),
                // (W4-a) Inherit the enclosing function's owning module.
                module_id: env.module_id.clone(),
                // (W5-g) Inherit handle liveness so a handle moved before this
                // nested expression scope is still detected if read inside it. Move
                // MARKING is a statement-position concern (never happens in an
                // expression scope), so `loop_handles` is inherited but never pushed.
                moved: env.moved.clone(),
                loop_handles: env.loop_handles.clone(),
            };
            // Bind every param: the first to the iterable element type, the
            // rest to their declared type or Unknown (map/filter pass a single
            // element, so only the first param is meaningfully constrained).
            for (i, (param_name, param_ty)) in params.iter().enumerate() {
                let ty = if i == 0 {
                    elem.cloned().unwrap_or(Ty::Unknown)
                } else {
                    lambda_param_ty(param_ty)
                };
                lambda_env.locals.insert(param_name.clone(), ty);
            }
            let body_ty = check_expr(body, &mut lambda_env)?;
            return Ok(Some(body_ty));
        }
    }
    // Non-lambda callable (or zero-param lambda): still check it for its own
    // errors, but we cannot infer an applied return type here.
    check_expr(callable, env)?;
    Ok(None)
}

// (card 87bd8eb4) The former `range_step_nonpositive_literal` gate is GONE:
// a 3-arg range now lowers through the direction-aware `__py_range_step` builder
// (codegen), so a negative literal step DESCENDS and a zero step raises a
// catchable runtime `ValueError` — both valid CPython, no longer a check error.

pub(crate) fn check_expr(e: &Expr, env: &mut FuncEnv) -> Result<Ty> {
    Ok(match e {
        Expr::Int(_, _) => Ty::Int,
        Expr::Float(_, _) => Ty::Float,
        Expr::Str(_, _) => Ty::Str,
        Expr::Bytes(_, _) => Ty::Bytes,
        Expr::FStr(parts, fstr_span) => {
            // Visit each interpolation: an f-string FORMATS each `{expr}` via the
            // value's `Display`. Generics v2: a bare type variable (`f"{x}"` where
            // `x: T`) is now LEGAL — it infers a `Display` bound on `T` (collected
            // by `infer_func_typevar_bounds`, emitted in the generic clause), so
            // the generated `format!("{}", x)` is well-typed. Checking the
            // sub-exprs still surfaces any of THEIR own errors.
            for part in parts {
                if let FStrPart::Interp(expr, _) = part {
                    let ity = check_expr(expr, env)?;
                    // (LAZY-GEN V1-d) Formatting a generator would print an opaque
                    // handle, not its contents (like `str(g)`/`print(g)`). Reject
                    // with the materialize fix (docs/design/lazy-generators.md §D.2).
                    if matches!(ity, Ty::Iterator(_)) {
                        // Interpolation exprs may carry a DUMMY span (the parser
                        // does not always thread one), so caret the whole f-string.
                        return Err(iterator_materialize_error(
                            "has no string form (it would show an opaque generator handle)",
                            "f\"{list(g)}\"", *fstr_span));
                    }
                    // (W5-g) A handle has no `Display`/`PyRepr`, so it cannot be
                    // interpolated into an f-string — honest error, not rustc E0277.
                    reject_handle_op(&ity, "interpolate into an f-string", *fstr_span)?;
                }
            }
            Ty::Str
        }
        Expr::Bool(_, _) => Ty::Bool,
        Expr::Tuple(elems, span) => {
            let tys = elems.iter().map(|e| check_expr(e, env)).collect::<Result<Vec<_>>>()?;
            // (W5-g) A handle cannot be stored in a tuple (non-clonable, move-only).
            for t in &tys { reject_handle_op(t, "store in a tuple", *span)?; }
            Ty::Tuple(tys)
        }
        Expr::IfExp { test, body, orelse, span } => {
            // (Z4, card 2b37b965) A bare `Optional` ternary condition leaks a
            // rustc error at build; reject it at check for parity with `if`.
            let test_ty = check_expr(test, env)?;
            reject_optional_truthiness(&test_ty, test.span())?;
            let bt = check_expr(body, env)?;
            let ot = check_expr(orelse, env)?;
            // Both arms must agree; the more concrete side wins so a branch like
            // `[]` (List(Unknown)) unifies with `[1, 2, 3]` (List(Int)).
            unify_branch_types(bt.clone(), ot.clone(), env.ctx).ok_or_else(|| Error::Type {
                span: *span,
                msg: format!(
                    "conditional expression branches have incompatible types: `{}` vs `{}`",
                    bt, ot
                ),
            })?
        }
        Expr::ListComp { elt, targets, iter, cond, .. } => {
            let iter_ty = check_expr(iter, env)?;
            // Generics v1: a comprehension iterates its source, so iterating a bare
            // type variable needs an `IntoIterator` bound (E0599 otherwise).
            // (The element-type match below falls through to a concrete type for
            // an opaque iterable, hiding the gap from `check` — so reject here,
            // mirroring the `Stmt::For` gate.)
            reject_typevar_op(&iter_ty, "iterate over", iter.span())?;
            let elem_ty = match &iter_ty {
                // LAZY-GEN V1-a: a generator source (`Ty::Iterator`) yields the
                // same element type as a `list[T]` — treated identically.
                Ty::List(inner) | Ty::Iterator(inner) => *inner.clone(),
                Ty::Set(inner) => *inner.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Int, // ranges and unknown iterables -> Int
            };
            // Create a new scope with the loop variable(s) bound
            let mut inner_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: env.ret_ty.clone(),
                used_vars: env.used_vars.clone(),
                params: env.params.clone(),
                reassigned_params: env.reassigned_params.clone(),
                returned_params: env.returned_params.clone(),
                by_ref_params: env.by_ref_params.clone(),
                // A comprehension runs inside the enclosing function scope, so it
                // inherits any `global` declarations (its body may read a global).
                globals_declared: env.globals_declared.clone(),
                // A comprehension lives inside the current function; inherit its
                // generator status so the bare-return / yield rules stay coherent
                // (a `yield` cannot appear inside a comprehension expression, but
                // propagating keeps the env honest).
                is_generator: env.is_generator,
                // Inherit the enclosing generic function's type parameters so a
                // comprehension body inside a generic function keeps the
                // ops-on-`T` restriction.
                type_params: env.type_params.clone(),
                // Inherit the enclosing scope's active narrows so the comprehension
                // body sees the same narrowed view (it is a single expression and
                // never reassigns, so the map is only ever read here).
                narrowed: env.narrowed.clone(),
                // (W4-a) A comprehension runs in the enclosing function scope —
                // inherit its owning module so any global it reads resolves per-owner.
                module_id: env.module_id.clone(),
                // (W5-g) Inherit handle liveness so a handle moved before this
                // nested expression scope is still detected if read inside it. Move
                // MARKING is a statement-position concern (never happens in an
                // expression scope), so `loop_handles` is inherited but never pushed.
                moved: env.moved.clone(),
                loop_handles: env.loop_handles.clone(),
            };
            bind_comp_targets(targets, elem_ty, &mut inner_env.locals);
            if let Some(c) = cond { let ct = check_expr(c, &mut inner_env)?; reject_optional_truthiness(&ct, c.span())?; }
            let elt_ty = check_expr(elt, &mut inner_env)?;
            // (W5-g, H1) A comprehension-BUILT container of handles is the same
            // hole as a container LITERAL of handles: `[open(p) for p in paths]`
            // would emit an un-clonable `Vec<PyFile>`. The literal-List arm rejects
            // this; the comprehension arm must too. Honest error naming the kind.
            reject_handle_op(&elt_ty, "store in a list", elt.span())?;
            Ty::List(Box::new(elt_ty))
        }
        Expr::SetComp { elt, targets, iter, cond, span } => {
            let iter_ty = check_expr(iter, env)?;
            // Generics v1: a comprehension iterates its source, so iterating a bare
            // type variable needs an `IntoIterator` bound (E0599 otherwise).
            // (The element-type match below falls through to a concrete type for
            // an opaque iterable, hiding the gap from `check` — so reject here,
            // mirroring the `Stmt::For` gate.)
            reject_typevar_op(&iter_ty, "iterate over", iter.span())?;
            let elem_ty = match &iter_ty {
                // LAZY-GEN V1-a: a generator source (`Ty::Iterator`) yields the
                // same element type as a `list[T]` — treated identically.
                Ty::List(inner) | Ty::Iterator(inner) => *inner.clone(),
                Ty::Set(inner) => *inner.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Int,
            };
            let mut inner_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: env.ret_ty.clone(),
                used_vars: env.used_vars.clone(),
                params: env.params.clone(),
                reassigned_params: env.reassigned_params.clone(),
                returned_params: env.returned_params.clone(),
                by_ref_params: env.by_ref_params.clone(),
                // A comprehension runs inside the enclosing function scope, so it
                // inherits any `global` declarations (its body may read a global).
                globals_declared: env.globals_declared.clone(),
                // A comprehension lives inside the current function; inherit its
                // generator status so the bare-return / yield rules stay coherent
                // (a `yield` cannot appear inside a comprehension expression, but
                // propagating keeps the env honest).
                is_generator: env.is_generator,
                // Inherit the enclosing generic function's type parameters so a
                // comprehension body inside a generic function keeps the
                // ops-on-`T` restriction.
                type_params: env.type_params.clone(),
                // Inherit the enclosing scope's active narrows so the comprehension
                // body sees the same narrowed view (it is a single expression and
                // never reassigns, so the map is only ever read here).
                narrowed: env.narrowed.clone(),
                // (W4-a) A comprehension runs in the enclosing function scope —
                // inherit its owning module so any global it reads resolves per-owner.
                module_id: env.module_id.clone(),
                // (W5-g) Inherit handle liveness so a handle moved before this
                // nested expression scope is still detected if read inside it. Move
                // MARKING is a statement-position concern (never happens in an
                // expression scope), so `loop_handles` is inherited but never pushed.
                moved: env.moved.clone(),
                loop_handles: env.loop_handles.clone(),
            };
            bind_comp_targets(targets, elem_ty, &mut inner_env.locals);
            if let Some(c) = cond { let ct = check_expr(c, &mut inner_env)?; reject_optional_truthiness(&ct, c.span())?; }
            let elt_ty = check_expr(elt, &mut inner_env)?;
            // (W5-g, H1) A comprehension-built SET of handles (`{open(p) for ..}`)
            // is the same un-clonable-container hole as the set literal — reject it.
            reject_handle_op(&elt_ty, "store in a set", elt.span())?;
            // Same hashability rule as set literals: a Float element produces
            // the uncompilable `HashSet<f64>`, so reject it here too.
            require_hashable(&elt_ty, *span, "set element")?;
            Ty::Set(Box::new(elt_ty))
        }
        Expr::DictComp { key, val, targets, iter, cond, span } => {
            let iter_ty = check_expr(iter, env)?;
            // Generics v1: a comprehension iterates its source, so iterating a bare
            // type variable needs an `IntoIterator` bound (E0599 otherwise).
            // (The element-type match below falls through to a concrete type for
            // an opaque iterable, hiding the gap from `check` — so reject here,
            // mirroring the `Stmt::For` gate.)
            reject_typevar_op(&iter_ty, "iterate over", iter.span())?;
            let elem_ty = match &iter_ty {
                // LAZY-GEN V1-a: a generator source (`Ty::Iterator`) yields the
                // same element type as a `list[T]` — treated identically.
                Ty::List(inner) | Ty::Iterator(inner) => *inner.clone(),
                Ty::Set(inner) => *inner.clone(),
                Ty::Str => Ty::Str, // iterating a string yields 1-char strings
                _ => Ty::Int,
            };
            let mut inner_env = FuncEnv {
                ctx: env.ctx,
                locals: env.locals.clone(),
                ret_ty: env.ret_ty.clone(),
                used_vars: env.used_vars.clone(),
                params: env.params.clone(),
                reassigned_params: env.reassigned_params.clone(),
                returned_params: env.returned_params.clone(),
                by_ref_params: env.by_ref_params.clone(),
                // A comprehension runs inside the enclosing function scope, so it
                // inherits any `global` declarations (its body may read a global).
                globals_declared: env.globals_declared.clone(),
                // A comprehension lives inside the current function; inherit its
                // generator status so the bare-return / yield rules stay coherent
                // (a `yield` cannot appear inside a comprehension expression, but
                // propagating keeps the env honest).
                is_generator: env.is_generator,
                // Inherit the enclosing generic function's type parameters so a
                // comprehension body inside a generic function keeps the
                // ops-on-`T` restriction.
                type_params: env.type_params.clone(),
                // Inherit the enclosing scope's active narrows so the comprehension
                // body sees the same narrowed view (it is a single expression and
                // never reassigns, so the map is only ever read here).
                narrowed: env.narrowed.clone(),
                // (W4-a) A comprehension runs in the enclosing function scope —
                // inherit its owning module so any global it reads resolves per-owner.
                module_id: env.module_id.clone(),
                // (W5-g) Inherit handle liveness so a handle moved before this
                // nested expression scope is still detected if read inside it. Move
                // MARKING is a statement-position concern (never happens in an
                // expression scope), so `loop_handles` is inherited but never pushed.
                moved: env.moved.clone(),
                loop_handles: env.loop_handles.clone(),
            };
            bind_comp_targets(targets, elem_ty, &mut inner_env.locals);
            if let Some(c) = cond { let ct = check_expr(c, &mut inner_env)?; reject_optional_truthiness(&ct, c.span())?; }
            let key_ty = check_expr(key, &mut inner_env)?;
            let val_ty = check_expr(val, &mut inner_env)?;
            // (W5-g, H1) A comprehension-built DICT storing handles as keys or
            // values (`{p: open(p) for ..}`) is the same un-clonable-container hole
            // as the dict literal — reject either position.
            reject_handle_op(&key_ty, "use as a dict key", key.span())?;
            reject_handle_op(&val_ty, "store in a dict", val.span())?;
            // Same hashability rule as dict literals: a Float KEY produces the
            // uncompilable `HashMap<f64, _>`. Values may be Float.
            require_hashable(&key_ty, *span, "dict key")?;
            Ty::Dict(Box::new(key_ty), Box::new(val_ty))
        }
        Expr::None_(_) => Ty::NoneVal,
        Expr::List(elems, span) => {
            let elem_ty = if elems.is_empty() {
                Ty::Unknown
            } else {
                // Unify all element types: every element is checked (for its own
                // errors), and their types are folded together. A genuinely
                // heterogeneous literal (two both-concrete, non-Unknown,
                // non-numeric-mixable types) is rejected here instead of being
                // silently typed as `List(first-element-type)` and deferred to
                // rustc. Int/Float mixing and Unknown elements stay permissive.
                let mut acc = check_expr(&elems[0], env)?;
                for e in &elems[1..] {
                    let next = check_expr(e, env)?;
                    // Lists may hold floats, so int/float elements widen to Float.
                    acc = unify_elem_types(acc.clone(), next.clone(), true, env.ctx).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "list elements have incompatible types: {} vs {}",
                            acc, next
                        ),
                    })?;
                }
                acc
            };
            // (W5-g) A move-only handle cannot be stored in a container (a
            // `Vec<PyFile>` is not clonable, breaking value semantics). Deferred to a
            // later handle iteration; honest error now, naming the kind.
            reject_handle_op(&elem_ty, "store in a list", *span)?;
            Ty::List(Box::new(elem_ty))
        }
        Expr::Set(elems, span) => {
            let elem_ty = if elems.is_empty() {
                Ty::Unknown
            } else {
                // Same element-type unification as list literals above, but
                // WITHOUT Int/Float widening: a set's element type must be
                // hashable and `set[float]` (`HashSet<f64>`) is not representable
                // in Rust, so `{1, 2.0}` is rejected rather than typed Set(Float).
                let mut acc = check_expr(&elems[0], env)?;
                for e in &elems[1..] {
                    let next = check_expr(e, env)?;
                    acc = unify_elem_types(acc.clone(), next.clone(), false, env.ctx).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "set elements have incompatible types: {} vs {}",
                            acc, next
                        ),
                    })?;
                }
                acc
            };
            // A pure-float set literal (`{1.0, 2.0}`) folds to Set(Float), which
            // codegen would emit as the uncompilable `HashSet<f64>`. Reject it
            // here so typeck and codegen agree. (`{1, 2.0}` is already rejected
            // by the widen_numeric=false fold above; this closes the all-float
            // case.) Unknown element types (`set()`) stay permissive.
            reject_handle_op(&elem_ty, "store in a set", *span)?;
            require_hashable(&elem_ty, *span, "set element")?;
            Ty::Set(Box::new(elem_ty))
        }
        Expr::Dict(pairs, span) => {
            if pairs.is_empty() {
                Ty::Dict(Box::new(Ty::Unknown), Box::new(Ty::Unknown))
            } else {
                // Unify all key types and all value types independently via a
                // left-to-right fold. Genuinely heterogeneous dicts (two
                // both-concrete incompatible key or value types) are rejected
                // here instead of silently using first-pair types and deferring
                // the error to rustc. widen_numeric=false for both: float dict
                // keys are non-hashable (HashMap<f64,_> doesn't compile), and
                // there is no codegen value-cast for dict values, so mixed
                // Int/Float values would also fail at rustc.
                let mut k_ty = check_expr(&pairs[0].0, env)?;
                let mut v_ty = check_expr(&pairs[0].1, env)?;
                for (k, v) in &pairs[1..] {
                    let kt = check_expr(k, env)?;
                    let vt = check_expr(v, env)?;
                    k_ty = unify_elem_types(k_ty.clone(), kt.clone(), false, env.ctx).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "dict keys have incompatible types: {} vs {}",
                            k_ty, kt
                        ),
                    })?;
                    v_ty = unify_elem_types(v_ty.clone(), vt.clone(), false, env.ctx).ok_or_else(|| Error::Type {
                        span: *span,
                        msg: format!(
                            "dict values have incompatible types: {} vs {}",
                            v_ty, vt
                        ),
                    })?;
                }
                // A float-keyed dict literal (`{1.0: "a"}`) folds to Dict(Float, _),
                // which codegen would emit as the uncompilable `HashMap<f64, _>`.
                // Reject the KEY only — float VALUES are fine (`HashMap<_, f64>`
                // compiles), so v_ty is left untouched.
                reject_handle_op(&k_ty, "use as a dict key", *span)?;
                reject_handle_op(&v_ty, "store in a dict", *span)?;
                require_hashable(&k_ty, *span, "dict key")?;
                Ty::Dict(Box::new(k_ty), Box::new(v_ty))
            }
        }
        Expr::Ident(name, span) => {
            // Track variable usage for dead code detection
            if env.locals.contains_key(name.as_str()) {
                env.used_vars.insert(name.clone());
            }
            // (W5-g) Backstop use-after-move check: reading a handle that a prior
            // statement moved is an honest error. `check_handle_flow` is the primary
            // (eval-order, per-statement) pass; this catches any CROSS-statement read
            // that reaches `check_expr` first, so a moved handle can never slip
            // through to a rustc E0382/E0599. Naming the binding + move site.
            if let Some(move_span) = env.moved.get(name).copied() {
                let kind = env.locals.get(name).and_then(|t| t.handle_name())
                    .unwrap_or("handle").to_string();
                return Err(Error::Type {
                    span: *span,
                    msg: format!(
                        "handle `{name}` (`{kind}`) was already moved (consumed) at line \
                         {ln}:{col} and cannot be used again — a move-only handle is consumed \
                         when it is passed to a function, returned, or reassigned; open or \
                         create a fresh handle instead of reusing `{name}`",
                        ln = move_span.line, col = move_span.col,
                    ),
                });
            }
            // A bare STDLIB module name (used opaquely as the base of `os.x` before
            // the Attr/Call arm recurses here) types as `Ty::Unknown`. `dataclasses`
            // is the decorator-only skip-list module (never a real loaded module), so
            // it stays unconditionally opaque as before.
            //
            // (W3-3) IMPORT-AWARENESS: the listed stdlib names are opaque ONLY when
            // the module is actually IMPORTED. An UNIMPORTED stdlib name used
            // qualified — `os.getcwd()` / `os.sep` with only `import os.path` (or no
            // `os` import at all) — is an honest "not imported" CHECK error, not a
            // silent `Ty::Unknown` that passes `check` and then dies at rustc as an
            // undefined `os` (E0425). This is the death of the `import os.path`
            // silent-truncation symptom (design P3b): `import os.path` no longer
            // loads `os`, so a bare `os.getcwd()` beside it must be rejected, and
            // rejected HERE (at check) rather than deferred to `build`. A LOCAL
            // variable that happens to share a stdlib name (`string = "hi"`) is a
            // normal local and resolves via `env.lookup` below (the `!locals` guard),
            // so no real program regresses.
            //
            // (card 0a70d607) This is an EXPLICIT, curated list — NOT the full
            // embedded-stdlib registry (`crate::stdlib::lookup`). Generalizing to
            // every embedded module name was tried and REVERTED: several module
            // names are legitimately used as an imported CLASS
            // (`from datetime import time`) or a plain local (`platform =
            // sys.platform`) in real programs, and intercepting those bare names
            // here wrongly rejected working goldens (parity_datetime via `time`,
            // parity_sys via `platform`). `warnings`/`logging` are added to the
            // list — they ship a module surface and are never used as class/local
            // names — so a bare unimported `warnings`/`logging` gets the honest
            // "not imported" message like `os`/`math`. The kwarg-bearing "module X
            // has no function Y" diagnostic these modules actually needed is handled
            // structurally in `reject_unmodeled_kwargs`, independent of this list.
            if name == "dataclasses" {
                Ty::Unknown
            } else if matches!(name.as_str(), "math" | "sys" | "os" | "json" | "re" | "collections" | "itertools" | "warnings" | "logging")
                && !env.locals.contains_key(name.as_str())
            {
                let imported = env.ctx.module_funcs.contains_key(name.as_str())
                    || env.ctx.module_consts.contains_key(name.as_str())
                    || env.ctx.module_symbols.contains_key(name.as_str());
                if imported {
                    Ty::Unknown
                } else {
                    return Err(Error::Type {
                        span: *span,
                        msg: format!(
                            "module `{name}` is used but not imported; add `import {name}`"
                        ),
                    });
                }
            } else {
                env.lookup(name).ok_or_else(|| Error::Type {
                    span: *span,
                    msg: format!("undefined name `{}`", name),
                })?
            }
        }
        Expr::Call { callee, args, kwargs, span } => {
            // (card 49170944) `str.maketrans(x, y)` STATIC call -> `dict[int, int]`
            // translation table. Check the two string args, reject kwargs / wrong
            // arity, then type as the dict. Matched structurally so it is
            // independent of how the bare name `str` types.
            if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                if name == "maketrans"
                    && matches!(obj.as_ref(), Expr::Ident(sn, _) if sn == "str")
                {
                    if !kwargs.is_empty() {
                        return Err(Error::Type {
                            span: *span,
                            msg: "str.maketrans does not take keyword arguments".into(),
                        });
                    }
                    if args.len() != 2 {
                        return Err(Error::Type {
                            span: *span,
                            msg: "str.maketrans(x, y): only the 2-argument (equal-length \
                                  from/to) form is supported".into(),
                        });
                    }
                    for a in args {
                        let at = check_expr(a, env)?;
                        if !matches!(at, Ty::Str) {
                            return Err(Error::Type {
                                span: *span,
                                msg: "str.maketrans(x, y) requires str arguments".into(),
                            });
                        }
                    }
                    return Ok(Ty::Dict(Box::new(Ty::Int), Box::new(Ty::Int)));
                }
                // (W5-b) `bytes.fromhex(s)` STATIC constructor -> bytes. Takes exactly
                // one `str` (hex digits, ASCII whitespace between pairs ignored); an
                // odd length / bad digit is a catchable runtime ValueError, so only
                // the arity + arg-type are checked here.
                if name == "fromhex"
                    && matches!(obj.as_ref(), Expr::Ident(bn, _) if bn == "bytes")
                {
                    if !kwargs.is_empty() {
                        return Err(Error::Type {
                            span: *span,
                            msg: "bytes.fromhex(s) does not take keyword arguments".into(),
                        });
                    }
                    if args.len() != 1 {
                        return Err(Error::Type {
                            span: *span,
                            msg: "bytes.fromhex(s) takes exactly one str argument".into(),
                        });
                    }
                    let at = check_expr(&args[0], env)?;
                    if !matches!(at, Ty::Str | Ty::Unknown) {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("bytes.fromhex(s) requires a `str` argument, found `{}`", at),
                        });
                    }
                    return Ok(Ty::Bytes);
                }
            }
            // (W5-a) `bytes(...)` constructor. Validate the argument TYPE up front so
            // an unsupported form is an honest CHECK error, never a `rustc` leak:
            //   bytes()          -> empty            bytes(n: int)     -> n zero bytes
            //   bytes(list[int]) -> range-checked    bytes(b: bytes)   -> a copy
            // `bytes(str)` (CPython "string argument without an encoding"),
            // `bytes(float)`, and the 2-arg `(source, encoding)` form are rejected.
            if matches!(callee.as_ref(), Expr::Ident(n, _) if n == "bytes") {
                if !kwargs.is_empty() {
                    return Err(Error::Type {
                        span: *span,
                        msg: "bytes() does not take keyword arguments".into(),
                    });
                }
                if args.len() > 1 {
                    return Err(Error::Type {
                        span: *span,
                        msg: "bytes(...) takes at most one argument in W5-a (an int count, a \
                              list[int], or another bytes); the (source, encoding) form is deferred".into(),
                    });
                }
                if let Some(a) = args.first() {
                    let at = check_expr(a, env)?;
                    let ok = matches!(at, Ty::Int | Ty::Bytes)
                        || matches!(&at, Ty::List(inner) if matches!(inner.as_ref(), Ty::Int | Ty::Unknown));
                    if !ok {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!(
                                "bytes(...) accepts an int count, a list[int], or another bytes, \
                                 not `{}` (to convert a `str`, use `s.encode()`)",
                                at
                            ),
                        });
                    }
                }
                return Ok(Ty::Bytes);
            }
            // Generics v2 (generic CLASSES): an EXPLICIT type-argument constructor
            // `Box[int](5)` parses as a CALL whose callee is `Box[int]` — an
            // `Index` of the class name. pyrst infers a generic class's type args
            // from `__init__` (`Box(5)` -> `Box[int]`), and the `Box[int]` callee
            // would otherwise be (mis)read as a list-index expression that
            // type-checks but emits broken Rust. Reject it honestly here, pointing
            // at the supported inferred form. (A genuine index-then-call like
            // `ops["double"](7)` has a non-class base and is unaffected.)
            if let Expr::Index { obj, .. } = callee.as_ref() {
                if let Expr::Ident(cls, _) = obj.as_ref() {
                    if env.ctx.classes.contains_key(cls.as_str()) {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!(
                                "explicit type arguments on a constructor are not supported: \
                                 write `{}(...)` and let the type arguments be inferred from the \
                                 constructor arguments",
                                cls
                            ),
                        });
                    }
                }
            }
            // (card d8a1ed83) Uniform check-time kwargs gate. A call carrying
            // keyword arguments is honest ONLY at a site pyrst actually MODELS a
            // kwarg for — a class constructor, or the builtin key=/reverse= of
            // sorted/min/max/list.sort. Every other call (flat free fn, qualified
            // module fn, user/builtin method) previously threaded kwargs through
            // and then DROPPED them: silently (json.dumps(indent=4) printed
            // compact), or as a late codegen error, or a leaked rustc E0061.
            // Reject those HERE — this pass runs for both `pyrst check` and
            // `pyrst build`, so no keyword argument is ever silently discarded.
            if !kwargs.is_empty() {
                reject_unmodeled_kwargs(callee.as_ref(), kwargs, env, *span)?;
            }
            // (enabler-fix-1 #1) Ordering builtins (`sorted`/`min`/`max`) over
            // USER-CLASS operands REQUIRE a defined `__lt__` (walking the base
            // chain) UNLESS a `key=` selects a different sort key. A class used as a
            // dict KEY silently derives `Ord` from field-declaration order
            // (codegen/items.rs), and a non-key class leaked a raw rustc E0277 — both
            // let a program CPython REJECTS (`TypeError: '<' not supported between
            // instances`) build and RUN. The honest gate is a real `__lt__`,
            // independent of hash-key status. Uses the non-erroring inference oracle
            // so a malformed argument still surfaces its own error in the arms below.
            if let Expr::Ident(bn, _) = callee.as_ref() {
                if matches!(bn.as_str(), "sorted" | "min" | "max")
                    && !args.is_empty()
                    && !kwargs.iter().any(|(k, _)| k == "key")
                {
                    let operand_tys: Vec<Ty> = if args.len() == 1 {
                        match infer_expr_ty(&args[0], &env.locals, env.ctx) {
                            Ty::List(e) | Ty::Set(e) | Ty::Iterator(e) => vec![*e],
                            other => vec![other],
                        }
                    } else {
                        args.iter().map(|a| infer_expr_ty(a, &env.locals, env.ctx)).collect()
                    };
                    for t in &operand_tys {
                        if let Ty::Class(cn, _) = t {
                            if env.ctx.get_method(cn, "__lt__").is_none() {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!(
                                        "`{}` over instances of class `{}` requires a `__lt__` \
                                         method (Python raises `TypeError: '<' not supported \
                                         between instances of '{}'`); define `__lt__` or pass `key=`",
                                        bn, cn, cn
                                    ),
                                });
                            }
                        }
                    }
                }
            }
            // (card aabf4ada) Honest arity / support gates for VARIADIC-EXEMPT
            // builtins (typeck skips their arity check, so codegen was silently
            // consuming FEWER args than typeck accepted — the P0 accept-N-consume-fewer
            // miscompiles surfaced by THE AUDIT this card demands). Reject the
            // unsupported shapes HERE (this pass runs for both `pyrst check` and
            // `pyrst build`), keeping typeck acceptance and codegen consumption in
            // EXACT agreement — the invariant the n-ary min/max fix (b557b9c1)
            // established. Each closed leak was probed build+run vs python3:
            //   sum(it, start, ...) -> `start` now folds in (fixed below); a 3+-arg
            //     call is CPython TypeError.
            //   min()/max()         -> CPython "expected at least 1 argument, got 0"
            //     (was a codegen `parts[0]`-on-empty-vec ICE / exit 101).
            //   int(x, base)        -> the base was DROPPED (int("10",2) -> 10 not 2).
            //   open(p, m, extra..) -> the 3rd+ positional (buffering) was DROPPED.
            //   getattr/setattr/hasattr -> dynamic-attribute stubs that returned the
            //     NAME string / no-op'd / were always-true (silent WRONG output);
            //     pyrst resolves attributes STATICALLY, so they have no faithful
            //     lowering — reject rather than miscompile.
            // These gates only REJECT; a valid arity falls through UNCHANGED to the
            // arms below and is result-typed there / by the inference oracle.
            if let Expr::Ident(bn, _) = callee.as_ref() {
                match bn.as_str() {
                    "sum" if args.len() > 2 => {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!(
                                "sum() takes at most 2 arguments ({} given) — \
                                 `sum(iterable[, start])`",
                                args.len()
                            ),
                        });
                    }
                    "min" | "max" if args.is_empty() => {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("{}() expected at least 1 argument, got 0", bn),
                        });
                    }
                    // (card 00fb0e6d) 0-arg calls to arity-expecting builtins ICE'd
                    // at BUILD (codegen indexes `parts[0]` on an empty arg vec ->
                    // panic / exit 101 — loud but ugly, and below the diagnostics
                    // bar). Each needs at least one positional argument in CPython;
                    // reject them at CHECK (this pass runs for `check` AND `build`)
                    // with CPython's own wording. DELIBERATELY EXCLUDED: int / float /
                    // str / bool, whose 0-arg form CPython makes VALID (0 / 0.0 / '' /
                    // False) — those fall through and are defaulted in codegen.
                    "abs" | "len" | "ord" | "chr" | "hex" | "oct" | "bin" | "any" | "all"
                        if args.is_empty() => {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("{}() takes exactly one argument (0 given)", bn),
                        });
                    }
                    // (card c34ac64a fix D) 0-arg (and under-arity) calls to these
                    // builtins ICE'd at BUILD (codegen indexes a missing positional
                    // arg -> panic / exit 101 — loud but below the diagnostics bar).
                    // Reject at CHECK (this pass runs for `check` AND `build`) with
                    // CPython 3.12's exact wording. round() gets its own message
                    // (`missing required argument 'number' (pos 1)`) rather than the
                    // shared "takes exactly one argument" above (workflow #19).
                    "round" if args.is_empty() => {
                        return Err(Error::Type {
                            span: *span,
                            msg: "round() missing required argument 'number' (pos 1)".to_string(),
                        });
                    }
                    "open" if args.is_empty() => {
                        return Err(Error::Type {
                            span: *span,
                            msg: "open() missing required argument 'file' (pos 1)".to_string(),
                        });
                    }
                    "pow" if args.len() < 2 => {
                        return Err(Error::Type {
                            span: *span,
                            msg: if args.is_empty() {
                                "pow() missing required argument 'base' (pos 1)".to_string()
                            } else {
                                "pow() missing required argument 'exp' (pos 2)".to_string()
                            },
                        });
                    }
                    "map" if args.len() < 2 => {
                        return Err(Error::Type {
                            span: *span,
                            msg: "map() must have at least two arguments.".to_string(),
                        });
                    }
                    "filter" if args.len() < 2 => {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("filter expected 2 arguments, got {}", args.len()),
                        });
                    }
                    "sum" | "sorted" | "reversed" if args.is_empty() => {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("{}() expected at least 1 argument, got 0", bn),
                        });
                    }
                    // (card 87bd8eb4) A 3-arg `range(a, b, step)` with a
                    // negative/zero literal step is NO LONGER rejected: codegen now
                    // lowers 3-arg range through the direction-aware `__py_range_step`
                    // builder, so a negative literal DESCENDS (valid CPython) and a
                    // zero step raises a catchable `ValueError` at runtime — exactly
                    // CPython's behavior. (Was an honest rejection when range only
                    // lowered to an ascending Rust range.)
                    "int" if args.len() > 1 => {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!(
                                "int() with a base argument is not supported ({} arguments \
                                 given) — pyrst's `int(x)` parses base 10 only",
                                args.len()
                            ),
                        });
                    }
                    "open" if args.len() > 2 => {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!(
                                "open() takes at most 2 positional arguments ({} given) — \
                                 pyrst supports `open(path[, mode])` only",
                                args.len()
                            ),
                        });
                    }
                    "getattr" | "setattr" | "hasattr" => {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!(
                                "`{}` (dynamic attribute access) is not supported — pyrst \
                                 resolves attributes statically; use direct field access",
                                bn
                            ),
                        });
                    }
                    _ => {}
                }
            }
            // Check if this is a class constructor or function call.
            match callee.as_ref() {
                Expr::Ident(name, _) => {
                    // (W5-h) An `@extern class` handle is NON-USER-CONSTRUCTIBLE: it is
                    // an opaque external resource produced ONLY by its lib constructor
                    // (e.g. `re.compile(...)` for `re.Pattern`), whose `@extern`
                    // template builds the private `__PyHandle_<name>` struct. A direct
                    // `Pattern()` has no `new()` and no value-struct to build — it
                    // check-passed and then died at rustc (E0422) — so reject it
                    // honestly here, pointing at the real constructor.
                    if env.ctx.is_handle_class(name.as_str()) {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!(
                                "`{}` is an opaque handle and cannot be constructed directly — \
                                 it is produced only by its library constructor (an `@extern` \
                                 function that returns a `{}`, e.g. `re.compile(...)` for a \
                                 `Pattern`), never by calling `{}(...)`",
                                name, name, name
                            ),
                        });
                    }
                    if env.ctx.classes.contains_key(name.as_str()) {
                        // (W1.5 fix B) CPython binds constructor arguments to the
                        // class's __init__ PARAMETERS. When __init__ exists, map
                        // positionals + keywords onto its (self-exclusive)
                        // signature via the SHARED mapper — closing the old
                        // check-pass / build-fail split for a class whose __init__
                        // param names differ from its field names
                        // (`__init__(self, a, b)` assigning `self.x`/`self.y`,
                        // called `C(a=1, b=2)`), and turning an unknown / duplicate
                        // / missing / too-many argument into an honest check-time
                        // error naming `<Class>.__init__`. The slot-aligned arg
                        // types (default holes `Unknown`, non-binding) feed generic
                        // type-argument inference; a plain class ignores them and
                        // yields `Ty::Class(name, [])` (unchanged).
                        //
                        // A class WITHOUT __init__ is a struct-literal: a BARE
                        // `C()` keeps the zero-init (struct-default) idiom, else the
                        // arguments are matched against a synthesized field-order
                        // parameter list (every field required — partial explicit
                        // construction was already a build error) so dup / unknown /
                        // missing become check errors and the codegen positional +
                        // keyword merge stays honest.
                        let init_key = format!("{}.__init__", name);
                        if let Some(sig) = env.ctx.funcs.get(&init_key).cloned() {
                            let slots = map_kwargs_to_slots(&init_key, &sig, args.len(), kwargs, *span)?;
                            let mut arg_tys = vec![Ty::Unknown; sig.params.len()];
                            for (p, a) in kwargs_provided_in_eval_order(args, kwargs, &slots) {
                                arg_tys[p] = check_expr(a, env)?;
                            }
                            check_class_instantiation(name, &arg_tys, env.ctx, *span)?
                        } else if args.is_empty() && kwargs.is_empty() {
                            check_class_instantiation(name, &[], env.ctx, *span)?
                        } else {
                            // (enabler-fix-1 #3a) EXCLUDE promoted class constants from
                            // the synthesized ctor signature — they are associated
                            // `const`s, not instance fields, so counting one as a ctor
                            // param gave the wrong arity (a check-pass / build-fail).
                            let fields: Vec<_> = env.ctx
                                .get_all_fields(name.as_str())
                                .into_iter()
                                .filter(|f| !env.ctx.is_promoted_const(name, &f.name))
                                .collect();
                            let synth = FuncSig {
                                params: fields
                                    .iter()
                                    .map(|f| {
                                        (f.name.clone(), Ty::from_type_expr(&f.ty, f.span).unwrap_or(Ty::Unknown))
                                    })
                                    .collect(),
                                ret: Ty::Class(name.to_string(), vec![]),
                                // (card 6f69d4a3) Honor field DEFAULTS in the
                                // synthesized (dataclass / no-__init__) constructor
                                // signature so `Config("localhost")` fills a defaulted
                                // `port: int = 8080` instead of erroring "missing a
                                // required argument" — matching CPython's synthesized
                                // __init__. Was `vec![None; ..]` (all required).
                                param_defaults: fields.iter().map(|f| f.default.clone()).collect(),
                                param_by_ref: Vec::new(),
                            };
                            let slots = map_kwargs_to_slots(name, &synth, args.len(), kwargs, *span)?;
                            for (_p, a) in kwargs_provided_in_eval_order(args, kwargs, &slots) {
                                check_expr(a, env)?;
                            }
                            check_class_instantiation(name, &[], env.ctx, *span)?
                        }
                    } else if (name == "min" || name == "max") && args.len() == 1 {
                        // Single-iterable min/max: the result is the element type
                        // of the list/set argument. A `key=`/other kwarg may also
                        // be present (e.g. `min(words, key=len)`) — the lone
                        // positional arg is still the iterable. The 2-arg / n-ary
                        // scalar form `min(a, b, ...)` is handled by the next arm.
                        let arg_ty = check_expr(&args[0], env)?;
                        // Generics v1: `min`/`max` iterate the argument (and order
                        // its elements) — a bare type variable has neither
                        // IntoIterator nor Ord, so reject it honestly here.
                        reject_typevar_op(&arg_ty, "consume the contents of", *span)?;
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        match arg_ty {
                            // (LAZY-GEN V1-c) A generator argument's min/max is
                            // its element type, same as a list/set.
                            Ty::List(elem) | Ty::Set(elem) | Ty::Iterator(elem) => *elem,
                            _ => Ty::Unknown,
                        }
                    } else if (name == "min" || name == "max") && args.len() >= 2 {
                        // (card b557b9c1) n-ary scalar form `min(a, b, c, ...)`:
                        // codegen folds ALL positional args (see codegen/exprs.rs).
                        // `key=` is meaningful only for the single-iterable form;
                        // combined with 2+ positional args it has no supported
                        // lowering (the codegen `key=` arm treats arg0 as an
                        // iterable), so reject it HONESTLY here rather than leak a
                        // codegen/rustc error — keeping typeck acceptance and codegen
                        // consumption in exact agreement.
                        if kwargs.iter().any(|(k, _)| k == "key") {
                            return Err(Error::Type {
                                span: *span,
                                msg: format!(
                                    "`{}` with {} positional arguments does not support \
                                     `key=` (pass a single iterable as `{}(iterable, key=...)`, \
                                     or drop `key=`)",
                                    name, args.len(), name
                                ),
                            });
                        }
                        // Check every argument so its own errors surface; the winner
                        // is one of the (homogeneous) args, so the result type is the
                        // first arg's type — restoring correct float DISPLAY that the
                        // old generic `ret: Unknown` fall-through lost. A `__lt__`-less
                        // user-class arg was already rejected by the ordering gate above.
                        let mut result_ty = Ty::Unknown;
                        for (i, a) in args.iter().enumerate() {
                            let t = check_expr(a, env)?;
                            if i == 0 {
                                result_ty = t;
                            }
                        }
                        result_ty
                    } else if name == "enumerate" && !args.is_empty() {
                        // enumerate(iterable[, start]) -> List(Tuple(Int, elem))
                        // Check all args/kwargs for their own errors first.
                        let arg0_ty = check_expr(&args[0], env)?;
                        // Generics v1: enumerate iterates its argument — a bare
                        // type variable has no IntoIterator bound.
                        reject_typevar_op(&arg0_ty, "consume the contents of", *span)?;
                        for a in &args[1..] {
                            check_expr(a, env)?;
                        }
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        let elem = match arg0_ty {
                            // (LAZY-GEN V1-c) A generator argument enumerates by
                            // its element type, same as a list/set.
                            Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => *inner,
                            Ty::Str => Ty::Str,
                            _ => Ty::Unknown,
                        };
                        if matches!(elem, Ty::Unknown) {
                            Ty::Unknown
                        } else {
                            Ty::List(Box::new(Ty::Tuple(vec![Ty::Int, elem])))
                        }
                    } else if name == "zip" {
                        // zip(a, b, ...) -> List(Tuple(elem_a, elem_b, ...))
                        // Check all args/kwargs for their own errors first.
                        let mut elem_tys: Vec<Ty> = Vec::new();
                        let mut any_unknown = false;
                        for a in args {
                            let ty = check_expr(a, env)?;
                            // Generics v1: zip iterates each argument — a bare type
                            // variable has no IntoIterator bound.
                            reject_typevar_op(&ty, "consume the contents of", *span)?;
                            match ty {
                                // (LAZY-GEN V1-c) `zip` accepts a mix of sources
                                // per argument — a generator arg contributes its
                                // element type, same as a list/set.
                                Ty::List(inner) | Ty::Set(inner) | Ty::Iterator(inner) => elem_tys.push(*inner),
                                Ty::Str => elem_tys.push(Ty::Str),
                                _ => any_unknown = true,
                            }
                        }
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        // (CARD 0c4bb6be) Codegen's zip lowering has a
                        // MECHANICAL N-ary form for 2-4 arguments only: a bare
                        // `.zip()` for 2, and a chained
                        // `.zip().zip()....map()` flatten for 3/4 (CPython
                        // yields a flat N-tuple; Rust's chained `.zip()`
                        // nests — `a.zip(b).zip(c)` is `((a,b),c)`, not
                        // `(a,b,c)` — so the flatten is hand-written per
                        // arity rather than open-ended). 5+ args previously
                        // typechecked OK here (this arm never looked at
                        // `args.len()`) and then failed at BUILD with a raw,
                        // unexplained rustc E0425/E0599. Reject it HONESTLY
                        // here instead — both `check` and `build` run this
                        // same `check_bodies` pass, so both reject it with a
                        // clear message rather than only one failing loudly.
                        if args.len() > 4 {
                            return Err(Error::Type {
                                span: *span,
                                msg: format!(
                                    "zip with {} arguments is not supported; nest zip calls",
                                    args.len()
                                ),
                            });
                        }
                        if any_unknown || elem_tys.is_empty() {
                            Ty::Unknown
                        } else {
                            Ty::List(Box::new(Ty::Tuple(elem_tys)))
                        }
                    } else if name == "map" && args.len() == 2 {
                        // map(f, iterable) -> List(return type of f applied to the
                        // iterable's element type). Only a List iterable yields a
                        // concrete result: codegen's `.iter().cloned().map(..)`
                        // compiles for a Vec, but a String has no `.iter()` and
                        // map-over-set is unverified, so Set/Str/unknown stay
                        // permissive (Unknown), matching the filter arm below. The
                        // lambda body is still checked for its own errors, and we
                        // never narrow types_compatible.
                        let iter_ty = check_expr(&args[1], env)?;
                        let elem = match &iter_ty {
                            Ty::List(inner) => Some((**inner).clone()),
                            _ => None,
                        };
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        let body_ty = lambda_ret_with_elem(&args[0], elem.as_ref(), env)?;
                        match (&iter_ty, body_ty) {
                            (Ty::List(_), Some(t)) if !matches!(t, Ty::Unknown) => {
                                Ty::List(Box::new(t))
                            }
                            _ => Ty::Unknown,
                        }
                    } else if name == "filter" && args.len() == 2 {
                        // filter(pred, iterable) -> the iterable's list type
                        // unchanged (filter preserves elements). The predicate body
                        // is still checked (binding its first param to the element
                        // type) so a malformed predicate is caught; its return type
                        // is irrelevant to the result element type.
                        let iter_ty = check_expr(&args[1], env)?;
                        let elem = match &iter_ty {
                            Ty::List(inner) | Ty::Set(inner) => Some((**inner).clone()),
                            Ty::Str => Some(Ty::Str),
                            _ => None,
                        };
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        let _ = lambda_ret_with_elem(&args[0], elem.as_ref(), env)?;
                        match iter_ty {
                            Ty::List(_) => iter_ty,
                            _ => Ty::Unknown,
                        }
                    } else if let Some(sig) = env.ctx.funcs.get(name.as_str()) {
                        // Regular function call: check arity (positional only in v0).
                        let expected = sig.params.len();
                        let got = args.len() + kwargs.len();
                        // Variadic builtins: skip arity check.
                        let variadic = matches!(name.as_str(),
                            "print" | "range" | "len" | "str" | "int" | "float" | "bool" | "enumerate" | "zip"
                            | "abs" | "min" | "max" | "sorted" | "sum" | "input" | "list" | "dict" | "tuple" | "set"
                            // (W5-a) bytes() takes 0 args, bytes(x) takes 1 — variadic.
                            | "bytes"
                            | "getattr" | "setattr" | "hasattr" | "open");
                        // (kwargs v1) A keyword-bearing call to a USER or MODULE
                        // function runs the keyword→positional mapping, which
                        // subsumes the legacy arity check (unknown / duplicate /
                        // missing / too-many-positional are its errors). The
                        // modeled builtins (`sorted`/`min`/`max`, all variadic)
                        // keep their legacy path — their stub sigs carry invented
                        // param names the mapper must never bind against.
                        let kw_slots: Option<Vec<ArgSlot>> = if !kwargs.is_empty()
                            && !variadic
                            && !env.ctx.builtin_funcs.contains(name.as_str())
                        {
                            Some(map_kwargs_to_slots(name, sig, args.len(), kwargs, *span)?)
                        } else {
                            None
                        };
                        // Count required parameters (those without defaults)
                        let required = sig.param_defaults.iter().take_while(|d| d.is_none()).count();
                        if kw_slots.is_none() && !variadic && (got < required || got > expected) {
                            return Err(Error::Type {
                                span: *span,
                                msg: format!(
                                    "function `{}` takes {} argument(s), {} given",
                                    name, expected, got
                                ),
                            });
                        }
                        let sig_params = sig.params.clone();
                        let sig_by_ref = sig.param_by_ref.clone();
                        let sig_ret = sig.ret.clone();
                        // Generics v1: is this a parametric generic function? Its
                        // type-var-bearing params are validated by call-site
                        // UNIFICATION (below), not the concrete `types_compatible`
                        // check, so a `T` param accepts any argument type while a
                        // CONCRETE param of a generic function is still checked.
                        let is_generic = env.ctx.generic_funcs
                            .get(name.as_str())
                            .is_some_and(|tps| !tps.is_empty());
                        // (kwargs v1) The provided (slot, expr) pairs in CPython
                        // evaluation order. Without kwargs this is exactly
                        // `enumerate(args)` and the loop below is byte-identical
                        // to the legacy positional loop; with kwargs the keyword
                        // values are appended in source order, each aligned to
                        // its mapped parameter slot, and `arg_tys` is built
                        // SLOT-ALIGNED (default holes stay `Ty::Unknown`, which
                        // unification treats as no-information).
                        let provided: Vec<(usize, &Expr)> = match &kw_slots {
                            None => args.iter().enumerate().collect(),
                            Some(slots) => kwargs_provided_in_eval_order(args, kwargs, slots),
                        };
                        let mut arg_tys: Vec<Ty> = if kw_slots.is_some() {
                            vec![Ty::Unknown; expected]
                        } else {
                            Vec::with_capacity(args.len())
                        };
                        for (i, a) in provided {
                            // EPIC-4 V2: an argument bound to a by-reference
                            // (`Mut[T]`) param must be a PLACE — an lvalue we can
                            // take `&mut` of (variable / field / index). A
                            // temporary (call/constructor/literal/binop result)
                            // has no caller-visible storage to borrow, so it is an
                            // honest typeck error here rather than a later rustc
                            // borrow failure. The arg's TYPE is still checked
                            // against the inner `T` by the compatibility check
                            // below (the param type was unwrapped from `Mut[T]`).
                            if sig_by_ref.get(i).copied().unwrap_or(false)
                                && !is_place_expr(a)
                            {
                                let pname = sig_params.get(i)
                                    .map(|(n, _)| n.as_str())
                                    .unwrap_or("<arg>");
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!(
                                        "by-reference parameter `{}` requires a variable, not a temporary",
                                        pname
                                    ),
                                });
                            }
                            let arg_ty = check_expr(a, env)?;
                            // A builtin that uses the SHAPE of its argument cannot
                            // accept a bare type variable from the `T: Clone` bound
                            // alone. Two families differ in v2:
                            //  - FORMAT (`print`/`str`/`repr`/`ascii`): generics v2
                            //    INFERS a `Display` bound on `T` (collected by
                            //    `infer_func_typevar_bounds`), so a bare `T` is now
                            //    LEGAL here — no rejection.
                            //  - SHAPE-CONSUMING (`len`/`sum`/`sorted`/`reversed`/
                            //    `any`/`all`/`list`/`tuple`/`set`/`dict`/
                            //    `enumerate`/`zip`) iterate/index/sum the argument
                            //    (IntoIterator / Add / etc.) — beyond v2, so a bare
                            //    `T` STAYS an honest rejection.
                            // (`first([...])` etc. are fine: their RESULT is
                            // concrete after unification; only a BARE `T` value
                            // reaches here as `Ty::TypeVar`.)
                            if matches!(name.as_str(),
                                "len" | "sum" | "sorted" | "reversed" | "any" | "all"
                                | "list" | "tuple" | "set" | "dict" | "enumerate" | "zip")
                            {
                                reject_typevar_op(&arg_ty, "consume the contents of", *span)?;
                            }
                            // (LAZY-GEN V1-d) These builtins cannot consume a lazy
                            // generator: `len` needs a length, `reversed` a backward
                            // pass, and the format family (`str`/`repr`/`ascii`/
                            // `print`) would show an opaque handle. Reject with the
                            // materialize fix. The V1-c WORKS builtins (list/sum/
                            // sorted/any/all here, and the earlier-branch min/max/
                            // enumerate/zip) deliberately consume a generator lazily
                            // and are NOT in this set (docs §D.1/§D.2).
                            if matches!(arg_ty, Ty::Iterator(_)) {
                                match name.as_str() {
                                    "len" => return Err(iterator_materialize_error(
                                        "has no len()", "len(list(g))", *span)),
                                    "reversed" => return Err(iterator_materialize_error(
                                        "cannot be reversed", "reversed(list(g))", *span)),
                                    "str" | "repr" | "ascii" => return Err(iterator_materialize_error(
                                        "has no string form (it would show an opaque generator handle)",
                                        &format!("{}(list(g))", name), *span)),
                                    "print" => return Err(iterator_materialize_error(
                                        "has no printable form (it would show an opaque generator handle)",
                                        "print(list(g))", *span)),
                                    _ => {}
                                }
                            }
                            // (W5-g) A move-only handle passed to a builtin that needs
                            // display (`print`/`str`/`repr`/`ascii`), a length, or
                            // iteration/collection is an honest error naming the kind —
                            // a `PyFile` has no `Display`/`PyRepr` (probe: rustc E0277).
                            // NOTE: passing a handle to a USER function is a legal MOVE
                            // (checked elsewhere); only these capability-needing
                            // builtins are rejected here.
                            if matches!(name.as_str(),
                                "print" | "str" | "repr" | "ascii" | "len" | "sum"
                                | "sorted" | "reversed" | "any" | "all" | "list"
                                | "tuple" | "set" | "dict" | "enumerate" | "zip"
                                | "min" | "max")
                            {
                                reject_handle_op(&arg_ty, &format!("pass to `{}()`", name), *span)?;
                            }
                            // `repr(instance)` of a user class routes through the
                            // class's __repr__ (per-class `impl PyRepr`). CPython's
                            // repr uses __repr__ ONLY — never __str__ — and falls
                            // back to `<C object at 0x..>` when absent. pyrst has no
                            // stable object identity to print, so a class WITHOUT a
                            // __repr__ (in itself or any ancestor) is an HONEST error
                            // here rather than either silently borrowing __str__
                            // (wrong output) or a later rustc `PyRepr` build wall.
                            if name == "repr" {
                                if let Ty::Class(cn, _) = &arg_ty {
                                    let mut seen = std::collections::HashSet::new();
                                    // (card 6f69d4a3) A @dataclass with no user
                                    // __str__/__repr__ gets a SYNTHESIZED __repr__
                                    // (`ClassName(field=value)`), so repr() is valid
                                    // on it — matches the codegen synthesis guard.
                                    let dataclass_synth_repr = env.ctx.classes.get(cn).is_some_and(|cd| {
                                        cd.is_dataclass
                                            && !cd.methods.iter().any(|m| m.name == "__str__" || m.name == "__repr__")
                                    });
                                    if !class_defines_repr(env.ctx, cn, &mut seen)
                                        && !dataclass_synth_repr
                                    {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "repr() of `{}` requires a `__repr__` method (pyrst has no default object repr; define __repr__ or use str())",
                                                cn
                                            ),
                                        });
                                    }
                                }
                            }
                            // (enabler-fix-1 #5a / wf#18) print()/str()/repr()/ascii()
                            // of a CONTAINER renders every element via `.py_repr()`
                            // (codegen's PyRepr), so a container whose element class has
                            // no `__repr__` (nor a synthesized-dataclass repr) leaked
                            // rustc E0599. Require element reprability at CHECK. A BARE
                            // class arg is untouched (print/str use its Display/__str__,
                            // which does not need __repr__).
                            if matches!(name.as_str(), "print" | "str" | "repr" | "ascii")
                                && matches!(
                                    arg_ty,
                                    Ty::List(_) | Ty::Set(_) | Ty::Dict(..) | Ty::Tuple(_) | Ty::Iterator(_)
                                )
                                && !type_is_reprable(env.ctx, &arg_ty)
                            {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!(
                                        "{}() of this container renders each element via \
                                         repr(), but an element type has no `__repr__` — \
                                         define `__repr__` on the element's class",
                                        name
                                    ),
                                });
                            }
                            // Concrete-only positional arg-type check (skip variadic builtins).
                            // Only fires when BOTH param and arg types are concrete and
                            // incompatible. Int->Float is explicitly allowed (Python coercion).
                            // A param that IS (or contains) a type variable is skipped
                            // here — unification validates it structurally afterwards.
                            if !variadic {
                                if let Some((_, param_ty)) = sig_params.get(i) {
                                    // (LAZY-GEN V1-d) A generator passed where a
                                    // concrete `list[T]` is required: honest
                                    // MATERIALIZE error (`list(g)`) instead of the
                                    // bare "expected list, found Iterator" mismatch.
                                    reject_iterator_into_list(&arg_ty, param_ty, *span)?;
                                    let int_to_float =
                                        matches!(arg_ty, Ty::Int) && matches!(param_ty, Ty::Float);
                                    if !int_to_float
                                        && !matches!(arg_ty, Ty::Unknown)
                                        && !matches!(param_ty, Ty::Unknown)
                                        && !contains_typevar(param_ty)
                                        && !types_compatible(&arg_ty, param_ty, env.ctx)
                                    {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "argument {} to `{}`: expected {}, found {}",
                                                i + 1, name, param_ty, arg_ty
                                            ),
                                        });
                                    }
                                }
                            }
                            if kw_slots.is_some() {
                                arg_tys[i] = arg_ty;
                            } else {
                                arg_tys.push(arg_ty);
                            }
                        }
                        if is_generic {
                            // (enabler-fix-2 #1b) A user class bound to a Hash-position
                            // type var that would not otherwise derive Eq/Hash is an
                            // honest error here — pyrst can't thread the derive through
                            // a type parameter (was a rustc E0277/E0599 build wall).
                            reject_class_key_through_generic(name, &arg_tys, env.ctx, *span)?;
                            // Unify the declared (type-var-bearing) params against
                            // the actual argument types: surfaces a conflicting
                            // binding ("conflicting types for type parameter `T`")
                            // or an uninferable type parameter, and yields the
                            // SUBSTITUTED concrete return type for this call.
                            infer_generic_call_result(name, &arg_tys, env.ctx, *span)?
                                .unwrap_or(sig_ret)
                        } else {
                            sig_ret
                        }
                    } else if name == "super" && args.is_empty() && kwargs.is_empty() {
                        // super() returns Unknown type — the codegen handles super().method() specially
                        Ty::Unknown
                    } else if let Some(local_ty) = env.lookup(name) {
                        // Calling a function-VALUED local/param by bare name
                        // (`f(x)` where `f: Callable[[int], int]`). Check the
                        // arguments first (for their own errors), then — if the
                        // value's type is a `Ty::Func` — enforce arity and per-arg
                        // compatibility and yield its return type. A non-Func
                        // callable value (untyped lambda binding, Unknown) stays
                        // permissive (Unknown), exactly as before.
                        let arg_tys = args.iter()
                            .map(|a| check_expr(a, env))
                            .collect::<Result<Vec<_>>>()?;
                        for (_, v) in kwargs {
                            check_expr(v, env)?;
                        }
                        if let Ty::Func(param_tys, ret) = &local_ty {
                            if args.len() != param_tys.len() {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!(
                                        "function value `{}` takes {} argument(s), {} given",
                                        name, param_tys.len(), args.len()
                                    ),
                                });
                            }
                            for (i, (arg_ty, param_ty)) in arg_tys.iter().zip(param_tys.iter()).enumerate() {
                                let int_to_float =
                                    matches!(arg_ty, Ty::Int) && matches!(param_ty, Ty::Float);
                                if !int_to_float
                                    && !matches!(arg_ty, Ty::Unknown)
                                    && !matches!(param_ty, Ty::Unknown)
                                    && !types_compatible(arg_ty, param_ty, env.ctx)
                                {
                                    return Err(Error::Type {
                                        span: *span,
                                        msg: format!(
                                            "argument {} to `{}`: expected {}, found {}",
                                            i + 1, name, param_ty, arg_ty
                                        ),
                                    });
                                }
                            }
                            (**ret).clone()
                        } else if is_noncallable_ty(&local_ty) {
                            // (honest errors) Calling a value of a KNOWN
                            // non-callable type (`x: int = 5; x(3)`) is a type
                            // error, not a deferred rustc E0618. `Unknown` and
                            // `Class` stay permissive (escape hatch: a class
                            // instance may be callable in a later increment).
                            return Err(Error::Type {
                                span: *span,
                                msg: format!("`{}` of type {} is not callable", name, local_ty),
                            });
                        } else {
                            Ty::Unknown
                        }
                    } else {
                        return Err(Error::Type {
                            span: *span,
                            msg: format!("undefined function `{}`", name),
                        });
                    }
                }
                // Method call: e.g., p.magnitude() — callee is Attr
                _ => {
                    // Qualified module call `X.f(args)` for a REAL imported module
                    // (card 81db88e0). When the callee is `Attr{Ident(X), f}` and X
                    // is a tracked module name, this is NOT a method call: it is a
                    // call to module X's function `f`, whose signature lives FLAT in
                    // `ctx.funcs` under the bare name. We type it exactly like a flat
                    // call to `f` (arity + per-arg compatibility + return). `math`
                    // is now a real embedded module, so `math.sqrt(x)` resolves
                    // through here like any other module's function. A qualified
                    // call to a name the module does NOT define is an honest error
                    // here (see the unknown-qualified-call rejection below), not a
                    // silently-Unknown call.
                    if let Expr::Attr { obj, name, span: attr_span } = callee.as_ref() {
                        if let Some(modname) = module_owner_of(obj) {
                            // (W3-3) `modname` is the single- OR two-component module
                            // owner (`os` for `os.f()`, `os.path` for `os.path.f()`).
                            if let Some(mod_fns) = env.ctx.module_funcs.get(&modname) {
                                if mod_fns.iter().any(|n| n == name) {
                                    // (W3-1) f is defined by module `modname` —
                                    // resolve its signature OWNER-FIRST against that
                                    // module's own per-module table (flat fallback
                                    // only for synthetic ctxs), not the flat table.
                                    let sig = env.ctx.resolve_module_func(&modname, name).cloned().ok_or_else(|| Error::Type {
                                        span: *attr_span,
                                        msg: format!("module `{}` function `{}` has no signature", modname, name),
                                    })?;
                                    // Arity. (kwargs v1) A keyword-bearing
                                    // qualified call runs the keyword→positional
                                    // mapping instead, which subsumes this check
                                    // (unknown / duplicate / missing / too many
                                    // positional are its errors).
                                    let expected = sig.params.len();
                                    let got = args.len() + kwargs.len();
                                    let diag_label = format!("{}.{}", modname, name);
                                    let kw_slots: Option<Vec<ArgSlot>> = if !kwargs.is_empty() {
                                        Some(map_kwargs_to_slots(&diag_label, &sig, args.len(), kwargs, *span)?)
                                    } else {
                                        None
                                    };
                                    let required = sig.param_defaults.iter().take_while(|d| d.is_none()).count();
                                    if kw_slots.is_none() && (got < required || got > expected) {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "function `{}.{}` takes {} argument(s), {} given",
                                                modname, name, expected, got
                                            ),
                                        });
                                    }
                                    // Per-arg type-check + result resolution via the
                                    // SHARED helper, so a qualified call to a GENERIC
                                    // imported function (`heapq.heappush(h, 5)`) runs
                                    // the SAME call-site unification as the flat form
                                    // (`heappush(h, 5)`): a `list[T]` param accepts a
                                    // `list[int]` arg (T=int), the return type is
                                    // substituted, and conflicting/uninferable type
                                    // parameters are honest errors here too. A
                                    // non-generic qualified call (`string.capwords`)
                                    // is unchanged — concrete params are still checked
                                    // and the declared return is returned.
                                    // (kwargs v1) With kwargs, build the argument
                                    // types SLOT-ALIGNED (evaluating in CPython
                                    // call order: positionals, then keyword values
                                    // in source order; default holes stay
                                    // `Ty::Unknown`). Without kwargs this is the
                                    // legacy positional collection, unchanged.
                                    let arg_tys: Vec<Ty> = match &kw_slots {
                                        None => args.iter()
                                            .map(|a| check_expr(a, env))
                                            .collect::<Result<Vec<_>>>()?,
                                        Some(slots) => {
                                            let mut tys = vec![Ty::Unknown; expected];
                                            for (p, a) in
                                                kwargs_provided_in_eval_order(args, kwargs, slots)
                                            {
                                                tys[p] = check_expr(a, env)?;
                                            }
                                            tys
                                        }
                                    };
                                    let result = check_call_arg_types_and_result(
                                        name, &diag_label, &sig, &arg_tys, env.ctx, *span,
                                    )?;
                                    return Ok(result);
                                } else {
                                    // X IS a tracked module but defines no such `f`.
                                    return Err(Error::Type {
                                        span: *attr_span,
                                        msg: format!("module `{}` has no function `{}`", modname, name),
                                    });
                                }
                            } else if let Some((parent, sub)) = modname.split_once('.') {
                                // (W4-b) A qualified MUTABLE-GLOBAL method call
                                // `m.g.method(...)` — `m.g` reads another module's
                                // module-level global (e.g. `sys.argv`), NOT a
                                // submodule. `module_owner_of` flattened the two-hop
                                // receiver to `m.g`, which has no `module_funcs`
                                // entry, so it lands here; disambiguate it from the
                                // genuine submodule case with a mutable-global lookup
                                // on (owner=`parent`, name=`sub`), guarded on `parent`
                                // not being a shadowing local.
                                if !env.locals.contains_key(parent)
                                    && env.ctx.is_mutable_global(Some(parent), sub)
                                {
                                    // An in-place MUTATING method (`append`/`extend`/
                                    // …) on another module's global is a cross-module
                                    // WRITE — a v1 honest error mirroring the
                                    // `AttrAssign` cross-module-mutation diagnostic.
                                    // Accepting it would SILENTLY mutate the
                                    // value-semantics read CLONE and drop it (the
                                    // CPython append never reaches the real vector).
                                    // This REPLACES the misleading "has no attribute /
                                    // submodule" message the flattening accidentally
                                    // produced for `sys.argv.append(...)`.
                                    if MUTATING_METHODS.contains(&name.as_str()) {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "cross-module mutation of `{0}.{1}` is not supported; \
                                                 mutate it from a function inside `{0}` (a `def` in `{0}` \
                                                 that declares `global {1}` and assigns it)",
                                                parent, sub
                                            ),
                                        });
                                    }
                                    // A NON-mutating method (`sys.argv.count(x)`) is a
                                    // READ: fall through to the ordinary value-method
                                    // path below, which types the receiver as the
                                    // qualified-read CLONE (`list[str]`) and dispatches
                                    // the builtin method — behaviorally identical to
                                    // `tmp = sys.argv; tmp.count(x)`.
                                }
                                // (W3-3, item 3) `a.b.f()` where the dotted module
                                // `a.b` is NOT imported, but its PARENT `a` IS a known
                                // module. pyrst does not auto-expose submodules on
                                // `import a` (the explicit-import-required divergence),
                                // so this is an honest error that SUGGESTS the missing
                                // submodule import — never a silent `Ty::Unknown` call
                                // that then dies at rustc, and never a truncation to
                                // the parent (`os.path.join(...)` must not run
                                // `os.join`).
                                else if env.ctx.module_funcs.contains_key(parent)
                                    || env.ctx.module_consts.contains_key(parent)
                                    || env.ctx.module_symbols.contains_key(parent)
                                {
                                    return Err(Error::Type {
                                        span: *attr_span,
                                        msg: format!(
                                            "module `{parent}` has no attribute `{sub}`; \
                                             if `{modname}` is a submodule, import it \
                                             explicitly with `import {modname}` (pyrst \
                                             does not auto-expose submodules on `import \
                                             {parent}`)"
                                        ),
                                    });
                                }
                            }
                        }
                    }
                    if let Expr::Attr { obj, name, .. } = callee.as_ref() {
                        let obj_ty = check_expr(obj, env)?;
                        // Generics v1: calling a method on a bare type variable
                        // (`t.foo()` where `t: T`) needs a trait bound and is
                        // rejected — `T` is opaque, with no known methods.
                        reject_typevar_op(&obj_ty, "call a method on", *span)?;
                        // (W5-h) A lib handle (`Ty::Handle(n)`) routes its method call
                        // through the same class path as a value class, so
                        // `p.search(text)` on a `re.Pattern` handle checks (arity /
                        // kwargs / by-ref / return) like a class method.
                        if let Some(class_name) = method_lookup_class(&obj_ty, env.ctx) {
                            let key = format!("{}.{}", class_name, name);
                            // (kwargs v1) Fall back to the inheritance-aware
                            // `get_method` so an INHERITED method call resolves
                            // here too (its arity and keyword mapping are then
                            // checked like an own-class method; the returned type
                            // is the same `subst_class_member(ret)` the generic
                            // attr fallback produced before).
                            let resolved_sig = env.ctx.funcs.get(&key).cloned()
                                .or_else(|| env.ctx.get_method(class_name, name));
                            if let Some(sig) = resolved_sig {
                                // (kwargs v1) Keyword→positional mapping for
                                // method calls; without kwargs, a default-aware
                                // ARITY check (methods previously leaked a raw
                                // rustc E0061 on wrong arity — now check-time).
                                let site = format!("{}.{}", class_name, name);
                                let kw_slots: Option<Vec<ArgSlot>> = if !kwargs.is_empty() {
                                    Some(map_kwargs_to_slots(&site, &sig, args.len(), kwargs, *span)?)
                                } else {
                                    let expected = sig.params.len();
                                    let required = sig
                                        .param_defaults
                                        .iter()
                                        .take_while(|d| d.is_none())
                                        .count();
                                    if args.len() < required || args.len() > expected {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "method `{}` takes {} argument(s), {} given",
                                                site, expected, args.len()
                                            ),
                                        });
                                    }
                                    None
                                };
                                // (EPIC-4 V2-c) Enforce the by-reference (`Mut[T]`)
                                // place-requirement at METHOD call sites too (it was
                                // already enforced for free functions in V2-ab). An
                                // arg bound to a by-ref method param must be a PLACE
                                // (Ident/Attr/Index) — a temporary has no
                                // caller-visible storage to borrow `&mut`. We look
                                // up the by-ref flags via get_method, whose vectors
                                // are self-EXCLUSIVE and index-aligned to `args`
                                // (mirrors the resolver alignment fixed in STEP 0).
                                let method_sig = env.ctx.get_method(class_name, name);
                                if let Some(msig) = &method_sig {
                                    // (kwargs v1) The place-requirement covers a
                                    // by-ref param bound by KEYWORD too — the
                                    // provided pairs align each argument (positional
                                    // or keyword value) with its parameter slot.
                                    let by_ref_pairs: Vec<(usize, &Expr)> = match &kw_slots {
                                        None => args.iter().enumerate().collect(),
                                        Some(slots) => kwargs_provided_in_eval_order(args, kwargs, slots),
                                    };
                                    for (i, a) in by_ref_pairs {
                                        if msig.param_by_ref.get(i).copied().unwrap_or(false)
                                            && !is_place_expr(a)
                                        {
                                            let pname = msig.params.get(i)
                                                .map(|(n, _)| n.as_str())
                                                .unwrap_or("<arg>");
                                            return Err(Error::Type {
                                                span: *span,
                                                msg: format!(
                                                    "by-reference parameter `{}` requires a variable, not a temporary",
                                                    pname
                                                ),
                                            });
                                        }
                                    }
                                }
                                for a in args { check_expr(a, env)?; }
                                // (kwargs v1) Keyword VALUES are checked for their
                                // own errors too (undefined names, bad exprs).
                                for (_, v) in kwargs { check_expr(v, env)?; }
                                // Generics v2: the registered sig's return may
                                // contain the class's type vars (`get(self) -> T`).
                                // Substitute the RECEIVER instance's type args
                                // (`b: Box[int]` -> `{T -> int}`) so the call types
                                // concretely (`b.get(): int`). A non-generic / arg-
                                // less receiver yields an empty subst and returns
                                // the ret unchanged.
                                return Ok(subst_class_member(&sig.ret, &obj_ty, env.ctx));
                            }
                        }
                        // (a) Builtin method existence — only on concrete Str/List/Set/Dict.
                        // Skipped for Unknown (unprovable) and Class (handled above).
                        if let Some((type_name, table)) = builtin_method_table(&obj_ty) {
                            if !table.contains(&name.as_str()) {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!("type `{}` has no method `{}`", type_name, name),
                                });
                            }
                            // (b) Detect in-place mutating method calls on a by-value param.
                            // e.g. `visited.add(node)` where `visited` is a Set parameter,
                            // OR `param.field.append(x)` / `param[0].add(x)` — a mutator on
                            // any PLACE rooted at the param (the mutation is lost on the
                            // caller's clone either way). EPIC-4 V2-d closes the former
                            // nested-mutation gap: we now root the receiver via `root_ident`
                            // (like the AttrAssign / IndexAssign backstops already do)
                            // instead of requiring the receiver to be the bare param ident.
                            // `obj_ty` is the RECEIVER's type (the collection being mutated),
                            // which is always owned inside this builtin-method-table arm, so
                            // the `is_owned(&obj_ty)` guard still holds for the field/index
                            // case. The suppressions are preserved verbatim: self-exclusion,
                            // reassigned, returned, and — critically — by_ref (`Mut[T]`)
                            // params, whose nested mutation IS caller-visible and must NOT
                            // fire.
                            if MUTATING_METHODS.contains(&name.as_str()) {
                                if let Some(param_name) = root_ident(obj) {
                                    if param_name != "self"
                                        && env.params.contains(param_name)
                                        && !env.reassigned_params.contains(param_name)
                                        && !env.returned_params.contains(param_name)
                                        && !env.by_ref_params.contains(param_name)
                                        && is_owned(&obj_ty)
                                    {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: by_value_mutation_error(param_name),
                                        });
                                    }
                                }
                            }
                            // (c) Element-type argument check for set mutators only.
                            if let Some(elem_ty) = elem_arg_check_ty(&obj_ty, name) {
                                if let Some(arg0) = args.first() {
                                    let arg_ty = check_expr(arg0, env)?;
                                    let int_to_float =
                                        matches!(arg_ty, Ty::Int) && matches!(elem_ty, Ty::Float);
                                    if !int_to_float
                                        && !matches!(arg_ty, Ty::Unknown)
                                        && !matches!(elem_ty, Ty::Unknown)
                                        && !types_compatible(&arg_ty, &elem_ty, env.ctx)
                                    {
                                        return Err(Error::Type {
                                            span: *span,
                                            msg: format!(
                                                "argument to `{}.{}`: expected element type {}, found {}",
                                                type_name, name, elem_ty, arg_ty
                                            ),
                                        });
                                    }
                                }
                            }
                            for a in args { check_expr(a, env)?; }
                            // (W5-b) bytes-method + str.encode arg/arity validation —
                            // honest CHECK errors for every deferred parameter shape
                            // (design §E iron rule) so no `rustc` leak and each
                            // fail_* negative rejects at `check`. Args are already
                            // checked above, so this reads types via infer_expr_ty.
                            if matches!(obj_ty, Ty::Bytes) {
                                check_bytes_method_call(name.as_str(), args, kwargs, env, *span)?;
                            } else if matches!(obj_ty, Ty::Str) && name == "encode" {
                                check_str_encode_call(args, kwargs, *span)?;
                            }
                            // dict.get is arg-count-aware: get(k) -> Optional[V],
                            // get(k, default) -> V. Route through the shared helper
                            // so the checker and the inference oracle agree; fall
                            // back to builtin_method_ret for every other method.
                            if let Some(t) = dict_get_ret(&obj_ty, name.as_str(), args.len()) {
                                return Ok(t);
                            }
                            return Ok(builtin_method_ret(&obj_ty, name.as_str()));
                        }
                    }
                    // Calling a function VALUE whose callee is an arbitrary
                    // expression (not a bare name or method). Two cases:
                    //  - An inline lambda `(lambda x: body)(args)`: the call's
                    //    value type is the lambda BODY type (computed directly so
                    //    it is unaffected by the Lambda arm now yielding Ty::Func).
                    //  - Any other func-valued callee (`ops["double"](7)`,
                    //    `(make_adder(5))(10)`): the result is the function value's
                    //    return type, surfaced from its `Ty::Func`.
                    let result = if let Expr::Lambda { params, body, .. } = callee.as_ref() {
                        lambda_body_ty(params, body, env)?
                    } else {
                        let callee_ty = check_expr(callee, env)?;
                        // (honest errors) Calling the result of an expression whose
                        // type is a KNOWN non-callable (`xs[0](3)` where `xs:
                        // list[int]`) is a type error, not a deferred rustc E0618.
                        // `Unknown`/`Class` stay permissive. CRUCIAL EXCLUSION: an
                        // `Expr::Attr` callee here is an UNRESOLVED method call
                        // (`m.kind()`, `self.bump()`) that the method-dispatch block
                        // above did not match and let fall through — `check_expr`
                        // returns the method's RETURN type, not the callee's own
                        // type, so the non-callable test would misfire on a method
                        // that returns str/None/etc. Method calls are never the
                        // value-call form this gate targets, so skip them.
                        let is_method_callee = matches!(callee.as_ref(), Expr::Attr { .. });
                        match callee_ty {
                            Ty::Func(_, ret) => *ret,
                            ref t if !is_method_callee && is_noncallable_ty(t) => {
                                return Err(Error::Type {
                                    span: *span,
                                    msg: format!("value of type {} is not callable", callee_ty),
                                });
                            }
                            _ => Ty::Unknown,
                        }
                    };
                    for a in args { check_expr(a, env)?; }
                    result
                }
            }
        }
        Expr::Attr { obj, name, span } => {
            // Qualified MODULE CONSTANT `X.CONST` for a REAL imported module:
            // when X is a tracked module and CONST is one of its module-level
            // constants, the access type-checks as the const's declared type.
            // GENERALIZES the former hardcoded `math.pi` handling (where `math`
            // was a Ty::Unknown placeholder and `math.pi` silently stayed
            // Unknown); `math` is now a real embedded module whose consts are
            // tracked in `module_consts`.
            if let Expr::Ident(modname, _) = obj.as_ref() {
                // (W3-1) OWNER-FIRST qualified const type (flat `module_consts`
                // fallback for synthetic ctxs); both module-keyed, never diverges.
                if let Some(ty) = env.ctx.resolve_module_const(modname, name) {
                    return Ok(ty.clone());
                }
                // (W4-a) Qualified MUTABLE-GLOBAL READ `m.x`: a container/promoted
                // global is absent from `module_consts`, so resolve it here to its
                // declared type. Qualified reads work for free (W3 owner machinery);
                // cross-module WRITES are rejected in the `AttrAssign` arm. Guarded
                // on `m` not being a local so a class-typed local's field of the
                // same name is not intercepted.
                if !env.locals.contains_key(modname) {
                    if let Some(ty) = env.ctx.mutable_global_ty(Some(modname), name) {
                        return Ok(ty.clone());
                    }
                }
                // (Honest-errors) `X.attr` (non-call) where X is a KNOWN imported
                // module (it has tracked functions or constants) but `attr` is
                // neither a constant nor a function of X is an UNKNOWN ATTRIBUTE.
                // Reject it honestly at `check` rather than letting it fall to
                // Ty::Unknown and miscompile at `build` (e.g. `math.inf` — inf/nan
                // are not pyrst constants — would emit a bare `math` and fail rustc
                // E0425). Mirrors the unknown-qualified-FUNCTION rejection on the
                // call path. A known constant returned above; a function name
                // (used as a value) is a separate, deferred feature and is left to
                // fall through unchanged.
                let is_known_module = env.ctx.module_funcs.contains_key(modname)
                    || env.ctx.module_consts.contains_key(modname);
                let is_module_func = env
                    .ctx
                    .module_funcs
                    .get(modname)
                    .is_some_and(|fns| fns.iter().any(|f| f == name));
                if is_known_module && !is_module_func {
                    return Err(Error::Type {
                        span: *span,
                        msg: format!("module `{}` has no attribute `{}`", modname, name),
                    });
                }
            }
            let obj_ty = check_expr(obj, env)?;
            // Generics v1: accessing an attribute of a bare type variable
            // (`t.x` where `t: T`) is rejected — `T` is opaque, with no known
            // fields/attributes (E0609 otherwise). A method CALL on a type var is
            // rejected separately in the Call arm.
            reject_typevar_op(&obj_ty, "access an attribute of", *span)?;
            if let Ty::Class(class_name, _) = &obj_ty {
                if let Some(class_def) = env.ctx.classes.get(class_name.as_str()) {
                    // Check field access (including inherited fields).
                    let all_fields = env.ctx.get_all_fields(class_name.as_str());
                    if let Some(field) = all_fields.iter().find(|f| &f.name == name) {
                        // Generics v2: lower the field annotation with the class's
                        // type params in scope (`value: T` -> `Ty::TypeVar(T)`),
                        // then substitute the RECEIVER instance's type args
                        // (`b: Box[int]` -> `{T -> int}`) so `b.value: int`. A
                        // non-generic class scopes/substitutes with an empty set,
                        // identical to the legacy `from_type_expr` result.
                        let field_ty = Ty::from_type_expr_scoped(&field.ty, *span, &class_def.type_params)?;
                        return Ok(subst_class_member(&field_ty, &obj_ty, env.ctx));
                    }
                    // Check method access (including inherited methods). A bare
                    // method reference's return type substitutes the receiver's
                    // type args too (parity with the method-CALL arm).
                    if let Some(method) = env.ctx.get_method(class_name.as_str(), name) {
                        return Ok(subst_class_member(&method.ret, &obj_ty, env.ctx));
                    }
                    return Err(Error::Type {
                        span: *span,
                        msg: format!("class `{}` has no attribute `{}`", class_name, name),
                    });
                }
            }
            // (card c34ac64a, shape 1b) ATTRIBUTE-CHAIN narrowing is REJECTED, not
            // silently narrowed. Accessing `.{name}` on an Optional value (`o.slot.v`
            // where `o.slot: Optional[Slot]`) may be a None deref; without this it
            // fell to `Ty::Unknown` and leaked a rustc E0609 (`no field on
            // Option<_>`). pyrst deliberately does NOT flow-narrow a FIELD/ATTRIBUTE
            // *place*: unlike a local, a field can be invalidated between the
            // `is not None` guard and the use by ANY intervening call or assignment
            // (`o.mutate()`, `o.slot = None`), so a place-narrowing would be unsound
            // in the general case — the same soundness wall CPython's type-checkers
            // hit. Bind the Optional to a LOCAL first; a local narrows soundly
            // (name-based, no aliasing). Honest error naming that idiom.
            if matches!(&obj_ty, Ty::Option(_)) {
                return Err(Error::Type {
                    span: *span,
                    msg: format!(
                        "attribute `{}` accessed on an Optional value — it may be None. \
                         pyrst does not narrow an attribute/field place (an intervening \
                         call or assignment could invalidate it); bind it to a local \
                         first and narrow the local: `tmp = <the Optional expression>; \
                         if tmp is not None: tmp.{}`",
                        name, name
                    ),
                });
            }
            Ty::Unknown
        }
        Expr::Index { obj, idx, span } => {
            let obj_ty = check_expr(obj, env)?;
            check_expr(idx, env)?;
            // Generics v1: a bare type variable is OPAQUE — it is not known to be
            // a container, so indexing it (`t[i]`) needs a bound and is rejected.
            // (Indexing a `list[T]`/`dict[K, V]` whose ELEMENT is a type var is
            // fine — that yields the element type below.)
            reject_typevar_op(&obj_ty, "index", *span)?;
            // (W5-g) A handle is not indexable.
            reject_handle_op(&obj_ty, "index", *span)?;
            // (LAZY-GEN V1-d) A generator is single-pass with no random access —
            // `g[i]` is a `TypeError` in CPython too. Honest MATERIALIZE error
            // (closes the Index-vs-Slice asymmetry from review comment 123: Index
            // fell to `Ty::Unknown` with no pyrst error while Slice already errored).
            if matches!(obj_ty, Ty::Iterator(_)) {
                return Err(iterator_materialize_error(
                    "is not subscriptable (no random access)", "list(g)[i]", *span));
            }
            // (CARD a40d603e) A tuple compiles to a fixed-size, heterogeneous Rust
            // tuple — only a LITERAL integer index has a single well-defined
            // field type, and codegen's `emit_expr` Index arm only lowers that
            // literal-int-on-Tuple shape (to `.N` field access). Python allows a
            // dynamic `t[i]` (raising `IndexError` at runtime for an out-of-range
            // `i`), but pyrst cannot: reject a non-literal or out-of-range index
            // here with a clear pyrst-level message instead of leaking the raw
            // rustc error a non-literal index previously fell through to (the
            // list-indexing path applied to a non-`Vec` tuple field).
            if let Ty::Tuple(elems) = &obj_ty {
                return match idx.as_ref() {
                    Expr::Int(n, _) if *n >= 0 && (*n as usize) < elems.len() => {
                        Ok(elems[*n as usize].clone())
                    }
                    Expr::Int(n, _) => Err(Error::Type {
                        span: *span,
                        msg: format!(
                            "tuple index {} out of range for a {}-element tuple",
                            n, elems.len()
                        ),
                    }),
                    _ => Err(Error::Type {
                        span: *span,
                        msg: "tuple index must be a literal integer (e.g. `t[0]`); \
                              pyrst compiles a tuple to a fixed-size Rust tuple, so a \
                              dynamic index (`t[i]`) has no single field type — Python \
                              allows this at runtime but pyrst cannot"
                            .to_string(),
                    }),
                };
            }
            match obj_ty {
                Ty::List(inner) => *inner,
                Ty::Dict(_, v) => *v,
                Ty::Str => Ty::Str,
                // (W5-a) `b[i]` -> int (u8 as i64) — see the oracle Index arm.
                Ty::Bytes => Ty::Int,
                _ => Ty::Unknown,
            }
        }
        Expr::Slice { obj, start, stop, step, span } => {
            let obj_ty = check_expr(obj, env)?;
            // Generics v1: a bare type variable is OPAQUE — slicing it (`t[a:b]`)
            // needs a slice/Index bound and is rejected (mirrors the Index arm).
            reject_typevar_op(&obj_ty, "slice", *span)?;
            // (W5-g) A handle is not sliceable.
            reject_handle_op(&obj_ty, "slice", *span)?;
            // (LAZY-GEN V1-d) Slicing a generator is a `TypeError` in CPython too.
            // Honest MATERIALIZE error at `check` time (previously only a codegen
            // error fired, so `pyrst check` leaked it — review comment 123).
            if matches!(obj_ty, Ty::Iterator(_)) {
                return Err(iterator_materialize_error(
                    "cannot be sliced (no random access)", "list(g)[a:b]", *span));
            }
            // Validate slice indices are integers
            for e in &[start.as_ref(), stop.as_ref(), step.as_ref()] {
                if let Some(e) = e {
                    let ty = check_expr(e, env)?;
                    if !matches!(ty, Ty::Int | Ty::Unknown) {
                        return Err(Error::Type {
                            span: e.span(),
                            msg: "slice indices must be integers".into(),
                        });
                    }
                }
            }
            // Slicing a list/string returns the same type
            match obj_ty {
                Ty::List(inner) => Ty::List(inner),
                Ty::Str => Ty::Str,
                // (W5-a) `b[i:j]` -> bytes (a sub-`Vec<u8>`).
                Ty::Bytes => Ty::Bytes,
                _ => Ty::Unknown,
            }
        }
        Expr::BinOp { op, lhs, rhs, span } => {
            let l = check_expr(lhs, env)?;
            let r = check_expr(rhs, env)?;
            // (LAZY-GEN V1-d) A lazy generator has no binary-operator form in V1 —
            // `g + g` / `g * n` would need a materialized list, and membership
            // `x in g` (valid in Python but DRAINS the generator) has no lazy analog
            // until V2. Reject any binop/membership with a generator operand, with
            // the op-appropriate materialize fix (docs/design/lazy-generators.md §D.2).
            if matches!(l, Ty::Iterator(_)) || matches!(r, Ty::Iterator(_)) {
                return Err(match op {
                    BinOp::In | BinOp::NotIn => iterator_materialize_error(
                        "has no membership test (`in` would drain it; lazy membership \
                         arrives in V2)",
                        "x in list(g)", *span),
                    // Show the fix with the ACTUAL operator (`g * 2` -> `list(g) * 2`,
                    // `g == g2` -> `list(g) == list(g2)`), not a hard-coded `+`.
                    _ => iterator_materialize_error(
                        "has no binary-operator form (an operator would consume it)",
                        &format!("list(g) {} xs", binop_symbol(*op)), *span),
                });
            }
            // (enabler-fix-2 #2) Membership over a fixed-shape TUPLE is an honest
            // CHECK error: a pyrst tuple lowers to a Rust tuple with no `.contains`
            // (the old emission was a rustc E0599). Direct to a list or destructure,
            // mirroring the for-in tuple rejection. An empty `Ty::Tuple` is the
            // unknown-shape placeholder, left permissive.
            if matches!(op, BinOp::In | BinOp::NotIn)
                && matches!(&r, Ty::Tuple(tys) if !tys.is_empty())
            {
                return Err(Error::Type {
                    span: *span,
                    msg: "membership (`in`) over a tuple is not supported — test \
                          against a list (`x in [a, b, c]`) or destructure the tuple"
                        .to_string(),
                });
            }
            // Generics v2: a SUPPORTED binary operator on two values of the SAME
            // type variable (`T op T`) is now LEGAL — codegen emits the inferred
            // trait bound (`PartialOrd` / `PartialEq` / `Add<Output=T>` / ...) in
            // the generic clause. An UNSUPPORTED op on a bare `T` (membership,
            // boolean, bitwise, `**`, `//`), or a MIXED `T op concrete` /
            // `T op differentU`, stays an honest rejection. The `op_desc`
            // distinguishes comparison from arithmetic so the message reads
            // naturally.
            let op_desc = match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => "compare",
                BinOp::In | BinOp::NotIn => "test membership of",
                _ => "apply an operator to",
            };
            // (W5-g) A handle has NO operators — no `==`/`!=` (probe: `PyFile` has no
            // PartialEq -> rustc E0277), no ordering, no arithmetic, no membership.
            // `bytes == str` etc. taught the lesson: reject explicitly here instead of
            // riding the loose generic path into a rustc wall. Names the handle kind.
            reject_handle_op(&l, op_desc, *span)?;
            reject_handle_op(&r, op_desc, *span)?;
            // The single supported shape is `T op T` of the SAME variable with a
            // mapped bound. Recognise it first; anything else with a TypeVar
            // operand falls through to the v1 rejection.
            let same_typevar = matches!((&l, &r), (Ty::TypeVar(a), Ty::TypeVar(b)) if a == b);
            let supported_typevar_op = same_typevar && binop_typevar_bound(*op).is_some();
            // Generics v2: membership where the CONTAINER (rhs) is a known
            // `dict`/`set`/`list` and the ELEMENT/key (lhs) is a TypeVar is a
            // VALID, bound-inferable op — `k in d` infers `K: Hash + Eq`
            // (dict/set) or `K: PartialEq` (list), mirroring `infer_bounds_expr`.
            // Only `x in t` where `t` itself is a BARE TypeVar (an unknown
            // container) stays rejected by the bare-T sweep below.
            let container_membership = matches!(op, BinOp::In | BinOp::NotIn)
                && matches!(r, Ty::Dict(..) | Ty::Set(_) | Ty::List(_));
            if !supported_typevar_op && !container_membership {
                reject_typevar_op(&l, op_desc, *span)?;
                reject_typevar_op(&r, op_desc, *span)?;
            }
            // (EPIC-5) Reject using a raw `Optional[T]` operand without narrowing.
            // An Option only supports identity/equality testing against `None`
            // (`is` / `is not` / `==` / `!=`); any other operator (arithmetic,
            // ordering, membership, boolean) on an un-narrowed Optional is an
            // honest error — the value must be narrowed via `is None` /
            // `is not None` first (see PYTHON_COMPATIBILITY.md, Optional section).
            // Without this, `x + 1` on an `Optional[int]` would infer `Unknown`
            // and silently slip through, then miscompile.
            let nullary_ok = matches!(op, BinOp::Is | BinOp::IsNot | BinOp::Eq | BinOp::Ne);
            if !nullary_ok && (matches!(l, Ty::Option(_)) || matches!(r, Ty::Option(_))) {
                return Err(Error::Type {
                    span: *span,
                    msg: format!(
                        "operator on an Optional value requires narrowing first: \
                         use `if x is not None:` to obtain the inner value before applying `{:?}`",
                        op
                    ),
                });
            }
            // Generics v2: type the result of a SUPPORTED `T op T`. Comparison /
            // equality yield `bool`; the supported arithmetic ops (`+ - *`) yield
            // `T` (the same-type rule, matching the emitted `Add`/`Sub`/`Mul<Output
            // = T>` bound). This explicit redirect fires ONLY for the recognised
            // same-`T` shape; concrete operands keep the Python rules below.
            if supported_typevar_op {
                return Ok(match op {
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Ty::Bool,
                    // Add/Sub/Mul on `T op T` -> `T`.
                    _ => l,
                });
            }
            // (W0-a, honesty hole p05) `str % x` is Python %-formatting, which
            // pyrst does not implement. codegen would emit Rust `String % _`
            // (E0369) — a check-passes / build-fails leak. Reject it honestly
            // here with the f-string fix. GUARDED on a `Str` lhs so integer /
            // float modulo (`a % b`) is completely untouched.
            if *op == BinOp::Mod && matches!(l, Ty::Str) {
                return Err(Error::Type {
                    span: *span,
                    msg: "string %-formatting (`\"...\" % x`) is not supported; \
                          use an f-string instead, e.g. f\"{x}\"".to_string(),
                });
            }
            // (W0-c, p20) Set algebra: `&`/`|`/`^`/`-` over sets yield the set
            // type (see `set_binop_result_elem`). MUST precede the blanket
            // bitwise->`Int` rule below (correct only for integer bit-twiddling)
            // and the same-type arithmetic rule (which typed `set - set` but not
            // `set & set`); makes the former flow.rs `reassign_value_ty` set
            // special-case dead. A set with a non-set, or conflicting element
            // types, falls through to the honest rejection below.
            if is_set_algebra_op(*op) {
                if let Some(elem) = set_binop_result_elem(&l, &r) {
                    return Ok(Ty::Set(Box::new(elem)));
                }
            }
            // (enabler-fix-1 #1) An ORDERING comparison (`< <= > >=`) over a
            // USER-CLASS operand REQUIRES a defined `__lt__` (base chain). Without
            // it CPython raises `TypeError: '<' not supported between instances`,
            // yet pyrst accepted `Node(1) < Node(2)` and ran it (a dict-key class
            // derives `Ord` from field-declaration order in codegen/items.rs; a
            // non-key class leaked rustc E0277). Independent of hash-key status.
            // `==`/`!=` are unaffected (structural `PartialEq`); `Optional` operands
            // were already rejected above (narrow first).
            if matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) {
                for operand in [&l, &r] {
                    if let Ty::Class(cn, _) = operand {
                        if env.ctx.get_method(cn, "__lt__").is_none() {
                            return Err(Error::Type {
                                span: *span,
                                msg: format!(
                                    "class `{}` does not support `<`/`<=`/`>`/`>=` \
                                     comparison: define a `__lt__` method (Python raises \
                                     `TypeError: '<' not supported between instances of '{}'`)",
                                    cn, cn
                                ),
                            });
                        }
                    }
                }
            }
            // (W5-a) EXPLICIT bytes-operator typing — MUST precede the generic
            // arms below so `bytes + str` / `bytes == str` are honest CHECK errors
            // instead of riding the loose path to a `rustc` failure (PN1/PN2).
            if is_bytes_binop(*op, &l, &r) {
                return bytes_binop_ty(*op, &l, &r, *span);
            }
            match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le
                | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or
                | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => Ty::Bool,
                BinOp::Pow | BinOp::Div => Ty::Float,  // Division always returns float in Python
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::LShift | BinOp::RShift => Ty::Int,
                _ => {
                    // Arithmetic: apply numeric type promotion rules
                    match (&l, &r) {
                        // Operator overloading: a class lhs dispatches to the
                        // declared return type of its dunder (__add__/__sub__/__mul__).
                        (Ty::Class(cls, _), _) => {
                            let dunder = match op {
                                BinOp::Add => Some("__add__"),
                                BinOp::Sub => Some("__sub__"),
                                BinOp::Mul => Some("__mul__"),
                                _ => None,
                            };
                            dunder.and_then(|d| env.ctx.get_method(cls, d))
                                .map(|s| s.ret.clone())
                                .unwrap_or_else(|| l.clone())
                        }
                        // Same type: return that type
                        (a, b) if a == b => l,
                        // Mixed numeric types: promote to float
                        (Ty::Int, Ty::Float) | (Ty::Float, Ty::Int) => Ty::Float,
                        // String + String = String (for concatenation)
                        (Ty::Str, Ty::Str) => Ty::Str,
                        // List + List = List (for concatenation)
                        (Ty::List(inner_l), Ty::List(inner_r)) if inner_l == inner_r => Ty::List(inner_l.clone()),
                        // Otherwise unknown
                        _ => Ty::Unknown,
                    }
                }
            }
        }
        Expr::UnOp { op, expr, span } => {
            let t = check_expr(expr, env)?;
            // Generics v1: a unary operator on a bare type variable is rejected
            // (needs `Neg`/`Not` bounds, out of v1 scope).
            reject_typevar_op(&t, "apply a unary operator to", *span)?;
            match op {
                // (Z4, card 2b37b965) `not <Optional>` is a truthiness use —
                // reject a bare Optional here (check/build agreement). Neg/BitNot
                // are arithmetic, not truthiness, so they are untouched.
                UnOp::Not => { reject_optional_truthiness(&t, *span)?; Ty::Bool }
                UnOp::Neg => t,
                UnOp::BitNot => Ty::Int,
            }
        }
        Expr::Lambda { params, body, .. } => {
            // The lambda's value type is its first-class function type
            // `Callable[[unknown, ...], body_ty]`. Checking the body in a child
            // env (params bound to Unknown) both validates the body for its own
            // errors and yields the return type. Returning a `Ty::Func` (rather
            // than the bare body type) is what lets a lambda flow into a declared
            // `Callable` slot — assignment, argument, return, and dict/list value.
            // The two inline-call paths (the Ident-callee Lambda branch and the
            // `_`-callee branch in the Call arm) compute the body type DIRECTLY,
            // so they are unaffected by this change.
            let body_ty = lambda_body_ty(params, body, env)?;
            Ty::Func(vec![Ty::Unknown; params.len()], Box::new(body_ty))
        }
    })
}

/// Type-check a lambda body in a child environment with each parameter bound to
/// `Unknown` (pyrst lambda params are unannotated), returning the body's type.
/// Shared by the `Expr::Lambda` value arm (which wraps it in `Ty::Func`) and the
/// inline-invocation call paths (which surface the body type as the call result).
/// (W5-g, H2) Reject a lambda that CAPTURES an outer move-only handle. A lambda
/// lowers to a `move` closure whose non-Copy captures are snapshotted by CLONE —
/// but a handle is non-`Clone`, so the emitted `f.clone()` fails rustc E0599 (the
/// same hole as the old `Ty::File`, and the nested-`def` capture gate). Unlike a
/// comprehension (which lowers to a borrowing iterator adaptor), a lambda cannot
/// alias a handle at all in v1. The lambda's OWN params are excluded (they shadow
/// any enclosing name and are never handle-typed). Shared by every lambda-checking
/// entry (`lambda_body_ty` and the map/filter `lambda_ret_with_elem`) so no lambda
/// shape can smuggle a handle capture past `check` into a rustc wall.
pub(crate) fn reject_lambda_handle_capture(
    params: &[(String, TypeExpr)],
    body: &Expr,
    env: &FuncEnv,
) -> Result<()> {
    let mut reads: std::collections::HashSet<String> = std::collections::HashSet::new();
    expr_reads(body, &mut reads);
    for (p, _) in params { reads.remove(p); }
    for name in &reads {
        if let Some(kind) = env.locals.get(name).and_then(|t| t.handle_name()) {
            return Err(Error::Type {
                span: body.span(),
                msg: format!(
                    "the `{kind}` handle `{name}` cannot be captured by a lambda — a \
                     move-only handle is non-clonable, so it cannot be snapshotted into a \
                     closure (v1 handles are move-only); operate on the handle outside the \
                     lambda, or use a named function that takes it as a parameter",
                ),
            });
        }
    }
    Ok(())
}

pub(crate) fn lambda_body_ty(
    params: &[(String, TypeExpr)],
    body: &Expr,
    env: &mut FuncEnv,
) -> Result<Ty> {
    // (W5-g, H2) A lambda cannot capture a move-only handle (clone-on-capture is
    // non-`Clone`); reject before checking the body.
    reject_lambda_handle_capture(params, body, env)?;
    let mut lambda_env = FuncEnv {
        ctx: env.ctx,
        locals: env.locals.clone(),
        ret_ty: Ty::Unknown,
        used_vars: env.used_vars.clone(),
        params: std::collections::HashSet::new(),
        reassigned_params: std::collections::HashSet::new(),
        returned_params: std::collections::HashSet::new(),
        by_ref_params: std::collections::HashSet::new(),
        // A lambda body is a single expression — never a generator.
        is_generator: false,
        // A lambda's params are its own; enclosing type variables don't apply.
        type_params: std::collections::HashSet::new(),
        // A lambda has no statement body, so it declares no `global`s.
        globals_declared: std::collections::HashSet::new(),
        // A lambda body is a single expression — narrow-tracking is never used.
        narrowed: std::collections::HashMap::new(),
        // (W4-a) Inherit the enclosing function's owning module.
        module_id: env.module_id.clone(),
        // (W5-g) Inherit handle liveness (see the comprehension envs).
        moved: env.moved.clone(),
        loop_handles: env.loop_handles.clone(),
    };
    for (param_name, param_ty) in params {
        let ty = lambda_param_ty(param_ty);
        lambda_env.locals.insert(param_name.clone(), ty);
    }
    check_expr(body, &mut lambda_env)
}

// =============================================================================
// UNIT TESTS
