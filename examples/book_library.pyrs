# Book library management system

class Book:
    title: str
    author: str
    pages: float
    year: float

    def __init__(self, title: str, author: str, pages: float, year: float) -> None:
        self.title = title
        self.author = author
        self.pages = pages
        self.year = year

def main() -> None:
    books = [
        Book("Python Basics", "Smith", 300.0, 2020.0),
        Book("Data Science", "Johnson", 450.0, 2021.0),
        Book("Web Dev", "Williams", 350.0, 2019.0),
        Book("AI Guide", "Brown", 500.0, 2022.0),
        Book("Cloud Systems", "Davis", 400.0, 2021.0),
    ]

    print("=== Book Library ===")
    print(len(books))

    # Page analysis
    total_pages = 0.0
    max_pages = books[0].pages
    min_pages = books[0].pages

    for book in books:
        total_pages = total_pages + book.pages
        if book.pages > max_pages:
            max_pages = book.pages
        if book.pages < min_pages:
            min_pages = book.pages

    print(total_pages)
    print(max_pages)
    print(min_pages)

    avg_pages = total_pages / 5.0
    print(avg_pages)

    # Year analysis
    total_year = 0.0
    max_year = books[0].year
    min_year = books[0].year

    for book in books:
        total_year = total_year + book.year
        if book.year > max_year:
            max_year = book.year
        if book.year < min_year:
            min_year = book.year

    print(total_year)
    print(max_year)
    print(min_year)

    avg_year = total_year / 5.0
    print(avg_year)

    # Recent books
    recent_count = 0
    for book in books:
        if book.year >= 2021.0:
            recent_count = recent_count + 1

    print(recent_count)

    # Long books
    long_count = 0
    for book in books:
        if book.pages > 400.0:
            long_count = long_count + 1

    print(long_count)

    # Print details
    print("=== Details ===")
    for book in books:
        print(book.title)
        print(book.author)
        print(book.pages)
        print(book.year)

    # Combined analysis
    print("=== Analysis ===")
    total_by_recent = 0.0
    total_by_old = 0.0

    for book in books:
        if book.year >= 2021.0:
            total_by_recent = total_by_recent + book.pages
        else:
            total_by_old = total_by_old + book.pages

    print(total_by_recent)
    print(total_by_old)

    # Author count
    print("=== Summary ===")
    print(total_pages)
    print(recent_count)
    print(long_count)
