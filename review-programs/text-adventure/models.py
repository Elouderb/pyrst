# models.py — core data types for the text adventure engine.
#
# pyrst constraints that shaped this file (all confirmed against the compiler):
#  * Class instances are VALUE types; assignment copies the struct.
#  * Attribute/index assignment only works on a *simple identifier* base.
#    `self.exits[k] = v` and `rooms[i].field = v` do NOT parse, and you cannot
#    even index-assign a collection that lives on `self`. So mutable per-element
#    state is kept OUT of these records and tracked on the engine.
#  * `String` is a move-only (non-Copy) type and codegen does not auto-clone it
#    when a value is passed by value to a function/method or reused. To stay
#    inside that rule, EXITS ARE STORED AS INTEGER ROOM INDICES, not as a
#    `dict[str, str]` whose string values would have to be moved around. An
#    exit value of -1 means "no exit in that direction". Integers are Copy, so
#    they pass and reuse freely.

class Item:
    item_id: str
    name: str
    description: str
    takeable: bool
    weight: int

    def __init__(self, item_id: str, name: str, description: str, takeable: bool, weight: int) -> None:
        self.item_id = item_id
        self.name = name
        self.description = description
        self.takeable = takeable
        self.weight = weight


class Room:
    room_id: str
    name: str
    description: str
    # Exits as destination ROOM INDICES (-1 == no exit). See module note.
    north: int
    south: int
    east: int
    west: int
    # Item indices (into the engine's item list) that START in this room.
    start_items: list[int]
    # True if entering this room is blocked until the engine opens the gate.
    locked: bool

    def __init__(self, room_id: str, name: str, description: str, north: int, south: int, east: int, west: int, start_items: list[int], locked: bool) -> None:
        self.room_id = room_id
        self.name = name
        self.description = description
        self.north = north
        self.south = south
        self.east = east
        self.west = west
        self.start_items = start_items
        self.locked = locked

    def exit_count(self) -> int:
        count: int = 0
        if self.north != -1:
            count = count + 1
        if self.south != -1:
            count = count + 1
        if self.east != -1:
            count = count + 1
        if self.west != -1:
            count = count + 1
        return count
