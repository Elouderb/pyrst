class Account:
    owner: str
    balance: int
    def __init__(self, owner: str, balance: int) -> None:
        self.owner = owner
        self.balance = balance
    def deposit(self, amount: int) -> None:
        self.balance = self.balance + amount
    def __str__(self) -> str:
        return f"{self.owner}: {self.balance}"

class Savings(Account):
    rate: int
    def __init__(self, owner: str, balance: int, rate: int) -> None:
        super().__init__(owner, balance)
        self.rate = rate
    def add_interest(self) -> None:
        self.balance = self.balance + (self.balance * self.rate) // 100

def main() -> None:
    a: Account = Account("Alice", 100)
    a.deposit(50)
    print(a.balance)
    s: Savings = Savings("Bob", 200, 10)
    s.add_interest()
    print(s.balance)
    accts: list[Account] = [a]
    total: int = 0
    for acct in accts:
        total = total + acct.balance
    print(total)
