# Positive: the safe pattern for updating a by-value parameter is to construct
# a new value and return it.  This must compile and run without errors.

def add_item(items: list[int], x: int) -> list[int]:
    result: list[int] = []
    for item in items:
        result.append(item)
    result.append(x)
    return result

def increment_all(nums: list[int], delta: int) -> list[int]:
    out: list[int] = []
    for n in nums:
        out.append(n + delta)
    return out

def main() -> None:
    xs: list[int] = [1, 2, 3]
    ys: list[int] = add_item(xs, 4)
    zs: list[int] = increment_all(ys, 10)
    print(len(ys))
    print(len(zs))
