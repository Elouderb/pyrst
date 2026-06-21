def greet(name: str) -> None:
    print(name)

def main() -> None:
    xs: list[str] = ["alpha", "beta", "gamma"]
    for i, x in enumerate(xs):
        greet(i)

main()
