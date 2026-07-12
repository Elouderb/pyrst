# extern/ — dogfood programs and packages

Real programs and reusable packages written IN pyrst, kept separate from the
compiler (`src/`) and the embedded standard library (`lib/`). Nothing here is
compiled into the pyrst binary; everything imports through the normal module
resolver (local siblings, an active virtual environment's package store, or
`PYRST_PATH`).

The four packages (numpyrs, tzdata, dateutil, kodiak) are also published as
public GitHub repos under `github.com/Elouderb/` and are installable via the
pyrst package manager (`pyrst venv` + `pyrst install <github-url>`; design in
`docs/design/package-management.md`). The copies here are the dev source and the
mirror source — each package's `pyrst.yaml` uses `git:` deps pointing at those
repos. For in-monorepo dogfooding, build with `PYRST_PATH=extern/packages` (no
install needed).

- `programs/` — runnable applications (`extern/programs/<name>/main.pyrs` + a
  README with run instructions).
- `packages/` — importable libraries (`extern/packages/<name>/`), each with its
  own tests and a demo program under `programs/`.

Dogfood rules: agents working here may NOT modify `src/` or `lib/` — compiler
bugs or missing language features get logged on the project's card and
escalated; fixes go through the normal compiler review pipeline. Project state
lives on each project's card.
