# EPIC-5 C2 polymorphism: a Derived value into a Base-annotated slot. Was the C1
# honest-gate fixture (typeck-accepted, codegen-rejected); after the C2-2b-i
# keystone (companion-enum codegen) it is an ORDINARY positive — `a: Animal` is
# the companion enum `Animal__`, the `Dog(...)` value is wrapped as the `Dog`
# variant, and `a.speak()` dispatches through `Animal__::speak()`.
class Animal:
    name: str

    def __init__(self, name: str) -> None:
        self.name = name

    def speak(self) -> str:
        return self.name

class Dog(Animal):
    breed: str

    def __init__(self, name: str, breed: str) -> None:
        super().__init__(name)
        self.breed = breed

def main() -> None:
    # Derived value into a Base-annotated slot: typeck accepts, codegen wraps.
    a: Animal = Dog("Rex", "Labrador")
    print(a.speak())
