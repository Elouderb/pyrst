# dict.items() returns List[Tuple[K, V]], not an int. Passing it where an int
# is expected must be rejected at typeck rather than deferred to rustc.
def expect_int(x: int) -> None:
    print(x)

def main() -> None:
    d: dict[str, int] = {"a": 1}
    expect_int(d.items())  # items() -> list[tuple[str, int]], not int
