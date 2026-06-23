# D7: inherited field access.  Parent class sets self.balance: float in
# __init__; subclass instance reads self.balance in a method AND the
# external caller does float math on obj.balance.  Proves Attr resolves
# the inherited field via get_all_fields (not Unknown).
class BankAccount:
    balance: float

    def __init__(self, balance: float) -> None:
        self.balance = balance

    def deposit(self, amount: float) -> None:
        self.balance = self.balance + amount


class SavingsAccount(BankAccount):
    rate: float

    def __init__(self, balance: float, rate: float) -> None:
        super().__init__(balance)
        self.rate = rate

    def add_interest(self) -> None:
        # Reads self.balance — inherited from BankAccount.
        # Proves Attr resolves the inherited field as Float.
        self.balance = self.balance + (self.balance * self.rate)


def main() -> None:
    acc: SavingsAccount = SavingsAccount(100.0, 0.05)
    acc.add_interest()
    print(acc.balance)            # 105.0  — inherited field via subclass instance
    print(acc.balance + 1.0)      # 106.0  — float math on inherited field
