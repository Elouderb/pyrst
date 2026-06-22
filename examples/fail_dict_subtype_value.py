# EPIC-5 C2-3 negative: a dict LITERAL with a subclass value into a base-typed
# dict slot is NOT yet wrapped. Unlike list[Animal] = [Dog(), Cat()] (which the
# keystone wraps element-wise), a `dict[str, Animal] = {"a": Dog(...)}` literal is
# rejected at TYPECK ("type mismatch in assignment: declared Dict(Str,
# Class(\"Animal\")), got Dict(Str, Class(\"Dog\"))") — dict-value subtype
# unification + element wrapping is a documented C2-3 limitation, an HONEST error,
# never a miscompile. Construct the value as `Animal` (or wrap explicitly) instead.
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

def main() -> None:
    d: dict[str, Animal] = {"a": Dog("Rex")}
    print(d["a"].speak())
