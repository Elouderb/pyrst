# Negative (EPIC-4 V2-d): calling an in-place mutating method on a FIELD of a
# by-value non-Copy class parameter is rejected at typeck. Before V2-d this
# escaped SILENTLY — codegen mutated a clone of `ds`, so the caller's `values`
# never grew (silent wrong output). The method-call backstop now roots the
# receiver via `root_ident`, so `ds.values.append(x)` fires with the Mut[T]
# remedy.
#
# EXPECTED: typeck error — "mutation of by-value parameter `ds` is not visible
# to the caller; ... or declare the parameter `Mut[T]` to mutate it in place"

class DataSet:
    values: list[int]
    def __init__(self) -> None:
        self.values = []

def add_value(ds: DataSet, x: int) -> None:
    ds.values.append(x)
