# EPIC-5 C2-3 golden: 3-level hierarchy A <- B <- C, constructing the LEAF `C`
# directly into an `A`-typed slot. `C` is a variant of the companion enum `A__`
# (poly_map is transitive), so `a: A = C(...)` wraps as `A__::C(...)` and both
# method dispatch (`a.kind()`) and base-field read (`a.x`) resolve through `A__`.
# (This is the DIRECT-construct case, which WORKS — distinct from upcasting an
# intermediate base, e.g. `b: B = B(...); a: A = b`, which is an honest error.)
class A:
    x: int

    def __init__(self, x: int) -> None:
        self.x = x

    def kind(self) -> str:
        return "A"

class B(A):
    def __init__(self, x: int) -> None:
        self.x = x

    def kind(self) -> str:
        return "B"

class C(B):
    def __init__(self, x: int) -> None:
        self.x = x

    def kind(self) -> str:
        return "C"

def main() -> None:
    a: A = C(7)
    print(a.kind())
    print(a.x)
