# main.py
# Entry point: builds a library catalog, runs a checkout/return scenario, and
# prints a deterministic report. Imports across three modules to exercise
# pyrst's multi-file compilation (DFS import resolution, flat namespace merge).

from models import Book, Member
from catalog import Catalog
from reports import print_catalog_listing, print_category_report, print_availability_report, print_overdue_report, busiest_title


def seed_catalog() -> Catalog:
    catalog: Catalog = Catalog()
    # item_id, title, author, category, year, copies  (all positional: pyrst
    # does not fill default params or reorder keyword args at call sites).
    catalog.add_book(Book("BK0001", "Dune", "Herbert", "scifi", 1965, 3))
    catalog.add_book(Book("BK0002", "Neuromancer", "Gibson", "scifi", 1984, 2))
    catalog.add_book(Book("BK0003", "Sapiens", "Harari", "history", 2011, 4))
    catalog.add_book(Book("BK0004", "The Pragmatic Programmer", "Hunt", "tech", 1999, 2))
    catalog.add_book(Book("BK0005", "Clean Code", "Martin", "tech", 2008, 5))
    catalog.add_book(Book("BK0006", "Guns Germs and Steel", "Diamond", "history", 1997, 1))
    catalog.add_book(Book("BK0007", "The Martian", "Weir", "scifi", 2011, 3))
    return catalog


# NB: classes have value semantics in pyrst — a Catalog passed to a function is
# COPIED, so mutations inside would not be visible to the caller. We therefore
# take the catalog, mutate it, and RETURN it so main can rebind.
def run_scenario(catalog: Catalog) -> Catalog:
    members: list[Member] = [
        Member("M01", "Ada"),
        Member("M02", "Linus"),
        Member("M03", "Grace"),
    ]
    print(f"Members registered: {len(members)}")

    # day = 10 is "today" for due-date math; due_day values below it are overdue.
    catalog.checkout("BK0001", "M01", 6)   # due day 6  -> overdue
    catalog.checkout("BK0001", "M02", 12)  # due day 12 -> not yet due
    catalog.checkout("BK0005", "M03", 4)   # due day 4  -> overdue
    catalog.checkout("BK0007", "M01", 14)  # due day 14 -> not yet due

    # Error path: attempt to over-checkout a single-copy title.
    catalog.checkout("BK0006", "M02", 9)
    try:
        catalog.checkout("BK0006", "M03", 9)
    except ValueError as e:
        print("checkout rejected: " + e)

    # Error path: returning something that was never out.
    try:
        catalog.checkin("BK0003")
    except ValueError as e:
        print("checkin rejected: " + e)

    # A normal return frees a copy back up.
    catalog.checkin("BK0001")

    print(f"Active loans: {catalog.active_loan_count()}")
    return catalog


def main() -> None:
    print("=== Library Inventory ===")
    catalog: Catalog = seed_catalog()

    print(f"Seeded titles: {len(catalog.books)}")
    print(f"Categories: {len(catalog.categories())}")

    # Search before any checkouts.
    hits: list[Book] = catalog.search_by_title("the")
    print(f"Titles containing 'the': {len(hits)}")
    scifi: list[Book] = catalog.search_by_category("scifi")
    print(f"Sci-fi titles: {len(scifi)}")

    catalog = run_scenario(catalog)

    print_catalog_listing(catalog)
    print_category_report(catalog)
    print_availability_report(catalog)
    print_overdue_report(catalog, 10)

    print(f"Busiest title: {busiest_title(catalog)}")

    # Remove a title and confirm the count drops.
    catalog.remove_book("BK0002")
    print(f"Titles after removal: {len(catalog.books)}")
