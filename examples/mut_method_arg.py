# EPIC-4 V2-c: a METHOD taking a Mut[T] argument. `Bank.pay_into` borrows the
# caller's Account `&mut` and credits it; the caller's balance reflects the
# change. The call site threads `&mut a` to the method.
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
    a: Account = Account(100)
    b.pay_into(a, 25)
    b.pay_into(a, 5)
    print(a.balance)
