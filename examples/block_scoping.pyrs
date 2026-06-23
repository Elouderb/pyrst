# A variable first assigned inside an if/else, for, or while body remains
# visible after the block (Python has function scope, not block scope).
def grade(score: int) -> str:
    if score >= 90:
        letter: str = "A"
    elif score >= 80:
        letter: str = "B"
    else:
        letter: str = "C"
    return letter

def main() -> None:
    print(grade(95))
    print(grade(85))
    print(grade(70))
    # First assigned in a for-body, used after the loop.
    total: int = 0
    for i in range(5):
        squared: int = i * i
        total = total + squared
    print(total)
    print(squared)
    # Float accumulation across a loop, averaged after.
    sum_v: float = 0.0
    for v in [2.0, 4.0, 6.0]:
        sum_v = sum_v + v
    avg: float = sum_v / 3.0
    print(avg)
