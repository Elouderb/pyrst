# Item 1 (card c27fbc7f): exercise the Display dedup across the RESOLVED
# (transitive) method set, both directions.
#
#   InheritsStr:  inherits __str__ from Base, defines its own __repr__.
#   InheritsRepr: inherits __repr__ from Base, defines its own __str__.
#
# In each subclass the resolved set contains BOTH a __str__ and a __repr__
# (one inherited, one local). Without trait-level dedup each subclass would
# emit two `impl Display` -> E0119. The dedup prefers __str__, so print()
# always shows the __str__ rendering regardless of which one is inherited.
class Base:
    n: int

    def __init__(self, n: int) -> None:
        self.n = n

    def __str__(self) -> str:
        return "base-str(" + str(self.n) + ")"

    def __repr__(self) -> str:
        return "base-repr(" + str(self.n) + ")"


class InheritsStr(Base):
    def __init__(self, n: int) -> None:
        self.n = n

    # Inherits __str__ from Base; overrides __repr__ locally.
    def __repr__(self) -> str:
        return "child-repr(" + str(self.n) + ")"


class InheritsRepr(Base):
    def __init__(self, n: int) -> None:
        self.n = n

    # Inherits __repr__ from Base; overrides __str__ locally.
    def __str__(self) -> str:
        return "child-str(" + str(self.n) + ")"


def main() -> None:
    a: InheritsStr = InheritsStr(1)
    b: InheritsRepr = InheritsRepr(2)
    # a: __str__ inherited from Base -> "base-str(1)".
    print(a)
    # b: __str__ defined locally -> "child-str(2)".
    print(b)
