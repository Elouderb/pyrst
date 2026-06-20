# tuple() constructs a tuple value (Ty::Tuple), not an int. Assigning it to an
# int-typed variable must be rejected at typeck rather than deferred to rustc.
def main() -> None:
    x: int = tuple()
    print(x)
