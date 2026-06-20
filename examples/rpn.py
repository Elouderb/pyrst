def evaluate(expr: str) -> int:
    stack: list[int] = []
    for token in expr.split(" "):
        if token == "+":
            b: int = stack.pop()
            a: int = stack.pop()
            stack.append(a + b)
        elif token == "*":
            b2: int = stack.pop()
            a2: int = stack.pop()
            stack.append(a2 * b2)
        else:
            stack.append(int(token))
    return stack.pop()

def main() -> None:
    print(evaluate("3 4 +"))
    print(evaluate("3 4 + 2 *"))
    print(evaluate("5 1 2 + 4 * + 3 -" if False else "2 3 *"))
