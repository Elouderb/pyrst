# Item 5: finally runs on every path — after a caught exception and on the
# success path.
def main() -> None:
    # finally after a caught exception
    try:
        raise ValueError("boom")
    except ValueError as e:
        print("caught: " + e)
    finally:
        print("finally after catch")

    # finally on the success path
    try:
        print("no error")
    finally:
        print("finally after success")

    print("done")
