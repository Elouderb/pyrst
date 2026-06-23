# EPIC-5: is-None / is-not-None narrowing of Optional[T] -> T, plus bare T
# flowing into an Optional[T] parameter (auto-Some at the call site).
def double_or_zero(x: Optional[int]) -> int:
    if x is not None:
        return x * 2
    return 0


def describe(x: Optional[int]) -> str:
    if x is None:
        return "none"
    else:
        return "value " + str(x)


def main() -> None:
    print(double_or_zero(5))
    print(double_or_zero(None))
    print(describe(7))
    print(describe(None))
