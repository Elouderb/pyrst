# D1: str index -> str.  s[0] must be typed Str (not Unknown) so that
# calling a str method on the result type-checks and compiles correctly.
def main() -> None:
    s: str = "hello"
    c: str = s[0]
    print(c.upper())       # H — proves c is Str, not Unknown
    print(c + "!")         # h! — proves str concat works on indexed char
