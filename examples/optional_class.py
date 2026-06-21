# EPIC-5: Optional[Class] lowers to Option<Class>; narrowed field access;
# Option value flowing through (Option ~ Option) into another Optional slot.
class Point:
    x: int
    y: int


def make(present: bool) -> Optional[Point]:
    if present:
        return Point(3, 4)
    return None


def passthrough(p: Optional[Point]) -> Optional[Point]:
    return p


def main() -> None:
    p: Optional[Point] = make(True)
    q: Optional[Point] = passthrough(p)
    if q is not None:
        print(q.x)
        print(q.y)
    absent: Optional[Point] = make(False)
    print(absent is None)
