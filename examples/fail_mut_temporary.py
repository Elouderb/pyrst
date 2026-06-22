# Negative (EPIC-4 V2): a by-reference parameter (`Mut[T]`) must be given a
# PLACE — a variable, field, or index — never a temporary. Passing the result
# of a constructor call has no caller-visible storage to borrow `&mut` from, so
# it is an honest typeck error rather than a deferred rustc borrow failure.
#
# EXPECTED: typeck error — "by-reference parameter `account` requires a
# variable, not a temporary"

class Account:
    balance: int
    def __init__(self, balance: int) -> None:
        self.balance = balance

def make_account() -> Account:
    return Account(0)

def deposit(account: Mut[Account], amt: int) -> None:
    account.balance = account.balance + amt

def main() -> None:
    # `make_account()` is a temporary — not a borrowable place.
    deposit(make_account(), 5)
