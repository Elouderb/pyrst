# Number processing with mathematical operations and filtering

def process_numbers(numbers: list[int]) -> None:
    print("=== Number Processing ===")
    print(len(numbers))
    print(sum(numbers))

    # Basic statistics
    total = sum(numbers)
    avg = total / len(numbers)
    max_num = max(numbers)
    min_num = min(numbers)
    range_num = max_num - min_num

    print(avg)
    print(max_num)
    print(min_num)
    print(range_num)

    # Filtering
    positive = [n for n in numbers if n > 0]
    negative = [n for n in numbers if n < 0]
    zero_vals = [n for n in numbers if n == 0]

    print(len(positive))
    print(len(negative))
    print(len(zero_vals))

    # Mathematical operations
    squared = [n * n for n in numbers]
    doubled = [n * 2 for n in numbers]
    absolute = [abs(n) for n in numbers]

    print(sum(squared))
    print(sum(doubled))
    print(sum(absolute))

    # Ranges and steps
    evens = [n for n in numbers if n % 2 == 0]
    odds = [n for n in numbers if n % 2 == 1]

    print(len(evens))
    print(len(odds))

    # Power operations
    powers_of_two = [2 * 2, 2 * 2 * 2, 2 * 2 * 2 * 2]
    factorials = [1, 1 * 2, 1 * 2 * 3, 1 * 2 * 3 * 4]

    print(sum(powers_of_two))
    print(sum(factorials))

    # Sorted and reversed
    sorted_nums = sorted(numbers)
    reversed_nums = sorted_nums[::-1]

    print(len(sorted_nums))
    print(len(reversed_nums))

def main() -> None:
    numbers1 = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    process_numbers(numbers1)

    numbers2 = [10, 20, 30, 40, 50]
    process_numbers(numbers2)

    numbers3 = [-5, -3, -1, 0, 1, 3, 5]
    process_numbers(numbers3)
