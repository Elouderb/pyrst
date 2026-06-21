# models.py
# Domain entities for the library inventory system.
#
# Demonstrates: class definitions with typed instance attributes, single
# inheritance with super().__init__, an operator/str dunder (__str__), and
# instance methods that compute derived values.
#
# Design note: pyrst has no subtype polymorphism (a list[LibraryItem] cannot
# hold Book instances), so the inheritance here is used purely for code reuse:
# Book extends a concrete LibraryItem base and is the only concrete type we
# actually store in collections.


class LibraryItem:
    item_id: str
    title: str
    category: str
    total_copies: int
    available_copies: int

    def __init__(self, item_id: str, title: str, category: str, copies: int) -> None:
        self.item_id = item_id
        self.title = title
        self.category = category
        self.total_copies = copies
        self.available_copies = copies

    def is_available(self) -> bool:
        return self.available_copies > 0

    def checked_out_count(self) -> int:
        return self.total_copies - self.available_copies

    def utilization(self) -> float:
        # Fraction of copies currently checked out, 0.0 .. 1.0.
        if self.total_copies == 0:
            return 0.0
        return self.checked_out_count() / self.total_copies


class Book(LibraryItem):
    author: str
    year: int

    def __init__(self, item_id: str, title: str, author: str, category: str, year: int, copies: int) -> None:
        super().__init__(item_id, title, category, copies)
        self.author = author
        self.year = year

    def citation(self) -> str:
        # A human-readable one-line citation.
        return f"{self.author} - {self.title} ({self.year})"

    def is_recent(self, cutoff: int) -> bool:
        return self.year >= cutoff

    def __str__(self) -> str:
        status: str = "available" if self.is_available() else "all out"
        return f"[{self.item_id}] {self.title} - {status}"


class Member:
    member_id: str
    name: str
    loans: int

    def __init__(self, member_id: str, name: str) -> None:
        self.member_id = member_id
        self.name = name
        self.loans = 0

    def can_borrow(self, limit: int) -> bool:
        return self.loans < limit

    def __str__(self) -> str:
        return f"{self.name} ({self.member_id}): {self.loans} on loan"
