# Negative: dict[str,str].values() returns list[str], not int.
def expect_int(x: int) -> None:
    print(x)

def main() -> None:
    d: dict[str, str] = {"a": "b"}
    expect_int(d.values())
