# Exception-class hierarchy matching:
#   (a) a broad handler catches a subclass — except LookupError catches KeyError.
#   (b) handler ordering is respected — a narrow handler placed first takes priority
#       over a later broad handler for the same exception.

def lookup() -> None:
    raise KeyError("missing key")

def divide() -> None:
    raise ZeroDivisionError("division by zero")

def main() -> None:
    # (a) LookupError handler catches a raised KeyError.
    try:
        lookup()
    except LookupError as e:
        print("caught LookupError (was KeyError): " + e)

    # (b) Narrow handler placed before broad: KeyError matches first.
    try:
        lookup()
    except KeyError as e:
        print("narrow KeyError caught first: " + e)
    except LookupError as e:
        print("broad LookupError (should not appear): " + e)

    # (c) ArithmeticError catches ZeroDivisionError.
    try:
        divide()
    except ArithmeticError as e:
        print("caught ArithmeticError (was ZeroDivisionError): " + e)

    print("done")
