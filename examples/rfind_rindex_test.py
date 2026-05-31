def main() -> None:
    s: str = "hello world hello"

    # rfind - find from right
    pos1: int = s.rfind("l")
    print(pos1)

    pos2: int = s.rfind("hello")
    print(pos2)

    pos3: int = s.rfind("xyz")
    print(pos3)

    # rindex - find from right, raise if not found
    idx1: int = s.rindex("o")
    print(idx1)

    idx2: int = s.rindex("world")
    print(idx2)
