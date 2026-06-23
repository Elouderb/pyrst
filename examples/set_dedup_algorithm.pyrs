def has_overlap(set1: set[int], set2: set[int]) -> bool:
    for elem in set1:
        if elem in set2:
            return True
    return False

def count_common(set1: set[int], set2: set[int]) -> int:
    count: int = 0
    for elem in set1:
        if elem in set2:
            count += 1
    return count

def main() -> None:
    set_a: set[int] = {1, 2, 3, 4, 5}
    set_b: set[int] = {4, 5, 6, 7, 8}
    if has_overlap(set_a, set_b):
        print(1)
    else:
        print(0)

    set_c: set[int] = {10, 20, 30}
    set_d: set[int] = {40, 50, 60}
    if has_overlap(set_c, set_d):
        print(1)
    else:
        print(0)

    set_e: set[int] = {1, 2, 3}
    set_f: set[int] = {2, 3, 4}
    common: int = count_common(set_e, set_f)
    print(common)
