# EPIC-5 C1 §D invariant: field access on a BASE-typed variable stays a TYPECK
# error. `a` is declared `Animal`; `breed` is a Dog-only field, so `a.breed` is
# rejected at typeck (via get_all_fields on the DECLARED type) — NOT deferred to
# the codegen subtyping gate. This is a genuine type error, so it is a normal
# fail_* fixture exercised by BOTH the build and the typeck-check negative loops.
class Animal:
    name: str

    def __init__(self, name: str) -> None:
        self.name = name

class Dog(Animal):
    breed: str

    def __init__(self, name: str, breed: str) -> None:
        super().__init__(name)
        self.breed = breed

def main() -> None:
    a: Animal = Dog("Rex", "Labrador")
    print(a.breed)
