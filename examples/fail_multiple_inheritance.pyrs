# Negative: a class declaration with more than one base class is rejected at typeck
# with "multiple inheritance is not supported". This must be caught by `pyrst check`
# (typeck-level), not deferred to rustc. pyrst supports only single inheritance.

class A:
    def hello(self) -> None:
        pass

class B:
    def world(self) -> None:
        pass

class C(A, B):
    def greet(self) -> None:
        pass

def main() -> None:
    pass
