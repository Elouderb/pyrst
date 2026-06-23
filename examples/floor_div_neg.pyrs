# Floor division `//` must floor toward negative infinity (Python semantics),
# not truncate toward zero (Rust's integer `/`), and float `//` must actually
# floor. All cases below have a negative operand or float operands, where the
# old truncating lowering silently produced the wrong value.
def main() -> None:
    # Integer floor division with negatives.
    print(-7 // 2)      # Python: -4   (truncation would give -3)
    print(7 // -2)      # Python: -4   (truncation would give -3)
    print(-7 // -2)     # Python:  3
    print(7 // 2)       # Python:  3   (non-negative: unchanged)

    # Variable operands (not constant-folded) exercise the BinOp lowering.
    a: int = -7
    b: int = 2
    print(a // b)       # Python: -4

    # Augmented floor division `//=` with a negative accumulator.
    c: int = -7
    c //= 2
    print(c)            # Python: -4

    # Float floor division must floor (old lowering left 3.5).
    print(7.0 // 2.0)   # Python: 3.0
    print(-7.0 // 2.0)  # Python: -4.0
