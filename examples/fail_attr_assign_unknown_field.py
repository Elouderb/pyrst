# Negative: assigning to a non-existent attribute on a known class is rejected
# at typeck (not deferred to rustc). The target base `p` type-checks as `Point`,
# but `Point` has no field `nonexistent`, so `p.nonexistent = 5` is ill-formed.

class Point:
    x: int
    y: int

    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y


def main() -> None:
    p = Point(1, 2)
    p.nonexistent = 5
    print(p.x)
