def calculate_mean(numbers: list[int]) -> float:
    return sum(numbers) / len(numbers)

def calculate_median(numbers: list[int]) -> float:
    sorted_nums: list[int] = sorted(numbers)
    n: int = len(sorted_nums)
    if n % 2 == 0:
        return (sorted_nums[n // 2 - 1] + sorted_nums[n // 2]) / 2
    else:
        return sorted_nums[n // 2]

def main() -> None:
    # Dataset: test scores
    test_scores = [
        65, 72, 78, 85, 88, 90, 91, 92, 95, 78,
        82, 87, 89, 93, 86, 80, 77, 84, 88, 91
    ]

    print("=== Statistical Analysis ===")
    print(len(test_scores))
    print(sum(test_scores))

    # Calculate statistics
    mean_score = calculate_mean(test_scores)
    max_score = max(test_scores)
    min_score = min(test_scores)
    range_score = max_score - min_score

    print(mean_score)
    print(max_score)
    print(min_score)
    print(range_score)

    # Percentile analysis
    below_mean = [x for x in test_scores if x < mean_score]
    above_mean = [x for x in test_scores if x >= mean_score]

    print(len(below_mean))
    print(len(above_mean))

    # Grade distribution
    excellent = [x for x in test_scores if x >= 90]
    good = [x for x in test_scores if x >= 80 and x < 90]
    average = [x for x in test_scores if x >= 70 and x < 80]
    poor = [x for x in test_scores if x < 70]

    print(len(excellent))
    print(len(good))
    print(len(average))
    print(len(poor))

    # Quartile analysis
    sorted_scores = sorted(test_scores)
    q1_idx = len(sorted_scores) // 4
    q3_idx = (len(sorted_scores) * 3) // 4

    q1 = sorted_scores[q1_idx]
    q3 = sorted_scores[q3_idx]
    iqr = q3 - q1

    print(q1)
    print(q3)
    print(iqr)

    # Outlier detection
    lower_bound = q1 - (iqr * 15) / 10
    upper_bound = q3 + (iqr * 15) / 10

    outliers = [x for x in test_scores if x < lower_bound or x > upper_bound]
    normal = [x for x in test_scores if x >= lower_bound and x <= upper_bound]

    print(len(outliers))
    print(len(normal))

    # Frequency analysis
    freq_90_plus = len([x for x in test_scores if x >= 90])
    freq_80_89 = len([x for x in test_scores if x >= 80 and x < 90])
    freq_70_79 = len([x for x in test_scores if x >= 70 and x < 80])

    print(freq_90_plus)
    print(freq_80_89)
    print(freq_70_79)

    # Variance calculation
    total_squared_diff = 0
    for score in test_scores:
        diff = score - mean_score
        squared_diff = diff * diff
        total_squared_diff = total_squared_diff + squared_diff

    variance = total_squared_diff / len(test_scores)
    print(variance)

    # Performance consistency
    consecutive_high = 0
    max_consecutive = 0
    for score in test_scores:
        if score >= 85:
            consecutive_high = consecutive_high + 1
            if consecutive_high > max_consecutive:
                max_consecutive = consecutive_high
        else:
            consecutive_high = 0

    print(max_consecutive)

    # Improvement detection
    first_half: list[int] = test_scores[:10]
    second_half: list[int] = test_scores[10:]

    first_avg: float = sum(first_half) / len(first_half)
    second_avg: float = sum(second_half) / len(second_half)
    improvement: float = second_avg - first_avg

    print(first_avg)
    print(second_avg)
    print(improvement)

    # Pass/fail analysis
    passing = [x for x in test_scores if x >= 70]
    failing = [x for x in test_scores if x < 70]
    pass_rate = (len(passing) * 100) / len(test_scores)

    print(len(passing))
    print(len(failing))
    print(pass_rate)
