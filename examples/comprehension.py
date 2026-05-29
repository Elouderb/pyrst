def main() -> None:
    nums: list[int]  = [1, 2, 3, 4, 5]
    squares: list[int]  = [(x * x) for x in nums]
    evens: list[int]  = [x for x in nums if ((x % 2) == 0)]
    print(len(squares))
    print(len(evens))

