def main() -> None:
    s: str = "hello\nworld\npython"
    lines: list[str] = s.splitlines()
    for line in lines:
        print(line)
