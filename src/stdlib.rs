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
}
