# EPIC-4 V2-c: Mut[T] by-reference param — the callee's mutation of the
# caller's Account PERSISTS (the whole point). `try_withdraw` borrows `&mut`,
# so the caller's balance reflects the withdrawal.
class Account:
    owner: str
    balance: int
    def __init__(self, owner: str, balance: int) -> None:
        self.owner = owner
        self.balance = balance

def try_withdraw(account: Mut[Account], amt: int) -> bool:
    if account.balance >= amt:
        account.balance = account.balance - amt
        return True
    return False

def main() -> None:
    a: Account = Account("Alice", 100)
    ok1: bool = try_withdraw(a, 30)
    print(a.balance)
    print(ok1)
    ok2: bool = try_withdraw(a, 1000)
    print(a.balance)
    print(ok2)
