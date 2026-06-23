# Text analysis tool with string operations and comprehensions

def analyze_text(text: str) -> None:
    words = text.split(" ")
    word_count = len(words)
    char_count = len(text)

    print("=== Text Analysis ===")
    print(word_count)
    print(char_count)

    # Word length analysis
    word_lengths = [len(w) for w in words]
    max_length = max(word_lengths)
    min_length = min(word_lengths)
    avg_length = sum(word_lengths) / len(word_lengths)

    print(max_length)
    print(min_length)
    print(avg_length)

    # Case analysis
    upper_words = [w for w in words if w.isupper()]
    lower_words = [w for w in words if w.islower()]

    print(len(upper_words))
    print(len(lower_words))

    # String operations
    replaced = text.replace("the", "THE")
    uppercase = text.upper()
    lowercase = text.lower()

    print(replaced)
    print(len(uppercase))
    print(len(lowercase))

    # Find operations
    pos = text.find("world")
    print(pos)

    # Character analysis
    digits = [c for c in text if c.isdigit()]
    alphas = [c for c in text if c.isalpha()]

    print(len(digits))
    print(len(alphas))

    # Reversed and sorted
    sorted_words = sorted(words)
    reversed_text = text[::-1]

    print(len(sorted_words))
    print(len(reversed_text))

def main() -> None:
    # Simple text
    text1 = "Hello world from Python"
    analyze_text(text1)

    # Longer text
    text2 = "The quick brown fox jumps over the lazy dog"
    analyze_text(text2)

    # Text with mixed case
    text3 = "PyThOn Is AwEsOmE"
    analyze_text(text3)
