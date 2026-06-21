# tokens.py — the lexer (tokenizer) stage of the expression interpreter.
#
# A Token carries a `kind` tag (a string, since pyrst has no enums) and the
# literal source `text` it was produced from. tokenize() turns a raw source
# string into a flat list[Token], hand-scanning character by character.
#
# Token kinds used across the project:
#   "NUM"  numeric literal (integer or decimal)
#   "NAME" identifier / variable name
#   "OP"   one of + - * / and parentheses ( )
#   "ASSIGN"  the '=' sign used by `let`-style bindings
#   "END"  synthetic end-of-input sentinel appended by tokenize()


class Token:
    kind: str
    text: str

    def __init__(self, kind: str, text: str) -> None:
        self.kind = kind
        self.text = text

    def describe(self) -> str:
        return self.kind + "(" + self.text + ")"


def is_op_char(c: str) -> bool:
    return c == "+" or c == "-" or c == "*" or c == "/" or c == "(" or c == ")"


def tokenize(src: str) -> list[Token]:
    tokens: list[Token] = []
    i: int = 0
    n: int = len(src)

    while i < n:
        c: str = src[i]

        # Skip runs of whitespace.
        if c == " " or c == "\t":
            i = i + 1
            continue

        # Numbers: a run of digits, optionally with a single decimal point.
        if c.isdigit():
            num: str = ""
            seen_dot: bool = False
            while i < n:
                d: str = src[i]
                if d.isdigit():
                    num = num + d
                    i = i + 1
                    continue
                if d == "." and not seen_dot:
                    seen_dot = True
                    num = num + d
                    i = i + 1
                    continue
                break
            tokens.append(Token("NUM", num))
            continue

        # Identifiers: a letter or underscore followed by letters/digits/underscores.
        if c.isalpha() or c == "_":
            name: str = ""
            while i < n:
                d2: str = src[i]
                if d2.isalpha() or d2.isdigit() or d2 == "_":
                    name = name + d2
                    i = i + 1
                    continue
                break
            tokens.append(Token("NAME", name))
            continue

        # Assignment.
        if c == "=":
            tokens.append(Token("ASSIGN", "="))
            i = i + 1
            continue

        # Operators and parentheses.
        if is_op_char(c):
            tokens.append(Token("OP", c))
            i = i + 1
            continue

        # Anything else is a lexical error. raise carries a message string,
        # which `except ... as e` rebinds as a plain str in the caller.
        raise ValueError("unexpected character: " + c)

    tokens.append(Token("END", ""))
    return tokens
