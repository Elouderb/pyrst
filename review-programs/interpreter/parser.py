# parser.py — a recursive-descent / precedence-climbing parser.
#
# Grammar (lowest precedence first):
#   statement := NAME '=' expr        (a binding, e.g. `x = 3 + 4`)
#              | expr
#   expr      := term   (('+' | '-') term)*
#   term      := factor (('*' | '/') factor)*
#   factor    := '-' factor
#              | NUM
#              | NAME
#              | '(' expr ')'
#
# The Parser owns BOTH the token cursor and the NodePool. That is deliberate:
# pyrst passes class instances BY VALUE, so a free function that mutates a
# parser or pool argument has no visible effect in the caller (the mutation
# happens on a copy). Keeping all mutable state inside the Parser and only
# mutating it through `self.*` methods is the reliable pattern. Every parse_*
# method advances `self.pos` and pushes into `self.pool`, returning the integer
# index of the node it built.

from tokens import Token, tokenize
from ast_nodes import NodePool


class ParseResult:
    pool: NodePool
    root: int
    target: str   # variable name for a binding statement, else ""

    def __init__(self, pool: NodePool, root: int, target: str) -> None:
        self.pool = pool
        self.root = root
        self.target = target


class Parser:
    toks: list[Token]
    pos: int
    pool: NodePool
    # Recursion-depth counter for the descent. Beyond being a real guard
    # against pathological nesting, the explicit `self.depth = ...` writes in
    # each recursive method are LOad-bearing for codegen: pyrst decides whether
    # a method borrows `&self` or `&mut self` by scanning its body for a DIRECT
    # `self.x = ...` (or a known mutating call like append on self). It does
    # NOT see that calling another self-method (self.advance(), which writes
    # self.pos) or self.pool.add_binop() requires &mut. Without a direct
    # self-write, parse_expr/parse_term/parse_factor are emitted as `&self`
    # and then fail to borrow-check ("cannot borrow *self as mutable").
    # Touching self.depth in each method makes the inference correct.
    depth: int
    # Result fields. parse_statement writes the parsed root index and binding
    # target here rather than constructing a ParseResult inside the method:
    # passing `self.pool` (a non-Copy struct) into a constructor would move it
    # out of `&mut self`, which rustc rejects (pyrst inserts no clone). The
    # caller instead reads parser.pool / parser.root_index / parser.target_name
    # from OUTSIDE the object, a position where pyrst does clone on field read.
    root_index: int
    target_name: str

    def __init__(self, toks: list[Token]) -> None:
        self.toks = toks
        self.pos = 0
        self.pool = NodePool()
        self.depth = 0
        self.root_index = -1
        self.target_name = ""

    def peek_kind(self) -> str:
        return self.toks[self.pos].kind

    def peek_text(self) -> str:
        return self.toks[self.pos].text

    def at_end(self) -> bool:
        return self.toks[self.pos].kind == "END"

    def advance(self) -> Token:
        tok: Token = self.toks[self.pos]
        self.pos = self.pos + 1
        return tok

    def expect(self, kind: str) -> Token:
        if self.peek_kind() != kind:
            raise SyntaxError("expected " + kind + " but found " + self.peek_kind())
        # Inline the advance (rather than calling self.advance()) so this method
        # contains a DIRECT self-write and is therefore inferred `&mut self`.
        tok: Token = self.toks[self.pos]
        self.pos = self.pos + 1
        return tok

    # statement := NAME '=' expr | expr
    # Records the result into self fields (see root_index/target_name notes)
    # instead of returning it, so the caller can read self.pool from outside.
    def parse_statement(self) -> None:
        self.depth = self.depth + 1
        target: str = ""
        # Lookahead: NAME followed by '=' is a binding.
        if self.peek_kind() == "NAME" and self.toks[self.pos + 1].kind == "ASSIGN":
            name_tok: Token = self.advance()
            target = name_tok.text
            self.expect("ASSIGN")
        root: int = self.parse_expr()
        if not self.at_end():
            raise SyntaxError("unexpected trailing token: " + self.peek_text())
        self.root_index = root
        self.target_name = target
        self.depth = self.depth - 1

    # expr := term (('+' | '-') term)*
    def parse_expr(self) -> int:
        self.depth = self.depth + 1
        if self.depth > 200:
            raise RuntimeError("expression nested too deeply")
        left: int = self.parse_term()
        while self.peek_kind() == "OP" and (self.peek_text() == "+" or self.peek_text() == "-"):
            op_tok: Token = self.advance()
            right: int = self.parse_term()
            left = self.pool.add_binop(op_tok.text, left, right)
        self.depth = self.depth - 1
        return left

    # term := factor (('*' | '/') factor)*
    def parse_term(self) -> int:
        self.depth = self.depth + 1
        left: int = self.parse_factor()
        while self.peek_kind() == "OP" and (self.peek_text() == "*" or self.peek_text() == "/"):
            op_tok: Token = self.advance()
            right: int = self.parse_factor()
            left = self.pool.add_binop(op_tok.text, left, right)
        self.depth = self.depth - 1
        return left

    # factor := '-' factor | NUM | NAME | '(' expr ')'
    # Single exit point so the depth counter is balanced on the success path.
    def parse_factor(self) -> int:
        self.depth = self.depth + 1
        if self.depth > 200:
            raise RuntimeError("expression nested too deeply")
        kind: str = self.peek_kind()
        result: int = -1

        # Unary minus.
        if kind == "OP" and self.peek_text() == "-":
            self.advance()
            operand: int = self.parse_factor()
            result = self.pool.add_neg(operand)
        elif kind == "NUM":
            tok: Token = self.advance()
            result = self.pool.add_num(parse_float(tok.text))
        elif kind == "NAME":
            tok2: Token = self.advance()
            result = self.pool.add_var(tok2.text)
        elif kind == "OP" and self.peek_text() == "(":
            self.advance()
            inner: int = self.parse_expr()
            if self.peek_kind() != "OP" or self.peek_text() != ")":
                raise SyntaxError("missing closing parenthesis")
            self.advance()
            result = inner
        else:
            raise SyntaxError("unexpected token in expression: " + self.peek_text())

        self.depth = self.depth - 1
        return result


# Convert a numeric token's text into a float. pyrst's float() does not accept
# strings that contain a decimal point cleanly in every build, so we parse the
# integer and fractional parts by hand to stay deterministic and portable.
def parse_float(text: str) -> float:
    if "." not in text:
        return float(int(text))
    dot: int = text.find(".")
    whole_part: str = text[0:dot]
    frac_part: str = text[dot + 1:]
    whole: float = 0.0
    if len(whole_part) > 0:
        whole = float(int(whole_part))
    frac_value: float = 0.0
    scale: float = 1.0
    # NOTE: index-based iteration, NOT `for ch in frac_part:`.
    # pyrst's for-STATEMENT codegen emits `.iter().cloned()` even when the
    # iterable is a str, which is invalid Rust (`String` has no `iter`). Only
    # the list-COMPREHENSION form `[... for c in s]` correctly emits
    # `.chars()`. Hand-indexing the string sidesteps the broken path.
    fi: int = 0
    flen: int = len(frac_part)
    while fi < flen:
        ch: str = frac_part[fi]
        scale = scale * 10.0
        digit: int = ord(ch) - ord("0")
        frac_value = frac_value * 10.0 + float(digit)
        fi = fi + 1
    return whole + frac_value / scale


# Convenience entry point: tokenize then parse one statement.
# Reads the parser's result fields from OUTSIDE the object (where pyrst clones
# on field access) and packages them into a ParseResult for the caller.
def parse_source(src: str) -> ParseResult:
    toks: list[Token] = tokenize(src)
    parser: Parser = Parser(toks)
    parser.parse_statement()
    return ParseResult(parser.pool, parser.root_index, parser.target_name)
