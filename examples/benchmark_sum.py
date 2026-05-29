def main() -> None:
    nums: list[int] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    total: int = 0
    for n in nums:
        total = total + n
    print(total)
