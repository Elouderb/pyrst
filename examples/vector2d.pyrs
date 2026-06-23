class Vector2D:
    x: float
    y: float

    def __init__(self, x: float, y: float) -> None:
        self.x = x
        self.y = y

    def __add__(self, other: Vector2D) -> Vector2D:
        return Vector2D(self.x + other.x, self.y + other.y)

    def __sub__(self, other: Vector2D) -> Vector2D:
        return Vector2D(self.x - other.x, self.y - other.y)

    def __neg__(self) -> Vector2D:
        return Vector2D(-self.x, -self.y)

    def __lt__(self, other: Vector2D) -> bool:
        mag_self: float = self.x * self.x + self.y * self.y
        mag_other: float = other.x * other.x + other.y * other.y
        return mag_self < mag_other

    def magnitude(self) -> float:
        return (self.x * self.x + self.y * self.y)

def main() -> None:
    a: Vector2D = Vector2D(3.0, 4.0)
    b: Vector2D = Vector2D(1.0, 2.0)
    c: Vector2D = a + b
    print(c.x)
    print(c.y)
    d: Vector2D = a - b
    print(d.x)
    e: Vector2D = -a
    print(e.x)
    print(a.magnitude())
    less: bool = a < b
    print(less)
