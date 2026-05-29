def main() -> None:
    nums: list[int] = [1, 2, 3, 4, 5]
    total: int = 0
    for n in nums:
        total += n
    print(total)
    nums.append(6)
    print(len(nums))
