# Negative: assigning to a field on a by-value non-Copy class parameter is
# rejected at typeck (not deferred to rustc).  The mutation is invisible to the
# caller because the parameter is a Rust clone of the caller's value.
#
# EXPECTED: typeck error — "mutation of by-value parameter `p` is not visible
# to the caller; mutate via a method on it or return the updated value"

class Point:
    x: int
    y: int
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y

def shift_x(p: Point, dx: int) -> None:
    p.x = p.x + dx
