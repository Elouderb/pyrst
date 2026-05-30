def count_odds(nums: list[int]) -> int:
    count: int = 0
    odds: set[int] = {1, 3, 5, 7, 9}
    for n in nums:
        if n in odds:
            count += 1
    return count

def get_set_sum(s: set[int]) -> int:
    total: int = 0
    for elem in s:
        total += elem
    return total

def count_evens(nums: list[int]) -> int:
    count: int = 0
    evens: set[int] = {2, 4, 6, 8, 10, 12, 14}
    for n in nums:
        if n in evens:
            count += 1
    return count

def main() -> None:
    nums1: list[int] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    odd_count: int = count_odds(nums1)
    print(odd_count)

    my_set: set[int] = {2, 4, 6, 8, 10}
    total: int = get_set_sum(my_set)
    print(total)

    nums2: list[int] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    even_count: int = count_evens(nums2)
    print(even_count)
