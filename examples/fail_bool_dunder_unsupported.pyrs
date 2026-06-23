# Item 2 (card c27fbc7f): __bool__ is NOT yet supported. pyrst has no working
# object-truthiness lowering (bool() lowers numerically; if/while conditions are
# not constrained to bool), and codegen lists __bool__ among the dunder-trait
# names without a trait-impl arm, which would otherwise silently DROP it. The
# honest behavior is to reject __bool__ at typeck so the user is not misled into
# thinking their truthiness override took effect. This file must be rejected by
# BOTH `pyrst check` and `pyrst build`.
class Flag:
    on: bool

    def __init__(self, on: bool) -> None:
        self.on = on

    def __bool__(self) -> bool:
        return self.on


def main() -> None:
    f: Flag = Flag(True)
    print(f.on)
