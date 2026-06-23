# EPIC-4 V2-c: the V2-ab check-pass-only case promoted to a real build+run
# golden. A Mut[Account] param that IS mutated now emits `&mut Account` and the
# mutation persists to the caller (in V2-ab codegen could not emit &mut, so this
# was a check-only assertion; now it is a full golden).
class Account:
    owner: str
    balance: int
    def __init__(self, owner: str, balance: int) -> None:
        self.owner = owner
        self.balance = balance

def deposit(account: Mut[Account], amt: int) -> None:
    account.balance = account.balance + amt

def main() -> None:
    a: Account = Account("Bob", 200)
    deposit(a, 50)
    deposit(a, 50)
    print(a.owner)
    print(a.balance)
