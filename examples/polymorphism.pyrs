# EPIC-5 C2-2b-i polymorphism keystone golden: a polymorphic base `Animal`
# becomes the companion enum `Animal__`. This exercises every activation path:
#   - a HETEROGENEOUS list[Animal] literal of Dog + Cat constructors (each
#     element wrapped into the right enum variant) iterated with method dispatch;
#   - a base-typed PARAM `feed(a: Animal)` called with a Dog and a Cat (arg wrap);
#   - a base RETURN `-> Animal` returning a Dog (return wrap);
#   - a base-field READ `a.name` through a base var (companion-enum accessor).
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

# Base-typed PARAM: `a` is `Animal__`; `a.speak()` dispatches and `a.name`
# lowers to the companion-enum field accessor.
def feed(a: Animal) -> None:
    print(a.name + " says " + a.speak())

# Base RETURN slot: a Dog value is wrapped into `Animal__::Dog(..)`.
def make_pet() -> Animal:
    return Dog("Rex")

def main() -> None:
    # Heterogeneous list[Animal]: each constructor wrapped into its variant.
    animals: list[Animal] = [Dog("Fido"), Cat("Tom")]
    for a in animals:
        print(a.speak())

    # Base-typed param wrapping with two different subclasses.
    feed(Dog("Buddy"))
    feed(Cat("Whiskers"))

    # Base return wrapping + base-field read through a base var.
    pet: Animal = make_pet()
    print(pet.name)
    print(pet.speak())
