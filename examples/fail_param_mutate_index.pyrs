# Negative: index-assigning into a by-value non-Copy parameter is rejected at
# typeck (not deferred to rustc).  The mutation is invisible to the caller
# because the list is a Rust clone of the caller's value.
#
# EXPECTED: typeck error — "mutation of by-value parameter `items` is not
# visible to the caller; mutate via a method on it or return the updated value"

def zero_first(items: list[int]) -> None:
    items[0] = 0
