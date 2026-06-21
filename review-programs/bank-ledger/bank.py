# bank.py -- the Bank aggregate: a dict of accounts plus per-account history.
#
# This module is shaped almost entirely by pyrst's value/ownership model, which
# is Rust's move semantics surfaced (mostly) unsoftened:
#
#   * No subtype polymorphism: self.accounts is dict[str, Account] (the unified
#     struct), never a mix of subclasses.
#   * `self.d[k] = v` is NOT a legal assignment target (the target object must be
#     a bare name), and mutating a struct read out of a dict does not persist.
#     So every mutation copies the whole dict into a local, edits the local, and
#     reassigns the attribute (self.accounts = accts).
#   * A method is generated as `&mut self` ONLY if it *directly* assigns a
#     self.attr; delegating all mutation to a helper method leaves the caller as
#     `&self` and fails to compile. So every mutating method below performs its
#     store/record inline (a direct `self.accounts = ...` / `self.history = ...`).
#   * Passing a non-Copy value (str / struct / collection) by value MOVES it; it
#     cannot be reused afterward, and there is no `.clone()` in the surface
#     language. To use an id more than once we first clone it with `"" + id`
#     (string concat clones its operand), then consume each copy once.

from money import format_cents
from accounts import Account
from accounts import InsufficientFundsError
from accounts import InvalidAmountError
from accounts import AccountNotFoundError
from accounts import AccountFrozenError
from transactions import Transaction
from transactions import total_debits
from transactions import total_credits


class Bank:
    name: str
    accounts: dict[str, Account]
    history: dict[str, list[Transaction]]
    next_seq: int

    def __init__(self, name: str) -> None:
        self.name = name
        self.accounts = {}
        self.history = {}
        self.next_seq = 1

    # ---- internal helpers ---------------------------------------------------

    # Snapshot accessors. Reading `self.accounts` directly into a local is a
    # MOVE out of &mut self (rejected by the backend), but *returning* a field
    # clones it. Routing the read through these one-line methods is the only way
    # to get a mutable working copy of an attribute-owned collection.
    def _accts(self) -> dict[str, Account]:
        return self.accounts

    def _hist(self) -> dict[str, list[Transaction]]:
        return self.history

    # Append a transaction to an account's history, allocating the list lazily.
    # `account_id` and `counterparty` are consumed (moved) here; callers pass
    # freshly-cloned copies. Directly assigns self.history so the method is
    # generated as &mut self.
    def _record(self, account_id: str, kind: str, delta: int, balance_after: int, counterparty: str) -> None:
        key: str = "" + account_id
        write_key: str = "" + account_id
        t: Transaction = Transaction(self.next_seq, account_id, kind, delta, balance_after, counterparty)
        self.next_seq = self.next_seq + 1
        hist: dict[str, list[Transaction]] = self._hist()
        rows: list[Transaction] = []
        if key in hist:
            rows = hist[key]
        rows.append(t)
        hist[write_key] = rows
        self.history = hist

    # ---- public API ---------------------------------------------------------

    def open_account(self, account_id: str, owner: str, kind: str, opening_cents: int, rate_bps: int, overdraft_limit: int) -> None:
        if opening_cents < 0:
            raise InvalidAmountError("opening balance may not be negative")
        # We need account_id three times: as the dict key, inside the Account,
        # and (maybe) in the opening-deposit record. Clone up front.
        key: str = "" + account_id
        rec_id: str = "" + account_id
        a: Account = Account(account_id, owner, kind, opening_cents, rate_bps, overdraft_limit)
        accts: dict[str, Account] = self._accts()
        accts[key] = a
        self.accounts = accts
        if opening_cents > 0:
            self._record(rec_id, "deposit", opening_cents, opening_cents, "")

    def freeze(self, account_id: str) -> None:
        key: str = "" + account_id
        write_key: str = "" + account_id
        if account_id not in self.accounts:
            raise AccountNotFoundError(key)
        accts: dict[str, Account] = self._accts()
        a: Account = accts[write_key]
        a.frozen = True
        accts[key] = a
        self.accounts = accts

    def unfreeze(self, account_id: str) -> None:
        key: str = "" + account_id
        write_key: str = "" + account_id
        if account_id not in self.accounts:
            raise AccountNotFoundError(key)
        accts: dict[str, Account] = self._accts()
        a: Account = accts[write_key]
        a.frozen = False
        accts[key] = a
        self.accounts = accts

    def balance_of(self, account_id: str) -> int:
        if account_id not in self.accounts:
            raise AccountNotFoundError(account_id)
        return self.accounts[account_id].balance

    def deposit(self, account_id: str, amount: int) -> int:
        if amount <= 0:
            raise InvalidAmountError("deposit must be positive")
        key: str = "" + account_id
        write_key: str = "" + account_id
        rec_id: str = "" + account_id
        if account_id not in self.accounts:
            raise AccountNotFoundError(key)
        accts: dict[str, Account] = self._accts()
        a: Account = accts[key]
        if a.frozen:
            raise AccountFrozenError(rec_id)
        a.balance = a.balance + amount
        new_balance: int = a.balance
        accts[write_key] = a
        self.accounts = accts
        self._record(rec_id, "deposit", amount, new_balance, "")
        return new_balance

    def withdraw(self, account_id: str, amount: int) -> int:
        if amount <= 0:
            raise InvalidAmountError("withdrawal must be positive")
        key: str = "" + account_id
        write_key: str = "" + account_id
        rec_id: str = "" + account_id
        if account_id not in self.accounts:
            raise AccountNotFoundError(key)
        accts: dict[str, Account] = self._accts()
        a: Account = accts[key]
        if a.frozen:
            raise AccountFrozenError(rec_id)
        if not a.can_withdraw(amount):
            raise InsufficientFundsError(rec_id)
        a.balance = a.balance - amount
        new_balance: int = a.balance
        accts[write_key] = a
        self.accounts = accts
        self._record(rec_id, "withdraw", -amount, new_balance, "")
        return new_balance

    # Move money between two accounts. In this single-threaded model "atomic"
    # just means we validate both sides before mutating either.
    def transfer(self, src_id: str, dst_id: str, amount: int) -> None:
        if amount <= 0:
            raise InvalidAmountError("transfer must be positive")
        src_key: str = "" + src_id
        dst_key: str = "" + dst_id
        src_wkey: str = "" + src_id
        dst_wkey: str = "" + dst_id
        src_rec: str = "" + src_id
        dst_rec: str = "" + dst_id
        src_cp: str = "" + src_id
        dst_cp: str = "" + dst_id
        if src_id not in self.accounts:
            raise AccountNotFoundError(src_key)
        if dst_id not in self.accounts:
            raise AccountNotFoundError(dst_key)
        accts: dict[str, Account] = self._accts()
        src: Account = accts[src_wkey]
        dst: Account = accts[dst_wkey]
        if src.frozen:
            raise AccountFrozenError(src_rec)
        if dst.frozen:
            raise AccountFrozenError(dst_rec)
        if not src.can_withdraw(amount):
            raise InsufficientFundsError(src_rec)
        src.balance = src.balance - amount
        dst.balance = dst.balance + amount
        src_after: int = src.balance
        dst_after: int = dst.balance
        accts[src_key] = src
        accts[dst_key] = dst
        self.accounts = accts
        self._record(src_rec, "transfer_out", -amount, src_after, dst_cp)
        self._record(dst_rec, "transfer_in", amount, dst_after, src_cp)

    # Post one period of interest to every savings account. Returns the total
    # interest paid out across the bank (cents).
    def post_interest(self) -> int:
        total_paid: int = 0
        ids: list[str] = self.sorted_ids()
        for account_id in ids:
            key: str = "" + account_id
            write_key: str = "" + account_id
            accts: dict[str, Account] = self._accts()
            a: Account = accts[key]
            earned: int = a.accrue()
            if earned > 0 and not a.frozen:
                a.balance = a.balance + earned
                new_balance: int = a.balance
                accts[write_key] = a
                self.accounts = accts
                self._record(account_id, "interest", earned, new_balance, "")
                total_paid = total_paid + earned
        return total_paid

    # ---- queries / reporting ------------------------------------------------

    # Account ids sorted lexicographically for deterministic iteration.
    def sorted_ids(self) -> list[str]:
        ids: list[str] = []
        for account_id in self.accounts.keys():
            ids.append(account_id)
        return sorted(ids)

    def total_assets(self) -> int:
        total: int = 0
        for account_id in self.sorted_ids():
            total = total + self.accounts[account_id].balance
        return total

    def history_for(self, account_id: str) -> list[Transaction]:
        if account_id in self.history:
            return self.history[account_id]
        empty: list[Transaction] = []
        return empty

    # Number of accounts of a given kind.
    def count_kind(self, kind: str) -> int:
        n: int = 0
        for account_id in self.sorted_ids():
            if self.accounts[account_id].kind == kind:
                n = n + 1
        return n

    # Build the multi-line summary report as a single string.
    def summary(self) -> str:
        lines: list[str] = []
        lines.append("===== " + self.name + " : Account Summary =====")
        for account_id in self.sorted_ids():
            lines.append(str(self.accounts[account_id]))
        lines.append("-------------------------------------------")
        checking: int = self.count_kind("checking")
        savings: int = self.count_kind("savings")
        lines.append(f"Accounts: {len(self.accounts)}  (checking {checking}, savings {savings})")
        lines.append("Total assets: " + format_cents(self.total_assets()))
        return "\n".join(lines)

    # Statement for one account: header, every transaction row, then totals.
    def statement(self, account_id: str) -> str:
        key: str = "" + account_id
        owner_key: str = "" + account_id
        bal_key: str = "" + account_id
        if account_id not in self.accounts:
            raise AccountNotFoundError(key)
        rows: list[Transaction] = self.history_for(account_id)
        lines: list[str] = []
        lines.append("----- Statement for " + key + " (" + self.accounts[owner_key].owner + ") -----")
        for t in rows:
            lines.append(str(t))
        credits: int = total_credits(rows)
        debits: int = total_debits(rows)
        lines.append(f"Credits: {format_cents(credits)}   Debits: {format_cents(debits)}")
        lines.append("Closing balance: " + format_cents(self.accounts[bal_key].balance))
        return "\n".join(lines)
