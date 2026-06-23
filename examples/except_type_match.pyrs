# Exception handlers match on the raised type and fall through to the
# correct handler; execution resumes after the try block.
def risky(kind: str) -> None:
    if kind == "value":
        raise ValueError("bad value")
    raise KeyError("missing key")

def main() -> None:
    try:
        risky("value")
    except KeyError as e:
        print("caught KeyError: " + e)
    except ValueError as e:
        print("caught ValueError: " + e)

    try:
        risky("key")
    except KeyError as e:
        print("caught KeyError: " + e)
    except ValueError as e:
        print("caught ValueError: " + e)

    print("done")
