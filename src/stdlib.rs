//! Embedded standard library.
//!
//! pyrst's stdlib modules are pyrst SOURCE (`.pyrs`) files that live under
//! `lib/` and are baked into the compiler binary at build time via
//! [`include_str!`]. Embedding (rather than reading from disk relative to the
//! binary) means the stdlib travels WITH the binary: it keeps working after
//! `cargo install` with no `PYRST_STDLIB` path configuration and no filesystem
//! dependency at runtime.
//!
//! The resolver ([`crate::resolver`]) consults [`lookup`] when a `from X import
//! …` / `import X` names a module with no local `X.pyrs` on disk. A LOCAL file
//! always SHADOWS an embedded module of the same name (the resolver tries the
//! local path first).
//!
//! Phase-1 scope: modules backed by `@extern` over Rust std (and/or pure-pyrst
//! helpers and module-level constants). `math` is now a REAL embedded module
//! (`lib/math.pyrs`): qualified `import math; math.sqrt(x)` calls resolve via
//! the general qualified-call path, and `math.pi`/`e`/`tau` are module-level
//! constants — the former hardcoded-in-codegen `math` arms have been removed.

/// Embedded stdlib modules: `(module_name, module_source)`.
///
/// `include_str!` bakes each module's source text into the binary at compile
/// time, so this map is fully static (no filesystem read at runtime).
pub const EMBEDDED_STDLIB: &[(&str, &str)] = &[
    ("os", include_str!("../lib/os.pyrs")),
    // (W3-3) DOTTED submodule key: `import os.path` / `from os.path import …`
    // resolves this embedded package module (directory layout `lib/os/path.pyrs`,
    // keyed by the dotted id `"os.path"`). `os` (a plain module, `lib/os.pyrs`) and
    // `os.path` (a distinct submodule) coexist on disk exactly as in CPython, and
    // are independent imports — `import os` does NOT expose `os.path` (explicit
    // import required, a documented honest v1 divergence). W3-4 fills out the full
    // faithful `os.path`; this is the minimal real module (basename/dirname/isabs).
    ("os.path", include_str!("../lib/os/path.pyrs")),
    ("time", include_str!("../lib/time.pyrs")),
    ("operator", include_str!("../lib/operator.pyrs")),
    ("functools", include_str!("../lib/functools.pyrs")),
    ("statistics", include_str!("../lib/statistics.pyrs")),
    ("math", include_str!("../lib/math.pyrs")),
    // Rust interop Phase 2: `re` is backed by the external `regex` crate (it
    // declares `@crate("regex", "1")`), so importing it routes `build` through
    // the Cargo-project path. The other embedded modules use only Rust std.
    ("re", include_str!("../lib/re.pyrs")),
    // Tier-2 batch: pure-pyrst modules built on generics (functions + inferred
    // bounds) and module-level constants. `string` is constants + a str helper;
    // `bisect`/`heapq` are generic algorithms over `list[T]` (PartialOrd via
    // `<`), with the mutating variants taking a `Mut[list[T]]` by-ref param;
    // `collections` provides `Counter`/`most_common` over hashable `T`. None
    // need an external crate (Rust std only), so importing them stays on the
    // single-file build path.
    ("string", include_str!("../lib/string.pyrs")),
    ("bisect", include_str!("../lib/bisect.pyrs")),
    ("heapq", include_str!("../lib/heapq.pyrs")),
    ("collections", include_str!("../lib/collections.pyrs")),
    // Tier-3 batch: pure-pyrst modules over generics + classes with mutable
    // instance state. `itertools` is an EAGER combinatoric subset (`chain`/
    // `repeat`/`product`/`combinations`/`permutations` over `list[T]`, plus an
    // int-specialised `accumulate`); it was also removed from the resolver
    // skip-list so the import resolves. `textwrap` is pure str-method text
    // wrapping. `random` is a seedable LCG `Random` CLASS (mutable `seed`
    // field) with free generic `choice`/`shuffle` helpers. None need an external
    // crate (Rust std only), so importing them stays on the single-file build
    // path.
    ("itertools", include_str!("../lib/itertools.pyrs")),
    ("textwrap", include_str!("../lib/textwrap.pyrs")),
    ("random", include_str!("../lib/random.pyrs")),
    // Tier-4: `json` is a PURE-PYRST module (no crate) — a recursive-descent
    // parser (`loads`) and serializer (`dumps`) over a recursive, tagged
    // `JsonValue` class (`kind` discriminant + per-shape payload fields,
    // `arr: list[JsonValue]` / `obj: dict[str, JsonValue]`). It was removed from
    // the resolver skip-list so the import resolves; it needs only Rust std, so
    // importing it stays on the single-file build path.
    ("json", include_str!("../lib/json.pyrs")),
    // ── W2 stdlib wave ───────────────────────────────────────────────────
    // Tier-5: `calendar` is a PURE-PYRST module (no crate) reimplementing
    // CPython's `calendar.py` calendar math and `TextCalendar` text
    // formatting directly (no `datetime` dependency): a from-scratch
    // days-from-civil weekday algorithm plus a byte-exact port of
    // `formatmonth`/`formatyear`. `Day`/`Month` enums (G5) collapse to plain
    // `int`; module-level mutable `firstweekday` state (G2) is deferred to an
    // explicit `firstweekday: int = 0` keyword param on the affected
    // functions (see the module header for the exact shape); the custom
    // `IllegalMonthError`/`IllegalWeekdayError` hierarchy (G3) collapses to
    // `ValueError` with CPython's exact message text. Needs only Rust std, so
    // importing it stays on the single-file build path.
    ("calendar", include_str!("../lib/calendar.pyrs")),
    // W2 ccr-trio: `colorsys`/`copy`/`reprlib` — pure-pyrst, Rust std only.
    // NOTE: `copy.copy` collides with `shutil.copy` on co-import (both keep
    // their CPython names; flat cross-module namespace — see module headers).
    ("colorsys", include_str!("../lib/colorsys.pyrs")),
    ("copy", include_str!("../lib/copy.pyrs")),
    ("reprlib", include_str!("../lib/reprlib.pyrs")),
    // W2: `configparser` is a PURE-PYRST module (no crate) — a single
    // `ConfigParser` class over INI-style section/option config text, with
    // `BasicInterpolation`-style `%(name)s` substitution. Insertion order for
    // sections/options is tracked EXPLICITLY via parallel `list[str]` fields
    // (pyrst `dict` iteration order is not insertion-preserving) — see the
    // module header in `lib/configparser.pyrs` for the full design writeup
    // and the documented divergences (custom exception classes -> ValueError,
    // `write()` returns `str` instead of taking a file object, etc). Needs
    // only Rust std, so importing it stays on the single-file build path.
    ("configparser", include_str!("../lib/configparser.pyrs")),
    // W2 card cffa1ca4: `csv` is a PURE-PYRST module (no crate) — a
    // character-by-character quoting state machine (`_csv_Parser`) mirroring
    // CPython's own `_csv` reader, plus a buffer-holding `_csv_Writer` /
    // `DictReader` / `DictWriter` / `Sniffer`. No file-object type exists in
    // pyrst, so reader/writer work over `str`/`list[str]` directly (see the
    // module header in `lib/csv.pyrs` for the full divergence list). It
    // needs only Rust std, so importing it stays on the single-file build path.
    ("csv", include_str!("../lib/csv.pyrs")),
    // W2 card 855fe2e5: `dataclasses` companions (`field`/`asdict`/`astuple`/
    // `fields`/`is_dataclass`/`FrozenInstanceError`/`MISSING`/`Field`) were
    // probed against the real compiler and found to be UNREACHABLE with
    // library-only pyrst source (no generic field reflection, no generic
    // class-field-default function calls — see `lib/dataclasses.pyrs`'s
    // header for the reproduction transcripts). The file therefore defines no
    // runtime names; it is registered here for documentation/future-card
    // discoverability, but `src/resolver.rs` still hard-skips the module name
    // `dataclasses` (same treatment `sys` used to get) BEFORE this lookup is
    // ever consulted, so this entry is presently inert — `@dataclass`
    // (compiler-synthesized, card 6f69d4a3) and the no-op `from dataclasses
    // import dataclass` behavior are both unchanged.
    ("dataclasses", include_str!("../lib/dataclasses.pyrs")),
    // W2 batch: `datetime` is the biggest stdlib unit so far — `date`/`time`/
    // `datetime`/`timedelta` classes (+ a small `IsoCalendarDate` value class
    // for `isocalendar()`), pure-pyrst calendar math ported from CPython's
    // own `Lib/datetime.py`. NO tzinfo/fold anywhere (deferred, documented in
    // the module header). NOTE: `class time` (this module) collides with the
    // top-level `def time()` in `lib/time.pyrs` on co-import — pyrst's flat
    // module namespace rejects `import time` + `import datetime` together
    // (same shape as the pre-existing operator/re collision precedent); see
    // the module header for the exact error and the workaround. Needs only
    // Rust std (one small `@extern` wall-clock read for `today()`/`now()`),
    // so importing it stays on the single-file build path.
    ("datetime", include_str!("../lib/datetime.pyrs")),
    // W2: `difflib` is a PURE-PYRST module (no crate) — SequenceMatcher's
    // greedy longest-match recursion over a shared `list[str]`-token engine
    // (`b2j: dict[str, list[int]]`), exact `Match`/opcode tuples,
    // ratio/quick_ratio/real_quick_ratio, get_close_matches, unified_diff,
    // and ndiff (see the module header for the full fidelity/divergence
    // account). Needs only Rust std, so importing it stays on the
    // single-file build path.
    ("difflib", include_str!("../lib/difflib.pyrs")),
    // W2 batch: `enum` is a DOCUMENTATION-PLUS-HELPER-CONVENTIONS module (no
    // `Enum`/`IntEnum`/`Flag`/`auto`/`unique` — see card 03eb4e2c's design
    // decision and `lib/enum.pyrs`'s header). It needs only Rust std (three
    // small generic functions), so importing it stays on the single-file
    // build path.
    ("enum", include_str!("../lib/enum.pyrs")),
    // W2: `fnmatch` is a PURE-PYRST module that ITSELF `import`s `re` (the
    // resolver recurses into an embedded module's own imports), so it
    // transitively carries `re`'s `@crate("regex", "1")` dependency and
    // routes the build through the Cargo-project path, same as importing
    // `re` directly.
    ("fnmatch", include_str!("../lib/fnmatch.pyrs")),
    // Tier-5 (W2): `fractions` is a PURE-PYRST module (no crate) — a single
    // `Fraction` class (i64 numerator/denominator, GCD-canonicalized) over
    // the compiler's fixed 9-dunder set (`__init__ __str__ __repr__ __eq__
    // __lt__ __add__ __sub__ __mul__ __neg__ __bool__`); `__truediv__` is
    // unavailable, so division ships as a `.div()` method instead (see the
    // module header for the full divergence list, incl. i64-only G9
    // overflow). Needs only Rust std, so importing it stays on the
    // single-file build path.
    ("fractions", include_str!("../lib/fractions.pyrs")),
    // W2 fs-trio (card 39bf959a, docs/design/w2-dossiers/fs-trio.md): `shutil`/
    // `tempfile`/`filecmp`, each a self-contained @extern-over-std::fs module
    // (no cross-module imports — every private helper is re-declared
    // `_<module>_`-prefixed per module, matching every other embedded lib
    // file's self-containment). `shutil` and `tempfile` additionally
    // `@crate("getrandom", "0.2")` (already pinned by `os.pyrs`; the crate dep
    // collector dedups identical name+version declarations, so this is not a
    // new dependency). `shutil.disk_usage` shells out to `df -Pk` (no stable
    // Rust std disk-usage API, and no new crate/FFI added this wave).
    ("shutil", include_str!("../lib/shutil.pyrs")),
    ("tempfile", include_str!("../lib/tempfile.pyrs")),
    ("filecmp", include_str!("../lib/filecmp.pyrs")),
    // Tier-5 (W2): `graphlib` is a PURE-PYRST module (no crate) — the single
    // `TopologicalSorter[T]` class over generic `T` (dict-key hashable),
    // Kahn's-algorithm topological sort with CPython-exact insertion-order
    // tie-breaking (tracked via an explicit `list[T]`, not dict iteration).
    // `add`/`done` take `list[T]` params (no `*args`); `CycleError` maps to
    // `ValueError` with CPython's exact message text. Needs only Rust std, so
    // importing it stays on the single-file build path.
    ("graphlib", include_str!("../lib/graphlib.pyrs")),
    // W2: `html` (card d9182d50) is a PURE-PYRST module (no crate) — a
    // CPython-exact `escape`, plus a hand-rolled (no `re` dependency)
    // `unescape` scanner over a deliberately-narrowed ~252-name
    // `name2codepoint` entity core (not the full 2231-entry `html5` dict;
    // see the module header for the exact coverage line). `html.entities`
    // is NOT exposed (G3: no dotted submodules). NOTE: `escape` also exists
    // as a top-level name in `lib/re.pyrs` (regex-metachar escaping) — a
    // program cannot `import` both `html` and `re` (cross-module collision,
    // card 6c8b4a39); documented in both modules' headers.
    ("html", include_str!("../lib/html.pyrs")),
    // Tier-4 (w2-writer, card 066173c4): `io` is a PURE-PYRST module (no
    // crate) — a `StringIO` class over a `str` buffer + cursor, matching
    // CPython's `io.StringIO` I/O and positioning semantics (seek-beyond-end
    // null-padding, EOF, overwrite-after-seek). Needs only Rust std, so
    // importing it stays on the single-file build path.
    ("io", include_str!("../lib/io.pyrs")),
    // W2 stdlib writer wave (card cc0ff8fe): `pathlib` is a PURE-PYRST module
    // — a `PurePosixPath` class implementing str-only POSIX path
    // manipulation (parts/name/stem/suffix/parents/joinpath/with_name/
    // with_suffix/with_stem/relative_to/match_/is_relative_to). No
    // filesystem access (this is PurePath, not the filesystem-touching
    // Path), so it needs only Rust std — importing it stays on the
    // single-file build path. See the module header for the full
    // gate-forced divergence list (no *args constructor/joinpath, no `/`
    // operator, `match_` instead of `match` — a pyrst statement keyword).
    ("pathlib", include_str!("../lib/pathlib.pyrs")),
    // W2: `pprint` is a PURE-PYRST module (no crate) built entirely on top
    // of the new `repr()`/PyRepr machinery — `pformat` computes `repr
    // (object)` then does depth-truncation / width-wrapping / underscore-
    // grouping as generic, type-agnostic bracket/quote-aware string
    // scanning (see lib/pprint.pyrs header for why: pyrst's generics can't
    // branch on a type parameter's concrete shape from inside one function
    // body, so this sidesteps needing per-container-shape overloads).
    ("pprint", include_str!("../lib/pprint.pyrs")),
    // W2 sep-quad: stat, errno, platform, getpass — four small modules
    // with POSIX constants and system bindings. stat exports S_IS* predicates
    // and file-mode constants. errno exports POSIX error codes + an
    // errorcode(code) LOOKUP FUNCTION (HARDENED: not a dict — pyrst has no
    // module-level dict state, see lib/errno.pyrs header). platform exports
    // system/machine/release/version/python_version/platform/node (registered
    // under its REAL CPython name `platform`, so `import platform` works; a
    // minimal subset of CPython's platform module — see lib/platform.pyrs).
    // getpass exports getuser (env chain) and getpass(prompt) (interactive,
    // @extern; HARDENED: the stream param was dropped, see lib/getpass.pyrs).
    ("stat", include_str!("../lib/stat.pyrs")),
    ("errno", include_str!("../lib/errno.pyrs")),
    ("platform", include_str!("../lib/platform.pyrs")),
    ("getpass", include_str!("../lib/getpass.pyrs")),
    // W2: `shlex` is a pure-pyrst port of the 3 module-level functions
    // (`split`/`join`/`quote`) — no `shlex.shlex` class, no external crate,
    // Rust std only, so importing it stays on the single-file build path.
    // NOTE: `split` collides with `re.split` and `join` collides with
    // `os.join` under pyrst's flat cross-module namespace (see
    // `lib/shlex.pyrs` header) — a program cannot import both.
    ("shlex", include_str!("../lib/shlex.pyrs")),
    // Tier-5 (card cd3aa7b7): `sys` PARTIAL scope — `maxsize`/`platform`/
    // `version`/`version_info`/`exit` only (`argv`/`stdin`/`stdout`/`stderr`
    // are G2-deferred, module-level mutable state). It was removed from the
    // resolver's stdlib skip-list (see src/resolver.rs) so the import
    // resolves; it needs only Rust std (`exit` is a single `@extern` over
    // `std::process::exit`), so importing it stays on the single-file build
    // path. NOTE: `platform`/`version` (consts here) collide with the
    // `platform` module's top-level functions of the same names on co-import
    // (flat namespace) — a program cannot import both `sys` and `platform`.
    ("sys", include_str!("../lib/sys.pyrs")),
];

/// Look up an embedded stdlib module's source by NAME (e.g. `"os"`).
///
/// Returns the module's pyrst source text when `name` is an embedded module,
/// or `None` otherwise. The resolver calls this only AFTER a local `<base
/// dir>/<name>.pyrs` lookup misses, so local files shadow embedded modules.
pub fn lookup(name: &str) -> Option<&'static str> {
    EMBEDDED_STDLIB
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, src)| *src)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `os` module is embedded and its source is non-empty and looks like a
    /// pyrst `@extern` module (a sanity check that `include_str!` resolved the
    /// path and baked real content, not an empty/placeholder file).
    #[test]
    fn os_module_is_embedded() {
        let src = lookup("os").expect("`os` must be an embedded stdlib module");
        assert!(!src.trim().is_empty(), "embedded os source must be non-empty");
        assert!(src.contains("def getenv"), "os must define getenv");
        assert!(src.contains("@extern"), "os bindings must be @extern");
    }

    /// A name with no embedded module returns `None` (the resolver then reports
    /// `ImportNotFound` unless a local file exists).
    #[test]
    fn unknown_module_is_not_embedded() {
        assert!(lookup("notamodule").is_none());
    }

    /// Rust interop Phase 2: `re` is a REAL embedded module backed by the
    /// external `regex` crate. Its source is baked in, declares the crate
    /// dependency via `@crate("regex", "1")`, and defines the four `@extern`
    /// wrappers — the signal that importing `re` routes `build` through the
    /// Cargo-project path.
    #[test]
    fn re_module_is_embedded_and_declares_regex_crate() {
        let src = lookup("re").expect("`re` must be an embedded stdlib module");
        assert!(!src.trim().is_empty(), "embedded re source must be non-empty");
        assert!(src.contains("@crate(\"regex\", \"1\")"), "re must declare the regex crate");
        assert!(src.contains("@extern"), "re bindings must be @extern");
        for f in ["def is_match", "def find_all", "def replace_all", "def split"] {
            assert!(src.contains(f), "re must define {}", f);
        }
    }

    /// `math` is now a REAL embedded module (`lib/math.pyrs`): its source is
    /// baked in, defines the @extern `sqrt` wrapper, and carries the module-level
    /// `pi` constant. (It was previously hardcoded in codegen and deliberately
    /// absent here; this asserts the migration.)
    #[test]
    fn math_module_is_embedded() {
        let src = lookup("math").expect("`math` must now be an embedded stdlib module");
        assert!(!src.trim().is_empty(), "embedded math source must be non-empty");
        assert!(src.contains("def sqrt"), "math must define sqrt");
        assert!(src.contains("@extern"), "math function bindings must be @extern");
        assert!(src.contains("pi: float"), "math must define the pi constant");
    }

    /// `json` is a PURE-PYRST embedded module (`lib/json.pyrs`): its source is
    /// baked in, defines the `loads`/`dumps` entry points and the recursive
    /// `JsonValue` class, and — being pure pyrst — declares NO `@crate`
    /// dependency (it stays on the single-file build path).
    #[test]
    fn json_module_is_embedded() {
        let src = lookup("json").expect("`json` must be an embedded stdlib module");
        assert!(!src.trim().is_empty(), "embedded json source must be non-empty");
        assert!(src.contains("def loads"), "json must define loads");
        assert!(src.contains("def dumps"), "json must define dumps");
        assert!(src.contains("class JsonValue"), "json must define JsonValue");
        assert!(!src.contains("@crate"), "json is pure pyrst and needs no crate");
    }
}
