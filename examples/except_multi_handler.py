# Item 5: multiple handlers on one try; first matching handler wins and
# distinct exception types route to their own handler.
def boom(kind: str) -> None:
    if kind == "value":
        raise ValueError("v")
    if kind == "key":
        raise KeyError("k")
    raise RuntimeError("r")

def handle(kind: str) -> None:
    try:
        boom(kind)
    except ValueError as e:
        print("value handler: " + e)
    except KeyError as e:
        print("key handler: " + e)
    except RuntimeError as e:
        print("runtime handler: " + e)

def main() -> None:
    handle("value")
    handle("key")
    handle("runtime")
    print("done")
