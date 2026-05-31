def calculate_statistics(numbers: list[int]) -> dict[str, int]:
    result: dict[str, int] = {}

    # Calculate sum and count
    total: int = sum(numbers)
    count: int = len(numbers)

    result["sum"] = total
    result["count"] = count
    result["min"] = min(numbers)
    result["max"] = max(numbers)
    result["range"] = result["max"] - result["min"]

    return result

def main() -> None:
    # Monthly sales data
    sales_data: list[int] = [
        1200, 1500, 1300, 1800, 2100, 1900,
        2200, 2000, 1800, 2300, 2400, 2600
    ]

    # Calculate statistics
    stats: dict[str, int] = calculate_statistics(sales_data)

    print("=== Sales Statistics ===")
    print(stats["sum"])
    print(stats["count"])
    print(stats["min"])
    print(stats["max"])
    print(stats["range"])

    # Find above-average months
    avg: float = stats["sum"] / stats["count"]
    above_avg: list[int] = [x for x in sales_data if x > avg]
    print(len(above_avg))

    # Categorize sales performance
    excellent: list[int] = [x for x in sales_data if x > 2200]
    good: list[int] = [x for x in sales_data if x > 1800 and x <= 2200]
    average: list[int] = [x for x in sales_data if x > 1400 and x <= 1800]
    poor: list[int] = [x for x in sales_data if x <= 1400]

    print(len(excellent))
    print(len(good))
    print(len(average))
    print(len(poor))

    # Growth rate analysis
    growth_rates: list[int] = []
    for i in range(1, len(sales_data)):
        prev: int = sales_data[i - 1]
        curr: int = sales_data[i]
        growth: int = curr - prev
        growth_rates.append(growth)

    total_growth: int = sum(growth_rates)
    avg_growth: float = total_growth / len(growth_rates)

    print(total_growth)
    print(avg_growth)

    # Identify trends
    positive_months: int = len([x for x in growth_rates if x > 0])
    negative_months: int = len([x for x in growth_rates if x < 0])

    print(positive_months)
    print(negative_months)

    # Performance bands
    best_month: int = max(sales_data)
    worst_month: int = min(sales_data)

    print(best_month)
    print(worst_month)

    # Verify data integrity
    has_negative: bool = any([x < 0 for x in sales_data])
    all_positive: bool = all([x > 0 for x in sales_data])

    print(has_negative)
    print(all_positive)

    # Sorted analysis
    sorted_sales: list[int] = sorted(sales_data)
    print(sorted_sales[0])
    print(sorted_sales[len(sorted_sales) - 1])
