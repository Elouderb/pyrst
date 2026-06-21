# D3: abs(float) -> float.  abs must preserve the Float type so that the
# result can be used in float arithmetic without a type error.
def main() -> None:
    x: float = abs(-1.5)
    print(x)               # 1.5
    print(x + 0.5)         # 2.0
