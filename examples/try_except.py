def main() -> None:
    result: int = 0
    try:
        x: int = 10
        y: int = 2
        result = x // y
    except:
        result = -1
    print(result)

