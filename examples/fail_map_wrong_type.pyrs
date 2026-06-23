# map(lambda x: str(x), list[int]) returns list[str], not list[int]. With
# lambda/map return-type inference (card 21424502) the type checker now resolves
# the map result to List(Str) and rejects the list[int] annotation — the
# accept-at-typeck/reject-at-rustc gap is closed at the type-checker layer.
def main() -> None:
    nums: list[int] = [1, 2, 3]
    result: list[int] = map(lambda x: str(x), nums)
    print(result)
