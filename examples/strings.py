def main() -> None:
    s: str = "Hello, World!"
    print(s.upper())
    print(s.lower())
    words: list[str] = s.split(", ")
    print(len(words))
