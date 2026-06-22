# EPIC-4 V2-c: recursive Mut[set] pass-through. `fill` mutates the CALLER's set
# and RECURSES, forwarding the same `visited` (itself a `&mut` binding) into the
# next call. The forwarded-by-reference arg must emit an explicit reborrow
# (`&mut *visited`) — a bare `&mut visited` would be rustc E0596. Membership and
# size are deterministic; set iteration order is not, so we don't print the set.
def fill(visited: Mut[set[int]], node: int) -> None:
    if node > 5:
        return
    visited.add(node)
    fill(visited, node + 1)

def main() -> None:
    seen: set[int] = set()
    fill(seen, 1)
    print(len(seen))
    print(1 in seen)
    print(3 in seen)
    print(5 in seen)
    print(6 in seen)
    print(0 in seen)
