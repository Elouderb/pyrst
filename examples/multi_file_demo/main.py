from common import clamp
from math_utils import safe_div, bounded_sum

def main() -> None:
    print(clamp(150, 0, 100))
    print(safe_div(10, 2))
    print(bounded_sum(600, 700))
