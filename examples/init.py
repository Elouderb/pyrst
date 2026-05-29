class Counter:
    count: int

    def __init__(self) -> None:
        self.count = 0

    def increment(self) -> None:
        self.count += 1

    def value(self) -> int:
        return self.count

def main() -> None:
    c: Counter = Counter()
    c.increment()
    c.increment()
    c.increment()
    print(c.value())
