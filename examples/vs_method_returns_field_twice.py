# EPIC-4 V1-bc: a method returning self.field (a non-Copy list) clones the field
# place via emit_consuming (the deleted should_clone special-case is subsumed).
# Calling it twice must work — each return hands back an independent copy and
# &self is never moved-out-of.
class Registry:
    def __init__(self, names: list[str]) -> None:
        self.names = names

    def snapshot(self) -> list[str]:
        return self.names

def main() -> None:
    r = Registry(["x", "y"])
    first = r.snapshot()
    second = r.snapshot()
    # mutate one snapshot; the other and the registry stay independent
    first.append("z")
    print(len(first))
    print(len(second))
    print(len(r.names))
    print(r.names[0])

main()
