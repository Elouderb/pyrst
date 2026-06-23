# EPIC-4 V2-c NEGATIVE: `Mut[T]` is a parameter-only mode marker, NOT a real
# type. A class-FIELD annotated `Mut[int]` is rejected — at BOTH `check` and
# `build` (check_bodies now validates field annotations through from_type_expr).
class Holder:
    value: Mut[int]
    def __init__(self, value: int) -> None:
        self.value = value

def main() -> None:
    h: Holder = Holder(5)
    print(h.value)
