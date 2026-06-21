# catalog.py
# The catalog / inventory management layer.
#
# Demonstrates: a class holding a dict[str, Book] (ISBN -> Book) and a
# list[str] of due records, add/remove/search/checkout operations, raising
# typed exceptions on invalid operations, and the pull-mutate-reassign idiom
# required because mutating a stored object through a subscript does not persist
# under pyrst's value semantics.

from models import Book


class Catalog:
    # ISBN -> Book. dict-of-objects: string keys, Book values.
    books: dict[str, Book]
    # Parallel checkout ledger as a list of "isbn|member|due_day" records.
    # (A list[str] keeps the reporting layer simple and deterministic to print.)
    loans: list[str]

    def __init__(self) -> None:
        self.books = {}
        self.loans = []

    def add_book(self, book: Book) -> None:
        if book.item_id in self.books:
            raise ValueError("duplicate item id: " + book.item_id)
        # self.books[k] = v is not allowed (only `localvar[k] = v`), and a bare
        # `local = self.books` would try to MOVE the map out of &mut self, so we
        # copy, mutate the copy, and write the whole map back.
        # `catalog[book.item_id] = book` (or even `key = book.item_id`) partially
        # MOVES book.item_id out of book, so the later `catalog[key] = book`
        # borrow of book fails to compile. Concatenating "" forces a fresh String
        # copy of the id, leaving book intact for the insert.
        key: str = "" + book.item_id
        catalog: dict[str, Book] = self.books.copy()
        catalog[key] = book
        self.books = catalog

    def remove_book(self, item_id: str) -> None:
        if item_id not in self.books:
            raise KeyError("no such item: " + item_id)
        catalog: dict[str, Book] = self.books.copy()
        catalog.pop(item_id)
        self.books = catalog

    def get_book(self, item_id: str) -> Book:
        if item_id not in self.books:
            raise KeyError("no such item: " + item_id)
        return self.books[item_id]

    def all_books(self) -> list[Book]:
        # Return books sorted by id so iteration/printing is deterministic
        # (dict iteration order in pyrst is NOT insertion order).
        result: list[Book] = []
        for item_id in sorted(self.books.keys()):
            result.append(self.books[item_id])
        return result

    def search_by_title(self, term: str) -> list[Book]:
        # Case-insensitive substring search over titles.
        needle: str = term.lower()
        matches: list[Book] = []
        for book in self.all_books():
            if needle in book.title.lower():
                matches.append(book)
        return matches

    def search_by_category(self, category: str) -> list[Book]:
        return [b for b in self.all_books() if b.category == category]

    def categories(self) -> list[str]:
        # Distinct categories, sorted. We dedup manually into a list rather than
        # using a set: pyrst's sorted() does not lower a set to a sortable Vec,
        # so sorted(some_set) / sorted(list(some_set)) fail to compile.
        distinct: list[str] = []
        for book in self.all_books():
            if book.category not in distinct:
                distinct.append(book.category)
        return sorted(distinct)

    def category_index(self) -> dict[str, list[str]]:
        # Build category -> [titles]. Demonstrates the pull-mutate-reassign
        # workaround: groups[cat].append(...) through a subscript would mutate
        # a temporary clone, so we read the list out, append, then store it back.
        groups: dict[str, list[str]] = {}
        for book in self.all_books():
            # Bind both fields to locals up front: reading book.title after
            # `cat = book.category` would be a use-after-partial-move in the
            # generated Rust (fields are moved out, not copied, by field access).
            cat: str = book.category
            title: str = book.title
            if cat in groups:
                titles: list[str] = groups[cat]
                titles.append(title)
                groups[cat] = titles
            else:
                groups[cat] = [title]
        return groups

    def checkout(self, item_id: str, member_id: str, due_day: int) -> None:
        # NB: we read self.books[item_id] inline rather than calling
        # self.get_book(item_id). Method parameters are passed by value in the
        # generated Rust, so get_book(item_id) would MOVE item_id and the later
        # uses below (catalog[item_id], the record string) would not compile.
        if item_id not in self.books:
            raise KeyError("no such item: " + item_id)
        book: Book = self.books[item_id]
        if not book.is_available():
            raise ValueError("no copies available: " + item_id)
        # Decrement availability on the stored object: mutate a local copy and
        # write the whole Book back into the dict (value semantics).
        book.available_copies = book.available_copies - 1
        catalog: dict[str, Book] = self.books.copy()
        catalog[item_id] = book
        self.books = catalog
        record: str = item_id + "|" + member_id + "|" + str(due_day)
        ledger: list[str] = self.loans.copy()
        ledger.append(record)
        self.loans = ledger

    def checkin(self, item_id: str) -> None:
        if item_id not in self.books:
            raise KeyError("no such item: " + item_id)
        book: Book = self.books[item_id]
        if book.available_copies >= book.total_copies:
            raise ValueError("nothing checked out: " + item_id)
        book.available_copies = book.available_copies + 1
        catalog: dict[str, Book] = self.books.copy()
        catalog[item_id] = book
        self.books = catalog

    def active_loan_count(self) -> int:
        return len(self.loans)

    def overdue(self, today: int) -> list[str]:
        # A loan record is "isbn|member|due_day"; overdue if due_day < today.
        result: list[str] = []
        for record in self.loans:
            parts: list[str] = record.split("|")
            due_day: int = int(parts[2])
            if due_day < today:
                result.append(record)
        return result
