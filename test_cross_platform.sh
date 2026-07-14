#!/usr/bin/env bash
# test_cross_platform.sh — cross-platform regression guard for the embedded stdlib
# (card 54c2bc10).
#
# WHAT IT PROVES
#   The embedded stdlib (lib/os.pyrs, lib/shutil.pyrs, lib/tempfile.pyrs) once
#   hardcoded `std::os::unix::…` in six @extern snippets. Importing ANY symbol
#   from those modules compiles ALL of the module's reachable functions, so a
#   Windows user importing e.g. kodiak (which imports os) hit
#   `error[E0433]: could not find 'unix' in 'os'`. The unix-only paths are now
#   `#[cfg(unix)]`-gated with a portable / windows arm. This guard emits a small
#   corpus touching os / shutil / tempfile (plus `import kodiak`) and TYPE-CHECKS
#   the emitted Rust for the Windows target, asserting 0 errors — and re-checks
#   the Linux target so the unix path is proven untouched.
#
# HOW (no Windows box, no linker needed)
#   Type-checking for a target — `rustc --emit=metadata` for crate-free programs,
#   or `cargo check --target` for programs that pull an `@crate` dependency (e.g.
#   tempfile's random suffix uses `getrandom`) — runs the full front-end WITHOUT
#   invoking a linker. That is exactly what surfaces the E0433/E0599
#   "no such item / method on this platform" class this card fixes.
#
# PREREQUISITE (one-time):
#   rustup target add x86_64-pc-windows-gnu
#   The Linux host target is assumed present. If the Windows target is absent this
#   script SKIPS WITH A NOTICE and exits 0 — it never silently passes a real
#   failure, but it also does not fail on a box that simply lacks the cross target.
#   (A program that pulls an @crate dep runs `cargo check`, which may fetch that
#   crate's Windows deps the first time — same trust/network model as `pyrst build`.)
#
# Exit code: 0 only when every program cross-checks with 0 errors on every
# available target (or the Windows target is not installed -> skip). Non-zero on
# any type-check failure.

set -u
cd "$(dirname "$0")"

BIN=./target/release/pyrst
WIN_TARGET=x86_64-pc-windows-gnu
LINUX_TARGET=x86_64-unknown-linux-gnu
PKG_PATH=extern/packages

RED=$'\033[31m'; GRN=$'\033[32m'; YLW=$'\033[33m'; RST=$'\033[0m'
ok()   { echo "  ${GRN}ok${RST}   $*"; }
bad()  { echo "  ${RED}FAIL${RST} $*"; }
note() { echo "  ${YLW}note${RST} $*"; }

failures=0

if [[ ! -x "$BIN" ]]; then
  echo "${RED}error${RST}: $BIN not found — run 'cargo build --release' first" >&2
  exit 2
fi

# Which targets can we actually check? Linux host is required; Windows is the
# point of this guard but is skip-with-notice if the cross target is absent.
installed="$(rustup target list --installed 2>/dev/null)"
targets=()
if grep -q "^$LINUX_TARGET$" <<<"$installed"; then
  targets+=("$LINUX_TARGET")
else
  note "$LINUX_TARGET not installed — skipping the Linux baseline re-check"
fi
if grep -q "^$WIN_TARGET$" <<<"$installed"; then
  targets+=("$WIN_TARGET")
else
  note "$WIN_TARGET not installed — SKIPPING the Windows cross-check."
  note "install it with:  rustup target add $WIN_TARGET"
fi
if [[ ${#targets[@]} -eq 0 ]]; then
  note "no usable rustc targets installed — nothing to check; skipping."
  exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
# One reusable cargo project with a SHARED target dir so dependency crates are
# compiled once and cached across programs/targets.
CC_PROJ="$WORK/_ccproj"
mkdir -p "$CC_PROJ/src"

# Map a crate IDENT as it appears in emitted Rust (`getrandom::…`) to the
# `name = "version"` line pyrst's driver would write into Cargo.toml. Kept in
# sync with the @crate decorators across lib/*.pyrs.
crate_dep_for() {
  case "$1" in
    getrandom) echo 'getrandom = "0.2"' ;;
    regex)     echo 'regex = "1"' ;;
    sha2)      echo 'sha2 = "0.10"' ;;
    sha1)      echo 'sha1 = "0.10"' ;;
    md5)       echo 'md-5 = "0.10"' ;;
    hmac)      echo 'hmac = "0.12"' ;;
    *)         echo '' ;;
  esac
}

# Emit deps needed by an emitted-Rust file, one Cargo line per crate referenced.
deps_of() {
  local rs="$1" ident line
  for ident in getrandom regex sha2 sha1 md5 hmac; do
    if grep -q "\\b${ident}::" "$rs"; then
      line="$(crate_dep_for "$ident")"
      [[ -n "$line" ]] && echo "$line"
    fi
  done
}

# Type-check an emitted .rs for one target. Crate-free -> fast rustc metadata;
# otherwise a `cargo check` against a materialized crate. Echoes the error count.
typecheck() {
  local rs="$1" t="$2" deps="$3" out
  if [[ -z "$deps" ]]; then
    # --out-dir keeps the emitted .rmeta in the temp workdir, never the repo root.
    out="$(rustc --target "$t" --edition 2021 --emit=metadata --crate-type bin --out-dir "$WORK" "$rs" 2>&1)"
  else
    cp "$rs" "$CC_PROJ/src/main.rs"
    printf '[package]\nname = "xcheck"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\n%s\n' "$deps" > "$CC_PROJ/Cargo.toml"
    out="$( (cd "$CC_PROJ" && cargo check --quiet --target "$t") 2>&1 )"
  fi
  LAST_ERR_OUT="$out"
  grep -cE '^error(\[|:| )' <<<"$out"
}

# ── Corpus ───────────────────────────────────────────────────────────────────
# Each program touches one of the previously-unix-only stdlib surfaces.

cat > "$WORK/os_stat.pyrs" <<'PY'
# os.stat -> _os_stat_raw (was: std::os::unix::fs::MetadataExt m.mode())
from os import stat

def main() -> None:
    s = stat(".")
    print(s.st_size)
    print(s.st_mode)
PY

cat > "$WORK/os_rw.pyrs" <<'PY'
# os.read_file/write_file — the exact surface kodiak imports; importing it
# still pulls in the (now cfg-gated) _os_stat_raw.
from os import read_file, write_file

def main() -> None:
    write_file("cross_rw.tmp", "hello")
    print(read_file("cross_rw.tmp"))
PY

cat > "$WORK/shutil_prog.pyrs" <<'PY'
# copyfile -> _shutil_same_file (was dev/ino via MetadataExt); copytree with
# symlinks=True -> _shutil_make_symlink (was std::os::unix::fs::symlink).
from shutil import copyfile, copytree

def main() -> None:
    copyfile("a.txt", "b.txt")
    copytree("src_dir", "dst_dir", symlinks=True)
PY

cat > "$WORK/tempfile_prog.pyrs" <<'PY'
# mkdtemp -> _tempfile_mkdir_exclusive (was DirBuilderExt.mode 0o700);
# mkstemp -> _tempfile_open_exclusive_fd (was AsRawFd + OpenOptionsExt.mode).
from tempfile import mkdtemp, mkstemp

def main() -> None:
    d = mkdtemp()
    print(d)
    fd, path = mkstemp()
    print(path)
PY

cat > "$WORK/kodiak_prog.pyrs" <<'PY'
# The user-reported blocker: importing kodiak transitively imports os, which
# used to drag in the unix-only _os_stat_raw and break the Windows build.
import kodiak

def main() -> None:
    print("kodiak imported")
PY

declare -a PROGS=(os_stat os_rw shutil_prog tempfile_prog kodiak_prog)

echo "cross-platform stdlib guard — targets: ${targets[*]}"
echo

for prog in "${PROGS[@]}"; do
  src="$WORK/$prog.pyrs"
  rs="$WORK/$prog.rs"
  if [[ "$prog" == kodiak_prog ]]; then
    PYRST_PATH="$PKG_PATH" "$BIN" emit "$src" > "$rs" 2> "$WORK/$prog.emit.err"
  else
    "$BIN" emit "$src" > "$rs" 2> "$WORK/$prog.emit.err"
  fi
  if [[ $? -ne 0 || ! -s "$rs" ]]; then
    bad "$prog: pyrst emit failed"
    sed 's/^/       /' "$WORK/$prog.emit.err"
    failures=$((failures + 1))
    continue
  fi
  deps="$(deps_of "$rs")"
  via="rustc --emit=metadata"
  [[ -n "$deps" ]] && via="cargo check ($(echo "$deps" | tr '\n' ',' | sed 's/,$//'))"
  for t in "${targets[@]}"; do
    errs="$(typecheck "$rs" "$t" "$deps")"
    if [[ "$errs" == "0" ]]; then
      ok "$prog  [$t]  0 errors   via $via"
    else
      bad "$prog  [$t]  $errs error(s)   via $via"
      grep -E '^error' <<<"$LAST_ERR_OUT" | head -8 | sed 's/^/       /'
      failures=$((failures + 1))
    fi
  done
done

echo
echo "══════════════════════════════════════════════"
if [[ $failures -gt 0 ]]; then
  echo "${RED}CROSS-PLATFORM GUARD FAILED${RST}: $failures failure(s)"
  exit 1
fi
echo "${GRN}CROSS-PLATFORM GUARD PASSED${RST}"
exit 0
