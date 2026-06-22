# EPIC-4 V2-c (STEP-3 seed path): a method whose ONLY self-mutation is passing a
# self-rooted place (`self.items`) into a `Mut[T]` FREE function. The V3 mut-self
# seed must recognize that forwarding `self.items` by-reference mutates self, so
# `accumulate` is emitted `&mut self` (otherwise `&mut self.items` would be rustc
# E0596). The field mutation persists across calls, proving end-to-end.
def push(buf: Mut[list[int]], val: int) -> None:
    buf.append(val)

class Accumulator:
    items: list[int]
    def __init__(self) -> None:
        self.items = []
    def accumulate(self, val: int) -> None:
        push(self.items, val)
    def total(self) -> int:
        s: int = 0
        for x in self.items:
            s = s + x
        return s

def main() -> None:
    acc: Accumulator = Accumulator()
    acc.accumulate(10)
    acc.accumulate(20)
    acc.accumulate(5)
    print(len(acc.items))
    print(acc.items[0])
    print(acc.items[2])
    print(acc.total())
