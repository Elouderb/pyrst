# EPIC-4 V1-bc: the E0382 fix. Passing `self.field` (a non-Copy list field) into
# a constructor must clone the field place, not move it out of `&self`. Before
# clone-on-use for Attr places, `Wrapper(self.items)` could not compile.
class Wrapper:
    def __init__(self, data: list[int]) -> None:
        self.data = data

    def size(self) -> int:
        return len(self.data)

class Holder:
    def __init__(self, items: list[int]) -> None:
        self.items = items

    def wrap(self) -> Wrapper:
        # pass self.items into a constructor, then keep using self.items
        w = Wrapper(self.items)
        # self.items must still be readable here (it was cloned, not moved)
        self.items.append(99)
        return w

def main() -> None:
    h = Holder([1, 2, 3])
    w = h.wrap()
    # w holds the snapshot taken before append; h.items grew by one
    print(w.size())
    print(len(h.items))
    print(h.items[3])

main()
