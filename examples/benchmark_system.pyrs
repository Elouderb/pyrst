# Benchmark and performance tracking system

class Benchmark:
    name: str
    times: list[float]

    def __init__(self, name: str) -> None:
        self.name = name
        self.times = []

def main() -> None:
    # Create benchmarks
    bench1 = Benchmark("Operation A")
    bench1.times = [1.23, 1.45, 1.32, 1.50, 1.40]

    bench2 = Benchmark("Operation B")
    bench2.times = [2.10, 2.05, 2.15, 2.08, 2.12]

    bench3 = Benchmark("Operation C")
    bench3.times = [0.95, 0.98, 0.92, 1.00, 0.96]

    print("=== Benchmark Results ===")

    # Benchmark 1 analysis
    print("Operation A")
    total_a = 0.0
    for t in bench1.times:
        total_a = total_a + t
    avg_a = total_a / 5.0
    print(total_a)
    print(avg_a)

    max_a = bench1.times[0]
    min_a = bench1.times[0]
    for t in bench1.times:
        if t > max_a:
            max_a = t
        if t < min_a:
            min_a = t
    print(max_a)
    print(min_a)

    # Benchmark 2 analysis
    print("Operation B")
    total_b = 0.0
    for t in bench2.times:
        total_b = total_b + t
    avg_b = total_b / 5.0
    print(total_b)
    print(avg_b)

    max_b = bench2.times[0]
    min_b = bench2.times[0]
    for t in bench2.times:
        if t > max_b:
            max_b = t
        if t < min_b:
            min_b = t
    print(max_b)
    print(min_b)

    # Benchmark 3 analysis
    print("Operation C")
    total_c = 0.0
    for t in bench3.times:
        total_c = total_c + t
    avg_c = total_c / 5.0
    print(total_c)
    print(avg_c)

    max_c = bench3.times[0]
    min_c = bench3.times[0]
    for t in bench3.times:
        if t > max_c:
            max_c = t
        if t < min_c:
            min_c = t
    print(max_c)
    print(min_c)

    # Comparisons
    print("=== Comparisons ===")
    fastest_avg = avg_c
    if avg_a < fastest_avg:
        fastest_avg = avg_a
    if avg_b < fastest_avg:
        fastest_avg = avg_b
    print(fastest_avg)

    slowest_avg = avg_a
    if avg_b > slowest_avg:
        slowest_avg = avg_b
    if avg_c > slowest_avg:
        slowest_avg = avg_c
    print(slowest_avg)

    # Speed ratios
    ratio_a_to_c = avg_a / avg_c
    ratio_b_to_c = avg_b / avg_c
    print(ratio_a_to_c)
    print(ratio_b_to_c)

    # Consistency analysis (variance)
    print("=== Consistency ===")
    var_a = 0.0
    for t in bench1.times:
        var_a = var_a + ((t - avg_a) * (t - avg_a))
    var_a = var_a / 5.0
    print(var_a)

    var_c = 0.0
    for t in bench3.times:
        var_c = var_c + ((t - avg_c) * (t - avg_c))
    var_c = var_c / 5.0
    print(var_c)

    # Improvement potential
    improvement = avg_a - avg_c
    print(improvement)
