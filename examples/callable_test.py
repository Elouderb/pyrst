def my_func() -> None:
    pass

class MyClass:
    pass

def main() -> None:
    x: int = 5
    print(callable(my_func))
    print(callable(MyClass))
    print(callable(x))
    print(callable(print))
