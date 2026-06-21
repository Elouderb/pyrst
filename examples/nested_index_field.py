# Shape: rooms[i].field = v  — mutate a field of a list element in place.
# Exercises AttrAssign whose base is an Index expression (rooms[i]),
# lowering to `rooms[i as usize].field = v` (a true place expression).

class Room:
    name: str
    occupied: int

    def __init__(self, name: str) -> None:
        self.name = name
        self.occupied = 0


def main() -> None:
    rooms = [Room("north"), Room("south"), Room("east")]
    rooms[0].occupied = 2
    rooms[2].occupied = 5
    rooms[0].name = "north-wing"

    for r in rooms:
        print(r.name)
        print(r.occupied)
