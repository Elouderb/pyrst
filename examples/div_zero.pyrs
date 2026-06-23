# Regression: int // 0 must raise ZeroDivisionError, not silently return i64::MAX.
# int % 0 must also raise ZeroDivisionError (was an uncatchable Rust panic).
def main() -> None:
    x: int = 7
    y: int = 0

    # Floor division by zero — must be caught as ZeroDivisionError
    try:
        result: int = x // y
        print("WRONG: got " + str(result))
    except ZeroDivisionError as e:
        print("caught ZeroDivisionError from //: " + e)

    # Modulo by zero — must be caught as ZeroDivisionError
    try:
        result2: int = x % y
        print("WRONG: got " + str(result2))
    except ZeroDivisionError as e:
        print("caught ZeroDivisionError from %: " + e)

    # //= by zero
    z: int = 5
    try:
        z //= y
        print("WRONG: z is " + str(z))
    except ZeroDivisionError as e:
        print("caught ZeroDivisionError from //=: " + e)

    print("done")
