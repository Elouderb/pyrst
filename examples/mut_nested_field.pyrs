# EPIC-4 V2-d: the nested-FIELD mutation shape that the by-value backstop now
# rejects, done CORRECTLY through a Mut[T] by-reference param. `record` borrows
# the caller's DataSet `&mut` and appends into its `values` list field in place;
# the mutation persists to the caller. This is the positive counterpart to
# fail_param_mutate_nested_field.py (which mutates `ds.values` by value and is
# now a loud compile error), demonstrating the remedy the backstop points at.

class DataSet:
    values: list[int]
    def __init__(self) -> None:
        self.values = []

def record(ds: Mut[DataSet], x: int) -> None:
    ds.values.append(x)

def main() -> None:
    d: DataSet = DataSet()
    record(d, 10)
    record(d, 20)
    record(d, 30)
    print(len(d.values))
    print(d.values[0])
    print(d.values[1])
    print(d.values[2])
