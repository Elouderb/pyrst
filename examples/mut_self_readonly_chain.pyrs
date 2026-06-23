# EPIC-4 V3 precision: NO over-marking.
# `width`, `height`, and `area` are pure getters. `describe` calls ONLY those
# non-mutating self-methods. The fixpoint seed is precise and propagation adds a
# method only when it reaches a TRANSITIVELY mutating self-method, so every one
# of these methods must stay &self (verified against the generated Rust). A
# spurious &mut self here would be the over-marking bug this case guards against.
class Rect:
    w: int
    h: int
    def __init__(self, w: int, h: int) -> None:
        self.w = w
        self.h = h
    def width(self) -> int:
        return self.w
    def height(self) -> int:
        return self.h
    def area(self) -> int:
        return self.width() * self.height()
    def describe(self) -> int:
        return self.area() + self.width() + self.height()

def main() -> None:
    r: Rect = Rect(3, 4)
    print(r.describe())
