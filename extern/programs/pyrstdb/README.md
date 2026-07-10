# pyrstdb

A small SQL database, written in pyrst, dogfooding the language. REPL-first
(interactive shell + a scriptable non-interactive mode); a line-protocol TCP
server mode is planned but deferred until `lib/socket.pyrs` lands (card
`2f62ad54`) — see [Server mode](#server-mode-deferred) below. All project
state lives on card `283e473a`.

Status: **engine implemented**. Tokenizer, recursive-descent parser, typed
AST, executor over an in-memory table store, and file persistence are all live.
The REPL parses and executes real SQL, formats result tables, reports named
positional errors, and persists to a file when started with `--db`. Server mode
remains deferred (see [Server mode](#server-mode-deferred)).

## Build & run

From this directory:

```sh
# from extern/programs/pyrstdb/
../../../target/release/pyrst build main.pyrs
./main                            # interactive REPL, ephemeral in-memory DB
./main path/to/script.sql         # run a file's statements, one per line, then exit
./main --db mydb.pdb              # interactive REPL persisted to mydb.pdb
./main --db mydb.pdb script.sql   # run a script against a persisted DB
```

With `--db`, the database is loaded on start (a missing file starts empty), and
saved by `.save` / a bare `SAVE` statement and automatically on `.quit`/`.exit`
and at the end of a script run. Without `--db` the session is in-memory only.

(Substitute the path to your own `pyrst` binary if it isn't at
`target/release/pyrst`.)

### Manual smoke check

```sh
printf 'CREATE TABLE t (id INT, name TEXT);\nINSERT INTO t VALUES (1, '\''hi'\'');\nSELECT * FROM t;\n.quit\n' | ./main
```

Should print the create/insert status lines, an aligned result table with one
row, then `bye`, and exit 0.

### Automated test

`pyrst build <file.pyrs>` always emits its binary into the **current
directory**, named after the source file's basename (a `-o`/output-path flag
is not honored) — so build `tests/smoke_repl.pyrs` from this directory too:

```sh
../../../target/release/pyrst build main.pyrs
../../../target/release/pyrst build tests/smoke_repl.pyrs
./smoke_repl                  # looks for ./main by default; pass a path as argv[1] to override
```

Prints `PASS: smoke_repl` and exits 0, or `FAIL: <reason>` and exits 1.

**Why the test drives `main.pyrs` in *script* mode, not by piping into a live
process:** pyrst's `subprocess.run` has no `input=` parameter to feed a
child process's stdin (see `PYTHON_COMPATIBILITY.md`, "subprocess" row), and
`input()` itself has no EOF signal — reading past end-of-stdin returns `""`
forever rather than raising (empirically verified; not yet documented
upstream). A REPL loop fed piped stdin with no trailing `.quit` never
terminates. `main.pyrs`'s script-file mode (`./main script.sql`, iterating
the file's lines instead of calling `input()`) is therefore the only
automatable non-interactive seam today, and is what every future pyrstdb
test should drive. The interactive+piped path is still real (see the manual
smoke check above) — it just always needs an explicit `.quit`/`.exit` line.

## Module layout

| File | Responsibility |
|------|-----------------|
| `main.pyrs` | REPL shell: prompt loop, meta-commands, script-file mode, statement dispatch. Entry point. |
| `tokenizer.pyrs` | `tokenize(src: str) -> list[Token]` — lexer for the SQL subset below. |
| `parser.pyrs` | Recursive-descent `Parser` + `parse(src: str) -> Statement` — builds the typed AST. |
| `ast_types.pyrs` | AST node types (`Statement`, `ColumnDef`, `Value`, `Condition`, `WhereClause`) — see design note in the file header on why `Statement` is one flat tagged struct rather than a class hierarchy. |
| `executor.pyrs` | `execute(stmt, db) -> ExecResult` — runs a `Statement` against a `storage.Database`; `format_result` renders output (shared by REPL and the future server). |
| `storage.pyrs` | In-memory `Database`/`Table`/`Row` store + `load_database`/`save_database` file persistence. |
| `tests/` | pyrst test programs; each prints `PASS: <name>` / `FAIL: <reason>` and exits 0/nonzero to match. |

Import graph (no cycles): `ast_types` ← `storage` ← `executor`; `tokenizer` ←
`parser` (also imports `ast_types`); `main` imports all five.

## SQL subset

Keywords are case-insensitive. Statement grammar (`;` terminator optional):

| Statement | Grammar | Notes |
|-----------|---------|-------|
| `CREATE TABLE` | `CREATE TABLE name (col TYPE, col TYPE, ...)` | `TYPE` is one of `INT`/`FLOAT`/`TEXT`/`BOOL`. |
| `INSERT` | `INSERT INTO name [(col, ...)] VALUES (v, ...) [, (v, ...) ...]` | Omitted column list = all columns, declared order. Multi-row insert supported. |
| `SELECT` | `SELECT (* \| col, ...) FROM name [WHERE ...] [ORDER BY col [ASC\|DESC]] [LIMIT n]` | |
| `UPDATE` | `UPDATE name SET col = v, ... [WHERE ...]` | |
| `DELETE` | `DELETE FROM name [WHERE ...]` | |
| `DROP TABLE` | `DROP TABLE name` | |
| `WHERE` clause | `col op literal [(AND\|OR) col op literal]*` | `op` is one of `= != < > <= >=`. Strict left-to-right evaluation — **no parenthesized sub-expressions** (an honest documented limit of this SQL subset, not a pyrst-language gap). |
| `JOIN` | not supported | Out of scope for v1; a `JOIN` keyword is a named parse error, never silently ignored. |

Any other syntax — an unsupported statement, a stray token, an unterminated
string, an unknown identifier where a keyword was expected — must produce a
**named parse error with a source position** (`ParseError`/`TokenizeError`,
both carrying a message; the tokenizer/parser TODO bodies name the exact
`raise` sites). Never a silent misparse.

## File format

A pyrstdb database is one flat text file (path chosen by the caller, e.g.
`mydb.pdb`), designed to be readable/writable with only the `open`/`read`/
`write`/`close` file-handle API (no external serde crate is available to
this program). Sketch (subject to refinement once `storage.pyrs` is
implemented — this is a starting seam, not a frozen spec):

```
TABLE <name> <col1>:<TYPE1> <col2>:<TYPE2> ...
ROW <name> <cell1>|<cell2>|...
TABLE <name2> ...
ROW <name2> ...
```

- One `TABLE` line per table, declaring its name and typed columns.
- One `ROW` line per row, naming its owning table and `|`-separated cell
  values in column order.
- Cells are the literal's `str()` form (`INT`/`FLOAT`/`BOOL` print as
  decimal/`True`/`False`; `TEXT` is written raw — a `|` or newline inside a
  `TEXT` cell is a named error on save, not silent corruption).
- A `NULL` cell is written as the sentinel `\N`. A `TEXT` value that is
  literally the two characters `\N` round-trips as `NULL` — a documented v1
  edge, not silent corruption.
- A `ROW` line's cells are the substring after `ROW <name> `, split on `|`, so
  `TEXT` values containing spaces round-trip faithfully (a single-column table
  takes the whole remainder as one cell).
- Loading builds `Database.tables` from `TABLE` lines (in order), then
  appends each `ROW` to its named table; an unknown-table `ROW` or an
  arity/type mismatch against its `TABLE` declaration is a load-time error
  naming the offending line.
- A missing file loads as an empty, fresh `Database` (first-run UX — no
  separate "init" command required).

## Meta-commands (REPL)

| Command | Effect |
|---------|--------|
| `.help` | Print the SQL subset + meta-command summary. |
| `.tables` | List table names in the current database. |
| `.schema [table]` | Print a reconstructed `CREATE TABLE` for one table, or every table. |
| `.save` | Persist the database to its `--db` file (an error if started without `--db`). |
| `.quit` / `.exit` | Save (if `--db`) and exit cleanly. **Always required** to end a session — see the `input()` EOF note above. |

`SAVE` is also accepted as a SQL-adjacent statement (equivalent to `.save`); it
is intercepted before parsing so it never reserves a `save` column name.

## Server mode (deferred)

Planned once `lib/socket.pyrs` (card `2f62ad54`) lands: a line-protocol TCP
server — one SQL statement per line in, a table-formatted response out,
reusing `executor.execute` + `executor.format_result` unchanged — plus a
small client program. Sequential (non-concurrent) connection handling is
acceptable for v1 (pyrst is single-threaded); document that limit rather
than attempting concurrency. This work item stays open on card `283e473a`
until the socket library lands; the REPL can be accepted independently.

## Language gaps hit while scaffolding (logged on card `283e473a`)

- **No qualified generic type args.** `list[tokenizer.Token]` /
  `def f(x: storage.Database)` do not parse — a dotted module-qualified name
  is rejected wherever a type annotation is expected. Workaround used
  throughout: `from tokenizer import Token` then the bare name. Confirmed
  empirically (`pyrst check` on a two-line repro); not found stated in
  `PYTHON_COMPATIBILITY.md`.
- **No algebraic sum types / no downcasting a base-class variable to a
  concrete subclass's own fields.** Shaped the `ast_types.Statement` design
  — see that file's header comment — into one flat tagged struct
  (`kind: str` discriminant + every field any kind might need) rather than
  a `CreateTableStmt`/`InsertStmt`/... class hierarchy, since the executor
  has no way to recover subclass-only fields from a `Statement`-typed value
  (virtual methods dispatch fine; field *reads* through a base-typed
  variable are typeck-rejected for derived-only fields, and `match`/`case`
  only supports literal/`_` patterns, not class-structural matching).
- **No f-string `!r` / `!s` conversion.** `f"...{x!r}..."` is a lex error
  ("unexpected character '!'"). Not stated in `PYTHON_COMPATIBILITY.md`.
  Workaround: build messages with `+ str(...)` concatenation (no repr-quoting
  in error text).
- **Nested self-mutating method calls miscompile to a Rust double-borrow.**
  `self.finish(self.parse_x())` — an outer `&mut self` method taking the result
  of an inner `&mut self` method — passes `pyrst check` but fails codegen with
  rustc `E0499` (two overlapping `&mut self` borrows). Workaround: bind the
  inner result to a local first (`stmt = self.parse_x(); self.finish(stmt)`).
  See `parser.parse_statement`.
- **`input()` has no EOF signal.** Reading past end-of-stdin returns `""`
  forever instead of raising (empirically verified: a piped-stdin program
  with no terminating `.quit` spins printing `got: ` indefinitely rather
  than stopping at EOF). Documented in `PYTHON_COMPATIBILITY.md`'s `input()`
  row as "Reads a line from stdin" with no EOF-behavior note. Drove the
  script-file mode (`main.pyrs`'s `run_script`) as the automatable
  non-interactive seam, and the "always `.quit`" REPL contract.
