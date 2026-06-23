# EPIC-8 Card 3 — real spans for `from_type_expr` diagnostics.
#
# A class FIELD declared `set[float]` reaches `Ty::from_type_expr` (check_bodies
# validates every field annotation) and hits the require_hashable("set element")
# path. Before this card that error rendered at `0:0` with no caret; now it
# points at the field annotation's real `line:col` via the field's own span.
#
# EXPECTED: typeck error — "set element type must be hashable" at the FIELD line.

class Bag:
    items: set[float]

    def __init__(self) -> None:
        self.items = set()
