# EPIC-5 honest rejection: using an Optional[int] as a bare int WITHOUT
# narrowing must be rejected at typeck — never silently miscompiled. The value
# must be narrowed (`if x is not None:`) before any arithmetic/ordering use.
def add_one(x: Optional[int]) -> int:
    return x + 1


def main() -> None:
    print(add_one(5))
