# min() of a list[int] yields an int element, not a str. Assigning it to a
# str-typed variable must be rejected at typeck rather than deferred to rustc.
def main() -> None:
    x: str = min([1, 2, 3])
    print(x)
