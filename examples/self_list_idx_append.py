# Shape: self.list[i].append(x)  — mutate an element of a list-of-lists held
# by self, via a method. Exercises a mutating method call whose receiver chain
# (self.rows[i]) roots at self, so the method needs &mut self and the element
# must be reached as a place.

class Grid:
    rows: list[list[int]]
    name: str

    def __init__(self, name: str) -> None:
        self.rows = [[1], [2], [3]]
        self.name = name

    def push(self, row: int, value: int) -> None:
        self.rows[row].append(value)


def main() -> None:
    g = Grid("g1")
    g.push(0, 10)
    g.push(0, 11)
    g.push(2, 30)

    print(g.name)
    for row in g.rows:
        print(row)
