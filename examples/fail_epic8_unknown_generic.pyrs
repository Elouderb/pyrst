# EPIC-8 Card 3 — real spans for `from_type_expr` diagnostics.
#
# `Bogus[int]` is not a known generic type (the recognized ones are list / set /
# dict / tuple / Optional / Union / Mut), so it hits the unknown-generic arm of
# `from_type_expr`. Routed through the annotated-assignment call, the error now
# carries the assignment's real span and renders a caret instead of `0:0`.
#
# EXPECTED: typeck error — "unknown generic type `Bogus`" at the annotation.

def main() -> None:
    value: Bogus[int] = 0
    print(value)
