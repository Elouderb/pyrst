def make_pair(a: int, b: int) -> tuple[int, int]:
    return (a, b)


def main() -> None:
    p: tuple[int, int]  = make_pair(3, 4)
    (a, b) = p
    print(a)
    print(b)

