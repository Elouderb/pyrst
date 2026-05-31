def main() -> None:
    s: str = "hello world"

    # Every second character
    result1: str = s[::2]
    print(result1)

    # Starting from index 1, every second
    result2: str = s[1::2]
    print(result2)

    # Reverse string
    result3: str = s[::-1]
    print(result3)

    # Reverse with step
    result4: str = s[::-2]
    print(result4)
