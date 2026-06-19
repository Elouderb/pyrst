# Negative: str.split() returns list[str], not int.
def expect_int(x: int) -> None:
    print(x)

def main() -> None:
    words = "a b c".split(" ")
    expect_int(words)
