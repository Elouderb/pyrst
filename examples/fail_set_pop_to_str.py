# Negative: set[int].pop() returns int, not str.
def expect_str(x: str) -> None:
    print(x)

def main() -> None:
    s: set[int] = {1, 2, 3}
    expect_str(s.pop())
