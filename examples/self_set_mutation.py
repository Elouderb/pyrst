# Regression: self.field.add(x) inside a method must infer &mut self.
# Previously method_modifies_self missed 'add'/'discard', so the generated
# Rust had &self, causing a compile error.

class Bag:
    items: set[int]

    def __init__(self) -> None:
        self.items = set()

    def insert(self, x: int) -> None:
        self.items.add(x)

    def remove_item(self, x: int) -> None:
        self.items.discard(x)

    def size(self) -> int:
        return len(self.items)

    def has(self, x: int) -> bool:
        return x in self.items


def main() -> None:
    b = Bag()
    b.insert(10)
    b.insert(20)
    b.insert(30)
    print(b.size())
    print(b.has(20))
    print(b.has(99))
    b.remove_item(20)
    print(b.size())
    print(b.has(20))
