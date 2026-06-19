# Negative test: set iteration now binds the element type, so a set[int] loop
# variable passed where a str is expected is caught at `pyrst check`.
def expect_str_arg(x: str) -> None:
    print(x)

def main() -> None:
    nums: set[int] = {1, 2, 3}
    for n in nums:
        expect_str_arg(n)
