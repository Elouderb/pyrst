# accounts.py -- account model, the inheritance demo, and ledger exceptions.
#
# Two things live here:
#   1. A *unified* Account struct used for polymorphic storage in the bank's
#      dict[str, Account]. pyrst has no subtype polymorphism (a dict[str, Base]
#      cannot hold Derived instances), so account variation is carried by a
#      `kind` discriminator string plus per-kind rule fields, NOT by subclasses.
#   2. Genuine SavingsAccount / CheckingAccount subclasses that demonstrate
#      inheritance, super(), method override, and dunder __str__. They are used
#      standalone (not stored in the heterogeneous dict) precisely because the
#      language forbids storing mixed subclasses together.

from money import format_cents
from money import interest_cents

# ---- Ledger exceptions -------------------------------------------------------
# Custom exception classes. In pyrst a `raise Foo("msg")` panics with a payload
# tagged by the class name, and `except Foo as e:` matches that exact name and
# binds the message string. The bodies are empty placeholders -- the class name
# is what the runtime matches on.

class InsufficientFundsError(Exception):
    pass

class InvalidAmountError(Exception):
    pass

class AccountNotFoundError(Exception):
    pass

class AccountFrozenError(Exception):
    pass


# ---- Unified account (used by the Bank) -------------------------------------

class Account:
    account_id: str
    owner: str
    kind: str            # "checking" or "savings"
    balance: int         # cents
    rate_bps: int        # interest rate in basis points (savings only; 0 otherwise)
    overdraft_limit: int # how far below zero a checking account may go (cents)
    frozen: bool

    def __init__(self, account_id: str, owner: str, kind: str, balance: int, rate_bps: int, overdraft_limit: int) -> None:
        self.account_id = account_id
        self.owner = owner
        self.kind = kind
        self.balance = balance
        self.rate_bps = rate_bps
        self.overdraft_limit = overdraft_limit
        self.frozen = False

    # The lowest balance this account may reach. Checking allows an overdraft;
    # savings may never go negative.
    def floor(self) -> int:
        if self.kind == "checking":
            return -self.overdraft_limit
        return 0

    # Would a withdrawal of `amount` cents leave the balance at or above the
    # floor? Pure predicate; does not mutate.
    def can_withdraw(self, amount: int) -> bool:
        return (self.balance - amount) >= self.floor()

    # Interest accrued this period (savings earn rate_bps; checking earns 0).
    def accrue(self) -> int:
        if self.kind == "savings":
            return interest_cents(self.balance, self.rate_bps)
        return 0

    def __str__(self) -> str:
        status: str = "FROZEN" if self.frozen else "active"
        return f"[{self.account_id}] {self.owner:<10} {self.kind:<9} {format_cents(self.balance):>12} ({status})"


# ---- Inheritance demonstration ----------------------------------------------
# A small parallel hierarchy that exercises inheritance / super() / override.
# Kept separate from the Bank's dict because mixed subclasses cannot share a
# collection in pyrst.

class BaseAccount:
    holder: str
    cents: int

    def __init__(self, holder: str, cents: int) -> None:
        self.holder = holder
        self.cents = cents

    # Default: no interest, no overdraft. Subclasses override.
    def monthly_adjustment(self) -> int:
        return 0

    def account_type(self) -> str:
        return "base"

    def __str__(self) -> str:
        return f"{self.account_type()} account of {self.holder}: {format_cents(self.cents)}"


class SavingsAccount(BaseAccount):
    rate_bps: int

    def __init__(self, holder: str, cents: int, rate_bps: int) -> None:
        super().__init__(holder, cents)
        self.rate_bps = rate_bps

    # Override: savings accrue interest.
    def monthly_adjustment(self) -> int:
        return interest_cents(self.cents, self.rate_bps)

    def account_type(self) -> str:
        return "savings"

    # NOTE: __str__ must be redefined here. pyrst emits the Display impl only for
    # the class that *literally* defines __str__; an inherited __str__ is not
    # re-emitted for subclasses, so str(savings_instance) would otherwise fail
    # to compile. This is a real codegen limitation, not a stylistic choice.
    def __str__(self) -> str:
        return f"{self.account_type()} account of {self.holder}: {format_cents(self.cents)}"


class CheckingAccount(BaseAccount):
    monthly_fee: int

    def __init__(self, holder: str, cents: int, monthly_fee: int) -> None:
        super().__init__(holder, cents)
        self.monthly_fee = monthly_fee

    # Override: checking pays a maintenance fee (a negative adjustment).
    def monthly_adjustment(self) -> int:
        return -self.monthly_fee

    def account_type(self) -> str:
        return "checking"

    # See the note on SavingsAccount.__str__: inherited __str__ is not emitted
    # for subclasses, so it must be repeated here.
    def __str__(self) -> str:
        return f"{self.account_type()} account of {self.holder}: {format_cents(self.cents)}"
