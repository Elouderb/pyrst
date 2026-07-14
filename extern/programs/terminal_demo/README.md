# terminal_demo — interactive function plotter

A compact interactive demo of the [`terminal`](../../packages/terminal/) package:
type a function of `x`, live-plot it to a character grid on the alternate screen,
pan with the arrow keys, zoom with `+`/`-`, re-enter the function with `f`, and
quit with `q` / `Esc` / `Ctrl+C`.

It exercises **every** `terminal` API call — the full `Screen` lifecycle, all the
drawing and style calls, both blocking `getch()` (function entry) and non-blocking
`getch_timeout()` (the animated sweep marker), `size()`, `show_cursor()`, and the
colour constants — and the terminal is fully restored on every exit path (normal
quit, Ctrl+C, or an uncaught error), because the session runs inside
`try: ... finally: s.close()`.

## Build & run

```sh
PYRST_PATH=/home/ethos/Coding/pyrst/extern/packages \
  /home/ethos/Coding/pyrst/target/release/pyrst build main.pyrs
./main
```

## Controls

| Key | Action |
|-----|--------|
| type + `Enter` | enter a function of `x` (e.g. `sin(x)`, `x*x - 3`, `sqrt(x)+1`, `2^x`, `x*sin(x)`) |
| arrow keys | pan the view |
| `+` / `-` | zoom in / out |
| `f` | re-enter the function |
| `q` / `Esc` / `Ctrl+C` | quit (restores the terminal) |

Supported expression grammar: `+ - * /`, `^` (power), parentheses, unary minus,
the variable `x`, the constant `pi`, and the functions
`sin cos tan sqrt exp log abs`.
