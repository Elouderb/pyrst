def main() -> None:
    s: str = "hello world"
    print(s.removeprefix("hello"))
    print(s.removeprefix("world"))

    s2: str = "prefix_text"
    print(s2.removeprefix("prefix_"))
    print(s2.removeprefix(""))
