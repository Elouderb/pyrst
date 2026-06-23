# EPIC-4 V3: MRO-aware transitive &mut self.
# `Base.bump` mutates self.value. `Derived` does NOT redefine bump; it only adds
# `tick`, which calls the INHERITED `self.bump()`. The fixpoint resolves
# `self.bump` through Derived's MRO (resolved_methods includes the inherited
# bump, keyed under Derived) so `tick` is emitted &mut self and the inherited
# mutation persists. Without MRO-aware resolution `tick` would be &self -> E0596.
class Base:
    value: int
    def __init__(self) -> None:
        self.value = 0
    def bump(self) -> None:
        self.value = self.value + 5

class Derived(Base):
    def __init__(self) -> None:
        super().__init__()
    def tick(self) -> None:
        self.bump()

def main() -> None:
    d: Derived = Derived()
    d.tick()
    d.tick()
    print(d.value)
