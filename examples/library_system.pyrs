class Book:
    title: str
    author: str
    year: int
    available: bool

    def __init__(self, title: str, author: str, year: int, available: bool) -> None:
        self.title = title
        self.author = author
        self.year = year
        self.available = available

def create_library() -> list[Book]:
    books = []
    books.append(Book("Python Basics", "John Smith", 2020, True))
    books.append(Book("Data Science", "Jane Doe", 2021, False))
    books.append(Book("Web Development", "Bob Johnson", 2019, True))
    books.append(Book("Machine Learning", "Alice Brown", 2022, True))
    books.append(Book("Advanced Python", "Charlie Wilson", 2023, False))
    books.append(Book("Cloud Computing", "Diana Lee", 2021, True))
    return books

def get_available_count(books: list[Book]) -> int:
    return len([b for b in books if b.available])

def main() -> None:
    # Create library
    books = create_library()

    print("=== Library Management ===")
    print(len(books))
    print(get_available_count(books))

    # Availability analysis
    available_books = [b for b in books if b.available]
    unavailable_books = [b for b in books if not b.available]

    print(len(available_books))
    print(len(unavailable_books))

    # Year analysis
    recent_books = [b for b in books if b.year >= 2022]
    old_books = [b for b in books if b.year < 2020]

    print(len(recent_books))
    print(len(old_books))

    # Title analysis
    book_titles = [b.title for b in books]
    sorted_titles = sorted(book_titles)

    print("=== Sorted Titles ===")
    for title in sorted_titles:
        print(title)

    # Book details
    print("=== Book Inventory ===")
    for book in books:
        print(book.title)
        print(book.author)
        print(book.year)
        if book.available:
            print("Available")
        else:
            print("Checked Out")

    # Search by partial title
    search_term = "Python"
    matching = [b for b in books if search_term in b.title]

    print(len(matching))

    # Availability percentage
    total = len(books)
    available = get_available_count(books)
    availability_pct = (available * 100) / total
    print(availability_pct)

    # Book age analysis
    oldest_book = books[0]
    for book in books:
        if book.year < oldest_book.year:
            oldest_book = book

    newest_book = books[0]
    for book in books:
        if book.year > newest_book.year:
            newest_book = book

    print(oldest_book.title)
    print(oldest_book.year)
    print(newest_book.title)
    print(newest_book.year)

    # Capacity planning
    max_capacity = 50
    current_usage = (total * 100) / max_capacity
    space_left = max_capacity - total

    print(current_usage)
    print(space_left)

    # Summary statistics
    titles_count = len(book_titles)
    unique_authors = len([b.author for b in books])
    print(titles_count)
    print(unique_authors)
