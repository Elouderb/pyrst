#!/bin/bash
# Positive examples: must transpile AND run successfully.
# Negative examples (*fail*): must be REJECTED by `pyrst build` (at the type
# checker or by rustc); a negative that builds is a soundness regression.
BIN="$HOME/.cargo/bin/cargo run --release --quiet --"

count=0
passed=0
for f in examples/*.py; do
  base=$(basename "$f" .py)
  [[ "$base" == *fail* ]] && continue
  count=$((count + 1))
  timeout 30 $BIN build "$f" 2>/dev/null && timeout 5 ./"$base" >/dev/null 2>&1 && passed=$((passed + 1)) || echo "FAIL: $base"
  rm -f "$base" "$base.rs"
done
echo "PASSED: $passed / $count"

neg_count=0
neg_ok=0
for f in examples/*fail*.py; do
  [[ -e "$f" ]] || continue
  base=$(basename "$f" .py)
  neg_count=$((neg_count + 1))
  if timeout 30 $BIN build "$f" >/dev/null 2>&1; then
    echo "LEAK (negative wrongly accepted): $base"
  else
    neg_ok=$((neg_ok + 1))
  fi
  rm -f "$base" "$base.rs"
done
echo "NEGATIVES REJECTED: $neg_ok / $neg_count"
