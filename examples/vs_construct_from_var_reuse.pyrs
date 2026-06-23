# EPIC-4 V1-bc: constructing a value from a non-Copy variable must DEEP-CLONE
# the variable so the original binding stays usable afterward (Python value
# semantics). Without clone-on-use at the constructor arg, `nums` would be moved
# into `Box` and the later `len(nums)` would be a Rust E0382 on valid Python.
class Box:
    def __init__(self, items: list[int]) -> None:
        self.items = items

    def total(self) -> int:
        s = 0
        for v in self.items:
            s = s + v
        return s

def main() -> None:
    nums = [10, 20, 30]
    b = Box(nums)
    # mutate the box's copy; the original list must be untouched (independent value)
    b.items.append(40)
    # reuse nums AFTER it was consumed by the constructor
    print(len(nums))
    print(nums[0])
    print(len(b.items))
    print(b.total())

main()
