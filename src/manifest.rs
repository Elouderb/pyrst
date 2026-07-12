//! (PKG Phase 1) The `pyrst.yaml` package manifest — parse + verify.
//!
//! A pyrst *package* is a directory (or repo) whose `package_root` holds a
//! `pyrst.yaml` plus the package's `.pyrs` modules. The manifest's PRESENCE and a
//! VALID `name` + `version` is what certifies "this is a real pyrst package";
//! `pyrst install` refuses a directory without a valid one (an honest error, per
//! the project's iron rule — honest errors over silent miscompiles).
//!
//! YAML: a SMALL HAND-ROLLED subset parser, NOT a heavy dependency. The compiler's
//! only crate deps are `tower-lsp-server` + `tokio` (see `Cargo.toml`); the schema
//! here is tiny and entirely ours (scalars + one nested list-of-maps), so a full
//! `serde_yaml` dependency in the COMPILER is unjustified (design doc §C/§J). The
//! subset understood:
//!   - top-level `key: value` scalars (`name`, `version`, `package_root`,
//!     `description`), and
//!   - a `dependencies:` block: a sequence of `- name: X` items, each with a
//!     `path:` (Phase 1, local) or `git:` (Phase 2, schema-only here) source.
//! Comments (`#` at line-start or preceded by whitespace) and blank lines are
//! ignored; surrounding quotes on a scalar are stripped. An unknown top-level key
//! is ignored (forward-compatible with later phases).

use std::path::Path;

use crate::diag::{Error, Result};

/// Where a declared dependency's source lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSource {
    /// A LOCAL directory (Phase 1 workflow). Relative paths are interpreted
    /// relative to the DEPENDING package's own directory at install time.
    Path(String),
    /// A git URL (Phase 2). Accepted by the parser so a manifest can already
    /// declare git deps, but `pyrst install` in Phase 1 does not fetch it.
    Git(String),
}

/// One entry of a manifest's `dependencies:` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub source: DepSource,
}

/// A parsed + verified `pyrst.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    /// Subdir (relative to the manifest's repo root) holding the `.pyrs` modules;
    /// default `"."`.
    pub package_root: String,
    pub description: Option<String>,
    pub dependencies: Vec<Dependency>,
}

/// The manifest file name, fixed by the design.
pub const MANIFEST_FILE: &str = "pyrst.yaml";

impl Manifest {
    /// Read + parse + verify the `pyrst.yaml` at `<dir>/pyrst.yaml`.
    pub fn load(dir: &Path) -> Result<Manifest> {
        let path = dir.join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&path).map_err(|_| {
            Error::Pkg(format!(
                "'{}' is not a pyrst package (no {} at {})",
                dir.display(),
                MANIFEST_FILE,
                path.display()
            ))
        })?;
        parse_manifest(&text).map_err(|e| match e {
            // Re-key a bare parse error to name the offending file.
            Error::Pkg(msg) => Error::Pkg(format!("{}: {}", path.display(), msg)),
            other => other,
        })
    }
}

/// Parse `text` as a `pyrst.yaml`, then VERIFY the required fields.
pub fn parse_manifest(text: &str) -> Result<Manifest> {
    let lines: Vec<&str> = text.lines().collect();
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut package_root: Option<String> = None;
    let mut description: Option<String> = None;
    let mut dependencies: Vec<Dependency> = Vec::new();

    let mut i = 0usize;
    while i < lines.len() {
        let line = strip_comment(lines[i]);
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        // Manifest top-level keys are unindented. A stray indented line outside a
        // recognized block is ignored (forward-compatible / lenient).
        if leading_spaces(line) != 0 {
            i += 1;
            continue;
        }
        let (key, val) = split_kv(line.trim())?;
        match key {
            "name" => name = Some(unquote(val).to_string()),
            "version" => version = Some(unquote(val).to_string()),
            "package_root" => package_root = Some(unquote(val).to_string()),
            "description" => description = Some(unquote(val).to_string()),
            "dependencies" => {
                // A `dependencies:` with an inline non-empty value (`[]`) is treated
                // as an empty list; otherwise consume the following indented block.
                i += 1;
                dependencies = parse_deps(&lines, &mut i)?;
                continue; // `i` already advanced past the block
            }
            _ => { /* unknown top-level key: ignore for forward-compatibility */ }
        }
        i += 1;
    }

    let name = name.ok_or_else(|| Error::Pkg("manifest is missing the required `name` field".into()))?;
    let version = version
        .ok_or_else(|| Error::Pkg("manifest is missing the required `version` field".into()))?;

    if !is_valid_name(&name) {
        return Err(Error::Pkg(format!(
            "invalid package name `{}`: names must be non-empty and match [a-z0-9_-]+",
            name
        )));
    }
    if !is_valid_semver(&version) {
        return Err(Error::Pkg(format!(
            "invalid version `{}`: expected a semver string like `0.1.0`",
            version
        )));
    }

    Ok(Manifest {
        name,
        version,
        package_root: package_root.unwrap_or_else(|| ".".to_string()),
        description,
        dependencies,
    })
}

/// Parse the indented `dependencies:` block, starting at `*i` (the first line
/// AFTER the `dependencies:` key). Consumes indented lines only; stops (without
/// consuming) at the first line that returns to column 0, so the caller resumes
/// top-level parsing there.
fn parse_deps(lines: &[&str], i: &mut usize) -> Result<Vec<Dependency>> {
    let mut deps: Vec<Dependency> = Vec::new();
    let mut cur: Option<PartialDep> = None;

    while *i < lines.len() {
        let line = strip_comment(lines[*i]);
        if line.trim().is_empty() {
            *i += 1;
            continue;
        }
        if leading_spaces(line) == 0 {
            break; // back to a top-level key — do not consume
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('-') {
            // New list item. Flush the previous one.
            if let Some(p) = cur.take() {
                deps.push(p.finish()?);
            }
            let mut p = PartialDep::default();
            let rest = rest.trim();
            if !rest.is_empty() {
                let (k, v) = split_kv(rest)?;
                p.set(k, v)?;
            }
            cur = Some(p);
        } else {
            // A continuation field (`path:`/`git:`/`name:`) of the current item.
            let (k, v) = split_kv(trimmed)?;
            match cur.as_mut() {
                Some(p) => p.set(k, v)?,
                None => {
                    return Err(Error::Pkg(
                        "dependency field appears before any `- ` list item".into(),
                    ))
                }
            }
        }
        *i += 1;
    }
    if let Some(p) = cur.take() {
        deps.push(p.finish()?);
    }
    Ok(deps)
}

/// A dependency under construction while its fields are read line by line.
#[derive(Default)]
struct PartialDep {
    name: Option<String>,
    path: Option<String>,
    git: Option<String>,
}

impl PartialDep {
    fn set(&mut self, key: &str, val: &str) -> Result<()> {
        let val = unquote(val).to_string();
        match key {
            "name" => self.name = Some(val),
            "path" => self.path = Some(val),
            "git" => self.git = Some(val),
            other => {
                return Err(Error::Pkg(format!(
                    "unknown dependency field `{}` (expected `name`, `path`, or `git`)",
                    other
                )))
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<Dependency> {
        let name = self
            .name
            .ok_or_else(|| Error::Pkg("a dependency entry is missing its `name`".into()))?;
        if !is_valid_name(&name) {
            return Err(Error::Pkg(format!(
                "invalid dependency name `{}`: names must match [a-z0-9_-]+",
                name
            )));
        }
        let source = match (self.path, self.git) {
            (Some(_), Some(_)) => {
                return Err(Error::Pkg(format!(
                    "dependency `{}` declares both `path` and `git` — pick exactly one",
                    name
                )))
            }
            (Some(p), None) => DepSource::Path(p),
            (None, Some(g)) => DepSource::Git(g),
            (None, None) => {
                return Err(Error::Pkg(format!(
                    "dependency `{}` has no source — declare a `path:` (Phase 1) or `git:` URL",
                    name
                )))
            }
        };
        Ok(Dependency { name, source })
    }
}

// ── small YAML-subset helpers ───────────────────────────────────────────────

/// Count leading ASCII spaces (indentation). Tabs are not expected in our
/// generated manifests; a leading tab counts as non-space and is treated as
/// indentation of 0-with-content, which `split_kv` still parses.
fn leading_spaces(s: &str) -> usize {
    s.chars().take_while(|c| *c == ' ').count()
}

/// Strip a trailing `#` comment. Per YAML, a `#` begins a comment only at the
/// start of the (trimmed-left) content or when preceded by whitespace — so a
/// `#` embedded in a token (e.g. a `url#sha` fragment, Phase 2) is preserved.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut idx: Option<usize> = None;
    for (k, &b) in bytes.iter().enumerate() {
        if b == b'#' && (k == 0 || bytes[k - 1] == b' ' || bytes[k - 1] == b'\t') {
            idx = Some(k);
            break;
        }
    }
    match idx {
        Some(k) => &line[..k],
        None => line,
    }
}

/// Split `key: value` on the FIRST colon. The value is trimmed; a URL's `://`
/// therefore stays intact (only the first colon separates key from value).
fn split_kv(s: &str) -> Result<(&str, &str)> {
    match s.find(':') {
        Some(pos) => Ok((s[..pos].trim(), s[pos + 1..].trim())),
        None => Err(Error::Pkg(format!("expected `key: value`, found `{}`", s))),
    }
}

/// Strip a single pair of surrounding single/double quotes, if present.
fn unquote(s: &str) -> &str {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// A package/dependency name is non-empty and every char is `[a-z0-9_-]`.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// A minimal semver check: a `MAJOR.MINOR.PATCH` numeric core, optionally followed
/// by a `-prerelease` and/or `+build` suffix. Phase 1 only needs the version to be
/// a well-formed string (it is informational until Phase 4's constraint solving),
/// so this validates the core shape without a full semver grammar.
pub fn is_valid_semver(v: &str) -> bool {
    // Peel the optional build metadata (`+...`) then the optional pre-release
    // (`-...`); both must be non-empty when their marker is present.
    let core = match v.split_once('+') {
        Some((c, build)) => {
            if build.is_empty() {
                return false;
            }
            c
        }
        None => v,
    };
    let core = match core.split_once('-') {
        Some((c, pre)) => {
            if pre.is_empty() {
                return false;
            }
            c
        }
        None => core,
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_valid_manifest() {
        let src = "\
name: kodiak
version: 0.1.0
package_root: .
description: pandas-ergonomics dataframes
dependencies:
  - name: numpyrs
    path: ../numpyrs
  - name: dateutil
    path: ../dateutil
  - name: tzdata
    path: ../tzdata
";
        let m = parse_manifest(src).expect("valid manifest must parse");
        assert_eq!(m.name, "kodiak");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.package_root, ".");
        assert_eq!(m.description.as_deref(), Some("pandas-ergonomics dataframes"));
        assert_eq!(m.dependencies.len(), 3);
        assert_eq!(m.dependencies[0], Dependency { name: "numpyrs".into(), source: DepSource::Path("../numpyrs".into()) });
        assert_eq!(m.dependencies[2], Dependency { name: "tzdata".into(), source: DepSource::Path("../tzdata".into()) });
    }

    #[test]
    fn package_root_defaults_to_dot_and_deps_optional() {
        let m = parse_manifest("name: tzdata\nversion: 0.1.0\n").expect("minimal manifest parses");
        assert_eq!(m.package_root, ".");
        assert!(m.dependencies.is_empty());
        assert!(m.description.is_none());
    }

    #[test]
    fn missing_name_is_an_honest_error() {
        let err = parse_manifest("version: 0.1.0\n").unwrap_err().to_string();
        assert!(err.contains("name"), "must name the missing field: {err}");
    }

    #[test]
    fn missing_version_is_an_honest_error() {
        let err = parse_manifest("name: pkg\n").unwrap_err().to_string();
        assert!(err.contains("version"), "must name the missing field: {err}");
    }

    #[test]
    fn bad_name_is_rejected() {
        let err = parse_manifest("name: Kodiak\nversion: 0.1.0\n").unwrap_err().to_string();
        assert!(err.contains("invalid package name"), "uppercase name must be rejected: {err}");
    }

    #[test]
    fn bad_version_is_rejected() {
        let err = parse_manifest("name: pkg\nversion: not-a-version\n").unwrap_err().to_string();
        assert!(err.contains("invalid version"), "non-semver must be rejected: {err}");
    }

    #[test]
    fn nested_dependency_list_and_comments() {
        let src = "\
# a comment line
name: dateutil   # inline comment
version: 0.1.0
dependencies:
  - name: tzdata
    path: ../tzdata   # relative to dateutil's dir
";
        let m = parse_manifest(src).expect("manifest with comments parses");
        assert_eq!(m.name, "dateutil");
        assert_eq!(m.dependencies, vec![Dependency { name: "tzdata".into(), source: DepSource::Path("../tzdata".into()) }]);
    }

    #[test]
    fn git_dependency_is_accepted_by_the_parser() {
        let src = "\
name: kodiak
version: 0.1.0
dependencies:
  - name: numpyrs
    git: https://github.com/Elouderb/numpyrs
";
        let m = parse_manifest(src).expect("git dep is valid schema (fetched in Phase 2)");
        assert_eq!(
            m.dependencies[0].source,
            DepSource::Git("https://github.com/Elouderb/numpyrs".into()),
            "the `://` must survive the first-colon split"
        );
    }

    #[test]
    fn dependency_with_both_sources_is_rejected() {
        let src = "name: p\nversion: 0.1.0\ndependencies:\n  - name: d\n    path: ../d\n    git: https://x/d\n";
        let err = parse_manifest(src).unwrap_err().to_string();
        assert!(err.contains("both `path` and `git`"), "got: {err}");
    }

    #[test]
    fn dependency_without_a_source_is_rejected() {
        let src = "name: p\nversion: 0.1.0\ndependencies:\n  - name: d\n";
        let err = parse_manifest(src).unwrap_err().to_string();
        assert!(err.contains("no source"), "got: {err}");
    }

    #[test]
    fn quoted_scalars_are_unquoted() {
        let m = parse_manifest("name: \"pkg\"\nversion: '1.2.3'\n").expect("quoted scalars parse");
        assert_eq!(m.name, "pkg");
        assert_eq!(m.version, "1.2.3");
    }

    #[test]
    fn semver_shapes() {
        assert!(is_valid_semver("0.1.0"));
        assert!(is_valid_semver("1.2.3"));
        assert!(is_valid_semver("1.0.0-alpha.1"));
        assert!(is_valid_semver("1.0.0+build.5"));
        assert!(is_valid_semver("1.0.0-rc.1+build"));
        assert!(!is_valid_semver("1.0"));
        assert!(!is_valid_semver("1.2.x"));
        assert!(!is_valid_semver(""));
        assert!(!is_valid_semver("1.2.3-"));
    }
}
