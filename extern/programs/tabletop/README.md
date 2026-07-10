# tabletop

A terminal tabletop-game sim, written in pyrst. Card f1ac8d46 (epic
47cafe10, Track A). All games are player-vs-CPU.

## Build + run

```sh
cd extern/programs/tabletop
../../../target/release/pyrst build main.pyrs
./main
```

(or, with a debug/dev build of the compiler: `pyrst build main.pyrs` from
this directory if `pyrst` is on `PATH`.)

You'll get a menu:

```
1) Blackjack   (player + 2 CPU seats)
2) Texas Hold'em (player + 2 CPU seats)
3) Checkers    (vs CPU)
4) Chess       (vs CPU)
5) Quit
```

Enter a number 1-5. Bad input (non-numeric or out of range) re-prompts
instead of crashing.

## Layout

- `main.pyrs` — menu shell; entry point.
- `cards.pyrs` — shared `Card`/`Deck` types + seeded shuffle, used by
  blackjack and hold-em.
- `ui.pyrs` — shared formatting helpers: fixed-width (<=80 col) layout,
  separators, unicode suit symbols (with an ASCII-fallback switch), and
  robust `input()`-validation helpers (`prompt_int`, `prompt_yes_no`).
- `blackjack.pyrs` / `holdem.pyrs` / `checkers.pyrs` / `chess.pyrs` — one
  module per game, each exposing a uniquely-named entry function
  (`play_blackjack` / `play_holdem` / `play_checkers` / `play_chess`). Pure
  engine logic (hand evaluation, move generation) is separated from I/O so it
  can be unit-tested in isolation.
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

- Turn-based games print a clean board/table **once per turn** — no live
  console rewriting, no window-size dependence.
- CPU quality: blackjack = basic strategy + dealer rules; hold-em =
  hand-strength heuristic w/ pot-odds flavor; checkers/chess = depth-limited
  minimax + alpha-beta (material + positional eval), tuned for <2s/move.
- Checkers/chess move generation is written as pure functions, separate
  from I/O, so it can be tested in isolation (perft-lite style counts,
  forced-capture cases).
- Only pyrst stdlib is used (`random` for seeded shuffles, `sys`, etc.) —
  no compiler (`src/`/`lib/`) changes. Language gaps encountered along the
  way are logged on card f1ac8d46 prefixed `GAP:`.
