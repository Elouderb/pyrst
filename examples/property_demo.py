class Counter:
    _count: int
    def __init__(self) -> None:
        self._count = 7
    @property
    def count(self) -> int:
        return self._count

def main() -> None:
    c: Counter = Counter()
    print(c.count)
