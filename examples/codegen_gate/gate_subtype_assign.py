# EPIC-5 C1 honest gate: class subtyping is ACCEPTED by typeck but is not yet
# emittable by codegen (each pyrst class is a standalone Rust struct). This must
# PASS `pyrst check` (typeck) and be REJECTED by `pyrst build` (codegen) with the
# honest EPIC-5-C2 message, NOT a raw rustc E0308. In C2-F this becomes a
# positive once companion-enum codegen exists.
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
    # Derived value into a Base-annotated slot: typeck accepts, codegen gates.
    a: Animal = Dog("Rex", "Labrador")
    print(a.speak())
