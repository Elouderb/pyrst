# Regression: a raised exception whose MESSAGE itself contains the literal
# substring " panic: " must be captured INTACT by `except ... as e`.
#
# The exception payload separates the type from the message with a NUL byte
# (a delimiter that cannot occur in user data), so a message containing the
# old human-readable " panic: " separator is no longer mangled or truncated.
# The type dispatch (ValueError) must still match, and the bound `e` text must
# equal the original message byte-for-byte.
def main() -> None:
    try:
        raise ValueError("boom panic: tail")
    except ValueError as e:
        print("caught: " + e)

    # A message that is *only* the delimiter substring still round-trips.
    try:
        raise ValueError(" panic: ")
    except ValueError as e:
        print("caught: [" + e + "]")

    print("done")
