# EPIC-4 V3: 3-deep transitive &mut self chain.
# Only `advance` directly mutates self (self.pos). `step` calls `advance`, and
# `run` calls `step`. The fixpoint must propagate &mut self up TWO links:
# advance -> step -> run, or rustc rejects the call in `run` (and `step`) with
# E0596. `total` is read-only and must stay &self (no over-marking).
class Machine:
    pos: int
    def __init__(self) -> None:
        self.pos = 0
    def advance(self) -> None:
        self.pos = self.pos + 2
    def step(self) -> None:
        self.advance()
    def run(self, times: int) -> None:
        for _ in range(times):
            self.step()
    def total(self) -> int:
        return self.pos

def main() -> None:
    m: Machine = Machine()
    m.run(4)
    print(m.total())
