# EPIC-8 Card 3 — real spans for `from_type_expr` diagnostics.
#
# `Mut[T]` is a by-reference PARAMETER mode marker, never a type. Here it is
# NESTED inside a return annotation (`list[Mut[int]]`), an illegal position. The
# recursive `from_type_expr` call for the list element reuses the whole
# annotation's span, so the error points at the function definition (a return
# annotation carries no span of its own) instead of the old `0:0`.
#
# EXPECTED: typeck error — "Mut[...] is only valid on a parameter" at the def.

def wrap(x: int) -> list[Mut[int]]:
    return [x]
