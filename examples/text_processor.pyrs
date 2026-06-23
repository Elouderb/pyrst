def process_text(text: str) -> dict[str, str]:
    result: dict[str, str] = {}

    result["original"] = text
    result["uppercase"] = text.upper()
    result["lowercase"] = text.lower()
    result["reversed"] = text[::-1]

    return result

def count_words(text: str) -> int:
    words: list[str] = text.split(" ")
    return len(words)

def main() -> None:
    # Sample text processing
    text: str = "The quick brown fox jumps over the lazy dog"

    # Process text
    processed: dict[str, str] = process_text(text)

    print("=== Text Processing ===")
    print(processed["original"])
    print(processed["uppercase"])
    print(processed["lowercase"])
    print(processed["reversed"])

    # Word analysis
    words: list[str] = text.split(" ")
    word_count: int = len(words)

    print("=== Word Analysis ===")
    print(word_count)

    # Word length analysis
    word_lengths: list[int] = [len(w) for w in words]
    avg_length: float = sum(word_lengths) / len(word_lengths)

    print(avg_length)
    print(min(word_lengths))
    print(max(word_lengths))

    # Long words
    long_words: list[str] = [w for w in words if len(w) > 4]
    print(len(long_words))

    # Short words
    short_words: list[str] = [w for w in words if len(w) <= 4]
    print(len(short_words))

    # Character analysis
    char_count: int = len(text)
    space_count: int = text.count(" ")
    letter_count: int = char_count - space_count

    print(char_count)
    print(space_count)
    print(letter_count)

    # Case analysis
    uppercase_count: int = len([c for c in text if c.isupper()])
    lowercase_count: int = len([c for c in text if c.islower()])

    print(uppercase_count)
    print(lowercase_count)

    # Words containing specific letters
    words_with_o: list[str] = [w for w in words if "o" in w.lower()]
    words_with_e: list[str] = [w for w in words if "e" in w.lower()]

    print(len(words_with_o))
    print(len(words_with_e))

    # Check properties
    is_all_lower: bool = text.islower()
    is_all_upper: bool = text.isupper()
    is_title: bool = text.istitle()

    print(is_all_lower)
    print(is_all_upper)
    print(is_title)

    # Text manipulation
    replaced: str = text.replace("fox", "cat")
    print(replaced)

    removed: str = text.replace(" ", "")
    print(len(removed))

    # Find operations
    pos_quick: int = text.find("quick")
    pos_lazy: int = text.rfind("lazy")

    print(pos_quick)
    print(pos_lazy)

    # String properties
    starts_the: bool = text.lower().startswith("the")
    ends_dog: bool = text.lower().endswith("dog")

    print(starts_the)
    print(ends_dog)
