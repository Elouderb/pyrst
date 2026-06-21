# Shape: a.b.c = v  — assign through a multi-level attribute chain.
# Exercises AttrAssign whose base is itself an Attr expression (point.inner),
# lowering to `point.inner.x = v`.

class Inner:
    x: int
    y: int
    label: str

    def __init__(self, label: str) -> None:
        self.x = 0
        self.y = 0
        self.label = label


class Point:
    inner: Inner

    def __init__(self, label: str) -> None:
        self.inner = Inner(label)

    def move_to(self, x: int, y: int) -> None:
        self.inner.x = x
        self.inner.y = y


def main() -> None:
    p = Point("origin")
    print(p.inner.label)

    p.inner.x = 11
    p.inner.y = 22
    print(p.inner.x)
    print(p.inner.y)

    p.move_to(100, 200)
    print(p.inner.x)
    print(p.inner.y)
