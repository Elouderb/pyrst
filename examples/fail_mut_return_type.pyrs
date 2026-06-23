# Negative (EPIC-4 V2): `Mut[T]` is a by-reference PARAMETER mode marker, not a
# type. Using it as a return type is illegal — it has no meaning anywhere except
# a parameter annotation.
#
# EXPECTED: typeck error — "Mut[...] is only valid on a parameter"

def identity(x: int) -> Mut[int]:
    return x
