def main() -> None:
    s: str = "hello world"
    print(s.removesuffix("world"))
    print(s.removesuffix("hello"))

    s2: str = "text_suffix"
    print(s2.removesuffix("_suffix"))
    print(s2.removesuffix(""))
