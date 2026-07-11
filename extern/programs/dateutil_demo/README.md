# dateutil_demo

A trivial meeting-scheduler printout demonstrating the `pyrst-dateutil`
package scaffold (`extern/packages/dateutil/`). Project state lives on
card `0223a4b8`.

Status: **scaffold-stage demo**. `pyrst-dateutil` itself is a scaffold
right now (see `extern/packages/dateutil/README.md`'s status note) —
`parser.parse` works for naive ISO-8601 strings, but `relativedelta`
arithmetic and `rrule` expansion are still `TODO`/`NotImplementedError`
stubs. This demo therefore:

1. Parses a small list of meeting-start strings via `dateutil.parser.parse`
   and prints each as a schedule line — the WORKING slice today.
2. Constructs a `relativedelta` and an `rrule` for a recurring weekly
   standup and prints their fields directly — proving the classes are
   importable/constructible across the `dateutil` package boundary — but
   does **not** call `rrule.iterate`/`rrule.materialize` (recurrence
   expansion) or any `relativedelta` arithmetic, since those bodies still
   raise `NotImplementedError`.

Once `complex-implementer` fills in the arithmetic/expansion engines, this
demo is expected to grow into the full recurrence-printer described on the
card — printing every occurrence of the recurring meeting via
`dateutil.rrule.materialize`, and applying the `relativedelta` to shift a
meeting by a business rule (e.g. "next Monday").

## Build & run

From this directory:

```sh
PYRST_PATH=/home/ethos/Coding/pyrst/extern/packages \
  /home/ethos/Coding/pyrst/target/release/pyrst build main.pyrs
./main
```

(Substitute your own `pyrst` binary path/repo root if different.)

Expected output:

```
dateutil_demo — meeting scheduler (scaffold stage)

Meeting           Start
Standup           2026-07-13T09:00:00
Design review     2026-07-14T14:30:00
All-hands         2026-07-17T11:00:00

Recurring meeting (construction-only proof — expansion is TODO):
  freq=WEEKLY interval=1 count=4
  relativedelta anchored to weekday=0 (Monday)
```

### Automated test

`pyrst build` always emits its binary into the current directory, named
after the source file's basename — build the test from this directory too:

```sh
PYRST_PATH=/home/ethos/Coding/pyrst/extern/packages \
  /home/ethos/Coding/pyrst/target/release/pyrst build tests/smoke_main.pyrs
./smoke_main                  # looks for ./main by default; pass a path as argv[1] to override
```

Prints `PASS: smoke_main` and exits 0, or `FAIL: <reason>` lines and exits
1. The test drives the BUILT `main` binary via `subprocess.run` (same
pattern as `extern/programs/tzdata_demo`'s and `pyrstdb`'s tests) rather
than importing `dateutil` directly, since this program's job is to prove
the end-to-end package + demo wiring, not re-test the package's own logic
(that's `extern/packages/dateutil/tests/smoke_scaffold.pyrs`'s job).

## Module layout

| File | Responsibility |
|------|-----------------|
| `main.pyrs` | Entry point: parses/prints a fixed list of meeting-start strings via `dateutil.parser.parse`, and constructs (without evaluating) a `relativedelta`/`rrule` for a recurring weekly standup. |
| `tests/smoke_main.pyrs` | Builds/drives the compiled `main` binary and asserts its output against the pinned expected lines above. |
