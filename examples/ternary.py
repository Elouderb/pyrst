# Conditional (ternary) expression: `body if test else orelse`.
def classify(n: int) -> str:
    # Right-associative nesting: a if p else (b if q else c).
    return "negative" if n < 0 else "zero" if n == 0 else "positive"

def main() -> None:
    x: int = 5
    print("big" if x > 3 else "small")
    y: int = 100 if x % 2 == 1 else 200
    print(y)
    print(classify(-3))
    print(classify(0))
    print(classify(7))
    print(f"{x} is {'odd' if x % 2 == 1 else 'even'}")
    parities: list[str] = ["even" if v % 2 == 0 else "odd" for v in range(3)]
    print(parities[0])
    print(parities[1])
    print(parities[2])
