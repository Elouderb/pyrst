# Negative: str.upper() returns str, not int.
def expect_int(x: int) -> None:
    print(x)

def main() -> None:
    text: str = "hello"
    expect_int(text.upper())
