# Negative: a decorator that is not in the supported whitelist {staticmethod, property,
# dataclass} is rejected at typeck with "decorator `@<name>` is not supported".
# This must be caught by `pyrst check` (typeck-level), not deferred to rustc.

@memoize
def fib(n: int) -> int:
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

def main() -> None:
    pass
