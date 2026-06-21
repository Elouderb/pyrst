# D6: dict literal fold reads ALL pairs.  A dict with 3+ pairs where the
# LATER pairs' value types are what matter — all values must be the same
# type (heterogeneous values are a typeck error).  This proves the fold
# doesn't short-circuit after the first pair.
def main() -> None:
    scores: dict[str, int] = {"alice": 95, "bob": 87, "carol": 91, "dave": 78}
    print(scores["alice"])   # 95
    print(scores["carol"])   # 91 — value from a later pair
    print(scores["dave"])    # 78 — value from the last pair
    print(len(scores))       # 4
