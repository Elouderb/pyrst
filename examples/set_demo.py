def is_in_set(items: list[int], target_set: set[int]) -> int:
    count: int = 0
    for item in items:
        if item in target_set:
            count += 1
    return count

def main() -> None:
    primes: set[int] = {2, 3, 5, 7, 11}
    print(len(primes))

    nums: set[int] = {1, 2, 3, 4, 5}
    print(len(nums))

    evens: set[int] = {2, 4, 6, 8, 10}
    print(len(evens))

    data: list[int] = [1, 2, 3, 4, 5]
    valid_set: set[int] = {1, 3, 5}
    match_count: int = is_in_set(data, valid_set)
    print(match_count)
