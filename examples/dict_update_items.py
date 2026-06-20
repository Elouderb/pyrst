# dict.update() and `for k, v in d.items()` iteration (codegen gap fix).
def main() -> None:
    d: dict[str, int] = {"a": 1, "b": 2}
    d.update({"c": 3})
    print(len(d))
    total: int = 0
    for k, v in d.items():
        total += v
    print(total)
