from common import clamp

def safe_div(a: int, b: int) -> int:
    if b == 0:
        return 0
    return a // b

def bounded_sum(x: int, y: int) -> int:
    return clamp(x + y, 0, 1000)
