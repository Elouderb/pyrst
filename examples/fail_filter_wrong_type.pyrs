# filter preserves the iterable's element type: filter(pred, list[int]) returns
# list[int], not list[str]. With filter return-type inference (card 21424502) the
# type checker resolves the result to List(Int) and rejects the list[str]
# annotation at the type-checker layer.
def main() -> None:
    nums: list[int] = [1, 2, 3, 4]
    evens: list[str] = filter(lambda x: x % 2 == 0, nums)
    print(evens)
