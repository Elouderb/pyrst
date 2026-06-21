# reports.py
# The reporting layer: pure functions that turn catalog state into formatted
# lines and aggregate statistics.
#
# Demonstrates: list comprehensions, sum/max/min/len aggregates, f-string
# format specs (.Nf, alignment, zero-pad), manual percentage math (the `%`
# format spec does not apply Python percent scaling in pyrst), and building
# dict[str, int] summaries.

from models import Book
from catalog import Catalog


def percent(part: int, whole: int) -> float:
    # Percentage as a plain float; callers format with `:.1f` and add a literal %.
    if whole == 0:
        return 0.0
    return (part * 100.0) / whole


def inventory_summary(catalog: Catalog) -> dict[str, int]:
    # A small dict[str, int] of headline counts.
    books: list[Book] = catalog.all_books()
    total_titles: int = len(books)
    total_copies: int = sum([b.total_copies for b in books])
    available: int = sum([b.available_copies for b in books])
    summary: dict[str, int] = {}
    summary["titles"] = total_titles
    summary["copies"] = total_copies
    summary["available"] = available
    summary["checked_out"] = total_copies - available
    return summary


def category_counts(catalog: Catalog) -> dict[str, int]:
    # category -> number of titles, derived from the catalog's category index.
    index: dict[str, list[str]] = catalog.category_index()
    counts: dict[str, int] = {}
    for cat in sorted(index.keys()):
        counts[cat] = len(index[cat])
    return counts


def busiest_title(catalog: Catalog) -> str:
    # Title with the highest checked-out count; "" if the catalog is empty.
    books: list[Book] = catalog.all_books()
    if len(books) == 0:
        return ""
    best: Book = books[0]
    for book in books:
        if book.checked_out_count() > best.checked_out_count():
            best = book
    return best.title


def print_catalog_listing(catalog: Catalog) -> None:
    print("=== Catalog Listing ===")
    books: list[Book] = catalog.all_books()
    for book in books:
        # Aligned columns: id (left, width 6), title (left, width 22), counts.
        line: str = f"{book.item_id:<6} {book.title:<22} {book.available_copies:>2}/{book.total_copies:<2}  {book.author}"
        print(line)


def print_category_report(catalog: Catalog) -> None:
    print("=== By Category ===")
    counts: dict[str, int] = category_counts(catalog)
    total: int = sum(counts.values())
    for cat in sorted(counts.keys()):
        n: int = counts[cat]
        share: float = percent(n, total)
        print(f"{cat:<12} {n:>2} titles  ({share:.1f}%)")


def print_availability_report(catalog: Catalog) -> None:
    print("=== Availability ===")
    summary: dict[str, int] = inventory_summary(catalog)
    copies: int = summary["copies"]
    available: int = summary["available"]
    checked_out: int = summary["checked_out"]
    avail_pct: float = percent(available, copies)
    print(f"Titles:        {summary['titles']:>3}")
    print(f"Total copies:  {copies:>3}")
    print(f"Available:     {available:>3}  ({avail_pct:.1f}%)")
    print(f"Checked out:   {checked_out:>3}")


def print_overdue_report(catalog: Catalog, today: int) -> None:
    print("=== Overdue ===")
    overdue: list[str] = catalog.overdue(today)
    if len(overdue) == 0:
        print("none")
        return
    for record in overdue:
        parts: list[str] = record.split("|")
        item_id: str = parts[0]
        member_id: str = parts[1]
        due_day: int = int(parts[2])
        days_late: int = today - due_day
        print(f"{item_id:<6} member {member_id:<5} {days_late:>2} day(s) late")
