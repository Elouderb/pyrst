# EPIC-7 sub-item 1: iterating a dict yields its KEYS (Python semantics).
# Keys are emitted in sorted order for deterministic output.
def main() -> None:
    d: dict[str, int] = {"banana": 3, "apple": 1, "cherry": 2}
    for k in d:
        print(k)

    # The loop variable is the KEY type, so it can index back into the dict.
    counts: dict[str, int] = {"x": 10, "y": 20, "z": 30}
    total: int = 0
    for key in counts:
        total += counts[key]
    print(total)
