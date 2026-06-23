# EPIC-4 V2-c NEGATIVE: a by-reference (`Mut[T]`) METHOD parameter requires a
# PLACE (variable / field / index), not a temporary. Passing a constructor
# result `Account(5)` has no caller-visible storage to borrow `&mut`, so it is an
# honest typeck error (the method-call place-check wired in V2-c).
class Account:
    balance: int
    def __init__(self, balance: int) -> None:
        self.balance = balance

class Bank:
    name: str
    def __init__(self, name: str) -> None:
        self.name = name
    def pay_into(self, acct: Mut[Account], amt: int) -> None:
        acct.balance = acct.balance + amt

def main() -> None:
    b: Bank = Bank("ACME")
    b.pay_into(Account(5), 25)
    print(b.name)
