# Guards Python-style True/False printing for bool literals, comparisons,
# and bool-returning predicate methods (str is*/startswith).
def main() -> None:
    print(True)
    print(False)
    print("12".isdigit())
    print("ab".isdigit())
    print("hello".startswith("he"))
    print(2 < 3)
