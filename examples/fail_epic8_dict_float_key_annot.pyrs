# EPIC-8 Card 3 — real spans for `from_type_expr` diagnostics.
#
# A DECLARED `dict[float, str]` annotation resolves to Dict(Float, Str) and hits
# the require_hashable("dict key") path (float keys cannot be HashMap<f64, _>).
# This is reached through the annotated-assignment `from_type_expr` call, which
# now passes the assignment statement's real span — so the error renders with a
# caret at the variable's line instead of the old `0:0`.
#
# EXPECTED: typeck error — "dict key type must be hashable" at the annotation.

def main() -> None:
    table: dict[float, str] = {}
    print(len(table))
