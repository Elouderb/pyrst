# list.pop() (last element) and list.pop(i) (by index) — codegen gap fix.
def main() -> None:
    xs: list[int] = [10, 20, 30]
    last: int = xs.pop()
    print(last)
    print(len(xs))
    first: int = xs.pop(0)
    print(first)
    print(len(xs))
    print(xs[0])
