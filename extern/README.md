# extern/ — dogfood programs and packages

Real programs and reusable packages written IN pyrst, kept separate from the
compiler (`src/`) and the embedded standard library (`lib/`). Nothing here is
compiled into the pyrst binary; everything imports through the normal module
resolver (local siblings + `PYRST_PATH` once the import-search-path enabler
lands).

- `programs/` — runnable applications (`extern/programs/<name>/main.pyrs` + a
  README with run instructions).
- `packages/` — importable libraries (`extern/packages/<name>/`), each with its
  own tests and a demo program under `programs/`.

Dogfood rules: agents working here may NOT modify `src/` or `lib/` — compiler
bugs or missing language features get logged on the project's card and
escalated; fixes go through the normal compiler review pipeline. Project state
lives on each project's card.
