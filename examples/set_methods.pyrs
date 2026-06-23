# Set mutation and algebra methods (codegen gap fix). Outputs use len()/bool
# predicates only, since HashSet iteration order is non-deterministic.
def main() -> None:
    a: set[int] = {1, 2, 3}
    a.add(4)
    print(len(a))
    a.discard(4)
    print(len(a))
    a.remove(1)
    print(len(a))
    b: set[int] = {3, 4, 5}
    print(len(a.union(b)))
    print(len(a.intersection(b)))
    print(len(a.difference(b)))
    print(len(a.symmetric_difference(b)))
    print(a.issubset(b))
    print(b.issuperset(b))
    print(a.isdisjoint({9, 8}))
    c: set[int] = {10}
    c.update({11, 12})
    print(len(c))
