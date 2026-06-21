# main.py -- deterministic end-to-end scenario for the bank ledger.
#
# Run with:  pyrst build main.py && ./main   (from this directory)
# The imports pull in the other four modules (DFS resolution, flat namespace
# merge).
#
# CRITICAL value-semantics note: pyrst classes have VALUE semantics (a class is
# a Rust struct, copied on assignment and on pass-by-value). Passing `bank` into
# a helper that mutates it would mutate a COPY and silently lose the change. So
# every state-changing call below is made directly on `bank` in this function;
# the only helpers are pure (they format / inspect, they do not mutate the bank).

from money import dollars
from money import format_cents
from bank import Bank
from accounts import InsufficientFundsError
from accounts import InvalidAmountError
from accounts import AccountNotFoundError
from accounts import AccountFrozenError
from accounts import SavingsAccount
from accounts import CheckingAccount


def inheritance_demo() -> None:
    print("===== Inheritance demo (super / override / __str__) =====")
    # A heterogeneous list is impossible (no subtype polymorphism), so each
    # subclass instance is handled on its own.
    sav: SavingsAccount = SavingsAccount("Dana", dollars(2000), 250)   # 2.5%/period
    chk: CheckingAccount = CheckingAccount("Evan", dollars(800), dollars(5))
    print(str(sav))
    print(str(chk))
    print("savings monthly adjustment: " + format_cents(sav.monthly_adjustment()))
    print("checking monthly adjustment: " + format_cents(chk.monthly_adjustment()))
    # Apply one period and show the new balances.
    sav.cents = sav.cents + sav.monthly_adjustment()
    chk.cents = chk.cents + chk.monthly_adjustment()
    print("after one period:")
    print(str(sav))
    print(str(chk))


def main() -> None:
    bank: Bank = Bank("Pyrst Federal")

    # Open a mix of checking and savings accounts. Money is in cents.
    bank.open_account("AC-100", "Alice", "checking", dollars(1500), 0, dollars(200))
    bank.open_account("AC-200", "Bob", "savings", dollars(5000), 300, 0)
    bank.open_account("AC-300", "Carol", "checking", dollars(50), 0, dollars(100))
    bank.open_account("AC-400", "Dave", "savings", dollars(12000), 150, 0)

    print(bank.summary())
    print("")

    # --- normal operations (all called directly on `bank`) ---
    print("===== Operations =====")
    bank.deposit("AC-100", dollars(250))
    bank.deposit("AC-300", dollars(75))

    b1: int = bank.withdraw("AC-100", dollars(400))
    print("withdrew $400.00 from AC-100, balance now " + format_cents(b1))

    bank.transfer("AC-200", "AC-100", dollars(1000))
    print("transferred $1000.00 from AC-200 to AC-100")

    # --- overdraft rules: checking may go negative to its limit, savings may not ---
    print("")
    print("===== Overdraft / rule enforcement =====")
    # AC-300 (checking) has $125.00; limit $100.00 -> may reach -$100.00, so a
    # $200.00 withdrawal (-> -$75.00) is allowed.
    b2: int = bank.withdraw("AC-300", dollars(200))
    print("withdrew $200.00 from AC-300, balance now " + format_cents(b2))
    # ...but a further $50.00 (-> -$125.00) breaches the -$100.00 floor.
    try:
        bank.withdraw("AC-300", dollars(50))
        print("withdrew $50.00 from AC-300 (unexpected)")
    except InsufficientFundsError as e:
        print("DENIED further withdraw on " + e + ": would breach overdraft floor")
    # Savings AC-400 may never go negative: over-withdrawal is denied.
    try:
        bank.withdraw("AC-400", dollars(13000))
        print("withdrew $13000.00 from AC-400 (unexpected)")
    except InsufficientFundsError as e:
        print("DENIED withdraw on " + e + ": savings cannot go negative")

    # --- invalid amounts and frozen accounts ---
    print("")
    print("===== Validation =====")
    try:
        bank.withdraw("AC-100", 0)
        print("withdrew $0 (unexpected)")
    except InvalidAmountError as e:
        print("DENIED withdraw: " + e)

    bank.freeze("AC-200")
    try:
        bank.withdraw("AC-200", dollars(10))
        print("withdrew from frozen account (unexpected)")
    except AccountFrozenError as e:
        print("DENIED withdraw on frozen account " + e)
    try:
        bank.transfer("AC-200", "AC-100", dollars(10))
        print("transferred from frozen account (unexpected)")
    except AccountFrozenError as e:
        print("DENIED transfer from frozen account " + e)
    bank.unfreeze("AC-200")

    # --- missing account ---
    try:
        bank.deposit("AC-999", dollars(100))
        print("deposited to missing account (unexpected)")
    except AccountNotFoundError as e:
        print("no such account for deposit: " + e)

    # --- interest run ---
    print("")
    print("===== Interest posting =====")
    paid: int = bank.post_interest()
    print("total interest paid: " + format_cents(paid))

    # --- final report ---
    print("")
    print(bank.summary())

    print("")
    print(bank.statement("AC-100"))
    print("")
    print(bank.statement("AC-300"))

    print("")
    inheritance_demo()
