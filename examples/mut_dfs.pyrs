# EPIC-4 V2-c: Mut[set] by-reference param. `visit` fills the CALLER's set
# in place; membership checks afterward prove the mutation persisted. Iteration
# order of a set is unspecified, so we assert via deterministic membership and
# size, not by printing the set.
def visit(visited: Mut[set[int]], node: int) -> None:
    visited.add(node)

def main() -> None:
    seen: set[int] = set()
    visit(seen, 1)
    visit(seen, 2)
    visit(seen, 2)
    print(len(seen))
    print(1 in seen)
    print(2 in seen)
    print(3 in seen)
