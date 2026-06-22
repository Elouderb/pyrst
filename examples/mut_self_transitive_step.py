# EPIC-4 V3: transitive &mut self.
# `step` mutates self ONLY by calling the mutating `advance`. Before V3, `step`
# was emitted `&self` and rustc rejected the inner `self.advance()` call with
# E0596 (cannot borrow `*self` as mutable). After V3 the call-graph fixpoint
# marks `step` &mut self too, so the mutation to self.pos persists to the caller.
class Counter:
    pos: int
    def __init__(self) -> None:
        self.pos = 0
    def advance(self) -> None:
        self.pos = self.pos + 1
    def step(self) -> None:
        self.advance()

def main() -> None:
    c: Counter = Counter()
    c.step()
    c.step()
    c.step()
    print(c.pos)
