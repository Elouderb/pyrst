# Item 4: prescan_types recurse into handler bodies.
# `e` is bound by `except E as e`; calling len(e) directly on e must use
# char-count (Ty::Str path), not byte-count.  The message "café" is 4
# characters but 5 UTF-8 bytes — a byte-count `.len()` would yield 5,
# the correct char-count `.chars().count()` yields 4.
def main() -> None:
    try:
        raise ValueError("café")
    except ValueError as e:
        print(len(e))
