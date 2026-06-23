# Negative: str.format() is not emittable by codegen — must be rejected at typeck.
# Expected: `pyrst check` exits non-zero (card 36f66dd2 stopgap).
def main() -> None:
    x: int = 42
    result: str = "value is {}".format(x)
    print(result)
