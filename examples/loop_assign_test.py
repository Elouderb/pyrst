def test_update() -> int:
    x: int  = 5
    for n in [1, 2, 3]:
        x = n
    return x


def main() -> None:
    result: int  = test_update()
    print(result)

