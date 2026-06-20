# Iterating dict.items() unpacks List[Tuple[str, int]], so `k` is str and `v`
# is int. Passing the str key where an int is expected must be rejected at
# typeck rather than deferred to rustc.
def expect_int(x: int) -> None:
    print(x)

def main() -> None:
    d: dict[str, int] = {"a": 1, "b": 2}
    for k, v in d.items():
        expect_int(k)  # k is str, not int
