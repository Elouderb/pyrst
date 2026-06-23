# EPIC-5 C2-3 golden: a polymorphic base used as a struct FIELD.
#   - `Zoo.star: Animal` is a base-typed field -> the field lowers to the
#     companion enum `Animal__`;
#   - constructing `Zoo(Dog(...))` wraps the subclass value into `Animal__::Dog`
#     at the constructor-argument site (the C2-3 constructor-arg fix);
#   - reading `z.star` then dispatching `.speak()` picks the subclass override;
#   - reading a base FIELD through the field (`z.star.name`) goes through the
#     companion-enum accessor.
# The containing struct `Zoo` correctly omits PartialEq/Default from its derives
# (the companion enum `Animal__` has neither), so it compiles.
class Animal:
    name: str

    def __init__(self, name: str) -> None:
        self.name = name

    def speak(self) -> str:
        return "..."

class Dog(Animal):
    def __init__(self, name: str) -> None:
        self.name = name

    def speak(self) -> str:
        return "woof"

class Cat(Animal):
    def __init__(self, name: str) -> None:
        self.name = name

    def speak(self) -> str:
        return "meow"

class Zoo:
    star: Animal

    def __init__(self, star: Animal) -> None:
        self.star = star

def main() -> None:
    z1: Zoo = Zoo(Dog("Rex"))
    z2: Zoo = Zoo(Cat("Tom"))
    # Base-typed field READ then polymorphic dispatch.
    print(z1.star.speak())
    print(z2.star.speak())
    # Base-field READ through a base-typed field (companion-enum accessor).
    print(z1.star.name)
