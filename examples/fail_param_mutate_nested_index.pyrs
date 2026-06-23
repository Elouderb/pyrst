# Negative (EPIC-4 V2-d): calling an in-place mutating method on an INDEX of a
# by-value non-Copy parameter is rejected at typeck. `rows` is a by-value
# `list[list[int]]`; `rows[0].append(x)` mutates a clone of the outer list, so
# the caller's data is never updated — a silent wrong-output bug before V2-d.
# The backstop now roots the receiver via `root_ident`, so this fires.
#
# EXPECTED: typeck error — "mutation of by-value parameter `rows` is not visible
# to the caller; ... or declare the parameter `Mut[T]` to mutate it in place"

def push_into_first(rows: list[list[int]], x: int) -> None:
    rows[0].append(x)
