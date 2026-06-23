# `except E as e` binds the exception message as a str usable in the handler.
def main() -> None:
    try:
        raise RuntimeError("something broke")
    except RuntimeError as e:
        msg: str = e
        print("caught: " + msg)
        print("length: " + str(len(msg)))
    print("recovered")
