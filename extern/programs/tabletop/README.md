# tabletop

A terminal tabletop-game sim, written in pyrst. Card f1ac8d46 (epic
47cafe10, Track A). All games are player-vs-CPU.

## Build + run

```sh
cd extern/programs/tabletop
PYRST_PATH=../../packages ../../../target/release/pyrst build main.pyrs
./main
```

`PYRST_PATH` points the build at `extern/packages` so the `terminal` package
resolves — the menu, checkers, and chess all import it, and a program that
imports `terminal` auto-builds as a Cargo project so `crossterm` is fetched and
linked. (With a dev build of the compiler on `PATH`:
`PYRST_PATH=../../packages pyrst build main.pyrs` from this directory.)

You'll get a **live menu** (card 73098a55, PASS 2) drawn on the
[`terminal`](../../packages/terminal/) alternate screen:

```
1) Blackjack     (player + 2 CPU seats)
2) Texas Hold'em (player + 2 CPU seats)
3) Checkers      (vs CPU)
4) Chess         (vs CPU)
5) Quit
```

The whole app is now **one shared `terminal` session**: `main` opens the
alternate screen once, runs the menu on it, and dispatches every game onto the
**same** Screen (so entering/leaving a game never re-opens its own screen). Move
the highlight with the **up/down arrows** and press **Enter**, or press a **number
key 1-5** to jump straight to a game. **q / Esc / Ctrl+C** quits the app. Because
the live menu needs a real terminal, running it with piped/redirected stdin (as
the scripted-stdin tests do) prints an honest "needs a real terminal" line and
exits 0 instead of hanging.

## Checkers — live board (card 73098a55)

Checkers is the pilot for the live-board retrofit: instead of reprinting the
board once per turn, it opens a full-screen, in-place session on the
[`terminal`](../../packages/terminal/) package's alternate screen and redraws the
8x8 board after every move. Pieces are coloured (Red vs White, kings shown bold);
the square under the cursor, the piece you have selected, and its **legal
destinations** are highlighted (the highlights reuse the pure move generator, so
forced captures and multi-jumps are surfaced automatically). A status bar shows
whose turn it is, the piece counts, "CPU is thinking...", and the win/lose/draw
result.

Controls (also shown on the bottom help line):

| Key | Action |
|-----|--------|
| arrow keys | move the cursor around the board |
| Enter / Space | select the piece under the cursor, then confirm a destination |
| Esc | deselect the piece (or, with nothing selected, quit to the menu) |
| q / Ctrl+C | quit back to the menu |

The terminal is **always restored** — on a normal quit, on Ctrl+C (raw mode
delivers it as a catchable key, not a signal), and on any error — because the
session runs `s.init()` then `try: … finally: s.close()`. If it is launched
without a real terminal (piped/redirected stdin, as the scripted-stdin tests do),
`init()` raises before entering the alternate screen; checkers prints an honest
"needs a real terminal" line and returns to the menu instead of crashing or
hanging. Because it builds on `terminal` (a thin crossterm wrapper), the live
board is Windows-compatible too (verified via `cargo check --target
x86_64-pc-windows-gnu`, no `std::os::unix`).

## Chess — live board (card 73098a55, PASS 2)

Chess is now a live in-place board too, on the same shared alternate screen as
the menu and checkers. The 8x8 board redraws after every move (no scrolling); the
two sides are coloured (**White** bright-white, **Black** amber) and the king of
the side to move is flagged red when it is **in check**. Selection is two-phase
with an arrow cursor: move onto one of your pieces and press **Enter/Space** to
select it (its **legal destinations** light up green — the highlights reuse the
pure `gen_legal`, so pins, castling, and en passant are surfaced automatically),
then move onto a green square and press Enter/Space to play the move. When a pawn
reaches the back rank a **promotion prompt** offers **q**ueen / **r**ook /
**b**ishop / k**n**ight (Enter defaults to queen, Esc cancels). A status line
shows whose turn it is, `CHECK`, `CPU is thinking...`, and the
`CHECKMATE` / `STALEMATE` result.

Controls (also shown on the bottom help line):

| Key | Action |
|-----|--------|
| arrow keys | move the cursor around the board |
| Enter / Space | select the piece under the cursor, then confirm a destination |
| Esc | deselect the piece (or, with nothing selected, quit to the menu) |
| q / Ctrl+C | quit back to the menu |

The pure chess engine (legal move generation incl. castling / en passant /
promotion, check / checkmate / stalemate detection, and the alpha-beta CPU) is
**unchanged** — only the presentation + input layer was replaced — so
`test_chess.pyrs` (published perft counts) still passes. Like checkers, the live
board is Windows-compatible (it builds on the `terminal` crossterm wrapper;
verified via `cargo check --target x86_64-pc-windows-gnu`, no `std::os::unix`).

Blackjack and hold-em are **still line-based** (their live-board retrofit is
PASS 3). They are reached through a temporary **bridge**: the shared alternate
screen is suspended (`close()`), the line-based game runs on the normal terminal,
and the alternate screen is resumed (`init()`) on the way back to the menu.

## Layout

- `main.pyrs` — menu shell + entry point. Owns the ONE shared `terminal` Screen
  for the whole app (init once, `try: menu_loop finally: close`), draws the live
  menu, dispatches the games onto that shared Screen, and bridges the still
  line-based card games (suspend/resume the alt screen).
- `ttscreen.pyrs` — tiny **pure-presentation** helper shared by `main` and
  `chess`: frame-safe string clipping (`clip` / `clip_pad`) and the reusable
  full-screen `draw_menu`. No game logic, no session lifecycle. (checkers keeps
  its own equivalents — it landed first and is left frozen.)
- `cards.pyrs` — shared `Card`/`Deck` types + seeded shuffle, used by
  blackjack and hold-em.
- `ui.pyrs` — shared formatting helpers for the **line-based** games: fixed-width
  (<=80 col) layout, separators, unicode suit symbols (with an ASCII-fallback
  switch), and robust `input()`-validation helpers (`prompt_int`,
  `prompt_yes_no`). Still used by blackjack + hold-em; the menu, checkers, and
  chess no longer use it (they run on the `terminal` Screen).
- `blackjack.pyrs` / `holdem.pyrs` / `checkers.pyrs` / `chess.pyrs` — one
  module per game, each exposing a uniquely-named entry function. The two live
  games take the shared Screen (`play_checkers(s)` / `play_chess(s)`); the two
  line-based games keep their line loops (`play_blackjack(seed)` /
  `play_holdem(seed)`). Pure engine logic (hand evaluation, move generation) is
  separated from I/O so it can be unit-tested in isolation.
- `test_*.pyrs` (root siblings) — pure-logic unit tests that import a game
  module. They live in the root, not `tests/`, because pyrst import
  resolution is sibling-only and a program under `tests/` cannot import the
  parent modules. Build + run each (exit 0 = PASS):
  `pyrst build test_chess.pyrs && ./test_chess` (likewise blackjack /
  checkers / holdem).
- `tests/` — driver/smoke tests that don't import the game modules.
  `tests/smoke_main.pyrs` shells out to `./main` via subprocess — see its
  header for how it's invoked.

## Rules & scope

- **Blackjack** — soft-ace valuation; 2 CPU seats play textbook basic strategy
  (hard/soft tables vs the dealer upcard); dealer stands on all 17; naturals
  pay 3:2 with natural-vs-natural pushing; per-seat chip bankroll. *Deferred
  (documented):* splitting pairs, doubling down, and insurance.
- **Texas Hold'em** — blinds, button rotation, four streets, a full 7-card
  evaluator (best 5 of 7, correct category ranking **and** kickers), correct
  multi-way **side pots** built from contribution layers, and heuristic CPU
  seats (Chen preflop, made-hand category + pot odds postflop — not a solver).
- **Checkers** — American rules: forced captures, maximal multi-jumps, kinging
  (a man promoting by a jump ends the move). CPU is depth-limited
  alpha-beta minimax.
- **Chess** — full legal move generation incl. castling, en passant, and
  promotion (verified against published perft counts for five standard
  positions, incl. Kiwipete); check / checkmate / stalemate detection. CPU is
  alpha-beta minimax with material + piece-square evaluation and adaptive
  depth, tuned for &lt;2s/move.

## Design notes

- Presentation: the app runs as **one shared `terminal` session** (`main` owns
  the Screen; card 73098a55). The **menu**, **checkers**, and **chess** render
  **live in-place** on the alternate screen (redraw per move, colours, move
  highlighting — see "Checkers — live board" and "Chess — live board" above).
  Blackjack and hold-em still print a clean board/table **once per turn** and are
  reached via a temporary suspend/resume **bridge** pending their PASS 3 retrofit.
  The pure engines (checkers/chess move-gen, blackjack/hold-em evaluation) were
  **not touched** by the retrofit — only the presentation + input layer changed.
- CPU quality: blackjack = basic strategy + dealer rules; hold-em =
  hand-strength heuristic w/ pot-odds flavor; checkers/chess = depth-limited
  minimax + alpha-beta (material + positional eval), tuned for <2s/move.
- Checkers/chess move generation is written as pure functions, separate
  from I/O, so it can be tested in isolation (perft-lite style counts,
  forced-capture cases).
- Only pyrst stdlib is used (`random` for seeded shuffles, `sys`, etc.) —
  no compiler (`src/`/`lib/`) changes. Language gaps encountered along the
  way are logged on card f1ac8d46 prefixed `GAP:`.
