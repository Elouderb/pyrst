# Negative (EPIC-4 V2): `Mut[T]` is only valid on a parameter. Using it as a
# local variable annotation is a non-parameter position, so it reaches
# `Ty::from_type_expr` and is rejected at typeck. (Return types, field types,
# and nested forms like `list[Mut[T]]` all hit the same guard.)
#
# EXPECTED: typeck error — "Mut[...] is only valid on a parameter"

def main() -> None:
    x: Mut[int] = 3
    print(x)
