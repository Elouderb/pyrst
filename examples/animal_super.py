class Animal:
    name: str
    sound: str

    def __init__(self, name: str, sound: str) -> None:
        self.name = name
        self.sound = sound

    def speak(self) -> str:
        return self.sound

class Dog(Animal):
    breed: str

    def __init__(self, name: str, breed: str) -> None:
        super().__init__(name, "Woof")
        self.breed = breed

    def describe(self) -> str:
        return self.name

def main() -> None:
    d: Dog = Dog("Rex", "Labrador")
    print(d.speak())
    print(d.describe())
    print(d.breed)
