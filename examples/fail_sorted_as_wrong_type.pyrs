# Negative test: sorted() now types as a list, so passing it where an int is
# expected is caught by pyrst's checker (previously deferred to rustc).
def expect_int(x: int) -> None:
    print(x)

def main() -> None:
    nums: list[int] = [3, 1, 2]
    s = sorted(nums)
    expect_int(s)
