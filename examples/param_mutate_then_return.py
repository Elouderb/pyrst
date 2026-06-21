# Positive: mutating a by-value parameter and returning it is a valid pattern.
# The function receives its own copy of the caller's value (value semantics),
# mutates that copy, and returns it.  The caller's original is unchanged.
# This is a common functional idiom and must NOT be rejected by typeck.

def grow(xs: list[int]) -> list[int]:
    xs.append(99)
    return xs

def main() -> None:
    a: list[int] = [1, 2, 3]
    b: list[int] = grow(a)
    # b has the appended element
    print(len(b))
    print(b)
    # a is unchanged — value semantics: grow received a clone, not a reference
    print(len(a))
    print(a)
