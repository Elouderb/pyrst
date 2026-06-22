# Item 1 (card c27fbc7f): a class defining BOTH __str__ and __repr__.
# Both Python dunders map to the same Rust trait (Display). Without the
# trait-level dedup this emits two `impl Display` -> rustc E0119. The dedup
# prefers __str__ (Python uses __str__ for str()/print), so print() shows the
# __str__ rendering, not the __repr__ one.
class Point:
    x: int
    y: int

    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y

    def __str__(self) -> str:
        return "Point(" + str(self.x) + ", " + str(self.y) + ")"

    def __repr__(self) -> str:
        return "Point(x=" + str(self.x) + ", y=" + str(self.y) + ")"


def main() -> None:
    p: Point = Point(1, 2)
    # __str__ wins for print().
    print(p)
