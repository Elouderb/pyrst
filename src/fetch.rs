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
#[derive(Debug, Clone)]
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
pub fn parse_url_spec(spec: &str) -> Result<(String, RefSpec)> {
    if let Some((left, frag)) = spec.split_once('#') {
        // A `#sha` pin: also drop a stray trailing `@ref` from the base, if any.
        // The sha is caller-controlled (and can arrive via an UNTRUSTED package's
        // dependency URL), so it MUST be a real 40-char hex commit before it is ever
        // allowed to shape a filesystem path — reject anything else honestly (a
        // `#../../evil` traversal payload never becomes a `RefSpec::Commit`).
        let (base, _) = strip_trailing_ref(left);
        let sha = normalize_sha(frag).ok_or_else(|| {
            Error::Pkg("invalid commit sha in url#<sha>: expected a 40-char hex commit".into())
        })?;
        return Ok((base.to_string(), RefSpec::Commit(sha)));
    }
    match strip_trailing_ref(spec) {
        (base, Some(r)) => Ok((base.to_string(), RefSpec::Ref(r.trim().to_string()))),
        (base, None) => Ok((base.to_string(), RefSpec::Default)),
    }
}

/// Normalise a `#<sha>` fragment to a canonical pinned commit: exactly 40 hex
/// digits, lowercased. `None` for anything else (abbreviated shas are ambiguous for
/// a pinned cache/lock, and a non-hex value must never reach a cache path).
fn normalize_sha(frag: &str) -> Option<String> {
    let t = frag.trim();
    if t.len() == 40 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(t.to_ascii_lowercase())
    } else {
        None
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
pub fn clones_dir() -> Result<PathBuf> {
    let base = if let Some(c) = std::env::var_os("PYRST_CACHE") {
        PathBuf::from(c)
    } else {
        // Platform-native cache home. UNIX path is unchanged from before; Windows
        // uses %LOCALAPPDATA% (HOME is typically unset in cmd/PowerShell).
        #[cfg(windows)]
        {
            if let Some(l) = std::env::var_os("LOCALAPPDATA") {
                PathBuf::from(l).join("pyrst")
            } else if let Some(u) = std::env::var_os("USERPROFILE") {
                PathBuf::from(u).join(".cache").join("pyrst")
            } else {
                return Err(Error::Pkg(
                    "cannot locate a clone cache: set PYRST_CACHE (or LOCALAPPDATA/USERPROFILE)".into(),
                ));
            }
        }
        #[cfg(not(windows))]
        {
            if let Some(h) = std::env::var_os("HOME") {
                PathBuf::from(h).join(".cache").join("pyrst")
            } else {
                return Err(Error::Pkg(
                    "cannot locate a clone cache: neither PYRST_CACHE nor HOME is set".into(),
                ));
            }
        }
    };
    Ok(base.join("clones"))
}

// ── cache management (`pyrst cache …`) ───────────────────────────────────────

/// Dispatch `pyrst cache <dir|list|clean>`. The clone cache is GLOBAL (independent
/// of any active env); its location is `$PYRST_CACHE` (else `~/.cache/pyrst`)
/// `/clones/`, overridable with `--cache <dir>` (mirrors `PYRST_CACHE`).
pub fn cache_command(sub: Option<&str>) -> Result<()> {
    match sub {
        Some("dir") => cache_dir(),
        Some("list") => cache_list(),
        Some("clean") => cache_clean(),
        Some(other) => Err(Error::Pkg(format!(
            "unknown cache subcommand `{}` (expected: dir, list, or clean)",
            other
        ))),
        None => Err(Error::Pkg(
            "cache requires a subcommand: dir, list, or clean".into(),
        )),
    }
}

/// `pyrst cache dir` — print the clone-cache path (whether or not it exists yet).
fn cache_dir() -> Result<()> {
    println!("{}", clones_dir()?.display());
    Ok(())
}

/// `pyrst cache list` — one cached clone per line (`<url> @ <short-sha>   <size>`),
/// sorted for determinism, then a total. Reads only the on-disk cache; no git calls.
fn cache_list() -> Result<()> {
    let clones = clones_dir()?;
    let mut entries: Vec<(String, u64)> = Vec::new();
    if clones.is_dir() {
        for e in std::fs::read_dir(&clones)?.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(".tmp-") {
                continue; // an in-flight clone, not a finished cache entry
            }
            entries.push((name, dir_size(&e.path())));
        }
    }
    entries.sort();
    if entries.is_empty() {
        println!("clone cache is empty ({})", clones.display());
        return Ok(());
    }
    let mut total = 0u64;
    for (name, size) in &entries {
        total += *size;
        let (url, short) = parse_cache_entry(name);
        println!("{} @ {}   {}", url, short, human_size(*size));
    }
    println!("total: {} clone(s), {}", entries.len(), human_size(total));
    Ok(())
}

/// `pyrst cache clean` — remove the WHOLE clone cache, reporting what/how-much.
fn cache_clean() -> Result<()> {
    let clones = clones_dir()?;
    if !clones.is_dir() {
        println!("clone cache is already empty ({})", clones.display());
        return Ok(());
    }
    let mut count = 0usize;
    for e in std::fs::read_dir(&clones)?.flatten() {
        if e.path().is_dir() && !e.file_name().to_string_lossy().starts_with(".tmp-") {
            count += 1;
        }
    }
    let total = dir_size(&clones);
    std::fs::remove_dir_all(&clones)?;
    println!(
        "removed {} cached clone(s) ({}) from {}",
        count,
        human_size(total),
        clones.display()
    );
    Ok(())
}

/// Recursive on-disk size of `path` in bytes. Symlinks are not followed (a symlink
/// is neither `is_dir` nor `is_file` via `file_type`), so there is no cycle risk.
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(path) {
        for e in rd.flatten() {
            match e.file_type() {
                Ok(ft) if ft.is_dir() => total += dir_size(&e.path()),
                Ok(ft) if ft.is_file() => {
                    if let Ok(md) = e.metadata() {
                        total += md.len();
                    }
                }
                _ => {}
            }
        }
    }
    total
}

/// Human-readable byte size (`0B`, `512B`, `1.5K`, `1.0M`, …). Deterministic.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    if bytes < 1024 {
        return format!("{}B", bytes);
    }
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{:.1}{}", v, UNITS[u])
}

/// Recover a readable `(url-ish, short-sha)` from a cache directory name
/// `<sanitized-url>-<8hex>@<40hex-sha>` for display. Purely lexical — no git.
fn parse_cache_entry(name: &str) -> (String, String) {
    match name.rsplit_once('@') {
        Some((left, sha)) => {
            let url = strip_hash_suffix(left);
            let short: String = sha.chars().take(10).collect();
            (url, short)
        }
        None => (name.to_string(), String::new()),
    }
}

/// Drop the trailing `-<8 hex>` djb2 disambiguator from a sanitized-url prefix.
fn strip_hash_suffix(left: &str) -> String {
    if let Some((head, tail)) = left.rsplit_once('-') {
        if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_hexdigit()) {
            return head.to_string();
        }
    }
    left.to_string()
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

/// Coarse classification of a git failure so the user sees a clear one-liner rather
/// than a raw `fatal:` dump.
#[derive(Debug, PartialEq, Eq)]
enum GitFailure {
    /// Auth required / private repo / repo-not-found.
    Auth,
    /// Host unreachable / offline / DNS failure.
    Unreachable,
    /// Anything else — fall back to git's own message.
    Other,
}

/// Classify git's stderr (case-insensitive substring match on the well-known
/// phrases). Network problems are checked first: an offline clone fails at the
/// transport before any auth exchange.
fn classify_git_error(stderr: &str) -> GitFailure {
    let s = stderr.to_ascii_lowercase();
    const NET: [&str; 7] = [
        "could not resolve host",
        "could not resolve proxy",
        "failed to connect",
        "connection refused",
        "connection timed out",
        "network is unreachable",
        "temporary failure in name resolution",
    ];
    const AUTH: [&str; 8] = [
        "authentication failed",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "permission denied",
        "access denied",
        "repository not found",
        "invalid username or password",
    ];
    if NET.iter().any(|p| s.contains(p)) {
        GitFailure::Unreachable
    } else if AUTH.iter().any(|p| s.contains(p)) {
        GitFailure::Auth
    } else {
        GitFailure::Other
    }
}

/// Best-effort host extraction for the offline message: handles `scheme://host/…`,
/// scp `user@host:path`, and a bare `host/…`.
fn host_of(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    let hostport = authority.rsplit_once('@').map(|(_, r)| r).unwrap_or(authority);
    let host = hostport.split(':').next().unwrap_or(hostport);
    if host.is_empty() {
        url.to_string()
    } else {
        host.to_string()
    }
}

/// Turn a failed git invocation into an HONEST, specific one-liner — the common
/// auth/offline cases mapped to a clear explanation, otherwise git's own final
/// `fatal:` line. (`GIT_TERMINAL_PROMPT=0`, set in `run_git`, guarantees an
/// auth-required repo fails HERE instead of hanging on a credential prompt.)
fn friendly_git_error(url: &str, out: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    match classify_git_error(&stderr) {
        GitFailure::Auth => "authentication failed or repository not found (is it private? \
             pyrst has no credential handling yet)"
            .to_string(),
        GitFailure::Unreachable => format!("could not reach {} (offline?)", host_of(url)),
        GitFailure::Other => git_error_tail(out),
    }
}

// ── public resolve/clone entry points ────────────────────────────────────────

/// Resolve + clone from a raw install spec (`url`, `url@ref`, or `url#sha`).
pub fn resolve_spec(spec: &str) -> Result<GitCheckout> {
    let (url, refspec) = parse_url_spec(spec)?;
    resolve(&url, &refspec)
}

/// Resolve + clone `url` at `refspec` into the SHA-keyed cache, returning the
/// checkout dir + clean url + resolved commit SHA. Idempotent: a cached commit is
/// reused without re-fetching.
///
/// For a bare URL or an `@ref`, the target commit is resolved CHEAPLY first with
/// `git ls-remote` so a commit already in the SHA-keyed cache is a pure cache HIT —
/// no clone. Only on a cache MISS (or if `ls-remote` is unavailable) do we actually
/// clone. This is what makes a diamond dependency referenced via a bare URL from N
/// siblings fetch once rather than N times.
pub fn resolve(url: &str, refspec: &RefSpec) -> Result<GitCheckout> {
    ensure_git()?;
    let clones = clones_dir()?;
    std::fs::create_dir_all(&clones)?;

    match refspec {
        // The SHA is known up front → cache dir is known → check the cache first.
        RefSpec::Commit(sha) => {
            let dst = cache_path(&clones, url, sha)?;
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
        // The SHA is unknown until resolved. Try `git ls-remote` FIRST (cheap): if
        // that commit is already cached, reuse it without cloning. Clone only on a
        // miss (or if ls-remote gives us nothing).
        RefSpec::Default | RefSpec::Ref(_) => {
            if let Some(sha) = ls_remote_sha(url, refspec) {
                let dst = cache_path(&clones, url, &sha)?;
                if is_valid_clone(&dst) {
                    return Ok(GitCheckout { dir: dst, url: url.to_string(), commit: sha });
                }
            }
            // Miss (or ls-remote unsupported): clone to a temp dir, resolve the
            // authoritative HEAD, then move into the SHA-keyed cache (deduping
            // against a concurrent hit).
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
            let dst = match cache_path(&clones, url, &sha) {
                Ok(d) => d,
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&tmp);
                    return Err(e);
                }
            };
            if is_valid_clone(&dst) {
                let _ = std::fs::remove_dir_all(&tmp);
            } else {
                finalize(&tmp, &dst)?;
            }
            Ok(GitCheckout { dir: dst, url: url.to_string(), commit: sha })
        }
    }
}

/// Build the SHA-keyed cache directory for `<url>@<sha>`, refusing anything unsafe.
/// A pinned commit is ALWAYS a 40-char lowercase hex string; a value that is not (a
/// caller-controlled `#<sha>`, a hand-edited lock, or unexpected git output) must
/// never shape a filesystem path. Defence in depth against path traversal: the sha
/// is validated, AND the resulting dir must be a single segment directly under the
/// clone-cache root.
fn cache_path(clones: &Path, url: &str, sha: &str) -> Result<PathBuf> {
    if !is_pinned_sha(sha) {
        return Err(Error::Pkg(format!(
            "refusing to use a non-40-hex commit sha in a cache path: {:?}",
            sha
        )));
    }
    let dst = clones.join(cache_key(url, sha));
    if dst.parent() != Some(clones) {
        return Err(Error::Pkg(format!(
            "refusing an unsafe clone-cache path for commit {}",
            sha
        )));
    }
    Ok(dst)
}

/// A canonical pinned commit: exactly 40 lowercase hex digits (git's own output
/// form). By construction it holds no path separators or `..`, so it is safe to
/// interpolate into a cache path.
fn is_pinned_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Resolve the commit a bare URL / `@ref` points to CHEAPLY (no full clone) via
/// `git ls-remote`. Returns `None` — so the caller falls back to cloning — if git,
/// the server, or the ref does not cooperate. For an annotated tag we also request
/// the peeled `<ref>^{}` line so we get the COMMIT the tag names (what a clone would
/// check out), not the intermediate tag object.
fn ls_remote_sha(url: &str, refspec: &RefSpec) -> Option<String> {
    let (refname, peel) = match refspec {
        RefSpec::Default => ("HEAD".to_string(), None),
        RefSpec::Ref(r) => (r.clone(), Some(format!("{}^{{}}", r))),
        RefSpec::Commit(_) => return None,
    };
    let mut args: Vec<&str> = vec!["ls-remote", "--", url, refname.as_str()];
    if let Some(p) = peel.as_deref() {
        args.push(p);
    }
    let out = run_git(&args, None).ok()?;
    if !out.status.success() {
        return None;
    }
    parse_ls_remote(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `git ls-remote` output into a commit sha. Each line is `<sha>\t<ref>`; an
/// annotated tag also emits a peeled `<sha>\t<ref>^{}` line whose sha is the COMMIT
/// the tag names — prefer it. Otherwise the first well-formed sha wins.
fn parse_ls_remote(stdout: &str) -> Option<String> {
    let mut first: Option<String> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split_whitespace();
        let sha = match cols.next() {
            Some(s) if is_pinned_sha(s) => s,
            _ => continue,
        };
        let refname = cols.next().unwrap_or("");
        if refname.ends_with("^{}") {
            return Some(sha.to_string());
        }
        if first.is_none() {
            first = Some(sha.to_string());
        }
    }
    first
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
        return Err(Error::Pkg(format!("failed to {}: {}", what, friendly_git_error(url, &out))));
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
            friendly_git_error(url, &fetch)
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

    /// A valid 40-char lowercase hex commit for `#sha` tests.
    const SHA40: &str = "d3543dfe79920046e4b281b377af14bbd2026be2";

    #[test]
    fn parse_bare_url_is_default() {
        let (u, r) = parse_url_spec("https://github.com/Elouderb/numpyrs").unwrap();
        assert_eq!(u, "https://github.com/Elouderb/numpyrs");
        assert_eq!(r, RefSpec::Default);
    }

    #[test]
    fn parse_at_ref() {
        let (u, r) = parse_url_spec("https://github.com/Elouderb/numpyrs@v0.2.0").unwrap();
        assert_eq!(u, "https://github.com/Elouderb/numpyrs");
        assert_eq!(r, RefSpec::Ref("v0.2.0".into()));
    }

    #[test]
    fn parse_hash_sha() {
        let (u, r) =
            parse_url_spec(&format!("https://github.com/Elouderb/numpyrs#{SHA40}")).unwrap();
        assert_eq!(u, "https://github.com/Elouderb/numpyrs");
        assert_eq!(r, RefSpec::Commit(SHA40.into()));
    }

    #[test]
    fn parse_rejects_non_40_hex_sha() {
        // Abbreviated shas are ambiguous; a traversal payload must never parse.
        assert!(parse_url_spec("file:///tmp/x/repo#abc123").is_err());
        assert!(parse_url_spec("file:///tmp/x/repo#deadbeef").is_err());
        assert!(parse_url_spec("file:///tmp/x/repo#../../../../etc/passwd").is_err());
        // 40 chars but not all hex → rejected.
        let forty_non_hex = "z".repeat(40);
        assert!(parse_url_spec(&format!("file:///tmp/x/repo#{forty_non_hex}")).is_err());
    }

    #[test]
    fn parse_normalizes_valid_sha_to_lowercase() {
        let up = "ABCDEF0123456789ABCDEF0123456789ABCDEF01";
        let (_, r) = parse_url_spec(&format!("file:///r#{up}")).unwrap();
        assert_eq!(r, RefSpec::Commit(up.to_ascii_lowercase()));
    }

    #[test]
    fn scp_authority_at_is_not_a_ref() {
        // The `git@` authority `@` precedes the last `/`, so it is NOT a ref.
        let (u, r) = parse_url_spec("git@github.com:Elouderb/numpyrs").unwrap();
        assert_eq!(u, "git@github.com:Elouderb/numpyrs");
        assert_eq!(r, RefSpec::Default);
    }

    #[test]
    fn scp_form_with_trailing_ref() {
        let (u, r) = parse_url_spec("git@github.com:Elouderb/numpyrs.git@v1.0").unwrap();
        assert_eq!(u, "git@github.com:Elouderb/numpyrs.git");
        assert_eq!(r, RefSpec::Ref("v1.0".into()));
    }

    #[test]
    fn https_userinfo_at_is_not_a_ref() {
        let (u, r) = parse_url_spec("https://user@host.example/owner/repo").unwrap();
        assert_eq!(u, "https://user@host.example/owner/repo");
        assert_eq!(r, RefSpec::Default);
    }

    #[test]
    fn file_url_forms() {
        assert_eq!(parse_url_spec("file:///tmp/x/repo").unwrap().1, RefSpec::Default);
        assert_eq!(
            parse_url_spec("file:///tmp/x/repo@v0.1.0").unwrap().1,
            RefSpec::Ref("v0.1.0".into())
        );
        let (u, r) = parse_url_spec(&format!("file:///tmp/x/repo#{SHA40}")).unwrap();
        assert_eq!(u, "file:///tmp/x/repo");
        assert_eq!(r, RefSpec::Commit(SHA40.into()));
    }

    #[test]
    fn is_pinned_sha_only_accepts_40_lowercase_hex() {
        assert!(is_pinned_sha(SHA40));
        assert!(!is_pinned_sha("ABCDEF0123456789ABCDEF0123456789ABCDEF01")); // uppercase
        assert!(!is_pinned_sha("abc123")); // too short
        assert!(!is_pinned_sha("../../../../etc/passwd"));
        assert!(!is_pinned_sha(""));
    }

    #[test]
    fn cache_path_rejects_traversal_and_stays_under_clones() {
        let clones = Path::new("/var/cache/pyrst/clones");
        // A valid pinned sha yields a single child segment directly under `clones`.
        let dst = cache_path(clones, "https://h/o/r", SHA40).unwrap();
        assert_eq!(dst.parent(), Some(clones));
        // A non-pinned sha is refused before it can shape a path.
        assert!(cache_path(clones, "https://h/o/r", "../../etc").is_err());
        assert!(cache_path(clones, "https://h/o/r", "abc123").is_err());
    }

    #[test]
    fn parse_ls_remote_prefers_peeled_commit() {
        // Annotated tag: the peeled `^{}` line is the COMMIT the tag names.
        let out = "639edb2989ab785ebcbaf93d8a2268943a294d99\trefs/tags/v0.2.0\n\
                   be48a1d348e5fd0cdce0299e1a4f1f541fe28e5e\trefs/tags/v0.2.0^{}\n";
        assert_eq!(
            parse_ls_remote(out).as_deref(),
            Some("be48a1d348e5fd0cdce0299e1a4f1f541fe28e5e")
        );
    }

    #[test]
    fn parse_ls_remote_plain_and_empty() {
        let out = "5c42b6fc7b92ac14e2a642bb47b0092133e87806\tHEAD\n";
        assert_eq!(
            parse_ls_remote(out).as_deref(),
            Some("5c42b6fc7b92ac14e2a642bb47b0092133e87806")
        );
        assert_eq!(parse_ls_remote("").as_deref(), None);
        assert_eq!(parse_ls_remote("garbage line\n").as_deref(), None);
    }

    #[test]
    fn classify_git_error_maps_auth_and_offline() {
        assert_eq!(
            classify_git_error(
                "fatal: could not read Username for 'https://x': terminal prompts disabled"
            ),
            GitFailure::Auth
        );
        assert_eq!(
            classify_git_error("remote: Repository not found.\nfatal: repository 'https://x' not found"),
            GitFailure::Auth
        );
        assert_eq!(
            classify_git_error("fatal: unable to access 'https://x': Could not resolve host: x"),
            GitFailure::Unreachable
        );
        assert_eq!(
            classify_git_error("fatal: '/x' does not appear to be a git repository"),
            GitFailure::Other
        );
    }

    #[test]
    fn host_of_handles_url_forms() {
        assert_eq!(host_of("https://github.com/o/r"), "github.com");
        assert_eq!(host_of("https://user@github.com/o/r"), "github.com");
        assert_eq!(host_of("git@github.com:o/r"), "github.com");
        assert_eq!(host_of("ssh://git@host.example:2222/o/r"), "host.example");
    }

    #[test]
    fn human_size_is_readable() {
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(512), "512B");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(1024 * 1024), "1.0M");
    }

    #[test]
    fn parse_cache_entry_recovers_url_and_short_sha() {
        let name = cache_key("https://github.com/Elouderb/numpyrs", SHA40);
        let (url, short) = parse_cache_entry(&name);
        assert!(url.contains("github.com"), "recovered url: {url}");
        assert!(!url.contains('@'), "no sha in the url part: {url}");
        assert_eq!(short, &SHA40[..10]);
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
