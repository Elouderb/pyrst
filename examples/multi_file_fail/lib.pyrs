# EPIC-8 multi-file error-sourcing NEGATIVE fixture (the imported module).
# `broken` claims to return int but returns a str on line 8 — a type error that
# must be rendered against THIS file (lib.py), not the importing root (main.py).
def add_one(n: int) -> int:
    return n + 1

def broken(n: int) -> int:
    return "not an int"
