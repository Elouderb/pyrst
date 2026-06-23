# Dict views (keys/values/items) and set/list method results now carry their
# collection type, so they render via the repr path. Single-key dict keeps the
# output deterministic (multi-key dict iteration order is unspecified).
def main() -> None:
    d: dict[str, int] = {"x": 10}
    print(d.keys())
    print(d.values())
    print(d.items())
    a: set[int] = {1, 2}
    b: set[int] = {2, 3}
    print(a.union(b))
    xs: list[int] = [5, 6, 7]
    print(xs.copy())
