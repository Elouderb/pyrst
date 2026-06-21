# Negative: calling an in-place mutating method on a by-value non-Copy
# parameter is rejected at typeck (not deferred to rustc).  The mutation is
# invisible to the caller because the container is a Rust clone of the caller's
# value — classic silent wrong-output bug (e.g. dfs fills a copy of visited).
#
# EXPECTED: typeck error — "mutation of by-value parameter `visited` is not
# visible to the caller; mutate via a method on it or return the updated value"

def mark(visited: set[int], node: int) -> None:
    visited.add(node)
