# world.py — builds the static world. Rooms form a small directed graph; exits
# are stored as destination ROOM INDICES so no strings have to be moved around
# at runtime. Item lists are built on locals (you cannot index-assign a list
# that lives on `self`) and handed to the Room constructor.
#
# Fixed indices (build order is the contract the engine relies on):
#   Rooms:  0 entrance, 1 corridor, 2 armory, 3 hall
#   Items:  0 torch, 1 key, 2 rope, 3 idol, 4 statue, 5 gate

from models import Item
from models import Room


def build_items() -> list[Item]:
    items: list[Item] = []
    items.append(Item("torch", "Brass Torch", "A torch that throws a steady amber light.", True, 2))
    items.append(Item("key", "Iron Key", "A heavy key, cold to the touch.", True, 1))
    items.append(Item("rope", "Coil of Rope", "Forty feet of sturdy hemp rope.", True, 4))
    items.append(Item("idol", "Golden Idol", "The fabled idol of the Sunken Hall. Your prize.", True, 6))
    items.append(Item("statue", "Stone Statue", "A worn guardian statue, far too heavy to move.", False, 999))
    items.append(Item("gate", "Sealed Gate", "An iron gate set into the north wall.", False, 999))
    return items


def build_rooms() -> list[Room]:
    rooms: list[Room] = []

    # 0 Entrance Hall: north -> corridor(1). Holds the torch (item 0).
    entrance_items: list[int] = []
    entrance_items.append(0)
    rooms.append(Room("entrance", "Entrance Hall", "A cold stone hall. Dust hangs in the air.", 1, -1, -1, -1, entrance_items, False))

    # 1 Long Corridor: south -> entrance(0), east -> armory(2), north -> hall(3).
    corridor_items: list[int] = []
    corridor_items.append(4)
    rooms.append(Room("corridor", "Long Corridor", "A narrow corridor lined with broken sconces.", 3, 0, 2, -1, corridor_items, False))

    # 2 Old Armory: west -> corridor(1). Holds the key (1) and rope (2).
    armory_items: list[int] = []
    armory_items.append(1)
    armory_items.append(2)
    rooms.append(Room("armory", "Old Armory", "Rusted weapon racks line the walls.", -1, -1, -1, 1, armory_items, False))

    # 3 Sunken Hall: south -> corridor(1). Locked until the gate is opened.
    # Holds the gate (5) and the idol (3).
    hall_items: list[int] = []
    hall_items.append(5)
    hall_items.append(3)
    rooms.append(Room("hall", "Sunken Hall", "A flooded hall. A sealed gate bars the inner vault.", -1, 1, -1, -1, hall_items, True))

    return rooms
