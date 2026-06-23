# EPIC-7 sub-item 3: sorted(set), list(set), and sorted(dict) (sorts keys).
def main() -> None:
    nums: set[int] = {3, 1, 4, 1, 5, 9, 2, 6}

    # sorted(set) -> ascending list of the set's elements.
    for x in sorted(nums):
        print(x)

    # list(set) -> a list of the set's elements; length is deterministic.
    materialized: list[int] = list(nums)
    print(len(materialized))

    # sorted(dict) -> sorted list of the dict's KEYS (Python semantics).
    scores: dict[str, int] = {"gamma": 3, "alpha": 1, "beta": 2}
    for name in sorted(scores):
        print(name)
