def main() -> None:
    scores: dict[str, int] = {"alice": 90, "bob": 85}
    print(len(scores))
    scores["charlie"] = 95
    print(scores.get("alice", 0))
