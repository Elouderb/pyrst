# engine.py — the game engine. Holds ALL mutable state and exposes command
# methods that mutate `self`. A scripted command list drives it; no real stdin.
#
# Two pyrst realities forced the shape of this file:
#
#  1. A method is generated with `&mut self` only when codegen's syntactic
#     heuristic SEES a direct `self.field = ...` (or `self.coll.append(...)`)
#     in the body. It does NOT follow calls to other mutating methods on self.
#     So every command that changes state mutates a `self.field` *directly*
#     (the take/drop bookkeeping is inlined rather than delegated), and the
#     command match is inlined into `run_command`, which is already `&mut self`.
#
#  2. `String` is move-only and codegen does not auto-clone it across a
#     function/method call. Passing a string arg to a helper and then reusing it
#     is a "use of moved value" error. So string command words are ONLY ever
#     compared (a borrow) to resolve an integer index, then the integer index is
#     what travels. No helper takes a string it cannot consume exactly once.
#
# Item-location model (no per-item mutable field, since you cannot index-assign
# a collection that lives on `self`):
#   item i is "in room r" iff it is NOT carried AND
#     (it was relocated to r) OR (its origin is r and it was not relocated).

from models import Item
from models import Room


class Engine:
    rooms: list[Room]
    items: list[Item]
    origin: list[int]            # origin[i] = room index where item i starts
    dropped: dict[int, int]      # item index -> room index it was dropped into
    current: int                 # current room index
    inventory: list[int]         # item indices the player carries
    visited: list[int]           # room indices already seen
    gate_open: bool
    won: bool
    moves: int
    log: list[str]

    def __init__(self, rooms: list[Room], items: list[Item], origin: list[int], start: int) -> None:
        self.rooms = rooms
        self.items = items
        self.origin = origin
        self.dropped = {}
        self.current = start
        self.inventory = []
        self.visited = []
        self.visited.append(start)
        self.gate_open = False
        self.won = False
        self.moves = 0
        self.log = []

    # ----- queries (all integer-keyed; never consume a String) -------------

    def item_room(self, i: int) -> int:
        # Where item i currently is: -1 == carried, else a room index.
        if i in self.inventory:
            return -1
        moved: int = self.dropped.get(i, -2)
        if moved != -2:
            return moved
        return self.origin[i]

    def items_in_room(self, r: int) -> list[int]:
        present: list[int] = []
        count: int = len(self.items)
        for i in range(count):
            if self.item_room(i) == r:
                present.append(i)
        return present

    # ----- rendering -------------------------------------------------------

    def describe_current(self) -> str:
        room: Room = self.rooms[self.current]
        lines: list[str] = []
        lines.append(f"== {room.name} ==")
        lines.append(room.description)

        present: list[int] = self.items_in_room(self.current)
        if len(present) == 0:
            lines.append("You see nothing of note here.")
        else:
            names: list[str] = []
            for i in present:
                it: Item = self.items[i]
                if it.takeable:
                    names.append(f"{it.name} (can be taken)")
                else:
                    names.append(f"{it.name} (fixed in place)")
            joined: str = ", ".join(names)
            lines.append(f"You see: {joined}.")

        dirs: list[str] = []
        if room.north != -1:
            dirs.append("north")
        if room.south != -1:
            dirs.append("south")
        if room.east != -1:
            dirs.append("east")
        if room.west != -1:
            dirs.append("west")
        if len(dirs) == 0:
            lines.append("There are no obvious exits.")
        else:
            exits_joined: str = ", ".join(dirs)
            lines.append(f"Exits: {exits_joined}.")
        return "\n".join(lines)

    def inventory_line(self) -> str:
        if len(self.inventory) == 0:
            return "You are carrying nothing."
        names: list[str] = []
        total_weight: int = 0
        for i in self.inventory:
            it: Item = self.items[i]
            names.append(it.name)
            total_weight = total_weight + it.weight
        joined: str = ", ".join(names)
        return f"You are carrying: {joined}. (weight {total_weight})"

    # ----- commands (each mutates self.* directly so it is &mut self) ------

    def do_go(self, direction: str) -> str:
        room: Room = self.rooms[self.current]
        # `direction` is only compared (borrowed) below, never moved.
        dest: int = -1
        if direction == "north":
            dest = room.north
        if direction == "south":
            dest = room.south
        if direction == "east":
            dest = room.east
        if direction == "west":
            dest = room.west
        if dest == -1:
            return f"You can't go {direction} from here."
        target: Room = self.rooms[dest]
        if target.locked and not self.gate_open:
            return "A sealed iron gate blocks the way. It needs unlocking."
        self.current = dest
        self.moves = self.moves + 1
        if dest not in self.visited:
            self.visited.append(dest)
        return f"You move {direction} into the {target.name}."

    def do_take(self, token: str) -> str:
        # Resolve token -> item index by comparison only (token is borrowed).
        i: int = -1
        idx: int = 0
        for it in self.items:
            if it.item_id == token:
                i = idx
            idx = idx + 1
        if i == -1:
            return f"There is no {token} here."
        if self.item_room(i) != self.current:
            return f"There is no {token} here to take."
        item: Item = self.items[i]
        if not item.takeable:
            return f"The {item.name} cannot be taken."
        # inlined take bookkeeping (direct self.* mutation => &mut self)
        self.inventory.append(i)
        nd: dict[int, int] = {}
        for k, v in self.dropped.items():
            if k != i:
                nd[k] = v
        self.dropped = nd
        return f"You take the {item.name}."

    def do_drop(self, token: str) -> str:
        i: int = -1
        idx: int = 0
        for it in self.items:
            if it.item_id == token:
                i = idx
            idx = idx + 1
        if i == -1 or i not in self.inventory:
            return f"You aren't carrying any {token}."
        name: str = self.items[i].name
        new_inv: list[int] = []
        for held in self.inventory:
            if held != i:
                new_inv.append(held)
        self.inventory = new_inv
        nd: dict[int, int] = {}
        for k, v in self.dropped.items():
            nd[k] = v
        nd[i] = self.current
        self.dropped = nd
        return f"You drop the {name}."

    def do_unlock(self) -> str:
        # Gate opens only in the corridor (room 1) while holding the key (item 1).
        if self.current != 1:
            return "There is nothing to unlock here."
        if 1 not in self.inventory:
            return "The gate is locked. You need a key."
        if self.gate_open:
            return "The gate is already open."
        self.gate_open = True
        return "The Iron Key turns. The gate north grinds open!"

    # ----- driver: parse + dispatch + win check, all in one &mut self body -

    def run_command(self, line: str) -> None:
        parts: list[str] = line.split(" ")
        verb: str = parts[0]
        arg: str = ""
        if len(parts) > 1:
            arg = parts[1]
        self.log.append(f"> {line}")

        # Inlined dispatch. The default "unknown" message is built BEFORE the
        # match (the f-string only borrows `verb`); the match then consumes
        # `verb` as its subject, and the wildcard arm is a no-op. `arg` is
        # consumed by at most one command call, so it is never reused after a
        # move. (Referencing `verb` inside an arm would be a use-after-move,
        # because codegen moves the match subject into a temporary.)
        out: str = f"Unknown command: {verb}"
        match verb:
            case "look":
                out = self.describe_current()
            case "inventory":
                out = self.inventory_line()
            case "unlock":
                out = self.do_unlock()
            case "go":
                out = self.do_go(arg)
            case "take":
                out = self.do_take(arg)
            case "drop":
                out = self.do_drop(arg)
            case _:
                pass
        self.log.append(out)

        # Win check: carry the idol (item 3) to the Entrance Hall (room 0).
        if not self.won:
            if 3 in self.inventory and self.current == 0:
                self.won = True
                self.log.append("*** You escape the ruins with the Golden Idol. You WIN! ***")
