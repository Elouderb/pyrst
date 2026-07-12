#!/usr/bin/env bash
# test_pkg.sh — pyrst package-management (PKG Phase 1) integration harness.
#
# Drives the RELEASE binary end-to-end against SYNTHETIC fixture packages in a
# throwaway temp dir (never touches the repo, so it cannot pollute the ambient
# `.pyrstenv` auto-detect that test_all.sh relies on staying absent). Exercises:
#   1. `pyrst venv` skeleton
#   2. transitive local install with a DIAMOND (top -> mid -> leaf, top -> leaf)
#   3. build+run a consumer against the env with NO PYRST_PATH
#   4. an UNINSTALLED import -> honest env-aware error
#   5. a dependency CYCLE -> honest cycle error
#   6. determinism: install+emit into TWO separate envs -> byte-identical emit
#   7. `pyrst list` / `pyrst freeze`
#
# Exit code 0 only when every check passes. Not run by test_all.sh (packaging is
# additive and env-gated); run it explicitly.

set -u
BIN="$(cd "$(dirname "$0")" && pwd)/target/release/pyrst"
[[ -x "$BIN" ]] || { echo "FATAL: build the release binary first (cargo build --release)"; exit 1; }

pass=0; fail=0
ok()   { echo "  PASS: $1"; pass=$((pass+1)); }
bad()  { echo "  FAIL: $1"; fail=$((fail+1)); }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/pyrst-pkg-test.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
# Ensure NO ambient env/search path leaks into the run.
unset PYRST_PATH
unset PYRST_VENV

# ── author synthetic fixture packages ───────────────────────────────────────
SRC="$WORK/src"; mkdir -p "$SRC"

mkpkg() { # mkpkg <name> <manifest-deps-block> <module-body>
  local name="$1" deps="$2" body="$3"
  mkdir -p "$SRC/$name"
  { echo "name: $name"; echo "version: 0.1.0"; echo "package_root: ."; printf '%s\n' "$deps"; } > "$SRC/$name/pyrst.yaml"
  printf '%s\n' "$body" > "$SRC/$name/core.pyrs"
}

mkpkg leaf "dependencies: []" \
  "def leaf_val() -> int:
    return 7"

mkpkg mid "dependencies:
  - name: leaf
    path: ../leaf" \
  "from leaf.core import leaf_val

def mid_val() -> int:
    return leaf_val() + 10"

mkpkg top "dependencies:
  - name: mid
    path: ../mid
  - name: leaf
    path: ../leaf" \
  "from mid.core import mid_val
from leaf.core import leaf_val

def top_val() -> int:
    return mid_val() + leaf_val()"

# cycle fixtures
mkpkg cyca "dependencies:
  - name: cycb
    path: ../cycb" \
  "def a() -> int:
    return 1"
mkpkg cycb "dependencies:
  - name: cyca
    path: ../cyca" \
  "def b() -> int:
    return 2"

# ── 1. venv skeleton ────────────────────────────────────────────────────────
echo "[1] pyrst venv"
ENV1="$WORK/env1/.pyrstenv"
mkdir -p "$WORK/env1"
( cd "$WORK/env1" && "$BIN" venv .pyrstenv >/dev/null 2>&1 )
[[ -f "$ENV1/pyrst-env.yaml" && -d "$ENV1/packages" && -f "$ENV1/pyrst.lock" && -f "$ENV1/activate" ]] \
  && ok "venv writes the skeleton (pyrst-env.yaml, packages/, pyrst.lock, activate)" \
  || bad "venv skeleton incomplete"
grep -q "PYRST_VENV=" "$ENV1/activate" && ok "activate exports PYRST_VENV" || bad "activate missing PYRST_VENV export"

# ── 2. transitive install with a diamond ────────────────────────────────────
echo "[2] pyrst install (transitive + diamond)"
out="$("$BIN" --venv "$ENV1" install "$SRC/top" 2>&1)"
echo "$out" | grep -q "installed 3 package(s)" && ok "install reports 3 packages (top+mid+leaf; leaf deduped)" \
  || bad "unexpected install count: $out"
for p in top mid leaf; do
  [[ -f "$ENV1/packages/$p/core.pyrs" && -f "$ENV1/packages/$p/pyrst.yaml" ]] \
    && ok "store holds $p (module + manifest)" || bad "store missing $p"
done

# ── 3. build+run a consumer against the env, NO PYRST_PATH ───────────────────
echo "[3] build+run consumer against the env (no PYRST_PATH)"
APP="$WORK/app"; mkdir -p "$APP"
cat > "$APP/prog.pyrs" <<'EOF'
from top.core import top_val

def main() -> None:
    print(top_val())
EOF
( cd "$APP" && env -u PYRST_PATH PYRST_VENV="$ENV1" "$BIN" build prog.pyrs >/dev/null 2>&1 )
if [[ -x "$APP/prog" ]]; then
  got="$("$APP/prog")"
  [[ "$got" == "24" ]] && ok "consumer builds+runs against the env (top_val()=24)" || bad "wrong output: $got"
else
  bad "consumer failed to build against the env"
fi

# ── 4. uninstalled import -> honest env-aware error ─────────────────────────
echo "[4] uninstalled import -> honest error"
cat > "$APP/bad.pyrs" <<'EOF'
from notinstalled.core import x

def main() -> None:
    print(1)
EOF
err="$( ( cd "$APP" && env -u PYRST_PATH PYRST_VENV="$ENV1" "$BIN" build bad.pyrs ) 2>&1 )"
echo "$err" | grep -q "not installed in the active environment" \
  && echo "$err" | grep -q "notinstalled" \
  && ok "uninstalled import names the module + env, points at pyrst install" \
  || bad "uninstalled error not honest: $err"

# ── 5. dependency cycle -> honest cycle error ───────────────────────────────
echo "[5] dependency cycle -> honest error"
ENVC="$WORK/envc/.pyrstenv"; mkdir -p "$WORK/envc"
( cd "$WORK/envc" && "$BIN" venv .pyrstenv >/dev/null 2>&1 )
cerr="$("$BIN" --venv "$ENVC" install "$SRC/cyca" 2>&1)"
echo "$cerr" | grep -q "dependency cycle detected" \
  && echo "$cerr" | grep -q "cyca" && echo "$cerr" | grep -q "cycb" \
  && ok "install rejects the cycle honestly, naming cyca+cycb" \
  || bad "cycle not detected honestly: $cerr"

# ── 6. determinism: two envs -> byte-identical emit ─────────────────────────
echo "[6] determinism (install+emit twice -> identical emit)"
ENV2="$WORK/env2/.pyrstenv"; mkdir -p "$WORK/env2"
( cd "$WORK/env2" && "$BIN" venv .pyrstenv >/dev/null 2>&1 )
"$BIN" --venv "$ENV2" install "$SRC/top" >/dev/null 2>&1
env -u PYRST_PATH PYRST_VENV="$ENV1" "$BIN" emit "$APP/prog.pyrs" > "$WORK/emit1.rs" 2>/dev/null
env -u PYRST_PATH PYRST_VENV="$ENV2" "$BIN" emit "$APP/prog.pyrs" > "$WORK/emit2.rs" 2>/dev/null
if diff -q "$WORK/emit1.rs" "$WORK/emit2.rs" >/dev/null; then
  ok "emit against two independently-installed envs is byte-identical"
else
  bad "emit differs across envs (non-deterministic)"
fi

# ── 7. list / freeze ────────────────────────────────────────────────────────
echo "[7] list / freeze"
lst="$("$BIN" --venv "$ENV1" list 2>&1)"
echo "$lst" | grep -q "top@0.1.0" && echo "$lst" | grep -q "leaf@0.1.0" \
  && ok "list shows installed name@version" || bad "list output wrong: $lst"
frz="$("$BIN" --venv "$ENV1" freeze 2>&1)"
echo "$frz" | grep -q "top	0.1.0	path" && ok "freeze prints the pinned lock" || bad "freeze output wrong: $frz"

echo "══════════════════════════════════════════════"
echo "PKG INTEGRATION: $pass passed, $fail failed"
echo "══════════════════════════════════════════════"
[[ $fail -eq 0 ]] && { echo "ALL PKG TESTS PASSED"; exit 0; } || { echo "PKG TESTS FAILED"; exit 1; }
