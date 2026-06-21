class Base:
    x: int

    def __init__(self, x: int) -> None:
        self.x = x

    def __str__(self) -> str:
        return "Base(" + str(self.x) + ")"

    def __eq__(self, other: Base) -> bool:
        return self.x == other.x

    def __lt__(self, other: Base) -> bool:
        return self.x < other.x

    def kind(self) -> str:
        return "base-kind"

class Mid(Base):
    def __init__(self, x: int) -> None:
        self.x = x

    def __str__(self) -> str:
        return "Mid(" + str(self.x) + ")"

class Leaf(Mid):
    def __init__(self, x: int) -> None:
        self.x = x

def main() -> None:
    # Mid overrides __str__ but inherits __eq__, __lt__, and kind().
    m: Mid = Mid(5)
    print(m)
    print(m.kind())

    # Leaf inherits everything transitively: __str__ from Mid (1 level up),
    # __eq__/__lt__/kind from Base (2 levels up).
    a: Leaf = Leaf(3)
    b: Leaf = Leaf(7)
    print(a)
    print(a.kind())

    if a == b:
        print("a eq b")
    else:
        print("a ne b")
    if a == Leaf(3):
        print("a eq three")
    if a < b:
        print("a lt b")
    if b < a:
        print("b lt a")
    else:
        print("b not lt a")
