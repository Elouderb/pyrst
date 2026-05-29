class Animal:
    name: str
    sound: str

    def speak(self) -> str:
        return self.sound


class Dog(Animal):
    breed: str

    def describe(self) -> str:
        return f"{self.name} is a {self.breed}"


def main() -> None:
    d: Dog  = Dog()
    print(d.name)
    print(d.speak())

