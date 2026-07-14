# terminal — cross-platform terminal-control essentials for pyrst (curses-lite)

An **essentials** terminal-control library so pyrst CLI programs can be
interactive: raw-mode input, cursor positioning, colours, and flicker-free live
redraw on an alternate screen. It is a thin, pyrst-native wrapper around the
pure-Rust, cross-platform [`crossterm`](https://crates.io/crates/crossterm)
crate (0.28), bound through pyrst's `@crate` + `@extern` inline-Rust mechanism
(the same plumbing `re` uses for `regex`). Because `crossterm` is cross-platform
by construction (Unix termios + the Windows console, VT enabled automatically),
this package is **Windows-compatible from day one** — every `@extern` snippet uses
only crossterm's platform-independent API, with **no `std::os::unix` and no
`#[cfg]`** anywhere.

> A program that imports `terminal` auto-builds as a Cargo project (the driver
> collects the `@crate` dependency), so `crossterm` is fetched and linked for you.

---

## Coordinate convention

**`(x, y) = (col, row)`, 0-based, origin at the top-left.**

This is the **opposite order from Python's `curses`** (which is `(y, x)`); it
matches crossterm's own `MoveTo(col, row)`. Everywhere this package takes
coordinates, `x` is the column and `y` is the row.

---

## Restoring the terminal (read this)

pyrst does **not** support the context-manager protocol on user-defined classes
(`with Screen() as s:` is a compile error — only `with open(...)` is a context
manager in pyrst today). So `Screen` uses explicit `init()` / `close()`. Wrap the
session in **`try: ... finally: s.close()`** so the terminal is always restored:

```python
from terminal import Screen

def main() -> None:
    s: Screen = Screen()
    s.init()                 # raw mode + alternate screen + hidden cursor
    try:
        while True:
            s.clear()
            s.addstr(2, 1, "hello — press q to quit")
            s.refresh()
            k: str = s.getch()
            if k == "q" or k == "ctrl+c" or k == "esc":
                break
    finally:
        s.close()            # ALWAYS restores: raw mode off, leaves the alt
                             # screen, shows the cursor
    print("bye")
```

`close()` is **idempotent** and best-effort (it never itself raises). pyrst runs a
`finally` block even when an exception propagates **uncaught** (an uncaught pyrst
exception is a panic, and the `finally` still runs before it surfaces), so this
pattern restores the terminal on a normal quit, on **Ctrl+C** (see below), **and**
on any error mid-session. A crashed program will not leave your terminal in raw
mode.

If `init()` cannot enable raw mode (e.g. stdout is not a terminal), it raises an
honest `OSError`-shaped exception **before** entering the alternate screen, so
there is nothing to restore.

---

## API

Coordinates are `(x, y) = (col, row)`. Import the `Screen` class and the colour
constants from `terminal`.

### Session lifecycle
| Call | Effect |
|------|--------|
| `Screen()` | Construct a session handle (does not touch the terminal yet). |
| `s.init()` | Enter: enable raw mode, switch to the alternate screen, hide the cursor. Raises `OSError` if raw mode can't be enabled (not a tty). |
| `s.close()` | Exit + FULL restore: leave the alternate screen, show the cursor, reset colours, disable raw mode. Idempotent, best-effort. |

### Drawing (queued; call `refresh()` to present a frame)
| Call | Effect |
|------|--------|
| `s.clear()` | Clear the screen and home the cursor to `(0, 0)`. |
| `s.move_to(x, y)` | Move the cursor to `(col, row)`. |
| `s.write(str)` | Write text at the current cursor position. |
| `s.addstr(x, y, str)` | `move_to(x, y)` then `write(str)` (the common combined op). |
| `s.refresh()` | Flush the queued frame to the terminal (call once per frame). |

Draw calls are buffered and only appear on `refresh()`, so you build a whole frame
then present it — no flicker.

### Style
| Call | Effect |
|------|--------|
| `s.set_fg(color)` | Set the foreground colour (a colour constant, `0..15`). |
| `s.set_bg(color)` | Set the background colour. |
| `s.bold(on)` | Bold on/off (`on: bool`). |
| `s.reverse(on)` | Reverse-video on/off. |
| `s.reset_style()` | Reset all colours and attributes to the default. |

Colour constants (module-level): `BLACK RED GREEN YELLOW BLUE MAGENTA CYAN WHITE`
(`0..7`) and the bright variants `BRIGHT_BLACK BRIGHT_RED BRIGHT_GREEN
BRIGHT_YELLOW BRIGHT_BLUE BRIGHT_MAGENTA BRIGHT_CYAN BRIGHT_WHITE` (`8..15`). They
map to crossterm's named colours (the widely-supported basic SGR codes).

### Input
| Call | Returns |
|------|---------|
| `s.getch()` | Blocks for one key; returns its name (see below). |
| `s.getch_timeout(ms)` | Waits up to `ms` ms; returns the key name, or `""` if no key arrived (non-blocking — essential for games/animation). |

**Key names.** A printable character returns that character (e.g. `"a"`, `"7"`,
`"+"`). The space bar returns `"space"`. Ctrl + a letter returns
`"ctrl+<letter>"` — so **Ctrl+C is delivered as `"ctrl+c"`** (raw mode does not
turn it into SIGINT), letting your program catch it and quit cleanly instead of
being un-quittable. Special keys return their names:

`"up" "down" "left" "right" "enter" "esc" "backspace" "tab" "backtab" "home"
"end" "pageup" "pagedown" "delete" "insert" "f1".."f12"`.

(Key **release** events, which the Windows console emits but Unix does not, are
dropped so a keypress never double-reports — behaviour is identical on both OSes.)

### Size & cursor
| Call | Effect |
|------|--------|
| `s.size()` | Returns `(cols, rows)`. |
| `s.show_cursor(on)` | Show (`True`) or hide (`False`) the cursor. |

---

## Building against this package

```sh
PYRST_PATH=/path/to/extern/packages \
  /path/to/target/release/pyrst build your_program/main.pyrs
./your_program/main
```

See `extern/programs/terminal_demo/` for a complete worked example: an interactive
function plotter that types a function of `x`, live-plots it to a character grid on
the alternate screen, pans with the arrow keys, zooms with `+`/`-`, and quits on
`q` / `Esc` / `Ctrl+C` — restoring the terminal cleanly in every case.
