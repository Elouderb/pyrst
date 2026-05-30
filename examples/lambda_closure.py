def main() -> None:
    multiplier: int = 5
    result: int = (lambda x: x * multiplier)(3)
    print(result)
