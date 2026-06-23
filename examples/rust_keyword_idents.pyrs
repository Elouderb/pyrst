# EPIC-6: identifiers that collide with RUST keywords (but are NOT pyrst
# keywords) must round-trip through rustc via raw-identifier escaping (`r#kw`).
# Exercises a keyword-named free function (def + call), a keyword-named
# parameter (def + use), a keyword-named local (read + write + augassign), and
# a class with a keyword-named field (init + read + write + read-after-write).


class Token:
    type: str
    move: int

    def bump(self, loop: int) -> None:
        # `loop` is a keyword-named parameter; `self.move` is a keyword field.
        self.move = self.move + loop


def loop(type: int) -> int:
    # `loop` is a keyword-named free function; `type` is a keyword-named param.
    type = type + 1
    return type * 2


def main() -> None:
    # keyword-named local var: read, write, augassign
    type: int = 10
    type = type + 5
    type += 3
    print(type)

    # call the keyword-named free function with the keyword-named local
    print(loop(type))

    # keyword-named struct field: init, read, write via method, read-after-write
    t: Token = Token("ident", 7)
    print(t.type)
    print(t.move)
    t.move = 100
    print(t.move)
    t.bump(5)
    print(t.move)
