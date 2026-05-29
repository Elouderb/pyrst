def divide(a: int, b: int) -> int:
    assert (b != 0), "divisor must be non-zero"
    return (a // b)


def main() -> None:
    print(divide(10, 2))
    print(divide(7, 3))

