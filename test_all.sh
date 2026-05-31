#!/bin/bash
count=0
passed=0
for f in examples/*.py; do
  base=$(basename "$f" .py)
  [[ "$base" == *fail* ]] && continue
  count=$((count + 1))
  timeout 5 $HOME/.cargo/bin/cargo run --release --quiet -- build "$f" 2>/dev/null && timeout 5 ./"$base" >/dev/null 2>&1 && passed=$((passed + 1)) || echo "FAIL: $base"
  rm -f "$base" "$base.rs"
done
echo "PASSED: $passed / $count"
