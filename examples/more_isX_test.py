def main() -> None:
    # isnumeric
    print("123".isnumeric())
    print("abc".isnumeric())
    print("".isnumeric())

    # isprintable
    print(" ".isprintable())
    print("hello".isprintable())

    # istitle
    print("Hello World".istitle())
    print("hello world".istitle())
