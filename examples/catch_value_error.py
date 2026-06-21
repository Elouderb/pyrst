def main() -> None:
    # int() on a non-numeric string must raise a catchable ValueError.
    try:
        n: int = int("oops")
        print("WRONG: got " + str(n))
    except ValueError as e:
        print("caught ValueError: " + e)

    # float() on a non-numeric string must also raise a catchable ValueError.
    try:
        f: float = float("nope")
        print("WRONG: got " + str(f))
    except ValueError as e:
        print("caught ValueError: " + e)

    print("done")
