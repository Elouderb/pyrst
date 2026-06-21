# transactions.py -- the immutable record of a single ledger event.

from money import format_cents

# A single posted transaction. `kind` is a discriminator string because pyrst
# has no enums; allowed values are "deposit", "withdraw", "transfer_in",
# "transfer_out", "interest". `delta` is the signed change to the balance in
# cents. `balance_after` is the running balance snapshot. `counterparty` is the
# other account id for transfers, "" otherwise.
class Transaction:
    seq: int
    account_id: str
    kind: str
    delta: int
    balance_after: int
    counterparty: str

    def __init__(self, seq: int, account_id: str, kind: str, delta: int, balance_after: int, counterparty: str) -> None:
        self.seq = seq
        self.account_id = account_id
        self.kind = kind
        self.delta = delta
        self.balance_after = balance_after
        self.counterparty = counterparty

    # Human label for the transaction kind.
    def label(self) -> str:
        if self.kind == "deposit":
            return "Deposit"
        elif self.kind == "withdraw":
            return "Withdrawal"
        elif self.kind == "transfer_in":
            return "Transfer in"
        elif self.kind == "transfer_out":
            return "Transfer out"
        elif self.kind == "interest":
            return "Interest"
        else:
            return "Unknown"

    # A one-line statement row, e.g. "#0003 Deposit        +$50.00  -> $150.00".
    def __str__(self) -> str:
        # Build the signed amount string. NOTE: each format_cents(...) result is
        # consumed exactly once -- pyrst MOVES a String local when it is passed
        # to a function or assigned to another local, so a value cannot be reused
        # after such a use. We therefore recompute rather than alias.
        signed: str = ""
        if self.delta >= 0:
            signed = "+" + format_cents(self.delta)
        else:
            signed = format_cents(self.delta)
        running: str = format_cents(self.balance_after)
        return f"#{self.seq:04d} {self.label():<13} {signed:>11}  -> {running}"

    # True if this row increased the balance.
    def is_credit(self) -> bool:
        return self.delta > 0


# Sum the absolute value of all debits (money leaving) in a history list.
def total_debits(history: list[Transaction]) -> int:
    out: int = 0
    for t in history:
        if t.delta < 0:
            out = out + (-t.delta)
    return out

# Sum all credits (money entering) in a history list.
def total_credits(history: list[Transaction]) -> int:
    out: int = 0
    for t in history:
        if t.delta > 0:
            out = out + t.delta
    return out
