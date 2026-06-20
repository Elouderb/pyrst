def count_words(text: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for word in text.lower().split(" "):
        clean: str = word.strip()
        if clean == "":
            continue
        if clean in counts:
            counts[clean] = counts[clean] + 1
        else:
            counts[clean] = 1
    return counts

def main() -> None:
    text: str = "the cat the dog THE bird the cat"
    counts: dict[str, int] = count_words(text)
    print(counts["the"])
    print(counts["cat"])
    total: int = sum(counts.values())
    print(total)
    uniq: int = len(counts.keys())
    print(uniq)
    label: str = "many" if uniq > 3 else "few"
    print(label)
