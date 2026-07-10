# graphcalc

A terminal graphing calculator ("Desmos-lite"), written in pyrst. Card
d949c3e7 (epic 47cafe10, Track A). Enter a function of `x`, see it plotted
as an ASCII character grid in your terminal.

## Build + run

```sh
cd extern/programs/graphcalc
../../../target/release/pyrst build main.pyrs
./main
```

(or, with a debug/dev build of the compiler: `pyrst build main.pyrs` from
this directory if `pyrst` is on `PATH`.)

You'll get a REPL:

```
graphcalc — a terminal graphing calculator. Type 'help' for commands, 'quit' to exit.
graphcalc>
```

## Command reference

| Command | Effect |
|---|---|
| `plot <expr>` | Replace the current plot set with a single function of `x`, then draw it. |
| `add <expr>` | Add another function to the current plot set (distinct glyph), then redraw. |
| `window xmin xmax [ymin ymax]` | Set the view window (`ymin`/`ymax` optional — auto-scale when omitted), then redraw. |
| `clear` | Remove all plotted functions. |
| `table <expr>` | Print a sample table (x, y pairs) for one function over the current x-window. |
| `help` | List these commands. |
| `quit` | Exit graphcalc. |

**Expression grammar:** `+ - * / **`, parens, unary minus, the variable `x`,
constants `pi`/`e`, and the functions `sin cos tan sqrt abs log ln exp floor
ceil`. See `expr_parser.pyrs` for the full grammar comment.

**Default window:** x in `[-10, 10]`, y auto-scaled to the sampled values
(or a symmetric fallback range when nothing is plotted yet).

**Domain errors and discontinuities never crash the REPL.** A sample where
the function is undefined (`sqrt(-1)`, `log(0)`, `1/0`) is skipped or
clipped, not treated as a fatal error — see `evaluator.pyrs`'s module
docstring for exactly how pyrst's `math` module surfaces these (a catchable
`ValueError`, not a silent `nan`) and how the evaluator normalizes that.

**Blank line / EOF exits the REPL**, exactly like `quit` — see main.pyrs's
module docstring for why (pyrst's `input()` does not raise on EOF).

## Layout

- `main.pyrs` — REPL entry point: reads a line via `input()`, and
  `dispatch(state, line) -> bool` (a plain function taking/returning
  ordinary values, not tangled up in the I/O loop) does the command
  parsing/routing, so it's directly callable from a test without going
  through stdin.
- `expr_parser.pyrs` — tokenizer (`tokenize`) + recursive-descent parser
  (`parse`) producing a flat `Expr` AST (`kind`-tagged, `args: list[Expr]`
  children — no `Optional[Self]` boxing needed). Pure functions; parse
  errors raise `ValueError` naming the offending token/column.
- `evaluator.pyrs` — `eval_expr(node, x) -> float` walks the `Expr` tree at
  a given `x`; `sample(node, xs) -> list[float]` is the vectorized form the
  rasterizer and `table` command both use. Pure; never raises (domain
  errors are caught internally and normalized to `float('nan')`).
- `rasterizer.pyrs` — `render(specs, window, width, height) -> str` draws
  one or more `PlotSpec`s (each an `Expr` + source text + glyph) to a fixed
  76x30 ASCII character grid with axes; `make_table(...) -> str` builds a
  plain-text sample table. Both are pure string-returning functions (no
  printing), so they're testable against golden strings.
- `tests/` — plain pyrst programs that print `PASS`/`FAIL` and exit nonzero
  on failure. `tests/smoke_main.pyrs` builds nothing itself (it's a pyrst
  program, not a shell script) — see its header for how it's invoked and
  for the `subprocess` stdin-piping GAP it works around (same pattern as
  the sibling `tabletop` dogfood project).

**Status:** fully implemented. `expr_parser` tokenizes + recursive-descent
parses (with column-pointing parse errors), `evaluator` walks the AST and
normalizes domain errors to `nan`, `rasterizer` renders the 76x30 ASCII grid
with auto-scaled y and a sample table, and `main`'s REPL wires every command
(`plot`/`add`/`window`/`clear`/`table`/`help`/`quit`). Verified by piped
sessions plotting `sin(x)`, `x**2`, `1/x`, `sqrt(x)`, `-x**2`, `tan(x)` and
multi-function overlays. See card d949c3e7 for the design log and language
gaps.

**Operator precedence note:** unary minus binds *looser* than `**`
(Python-faithful), so `-x**2` is `-(x**2)` — a downward parabola — not
`(-x)**2`. This diverges from the original scaffold grammar sketch (which put
unary minus above `**`); the change is deliberate, since the wrong precedence
would silently flip the single most common calculator input.

## Design notes

- Rendering picks ONE honest default (plain ASCII glyphs, not a
  unicode-braille/block density mode) rather than a runtime toggle, so the
  same window+function always produces the exact same grid string — the
  property golden tests rely on.
- Multiple simultaneous functions get distinct single-character glyphs,
  round-robined from a fixed set; a plot never crashes on domain errors —
  worst case a function's line has gaps (nan) or runs off an edge (inf).
- No live redraw: each command prints its plot once. No window-size
  dependence beyond the fixed 76x30 grid.
- Only pyrst stdlib is used (`math`, `subprocess` for the smoke test) — no
  compiler (`src/`/`lib/`) changes. Language gaps encountered along the way
  are logged on card d949c3e7 prefixed `GAP:`.
