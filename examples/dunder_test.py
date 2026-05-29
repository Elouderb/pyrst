class Vector:
    x: int
    y: int

    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y


def main() -> None:
    v1: Vector  = Vector(1, 2)
    print(v1.x)
    print(v1.y)
    v2: Vector  = Vector(3, 4)
    print(v2.x)

