# A conditional expression whose branches have incompatible types must be
# rejected at the type checker, not deferred to rustc.
def main() -> None:
    x: int = 1
    y: int = 5 if x > 0 else "no"  # int vs str
    print(y)
