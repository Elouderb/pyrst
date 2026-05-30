def main() -> None:
    x: float = 3.14159
    print(f"{x:.2f}")
    n: int = 42
    print(f"{n:>8}")
    print(f"{n:08d}")
    label: str = "score"
    print(f"{label}: {x:.1f}")
