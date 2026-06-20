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

# Output assertions: a handful of examples whose stdout must match exactly, so
# silent corruption (e.g. a UTF-8 lexer regression that still exits 0) is caught.
out_ok=0
out_count=0
assert_output() {
  local base="$1"; local expected="$2"
  out_count=$((out_count + 1))
  if timeout 30 $BIN build "examples/$base.py" >/dev/null 2>&1; then
    local got; got=$(timeout 5 ./"$base" 2>/dev/null)
    if [[ "$got" == "$expected" ]]; then
      out_ok=$((out_ok + 1))
    else
      echo "OUTPUT MISMATCH: $base"
    fi
  else
    echo "OUTPUT BUILD FAIL: $base"
  fi
  rm -f "$base" "$base.rs"
}
assert_output unicode_strings "$(printf 'café déjà vu\n日本語 世界\nrocket 🚀 star ✨\nf-string with naïve and 日本語\ncafé déjà vu — 日本語 世界\n15')"
assert_output except_type_match "$(printf 'caught ValueError: bad value\ncaught KeyError: missing key\ndone')"
assert_output except_as_binding "$(printf 'caught: something broke\nlength: 15\nrecovered')"
assert_output except_hierarchy "$(printf 'caught LookupError (was KeyError): missing key\nnarrow KeyError caught first: missing key\ncaught ArithmeticError (was ZeroDivisionError): division by zero\ndone')"
assert_output except_bound_len "$(printf '4')"
assert_output except_multi_handler "$(printf 'value handler: v\nkey handler: k\nruntime handler: r\ndone')"
assert_output except_finally_always "$(printf 'caught: boom\nfinally after catch\nno error\nfinally after success\ndone')"
assert_output ternary "$(printf 'big\n100\nnegative\nzero\npositive\n5 is odd\neven\nodd\neven\nhi\n3')"
assert_output bool_print "$(printf 'True\nFalse\nTrue\nFalse\nTrue\nTrue')"
assert_output chained_predicate "$(printf 'True\nFalse\nTrue\n4')"
assert_output set_methods "$(printf '4\n3\n2\n4\n1\n1\n3\nFalse\nTrue\nTrue\n3')"
assert_output dict_update_items "$(printf '3\n6')"
assert_output list_pop "$(printf '30\n2\n10\n1\n20')"
assert_output list_pop_negative "$(printf '40\n3\n20\n2\n10\n30')"
echo "OUTPUT ASSERTIONS: $out_ok / $out_count"
