# EPIC-4 V3: fixpoint convergence under mutual recursion (A <-> B).
# `ping` calls `pong` and `pong` calls `ping` — a cycle in the self-call graph.
# `ping` itself mutates self.count; `pong` mutates self ONLY by reaching `ping`
# through the cycle. The monotone-boolean fixpoint must converge (it caps
# iterations defensively) and mark BOTH &mut self: ping directly, pong
# transitively. The `n` guard makes the recursion terminate at runtime.
class Bouncer:
    count: int
    def __init__(self) -> None:
        self.count = 0
    def ping(self, n: int) -> None:
        self.count = self.count + 1
        if n > 0:
            self.pong(n - 1)
    def pong(self, n: int) -> None:
        if n > 0:
            self.ping(n - 1)

def main() -> None:
    b: Bouncer = Bouncer()
    b.ping(5)
    print(b.count)
