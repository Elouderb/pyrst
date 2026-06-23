# EPIC-6 negative: `crate` is a Rust keyword with NO raw-identifier form
# (`r#crate` is rejected by rustc), so a USER identifier named `crate` cannot be
# lowered. pyrst must reject it HONESTLY at typeck (in both `check` and `build`)
# rather than emit `crate` and let rustc fail with a confusing message.


def use_crate(crate: int) -> int:
    return crate + 1


def main() -> None:
    print(use_crate(5))
