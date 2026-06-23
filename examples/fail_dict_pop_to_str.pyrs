# dict.pop(k) returns the value type V (here int), not str. Passing it where a
# str is expected must be rejected at typeck.
def expect_str(x: str) -> None:
    print(x)

def main() -> None:
    d: dict[str, int] = {"a": 1}
    expect_str(d.pop("a"))  # pop() -> int (value type), not str
