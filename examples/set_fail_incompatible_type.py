def main() -> None:
    mixed_set: set[int] = {1, 2, 3}

    result: bool = False
    for elem in mixed_set:
        if elem > 2:
            result = True
    print(result)

    mixed_set.add(4)
    mixed_set.add("not an int")
    print(len(mixed_set))
