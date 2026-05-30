def main() -> None:
    my_set: set[int] = {5, 10, 15, 20, 25}

    total: int = 0
    for elem in my_set:
        total += elem
    print(total)

    first: int = my_set.first()
    print(first)
