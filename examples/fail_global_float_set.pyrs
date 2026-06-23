# ASSERT: typeck must reject this file — a module-level global declared as set[float]
# is not hashable and cannot be represented as HashSet<f64> in Rust.
g: set[float] = set()

def main() -> None:
    print(len(g))
