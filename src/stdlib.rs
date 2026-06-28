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
//! Phase-1 scope: only modules backed by `@extern` over Rust std are embedded
//! here. `math` is intentionally NOT embedded — it stays hardcoded in codegen
//! because `import math; math.sqrt(x)` uses qualified `module.fn()` calls, which
//! file/embedded modules do not yet support (deferred to a separate card).

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
        // `math` is deliberately NOT embedded (it stays hardcoded in codegen).
        assert!(lookup("math").is_none());
    }
}
