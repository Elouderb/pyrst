def transform_and_check(values: list[int]) -> int:
    valid_set: set[int] = {10, 20, 30, 40, 50}
    result: int = 0

    for val in values:
        transformed: int = (lambda v: v * 2)(val)
        if transformed in valid_set:
            result += 1
    return result

def check_membership(items: list[int], membership_set: set[int]) -> int:
    count: int = 0
    for item in items:
        if (lambda i: i in membership_set)(item):
            count += 1
    return count

def main() -> None:
    nums1: list[int] = [5, 10, 15, 20, 25]
    processed: int = transform_and_check(nums1)
    print(processed)

    nums2: list[int] = [5, 10, 15, 20, 25]
    test_set: set[int] = {5, 10, 15}
    found: int = check_membership(nums2, test_set)
    print(found)

    nums3: list[int] = [5, 10, 15, 20, 25]
    original_set: set[int] = {1, 2, 3, 4, 5}
    doubled_check: int = 0
    for n in nums3:
        if (lambda x: x // 2 in original_set)(n):
            doubled_check += 1
    print(doubled_check)
