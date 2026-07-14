# graphcalc

A terminal graphing calculator ("Desmos-lite"), written in pyrst. Card
d949c3e7 (epic 47cafe10, Track A); retrofitted to a LIVE interactive UI on
card 4cb345cf. Enter functions of `x` and watch them plotted as a character
grid that redraws live as you pan, zoom, and edit — built on the `terminal`
package (`extern/packages/terminal/`).

## Build + run

graphcalc imports the `terminal` package, so build it with `PYRST_PATH`
pointing at the packages directory (the driver collects the `crossterm`
`@crate` dependency and auto-builds a Cargo project):

```sh
PYRST_PATH=/path/to/pyrst/extern/packages \
  /path/to/pyrst/target/release/pyrst build extern/programs/graphcalc/main.pyrs
./main
```

`./main` launches a full-screen, live-redrawing plotter on the alternate
screen. It needs a real terminal (a TTY) — run it directly in your terminal.
When stdin/stdout is not a terminal (piped or redirected — as CI or the smoke
test runs it), graphcalc prints an honest one-line "needs a TTY" message and
exits 0 rather than crashing.

## Interactive controls

Everything is live: each keypress redraws the plot to the full terminal size.

| Key | Effect |
|---|---|
| arrow keys | Pan the view window (left/right on x, up/down on y). |
| `+` / `=` | Zoom in (both axes, about the centre). |
| `-` / `_` | Zoom out. |
| `f` | Add a function of `x` — a text-entry screen; type the expression and press Enter (Esc cancels). A parse error is shown inline so you can fix it. Each function gets a distinct glyph and colour. |
| `d` | Delete the last function. |
| `c` | Clear all functions and reset the window. |
| `t` | Toggle the value-table view (`x, y` samples for the last function over the current x-window). |
| `a` | Re-enable y auto-fit (the y-range auto-scales to the data until you pan/zoom y). |
| `r` | Reset the view window to the default. |
| `q` / `Esc` / `Ctrl+C` | Quit — the terminal is always fully restored. |

On startup graphcalc opens the add-function text entry so you can plot your
first function immediately (Esc there starts with an empty plot).

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

**Not a TTY?** The interactive UI needs raw mode, which requires a real
terminal. When run with a non-terminal stdin/stdout (piped, redirected, under
CI, or by `tests/smoke_main.pyrs`), `terminal`'s `Screen.init()` raises an
`OSError`; graphcalc catches it, prints a one-line "needs a TTY" message, and
exits 0 — so it degrades honestly instead of crashing or hanging.

## Layout

- `main.pyrs` — the interactive I/O layer (the ONLY file changed by the
  card-4cb345cf retrofit). It drives the existing pure `render(...)` /
  `make_table(...)` onto a `terminal.Screen`: `init()`, then
  `try: run(s) finally: s.close()` so the terminal always restores (on quit,
  Ctrl+C, and uncaught error); a per-keypress live-redraw loop that sizes the
  grid from `Screen.size()`; and a key-by-key text-entry screen for functions
  (raw mode has no `input()`). Contains no plotting math of its own.
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
normalizes domain errors to `nan`, `rasterizer` renders the ASCII grid with
auto-scaled y and a sample table, and `main` drives that renderer live on a
`terminal.Screen` (pan/zoom/edit/table, always-restore). Verified by the pure
golden tests (`test_parser`/`test_evaluator`/`test_rasterizer`), the
`tests/smoke_main.pyrs` non-TTY smoke (exit 0), a pty-driven interactive run
(clean alt-screen enter/leave + cursor restore on `q` and Ctrl+C), and a
Windows cross-check (`cargo check --target x86_64-pc-windows-gnu` on the
emitted crate — `crossterm` is cross-platform). See cards d949c3e7 (original)
and 4cb345cf (interactive retrofit) for the design logs.

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
- Live redraw + responsive: the interactive UI redraws on every keypress and
  sizes the char grid to the current terminal via `Screen.size()` (no fixed
  76x30). The pure renderer is unchanged — `main.pyrs` just calls it with the
  computed width/height each frame — so the golden tests, which pin explicit
  dimensions, are unaffected.
- Beyond pyrst stdlib (`math`; `subprocess` for the smoke test), the
  interactive UI depends only on the local `terminal` package
  (`extern/packages/terminal/`, a cross-platform `crossterm` wrapper) — still
  no compiler (`src/`/`lib/`) changes. Language gaps are logged on the cards
  prefixed `GAP:`.
