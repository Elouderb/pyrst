#!/usr/bin/env bash
# test_pkg_git.sh — pyrst package-management (PKG Phase 2) git-install harness.
#
# HERMETIC BY CONSTRUCTION: everything runs in a throwaway temp dir against LOCAL
# `file://` git fixtures — NO network, and PYRST_CACHE is pointed at a temp dir so
# the real ~/.cache is never touched. `pyrst install <git-url>` is `git clone` under
# the hood and git is transport-agnostic, so file:// repos exercise the exact same
# code path GitHub would. Exercises:
#   1.  `pyrst install file://.../a` into a fresh env -> a + TRANSITIVE git dep b
#   2.  `pyrst list` / build+run a consumer against the env (no PYRST_PATH)
#   3.  `pyrst freeze` pins commit SHAs (git\turl\t<40-hex>)
#   4.  wipe store + cache, `pyrst install` (no arg) -> re-clone at pinned SHAs,
#       byte-reproducible (emit + freeze identical)
#   5.  a repo with NO pyrst.yaml -> honest "not a pyrst package" error
#   6.  `url@<tag>` installs that tag (pins the tag's SHA)
#   7.  `url#<sha>` installs that exact commit
#   8.  a bad URL -> clean error (not a raw git dump)
#   9.  idempotent re-install of the same url@SHA
#   10. cache lives under PYRST_CACHE (hermeticity assertion)
#
# Exit 0 only when every check passes. Not run by test_all.sh (packaging is
# additive + env-gated); run it explicitly. Requires git on PATH.

set -u
BIN="$(cd "$(dirname "$0")" && pwd)/target/release/pyrst"
[[ -x "$BIN" ]] || { echo "FATAL: build the release binary first (cargo build --release)"; exit 1; }
command -v git >/dev/null 2>&1 || { echo "FATAL: git is required on PATH"; exit 1; }

pass=0; fail=0
ok()  { echo "  PASS: $1"; pass=$((pass+1)); }
bad() { echo "  FAIL: $1"; fail=$((fail+1)); }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/pyrst-pkg-git.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# ── hermeticity: no ambient env/path/creds leak in; cache -> temp ────────────
unset PYRST_PATH
unset PYRST_VENV
export PYRST_CACHE="$WORK/cache"            # <-- clone cache lands HERE, not ~/.cache
export GIT_CONFIG_GLOBAL=/dev/null          # deterministic, config-independent git
export GIT_CONFIG_SYSTEM=/dev/null
export GIT_TERMINAL_PROMPT=0

git_commit() { git -C "$1" -c user.email=t@pyrst -c user.name=pyrst commit -q "${@:2}"; }

# ── author fixture GIT repos ────────────────────────────────────────────────
REPOS="$WORK/repos"; mkdir -p "$REPOS"

# leaf package `b` (no deps)
mkrepo_b() {
  local d="$REPOS/b"; mkdir -p "$d"; git -C "$d" init -q -b main
  printf 'name: b\nversion: 0.1.0\npackage_root: .\ndependencies: []\n' > "$d/pyrst.yaml"
  printf 'def b_val() -> int:\n    return 5\n' > "$d/core.pyrs"
  git -C "$d" add -A; git_commit "$d" -m "b v0.1.0"
  git -C "$d" tag v0.1.0
}
mkrepo_b
B_URL="file://$REPOS/b"

# package `a` depends on `b` via a GIT url (embedded in a's committed manifest)
mkrepo_a() {
  local d="$REPOS/a"; mkdir -p "$d"; git -C "$d" init -q -b main
  { echo "name: a"; echo "version: 0.1.0"; echo "package_root: ."; \
    echo "dependencies:"; echo "  - name: b"; echo "    git: $B_URL"; } > "$d/pyrst.yaml"
  printf 'from b.core import b_val\n\ndef a_val() -> int:\n    return b_val() + 3\n' > "$d/core.pyrs"
  git -C "$d" add -A; git_commit "$d" -m "a v0.1.0"
}
mkrepo_a
A_URL="file://$REPOS/a"

# a repo that is NOT a pyrst package (no pyrst.yaml)
mkrepo_nonpkg() {
  local d="$REPOS/nonpkg"; mkdir -p "$d"; git -C "$d" init -q -b main
  printf '# just a readme\n' > "$d/README.md"
  git -C "$d" add -A; git_commit "$d" -m "readme"
}
mkrepo_nonpkg
NON_URL="file://$REPOS/nonpkg"

# ── fresh env ───────────────────────────────────────────────────────────────
ENVROOT="$WORK/proj"; mkdir -p "$ENVROOT"
ENV="$ENVROOT/.pyrstenv"
( cd "$ENVROOT" && "$BIN" venv .pyrstenv >/dev/null 2>&1 )

# ── 1. install a git url -> transitive git dep ──────────────────────────────
echo "[1] pyrst install <git-url> (transitive git dep)"
out="$("$BIN" --venv "$ENV" install "$A_URL" 2>&1)"
echo "$out" | grep -q "installed 2 package(s)" \
  && ok "install reports 2 packages (a + transitive git dep b)" \
  || bad "unexpected install count: $out"
for p in a b; do
  [[ -f "$ENV/packages/$p/core.pyrs" && -f "$ENV/packages/$p/pyrst.yaml" ]] \
    && ok "store holds $p (module + manifest)" || bad "store missing $p"
done
# the store must NOT contain a leaked .git directory
[[ ! -d "$ENV/packages/a/.git" && ! -d "$ENV/packages/b/.git" ]] \
  && ok "no .git leaked into the store" || bad ".git leaked into the store"

# ── 2. list + build/run a consumer (no PYRST_PATH) ──────────────────────────
echo "[2] list + build+run consumer"
lst="$("$BIN" --venv "$ENV" list 2>&1)"
echo "$lst" | grep -q "a@0.1.0" && echo "$lst" | grep -q "b@0.1.0" \
  && ok "list shows a + b" || bad "list wrong: $lst"
APP="$WORK/app"; mkdir -p "$APP"
cat > "$APP/prog.pyrs" <<'EOF'
from a.core import a_val

def main() -> None:
    print(a_val())
EOF
( cd "$APP" && env -u PYRST_PATH PYRST_VENV="$ENV" "$BIN" build prog.pyrs >/dev/null 2>&1 )
if [[ -x "$APP/prog" ]]; then
  got="$("$APP/prog")"
  [[ "$got" == "8" ]] && ok "consumer builds+runs against the git env (a_val()=8)" || bad "wrong output: $got"
else
  bad "consumer failed to build against the git env"
fi

# ── 3. freeze pins commit SHAs ──────────────────────────────────────────────
echo "[3] freeze pins commit SHAs"
frz="$("$BIN" --venv "$ENV" freeze 2>&1)"
echo "$frz" | grep -qE $'a\t0.1.0\tgit\t.+\t[0-9a-f]{40}' \
  && echo "$frz" | grep -qE $'b\t0.1.0\tgit\t.+\t[0-9a-f]{40}' \
  && ok "freeze pins a + b to full commit SHAs" || bad "freeze not SHA-pinned: $frz"

# capture the reference emit for the reproducibility check
env -u PYRST_PATH PYRST_VENV="$ENV" "$BIN" emit "$APP/prog.pyrs" > "$WORK/emit.before" 2>/dev/null

# ── 10. hermeticity: the clone cache is under PYRST_CACHE ────────────────────
echo "[10] hermetic clone cache (PYRST_CACHE)"
[[ -d "$PYRST_CACHE/clones" ]] && [[ -n "$(ls -A "$PYRST_CACHE/clones" 2>/dev/null)" ]] \
  && ok "clones landed under PYRST_CACHE ($PYRST_CACHE/clones)" \
  || bad "clone cache not under PYRST_CACHE"

# ── 4. wipe store + cache, reproduce from lock -> byte-identical ─────────────
echo "[4] reproduce from lock (cold cache) -> byte-reproducible"
rm -rf "$ENV/packages"/* "$PYRST_CACHE/clones"           # cold: force a real re-clone
mkdir -p "$ENV/packages"
rout="$("$BIN" --venv "$ENV" install 2>&1)"              # no arg = reproduce from lock
echo "$rout" | grep -q "installed 2 package(s)" \
  && ok "no-arg install re-clones 2 packages from the lock" || bad "reproduce count wrong: $rout"
frz2="$("$BIN" --venv "$ENV" freeze 2>&1)"
[[ "$frz" == "$frz2" ]] && ok "freeze (pinned SHAs) unchanged after reproduce" || bad "SHAs drifted on reproduce"
env -u PYRST_PATH PYRST_VENV="$ENV" "$BIN" emit "$APP/prog.pyrs" > "$WORK/emit.after" 2>/dev/null
diff -q "$WORK/emit.before" "$WORK/emit.after" >/dev/null \
  && ok "emit byte-identical after cold re-clone from lock" || bad "emit differs after reproduce"

# ── 5. non-pyrst repo -> honest error ───────────────────────────────────────
echo "[5] non-pyrst repo -> honest error"
ENV5="$WORK/p5/.pyrstenv"; mkdir -p "$WORK/p5"; ( cd "$WORK/p5" && "$BIN" venv .pyrstenv >/dev/null 2>&1 )
e5="$("$BIN" --venv "$ENV5" install "$NON_URL" 2>&1)"
echo "$e5" | grep -q "is not a pyrst package" && echo "$e5" | grep -q "pyrst.yaml" \
  && ok "a repo without pyrst.yaml is an honest 'not a pyrst package' error" || bad "non-pkg error not honest: $e5"

# ── 6. url@<tag> installs that tag ──────────────────────────────────────────
echo "[6] install url@<tag>"
ENV6="$WORK/p6/.pyrstenv"; mkdir -p "$WORK/p6"; ( cd "$WORK/p6" && "$BIN" venv .pyrstenv >/dev/null 2>&1 )
"$BIN" --venv "$ENV6" install "$B_URL@v0.1.0" >/dev/null 2>&1
TAG_SHA="$(git -C "$REPOS/b" rev-parse v0.1.0)"
f6="$("$BIN" --venv "$ENV6" freeze 2>&1)"
echo "$f6" | grep -q "$TAG_SHA" && ok "url@v0.1.0 pins the tag's commit SHA" || bad "tag install wrong: $f6"

# ── 7. url#<sha> installs that exact commit ─────────────────────────────────
echo "[7] install url#<sha>"
ENV7="$WORK/p7/.pyrstenv"; mkdir -p "$WORK/p7"; ( cd "$WORK/p7" && "$BIN" venv .pyrstenv >/dev/null 2>&1 )
HEAD_SHA="$(git -C "$REPOS/b" rev-parse HEAD)"
"$BIN" --venv "$ENV7" install "$B_URL#$HEAD_SHA" >/dev/null 2>&1
f7="$("$BIN" --venv "$ENV7" freeze 2>&1)"
echo "$f7" | grep -q "$HEAD_SHA" && ok "url#<sha> pins that exact commit" || bad "sha install wrong: $f7"

# ── 8. bad URL -> clean error ───────────────────────────────────────────────
echo "[8] bad URL -> clean error"
ENV8="$WORK/p8/.pyrstenv"; mkdir -p "$WORK/p8"; ( cd "$WORK/p8" && "$BIN" venv .pyrstenv >/dev/null 2>&1 )
e8="$("$BIN" --venv "$ENV8" install "file://$REPOS/does-not-exist" 2>&1)"
echo "$e8" | grep -q "failed to clone" \
  && ok "a bad URL yields a clean 'failed to clone' error (no raw dump)" || bad "bad-url error not clean: $e8"

# ── 9. idempotent re-install of the same url ────────────────────────────────
echo "[9] idempotent re-install"
r9="$("$BIN" --venv "$ENV" install "$A_URL" 2>&1)"
echo "$r9" | grep -q "installed 2 package(s)" \
  && ok "re-installing the same url@SHA is a clean idempotent no-op" || bad "re-install not idempotent: $r9"

echo "══════════════════════════════════════════════"
echo "PKG GIT INTEGRATION: $pass passed, $fail failed"
echo "══════════════════════════════════════════════"
[[ $fail -eq 0 ]] && { echo "ALL PKG GIT TESTS PASSED"; exit 0; } || { echo "PKG GIT TESTS FAILED"; exit 1; }
