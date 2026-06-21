# main.py — entry point. Builds the world, drives the engine with a scripted
# list of commands (no real stdin), prints the full transcript, then a summary.
#
# Winning playthrough: grab the torch, cross to the armory for the key and rope,
# return and unlock the gate, enter the Sunken Hall, take the Golden Idol, then
# carry it back out to the Entrance Hall to win.

from models import Item
from models import Room
from engine import Engine
from world import build_items
from world import build_rooms


def build_origin() -> list[int]:
    # origin[i] = the room index where item i starts. Mirrors world.py.
    #   0 torch -> entrance(0)   1 key -> armory(2)   2 rope -> armory(2)
    #   3 idol  -> hall(3)       4 statue -> corridor(1)   5 gate -> hall(3)
    origin: list[int] = []
    origin.append(0)
    origin.append(2)
    origin.append(2)
    origin.append(3)
    origin.append(1)
    origin.append(3)
    return origin


def scripted_commands() -> list[str]:
    cmds: list[str] = []
    cmds.append("look")
    cmds.append("take torch")
    cmds.append("go north")
    cmds.append("go east")
    cmds.append("look")
    cmds.append("take key")
    cmds.append("take rope")
    cmds.append("inventory")
    cmds.append("go west")
    cmds.append("go north")      # blocked: gate still sealed
    cmds.append("unlock")
    cmds.append("go north")      # now allowed
    cmds.append("look")
    cmds.append("take statue")   # refused: not takeable (and not even here)
    cmds.append("take idol")
    cmds.append("inventory")
    cmds.append("go south")
    cmds.append("go south")      # back to the entrance: triggers the win
    cmds.append("look")
    return cmds


def main() -> None:
    items: list[Item] = build_items()
    rooms: list[Room] = build_rooms()
    origin: list[int] = build_origin()

    game: Engine = Engine(rooms, items, origin, 0)

    print("########################################")
    print("#   THE SUNKEN HALL - a text adventure  #")
    print("########################################")
    print("")

    commands: list[str] = scripted_commands()
    for line in commands:
        game.run_command(line)

    for entry in game.log:
        print(entry)

    print("")
    print("---------- SUMMARY ----------")
    print(f"Moves taken: {game.moves}")
    print(f"Rooms visited: {len(game.visited)} of {len(game.rooms)}")
    print(f"Items carried at the end: {len(game.inventory)}")

    carried_idol: bool = 3 in game.inventory
    print(f"Recovered the Golden Idol: {carried_idol}")
    print(f"Victory: {game.won}")

    if game.won:
        print("Status: COMPLETE")
    else:
        print("Status: INCOMPLETE")
