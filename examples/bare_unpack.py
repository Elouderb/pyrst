def get_pair() -> tuple[int, int]:
    return (10, 20)


def main() -> None:
    # bare LHS, literal tuple RHS
    x, y = 1, 2
    print(x)
    print(y)

    # bare LHS, function-return RHS
    a, b = get_pair()
    print(a)
    print(b)

    # swap: bare LHS, bare tuple RHS
    p: int = 7
    q: int = 9
    p, q = q, p
    print(p)
    print(q)

    # trailing comma in call args
    print(42,)
