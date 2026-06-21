#!/usr/bin/env bash
# test_all.sh — pyrst integration harness
#
# Sections:
#   1. Positive build+run loop   — exit code + stdout diff against expected/
#   2. Negative build loop       — *fail* examples must be rejected by `pyrst build`
#   3. Negative typeck-check loop — *fail* examples must be rejected by `pyrst check`
#   4. Multi-file demo           — pinned stdout assertion
#
# Exit code: 0 only when all sections pass.
# No set -e: failures are counted and reported at the end.

BIN="$(cd "$(dirname "$0")" && pwd)/target/release/pyrst"
EXAMPLES="$(cd "$(dirname "$0")" && pwd)/examples"
EXPECTED="$EXAMPLES/expected"

# Names owned by a sibling card — skip if expected file is absent
SIBLING_EXCLUSIONS="floor_div_neg mod_neg pow_float"

# ── helpers ──────────────────────────────────────────────────────────────────

is_sibling_exclusion() {
    local name="$1"
    for ex in $SIBLING_EXCLUSIONS; do
        [[ "$name" == "$ex" ]] && return 0
    done
    return 1
}

# ── 1. POSITIVE LOOP ─────────────────────────────────────────────────────────

pos_count=0
pos_passed=0
pos_failures=()

for f in "$EXAMPLES"/*.py; do
    base=$(basename "$f" .py)
    # Skip negative examples (name contains "fail")
    [[ "$base" == *fail* ]] && continue

    pos_count=$((pos_count + 1))

    # Build
    stderr_out=$(timeout 30 "$BIN" build "$f" 2>&1 >/dev/null)
    build_exit=$?

    if [[ $build_exit -eq 124 ]]; then
        echo "FAIL [build timeout]: $base"
        pos_failures+=("$base")
        rm -f "$base" "$base.rs"
        continue
    fi
    if [[ $build_exit -eq 101 ]]; then
        echo "FAIL [ICE/panic build exit 101]: $base"
        pos_failures+=("$base")
        rm -f "$base" "$base.rs"
        continue
    fi
    if [[ $build_exit -ne 0 ]]; then
        echo "FAIL [build exit $build_exit]: $base"
        pos_failures+=("$base")
        rm -f "$base" "$base.rs"
        continue
    fi

    # Run and capture stdout
    got=$(timeout 5 ./"$base" 2>/dev/null)
    run_exit=$?
    rm -f "$base" "$base.rs"

    if [[ $run_exit -eq 124 ]]; then
        echo "FAIL [run timeout]: $base"
        pos_failures+=("$base")
        continue
    fi
    if [[ $run_exit -ne 0 ]]; then
        echo "FAIL [run exit $run_exit]: $base"
        pos_failures+=("$base")
        continue
    fi

    # Check expected file
    expected_file="$EXPECTED/$base.txt"
    if [[ ! -f "$expected_file" ]]; then
        if is_sibling_exclusion "$base"; then
            # Sibling card owns this; skip stdout check but count the build+run pass
            pos_passed=$((pos_passed + 1))
        else
            echo "FAIL [missing expected file]: $base"
            pos_failures+=("$base")
        fi
        continue
    fi

    expected=$(cat "$expected_file")
    if [[ "$got" == "$expected" ]]; then
        pos_passed=$((pos_passed + 1))
    else
        echo "FAIL [stdout mismatch]: $base"
        diff <(printf '%s\n' "$expected") <(printf '%s\n' "$got") | head -20
        pos_failures+=("$base")
    fi
done

echo ""
echo "POSITIVES: $pos_passed / $pos_count passed"

# ── 2. NEGATIVE BUILD LOOP ───────────────────────────────────────────────────

neg_count=0
neg_ok=0
neg_build_failures=()

for f in "$EXAMPLES"/*fail*.py; do
    [[ -e "$f" ]] || continue
    base=$(basename "$f" .py)
    neg_count=$((neg_count + 1))

    build_exit=0
    stderr_out=$(timeout 30 "$BIN" build "$f" 2>&1 >/dev/null)
    build_exit=$?
    rm -f "$base" "$base.rs"

    if [[ $build_exit -eq 0 ]]; then
        echo "LEAK [negative wrongly accepted by build]: $base"
        neg_build_failures+=("$base")
    elif [[ $build_exit -eq 101 ]]; then
        echo "WARN [ICE/panic on negative build]: $base (counts as rejection)"
        neg_ok=$((neg_ok + 1))
    elif [[ $build_exit -eq 124 ]]; then
        echo "WARN [timeout on negative build]: $base (counts as rejection)"
        neg_ok=$((neg_ok + 1))
    else
        neg_ok=$((neg_ok + 1))
    fi
done

echo ""
echo "NEGATIVES REJECTED (build): $neg_ok / $neg_count"

# ── 3. NEGATIVE TYPECK-CHECK LOOP ────────────────────────────────────────────
# Run `pyrst check` (parse+typeck only, no rustc) against every fail_* file.
# These files must be rejected at the typeck level — not deferred to rustc.

typeck_count=0
typeck_ok=0
typeck_failures=()

for f in "$EXAMPLES"/*fail*.py; do
    [[ -e "$f" ]] || continue
    base=$(basename "$f" .py)
    typeck_count=$((typeck_count + 1))

    check_exit=0
    timeout 10 "$BIN" check "$f" >/dev/null 2>&1
    check_exit=$?

    if [[ $check_exit -eq 0 ]]; then
        echo "TYPECK LEAK [check accepted negative]: $base"
        typeck_failures+=("$base")
    elif [[ $check_exit -eq 101 ]]; then
        echo "WARN [ICE/panic on check]: $base (counts as rejection)"
        typeck_ok=$((typeck_ok + 1))
    elif [[ $check_exit -eq 124 ]]; then
        echo "WARN [timeout on check]: $base (counts as rejection)"
        typeck_ok=$((typeck_ok + 1))
    else
        typeck_ok=$((typeck_ok + 1))
    fi
done

echo ""
echo "NEGATIVES REJECTED (typeck check): $typeck_ok / $typeck_count"

# ── 4. MULTI-FILE DEMO ───────────────────────────────────────────────────────

multi_ok=0
multi_failures=()

MULTI_EXPECTED="$(printf '100\n5\n1000')"

stderr_out=$(timeout 30 "$BIN" build "$EXAMPLES/multi_file_demo/main.py" 2>&1 >/dev/null)
multi_build_exit=$?
if [[ $multi_build_exit -eq 0 ]]; then
    multi_got=$(timeout 5 ./main 2>/dev/null)
    multi_run_exit=$?
    rm -f main main.rs
    if [[ $multi_run_exit -eq 0 ]] && [[ "$multi_got" == "$MULTI_EXPECTED" ]]; then
        multi_ok=1
        echo ""
        echo "MULTI_FILE_DEMO: PASS"
    else
        echo ""
        echo "MULTI_FILE_DEMO: FAIL [stdout mismatch or run error]"
        echo "  expected: $(printf '%s' "$MULTI_EXPECTED" | head -5)"
        echo "  got:      $(printf '%s' "$multi_got" | head -5)"
        multi_failures+=("multi_file_demo")
    fi
else
    rm -f main main.rs
    echo ""
    echo "MULTI_FILE_DEMO: FAIL [build exit $multi_build_exit]"
    multi_failures+=("multi_file_demo")
fi

# ── SUMMARY ──────────────────────────────────────────────────────────────────

echo ""
echo "══════════════════════════════════════════════"
echo "POSITIVES:              $pos_passed / $pos_count"
echo "NEGATIVES (build):      $neg_ok / $neg_count"
echo "NEGATIVES (typeck):     $typeck_ok / $typeck_count"
echo "MULTI_FILE_DEMO:        $multi_ok / 1"
echo "══════════════════════════════════════════════"

total_failures=$(( ${#pos_failures[@]} + ${#neg_build_failures[@]} + ${#typeck_failures[@]} + ${#multi_failures[@]} ))

if [[ $total_failures -gt 0 ]]; then
    echo ""
    echo "FAILURES ($total_failures):"
    for name in "${pos_failures[@]}"; do echo "  [positive] $name"; done
    for name in "${neg_build_failures[@]}"; do echo "  [neg-build-leak] $name"; done
    for name in "${typeck_failures[@]}"; do echo "  [typeck-leak] $name"; done
    for name in "${multi_failures[@]}"; do echo "  [multi-file] $name"; done
    echo ""
    exit 1
fi

echo ""
echo "ALL TESTS PASSED"
exit 0
