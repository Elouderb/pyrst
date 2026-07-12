//! (PKG Phase 2) Git fetch + a SHA-keyed clone cache for `pyrst install <url>`.
//!
//! `pyrst install <url>` is `git clone <url>` under the hood — git is
//! transport-agnostic, so `https://`, `ssh://`, `git@host:...`, `file://`, and a
//! bare local path all work identically. GitHub is merely where these repos live
//! in production; nothing here is GitHub-specific.
//!
//! We shell out to the `git` binary (like every real package manager) rather than
//! pull in a heavy git crate — the compiler's only deps stay `tower-lsp-server` +
//! `tokio`.
//!
//! The design (§E):
//!   - A URL spec selects a commit: bare `url` → the default branch's current HEAD
//!     (resolved at install time, then PINNED); `url@<ref>` → a tag/branch (→ its
//!     commit, pinned); `url#<sha>` → an exact commit (already pinned).
//!   - Every clone lands in a **SHA-keyed clone cache** so a repeated install of
//!     the same commit is a cheap cache hit and byte-reproducible. The cache root
//!     is `$PYRST_CACHE` (else `~/.cache/pyrst`) `/clones/` — overridable so the
//!     test suite is hermetic (point `PYRST_CACHE` at a temp dir; never touch the
//!     real `~/.cache`, never hit the network — local `file://` fixtures suffice).
//!   - Errors are HONEST (the project's iron rule): a missing `git`, a clone
//!     failure, or a bad ref yield a clear message, never a raw git dump.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::diag::{Error, Result};

/// Which commit of a repo an install spec selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefSpec {
    /// Bare URL → the default branch's current HEAD (resolved, then pinned).
    Default,
    /// `url@<ref>` → a tag or branch name (resolved to its commit, then pinned).
    Ref(String),
    /// `url#<sha>` → an exact commit (already pinned).
    Commit(String),
}

/// A resolved, on-disk checkout of a git repo at a pinned commit.
pub struct GitCheckout {
    /// The cache directory holding the checked-out repo (its root == package root).
    pub dir: PathBuf,
    /// The clean base URL (any `@ref`/`#sha` fragment stripped) — recorded in the
    /// lock so a reproduce re-clones the same remote.
    pub url: String,
    /// The resolved 40-hex commit SHA — recorded in the lock (the pin).
    pub commit: String,
}

// ── URL-spec parsing (pure) ──────────────────────────────────────────────────

/// Split an install spec into its clean base URL and the ref it selects.
///
/// `#<sha>` is unambiguous (git URLs never carry a `#` fragment) and takes
/// precedence. An `@<ref>` is honoured ONLY when the `@` sits after the last `/`,
/// so an scp-form authority (`git@github.com:owner/repo`) or a `user@host` in an
/// `https://` URL is never mistaken for a ref.
pub fn parse_url_spec(spec: &str) -> (String, RefSpec) {
    if let Some((left, frag)) = spec.split_once('#') {
        // A `#sha` pin: also drop a stray trailing `@ref` from the base, if any.
        let (base, _) = strip_trailing_ref(left);
        return (base.to_string(), RefSpec::Commit(frag.trim().to_string()));
    }
    match strip_trailing_ref(spec) {
        (base, Some(r)) => (base.to_string(), RefSpec::Ref(r.trim().to_string())),
        (base, None) => (base.to_string(), RefSpec::Default),
    }
}

/// Peel a trailing `@<ref>` from a URL, but only when the `@` follows the last
/// `/` (i.e. it is in the final path segment, not an authority separator).
fn strip_trailing_ref(s: &str) -> (&str, Option<&str>) {
    if let Some(at) = s.rfind('@') {
        let after_last_slash = match s.rfind('/') {
            Some(sl) => at > sl,
            None => false,
        };
        if after_last_slash && at + 1 < s.len() {
            return (&s[..at], Some(&s[at + 1..]));
        }
    }
    (s, None)
}

// ── cache-key derivation (pure) ──────────────────────────────────────────────

/// The clone-cache directory name for `<base_url>` at `<commit>`:
/// `<sanitized-url>-<djb2hex>@<commit>`. The full commit SHA is the real key (same
/// SHA → same dir → cheap hit → byte-reproducible); the url-hash suffix guarantees
/// two DIFFERENT URLs never collide even if sanitisation maps them to the same
/// readable prefix.
pub fn cache_key(base_url: &str, commit: &str) -> String {
    format!("{}-{:08x}@{}", sanitize_url(base_url), djb2(base_url), commit)
}

/// Map a URL to a filesystem-safe, readable single path segment: drop the scheme,
/// lowercase, collapse every run of non-`[a-z0-9._-]` to a single `-`, trim and
/// cap the length. Never empty.
fn sanitize_url(url: &str) -> String {
    let no_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let mut out = String::new();
    let mut prev_dash = false;
    for c in no_scheme.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    let capped: String = trimmed.chars().take(60).collect();
    let capped = capped.trim_matches('-').to_string();
    if capped.is_empty() {
        "repo".to_string()
    } else {
        capped
    }
}

/// A tiny stable string hash (djb2) — enough to disambiguate cache dirs; not
/// cryptographic.
fn djb2(s: &str) -> u32 {
    let mut h: u32 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    h
}

// ── clone cache ──────────────────────────────────────────────────────────────

/// The clone-cache root: `$PYRST_CACHE` (else `~/.cache/pyrst`) `/clones/`.
/// Overridable so tests are hermetic (never touch the real `~/.cache`).
fn clones_dir() -> Result<PathBuf> {
    let base = if let Some(c) = std::env::var_os("PYRST_CACHE") {
        PathBuf::from(c)
    } else if let Some(h) = std::env::var_os("HOME") {
        PathBuf::from(h).join(".cache").join("pyrst")
    } else {
        return Err(Error::Pkg(
            "cannot locate a clone cache: neither PYRST_CACHE nor HOME is set".into(),
        ));
    };
    Ok(base.join("clones"))
}

// ── git invocation ───────────────────────────────────────────────────────────

/// Confirm `git` is on PATH — an honest error otherwise (the design's documented
/// prerequisite).
fn ensure_git() -> Result<()> {
    match Command::new("git").arg("--version").output() {
        Ok(o) if o.status.success() => Ok(()),
        _ => Err(Error::Pkg(
            "pyrst install requires git on PATH (it fetches packages by shelling out to `git`)"
                .into(),
        )),
    }
}

/// Run `git <args>` (optionally in `cwd`), capturing output. `GIT_TERMINAL_PROMPT=0`
/// makes an auth-required (e.g. private) repo fail HONESTLY instead of hanging on a
/// credential prompt.
fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<std::process::Output> {
    let mut cmd = Command::new("git");
    cmd.args(args).env("GIT_TERMINAL_PROMPT", "0");
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    cmd.output()
        .map_err(|e| Error::Pkg(format!("failed to run git: {}", e)))
}

/// The last non-empty line of git's stderr (typically its `fatal: …`), so a clone
/// failure surfaces a clear one-liner rather than a raw multi-line dump.
fn git_error_tail(out: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("git failed")
        .trim()
        .to_string()
}

// ── public resolve/clone entry points ────────────────────────────────────────

/// Resolve + clone from a raw install spec (`url`, `url@ref`, or `url#sha`).
pub fn resolve_spec(spec: &str) -> Result<GitCheckout> {
    let (url, refspec) = parse_url_spec(spec);
    resolve(&url, &refspec)
}

/// Resolve + clone `url` at `refspec` into the SHA-keyed cache, returning the
/// checkout dir + clean url + resolved commit SHA. Idempotent: a cached commit is
/// reused without re-fetching.
pub fn resolve(url: &str, refspec: &RefSpec) -> Result<GitCheckout> {
    ensure_git()?;
    let clones = clones_dir()?;
    std::fs::create_dir_all(&clones)?;

    match refspec {
        // The SHA is known up front → cache dir is known → check the cache first.
        RefSpec::Commit(sha) => {
            let dst = clones.join(cache_key(url, sha));
            if is_valid_clone(&dst) {
                return Ok(GitCheckout { dir: dst, url: url.to_string(), commit: sha.clone() });
            }
            let tmp = temp_clone_dir(&clones);
            let res = fetch_commit_into(&tmp, url, sha);
            if let Err(e) = res {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(e);
            }
            let got = git_head_sha(&tmp)?;
            finalize(&tmp, &dst)?;
            Ok(GitCheckout { dir: dst, url: url.to_string(), commit: got })
        }
        // The SHA is unknown until we clone → clone to a temp dir, resolve HEAD,
        // then move into the SHA-keyed cache (deduping against a concurrent hit).
        RefSpec::Default | RefSpec::Ref(_) => {
            let branch = match refspec {
                RefSpec::Ref(r) => Some(r.as_str()),
                _ => None,
            };
            let tmp = temp_clone_dir(&clones);
            let res = clone_into(&tmp, url, branch);
            if let Err(e) = res {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(e);
            }
            let sha = git_head_sha(&tmp)?;
            let dst = clones.join(cache_key(url, &sha));
            if is_valid_clone(&dst) {
                let _ = std::fs::remove_dir_all(&tmp);
            } else {
                finalize(&tmp, &dst)?;
            }
            Ok(GitCheckout { dir: dst, url: url.to_string(), commit: sha })
        }
    }
}

/// A directory counts as a completed clone iff it holds a `.git`.
fn is_valid_clone(dir: &Path) -> bool {
    dir.is_dir() && dir.join(".git").exists()
}

/// A unique temp dir under the cache root (same filesystem as the final dir, so
/// the finalise `rename` is cheap + atomic).
fn temp_clone_dir(clones: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    clones.join(format!(".tmp-{}-{}", std::process::id(), nanos))
}

/// Move a freshly-cloned temp dir into its final SHA-keyed location. If a
/// concurrent install already produced the destination, discard our temp and use
/// the existing cached clone.
fn finalize(tmp: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(tmp, dst) {
        Ok(()) => Ok(()),
        Err(e) => {
            if is_valid_clone(dst) {
                let _ = std::fs::remove_dir_all(tmp);
                Ok(())
            } else {
                let _ = std::fs::remove_dir_all(tmp);
                Err(Error::Pkg(format!(
                    "failed to populate clone cache at {}: {}",
                    dst.display(),
                    e
                )))
            }
        }
    }
}

/// `git clone --depth 1 [--branch <ref>] -- <url> <dst>`.
fn clone_into(dst: &Path, url: &str, branch: Option<&str>) -> Result<()> {
    let dst_s = dst.to_string_lossy();
    let mut args: Vec<&str> = vec![
        "clone",
        "--depth",
        "1",
        "-q",
        "-c",
        "advice.detachedHead=false",
    ];
    if let Some(b) = branch {
        args.push("--branch");
        args.push(b);
    }
    args.push("--");
    args.push(url);
    args.push(&dst_s);
    let out = run_git(&args, None)?;
    if !out.status.success() {
        let what = match branch {
            Some(b) => format!("clone {} at ref '{}'", url, b),
            None => format!("clone {}", url),
        };
        return Err(Error::Pkg(format!("failed to {}: {}", what, git_error_tail(&out))));
    }
    Ok(())
}

/// `git init` + `git fetch --depth 1 <url> <sha>` + `git checkout --detach
/// FETCH_HEAD` — the shallow way to materialise a single pinned commit.
fn fetch_commit_into(dst: &Path, url: &str, sha: &str) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    let init = run_git(&["init", "-q"], Some(dst))?;
    if !init.status.success() {
        return Err(Error::Pkg(format!(
            "failed to init a clone for {}: {}",
            url,
            git_error_tail(&init)
        )));
    }
    let fetch = run_git(&["fetch", "--depth", "1", "-q", "--", url, sha], Some(dst))?;
    if !fetch.status.success() {
        return Err(Error::Pkg(format!(
            "failed to fetch commit {} from {}: {}",
            sha,
            url,
            git_error_tail(&fetch)
        )));
    }
    let co = run_git(&["checkout", "-q", "--detach", "FETCH_HEAD"], Some(dst))?;
    if !co.status.success() {
        return Err(Error::Pkg(format!(
            "failed to check out commit {} from {}: {}",
            sha,
            url,
            git_error_tail(&co)
        )));
    }
    Ok(())
}

/// `git -C <dir> rev-parse HEAD`, trimmed.
fn git_head_sha(dir: &Path) -> Result<String> {
    let out = run_git(&["rev-parse", "HEAD"], Some(dir))?;
    if !out.status.success() {
        return Err(Error::Pkg(format!(
            "failed to resolve the cloned commit: {}",
            git_error_tail(&out)
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bare_url_is_default() {
        let (u, r) = parse_url_spec("https://github.com/Elouderb/numpyrs");
        assert_eq!(u, "https://github.com/Elouderb/numpyrs");
        assert_eq!(r, RefSpec::Default);
    }

    #[test]
    fn parse_at_ref() {
        let (u, r) = parse_url_spec("https://github.com/Elouderb/numpyrs@v0.2.0");
        assert_eq!(u, "https://github.com/Elouderb/numpyrs");
        assert_eq!(r, RefSpec::Ref("v0.2.0".into()));
    }

    #[test]
    fn parse_hash_sha() {
        let (u, r) = parse_url_spec("https://github.com/Elouderb/numpyrs#abc123");
        assert_eq!(u, "https://github.com/Elouderb/numpyrs");
        assert_eq!(r, RefSpec::Commit("abc123".into()));
    }

    #[test]
    fn scp_authority_at_is_not_a_ref() {
        // The `git@` authority `@` precedes the last `/`, so it is NOT a ref.
        let (u, r) = parse_url_spec("git@github.com:Elouderb/numpyrs");
        assert_eq!(u, "git@github.com:Elouderb/numpyrs");
        assert_eq!(r, RefSpec::Default);
    }

    #[test]
    fn scp_form_with_trailing_ref() {
        let (u, r) = parse_url_spec("git@github.com:Elouderb/numpyrs.git@v1.0");
        assert_eq!(u, "git@github.com:Elouderb/numpyrs.git");
        assert_eq!(r, RefSpec::Ref("v1.0".into()));
    }

    #[test]
    fn https_userinfo_at_is_not_a_ref() {
        let (u, r) = parse_url_spec("https://user@host.example/owner/repo");
        assert_eq!(u, "https://user@host.example/owner/repo");
        assert_eq!(r, RefSpec::Default);
    }

    #[test]
    fn file_url_forms() {
        assert_eq!(parse_url_spec("file:///tmp/x/repo").1, RefSpec::Default);
        assert_eq!(
            parse_url_spec("file:///tmp/x/repo@v0.1.0").1,
            RefSpec::Ref("v0.1.0".into())
        );
        let (u, r) = parse_url_spec("file:///tmp/x/repo#deadbeef");
        assert_eq!(u, "file:///tmp/x/repo");
        assert_eq!(r, RefSpec::Commit("deadbeef".into()));
    }

    #[test]
    fn cache_key_is_sha_keyed_and_url_sensitive() {
        let sha = "d3543dfe79920046e4b281b377af14bbd2026be2";
        let k = cache_key("https://github.com/Elouderb/numpyrs", sha);
        // Ends with the full SHA (the real key) and holds a readable prefix.
        assert!(k.ends_with(&format!("@{}", sha)), "key must be SHA-keyed: {k}");
        assert!(k.contains("github.com"), "key keeps a readable prefix: {k}");
        // Same URL+SHA is stable (cheap cache hit); a different URL at the same SHA
        // gets a distinct dir (no cross-repo collision).
        assert_eq!(k, cache_key("https://github.com/Elouderb/numpyrs", sha));
        assert_ne!(k, cache_key("https://github.com/Elouderb/dateutil", sha));
        // No path separators leak into the single directory segment.
        assert!(!k.contains('/'), "cache key must be one path segment: {k}");
    }

    #[test]
    fn sanitize_never_empty_and_safe() {
        assert_eq!(sanitize_url("://"), "repo");
        let s = sanitize_url("file:///tmp/x/repo");
        assert!(!s.contains('/'), "no slashes: {s}");
        assert!(!s.is_empty());
    }
}
