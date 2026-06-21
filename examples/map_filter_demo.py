def main() -> None:
    nums: list[int] = [1, 2, 3, 4, 5]

    doubled: list[int] = map(lambda x: x * 2, nums)
    print(doubled)

    total: int = sum(doubled)
    print(total)

    evens: list[int] = filter(lambda x: x % 2 == 0, nums)
    print(evens)
    print(len(evens))
