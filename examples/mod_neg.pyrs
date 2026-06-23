# Modulo `%` must take the sign of the divisor (Python semantics), not the sign
# of the dividend (Rust's `%`). Every case below has a negative operand, where
# the old lowering silently produced the wrong sign.
def main() -> None:
    print(-7 % 3)       # Python:  2   (Rust % gives -1)
    print(7 % -3)       # Python: -2   (Rust % gives  1)
    print(-7 % -3)      # Python: -1
    print(7 % 3)        # Python:  1   (non-negative: unchanged)

    # Variable operands (not constant-folded) exercise the BinOp lowering.
    a: int = -7
    b: int = 3
    print(a % b)        # Python: 2

    # Augmented modulo `%=` with a negative accumulator.
    c: int = -7
    c %= 3
    print(c)            # Python: 2

    # Float modulo takes the divisor's sign too.
    print(-7.0 % 3.0)   # Python: 2.0
    print(7.0 % -3.0)   # Python: -2.0
