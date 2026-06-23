# EPIC-5 soundness backstop: the result of a VOID function (`-> None`) must NOT
# satisfy an `Optional[T]` slot. The `None` LITERAL is `Optional`-compatible, but
# a void *call result* is not — its type is Unit, not the None value. Were this
# accepted, codegen would emit `Some(sink())` -> `Option<()>`, a silent miscompile
# caught only by rustc. typeck must reject this directly (both `check` and `build`).
def sink() -> None:
    print("hi")


def use_it() -> None:
    # `sink()` returns void (Unit), NOT the `None` literal — rejected at typeck.
    x: int | None = sink()
    print(x)


def main() -> None:
    use_it()
