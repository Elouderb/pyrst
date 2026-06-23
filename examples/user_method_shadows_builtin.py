# EPIC-6: receiver-type-guarded builtin method dispatch.
#
# A user class may define methods whose names collide with builtin
# str/list/dict/set method names (get, keys, values, items, update, split).
# Calling them on a *class instance* must dispatch to the USER method, not the
# builtin lowering. Before the dispatch guard, `r.get(2)` wrongly lowered to a
# dict `.get(&2).cloned()` and `r.split()` to `.split_whitespace()...` — a
# silent miscompile of user code. Each method below returns a value a builtin
# on this receiver could never produce, so the printed output proves the user
# method ran.

class Registry:
    base: int

    def __init__(self, base: int) -> None:
        self.base = base

    # collides with dict.get
    def get(self, k: int) -> int:
        return self.base + k

    # collides with dict.keys
    def keys(self) -> int:
        return self.base * 10

    # collides with dict.values
    def values(self) -> int:
        return self.base * 100

    # collides with dict.items
    def items(self) -> int:
        return self.base - 1

    # collides with str.split / list — and mutates self
    def split(self) -> int:
        self.base = self.base * 2
        return self.base

    # collides with dict.update — mutating, takes an argument
    def update(self, delta: int) -> None:
        self.base = self.base + delta


def main() -> None:
    r = Registry(5)

    # get(k) -> base + k  (NOT a dict .get)
    print(r.get(2))      # 7
    print(r.get(10))     # 15

    # keys() -> base * 10  (NOT a dict .keys() view)
    print(r.keys())      # 50

    # values() -> base * 100
    print(r.values())    # 500

    # items() -> base - 1
    print(r.items())     # 4

    # split() doubles base and returns it (NOT a str split)
    print(r.split())     # 10
    print(r.get(0))      # 10  (base is now 10)

    # update(delta) mutates base in place
    r.update(7)
    print(r.get(0))      # 17

    # Sanity: the genuine builtins still work on real builtin receivers.
    d: dict[int, int] = {1: 100, 2: 200}
    print(d.get(2, -1))  # 200  (real dict.get, two-arg form)
    print(d.get(9, -1))  # -1   (absent -> default)
    text: str = "a b c"
    parts: list[str] = text.split()
    print(len(parts))    # 3
