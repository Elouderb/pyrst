def main() -> None:
    d: dict[str, int] = {"a": 1, "b": 2}

    # Accessing a missing key must raise a catchable KeyError.
    try:
        v: int = d["missing"]
        print("WRONG: got " + str(v))
    except KeyError as e:
        print("caught KeyError: " + e)

    print("done")
