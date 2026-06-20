# print()/str()/f-strings render collections in CPython repr form: str elements
# quoted, bools as True/False, floats Python-style, nested collections recursing.
# Set/dict entries are emitted in a stable sorted-by-repr order.
def main() -> None:
    print([1, 2, 3])
    print(["a", "b", "c"])
    print([True, False])
    print([1.5, 2.0])
    print([[1, 2], [3, 4]])
    nums: list[int] = [10, 20, 30]
    print(nums)
    t: tuple[int, str, bool] = (1, "x", True)
    print(t)
    s: set[str] = {"apple", "banana", "cherry"}
    print(s)
    d: dict[str, int] = {"a": 1, "b": 2}
    print(d)
    print(f"data: {nums}")
    label: str = str([7, 8, 9])
    print(label)
