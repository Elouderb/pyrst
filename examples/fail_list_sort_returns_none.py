# list.sort() mutates in place and returns None (Ty::Unit), not the list.
# Passing its result where an int is expected must be rejected at typeck.
def expect_int(x: int) -> None:
    print(x)

def main() -> None:
    nums: list[int] = [3, 1, 2]
    expect_int(nums.sort())  # sort() -> None, not int
