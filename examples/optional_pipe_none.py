# EPIC-5: the `X | None` annotation syntax lowers to Option<X> identically to
# Optional[X] (the parser folds `X | None` -> Optional(X)).
class Box:
    value: int


def wrap(present: bool) -> Box | None:
    if present:
        return Box(99)
    return None


def main() -> None:
    b: Box | None = wrap(True)
    if b is not None:
        print(b.value)
    n: Box | None = wrap(False)
    print(n is None)
    i: int | None = None
    print(i is None)
