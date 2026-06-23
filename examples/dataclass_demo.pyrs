from dataclasses import dataclass

@dataclass
class Point:
    x: float
    y: float

@dataclass
class Rectangle:
    width: float
    height: float

    def area(self) -> float:
        return self.width * self.height

def main() -> None:
    p: Point = Point(3.0, 4.0)
    print(p.x)
    print(p.y)
    r: Rectangle = Rectangle(5.0, 3.0)
    print(r.area())
