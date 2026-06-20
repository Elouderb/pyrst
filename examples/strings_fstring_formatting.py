def format_report(name: str, value: float, percent: float) -> str:
    formatted: str = f"{name}: {value:.2f} ({percent:.1f}%)"
    return formatted

def main() -> None:
    # Test basic f-string with format specs
    price: float = 19.99
    quantity: int = 5
    total: float = price * quantity
    print(f"Price: ${price:.2f}")
    print(f"Total: ${total:.2f}")
    
    # Test alignment and padding in f-strings
    name: str = "Item"
    count: int = 42
    print(f"{name:>10} | {count:05d}")
    
    # Test chained method calls on strings with f-strings
    raw_text: str = "  Hello WORLD  "
    cleaned: str = raw_text.strip().lower()
    print(f"Input: [{raw_text}]")
    print(f"Cleaned: [{cleaned}]")
    
    # Test string methods with find/count and f-strings
    sentence: str = "the cat in the hat sat on the mat"
    the_count: int = sentence.count("the")
    cat_pos: int = sentence.find("cat")
    print(f"Count of 'the': {the_count}")
    print(f"Position of 'cat': {cat_pos}")
    
    # Test using format_report function
    result: str = format_report("revenue", 1234.567, 87.3)
    print(result)
    
    # Test replace and split with f-strings
    original: str = "apple,banana,cherry"
    replaced: str = original.replace(",", "; ")
    print(f"Replaced: {replaced}")
    parts: list[str] = original.split(",")
    print(f"Parts count: {len(parts)}")
    print(f"First: {parts[0]}")