# `**` exponentiation into a float-annotated binding must produce f64. typeck
# types `**` as float; the old codegen emitted i64 for int**int, so
# `x: float = 2 ** 3` failed to compile (rustc E0308). The fix promotes the
# integer power to f64 at the float-annotated binding.
def main() -> None:
    # Constant int ** int folded then bound to a float.
    x: float = 2 ** 3
    print(x)            # 8.0

    # Variable int ** int (not constant-folded) bound to a float.
    a: int = 3
    b: int = 2
    y: float = a ** b
    print(y)            # 9.0

    # A larger int ** int folded into a float binding.
    z: float = 2 ** 10
    print(z)            # 1024.0

    # A float operand already produced a float power.
    w: float = 2.0 ** 3
    print(w)            # 8.0
