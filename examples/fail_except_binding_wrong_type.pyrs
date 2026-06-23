# The bound exception value is a str; assigning it to an int must be rejected
# by the type checker, not deferred to rustc.
def main() -> None:
    try:
        raise ValueError("oops")
    except ValueError as e:
        x: int = e
        print(x)
