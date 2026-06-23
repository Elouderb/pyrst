# Shape: self.dict[k] = v  — a class owns a dict and mutates it via a method.
# Exercises IndexAssign with an attribute base (self.counts) lowering to
# HashMap::insert on a &mut self method.

class WordCounter:
    label: str
    counts: dict[str, int]

    def __init__(self, label: str) -> None:
        self.label = label
        self.counts = {}

    def bump(self, word: str, by: int) -> None:
        self.counts[word] = by

    def count_of(self, word: str) -> int:
        return self.counts.get(word, 0)


def main() -> None:
    wc = WordCounter("fruit")
    print(wc.label)
    wc.bump("apple", 3)
    wc.bump("pear", 7)
    wc.bump("apple", 10)
    print(wc.count_of("apple"))
    print(wc.count_of("pear"))
    print(wc.count_of("missing"))
    print(len(wc.counts))
