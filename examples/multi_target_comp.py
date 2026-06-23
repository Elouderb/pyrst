# EPIC-10 Card 2: multi-target (tuple-unpacking) loop variables in
# list / set / dict comprehensions, e.g. `[v for k, v in d.items()]`.
# Outputs are made order-independent (sum / len / single-key lookups) so the
# nondeterministic HashMap/HashSet iteration order cannot affect the result.
def main() -> None:
    d: dict[str, int] = {"a": 10, "b": 20, "c": 30}

    # list comprehension over dict.items() — element is (k, v); keep the value
    values: list[int] = [v for k, v in d.items()]
    print(sum(values))          # 60

    # list comprehension keeping the key, with a filter on the value
    big_keys: list[str] = [k for k, v in d.items() if v >= 20]
    print(len(big_keys))        # 2

    # dict comprehension: increment each value, swap is unnecessary — verify by
    # single-key lookups (order-independent)
    incremented: dict[str, int] = {k: v + 1 for k, v in d.items()}
    print(incremented["a"])     # 11
    print(incremented["c"])     # 31

    # set comprehension over a list of tuples — keep the first component
    pairs: list[tuple[int, int]] = [(1, 100), (2, 200), (3, 300)]
    firsts: set[int] = {a for a, b in pairs}
    print(len(firsts))          # 3
    print(sum(firsts))          # 6  (1 + 2 + 3)

    # set comprehension over dict.items() keeping the value
    seconds: set[int] = {b for a, b in pairs}
    print(sum(seconds))         # 600 (100 + 200 + 300)

    # single-target comprehension still works (regression guard)
    squares: list[int] = [x * x for x in [1, 2, 3]]
    print(sum(squares))         # 14


main()
