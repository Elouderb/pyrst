# Unicode string literals: accented Latin, CJK, and emoji must round-trip
# through the lexer, codegen, and the compiled binary byte-for-byte.
def main() -> None:
    accented: str = "café déjà vu"
    cjk: str = "日本語 世界"
    emoji: str = "rocket 🚀 star ✨"
    print(accented)
    print(cjk)
    print(emoji)

    name: str = "naïve"
    print(f"f-string with {name} and 日本語")

    combined: str = accented + " — " + cjk
    print(combined)
    print(len(emoji))
