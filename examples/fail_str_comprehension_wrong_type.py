# Negative: comprehension over a str binds str elements (consistent with for-loops),
# so using a char where an int is expected is caught at `pyrst check`.
def need_int(x: int) -> None:
    print(x)

def main() -> None:
    text: str = "abc"
    bad: list[int] = [need_int(c) for c in text]
