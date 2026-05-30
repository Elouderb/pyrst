def main() -> None:
    x: int = 5
    y: str = "hello"
    z: list[int] = [1, 2, 3]

    print(isinstance(x, int))
    print(isinstance(x, str))
    print(isinstance(y, str))
    print(isinstance(y, int))
    print(isinstance(z, list))
    print(isinstance(z, int))
