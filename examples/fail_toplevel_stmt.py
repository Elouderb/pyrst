# Negative fixture: a top-level assignment statement (not inside any function
# or class) must be rejected by both `pyrst check` and `pyrst build` with an
# honest error — not silently dropped.

def helper() -> int:
    return 42

x: int = 5   # top-level assignment — not supported

def main() -> None:
    pass
