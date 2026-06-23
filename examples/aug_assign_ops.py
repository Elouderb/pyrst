def main() -> None:
    # **= keeps the int target's type (Python semantics): 12 ** 2 == 144 (int).
    x: int = 12
    x **= 2
    print(x)

    # Bitwise augmented assignment on fresh ints.
    a: int = 12
    a &= 10
    print(a)

    b: int = 12
    b |= 1
    print(b)

    c: int = 12
    c ^= 5
    print(c)

    # Shift augmented assignment.
    d: int = 3
    d <<= 2
    print(d)

    e: int = 12
    e >>= 1
    print(e)
