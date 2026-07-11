# tzdata_demo

A trivial world-clock printout demonstrating the `tzdata` package
(`extern/packages/tzdata/`). Project state lives on card `965bf13f`.

Status: **implemented**. Given one reference instant in **UTC**
(`2026-07-15 12:00`, hardcoded — pyrst has no timezone-aware "now" primitive
yet), prints the **local wall-clock time**, UTC offset, and DST status of a
curated set of major world cities, driving `tzdata`'s frozen API
(`available_zones` / `utc_offset_at` / `is_dst_at`). Local time is resolved
with a two-pass offset lookup and normalized across day/month/year rollover
(e.g. Auckland lands on the next calendar day).

## Build & run

From this directory:

```sh
PYRST_PATH=/home/ethos/Coding/pyrst/extern/packages \
  /home/ethos/Coding/pyrst/target/release/pyrst build main.pyrs
./main
```

(Substitute your own `pyrst` binary path/repo root if different.)

Expected output (every local-time/offset/DST value oracle-pinned against
python3.12 `zoneinfo` for the UTC reference instant):

```
tzdata world clock
reference instant (UTC): 2026-07-15 12:00
curated zones available: 74

City          Local time          Offset      DST
Los Angeles   2026-07-15 05:00    UTC-07:00   DST
Denver        2026-07-15 06:00    UTC-06:00   DST
Chicago       2026-07-15 07:00    UTC-05:00   DST
New York      2026-07-15 08:00    UTC-04:00   DST
Sao Paulo     2026-07-15 09:00    UTC-03:00   std
London        2026-07-15 13:00    UTC+01:00   DST
Paris         2026-07-15 14:00    UTC+02:00   DST
Athens        2026-07-15 15:00    UTC+03:00   DST
Moscow        2026-07-15 15:00    UTC+03:00   std
Dubai         2026-07-15 16:00    UTC+04:00   std
Kolkata       2026-07-15 17:30    UTC+05:30   std
Shanghai      2026-07-15 20:00    UTC+08:00   std
Tokyo         2026-07-15 21:00    UTC+09:00   std
Sydney        2026-07-15 22:00    UTC+10:00   std
Auckland      2026-07-16 00:00    UTC+12:00   std
```

### Automated test

`pyrst build` always emits its binary into the current directory, named
after the source file's basename — build the test from this directory too:

```sh
PYRST_PATH=/home/ethos/Coding/pyrst/extern/packages \
  /home/ethos/Coding/pyrst/target/release/pyrst build tests/smoke_main.pyrs
./smoke_main                  # looks for ./main by default; pass a path as argv[1] to override
```

Prints `PASS: smoke_main` and exits 0, or `FAIL: <reason>` and exits 1. The
test drives the BUILT `main` binary via `subprocess.run` (same pattern as
`extern/programs/pyrstdb`'s tests) rather than importing `tzdata` directly,
since this program's job is to prove the end-to-end package + demo wiring,
not re-test the package's own logic (that's `tzdata/tests/smoke_api.pyrs`'s
job).

## Module layout

| File | Responsibility |
|------|-----------------|
| `main.pyrs` | Entry point: fixed UTC reference instant, resolves + prints local time/offset/DST for a curated set of major cities via `tzdata.api`. |
| `tests/smoke_main.pyrs` | Builds/drives the compiled `main` binary and asserts its output against the pinned expected lines above. |
